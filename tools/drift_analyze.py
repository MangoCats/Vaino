#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Estimate a node's audio clock rate error, in ppm, from drift samples.

    tools/drift_analyze.py drift-samples.tsv [clock-log.txt]

THE ONE RULE: never measure `hw_ptr` against `tstamp`.

ALSA derives `tstamp` from `hw_ptr` at the *nominal* rate, so the two are the
same counter wearing different units. `(hw_ptr / rate) - (tstamp -
trigger_time)` therefore cannot express the difference between nominal and
actual rate -- which is the whole quantity -- and instead reports a fixed
prefill offset divided by elapsed time, decaying toward zero as the window
grows. That formula produced `bose`'s published +0.432 ppm; the true figure is
+14.20 ppm, a factor of 33. See docs/LOG007 `[LOG-FIX-010]`.

It was convincing because it was *stable*, had a tiny quantisation floor, and
reproduced across windows. Agreement between two circular measurements is not
corroboration. So this tool reads `uptime` and refuses to touch `tstamp`.
"""
import sys

# A stream's first sampling window is short by the buffer prefill -- measured
# at -169.9 ppm on `bose`, a one-time 0.66 s deficit, against +/-0.02 s for
# every window after it. Fitting from the second window onward is not tidying
# the data; including it corrupts the rate `[LOG-FIX-050]`.
SKIP_AFTER_TRIGGER_S = 900


def lsq(xs, ys):
    n = len(xs)
    mx, my = sum(xs) / n, sum(ys) / n
    sxx = sum((x - mx) ** 2 for x in xs)
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sxx


def read_samples(path):
    """Rows of (uptime, trigger_time, hw_ptr, rate) from the sampler's TSV."""
    rows, head = [], None
    for line in open(path, encoding="utf-8"):
        f = line.rstrip("\n").split("\t")
        if head is None:
            head = f
            continue
        if len(f) < len(head) or f[head.index("state")] != "RUNNING":
            continue
        g = lambda k: f[head.index(k)]
        rows.append((float(g("uptime")), g("trigger_time"),
                     int(g("hw_ptr")), float(g("rate"))))
    return rows


def read_clock_log(path):
    """Rows of (seconds, frames) from the player's `clock:` journal lines.

    `frames` and `at_nanos` are stored together inside one audio callback, so
    the pair is near-atomic. The sampler's is not: it reads the status file
    and `/proc/uptime` separately, and the skew shows -- 17.6 ppm of hourly
    scatter against this source's 2.9 ppm `[LOG-FIX-070]`.
    """
    import re
    out = []
    for line in open(path, encoding="utf-8"):
        m = re.search(r"clock: frames=(\d+) at_nanos=(\d+)", line)
        if m and int(m.group(1)) and int(m.group(2)):
            out.append((int(m.group(2)) / 1e9, int(m.group(1))))
    return sorted(out)


def report(label, pairs, rate):
    """Rate error over the longest span, plus hourly windows as a spread."""
    if len(pairs) < 3:
        print("  %-22s too few samples (%d)" % (label, len(pairs)))
        return
    span = pairs[-1][0] - pairs[0][0]
    endpoint = ((pairs[-1][1] - pairs[0][1]) / span / rate - 1) * 1e6
    fit = (lsq([p[0] for p in pairs], [float(p[1]) for p in pairs]) / rate - 1) * 1e6
    hourly, i = [], 0
    while i < len(pairs) - 1:
        j = i
        while j < len(pairs) - 1 and pairs[j][0] - pairs[i][0] < 3600:
            j += 1
        dt = pairs[j][0] - pairs[i][0]
        if dt > 1800:
            hourly.append(((pairs[j][1] - pairs[i][1]) / dt / rate - 1) * 1e6)
        i = j
    print("  %-22s %8.0f s  n=%3d   %+8.3f ppm" % (label, span, len(pairs), endpoint))
    # A large endpoint/fit gap means the rate is not constant across the
    # window; report it rather than averaging the disagreement away
    # `[GOV-SRC-040]`. On clean data the two agree to a fraction of a ppm.
    if abs(endpoint - fit) > 1.0:
        print("      least-squares fit says %+.3f ppm -- the rate is not constant;"
              % fit)
        print("      check for a stream restart or an unexcluded prefill window.")
    if len(hourly) > 2:
        mean = sum(hourly) / len(hourly)
        sd = (sum((v - mean) ** 2 for v in hourly) / (len(hourly) - 1)) ** 0.5
        print("      hourly windows: n=%d  mean %+.3f  sd %.3f  min %+.3f  max %+.3f"
              % (len(hourly), mean, sd, min(hourly), max(hourly)))


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    rows = read_samples(argv[0])
    if not rows:
        print("no RUNNING samples in %s" % argv[0])
        return 1
    print("%s: %d RUNNING samples" % (argv[0], len(rows)))
    segs, cur = [], [rows[0]]
    for r in rows[1:]:
        if r[1] != cur[-1][1]:   # trigger_time changed: a new stream
            segs.append(cur)
            cur = []
        cur.append(r)
    segs.append(cur)
    for k, seg in enumerate(segs):
        rate = seg[-1][3]
        kept = [r for r in seg if r[0] - seg[0][0] >= SKIP_AFTER_TRIGGER_S]
        print("\nstream %d of %d  (trigger_time %s, %d samples, %d after prefill)"
              % (k + 1, len(segs), seg[0][1], len(seg), len(kept)))
        report("hw_ptr vs uptime", [(r[0], r[2]) for r in kept], rate)
    if len(argv) > 1:
        fc = read_clock_log(argv[1])
        print("\n%s: %d clock samples" % (argv[1], len(fc)))
        kept = [p for p in fc if p[0] - fc[0][0] >= SKIP_AFTER_TRIGGER_S]
        report("frames vs at_nanos", kept, rows[-1][3])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

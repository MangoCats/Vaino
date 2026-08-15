# SPDX-License-Identifier: AGPL-3.0-or-later
"""Repair `files.duration_ms` from the decoded length `[REQ-LIB-145]`.

`[SPEC-SC-030]` specifies `duration_ms` as **decoded, not header-claimed**, and
the migrated library violates it: 29.2% of files differ from their decoded
length by more than 5 s, and one overstates by 38.4 minutes `[GDE-FEX-106]`.

Not cosmetic. Segmentation used an inflated value to invent a **phantom
passage** in a tail that does not exist, and the player uses `duration_ms` for
lead-out timing. So this repairs the passages too:

  * `files.duration_ms`  → the decoded value, where it disagrees
  * `passages.end_ms`    → clamped to the real end, where it overruns
  * phantom passages     → reported; deleted only on explicit request

Repair only where it disagrees. Rewriting a correct value is churn, and the
`updated` counts below are the evidence of what was actually wrong.

Usage:
  python tools/repair_durations.py <vaino.db> [--write] [--delete-phantoms]
"""

from __future__ import annotations

import concurrent.futures as futures
import json
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path

FFPROBE = shutil.which("ffprobe")

# Below this, a difference is rounding rather than error. MP3 frame duration is
# ~26 ms, so a second of slack is comfortably clear of encoder granularity.
TOLERANCE_MS = 1000


def probe(row: tuple) -> tuple[int, str, int, float | None]:
    fid, path, dur = row
    try:
        r = subprocess.run(
            [FFPROBE, "-v", "error", "-show_entries", "format=duration",
             "-of", "json", path],
            capture_output=True, timeout=120,
        )
        if r.returncode == 0:
            return fid, path, dur, float(json.loads(r.stdout)["format"]["duration"]) * 1000
    except (subprocess.TimeoutExpired, json.JSONDecodeError, KeyError, ValueError, OSError):
        pass
    return fid, path, dur, None


def main() -> int:
    args = sys.argv[1:]
    if not args or not FFPROBE:
        print(__doc__ if args else "ffprobe not found")
        return 2
    db = Path(args[0])
    write = "--write" in args
    delete_phantoms = "--delete-phantoms" in args

    con = sqlite3.connect(db)
    files = con.execute("SELECT file_id, path, duration_ms FROM files").fetchall()
    print(f"probing {len(files)} files...", flush=True)

    real: dict[int, float] = {}
    unreadable = 0
    with futures.ThreadPoolExecutor(max_workers=12) as pool:
        for i, (fid, _p, _d, rv) in enumerate(pool.map(probe, files), 1):
            if rv is None:
                unreadable += 1
            else:
                real[fid] = rv
            if i % 1000 == 0:
                print(f"  {i}/{len(files)}", flush=True)

    wrong = [(fid, p, d) for fid, p, d in files
             if fid in real and abs(d - real[fid]) > TOLERANCE_MS]
    over = [x for x in wrong if x[2] - real[x[0]] > TOLERANCE_MS]
    print(f"\nfiles probed {len(real)}, unreadable {unreadable}")
    print(f"  duration wrong by >{TOLERANCE_MS} ms: {len(wrong)} ({100*len(wrong)/max(len(real),1):.1f}%)")
    print(f"    of which OVER-state the file: {len(over)}")
    if wrong:
        diffs = sorted(abs(d - real[fid]) / 1000 for fid, _p, d in wrong)
        print(f"    error seconds: median {diffs[len(diffs)//2]:.1f}  p95 "
              f"{diffs[int(len(diffs)*0.95)]:.1f}  max {diffs[-1]:.1f}")

    # Passages measured against a wrong duration.
    rows = con.execute(
        "SELECT passage_id, file_id, start_ms, end_ms, kind FROM passages"
    ).fetchall()
    phantom = [r for r in rows if r[1] in real and r[2] >= real[r[1]] - TOLERANCE_MS]
    overrun = [r for r in rows if r[1] in real and r[2] < real[r[1]] - TOLERANCE_MS
               and r[3] > real[r[1]] + TOLERANCE_MS]
    print(f"\npassages {len(rows)}")
    print(f"  PHANTOM  (start at/past the real end, unplayable): {len(phantom)}")
    print(f"  overrun  (end past the real end, clamp to it):     {len(overrun)}")
    for pid, fid, s, e, kind in phantom[:6]:
        print(f"     phantom {pid} [{kind}] {s/60000:.1f}-{e/60000:.1f} min, "
              f"real end {real[fid]/60000:.1f}")

    if not write:
        print("\n(dry run -- pass --write to apply)")
        return 0

    for fid, _p, _d in wrong:
        con.execute("UPDATE files SET duration_ms = ? WHERE file_id = ?",
                    (int(round(real[fid])), fid))
    for pid, fid, _s, _e, _k in overrun:
        con.execute("UPDATE passages SET end_ms = ? WHERE passage_id = ?",
                    (int(round(real[fid])), pid))
    print(f"\nupdated {len(wrong)} durations, clamped {len(overrun)} passage ends")

    if delete_phantoms and phantom:
        ids = [p[0] for p in phantom]
        q = ",".join("?" * len(ids))
        con.execute(f"DELETE FROM passage_recordings WHERE passage_id IN ({q})", ids)
        con.execute(f"DELETE FROM passages WHERE passage_id IN ({q})", ids)
        print(f"deleted {len(ids)} phantom passages")
    elif phantom:
        print(f"{len(phantom)} phantom passages left in place "
              f"(pass --delete-phantoms to remove)")
    con.commit()

    bad = con.execute("SELECT COUNT(*) FROM passages WHERE end_ms <= start_ms").fetchone()[0]
    print(f"integrity: {con.execute('PRAGMA integrity_check').fetchone()[0]}, "
          f"passages violating end>start: {bad}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

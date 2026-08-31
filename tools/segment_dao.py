#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Segment a disc-at-once capture into its tracks `[SPEC-SA-070]`.

A DAO file is a whole side or disc in one recording. Ingested whole it becomes
one 160-minute "track": useless for rotation, and its flavor is the average of
forty songs, which describes none of them.

Inherited from McRhythm's `[AFS-SIL-010..040]`, with one correction its
document does not make. McRhythm places a boundary "at the midpoint of each
silent period". The 188 files already segmented in this library do not do
that -- their passages leave the silence OUT, ending where the music ends and
resuming where the next begins. A passage is the audible content, and the gap
between two of them is the gap between two songs. Matching the existing
convention is what makes the result comparable with 2,676 hand-checked
boundaries rather than merely plausible.

    python tools/segment_dao.py <db> --validate [--limit N]         against known files
    python tools/segment_dao.py <db> --file <path>                 propose, print only
    python tools/segment_dao.py <db> --file <path> --expect N       count-driven, print only
    python tools/segment_dao.py <db> --file <path> --commit         propose, identify, write
    python tools/segment_dao.py <db> --file <path> --expect N --commit

`--commit` requires the file to already be in the library -- run
`tools/ingest_folder.py` on its folder first `[SPEC-SA-070]`, `[SPEC-SA-110]`.
It replaces that whole-file placeholder with one identified `radio`/`album`
pair per track, the same album/radio duality `ingest_folder.py` now writes
by default for a single-track file `[GDE-BMK-030]`. Without `--expect`, both
the preview and `--commit` need an AcoustID key (`secrets/acoustid.key` or
`ACOUSTID_KEY`) -- `propose()`'s own threshold choice is identification
rate, so even looking has to look things up.
"""

import argparse
import os
import re
import subprocess
import sys

# `[AFS-SIL-020]` gives one threshold per source medium, and measurement says
# that is not enough. These are MP3 rips: lossy encoding leaves noise where the
# CD had digital silence, so -80 dB finds almost nothing -- 34 boundaries where
# 298 were known. Worse, the right value is not a property of the medium but of
# the individual rip. Across 14 known files the winner ranged from -70 dB to
# -30 dB, and no single value came close on all of them.
#
# So the threshold is SWEPT and chosen by the count it produces. That needs an
# expected count to aim at, which is exactly what `[AFS-MB-030]` provides: the
# track count of the MusicBrainz release. Given one, this found the known
# segmentation of 14 of 14 files exactly.
THRESHOLDS = {"cd": -80, "vinyl": -60, "cassette-dolby": -70,
              "cassette": -50, "other": -60}

# Coarse to fine; -70 wins most often, so it is first and cheap runs stop early.
SWEEP = (-70, -60, -50, -45, -40, -35, -30, -25, -80)
MIN_SILENCE_S = 0.5          # `[AFS-SIL-020]`

# Below this a "track" is an artefact -- a lead-in tick, applause between
# movements -- not something to put in rotation.
MIN_TRACK_S = 20.0


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def silences(path: str, threshold_db: int, min_s: float) -> list[tuple[float, float]]:
    """Every silent span, from ffmpeg's own detector.

    One decode of the file, which is the expensive part; the thresholds are
    cheap to vary afterwards only by decoding again, so callers that sweep
    should expect to pay per sweep.
    """
    r = subprocess.run(
        ["ffmpeg", "-hide_banner", "-nostats", "-i", path,
         "-af", f"silencedetect=noise={threshold_db}dB:d={min_s}",
         "-f", "null", "-"],
        capture_output=True, text=True, errors="replace")
    out = []
    start = None
    for m in re.finditer(r"silence_(start|end): (-?[\d.]+)", r.stderr):
        kind, at = m.group(1), float(m.group(2))
        if kind == "start":
            start = at
        elif start is not None:
            out.append((start, at))
            start = None
    return out


def duration(path: str) -> float | None:
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                        "-of", "default=nw=1:nk=1", path], capture_output=True, text=True)
    try:
        return float(r.stdout.strip())
    except ValueError:
        return None


def spans_at(path: str, threshold_db: int, total: float,
             min_track_s: float = MIN_TRACK_S):
    """Audible spans, in seconds. The silence between them belongs to neither."""
    spans, at = [], 0.0
    for s, e in silences(path, threshold_db, MIN_SILENCE_S):
        if s - at >= min_track_s:
            spans.append((at, s))
        at = e
    if total - at >= min_track_s:
        spans.append((at, total))
    return spans


def segment_with_threshold(path: str, medium: str = "cd", min_track_s: float = MIN_TRACK_S,
                            expect: int | None = None):
    """Tracks in a capture, and the threshold that found them.

    With `expect` -- the release's track count -- the threshold is swept and
    the one hitting that count wins, stopping at the first exact hit. Without
    it, the medium's single default is used and the answer is a guess: that is
    the mode that found 34 boundaries where 298 were known.

    Returns `(threshold_db, spans)`, or `None` if the file would not decode.
    The threshold travels with the spans because `--commit` needs it for
    `boundary_src` `[SPEC-SA-110]`; `segment()` below drops it for callers
    that only ever wanted the spans.
    """
    total = duration(path)
    if total is None:
        return None
    if expect is None:
        db = THRESHOLDS.get(medium, -60)
        return db, spans_at(path, db, total, min_track_s)
    best = None
    for db in SWEEP:
        spans = spans_at(path, db, total, min_track_s)
        err = abs(len(spans) - expect)
        if best is None or err < best[0]:
            best = (err, db, spans)
        if err == 0:
            break
    return best[1], best[2]


def segment(path: str, medium: str = "cd", min_track_s: float = MIN_TRACK_S,
            expect: int | None = None):
    """Tracks in a capture -- spans only. See `segment_with_threshold` for
    the threshold alongside them, which `validate()` below has no use for
    but `--commit` does.
    """
    result = segment_with_threshold(path, medium, min_track_s, expect)
    return None if result is None else result[1]


# ------------------------------------------------------------- identification

def _best_recording(path: str, start: float, end: float, key: str) -> dict | None:
    """The AcoustID `recordings` entry that best matches this span, as the
    raw structured record (`id`, `title`, `artists`), or `None`.

    Shared by `identify()` (a display string, for the threshold sweep and the
    plain preview) and `identify_recording()` (`--commit`'s own structured
    need `[SPEC-SA-110]`) -- one fingerprint-and-lookup, two shapes of
    answer, so the two can never disagree about what a lookup returned.
    """
    import json
    import urllib.error
    import urllib.parse
    import urllib.request
    fp = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-ss", f"{start:.2f}",
         "-t", f"{min(120.0, end - start):.2f}", "-i", path,
         "-f", "chromaprint", "-fp_format", "base64", "-"],
        capture_output=True).stdout.decode("ascii", "ignore").strip()
    if not fp:
        return None
    q = urllib.parse.urlencode({"client": key, "duration": str(int(end - start)),
                                "fingerprint": fp, "meta": "recordings"})
    req = urllib.request.Request("https://api.acoustid.org/v2/lookup", data=q.encode(),
                                 headers={"User-Agent": "Vaino/0.1"})
    try:
        d = json.load(urllib.request.urlopen(req, timeout=45))
    except Exception:                                     # noqa: BLE001
        return None
    for r in d.get("results") or []:
        for rec in r.get("recordings") or []:
            if rec.get("title"):
                return rec
    return None


def identify(path: str, start: float, end: float, key: str):
    """What AcoustID calls the audio between two marks, or `None`.

    The acceptance test that generalises `[AFS-MB-030]`. Choosing a threshold
    by track count needs the count to be right, and a pressing that differs
    from MusicBrainz by two or three tracks -- which is the normal state of a
    special edition -- can never satisfy it. Whether a segment *identifies*
    asks nothing about the total: a boundary in the wrong place cuts a song in
    half and matches nothing, which is the signal worth having.
    """
    rec = _best_recording(path, start, end, key)
    if rec is None:
        return None
    artists = ", ".join(a.get("name", "") for a in rec.get("artists") or [])
    return f"{rec['title']} — {artists}" if artists else rec["title"]


def identify_recording(path: str, start: float, end: float, key: str) -> dict | None:
    """Structured identification for `--commit` `[SPEC-SA-110]`: the
    recording's own mbid, title, and every credited artist's `(mbid, name)`
    -- everything `recordings`/`recording_artists`/`passage_recordings` need,
    not just `identify()`'s display string. `None` for the same "nothing
    matched" case `identify()` already returns for.
    """
    rec = _best_recording(path, start, end, key)
    if rec is None:
        return None
    return {
        "mbid": rec["id"],
        "title": rec["title"],
        "artists": [(a["id"], a.get("name") or "?")
                    for a in rec.get("artists") or [] if a.get("id")],
    }


def propose(path: str, key: str, samples: int = 8, grid=SWEEP):
    """The threshold whose segments identify best, and its boundaries."""
    total = duration(path)
    if total is None:
        return None
    best = None
    for db in grid:
        spans = spans_at(path, db, total)
        if not spans:
            continue
        step = max(1, len(spans) // samples)
        picks = list(range(0, len(spans), step))[:samples]
        hits = sum(1 for i in picks if identify(path, *spans[i], key) is not None)
        rate = hits / len(picks)
        say(f"    {db:>4} dB: {len(spans):3d} segments, {hits}/{len(picks)} identified")
        if best is None or rate > best[0]:
            best = (rate, db, spans)
        if rate == 1.0:
            break
    return best


# ------------------------------------------------------------------ validate

def validate(db: str, limit: int, medium: str, tolerance: float) -> int:
    """Run the detector over files whose segmentation is already known.

    The only honest way to find out whether this works. 188 files, 2,676
    boundaries, none of them produced by this code.
    """
    import sqlite3
    c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    files = c.execute(
        """SELECT f.file_id, f.path, COUNT(*) n FROM passages p JOIN files f USING(file_id)
            WHERE p.kind='radio' GROUP BY f.file_id HAVING n>1 ORDER BY n DESC""").fetchall()
    files = [f for f in files if os.path.exists(f[1])]
    if limit:
        files = files[:limit]

    say(f"{len(files)} already-segmented file(s); threshold swept per file\n")
    exact = 0
    total_known = total_found = matched = 0
    off = []
    drift = []
    for i, (fid, path, n) in enumerate(files, 1):
        known = [(s / 1000.0, e / 1000.0) for s, e in c.execute(
            "SELECT start_ms,end_ms FROM passages WHERE file_id=?1 AND kind='radio' "
            "ORDER BY start_ms", (fid,))]
        # The known count stands in for the release's track count, which
        # is what `[AFS-MB-030]` supplies in the real pipeline.
        found = segment(path, medium, expect=len(known))
        if found is None:
            say(f"  {os.path.basename(path)[:44]}: would not decode")
            continue
        total_known += len(known)
        total_found += len(found)
        if len(found) == len(known):
            exact += 1
        # A found start counts as matching a known start within tolerance.
        used = set()
        for ks, _ in known:
            for j, (fs, _) in enumerate(found):
                if j not in used and abs(fs - ks) <= tolerance:
                    used.add(j)
                    matched += 1
                    break
        if len(found) != len(known):
            off.append((os.path.basename(path), len(known), len(found)))
        else:
            # Where the count is right, how close are the boundaries?
            drift.extend(abs(f[0]-k[0]) for k, f in zip(known, found))
        if i % 25 == 0:
            say(f"  {i}/{len(files)}  exact track count on {exact}")

    say(f"\n  files with the exact track count : {exact}/{len(files)}  "
        f"({exact/max(len(files),1):.0%})")
    say(f"  boundaries known / found         : {total_known} / {total_found}")
    say(f"  starts matched within {tolerance:.1f}s        : {matched} "
        f"({matched/max(total_known,1):.0%})")
    if off:
        say(f"\n  worst count mismatches (of {len(off)}):")
        for name, k, f in sorted(off, key=lambda x: -abs(x[1]-x[2]))[:8]:
            say(f"    known {k:3d}  found {f:3d}   {name[:46]}")
    return 0


# --------------------------------------------------------------------- commit

def commit_segments(conn, file_id: int, audio_md5: str, path: str,
                     spans: list[tuple[float, float]], threshold_db: int, key: str) -> dict:
    """Replace `file_id`'s passages with a real segmentation `[SPEC-SA-070]`,
    one identified `radio`/`album` pair per track `[SPEC-SA-110]`,
    `[GDE-BMK-030]`.

    Replaces rather than adds to what is already there: the ordinary
    starting point for a DAO capture is `tools/ingest_folder.py`'s own
    whole-file `radio`/`album` pair, a deliberate one-track-per-file
    placeholder for exactly this shape of file, not a rival segmentation
    worth keeping once a real one exists.

    Each span is identified independently, the same convention this
    library's own 133-of-136 already-segmented tracks were produced under: a
    confident AcoustID match gets the real MusicBrainz recording (and every
    credited artist); anything else gets `local:audio:<md5>:<start_ms>`, the
    same placeholder shape `local:audio:<md5>` alone already means for a
    single-track file, extended with the one thing that makes it unique
    inside a DAO capture -- where in the file it starts.
    """
    old = [r[0] for r in conn.execute(
        "SELECT passage_id FROM passages WHERE file_id=?1", (file_id,))]
    if old:
        conn.execute(
            f"DELETE FROM passage_recordings WHERE passage_id IN ({','.join('?' * len(old))})", old)
        conn.execute("DELETE FROM passages WHERE file_id=?1", (file_id,))

    boundary_src = f"segment:silence-{threshold_db}dB+acoustid"
    identified = unidentified = 0
    for start_s, end_s in spans:
        start_ms, end_ms = round(start_s * 1000), round(end_s * 1000)
        rec = identify_recording(path, start_s, end_s, key)
        if rec is not None:
            mbid, source = rec["mbid"], "segment:acoustid"
            conn.execute(
                "INSERT OR IGNORE INTO recordings (mbid,title,length_ms,source) "
                "VALUES (?1,?2,?3,?4)", (mbid, rec["title"], end_ms - start_ms, source))
            for artist_mbid, name in rec["artists"]:
                conn.execute(
                    "INSERT OR IGNORE INTO artists (mbid,name,source) VALUES (?1,?2,?3)",
                    (artist_mbid, name, source))
                conn.execute(
                    "INSERT OR IGNORE INTO recording_artists (mbid,artist_mbid,weight,source) "
                    "VALUES (?1,?2,1.0,?3)", (mbid, artist_mbid, source))
            identified += 1
        else:
            mbid, source = f"local:audio:{audio_md5}:{start_ms}", "segment:unidentified"
            conn.execute(
                "INSERT OR IGNORE INTO recordings (mbid,title,length_ms,source) "
                "VALUES (?1,?2,?3,?4)",
                (mbid, f"unidentified segment at {round(start_ms / 60000)} min",
                 end_ms - start_ms, source))
            unidentified += 1

        # Both kinds, one identification `[SPEC-SA-110]` -- see
        # `ingest_folder.py`'s own `--kind both` for why `album`'s lead/gain
        # are 0, not NULL: its segue points equal its own hard boundaries
        # `[GDE-BMK-030]`, permanently, not a value awaiting analysis.
        for kind in ("radio", "album"):
            if kind == "album":
                pid = conn.execute(
                    "INSERT INTO passages (file_id,kind,start_ms,end_ms,"
                    "lead_in_ms,lead_out_ms,gain_db,boundary_src) "
                    "VALUES (?1,?2,?3,?4,0,0,0.0,?5)",
                    (file_id, kind, start_ms, end_ms, boundary_src)).lastrowid
            else:
                pid = conn.execute(
                    "INSERT INTO passages (file_id,kind,start_ms,end_ms,boundary_src) "
                    "VALUES (?1,?2,?3,?4,?5)",
                    (file_id, kind, start_ms, end_ms, boundary_src)).lastrowid
            conn.execute(
                "INSERT INTO passage_recordings (passage_id,mbid,weight,source) "
                "VALUES (?1,?2,1.0,?3)", (pid, mbid, source))

    return {"tracks": len(spans), "identified": identified,
            "unidentified": unidentified, "replaced": len(old)}


def do_commit(db_path: str, path: str, spans: list[tuple[float, float]], threshold_db: int) -> int:
    """`--commit`'s own I/O: find the file, get a key, write the transaction.

    Deliberately does not create `files`/`file_tags` rows itself -- that is
    `tools/ingest_folder.py`'s job, and it already ran first for this file to
    exist here at all `[SPEC-SA-070]`. Refusing plainly when it has not, per
    `[SPEC-DF-095]`, beats guessing at tags this tool never read.
    """
    import sqlite3
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import ingest_folder  # noqa: E402  -- same audio_md5(), not a second copy of it
    import secret  # noqa: E402

    md5 = ingest_folder.audio_md5(path)
    if md5 is None:
        say("would not decode")
        return 1

    conn = sqlite3.connect(db_path, timeout=60)
    conn.execute("PRAGMA busy_timeout = 60000")
    conn.execute("PRAGMA foreign_keys = ON")
    row = conn.execute("SELECT file_id FROM files WHERE audio_md5=?1", (md5,)).fetchone()
    if row is None:
        say(f"\nnot in the library yet -- run tools/ingest_folder.py on its folder "
            f"first, then re-run this with --commit")
        return 1
    file_id = row[0]

    key = secret.acoustid_key()  # required=True: --commit always identifies
    conn.execute("BEGIN IMMEDIATE")
    result = commit_segments(conn, file_id, md5, path, spans, threshold_db, key)
    conn.commit()
    say(f"\nreplaced {result['replaced']} old passage(s) with {result['tracks']} track(s), "
        f"{result['tracks']} radio + {result['tracks']} album: "
        f"{result['identified']} identified, {result['unidentified']} not")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("--validate", action="store_true")
    ap.add_argument("--file")
    ap.add_argument("--medium", default="cd", choices=sorted(THRESHOLDS))
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--tolerance", type=float, default=2.0)
    ap.add_argument("--commit", action="store_true")
    # The release's own track count, when it is known `[AFS-MB-030]` -- swept
    # for the threshold hitting that count, no AcoustID lookup needed to
    # *choose* one. Without it, the threshold is chosen by identification
    # rate instead (`propose()`), which does need a key even just to preview.
    ap.add_argument("--expect", type=int)
    args = ap.parse_args()

    if args.validate:
        return validate(args.db, args.limit, args.medium, args.tolerance)
    if args.file:
        if args.expect:
            found = segment_with_threshold(args.file, args.medium, expect=args.expect)
            if found is None:
                say("would not decode")
                return 1
            threshold_db, spans = found
        else:
            sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
            import secret  # noqa: E402
            # A key is required here even for a bare preview: unlike
            # `--expect`, `propose()`'s own threshold choice is identification
            # rate, which has no meaning without looking anything up.
            key = secret.acoustid_key()
            proposed = propose(args.file, key)
            if proposed is None:
                say("would not decode, or nothing identified at any threshold")
                return 1
            _, threshold_db, spans = proposed
        say(f"\n{len(spans)} track(s) in {os.path.basename(args.file)} ({threshold_db} dB)")
        for i, (s, e) in enumerate(spans, 1):
            say(f"  {i:3d}  {s/60:6.2f}–{e/60:6.2f} min   ({e-s:6.1f}s)")
        if args.commit:
            return do_commit(args.db, args.file, spans, threshold_db)
        return 0
    say(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())

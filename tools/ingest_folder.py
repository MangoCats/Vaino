#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Bring a folder of audio into the library `[REQ-LIB-100]`.

The smallest honest version of the requirement the project exists for. Vaino's
library was a one-time migration of MuLibPlay's catalogue -- every row of
`passage_recordings` reads `inherited:mulib` -- so nothing has ever noticed a
new file appearing on disk. A four-track EP sat in `Music/Mangocats/Tropicat`
through the entire scan, four months old and perfectly readable, and was never
ingested because MuLibPlay had never heard of it.

What this does NOT do, deliberately: segment multi-track files `[SPEC-SA-070]`,
compute flavor `[REQ-LIB-120]`, or invent a MusicBrainz identity. One passage
per file, spanning the whole file, is right for single-track audio and wrong
for a DAO capture -- so it refuses nothing and claims nothing.

    python tools/ingest_folder.py data/vaino_new.db "C:/path/to/album"
    python tools/ingest_folder.py data/vaino_new.db "C:/path/to/album" --commit
"""

import argparse
import json
import os
import re
import sqlite3
import subprocess
import sys
import time

AUDIO = (".mp3", ".flac", ".ogg", ".m4a", ".wav", ".opus")

# Self-published music has no MusicBrainz entry and inventing one would be a
# lie that everything downstream believes. The id is derived from the audio, so
# it is stable across re-ingests, unique per recording, and -- crucially --
# fails `is_mbid()`, which is how the player knows not to trust it as one.
#
# Distinct from the migration's `local:track:N`, which was a positional
# placeholder: two passages shared `local:track:827`, so it did not even
# identify a track.
LOCAL_PREFIX = "local:audio:"
LOCAL_SOURCE = "local:ingest"


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def audio_md5(path: str) -> str | None:
    """Essentia's `md5_encoded`, via ffmpeg -- the same value the migration
    used, so a file ingested here and one migrated hash alike."""
    r = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vn", "-c:a", "copy",
                        "-f", "md5", "-"], capture_output=True, text=True)
    if r.returncode != 0:
        return None
    m = re.search(r"MD5=([0-9a-f]{32})", r.stdout)
    return m.group(1) if m else None


def probe(path: str) -> dict | None:
    """Duration and tags in one call."""
    r = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries",
         "format=duration:format_tags=title,artist,album,track,disc",
         "-show_entries", "stream=codec_type", "-of", "json", path],
        capture_output=True, text=True)
    if r.returncode != 0:
        return None
    try:
        d = json.loads(r.stdout)
    except json.JSONDecodeError:
        return None
    fmt = d.get("format") or {}
    if not fmt.get("duration"):
        return None
    tags = {k.lower(): v for k, v in (fmt.get("tags") or {}).items()}
    has_art = any(s.get("codec_type") == "video" for s in d.get("streams") or [])
    return {
        "duration_ms": int(float(fmt["duration"]) * 1000),
        "title": tags.get("title"),
        "artist": tags.get("artist"),
        "album": tags.get("album"),
        # "3/12" is as common as "3"; the leading number is the answer.
        "track_no": first_int(tags.get("track")),
        "disc_no": first_int(tags.get("disc")),
        "has_art": 1 if has_art else 0,
    }


def first_int(v):
    if not v:
        return None
    m = re.match(r"\s*(\d+)", str(v))
    return int(m.group(1)) if m else None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("folder")
    ap.add_argument("--commit", action="store_true")
    ap.add_argument("--kind", default="radio", choices=("radio", "album"))
    args = ap.parse_args()

    if not os.path.isdir(args.folder):
        say(f"not a folder: {args.folder}")
        return 1

    files = sorted(
        os.path.join(dp, n)
        for dp, _, ns in os.walk(args.folder)
        for n in ns if n.lower().endswith(AUDIO))
    if not files:
        say(f"no audio under {args.folder}")
        return 1
    say(f"{len(files)} audio file(s) under {args.folder}\n")

    conn = sqlite3.connect(args.db, timeout=60)
    conn.execute("PRAGMA busy_timeout = 60000")
    conn.execute("PRAGMA foreign_keys = ON")

    added = skipped = failed = 0
    now = time.strftime("%Y-%m-%dT%H:%M:%S")
    if args.commit:
        conn.execute("BEGIN IMMEDIATE")

    for path in files:
        md5 = audio_md5(path)
        info = probe(path)
        name = os.path.basename(path)
        if md5 is None or info is None:
            say(f"  SKIP  {name}  (would not decode)")
            failed += 1
            continue
        # `files.audio_md5` is UNIQUE, and it is the identity: the same audio
        # under a different name is the same file, which is also what makes
        # re-running this safe.
        seen = conn.execute("SELECT file_id FROM files WHERE audio_md5 = ?1", (md5,)).fetchone()
        if seen:
            say(f"  have  {name}")
            skipped += 1
            continue

        say(f"  ADD   {name}  {info['duration_ms']/1000:.0f}s"
            + (f"  \u201c{info['title']}\u201d" if info["title"] else "")
            + ("  [embedded art]" if info["has_art"] else ""))
        added += 1
        if not args.commit:
            continue

        st = os.stat(path)
        cur = conn.execute(
            "INSERT INTO files (audio_md5,path,size_bytes,mtime,format,duration_ms,"
            "                   first_seen,last_seen) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            (md5, path, st.st_size, st.st_mtime,
             os.path.splitext(path)[1].lstrip(".").lower(), info["duration_ms"], now))
        fid = cur.lastrowid

        conn.execute(
            "INSERT INTO file_tags (file_id,title,artist,album,track_no,disc_no,"
            "                       has_art,scanned_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            (fid, info["title"], info["artist"], info["album"],
             info["track_no"], info["disc_no"], info["has_art"], int(time.time())))

        # One passage spanning the file. Right for single-track audio; a DAO
        # capture needs segmentation, which is not this tool's business.
        pid = conn.execute(
            "INSERT INTO passages (file_id,kind,start_ms,end_ms,boundary_src) "
            "VALUES (?1,?2,0,?3,'ingest:whole-file')",
            (fid, args.kind, info["duration_ms"])).lastrowid

        mbid = LOCAL_PREFIX + md5
        conn.execute(
            "INSERT OR IGNORE INTO recordings (mbid,title,length_ms,source) VALUES (?1,?2,?3,?4)",
            (mbid, info["title"] or os.path.splitext(name)[0], info["duration_ms"], LOCAL_SOURCE))
        conn.execute(
            "INSERT INTO passage_recordings (passage_id,mbid,weight,source) "
            "VALUES (?1,?2,1.0,?3)", (pid, mbid, LOCAL_SOURCE))

    if args.commit:
        conn.commit()
        say(f"\nadded {added}, already present {skipped}, unreadable {failed}")
        say("They are `kind=radio` passages, so the Director can pick them now.")
        say(f"Identity is {LOCAL_PREFIX}<md5> -- deliberately not an MBID, because")
        say("there is no MusicBrainz entry to point at and inventing one would")
        say("be a lie the whole library would then believe.")
    else:
        say(f"\nwould add {added}, already present {skipped}, unreadable {failed}")
        say("nothing was written. Re-run with --commit to do it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

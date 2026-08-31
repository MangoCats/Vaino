#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `ingest_folder.py`'s album/radio duality `[SPEC-SA-110]`,
`[GDE-BMK-030]`.

Exercises the contract on its own -- `--kind both` (the new default) writes
one `radio` and one `album` passage per file, sharing the same span and the
same recording link; an album cut's lead/gain are 0, permanently, not NULL
the way a fresh radio cut's are; and a re-run over the same audio adds
nothing a second time. `probe()`/`audio_md5()` shell out to ffmpeg/ffprobe --
mocked here rather than run for real, the same reasoning every other tool
test in this directory already takes toward audio it does not need to
actually decode.

    python tools/test_ingest_folder.py
"""

import os
import sqlite3
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ingest_folder  # noqa: E402

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL: {msg}")


# The minimum this tool's own INSERTs touch -- not the full `sql/schema.sql`,
# the same "just enough" shape every other tool test in this directory uses.
SCHEMA = """
CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT NOT NULL UNIQUE,
    path TEXT, size_bytes INTEGER, mtime REAL, format TEXT, duration_ms INTEGER,
    first_seen TEXT, last_seen TEXT);
CREATE TABLE file_tags (file_id INTEGER, title TEXT, artist TEXT, album TEXT,
    track_no INTEGER, disc_no INTEGER, has_art INTEGER, scanned_at INTEGER);
CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER,
    kind TEXT NOT NULL, start_ms INTEGER, end_ms INTEGER,
    lead_in_ms INTEGER, lead_out_ms INTEGER, gain_db REAL,
    boundary_src TEXT);
CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT, length_ms INTEGER, source TEXT);
CREATE TABLE passage_recordings (passage_id INTEGER NOT NULL, mbid TEXT NOT NULL,
    weight REAL, source TEXT);
CREATE TABLE ingest_decisions (decision_id INTEGER PRIMARY KEY, audio_md5 TEXT,
    stage TEXT, outcome TEXT, confidence REAL, detail TEXT, decided_at TEXT);
"""

FAKE_MD5 = "deadbeef" * 4  # 32 hex chars, the real shape audio_md5() returns


def run(db_path: str, folder: str, extra_args=()) -> int:
    old_argv = sys.argv
    sys.argv = ["ingest_folder.py", db_path, folder, "--commit", *extra_args]
    try:
        return ingest_folder.main()
    finally:
        sys.argv = old_argv


def main() -> int:
    fd, tmp_db = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    os.remove(tmp_db)
    conn = sqlite3.connect(tmp_db)
    conn.executescript(SCHEMA)
    conn.close()

    # Real shelling-out replaced with a fixed answer -- what this test checks
    # is what `main()` does with a probe result, not whether ffmpeg runs.
    ingest_folder.audio_md5 = lambda p: FAKE_MD5
    ingest_folder.probe = lambda p: {
        "duration_ms": 200000, "title": "A Song", "artist": "Someone",
        "album": None, "track_no": None, "disc_no": None, "has_art": 0,
    }

    with tempfile.TemporaryDirectory() as folder:
        with open(os.path.join(folder, "song.mp3"), "wb") as f:
            f.write(b"not real audio -- probe()/audio_md5() are mocked above")

        print("--kind both (the new default): one radio cut, one album cut, "
              "same span, same recording")
        run(tmp_db, folder)
        conn = sqlite3.connect(tmp_db)
        conn.row_factory = sqlite3.Row
        rows = conn.execute("SELECT * FROM passages ORDER BY kind").fetchall()
        check(len(rows) == 2, f"expected 2 passages (radio+album), got {len(rows)}")
        by_kind = {r["kind"]: r for r in rows}
        check(set(by_kind) == {"radio", "album"}, f"expected radio+album, got {set(by_kind)}")
        if set(by_kind) == {"radio", "album"}:
            radio, album = by_kind["radio"], by_kind["album"]
            check(radio["lead_in_ms"] is None and radio["lead_out_ms"] is None,
                  "a fresh radio cut must await analysis (NULL), not claim 0 up front")
            check((album["lead_in_ms"], album["lead_out_ms"], album["gain_db"]) == (0, 0, 0.0),
                  "an album cut's segue points must equal its own hard boundaries "
                  f"[GDE-BMK-030], got {(album['lead_in_ms'], album['lead_out_ms'], album['gain_db'])}")
            check((radio["start_ms"], radio["end_ms"]) == (album["start_ms"], album["end_ms"]) == (0, 200000),
                  "both must share the file's own full span")

            mbid = ingest_folder.LOCAL_PREFIX + FAKE_MD5
            prs = {r["passage_id"] for r in conn.execute(
                "SELECT passage_id FROM passage_recordings WHERE mbid=?", (mbid,))}
            check(prs == {radio["passage_id"], album["passage_id"]},
                  f"both passages must link to the same recording, got {prs}")
        conn.close()

        print()
        print("re-running against the same audio: already present, nothing duplicated")
        run(tmp_db, folder)
        conn = sqlite3.connect(tmp_db)
        n = conn.execute("SELECT COUNT(*) FROM passages").fetchone()[0]
        check(n == 2, f"a re-run over the same audio must not add more passages, got {n}")
        conn.close()

    print()
    print("--kind radio (explicit override): the single-kind path still works")
    fd, tmp_db2 = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    os.remove(tmp_db2)
    conn = sqlite3.connect(tmp_db2)
    conn.executescript(SCHEMA)
    conn.close()
    with tempfile.TemporaryDirectory() as folder:
        with open(os.path.join(folder, "song.mp3"), "wb") as f:
            f.write(b"not real audio -- probe()/audio_md5() are mocked above")
        run(tmp_db2, folder, extra_args=["--kind", "radio"])
        conn = sqlite3.connect(tmp_db2)
        rows = conn.execute("SELECT kind FROM passages").fetchall()
        check(rows == [("radio",)], f"expected exactly one radio passage, got {rows}")
        conn.close()

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("ingest_folder: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

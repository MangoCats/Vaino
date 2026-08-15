#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `apply_reviews.py` `[REQ-LIB-165]`.

This tool had no tests, and the first version of it could not apply anything
at all: `recordings.title` and `recordings.source` are both NOT NULL, it
supplied neither, and `INSERT OR IGNORE` turned the violation into nothing
happening -- so the row was silently skipped and the foreign key failed on the
statement after. It passed every check that existed and failed on first
contact with the real library.

The schema below is therefore **copied from SPEC008 including every NOT NULL**.
That is the whole point: a fixture looser than production is a fixture that
certifies writes the real database rejects.

    python tools/test_apply_reviews.py
"""

import json
import os
import sqlite3
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "apply_reviews.py")

# Exactly as SPEC008 declares them. Do not relax these to make a test pass.
SCHEMA = """
CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT NOT NULL,
    path TEXT NOT NULL, size_bytes INTEGER NOT NULL, mtime REAL NOT NULL,
    format TEXT NOT NULL, duration_ms INTEGER NOT NULL,
    first_seen TEXT NOT NULL, last_seen TEXT NOT NULL);
CREATE TABLE passages (passage_id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(file_id),
    kind TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
    lead_in_ms INTEGER, lead_out_ms INTEGER, gain_db REAL,
    boundary_src TEXT NOT NULL);
CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT NOT NULL,
    length_ms INTEGER, source TEXT NOT NULL);
CREATE TABLE artists (mbid TEXT PRIMARY KEY, name TEXT NOT NULL,
    sort_name TEXT, source TEXT NOT NULL);
CREATE TABLE recording_artists (
    mbid TEXT NOT NULL REFERENCES recordings(mbid) ON DELETE CASCADE,
    artist_mbid TEXT NOT NULL REFERENCES artists(mbid),
    weight REAL NOT NULL DEFAULT 1.0, source TEXT NOT NULL,
    PRIMARY KEY (mbid, artist_mbid)) WITHOUT ROWID;
CREATE TABLE passage_recordings (
    passage_id INTEGER NOT NULL REFERENCES passages(passage_id) ON DELETE CASCADE,
    mbid TEXT NOT NULL REFERENCES recordings(mbid),
    weight REAL NOT NULL DEFAULT 1.0, source TEXT NOT NULL,
    PRIMARY KEY (passage_id, mbid)) WITHOUT ROWID;
CREATE TABLE releases (mbid TEXT PRIMARY KEY, title TEXT NOT NULL,
    release_date TEXT, source TEXT NOT NULL, release_group TEXT, status TEXT,
    primary_type TEXT, secondary_types TEXT, country TEXT, track_count INTEGER);
CREATE TABLE release_recordings (
    release_mbid TEXT NOT NULL REFERENCES releases(mbid) ON DELETE CASCADE,
    mbid TEXT NOT NULL REFERENCES recordings(mbid) ON DELETE CASCADE,
    position INTEGER, source TEXT NOT NULL, track_length_ms INTEGER,
    chosen INTEGER DEFAULT 0, disc INTEGER,
    PRIMARY KEY (release_mbid, mbid)) WITHOUT ROWID;
CREATE TABLE identification_cache (audio_md5 TEXT NOT NULL, service TEXT NOT NULL,
    request_key TEXT NOT NULL, response BLOB NOT NULL, fetched_at TEXT NOT NULL,
    PRIMARY KEY (audio_md5, service, request_key)) WITHOUT ROWID;
CREATE TABLE id_reviews (passage_id INTEGER PRIMARY KEY, decision TEXT NOT NULL,
    chosen_mbid TEXT, decided_at TEXT NOT NULL,
    chosen_release_mbid TEXT, previous_mbid TEXT, applied_at TEXT);
"""

OLD = "11111111-1111-1111-1111-111111111111"
NEW = "22222222-2222-2222-2222-222222222222"
ART = "33333333-3333-3333-3333-333333333333"
REL_A = "44444444-4444-4444-4444-444444444444"
REL_B = "55555555-5555-5555-5555-555555555555"

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL  {msg}")
    return cond


def build(tmp: str, *, cached=True, release=None) -> str:
    """A fresh library in its own directory, so each case starts clean."""
    os.makedirs(tmp, exist_ok=True)
    db = os.path.join(tmp, "t.db")
    c = sqlite3.connect(db)
    c.executescript(SCHEMA)
    c.execute("INSERT INTO files VALUES (1,'md5','/m/a.mp3',1,1.0,'mp3',300000,'t','t')")
    c.execute("INSERT INTO passages VALUES (1,1,'radio',1000,200000,NULL,NULL,NULL,'s')")
    c.execute("INSERT INTO recordings VALUES (?,'Wrong Name',NULL,'inherited:mulib')", (OLD,))
    c.execute("INSERT INTO passage_recordings VALUES (1,?,1.0,'inherited:mulib')", (OLD,))
    # Two releases carry the new recording, so "which album" is a real question.
    for mbid, title, date in ((REL_A, "The Album", "1985-01-01"),
                              (REL_B, "A Later Compilation", "2004-01-01")):
        c.execute("INSERT INTO releases (mbid,title,release_date,source,track_count) "
                  "VALUES (?,?,?,'mb',12)", (mbid, title, date))
        c.execute("INSERT INTO release_recordings (release_mbid,mbid,source,chosen) "
                  "VALUES (?,?,'mb',0)", (mbid, NEW))
    if cached:
        payload = json.dumps([{ "score": 0.99, "recordings": [
            {"id": NEW, "title": "Right Name",
             "artists": [{"id": ART, "name": "The Band"}]}]}])
        c.execute("INSERT INTO identification_cache VALUES "
                  "('md5','acoustid','chromaprint:1000-200000:120:base64',?,'t')",
                  (payload,))
    c.execute("INSERT INTO id_reviews (passage_id,decision,chosen_mbid,"
              "chosen_release_mbid,previous_mbid,decided_at) VALUES (1,'reassigned',?,?,?,'t')",
              (NEW, release, OLD))
    c.commit()
    c.close()
    return db


def run(db, *args):
    return subprocess.run([sys.executable, TOOL, db, *args],
                          capture_output=True, text=True)


def linked(db):
    c = sqlite3.connect(db)
    row = c.execute("SELECT mbid, source FROM passage_recordings WHERE passage_id=1").fetchone()
    c.close()
    return row


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:

        print("a rehearsal writes nothing")
        db = build(tmp + "/a")
        r = run(db)
        check(r.returncode == 0, f"rehearsal exited {r.returncode}: {r.stderr[:200]}")
        check("would apply 1" in r.stdout, f"expected a rehearsal summary, got {r.stdout!r}")
        check(linked(db)[0] == OLD, "a rehearsal must not change the link")

        print("a commit rewrites the link and creates what the key needs")
        r = run(db, "--commit")
        check(r.returncode == 0, f"commit exited {r.returncode}: {r.stderr[:300]}")
        mbid, source = linked(db)
        check(mbid == NEW, f"link is {mbid}, want {NEW}")
        check(source == "review:acoustid", f"source is {source!r}")
        c = sqlite3.connect(db)
        # The bug that shipped: NOT NULL columns omitted, INSERT OR IGNORE
        # swallowing it, foreign key failing on the next statement.
        title, rsrc = c.execute("SELECT title, source FROM recordings WHERE mbid=?",
                                (NEW,)).fetchone()
        check(title == "Right Name", f"recording title is {title!r}")
        check(rsrc is not None, "recordings.source must be filled")
        check(c.execute("SELECT COUNT(*) FROM recording_artists WHERE mbid=?",
                        (NEW,)).fetchone()[0] == 1, "the artist link was not made")
        check(c.execute("SELECT applied_at FROM id_reviews WHERE passage_id=1")
               .fetchone()[0] is not None, "applied_at must be stamped")
        c.close()

        print("re-running is a no-op, not a second application")
        r = run(db, "--commit")
        check("0 reassignment(s)" in r.stdout, f"expected nothing pending, got {r.stdout!r}")

        print("--revert puts the old id back and re-opens the passage")
        r = run(db, "--revert", "1")
        check(linked(db)[0] == NEW, "a revert rehearsal must not change the link")
        r = run(db, "--revert", "1", "--commit")
        check(r.returncode == 0, f"revert exited {r.returncode}: {r.stderr[:300]}")
        check(linked(db)[0] == OLD, f"after revert the link is {linked(db)[0]}, want {OLD}")
        c = sqlite3.connect(db)
        check(c.execute("SELECT COUNT(*) FROM id_reviews WHERE passage_id=1")
               .fetchone()[0] == 0, "revert must clear the decision so it returns to the queue")
        c.close()

        print("a recording with no cached name is REFUSED, not written nameless")
        db2 = build(tmp + "/b", cached=False)
        r = run(db2, "--commit")
        check(r.returncode == 0, f"exited {r.returncode}: {r.stderr[:200]}")
        check("refused" in r.stdout.lower(), f"expected a refusal, got {r.stdout!r}")
        check(linked(db2)[0] == OLD, "a nameless reassignment must not be applied")
        c = sqlite3.connect(db2)
        check(c.execute("SELECT COUNT(*) FROM recordings WHERE mbid=?",
                        (NEW,)).fetchone()[0] == 0,
              "no nameless recording row may be created")
        c.close()

        print("a preferred album is marked chosen, and only that one")
        db3 = build(tmp + "/c", release=REL_A)
        r = run(db3, "--commit")
        check(r.returncode == 0, f"exited {r.returncode}: {r.stderr[:300]}")
        c = sqlite3.connect(db3)
        chosen = dict(c.execute(
            "SELECT release_mbid, chosen FROM release_recordings WHERE mbid=?", (NEW,)))
        check(chosen.get(REL_A) == 1, f"the named release should be chosen, got {chosen}")
        check(chosen.get(REL_B) == 0, f"only one release may be chosen, got {chosen}")
        c.close()

        print("a release not linked to the recording is reported, not forced")
        db4 = build(tmp + "/d", release="99999999-9999-9999-9999-999999999999")
        r = run(db4, "--commit")
        check(r.returncode == 0, f"exited {r.returncode}: {r.stderr[:200]}")
        check("not linked" in r.stdout, f"expected a warning, got {r.stdout!r}")
        check(linked(db4)[0] == NEW, "the reassignment itself should still apply")

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("apply_reviews: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

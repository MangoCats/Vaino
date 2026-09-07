#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `split_database.py` `[IMPL002 §8]`.

Builds a small real source database -- both catalog and listener tables,
one deliberate index on each side -- and exercises the actual `main()`
entry point in both rehearsal and `--commit` modes, the same way this
project's other CLI tools are tested: through their real interface, not a
reimplementation of their internals.

    python tools/test_split_database.py
"""

import os
import sqlite3
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import split_database as sd  # noqa: E402

SCHEMA = """
CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT NOT NULL);
CREATE TABLE files (file_id INTEGER PRIMARY KEY, path TEXT NOT NULL);
CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
    kind TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL);
CREATE UNIQUE INDEX passages_span ON passages(file_id, kind, start_ms, end_ms);
CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY, mbid TEXT, played_at INTEGER);
CREATE INDEX listener_play_mbid ON listener_play_history(mbid);
CREATE TABLE player_state (id INTEGER PRIMARY KEY, position_ms INTEGER);
CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

INSERT INTO recordings VALUES ('mb-1', 'A Song');
INSERT INTO files VALUES (1, '/music/a.flac');
INSERT INTO passages VALUES (1, 1, 'album', 0, 200000);
INSERT INTO listener_play_history VALUES (1, 'mb-1', 1700000000);
INSERT INTO player_state VALUES (1, 5000);
INSERT INTO schema_meta VALUES ('schema_version', '1');
"""

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL  {msg}")
    return cond


def source_db() -> str:
    fd, path = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    conn = sqlite3.connect(path)
    conn.executescript(SCHEMA)
    conn.close()  # Windows holds the file open otherwise, blocking os.unlink later
    return path


def out_paths():
    d = tempfile.mkdtemp()
    return os.path.join(d, "library.db"), os.path.join(d, "listener.db")


def test_rehearsal_writes_neither_output_file():
    src = source_db()
    library_path, listener_path = out_paths()
    # main() reads argv-style args via argparse, called directly with sys.argv
    # patched -- the same shape the tool's own CLI takes, not a private API.
    old_argv = sys.argv
    try:
        sys.argv = ["split_database.py", src, "--library-out", library_path, "--listener-out", listener_path]
        rc = sd.main()
        check(rc == 0, "rehearsal should exit 0")
        check(not os.path.exists(library_path), "rehearsal must not write library.db")
        check(not os.path.exists(listener_path), "rehearsal must not write listener.db")
    finally:
        sys.argv = old_argv
        os.unlink(src)


def test_commit_produces_two_correct_files_and_leaves_the_source_alone():
    src = source_db()
    library_path, listener_path = out_paths()
    old_argv = sys.argv
    try:
        sys.argv = ["split_database.py", src, "--library-out", library_path,
                    "--listener-out", listener_path, "--commit"]
        rc = sd.main()
        check(rc == 0, "commit should exit 0")
        check(os.path.exists(library_path), "commit must write library.db")
        check(os.path.exists(listener_path), "commit must write listener.db")

        lib = sqlite3.connect(library_path)
        check(lib.execute("SELECT title FROM recordings WHERE mbid='mb-1'").fetchone() == ("A Song",),
              "library.db must have the catalog data")
        check(
            lib.execute(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='passages_span'"
            ).fetchone()[0] == 1,
            "library.db must carry the passages_span UNIQUE index, not just the table",
        )
        check(
            lib.execute("SELECT value FROM schema_meta WHERE key='schema_version'").fetchone() == ("1",),
            "schema_meta must be copied to library.db too (a judgment call, not an omission)",
        )
        check(
            lib.execute(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='listener_play_history'"
            ).fetchone()[0] == 0,
            "library.db must NOT contain a listener-side table",
        )

        listener = sqlite3.connect(listener_path)
        check(
            listener.execute("SELECT mbid, played_at FROM listener_play_history").fetchone() == ("mb-1", 1700000000),
            "listener.db must have the listener data",
        )
        check(
            listener.execute(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='listener_play_mbid'"
            ).fetchone()[0] == 1,
            "listener.db must carry its own index too",
        )
        check(
            listener.execute("SELECT value FROM schema_meta WHERE key='schema_version'").fetchone() == ("1",),
            "schema_meta must be copied to listener.db as well",
        )
        check(
            listener.execute(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='recordings'"
            ).fetchone()[0] == 0,
            "listener.db must NOT contain a catalog-side table",
        )

        source_untouched = sqlite3.connect(src)
        check(
            source_untouched.execute("SELECT COUNT(*) FROM recordings").fetchone()[0] == 1,
            "the source file must be completely unmodified after --commit",
        )
        lib.close()
        listener.close()
        source_untouched.close()
    finally:
        sys.argv = old_argv
        os.unlink(src)


def test_refuses_to_overwrite_an_existing_output_file():
    src = source_db()
    library_path, listener_path = out_paths()
    with open(library_path, "w") as f:
        f.write("not a database, just occupying the path")
    old_argv = sys.argv
    try:
        sys.argv = ["split_database.py", src, "--library-out", library_path,
                    "--listener-out", listener_path, "--commit"]
        rc = sd.main()
        check(rc != 0, "must refuse rather than overwrite an existing output path")
    finally:
        sys.argv = old_argv
        os.unlink(src)


def test_a_missing_optional_table_is_skipped_not_an_error():
    # No `flavor` table at all in this source -- an older or leaner library.
    fd, src = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    conn = sqlite3.connect(src)
    conn.executescript(SCHEMA)  # SCHEMA already lacks `flavor`
    conn.close()
    library_path, listener_path = out_paths()
    old_argv = sys.argv
    try:
        sys.argv = ["split_database.py", src, "--library-out", library_path,
                    "--listener-out", listener_path, "--commit"]
        rc = sd.main()
        check(rc == 0, "a source missing an optional table must not fail the split")
    finally:
        sys.argv = old_argv
        os.unlink(src)


def main() -> int:
    test_rehearsal_writes_neither_output_file()
    test_commit_produces_two_correct_files_and_leaves_the_source_alone()
    test_refuses_to_overwrite_an_existing_output_file()
    test_a_missing_optional_table_is_skipped_not_an_error()

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("split_database: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

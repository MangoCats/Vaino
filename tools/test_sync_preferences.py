#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `sync_preferences.py`'s decision logic `[SPEC030]`.

`decide()` and `patch_sql_for()` are pure -- no database, no ssh call --
which is exactly what's checked here, the same discipline
`ingest_cd.py`/`segment_dao.py`'s own tests already use. `read_local_manifest`/
`apply_local` get one small in-memory-db round trip each, since that part
is genuinely I/O.

    python tools/test_sync_preferences.py
"""

import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import sync_preferences as sp  # noqa: E402

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL: {msg}")


def always(_kind, _id):
    return True


def never(_kind, _id):
    return False


def main() -> int:
    print("decide: remote newer -> pull")
    local = {("recording", "r1"): (1.0, 2.0, 0.0, "2026-09-01T00:00:00")}
    remote = {("recording", "r1"): (1.5, 2.0, 0.0, "2026-09-02T00:00:00")}
    plan = sp.decide(local, remote, always, always)
    check(plan == {"pull": [("recording", "r1")], "push": [], "skip_missing": [], "tie": []},
          f"got {plan}")

    print()
    print("decide: local newer -> push")
    local = {("artist", "a1"): (1.0, 2.0, 0.0, "2026-09-03T00:00:00")}
    remote = {("artist", "a1"): (0.5, 2.0, 0.0, "2026-09-01T00:00:00")}
    plan = sp.decide(local, remote, always, always)
    check(plan["push"] == [("artist", "a1")], f"got {plan}")

    print()
    print("decide: identical rows on both sides -> neither list")
    row = (1.0, 2.0, 0.0, "2026-09-01T00:00:00")
    plan = sp.decide({("recording", "r1"): row}, {("recording", "r1"): row}, always, always)
    check(plan == {"pull": [], "push": [], "skip_missing": [], "tie": []}, f"got {plan}")

    print()
    print("decide: equal timestamp, differing values -> tie, not guessed at")
    local = {("recording", "r1"): (1.0, 2.0, 0.0, "2026-09-01T00:00:00")}
    remote = {("recording", "r1"): (1.5, 2.0, 0.0, "2026-09-01T00:00:00")}
    plan = sp.decide(local, remote, always, always)
    check(plan["tie"] == [("recording", "r1")], f"got {plan}")
    check(not plan["pull"] and not plan["push"], "a tie must not also be pulled or pushed")

    print()
    print("decide: tuned locally only, remote HAS the artist/recording -> push")
    local = {("artist", "a1"): (1.0, None, None, "2026-09-01T00:00:00")}
    plan = sp.decide(local, {}, always, always)
    check(plan["push"] == [("artist", "a1")], f"got {plan}")

    print()
    print("decide: tuned locally only, remote does NOT have that artist -> skip_missing")
    local = {("artist", "a1"): (1.0, None, None, "2026-09-01T00:00:00")}
    plan = sp.decide(local, {}, always, never)
    check(plan["skip_missing"] == [("artist", "a1")], f"got {plan}")
    check(not plan["push"], "must not push onto a library that lacks the artist")

    print()
    print("decide: tuned remotely only, local does NOT have that recording -> skip_missing")
    remote = {("recording", "r9"): (None, 2.0, -0.5, "2026-09-01T00:00:00")}
    plan = sp.decide({}, remote, never, always)
    check(plan["skip_missing"] == [("recording", "r9")], f"got {plan}")
    check(not plan["pull"], "must not pull onto a library that lacks the recording")

    print()
    print("decide: NULL fields are preserved through the comparison, not treated as 0")
    local = {("recording", "r1"): (None, None, None, "2026-09-01T00:00:00")}
    remote = {("recording", "r1"): (None, None, None, "2026-09-01T00:00:00")}
    plan = sp.decide(local, remote, always, always)
    check(plan == {"pull": [], "push": [], "skip_missing": [], "tie": []},
          f"two NULL rows with the same timestamp must read as identical, got {plan}")

    print()
    print("patch_sql_for: one INSERT OR REPLACE per row, literal-quoted, NULLs kept as NULL")
    source = {("recording", "r1"): (1.5, None, -0.939, "2026-09-01T00:00:00")}
    sql = sp.patch_sql_for([("recording", "r1")], source)
    check("INSERT OR REPLACE INTO listener_preferences" in sql, f"got {sql!r}")
    check("'recording'" in sql and "'r1'" in sql, f"got {sql!r}")
    check("1.5" in sql, f"got {sql!r}")
    check("NULL" in sql, f"a None field must render as literal NULL, got {sql!r}")
    check("-0.939" in sql, f"got {sql!r}")
    check(sql.strip().startswith("BEGIN IMMEDIATE;") and sql.strip().endswith("COMMIT;"),
          f"got {sql!r}")

    # A value containing a quote must not break the statement -- the same
    # escaping discipline `remote_peek.literal` already tests for elsewhere.
    tricky = {("artist", "o'brien"): (1.0, None, None, "t")}
    sql2 = sp.patch_sql_for([("artist", "o'brien")], tricky)
    check("o''brien" in sql2, f"a quote in an id must be doubled, got {sql2!r}")

    print()
    print("read_local_manifest / apply_local: a real round trip against an in-memory db")
    conn = sqlite3.connect(":memory:")
    conn.execute(
        "CREATE TABLE listener_preferences (subject_kind TEXT, subject_id TEXT, "
        "rotation REAL, recovery REAL, restraint REAL, updated_at TEXT, "
        "PRIMARY KEY (subject_kind, subject_id))")
    conn.execute(
        "INSERT INTO listener_preferences VALUES ('recording','r1',1.0,2.0,0.0,'2026-09-01T00:00:00')")
    manifest = sp.read_local_manifest(conn)
    check(manifest == {("recording", "r1"): (1.0, 2.0, 0.0, "2026-09-01T00:00:00")},
          f"got {manifest}")

    # apply_local: an INSERT for a subject not yet present, an UPDATE for one that is.
    incoming = {
        ("recording", "r1"): (1.5, 2.0, 0.0, "2026-09-02T00:00:00"),
        ("artist", "a1"): (None, 1.0, -0.5, "2026-09-02T00:00:00"),
    }
    sp.apply_local(conn, [("recording", "r1"), ("artist", "a1")], incoming)
    after = sp.read_local_manifest(conn)
    check(after[("recording", "r1")] == (1.5, 2.0, 0.0, "2026-09-02T00:00:00"),
          f"existing row must be overwritten, got {after}")
    check(after[("artist", "a1")] == (None, 1.0, -0.5, "2026-09-02T00:00:00"),
          f"a new subject must be inserted, got {after}")
    conn.close()

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("sync_preferences: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

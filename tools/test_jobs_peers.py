#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `jobs.py`'s peer registry `[SPEC-MESH-090..094]`.

Plain sqlite against the real sidecar schema -- nothing here shells out, so
nothing is faked.

    python tools/test_jobs_peers.py
"""

import os
import sqlite3
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import jobs as jobmod  # noqa: E402

SCHEMA = "CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT);"

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL  {msg}")


def _runner(tmp: str) -> "jobmod.Runner":
    db = os.path.join(tmp, "lib.db")
    c = sqlite3.connect(db)
    c.executescript(SCHEMA)
    c.commit()
    c.close()
    return jobmod.Runner(db, os.path.join(tmp, "lib.console.db"))


def test_add_list_and_remove(tmp: str) -> None:
    runner = _runner(tmp)
    check(runner.list_peers() == [], "a fresh sidecar has no peers")

    runner.upsert_peer("bose", "pi@bose:/var/vaino/vaino.db")
    runner.upsert_peer("vainopi", "pi@vainopi:/srv/library/vaino.db")
    peers = runner.list_peers()
    check(len(peers) == 2, f"got {peers}")
    check(peers[0]["name"] == "bose", f"expected alphabetical order, got {peers}")
    check(peers[0]["enabled"] == 1, f"got {peers[0]}")

    runner.delete_peer("bose")
    peers = runner.list_peers()
    check([p["name"] for p in peers] == ["vainopi"], f"got {peers}")


def test_upsert_overwrites_by_name(tmp: str) -> None:
    runner = _runner(tmp)
    runner.upsert_peer("bose", "pi@bose:/var/vaino/vaino.db")
    runner.upsert_peer("bose", "pi@bose:/var/vaino/vaino-new.db")
    peers = runner.list_peers()
    check(len(peers) == 1, f"a second upsert of the same name must not duplicate: {peers}")
    check(peers[0]["remote"] == "pi@bose:/var/vaino/vaino-new.db", f"got {peers}")


def test_activate_peer_sets_remote_config(tmp: str) -> None:
    runner = _runner(tmp)
    check(runner.get_remote() is None, "nothing configured yet")
    runner.upsert_peer("bose", "pi@bose:/var/vaino/vaino.db")

    remote = runner.activate_peer("bose")
    check(remote == "pi@bose:/var/vaino/vaino.db", f"got {remote}")
    check(runner.get_remote() == "pi@bose:/var/vaino/vaino.db",
          "[SPEC-MESH-092]: activating a peer must set remote_config, so the "
          "three existing sync jobs act on it unchanged")

    check(runner.activate_peer("no-such-peer") is None, "an unknown name must not silently succeed")
    check(runner.get_remote() == "pi@bose:/var/vaino/vaino.db",
          "a failed activation must not clear the previously active remote")


def main() -> int:
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp:
        test_add_list_and_remove(tmp)
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp:
        test_upsert_overwrites_by_name(tmp)
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp:
        test_activate_peer_sets_remote_config(tmp)

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("jobs peers: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

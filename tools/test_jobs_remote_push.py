#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for `jobs.py`'s `_remote_push` wiring `[SPEC-DF-120]`.

Runs the real `Runner` and its real background worker thread. `export` and
`snapshot` are faked -- the former would otherwise read the real library
(fine, but not the point of this test), the latter would otherwise shell out
to `ssh`; `send`/`apply-remote` are faked the same way `test_jobs_remote_pull
.py` already fakes the pull side's own ssh-touching stage. `compare`
(`apply_changes.py`) runs for real, against a real tiny snapshot db a fake
`remote_snapshot.py` stage produced -- what is under test here is
specifically that `remote_snapshot.py` replaced the old `scp` full-copy
`fetch` stage without changing anything about the stages around it.

    python tools/test_jobs_remote_push.py
"""

import json
import os
import sqlite3
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import apply_changes  # noqa: E402
import jobs as jobmod  # noqa: E402

SCHEMA = """
CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT NOT NULL,
    length_ms INTEGER, source TEXT NOT NULL);
CREATE TABLE listener_flags (subject_kind TEXT NOT NULL, subject_id TEXT NOT NULL,
    flagged_at TEXT NOT NULL, origin TEXT, PRIMARY KEY (subject_kind, subject_id)) WITHOUT ROWID;
CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT);
CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, kind TEXT);
CREATE TABLE passage_recordings (passage_id INTEGER, mbid TEXT);
CREATE TABLE flavor (subject_kind TEXT, subject_id TEXT);
CREATE TABLE id_checks (passage_id INTEGER);
"""

MD5_A = "a" * 32
REC_TARGET = "aaaaaaaa-0000-0000-0000-000000000002"

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL  {msg}")
    return cond


def build_library(path: str) -> None:
    c = sqlite3.connect(path)
    c.executescript(SCHEMA)
    c.commit()
    c.close()


def build_snapshot(path: str) -> None:
    """What a real `remote_snapshot.py` run would have produced for one
    resolvable `id_review` change -- a passage the remote already has, ready
    to fast-forward.

    `jobs.py`'s own `work` directory is keyed by `job_id`, which restarts
    from 1 for every fresh `Runner` -- the same path can be reused across
    this file's own two tests (and across repeat runs of this file), so a
    stale file from a previous run has to be cleared first, the same guard
    `remote_snapshot.py`'s own real `build()` already takes.
    """
    if os.path.exists(path):
        os.remove(path)
    c = sqlite3.connect(path)
    c.executescript("""
        CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT NOT NULL UNIQUE);
        CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER, kind TEXT,
            start_ms INTEGER, end_ms INTEGER);
        CREATE TABLE passage_recordings (passage_id INTEGER, mbid TEXT, weight REAL, source TEXT);
        CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT, length_ms INTEGER, source TEXT);
    """)
    c.execute(f"INSERT INTO files VALUES (1, '{MD5_A}')")
    c.execute("INSERT INTO passages VALUES (100, 1, 'radio', 0, 200000)")
    apply_changes.ensure_review_tables(c)
    c.commit()
    c.close()


CHANGES_DOC = {"format_version": 1, "changes": [{
    "kind": "id_review",
    "anchor": {"audio_md5": MD5_A, "passage_kind": "radio", "start_ms": 0, "end_ms": 200000},
    "baseline": {"mbid": None},
    "target": {"mbid": REC_TARGET, "title": "A Song", "artists": []},
    "decided_at": "2026-08-31T00:00:00", "origin": "desktop",
}]}


def wait_for(runner, job_id, timeout=10.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        j = runner.job(job_id)
        if j and j["state"] not in ("queued", "running"):
            return j
        time.sleep(0.05)
    raise TimeoutError(f"job {job_id} did not finish within {timeout}s")


def fake_spawn_success(changes_doc, captured_argv=None):
    def _spawn(self, job_id, stage, argv):
        if stage == "export":
            check("export_changes.py" in argv[1], f"export must run export_changes.py, got {argv}")
            out_path = argv[argv.index("-o") + 1]
            with open(out_path, "w", encoding="utf-8") as f:
                json.dump(changes_doc, f)
            return 0, "1 change(s) exported"
        if stage == "snapshot":
            check("remote_snapshot.py" in argv[1], f"snapshot must run remote_snapshot.py, got {argv}")
            out_path = argv[argv.index("-o") + 1]
            build_snapshot(out_path)
            return 0, json.dumps({"ok": True, "changes": 1, "resolved": 1, "out": out_path})
        if stage in ("send", "apply-remote"):
            if captured_argv is not None:
                captured_argv[stage] = argv
            return 0, ""
        return jobmod.Runner._spawn(self, job_id, stage, argv)
    return _spawn


def fake_spawn_snapshot_unreachable(self, job_id, stage, argv):
    if stage == "export":
        out_path = argv[argv.index("-o") + 1]
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(CHANGES_DOC, f)
        return 0, "1 change(s) exported"
    if stage == "snapshot":
        return 1, "could not reach pi@vainopi:/srv/library/vaino.db: no route to host"
    raise AssertionError(f"stage {stage!r} must never run after snapshot fails")


def test_push_lands_a_change(tmp: str) -> None:
    print("export -> snapshot -> compare -> send -> apply-remote, no full-copy fetch anywhere, "
          "and the change actually lands")
    library = os.path.join(tmp, "library.db")
    build_library(library)
    sidecar = os.path.join(tmp, "library.console.db")
    runner = jobmod.Runner(library, sidecar)
    captured = {}
    runner._spawn = fake_spawn_success(CHANGES_DOC, captured).__get__(runner, jobmod.Runner)
    job_id = runner.submit("remote-push", "pi@vainopi:/srv/library/vaino.db")
    j = wait_for(runner, job_id)
    check(j["state"] == "done", f"expected done, got {j}")
    stages = [e["stage"] for e in j["events"] if e["kind"] == "stage"]
    check(stages == ["export", "snapshot", "compare", "send", "apply-remote"],
          f"expected exactly these five stages in this order, got {stages}")
    check("scp" not in json.dumps(j["events"]),
          "no scp full-copy step should appear anywhere in this job's log")

    # `[SPEC-DF-121]` A real, previously-uncaught bug: a bare `systemctl` as
    # the unprivileged deploy user fails outright with "Interactive
    # authentication required" -- found live against a real vainopi, not by
    # any test, because this stage was faked wholesale above (and everywhere
    # else this job is tested) without ever inspecting the argv it built.
    apply_argv = captured.get("apply-remote")
    check(apply_argv is not None, "apply-remote must actually run for a change that lands")
    if apply_argv is not None:
        remote_cmd = apply_argv[-1]
        check("sudo systemctl stop vaino" in remote_cmd,
              f"stop must run with sudo -- a bare systemctl needs a password "
              f"non-interactively, got: {remote_cmd!r}")
        check("sudo systemctl start vaino" in remote_cmd,
              f"start must run with sudo too, got: {remote_cmd!r}")
    check(j["result"].get("landed") is True, f"the id_review must have landed, got {j['result']}")
    check(j["result"].get("fastforward") == 1, f"expected 1 fast-forward, got {j['result']}")
    logs = [e["text"] for e in j["events"] if e["kind"] == "log"]
    check(any("1 change(s) to push" in t for t in logs),
          f"a one-sentence summary must say what is about to be pushed, got {logs}")
    check(any("vainopi now has these changes" in t for t in logs),
          f"a final confirmation must say the push actually landed, got {logs}")


def test_push_nothing_pending(tmp: str) -> None:
    print("no pending edits at all: a plain-English 'nothing to sync' line, not just raw JSON")
    library = os.path.join(tmp, "library3.db")
    build_library(library)
    sidecar = os.path.join(tmp, "library3.console.db")
    runner = jobmod.Runner(library, sidecar)
    empty = {"format_version": 1, "changes": []}
    runner._spawn = fake_spawn_success(empty).__get__(runner, jobmod.Runner)
    job_id = runner.submit("remote-push", "pi@vainopi:/srv/library/vaino.db")
    j = wait_for(runner, job_id)
    check(j["state"] == "done", f"expected done, got {j}")
    stages = [e["stage"] for e in j["events"] if e["kind"] == "stage"]
    check(stages == ["export", "snapshot", "compare"],
          f"send/apply-remote must not run when there is nothing to push, got {stages}")
    logs = [e["text"] for e in j["events"] if e["kind"] == "log"]
    check(any("nothing to sync" in t for t in logs),
          f"a plain-English 'nothing to sync' line must appear, got {logs}")
    check(any(t == "the remote was not touched." for t in logs),
          f"must say plainly that vainopi was never touched, got {logs}")


def test_snapshot_unreachable_fails_before_compare(tmp: str) -> None:
    print("snapshot failing (an unreachable remote) fails the job before compare/send/apply-remote ever run")
    library = os.path.join(tmp, "library2.db")
    build_library(library)
    sidecar = os.path.join(tmp, "library2.console.db")
    runner = jobmod.Runner(library, sidecar)
    runner._spawn = fake_spawn_snapshot_unreachable.__get__(runner, jobmod.Runner)
    job_id = runner.submit("remote-push", "pi@vainopi:/srv/library/vaino.db")
    j = wait_for(runner, job_id)
    check(j["state"] == "failed", f"expected failed, got {j}")
    stages = [e["stage"] for e in j["events"] if e["kind"] == "stage"]
    check(stages == ["export", "snapshot"],
          f"compare/send/apply-remote must never run once snapshot failed, got {stages}")


def main() -> int:
    with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as tmp:
        test_push_lands_a_change(tmp)
        test_push_nothing_pending(tmp)
        test_snapshot_unreachable_fails_before_compare(tmp)

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("jobs remote_push: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

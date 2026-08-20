#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Jobs for the Sampo console `[SPEC-SUI-080]`, `[SPEC-SUI-085]`.

The job model is the real work of stage 3; induct is a thin caller
`[IMPL-SUI-050]`.

**The console still never writes the library.** Jobs run the same CLIs a person
runs by hand `[SPEC-SUI-015]`, as subprocesses; those write, as they always
have. Job bookkeeping goes to a sidecar beside the library. So the console's own
connection stays `mode=ro` through stage 3, and "it cannot damage the library"
remains structural rather than becoming a promise the moment writes appear.

The sidecar is also the answer to `[SPEC-SUI-085]`'s requirement that job state
survive the browser: it is a file, so a reload, a closed tab or a restarted
console all find the run still there. Putting it in `vaino.db` instead would add
tables Vaino never reads `[SPEC-SC-015]` and would take the write lock for
bookkeeping -- contending with the player `[SPEC-SUI-082]` to record that
nothing had happened.

Progress is **committed** work, never predicted work. Counts come from querying
the library for what the stage has actually landed, so the bar cannot claim what
the database has not accepted.
"""

import json
import os
import queue
import sqlite3
import subprocess
import sys
import threading
import time

DDL = """
CREATE TABLE IF NOT EXISTS jobs (
    job_id     INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL,          -- 'propose' | 'induct'
    target     TEXT NOT NULL,          -- the folder
    state      TEXT NOT NULL,          -- queued|running|done|failed|stopped
    plan       TEXT,                   -- the proposal, as returned by --json
    result     TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    ended_at   TEXT
);
CREATE TABLE IF NOT EXISTS job_events (
    event_id INTEGER PRIMARY KEY,
    job_id   INTEGER NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    at       TEXT NOT NULL,
    stage    TEXT,
    kind     TEXT NOT NULL,            -- stage|log|counts|error|done
    text     TEXT
);
CREATE INDEX IF NOT EXISTS job_events_job ON job_events(job_id, event_id);
"""

# Stage 0 ran these by hand in this order and the transcript is the reference
# `[IMPL-SUI-020]`. Segmentation is absent because it is for DAO captures and
# wrong for single-track files `[SPEC-SA-070]`; release and cover-art fetches
# are absent because they need a real MBID, which self-published audio does not
# get `[SPEC-SUI-075]`. The plan says which it skips and why, rather than
# quietly running seven things.
def steps_for(db: str, folder: str) -> list:
    tools = os.path.dirname(os.path.abspath(__file__))
    return [
        ("ingest", [sys.executable, os.path.join(tools, "ingest_folder.py"),
                    db, folder, "--commit", "--json"]),
        ("extract", [sys.executable, os.path.join(tools, "extract_library.py"), db]),
        ("identify", [sys.executable, os.path.join(tools, "fingerprint_ids.py"), db]),
        ("merge", [sys.executable, os.path.join(tools, "fingerprint_ids.py"), db, "--merge"]),
    ]


SKIPPED = [
    ("segment", "for DAO captures; these are single-track files [SPEC-SA-070]"),
    ("releases", "needs a MusicBrainz id, which ingest does not invent"),
    ("cover art", "needs a release id; art beside the file is found by the player"),
]


def counts(db: str) -> dict:
    """What the library actually holds, right now. Read-only, and cheap.

    Every `flavor` lookup names `subject_kind` -- the prefix column of both the
    key and the index. Omitting it scans 578,452 rows per passage
    `[IMPL-SUI-045]`.
    """
    c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        q = lambda s: c.execute(s).fetchone()[0]  # noqa: E731
        return {
            "files": q("SELECT count(*) FROM files"),
            "radio": q("SELECT count(*) FROM passages WHERE kind='radio'"),
            "flavor": q("SELECT count(*) FROM passages p JOIN passage_recordings pr "
                        "USING(passage_id) WHERE p.kind='radio' AND EXISTS (SELECT 1 FROM flavor f "
                        "WHERE f.subject_kind='recording' AND f.subject_id=pr.mbid)"),
            "checked": q("SELECT count(*) FROM id_checks"),
        }
    finally:
        c.close()


class Runner:
    """One job at a time, and it says so `[SPEC-SUI-080]`.

    A second job would contend for the library's write lock with the first and
    with the player, and surface as an unexplained stall. Requests made while
    one runs are queued, and shown as queued.
    """

    def __init__(self, library: str, sidecar: str):
        self.library = library
        self.sidecar = sidecar
        self.q = queue.Queue()
        self.lock = threading.Lock()
        self.current = None          # job_id, while one runs
        self.proc = None             # the live subprocess, so it can be stopped
        db = self._db()
        db.executescript(DDL)
        # A console killed mid-job leaves a row saying `running` and no process.
        # Say what happened rather than letting it look live for ever.
        db.execute("UPDATE jobs SET state='stopped', ended_at=?1 "
                   "WHERE state IN ('running','queued')", (now(),))
        db.commit()
        db.close()
        threading.Thread(target=self._work, daemon=True).start()

    def _db(self):
        db = sqlite3.connect(self.sidecar, timeout=30, check_same_thread=False)
        db.row_factory = sqlite3.Row
        db.execute("PRAGMA busy_timeout = 30000")
        return db

    # -- public ------------------------------------------------------------

    def submit(self, kind: str, target: str) -> int:
        db = self._db()
        cur = db.execute("INSERT INTO jobs (kind,target,state,created_at) VALUES (?1,?2,'queued',?3)",
                         (kind, target, now()))
        job_id = cur.lastrowid
        db.commit()
        db.close()
        self.q.put(job_id)
        return job_id

    def stop(self, job_id: int) -> bool:
        """Interrupt without loss `[REQ-LIB-130]`.

        The stage is killed between items, so the transaction it was inside is
        rolled back by SQLite and the rows it had already committed stand. At
        most the in-flight item is lost `[SPEC-SA-028]`.
        """
        with self.lock:
            if self.current == job_id and self.proc and self.proc.poll() is None:
                self.proc.terminate()
                return True
        db = self._db()
        n = db.execute("UPDATE jobs SET state='stopped', ended_at=?1 "
                       "WHERE job_id=?2 AND state='queued'", (now(), job_id)).rowcount
        db.commit()
        db.close()
        return bool(n)

    def job(self, job_id: int) -> dict:
        db = self._db()
        r = db.execute("SELECT * FROM jobs WHERE job_id=?1", (job_id,)).fetchone()
        ev = db.execute("SELECT * FROM job_events WHERE job_id=?1 ORDER BY event_id",
                        (job_id,)).fetchall()
        db.close()
        if r is None:
            return {}
        d = dict(r)
        d["plan"] = json.loads(d["plan"]) if d["plan"] else None
        d["result"] = json.loads(d["result"]) if d["result"] else None
        d["events"] = [dict(e) for e in ev]
        return d

    def recent(self, limit: int = 25) -> list:
        db = self._db()
        rows = db.execute("SELECT job_id,kind,target,state,created_at,started_at,ended_at "
                          "FROM jobs ORDER BY job_id DESC LIMIT ?1", (limit,)).fetchall()
        db.close()
        return [dict(r) for r in rows]

    def events_since(self, job_id: int, after: int) -> list:
        db = self._db()
        rows = db.execute("SELECT * FROM job_events WHERE job_id=?1 AND event_id>?2 "
                          "ORDER BY event_id", (job_id, after)).fetchall()
        db.close()
        return [dict(r) for r in rows]

    # -- worker ------------------------------------------------------------

    def _emit(self, job_id, kind, text=None, stage=None):
        db = self._db()
        db.execute("INSERT INTO job_events (job_id,at,stage,kind,text) VALUES (?1,?2,?3,?4,?5)",
                   (job_id, now(), stage, kind, text))
        db.commit()
        db.close()

    def _work(self):
        while True:
            job_id = self.q.get()
            db = self._db()
            row = db.execute("SELECT * FROM jobs WHERE job_id=?1", (job_id,)).fetchone()
            db.close()
            if row is None or row["state"] != "queued":
                continue          # stopped before it ever started
            with self.lock:
                self.current = job_id
            try:
                self._run(job_id, row["kind"], row["target"])
            except Exception as e:
                self._emit(job_id, "error", f"{type(e).__name__}: {e}")
                self._finish(job_id, "failed")
            finally:
                with self.lock:
                    self.current = None
                    self.proc = None

    def _run(self, job_id: int, kind: str, target: str):
        db = self._db()
        db.execute("UPDATE jobs SET state='running', started_at=?1 WHERE job_id=?2",
                   (now(), job_id))
        db.commit()
        db.close()
        self._emit(job_id, "counts", json.dumps(counts(self.library)))

        if kind == "propose":
            # Propose is the real dry run, not an estimate: the tools already
            # refuse to write without --commit, so what is confirmed is what
            # was read `[SPEC-SUI-070]`.
            tools = os.path.dirname(os.path.abspath(__file__))
            code, out = self._spawn(job_id, "propose", [
                sys.executable, os.path.join(tools, "ingest_folder.py"),
                self.library, target, "--json"])
            plan = parse_json_tail(out)
            if plan is not None:
                plan["skipped"] = [{"stage": s, "why": w} for s, w in SKIPPED]
            db = self._db()
            db.execute("UPDATE jobs SET plan=?1 WHERE job_id=?2",
                       (json.dumps(plan) if plan else None, job_id))
            db.commit()
            db.close()
            return self._finish(job_id, "done" if code == 0 else "failed")

        result = {}
        for stage, argv in steps_for(self.library, target):
            if self._stopped(job_id):
                return self._finish(job_id, "stopped")
            self._emit(job_id, "stage", stage, stage=stage)
            code, out = self._spawn(job_id, stage, argv)
            if stage == "ingest":
                result["ingest"] = parse_json_tail(out)
            self._emit(job_id, "counts", json.dumps(counts(self.library)), stage=stage)
            if code != 0:
                self._emit(job_id, "error", f"{stage} exited {code}", stage=stage)
                db = self._db()
                db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2",
                           (json.dumps(result), job_id))
                db.commit()
                db.close()
                return self._finish(job_id, "stopped" if code < 0 else "failed")
        db = self._db()
        db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
        db.commit()
        db.close()
        self._finish(job_id, "done")

    def _spawn(self, job_id, stage, argv):
        # UTF-8 on both sides. `ingest_folder.say()` falls back to the console
        # encoding and renders a smart-quoted title as `?Duala?`; that is a
        # rendering artefact of a terminal, and a job is not one
        # `[IMPL-SUI-025]`.
        env = dict(os.environ, PYTHONIOENCODING="utf-8", PYTHONUNBUFFERED="1")
        p = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, encoding="utf-8", errors="replace",
                             env=env, cwd=os.path.dirname(os.path.dirname(
                                 os.path.abspath(__file__))))
        with self.lock:
            self.proc = p
        lines = []
        for line in p.stdout:
            line = line.rstrip("\n")
            lines.append(line)
            if line.strip():
                self._emit(job_id, "log", line, stage=stage)
        p.wait()
        return p.returncode, "\n".join(lines)

    def _stopped(self, job_id) -> bool:
        db = self._db()
        r = db.execute("SELECT state FROM jobs WHERE job_id=?1", (job_id,)).fetchone()
        db.close()
        return r is not None and r["state"] == "stopped"

    def _finish(self, job_id, state):
        db = self._db()
        db.execute("UPDATE jobs SET state=?1, ended_at=?2 WHERE job_id=?3",
                   (state, now(), job_id))
        db.commit()
        db.close()
        self._emit(job_id, "done", state)


def parse_json_tail(out: str):
    """The last line that parses as a JSON object.

    `--json` prints one object and nothing else, but a warning on stderr is
    folded into the same stream, so the object is found rather than assumed to
    be the whole of it.
    """
    for line in reversed(out.splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


def now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S")

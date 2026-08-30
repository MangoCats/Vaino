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
    kind       TEXT NOT NULL,          -- 'propose' | 'induct' | 'reanalyze'
                                        -- | 'export-bundle' | 'remote-pull'
                                        -- | 'remote-push' | 'accept-remote'
                                        -- | 'suggest-release' | 'accept-release'
    target     TEXT NOT NULL,          -- the folder, or a remote's user@host:/path
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
-- One remembered remote per library, console-owned bookkeeping like the rest
-- of this sidecar -- never a table Vaino itself reads [SPEC-SC-015]. A single
-- row, key='sync_remote', value='user@host:/path/to/vaino.db' -- the exact
-- form `scp`/`ssh` already want, so nothing here parses or validates it
-- beyond what the remote-pull/remote-push jobs need to split off a host.
CREATE TABLE IF NOT EXISTS remote_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"""

# Stage 0 ran these by hand in this order and the transcript is the reference
# `[IMPL-SUI-020]`. Segmentation is absent because it is for DAO captures and
# wrong for single-track files `[SPEC-SA-070]`; release and cover-art fetches
# are absent because they need a real MBID, which self-published audio does not
# get `[SPEC-SUI-075]`. The plan says which it skips and why, rather than
# quietly running seven things.
def steps_for(db: str, folder: str, recheck: bool = False) -> list:
    """`recheck=True` is the one thing plain `induct` cannot do `[IMPL-SUI-020]`
    reused for `[SPEC-SUI-214]`'s `reanalyze` job: `fingerprint_ids.py` already
    skips any passage already in `id_checks`, `unmatched` included, so a
    second `induct` over an already-ingested folder can never retry one --
    `--recheck` is the tool's own existing flag for exactly that, simply never
    wired to a caller before now.
    """
    tools = os.path.dirname(os.path.abspath(__file__))
    identify = [sys.executable, os.path.join(tools, "fingerprint_ids.py"), db]
    if recheck:
        identify.append("--recheck")
    return [
        ("ingest", [sys.executable, os.path.join(tools, "ingest_folder.py"),
                    db, folder, "--commit", "--json"]),
        ("extract", [sys.executable, os.path.join(tools, "extract_library.py"), db]),
        ("identify", identify),
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

    def __init__(self, library: str, sidecar: str, roots: list | None = None):
        self.library = library
        self.roots = roots or []
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

    def get_remote(self) -> str | None:
        db = self._db()
        r = db.execute("SELECT value FROM remote_config WHERE key='sync_remote'").fetchone()
        db.close()
        return r["value"] if r else None

    def set_remote(self, value: str) -> None:
        db = self._db()
        db.execute("INSERT INTO remote_config (key, value) VALUES ('sync_remote', ?1) "
                   "ON CONFLICT(key) DO UPDATE SET value=excluded.value", (value,))
        db.commit()
        db.close()

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

        if kind == "export-bundle":
            # A GUI over `export_bundle.py` `[IMPL007 Stage 4]`, one job, no
            # multi-stage loop -- the tool is already one atomic operation.
            # `target` carries the SQL LIKE pattern the console page built
            # from what was typed; the output directory is deterministic so
            # the page can offer it back without the tool needing to report it.
            tools = os.path.dirname(os.path.abspath(__file__))
            out_dir = os.path.join(os.path.dirname(tools), "out", f"bundle-{job_id}")
            argv = [sys.executable, os.path.join(tools, "export_bundle.py"),
                    self.library, "--like", target, "--gzip", "-o", out_dir]
            for root in self.roots:
                argv += ["--root", root]
            code, _ = self._spawn(job_id, "export", argv)
            db = self._db()
            db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2",
                       (json.dumps({"out_dir": out_dir}), job_id))
            db.commit()
            db.close()
            return self._finish(job_id, "done" if code == 0 else "failed")

        if kind == "remote-pull":
            return self._remote_pull(job_id, target)

        if kind == "remote-push":
            return self._remote_push(job_id, target)

        if kind == "accept-remote":
            return self._accept_remote(job_id, target)

        if kind == "suggest-release":
            return self._suggest_release(job_id, target)

        if kind == "accept-release":
            return self._accept_release(job_id, target)

        # 'induct' and 'reanalyze' `[SPEC-SUI-214]` are the same four-stage
        # pipeline, differing only in whether `identify` is told to retry
        # what it already tried -- anything else unrecognized also lands
        # here, matching this method's own long-standing fallthrough.
        return self._run_pipeline(job_id, target, recheck=(kind == "reanalyze"))

    def _run_pipeline(self, job_id: int, target: str, recheck: bool):
        result = {}
        for stage, argv in steps_for(self.library, target, recheck=recheck):
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

    def _remote_pull(self, job_id: int, target: str):
        """A GUI over `remote_flags.py`/`import_flags.py` `[SPEC-DF-119]` --
        `target` is `user@host:/path/to/vaino.db`. No `scp`, no database
        copy: `listener_flags` is one small table, fetched over one `ssh
        ... sqlite3 -json ...` round trip, the same targeted-read mechanism
        `[SPEC-DF-116]` already gave a single review anchor -- what
        `[SPEC-DF-114]` measured at over an hour was the copy, never the
        actual data. Whatever comes back is landed against the local
        library -- reporting, not silently dropping, whatever does not
        `[SPEC-DF-109]`. That `unmatched` count is exactly "flags on tracks
        that don't exist locally," already computed by `import_flags.py`,
        not re-derived here.
        """
        tools = os.path.dirname(os.path.abspath(__file__))
        work = os.path.join(os.path.dirname(tools), "out", f"remote-pull-{job_id}")
        os.makedirs(work, exist_ok=True)
        flags_json = os.path.join(work, "flags.json")

        self._emit(job_id, "stage", "fetch-flags", stage="fetch-flags")
        code, _ = self._spawn(job_id, "fetch-flags", [
            sys.executable, os.path.join(tools, "remote_flags.py"), target, "-o", flags_json])
        if code != 0:
            return self._finish(job_id, "failed")
        self._emit(job_id, "stage", "import", stage="import")
        code, out = self._spawn(job_id, "import", [
            sys.executable, os.path.join(tools, "import_flags.py"),
            self.library, flags_json, "--commit", "--json"])
        result = parse_json_tail(out) or {}
        db = self._db()
        db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
        db.commit()
        db.close()
        return self._finish(job_id, "done" if code == 0 else "failed")

    def _remote_push(self, job_id: int, target: str):
        """A GUI over `export_changes.py`/`apply_changes.py --emit-sql`
        `[SPEC-DF-108..112]` -- the *edits* leg (id/boundary/artist reviews),
        not a raw flag push: Sampo never sets a flag itself, only clears one
        (`--clear-flags`) when the correction it named actually lands. `target`
        splits into an ssh host and the remote's own db path -- the two
        things `scp`/`ssh` need that a single `scp`-style argument does not
        carry on its own.

        Batched, not automatic: this runs only when asked, applying whatever
        has accumulated in the review tables since the last push -- the
        three-way merge already makes a second push of the same edits a
        no-op, so nothing here needs its own queue of "what changed since
        last time."
        """
        tools = os.path.dirname(os.path.abspath(__file__))
        work = os.path.join(os.path.dirname(tools), "out", f"remote-push-{job_id}")
        os.makedirs(work, exist_ok=True)
        copy_db = os.path.join(work, "remote-copy.db")
        changes_json = os.path.join(work, "changes.json")
        patch_sql = os.path.join(work, "patch.sql")

        host, sep, remote_path = target.partition(":")
        if not sep or not remote_path:
            self._emit(job_id, "error", f"target must be user@host:/path, got {target!r}")
            return self._finish(job_id, "failed")

        self._emit(job_id, "stage", "fetch", stage="fetch")
        code, _ = self._spawn(job_id, "fetch", ["scp", target, copy_db])
        if code != 0:
            return self._finish(job_id, "failed")
        self._emit(job_id, "stage", "export", stage="export")
        code, _ = self._spawn(job_id, "export", [
            sys.executable, os.path.join(tools, "export_changes.py"), self.library, "-o", changes_json])
        if code != 0:
            return self._finish(job_id, "failed")
        # `--emit-sql`: `copy_db` is a disposable comparison, never the
        # target of the write itself `[SPEC-DF-111]` -- the real write to
        # vainopi happens two stages further down, via its own `sqlite3` CLI.
        self._emit(job_id, "stage", "compare", stage="compare")
        code, out = self._spawn(job_id, "compare", [
            sys.executable, os.path.join(tools, "apply_changes.py"), copy_db, changes_json,
            "--commit", "--emit-sql", patch_sql, "--clear-flags", "--json"])
        result = parse_json_tail(out) or {}
        if code != 0:
            db = self._db()
            db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
            db.commit()
            db.close()
            return self._finish(job_id, "failed")

        if not result.get("landed") and not result.get("cleared"):
            # Nothing to land -- vainopi is never stopped for an empty patch.
            # `patch_statements` alone cannot tell this: it always includes
            # `ensure_review_tables`'s own schema-setup statements, so it is
            # never zero even when nothing actually changed. A second push
            # after the first, with nothing edited since, must cost the
            # household nothing `[SPEC-SUI-082]`'s own posture toward the
            # player applied to its live service instead of just its write
            # lock.
            self._emit(job_id, "log", "nothing to push -- the remote was not touched")
            db = self._db()
            db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
            db.commit()
            db.close()
            return self._finish(job_id, "done")

        self._emit(job_id, "stage", "send", stage="send")
        code, _ = self._spawn(job_id, "send", ["scp", patch_sql, f"{host}:/tmp/vaino-sync-patch.sql"])
        if code != 0:
            return self._finish(job_id, "failed")
        # The one command vainopi already has `[SPEC-DF-111]`: stop so the
        # patch is never applied underneath a live writer, apply it through
        # vainopi's own `sqlite3`, restart. Briefly interrupts whatever is
        # playing -- why this job runs only on explicit request, never per edit.
        self._emit(job_id, "stage", "apply-remote", stage="apply-remote")
        code, _ = self._spawn(job_id, "apply-remote", [
            "ssh", host,
            f"systemctl stop vaino && sqlite3 {remote_path} < /tmp/vaino-sync-patch.sql "
            f"&& systemctl start vaino"])
        db = self._db()
        db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
        db.commit()
        db.close()
        return self._finish(job_id, "done" if code == 0 else "failed")

    def _accept_remote(self, job_id: int, target: str):
        """`[SPEC-DF-116..117]`'s one deliberate exception to "the console
        never writes the library" -- kept to that discipline's own shape:
        `accept_remote_basis.py` does the write, spawned the same way every
        other write here is, never this process's own (`mode=ro`)
        connection. `target` carries the small JSON the profile page's own
        POST already resolved server-side -- kind, anchor, and the remote
        value fetched moments before by `/api/profile/:id/remote`.
        """
        payload = json.loads(target)
        tools = os.path.dirname(os.path.abspath(__file__))
        anchor = payload["anchor"]
        argv = [sys.executable, os.path.join(tools, "accept_remote_basis.py"), self.library,
                "--kind", payload["kind"], "--audio-md5", anchor["audio_md5"],
                "--passage-kind", anchor["passage_kind"], "--start-ms", str(anchor["start_ms"]),
                "--end-ms", str(anchor["end_ms"]), "--value", json.dumps(payload["value"]),
                "--commit", "--json"]
        self._emit(job_id, "stage", "accept", stage="accept")
        code, out = self._spawn(job_id, "accept", argv)
        result = parse_json_tail(out) or {}
        db = self._db()
        db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
        db.commit()
        db.close()
        return self._finish(job_id, "done" if code == 0 and result.get("ok") else "failed")

    def _suggest_release(self, job_id: int, target: str):
        """Discovery only `[SPEC-SUI-215]` -- `target` is
        `{"folder", "query"}`, `query` optional (the "browse" half: a
        person's own search overriding the algorithm's guessed one).
        `suggest_release.py` itself never touches `passage_recordings`
        without `--accept`, so this is safe to run as freely as a search.
        """
        payload = json.loads(target)
        tools = os.path.dirname(os.path.abspath(__file__))
        argv = [sys.executable, os.path.join(tools, "suggest_release.py"),
                self.library, payload["folder"], "--json"]
        if payload.get("query"):
            argv += ["--query", payload["query"]]
        self._emit(job_id, "stage", "search", stage="search")
        code, out = self._spawn(job_id, "search", argv)
        result = parse_json_tail(out) or {}
        db = self._db()
        db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
        db.commit()
        db.close()
        return self._finish(job_id, "done" if code == 0 and result.get("ok") else "failed")

    def _accept_release(self, job_id: int, target: str):
        """The write half `[SPEC-SUI-215]` -- `target` is
        `{"folder", "release_mbid"}`, the same JSON-in-`target` shape
        `accept-remote` already uses. `suggest_release.py --accept` re-derives
        the same per-file matches from the release now cached by the
        discovery job (or fetches it fresh if this MBID was picked from
        outside the top candidates) and applies them.
        """
        payload = json.loads(target)
        tools = os.path.dirname(os.path.abspath(__file__))
        argv = [sys.executable, os.path.join(tools, "suggest_release.py"),
                self.library, payload["folder"], "--accept", payload["release_mbid"],
                "--commit", "--json"]
        self._emit(job_id, "stage", "apply", stage="apply")
        code, out = self._spawn(job_id, "apply", argv)
        result = parse_json_tail(out) or {}
        db = self._db()
        db.execute("UPDATE jobs SET result=?1 WHERE job_id=?2", (json.dumps(result), job_id))
        db.commit()
        db.close()
        return self._finish(job_id, "done" if code == 0 and result.get("ok") else "failed")

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

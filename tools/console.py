#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The Sampo console, read-only half `[SPEC013]`, `[IMPL-SUI-040]`.

Stage 2 of [IMPL003](../docs/IMPL003-sampo-console-build.md): the views. This
file's own connection to the library stays `mode=ro` throughout -- nothing in
it ever executes a `sqlite3` write -- so the safety claim is structural
rather than promised: a console that cannot open the database for writing
cannot damage a library a player is using, whatever route triggers the
request. That is narrower than "no POST route in this file," which was true
once but no longer is: stage 3's job dispatch and `[REQ-VIS-265]`'s unflag
both hang off `do_POST` below. Neither writes *here* -- a job runs the same
CLI a person would run by hand, against its own connection; unflaging
signals the already-running Vaino to write its own listener state over HTTP,
in `vaino_control.py`, never a `listener_flags` write from this process.

That is what makes it runnable against the live database on day one. The
library is WAL `[SPEC-SUI-082]`, so readers never block the player: browsing
seven thousand files here cannot interrupt a note being played there.

Three views:

  /            library -- what is known, and how it came to be known
  /folder      what is on disk, against what the database claims
  /profile/N   one passage's whole derivation

    python tools/console.py data/vaino_new.db --root "C:/Users/Mango Cat/Music"
"""

import argparse
import html
import json
import os
import socketserver
import sqlite3
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs, unquote

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ingest_folder import AUDIO  # noqa: E402  -- one list of what counts as audio
import jobs as jobmod  # noqa: E402
import vaino_control  # noqa: E402  -- process/network side of the handoff

WEB = os.path.join(os.path.dirname(os.path.abspath(__file__)), "console_web")
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Sampo's own port. Different from vaino_control.VAINO_PORT (5720) because
# they are different services on the same machine, and because
# `[SPEC-SUI-170]` may start the player: colliding would make each look like
# the other's failure.
DEFAULT_PORT = 5730

# 71 (characteristic, class) pairs across 18 characteristics is a complete
# vector `[SPEC-SA-040]`. Measured on the four reference tracks, and the number
# the completeness tick compares against.
FULL_FLAVOR = 71

STATE = {"db": None, "path": None, "roots": [], "scan": None, "scanned_at": 0, "jobs": None,
         "build": None, "started_at": None, "port": None}


# ---------------------------------------------------------------- database ---

def ro(db: str) -> sqlite3.Connection:
    """Read-only, and it must stay that way `[IMPL-SUI-040]`."""
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


def totals(conn) -> dict:
    q = lambda s: conn.execute(s).fetchone()[0]  # noqa: E731
    # `id_checks` is written by the fingerprint pass, not by schema.sql -- a
    # library nothing has ever fingerprinted has no such table at all, and a
    # query naming a missing table fails outright rather than finding nothing
    # `[REQ-LIB-165]`. "Never checked" must not crash the page that would say so.
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    return {
        "files": q("SELECT count(*) FROM files"),
        "passages": q("SELECT count(*) FROM passages"),
        "radio": q("SELECT count(*) FROM passages WHERE kind='radio'"),
        "recordings": q("SELECT count(*) FROM recordings"),
        # The two facets that are Sampo's business and never the player's.
        "unidentified": q("SELECT count(*) FROM recordings WHERE mbid NOT LIKE '________-____-____-____-____________'"),
        "unchecked": q("SELECT count(*) FROM passages p WHERE p.kind='radio' AND NOT EXISTS "
                       "(SELECT 1 FROM id_checks c WHERE c.passage_id = p.passage_id)")
                     if "id_checks" in have else q("SELECT count(*) FROM passages WHERE kind='radio'"),
        "no_flavor": q("SELECT count(*) FROM passages p JOIN passage_recordings pr USING(passage_id) "
                       "WHERE p.kind='radio' AND NOT EXISTS "
                       "(SELECT 1 FROM flavor f WHERE f.subject_kind = 'recording' AND f.subject_id = pr.mbid)"),
    }


# Every `flavor` lookup here names `subject_kind` as well as `subject_id`, and
# that is load-bearing rather than tidy. The key is
# (subject_kind, subject_id, characteristic, class) and the index repeats that
# prefix `[SPEC-SC-060]`, so a lookup on `subject_id` alone matches neither and
# SQLite scans all 578,452 rows ONCE PER PASSAGE. Measured: >180 s against
# 0.044 s, and the plan goes from SCAN to SEARCH.
#
# This is the same fault `[REQ-LIB-165]` recorded against
# `release_recordings(mbid)` -- "the lookup uses the second column of the
# primary key, so no index applies". It was fixed there with a new index; here
# the prefix column is already known, so naming it costs nothing.
def library(conn, q: str = "", facet: str = "", limit: int = 400) -> list:
    """Rows for the library view.

    Sampo browses to *inspect*, so every row carries derivation state the
    player's browse deliberately does not show `[SPEC-SUI-020]`: how much
    flavor, whether the id was ever checked, what named it.
    """
    where, args = ["p.kind = 'radio'"], []
    if q:
        where.append("(t.title LIKE ?1 OR t.artist LIKE ?1 OR t.album LIKE ?1 OR r.title LIKE ?1)")
        args.append(f"%{q}%")
    if facet == "unidentified":
        # Not an MBID: `local:audio:`, `local:track:N`, anything malformed.
        # Shape-checked rather than prefix-checked, so a fourth kind is caught
        # too -- the same test `[REQ-LIB-165]` applies.
        where.append("pr.mbid NOT LIKE '________-____-____-____-____________'")
    elif facet == "unchecked":
        where.append("NOT EXISTS (SELECT 1 FROM id_checks c WHERE c.passage_id = p.passage_id)")
    elif facet == "no-flavor":
        where.append("NOT EXISTS (SELECT 1 FROM flavor f WHERE f.subject_kind = 'recording' AND f.subject_id = pr.mbid)")

    sql = f"""
      SELECT p.passage_id, pr.mbid,
             COALESCE(r.title, t.title) AS title, t.artist, t.album,
             p.end_ms - p.start_ms AS len_ms, p.boundary_src,
             (SELECT count(*) FROM flavor f WHERE f.subject_kind = 'recording' AND f.subject_id = pr.mbid) AS flavor,
             (SELECT verdict FROM id_checks c WHERE c.passage_id = p.passage_id) AS verdict
      FROM passages p
      JOIN passage_recordings pr USING (passage_id)
      JOIN files fi USING (file_id)
      LEFT JOIN recordings r ON r.mbid = pr.mbid
      LEFT JOIN file_tags t ON t.file_id = fi.file_id
      WHERE {' AND '.join(where)}
      ORDER BY t.artist IS NULL, t.artist, t.album, p.passage_id
      LIMIT {int(limit)}"""
    return [dict(r) for r in conn.execute(sql, args)]


def flags(conn) -> list:
    """Recordings and passages flagged "for review" from Vaino's own
    play-history page `[REQ-VIS-265]`, newest flag first.

    Read-only, like everything else in this file -- the checkbox that sets
    and clears a flag lives in Vaino, because it is listener state and
    listener state is Vaino's to write `[SPEC-SC-020]`. This only ever looks.

    `listener_flags` may not exist at all on a library no version of Vaino
    carrying this feature has ever opened; that is "nothing flagged yet",
    not a broken page `[REQ-LIB-165]`.
    """
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "listener_flags" not in have:
        return []

    out = []
    for kind, subject_id, flagged_at in conn.execute(
            "SELECT subject_kind, subject_id, flagged_at FROM listener_flags "
            "ORDER BY flagged_at DESC"):
        passages, mbid = [], None
        if kind == "recording":
            mbid = subject_id
            passages = [r[0] for r in conn.execute(
                "SELECT passage_id FROM passage_recordings WHERE mbid=? "
                "ORDER BY weight DESC, passage_id", (mbid,))]
        else:
            pid = int(subject_id)
            if conn.execute("SELECT 1 FROM passages WHERE passage_id=?", (pid,)).fetchone():
                passages = [pid]
                row = conn.execute(
                    "SELECT mbid FROM passage_recordings WHERE passage_id=? "
                    "ORDER BY weight DESC, mbid LIMIT 1", (pid,)).fetchone()
                mbid = row[0] if row else None

        title = artist = None
        if mbid:
            row = conn.execute("SELECT title FROM recordings WHERE mbid=?", (mbid,)).fetchone()
            title = row[0] if row else None
            row = conn.execute(
                "SELECT a.name FROM recording_artists ra JOIN artists a ON a.mbid=ra.artist_mbid "
                "WHERE ra.mbid=? ORDER BY ra.weight DESC LIMIT 1", (mbid,)).fetchone()
            artist = row[0] if row else None
        if title is None and passages:
            # No recording (or the recording carries no title of its own) --
            # the file's own tag is what a listener actually saw play.
            row = conn.execute(
                "SELECT t.title, t.artist FROM passages p JOIN files fi USING(file_id) "
                "LEFT JOIN file_tags t ON t.file_id=fi.file_id WHERE p.passage_id=?",
                (passages[0],)).fetchone()
            if row:
                title, artist = title or row[0], artist or row[1]

        out.append({
            "subject_kind": kind, "subject_id": subject_id, "flagged_at": flagged_at,
            "title": title, "artist": artist, "passages": passages,
            # A passage-keyed flag from before a rescan renumbered things
            # resolves to nothing at all -- said plainly, not left blank
            # `[SPEC-DF-035]`.
            "resolved": bool(passages),
        })
    return out


def pending_counts(conn) -> dict:
    """How many reviewed decisions are sitting as drafts, not yet folded into
    the library `[REQ-VIS-275]` -- the same three tables `tools/apply_reviews
    .py`/`tools/apply_boundary_reviews.py` already read, counted rather than
    listed: a naive user has no reason to know these tools, or that saving an
    edit in Vaino's own editor is only the first of two deliberate steps
    before it can even be pushed anywhere `[SPEC021 §2]`. Zero for any table
    this library predates -- absence is "nothing pending," not an error.
    """
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    counts = {}
    for kind, table in (("id", "id_reviews"), ("boundary", "boundary_reviews"),
                        ("artist", "artist_reviews")):
        counts[kind] = (conn.execute(
            f"SELECT COUNT(*) FROM {table} WHERE applied_at IS NULL").fetchone()[0]
            if table in have else 0)
    counts["total"] = sum(counts.values())
    return counts


def profile(conn, pid: int) -> dict:
    """One passage's whole derivation `[SPEC-SUI-040]`.

    Per-characteristic provenance is shown because it is stored per
    characteristic `[SPEC-SC-060]`: an aggregate "flavor: yes" would hide a
    mixture that measurably costs retrieval accuracy `[SPEC-FD-145]`.
    """
    p = conn.execute(
        "SELECT p.*, f.audio_md5, f.path, f.format, f.duration_ms, f.size_bytes "
        "FROM passages p JOIN files f USING(file_id) WHERE p.passage_id = ?", (pid,)).fetchone()
    if p is None:
        return {}
    creds = [dict(r) for r in conn.execute(
        "SELECT * FROM passage_recordings WHERE passage_id = ? ORDER BY mbid", (pid,))]
    recs = []
    for c in creds:
        r = conn.execute("SELECT * FROM recordings WHERE mbid = ?", (c["mbid"],)).fetchone()
        flav = [dict(x) for x in conn.execute(
            "SELECT characteristic, class, value, source, accuracy FROM flavor "
            "WHERE subject_kind='recording' AND subject_id = ? ORDER BY characteristic, class",
            (c["mbid"],))]
        recs.append({
            "credit": c,
            "recording": dict(r) if r else None,
            "flavor": flav,
            # Provenance is per characteristic, so a single source string would
            # be a claim the data does not support. Count them instead.
            "flavor_sources": sorted({x["source"] for x in flav}),
            "artists": [dict(a) for a in conn.execute(
                "SELECT ra.artist_mbid, ra.weight, ra.source, ar.name FROM recording_artists ra "
                "LEFT JOIN artists ar ON ar.mbid = ra.artist_mbid WHERE ra.mbid = ?", (c["mbid"],))],
        })
    return {
        "passage": dict(p),
        "tags": dict(conn.execute("SELECT * FROM file_tags WHERE file_id = ?",
                                  (p["file_id"],)).fetchone() or {}),
        "recordings": recs,
        "cached": conn.execute(
            "SELECT count(*) FROM lowlevel_cache WHERE audio_md5 = ? AND start_ms = ? AND end_ms = ?",
            (p["audio_md5"], p["start_ms"], p["end_ms"])).fetchone()[0],
        "check": dict(conn.execute("SELECT * FROM id_checks WHERE passage_id = ?",
                                   (pid,)).fetchone() or {}),
        # What each stage decided, and what it rejected. Written since the
        # migration and read by nothing until now `[SPEC-SC-100]`.
        "decisions": [dict(d) for d in conn.execute(
            "SELECT stage, outcome, confidence, detail, decided_at FROM ingest_decisions "
            "WHERE audio_md5 = ? ORDER BY decided_at", (p["audio_md5"],))],
        # A saved-but-not-yet-applied edit `[REQ-VIS-275]` -- distinct from
        # `boundary_src == 'manual'`, which only ever shows an edit already
        # folded in. This is the state that looked identical to "pushed" from
        # this very page until it wasn't: Vaino's editor commits a draft here
        # and changes nothing else, so a naive glance at this profile has no
        # way to tell "edited" from "edited, but only as far as the draft."
        "pending": _pending_for_passage(conn, pid, [c["mbid"] for c in creds]),
    }


def _pending_for_passage(conn, pid: int, mbids: list) -> dict:
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    out = {}
    if "id_reviews" in have:
        row = conn.execute(
            "SELECT decided_at FROM id_reviews WHERE passage_id=?1 AND applied_at IS NULL", (pid,)
        ).fetchone()
        if row:
            out["id"] = {"decided_at": row[0]}
    if "boundary_reviews" in have:
        row = conn.execute(
            "SELECT decided_at FROM boundary_reviews WHERE passage_id=?1 AND applied_at IS NULL", (pid,)
        ).fetchone()
        if row:
            out["boundary"] = {"decided_at": row[0]}
    if "artist_reviews" in have and mbids:
        placeholders = ",".join("?" * len(mbids))
        row = conn.execute(
            f"SELECT decided_at FROM artist_reviews "
            f"WHERE recording_mbid IN ({placeholders}) AND applied_at IS NULL "
            f"ORDER BY decided_at DESC LIMIT 1", mbids).fetchone()
        if row:
            out["artist"] = {"decided_at": row[0]}
    return out


def _peek(remote: str, kind: str, anchor_args: list, timeout: float = 12.0) -> dict:
    """One `remote_peek.py` subprocess call `[SPEC-DF-116]`. Never this
    process's own connection reaching across the network -- a subprocess,
    the same posture every write/read-adjacent action in this console
    already takes -- and never allowed to hang past `timeout`: a check that
    cannot run must not stop someone from working `[SPEC-DF-118]`.
    """
    tools = os.path.dirname(os.path.abspath(__file__))
    try:
        r = subprocess.run(
            [sys.executable, os.path.join(tools, "remote_peek.py"), remote, "--kind", kind, *anchor_args],
            capture_output=True, text=True, timeout=timeout)
    except (subprocess.TimeoutExpired, OSError) as e:
        return {"ok": False, "error": f"{type(e).__name__}: {e}"}
    text = (r.stdout or "").strip()
    if not text:
        return {"ok": False, "error": (r.stderr or f"no output, exited {r.returncode}").strip()[:300]}
    try:
        return json.loads(text.splitlines()[-1])
    except json.JSONDecodeError as e:
        return {"ok": False, "error": f"unparseable reply: {e}"}


def remote_status(conn, pid: int) -> dict:
    """A targeted remote read at the moment a profile is opened
    `[SPEC-DF-116..118]` -- never a database copy, never a block. Checks the
    two identities this page can actually hand off to Vaino's own editors
    (id review, boundary editing); an artist-review divergence is not
    offered here because this page never offers one to accept either.
    """
    remote = STATE["jobs"].get_remote()
    if not remote:
        return {"remote": None}   # nothing configured -- nothing to check against

    p = conn.execute(
        "SELECT p.kind, p.start_ms, p.end_ms, p.lead_in_ms, p.lead_out_ms, p.gain_db, f.audio_md5 "
        "FROM passages p JOIN files f USING(file_id) WHERE p.passage_id = ?1", (pid,)).fetchone()
    if p is None:
        return {"remote": remote, "reachable": False, "error": "no such passage"}
    anchor = {"audio_md5": p["audio_md5"], "passage_kind": p["kind"],
              "start_ms": p["start_ms"], "end_ms": p["end_ms"]}
    anchor_args = ["--audio-md5", p["audio_md5"], "--passage-kind", p["kind"],
                   "--start-ms", str(p["start_ms"]), "--end-ms", str(p["end_ms"])]
    local_mbid = conn.execute(
        "SELECT mbid FROM passage_recordings WHERE passage_id=?1 ORDER BY weight DESC, mbid LIMIT 1",
        (pid,)).fetchone()

    reachable = True
    checks = {}
    for kind, local_value in (
        ("id_review", {"mbid": local_mbid[0] if local_mbid else None}),
        ("boundary_review", {"start_ms": p["start_ms"], "end_ms": p["end_ms"],
                              "lead_in_ms": p["lead_in_ms"], "lead_out_ms": p["lead_out_ms"],
                              "gain_db": p["gain_db"]}),
    ):
        result = _peek(remote, kind, anchor_args)
        if not result.get("ok"):
            reachable = False
            continue
        current = result.get("current")
        checks[kind] = {"current": current, "local": local_value,
                         "diverged": current is not None and current != local_value}
    return {"remote": remote, "reachable": reachable, "anchor": anchor, "checks": checks}


def passage_flag_subjects(conn, pid: int) -> list:
    """Every `listener_flags` subject that plausibly names this passage --
    its own passage-keyed row, plus every recording currently linked to it
    `[SPEC-DF-112]`'s own `clear_flags_for()` already establishes this same
    shape for the same reason: a listener may have flagged it before it had
    a recording at all, or under one it has since moved away from. Read-only
    here; shared by the sync-status check and the unflag action below so
    the two can never disagree about what "flagged" means for this passage.
    """
    subjects = [("passage", str(pid))]
    for row in conn.execute(
            "SELECT DISTINCT mbid FROM passage_recordings WHERE passage_id=?1", (pid,)):
        subjects.append(("recording", row[0]))
    return subjects


def passage_flagged_locally(conn, subjects: list) -> bool:
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "listener_flags" not in have:
        return False
    return any(
        conn.execute("SELECT 1 FROM listener_flags WHERE subject_kind=?1 AND subject_id=?2",
                     (kind, sid)).fetchone()
        for kind, sid in subjects)


def flag_sync_status(conn, pid: int) -> dict:
    """Is this passage flagged locally, on the remote, and does that match
    what this page already shows -- the display `[REQ-VIS-265]`'s checkbox
    needs before offering to clear it everywhere at once.

    "Shown" is never fetched separately: this process only ever renders a
    passage from its own local database, so whatever a person is looking at
    on this very page *is* `local` at the moment it loaded. The only
    genuinely open question a live check can answer is whether the remote
    still agrees -- `[SPEC-DF-115]`'s own point, that vainopi's flags can
    change with no involvement from Sampo at all.
    """
    subjects = passage_flag_subjects(conn, pid)
    local = passage_flagged_locally(conn, subjects)
    remote = STATE["jobs"].get_remote()
    if not remote:
        return {"local": local, "remote": None, "reachable": False, "remote_pid": None, "remote_mbids": []}

    p = conn.execute(
        "SELECT p.kind, p.start_ms, p.end_ms, f.audio_md5 FROM passages p "
        "JOIN files f USING(file_id) WHERE p.passage_id=?1", (pid,)).fetchone()
    if p is None:
        return {"local": local, "remote": None, "reachable": False, "remote_pid": None, "remote_mbids": []}
    anchor_args = ["--audio-md5", p["audio_md5"], "--passage-kind", p["kind"],
                   "--start-ms", str(p["start_ms"]), "--end-ms", str(p["end_ms"])]
    result = _peek(remote, "passage_flag", anchor_args)
    if not result.get("ok"):
        return {"local": local, "remote": None, "reachable": False, "remote_pid": None, "remote_mbids": []}
    current = result.get("current") or {}
    remote_mbids = []
    try:
        remote_mbids = json.loads(current.get("remote_mbids") or "[]")
    except (ValueError, TypeError):
        pass  # malformed reply from an old remote_peek.py -- treated as "none known"
    return {"local": local, "remote": bool(current.get("flagged")), "reachable": True,
            "remote_pid": current.get("remote_passage_id"), "remote_mbids": remote_mbids}


# ------------------------------------------------------------------ system ---
# Which running instance is this, and a way to stop it `[SPEC-SUI-210..212]`.
# Grew directly out of a real incident: two stale `console.py` processes were
# both alive against the same library, both bound to :5730 via a Windows
# `SO_REUSEADDR` quirk, and telling them apart took forensic process-listing
# by hand -- exactly the question this page exists to answer at a glance.

def build_info(repo_root: str) -> dict:
    """The commit (and working-tree state) this *process* loaded its source
    from at startup -- not a live `git status`, a snapshot `[SPEC-SUI-211]`.
    This tool has no compiled build to embed a version into; the checkout it
    runs from is the closest honest equivalent, and it cannot change under a
    process already running from it.
    """
    def git(*args):
        try:
            r = subprocess.run(["git", *args], cwd=repo_root, capture_output=True,
                               text=True, timeout=5)
        except OSError:
            return None
        return r.stdout.strip() if r.returncode == 0 else None

    commit = git("rev-parse", "HEAD")
    if commit is None:
        # Not a git checkout, or git isn't on PATH -- said plainly, the same
        # posture as every other capability here that can be absent
        # `[SPEC-DF-095]`, not a page that silently omits the section.
        return {"available": False}
    status = git("status", "--porcelain")
    dirty_files = status.count("\n") + 1 if status else 0
    return {
        "available": True,
        "commit": commit,
        "commit_short": git("rev-parse", "--short", "HEAD"),
        "branch": git("rev-parse", "--abbrev-ref", "HEAD"),
        "commit_date": git("show", "-s", "--format=%cI", "HEAD"),
        "commit_subject": git("show", "-s", "--format=%s", "HEAD"),
        "dirty": None if status is None else dirty_files > 0,
        "dirty_files": dirty_files,
    }


def system_status() -> dict:
    runner = STATE["jobs"]
    active = None
    current = runner.current if runner else None
    if current is not None:
        j = runner.job(current)
        if j:
            active = {"job_id": j["job_id"], "kind": j["kind"], "target": j["target"], "state": j["state"]}
    return {
        "build": STATE["build"],
        "pid": os.getpid(),
        "started_at": STATE["started_at"],
        "port": STATE["port"],
        "db_path": STATE["path"],
        "roots": STATE["roots"],
        "active_job": active,
    }


def _shutdown_soon(httpd) -> None:
    """Off the request-handling thread, on purpose `[SPEC-SUI-212]`:
    `BaseServer.shutdown()` blocks until `serve_forever()`'s own loop (the
    main thread) returns, and calling it from that same loop would deadlock.
    A short delay lets the triggering request's own response actually reach
    the browser before the socket that would carry it stops accepting more.
    """
    time.sleep(0.3)
    httpd.shutdown()


# ----------------------------------------------------------------- handoff ---
# Reaching the player's own pages from inside Sampo's workflow `[SPEC-SUI-140]`,
# `[SPEC-SUI-135]`. Sampo never asks Vaino anything about the *library* it is
# running -- only the operating system, whether the port answers at all
# `[SPEC-SUI-025]`, `[SPEC-SUI-170]`, plus one narrow capability probe
# `[SPEC-SUI-213]` a socket alone cannot answer. The round trip closes through
# the shared database on Sampo's next scan, not through this connection
# `[SPEC-SUI-145]`.
#
# The process/network primitives live in `vaino_control.py`, not here --
# reaching the player's own process, and signaling it to write, is a
# different concern than reading this file's own (`mode=ro`) connection, and
# keeping them apart is what makes this file's safety claim structural again.
# `unflag_everywhere` below stays a thin wrapper: the *read* side -- which
# subjects, whether a remote is configured, what it currently thinks -- is
# this file's own, resolved from `conn`; only the actual signal to each
# Vaino crosses into `vaino_control`.

def unflag_everywhere(conn, pid: int) -> dict:
    """Clear every plausible flag on this passage, locally and on the
    remote, in one action `[REQ-VIS-265]`. See `vaino_control.unflag_everywhere`
    for how the clear itself happens; this resolves what to clear.
    """
    subjects = passage_flag_subjects(conn, pid)
    remote = STATE["jobs"].get_remote()
    status = flag_sync_status(conn, pid) if remote else None
    return vaino_control.unflag_everywhere(subjects, remote, status)


# -------------------------------------------------------------------- scan ---

def scan(conn, roots: list) -> dict:
    """The cheap pass `[SPEC-SUI-060]`: stat, do not hash.

    Hashing 7,232 files costs about nine minutes at the measured 74 ms each
    `[SPEC-RLK-070]`, which is not a page load. `size_bytes` and `mtime` exist
    in the schema for exactly this -- "cheap change detection only"
    `[SPEC-SC-030]`.

    **Every verdict here is provisional and says so.** Only a hash separates
    `unknown` from `elsewhere`, or `changed` from `corrupt` `[SPEC-RLK-055]`,
    and a page that reported those without hashing would be asserting what it
    had not observed -- the hazard `[SPEC-RLK-140]` names. Resolving them is a
    job, and jobs are stage 3.
    """
    t0 = time.time()
    # Paths are compared with the platform's own case rules -- `normcase` is a
    # no-op on POSIX and folds on Windows. That is correct here and is NOT the
    # trap `[SPEC-RLK-020]` describes: that hazard is about paths crossing
    # between platforms, and nothing in this view is ever transported.
    def key(p):
        return os.path.normcase(os.path.normpath(p))

    disk = {}
    for root in roots:
        for dp, _, names in os.walk(root):
            for n in names:
                if n.lower().endswith(AUDIO):
                    full = os.path.join(dp, n)
                    try:
                        st = os.stat(full)
                    except OSError:
                        continue
                    disk[key(full)] = (full, st.st_size, st.st_mtime)

    rows = {}
    for r in conn.execute("SELECT file_id, audio_md5, path, size_bytes, mtime FROM files"):
        rows[key(r["path"])] = r

    here = changed = 0
    unclaimed, missing = [], []
    for k, (full, size, _mtime) in disk.items():
        r = rows.get(k)
        if r is None:
            # By path alone this is unknown. Its hash may yet match a row whose
            # own path is stale, which would make it `moved` -- not decidable
            # here, and not guessed at.
            unclaimed.append(full)
        elif r["size_bytes"] == size:
            here += 1
        else:
            # The bytes changed. A retag changes size and leaves `audio_md5`
            # untouched `[SPEC-DF-020]`; corruption changes both. Same
            # observation, opposite meanings, and only a hash tells them apart.
            changed += 1
    for k, r in rows.items():
        if k not in disk:
            missing.append(r["path"])

    return {
        "roots": roots,
        "walked_ms": int((time.time() - t0) * 1000),
        "on_disk": len(disk),
        "rows": len(rows),
        # `assumed`, never `verified`: passed on size and mtime, not hashed.
        "assumed_here": here,
        "changed": changed,
        "unclaimed": sorted(unclaimed),
        "missing": sorted(missing),
        "verified": 0,
        "note": "cheap pass: nothing was hashed, so nothing here is verified",
    }


def completeness(conn) -> dict:
    """Library-wide stage coverage -- the view stage 0 proved was needed.

    A backlog of 136 unchecked passages sat invisible until a run stumbled over
    it `[IMPL-SUI-025]`. Nothing reported it because nothing asked.
    """
    t = totals(conn)
    return {
        "radio": t["radio"],
        "with_flavor": t["radio"] - t["no_flavor"],
        "id_checked": t["radio"] - t["unchecked"],
        "identified": t["radio"] - conn.execute(
            "SELECT count(*) FROM passages p JOIN passage_recordings pr USING(passage_id) "
            "WHERE p.kind='radio' AND pr.mbid NOT LIKE "
            "'________-____-____-____-____________'").fetchone()[0],
        "amplitude": conn.execute(
            "SELECT count(*) FROM passages WHERE kind='radio' AND lead_in_ms IS NOT NULL"
        ).fetchone()[0],
    }


# ------------------------------------------------------------------ server ---

class Handler(BaseHTTPRequestHandler):
    server_version = "SampoConsole/0.1"

    def log_message(self, fmt, *args):  # quieter than the default
        if "--verbose" in sys.argv:
            super().log_message(fmt, *args)

    def send_json(self, obj, code=200):
        body = json.dumps(obj, ensure_ascii=False, default=str).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def send_file(self, name, ctype):
        path = os.path.join(WEB, name)
        if not os.path.isfile(path):
            return self.send_error(404)
        with open(path, "rb") as fh:
            body = fh.read()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    # GET only. There is no `do_POST` in this file and that is the stage 2
    # safety claim in its most direct form `[IMPL-SUI-040]`.
    def do_GET(self):
        u = urlparse(self.path)
        p, qs = u.path, parse_qs(u.query)
        conn = STATE["db"]
        try:
            if p == "/":
                return self.send_file("index.html", "text/html; charset=utf-8")
            if p == "/folder":
                return self.send_file("folder.html", "text/html; charset=utf-8")
            if p.startswith("/profile/"):
                return self.send_file("profile.html", "text/html; charset=utf-8")
            if p == "/console.css":
                return self.send_file("console.css", "text/css; charset=utf-8")
            if p == "/console.js":
                return self.send_file("console.js", "application/javascript; charset=utf-8")

            if p == "/api/totals":
                return self.send_json({"totals": totals(conn), "coverage": completeness(conn)})
            if p == "/api/pending":
                return self.send_json(pending_counts(conn))
            if p == "/api/library":
                return self.send_json(library(
                    conn, q=(qs.get("q") or [""])[0], facet=(qs.get("facet") or [""])[0]))
            if p.startswith("/api/profile/") and p.endswith("/remote"):
                pid = int(p.split("/")[3])
                return self.send_json(remote_status(conn, pid))
            if p.startswith("/api/profile/") and p.endswith("/flag"):
                pid = int(p.split("/")[3])
                return self.send_json(flag_sync_status(conn, pid))
            if p.startswith("/api/profile/"):
                pid = int(p.rsplit("/", 1)[-1])
                d = profile(conn, pid)
                return self.send_json(d) if d else self.send_error(404)
            if p == "/jobs":
                return self.send_file("jobs.html", "text/html; charset=utf-8")
            if p == "/export":
                return self.send_file("export.html", "text/html; charset=utf-8")
            if p == "/flags":
                return self.send_file("flags.html", "text/html; charset=utf-8")
            if p == "/api/flags":
                return self.send_json(flags(conn))
            if p == "/system":
                return self.send_file("system.html", "text/html; charset=utf-8")
            if p == "/api/system":
                return self.send_json(system_status())
            if p == "/api/remote":
                return self.send_json({"remote": STATE["jobs"].get_remote()})
            if p == "/api/jobs":
                return self.send_json(STATE["jobs"].recent())
            if p.startswith("/api/jobs/") and p.endswith("/stream"):
                return self.stream(int(p.split("/")[3]))
            if p.startswith("/api/jobs/"):
                d = STATE["jobs"].job(int(p.rsplit("/", 1)[-1]))
                return self.send_json(d) if d else self.send_error(404)
            if p == "/api/folder/scan":
                # Read-only and idempotent, so GET rather than the POST the
                # route sketch showed: a refresh must be harmless, and the
                # expensive half (hashing) is a job, not this.
                if STATE["scan"] is None or "refresh" in qs:
                    STATE["scan"] = scan(conn, STATE["roots"])
                    STATE["scanned_at"] = time.time()
                return self.send_json(STATE["scan"])
            if p == "/api/handoff/ensure":
                # Idempotent -- GET rather than a POST, the same reasoning as
                # the folder scan above: asking twice costs nothing when a
                # player is already there, which is the common case.
                return self.send_json(vaino_control.ensure_vaino(
                    db_path=STATE["path"], sampo_build=STATE["build"]))
            self.send_error(404)
        except BrokenPipeError:
            pass
        except Exception as e:  # a failed query must report, never render empty
            self.send_json({"error": f"{type(e).__name__}: {e}"}, code=500)

    # Server-sent events, not a WebSocket. A job emits progress in one
    # direction and takes its commands as POSTs, so a duplex socket would be
    # machinery for a direction nothing uses `[SPEC-SUI-030]`.
    def stream(self, job_id: int):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        after, idle = 0, 0
        try:
            while idle < 900:            # ~15 min of nothing, then let go
                evs = STATE["jobs"].events_since(job_id, after)
                for e in evs:
                    after = e["event_id"]
                    self.wfile.write(f"data: {json.dumps(e, default=str)}\n\n".encode())
                    self.wfile.flush()
                    if e["kind"] == "done":
                        return
                idle = 0 if evs else idle + 1
                time.sleep(1)
        except (BrokenPipeError, ConnectionAbortedError, ConnectionResetError):
            pass          # the page went away; the job does not care

    # Stage 3's writes. Note what is NOT here: nothing in this file opens the
    # library for writing. These start jobs, and the jobs run the same CLIs a
    # person runs `[SPEC-SUI-015]`.
    def do_POST(self):
        u = urlparse(self.path)
        p = u.path
        conn = STATE["db"]
        try:
            if p.startswith("/api/profile/") and p.endswith("/accept-remote"):
                # [SPEC-DF-116..117]'s one deliberate exception to "the
                # console never writes the library" -- the anchor is
                # resolved server-side, fresh, from `pid`, never trusted from
                # the client, so a stale page cannot aim a write at the
                # wrong row.
                pid = int(p.split("/")[3])
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                payload = json.loads(body or b"{}") or {}
                kind, value = payload.get("kind"), payload.get("value")
                if kind not in ("id_review", "boundary_review") or not isinstance(value, dict):
                    return self.send_json(
                        {"error": "expected {kind: id_review|boundary_review, value: {...}}"}, code=400)
                row = conn.execute(
                    "SELECT p.kind, p.start_ms, p.end_ms, f.audio_md5 FROM passages p "
                    "JOIN files f USING(file_id) WHERE p.passage_id=?1", (pid,)).fetchone()
                if row is None:
                    return self.send_json({"error": f"no such passage: {pid}"}, code=404)
                anchor = {"audio_md5": row["audio_md5"], "passage_kind": row["kind"],
                          "start_ms": row["start_ms"], "end_ms": row["end_ms"]}
                target = json.dumps({"kind": kind, "anchor": anchor, "value": value})
                return self.send_json({"job_id": STATE["jobs"].submit("accept-remote", target)})
            if p.startswith("/api/profile/") and p.endswith("/unflag"):
                # Not a job `[SPEC-SUI-080]`: unlike every write that model
                # wraps, nothing here spawns a Python tool against a
                # database at all -- both writes happen inside Vaino's own
                # process, over HTTP, synchronously, in the same request/
                # response cycle `_peek()`'s own remote check already uses.
                pid = int(p.split("/")[3])
                if conn.execute("SELECT 1 FROM passages WHERE passage_id=?1", (pid,)).fetchone() is None:
                    return self.send_json({"error": f"no such passage: {pid}"}, code=404)
                return self.send_json(unflag_everywhere(conn, pid))
            if p == "/api/induct/propose":
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                folder = (json.loads(body or b"{}") or {}).get("folder", "")
                if not folder or not os.path.isdir(folder):
                    return self.send_json({"error": f"not a folder: {folder}"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("propose", folder)})
            if p.startswith("/api/induct/") and p.endswith("/commit"):
                job_id = int(p.split("/")[3])
                prev = STATE["jobs"].job(job_id)
                # Confirm the plan that was read, not the folder as it is now
                # `[SPEC-SUI-070]`.
                if not prev or prev["kind"] != "propose" or prev["state"] != "done":
                    return self.send_json({"error": "no completed proposal to confirm"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("induct", prev["target"])})
            if p == "/api/reanalyze":
                # No propose/plan step, unlike fresh induction `[SPEC-SUI-070]`
                # -- this folder is already known, there is no "new files
                # discovered" surprise to preview, only whether to retry what
                # `identify` already gave up on `[SPEC-SUI-214]`.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                folder = (json.loads(body or b"{}") or {}).get("folder", "")
                if not folder or not os.path.isdir(folder):
                    return self.send_json({"error": f"not a folder: {folder}"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("reanalyze", folder)})
            if p == "/api/analyze-amplitude":
                # `[SPEC-SA-075]`, deliberately opt-in -- see `jobs.py`'s own
                # `SKIPPED` entry for why this is never part of `/api/reanalyze`
                # or fresh induction. `folder` is optional here (unlike
                # `/api/reanalyze`'s own required one): an empty/absent value
                # means the whole library, matching `analyze_amplitude.py`'s
                # own CLI default.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                folder = (json.loads(body or b"{}") or {}).get("folder", "") or ""
                if folder and not os.path.isdir(folder):
                    return self.send_json({"error": f"not a folder: {folder}"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("analyze-amplitude", folder)})
            if p == "/api/analyze-flavor":
                # Scoped to one passage, not a folder -- refreshing flavor
                # after a boundary edit is a per-passage question, and
                # re-running extraction over an entire folder just to reach
                # one changed passage would redo work on everything else
                # that is already cached and unaffected.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                passage_id = (json.loads(body or b"{}") or {}).get("passage_id")
                if not isinstance(passage_id, int) or passage_id <= 0:
                    return self.send_json({"error": f"not a passage id: {passage_id!r}"}, code=400)
                return self.send_json(
                    {"job_id": STATE["jobs"].submit("analyze-flavor", str(passage_id))})
            if p == "/api/release/suggest":
                # Discovery only `[SPEC-SUI-215]` -- never touches
                # passage_recordings, so no confirmation step belongs here.
                # `query` is optional -- the "browse" half of the feature:
                # a person overriding the algorithm's own guessed search.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                payload = json.loads(body or b"{}") or {}
                folder = payload.get("folder", "")
                if not folder or not os.path.isdir(folder):
                    return self.send_json({"error": f"not a folder: {folder}"}, code=400)
                target = json.dumps({"folder": folder, "query": payload.get("query") or None})
                return self.send_json({"job_id": STATE["jobs"].submit("suggest-release", target)})
            if p == "/api/release/accept":
                # The write half `[SPEC-SUI-215]` -- the one place this
                # feature touches the library, and only for whichever
                # release the operator actually picked, never automatically.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                payload = json.loads(body or b"{}") or {}
                folder, release_mbid = payload.get("folder", ""), payload.get("release_mbid", "")
                if not folder or not os.path.isdir(folder):
                    return self.send_json({"error": f"not a folder: {folder}"}, code=400)
                if not release_mbid:
                    return self.send_json({"error": "no release_mbid given"}, code=400)
                target = json.dumps({"folder": folder, "release_mbid": release_mbid})
                return self.send_json({"job_id": STATE["jobs"].submit("accept-release", target)})
            if p.startswith("/api/jobs/") and p.endswith("/stop"):
                return self.send_json({"stopped": STATE["jobs"].stop(int(p.split("/")[3]))})
            if p == "/api/remote":
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                remote = ((json.loads(body or b"{}") or {}).get("remote") or "").strip()
                if not remote or ":" not in remote:
                    return self.send_json({"error": "expected user@host:/path/to/vaino.db"}, code=400)
                STATE["jobs"].set_remote(remote)
                return self.send_json({"remote": remote})
            if p == "/api/remote/pull":
                # Direction one `[SPEC-DF-109]`: vainopi's own flags, resolved
                # against this library. A count of flags on recordings or
                # passages that do not exist here yet is the job's own
                # `result`, not an error.
                remote = STATE["jobs"].get_remote()
                if not remote:
                    return self.send_json({"error": "no remote configured yet"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("remote-pull", remote)})
            if p == "/api/remote/push":
                # Direction two `[SPEC-DF-108..112]`: whatever review edits
                # have accumulated locally, landed on the remote through its
                # own sqlite3 CLI. Batched -- only ever run on request.
                remote = STATE["jobs"].get_remote()
                if not remote:
                    return self.send_json({"error": "no remote configured yet"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("remote-push", remote)})
            if p == "/api/export/bundle":
                # A GUI over `export_bundle.py` `[IMPL007 Stage 4]`. `q`
                # becomes a `LIKE` pattern the same way `library()`'s own
                # search already works, not a second query language.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                q = ((json.loads(body or b"{}") or {}).get("q") or "").strip()
                if not q:
                    return self.send_json({"error": "type something to select by first"}, code=400)
                return self.send_json({"job_id": STATE["jobs"].submit("export-bundle", f"%{q}%")})
            if p == "/api/system/shutdown":
                # Refused while a job is running, not just discouraged --
                # `remote-push` briefly stops vainopi's own player mid-sync
                # `[SPEC-DF-111]`, and killing this process between that
                # `systemctl stop` and its own `systemctl start` would leave
                # the appliance silent with nothing left running to restart
                # it `[SPEC-SUI-212]`. Every job kind is refused, not only
                # that one -- the console has no way to tell "safe to
                # interrupt" apart from "not" any more cheaply than asking
                # whether one is running at all.
                active = system_status()["active_job"]
                if active:
                    return self.send_json(
                        {"error": f"{active['kind']} (job {active['job_id']}) is still running -- "
                                  f"stop it or wait for it to finish before shutting down"}, code=409)
                print(f"[system] shutdown requested via console UI (pid {os.getpid()})", flush=True)
                self.send_json({"ok": True, "message": "shutting down"})
                threading.Thread(target=_shutdown_soon, args=(self.server,), daemon=True).start()
                return
            if p == "/api/export/open-terminal":
                # An action, not a query -- POST, the same reasoning
                # `/api/jobs/:id/stop` already follows: it is not read-only
                # or idempotent to run twice, since a process starts each time.
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                d = (json.loads(body or b"{}") or {}).get("dir", "")
                return self.send_json(vaino_control.open_terminal(d))
            self.send_error(404)
        except Exception as e:
            self.send_json({"error": f"{type(e).__name__}: {e}"}, code=500)


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("--root", action="append", default=[], help="audio root; repeatable")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    if not os.path.isfile(args.db):
        print(f"no such database: {args.db}", file=sys.stderr)
        return 1
    STATE["db"] = ro(args.db)
    STATE["path"] = os.path.abspath(args.db)
    STATE["roots"] = [os.path.normpath(r) for r in args.root]
    # Beside the library, named after it, exactly as the id-check sidecar is.
    sidecar = os.path.splitext(STATE["path"])[0] + ".console.db"
    STATE["jobs"] = jobmod.Runner(STATE["path"], sidecar, roots=STATE["roots"])
    STATE["port"] = args.port
    STATE["started_at"] = time.strftime("%Y-%m-%dT%H:%M:%S")
    STATE["build"] = build_info(REPO_ROOT)

    t = totals(STATE["db"])
    print(f"library: {t['files']:,} files, {t['radio']:,} radio passages")
    if STATE["roots"]:
        print(f"roots:   {', '.join(STATE['roots'])}")
    else:
        print("roots:   none given; the folder view will have nothing to walk")
    # Loopback only. It holds no write lock today, but it reads a private
    # library and stage 3 gives it one `[SPEC-SUI-010]`.
    print(f"jobs:    {sidecar}")
    b = STATE["build"]
    if b["available"]:
        print(f"build:   {b['commit_short']} ({b['branch']}, {b['commit_date']})"
              + (f" -- {b['dirty_files']} uncommitted file(s)" if b["dirty"] else ""))
    else:
        print("build:   not a git checkout (or git not on PATH) -- /system will say so too")
    print(f"pid:     {os.getpid()}")
    print(f"console: http://127.0.0.1:{args.port}/   (library opened read-only)")
    with Server(("127.0.0.1", args.port), Handler) as httpd:
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nstopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())

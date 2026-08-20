#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The Sampo console, read-only half `[SPEC013]`, `[IMPL-SUI-040]`.

Stage 2 of [IMPL003](../docs/IMPL003-sampo-console-build.md): the views, and
nothing that writes. There is no POST route in this file and the database is
opened `mode=ro`, so the safety claim is structural rather than promised -- a
console that cannot write cannot damage a library a player is using.

That is what makes it runnable against the live database on day one. The
library is WAL `[SPEC-SUI-082]`, so readers never block the player: browsing
seven thousand files here cannot interrupt a note being played there.

Three views:

  /            library -- what is known, and how it came to be known
  /folder      what is on disk, against what the database claims
  /profile/N   one passage's whole derivation

Jobs, induct and export are stage 3 and 4 and are deliberately absent.

    python tools/console.py data/vaino_new.db --root "C:/Users/Mango Cat/Music"
"""

import argparse
import html
import json
import os
import socketserver
import sqlite3
import sys
import time
from http.server import BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs, unquote

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ingest_folder import AUDIO  # noqa: E402  -- one list of what counts as audio

WEB = os.path.join(os.path.dirname(os.path.abspath(__file__)), "console_web")

# The player is 5720. A different number because they are different services on
# the same machine, and because `[SPEC-SUI-170]` may start the player: colliding
# would make each look like the other's failure.
DEFAULT_PORT = 5730

# 71 (characteristic, class) pairs across 18 characteristics is a complete
# vector `[SPEC-SA-040]`. Measured on the four reference tracks, and the number
# the completeness tick compares against.
FULL_FLAVOR = 71

STATE = {"db": None, "roots": [], "scan": None, "scanned_at": 0}


# ---------------------------------------------------------------- database ---

def ro(db: str) -> sqlite3.Connection:
    """Read-only, and it must stay that way `[IMPL-SUI-040]`."""
    conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


def totals(conn) -> dict:
    q = lambda s: conn.execute(s).fetchone()[0]  # noqa: E731
    return {
        "files": q("SELECT count(*) FROM files"),
        "passages": q("SELECT count(*) FROM passages"),
        "radio": q("SELECT count(*) FROM passages WHERE kind='radio'"),
        "recordings": q("SELECT count(*) FROM recordings"),
        # The two facets that are Sampo's business and never the player's.
        "unidentified": q("SELECT count(*) FROM recordings WHERE mbid NOT LIKE '________-____-____-____-____________'"),
        "unchecked": q("SELECT count(*) FROM passages p WHERE p.kind='radio' AND NOT EXISTS "
                       "(SELECT 1 FROM id_checks c WHERE c.passage_id = p.passage_id)"),
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
    }


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
            if p == "/api/library":
                return self.send_json(library(
                    conn, q=(qs.get("q") or [""])[0], facet=(qs.get("facet") or [""])[0]))
            if p.startswith("/api/profile/"):
                pid = int(p.rsplit("/", 1)[-1])
                d = profile(conn, pid)
                return self.send_json(d) if d else self.send_error(404)
            if p == "/api/folder/scan":
                # Read-only and idempotent, so GET rather than the POST the
                # route sketch showed: a refresh must be harmless, and the
                # expensive half (hashing) is a job, not this.
                if STATE["scan"] is None or "refresh" in qs:
                    STATE["scan"] = scan(conn, STATE["roots"])
                    STATE["scanned_at"] = time.time()
                return self.send_json(STATE["scan"])
            self.send_error(404)
        except BrokenPipeError:
            pass
        except Exception as e:  # a failed query must report, never render empty
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
    STATE["roots"] = [os.path.normpath(r) for r in args.root]

    t = totals(STATE["db"])
    print(f"library: {t['files']:,} files, {t['radio']:,} radio passages")
    if STATE["roots"]:
        print(f"roots:   {', '.join(STATE['roots'])}")
    else:
        print("roots:   none given; the folder view will have nothing to walk")
    # Loopback only. It holds no write lock today, but it reads a private
    # library and stage 3 gives it one `[SPEC-SUI-010]`.
    print(f"console: http://127.0.0.1:{args.port}/   (read-only)")
    with Server(("127.0.0.1", args.port), Handler) as httpd:
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\nstopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())

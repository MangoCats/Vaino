#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Sync `listener_preferences` between two installations `[SPEC030]`.

`SPEC029` gave a listener a way to edit an artist's or a recording's own
`rotation`/`recovery`/`restraint` locally. This is what happens once two
installations -- a desktop and `vainopi`, say -- have each done that
independently: last-write-wins by `updated_at`, not the three-way baseline
merge `apply_changes.py` uses for review decisions. There is no baseline
to merge against here -- `listener_preferences` carries only ever "the
current tuning," so the side with the newer `updated_at` for a given
`(subject_kind, subject_id)` simply wins, and its row is copied to the
other side.

**A subject syncs only if it is a real artist/recording on *both* sides.**
Tuned on only one side but the *other* side has never heard of that
artist/recording at all -- skipped, not pushed onto a library it does not
belong to.

**No database copy.** One `ssh ... sqlite3 -json ...` round trip
(`remote_peek.run_remote_sql`) reads the whole remote table -- the same
economy `remote_flags.py` already established for `listener_flags`, a
table of the same small scale (a row exists only once a subject has
actually been tuned; MuLibPlay's own migrated data was 36% of tracks,
`[GDE-BMK-020]`, so this was never going to be a large table). What
"transfer only the changed preferences" actually costs is on the *write*
side: only the rows that differ are ever patched, in either direction.

    python tools/sync_preferences.py <local_db> user@host:/path/to/vaino.db
    python tools/sync_preferences.py <local_db> user@host:/path/to/vaino.db --commit
    python tools/sync_preferences.py <local_db> user@host:/path/to/vaino.db --commit --json

Rehearse by default: without `--commit`, nothing is written on either
side -- only counts are reported. The remote write, when there is one,
follows the identical `[PI5-LIB-010]` recipe `jobs.py::_remote_push` and
`push_file_tags.py` already use: stop the service, apply the patch,
restart it -- issued only when there is actually something to write.
The local write is advisory-only about the player, the same posture
`apply_changes.py` already takes for a local db; a best-effort
`POST /library/reload` afterward lets an already-running local Vaino pick
the change up without needing a restart.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import remote_peek as rp  # noqa: E402  -- run_remote_sql(), literal(): reused, not reinvented

MANIFEST_SQL = (
    "SELECT subject_kind, subject_id, rotation, recovery, restraint, updated_at "
    "FROM listener_preferences"
)

PATCH_TMP = "/tmp/vaino-preference-sync-patch.sql"
REMOTE_TABLE = "vaino_prefs_remote"


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


Row = tuple  # (rotation, recovery, restraint, updated_at)
Key = tuple  # (subject_kind, subject_id)


# ------------------------------------------------------------------- reading

def read_local_manifest(conn) -> dict[Key, Row]:
    """`listener_preferences`, local -- one plain `SELECT`, no I/O beyond it."""
    return {
        (r[0], r[1]): (r[2], r[3], r[4], r[5])
        for r in conn.execute(MANIFEST_SQL)
    }


def fetch_remote_manifest(remote: str) -> dict[Key, Row] | None:
    """The identical shape, over one `ssh ... sqlite3 -json` round trip.
    `None` -- not an exception -- on any transport failure, so a caller can
    report "could not reach `<remote>`" cleanly rather than crash midway.
    """
    result = rp.run_remote_sql(remote, MANIFEST_SQL)
    if not result["ok"]:
        return None
    return {
        (r["subject_kind"], r["subject_id"]): (r["rotation"], r["recovery"], r["restraint"], r["updated_at"])
        for r in result["rows"]
    }


# ------------------------------------------------------------------- deciding

def decide(local: dict[Key, Row], remote: dict[Key, Row],
           local_exists, remote_exists) -> dict[str, list]:
    """The whole decision, pure -- no I/O. `local_exists`/`remote_exists`
    are `(kind, id) -> bool` callables so this is testable against
    synthetic manifests with no database and no network at all.

    Buckets every subject in either manifest:
      * `pull`  -- remote's row is newer (or local has never tuned it, and
                   local's own library does carry that artist/recording);
                   apply locally.
      * `push`  -- the mirror case, applied to the remote.
      * `skip_missing` -- one-sided, and the *other* side's library simply
                   does not have that artist/recording at all.
      * `tie`   -- equal `updated_at`, differing values; reported, not
                   guessed at -- there is no clock left to decide with.
      * (absent from every list) -- already equal, or both sides silent.
    """
    out: dict[str, list] = {"pull": [], "push": [], "skip_missing": [], "tie": []}
    for key in sorted(set(local) | set(remote)):
        kind, subject_id = key
        loc = local.get(key)
        rem = remote.get(key)
        if loc is not None and rem is not None:
            if loc == rem:
                continue
            if loc[3] == rem[3]:
                out["tie"].append(key)
            elif loc[3] > rem[3]:
                out["push"].append(key)
            else:
                out["pull"].append(key)
            continue
        if loc is not None:
            # Tuned locally only. Pushing it needs the remote to actually
            # carry this artist/recording -- checked by the caller and
            # passed in via `remote_exists`, never inferred.
            (out["push"] if remote_exists(kind, subject_id) else out["skip_missing"]).append(key)
        else:
            (out["pull"] if local_exists(kind, subject_id) else out["skip_missing"]).append(key)
    return out


# -------------------------------------------------------------- existence

def local_exists_fn(conn):
    table = {"artist": "artists", "recording": "recordings"}

    def check(kind: str, subject_id: str) -> bool:
        row = conn.execute(
            f"SELECT 1 FROM {table[kind]} WHERE mbid = ?1", (subject_id,)).fetchone()
        return row is not None
    return check


def remote_exists_batch(remote: str, kind: str, ids: list[str]) -> set[str]:
    """Which of `ids` are a real artist/recording on the remote -- one
    `IN (...)` round trip per kind actually needed, never the whole
    catalogue. Empty `ids` costs nothing (no round trip at all)."""
    if not ids:
        return set()
    table = {"artist": "artists", "recording": "recordings"}[kind]
    in_list = ", ".join(rp.literal(i) for i in ids)
    result = rp.run_remote_sql(remote, f"SELECT mbid FROM {table} WHERE mbid IN ({in_list})")
    if not result["ok"]:
        return set()
    return {r["mbid"] for r in result["rows"]}


# ------------------------------------------------------------------- applying

def apply_local(conn, keys: list[Key], source: dict[Key, Row]) -> None:
    for kind, subject_id in keys:
        rotation, recovery, restraint, updated_at = source[(kind, subject_id)]
        conn.execute(
            "INSERT INTO listener_preferences (subject_kind, subject_id, rotation, "
            "recovery, restraint, updated_at) VALUES (?1,?2,?3,?4,?5,?6) "
            "ON CONFLICT(subject_kind, subject_id) DO UPDATE SET "
            "rotation=excluded.rotation, recovery=excluded.recovery, "
            "restraint=excluded.restraint, updated_at=excluded.updated_at",
            (kind, subject_id, rotation, recovery, restraint, updated_at))


def patch_sql_for(keys: list[Key], source: dict[Key, Row]) -> str:
    """The remote patch, fully rendered -- one `INSERT OR REPLACE` per row,
    `remote_peek.literal`-quoted, no bind parameters (none cross an `ssh`
    boundary, the same reason `remote_peek.py`'s own queries are built
    this way). A pure function so the exact SQL text is checkable without
    an ssh call.
    """
    lines = []
    for kind, subject_id in keys:
        rotation, recovery, restraint, updated_at = source[(kind, subject_id)]
        lines.append(
            "INSERT OR REPLACE INTO listener_preferences "
            "(subject_kind, subject_id, rotation, recovery, restraint, updated_at) VALUES "
            f"({rp.literal(kind)}, {rp.literal(subject_id)}, {rp.literal(rotation)}, "
            f"{rp.literal(recovery)}, {rp.literal(restraint)}, {rp.literal(updated_at)});")
    return "BEGIN IMMEDIATE;\n" + "\n".join(lines) + "\nCOMMIT;\n"


def apply_remote(remote: str, keys: list[Key], source: dict[Key, Row]) -> bool:
    """Ships and applies the patch, stopping/restarting the service the
    same `[PI5-LIB-010]` recipe `_remote_push`/`push_file_tags.py` already
    use -- only called when `keys` is non-empty; the caller never invokes
    this for an empty push set.
    """
    host, sep, path = remote.partition(":")
    if not sep or not path:
        raise ValueError(f"remote must be user@host:/path, got {remote!r}")
    sql_text = patch_sql_for(keys, source)
    local_tmp = PATCH_TMP.rsplit("/", 1)[-1]
    with open(local_tmp, "w", encoding="utf-8") as f:
        f.write(sql_text)
    try:
        r = subprocess.run(["scp", "-q", local_tmp, f"{host}:{PATCH_TMP}"], timeout=30)
        if r.returncode != 0:
            return False
        r = subprocess.run(
            ["ssh", host,
             f"sudo systemctl stop vaino && sqlite3 {path} < {PATCH_TMP} "
             f"&& sudo systemctl start vaino"],
            timeout=60)
        return r.returncode == 0
    finally:
        try:
            os.remove(local_tmp)
        except OSError:
            pass


def reload_local(port: int) -> None:
    """Best-effort: an already-running local Vaino picks the change up
    without a restart. Silently skipped if nothing answers -- the same
    "no Vaino running locally" case every other local-write tool already
    tolerates without treating it as a failure.
    """
    try:
        req = urllib.request.Request(f"http://localhost:{port}/library/reload", method="POST")
        urllib.request.urlopen(req, timeout=3)
    except (urllib.error.URLError, OSError, TimeoutError):
        pass


# ---------------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("remote", help="user@host:/path/to/vaino.db")
    ap.add_argument("--commit", action="store_true")
    ap.add_argument("--port", type=int, default=int(os.environ.get("VAINO_PORT", "5720")))
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    import sqlite3
    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True) if not args.commit \
        else sqlite3.connect(args.db, timeout=60)
    if args.commit:
        conn.execute("PRAGMA busy_timeout = 60000")

    local = read_local_manifest(conn)
    remote = fetch_remote_manifest(args.remote)
    if remote is None:
        result = {"ok": False, "error": f"could not reach {args.remote}"}
        if args.json:
            print(json.dumps(result))
        else:
            say(f"error: {result['error']}")
        return 1

    # One-sided subjects need an existence check on the *other* side --
    # collected first so the batches below cost exactly one round trip
    # per kind actually needed, never a query per subject.
    local_only = [k for k in local if k not in remote]
    remote_only = [k for k in remote if k not in local]
    remote_ids_by_kind: dict[str, list[str]] = {"artist": [], "recording": []}
    for kind, subject_id in local_only:
        remote_ids_by_kind[kind].append(subject_id)
    remote_known = {
        kind: remote_exists_batch(args.remote, kind, ids)
        for kind, ids in remote_ids_by_kind.items() if ids
    }
    remote_exists = lambda kind, sid: sid in remote_known.get(kind, ())  # noqa: E731
    local_check = local_exists_fn(conn)

    plan = decide(local, remote, local_check, remote_exists)

    # Fixed field names regardless of `--commit` -- `committed` says whether
    # `pull`/`push` already happened or are only a preview, so a caller
    # (the console's own job result) never has to branch on which keys a
    # response happens to carry.
    result = {
        "ok": True,
        "committed": args.commit,
        "pull": len(plan["pull"]),
        "push": len(plan["push"]),
        "skipped_missing": len(plan["skip_missing"]),
        "ties": len(plan["tie"]),
    }

    if args.commit:
        if plan["pull"]:
            conn.execute("BEGIN IMMEDIATE")
            apply_local(conn, plan["pull"], remote)
            conn.commit()
            reload_local(args.port)
        if plan["push"]:
            ok = apply_remote(args.remote, plan["push"], local)
            if not ok:
                result["ok"] = False
                result["error"] = "remote push failed -- see stderr above"

    conn.close()

    if args.json:
        print(json.dumps(result))
    else:
        say(f"{'would ' if not args.commit else ''}pull {len(plan['pull'])}, "
            f"{'would ' if not args.commit else ''}push {len(plan['push'])}, "
            f"{len(plan['skip_missing'])} skipped (missing on the other side), "
            f"{len(plan['tie'])} tied (equal timestamp, differing values)")
        if not args.commit and (plan["pull"] or plan["push"]):
            say("dry run -- nothing written. Re-run with --commit to apply.")
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())

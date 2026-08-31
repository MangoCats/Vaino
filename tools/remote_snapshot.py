#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Build a tiny local snapshot of exactly what a `changes.json` needs to
compare against on a remote installation, without copying its database
`[SPEC-DF-120]`.

`remote-push`'s own read was measured at ~1.16 GB / over an hour
`[SPEC-DF-114]` -- the identical cost `[SPEC-DF-119]` already eliminated for
`remote-pull`, left standing here because "converting it to a batch of
targeted reads... remains future work, not built here" `[SPEC-DF-119]`'s own
words. This is that work: one `remote_peek.py`-style `ssh ... sqlite3 -json`
round trip per change (two for a boundary edit whose anchor span has already
moved once), reconstructed into a disposable SQLite file just large enough
for `tools/apply_changes.py`'s existing merge logic to run against
unmodified -- same three-way merge, same `--emit-sql` capture, same
correctness guarantee, without the whole-file copy in between.

    python tools/remote_snapshot.py pi@vainopi:/srv/library/vaino.db changes.json -o snapshot.db
    python tools/apply_changes.py snapshot.db changes.json --commit --emit-sql patch.sql --clear-flags

Deliberately narrow: it answers only what `apply_changes.py`'s own
`resolve_passage()`/`current_recording()`/`current_boundary()`/
`current_artist()`/`history_for()` read for the exact identities named in
`changes.json` -- not a general-purpose remote query tool, and not a partial
replication scheme for anything else.
"""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import remote_peek  # noqa: E402  -- literal()/run_remote_sql(), not duplicated
import apply_changes  # noqa: E402  -- ensure_review_tables(), the one true schema

SOURCE = "remote:snapshot"

# Enough for `apply_changes.py`'s own reads and writes to run unmodified --
# not the real schema's constraints (no FKs, no UNIQUE indexes): a disposable
# comparison copy has no need to enforce anything a real library already
# does, and `apply_id_review`/`apply_boundary_review` never depend on one.
# `passages` omits the four fade columns on purpose -- see `_boundary_row()`.
SCHEMA = """
CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT NOT NULL UNIQUE);
CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER, kind TEXT,
    start_ms INTEGER, end_ms INTEGER, lead_in_ms INTEGER, lead_out_ms INTEGER, gain_db REAL,
    boundary_src TEXT);
CREATE TABLE passage_recordings (passage_id INTEGER, mbid TEXT, weight REAL, source TEXT);
CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT, length_ms INTEGER, source TEXT);
CREATE TABLE artists (mbid TEXT PRIMARY KEY, name TEXT, source TEXT);
CREATE TABLE recording_artists (mbid TEXT, artist_mbid TEXT, weight REAL, source TEXT);
CREATE TABLE lowlevel_cache (audio_md5 TEXT, start_ms INTEGER, end_ms INTEGER);
"""


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def _passage_row_sql(anchor: dict, extra_cols: str) -> str:
    lit = remote_peek.literal
    return (
        f"SELECT p.passage_id, p.start_ms, p.end_ms{extra_cols} "
        "FROM passages p JOIN files f ON f.file_id = p.file_id "
        f"WHERE f.audio_md5 = {lit(anchor['audio_md5'])} "
        f"AND p.kind = {lit(anchor['passage_kind'])} "
        f"AND p.start_ms = {lit(anchor['start_ms'])} "
        f"AND p.end_ms = {lit(anchor['end_ms'])}"
    )


def _id_review_row(remote: str, change: dict, timeout: float) -> dict:
    """One round trip: the anchor's own `passage_id` and current recording,
    whether the *target* recording already exists remotely (the one case
    `apply_id_review` cannot safely guess past -- a plain, non-`OR IGNORE`
    `INSERT INTO recordings` that would otherwise collide with a row already
    there under a target mbid this installation happens to know from
    elsewhere), and this passage's own applied-review history, if any.
    """
    lit = remote_peek.literal
    sql = _passage_row_sql(
        change["anchor"],
        ", (SELECT pr.mbid FROM passage_recordings pr WHERE pr.passage_id = p.passage_id "
        "    ORDER BY pr.weight DESC, pr.mbid LIMIT 1) AS current_mbid, "
        f"({_exists('recordings', 'mbid', change['target']['mbid'])}) AS target_recording_exists, "
        "(SELECT decided_at FROM id_reviews WHERE passage_id = p.passage_id "
        "    AND applied_at IS NOT NULL) AS hist_decided_at, "
        "(SELECT origin FROM id_reviews WHERE passage_id = p.passage_id "
        "    AND applied_at IS NOT NULL) AS hist_origin")
    result = remote_peek.run_remote_sql(remote, sql, timeout=timeout)
    if not result["ok"]:
        raise RuntimeError(result["error"])
    rows = result["rows"]
    return rows[0] if rows else {}


def _exists(table: str, column: str, value) -> str:
    return f"SELECT 1 FROM {table} WHERE {column} = {remote_peek.literal(value)}"


def _boundary_row(remote: str, change: dict, timeout: float) -> tuple[dict, bool]:
    """Two attempts, the same fallback `remote_peek.py`'s own `peek()`
    already uses for a lone anchor: with the four fade columns, then without,
    on exactly the failure an unmigrated `passages` (no
    `tools/add_fade_columns.py` run yet) produces. Returns `(row, has_fade)`
    -- `has_fade` decides whether the snapshot's own `passages` row carries
    real fetched fade values or omits the columns entirely and leaves
    `apply_changes.py`'s own `ensure_passages_fade_columns()` to default them,
    the identical path a real unmigrated remote's compare copy already takes.
    Falls back across *both* the anchor's own span and the boundary edit's
    target span `[SPEC-DF-103]`'s `resolve_boundary_passage()` already tries
    locally -- a second commit of the same edit can no longer find the
    passage at its pre-edit span once the first commit already moved it.
    """
    anchor = change["anchor"]
    target_anchor = {**anchor, "start_ms": change["target"]["start_ms"],
                      "end_ms": change["target"]["end_ms"]}
    fade_cols = (", p.lead_in_ms, p.lead_out_ms, p.gain_db, "
                 "p.fade_in_ms, p.fade_out_ms, p.fade_in_curve, p.fade_out_curve, "
                 "(SELECT decided_at FROM boundary_reviews WHERE passage_id = p.passage_id "
                 "    AND applied_at IS NOT NULL) AS hist_decided_at, "
                 "(SELECT origin FROM boundary_reviews WHERE passage_id = p.passage_id "
                 "    AND applied_at IS NOT NULL) AS hist_origin")
    no_fade_cols = (", p.lead_in_ms, p.lead_out_ms, p.gain_db, "
                    "(SELECT decided_at FROM boundary_reviews WHERE passage_id = p.passage_id "
                    "    AND applied_at IS NOT NULL) AS hist_decided_at, "
                    "(SELECT origin FROM boundary_reviews WHERE passage_id = p.passage_id "
                    "    AND applied_at IS NOT NULL) AS hist_origin")

    def attempt(a: dict, cols: str):
        return remote_peek.run_remote_sql(remote, _passage_row_sql(a, cols), timeout=timeout)

    result = attempt(anchor, fade_cols)
    has_fade = True
    if not result["ok"] and "fade_in_ms" in (result.get("error") or ""):
        result = attempt(anchor, no_fade_cols)
        has_fade = False
    if not result["ok"]:
        raise RuntimeError(result["error"])
    if result["rows"]:
        return result["rows"][0], has_fade

    result2 = attempt(target_anchor, fade_cols if has_fade else no_fade_cols)
    if not result2["ok"]:
        raise RuntimeError(result2["error"])
    return (result2["rows"][0] if result2["rows"] else {}), has_fade


def _artist_review_row(remote: str, change: dict, timeout: float) -> dict:
    """One round trip: whether the recording is known here at all, its
    current credited artist, whether the *target* artist already exists
    (`apply_artist_review`'s own `artists` write is `INSERT OR IGNORE`, so
    unlike `id_review` this is not load-bearing for correctness -- fetched
    anyway so the snapshot's `artists` table matches reality rather than
    always looking freshly created), and this recording's applied-review
    history.
    """
    rmbid = change["anchor"]["recording_mbid"]
    lit = remote_peek.literal
    sql = (
        f"SELECT ({_exists('recordings', 'mbid', rmbid)}) AS has_recording, "
        "(SELECT a.mbid FROM recording_artists ra JOIN artists a ON a.mbid = ra.artist_mbid "
        f"    WHERE ra.mbid = {lit(rmbid)} ORDER BY ra.weight DESC, a.name LIMIT 1) AS current_artist_mbid, "
        "(SELECT a.name FROM recording_artists ra JOIN artists a ON a.mbid = ra.artist_mbid "
        f"    WHERE ra.mbid = {lit(rmbid)} ORDER BY ra.weight DESC, a.name LIMIT 1) AS current_artist_name, "
        f"({_exists('artists', 'mbid', change['target']['artist_mbid'])}) AS target_artist_exists, "
        f"(SELECT decided_at FROM artist_reviews WHERE recording_mbid = {lit(rmbid)} "
        "    AND applied_at IS NOT NULL) AS hist_decided_at, "
        f"(SELECT origin FROM artist_reviews WHERE recording_mbid = {lit(rmbid)} "
        "    AND applied_at IS NOT NULL) AS hist_origin")
    result = remote_peek.run_remote_sql(remote, sql, timeout=timeout)
    if not result["ok"]:
        raise RuntimeError(result["error"])
    rows = result["rows"]
    return rows[0] if rows else {}


def fetch(remote: str, changes: list[dict], timeout: float = remote_peek.TOTAL_TIMEOUT) -> dict:
    """One targeted answer per change -- `{index: {"row": {...}, "has_fade": bool}}`,
    `has_fade` only meaningful for `boundary_review`. A change whose kind is
    not one of the three this tool knows is left out; `build()` treats an
    absent entry as "found nothing", matching `resolve_passage()`'s own
    `None` for an anchor that plainly does not resolve.
    """
    out = {}
    for i, change in enumerate(changes):
        kind = change["kind"]
        if kind == "id_review":
            out[i] = {"row": _id_review_row(remote, change, timeout), "has_fade": None}
        elif kind == "boundary_review":
            row, has_fade = _boundary_row(remote, change, timeout)
            out[i] = {"row": row, "has_fade": has_fade}
        elif kind == "artist_review":
            out[i] = {"row": _artist_review_row(remote, change, timeout), "has_fade": None}
        # An unknown kind is left for `apply_changes.py` itself to report,
        # exactly as it already does for one it reads locally.
    return out


def build(changes: list[dict], fetched: dict, out_path: str) -> None:
    """The disposable comparison copy `apply_changes.py` reads unmodified.

    One `files`/`passages` row per resolved anchor (a boundary edit gets a
    second, for the target-span fallback, when that is where it was found),
    keyed by the *real* remote `passage_id` -- the literal SQL `--emit-sql`
    captures references that id directly, so it must be the same id the real
    remote would recognise, not a locally-invented one.
    """
    if os.path.exists(out_path):
        os.remove(out_path)
    conn = sqlite3.connect(out_path)
    conn.executescript(SCHEMA)
    # The one true review-table schema, not a narrower stand-in `[SPEC-DF-120]`:
    # a history row inserted here has to satisfy the exact columns
    # `apply_changes.py`'s own later run will find already there (and try to
    # `ALTER TABLE ADD COLUMN` no-ops against) -- reused, not duplicated, so
    # the two can never drift apart.
    apply_changes.ensure_review_tables(conn)
    # `passages` starts without the four fade columns on purpose -- a
    # boundary change whose remote lookup found them absent (an unmigrated
    # `passages`) must stay that way here too, so `apply_changes.py`'s own
    # `ensure_passages_fade_columns()` defaults them exactly as it already
    # does for a real unmigrated remote's compare copy. Added only when at
    # least one fetched row actually carries real values to put in them.
    if any(e["has_fade"] for e in fetched.values() if e["has_fade"] is not None):
        for column in ("fade_in_ms INTEGER", "fade_out_ms INTEGER",
                        "fade_in_curve TEXT", "fade_out_curve TEXT"):
            conn.execute(f"ALTER TABLE passages ADD COLUMN {column}")

    file_ids: dict[str, int] = {}

    def file_id_for(audio_md5: str) -> int:
        if audio_md5 not in file_ids:
            fid = len(file_ids) + 1
            conn.execute("INSERT INTO files VALUES (?1, ?2)", (fid, audio_md5))
            file_ids[audio_md5] = fid
        return file_ids[audio_md5]

    for i, change in enumerate(changes):
        kind = change["kind"]
        entry = fetched.get(i)
        if entry is None:
            continue
        row = entry["row"]
        pid = row.get("passage_id")

        if kind == "id_review":
            if pid is not None:
                conn.execute(
                    "INSERT OR IGNORE INTO passages (passage_id, file_id, kind, start_ms, end_ms) "
                    "VALUES (?1,?2,?3,?4,?5)",
                    (pid, file_id_for(change["anchor"]["audio_md5"]),
                     change["anchor"]["passage_kind"], row["start_ms"], row["end_ms"]))
                if row.get("current_mbid") is not None:
                    conn.execute(
                        "INSERT INTO passage_recordings (passage_id, mbid, weight, source) "
                        "VALUES (?1,?2,1.0,?3)", (pid, row["current_mbid"], SOURCE))
            if row.get("target_recording_exists"):
                conn.execute(
                    "INSERT OR IGNORE INTO recordings (mbid, title, source) VALUES (?1,?2,?3)",
                    (change["target"]["mbid"], change["target"].get("title") or "?", SOURCE))
            if row.get("hist_decided_at") is not None:
                # `decision`/`chosen_mbid` are NOT NULL but otherwise
                # meaningless here -- `history_for()` reads only
                # `decided_at`/`origin`, and `apply_id_review`'s own
                # `ON CONFLICT DO UPDATE` overwrites both the moment a
                # matching change actually applies.
                conn.execute(
                    "INSERT INTO id_reviews (passage_id, decision, chosen_mbid, decided_at, "
                    "applied_at, origin) VALUES (?1,'reassigned',?2,?3,'x',?4)",
                    (pid, row.get("current_mbid"), row["hist_decided_at"], row.get("hist_origin")))

        elif kind == "boundary_review":
            if pid is not None:
                cols = ["passage_id", "file_id", "kind", "start_ms", "end_ms",
                        "lead_in_ms", "lead_out_ms", "gain_db"]
                vals = [pid, file_id_for(change["anchor"]["audio_md5"]),
                        change["anchor"]["passage_kind"], row["start_ms"], row["end_ms"],
                        row.get("lead_in_ms"), row.get("lead_out_ms"), row.get("gain_db")]
                if entry["has_fade"]:
                    for c in ("fade_in_ms", "fade_out_ms", "fade_in_curve", "fade_out_curve"):
                        cols.append(c)
                        vals.append(row.get(c))
                placeholders = ",".join(f"?{n}" for n in range(1, len(cols) + 1))
                conn.execute(f"INSERT OR IGNORE INTO passages ({','.join(cols)}) "
                             f"VALUES ({placeholders})", vals)
                if row.get("hist_decided_at") is not None:
                    conn.execute(
                        "INSERT INTO boundary_reviews (passage_id, start_ms, end_ms, decided_at, "
                        "applied_at, origin) VALUES (?1,?2,?3,?4,'x',?5)",
                        (pid, row["start_ms"], row["end_ms"], row["hist_decided_at"], row.get("hist_origin")))

        elif kind == "artist_review":
            rmbid = change["anchor"]["recording_mbid"]
            if row.get("has_recording"):
                conn.execute(
                    "INSERT OR IGNORE INTO recordings (mbid, title, source) VALUES (?1,?2,?3)",
                    (rmbid, "?", SOURCE))
            if row.get("current_artist_mbid") is not None:
                conn.execute(
                    "INSERT INTO recording_artists (mbid, artist_mbid, weight, source) "
                    "VALUES (?1,?2,1.0,?3)", (rmbid, row["current_artist_mbid"], SOURCE))
                conn.execute(
                    "INSERT OR IGNORE INTO artists (mbid, name, source) VALUES (?1,?2,?3)",
                    (row["current_artist_mbid"], row.get("current_artist_name") or "?", SOURCE))
            if row.get("target_artist_exists"):
                conn.execute(
                    "INSERT OR IGNORE INTO artists (mbid, name, source) VALUES (?1,?2,?3)",
                    (change["target"]["artist_mbid"], change["target"].get("artist_name") or "?", SOURCE))
            if row.get("hist_decided_at") is not None:
                # `artist_mbid`/`artist_name` are NOT NULL but otherwise
                # meaningless here, the same reasoning as `id_reviews`'
                # `decision` above -- `history_for()` never reads them.
                conn.execute(
                    "INSERT INTO artist_reviews (recording_mbid, artist_mbid, artist_name, "
                    "decided_at, applied_at, origin) VALUES (?1,?2,?3,?4,'x',?5)",
                    (rmbid, row.get("current_artist_mbid") or "?", row.get("current_artist_name") or "?",
                     row["hist_decided_at"], row.get("hist_origin")))

    conn.commit()
    conn.close()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("remote", help="user@host:/path/to/vaino.db")
    ap.add_argument("changes", help="changes.json from export_changes.py")
    ap.add_argument("-o", "--out", required=True)
    ap.add_argument("--timeout", type=float, default=remote_peek.TOTAL_TIMEOUT)
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    with open(args.changes, encoding="utf-8") as f:
        changes = json.load(f).get("changes", [])

    try:
        fetched = fetch(args.remote, changes, timeout=args.timeout)
    except RuntimeError as e:
        if args.json:
            say(json.dumps({"ok": False, "error": str(e)}))
        else:
            say(f"could not read {args.remote}: {e}")
        return 1

    build(changes, fetched, args.out)
    resolved = sum(1 for e in fetched.values() if e["row"].get("passage_id") is not None
                   or e["row"].get("has_recording"))
    if args.json:
        say(json.dumps({"ok": True, "changes": len(changes), "resolved": resolved, "out": args.out}))
    else:
        say(f"{len(changes)} change(s), {resolved} resolved against {args.remote} -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

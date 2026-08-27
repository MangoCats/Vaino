#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Apply synced edits to this installation `[SPEC006 §9]`.

Reads a `changes.json` from `tools/export_changes.py` and, for each record,
compares this installation's *current* value at the same portable identity
against the record's baseline and target -- the same three-way merge git
uses:

  * current == baseline   -> nothing has changed here since the source's
                              edit. FAST-FORWARD, applied automatically.
  * current == target      -> the same correction already landed. NO-OP.
  * current == neither      -> this installation changed it independently
                              since the shared baseline. CONFLICT: refused
                              until a person names a side.

Rehearse by default, like every other tool here. Run with the player
stopped, the same posture `[PI5-LIB-010]` used for the one real library
swap -- this writes to `passages`, `passage_recordings` and
`recording_artists` directly, with no dependency on Vaino being built with
`sampo-support` at all.

    python tools/apply_changes.py /srv/library/vaino.db changes.json
    python tools/apply_changes.py /srv/library/vaino.db changes.json --commit
    python tools/apply_changes.py /srv/library/vaino.db changes.json --resolve 3=ours
    python tools/apply_changes.py /srv/library/vaino.db changes.json --resolve 3=theirs --commit

`ours` keeps what is already on this machine, discarding the incoming
change. `theirs` applies the incoming change, overwriting what diverged.
Both still need `--commit` to actually write; without it, a `--resolve` run
only previews what that resolution would do.
"""

import argparse
import json
import socket
import sqlite3
import sys

SOURCE_PREFIX = "synced"


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def ensure_review_tables(conn: sqlite3.Connection) -> None:
    """This tool has no dependency on Vaino ever having run against the
    target file at all -- it writes to the SQLite path directly, the same as
    `apply_reviews.py`, and needs no `sampo-support` build on the target.
    A library a `sampo-support` Vaino has genuinely never touched has none
    of these three tables yet; one that has but predates `[SPEC-DF-104]`'s
    `origin` column is missing only that. Both gaps are closed here, the
    schema exactly matching what `PlayerStore::open`'s own
    `ensure_review_table` and its two siblings create.
    """
    conn.execute(
        "CREATE TABLE IF NOT EXISTS id_reviews (passage_id INTEGER PRIMARY KEY, "
        "decision TEXT NOT NULL, chosen_mbid TEXT, decided_at TEXT NOT NULL, "
        "chosen_release_mbid TEXT, previous_mbid TEXT, applied_at TEXT)")
    conn.execute(
        "CREATE TABLE IF NOT EXISTS boundary_reviews (passage_id INTEGER PRIMARY KEY, "
        "start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, lead_in_ms INTEGER, "
        "lead_out_ms INTEGER, gain_db REAL, decided_at TEXT NOT NULL, applied_at TEXT)")
    conn.execute(
        "CREATE TABLE IF NOT EXISTS artist_reviews (recording_mbid TEXT PRIMARY KEY, "
        "passage_id INTEGER, artist_mbid TEXT NOT NULL, artist_name TEXT NOT NULL, "
        "previous_artist_mbid TEXT, previous_artist_name TEXT, previous_artist_weight REAL, "
        "decided_at TEXT NOT NULL, applied_at TEXT)")
    for table, column in [
        ("boundary_reviews", "audio_md5 TEXT"),
        ("boundary_reviews", "orig_kind TEXT"),
        ("boundary_reviews", "orig_start_ms INTEGER"),
        ("boundary_reviews", "orig_end_ms INTEGER"),
        ("boundary_reviews", "orig_lead_in_ms INTEGER"),
        ("boundary_reviews", "orig_lead_out_ms INTEGER"),
        ("boundary_reviews", "orig_gain_db REAL"),
        ("id_reviews", "origin TEXT"),
        ("boundary_reviews", "origin TEXT"),
        ("artist_reviews", "origin TEXT"),
    ]:
        try:
            conn.execute(f"ALTER TABLE {table} ADD COLUMN {column}")
        except sqlite3.OperationalError:
            pass  # already has it


def resolve_passage(conn: sqlite3.Connection, anchor: dict):
    row = conn.execute(
        """SELECT p.passage_id FROM passages p JOIN files f ON f.file_id = p.file_id
            WHERE f.audio_md5 = ?1 AND p.kind = ?2 AND p.start_ms = ?3 AND p.end_ms = ?4""",
        (anchor["audio_md5"], anchor["passage_kind"], anchor["start_ms"], anchor["end_ms"]),
    ).fetchone()
    return row[0] if row else None


def resolve_boundary_passage(conn: sqlite3.Connection, change: dict):
    """`resolve_passage` against the anchor (the pre-edit span), falling back
    to the *target* span if that finds nothing.

    A boundary edit changes the very field its own anchor is keyed on, so a
    second run against a receiver that already landed it once can no longer
    find the passage at its pre-edit span -- it now sits at the target span,
    because the first run put it there. Without this fallback, re-sending
    the same `changes.json` (`[SPEC-DF-105]`'s whole idempotency argument)
    would report "not present" on the second run instead of "already in
    sync", for the one kind of change that moves its own identity.
    """
    found = resolve_passage(conn, change["anchor"])
    if found is not None:
        return found
    target_anchor = {**change["anchor"], "start_ms": change["target"]["start_ms"],
                      "end_ms": change["target"]["end_ms"]}
    return resolve_passage(conn, target_anchor)


def current_recording(conn: sqlite3.Connection, passage_id: int):
    row = conn.execute(
        "SELECT mbid FROM passage_recordings WHERE passage_id=?1 ORDER BY weight DESC, mbid LIMIT 1",
        (passage_id,)).fetchone()
    return row[0] if row else None


def current_boundary(conn: sqlite3.Connection, passage_id: int):
    row = conn.execute(
        "SELECT start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db FROM passages WHERE passage_id=?1",
        (passage_id,)).fetchone()
    if not row:
        return None
    keys = ["start_ms", "end_ms", "lead_in_ms", "lead_out_ms", "gain_db"]
    return dict(zip(keys, row))


def current_artist(conn: sqlite3.Connection, recording_mbid: str):
    row = conn.execute(
        """SELECT a.mbid, a.name FROM recording_artists ra JOIN artists a ON a.mbid = ra.artist_mbid
            WHERE ra.mbid=?1 ORDER BY ra.weight DESC, a.name LIMIT 1""", (recording_mbid,)).fetchone()
    return {"artist_mbid": row[0], "artist_name": row[1]} if row else {"artist_mbid": None, "artist_name": None}


def classify(current, baseline: dict, target: dict, keys: list) -> str:
    """`[SPEC-DF-101]`'s three-way merge, generic over which fields matter."""
    if current is None:
        return "missing"
    cur = tuple(current.get(k) for k in keys)
    if cur == tuple(target.get(k) for k in keys):
        return "noop"
    if cur == tuple(baseline.get(k) for k in keys):
        return "fastforward"
    return "conflict"


def history_for(conn: sqlite3.Connection, table: str, where: str, params: tuple):
    """The target's own recorded reason its current value is what it is, if
    any `[SPEC-DF-106]` -- distinct from "no correction history recorded
    here", which is itself informative: it means whatever is here came from
    ordinary ingest, not a considered decision.
    """
    row = conn.execute(
        f"SELECT decided_at, origin FROM {table} WHERE {where} AND applied_at IS NOT NULL",
        params).fetchone()
    if not row:
        return None
    decided_at, origin = row
    return {"decided_at": decided_at, "origin": origin or "here"}


def report_conflict(n: int, kind: str, subject: str, change: dict, current_desc: str, history) -> None:
    say(f"\n#{n} CONFLICT  {kind}: {subject}")
    say(f"   incoming ({change['origin']}, decided {change['decided_at']}): {describe(change['target'], kind)}")
    say(f"   baseline (what {change['origin']} saw before its edit):    {describe(change['baseline'], kind)}")
    say(f"   here now:                                                 {current_desc}")
    if history:
        say(f"     decided here {history['decided_at']} ({history['origin']}) -- diverged independently")
    else:
        say("     no correction history recorded here -- from ordinary ingest")
    say(f"   --resolve {n}=ours    keep what is here, discard the incoming change")
    say(f"   --resolve {n}=theirs  apply the incoming change, overwriting what is here")
    say(f"   --resolve {n}=skip    leave it for next time")


def describe(values: dict, kind: str) -> str:
    if kind == "id_review":
        return values.get("mbid") or "(none)"
    if kind == "artist_review":
        return values.get("artist_name") or values.get("artist_mbid") or "(none)"
    if kind == "boundary_review":
        return (f"{values.get('start_ms')}-{values.get('end_ms')}, "
                f"lead-in {values.get('lead_in_ms')}, lead-out {values.get('lead_out_ms')}, "
                f"gain {values.get('gain_db')}")
    return str(values)


def apply_id_review(conn: sqlite3.Connection, passage_id: int, change: dict) -> None:
    target = change["target"]
    mbid = target["mbid"]
    if not conn.execute("SELECT 1 FROM recordings WHERE mbid=?1", (mbid,)).fetchone():
        if not target.get("title"):
            raise ValueError(f"recording {mbid} is not known here and the change carries no title "
                              f"to create it with -- bundle this music here first")
        conn.execute(
            "INSERT INTO recordings (mbid, title, source) VALUES (?1, ?2, ?3)",
            (mbid, target["title"], f"{SOURCE_PREFIX}:{change['origin']}"))
        for a in target.get("artists") or []:
            if not a.get("name"):
                continue
            conn.execute(
                "INSERT OR IGNORE INTO artists (mbid, name, source) VALUES (?1, ?2, ?3)",
                (a["mbid"], a["name"], f"{SOURCE_PREFIX}:{change['origin']}"))
            conn.execute(
                "INSERT OR IGNORE INTO recording_artists (mbid, artist_mbid, weight, source) "
                "VALUES (?1, ?2, 1.0, ?3)", (mbid, a["mbid"], f"{SOURCE_PREFIX}:{change['origin']}"))
    conn.execute("DELETE FROM passage_recordings WHERE passage_id=?1", (passage_id,))
    conn.execute(
        "INSERT INTO passage_recordings (passage_id, mbid, weight, source) VALUES (?1, ?2, 1.0, ?3)",
        (passage_id, mbid, f"{SOURCE_PREFIX}:{change['origin']}"))
    conn.execute(
        """INSERT INTO id_reviews (passage_id, decision, chosen_mbid, previous_mbid, decided_at,
                                    applied_at, origin)
           VALUES (?1, 'reassigned', ?2, ?3, ?4, datetime('now'), ?5)
           ON CONFLICT(passage_id) DO UPDATE SET
               decision='reassigned', chosen_mbid=excluded.chosen_mbid,
               previous_mbid=excluded.previous_mbid, decided_at=excluded.decided_at,
               applied_at=excluded.applied_at, origin=excluded.origin""",
        (passage_id, mbid, change["baseline"].get("mbid"), change["decided_at"], change["origin"]))


def apply_boundary_review(conn: sqlite3.Connection, passage_id: int, change: dict) -> None:
    t = change["target"]
    conn.execute(
        """UPDATE passages SET start_ms=?1, end_ms=?2, lead_in_ms=?3, lead_out_ms=?4,
                                gain_db=?5, boundary_src='manual' WHERE passage_id=?6""",
        (t["start_ms"], t["end_ms"], t["lead_in_ms"], t["lead_out_ms"], t["gain_db"], passage_id))
    anchor = change["anchor"]
    if (t["start_ms"], t["end_ms"]) != (anchor["start_ms"], anchor["end_ms"]):
        audio_md5 = anchor["audio_md5"]
        still_used = conn.execute(
            """SELECT 1 FROM passages p2 JOIN files f2 ON f2.file_id=p2.file_id
                WHERE f2.audio_md5=?1 AND p2.start_ms=?2 AND p2.end_ms=?3 AND p2.passage_id != ?4""",
            (audio_md5, anchor["start_ms"], anchor["end_ms"], passage_id)).fetchone()
        if not still_used:
            conn.execute(
                "DELETE FROM lowlevel_cache WHERE audio_md5=?1 AND start_ms=?2 AND end_ms=?3",
                (audio_md5, anchor["start_ms"], anchor["end_ms"]))
    conn.execute(
        """INSERT INTO boundary_reviews
               (passage_id, start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db,
                audio_md5, orig_kind, orig_start_ms, orig_end_ms, orig_lead_in_ms,
                orig_lead_out_ms, orig_gain_db, decided_at, applied_at, origin)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,datetime('now'),?15)
           ON CONFLICT(passage_id) DO UPDATE SET
               start_ms=excluded.start_ms, end_ms=excluded.end_ms,
               lead_in_ms=excluded.lead_in_ms, lead_out_ms=excluded.lead_out_ms,
               gain_db=excluded.gain_db, decided_at=excluded.decided_at,
               applied_at=excluded.applied_at, origin=excluded.origin""",
        (passage_id, t["start_ms"], t["end_ms"], t["lead_in_ms"], t["lead_out_ms"], t["gain_db"],
         anchor["audio_md5"], anchor["passage_kind"], anchor["start_ms"], anchor["end_ms"],
         change["baseline"]["lead_in_ms"], change["baseline"]["lead_out_ms"], change["baseline"]["gain_db"],
         change["decided_at"], change["origin"]))


def apply_artist_review(conn: sqlite3.Connection, recording_mbid: str, change: dict) -> None:
    # Keyed by `recording_mbid` -- a synced correction has no originating
    # passage on this machine at all, which is exactly why the table is not
    # keyed by one `[SPEC-DF-103]`.
    t = change["target"]
    conn.execute(
        "INSERT OR IGNORE INTO artists (mbid, name, source) VALUES (?1, ?2, ?3)",
        (t["artist_mbid"], t["artist_name"], f"{SOURCE_PREFIX}:{change['origin']}"))
    conn.execute("DELETE FROM recording_artists WHERE mbid=?1", (recording_mbid,))
    conn.execute(
        "INSERT INTO recording_artists (mbid, artist_mbid, weight, source) VALUES (?1, ?2, 1.0, ?3)",
        (recording_mbid, t["artist_mbid"], f"{SOURCE_PREFIX}:{change['origin']}"))
    b = change["baseline"]
    conn.execute(
        """INSERT INTO artist_reviews
               (recording_mbid, artist_mbid, artist_name,
                previous_artist_mbid, previous_artist_name, previous_artist_weight,
                decided_at, applied_at, origin)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'), ?8)
           ON CONFLICT(recording_mbid) DO UPDATE SET
               artist_mbid=excluded.artist_mbid, artist_name=excluded.artist_name,
               previous_artist_mbid=excluded.previous_artist_mbid,
               previous_artist_name=excluded.previous_artist_name,
               previous_artist_weight=excluded.previous_artist_weight,
               decided_at=excluded.decided_at, applied_at=excluded.applied_at,
               origin=excluded.origin""",
        (recording_mbid, t["artist_mbid"], t["artist_name"],
         b.get("artist_mbid"), b.get("artist_name"), b.get("weight"),
         change["decided_at"], change["origin"]))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("changes", help="changes.json from export_changes.py")
    ap.add_argument("--commit", action="store_true")
    ap.add_argument("--resolve", action="append", default=[], metavar="N=ours|theirs|skip")
    args = ap.parse_args()

    resolutions = {}
    for spec in args.resolve:
        n, _, verdict = spec.partition("=")
        if verdict not in ("ours", "theirs", "skip"):
            say(f"--resolve {spec!r}: verdict must be ours, theirs or skip")
            return 2
        resolutions[int(n)] = verdict

    with open(args.changes, encoding="utf-8") as f:
        doc = json.load(f)
    changes = doc.get("changes", [])

    conn = sqlite3.connect(args.db, timeout=60)
    conn.execute("PRAGMA busy_timeout = 60000")
    conn.execute("PRAGMA foreign_keys = ON")
    # Schema readiness, not a decision -- run even in rehearsal, the same as
    # `ensure_review_table` and its siblings run unconditionally on every
    # `PlayerStore::open` regardless of whether anything gets written after.
    ensure_review_tables(conn)
    conn.commit()

    say(f"{len(changes)} change(s) in {args.changes}")
    counts = {"fastforward": 0, "noop": 0, "conflict": 0, "missing": 0, "resolved": 0, "error": 0}
    if args.commit:
        conn.execute("BEGIN IMMEDIATE")

    for i, change in enumerate(changes, 1):
        kind = change["kind"]
        anchor = change["anchor"]

        if kind == "id_review":
            passage_id = resolve_passage(conn, anchor)
            current = {"mbid": current_recording(conn, passage_id)} if passage_id else None
            keys = ["mbid"]
            subject = anchor["audio_md5"][:12] + "…"
            history = history_for(conn, "id_reviews", "passage_id=?1", (passage_id,)) if passage_id else None
        elif kind == "boundary_review":
            passage_id = resolve_boundary_passage(conn, change)
            current = current_boundary(conn, passage_id) if passage_id else None
            keys = ["start_ms", "end_ms", "lead_in_ms", "lead_out_ms", "gain_db"]
            subject = anchor["audio_md5"][:12] + "…"
            history = history_for(conn, "boundary_reviews", "passage_id=?1", (passage_id,)) if passage_id else None
        elif kind == "artist_review":
            recording_mbid = anchor["recording_mbid"]
            has_recording = conn.execute(
                "SELECT 1 FROM recordings WHERE mbid=?1", (recording_mbid,)).fetchone()
            current = current_artist(conn, recording_mbid) if has_recording else None
            keys = ["artist_mbid"]
            subject = f"recording {recording_mbid}"
            history = history_for(conn, "artist_reviews", "recording_mbid=?1", (recording_mbid,))
        else:
            say(f"#{i}: unknown change kind {kind!r}, skipped")
            counts["error"] += 1
            continue

        verdict = classify(current, change["baseline"], change["target"], keys)
        if verdict == "missing":
            say(f"#{i} {kind}: not present here ({subject}) -- nothing to resolve this against")
            counts["missing"] += 1
            continue
        if verdict == "noop":
            counts["noop"] += 1
            continue
        if verdict == "conflict":
            resolved = resolutions.get(i)
            if resolved == "skip" or resolved is None:
                report_conflict(i, kind, subject, change, describe(current, kind), history)
                counts["conflict"] += 1
                continue
            counts["resolved"] += 1
            if resolved == "ours":
                say(f"#{i} {kind}: keeping what is here ({subject})")
                continue
            # resolved == "theirs": falls through to apply, same as a fast-forward.
            say(f"#{i} {kind}: applying the incoming change over a local divergence ({subject})")
        else:
            counts["fastforward"] += 1
            say(f"#{i} {kind}: {subject} -> {describe(change['target'], kind)}")

        if not args.commit:
            continue
        try:
            if kind == "id_review":
                apply_id_review(conn, passage_id, change)
            elif kind == "boundary_review":
                apply_boundary_review(conn, passage_id, change)
            else:
                apply_artist_review(conn, recording_mbid, change)
        except (ValueError, sqlite3.Error) as e:
            say(f"    refused: {e}")
            counts["error"] += 1

    say(f"\n{counts['fastforward']} fast-forward, {counts['resolved']} resolved, "
        f"{counts['noop']} already in sync, {counts['conflict']} conflict(s) unresolved, "
        f"{counts['missing']} not present here, {counts['error']} refused")

    if args.commit:
        conn.commit()
        say("committed" if counts["fastforward"] or counts["resolved"] else "nothing to write")
    else:
        say("nothing was written. Re-run with --commit to do it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

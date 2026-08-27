#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Export applied edits for a remote installation `[SPEC006 §9]`.

The bundle transport (`[SPEC-SUI-095]`) carries new music. This carries a
*correction* to music both installations already have -- the case
`import_bundle` explicitly does nothing for, since it treats a held
`audio_md5` as fully present.

The unit exported is the reviewed decision, not the row it wrote:
`id_reviews`, `boundary_reviews` and `artist_reviews` `[SPEC021 §2]` are
already small journals of what changed, when, and (mostly) what it replaced.
Each applied row becomes one portable JSON record, keyed so a *different*
installation can find the same fact without ever seeing this one's
`passage_id` `[SPEC-DF-035]`.

    python tools/export_changes.py data/vaino_new.db -o changes.json
    rsync changes.json pi@vainopi:/srv/library/incoming/

Read-only: nothing here writes to the database it reads from. The write half
is `tools/apply_changes.py`, run against the *receiving* installation.
"""

import argparse
import json
import socket
import sqlite3
import sys


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def has_column(conn: sqlite3.Connection, table: str, column: str) -> bool:
    """This tool opens `mode=ro` `[SPEC013 Stage 2]`'s own reasoning applied
    here -- a read need not risk anything, so it cannot `ALTER TABLE` to add
    a column a review table predating `origin` `[SPEC-DF-104]` lacks. It
    reads around the gap instead: a `NULL` origin here means the same thing
    it always has, "made on this machine", just from a table too old to have
    a column that says so explicitly.
    """
    return any(row[1] == column for row in conn.execute(f"PRAGMA table_info({table})"))


def export_id_reviews(conn: sqlite3.Connection, hostname: str) -> list:
    changes = []
    origin_expr = "r.origin" if has_column(conn, "id_reviews", "origin") else "NULL"
    for (passage_id, chosen_mbid, previous_mbid, decided_at, origin,
         audio_md5, kind, start_ms, end_ms, title) in conn.execute(
        f"""SELECT r.passage_id, r.chosen_mbid, r.previous_mbid, r.decided_at, {origin_expr},
                  f.audio_md5, p.kind, p.start_ms, p.end_ms, rec.title
             FROM id_reviews r
             JOIN passages p ON p.passage_id = r.passage_id
             JOIN files f ON f.file_id = p.file_id
             LEFT JOIN recordings rec ON rec.mbid = r.chosen_mbid
            WHERE r.applied_at IS NOT NULL AND r.decision = 'reassigned'
              AND r.chosen_mbid IS NOT NULL"""):
        artists = [
            {"mbid": a_mbid, "name": a_name}
            for a_mbid, a_name in conn.execute(
                """SELECT a.mbid, a.name FROM recording_artists ra JOIN artists a ON a.mbid = ra.artist_mbid
                    WHERE ra.mbid = ?1 ORDER BY ra.weight DESC""", (chosen_mbid,))
        ]
        changes.append({
            "kind": "id_review",
            "anchor": {"audio_md5": audio_md5, "passage_kind": kind,
                       "start_ms": start_ms, "end_ms": end_ms},
            "baseline": {"mbid": previous_mbid},
            # `title`/`artists` are carried so a receiver that has never seen
            # this recording before can still construct it -- the same
            # NOT NULL constraints that made the first `apply_reviews.py`
            # unable to apply anything apply here too `[REQ-LIB-165]`.
            "target": {"mbid": chosen_mbid, "title": title, "artists": artists},
            "decided_at": decided_at,
            "origin": origin or hostname,
        })
    return changes


def export_boundary_reviews(conn: sqlite3.Connection, hostname: str) -> list:
    changes = []
    if not has_column(conn, "boundary_reviews", "audio_md5"):
        # A table from before `[SPEC-DF-102]` added the baseline columns at
        # all -- nothing here can be anchored on another machine, so there is
        # nothing to export, the same as any one row missing `audio_md5`.
        return changes
    origin_expr = "origin" if has_column(conn, "boundary_reviews", "origin") else "NULL"
    for (start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db,
         audio_md5, orig_kind, orig_start_ms, orig_end_ms,
         orig_lead_in_ms, orig_lead_out_ms, orig_gain_db,
         decided_at, origin) in conn.execute(
        f"""SELECT start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db,
                  audio_md5, orig_kind, orig_start_ms, orig_end_ms,
                  orig_lead_in_ms, orig_lead_out_ms, orig_gain_db, decided_at, {origin_expr}
             FROM boundary_reviews WHERE applied_at IS NOT NULL"""):
        if audio_md5 is None:
            # Applied before `[SPEC-DF-102]` added the baseline columns --
            # nothing to resolve this against on another machine, so it
            # cannot be exported. Not an error: it just predates sync.
            continue
        changes.append({
            "kind": "boundary_review",
            "anchor": {"audio_md5": audio_md5, "passage_kind": orig_kind,
                       "start_ms": orig_start_ms, "end_ms": orig_end_ms},
            "baseline": {"start_ms": orig_start_ms, "end_ms": orig_end_ms,
                         "lead_in_ms": orig_lead_in_ms, "lead_out_ms": orig_lead_out_ms,
                         "gain_db": orig_gain_db},
            "target": {"start_ms": start_ms, "end_ms": end_ms,
                       "lead_in_ms": lead_in_ms, "lead_out_ms": lead_out_ms,
                       "gain_db": gain_db},
            "decided_at": decided_at,
            "origin": origin or hostname,
        })
    return changes


def export_artist_reviews(conn: sqlite3.Connection, hostname: str) -> list:
    changes = []
    origin_expr = "origin" if has_column(conn, "artist_reviews", "origin") else "NULL"
    for (recording_mbid, artist_mbid, artist_name,
         previous_artist_mbid, previous_artist_name, previous_artist_weight,
         decided_at, origin) in conn.execute(
        f"""SELECT recording_mbid, artist_mbid, artist_name,
                  previous_artist_mbid, previous_artist_name, previous_artist_weight,
                  decided_at, {origin_expr}
             FROM artist_reviews WHERE applied_at IS NOT NULL"""):
        changes.append({
            "kind": "artist_review",
            "anchor": {"recording_mbid": recording_mbid},
            "baseline": {"artist_mbid": previous_artist_mbid, "artist_name": previous_artist_name,
                         "weight": previous_artist_weight},
            "target": {"artist_mbid": artist_mbid, "artist_name": artist_name},
            "decided_at": decided_at,
            "origin": origin or hostname,
        })
    return changes


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("-o", "--out", required=True)
    args = ap.parse_args()

    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    hostname = socket.gethostname()
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}

    changes = []
    by_kind = {}
    if "id_reviews" in have:
        by_kind["id_review"] = export_id_reviews(conn, hostname)
    if "boundary_reviews" in have:
        by_kind["boundary_review"] = export_boundary_reviews(conn, hostname)
    if "artist_reviews" in have:
        by_kind["artist_review"] = export_artist_reviews(conn, hostname)
    for kind_changes in by_kind.values():
        changes.extend(kind_changes)

    with open(args.out, "w", encoding="utf-8") as f:
        json.dump({"format_version": 1, "changes": changes}, f, indent=2)

    say(f"{len(changes)} applied change(s) exported to {args.out}")
    for kind, kind_changes in by_kind.items():
        say(f"  {kind}: {len(kind_changes)}")
    if not changes:
        say("nothing to sync yet -- no applied review decisions found")
    return 0


if __name__ == "__main__":
    sys.exit(main())

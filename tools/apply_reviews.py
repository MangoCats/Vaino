#!/usr/bin/env python3
"""Fold reviewed id decisions into the library `[REQ-LIB-165]`.

The review page records judgements and changes nothing else. This is the step
that acts on them, and it is deliberately separate, run by hand, and a rehearsal
by default -- for the same reason `restore_listener` is.

Reassigning a recording id changes what a passage *is*. Play history is keyed by
recording, so re-pointing a passage silently re-attributes every past play of
it, and the naming, the rotation and the "played 12 times" all move with it.
That is a migration. Migrations belong to Sampo and to a moment someone chose,
not to a web click.

    python tools/apply_reviews.py data/vaino_new.db            REHEARSE
    python tools/apply_reviews.py data/vaino_new.db --commit   do it

The numbers a rehearsal prints are the ones a real run produces: both are
measured from the same queries before anything is written.
"""

import argparse
import json
import sqlite3
import sys

# Where a link that came from a person's judgement is marked, as against the
# `inherited:mulib` that every other row in this table carries.
SOURCE = "review:acoustid"


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def cached_names(conn: sqlite3.Connection, passage_id: int) -> dict:
    """Titles and artists for the recordings AcoustID named for this passage.

    Read back out of the cached response rather than re-queried: the answer is
    already on disk, and a tool that needs the network to apply a decision
    someone already made is a tool that fails at the wrong moment.
    """
    row = conn.execute(
        """SELECT c.response FROM identification_cache c
             JOIN passages p ON p.passage_id = ?1
             JOIN files f ON f.file_id = p.file_id AND f.audio_md5 = c.audio_md5
            WHERE c.service = 'acoustid'
              AND c.request_key LIKE 'chromaprint:' || p.start_ms || '-' || p.end_ms || '%'
            LIMIT 1""",
        (passage_id,),
    ).fetchone()
    if not row or not row[0]:
        return {}
    blob = row[0].decode() if isinstance(row[0], bytes) else row[0]
    try:
        results = json.loads(blob)
    except json.JSONDecodeError:
        return {}
    out = {}
    for r in results:
        for rec in r.get("recordings") or []:
            if rec.get("id"):
                out[rec["id"]] = {
                    "title": rec.get("title"),
                    "artists": [(a.get("id"), a.get("name"))
                                for a in rec.get("artists") or [] if a.get("id")],
                }
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("--commit", action="store_true")
    args = ap.parse_args()

    conn = sqlite3.connect(args.db, timeout=60)
    conn.execute("PRAGMA busy_timeout = 60000")
    conn.execute("PRAGMA foreign_keys = ON")

    have = {r[0] for r in conn.execute(
        "SELECT name FROM sqlite_master WHERE type='table'")}
    if "id_reviews" not in have:
        say("no reviews recorded yet")
        return 1

    pending = conn.execute(
        """SELECT r.passage_id, r.chosen_mbid, pr.mbid
             FROM id_reviews r
             JOIN passage_recordings pr ON pr.passage_id = r.passage_id
            WHERE r.decision = 'reassigned' AND r.chosen_mbid IS NOT NULL
              AND pr.mbid <> r.chosen_mbid
            ORDER BY r.passage_id""").fetchall()

    kept = conn.execute(
        "SELECT COUNT(*) FROM id_reviews WHERE decision = 'kept'").fetchone()[0]
    deferred = conn.execute(
        "SELECT COUNT(*) FROM id_reviews WHERE decision = 'deferred'").fetchone()[0]

    say(f"{len(pending)} reassignment(s) to apply; "
        f"{kept} kept as they were; {deferred} deferred")
    if not pending:
        return 0

    applied = new_recordings = new_artists = unnamed = 0
    if args.commit:
        conn.execute("BEGIN IMMEDIATE")

    for passage_id, new_mbid, old_mbid in pending:
        names = cached_names(conn, passage_id)
        info = names.get(new_mbid)
        title = (info or {}).get("title")
        if title is None:
            # The link can still be made -- the id is what matters -- but a
            # recording row with no title would display as blank, so say so.
            unnamed += 1

        if args.commit:
            # The FK on passage_recordings requires the recording to exist.
            # `rowcount` on the INSERT, not `total_changes`: the latter is
            # cumulative for the connection and would count every row after
            # the first one as new.
            new_recordings += conn.execute(
                "INSERT OR IGNORE INTO recordings (mbid, title) VALUES (?1, ?2)",
                (new_mbid, title)).rowcount
            for artist_mbid, artist_name in (info or {}).get("artists", []):
                conn.execute(
                    "INSERT OR IGNORE INTO artists (mbid, name) VALUES (?1, ?2)",
                    (artist_mbid, artist_name))
                new_artists += conn.execute(
                    "INSERT OR IGNORE INTO recording_artists (mbid, artist_mbid, weight) "
                    "VALUES (?1, ?2, 1.0)", (new_mbid, artist_mbid)).rowcount
            # Replace the link rather than adding one: a passage with two
            # recordings is a medley, and this is not that -- it is one song
            # whose identity was wrong.
            conn.execute(
                "DELETE FROM passage_recordings WHERE passage_id = ?1", (passage_id,))
            conn.execute(
                "INSERT INTO passage_recordings (passage_id, mbid, weight, source) "
                "VALUES (?1, ?2, 1.0, ?3)", (passage_id, new_mbid, SOURCE))
        applied += 1
        if applied <= 10:
            say(f"  passage {passage_id}: {old_mbid} -> {new_mbid}"
                + (f"  ({title})" if title else "  (no title cached)"))

    if args.commit:
        conn.commit()
        say(f"\napplied {applied}; added {new_recordings} recording(s), "
            f"{new_artists} artist link(s)")
        say("Play history keyed to the OLD recording stays with the old id: "
            "those plays did happen, and to what the passage was then.")
    else:
        say(f"\nwould apply {applied}"
            + (f"; {unnamed} have no cached title" if unnamed else ""))
        say("nothing was written. Re-run with --commit to do it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fetch missing cover art from the Cover Art Archive `[REQ-VIS-170]`.

Measured on this library: 1,986 files carry no embedded picture, and 1,656 of
them (83%) already have a `folder.jpg` beside them -- the player now looks
there and needs nothing from the network. This covers what is left: **309
releases**, about five minutes at one request a second.

Keyed by **release**, not by folder. A directory can hold more than one album,
which is exactly the case on the DAO rips that make up most of the remaining
gap, and `folder.jpg` cannot tell them apart.

**Front and back both**, because MuLibPlay carried both and showed them side by
side; 559 of its 675 albums had a back. Nothing else: its art totalled 80.5 MB
of a 90.7 MB database, and a third image nobody displays would be that bargain
again.

Sampo's job, not the player's. `[REQ-NEG-100]` forbids *playback* depending on
a live external service; fetching at build time into local storage is the
division that requirement exists to protect. Nothing here runs on the appliance.

    python tools/fetch_cover_art.py data/vaino_new.db [--limit N] [--refetch]
"""

import argparse
import sqlite3
import sys
import time
import urllib.error
import urllib.request

# The archive asks for one request a second and a User-Agent that identifies
# the client. Both are conditions of use, not politeness.
GAP = 1.0
AGENT = "Vaino/0.1 (+https://github.com/MangoCats/Vaino)"
BASE = "https://coverartarchive.org"

# Below this it is not a picture -- the same floor the player applies, and the
# one MuLibPlay applied before it.
MIN_BYTES = 256

DDL = """
CREATE TABLE IF NOT EXISTS cover_art (
    release_mbid TEXT PRIMARY KEY,
    front        BLOB,
    back         BLOB,
    source       TEXT NOT NULL,
    fetched_at   TEXT NOT NULL);
"""


def say(text: str) -> None:
    enc = sys.stdout.encoding or "utf-8"
    print(text.encode(enc, "replace").decode(enc), flush=True)


def get(url: str) -> bytes | None:
    """One image, or `None` if the archive has not got it.

    A 404 is the ordinary answer for a release nobody has photographed, and is
    not worth a retry or a mention. 503 means slow down.
    """
    req = urllib.request.Request(url, headers={"User-Agent": AGENT})
    delay = 2.0
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=45) as r:
                data = r.read()
            return data if len(data) >= MIN_BYTES else None
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            if e.code in (429, 500, 502, 503, 504) and attempt < 3:
                time.sleep(delay)
                delay *= 2
                continue
            return None
        except (urllib.error.URLError, TimeoutError):
            if attempt < 3:
                time.sleep(delay)
                delay *= 2
                continue
            return None
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--refetch", action="store_true",
                    help="ask again about releases already tried")
    args = ap.parse_args()

    conn = sqlite3.connect(args.db, timeout=60)
    conn.execute("PRAGMA busy_timeout = 60000")
    conn.executescript(DDL)
    conn.commit()

    # Releases that a passage with no local art actually needs. Files whose
    # folder already holds a cover are excluded: the player reads those
    # directly, so fetching them would be 1,656 requests for nothing.
    todo = conn.execute(
        """SELECT DISTINCT rr.release_mbid
             FROM passages p
             JOIN file_tags ft ON ft.file_id = p.file_id AND ft.has_art = 0
             JOIN passage_recordings pr ON pr.passage_id = p.passage_id
             JOIN release_recordings rr ON rr.mbid = pr.mbid AND rr.chosen = 1
            WHERE p.kind = 'radio'
              AND (?1 OR rr.release_mbid NOT IN (SELECT release_mbid FROM cover_art))
            ORDER BY rr.release_mbid""",
        (1 if args.refetch else 0,)).fetchall()
    todo = [r[0] for r in todo]
    if args.limit:
        todo = todo[:args.limit]

    say(f"{len(todo)} release(s) to ask about  (~{len(todo) * GAP / 60:.0f} min)\n")
    if not todo:
        return 0

    fronts = backs = neither = 0
    now = time.strftime("%Y-%m-%dT%H:%M:%S")
    for i, mbid in enumerate(todo, 1):
        front = get(f"{BASE}/release/{mbid}/front")
        time.sleep(GAP)
        # The release group is the fallback for a front only. A back cover
        # belongs to a particular pressing, so the group's is not this one's.
        if front is None:
            front = get(f"{BASE}/release-group/{mbid}/front")
            time.sleep(GAP)
        back = get(f"{BASE}/release/{mbid}/back")
        time.sleep(GAP)

        conn.execute(
            "INSERT OR REPLACE INTO cover_art VALUES (?1,?2,?3,'coverartarchive',?4)",
            (mbid, front, back, now))
        conn.commit()
        fronts += front is not None
        backs += back is not None
        neither += front is None and back is None
        if i % 25 == 0 or i == len(todo):
            say(f"  {i}/{len(todo)}   {fronts} front, {backs} back, {neither} neither")

    size = conn.execute(
        "SELECT COALESCE(SUM(LENGTH(front)),0) + COALESCE(SUM(LENGTH(back)),0) "
        "  FROM cover_art").fetchone()[0]
    say(f"\n  {fronts} front, {backs} back, {neither} with nothing")
    say(f"  cover_art now holds {size / 1048576:.1f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())

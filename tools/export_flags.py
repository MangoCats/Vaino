#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Export flagged recordings and passages for a remote installation `[SPEC006 §10]`.

`[REQ-VIS-265]`'s checkbox is set on vainopi's own play-history page --
exactly the machine with no Sampo to act on it. This gets the name of a
flagged recording or passage off that installation and onto a portable form a
*different* installation's own library can resolve, the same way `export_changes.py`
already does for an applied decision rather than a mere identity.

vainopi has no Python `[SPEC-DF-108]`, so this never runs there -- it reads a
**copy** of its database, pulled down however you like:

    scp pi@vainopi:/srv/library/vaino.db /tmp/vainopi-copy.db
    python tools/export_flags.py /tmp/vainopi-copy.db -o flags.json

Read-only: nothing here writes to the database it reads from. The write half
is `tools/import_flags.py`, run against the *receiving* installation.
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
    return any(row[1] == column for row in conn.execute(f"PRAGMA table_info({table})"))


def export_flags(conn: sqlite3.Connection, hostname: str) -> list:
    """One portable record per flag `[SPEC-DF-107]`: a recording's own mbid
    needs no translation at all; a passage borrows the `(audio_md5, kind,
    start_ms, end_ms)` anchor `[SPEC-DF-103]` already resolves boundary edits
    against, via the same JOIN `apply_changes.py`'s `resolve_passage()` reads
    back out the other way.
    """
    have = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "listener_flags" not in have:
        return []

    origin_expr = "f.origin" if has_column(conn, "listener_flags", "origin") else "NULL"
    out = []
    for subject_kind, subject_id, flagged_at, origin in conn.execute(
            f"SELECT f.subject_kind, f.subject_id, f.flagged_at, {origin_expr} "
            f"FROM listener_flags f ORDER BY f.flagged_at"):
        if subject_kind == "recording":
            anchor = {"recording_mbid": subject_id}
        else:
            row = conn.execute(
                """SELECT fi.audio_md5, p.kind, p.start_ms, p.end_ms
                     FROM passages p JOIN files fi ON fi.file_id = p.file_id
                    WHERE p.passage_id = ?1""", (int(subject_id),)).fetchone()
            if not row:
                # The passage this flag named no longer exists in this copy --
                # nothing to anchor it to, the same reasoning `[SPEC-DF-102]`
                # already applies to a review row missing its own baseline.
                continue
            anchor = {"audio_md5": row[0], "passage_kind": row[1],
                      "start_ms": row[2], "end_ms": row[3]}
        out.append({
            "subject_kind": subject_kind,
            "anchor": anchor,
            "flagged_at": flagged_at,
            "origin": origin or hostname,
        })
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("-o", "--out", required=True)
    args = ap.parse_args()

    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    flags = export_flags(conn, socket.gethostname())

    with open(args.out, "w", encoding="utf-8") as f:
        json.dump({"format_version": 1, "flags": flags}, f, indent=2)

    say(f"{len(flags)} flag(s) exported to {args.out}")
    if not flags:
        say("nothing flagged there yet")
    return 0


if __name__ == "__main__":
    sys.exit(main())

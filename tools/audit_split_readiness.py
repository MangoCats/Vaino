#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Which `tools/` scripts are ready for a split database, and which are not.

The local split's remaining work is 60-odd scripts, and "migrate them all"
is not a plan -- it is a wish with no way to tell when it is done. This is
the checklist, derived from the scripts themselves rather than maintained
by hand, so it cannot drift from what is actually in the tree.

Each script is classified by two questions:

  **What does it touch?** `catalogue`, `listener`, `both`, or neither. A
  script touching only one half still has to open the *right* half; one
  touching both is where `role` actually requires thought.

  **How does it open a database?** Going through `vaino_db` is ready.
  Calling `sqlite3.connect` directly is not, and the two specific patterns
  that break silently on a split pair are called out by name:

    - an unqualified `sqlite_master` existence check, which answers only
      for `main` and so reports half the database missing `[IMPL-DBSPLIT-025]`
    - `CREATE TABLE [IF NOT EXISTS]`, which always targets `main` and can
      shadow the other half's table, masking it with an empty one

Not every hit is a defect. A script that only ever opens a *remote* path,
or builds its own throwaway database, is listed as `n/a` and is skipped by
the count. The point is a number that goes down, and a list short enough to
read.

    python tools/audit_split_readiness.py [--verbose]
"""

from __future__ import annotations

import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

CATALOGUE = {"files", "passages", "passage_recordings", "recordings", "artists",
             "recording_artists", "recording_relations", "releases",
             "release_recordings", "flavor", "flavor_constants", "cover_art",
             "file_tags", "id_checks", "lyrics", "ingest_decisions",
             "lowlevel_cache", "musicbrainz_cache", "identification_cache"}
LISTENER = {"listener_play_history", "listener_rejections", "listener_flags",
            "listener_preferences", "listener_likes", "listener_programs",
            "listener_program_seeds", "listener_occasions",
            "listener_occasion_points", "listener_characteristics",
            "listener_settings", "player_state", "player_settings",
            "id_reviews", "boundary_reviews", "artist_reviews",
            "selection_decisions"}

# Scripts with no database of their own to open: they drive a remote, build
# a scratch file, or are pure helpers imported by others.
NOT_APPLICABLE = {
    "vaino_db.py", "audit_split_readiness.py", "split_database.py",
    "remote_peek.py", "passage_orphans.py", "migrate_mulib.py",
}

TABLE_RE = re.compile(r"\b(?:FROM|JOIN|INTO|UPDATE)\s+([a-z_][a-z0-9_]*)", re.I)
MASTER_RE = re.compile(r"FROM\s+sqlite_master", re.I)
CREATE_RE = re.compile(r"CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([a-z_][a-z0-9_]*)", re.I)
CONNECT_RE = re.compile(r"sqlite3\.connect\s*\(")


def classify(path: str) -> dict:
    src = open(path, encoding="utf-8").read()
    named = {m.lower() for m in TABLE_RE.findall(src)}
    cat = named & CATALOGUE
    lis = named & LISTENER
    creates = {m.lower() for m in CREATE_RE.findall(src)}
    return {
        "catalogue": sorted(cat),
        "listener": sorted(lis),
        "uses_vaino_db": "import vaino_db" in src or "from vaino_db" in src,
        "raw_connect": len(CONNECT_RE.findall(src)),
        "bare_master": len(MASTER_RE.findall(src)),
        "creates_catalogue": sorted(creates & CATALOGUE),
        "creates_listener": sorted(creates & LISTENER),
    }


def touches(info: dict) -> str:
    if info["catalogue"] and info["listener"]:
        return "both"
    if info["catalogue"]:
        return "catalogue"
    if info["listener"]:
        return "listener"
    return "-"


def main() -> int:
    verbose = "--verbose" in sys.argv
    rows = []
    for name in sorted(os.listdir(HERE)):
        if not name.endswith(".py") or name.startswith("test_"):
            continue
        if name in NOT_APPLICABLE:
            continue
        info = classify(os.path.join(HERE, name))
        if touches(info) == "-":
            continue
        rows.append((name, info))

    ready = [r for r in rows if r[1]["uses_vaino_db"]]
    todo = [r for r in rows if not r[1]["uses_vaino_db"]]

    print(f"{'script':<34} {'touches':<10} {'opens':<7} {'master':<7} hazards")
    for name, info in rows:
        hz = []
        if info["creates_catalogue"]:
            hz.append("CREATE " + ",".join(info["creates_catalogue"]))
        if info["bare_master"]:
            hz.append(f"{info['bare_master']} bare sqlite_master")
        mark = "vaino_db" if info["uses_vaino_db"] else f"{info['raw_connect']} raw"
        if verbose or not info["uses_vaino_db"]:
            print(f"{name:<34} {touches(info):<10} {mark:<7} "
                  f"{info['bare_master'] or '':<7} {'; '.join(hz)}")

    both = [n for n, i in todo if touches(i) == "both"]
    print()
    print(f"ready: {len(ready)}    to migrate: {len(todo)}    "
          f"of which touch both halves: {len(both)}")
    if both:
        print("  both halves (migrate these with care -- `role` is a real choice):")
        for n in both:
            print(f"    {n}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

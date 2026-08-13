"""Load MuLibPlay's four seasonal curves as data `[SPEC-DIR-134]`.

MuLibPlay hardcoded `[C]`, `[W]`, `[S]` and `[K]` into a `switch`, which is why
it had exactly four. Vaino reads curves from `listener_occasions` and
`listener_occasion_points`, so these are rows, and a fifth occasion needs no
code `[SPEC-DIR-130]`.

The values below are transcribed from `occasionWeight()` in the inherited
`musicdirector.cpp`. The library already carries the characteristic values —
41 christmasy recordings, 140 for_children, and a handful of wintry/summery —
inherited from six years of tagging, so loading the curves is what makes that
tagging act again.

Usage:
  python tools/load_occasions.py <vaino.db> [--write] [--kids WEIGHT]
"""

from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

# MuLibPlay's `kidSongWeight`, whose shipped default is 0.000001 — an effective
# ban rather than a de-emphasis. It reaches 149 radio passages here, so it is a
# parameter rather than a constant: `--kids 0.5` merely damps them.
KIDS_DEFAULT = 0.000001

# `[C]` — a formula in the original, sampled here at the points where its shape
# changes. November climbs as (25/days)^3; December as 5/sqrt(days); the 25th
# spikes to 10 and the tail decays as -1/days. Interpolated in log space, which
# is what `[SPEC-DIR-132]` specifies for ratios.
CHRISTMAS = [
    (1, 1, 0.000001), (10, 31, 0.000001),
    (11, 1, 0.0992), (11, 15, 0.2441), (11, 30, 1.0),
    (12, 10, 1.2910), (12, 20, 2.2361), (12, 24, 5.0), (12, 25, 10.0),
    (12, 26, 1.0), (12, 31, 0.1667),
]

# `[W]` and `[S]` were whole-month constants, so they are step curves.
WINTER = [(1, 1, 1.5), (2, 1, 1.0), (3, 1, 0.25), (4, 1, 0.000001),
          (11, 1, 0.5), (12, 1, 2.0)]
SUMMER = [(1, 1, 0.2), (5, 1, 0.5), (6, 1, 2.0), (7, 1, 1.5), (8, 1, 1.0),
          (9, 1, 0.2)]


def curves(kids: float) -> list[tuple[str, str, str, list]]:
    return [
        ("user.christmas", "christmasy", "linear", CHRISTMAS),
        ("user.winter", "wintry", "step", WINTER),
        ("user.summer", "summery", "step", SUMMER),
        # `[K]` was never seasonal — a single flat multiplier all year. It
        # expresses as a one-point curve, which is a fair test of whether
        # "curves are data" actually holds `[SPEC-DIR-134]`.
        ("user.childrens", "for_children", "step", [(1, 1, kids)]),
    ]


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    db = Path(args[0])
    write = "--write" in args
    kids = float(args[args.index("--kids") + 1]) if "--kids" in args else KIDS_DEFAULT

    con = sqlite3.connect(db)
    print(f"{'occasion':<22} {'class':<16} {'interp':<7} {'points':>6}  reach")
    for ch, cl, interp, pts in curves(kids):
        n = con.execute(
            "SELECT COUNT(*) FROM passages p JOIN passage_recordings pr USING (passage_id) "
            "JOIN flavor f ON f.subject_id = pr.mbid AND f.characteristic = ? "
            "AND f.class = ? AND f.value > 0 WHERE p.kind = 'radio'",
            (ch, cl),
        ).fetchone()[0]
        print(f"{ch:<22} {cl:<16} {interp:<7} {len(pts):>6}  {n} radio passages")

    if not write:
        print("\n(dry run -- pass --write to store)")
        return 0

    for ch, cl, interp, pts in curves(kids):
        con.execute("INSERT OR REPLACE INTO listener_occasions VALUES (?,?,?)", (ch, cl, interp))
        con.execute("DELETE FROM listener_occasion_points WHERE characteristic=? AND class=?",
                    (ch, cl))
        for m, d, v in pts:
            con.execute("INSERT INTO listener_occasion_points VALUES (?,?,?,?,?)",
                        (ch, cl, m, d, v))
    con.commit()
    print(f"\nloaded {len(curves(kids))} curves, kids weight {kids}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

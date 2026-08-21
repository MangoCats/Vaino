# PI005: The Library on the Appliance

**Appliance Record — getting the real library onto vainopi, and what it cost**

The appliance runs from one SQLite file named outright by its unit
(`ExecStart=… /srv/library/vaino.db`). Nothing swaps that file on its own, and
this records the one time it was swapped, what the transfer had quietly broken,
and how long the repair actually took.

> **Related:** [SPEC012](../docs/spec/SPEC012-library-relink.md) for why binding
> is by content and not by path · [SPEC013 §5](../docs/spec/SPEC013-sampo-console.md#5-export--new-music-to-a-remote-vaino)
> for the bundle that carries new music · [PI004](PI004-speaker-operation.md)

---

## 1. The appliance takes the real library

**`[PI5-LIB-010]` Swapped 2026-08-20, and nothing was reverting on its own.**
The appliance had been running a 31-file test library while the full one sat
beside it as `vaino-new.db`, untouched since 17 August. The unit names its path
outright — `ExecStart=… /srv/library/vaino.db` — so no power cycle was ever
going to change that.

The test library's own listening was discarded deliberately: 252 plays made
against 31 files during testing, against 37,237 that describe six years of real
listening. A straight copy was therefore right, where `[SPEC-SUI-100]`'s
merge-don't-overwrite argument would otherwise apply — the rule protects
irreplaceable listening, and test plays are not that.

| | before | after |
| :--- | ---: | ---: |
| files / radio passages | 31 / 35 | **5,709 / 8,330** |
| plays · preferences | 37,489 · 3,261 | **37,238 · 3,261** |
| programmes · seeds | 8 · 49 | 8 · 49 |

**`[PI5-LIB-020]` 277 paths were rewritten, and the prediction held exactly.**
`[SPEC-RLK-025]` said 276 of the shipped paths would carry a Windows private-use
codepoint and fail to resolve on Linux. Measured before the swap: **276 carried
one, and the same 276 were precisely the paths that did not resolve.** Relink
rewrote 277 — the 276, plus one row bound to `The More We Get Together_2.mp3`
and rebound to the copy without the suffix, which is `[SPEC-RLK-120]` choosing
the first in sorted walk order.

Afterwards: **5,705 of 5,705 paths resolve, 0 missing, 0 private-use codepoints
left, 0 corrupt, `integrity_check` ok.** `History: America's Greatest Hits` now
browses with a real colon.

**`[PI5-LIB-030]` The four newest tracks had to be imported again**, because the
staged database predates them: it is an 17 August snapshot and they were
inducted on the 20th. Relink reported them as `unknown` — audio present, no row
claiming it — which is exactly the state `[SPEC-RLK-090]` says is ingest's
problem and not relink's. Re-running the bundle import restored them in one
command, against a **running** player and with no lock contention, and
`POST /library/reload` `[IMPL-SUI-078]` made them selectable without a restart.

> **Cost, for planning.** 2 h 37 m of hashing `[SPEC-RLK-075]`, during which the
> player is stopped and the appliance is silent. That is the number to quote
> before starting one, not the hour SPEC012 originally estimated.

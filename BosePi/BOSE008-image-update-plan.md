# BOSE008: Updating bose's Image — What to Add, and What to Refuse

**Implementation Guide — the plan, and the budget it is held to**

`bose` runs an older build, is unsplit, carries none of the specials data,
and cannot be reached by half the `tools/` directory. This is what to change
and, more usefully, what not to.

The goal it is written against: **make the ecosystem cheap to maintain
without taxing the Pi.** Those pull in opposite directions often enough that
the tension has to be stated rather than resolved by instinct — every
addition below carries what it costs, and the ones that were rejected say
why.

> **Related:** [BOSE003](BOSE003-build-procedure.md) for how the image is
> built · [BOSE005](BOSE005-power-loss-test.md) for the power cut this plan
> reasons about · [PI025](../VainoPi/PI025-what-the-local-split-owes-vainopi.md)
> for the same exercise done against `vainopi` · [IMPL001](../VainoPi/IMPL001-appliance-setup.md)
> for the package list this proposes one addition to

---

## 1. The budget

**`[BOS-IMG-010]` The Pi Zero 2W has 512 MB and no fan, and that is the
whole constraint.** `[REQ-HW-140]` already treats every crate as a memory
decision; the same discipline applies to every package. So each item here is
judged on **resident cost**, not disk: a command-line tool that runs for
200 ms during maintenance and is absent from RAM the rest of the time is
nearly free, and a daemon is not.

That distinction does most of the work below. It is why one package is
recommended and every daemon-shaped suggestion is refused.

**`[BOS-IMG-015]` Measured, not assumed.** `bose` currently has 922 MB free
on `/` and an apt cache from 2026-09-06 with working network. Whatever is
added here is small against that; the argument is never "it fits" but "it
earns its place while resident, which it is not".

## 2. Add: `sqlite3` (557 KB, no daemon, no resident cost)

**`[BOS-IMG-020]` Install it.** `bose` has no `sqlite3` command, which is
how the whole of this came up: `tools/remote_peek.py` is
`ssh <host> sqlite3 -json <path> "<sql>"`, so **every** tool built on it —
`remote_flags`, `sync_preferences`, `mesh_diff`, `resolve_mesh_conflict`,
`remote_snapshot` — reported that appliance unreachable when it was merely
differently equipped.

It costs 557 KB installed, has no reverse dependencies, runs only when
invoked, and is resident for the length of one query. Against that:

- **Diagnosis.** Every investigation run against `vainopi` this week — shadow
  tables, journal modes, orphan counts, flag counts — was one `sqlite3`
  invocation. On `bose` none of them were possible without writing tooling
  first. The value of the command is highest at exactly the moment nobody
  wants to be writing tooling.
- **`vaino-db-recover` needs it**, and `bose` will need that script the day
  it splits (§4).
- **One exercised path.** `remote_peek` now falls back to `python3` where
  `sqlite3` is missing, and that fallback is worth having — but if `bose` is
  the only host that takes it, it becomes the path nothing else exercises,
  which is precisely the failure `[PI-OWE-010]` names. The fallback should
  be the safety net for an unknown host, not the daily route for a known
  one.

**`[BOS-IMG-025]` And record it in [IMPL001](../VainoPi/IMPL001-appliance-setup.md).**
Neither appliance's documented build installs `sqlite3`; `vainopi` has it
because someone installed it by hand (`apt-mark showmanual` confirms), and
`bose` does not. That is not two policies, it is an absence of one, and the
next appliance built will be a coin flip. Whichever way this goes it belongs
in the package list beside `zram-tools` and `bluez`.

## 3. Refuse: everything daemon-shaped

**`[BOS-IMG-030]`** The tempting additions are all the ones that would sit
in RAM. Named here so they are refused once rather than re-proposed:

| Suggestion | Why not |
| :--- | :--- |
| A local `sshd`-triggered agent for sync | `ssh` already runs; a second listener buys nothing and costs memory permanently |
| `python3-full` / the whole stdlib | `python3` is present and sufficient; the extras are disk and update surface for nothing |
| A cron'd health reporter | `[BOS-OPS-*]`'s baseline is captured on demand by `power-test-manifest.sh`; a timer that samples a healthy machine forever is load in exchange for data nobody reads |
| Sampo's tool-chain on the Pi | `[SPEC035]` already settled this: Sampo work happens on Sampo-capable nodes. The Pi is a player |
| A second database for metrics | `[SPEC-SC-015]`'s "no field without a consumer", one level up |

**`[BOS-IMG-035]` The rule these share**: an appliance should carry what it
needs *to play music* and what a person needs *to diagnose it when it will
not*. Nothing that samples, reports or synchronises on its own schedule --
the desktop initiates, the appliance answers. That is what keeps the
maintenance cost on the machine that has 32 GB.

## 4. The split, and what it would oblige

**`[BOS-IMG-040]` `bose` is unsplit, and therefore does not need
`vaino-db-recover` today.** Checked rather than assumed, and the reason is
structural: the recovery script exists because a *split* player attaches
`library.db` `mode=ro`, and a read-only connection cannot roll back the hot
journal it is required to roll back — which on `vainopi` turned one power
cut into a 23-restart crash loop `[PI3-FOUND-120]`. `bose` opens one
database read-write, so it rolls back its own journal on the first open.
`[BOS-PWR-*]`'s power cut on 2026-09-10 passed for exactly this reason.

**`[BOS-IMG-045]` So splitting `bose` obliges three things at once**, and
they are a package, not a menu:

1. `sqlite3` installed (§2) — `vaino-db-recover` is `/bin/sh` and shells out
   to it.
2. `vaino-db-recover` installed and wired as `ExecStartPre`, as `vainopi`
   has it. Without this, the first power cut after the split is a crash
   loop, and the machine is cut from a speaker's USB port.
3. The unit's `ExecStart` changed to the two-path form
   (`listener.db --library library.db`).

Do fewer than three and the split is a trap that springs on the next power
cut rather than at the moment it is made.

**`[BOS-IMG-050]` There is no urgency to split `bose` at all.** The reason
`vainopi` is split is a read-only catalogue partition `[PI023]`; the reason
the desktop was split is that the split shape needed to be the one exercised
daily. Neither argument reaches `bose`, which has one writable filesystem
and runs no tools. Split it when there is a reason, and take §4's three
steps together when that day comes.

## 5. The data `bose` is missing

**`[BOS-IMG-060]`** Independent of any of the above, and cheap:

```
python tools/load_occasions.py    /var/vaino/vaino.db --write
python tools/backfill_profanity.py /var/vaino/vaino.db --mulib <path>/mulib.db --commit
```

Being unsplit, `bose` needs neither `--library` nor the `--sql-out` patch
route — both tools take the one path. That gives it the six specials and the
69 recovered profanity ratings the other two installations already carry
`[SPEC-PREF-080]`, `[SPEC-PREF-082]`.

It also needs a current build: it is running a player that predates the
specials panel entirely, so the data would sit unread until
`VainoPi/deploy.sh <tag> pi@bose` follows.

---

## 6. Open

**`[BOS-IMG-070]`** None of this is applied. §2 and §5 are independent and
can be done in any order; §4 is a package to be taken whole or not at all.

**`[BOS-IMG-075]`** The `python3` fallback in `remote_peek` will have no
regular exerciser once `bose` has `sqlite3`. It is pinned by shape in
`test_remote_peek.py`, which is not the same as being run. A forcing switch
— an environment variable that makes `run_remote_sql` take the fallback
deliberately — would let the suite exercise both paths against a real host,
and is the honest price of keeping a second path at all.

---

**Traceability:** `[BOS-IMG-010..075]` · holds `[REQ-HW-140]`'s memory
discipline over package choice · depends on `[PI3-FOUND-120]`'s crash loop
for §4's reasoning and `[BOS-PWR-*]`'s passed power cut for why it does not
apply yet · proposes one addition to [IMPL001](../VainoPi/IMPL001-appliance-setup.md)'s
package list · carries `[PI-OWE-010]`'s "a path only one machine takes is a
path that rots" into a decision about a package

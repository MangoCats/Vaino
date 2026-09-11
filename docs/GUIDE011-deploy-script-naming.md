# GUIDE011: What the Deploy Scripts Should Be Called

**Development Guidance — planned 2026-09-11, after a misnamed script deployed to RAM for five days**

A naming and signposting convention for the scripts that build, deploy and
verify Vaino across the fleet. Written because a name proved load-bearing:
`[IMPL-BOS-185]`'s five days of RAM-only deploys to `bose` happened partly
because the script doing them was called, and filed, as though it only ever
touched `vainopi`.

> **Related:** [BOSE010](../BosePi/BOSE010-changing-a-locked-card.md) — the traps this is meant to make visible · [BOSE006](../BosePi/BOSE006-what-lands-where.md) `[IMPL-BOS-185]` — the incident · [build/README.md](../build/README.md) — the cross-compile story these scripts automate

---

## 1. Why this is not cosmetic

**`[GDE-DEP-010]` The name is where the question should have been asked.**
`b032f3b` added `bose` to the appliance list by passing `pi@bose` to a script
called `deploy-vainopi.sh`, which delegates to `deploy-player.sh` living in the
`VainoPi/` folder. Both are host-generic — each takes a host argument and
defaults to vainopi — but nothing in either name, path, or call site ever
raised *does this appliance's root filesystem behave the same way?* It does
not: `bose` has an overlay root and `vainopi` does not, and every deploy to
`bose` from that day until 2026-09-11 went to RAM and reported success.

A per-machine folder that contains machine-generic tooling is worse than a flat
one, because it makes a false promise about scope. `[GOV-DOC-030]`'s reasoning
for segregating inherited material applies here to code.

---

## 2. What the survey actually found

**`[GDE-DEP-020]` Three scripts are misnamed. The rest are fine, and saying so
matters as much.** Every script taking a `HOST` argument was checked, 2026-09-11.

| script | takes a host | named for a machine | verdict |
| :--- | :--- | :--- | :--- |
| `deploy-vainopi.sh` (build/) | yes | yes | **misnamed** — generic appliance deploy |
| `deploy-player.sh` (VainoPi/) | yes | by folder | **misplaced** — generic installer |
| `deploy.sh` (VainoPi/) | yes | by folder | **misplaced** — thin wrapper |
| `provision-bose.sh`, `finalize-bose.sh`, `build-bose-card.sh`, `seed-library.sh`, `attended-import.sh`, `request-unlock.sh` (BosePi/) | yes | yes | **correct** |
| `deploy-everywhere.sh`, `deploy-local.sh`, `update-source-host.sh`, `verify-targets.sh` (build/) | n/a | no | **correct** |

The BosePi scripts are parameterised but genuinely implement *bose's*
procedure — its card layout, its lock-in, its library seed. A host argument is
not on its own evidence of genericity; what matters is whether the *procedure*
is machine-specific. Renaming those would be the same error in the opposite
direction.

---

## 3. The rule

**`[GDE-DEP-030]` A script that accepts a host must not be named after one, and
must not live in one machine's folder.** Its name should say what it acts on
and what it does to it — `install-player`, `deploy-appliance`,
`verify-targets` — so that an operator reading a command line can tell what is
about to be touched without opening the file.

**`[GDE-DEP-040]` Location carries scope.** `build/` means *runs on the
development host and reaches out to something else*. A machine folder
(`BosePi/`, `VainoPi/`, `SmartPC/`) means *this material is about that machine
and should not be pointed anywhere else*. Nothing generic belongs in a machine
folder, and nothing machine-specific belongs in `build/`.

**`[GDE-DEP-050]` Irreversible steps keep an explicit verb and a second
confirmation.** Already the practice worth preserving —
`finalize-bose.sh --lock-in --confirm-heard-it-play` says exactly what it is
and refuses to be typed by accident `[IMPL-BOS-120]`.

---

## 4. Proposed names

Proposed names are given as basenames; the directory is stated in the last
column, deliberately, so this plan does not cite paths that do not exist yet
and set `[GOV-DOC-040]`'s path check complaining about its own proposal.

| now | proposed basename | lands in |
| :--- | :--- | :--- |
| `deploy-vainopi.sh` (build/) | `deploy-appliance.sh` | `build/` — cross-compiles, then delegates |
| `deploy-player.sh` (VainoPi/) | `install-player.sh` | `build/` — the installer, out of the machine folder |
| `deploy.sh` (VainoPi/) | *fold into the above* | — a two-line wrapper over both |

`vainopi` remains the default host for all three, so existing invocations keep
working; only the name and path change.

---

## 5. The part that matters more than the rename

**`[GDE-DEP-060]` Every script must say aloud what class of target it thinks it
is talking to, before it acts.** Renaming would not, by itself, have caught
`[IMPL-BOS-185]`. What catches it is the target announcing itself:

```
deploy: pi@bose has an overlay root; will persist through /media/root-ro
deploy: persisted to /media/root-ro/usr/local/bin/vaino (ba3d3e95…)
deploy: WARNING -- /media/root-ro left read-write; it returns to ro on the next reboot
```

Those lines are the model, and the convention this document most wants adopted.
A script that silently assumes a target's shape produces a log indistinguishable
from one that assumed correctly; a script that *states* the assumption turns a
wrong guess into a visible line. The same applies to architecture, to whether a
service is running, and to whether a partition is writable — each already a
real failure mode here `[IMPL-BOS-175]`, `[BOS-PWR-050]`.

**`[GDE-DEP-070]` Verify the durable artefact, not the convenient one.**
`deploy-everywhere.sh` asks the running player what it is, which its own header
correctly calls the only check that catches a stale service. It is also blind
to the case where the disk is RAM. Where a target has two copies — live and
persisted — both must be checked, and the *persisted* one is the answer.

---

## 6. What the migration costs

**`[GDE-DEP-080]` The rename is cheap; the citations are not.** Measured
2026-09-11: `deploy-vainopi.sh` is named in 4 files, `VainoPi/deploy.sh` in 4,
and `deploy-player.sh` in **16** — including `HOWTO.md`, four `PI0*` records,
three `BOSE0*` records, and specs as far afield as `SPEC034` and `SPIN002`.

`tools/check_docs.py` validates that every cited path exists `[GOV-DOC-040]`,
so a rename and its documentation updates **must land in one commit** or CI
reports the difference. That is a feature: it makes the true cost visible up
front rather than leaving dangling references.

Historical records are the judgement call. `PI008` describes a bringup that
genuinely used the old name on the old date; rewriting it would falsify a
record. The rule applied here should be **rename living references, annotate
historical ones**, consistent with `[GOV-DOC-050]`'s separation of current from
historical.

---

## 7. Open

**`[GDE-DEP-090]` Not yet decided: whether anything persists *configuration* to
an overlay appliance.** `[IMPL-BOS-185]` fixed the binary path only. Unit files
and `/etc` are still written by `provision-bose.sh` pre-lock-in and by nothing
afterwards, so a unit change on a locked `bose` is a RAM write that looks
correct until the next reboot — the failure that actually stopped the music.
Whichever script grows that capability should be named for it, and this
document's rules should be applied to it when it is written rather than after.

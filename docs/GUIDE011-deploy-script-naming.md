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
| `deploy.sh` (VainoPi/) | yes | by folder | **misplaced** — see `[GDE-DEP-025]` |
| `provision-bose.sh`, `finalize-bose.sh`, `build-bose-card.sh`, `seed-library.sh`, `attended-import.sh`, `request-unlock.sh` (BosePi/) | yes | yes | **correct** |
| `deploy-everywhere.sh`, `deploy-local.sh`, `update-source-host.sh`, `verify-targets.sh` (build/) | n/a | no | **correct** |

**`[GDE-DEP-025]` Correction, 2026-09-11: `deploy.sh` is not a thin wrapper.**
An earlier revision of this document called it "a two-line wrapper over both"
and proposed folding it away. Reading it first would have prevented that: it is
~130 lines that build a *named ref* in a container-side git worktree without
touching the caller's checkout, refuse a dirty tree, handle MSYS path
translation, and cross-check the commit the appliance reports against the one
requested. It overlaps `deploy-appliance.sh` in the no-ref case only.

Whether those two should merge is a **consolidation** question, not a naming
one, and it is left open deliberately rather than answered by a rename
`[GDE-DEP-095]`.

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

## 4. The names, applied 2026-09-11

| was | is now |
| :--- | :--- |
| `deploy-vainopi.sh` (build/) | [`build/deploy-appliance.sh`](../build/deploy-appliance.sh) |
| `deploy-player.sh` (VainoPi/) | [`build/install-player.sh`](../build/install-player.sh) |
| `deploy.sh` (VainoPi/) | unchanged — `[GDE-DEP-025]` |

`vainopi` remains the default host, so existing invocations keep working; only
the name and path changed. Every executable reference was updated in the same
commit, and the documentation citations with them, because `[GOV-DOC-040]`'s
path check makes a half-done rename fail rather than rot — the cost being
visible up front is the point `[GDE-DEP-080]`.

Historical records keep the name they were written with, annotated once:
`PI008`'s bringup narrative and `IMPL003`'s 2026-08-20 deploy both still say
`deploy-player.sh`, each with a pointer to where it went. Current-state specs
(`SPEC034`, `PI005`) name today's script, because they describe today's
system.

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

## 7. Configuration, and what is still open

**`[GDE-DEP-090]` Configuration now persists too, by the same rule.**
[`build/install-config.sh`](../build/install-config.sh) puts one file on an
appliance so that it is still there after a reboot: detect the root's actual
filesystem, announce which kind it found, write the live copy and — on an
overlay — the durable copy beneath it, then verify the **durable** one by
checksum and report whether the lower layer could be returned to read-only.

It closes the gap that mattered most. `[IMPL-BOS-185]` cost a binary, but what
actually stopped the music was a *unit file* that reverted, and no tool existed
to put one on a locked appliance at all. Used 2026-09-11 to restore `bose`'s
`vaino.service` together with the `vaino-preflight` and `vaino-db-recover`
helpers its `ExecStartPre` lines need — all three absent since the reboot, all
three now verified across one. `vaino-db-recover` replayed an un-checkpointed
4.1 MB WAL on `listener.db` the first time it ran, which is the clearest
possible argument that a missing helper is not a cosmetic absence.

**`[GDE-DEP-092]` Packages are still the manual case.** `install-config.sh`
moves *files*. A package needs `overlayroot-chroot` and an `apt` run
`[IMPL-BOS-180]`, which also needs a nameserver inside the chroot, because A's
own `resolv.conf` is the NetworkManager stub. `sqlite3` remains absent from
`bose` for this reason — recovery falls back to `python3` and says so, which is
`[GOV-SRC-030]` working, but the preferred tool is still missing.

**`[GDE-DEP-095]` Whether `deploy.sh` and `deploy-appliance.sh` should merge is
open.** They overlap only when no ref is named `[GDE-DEP-025]`; `deploy.sh`
additionally builds a named tag in a container-side worktree and cross-checks
the reported commit. Consolidating them is a design decision about what the
operator-facing entry point should be, and should not be settled by whoever
next touches either file.

**`[GDE-DEP-097]` `deploy.sh`'s final cross-check reads the live binary.** It
asks `/usr/local/bin/vaino --version` over ssh, which on an overlay host is the
ephemeral copy — the exact blind spot `[GDE-DEP-070]` names. It is not wrong
today, because `install-player.sh` has already verified the durable copy by
then, but it is the weaker of the two available checks and should read the
durable one.

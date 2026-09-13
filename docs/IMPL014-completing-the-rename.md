# IMPL014: Completing the Rename — the Fleet, then the New Repository

**Implementation Guide — planned 2026-09-13. Nothing here is executed until [IMPL013](IMPL013-executing-the-rename.md)'s phase 2 has landed on `main`.**

Phases 3 and 4: rolling the renamed build across the three appliances one at a
time, and then seeding a new repository that carries the functionality and the
documentation but not the old name. IMPL013 covers what breaks and why; this
covers everything after the repository itself is renamed.

The two halves differ in kind. Phase 3 is reversible by restarting what was
never deleted. Phase 4 is not reversible at all — it is a new repository, and
the decision it encodes is which parts of the record travel.

> **Related:** [IMPL013](IMPL013-executing-the-rename.md) `[IMPL-NAM-070]` — the phases and their gates · [tools/check_rename.py](../tools/check_rename.py) — the audit, which becomes the permanent guard `[IMPL-NAM-180]` · [GUIDE011](GUIDE011-deploy-script-naming.md) `[GDE-DEP-060]`, `[GDE-DEP-070]` · [BOSE010](../BosePi/BOSE010-changing-a-locked-card.md) — the overlay-root procedure

---

## 1. The appliance migration, per host

**`[IMPL-NAM-080]` One host at a time, install-beside then cut over, never a
rename in place.** For each of `vainopi`, `bose`, `vainoplayer3`, in that order —
`bose` second because its overlay root makes it the most likely to teach
something, and `vainoplayer3` last — both because its framebuffer UI is the
newest and least settled, and because it is physically unplugged until shortly
before this runs `[IMPL-NAM-115]`. Its catch-up deploy happens in phase 1; this
is its second visit, not its first.

1. Install the new binary, the thirteen renamed helpers and the new unit
   **alongside** the existing ones. Nothing is removed yet.
2. `systemctl stop` the old unit. Confirm stopped, not merely asked to stop.
3. Migrate `/var/vaino` → `/var/lempi` by **copy, verify, then remove** — never
   `mv` as the first act. It holds `listener.db`, `unlock/request`,
   `touch-calibration.toml` and `listener-backups/`.
4. Start the new unit. Confirm audio is actually playing, from the speaker.
5. `systemctl disable --now` the old unit and delete its file, then delete the
   old binaries. **A stale enabled unit is two processes contending for one
   audio device**, which presents as intermittent silence rather than as an
   error.
6. Re-run the host's own preflight `[PI-PRE-010]` and confirm it reports the new
   paths, with versions.
7. Commit whatever that host taught to `main` before starting the next one.

**`[IMPL-NAM-090]` On `bose`, every write goes through both layers.** `bose` has
an overlay root: an ordinary write to `/` lands in tmpfs, survives a restart,
passes every check, and is gone at the next reboot. Use
[build/install-config.sh](../build/install-config.sh) or
`sudo overlayroot-chroot`, and verify the durable copy under `/media/root-ro`,
never the running one `[GDE-DEP-070]`. `vainopi` and `vainoplayer3` have plain
writable roots and do not need this — and each script must say which it assumes
before acting.

---

## 2. The handshake that makes a wrong deploy loud

**`[IMPL-NAM-085]` Mid-rollout the fleet is mixed, so the deploy must refuse a
mismatch rather than proceed.** Between the first host's cutover and the last
one's, some appliances run `lempi.service` and some still run `vaino.service`.
Pushing the wrong build to a node is a one-word mistake, and by
`[IMPL-BOS-185]`'s lesson it can complete and report success while leaving a
machine that never plays again.

Before installing anything, the deploy script reads the unit name actually
installed on the target and compares it with the build in hand. On a mismatch it
**stops and names both** — "target `pi@bose` has `vaino.service`; this build
installs `lempi.service`; refusing" — rather than installing beside it or
guessing an upgrade path. A migration is `[IMPL-NAM-080]`'s deliberate
procedure, never a side effect of an ordinary deploy.

This guard is deleted in phase 4, once no node carries the old name. It exists
only for the window in which both are true at once.

---

## 3. Hostnames

**`[IMPL-NAM-100]` Hostnames move after their host's software, per machine, and
cannot be atomic with a commit.** `vainopi` and `vainoplayer3` both carry the
name. A hostname rename reaches well beyond this tree: `/etc/hostname` and
`/etc/hosts` on the box, its mDNS `.local` name, the `http://vaino/` entry point
on :80, SSH `config` and `known_hosts` on all four development machines,
`pi@vainopi` in the deploy scripts, the appliance documentation folder, and
assertion strings such as the one in [player/src/echo.rs](../player/src/echo.rs).

`bose` is unaffected — its name was never derived from the project's.

Because the repository and the machines cannot change in the same instant, each
host is renamed only after its §1 migration is verified, and the deploy scripts
accept both names until the last machine is done.

**`[IMPL-NAM-110]` The fleet list is already incomplete — fix it in phase 1,
before the rename propagates it.**
[build/deploy-everywhere.sh](../build/deploy-everywhere.sh) declares
`APPLIANCES="pi@vainopi pi@bose"`. `vainoplayer3` runs the player and is not in
it. That is the same gap that let `bose` sit four commits behind while `vainopi`
was kept current, and it means phase 1's "deploy and test the deployments"
cannot be complete as written until it is fixed.

---

## 4. The new repository

**`[IMPL-NAM-150]` Seeded from the verified final state, as a single initial
commit, after the last node is green.** The new repository is not a filtered
history — it is the working tree as it stands once phase 3 is complete,
committed once. The acceptance gate is
`check_rename.py --expect-zero` across **every** surface, with the historical
document `[IMPL-NAM-160]` as the sole allowlist entry.

This means IMPL013, this document, and
[GUIDE015](GUIDE015-naming-and-branding.md) **do not travel**. They are
migration working papers; their durable residue is one short note. Delete them
from the seed rather than rewriting them.

**`[IMPL-NAM-160]` One historical document, and it is a writing job.** A single
short file under `docs/` — proposed as `GUIDE016-the-earlier-names.md`, the
number being whatever is free at seed time — recording that development
proceeded for a time under earlier names, that a conflict search found
trademarked commercial products of the same names in the same categories, and
that the project renamed rather than contest them. It covers **both** renames,
the player's and the builder's, in one note: they were dropped for the same
reason, and separating them would put a second old name in a second file. It is
the only place in the new repository where either old name appears.

Everything else is rewritten to the current name rather than preserved under the
old one, per `[IMPL-NAM-130]`: the project is continuous and the earlier name was
temporary, so a dated finding reads "Lempi measured -2.09 ppm" with no asterisk,
and [GUIDE001](GUIDE001-lineage-and-lessons.md)'s lineage survives as
MuLibPlay → McRhythm → Lempi v1 → Lempi.

**`[IMPL-NAM-170]` Cited commits name the previous repository, never the earlier
project name.** Twenty-one commit SHAs are cited across the documents as
evidence for measured claims. In the new repository they resolve to nothing
locally, so each is rewritten to the form *"the previous Lempi development
repository, commit `b032f3b`"*.

The old name is deliberately absent from that phrasing. A reader searching it
alongside this project would find a commercial product of the same name and
infer a relationship that does not exist; the citation's job is to let the
maintainer verify a measurement, not to be a search term.

**`[IMPL-NAM-180]` The guard reads the forbidden tokens from the one document
that is allowed to contain them.** A checker cannot search for a string without
holding it, which would put an old name in a second file. Instead the guard
extracts **both** tokens from `[IMPL-NAM-160]`'s document at run time and fails
if either appears anywhere else in the tree. The name then lives in exactly one place, and
the guard stays permanently useful: it catches the name creeping back in through
a copied snippet or a restored file. It runs in CI alongside `check_docs.py`,
and like `check_rename.py` before it, **a surface it cannot scan reports BROKEN
rather than clean** `[IMPL-NAM-030]`.

**`[IMPL-NAM-190]` Licence text and attribution.** [LICENSE](../LICENSE) carries
`Copyright (c) 2026 Vaino Project Contributors` — a copyright notice, not
branding, and therefore changed deliberately rather than by substitution.
[LICENSING.md](../LICENSING.md) names both works and both licences: its table
becomes Lempi (MIT) and **Vipunen** (AGPL-3.0-or-later). The two-work,
two-licence split itself is unchanged — only the names are, and the AGPL work's
name changes with it. The 141 SPDX headers carry licence
identifiers only and need no edit.

A single initial commit authored as `MangoCat <mangocats@gmail.com>` is the whole
of the attribution work: the corporate address on seven 2026-09-03 commits exists
only in the old repository's commit metadata and appears in no file in the tree,
so it does not travel. The `Copyright (c) 2020 by Mike Inman` notices on the
inherited MuLibPlay sources carry a name and no address, and stay exactly as they
are.

**`[IMPL-NAM-200]` The old repository is made private, not deleted and not
rewritten.** Private, because a public archive would index the old name beside
this project and produce exactly the false association `[IMPL-NAM-170]` avoids,
and because the corporate address sits in its commit metadata. Not rewritten,
because changing author metadata rewrites every commit SHA and would invalidate
the twenty-one citations the archive is being kept *for*. Those two goals cannot
both be served by editing it; keeping it private serves both.

It stays reachable by the maintainer, which is who needs to verify a
measurement. `[IMPL-NAM-160]`'s document should say so, so that a reader who
finds an unresolvable SHA knows why rather than assuming rot.

---

## 5. Rollback

**`[IMPL-NAM-210]` Phase 3 rolls back by restarting what was never deleted;
phase 4 does not roll back.** At any moment in phase 3 at most one appliance is
mid-migration, and the two not yet touched are known-good references to compare
against. If a host fails at step 4, the old unit and binaries are still present
and still correct — re-enable them, and the migration is undone by starting what
was left in place. `[IMPL-NAM-080]`'s copy-verify-remove ordering is what makes
that true of the data as well.

Phase 4 has no rollback because it has nothing to roll back to: the new
repository is additive, and the old one continues to exist, privately, with its
history intact. The irreversible act is not creating the new repository — it is
deciding what the new repository omits, which is why `[IMPL-NAM-150]`'s gate runs
before the initial commit rather than after it.

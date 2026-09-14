# IMPL013: Executing the Vaino → Lempi Rename

**Implementation Guide — planned 2026-09-13. Nothing here is executed until the echo work lands.**

The order of work for the rename [GUIDE015](GUIDE015-naming-and-branding.md)
decided, the guards each step is gated on, and the surfaces that a text search
does not show. Written because `[GDE-NAM-110]`'s five-step sketch was a
*documentation* plan, and this system also has a compiled artifact, thirteen
installed helper executables, twenty-three environment variables, three live
appliances and mutable data on disk.

Four of the failures below are silent. That is the reason this document is
long rather than a checklist.

> **Related:** [GUIDE015](GUIDE015-naming-and-branding.md) `[GDE-NAM-010]` — why the name changes at all · [tools/check_rename.py](../tools/check_rename.py) — the audit every step is gated on · [GUIDE011](GUIDE011-deploy-script-naming.md) `[GDE-DEP-060]`, `[GDE-DEP-070]` — say what you assume, verify the durable copy · [BOSE009](../BosePi/BOSE009-image-update-runbook.md) `[BOS-RUN-080]` — the split this must not undo

---

## 1. What a text search does not show

**`[IMPL-NAM-010]` Sixteen surfaces, re-measured 2026-09-13 after the rehearsal
`[IMPL-NAM-079]` corrected the audit's extensionless blind spot.**
`[GDE-NAM-030]` counted 3,360 string occurrences. That number is a workload estimate, not a work
plan: what matters is which *kind* of thing breaks, because each kind fails
differently and is gated differently.

| Surface | Count | What breaks |
| :--- | ---: | :--- |
| `cargo` | 3 | Package `vaino-player`, binary `vaino` — every `ExecStart` downstream |
| `code` | 101 | The `vaino_player::` library path; build fails until every use site moves |
| `env` | 103 | **23 distinct `VAINO_*` variables** — see `[IMPL-NAM-040]` |
| `bin` | 43 | `/usr/local/bin/vaino` and thirteen `vaino-*` helpers |
| `units` | 18 | systemd units; a stale enabled unit contends for the audio device |
| `runtime` | 84 | `/var/vaino/`, `/srv/library/vaino*` — live data, a migration |
| `hosts` | 64 | `vainopi`, `vainoplayer3` — renamed per machine, never atomically |
| `scripts` | 890 | Superset of the four above |
| `pytools` | 893 | Including `check_docs.py`'s own `PATH_PREFIXES` — see `[IMPL-NAM-060]` |
| `pymod` | 262 | `vaino_db` / `vaino_control` and their 47 import sites `[IMPL-NAM-047]` |
| `vcs` | 10 | `.gitattributes` line-ending rule, `.gitignore` `[IMPL-NAM-045]` |
| `docs` | 1,720 | Prose, and the cited paths `[GOV-DOC-040]` validates |
| `builder` | 432 | **Sampo → Vipunen** in code, SQL and the console `[GDE-NAM-025]` |
| `builderdocs` | 433 | SPEC007's identity, LICENSING.md's two-work table, every doc naming the builder |
| `paths` | **105** | Tracked files and directories whose own *name* carries either |
| `remote` | 1 | `git@github.com:MangoCats/Vaino.git` |

The 105 is the one `[GDE-NAM-030]` understated. It listed four path renames;
there are a hundred and five, including `VainoPi/vaino-common.sh`,
`vaino-rocker.sh`, `vaino-speaker.sh` and `setup-vainopi.sh`.

**Two renames run together, on separate surfaces.** The builder moves Sampo →
Vipunen in the same migration `[GDE-NAM-025]`, because its register position is
worse than the player's was and because doing it later costs a second migration
of everything below. Its 865 occurrences are tracked apart from the player's so
that neither can mask a half-finished rename of the other — a single combined
count reaching zero would say nothing about which of the two got there.

---

## 2. The audit comes first

**`[IMPL-NAM-020]` [tools/check_rename.py](../tools/check_rename.py) is committed
before any rename begins, while the old name is still everywhere.** An audit
written afterwards that reports zero is indistinguishable from an audit that is
broken. Committing it first means its opening run has something to find, and
that run is the evidence it works.

It reports per surface, and gates a step:

```
python tools/check_rename.py                      # report every surface
python tools/check_rename.py --expect-zero code env
```

**`[IMPL-NAM-030]` A surface that matches no files reports BROKEN, not zero.**
After a directory rename a glob that no longer matches anything would otherwise
report clean — the precise failure `[IMPL-NAM-060]` describes. `check_rename.py`
treats an empty file set as a failure and says so, and `--expect-zero` refuses
to pass while any surface is broken.

The script found a defect in itself on its first run, recorded here because it
is the kind that makes an audit worse than none: a case-insensitive
`VAINO_[A-Z_]+` also matched `vaino_player`, inflating `env` from 51 to 192 by
double-counting `code`. An audit nobody can trust the numbers of will be
ignored at exactly the step that needed it.

---

## 3. The five failures this plan exists to prevent

**`[IMPL-NAM-040]` Twenty-three environment variables, most read with a
default.** Not the handful a manual read finds:

```
VAINO_ACTIVE  VAINO_BRANCH  VAINO_CHASE_SECONDS  VAINO_COMMIT_DATE
VAINO_COMMIT_SUBJECT  VAINO_COMMON  VAINO_DB  VAINO_DB_PRESENT
VAINO_DEPLOY_WAIT  VAINO_DIRTY_FILES  VAINO_FALLBACK_SECONDS  VAINO_FEATURES
VAINO_GIT  VAINO_LIBRARY_DB  VAINO_LISTENER_DB  VAINO_LONG_FILE
VAINO_NULL_OUTPUT  VAINO_PORT  VAINO_RUN_DIR  VAINO_SINK_WAIT
VAINO_SPEAKER  VAINO_SPEAKER_WAIT  VAINO_TICK_SECONDS
```

Five are compile-time (`VAINO_GIT`, `VAINO_BRANCH`, `VAINO_COMMIT_DATE`,
`VAINO_COMMIT_SUBJECT`, `VAINO_DIRTY_FILES`): emitted by
[player/build.rs](../player/build.rs), consumed through `env!()` in
[player/src/lib.rs](../player/src/lib.rs). Rename one side alone and **the build
fails loudly**. Those are safe.

The rest are the hazard. `VAINO_NULL_OUTPUT` is read as
`std::env::var(...).is_ok()` in [player/src/bin/play.rs](../player/src/bin/play.rs)
and [player/src/bin/station.rs](../player/src/bin/station.rs). A renamed variable
does not error — it returns `false` and **falls through to the real audio
device**. The timing group (`VAINO_CHASE_SECONDS`, `VAINO_TICK_SECONDS`,
`VAINO_SINK_WAIT`, `VAINO_FALLBACK_SECONDS`, `VAINO_SPEAKER_WAIT`) silently
reverts to built-in defaults, changing timing in the one subsystem where timing
is the whole subject.

**Every variable moves on both sides in one commit**, and the rename is followed
by `check_rename.py --expect-zero env`. Where a variable controls behaviour
rather than tuning, prefer reading it into an explicit `Option` and logging
which branch was taken, so an unset variable is visible in the log rather than
inferred from behaviour `[PI-PRE-010]`.

**`[IMPL-NAM-045]` A line-ending rule is keyed to the name, and losing it breaks
the appliance, not the build.** [.gitattributes](../.gitattributes) line 16
applies `text eol=lf` to the `vaino-*` scripts inside the appliance folder —
keyed to *both* the directory name and the `vaino-` prefix, so renaming either
one stops the rule matching. The scripts it protects
then get CRLF on a Windows checkout and fail on the Pi with
`bad interpreter: /bin/sh^M` — at deploy time, on a machine, long after the
commit that caused it. [.gitignore](../.gitignore) is keyed the same way
(`vaino.db`, `/.vaino-deploy-build.sh`, `data/vaino_new.db`); its `go/vaino.exe`
and `go/vaino-arm64` rules are already dead, as no `go/` directory exists, and
should be deleted rather than renamed.

**`[IMPL-NAM-047]` Renaming the shared Python modules breaks 47 imports and one
detector that will not complain.** `tools/vaino_db.py` and
`tools/vaino_control.py` are imported by 47 files as `import vaino_db`. Those
break loudly. The one that does not is
[tools/audit_split_readiness.py](../tools/audit_split_readiness.py), which
detects the dependency by string match — `"import vaino_db" in src` — and after
a rename reports `uses_vaino_db: False` for every file in the tree, which is
indistinguishable from a codebase that has been fully migrated off it. Rename
the modules, their import sites and that predicate in one commit, and prefer
matching the module by AST or by a named constant afterwards.

**`[IMPL-NAM-050]` The binary and its units must not be renamed in separate
commits.** `[GDE-NAM-110]`'s step 2 renamed the binary and its step 4 the units.
Between those commits `main` installs an executable called `lempi` under a unit
whose `ExecStart` is `/usr/local/bin/vaino`. The appliance fails to start, and
by `[IMPL-BOS-185]`'s lesson may well report success while doing it. Cargo
manifest, binary name, helper names, unit files and the install scripts are one
commit.

**`[IMPL-NAM-060]` Renaming `VainoPi/` without editing `check_docs.py` stops
governance silently.** [tools/check_docs.py](../tools/check_docs.py) hard-codes
the folder in two places: its `PATH_PREFIXES` tuple and a
`glob.glob("VainoPi/*.md")`. Rename the directory alone and roughly forty
documents leave governance entirely — their tags uncollected, their cited paths
unvalidated — while the run still prints `0 error(s)` and CI goes green. This is
`[GDE-DEP-060]` exactly: a guard that cannot run must say so loudly.

`check_docs.py` moves in the same commit as the directory, and gains an
assertion that every entry in `PATH_PREFIXES` exists on disk, so the next
rename fails instead of skipping.

---

## 4. The four phases

**`[IMPL-NAM-070]` Each phase is gated on a command, not on judgement.** "Green"
in `[GDE-NAM-110]` meant `check_docs.py`, which compiles nothing and runs no
test. Every gate below is explicit.

| Phase | Work | Gate before proceeding |
| :--- | :--- | :--- |
| **1** | Finish the echo work under the old names: merge current development to `main`, test, deploy, test the deployments. **Add `vainoplayer3` to the fleet list first** `[IMPL-NAM-110]` | All three appliances deployed and verified playing — **blocked until `vainoplayer3` returns** `[IMPL-NAM-115]` |
| **2a** | Commit `check_rename.py` unchanged, old name everywhere | Reports non-zero on every surface, 0 BROKEN |
| **2b** | Cargo package, crate path, binary, **all 23 env vars both sides**, helper names, unit files, the shared Python modules and their 47 importers, install and deploy scripts, sources, tests, fixtures | `env -u CC cargo build --release`, `cargo test` **and** the Python suite green; `--expect-zero cargo code env bin units pymod vcs` |
| **2c** | Docs prose, `VainoPi/` → `LempiPi/`, **`check_docs.py` prefixes**, every citation, the 105 named paths, `.gitattributes` and `.gitignore` | `check_docs.py --strict` exit 0 **and** `--expect-zero docs pytools paths` |
| **2d** | Sampo → Vipunen: `tools/` code and console, SQL, SPEC007's identity section, [LICENSING.md](../LICENSING.md)'s two-work table, the AGPL attribution | Python suite green; `--expect-zero builder builderdocs` |
| **3** | Roll out to the fleet, one node at a time | [IMPL014](IMPL014-completing-the-rename.md) §1-§3 |
| **4** | Initialise the new repository | [IMPL014](IMPL014-completing-the-rename.md) §4 |

Phase 2b is large deliberately — `[IMPL-NAM-050]` is what a smaller step would
cost. Phase 2d is separable because the builder shares no binary, no unit and no
runtime path with the player: by `[SPEC-SA-015]` the two communicate only through
the SQLite file, so renaming one cannot break the other mid-flight.

**`[IMPL-NAM-077]` Every mechanical edit states how many occurrences it expects
to change, and fails if the number differs.** `sed -i`, `perl -pi -e` and
Python's `str.replace` share one defect: **a pattern that matches nothing is not
an error to them.** They exit 0, change the file not at all, and report nothing.

Not hypothetical: a scripted edit while writing these documents searched for a
line an earlier edit had re-wrapped, matched nothing, and passed silently. The
count is **exact, not a minimum** — a pattern that should hit thirty lines and
hits three, because the rest wrapped differently or use another case, is a
half-finished rename that "at least one" reports as success.

[tools/rename_edit.py](../tools/rename_edit.py) makes the count part of the
command and refuses to write on a mismatch; it dry-runs by default, treats an
empty file set as BROKEN rather than as zero `[IMPL-NAM-030]`, treats an
unreadable file as a failure rather than as a file without matches, verifies
after writing that none survive, and preserves exact line endings so a rewrite
cannot undo `[IMPL-NAM-045]`'s LF rule. `check_rename.py`'s surface counts are
the coarse proof that a phase finished; this is the fine one that says which edit
did not.

**`[IMPL-NAM-079]` Rehearse the whole rename on a throwaway copy first.**
`git archive HEAD | tar -x -C <tmp>` gives a complete tree to destroy. Apply the
ordered substitutions and the path renames there, then run both checkers. The
first rehearsal, 2026-09-13, applied 4,995 substitutions across 12 ordered
patterns and renamed 36 paths, and found four defects that reading would not
have:

| Found | Consequence |
| :--- | :--- |
| The pass **rewrites the guards themselves** — `check_rename.py`'s own patterns became `lempi\|vipunen` | The audit inverts: it reports a successful rename as total failure and a failed one as clean. `ALLOW` excludes a file from being *scanned*, not from being *edited*; the mechanical pass needs its own never-rewrite list |
| Extension globs missed **16 extensionless files** — all 14 `vaino-*` helper executables, plus `LICENSE`; also `.css`, `.conf`, `.log`, `.bat` | Files renamed, contents untouched. The file list comes from `git ls-files` filtered by content, **never** a guessed extension list |
| `check_rename.py` had the same blind spot and reported `bin`/`scripts` clean while three helpers held 38 occurrences | Fixed: directory-scoped globs, and BROKEN is now **per-glob**, so one stale glob cannot hide behind a healthy sibling |
| A blanket pass produced **189 `lempi.db`** | Violates `[IMPL-NAM-120]`. Database names are excluded from substitution and triaged **before** the blanket pass, not renamed by it |

Two results worth keeping: the most-specific-first ordering is correct — `vaino`
is a substring of `vainopi` and `vainoplayer3`, and all twelve patterns applied
at exact counts — and **`check_docs.py --strict` passed with 0 errors** on the
rehearsed tree, so the documentation graph survives the path renames intact when
the checker moves with them `[IMPL-NAM-060]`.

The rehearsal also confirmed the converse: pointed at the rehearsed tree, the
fixed audit reports **8 surfaces BROKEN** because its globs still name
`VainoPi/`. Any guard whose globs name a directory this rename moves must move in
the same commit — `check_docs.py` and `check_rename.py` alike.

**`[IMPL-NAM-115]` Phase 1 cannot close while `vainoplayer3` is unplugged.** It
is physically disconnected, in the garage, and returns before this plan runs. Two
consequences, recorded so neither is rediscovered: phase 1's gate — *deploy and
test the deployments* — is **not satisfiable** until it is back, so the rename
does not start on a partially-verified fleet; and when it returns it will be many
commits behind, which is exactly the condition that let `bose` drift four commits
`[IMPL-NAM-110]`. It gets a catch-up deploy and verification of its own before it
counts as current.

Add it to the fleet list **now** rather than on its return, with a comment
marking it expected-absent. A node missing from the list is invisible; a node in
the list that cannot be reached is loud `[GDE-DEP-060]`, and being forgotten is
how it came to be missing in the first place.

**`[IMPL-NAM-075]` Phase 2 lands on `main` within hours, and the node rollout
proceeds from `main`.** The branch is short-lived on purpose. This repository
took **280 commits in the last seven days** across two worktrees and several
agents; the rename rewrites 387 files. A branch held open across a physical
three-node rollout would accumulate days of divergence against a change that
touches nearly every file, and the reconciliation would be done by whoever is
least placed to notice that a `VAINO_*` variable was silently reverted on one
side `[IMPL-NAM-040]`.

So: phase 2 is verified locally, merged to `main` and pushed in one short
window during which nothing else merges. Everything committed afterwards is
Lempi-named by default, and phase 3's per-node findings land on `main` as small
targeted commits that do not conflict with feature work.

Per `[GDE-DEP-060]`, each commit message states what it assumes about the
targets it touches. A rename that half-happened reads exactly like one that
fully happened.

---

## 5. Deliberately not renamed

**`[IMPL-NAM-120]` The database files keep their names.** The split
`[BOS-RUN-080]` already made them name-neutral: the live unit names
`listener.db` and `library.db`. The `vaino.db` references that remain are
mostly **pre-split** and therefore historical. A mechanical `vaino.db →
lempi.db` would rewrite obsolete text into fresh-looking text, making dead
documentation indistinguishable from current — and would rename live data for no
gain, since no listener ever sees the filename. Move `/var/vaino` → `/var/lempi`
and leave the files alone. Treat surviving `vaino.db` mentions as triage: delete
or mark historical, one at a time.

**`[IMPL-NAM-130]` Dated findings are rewritten to the current name, not
preserved under the old one.** The old name was temporary and the project is
continuous: this player *is* the one those measurements were taken on, so
"Lempi measured -2.09 ppm" is the true statement and carries no asterisk.
[GUIDE001](GUIDE001-lineage-and-lessons.md)'s lineage therefore survives almost
intact as MuLibPlay → McRhythm → Lempi v1 → Lempi. Only the naming conflict
itself needs the old name, and in the new repository that is the **single**
historical document `[IMPL-NAM-160]` — not an allowlist of four files.

---

## 6. Rollback

**`[IMPL-NAM-140]` Phases 1 and 2 revert with `git revert`; phases 3 and 4 do
not.** The repository phases are ordinary commits, and because phase 2 lands on
`main` as a small number of large commits rather than a long branch, reverting
is a single operation rather than an unpick. The machine and repository-cutover
phases are not revertible that way — [IMPL014](IMPL014-completing-the-rename.md)
§5 carries their own rollback, which is why phase 3 copies before it removes and
why the new repository is seeded only after the fleet is verified.

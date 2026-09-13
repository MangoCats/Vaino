# IMPL013: Executing the Vaino → Lempi Rename

**Implementation Guide — planned 2026-09-13. Nothing here is executed until the echo work lands.**

The order of work for the rename [GUIDE015](GUIDE015-naming-and-branding.md)
decided, the guards each step is gated on, and the surfaces that a text search
does not show. Written because `[GDE-NAM-110]`'s five-step sketch was a
*documentation* plan, and this system also has a compiled artifact, thirteen
installed helper executables, twenty-three environment variables, three live
appliances and mutable data on disk.

Two of the failures below are silent. That is the reason this document is long
rather than a checklist.

> **Related:** [GUIDE015](GUIDE015-naming-and-branding.md) `[GDE-NAM-010]` — why the name changes at all · [tools/check_rename.py](../tools/check_rename.py) — the audit every step is gated on · [GUIDE011](GUIDE011-deploy-script-naming.md) `[GDE-DEP-060]`, `[GDE-DEP-070]` — say what you assume, verify the durable copy · [BOSE009](../BosePi/BOSE009-image-update-runbook.md) `[BOS-RUN-080]` — the split this must not undo

---

## 1. What a text search does not show

**`[IMPL-NAM-010]` Twelve surfaces, measured 2026-09-13.** `[GDE-NAM-030]`
counted 3,360 string occurrences. That number is a workload estimate, not a work
plan: what matters is which *kind* of thing breaks, because each kind fails
differently and is gated differently.

| Surface | Count | What breaks |
| :--- | ---: | :--- |
| `cargo` | 3 | Package `vaino-player`, binary `vaino` — every `ExecStart` downstream |
| `code` | 101 | The `vaino_player::` library path; build fails until every use site moves |
| `env` | 51 | **23 distinct `VAINO_*` variables** — see `[IMPL-NAM-040]` |
| `bin` | 35 | `/usr/local/bin/vaino` and thirteen `vaino-*` helpers |
| `units` | 18 | systemd units; a stale enabled unit contends for the audio device |
| `runtime` | 39 | `/var/vaino/`, `/srv/library/vaino*` — live data, a migration |
| `hosts` | 27 | `vainopi`, `vainoplayer3` — renamed per machine, never atomically |
| `scripts` | 397 | Superset of the four above |
| `pytools` | 893 | Including `check_docs.py`'s own `PATH_PREFIXES` — see `[IMPL-NAM-060]` |
| `docs` | 1,699 | Prose, and the cited paths `[GOV-DOC-040]` validates |
| `paths` | **99** | Tracked files and directories whose own *name* carries it |
| `remote` | 1 | `git@github.com:MangoCats/Vaino.git` |

The 99 is the one `[GDE-NAM-030]` understated. It listed four path renames; there
are ninety-nine, including `VainoPi/vaino-common.sh`, `vaino-rocker.sh`,
`vaino-speaker.sh` and `setup-vainopi.sh`.

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

## 3. The three failures this plan exists to prevent

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

## 4. Order of work

**`[IMPL-NAM-070]` Six steps, each gated on a command rather than on judgement.**
"Green" in `[GDE-NAM-110]` meant `check_docs.py`, which compiles nothing and runs
no test. Every gate below is explicit.

| # | Step | Gate before proceeding |
| :--- | :--- | :--- |
| 0 | Commit `check_rename.py` unchanged, old name everywhere | It reports non-zero on every surface, 0 BROKEN |
| 1 | Cargo package, crate path, binary, **all 23 env vars both sides**, helper names, unit files, install and deploy scripts, sources, tests, fixtures | `env -u CC cargo build --release` **and** `cargo test` green; `--expect-zero cargo code env bin units` |
| 2 | Docs prose, `VainoPi/` → `LempiPi/`, **`check_docs.py` prefixes**, every citation, the 99 named paths | `check_docs.py --strict` exit 0 **and** `--expect-zero docs pytools paths` |
| 3 | Per appliance: migrate binaries, helpers, units, `/var/vaino` | §5's per-host procedure, verified on the durable copy |
| 4 | Per machine: hostname, `/etc/hosts`, mDNS, then SSH config on the four dev checkouts | Host reachable under the new name; deploy list updated |
| 5 | GitHub remote, and re-point the four checkouts | `--expect-zero remote` |

Step 1 is large and deliberately so — `[IMPL-NAM-050]` is what a smaller step
would cost. Steps 3 and 4 are per machine and never atomic with a commit, so the
tree must tolerate both names throughout them.

Per `[GDE-DEP-060]`, each commit message states what it assumes about the
targets it touches. A rename that half-happened reads exactly like one that
fully happened.

---

## 5. The appliance migration, per host

**`[IMPL-NAM-080]` One host at a time, install-beside then cut over, never a
rename in place.** For each of `vainopi`, `bose`, `vainoplayer3`:

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

**`[IMPL-NAM-090]` On `bose`, every write goes through both layers.** `bose` has
an overlay root: an ordinary write to `/` lands in tmpfs, survives a restart,
passes every check, and is gone at the next reboot. Use
[build/install-config.sh](../build/install-config.sh) or
`sudo overlayroot-chroot`, and verify the durable copy under `/media/root-ro`,
never the running one `[GDE-DEP-070]`. `vainopi` and `vainoplayer3` have plain
writable roots and do not need this — and each script must say which it assumes
before acting.

---

## 6. Hostnames

**`[IMPL-NAM-100]` Hostnames move last, per machine, and cannot be atomic with a
commit.** `vainopi` and `vainoplayer3` both carry the name. A hostname rename
reaches well beyond this tree: `/etc/hostname` and `/etc/hosts` on the box, its
mDNS `.local` name, the `http://vaino/` entry point on :80, SSH `config` and
`known_hosts` on all four development machines, `pi@vainopi` in the deploy
scripts, the `VainoPi/` documentation folder, and assertion strings such as the
one in [player/src/echo.rs](../player/src/echo.rs).

Because the repository and the machines cannot change in the same instant, each
host is renamed only after its §5 migration is verified, and the deploy scripts
must accept both names until the last machine is done.

**`[IMPL-NAM-110]` The fleet list is already incomplete — fix it before
propagating it.** [build/deploy-everywhere.sh](../build/deploy-everywhere.sh)
declares `APPLIANCES="pi@vainopi pi@bose"`. `vainoplayer3` runs the player and is
not in it. That is the same gap that let `bose` sit four commits behind while
`vainopi` was kept current. Add it first, so the rename does not carry an
incomplete inventory forward.

---

## 7. Deliberately not renamed

**`[IMPL-NAM-120]` The database files keep their names.** The split
`[BOS-RUN-080]` already made them name-neutral: the live unit names
`listener.db` and `library.db`. The `vaino.db` references that remain are
mostly **pre-split** and therefore historical. A mechanical `vaino.db →
lempi.db` would rewrite obsolete text into fresh-looking text, making dead
documentation indistinguishable from current — and would rename live data for no
gain, since no listener ever sees the filename. Move `/var/vaino` → `/var/lempi`
and leave the files alone. Treat surviving `vaino.db` mentions as triage: delete
or mark historical, one at a time.

**`[IMPL-NAM-130]` Historical documents keep the old name.**
[GUIDE001](GUIDE001-lineage-and-lessons.md) records a lineage in which this
project *was* Vaino; rewriting it would falsify the record. GUIDE015, this
document and `check_rename.py` likewise. These are the audit's allowlist, and
the list is part of the script rather than a convention, so a future reader can
see exactly which occurrences are meant to survive.

---

## 8. Rollback

**`[IMPL-NAM-140]` Steps 0–2 revert with `git revert`; steps 3–5 do not.** The
repository steps are ordinary commits. The machine steps are not, which is why
step 3 copies before it removes and why each host is finished and verified
before the next is started: at any moment at most one appliance is mid-migration,
and the previous host is a known-good reference to compare against. If a host
fails at step 3.4, the old unit and binaries are still present and still
correct — re-enable them, and the migration is undone by starting what was never
deleted.

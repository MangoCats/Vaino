# BOSE009: Executing the bose Update — Order, Rollback, and What Breaks

**Implementation Guide — the runbook, written by trying to break the plan**

[BOSE008](BOSE008-image-update-plan.md) says what to change and why. This
says how, in what order, and what happens when a step fails — because the
plan as first written **would not have produced a working system**, and the
reasons are worth keeping rather than quietly fixing.

Asked directly: *will we end with a working system?* Yes, if executed in the
order below. Not if executed as `BOSE008` originally read.

> **Related:** [BOSE008](BOSE008-image-update-plan.md) for the decisions this
> executes · [PI025 `[PI-OWE-040]`](../VainoPi/PI025-what-the-local-split-owes-vainopi.md)
> for the ordering trap this inherits · [BOSE005](BOSE005-power-loss-test.md)
> for the power cut that can interrupt any step

---

## 1. What was verified safe

**`[BOS-RUN-010]` One binary genuinely serves both appliances.** The largest
unexamined risk, since they are now on different Debian generations. The
cross-build image is `rust:1.90-bookworm` (glibc 2.36); the deployed binary
imports nothing above `GLIBC_2.34`; `vainopi` has 2.36 and `bose` has 2.41.
Building against the **older** distribution is what makes that work, and it
is load-bearing rather than incidental: bumping `build/Dockerfile.aarch64`
to trixie would produce a binary that runs on `bose` and refuses to start on
`vainopi`, with no warning at build time.

**`[BOS-RUN-015]` `sqlite3` installs cleanly.** `apt-get install -s sqlite3`
on `bose`: one package, `3.46.1-7+deb13u1`, no dependencies pulled, nothing
upgraded, nothing removed. The player is unaffected either way — it links
SQLite **bundled** `[REQ-HW-145]`, so the system library is not its concern.

**`[BOS-RUN-020]` Writes persist, and the deploy keeps a rollback.** `bose`'s
root overlay is disk-backed, verified by a binary that predates the last
boot and survived it. `deploy-player.sh` checksums before and after upload
and leaves the previous binary at `/usr/local/bin/vaino.prev`.

**`[BOS-RUN-025]` `split_database.py` cannot damage the source.** It refuses
to overwrite an existing output, rehearses by default, verifies row counts
table-for-table plus `integrity_check` plus every index before reporting
success, and never modifies the file it reads.

## 2. What would have broken

**`[BOS-RUN-030]` `BOSE008 §5`'s commands cannot run as written.** They read
as though issued on `bose`. There is **no repository checkout on `bose`**,
and `tools/load_occasions.py` now imports `vaino_db`, so copying that one
file over fails on import. `tools/backfill_profanity.py` additionally reads
`mulib.db`, which is 95 MB and lives only on the desktop.

Corrected mechanism, which is the one `vainopi` was actually given:

```
# ship both files, not one
scp tools/load_occasions.py tools/vaino_db.py pi@bose:/tmp/
ssh pi@bose "python3 /tmp/load_occasions.py /var/vaino/vaino.db --write"

# profanity travels as a patch, so mulib.db never does
python tools/backfill_profanity.py data/library.db \
    --mulib ../MuLibPlay/mulib.db --sql-out /tmp/profanity.sql
scp /tmp/profanity.sql pi@bose:/tmp/
ssh pi@bose "sqlite3 /var/vaino/vaino.db < /tmp/profanity.sql"
```

The patch is self-guarding — every statement carries its own
`WHERE EXISTS` against `recordings` — so it is checked by `bose`'s catalogue
rather than by the desktop's `[SPEC-PREF-082]`.

**`[BOS-RUN-035]` Splitting before deploying would create shadow tables.**
`bose` runs `358c5b17`, which predates `1a7e100` — the commit that stopped
`PlayerStore::open(&db)` from telling the connection that the listener half
is the whole database. On the old build, a split `bose` would create empty
`file_tags` and `cover_art` in its listener half on **every start**, exactly
as `vainopi` did `[PI-OWE-040]`.

This is the same trap, on the same evidence, one appliance later. **Deploy
first.**

**`[BOS-RUN-040]` Every database write needs the service stopped.** The
player holds `vaino.db` open and writes to it continuously. `BOSE008` did
not say so. Each write step below stops `vaino` and starts it again.

**`[BOS-RUN-045]` `vaino-wifi-revert` must not be ported.** `BOSE008 §6`
listed it as hardware-neutral, which is true and beside the point: it is
scheduled **only** by `vaino-btctl`'s `wifi-connect`/`ap-start`/`ap-stop`
verbs via `systemd-run`, and `vaino-btctl` is not being ported. Installed
alone it is a script nothing ever calls — harmless, but it would leave
`bose` looking as though it had Wi-Fi revert protection when it has none,
which is worse than plainly not having it. Porting the scheduler instead
means bringing `[SPEC034]`'s whole Wi-Fi/AP feature to `bose`, which is a
separate decision this plan should not smuggle in.

## 3. The order

**`[BOS-RUN-050]`** Each step ends with a working system, so stopping after
any of them is safe.

| # | Step | Rollback |
| :--- | :--- | :--- |
| 0 | Back up `vaino.db` off-device (`VACUUM INTO`, then `scp`) | — |
| 1 | `apt-get install -y sqlite3` | `apt-get remove sqlite3`; nothing depends on it |
| 2 | `VainoPi/deploy.sh <tag> pi@bose` | `sudo cp /usr/local/bin/vaino.prev /usr/local/bin/vaino && systemctl restart vaino` |
| 3 | Stop `vaino`; ship and run `load_occasions.py`; apply `profanity.sql`; start `vaino` | restore the step-0 backup |
| 4 | Install the three neutral helpers (`vitals`, `underruns`, `startup-sample`) | delete them; nothing references them |
| 5 | **Split**, as one transaction — see §4 | point the unit back at `vaino.db`, which is untouched |

**`[BOS-RUN-055]`** Steps 1–4 are independent and individually reversible.
Step 5 is the only one that changes how the appliance is *shaped*, and it is
the only one with a sequencing requirement inside it.

## 4. Step 5, the split, in full

**`[BOS-RUN-060]`** Do these five together or none of them. Between the
second and the fifth, `bose` is a machine whose unit does not match its
databases.

1. `systemctl stop vaino`
2. `split_database.py /var/vaino/vaino.db --library-out … --listener-out … --commit`
   — needs `sqlite3`? No: it is stdlib Python, so it runs wherever python3
   is, which `bose` has. **Keep `vaino.db`**; it is the rollback.
3. `install -m755 vaino-db-recover /usr/local/bin/` — mandatory, not
   optional: without it the first power cut after the split is a crash loop
   `[PI3-FOUND-120]`, and this machine is cut from a speaker's switch.
4. Add `ExecStartPre=/usr/local/bin/vaino-db-recover` and change `ExecStart`
   to the two-path form; `systemctl daemon-reload`.
5. `systemctl start vaino`, then **hear it play** before calling it done
   `[IMPL-BOS-120]`.

**`[BOS-RUN-065]` A power cut mid-split is survivable at every point**,
which is worth stating on a machine that is switched off by having its power
removed. Before step 4 the unit still names `vaino.db`, which
`split_database.py` never modified, so the appliance boots exactly as it did
before. After step 4 it names the two new files, which exist and verified
clean. The window where neither is true does not exist.

## 5. Open

**`[BOS-RUN-070]`** Not executed. The answer to "will we end with a working
system" is yes **in this order**, and the ordering is the whole of the
answer — `[BOS-RUN-035]` alone would have produced a `bose` quietly
manufacturing shadow tables on every boot.

**`[BOS-RUN-075]`** `[BOS-RUN-010]`'s glibc floor is undefended: nothing
fails at build time if `build/Dockerfile.aarch64` is bumped past bookworm,
and the failure appears only when `vainopi` refuses to start. A check that
the built binary imports no symbol above the oldest deployed appliance's
glibc would close it.

---

**Traceability:** `[BOS-RUN-010..075]` · executes
[BOSE008](BOSE008-image-update-plan.md) · inherits `[PI-OWE-040]`'s
deploy-before-drop ordering as deploy-before-split · corrects `BOSE008 §5`'s
commands and `§6`'s `vaino-wifi-revert` recommendation

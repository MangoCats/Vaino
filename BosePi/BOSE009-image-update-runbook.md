# BOSE009: Executing the bose Update — Order, Rollback, and What Breaks

**Implementation Guide — the runbook, written by trying to break the plan**

[BOSE008](BOSE008-image-update-plan.md) says what to change and why. This
says how, in what order, and what happens when a step fails — because the
plan as first written **would not have produced a working system**, and the
reasons are worth keeping rather than quietly fixing.

Asked directly: *will we end with a working system?* Yes, if executed in the
order below. Not if executed as `BOSE008` originally read.

> **Executed 2026-09-11, and it does.** `bose` runs `5272e3f`, split, both
> halves WAL and integrity-clean, playing. §2's three corrections were all
> real. Executing it found two more the review had missed — `[BOS-RUN-080]`
> and `[BOS-RUN-085]` — which is the argument for doing this with the
> service stopped and one step at a time rather than as a script.

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
| 5 | **Split**, as one transaction — see §4 | point the unit back at `vaino.db`, which is untouched; `vaino.service.pre-split` is kept beside the unit |

**`[BOS-RUN-055]`** Steps 1–4 are independent and individually reversible.
Step 5 is the only one that changes how the appliance is *shaped*, and it is
the only one with a sequencing requirement inside it.

## 4. Step 5, the split, in full

**`[BOS-RUN-060]`** Do these five together or none of them. Between the
second and the fifth, `bose` is a machine whose unit does not match its
databases.

1. `systemctl stop vaino`
2. Split **inside an attended window**, because the halves do not land on the
   same partition — `[BOS-RUN-080]`:

   ```
   bash BosePi/attended-import.sh --check -- ssh pi@bose \
       "python3 /tmp/split_database.py /var/vaino/vaino.db \
        --library-out /srv/library/library.db \
        --listener-out /var/vaino/listener.db --commit"
   ```

   then the same with `--go --no-mpd-update`. `split_database.py` is stdlib
   Python and runs wherever python3 is. **Keep `vaino.db`**; it is the
   rollback.
3. `install -m755 vaino-db-recover /usr/local/bin/` — mandatory, not
   optional: without it the first power cut after the split is a crash loop
   `[PI3-FOUND-120]`, and this machine is cut from a speaker's switch.
4. Add `ExecStartPre=/usr/local/bin/vaino-db-recover` and change `ExecStart`
   to the two-path form; `systemctl daemon-reload`.
5. `systemctl start vaino`, then **hear it play** before calling it done,
   which is `[IMPL-BOS-120]`'s own rule and not a new one.

**`[BOS-RUN-065]` A power cut mid-split is survivable at every point**,
which is worth stating on a machine that is switched off by having its power
removed. Before step 4 the unit still names `vaino.db`, which
`split_database.py` never modified, so the appliance boots exactly as it did
before. After step 4 it names the two new files, which exist and verified
clean. The window where neither is true does not exist.

## 5. What executing it found that reviewing it did not

**`[BOS-RUN-080]` The two halves do not go on the same partition, and the
runbook's own §4 had them doing so.** `[IMPL-BOS-078]` settled this before
either document existed: `vaino.db` sits on C *"until the split is built"*,
and the catalogue belongs on B. That is not a preference — C is a 4 GB f2fs
partition with 2.6 GB free, and `library.db` is 1.17 GB, so putting both
halves there alongside the retained original would have left roughly 200 MB
on the partition that takes every write this appliance makes. B has 56 GB.

B is also genuinely `ro`, which C is not, so writing it needs
`BosePi/attended-import.sh` — the remount-run-restore window
`[IMPL-BOS-150]` already built. Used with `--check` first, as that script
insists.

This also quietly corrects `BOSE008 [BOS-IMG-050]`'s claim that `bose` "has
one writable filesystem". It has two partitions with different postures, and
that is closer to `vainopi`'s intent than `vainopi` itself manages — B here
really is read-only, where `vainopi`'s equivalent just stays `rw` forever.

**`[BOS-RUN-085]` Two of the three helpers are inert without their units.**
`vaino-vitals` and `vaino-startup-sample` each have a
`/etc/systemd/system/*.service` on `vainopi`; installed as bare scripts they
are exactly the half-a-mechanism mistake `[BOS-RUN-045]` had just caught
with `vaino-wifi-revert`, and the review made it again one paragraph later.
Both units are now on `bose` with `vainopi`'s own enablement: `startup-sample`
**enabled** (bounded — it samples the first minutes after a boot),
`vaino-vitals` **disabled** (an infinite sampler, started by hand when
something is being investigated). `vaino-underruns` genuinely is standalone
and needs no unit.

**`[BOS-RUN-090]` A WAL catalogue on read-only media is safe only while it
is clean.** `library.db` is WAL, inherited from the source by
`split_database.py`, and it now lives on a partition that is `ro` in normal
operation. Measured: a cleanly-closed WAL database removes its sidecars and
opens read-only without them, and `bose`'s `library.db-wal` is 0 bytes, so
the current state is sound.

The narrow risk is an attended window that closes with frames still
un-checkpointed: B would go `ro` carrying a dirty `-wal` that no read-only
open can replay. `attended-import.sh` `sync`s, which is not the same as a
checkpoint. Any future window that writes `library.db` should end with
`PRAGMA wal_checkpoint(TRUNCATE)` before B closes — noted in §6 rather than
built, since nothing writes that file today.

## 6. Open

**`[BOS-RUN-070]` Executed 2026-09-11.** The ordering was the whole of the
answer: `[BOS-RUN-035]` alone would have produced a `bose` quietly
manufacturing shadow tables on every boot, and the post-split check found
none.

**`[BOS-RUN-078]` `attended-import.sh` should checkpoint before closing B**
`[BOS-RUN-090]`. Not built: nothing writes `library.db` today, and the one
window that did leave it clean. It becomes real the first time an import
touches the catalogue rather than only the audio.

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

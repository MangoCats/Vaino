# PI001: VainoPi Image and Partition Design

**Design Specification — Tier 2 · PROVISIONAL**

How a Raspberry Pi Zero 2W is laid out so that pulling its power is a normal
event rather than an incident. Implements `[REQ-HW-020]`'s three-partition
model and the storage discussion in [embedded-hardware.md](embedded-hardware.md).

> **Related:** [IMPL001 appliance setup](IMPL001-appliance-setup.md) ·
> [REQ001 §HW](../docs/spec/REQ001-system-requirements.md) ·
> [SPEC006 data flow](../docs/spec/SPEC006-data-flow-and-portability.md) ·
> [SPEC008 schema](../docs/spec/SPEC008-database-schema.md)

---

## 1. The problem being solved

An appliance has no shutdown button anybody uses. It is switched off at the
wall, mid-track, mid-write, and is expected to come back playing. Everything
below follows from taking that literally.

**`[PI-PART-010]` The failure to design against is a write in flight, not a
disk wearing out.** An interrupted write can corrupt a filesystem structure,
not merely lose the bytes being written. So the layout's job is to ensure that
at any instant, the number of partitions with a write in progress is as close
to zero as it can be, and that whatever *was* being written is the thing we
can most afford to lose.

**`[PI-PART-020]` Three partitions, ordered by how often they are written.**

| | Mount | Written | On corruption |
| :--- | :--- | :--- | :--- |
| **A — system** | `/` read-only + overlay | only on software update | reflash |
| **B — library** | `/srv/library` ro, rw during import | only on music import | rebuild from Sampo |
| **C — state** | `/var/vaino` read-write | continuously | reinitialise; the player still runs |

The ordering is the design. A is never written, so it cannot be corrupted. B
is written on a deliberate, attended action. C absorbs every unattended write
in the system, and is the one partition whose total loss is survivable.

---

## 2. Partition A — system and application

**`[PI-A-010]` Read-only root with an overlay**, the arrangement `raspi-config`
calls Overlay File System. Writes land in a RAM overlay and are discarded at
reboot; the SD card is never written during normal running.

**`[PI-A-020]` The overlay is RAM, and RAM is the scarce resource.** A Pi Zero
2W has 512 MB, and `[REQ-HW-010A]` budgets the player under 30 MB. An overlay
that accumulates writes competes directly with the audio buffers. Everything
that writes routinely must therefore be moved *off* the overlay rather than
allowed to fill it:

- `/var/log` → bind-mounted to partition C. Left alone this is the single
  largest consumer, and it grows without bound on a machine that never reboots.
- `/tmp` → `tmpfs` with an explicit `size=` cap, so a runaway cannot take the
  memory the mixer needs.
- systemd journal → `Storage=volatile`, `RuntimeMaxUse=8M`.

**`[PI-A-030]` Nothing about the library lives here.** The binary, the unit
file and the OS. A software update is a new image or an `rw` remount performed
deliberately, never a background package upgrade.

---

## 3. Partition B — library

**`[PI-B-010]` Mounted read-only; remounted read-write only for an import.**
`ext4`, journalled. The music files and everything derived from them.

**`[PI-B-020]` This partition is a cache, not a source** `[SPEC-SC-010]`.
Every byte on it re-derives from the audio and from Sampo, so its corruption
costs time rather than data. That is what permits a plain filesystem here
instead of anything exotic.

Contents:

- the audio files themselves;
- `library.db` — see §5;
- cover art, which lives in `library.db` rather than beside the audio because
  the media is read-only at runtime and cannot be written to `[REQ-VIS-170]`.

**`[PI-B-030]` An import is an attended state change.** Remount `rw`, run the
ingest, `sync`, remount `ro`. The window in which this partition can be
corrupted is the window in which someone is standing there.

---

## 4. Partition C — listener state

**`[PI-C-010]` The only partition written while unattended, and the only one
whose loss is survivable.** Every unattended write in the system lands here by
construction, which is what keeps A and B safe.

Contents: `listener.db` (§5), logs, and the backup snapshots `[REQ-LIB-160]`
already produces.

**`[PI-C-020]` It holds the only irreplaceable data in the system**
`[SPEC-SC-090]` — 37,237 plays, 3,261 preferences, the programmes. Nothing
re-derives it. This is the tension the whole design turns on: the most
volatile partition carries the least replaceable data.

Three things follow.

**`[PI-C-030]` Off-device backup is not optional here.** `[REQ-LIB-160]`'s
snapshots must be copied off the Pi, not merely written to partition C, or
they share the fate of what they protect.

**`[PI-C-040]` The player must start without it.** A corrupt or absent
partition C means: recreate an empty `listener.db`, log loudly, and play. A
Vaino that refuses to make sound because it has lost its play counts has
mistaken its own bookkeeping for its purpose.

**`[PI-C-050]` `synchronous=FULL` here, unlike the library.** WAL with
`synchronous=NORMAL` can lose the last transactions on power loss. For the
library that is meaningless — it is a cache. For the listening it is the loss
of exactly the data nothing can rebuild, so this partition pays the fsync.

---

## 5. The database split

**`[PI-DB-010]` One file becomes two, along a line the schema already draws.**
`[SPEC-SC-020]` segregates listener state by the `listener_` prefix precisely
so the class-D export is a table-set selection rather than a per-column
judgement. That same line is the partition boundary.

| File | Partition | Tables | Size today |
| :--- | :--- | :--- | ---: |
| `library.db` | B (ro) | `files`, `passages`, `passage_recordings`, `recordings`, `artists`, `releases`, `release_recordings`, `flavor`, `cover_art`, `file_tags`, `id_checks`, the caches | ~1,020 MB |
| `listener.db` | C (rw) | `listener_*`, `player_state`, `id_reviews`, `selection_decisions` | ~2.4 MB |

**The ratio is the point: 0.2%.** The partition being written continuously is
a thousandth the size of the one that is not.

**`[PI-DB-020]` The player opens `listener.db` and ATTACHes `library.db`
read-only.** Not the reverse. The writable connection is the one the player
owns; the library arrives as a read-only attachment, which makes
`[SPEC-SA-015]`'s guard a filesystem fact rather than a convention:

```sql
-- on the player's connection
ATTACH DATABASE 'file:/srv/library/library.db?mode=ro' AS lib;
PRAGMA main.synchronous = FULL;      -- listener state, per [PI-C-050]
PRAGMA main.journal_mode = WAL;
```

**`[PI-DB-030]` Four queries cross the boundary and must be schema-qualified.**
This is the real work of the split, and it is not cosmetic — an unqualified
name silently resolves to `main` and would read an empty table rather than
failing:

- `PLAYS_EXPR` — counts `listener_play_history` against `lib.recordings`;
- `review_queue` — joins `lib.id_checks` to `main.id_reviews`;
- `backup::restore` — re-points history through `lib.passage_recordings`;
- the Director's taste centroids — `listener_likes` against `lib.flavor`.

**`[PI-DB-040]` Sampo writes `library.db` on a desktop and never on the Pi.**
Which is already true, and the split makes it enforceable rather than merely
intended.

---

## 6. Toward an image build

**`[PI-IMG-010]` The deliverable is a script, not a procedure.** A documented
sequence of manual steps is a procedure nobody performs identically twice.
`VainoPi/build-image.sh` should take a Pi OS Lite image and a prepared
`library.db` and produce a flashable `.img`.

Sketch, in the order the steps depend on each other:

1. start from Pi OS Lite (64-bit, Bookworm), `aarch64`;
2. repartition: A ~4 GB, C ~1 GB, B the remainder;
3. install `vaino` (built via `build/Dockerfile.aarch64`) and its unit file;
4. `/var/log` → bind mount to C; journal volatile; `/tmp` capped;
5. seed B with `library.db` and the audio; seed C with an empty `listener.db`;
6. enable the overlay on A **last** — every step above needs a writable root;
7. first-boot service: verify C, recreate it if absent `[PI-C-040]`, start
   playing before the network is up `[REQ-HW-010B]`.

**`[PI-IMG-020]` Order matters at step 6.** Enabling the overlay before the
seeding is done leaves an image that boots, appears correct, and has silently
discarded everything written after the overlay went on.

---

## 7. What is not yet decided

- **Filesystem for C.** `ext4` with `data=journal` is the conservative choice;
  `f2fs` is designed for exactly this wear pattern. Unmeasured either way.
- **Whether B holds the audio at all.** A USB stick or network share would
  make B small and the image portable between libraries.
- **No ARM64 build has been produced or run.** `build/Dockerfile.aarch64`
  exists as a build target and has not been exercised this session. Every
  measurement in this project so far is from x86 Windows, and nothing here is
  validated on the target hardware.

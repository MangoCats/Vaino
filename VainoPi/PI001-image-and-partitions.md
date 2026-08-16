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

**`[PI-DB-035]` The boundary is defaults on one side, the listener's answer on
the other.** Several values exist in both files and mean different things:

| | `library.db` (B) | `listener.db` (C) |
| :--- | :--- | :--- |
| Taste centroids | derived defaults | saved, user-editable |
| Artist / track cooldowns | defaults from ingest | `listener_preferences`, edited |
| Like / dislike | — | `listener_likes`, entirely the listener's |

So a read is layered: take the default from `lib`, override it with the row in
`main` if there is one. That is why the *writable* file is the one the player
opens — the override always has somewhere to go, even with the library
mounted read-only.

It also means a reinitialised partition C is not a broken system but a
**factory-reset** one: every default is still present in `library.db`, and
what is lost is the listener's accumulated opinion. That is a real loss
`[PI-C-020]` and precisely why C is backed up off-device, but it is a
different kind of loss from a system that will not run.

Like/dislike `[REQ-PD-150]` is unbuilt, and belongs on this side of the line
when it is written — it is the listener's judgement, and nothing re-derives it.

**`[PI-DB-040]` Sampo writes `library.db` on a desktop and never on the Pi.**
Which is already true, and the split makes it enforceable rather than merely
intended.

---

## 5a. Filesystem for partition C: ext4 vs f2fs

**`[PI-FS-010]` `data=journal` is the wrong instinct here, and it is worth
saying why.** It looks like the safest option — journal the data as well as
the metadata, so a torn write cannot leave a half-updated file. But it doubles
every write, and it is largely redundant with what SQLite is already doing.

SQLite in WAL mode with `synchronous=FULL` `[PI-C-050]` already guarantees
that a committed transaction survives power loss: it writes the WAL frame,
fsyncs, and only then reports success. What the filesystem must supply is
(a) metadata that is not corrupted by an interrupted write, and (b) an
`fsync` that is honest. **Metadata journalling gives both**; `data=journal`
adds a second copy of bytes SQLite has already made durable itself.

On an SD card the cost is not theoretical. Flash erases in blocks far larger
than a SQLite page, so the controller already amplifies small writes; doubling
them at the filesystem layer compounds it, and the write pattern here is a
steady drip that never stops.

**`[PI-FS-015]` How steady is now a setting.** The resume point is written on
an interval that defaults to **5 seconds** and is adjustable from the Vaino
skin's settings panel, 1 s to 5 min `[REQ-VIS-155]`. On an appliance this is
the single largest source of unattended writes, so it is the one number that
most directly decides how much of partition C's life is spent with a write in
flight — and it belongs to the installation, not to the source.

Lengthening it costs bounded and small: at most that much playback position
after a power cut. Passage changes, pause and resume bypass the interval and
are written the moment they happen, so what the setting trades is a few
seconds of position, never an event.

**`[PI-FS-020]` The real comparison is ext4 `data=ordered` against f2fs.**

| | ext4 (`data=ordered`, the default) | f2fs |
| :--- | :--- | :--- |
| Design target | spinning disks and SSDs alike | flash with an FTL, specifically |
| Write pattern | in-place update | log-structured, append |
| Amplification on SD | moderate | lower — writes align to erase blocks |
| Small frequent fsync | fine | better; this is what it was built for |
| Append-heavy logs | fine | well suited |
| Recovery | `e2fsck`, extremely mature | `fsck.f2fs`, far less exercised |
| Pi OS support | default, universal | in-kernel, not the default |
| If it goes wrong | a well-trodden path | fewer people have been there |

**`[PI-FS-030]` The recommendation is f2fs, and the reason it is a safe
recommendation is `[PI-C-040]`.** Partition C is the one the design already
declares expendable: the player must start with it absent and recreate it. So
the usual objection to f2fs — that its recovery tooling is less battle-tested
— costs much less here than it would on a root filesystem. If f2fs loses
partition C in a way `fsck.f2fs` cannot mend, the answer is the same answer we
had already committed to: make a new one and carry on.

Set against that, its advantages land exactly on Vaino's write pattern: many
small transactions, continuous appends, and an SD card underneath.

**`[PI-FS-040]` Partition B stays ext4.** It is written rarely and attended,
mounted read-only the rest of the time, and holds a gigabyte that takes hours
to rebuild. Maturity is worth more than wear levelling on a partition that
barely wears.

**`[PI-FS-050]` Unmeasured.** Every claim above is reasoning from how these
filesystems are built, not from Vaino running on a Pi — which has not
happened `[PI-IMG-030]`. The honest test is a power-pull rig: write
continuously, cut power at random, count how often the database survives and
how often `fsck` is needed. Until that is run, this is a recommendation and
not a finding.

---

## 5b. Appliance settings

**`[PI-SET-010]` Settings that only exist on the appliance.** The skip times
and the resume interval `[REQ-VIS-155]` are player settings and belong in
`player_state` on partition C. These are different: they configure the *host*,
not the player, and applying one means writing an OS config file and
restarting a service.

| Setting | Default | Applied by |
| :--- | :--- | :--- |
| Wi-Fi AP SSID | `Vaino` | `hostapd.conf`, restart `hostapd` |
| Wi-Fi AP password | `Vaino321` | `hostapd.conf`, restart `hostapd` |
| Wi-Fi band | `2.4` \| `5` \| `both` | `hostapd.conf` `hw_mode`/`channel` |
| Network mode | `ap` \| `client` \| `both` | `hostapd` / `wpa_supplicant` |
| Client SSID + password | none | `wpa_supplicant.conf` |
| Development mode | **off** | `sshd`, diagnostics |
| Bluetooth audio device for auto-connect | none | `bluetoothctl` / BlueZ D-Bus |
| Paired-device list maintenance | — | BlueZ; pair, trust, forget |

**`[PI-SET-012]` Three network modes, and they are not all simultaneous.**

| Mode | For | Default |
| :--- | :--- | :--- |
| **Access point** | direct connection, no infrastructure | on |
| **Client** | join a house network; reachable from anywhere on it | off |
| **Development** | SSH and diagnostics | **off** |

Access point and client are *not* freely combinable. One radio does one job:
concurrent AP+STA exists on some chips through `nl80211`, but both interfaces
must share a channel and the arrangement is fragile — which means the client
network dictates the AP's channel, and losing the client association can take
the access point with it. On a Pi Zero 2 W, treat them as **alternatives**,
and offer both at once only where a second interface is present, exactly as
`both` bands are treated `[PI-SET-032]`.

**`[PI-SET-014]` Every one of these settings can destroy the means of undoing
itself.** Switching to client mode with a mistyped password leaves an
appliance with no access point, on no network, with no screen. This is the
same hazard as an unhonourable band `[PI-SET-036]`, and it deserves a stronger
answer than a fallback, because here the configuration is *valid* — it simply
does not work.

**Confirm-or-revert.** Apply the new configuration, then require the browser
to re-connect and confirm within a timeout — five minutes is generous. If no
confirmation arrives, restore the previous configuration and restart the
services. The listener who mistyped a password sees the appliance come back on
its old settings rather than needing a card reader.

This is worth building once, in the helper, rather than per setting: it is the
only mechanism that makes a network change safe to attempt from the device
being reconfigured.

**`[PI-SET-016]` Development mode is off by default and says what it costs.**
It enables `sshd` and whatever diagnostics are useful. Two conditions:

- **Key-only, or a password the listener sets.** `[PI-SET-030]`'s published
  first-boot credential must never reach `sshd` — a known password on an
  appliance that may be on a house network is a different order of exposure
  from a known password on an access point in one room.
- **It survives reboot but is visible.** A mode that quietly stays on is worse
  than one that must be re-enabled; the settings page should show it as active
  rather than leaving the state to memory.

**`[PI-SET-020]` These need a privileged helper, and that is the design
problem.** The player runs unprivileged and partition A is read-only. So the
web UI cannot write `hostapd.conf` directly: it must hand a *validated*
request to a small root helper over a socket, which writes the file and
restarts the unit. The helper's job is to accept a fixed vocabulary and refuse
everything else — a web form that can write arbitrary text into a system
config as root is a remote shell with extra steps.

**`[PI-SET-030]` The default AP password is published in this repository, so
it is not a secret.** `Vaino321` is a *first-boot* credential, and the setup
page must say so plainly and invite a change. An appliance shipping a known
password on an open access point is a fair description of the problem, not a
convenience.

**`[PI-SET-032]` The band setting is limited by the radio, not by hostapd.**
The Pi Zero 2 W's wireless part is 2.4 GHz only — it has no 5 GHz radio at
all. On the primary target the setting therefore has one legal value, and the
interface must say so rather than offer a choice that cannot be honoured.

On a Pi 3B+/4/5 both bands exist, but **`both` is not a single-radio
capability**: one radio serves one band at a time. Simultaneous dual-band
needs a second interface — a USB adapter — with its own `hostapd` instance.

So the setting is offered as three values, and what is *selectable* is decided
at runtime from the adapter's capabilities:

- `2.4` — always available;
- `5` — only where the hardware has a 5 GHz radio;
- `both` — only where a second wireless interface is present.

A stored value the current hardware cannot honour falls back to 2.4 GHz and
says so, rather than leaving `hostapd` failing to start with no access point
and no way in `[PI-SET-036]`.

**`[PI-SET-035]` Not partition B, and the reason is that the risk was never
about the partition.** Putting `hostapd.conf` on B looks attractive — B is
written rarely and attended, which is the same profile as an AP config change.
But a single small config file is made safe by *how* it is written, not by
where it lives: write a temporary file, `fsync`, `rename`. Rename is atomic,
so the reader sees the old file or the new one and never a half-written one.
`backup.rs` already does exactly this for the same reason.

Once the write is atomic, B's advantage disappears and two disadvantages
remain:

- **It couples the network to the library.** B is a gigabyte of ext4 that must
  mount before `hostapd` could read its config. If B is corrupt — the case we
  explicitly plan to survive by rebuilding from Sampo — there would be no
  access point, and therefore no way to reach the appliance to fix it. The
  recovery path must not depend on the largest thing that can break.
- **It widens the wrong window.** Changing an SSID would mean remounting the
  entire library partition read-write for a reason that has nothing to do with
  music.

**`[PI-SET-036]` So: a default on A, an override on C.** The stock
`hostapd.conf` ships on the read-only system partition and is always present.
The helper writes an override to C, atomically. At boot, use the override if
it exists and parses; otherwise fall back to A's default.

That gives the recovery property the appliance actually needs. Reinitialising
C is already a factory reset `[PI-DB-035]`, and under this arrangement it
restores the *published* credentials `[PI-SET-030]` — so a listener who has
forgotten what they set can always get back in by clearing the partition the
design already treats as expendable.

**`[PI-SET-037]` The override does not appreciably delay the interface.**
Reading it costs a `stat` and a few hundred bytes; what it adds is an ordering
dependency — `hostapd` must wait for C to be mounted. Mounting a small
filesystem on the same card is tens of milliseconds, against `hostapd`'s own
bring-up of the radio, beacon and DHCP server, which is seconds. The override
is noise by two orders of magnitude.

Two things keep it that way:

- **`nofail` on the C mount, with a short timeout.** The one case that could
  actually cost time is `fsck` on an unclean C, and it must not be permitted
  to hold the boot. If C is not there promptly, boot proceeds on A's default
  and the access point comes up with the published credentials — which is the
  same recovery path as a factory reset.
- **Audio does not wait for any of this.** `[REQ-HW-010B]`'s one-second budget
  is the audio path's, and it depends on partition B and the sound device, not
  on the network. The web interface is explicitly allowed to arrive later
  `[REQ-HW-010B]`; a listener hears music before a browser could have
  connected either way.

**`[PI-SET-040]` Bluetooth pairing is stateful and belongs on partition C.**
BlueZ keeps its keys under `/var/lib/bluetooth`, which must therefore be
bind-mounted to C like `/var/log` `[PI-A-020]` — otherwise every pairing is
forgotten at reboot, since the overlay is discarded.

**`[PI-SET-050]` Tone control is Vaino's own, not the speaker's.** No
Bluetooth profile carries tone settings: A2DP carries audio and AVRCP carries
transport and metadata. A speaker's bass and treble are reachable only through
whatever proprietary protocol its own app speaks. So if Vaino is to offer tone
control, it belongs in the player's signal path before transmission, where it
works with every speaker rather than one model.

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

- ~~**Filesystem for C.**~~ Answered in §5a: **f2fs**, because partition C is
  already declared expendable `[PI-C-040]`, which is what makes its thinner
  recovery tooling an acceptable trade. `data=journal` is rejected as
  redundant with SQLite's own durability. Still unmeasured `[PI-FS-050]`.
- **Whether B holds the audio at all.** A USB stick or network share would
  make B small and the image portable between libraries.
- **`[PI-IMG-030]` No ARM64 build has been produced or run.**
  `build/Dockerfile.aarch64` exists as a build target and has not been
  exercised. Every measurement in this project is from x86 Windows, and
  nothing in this document is validated on the target hardware. The filesystem
  recommendation in §5a and the RAM budget in §2 are the two places where that
  matters most: both are reasoning, and both are testable.

# IMPL006: Building the `bose` Image

**Implementation plan — written 2026-08-23, not yet executed**

How to turn `bose` into a Vaino + MPD appliance: a new microSD image with
[PI001](PI001-image-and-partitions.md)'s three partitions, applied to the
hardware [PI008](PI008-bose-survey.md) found.

**Nothing here has been run.** It is a plan, and the parts of it that are
guesses are marked as guesses.

> **Related:** [PI008](PI008-bose-survey.md) for what is on the machine now ·
> [PI001](PI001-image-and-partitions.md) for the partition design ·
> [IMPL001](IMPL001-appliance-setup.md) for the `vainopi` build this follows

---

## 1. What is different from PI001, and why

PI001 designed A/B/C against a 464 MB Pi driving a Bluetooth speaker. `bose` is
a 2 GB Pi 4 with an I²S DAC and a library already on the card. Five departures:

**`[IMPL-BOS-010]` 64-bit, replacing the 32-bit OS.** `bose` runs `armv7l`
`[PI-BOS-010]`; the image is built from **Pi OS Lite 64-bit (Bookworm)** so the
existing `aarch64` toolchain and the binary already running on `vainopi` apply
unchanged. Adding an `armv7` target to serve one machine is the alternative and
is not worth it.

**`[IMPL-BOS-020]` Lite, not desktop.** The current install carries `lightdm`,
VNC, CUPS, `ModemManager`, `udisks2` `[PI-BOS-040]`. None of it plays music and
all of it writes. Starting from Lite is less work than removing them.

**`[IMPL-BOS-030]` The DAC gets a hardware mixer.** MPD uses
`mixer_type "hardware"` on the PCM512x's 208-step `Digital` control
`[PI-BOS-030]`, not the software mixer `vainopi` needed `[SPEC-MPD-140]`.

**`[IMPL-BOS-040]` Partition B is big and mostly empty at first.** 44 GB of
library `[PI-BOS-050]` on a 119 GB card, against PI001's sketch of "B the
remainder" written when B was assumed small.

**`[IMPL-BOS-050]` The overlay stops being the state store.** `bose` already
runs a RAM overlay and is losing 438 MB of memory and every play record to it
`[PI-BOS-050]`, `[PI-BOS-060]`. The overlay stays — for the OS. Everything that
writes moves to C.

---

## 2. Partition layout

On a **new card**, not the running one — see §6.

| | Size | FS | Mounted | Written |
| :--- | ---: | :--- | :--- | :--- |
| `p1` firmware | 512 MB | vfat | `/boot/firmware` **ro** | kernel updates only |
| `p2` **A — system** | 8 GB | ext4 | `/` **ro + RAM overlay** | deliberate updates only |
| `p3` **C — state** | 4 GB | f2fs | `/var/vaino` **rw** | continuously |
| `p4` **B — library** | remainder ≈ 106 GB | ext4 | `/srv/library` **ro** | attended imports |

Order on the card is deliberate: C sits between A and B so that growing B later
does not mean moving it.

**Sizes, and the reasoning behind each.** 8 GB for A against PI001's 4 GB,
because Bookworm plus a resident MPD plus its database index is comfortably more
than Bullseye Lite, and A is written once — spare capacity there costs nothing
but card space we have. 4 GB for C against PI001's 1 GB, because C now also
holds the backup snapshots `[PI-C-030]` and MPD's own state, and because f2fs
wants free space to log into. B takes what is left: 106 GB against a 44 GB
library is room for the library to roughly double.

---

## 3. What lands where

**A — system, read-only.** Kernel, Bookworm Lite, `vaino` binary and unit,
`mpd` and its unit drop-in, `/etc`. `[PI-A-030]`: nothing about the library or
the listener.

**B — library, read-only except during import.** The 44 GB of audio,
`library.db`, cover art, and the `.cue` sheets if they are ever enabled
`[REQ-VIS-205]`. MPD's own database — its index of 5,819 files — belongs here
too and not on A: it is derived from B's contents, it is rebuilt by
`update`, and it changes exactly when B changes.

**C — state, read-write, the only continuously written partition.**

| Path | What |
| :--- | :--- |
| `/var/vaino/listener.db` | plays, preferences, programmes — the irreplaceable data `[PI-C-020]` |
| `/var/vaino/log/` | `/var/log` bind-mounted here `[PI-A-020]` |
| `/var/vaino/mpd/` | MPD `state_file`, `sticker.sql`, playlists |
| `/var/vaino/backup/` | `[REQ-LIB-160]` snapshots, pending copy off-device `[PI-C-030]` |

**MPD's four state files split across two partitions**, which is the same
`[PI-DB-010]` line drawn again: `db_file` is derived from the library and lives
on B; `state_file`, `sticker_file` and `playlist_directory` are the listener's
and live on C. `vainopi` puts all four together under `/srv/library/mpd`
because it has no C to put them on; this image should not copy that.

---

## 4. Keeping writes off A without spending RAM

The overlay is kept for A, so an unexpected write cannot corrupt the system
partition. The work is making sure almost nothing uses it — on `bose` today the
overlay holds 438 MB `[PI-BOS-050]`, and every megabyte of that is memory the
player does not get.

**`[IMPL-BOS-060]` Redirect, do not merely cap.** A cap turns an overflow into a
failure; redirecting means the write lands on C where it belongs.

| Writer | Today | In the image |
| :--- | :--- | :--- |
| `/var/log` | overlay (RAM) | bind mount → C |
| systemd journal | overlay | `Storage=persistent`, `SystemMaxUse=64M`, on C via `/var/log` |
| `mulib.db` / `listener.db` | overlay — **lost at reboot** | C |
| MPD state, stickers | — | C |
| `/tmp`, `/var/tmp` | overlay | `tmpfs`, `size=64M` — genuinely ephemeral |
| `/home/pi` | overlay | C, or accept its loss |
| `apt` lists, caches | overlay | A, `ro`; updates are deliberate `[PI-A-030]` |

> **This is where PI001 and the survey disagree with each other, and the survey
> wins.** `[PI-A-020]` sends the journal to `Storage=volatile` to keep it out of
> RAM — but volatile *is* RAM (`/run`). On a machine with C available, the
> journal should be persistent **on C**, capped. Volatile was the right answer
> only while there was nowhere else to put it.

**`[IMPL-BOS-070]` The resume interval is an installation setting**
`[PI-FS-015]`. It defaults to 5 s and is the largest single source of
unattended writes. For an appliance that is switched off at the wall, 15–30 s
is the better default: it costs at most that much playback position, and it
cuts the writes to C by three to six times. Set it in `player_state` at build
time rather than leaving it at the source default.

---

## 5. Where the build actually runs

**`[IMPL-BOS-100]` There is no single machine that can do all of it, because a
running system cannot repartition the card it booted from.** That constraint,
not tooling, is what splits the work into three phases with different homes.

| Phase | Runs on | Target | Why there |
| :--- | :--- | :--- | :--- |
| **1 — write** | `bose`, driven over SSH | new card in a USB reader | needs a card slot; see below |
| **2 — partition and seed** | `bose`, driven over SSH | same card, still in the reader | the card is not booted, so it is free to be repartitioned |
| **3 — provision** | this host, over SSH | `bose` booted from the new card | exactly how `vainopi` is deployed today |

**The development host cannot host phase 1 or 2.** It is Windows with Docker and
a WSL that carries only the `docker-desktop` distro, and — checked — **it has no
card reader**: the only removable-capable device is an external SSD. Passing a
reader into WSL would need `usbipd-win`, which is not installed. So the natural
Linux host for an image build is not available, and inventing one is more work
than the alternative.

**`bose` can host both, and needs one accessory: a USB SD card reader.** It is a
Pi 4 with USB3 root hubs `[PI-BOS-020]`, it reaches
`downloads.raspberrypi.com` (HTTP/2 200), and — once `PATH` includes `sbin`,
which is why a first check appeared to show them missing — it already has
`parted`, `sgdisk`, `resize2fs`, `mkfs.ext4`, `partprobe`, `dd` and `xz`. Its
kernel lists `f2fs`. Two packages are needed: `f2fs-tools` and
`cloud-guest-utils` for `growpart`.

That reader is the one piece of hardware to obtain, and it earns its cost twice:
it is also what makes the 44 GB migration a local copy instead of a night of
WiFi (§7).

> **The answer to "can this host drive it over SSH?" is: phases 2 and 3 yes,
> phase 1 yes but only by using `bose` as the writer.** No step needs a person
> at a keyboard on either machine. Every step needs the reader.

**`[IMPL-BOS-110]` The script must refuse to write to `mmcblk*`.** Phase 1 and 2
run as root on a machine whose own root filesystem is a card, and the difference
between the target and the running system is one letter. `prepare-card.sh`
takes a device, resolves it, and **aborts unless it is USB-attached and is not
the device holding `/`** — checked, not documented. `[PI3-API-030]` applies to
build tooling as much as to a player: a script that will cheerfully destroy the
running system if mistyped is not fit to be run unattended.

---

## 6. Build sequence

Following `[PI-IMG-010]`, **two scripts rather than one**, because they run in
different places and at different times:

**`VainoPi/prepare-card.sh`** — runs on `bose` (booted from the *old* card),
against the new card in the reader:

1. Stream Pi OS Lite 64-bit (Bookworm) straight onto the card, no staging file:
   `bose` has 505 MB of writable space and it is RAM `[PI-BOS-050]`, so
   `curl … | xz -dc | dd of=/dev/sdX` is not an optimisation but a requirement.
2. Partition to §2 — `p1` firmware, `p2` A, `p3` C, `p4` B — and label them
   `SYSTEM`, `STATE`, `LIBRARY`. Grow `p2`'s filesystem to 8 GB.
3. **Disable first-boot root auto-expand.** Raspberry Pi OS otherwise grows
   `p2` to fill the card on first boot, which would consume the space C and B
   are meant to occupy. Remove the `init=…/firstboot` clause from
   `cmdline.txt`.
4. Pre-seed first boot while the card is mounted here: `userconf.txt` for the
   `pi` user, `wpa_supplicant`/NetworkManager for WiFi, `ssh` enabled, the
   development host's public key, hostname. **`bose` is wireless with no wired
   fallback** `[PI-BOS-010]` — if this is wrong the appliance does not come
   back, and the only recovery is another card swap.
5. `dtoverlay=hifiberry-dacplus` and `dtoverlay=vc4-kms-v3d,audio=off` in
   `/boot/firmware/config.txt` — the same two lines the current install runs
   `[PI-BOS-020]`, which are known to work on this hardware.

Then swap the cards and boot. From here **`VainoPi/provision-bose.sh`** runs on
the development host, over SSH, in the shape `deploy-player.sh` already has:

6. Install `mpd`, `f2fs-tools`, `cloud-guest-utils`; install `mpd.conf` and the
   systemd drop-in with §3's paths and `mixer_type "hardware"`.
7. Deploy `vaino`, cross-built `aarch64` with `--features mpd` — the same
   artefact and the same Docker toolchain `vainopi` uses today.
8. Seed B (§7) and C: empty `listener.db`, the tree from §3, and the resume
   interval from `[IMPL-BOS-070]`. Run `mpd --update` while B is still
   writable — budget five minutes, from 242 s for 5,758 songs on `vainopi`
   `[PI-CHR-085]`.
9. Acceptance: it plays, over the DAC, before anything is made read-only.
10. **Last:** set `ro` in `fstab` for A and B, and enable the overlay on A.
    Enabling it earlier leaves an image that boots, looks right, and has
    silently discarded steps 6–9 `[PI-IMG-020]`.

**`[IMPL-BOS-120]` Step 10 is the point of no easy return**, and it is also the
step that makes the appliance an appliance. Everything before it is recoverable
over SSH; after it, a mistake in `/etc` means another card swap. So step 9 is
not a formality: play something, hear it, then close the door.

---

## 7. Migrating the 44 GB

**`[IMPL-BOS-080]` With the reader, this stops being the risky part.** After the
swap, the **old** card goes into the same USB reader on `bose`, and the library
is a local copy — SD to USB3 — rather than 44 GB over WiFi `[PI-BOS-050]`.
`rsync -aH --info=progress2` from the old card's `/home/pi/Music` to `B`'s
`/srv/library/audio`, resumable, verifiable, and driven over SSH from here.

Without a reader the alternative is two WiFi transfers — off `bose` to this host
before the swap, and back afterwards — which is hours each way and has no
resumable middle. That is the comparison that justifies buying the reader.

> **The old card is the backup**, and remains one until the new image has played
> for a week. Do not reformat it to make anything easier. `bose` is somebody's
> working music player, with MuLibPlay running on it now, per `[PI-BOS-040]`;
> the swap is a physical card change and is reversible in thirty seconds by
> swapping back, which is the strongest reason to build a new card rather than
> convert this one.

---

## 8. Open, and to be measured before it is claimed

- **`[IMPL-BOS-130]` A USB SD card reader has to be obtained.** Nothing in §5
  or §7 happens without one, and no software substitute was found: this host
  has no card slot, and its WSL carries only `docker-desktop`.

- **`[IMPL-BOS-090]` Does the seek fault exist here?** `[SPEC-MPD-135]` works
  around an output wedged by `seekid`, measured against PipeWire on Bluetooth
  `[PI-CHR-100]`. `bose` has a local I²S card and may not need PipeWire at all.
  Measure it — with a silent baseline first — before deciding whether the
  workaround is load-bearing on this machine.
- **Whether PipeWire is wanted at all.** If nothing needs to mix, MPD and Vaino
  could take turns on ALSA directly. That would remove a whole layer, and it
  would also remove the ability for both to sound at once, which the handoff
  depends on `[SPEC-BK-030]`. Not free either way.
- **f2fs for C remains unmeasured** `[PI-FS-050]`. The power-pull rig described
  there has still not been built.
- **The DAC's real rate and format range** `[PI-BOS-060]`, once PulseAudio is
  not holding the device.
- **Whether 8 GB for A is right.** Guessed from Bookworm's footprint, not
  measured.

---

**Traceability:** `[IMPL-BOS-010..130]` · applies `[PI-PART-020]`'s A/B/C ·
supersedes `[PI-A-020]`'s volatile journal · survey in
[PI008](PI008-bose-survey.md) · mixer per `[SPEC-MPD-140]` · build tooling owes
`[PI3-API-030]` the same honesty as the player

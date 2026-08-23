# BOSE003: Building the Card, Step by Step

**Implementation plan — written 2026-08-23, not yet executed**

Where each phase runs, what runs it, and in what order. The design it carries
out is [BOSE002](BOSE002-image-build.md); the machine it targets is described in
[BOSE001](BOSE001-survey.md).

**Nothing here has been executed.** The scripts exist and their guards have been
tested; the destructive paths have not been run, because the card reader
`[IMPL-BOS-130]` does not exist yet.

> **Scripts:** [`prepare-card.sh`](prepare-card.sh) for phases 1–2 ·
> [`provision-bose.sh`](provision-bose.sh) for phase 3 ·
> [`mpd.conf`](mpd.conf) for the guest

---

## 1. Where the build actually runs

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
WiFi (§3 below).

> **The answer to "can this host drive it over SSH?" is: phases 2 and 3 yes,
> phase 1 yes but only by using `bose` as the writer.** No step needs a person
> at a keyboard on either machine. Every step needs the reader.

**`[IMPL-BOS-110]` The script must refuse to write to `mmcblk*`.** Phase 1 and 2
run as root on a machine whose own root filesystem is a card, and the difference
between the target and the running system is one letter. `prepare-card.sh`
takes a device, resolves it, and **aborts unless it is USB-attached and is not
the device holding `/`** — checked, not documented. The obvious form of that
check does not work here: `findmnt /` answers `overlay`, which has no parent
disk, so a guard written against `/` alone passes everything on the very
machine that runs it. It asks `/boot` and `/proc/cmdline` too, and refuses if
they all come back empty. `[PI3-API-030]` applies to
build tooling as much as to a player: a script that will cheerfully destroy the
running system if mistyped is not fit to be run unattended.

---

## 2. Build sequence

Following `[PI-IMG-010]`, **two scripts rather than one**, because they run in
different places and at different times:

**[`prepare-card.sh`](prepare-card.sh)** — runs on `bose` (booted from the *old* card),
against the new card in the reader:

1. Stream Pi OS Lite 64-bit (Bookworm) straight onto the card, no staging file:
   `bose` has 505 MB of writable space and it is RAM `[PI-BOS-050]`, so
   `curl … | xz -dc | dd of=/dev/sdX` is not an optimisation but a requirement.
2. Partition to [BOSE002 §2](BOSE002-image-build.md) — `p1` firmware, `p2` A, `p3` C, `p4` B — and label them
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

Then swap the cards and boot. From here **[`provision-bose.sh`](provision-bose.sh)** runs on
the development host, over SSH, in the shape `deploy-player.sh` already has:

6. Install `mpd`, `f2fs-tools`, `cloud-guest-utils`; install `mpd.conf` and the
   systemd drop-in — [`mpd.conf`](mpd.conf) carries the split paths from
   [BOSE002 §3](BOSE002-image-build.md) and the mixer decision `[IMPL-BOS-030]`.
7. Deploy `vaino`, cross-built `aarch64` with `--features mpd` — the same
   artefact and the same Docker toolchain `vainopi` uses today.
8. Seed B (§3 below) and C: empty `listener.db`, the tree from
   [BOSE002 §3](BOSE002-image-build.md), and the resume
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

## 3. Migrating the 44 GB

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

## 4. Open, and to be measured before it is claimed

- **`[IMPL-BOS-130]` A USB SD card reader has to be obtained.** Nothing in §1
  or §3 happens without one, and no software substitute was found: this host
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

**Traceability:** `[IMPL-BOS-100..130]` · applies `[PI-PART-020]`'s A/B/C ·
supersedes `[PI-A-020]`'s volatile journal · survey in
[BOSE001](BOSE001-survey.md) · mixer per `[SPEC-MPD-140]` · build tooling owes
`[PI3-API-030]` the same honesty as the player

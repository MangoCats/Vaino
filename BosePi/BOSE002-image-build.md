# BOSE002: Building the `bose` Image

**Implementation plan — written 2026-08-23, not yet executed**

The shape of the image: what goes on which partition, and why each choice
differs from
[PI001](../VainoPi/PI001-image-and-partitions.md). The procedure that builds
it is [BOSE003](BOSE003-build-procedure.md); the machine is [BOSE001](BOSE001-survey.md).

**Nothing here has been run.** It is a plan, and the parts of it that are
guesses are marked as guesses.

> **Related:** [BOSE001](BOSE001-survey.md) for what is on the machine now ·
> [PI001](../VainoPi/PI001-image-and-partitions.md) for the partition design ·
> [IMPL001](../VainoPi/IMPL001-appliance-setup.md) for the `vainopi` build this follows

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

**`[IMPL-BOS-030]` The DAC offers a hardware mixer, and taking it costs the
handoff.** The PCM512x has 208 steps in the chip `[PI-BOS-030]`, which beats a
software mixer by every measure. But reaching it means MPD opening the card
**directly**, and a card opened directly is one the player cannot also open —
while `[SPEC-BK-030]`'s handoff needs both to sound at once, briefly.

*(Corrected 2026-08-23: an earlier revision of this document simply said "MPD
uses `mixer_type "hardware"`". Writing the configuration is what exposed the
conflict, which is an argument for writing the configuration early.)*

The two arrangements, neither yet measured on this machine:

| | Shared sink | Direct to the card |
| :--- | :--- | :--- |
| `device` | `default` (a mixing layer) | `hw:sndrpihifiberry,0` |
| Mixer | software `[SPEC-MPD-140]` | **hardware**, `Digital` |
| Handoff crossfade | works | **not possible** |
| Evidence | the `vainopi` arrangement | none yet |

`BosePi/mpd.conf` ships the shared sink, because it is the one with evidence
behind it and because losing the crossfade is a visible loss where a software
mixer is an inaudible one. `[IMPL-BOS-090]` is the measurement that should
settle it.

**`[IMPL-BOS-040]` Partition B is big and mostly empty at first.** 44 GB of
library `[PI-BOS-050]` on a 119 GB card, against PI001's sketch of "B the
remainder" written when B was assumed small.

**`[IMPL-BOS-050]` The overlay stops being the state store.** `bose` already
runs a RAM overlay and is losing 438 MB of memory and every play record to it
`[PI-BOS-050]`, `[PI-BOS-060]`. The overlay stays — for the OS. Everything that
writes moves to C.

---

## 2. Partition layout

On a **new card**, not the running one — see [BOSE003 §3](BOSE003-build-procedure.md).

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

**Traceability:** `[IMPL-BOS-010..070]` · applies `[PI-PART-020]`'s A/B/C ·
supersedes `[PI-A-020]`'s volatile journal · survey in
[BOSE001](BOSE001-survey.md) · procedure in
[BOSE003](BOSE003-build-procedure.md)

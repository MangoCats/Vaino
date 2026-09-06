# BOSE002: Building the `bose` Image

**Implementation plan — written 2026-08-23. Largely built since 2026-09-06 —
see the `[IMPL-BOS-07x]` notes below for what changed on contact with real
hardware, and [BOSE003](BOSE003-build-procedure.md) for what's actually been run.**

The shape of the image: what goes on which partition, and why each choice
differs from
[PI001](../VainoPi/PI001-image-and-partitions.md). The procedure that builds
it is [BOSE003](BOSE003-build-procedure.md); the machine is [BOSE001](BOSE001-survey.md).

**Updated 2026-09-06.** Most of this plan has now been carried out once,
against real hardware — where it held up as written, that isn't repeated
here; where reality disagreed, an `[IMPL-BOS-07x]`-tagged note says what
changed and why, in place rather than as a silent rewrite. The parts still
marked as guesses (§1's `[IMPL-BOS-090]` mixer choice, §5a's f2fs
measurement) remain exactly that — untouched by this build, still open.

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

**B — library, read-only except during import.** The 44 GB of audio and cover
art, and the `.cue` sheets if they are ever enabled `[REQ-VIS-205]`. MPD's own
database — its index — belongs here too and not on A: it is derived from B's
contents, it is rebuilt by `update`, and it changes exactly when B changes.

**`[IMPL-BOS-078]` `vaino.db` itself goes on C, not B — a correction, found
building this card.** This section originally listed `library.db` as living
on B, per `[PI-DB-010]`'s split design. That split was never built: the
player still opens one `vaino.db`, no `ATTACH`, and the schema stays one file
per `[SPEC-SC-010]`. So there is no separate `library.db` to place — only the
one file, and it has to go somewhere that stays writable, because every play
is a write to it.

`vainopi` gets away with the same single file sitting under `/srv/library`
(`[PI002](../VainoPi/PI002-test-image-setup.md)`,
`[PI005](../VainoPi/PI005-appliance-library.md)`) only because that partition
is never actually made read-only in practice — `vainopi` has no C, and its
data partition just stays `rw` forever. `bose` is not that: step 10 of
`[BOSE003](BOSE003-build-procedure.md)` genuinely flips B to `ro`. Putting
`vaino.db` there would mean every play after that point fails to write. So
until the split is built, `vaino.db` lives at **`/var/vaino/vaino.db`** — C,
not B — and B holds only audio, cover art, and MPD's own derived index.

**`[IMPL-BOS-150]` The attended-import operation, finally built.**
`[PI-B-030]` said what this should be — "remount rw, run the ingest, sync,
remount ro" — from this project's earliest design pass; nothing carried it
out until now. Asked directly, checking the actual scripts rather than the
design doc, found the gap: B isn't even set `ro` by default
(`provision-bose.sh`'s own `fstab` line is plain `defaults,noatime`, `rw`
until `[BOSE003]` step 10's `--lock-in` flips it once), and nothing
performed the reopen-import-reclose cycle, because `seed-library.sh` never
needed to — it always ran before B was ever `ro` in the first place.
`BosePi/attended-import.sh` (dev host, over SSH, the same home every other
phase script has) closes it:

1. Reads B's *current* mount options and remembers them, whatever they are
   — `rw` pre-lock-in, `ro` after. This is what makes "restore afterward"
   correct in both worlds, rather than assuming lock-in has already
   happened.
2. Remounts `rw` only if it was not already — an asked-twice `--check`/
   `--go` decision, the same discipline `[IMPL-BOS-110]`'s destructive
   scripts already use: a wrapped command that misbehaves can corrupt
   exactly the partition this whole design protects, so nothing here
   defaults to acting.
3. Runs the caller's command verbatim inside the window — new audio via
   `rsync`, a mesh-approved bundle's `audio/` directory, whatever is
   needed. The script has no opinion on what belongs in the window, only
   that the window exists and closes.
4. `sync`s, then restores exactly the options step 1 captured — via a
   trap, so a wrapped command that fails or is interrupted still closes
   the window rather than leaving B open indefinitely.
5. Best-effort MPD reindex afterward (skippable with `--no-mpd-update`) —
   B's own index changed if the audio did, and every caller adding audio
   needs this, so it is default-on rather than one more step to remember.

**Run for real against `bose` 2026-09-06, three ways**: a dry run, a
successful command (wrote and removed a real file inside the actual `rw`
window), and a failing one (confirmed B still closes and nothing after it
runs) — the ro side simulated by hand, since `--lock-in` itself had not run
yet at the time. MPD's reindex trick needed no `mpc` client
(`provision-bose.sh` never installs one) — bash's own `/dev/tcp` speaks the
control port directly.

**`[IMPL-BOS-155]` This is the missing half of a mesh-approved import, not a
separate feature.** `[SPEC035](../docs/spec/SPEC035-mesh-library-sync.md)`'s
`mesh_diff.py`/`resolve_mesh_conflict.py` only ever touch
`/var/vaino/vaino.db` — correct, since C is what stays writable regardless
of lock-in — but a `local_only`/`peer_only` bundle's audio bytes still need
somewhere to land, and B was the half nothing wrote yet.
`attended-import.sh` is that half, run the same way the original seed
(`[IMPL-BOS-085]`) did before B was ever locked: `rsync` the bundle's
`audio/` into `/srv/library/audio/` inside the window it opens.

**C — state, read-write, the only continuously written partition.**

| Path | What |
| :--- | :--- |
| `/var/vaino/vaino.db` | the whole database — library cache and listener state together `[IMPL-BOS-078]` |
| `/var/vaino/log/` | `/var/log` bind-mounted here `[PI-A-020]` |
| `/var/vaino/mpd/` | MPD `state_file`, `sticker.sql`, playlists |
| `/var/vaino/backup/` | `[REQ-LIB-160]` snapshots, pending copy off-device `[PI-C-030]` |
| `/var/vaino/etc-ssh`, `/home-pi`, `/nm-connections` | bind-mount sources for host identity and known networks `[PI-A-025]` |

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
| `/home/pi` | overlay | **C** — bind-mounted before first user creation `[PI-A-025]` |
| `/etc/ssh` (host keys) | overlay | **C** — same reason, or every reboot is a new host `[PI-A-025]` |
| NM connection profiles | overlay | **C** — or `[SPEC034]`'s Wi-Fi setup is undone every power cycle `[PI-A-025]` |
| `apt` lists, caches | overlay | A, `ro`; updates are deliberate `[PI-A-030]` |

**`[IMPL-BOS-075]` Not a guess — found by building this card.** `authorized_keys`
written to `/home/pi/.ssh` for this build did not survive `bose`'s next power
cycle, because that path was still on the overlay when the key was added. The
three new rows above read `/home/pi | overlay | C, or accept its loss` — an
open question — until this happened; they are a firm decision now, made the
same way `[PI-C-040]`'s recovery behaviour was: something broke first, and the
fix generalized past the one instance.

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

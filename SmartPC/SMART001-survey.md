# SMART001: What Is on `smartboardpc` Today

**Measurement — Tier 1 · surveyed on `mango@smartboardpc`, 2026-09-11**

A reconnaissance of the fourth node before anything is built for it. Most
figures here were read from the running machine; the three changes that were
*made* are marked as such in §4, because a survey that quietly includes its own
side effects is not a survey.

`smartboardpc` — "Smart" — is not an appliance and not a second
`teacherslounge`. It is an **x86_64 mini-PC that is intended to play audio**,
which makes it the first node that is both a build host and a playback target.
Its audio reaches a powered speaker through a **USB dongle whose playback
endpoint is adaptive**, and that single fact decides most of what
[GUIDE008](../docs/GUIDE008-echo-playback-investigation.md) can say about it.

> **Related:** [BOSE001](../BosePi/BOSE001-survey.md) — the same survey done for `bose`, whose I²S DAC is this machine's opposite in every respect that matters here · [GUIDE008](../docs/GUIDE008-echo-playback-investigation.md) `[GDE-ECHO-075]` — why the adaptive endpoint matters · [GUIDE009](../docs/GUIDE009-echo-playback-plan.md) — the measurement phase this node is a subject of · [build/deploy-everywhere.sh](../build/deploy-everywhere.sh) — the source-host leg it was added to

---

## 1. The machine

**`[SMT-HW-010]` An Intel Celeron N5105 under Linux 6.8, x86_64, on eMMC.**

| | |
| :--- | :--- |
| CPU | Intel Celeron N5105 @ 2.00 GHz (Jasper Lake, 4 core) |
| Kernel | 6.8.0-137-generic, x86_64 |
| Root | `/dev/mmcblk0p2`, 113 G, 45 G used, **63 G free** |
| `/boot/efi` | `/dev/mmcblk0p1`, 511 M — EFI boot, not a Pi's `config.txt` world |
| RAM | 7.7 G (from `tmpfs` sizing: `/dev/shm` 3.8 G) |

It boots UEFI from eMMC and shares none of the A/B/C overlay design
`[PI001]` that both appliances use. Nothing here is read-only; the root
filesystem is ordinary and writable.

---

## 2. Audio — the part that decides everything else

**`[SMT-AUD-010]` The onboard `HDA Intel PCH` has no analog output at all.** It
enumerates devices 3, 7, 8 and 9 — **HDMI 0 through HDMI 3, and nothing else**.
Any expectation that this machine has a motherboard headphone jack is wrong, and
was wrong in the brief that prompted this survey.

**`[SMT-AUD-020]` Sound reaches the powered speaker through card 1, an
`Anlya.cn ATE1133` on USB.** Two USB audio devices are present and both are
plausible candidates, so the answer was obtained by **listening, not by
inference** — alternating tone patterns were played on each card in turn and the
one that came out of the speaker was identified by ear, 2026-09-11.

| card | device | bus |
| ---: | :--- | :--- |
| 0 | `USB AUDIO CODEC` — Burr-Brown/TI | `usb-0000:00:14.0-3.1.3` |
| **1** | **`ATE1133` — Anlya.cn** ← the speaker | `usb-0000:00:14.0-7`, **full speed** |
| 2 | `HDA Intel PCH` | HDMI only `[SMT-AUD-010]` |

**`[SMT-AUD-030]` It must be selected by name, never left to a default.** The
player takes `--device` as a case-insensitive substring `[BOS-PWR-050]`, so
`--device ATE1133` is the correct configuration. A default would be a coin
toss between two real output devices, and `[BOS-PWR-050]` already recorded what
ALSA card renumbering does to an assumption like that.

**`[SMT-AUD-040]` Its playback endpoint is `ADAPTIVE`, so the sample clock is
the host's, not the device's.** Read from `/proc/asound/card1/stream0`:

```
Playback:
  Interface 1, Altset 1
    Format: S16_LE      Channels: 2
    Endpoint: 0x03 (3 OUT) (ADAPTIVE)
    Rates: 48000, 96000
```

An adaptive sink slaves its conversion rate to the rate the host feeds it. This
is the **reverse** of an asynchronous device, where a crystal in the dongle
would be master and the host would follow. Smart's drift is therefore a property
of the Intel USB controller's frame timing and the driver's feed — not of
anything in the dongle, and **not predicted by any figure measured on `bose`**,
whose I²S DAC is master of its own clock `[PI-BOS-020]`. The two nodes' rate
errors do not share a physical cause.

**`[SMT-AUD-050]` There is no 44100 — only 48000 and 96000.** The library is
44.1 kHz, so **every passage is resampled on this node, always**, through
`player/src/resample.rs`. There is no bit-exact pass-through case on Smart to
protect, which changes one of the two arguments in `[GDE-ECHO-210]` for this
machine specifically. Format is `S16_LE`, two channels; the capture endpoint is
asynchronous but is a microphone input and irrelevant here.

---

## 3. Time

**`[SMT-TIME-010]` `systemd-timesyncd`, not chrony.** Probed 2026-09-11: chrony
and chronyd both inactive, `systemd-timesyncd` active, `timedatectl` reporting
*System clock synchronized: yes*, `America/New_York`.

That satisfies a wall-clock requirement and silently fails a frequency one.
timesyncd is an SNTP client: it corrects *what time it is* without disciplining
*how fast the clock runs*, which is the half `[GDE-ECHO-100]` actually needs.
Anything in [GUIDE009](../docs/GUIDE009-echo-playback-plan.md)'s Phase 2 that
assumes a common frequency reference is not yet true on this machine.

---

## 4. What was changed, and why

Three changes were made on 2026-09-11 rather than merely observed. They are
listed because §0's framing — a read-only survey — does not apply to them.

**`[SMT-BLD-010]` There was no Rust toolchain; rustup was installed.** The
machine had a clean checkout at `/home/mango/Dev/Vaino` on the correct origin,
already at `122518a`, but no `cargo`, no `rustc`, and nothing in `dpkg`. It
therefore could not have been a source host in
[build/deploy-everywhere.sh](../build/deploy-everywhere.sh)'s sense. Installed
to `~/.cargo/bin`: **cargo 1.98.1, rustc 1.98.1**. `cc` and `gcc` were already
present at `/usr/bin`, and no global `CC` is set.

**`[SMT-BLD-020]` `libasound2-dev` and `pkg-config` were missing, and were
installed.** Without them cpal's `alsa-sys` build script fails, so the
source-host leg would have failed on every run — which is worse than not having
the leg, for the reason `[SPEC-SUI-227]` already gives: a report that always
contains one failure trains people to stop reading it. `pkg-config
--modversion alsa` now answers 1.2.11. `mango` has NOPASSWD sudo.

**`[SMT-BLD-030]` It was added to `deploy-everywhere.sh` as a source host.**
`mango@smartboardpc:/home/mango/Dev/Vaino`, on the same leg as
`teacherslounge`: pull, rebuild, and verify by asking the **built binary** its
commit. That is the right verification *today*, because nothing is running. When
Smart actually plays, it becomes the first target that wants both checks — the
binary's commit and a running player's `/build` — and the script does not model
that yet.

**Verified rather than assumed**, 2026-09-11: the leg was run for real, built
from cold, and answered `vaino 0.1.0 (122518a1347f)` — matching HEAD. The
resulting binary links `libasound.so.2`, so `[SMT-BLD-020]`'s packages did the
job they were installed for and the node can actually open a device rather than
merely compile.

---

## 5. Standing findings not related to audio

**`[SMT-STO-010]` `/media/mango/PortableSSD` is full.** 1.9 T, **100 % used, 11
G available**. Whatever it is for, it has no room left.

**`[SMT-STO-020]` `/mnt/piback-backups` returns `Input/output error`.** `df`
cannot stat it. A stale or dead mount, reported here because a backup target
that errors on inspection is the kind of thing that is discovered when it is
needed rather than when it breaks.

---

## 6. Open

**`[SMT-OPN-010]` Smart has no music library, and no obvious room for one.**
Root has 63 G free against a library that is 44 G on `bose` alone
`[BOSE001]` — tight but not impossible — while the 1.9 T volume that would be
the natural home is full `[SMT-STO-010]`. Until this is answered, Smart is a
build host that could play, not a playback node.

**`[SMT-OPN-020]` Whether it joins the mesh as a Sampo-capable peer is
undecided.** [SPEC035](../docs/spec/SPEC035-mesh-library-sync.md)'s membership
list is explicit and human-maintained `[SPEC-MESH-025]`; Smart is not on it, and
adding it is a decision about what this machine is *for*, not a consequence of
it existing.

**`[SMT-OPN-030]` Its drift has not been measured.** It is a Phase 0 subject in
[GUIDE009](../docs/GUIDE009-echo-playback-plan.md), and `[SMT-AUD-040]` is the
reason its number cannot be guessed from `bose`'s.

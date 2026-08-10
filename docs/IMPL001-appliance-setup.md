# IMPL001: Raspberry Pi Zero 2W Appliance Setup

**Implementation Guide — Tier 3**

Step-by-step build of a Pi Zero 2W as a Vaino appliance. Implements the hardware and storage model in [embedded-hardware.md](embedded-hardware.md) and the requirements in [REQ002 §6](spec/REQ002-functional-requirements.md#6-appliance--hw).

> **Status: UNVALIDATED.** Written from specification and the target's known constraints; not yet executed on hardware. Each step carries a **Verify** line — record the actual result, and correct this document where reality disagrees. MuLibPlay's Pi notes decayed into scattered tips precisely because nothing was checked back in.

---

## 1. Bill of Materials

**`[IMPL-BOM-010]` The Pi Zero 2W has no analog audio output.** Only mini-HDMI and USB. This is the single most consequential hardware fact and it must be settled before flashing, because the audio path determines several later steps.

| Option | Notes |
| :--- | :--- |
| **I2S DAC HAT** (recommended) | Best quality, lowest CPU, no USB contention. Needs a device-tree overlay `[IMPL-AUD-020]`. |
| **USB DAC** | Works, but the Zero 2W has one micro-USB data port — a hub is needed if anything else is attached. |
| **Bluetooth** | Convenient, but adds latency and a reconnection failure mode. MuLibPlay needed a 5-second startup delay to work around exactly this. |
| **HDMI** | Only if the amplifier accepts HDMI. |

Also required: microSD (**A2-rated**, endurance-grade — this runs 24/7), a 5 V ≥2.5 A supply, and the music library.

---

## 2. Operating System

**`[IMPL-OS-010]` Raspberry Pi OS Lite (64-bit), Bookworm or later.**

- **Lite**, not Desktop: a desktop consumes over half of the 512 MB before Vaino starts.
- **64-bit**: matches the verified `aarch64-unknown-linux-gnu` build `[build/README.md]`. The Zero 2W's Cortex-A53 supports it.

> **If 32-bit is chosen instead**, the target triple becomes `armv7-unknown-linux-gnueabihf` and needs its own cross-build image — the same Dockerfile pattern with `gcc-arm-linux-gnueabihf` and `libasound2-dev:armhf`. The existing Pi (`bose.lan`) runs 32-bit, so do not assume the two machines share binaries.

**`[IMPL-OS-020]` Configure headless access in Raspberry Pi Imager before writing** (gear icon): hostname, SSH with public key, Wi-Fi credentials and country, locale. This avoids ever attaching a monitor — the Zero 2W's mini-HDMI is inconvenient and this is a permanently headless appliance.

> **Verify:** `ssh vaino@<hostname>.local` succeeds; `uname -m` reports `aarch64`.

---

## 3. Baseline Measurement — Before Changing Anything

**`[IMPL-BASE-010]`** Record the untouched baseline. Without it, later tuning cannot be shown to have helped.

```bash
free -m                      # total and used RAM
systemd-analyze              # boot time
systemd-analyze blame | head -20
df -h /
```

> **Verify:** record used RAM at idle and total boot time here. Expect roughly 40–70 MB used on Lite, and a boot in the tens of seconds. The `[REQ-HW-100]` budget is ≤150 MB **for Vaino**, so the OS baseline is what remains available.

---

## 4. Strip to Essentials

**`[IMPL-TRIM-010]`** Each removal must be justified by the appliance role — this is a single-purpose device with no local user.

```bash
sudo systemctl disable --now bluetooth        # ONLY if not using Bluetooth audio
sudo systemctl disable --now triggerhappy avahi-daemon
sudo systemctl disable --now ModemManager wpa_supplicant@wlan1 2>/dev/null
sudo apt-get purge -y  # nothing to purge on Lite by default; confirm before adding
sudo systemctl mask systemd-networkd-wait-online.service   # boots faster; see [IMPL-BOOT-010]
```

**`[IMPL-TRIM-020]` Use zram, not an SD swapfile.** Swapping to SD destroys cards and is slow enough to cause audio dropouts on a 512 MB machine.

```bash
sudo systemctl disable --now dphys-swapfile
sudo apt-get install -y zram-tools
echo -e "ALGO=zstd\nPERCENT=50" | sudo tee /etc/default/zramswap
sudo systemctl restart zramswap
```

> **Verify:** `swapon --show` lists `/dev/zram0` and no file-backed swap; `free -m` shows used RAM lower than baseline.

---

## 5. Audio Device

**`[IMPL-AUD-010]`** Identify the device before configuring anything: `aplay -l`.

**`[IMPL-AUD-020]` For an I2S DAC HAT**, add the overlay to `/boot/firmware/config.txt` and disable the on-board audio it conflicts with:

```
dtparam=audio=off
dtoverlay=hifiberry-dac        # substitute the overlay for the specific HAT
```

**`[IMPL-AUD-030]` Pin the default device** in `/etc/asound.conf` so Vaino never depends on probe order:

```
defaults.pcm.card 0
defaults.ctl.card 0
```

> **Verify:** `speaker-test -c2 -twav -l1` produces audible sound from the intended output. Do this **before** installing Vaino — otherwise a silent Vaino is ambiguous between "no audio configured" and "player broken".

**`[IMPL-AUD-040]`** Vaino's `cpal` backend uses ALSA directly. PulseAudio and PipeWire are unnecessary; if present, they add latency and another failure mode. Leave them uninstalled on Lite.

---

## 6. Storage Layout

**`[IMPL-STOR-010]`** Implements the 3-partition isolation model `[embedded-hardware.md]`. The Imager writes two partitions (boot, root); add the other two before first heavy use.

| Mount | Contents | Mode |
| :--- | :--- | :--- |
| `/` | OS, Vaino binaries | **read-only** `[IMPL-STOR-030]` |
| `/srv/music` | audio library | read-only in normal operation |
| `/var/lib/vaino` | `vaino.db`, exports, logs | read-write |

**`[IMPL-STOR-020]`** Keeping `vaino.db` on its own read-write partition means an unclean shutdown risks only that filesystem, and the class-D export `[SPEC-DF-094]` lives there too — so back it up **off the Pi**, since a card failure takes the partition and its backups together.

**`[IMPL-STOR-030]` Read-only root** via `sudo raspi-config` → Performance → Overlay File System. Enable the overlay **and** set the boot partition read-only.

> **Verify:** after reboot, `touch /test` fails; `mount | grep ' / '` shows `overlay`. Remember that from this point OS changes require disabling the overlay, rebooting, changing, and re-enabling — do all package installation *before* this step.

---

## 7. Vaino Service

**`[IMPL-SVC-010]`** Deploy the aarch64 binary built per [build/README.md](../build/README.md) to `/usr/local/bin/vaino`.

**`[IMPL-SVC-020]`** `/etc/systemd/system/vaino.service` — the unit exists to satisfy `[REQ-HW-110]` (reach audio quickly) and `[REQ-HW-120]` (survive power loss):

```ini
[Unit]
Description=Vaino
# Deliberately NOT After=network-online.target: audio must start without
# waiting for Wi-Fi association [REQ-HW-110]. The web UI binds later.
After=sound.target
Wants=sound.target

[Service]
Type=simple
ExecStart=/usr/local/bin/vaino --db /var/lib/vaino/vaino.db --music /srv/music
Restart=always
RestartSec=2
User=vaino
Nice=-5
# Modest priority, not real-time: a runaway RT thread on a single-purpose
# appliance can lock the machine out of SSH.
MemoryMax=200M

[Install]
WantedBy=multi-user.target
```

**`[IMPL-SVC-030]`** `MemoryMax=200M` turns the `[REQ-HW-100]` budget into an enforced limit rather than an aspiration: exceeding it kills the service and `Restart=always` recovers, which is loud and diagnosable instead of the machine silently thrashing into swap.

> **Verify:** `systemctl enable --now vaino`, then `systemd-analyze blame | grep vaino` and `systemctl show vaino -p MemoryPeak`.

---

## 8. Acceptance

**`[IMPL-ACC-010]`** The appliance is ready when all of these hold:

| Check | Requirement | Target |
| :--- | :--- | :--- |
| Peak RSS during playback | `[REQ-HW-100]` | ≤150 MB |
| Power-on to first audio | `[REQ-HW-110]` | measure, then set a target |
| 10 hard power cuts, no corruption | `[REQ-HW-120]` | database opens every time |
| Skip latency | `[REQ-AUD-110]` | ≤500 ms |
| 72 h unattended | `[GDE-PHS-020]` | no leak, no drift, no dropout |
| 244.9-minute DAO file plays | `[GDE-FBD-010]` | within the memory budget |

**`[IMPL-ACC-020]`** The last two are the ones that cannot be checked on a desktop and are the reason this hardware matters. `memcheck` measured 15.0 MB for the 244.9-minute file on x86; the Pi figure is what counts.

---

## 9. Open

1. **`[IMPL-OPN-010]`** Which audio output — the choice in `[IMPL-BOM-010]` is unmade, and it changes §5 entirely.
2. **`[IMPL-OPN-020]`** Whether 64-bit is right at 512 MB. It matches the verified build, but 32-bit uses less memory for pointer-heavy work. Vaino's footprint is buffer-dominated, so the difference should be small — worth measuring both if the margin proves tight.
3. **`[IMPL-OPN-030]`** Where the class-D export goes off-device `[SPEC-DF-094]`, since on-card backups die with the card.

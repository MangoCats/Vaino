# IMPL001: Raspberry Pi Zero 2W Appliance Setup

**Implementation Guide — Tier 3**

Step-by-step build of a Pi Zero 2W as a Vaino appliance. Implements the hardware and storage model in [embedded-hardware.md](embedded-hardware.md) and the requirements in [REQ002 §6](spec/REQ002-functional-requirements.md#6-appliance--hw).

> **Status: UNVALIDATED.** Written from specification and the target's known constraints; not yet executed on hardware. Each step carries a **Verify** line — record the actual result, and correct this document where reality disagrees. MuLibPlay's Pi notes decayed into scattered tips precisely because nothing was checked back in.

---

## 1. Output Profile — Choose First

**`[IMPL-BOM-010]` The Pi Zero 2W has no analog audio output.** Only mini-HDMI and USB. This is the most consequential hardware fact and it must be settled before flashing.

**`[IMPL-PROF-010]` The output choice also fixes the boot profile.** They are one decision, not two: a Bluetooth sink must associate before audio can flow, so fast boot and Bluetooth are **alternatives rather than a compromise to be split** `[REQ-HW-114]`.

| Profile | Output | Boot | Extra userspace |
| :--- | :--- | :--- | :--- |
| **A — Bluetooth** ⭐ *tested first* | A2DP sink | **slow, accepted** `[REQ-HW-112]` | `bluez`, `bluez-alsa-utils` |
| **B — I2S DAC HAT** | GPIO DAC | fastest | none |
| **C — USB DAC** | USB audio | fast | none |
| **D — HDMI** | mini-HDMI | fast | none |

**`[IMPL-PROF-020]` Profile A is the initially tested channel.** Its startup delay is accepted **for this profile only** and must not become the project's boot standard `[REQ-HW-112]`. MuLibPlay needed a 5-second delay for exactly this reason, so the cost is known and inherent to the channel rather than to Vaino.

**`[IMPL-PROF-030]` Profiles B–D stay supported, not merely possible.** Vaino selects its output by name at runtime (`Output::open_device`), so switching profiles is configuration rather than a rebuild. Anything added for Bluetooth must be conditional: `bluealsa` and the association wait belong to Profile A alone, and enabling them for everyone would impose Bluetooth's boot cost on hardware that does not need it.

Also required: microSD (**A2-rated**, endurance-grade — this runs 24/7), a 5 V ≥2.5 A supply, and the music library.

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
# Profiles B-D ONLY. Under Profile A this is what makes audio work at all.
[ "$VAINO_PROFILE" != A ] && sudo systemctl disable --now bluetooth
sudo systemctl disable --now triggerhappy avahi-daemon
sudo systemctl disable --now ModemManager wpa_supplicant@wlan1 2>/dev/null
sudo apt-get purge -y  # nothing to purge on Lite by default; confirm before adding
sudo systemctl mask systemd-networkd-wait-online.service   # audio must not wait for Wi-Fi
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

## 5. Audio — Profile A (Bluetooth)

**`[IMPL-AUD-010]`** Identify what exists first: `aplay -l`, and after setup `vaino --list-devices`.

**`[IMPL-AUD-050]` A2DP needs a bridge into ALSA.** Vaino's `cpal` backend speaks ALSA, and BlueZ alone does not expose an A2DP sink as an ALSA PCM. This corrects the "no PulseAudio, ALSA directly" guidance elsewhere in this document — that holds for Profiles B–D, **not** for Bluetooth.

```bash
sudo apt-get install -y bluez bluez-alsa-utils
```

`bluez-alsa` (`bluealsa`) is preferred over PipeWire or PulseAudio here purely on footprint: this is a 512 MB machine with a ≤150 MB budget for Vaino `[REQ-HW-100]`.

**`[IMPL-AUD-060]` Pair and trust once**, so later boots reconnect without interaction:

```bash
bluetoothctl
> power on
> agent on
> scan on            # note the speaker's MAC
> pair    AA:BB:CC:DD:EE:FF
> trust   AA:BB:CC:DD:EE:FF     # trust is what makes reconnection automatic
> connect AA:BB:CC:DD:EE:FF
> quit
```

**`[IMPL-AUD-070]` Vaino waits for the sink rather than systemd doing it.** The unit must not block on Bluetooth `[IMPL-SVC-020]`; instead Vaino retries `open_device(Some("bluealsa"))` until the sink appears, then starts audio. Two reasons: the web UI and database come up during the wait rather than after it, and the delay stays contained in Profile A's configuration instead of the service definition.

> **Verify:** `aplay -D bluealsa /usr/share/sounds/alsa/Front_Center.wav` plays through the speaker. Record **power-on to first audio** — that number is Profile A's accepted cost `[REQ-HW-112]`, and the figure Profiles B–D are measured against.

### Profiles B–D

**`[IMPL-AUD-020]` I2S DAC HAT.** In `/boot/firmware/config.txt`, and no extra userspace:

```
dtparam=audio=off
dtoverlay=hifiberry-dac        # substitute the overlay for the specific HAT
```

**`[IMPL-AUD-030]` Pin the default device** in `/etc/asound.conf` so Vaino never depends on probe order:

```
defaults.pcm.card 0
defaults.ctl.card 0
```

**`[IMPL-AUD-040]`** For Profiles B–D, install neither PulseAudio nor PipeWire: `cpal` uses ALSA directly, and each additional layer adds latency and a failure mode. Disable Bluetooth entirely to claim the faster boot `[REQ-HW-114]`.

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

**`[IMPL-SVC-020]`** `/etc/systemd/system/vaino.service`. The unit is **identical across all four profiles** — the output difference lives in Vaino's configuration, not here `[IMPL-PROF-030]`. That is deliberate: putting a Bluetooth wait into the unit would impose it on hardware that does not need it.

```ini
[Unit]
Description=Vaino
# Deliberately NOT After=network-online.target, and NOT After=bluetooth:
# audio must start without waiting for Wi-Fi, and under Profile A the wait for
# the A2DP sink belongs inside Vaino [IMPL-AUD-070] so the web UI and database
# come up during it rather than after it.
After=sound.target
Wants=sound.target

[Service]
Type=simple
# --output selects the profile: "bluealsa" (A), "hifiberry" (B), a USB DAC
# name (C), or omitted for the system default (D).
ExecStart=/usr/local/bin/vaino --db /var/lib/vaino/vaino.db --music /srv/music           --output ${VAINO_OUTPUT}
EnvironmentFile=/etc/vaino.conf
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
| Power-on to first audio, Profile A | `[REQ-HW-112]` | measure; accepted as-is |
| Power-on to first audio, Profiles B-D | `[REQ-HW-114]` | measure; must beat Profile A |
| 10 hard power cuts, no corruption | `[REQ-HW-120]` | database opens every time |
| Skip latency | `[REQ-AUD-110]` | ≤500 ms |
| 72 h unattended | `[GDE-PHS-020]` | no leak, no drift, no dropout |
| 244.9-minute DAO file plays | `[GDE-FBD-010]` | within the memory budget |

**`[IMPL-ACC-020]`** The last two are the ones that cannot be checked on a desktop and are the reason this hardware matters. `memcheck` measured 15.0 MB for the 244.9-minute file on x86; the Pi figure is what counts.

---

## 9. Open

1. ~~**`[IMPL-OPN-010]`** Which audio output~~ — **DECIDED: Profile A (Bluetooth) is tested first** `[IMPL-PROF-020]`, with B–D kept supported and configurable `[IMPL-PROF-030]`. Its boot delay is accepted for that profile alone.
2. ~~**`[IMPL-OPN-040]`** bluez-alsa vs PipeWire~~ — **DECIDED: `bluez-alsa`**, on footprint for a 512 MB host.
2. **`[IMPL-OPN-020]`** Whether 64-bit is right at 512 MB. It matches the verified build, but 32-bit uses less memory for pointer-heavy work. Vaino's footprint is buffer-dominated, so the difference should be small — worth measuring both if the margin proves tight.
3. **`[IMPL-OPN-030]`** Where the class-D export goes off-device `[SPEC-DF-094]`, since on-card backups die with the card.

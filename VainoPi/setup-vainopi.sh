#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Configure a Raspberry Pi Zero 2 W as a Vaino test appliance, per PI002.
#
# **Idempotent by construction.** Every step checks the state it intends to
# create and does nothing if it is already there, so this is safe to re-run
# after a partial failure, after a reboot, or simply to confirm the machine
# still matches the script. Re-running it is the supported way to find out
# what has drifted.
#
# Not the appliance of PI001: no read-only root, no three partitions, no
# overlay, no access point. Those are easier to add to a machine already known
# to play music.
#
#   scp VainoPi/setup-vainopi.sh pi@vainopi:
#   ssh pi@vainopi 'sudo bash setup-vainopi.sh'
#
# Options:
#   --speaker AA:BB:CC:DD:EE:FF   pair, trust and connect a Bluetooth sink
#   --no-boot-tune               skip config.txt changes (no reboot needed)

set -euo pipefail

SPEAKER=""
BOOT_TUNE=1
while [ $# -gt 0 ]; do
    case "$1" in
        --speaker) SPEAKER="${2:-}"; shift 2 ;;
        --no-boot-tune) BOOT_TUNE=0; shift ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

[ "$(id -u)" -eq 0 ] || { echo "run with sudo" >&2; exit 1; }

RUN_USER="${SUDO_USER:-pi}"
RUN_UID="$(id -u "$RUN_USER")"
CHANGED=0
note() { printf '  %-46s %s\n' "$1" "$2"; }
did()  { CHANGED=1; note "$1" "CHANGED"; }
ok()   { note "$1" "ok"; }

echo "VainoPi setup — user $RUN_USER (uid $RUN_UID)"
echo

# ---------------------------------------------------------------- packages
echo "packages"
NEED=""
for p in pipewire pipewire-pulse wireplumber libspa-0.2-bluetooth bluez \
         libasound2 alsa-utils sqlite3; do
    dpkg -s "$p" >/dev/null 2>&1 || NEED="$NEED $p"
done
if [ -n "$NEED" ]; then
    apt-get update -qq
    # shellcheck disable=SC2086
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq $NEED >/dev/null
    did "install:$NEED"
else
    ok "all present"
fi

# ------------------------------------------------------------ audio session
# PipeWire runs as the LOGIN user, not as root and not as the vaino service
# user. `linger` is what lets that session exist without anyone logged in --
# without it the audio graph disappears the moment the ssh session closes,
# which reads as "Bluetooth stopped working after I disconnected".
echo "audio session"
if [ "$(loginctl show-user "$RUN_USER" -p Linger --value 2>/dev/null)" != "yes" ]; then
    loginctl enable-linger "$RUN_USER"
    did "enable-linger $RUN_USER"
else
    ok "linger enabled"
fi

sudo -u "$RUN_USER" XDG_RUNTIME_DIR="/run/user/$RUN_UID" \
    systemctl --user enable pipewire pipewire-pulse wireplumber >/dev/null 2>&1 || true
ok "user services enabled"

# ------------------------------------------------------------- sample rate
# PipeWire's graph defaults to 48 kHz. The library is 44.1 and the speaker
# accepts 44.1, so a default install resamples for nothing and the sink may
# resample back. [PI2-RATE-010]
echo "sample rate"
CONF_DIR="/home/$RUN_USER/.config/pipewire/pipewire.conf.d"
RATE_CONF="$CONF_DIR/10-rate.conf"
WANT_RATE='context.properties = {
    default.clock.rate          = 44100
    default.clock.allowed-rates = [ 44100 ]
}'
if [ ! -f "$RATE_CONF" ] || [ "$(cat "$RATE_CONF")" != "$WANT_RATE" ]; then
    install -d -o "$RUN_USER" -g "$RUN_USER" "$CONF_DIR"
    printf '%s\n' "$WANT_RATE" > "$RATE_CONF"
    chown "$RUN_USER:$RUN_USER" "$RATE_CONF"
    did "44100 Hz pinned"
else
    ok "44100 Hz pinned"
fi

# -------------------------------------------------------------- vaino user
# A service account with no shell and no home: it plays audio and writes one
# database. `audio` for ALSA, `bluetooth` so it may talk to BlueZ.
echo "service account"
if ! id vaino >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin vaino
    did "created user vaino"
else
    ok "user vaino"
fi
for g in audio bluetooth; do
    getent group "$g" >/dev/null || continue
    if ! id -nG vaino | tr ' ' '\n' | grep -qx "$g"; then
        usermod -aG "$g" vaino
        did "vaino -> group $g"
    fi
done

# -------------------------------------------------------------- directories
echo "directories"
for d in /srv/library /var/vaino; do
    if [ ! -d "$d" ]; then
        install -d -o vaino -g vaino -m 0755 "$d"
        did "created $d"
    else
        ok "$d"
    fi
done

# ------------------------------------------------------------------ binary
# Installed only if one was staged beside this script; the script stays useful
# for re-configuring a machine whose binary is already in place.
echo "binary"
SRC=""
for c in ./vaino /home/"$RUN_USER"/vaino; do
    [ -f "$c" ] && SRC="$c" && break
done
if [ -n "$SRC" ]; then
    if ! cmp -s "$SRC" /usr/local/bin/vaino 2>/dev/null; then
        install -m 0755 "$SRC" /usr/local/bin/vaino
        did "installed $(/usr/local/bin/vaino --version 2>/dev/null || echo vaino)"
    else
        ok "binary current"
    fi
elif [ -x /usr/local/bin/vaino ]; then
    ok "binary present (none staged)"
else
    note "binary" "ABSENT — stage ./vaino beside this script"
fi

# ----------------------------------------------------------------- service
echo "service"
UNIT=/etc/systemd/system/vaino.service
read -r -d '' WANT_UNIT <<'EOF' || true
[Unit]
Description=Vaino
# Deliberately NOT network-online.target: audio depends on the library and the
# sound device, never on the network [REQ-HW-010B].
After=local-fs.target sound.target

[Service]
ExecStart=/usr/local/bin/vaino /srv/library/vaino.db --port 5720
Restart=always
RestartSec=2
User=vaino
Nice=-5
# PipeWire lives in the login user's session; the service reaches it there.
Environment=XDG_RUNTIME_DIR=/run/user/RUNUID
Environment=PULSE_SERVER=unix:/run/user/RUNUID/pulse/native

[Install]
WantedBy=multi-user.target
EOF
WANT_UNIT="${WANT_UNIT//RUNUID/$RUN_UID}"
if [ ! -f "$UNIT" ] || [ "$(cat "$UNIT")" != "$WANT_UNIT" ]; then
    printf '%s\n' "$WANT_UNIT" > "$UNIT"
    systemctl daemon-reload
    did "wrote vaino.service"
else
    ok "vaino.service"
fi
if ! systemctl is-enabled --quiet vaino 2>/dev/null; then
    systemctl enable vaino >/dev/null 2>&1 && did "enabled vaino" || note "enable vaino" "deferred"
else
    ok "enabled"
fi

# -------------------------------------------------------------- boot tuning
# Safe, reversible settings only. The riskier work -- initramfs trimming, unit
# parallelisation -- waits for a boot-time baseline.
if [ "$BOOT_TUNE" -eq 1 ]; then
    echo "boot tuning (needs a reboot to take effect)"
    CFG=/boot/firmware/config.txt
    [ -f "$CFG" ] || CFG=/boot/config.txt
    add_cfg() {
        if ! grep -qxF "$1" "$CFG"; then
            printf '%s\n' "$1" >> "$CFG"
            did "config.txt: $1"
        else
            ok "config.txt: $1"
        fi
    }
    # 16 MB to the GPU on a machine with no display. Measured 416 MB usable of
    # 512 before this; the split is the largest single reclaim available.
    add_cfg "gpu_mem=16"
    add_cfg "disable_splash=1"
    add_cfg "boot_delay=0"
    add_cfg "dtoverlay=disable-bt-led"

    for svc in triggerhappy avahi-daemon ModemManager; do
        if systemctl list-unit-files "$svc.service" >/dev/null 2>&1 \
           && systemctl is-enabled --quiet "$svc" 2>/dev/null; then
            systemctl disable --now "$svc" >/dev/null 2>&1
            did "disabled $svc"
        fi
    done
fi

# ---------------------------------------------------------------- bluetooth
if [ -n "$SPEAKER" ]; then
    echo "bluetooth $SPEAKER"
    systemctl is-active --quiet bluetooth || systemctl start bluetooth
    if bluetoothctl info "$SPEAKER" 2>/dev/null | grep -q "Paired: yes"; then
        ok "paired"
    else
        bluetoothctl --timeout 20 scan on >/dev/null 2>&1 || true
        bluetoothctl pair "$SPEAKER" >/dev/null 2>&1 && did "paired" \
            || note "pair" "FAILED — put the speaker in pairing mode"
    fi
    # `trust` is the step people miss: without it the speaker pairs, works,
    # and never reconnects after a reboot. [PI2-BT-010]
    if bluetoothctl info "$SPEAKER" 2>/dev/null | grep -q "Trusted: yes"; then
        ok "trusted"
    else
        bluetoothctl trust "$SPEAKER" >/dev/null 2>&1 && did "trusted" || true
    fi
    bluetoothctl info "$SPEAKER" 2>/dev/null | grep -q "Connected: yes" \
        && ok "connected" \
        || { bluetoothctl connect "$SPEAKER" >/dev/null 2>&1 && did "connected" \
             || note "connect" "not connected — is it powered on?"; }
fi

echo
if [ "$CHANGED" -eq 0 ]; then
    echo "No changes: the machine already matches this script."
else
    echo "Done. Re-run to confirm it settles with no further changes."
    [ "$BOOT_TUNE" -eq 1 ] && echo "A reboot is needed for the config.txt changes."
fi

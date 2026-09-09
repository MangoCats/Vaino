#!/bin/bash
# Read the speaker's rocker and act on it [PI3-ROCKER-010].
#
# Also the characterisation tool: EVERY key seen is logged with its code and
# name, whether or not it is mapped. The mapping from the Middleton's five
# gestures to key codes has never been observed, so the first run of this is
# the measurement, and an unmapped press is a finding rather than a fault.
#
# Safety: whenever this lowers Wi-Fi it FIRST arms a detached restore timer.
# The radio comes back even if this script is killed, wedges, or the machine
# does something unexpected -- a headless box that can silence its own way in
# must never depend on a running process to undo that.
set -uo pipefail
PORT="${VAINO_PORT:-5720}"
SPEAKER="${SPEAKER:-20:64:DE:CF:F3:AD}"
IFACE="${IFACE:-wlan0}"
DEADMAN="${DEADMAN:-900}"          # wifi returns after this no matter what
export XDG_RUNTIME_DIR="/run/user/$(id -u)"

say() { logger -t vaino-rocker "$*"; echo "$(date +%T) $*"; }
post() { curl -s -o /dev/null -m 4 -X POST "http://localhost:$PORT/command/$1"; }

find_dev() {
    grep -B4 'Handlers=.*event' /proc/bus/input/devices \
      | grep -A4 'AVRCP' | grep -oE 'event[0-9]+' | head -1
}

arm_deadman() {
    sudo systemd-run --on-active="$DEADMAN" --timer-property=AccuracySec=5s \
        --unit=vaino-rocker-wifi --quiet \
        /bin/sh -c "nmcli radio wifi on 2>/dev/null || ip link set $IFACE up" \
        2>/dev/null
}

wifi_down() {
    arm_deadman || { say "REFUSING to lower wifi: no restore timer"; return 1; }
    say "wifi down"
    sudo nmcli radio wifi off 2>/dev/null || sudo ip link set "$IFACE" down
    sleep 2
    # Bluetooth is established only AFTER the radio is quiet, so the link is
    # not negotiated under the interference that [PI3-FOUND-010] measured.
    bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes' \
        || bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
    sleep 5
    if bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes'; then
        post reopen-output
        say "bluetooth up with wifi down"
    else
        # Silent AND unreachable is the worst state available, and it is
        # reached by someone switching the speaker off. Do not sit in it.
        say "bluetooth did not come up; restoring wifi"
        wifi_up
    fi
}

wifi_up() {
    sudo nmcli radio wifi on 2>/dev/null || sudo ip link set "$IFACE" up
    sudo systemctl stop vaino-rocker-wifi.timer 2>/dev/null
    sudo systemctl reset-failed vaino-rocker-wifi.timer 2>/dev/null
    say "wifi up"
}

PLAYING=1        # the server is started playing for this test
DEV="$(find_dev)"
[ -z "$DEV" ] && { say "no AVRCP device; is the speaker connected?"; exit 1; }
say "reading /dev/input/$DEV"

sudo stdbuf -oL evtest "/dev/input/$DEV" 2>/dev/null | while read -r line; do
    case "$line" in *"(EV_KEY)"*", value 1"*) ;; *) continue ;; esac
    CODE=$(echo "$line" | sed -n 's/.*code \([0-9]*\) (\([A-Z_0-9]*\)).*/\1 \2/p')
    NAME=${CODE#* }
    say "KEY $CODE"
    case "$NAME" in
        KEY_PLAYPAUSE|KEY_PLAYCD|KEY_PAUSECD|KEY_PLAY|KEY_PAUSE)
            if [ "$PLAYING" = 1 ]; then
                post pause; PLAYING=0; wifi_up
            else
                post play;  PLAYING=1; wifi_down
            fi ;;
        KEY_NEXTSONG|KEY_FORWARD)  post skip ;;
        KEY_PREVIOUSSONG|KEY_BACK) say "left: reserved, ignored" ;;
        *) say "unmapped: $NAME" ;;
    esac
done

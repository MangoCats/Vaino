#!/bin/sh
# Keep the chosen speaker connected, without anyone typing anything.
#
# The gap this fills [PI3-AIM-030]: BlueZ accepts a trusted device that comes
# to it, but nothing on the Pi ever reaches OUT. So a speaker switched on after
# boot stays unconnected, the player honestly reports silence, and a listener
# concludes it is broken.
#
# Deliberately dumb: check, connect if absent, tell the player to reopen, stop.
# A timer runs it. Nothing here retries in a loop, because a loop is a thing
# that can wedge and this must not be the reason audio stops.
set -u
DB="${VAINO_DB:-/srv/library/vaino.db}"
export XDG_RUNTIME_DIR="/run/user/$(id -u)"

# The address is whatever the player last recorded through `use`/`pair`
# [PI3-AIM-020], [REQ-VIS-260] -- not a hard-coded guess. A speaker chosen
# once through the settings panel is the one this timer chases from then on,
# on any appliance, without editing this file or its unit. `SPEAKER` still
# overrides it, for a library with no player-chosen speaker yet, or a
# deliberate manual pin.
#
# **Absent is a real answer, not an error.** Paging a device the shared
# Bluetooth radio cannot reach stalls whatever the appliance IS playing for
# several seconds -- measured as an audible skip with the position display
# frozen, and invisible to the player's own underrun counter, because the
# stall happens on the radio and never touches the output ring at all. A
# stale or empty address must do nothing, not page something.
SPEAKER="${SPEAKER:-$(sqlite3 "$DB" \
    "SELECT value FROM player_settings WHERE key = 'speaker_address'" 2>/dev/null)}"
[ -n "$SPEAKER" ] || exit 0

bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes' && exit 0

bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
sleep 5
bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes' || exit 0

# Connected. The stream does not dependably follow a change of default sink
# [PI3-WHY-020], so the player is told explicitly -- and it is told only after
# a connection actually succeeded, so a reopen is never spent on nothing.
curl -s -o /dev/null -X POST "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
echo "connected $SPEAKER and asked the player to reopen"

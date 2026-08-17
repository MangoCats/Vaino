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
SPEAKER="${SPEAKER:-20:64:DE:CF:F3:AD}"
export XDG_RUNTIME_DIR="/run/user/$(id -u)"

bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes' && exit 0

bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
sleep 5
bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes' || exit 0

# Connected. The stream does not dependably follow a change of default sink
# [PI3-WHY-020], so the player is told explicitly -- and it is told only after
# a connection actually succeeded, so a reopen is never spent on nothing.
curl -s -o /dev/null -X POST "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
echo "connected $SPEAKER and asked the player to reopen"

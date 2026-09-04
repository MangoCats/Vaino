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
SPEAKER="${SPEAKER:-$(sqlite3 "$DB" \
    "SELECT value FROM player_settings WHERE key = 'speaker_address'" 2>/dev/null)}"

# **Ground truth first, stored belief second** [PI3-AIM-040]. Recorded once,
# live 2026-09-04: `speaker_address` had gone stale (still MIDDLETON, from
# earlier testing) while the appliance was actually connected to and playing
# through a different, real speaker (OontZ_Angle 3, paired straight through
# bluetoothctl rather than the player's own `use` picker, which is the one
# path that keeps this row honest). Every tick this ran, it paged MIDDLETON
# -- unreachable, since nothing was asking it to be reachable -- which ties
# up the one shared radio and stalled the speaker that WAS actually playing,
# for several seconds, invisible to the output ring's own underrun counter.
# Exactly the [PI3-AIM-020] fault recurring for a new reason: last time the
# address was wrong because it was hard-coded, this time because it was
# merely out of date. The fix generalises past "read the stored value" to
# "believe whatever is actually connected over whatever is merely
# remembered" -- if BlueZ already has a real, audio-capable device
# connected, right now, that is the answer, whether or not it matches
# `SPEAKER`, and there is nothing left to do: paging the stored address on
# top of a working connection is the disruption, not the fix.
CONNECTED=""
for addr in $(bluetoothctl devices Connected 2>/dev/null | awk '{print $2}'); do
    bluetoothctl info "$addr" 2>/dev/null | grep -q 'UUID: Audio Sink' || continue
    CONNECTED="$addr"
    break
done

if [ -n "$CONNECTED" ]; then
    if [ "$CONNECTED" != "$SPEAKER" ]; then
        # Reality moved on from what Vaino remembers -- catch the
        # bookkeeping up to it, silently: audio is already flowing, so there
        # is nothing to reopen and no reason to touch the player at all.
        # Shape-checked before it reaches SQL, the same discipline
        # `bluetooth.rs::is_address` applies to an address arriving from a
        # browser -- this one arrives from bluetoothctl's own output instead
        # of a request, but "about to become a value written to the
        # database" is the same property either way.
        case "$CONNECTED" in
            ??:??:??:??:??:??)
                sqlite3 "$DB" "INSERT INTO player_settings (key, value, updated_at) \
                     VALUES ('speaker_address', '$CONNECTED', datetime('now')) \
                     ON CONFLICT(key) DO UPDATE SET \
                         value = excluded.value, updated_at = excluded.updated_at" \
                    2>/dev/null \
                    && echo "adopted $CONNECTED as the speaker (was ${SPEAKER:-<none>})"
                ;;
        esac
    fi
    exit 0
fi

# **Absent is a real answer, not an error.** Paging a device the shared
# Bluetooth radio cannot reach stalls whatever the appliance IS playing for
# several seconds -- measured as an audible skip with the position display
# frozen, and invisible to the player's own underrun counter, because the
# stall happens on the radio and never touches the output ring at all. A
# stale or empty address must do nothing, not page something.
[ -n "$SPEAKER" ] || exit 0

bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
sleep 5
bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes' || exit 0

# Connected. The stream does not dependably follow a change of default sink
# [PI3-WHY-020], so the player is told explicitly -- and it is told only after
# a connection actually succeeded, so a reopen is never spent on nothing.
curl -s -o /dev/null -X POST "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
echo "connected $SPEAKER and asked the player to reopen"

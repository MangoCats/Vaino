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
        # bookkeeping up to it, silently.
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

    # **`[PI3-AIM-050]` Connected is not the same claim as playing.**
    # `[PI3-AIM-040]` stopped as soon as BlueZ showed a real device
    # connected, on the belief that audio must already be flowing -- true
    # only when this script itself put the connection there. It is false
    # whenever the player's own stream got bound before this device did:
    # `vaino-wait-sink` releases the player on the first *any* real sink it
    # sees, which on this hardware can be the onboard HDMI output, seconds
    # before Middleton's A2DP transport actually comes up. It is equally
    # false mid-session, when BlueZ reconnects a trusted device entirely on
    # its own -- which it does -- with nobody having asked this script to do
    # anything. Either way the device link is fine and the player is simply
    # talking to the wrong sink, which is indistinguishable from "can't
    # connect to Middleton" to anyone listening. So ground truth is checked
    # one layer further in than `[PI3-AIM-040]` did: not just "is the device
    # connected" but "is the player's stream actually linked to it"
    # `[PI3-WHY-020]` -- the same question `GET /audio/sink` already answers
    # for the settings panel `[SPEC-APS-060]`. `wpctl` names a PipeWire sink
    # node after the alias BlueZ reports for the device, so the two are
    # compared as text; a mismatch, including "nothing" or "Dummy Output",
    # is the one case a reopen is actually for.
    ALIAS=$(bluetoothctl info "$CONNECTED" 2>/dev/null |
        sed -n 's/^[[:space:]]*Alias: //p')
    ROUTED=$(curl -s "http://localhost:${VAINO_PORT:-5720}/audio/sink" 2>/dev/null |
        sed -n 's/.*"sink":"\([^"]*\)".*/\1/p')
    if [ -n "$ALIAS" ] &&
       [ "$(printf '%s' "$ROUTED" | tr a-z A-Z)" != "$(printf '%s' "$ALIAS" | tr a-z A-Z)" ]; then
        curl -s -o /dev/null -X POST "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
        echo "$CONNECTED is connected but the stream was on '${ROUTED:-nothing}', not '$ALIAS' -- asked the player to reopen"
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

# **`[PI3-AIM-060]` How hard to chase depends on what chasing can cost.**
#
# `[PI3-AIM-020]`/`[PI3-AIM-040]` both ended in the same injury: paging a
# device that could not answer tied up the shared radio and stalled the
# speaker that WAS playing. The conclusion drawn then -- try once, briefly,
# and stop -- was right about the risk and wrong as a general policy, because
# it also governs the case where nothing is playing at all, where there is
# no audio to protect and the timid attempt simply loses.
#
# And on this appliance losing is the default. `[PI3-FOUND-090]`: the
# Middleton powers the Pi from its own USB port, so the two can only power up
# together; the Pi needs ~30 s to reach a working Bluetooth stack, and any
# phone already awake in the room has had the speaker since second two. The
# speaker then stops answering pages entirely, so the Pi's one attempt comes
# back `br-connection-page-timeout` and the appliance concludes, every single
# power cycle, that its speaker is switched off.
#
# So the cost is what sets the effort, not a fixed rule: if the player is on
# a real sink right now, something is audible and the old timidity is exactly
# right. If it is on a dummy or on nothing, there is no audio to interrupt,
# and the radio's time is better spent staying after the speaker than idle.
ROUTED=$(curl -s "http://localhost:${VAINO_PORT:-5720}/audio/sink" 2>/dev/null |
    sed -n 's/.*"sink":"\([^"]*\)".*/\1/p')
case "${ROUTED:-none}" in
    none|"Dummy Output") BUDGET="${VAINO_CHASE_SECONDS:-22}" ;;
    *)                   BUDGET=0 ;;
esac

# Bounded by wall clock, not by a count of tries: each attempt costs whatever
# the page timeout happens to be, and the number that must stay under the
# timer's own period is seconds. Nothing here loops without a deadline --
# a wedged keeper is the one thing this must never become.
started=$(date +%s)
while :; do
    bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
    sleep 2
    if bluetoothctl info "$SPEAKER" 2>/dev/null | grep -qi 'Connected: yes'; then
        # Connected. The stream does not dependably follow a change of default
        # sink [PI3-WHY-020], so the player is told explicitly -- and only
        # after a connection actually succeeded, so a reopen is never spent on
        # nothing.
        curl -s -o /dev/null -X POST \
            "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
        echo "connected $SPEAKER after $(( $(date +%s) - started ))s and asked the player to reopen"
        exit 0
    fi
    [ $(( $(date +%s) - started )) -lt "$BUDGET" ] || break
done

# Not reached. Said once per tick, and only when there was nothing to lose by
# trying, so the log shows a speaker being waited for rather than an appliance
# repeating that it has failed.
[ "$BUDGET" -gt 0 ] && echo "$SPEAKER did not answer in ${BUDGET}s (held by another device, or asleep)"
exit 0

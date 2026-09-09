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
# **`[PI3-FOUND-280]` The listener's settings moved and this did not follow.**
# `[IMPL-DBSPLIT-025]` split the database: the catalog stayed in
# `/srv/library/`, and everything the listener chooses -- volume, programme,
# and `speaker_address` -- moved to `/var/vaino/listener.db`. These scripts
# kept reading the pre-split file, which still exists and still holds a
# `speaker_address` row. It was the *same* address, so nothing looked wrong
# for weeks.
#
# It stopped being the same the moment a second speaker was chosen. Measured:
# the settings page moved the player to the OontZ and wrote that address to
# `listener.db`; this script read MIDDLETON out of the stale file, decided the
# routing disagreed with the speaker "on record", and asked the player to
# reopen -- fighting the listener's own choice every thirty seconds, on the
# authority of a file nothing had written to in weeks.
#
# Chosen by what exists rather than by a build-time flag, so one script serves
# a split appliance and an unsplit one without being told which it is on.
DB="${VAINO_DB:-}"
if [ -z "$DB" ]; then
    if [ -f /var/vaino/listener.db ]; then
        DB=/var/vaino/listener.db
    else
        DB=/srv/library/vaino.db
    fi
fi
export XDG_RUNTIME_DIR="/run/user/$(id -u)"

# The address is whatever the player last recorded through `use`/`pair`
# [PI3-AIM-020], [REQ-VIS-260] -- not a hard-coded guess. A speaker chosen
# once through the settings panel is the one this timer chases from then on,
# on any appliance, without editing this file or its unit. `SPEAKER` still
# overrides it, for a library with no player-chosen speaker yet, or a
# deliberate manual pin.
SPEAKER="${SPEAKER:-$(sqlite3 "$DB" \
    "SELECT value FROM player_settings WHERE key = 'speaker_address'" 2>/dev/null)}"

# **`[PI3-FOUND-130]` Trust is what lets the speaker reconnect to US**, and it
# is the first thing a recovery throws away.
#
# BlueZ auto-authorises an incoming service connection only from a device
# marked trusted. Untrusted, it asks an agent instead; this appliance
# registers one only for the duration of a `pair` `[PI3-WHY-060]`, so at every
# other moment there is nobody to ask and the request is refused outright.
# Measured 2026-09-08, on the boot after the speaker was hard-reset and
# re-paired by hand: the Middleton had an ACL link to the Pi by 20.8 s and
# then tried three times to bring up A2DP -- 28.9 s, 37.8 s, 46.7 s -- and was
# rejected every time with `Authentication attempt without agent` /
# `Access denied`. Its persisted record read `Trusted=false` while the link
# key beside it was perfectly good. Audio did not arrive until this script's
# own outbound connect won at 55.7 s: some thirty-five seconds spent refusing
# the speaker's own offers to connect.
#
# That matters beyond the delay. The speaker reaching out to us is the ONE
# path that does not have to win the power-up race `[PI3-FOUND-090]` -- it
# costs no paging, no shared-radio time, and it starts the moment the speaker
# is awake. Losing trust silently disables it and leaves only the race, which
# is the path this appliance loses by design.
#
# `use` has always trusted `[PI3-WHY-040]`, but a listener who recovers by
# forgetting the device and reconnecting -- through `bluetoothctl`, or through
# any path but that one verb -- lands on a bonded, untrusted speaker and
# nothing ever puts it back. So it is asserted here instead: every tick,
# idempotent, checked before it is set so a healthy appliance spends nothing,
# and loud when it actually had to repair something.
# **`[PI3-FOUND-230]` Ask bluetoothd once, not four times.** Every
# `bluetoothctl` invocation opens a D-Bus connection and enumerates the
# adapter's objects, and this script had grown to three or four of them per
# tick -- trust, connected-list, audio-sink, alias -- against a daemon that is
# at that moment carrying an A2DP stream. Measured: the listener's stutters at
# 120 s and 151 s land on the timer's own ticks at 118 s and 153 s. The
# information wanted is all in one `info` block, so it is fetched once and
# read several times. The keeper polls; polling should cost as little as it
# can while something is playing.
INFO=$(bluetoothctl info "${SPEAKER:-none}" 2>/dev/null)

if [ -n "${SPEAKER:-}" ] && ! echo "$INFO" | grep -q 'Trusted: yes'; then
    bluetoothctl trust "$SPEAKER" >/dev/null 2>&1 &&
        echo "trusted $SPEAKER -- it was not, so it could not have reconnected on its own"
fi

# **Who is actually connected, which is not always who was chosen.**
#
# The history is worth keeping because it shaped everything below.
# `[PI3-AIM-040]`, 2026-09-04: `speaker_address` had gone stale (still
# MIDDLETON, from earlier testing) while the appliance was connected to and
# playing through a different, real speaker. Every tick, this script paged the
# stored address -- unreachable, since nothing was asking it to be reachable
# -- tying up the one shared radio and stalling the speaker that WAS playing,
# invisible to the output ring's own underrun counter. The conclusion drawn
# then was "believe whatever is connected over whatever is merely remembered",
# and this block adopted any connected audio device on the strength of it.
#
# **That conclusion no longer governs, and the code below no longer does it**
# `[PI3-FOUND-310]`. The injury it was written against is prevented at source
# by `[PI3-AIM-060]`: nothing is paged at all while audio reaches a real sink,
# so a stale address can no longer stall anything. What adoption still did was
# overwrite a choice the listener had just made in the settings panel, the
# moment a second trusted speaker connected itself. So what is computed here
# is only *who is connected*; what to do about it is decided below, and a
# recorded choice wins.
CONNECTED=""
if [ -n "${SPEAKER:-}" ] &&
   echo "$INFO" | grep -q 'Connected: yes' &&
   echo "$INFO" | grep -q 'UUID: Audio Sink'; then
    # The overwhelmingly common case, and it is already answered: the speaker
    # on record is the one connected `[PI3-FOUND-230]`. Asking BlueZ for the
    # connected list here would be asking a question whose answer is in hand.
    CONNECTED="$SPEAKER"
else
    # Reality may have moved on -- a different speaker, or none. Only now is
    # it worth the extra round trips to find out which.
    for addr in $(bluetoothctl devices Connected 2>/dev/null | awk '{print $2}'); do
        bluetoothctl info "$addr" 2>/dev/null | grep -q 'UUID: Audio Sink' || continue
        CONNECTED="$addr"
        break
    done
fi

if [ -n "$CONNECTED" ]; then
    # **`[PI3-FOUND-310]` A choice the listener made outranks whatever turned
    # up.** `[PI3-AIM-040]` had this adopt any connected audio device over the
    # stored address, and was right to: a stale address was being paged every
    # thirty seconds, stalling the speaker that was actually playing. But
    # `[PI3-AIM-060]` later fixed that injury at its source -- nothing is
    # paged at all while audio is reaching a real sink -- which leaves
    # adoption doing only harm.
    #
    # Measured: the listener chose the Middleton in the settings panel and
    # power-cycled. The keeper connected it at 30 s, the OontZ auto-connected
    # behind it (still trusted, so BlueZ reaches for it unprompted), took the
    # one A2DP transport `[PI3-FOUND-290]`, and the Middleton dropped. At
    # 50 s this adopted the OontZ and overwrote the stored address -- and the
    # appliance spent the rest of the boot playing through the speaker the
    # listener had just navigated away from, with the record of their choice
    # destroyed.
    #
    # So adoption now happens only where it cannot contradict anybody:
    # when no speaker has been chosen at all. A recorded choice stands until
    # the listener changes it, and a device that shows up uninvited is
    # reported rather than promoted.
    # Said once per interloper, not once per tick. This is a standing
    # condition rather than an event: with `withdraw_others` taking the
    # auto-connect away from everything but the chosen speaker, a different
    # device connected at all means somebody connected it deliberately, and it
    # may sit there for hours. Repeating it every thirty seconds would put
    # ~2,880 identical lines a day into a journal this appliance deliberately
    # keeps in RAM `[PI3-FOUND-120]`. The marker lives in `/run`, so it clears
    # itself on the next boot and a genuinely new interloper is reported again.
    # In the per-user runtime directory, not /run: this service runs as the
    # login user (`User=pi`), which cannot write /run at all -- the first
    # attempt failed with "Permission denied" every tick, leaking an error to
    # the journal instead of suppressing a message. XDG_RUNTIME_DIR is
    # exported above, is owned by that user, and is cleared on boot, which is
    # exactly the lifetime wanted.
    SEEN="${XDG_RUNTIME_DIR:-/tmp}/vaino-speaker.interloper"
    if [ -n "${SPEAKER:-}" ] && [ "$CONNECTED" != "$SPEAKER" ]; then
        if [ "$(cat "$SEEN" 2>/dev/null)" != "$CONNECTED" ]; then
            # Says what is actually happening. The earlier wording -- "leaving
            # the choice alone" -- read as though the listener's choice were
            # being honoured, when what is being left alone is the device
            # playing instead of it.
            echo "$CONNECTED is connected and audio is going there; $SPEAKER is the chosen speaker and is not present. Not disturbing what is playing -- switch from the settings panel to change it."
            echo "$CONNECTED" > "$SEEN" 2>/dev/null
        fi
    elif [ "$CONNECTED" != "$SPEAKER" ]; then
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
    else
        # The chosen speaker is the one connected, so any earlier interloper
        # is gone: forget it, and a future one gets reported rather than
        # silently matching a stale marker.
        rm -f "$SEEN" 2>/dev/null
    fi

    # **`[PI3-FOUND-380]` A link the speaker opened is worse than one we
    # opened, and redialling it once fixes the boot.**
    #
    # Switching the speaker off cuts this appliance's power with it
    # `[PI3-FOUND-090]`, so both cold-boot together and the speaker -- awake
    # first -- reaches out to its last device. That inbound link measures
    # identically to an outbound one in every respect that can be read: same
    # SBC configuration (`ay 4 17 21 2 53`), same codec, same transport state,
    # same volume. It simply sounds worse: stutters every fifteen seconds or
    # so for around three minutes.
    #
    # Measured 2026-09-09 as a same-boot intervention, which is as controlled
    # as this gets. A boot stuttering on schedule, link reading `>` (inbound);
    # disconnected and reconnected outbound; link then read `<`, configuration
    # unchanged, and the stuttering stopped -- underruns flat across the next
    # sixty seconds and none heard. Nothing else was touched.
    #
    # **Both explanations for that were then refuted `[PI3-FOUND-390]`.** The
    # next mode A boot came up on an *outbound* link, with zero AVDTP
    # collisions, and stuttered anyway -- so neither the direction nor the
    # collision is the mechanism, and a redial conditioned on either would not
    # have fired at all. Worse, the trial that appeared to prove the redial ran
    # at 57 s and a second at 180 s, and mode A settles on its own by about
    # 180 s: both were confounded by the very thing they were meant to measure.
    #
    # So this fires once per boot unconditionally, well before that settling
    # point, for two reasons. It is the only remaining candidate that can be
    # acted on at all, and firing it early is the only way to tell a redial
    # that fixes something from a speaker that was going to settle anyway --
    # if stuttering stops at ~50 s instead of ~180 s, that is an answer.
    #
    # It costs a mode B boot a few seconds of gap it did not previously pay.
    # That is a real regression if the redial turns out to do nothing, and the
    # reason this is written as an experiment with a date on it rather than as
    # a fix.
    REDIAL="${XDG_RUNTIME_DIR:-/tmp}/vaino-speaker.redialled"
    if [ "$CONNECTED" = "${SPEAKER:-}" ] && [ ! -f "$REDIAL" ]; then
        # Marked before it is attempted, not after: a redial that fails must
        # not become a redial that repeats every thirty seconds.
        : > "$REDIAL" 2>/dev/null
        echo "redialling $SPEAKER once, to renegotiate the link the boot came up with"
        # Polled rather than slept through. The first version spent a flat
        # four seconds after the disconnect and six after the connect, and the
        # listener heard the whole eleven-second hole; most of that was this
        # script waiting on a clock rather than on the speaker. Each wait now
        # ends the moment BlueZ agrees, with the fixed sleep kept only as the
        # ceiling.
        bluetoothctl disconnect "$SPEAKER" >/dev/null 2>&1
        i=0
        while [ "$i" -lt 4 ] &&
              bluetoothctl info "$SPEAKER" 2>/dev/null | grep -q 'Connected: yes'; do
            sleep 1
            i=$((i + 1))
        done
        bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
        i=0
        while [ "$i" -lt 10 ] &&
              ! bluetoothctl info "$SPEAKER" 2>/dev/null | grep -q 'Connected: yes'; do
            sleep 1
            i=$((i + 1))
        done
        # One second for WirePlumber to publish the sink the reopen will look
        # for; without it the player reopens onto whatever the old one was.
        sleep 1
        curl -s -o /dev/null -X POST             "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
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
    # From the block already in hand when it is the speaker on record, which
    # is the case that runs every thirty seconds forever `[PI3-FOUND-230]`.
    if [ "$CONNECTED" = "${SPEAKER:-}" ]; then
        ALIAS=$(echo "$INFO" | sed -n 's/^[[:space:]]*Alias: //p')
    else
        ALIAS=$(bluetoothctl info "$CONNECTED" 2>/dev/null |
            sed -n 's/^[[:space:]]*Alias: //p')
    fi
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
# **`[PI3-FOUND-140]` Never page a device the controller already has a link
# to.** `Connected` on the D-Bus device goes true when a PROFILE connects, so
# it reads false through the whole of A2DP negotiation -- and the first
# version of this chase took that as "absent" and paged again every two
# seconds. Every one of those collided with the negotiation already in
# flight: `avdtp_connect_cb() ... Operation already in progress (114)`, eight
# times in one boot, and A2DP that had previously completed at 75 s did not
# finish until 88 s. The chase delayed the thing it existed to hurry.
#
# `hcitool con` is the honest question, because it asks the controller
# whether a baseband link exists rather than asking BlueZ whether a profile
# finished. A link present means a connection is up or coming up, and the
# only useful thing to do is keep out of its way.
started=$(date +%s)
while :; do
    if hcitool con 2>/dev/null | grep -qi "$SPEAKER"; then
        sleep 3
    else
        bluetoothctl connect "$SPEAKER" >/dev/null 2>&1
        # Long enough that two pages cannot overlap: a page that goes
        # unanswered costs the controller its own timeout, and starting the
        # next one on top of it is what produced the collisions above.
        sleep 3
    fi
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

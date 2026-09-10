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
# Does PipeWire actually offer a sink by this name? Parsed from the
# numbered rows only, the same discipline as `vaino-wait-sink`
# `[PI3-FOUND-110]`, so no header or box-drawing line can be mistaken for a
# sink.
sink_present() {
    wpctl status 2>/dev/null |
        sed -n '/Sinks:/,/Sink endpoints/p' |
        sed -n 's/^[^0-9]*[0-9][0-9]*\.[[:space:]]*\(.*\)$/\1/p' |
        sed 's/[[:space:]]*\[vol.*$//; s/[[:space:]]*$//' |
        grep -qxF "$1"
}

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
    # Written where this user can actually write. $XDG_RUNTIME_DIR is *set*
    # under a root run but names /run/user/0, which does not exist, so an
    # empty-check is not enough -- the directory has to be tested.
    RUNDIR="${XDG_RUNTIME_DIR:-/tmp}"
    [ -d "$RUNDIR" ] && [ -w "$RUNDIR" ] || RUNDIR=/tmp
    SEEN="$RUNDIR/vaino-speaker.interloper"
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

    # **`[PI3-FOUND-450]` The once-per-boot redial was here, it worked, and
    # it has been removed anyway.**
    #
    # What it did is not in doubt. Captured at the HCI layer on 2026-09-10
    # `[PI3-FOUND-440]`: the link before it ran low with dips, the link after
    # it ran 46057 B/s flat for 110 s with no dips at all, and four Mode A
    # cycles in a row came up smooth once it had fired. It is the only
    # intervention in this whole investigation that demonstrably changed the
    # symptom.
    #
    # It was removed on the listener's judgement, which is the right authority
    # here: the cure is more disruptive than the disease. It tears down working
    # audio and rebuilds it, and the hole is not small -- 36.2 s on the
    # captured boot, against the eleven seconds the first version was tuned
    # down from. A stutter every fifteen seconds is irritating; half a minute
    # of silence in the middle of a track, on every single boot including the
    # Mode B boots that never stuttered, is worse.
    #
    # It also declared victory on the wrong signal. It polled `Connected: yes`
    # -- the ACL link -- with a ten-second ceiling, so on that boot it posted
    # `reopen-output` roughly 24 s before the speaker was carrying any audio.
    # Recovery came from a later tick's routing check `[PI3-AIM-050]`, not
    # from the redial. Anyone reinstating this must wait on the
    # `MediaTransport1` state instead; `vaino-btctl` has `transport_state()`
    # for it.
    #
    # **Do not read the removal as a finding about the stutter.** The stutter
    # is unfixed and will return on Mode A boots. What is gone is a mitigation
    # whose price the listener declined to keep paying, and removing it buys
    # something the investigation wanted anyway: a clean Mode A capture of a
    # full stutter train from boot, with nothing intervening.

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

    # **`[PI3-AIM-070]` Audio that is already playing somewhere stays**
    # **there.** Stated as the design by the listener on 2026-09-10: once
    # audio is established with one speaker it remains with that speaker
    # even as other recognised speakers become available, and another is
    # connected only if the current one becomes unavailable.
    #
    # This check used to do the opposite. It compared the stream against
    # the *connected* speaker and moved the stream whenever they differed,
    # so a chosen speaker coming back mid-session would drag audio off a
    # speaker that was playing perfectly well. That is the right rule for
    # deciding where to send audio that is going nowhere, and the wrong
    # one for audio that is already going somewhere.
    #
    # Viability, not identity, is the question now: a route is fine if it
    # names a sink PipeWire still offers. `Connected: yes` means BlueZ has
    # a link and says nothing about whether there is anywhere to send audio
    # `[PI3-FOUND-590]`, so the sink is what gets checked at both ends --
    # the one being kept and the one being moved to.
    # And **the listener's selected speaker is the preferred one at start**,
    # when several are available. Stated alongside the rule above. The chase
    # already implements most of it -- it goes after the chosen speaker and
    # only falls back once that has failed `[PI3-FOUND-560]` -- but there is
    # a gap it does not cover: if another speaker's sink appears first, the
    # stream lands there, and stickiness would then hold it there for the
    # rest of the session against the listener's stated choice.
    #
    # So the preference gets exactly one chance, early, and never again:
    # only while the chosen speaker is the connected one, only inside the
    # first two minutes of uptime, and only once per boot. After that the
    # rule above governs and nothing moves working audio. A preference that
    # could fire at any time would be the mid-track switch stickiness exists
    # to prevent.
    PREFERRED="$RUNDIR/vaino-speaker.preferred"
    UP=$(cut -d. -f1 /proc/uptime)
    if [ -n "$ROUTED" ] && [ "$ROUTED" != "Dummy Output" ] &&
       sink_present "$ROUTED"; then
        if [ "$CONNECTED" = "${SPEAKER:-}" ] && [ ! -f "$PREFERRED" ] &&
           [ "${UP:-999}" -lt 120 ] && [ -n "$ALIAS" ] && sink_present "$ALIAS" &&
           [ "$(printf '%s' "$ROUTED" | tr a-z A-Z)" != "$(printf '%s' "$ALIAS" | tr a-z A-Z)" ]; then
            : > "$PREFERRED" 2>/dev/null
            curl -s -o /dev/null -X POST "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
            echo "'$ALIAS' is the chosen speaker and is available -- moved the stream to it from '$ROUTED' (once, at start)"
        fi
        # Otherwise: playing, on something real. Nothing to do, and nothing
        # said, because this is the steady state thirty seconds out of
        # thirty.
    elif [ -n "$ALIAS" ] && sink_present "$ALIAS"; then
        : > "$PREFERRED" 2>/dev/null
        curl -s -o /dev/null -X POST "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
        echo "the stream was on '${ROUTED:-nothing}', which is not a sink any more -- moved it to '$ALIAS'"
    elif [ -n "$ALIAS" ]; then
        echo "$CONNECTED has a link but no sink in PipeWire (profile off, or still negotiating) -- leaving the stream on '${ROUTED:-nothing}'"
    fi

    # **`[PI3-FOUND-600]` One speaker at a time, because two is silence.**
    #
    # Measured 2026-09-10, both speakers powered and connected. With the
    # Oontz holding a link alongside the playing Middleton: **0, 0, 0**
    # `ACL Data TX` packets across three five-second samples. Disconnect the
    # Oontz and the same measurement reads **374, 375, 374**. Not degraded
    # -- stopped.
    #
    # The link list says why. The second device had taken an **eSCO** link
    # as well as an ACL one, which is the HSP/HFP headset profile
    # `[PI3-FOUND-290]` -- and a synchronous link does not share the radio
    # with A2DP, it pre-empts it. The listener heard exactly this: "Oontz is
    # connected, but silent", then "now Middleton is audible again" the
    # moment it was disconnected.
    #
    # The listener's design settles what to do: other speakers are connected
    # only if the current one becomes unavailable `[PI3-AIM-070]`. So a
    # speaker holding a link while another is playing is not a guest to be
    # tolerated -- it is silence waiting to happen, and it goes.
    #
    # Guarded on the stream actually playing somewhere real, so this can
    # never fire during startup while sinks are still appearing, and it
    # never touches the device the audio is going to.
    if [ -n "$ROUTED" ] && [ "$ROUTED" != "Dummy Output" ] &&
       sink_present "$ROUTED"; then
        for other in $(bluetoothctl devices Connected 2>/dev/null | awk '{print $2}'); do
            oinfo=$(bluetoothctl info "$other" 2>/dev/null)
            echo "$oinfo" | grep -q 'UUID: Audio Sink' || continue
            oalias=$(echo "$oinfo" | sed -n 's/^[[:space:]]*Alias: //p')
            [ "$oalias" = "$ROUTED" ] && continue
            echo "'$oalias' is holding a link while '$ROUTED' is playing -- disconnecting it, because two connected speakers is silence"
            bluetoothctl disconnect "$other" >/dev/null 2>&1
        done
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

# Said once per tick, and only when there was nothing to lose by trying, so the
# log shows a speaker being waited for rather than an appliance repeating that
# it has failed.
[ "$BUDGET" -gt 0 ] && echo "$SPEAKER did not answer in ${BUDGET}s (held by another device, or asleep)"

# **`[PI3-FOUND-560]` A remembered speaker that is switched off should not
# leave the appliance silent when another one is sitting there paired.**
#
# Asked for directly by the listener, after a boot spent waiting on a speaker
# that had been powered down while a second, known speaker was awake in the
# same room `[PI3-FOUND-540]`. The appliance had everything it needed to make
# sound and made none.
#
# Deliberately narrow. It runs only when the chase has already failed and only
# when `BUDGET` is non-zero, which means nothing is currently audible -- so it
# can never interrupt playback to go hunting, which is the injury of
# `[PI3-AIM-060]`. It considers only devices that are **paired and advertise
# an Audio Sink**: things somebody deliberately introduced to this
# appliance, never something merely in range.
#
# **Not trusted, deliberately.** Trust was the obvious test and it is the
# wrong one. This appliance untrusts every speaker but the chosen one, so
# that only the chosen one may reconnect to *us* unasked `[PI3-FOUND-130]`,
# and the Middleton duly read `Trusted: no` the moment the listener
# switched to the Oontz. Requiring trust here would have skipped precisely
# the speaker this exists to fall back to. Trust governs an inbound
# connection; this one is outbound, and a bond is what says it is known.
#
# It does not rewrite the listener's choice. `speaker_address` still names the
# speaker they picked, so when that one comes back it is preferred again on
# the next boot. This is a stand-in for a missing speaker, not a new decision
# about which speaker this is.
if [ "$BUDGET" -gt 0 ]; then
    FALLBACK="${VAINO_FALLBACK_SECONDS:-20}"
    fstart=$(date +%s)
    for addr in $(bluetoothctl devices Paired 2>/dev/null |
                  sed -n 's/^Device \([0-9A-F:]*\) .*/\1/p'); do
        [ "$addr" = "$SPEAKER" ] && continue
        [ $(( $(date +%s) - fstart )) -lt "$FALLBACK" ] || break
        cand=$(bluetoothctl info "$addr" 2>/dev/null)
        echo "$cand" | grep -q 'Audio Sink' || continue
        echo "$cand" | grep -qE 'Paired: yes|Bonded: yes' || continue
        echo "$SPEAKER is absent; trying known speaker $addr"
        bluetoothctl connect "$addr" >/dev/null 2>&1
        sleep 3
        if bluetoothctl info "$addr" 2>/dev/null | grep -qi 'Connected: yes'; then
            curl -s -o /dev/null -X POST \
                "http://localhost:${VAINO_PORT:-5720}/command/reopen-output"
            echo "connected known speaker $addr instead, and asked the player to reopen"
            exit 0
        fi
    done
fi
exit 0

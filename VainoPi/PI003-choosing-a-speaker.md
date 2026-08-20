# PI003: Choosing a Speaker

**Design — Tier 2**

How a listener picks their Bluetooth speaker from the Vaino settings panel, and
why the obvious implementation of that is broken.

> **Related:** [PI001 partitions](PI001-image-and-partitions.md) ·
> [PI002 test image](PI002-test-image-setup.md)

---

## 1. What this has to survive

Every requirement below is the residue of a specific failure on real hardware,
recorded in [PI002 §6a](PI002-test-image-setup.md). None of them is
hypothetical, and the naive version of this feature reproduces all of them.

**`[PI3-WHY-010]` PipeWire always offers a `Dummy Output`.** When no sink is
present it invents one, and it is a perfectly good sink: a player attached to
it plays flawlessly, forever, into nothing, reporting itself healthy from every
angle. Two days of this problem were spent looking at Bluetooth because the
player insisted it was fine.

**`[PI3-WHY-020]` A stream sometimes follows a change of default sink, and
sometimes does not.** This is the awkward one, and the reason the fault took so
long to place.

Tested deliberately: with the speaker untrusted and disconnected, the player
starts and binds to `Dummy Output`. Reconnecting the speaker then relinked the
stream to MIDDLETON **by itself**, same node ids, no reopen. But the original
failure was precisely a stream sitting on `Dummy Output` while the speaker was
connected, staying there, and playing to nobody.

Both were observed on the same machine. So the relink is real but not
dependable -- plausibly a race between the dummy being removed and the sink
appearing -- and a design that relies on it works most of the time, which is
the worst frequency for a fault to have.

**Therefore "select a speaker" must reopen the output rather than change the
default and hope.** Not because the stream never follows, but because it
cannot be trusted to, and a cosmetic selection is silent.

**`[PI3-WHY-030]` A2DP dies when nothing feeds it.** A speaker with no audio
hangs up after a few seconds, and the listener hears a disconnection tone. This
makes every silent failure above *sound like* a Bluetooth fault, which is
exactly the wrong place to look.

**`[PI3-WHY-040]` `pair` and `trust` are separate, and only `trust` survives a
reboot.** Pairing without trusting yields a speaker that works beautifully until
the appliance restarts, then never reconnects.

**`[PI3-WHY-050]` A stale trusted-but-not-paired record blocks re-pairing
silently.** BlueZ keeps the entry, `pair` fails, and nothing explains why. The
fix is to `remove` the device first, which no listener will ever guess.

**`[PI3-WHY-060]` Pairing needs a registered agent.** Without
`agent NoInputNoOutput` the exchange never completes, and the failure is a
timeout rather than a message.

---

## 1a. What the listener should experience

**`[PI3-AIM-010]` Choose a speaker once. It is remembered. Pressing play
plays.** That is the whole requirement, and everything else in this document is
machinery in service of it. Written down because the parts can each work while
the experience still does not.

What that costs, beyond what is already built:

- **The choice must be stored by Vaino**, not merely inferred from BlueZ trust
  and PipeWire's current default. Those two happen to agree today; they are not
  a record of what the listener asked for `[PI3-AIM-020]`.
- **Partly built: `vaino-speaker`,** a timer that connects the speaker if it is
  absent and then tells the player to reopen. It closes the gap below for a
  single hard-coded speaker; storing the listener's actual choice is still to
  do `[PI3-AIM-020]`.
- **Play must be willing to go and get the speaker.** Today the player waits
  for a sink to appear and nothing asks BlueZ to connect the remembered one. If
  the speaker was off when the appliance booted, pressing play finds a dummy
  and correctly reports silence -- correct, and not what was asked for
  `[PI3-AIM-030]`.
- **Failure has to stay legible.** A speaker that is off, flat, or in pairing
  mode cannot be reached by any amount of retrying, and the panel should say
  which of those it looks like rather than spinning `[PI3-UI-010]`.

## 2. The shape of it

**Built, but not yet seen.** The panel is written and serves correctly; it has
not been rendered in a browser by anyone. Treat the layout as unverified.

**`[PI3-UI-010]` One list, showing what is true.** The settings panel shows
every known and discoverable speaker in a single list, each with a state that
is *observed* rather than assumed:

| State | Meaning | Offered action |
|---|---|---|
| `playing` | connected, and Vaino's stream is attached to it | Forget |
| `connected` | linked, but audio is going elsewhere | Use this one |
| `paired` | known, not currently linked | Connect |
| `found` | in range, never paired | Pair |
| `stale` | trusted but not paired `[PI3-WHY-050]` | Repair |

The distinction between `playing` and `connected` is the entire lesson of
`[PI3-WHY-010]` made visible. A user who sees "connected" and hears nothing has
been told something useful; a user who sees a checkmark has been lied to.

**`[PI3-UI-020]` Selecting a speaker performs the whole sequence.** Connect,
trust, make default, **and reopen Vaino's output** -- the last step being the
one `[PI3-WHY-020]` makes mandatory and the one nobody would think to expose.
A selection that stops short of it appears to work and is silent.

**`[PI3-UI-030]` Confirm or revert, as for the network settings
`[PI-SET-014]`.** Switching speakers can destroy the means of hearing whether
the switch worked. So the new speaker is adopted provisionally, the panel asks
"can you hear this?" over a test tone, and an unanswered prompt returns to the
previous sink after 30 seconds. The listener cannot strand themselves in
silence by choosing wrongly.

**`[PI3-UI-040]` Pairing is a mode, not a button.** The panel offers "put my
speaker in pairing mode, then press this", scans for the ~20 s a speaker takes
to appear, and registers an agent for the duration `[PI3-WHY-060]`. Discovery
stops afterwards, because a permanently scanning radio is a permanently
degraded one on a shared antenna.

---

## 3. What the player must provide

**`[PI3-API-010]` Reopen the output on demand. Built and proven on hardware.**
`POST /command/reopen-output` rebuilds the stream against the current default
sink, keeping the same ring. Verified by the output node id changing and by
`pw-top` showing the rebuilt node running at 1102 quantum, 44100, F32LE 2ch,
with playback continuing across the reopen. A reopen that lands on a device not
ready yet -- a speaker still completing its connection is the normal case --
hands itself to the retry loop rather than failing once.

**`[PI3-API-020]` Report the sink actually in use. Built.** `GET /audio/sink`
answers from PipeWire, because ALSA only ever tells the player `default`.
Queried on demand rather than polled: it costs a subprocess, and a player that
shelled out every state tick would spend more effort describing its output than
producing it. `known: false` distinguishes "the query could not run" from "it
ran and found nothing", since the remedies differ. Verified on hardware, both
ways round:

    speaker connected  {"sink":"MIDDLETON","dummy":false,"known":true}
    speaker removed    {"sink":"Dummy Output","dummy":true,"known":true}

**`[PI3-API-030]` Never settle for a dummy, and notice when one arrives.
Built.** Two paths, because the first one shipped covering only half the
problem. The loud failure -- a stream that breaks -- is caught by the error
callback. The quiet one is a speaker switched off during normal playback:
PipeWire moves the stream to the `Dummy Output` and **reports no error at
all**. Nothing in the player is wrong at that moment. The callback runs, the
ring drains, the clock advances, and nobody can hear a thing. A guard that
fires only on reopen therefore almost never fires. So the engine also confirms,
every twenty seconds while playing, that the audio still reaches something
real.

**`[PI3-API-030]` (original) Never settle for a dummy.** `vaino-wait-sink` guards
boot; the engine now guards every reopen and every recovery. Opening
successfully says nothing about whether anyone can hear it -- the dummy accepts
audio perfectly -- so a reopen that lands there is marked failed and the retry
loop keeps looking. The player will not report itself recovered into silence.

**`[PI3-API-050]` Routes fronting the helper. Built.** `GET /audio/speakers`
lists; `POST /audio/speakers/:verb/:address` acts. The verb is an enum rather
than a string passed through, so an unknown one cannot reach the helper, and
the address is validated again in the player -- the helper's check is the one
that must not be bypassed, this one makes a malformed address a 400 with an
explanation rather than a non-zero exit nobody reads.

`use` reopens the output **as part of the same request** `[PI3-UI-020]`. Doing
it server-side rather than in the browser means no caller can forget the step
that makes the choice audible.

Every reply carries `audible`, which is the question a listener actually has.
It is `null` rather than `false` when the stream is not linked yet: "we could
not tell" and "it is not working" want different responses, and collapsing them
is how a fault stays hidden. The reply also waits for the reopen to land before
reporting -- reading immediately returned `sink:null` with `dummy:false`, which
reads as healthy and was merely early.

Choosing a speaker from silence, in one call:

    before  {"sink":"Dummy Output","dummy":true,"known":true}
    POST    /audio/speakers/use/20:64:DE:CF:F3:AD
    reply   {"audible":true,"state":"connected","reopened":true,
             "output":{"sink":"MIDDLETON","dummy":false,"known":true}}

**`[PI3-API-040]` Surface the recovery count `[REQ-VIS-140]`.** A link that
drops and recovers repeatedly is a range or battery problem, and a number
climbing in the diagnostics is how anyone would know. Silent recovery is right
for one dropout and wrong for fifty.

---

## 4. Privilege

**Built: `vaino-btctl`.** A closed set of verbs -- `list`, `scan`, `pair`,
`repair`, `use`, `forget`, `status` -- each taking at most a device address,
checked against an anchored MAC pattern *before* it reaches BlueZ. Nothing is
interpolated into a shell. It emits JSON so the caller parses a shape rather
than prose, and it is reached through a sudoers rule naming that one binary
rather than by granting the player broader rights.

Three verbs encode failures rather than operations. `pair` registers an agent
and paces its steps, because piping them as one block runs them faster than
bluetoothd establishes the connection `[PI3-WHY-060]`. `repair` removes the
device first, because a stale trusted-but-not-paired record makes every attempt
fail with no explanation `[PI3-WHY-050]`. `use` trusts as well as connects,
because trust is what survives a reboot `[PI3-WHY-040]`.

The sudoers file is validated with `visudo -c` and **removed if malformed** --
a bad one can lock the machine out of sudo entirely, and a setup script that
bricks its own escape route is worse than one that does nothing.

Verified on hardware. Injection and malformed addresses are refused
(`{"ok":false,"error":"not a device address"}`), unknown verbs are refused, and
the whole `[PI3-UI-020]` sequence runs from a silent start:

    before   {"sink":"Dummy Output","dummy":true,"known":true}
    use      {"ok":true,"state":"connected","sink_node":"46"}
    reopen   HTTP 204
    after    {"sink":"MIDDLETON","dummy":false,"known":true}

with `pw-top` confirming the node running at 1102 quantum, 44100, F32LE 2ch.

---

## 4a. Open: a reopened stream dies, a fresh one does not

**`[PI3-OPEN-010]`** Reproduced repeatedly on 2026-08-16. A stream created by
`recover()` against an already-running player loses the speaker **about
twenty-two seconds later**, every time. A stream created fresh at startup, with
the speaker connected first, holds indefinitely -- two separate runs of ninety
seconds and two minutes, connected on every sample, no dummy detections, no
errors.

Ruled out by measurement, not argument: CPU (4% during the failures), the
`wpctl` call on the engine thread (moved off it, and the failures continue),
battery (60-70%), pause (link held 8/8 across forty seconds paused), radio
coexistence (the control arm is clean), and range (80 cm, and the adapter sees
fourteen other devices).

So something about *rebuilding* the stream leaves it fragile in a way opening
it once does not -- the old handle not fully released, or the device reopened
while PipeWire is still settling. This matters because reopening is the
mechanism the whole speaker panel rests on `[PI3-WHY-020]`.

**STILL OPEN. Improved, not fixed** -- and the way I got that wrong is worth
recording. The settle moved the failure from about twenty-two seconds to about
two and a half minutes. A two-minute test reported eight samples out of eight
and I called it verified; the drop came twenty seconds after the window closed.
Every clean result of the day shares the flaw: **no test ran longer than the
failure it was measuring.** Future runs go ten minutes at least, with the
underrun counter watched throughout, since that is what gave the real signal
both times it mattered.

What follows is what the settle did achieve.

**The immediate cause was the obvious one nobody had tested.** `recover()`
released the device and reopened it in the same breath. Giving PipeWire 700 ms
to finish tearing the old stream down before opening the new one makes a
reopened stream hold as well as a fresh one: **two minutes, connected on every
sample, from the worst case** -- player already running, no speaker, then
connect and select.

Startup was stable only because it never reopens anything in flight. The gap
was the whole difference, and the connect-then-restart workaround is no longer
needed.

**`[PI3-OPEN-020]` Feed silence while paused. Built.** McRhythm did this, and
the same reasoning applies: A2DP tears down when nothing feeds it `[PI3-WHY-030]`, so a
paused Vaino loses its speaker after a few minutes and resuming needs a
reconnect. Pausing should stop the *music*, not the stream. This also likely
reduces how often the fragile reopen path is needed at all, which makes it
worth doing regardless. Pausing now silences the callback rather than stopping
the device: the ring is left untouched so resuming is still instant
`[REQ-AUD-142]`, and the silence is not counted as an underrun, because
inflating the one diagnostic that matters most to hide an intended quiet would
be its own small lie.

## 5. Deliberately not now

**`[PI3-FOUND-010]` Interference is the cause.** Measured 2026-08-16 with the
dark arm at ten minutes, four times the observed failure interval:

    connected while dark: 200/200
    flow while dark:      2 rows
    errors while dark:    (none)

With Wi-Fi down the link is perfect. With Wi-Fi up it had been dropping every
two and a half minutes all evening. One antenna serves both radios on a Pi Zero
2 W, and every drop today with ssh idle says it is **association and beaconing**
that does it, not traffic -- which rules out the cheapest possible fix.

Still owed: **the control arm at the same ten minutes.** A dark run compared
against a remembered impression of shorter runs is the exact mistake this
document has already recorded twice. Until `KEEP_WIFI=1 SECONDS_DOWN=600` fails
as expected, this is a strong result rather than a settled one.

**Withdrawn. `[PI3-FOUND-040]` explains this result without interference.**
The dark arm scored 200/200 because Wi-Fi was down, which meant **nobody could
ssh in** -- and an ssh login or logout is what tore the link down. The
experiment removed the cause along with the radio, and credited the radio.

Every drop "with ssh idle" was idle only in the sense that no bytes were
moving; sessions were opening and closing throughout to take the very samples
that recorded the drops. The measurement was the fault. Nothing here supports
buying a dongle, and the numbers above measure the method rather than the
hardware.

Remedies, best first. **A USB Wi-Fi dongle on 5 GHz**, since the Pi Zero 2 W is
2.4 GHz only: it removes the conflict and keeps the interface reachable. **A
USB Bluetooth dongle**, giving the radios separate antennas. **Toggling Wi-Fi
off during playback** `[PI3-FOUND-020]` costs no hardware but costs
reachability, and an appliance unreachable while playing cannot be debugged in
the state that matters.

**`[PI3-FOUND-030]` WirePlumber was demolishing the link, on its own schedule.**
Found 2026-08-16. `bluetoothd` shows the A2DP endpoints being unregistered and
re-registered in sweeps of nineteen, and the media transport rebuilt each time
-- `sep1/fd7`, `fd8`, `fd9`, `fd10` in the space of a minute. A2DP cannot
survive its endpoints being withdrawn, so every sweep is a dropped speaker, and
at rest the sweeps came about every two and a half minutes: **the same period
as the drops, arriving with the radio idle.**

Each sweep pairs, to the second, with

    Failed to get percentage from UPower: org.freedesktop.DBus.Error.NameHasNoOwner

`upower` was not installed. The package alone is not the fix: its unit ships
disabled and static, so D-Bus activation still finds no owner and the error
continues unchanged. It must be `enable --now`.

Two things this cost, worth naming. The single WirePlumber process reports
`NRestarts=0` throughout, so anything watching systemd for restarts sees a
healthy service -- an earlier session recorded "0 restarts" and ruled the
audio stack out on exactly that evidence. The fault is visible only in
`bluetoothd`'s log, from a process that never died. And the drops it causes are
indistinguishable by ear from interference, which is how an entire evening was
spent measuring a radio.

**`[PI3-FOUND-040]` Every ssh login and logout dropped the speaker.** Found
2026-08-16, and the cause of the drops.

WirePlumber gates the whole BlueZ monitor on logind seat state:

    -- /usr/share/wireplumber/scripts/monitors/bluez.lua:285
    logind_plugin = Plugin.find("logind")
    logind_plugin:connect("state-changed", function(p, s) startStopMonitor(s) end)

with `["with-logind"] = true` in `50-bluez-config.lua`, whose own comment
explains the purpose: arbitrating which of several logged-in users owns
Bluetooth audio, "particularly useful if you are using GDM". On an appliance
with one user and no display manager it arbitrates nothing and costs
everything -- stopping the monitor unregisters all nineteen A2DP endpoints,
and A2DP does not survive that.

Measured, with the trigger under our own control rather than waited for:

    before:  20 of 22 endpoint sweeps within 2s of "Removed session"
             (the other 4 all predate the upower fix [PI3-FOUND-030])
    after:   13 session teardowns, 0 unregistrations, link held

`linger` does not help. It keeps the graph alive across logouts; it does not
stop WirePlumber reacting to them. The remedy is one property, in
`/etc/wireplumber/bluetooth.lua.d/51-vaino-no-logind.lua`, applied by
`setup-vainopi.sh`.

**What this cost, and why.** Two evenings went to measuring a radio because
checking the link is what broke it: every `ssh pi@vainopi 'bluetoothctl info'`
opened a session, and closing it killed the speaker seconds later. The drops
therefore tracked *our sampling cadence*, which is why they looked periodic,
and why they always seemed to arrive just after a clean window closed. It is
the third instance in two days of a diagnostic causing the fault it measures
`[GDE-FBD-110]`, and by far the most expensive.

**`[PI3-FOUND-020]` Speaker-side transport controls.** The Middleton's rocker
offers five gestures, which under AVRCP arrive as ordinary key events from a
uinput device BlueZ creates -- play/pause, previous/next, volume. Worth building
regardless of the interference question: an appliance whose only control
surface is a web page is a poor appliance, and it is the thing that makes any
Wi-Fi-off-while-playing scheme usable at all.

**`[PI3-ROCKER-010]` The rocker's assignment. Measured 2026-08-16.** Five
gestures were pressed in order; three arrived as key events and two produced
nothing at all:

| Gesture | Code | Key | Function |
|---|---|---|---|
| Centre press | 200 | `KEY_PLAYCD` | Toggle play/pause |
| Right | 163 | `KEY_NEXTSONG` | Skip, identical to the existing control |
| Left | 165 | `KEY_PREVIOUSSONG` | Reserved for a "like", unassigned for now |
| Volume up | -- | none | Absolute volume, over the media transport |
| Volume down | -- | none | Absolute volume, over the media transport |

The volume silence is the useful half of that result. Volume is not a key event
here: AVRCP carries it as absolute volume on the media transport, so it reaches
the sink without passing through this device at all. The intention to leave
volume alone therefore costs nothing and cannot be got wrong -- there is no
event to swallow by accident, and exactly three signals exist to spend.

Left is deliberately left dead rather than given a placeholder. A control that
does something surprising is worse than one that does nothing, and reserving it
in writing is what stops it being spent on something lesser later.

The uinput device appears only once audio is flowing, not when the speaker
connects. Anything reading it must wait for it, and must wait again when it
goes: it is removed on every disconnect `[PI3-ROCKER-030]`.

**`[PI3-ROCKER-020]` Play and pause switch the radios. Withdrawn 2026-08-16.**
Centre press is a plain play/pause toggle and touches no radio.

The scheme was designed against `[PI3-FOUND-010]`, on the belief that the
periodic drops were interference and that the only cure available without new
hardware was to stop transmitting. `[PI3-FOUND-030]` then found a software
mechanism destroying the same link on a similar period, which means the share
of the drops attributable to the radio is not currently known. Paying for a
cure with an appliance that cannot be reached while it plays is a poor trade
against an unmeasured disease.

It is recorded rather than deleted because the three safeties it named are the
right ones if it is ever revived: a failed connection must raise Wi-Fi again,
or the appliance is silent *and* unreachable -- the worst state it can occupy,
reached by the ordinary path of someone switching the speaker off; the HTTP
response must be sent before the interface drops, or pressing play in a browser
reads as a crash; and the panel must say what play will do, because a control
that disconnects you is fine when expected and alarming when not.

**`[PI3-LED-010]` The ACT LED tracks the Wi-Fi radio. Built.** The Pi Zero 2 W
exposes one controllable LED, and the kernel does the entire job: `rfkill1` is
`phy0`, so binding that trigger makes the light follow **the radio itself
rather than our intention about it**. Nothing polls, nothing can drift, and it
stays correct when something other than Vaino switches the radio.

    echo rfkill1 > /sys/class/leds/ACT/trigger

That property is worth more than the convenience. It was built when
`[PI3-ROCKER-020]` meant to take Wi-Fi down during playback, where an
unreachable appliance otherwise just looks broken; that scheme is withdrawn,
and the LED is kept anyway. It costs nothing, it tells the truth about the
radio without anything having to remember to update it, and a status flag we
set ourselves could be wrong in exactly the situation where being wrong costs
most. This one is read from the hardware.

`/sys` does not survive a reboot, so it is a unit rather than a one-off write,
and it hands the LED back on stop. Verified across a real reboot: the trigger
reads `[rfkill1]` and the unit is active.

The cost is the card-activity indication, which shares the one LED. On an
appliance that is a fair trade -- radio state is something a listener can act
on, card access is not -- but it is a real loss when diagnosing a card.

**Polarity still wants one observation.** The trigger should light when the
radio is unblocked, so LED on means Wi-Fi up. Confirming it needs only the next
dark-arm run: watch the LED at the moment the radio drops.

**`[PI3-NOT-010]` (superseded) Interference was not being designed around.** The dark arm of
`radio-silence-test.sh` exists to answer that question and has not been run,
because with Wi-Fi up the link now measures 40/40 across two minutes with audio
flowing. Should unexplained connection problems appear, that test is the tool
to reach for. Building mitigations for a problem not yet observed would be
designing against a guess.

**`[PI3-NOT-020]` Multiple simultaneous sinks.** One speaker, chosen. Whole-house
audio is a different product.

---

## 6. A blocked radio looks exactly like a broken button

**`[PI3-FOUND-020]` `Connect` did nothing because there was no radio.**
*(Diagnosed 2026-08-20.)* The speaker was in pairing mode, its light flashing,
and clicking **Connect** in the settings screen had no visible effect. Nothing
about the pairing was wrong: MIDDLETON was `Paired`, `Bonded`, `Trusted`,
unblocked, advertising A2DP Audio Sink, and the BCM43436 firmware had loaded
cleanly at boot.

The controller was **soft-blocked at the rfkill layer** — `rfkill0: hci0
type=bluetooth soft=1 hard=0` — so `bluetoothctl power on` answered
`org.bluez.Error.Failed` and every connection attempt returned
`br-connection-adapter-not-powered`. Clearing `/sys/class/rfkill/rfkill0/soft`
made the whole chain work first time: powered → connected → PipeWire routed the
player's stream to MIDDLETON → the supervisor logged **`output recovered on
default`** by itself, which is `[SPEC-APS-060]` doing exactly its job.

**`[PI3-FOUND-025]` The setup script could not have fixed it, and was written
not to say so.** The line was `rfkill unblock bluetooth 2>/dev/null || true`,
and **`rfkill` is not installed on this image** — so it found no command,
discarded the error, returned true, and reported success. A step that cannot
fail cannot report failure either. It now writes sysfs, which is always present,
and reports `CHANGED` or `FAILED`.

**`[PI3-FOUND-030]` The state persists, in both directions.** `systemd-rfkill`
saves the block under `/var/lib/systemd/rfkill/` and restores it at boot, which
is how one block outlived every reboot since. It cuts the other way now that the
file reads `0`: the fix survives a reboot, and — because the file is already
written rather than written at shutdown — an unclean power loss too, which is
the failure mode an appliance actually has `[SPEC-DF-094]`.

> **What to check first, next time.** `Powered: no` in `bluetoothctl show` means
> look at rfkill before looking at pairing. The pairing was never the problem
> and would have absorbed an evening.

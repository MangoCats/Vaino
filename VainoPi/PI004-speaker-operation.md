# PI004: Operating the Speaker Link

**Appliance Record — what running the Bluetooth speaker actually taught**

Split from [PI003](PI003-choosing-a-speaker.md) on 2026-08-20, which had reached
506 lines against `[GOV-DOC-010]`'s 300-line limit. The seam was already there:
PI003 says what the speaker link must do, and its section 5 -- titled
*"Deliberately not now"* -- had quietly filled with measured findings and things
marked **Built**, which is the opposite of not now.

Everything here is dated and measured. Nothing here is a plan.

> **Related:** [PI003](PI003-choosing-a-speaker.md) for the design and the
> player's contract · [PI001](PI001-image-and-partitions.md) · `[SPEC-APS-060]`
> for the audio path supervisor these findings kept meeting

---

## 0. Where this stands, and where the rest of it lives

This document is the **operating record**: what the speaker link does, how it
fails, and what was built so it stops failing that way. The dated
investigations behind it were split out on 2026-09-08, when this file reached
1,112 lines against `[GOV-DOC-010]`'s limit — [PI009](PI009-the-silence-of-2026-09-08.md),
[PI010](PI010-startup-time-and-stutter.md) and
[PI011](PI011-two-speakers-and-placement.md). Read those as experiment
records: several reach answers a later section overturns, each says so where
it stands, and a paragraph is evidence of what was believed on a date.

Current understanding, for a reader who needs only that:

| Symptom | Cause | Where |
|---|---|---|
| Periodic stuttering for ~3 min after boot | **The Middleton, cold-starting.** Not the room and not this appliance: an Oontz cold-booted alongside the Pi in the same room ran at 100.2% of its own baseline and sounded clean, while the Middleton runs at 87% and stutters, 3 of 3. Unfixed here because it is not this appliance's to fix | `[PI3-FOUND-580]`, PI011 §11 |
| Connected, progress bar advancing, no sound | **The speaker, not the appliance.** Confirmed by counting packets on the air while every appliance layer read healthy. Fixed by disconnect/reconnect | `[PI3-FOUND-480]` |
| Player wedged after a power cut, 23 restarts | Hot SQLite journal, unrecoverable through a read-only attach | `[PI3-FOUND-120]`, PI009 |
| Speaker never reconnects after a power cycle | The speaker powers the Pi, so the Pi is always late — and it had lost `Trusted` | `[PI3-FOUND-090]`/`-130`, PI009 |
| ~19 s of startup that was not work | Contention with `mpd`, which now yields | `[PI3-FOUND-210]`, PI010 §2 |
| Second speaker silent at full volume | It connected as an HSP headset, not A2DP | `[PI3-FOUND-290]`, PI011 §2 |
| Chosen speaker replaced by another | The keeper adopted any connected device | `[PI3-FOUND-310]`, PI011 §4 |

**Three theories cost real time and are kept rather than deleted**, each being
one somebody else would reach for: that the 15-second stutter period matched
`BUFFER_FRAMES` (the ring is topped up every 10 ms and never drains on a
cycle); that the boot gate's wait should be shortened (`[PI3-FOUND-260]`); and
that setting an idle I/O class made anything polite (`[PI3-FOUND-250]` — no
scheduler in use honoured it).

**If it is connected and silent, count the packets before touching
anything** `[PI3-FOUND-480]`. Five seconds answers whether the appliance is
transmitting, which is the one thing the sink, the stream, the player and
BlueZ can all report wrongly at once:

    sudo timeout 5 btmon | grep -c 'ACL Data TX'

About 370 means the air is carrying a healthy stream and the fault is in the
speaker -- reconnect it, and if that fails the speaker needs its own attention.
Near zero means the fault is on this side, and the sink and routing checks
above become the ones that matter.

**If stutters return, the first question is how it was power-cycled**
`[PI3-FOUND-350]`. Switching the speaker off cuts the Pi's supply, so both
cold-boot together and the boot stutters for about three minutes; cycling the
Pi alone, leaving the speaker powered, has been clean every time. **And it is
the Middleton, not the mode in general**: an Oontz cold-booted alongside the Pi
in the same room, which is the same condition, ran at 100.2% of its own
baseline and sounded clean `[PI3-FOUND-580]`. So the practical answers are to
use the Oontz, to leave the Middleton powered when restarting the Pi, or to
re-establish the link once the speaker has been awake a while. **It is
currently unmitigated**, deliberately. A once-per-boot redial did stop it, 4 of 4, but was
removed on 2026-09-10 because it cost about half a minute of silence on every
boot including the ones that would have been clean `[PI3-FOUND-450]`.

**One instrument now detects it: `vaino-hci-capture`** `[PI3-FOUND-430]`. A
stutter train reads as a throughput deficit at the HCI layer, and a matched
pair captured on 2026-09-10 settles the mode question in packets: Mode A ran
at 87.1% of the clean rate with a dip every ~15 s, Mode B at 100.1% with none
`[PI3-FOUND-470]`. Severity tracks the ear in both directions, and the link is
either good or bad from its first packets -- it does not degrade. Everything *above* that
layer is still blind: signal level, load and disk I/O all measured *better* on
boots that stuttered, and the player's own underrun counter does not correlate
at all `[PI3-FOUND-420]`. Budget for that before forming a theory -- five have
been published here and withdrawn, every one of them argued from ear-points
alone, which is the thing the capture exists to stop.

Two older answers are **confounded** by that variable and no longer claimed
as causes `[PI3-FOUND-410]`: placement `[PI3-FOUND-320]` (its clean runs were
all the harmless power-cycle mode) and the graph quantum `[PI3-FOUND-330]`
(its evidence came from boots that would have stuttered regardless). Sitting
the Pi on the speaker still costs 11-16 dB of signal and is worth avoiding on
its own terms.

Diagnostic tools, and how to switch them on: PI010 §3.

---

## 1. Interference, and what shares the antenna

**`[PI3-FOUND-010]` Interference is the cause.** Measured 2026-08-16 with the
dark arm at ten minutes, four times the observed failure interval:

    connected while dark: 200/200
    flow while dark:      2 rows
    errors while dark:    (none)

With Wi-Fi down the link is perfect. With Wi-Fi up it had been dropping every
two and a half minutes all evening. One antenna serves both radios on a Pi Zero
2 W, and every drop today with ssh idle says it is **association and beaconing**
that does it, not traffic -- which rules out the cheapest possible fix.

A control arm at the same ten minutes was owed and never run — moot once the
result below withdrew the finding entirely.

**Withdrawn. `[PI3-FOUND-040]` explains this result without interference.**
The dark arm scored 200/200 because Wi-Fi was down, which meant **nobody could
ssh in** -- and an ssh login or logout is what tore the link down. The
experiment removed the cause along with the radio, and credited the radio.

Every drop "with ssh idle" was idle only in the sense that no bytes were
moving; sessions were opening and closing throughout to take the very samples
that recorded the drops. The measurement was the fault. Nothing here supports
buying a dongle, and the numbers above measure the method rather than the
hardware.

Remedies were proposed, best first: a **USB Wi-Fi dongle on 5 GHz** (the Zero
2 W is 2.4 GHz only), a **USB Bluetooth dongle** giving the radios separate
antennas, or **toggling Wi-Fi off during playback** `[PI3-FOUND-020]` — which
costs no hardware but costs reachability, and an appliance unreachable while
playing cannot be debugged in the state that matters.

> **Partly vindicated, 2026-09-08, by a different mechanism** — the placement
> finding `[PI3-FOUND-320]`, in
> [PI011 §5](PI011-two-speakers-and-placement.md). The withdrawal above
> is still correct: those numbers
> measured the method, not the hardware, and Wi-Fi *association* was never
> shown to drop the link. But the single shared antenna this section worried
> about does matter, and measurably — resting the appliance on the speaker
> cost 11-16 dB of signal and raised Wi-Fi retries roughly thirtyfold, which
> was audible as stuttering for most of a day. The cheapest remedy turned out
> to be none of the three listed here: move the box eighteen inches.

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

**`[PI3-ROCKER-030]` The uinput device appears with audio, not with the
connection.** Anything reading it must wait for it, and must wait again when it
goes: it is removed on every disconnect.

**`[PI3-ROCKER-020]` Play and pause switch the radios. Withdrawn 2026-08-16.**
Centre press is a plain play/pause toggle and touches no radio.

The scheme was designed against `[PI3-FOUND-010]`, believing the drops were
interference and the only cure without new hardware was to stop transmitting.
`[PI3-FOUND-030]` then found software destroying the same link on a similar
period, so the share attributable to the radio is unknown — a poor trade
against an unmeasured disease.

Recorded rather than deleted for the three safeties it named, which are the
right ones if it is revived: a failed connection must raise Wi-Fi again, or
the appliance is silent *and* unreachable, reached by the ordinary path of
someone switching the speaker off; the HTTP response must be sent before the
interface drops, or pressing play reads as a crash; and the panel must say
what play will do, because a control that disconnects you is fine when
expected and alarming when not.

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


---

## 2. A blocked radio looks exactly like a broken button

**`[PI3-FOUND-050]` `Connect` did nothing because there was no radio.**
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

**`[PI3-FOUND-055]` The setup script could not have fixed it, and was written
not to say so.** The line was `rfkill unblock bluetooth 2>/dev/null || true`,
and **`rfkill` is not installed on this image** — so it found no command,
discarded the error, returned true, and reported success. A step that cannot
fail cannot report failure either. It now writes sysfs, which is always present,
and reports `CHANGED` or `FAILED`.

**`[PI3-FOUND-060]` The state persists, in both directions.** `systemd-rfkill`
saves the block under `/var/lib/systemd/rfkill/` and restores it at boot, which
is how one block outlived every reboot since. It cuts the other way now that the
file reads `0`: the fix survives a reboot, and — because the file is already
written rather than written at shutdown — an unclean power loss too, which is
the failure mode an appliance actually has `[SPEC-DF-094]`.

> **What to check first, next time.** `Powered: no` in `bluetoothctl show` means
> look at rfkill before looking at pairing. The pairing was never the problem
> and would have absorbed an evening.

---

## 3. What was built so it cannot look like that again

**`[PI3-RF-010]` The settings panel shows every radio and whether it is on.**
`GET /audio/radios` fronts a new `radios` verb on `vaino-btctl`, which reads
`/sys/class/rfkill/*` and reports name, kind, soft block, hard block, and one
computed fact — whether blocking it would cut the route the answer is
travelling over. Measured on the appliance:

    {"radios":[{"name":"hci0","kind":"bluetooth","soft":0,"hard":0,"carries_route":false},
               {"name":"phy0","kind":"wlan",     "soft":0,"hard":0,"carries_route":true}]}

The panel reads *"A radio is switched off. Nothing on it can connect until it
is on."* when any is blocked. That one line is the whole of what `[PI3-FOUND-050]`
cost an evening for.

**`[PI3-RF-020]` Bluetooth can be switched from the panel; the route's radio
cannot.** `POST /audio/radio/<kind>/<on|off>`. Kind and state are checked
against closed lists in the player *and* again in the helper, which is the side
that holds privilege.

**`[PI3-RF-030]` The refusal is a property of the route, not a ban on Wi-Fi.**
The helper refuses to soft-block whichever radio carries the default route. On
this appliance that is always `wlan0` — it is the only interface, there is no
ethernet, and switching it off from a web page would sever the connection to
the page holding the switch with no way back but physical access. **On a machine
with wired ethernet the same code permits it**, because there the cost is
nothing. Encoding the condition rather than the platform is what makes it true
in both places.

rfkill is joined to the route through sysfs rather than by assuming `phy0` is
`wlan0`: `/sys/class/net/<if>/phy80211` points back at the wireless phy, and a
Bluetooth radio carries no route at all.

The rule lives in the helper alone. The player does not repeat it, so a second
caller — a person at a terminal — cannot be told something different.

> **Verified on hardware 2026-08-20.** `radios` lists both correctly;
> `radio wlan off` is refused with *"that radio carries the default route"*,
> through the player as well as directly; `radio ethernet off` and
> `radio bluetooth maybe` are refused as a bad kind and a bad state; a
> hardware-blocked radio is refused separately, because it cannot be overridden
> from software at all. The Bluetooth toggle was exercised at the helper —
> off, on, and the speaker reconnected — rather than a second time through the
> route, which would have interrupted music being listened to for a branch
> already proven.

---

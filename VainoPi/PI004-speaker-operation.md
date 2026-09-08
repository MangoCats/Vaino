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

**`[PI3-ROCKER-030]` The uinput device appears with audio, not with the
connection.** Anything reading it must wait for it, and must wait again when it
goes: it is removed on every disconnect.

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

## 5. The silence of 2026-09-08, and what it was not

**`[PI3-FOUND-065]` The reported fault was "vainopi cannot connect to
Middleton." Bluetooth was working the entire time.** Established by
measurement rather than by asking the stack whether it was happy: BlueZ's
own `MediaTransport1.State` read `active`, `hcitool con` showed an
authenticated encrypted ACL link, the PipeWire sink and the player's stream
were both in state `running` with their links `active`, and — the decisive
one — ten seconds captured off the sink's monitor with `pw-record` contained
real, non-silent music.

That last technique is the useful residue of this episode. Every layer in
this stack can be asked "are you all right?" and every layer will say yes;
the monitor capture asks instead "what did you actually send?", and it is
the only question whose answer cannot be a stale property:

```sh
pw-record --target <sink-id> -P '{ stream.capture.sink=true }' \
          --rate 44100 --channels 2 --format s16 /tmp/cap.wav
# then measure: rms/peak in dBFS. Music sits near -18 rms / -2 peak.
```

**`[PI3-FOUND-066]` Retired: the "split-brain" theory.** A previous pass read
`api.bluez5.connection = "disconnected"` and `bluez5.profile = "off"` out of
`wpctl inspect` on the bluez5 *device* node, saw them contradict BlueZ's own
`Connected: yes`, and concluded the sink was a correctly-named phantom with
no live profile behind it. **Those two properties are set when WirePlumber
creates the device object and are not maintained afterwards.** They contradict
live state routinely, on a perfectly working link, and they are not evidence
of anything. Recorded here because the theory was coherent, fitted the
symptom, and was wrong — and because the next person to run `wpctl inspect`
on a silent speaker will see exactly the same two lines.

**`[PI3-FOUND-090]` The speaker powers the Pi, so they can only power up
together — and the Pi always loses the race.** vainopi is fed from the
Middleton's own USB port. Switching the speaker off cuts power to the
appliance; switching it on starts a boot. The speaker is connectable within
about two seconds and the Pi needs some thirty to reach a working Bluetooth
stack, so any already-awake phone in the room takes the speaker first, every
time. A speaker that is carrying another device's connection stops answering
pages altogether: the Pi's attempt returns `br-connection-page-timeout`,
which is indistinguishable from the speaker being switched off, and the old
one-attempt-per-tick keeper duly concluded exactly that. **This is the normal
state at every power-up on this appliance, not an exceptional one**, which is
what makes a single attempt the wrong policy — see `[PI3-AIM-060]` for what
replaced it, and why the replacement is still timid whenever audio is
actually playing.

**`[PI3-FOUND-110]` `vaino-wait-sink` never held, on any machine, in any
state.** Its first condition was

```sh
wpctl status | sed -n '/Sinks:/,/^ *├─ [A-Z]/p' | grep -qv 'Dummy Output'
```

read as "a sink that is not the dummy exists". `grep -v` succeeds when *any*
line fails to match, and the block it is handed opens with the `├─ Sinks:`
header, so the condition is true unconditionally. The gate reduced to its
third clause alone, which passes whenever no sink is marked default.

The consequence needed a machine with no hardware sink to become visible, and
vainopi is one: `snd_bcm2835.enable_hdmi=0` and `enable_headphones=0` are both
on its kernel command line, so until Bluetooth arrives it genuinely has
nowhere audible to send audio. Measured on the 15:37 boot — *"real sink
present after 3s"*, thirty seconds before the speaker existed, releasing the
player to open its output onto a dummy and stay there. Every layer above
reported success, which is `[PI3-WHY-010]` arriving by a new road.

Rewritten to parse only the numbered sink rows, so no header or box-drawing
line can be counted as a sink, and to give the *chosen* speaker first refusal
before accepting any other real sink — a grace period rather than a
preference, so an appliance with no speaker still boots promptly.

**`[PI3-FOUND-100]` The `scan` verb could not see a speaker.**
`pair_sequence` has set `transport bredr` since it was written, for a reason
recorded in its own comment: the default discovery filter returns LE only,
and a speaker is a classic BR/EDR device. `scan` never did. So the one verb
whose entire purpose is finding a speaker was structurally incapable of
returning one — confirmed live, a scan returning nine devices, eight of them
nameless LE random addresses, and not the Middleton sitting a metre away.
It reads exactly like the speaker being switched off.

Fixed, and nameless devices are now dropped from the listing: bluetoothctl
renders a device it has no name for as its own address with dashes, a real
speaker always advertises a name, and a dozen identical-looking hex rows are
how the one row that matters gets missed. The panel went from nine junk
entries to the two real speakers.

**`[PI3-FOUND-070]` `Connected: yes` is not proof of audio, and now nothing
claims it is.** The device-level ACL link survives perfectly well while the
A2DP media profile underneath it is idle or absent. `MediaTransport1.State`
is the only honest answer — `active` means a stream endpoint is acquired and
carrying audio — and the helper's reports now carry it, alongside a `reach`
field separating "never answered, so held by another device or asleep" from
"answered and refused". Both used to render as plain failure, which sent the
listener to the wrong problem.

**`[PI3-FOUND-120]` Every power-down on this appliance is a power cut, and
one of them stopped the player from ever starting again.** Because the
speaker supplies the Pi `[PI3-FOUND-090]`, switching the speaker off yanks
the supply from a running system with open databases. There is no shutdown
and no opportunity for one, so it is a supported event rather than misuse.

What it leaves is a hot rollback journal beside the database — measured,
`/var/vaino/listener.db-journal`, 8720 bytes. SQLite's contract is that the
next connection rolls it back, but rolling back is a *write*, and Vaino
attaches deliberately `mode=ro` so the player cannot corrupt the library
`[IMPL-DBSPLIT-025]`. A read-only connection cannot perform the recovery it
is required to perform. It therefore fails, permanently:

```
$ sqlite3 vaino.db "ATTACH 'file:/var/vaino/listener.db?mode=ro' AS lsn; ..."
Error: stepping, attempt to write a readonly database (8)
```

`Restart=always` then turns that into a crash loop — observed at 23 restarts
and still climbing, with no web interface, no audio, and nothing in the
failure naming a journal file. Note the misdirection in the log: the fatal
line reads `attach library /srv/library/library.db`, and `library.db` was
provably fine the whole time; it was `listener.db` that could not be
recovered. Chasing the name in the error message leads to the wrong file.

Recovery took one read-write open, which is all SQLite needs to notice the
journal and roll it back. `vaino-db-recover` now does exactly that for all
three databases as the first `ExecStartPre`, so the appliance heals itself
instead of needing someone with an ssh key. It costs 30 ms and prints
nothing when there is nothing to recover. `PRAGMA user_version` rather than
`integrity_check`: the rollback happens on first read by a read-write
connection, so the cheapest read suffices, and a real check of a 1.1 GB
library on a Pi Zero 2W would add minutes to every boot to answer a question
nobody asked.

> **Verified 2026-09-08** on the real failure, not a simulation: the hot
> journal was present, the read-only attach failed with the error above, a
> single read-write open removed the journal, and the same attach then
> returned its row count. The player started, resumed, and reached the
> speaker. Attempts to manufacture a synthetic hot journal afterwards were
> abandoned — SQLite optimises the no-op transaction away — which is worth
> knowing before anyone tries to write a regression test for this.

**`[PI3-FOUND-130]` Trust is what lets the speaker reconnect to *us*, and a
hand recovery throws it away silently.** BlueZ auto-authorises an incoming
service connection only from a trusted device; untrusted, it asks an agent,
and this appliance registers one only for the duration of a `pair`
`[PI3-WHY-060]`. At every other moment there is nobody to ask, so the request
is refused outright.

Measured on the boot after the Middleton was hard-reset and re-paired by
hand. The speaker had an ACL link to the Pi by 20.8 s, then tried three times
to bring up A2DP and was refused each time:

```
[20.836] vaino-speaker: ...is connected but the stream was on 'nothing'
[28.952] bluetoothd: Authentication attempt without agent
[28.953] bluetoothd: a2dp.c:auth_cb() Access denied: org.bluez.Error.Rejected
[37.798] ... Rejected      [46.693] ... Rejected
[55.719] vaino-speaker: connected 20:64:DE:CF:F3:AD after 5s
```

Its persisted record read `Trusted=false` beside a perfectly good link key.
Thirty-five seconds were spent refusing the speaker's own offers to connect,
until this script's outbound connect finally won.

The delay is the smaller half. **The speaker reaching out to us is the one
path that does not have to win the power-up race `[PI3-FOUND-090]`** — it
costs no paging and no shared-radio time, and it begins the moment the
speaker is awake, which on this appliance is also the moment the Pi is
plugged in. Losing trust disables that path entirely and leaves only the
race, which is the path the Pi loses by design.

`use` has always trusted `[PI3-WHY-040]`, but a listener recovering by
forgetting the device and reconnecting through any other path lands on a
bonded, untrusted speaker, and nothing ever put it back. `vaino-speaker` now
asserts it every tick: checked before it is set, so a healthy appliance
spends nothing, and loud when it actually repaired something. Verified by
untrusting the speaker and watching the next tick restore `Trusted=true` to
disk.

**`[PI3-FOUND-140]` The chase collided with the connection it was hurrying.**
`[PI3-AIM-060]`'s persistent chase was written against a speaker that does
not answer at all, and it read "absent" from the D-Bus `Connected` property.
That property goes true when a *profile* connects, so it reads false through
the whole of A2DP negotiation — and the chase therefore paged again every
two seconds while a negotiation was already in flight. Each page collided
with it:

```
[50.6] avdtp_connect_cb() connect to ...: Operation already in progress (114)
[52.7] [54.8] [56.9] [59.0] [59.0]  ... the same, six more times
```

Eight collisions in one boot, and A2DP that had completed at 75 s on the
previous boot did not finish until 88 s. The measurable effect of making the
keeper more determined was to make audio arrive thirteen seconds later.

`hcitool con` is the honest question: it asks the controller whether a
baseband link exists, rather than asking BlueZ whether a profile has
finished. A link present means a connection is up or coming up, and the only
useful thing to do is keep out of its way. The chase now skips paging
entirely while a link exists, and waits 3 s rather than 2 s between pages so
that two unanswered pages cannot overlap each other's timeouts.

**`[PI3-FOUND-150]` The boot gate was serialising a wait the library load
would have covered for free.** The player does not touch its output when it
starts — it backs up listener state, then builds the library index, 8330
passages off a 1.1 GB database on a Pi Zero 2W, about twenty seconds — and
only then opens a device. Those twenty seconds are twenty seconds in which
Bluetooth may finish settling at no cost, so a second spent waiting *before*
starting the player is a second added to the total rather than hidden inside
it. Measured on a power cycle: the gate waited its full 45 s, gave up, and
only then let the twenty-second load begin. Ninety-five seconds to audio, of
which about forty-five were spent deliberately doing nothing.

The deadline is now 15 s. It still catches what it was written for — a warm
restart, where the sink is already present and the answer is instant — and
still gives the chosen speaker first refusal. A sink that turns up late is no
longer the dead end it was when this gate was written: the path supervisor
notices a dummy and reopens `[SPEC-APS-060]`, and `vaino-speaker` notices the
routing disagreeing with the connected device `[PI3-AIM-050]`. Two mechanisms
that did not exist then now cover the case this was holding the entire boot
still to prevent.

## 6. Startup stutter, and the observer that caused some of it

**`[PI3-FOUND-170]` Diagnosing this over ssh perturbs the thing being
measured.** The Pi Zero 2W puts Wi-Fi and Bluetooth on one chip behind one
antenna, so a `journalctl` dump competes directly with the A2DP stream it is
being used to investigate. On the first boot with a persistent journal to
read, the ssh login landed at 60.4 s and the reported stutters at roughly
80 s, 97 s and 101 s — during the diagnosis, not merely near it. Some earlier
"stuttery audio" reports coincide with active diagnosis in the same way.

This is not the whole explanation — stutter was also reported on cycles with
nobody connected — but it is enough to invalidate any measurement taken over
ssh while listening. `vaino-startup-sample` exists for that reason: a boot-time
service that reads `/proc` once a second into a local file, with no network,
no subprocesses in its loop, `Nice=19` and idle I/O. It can run while the
listener simply listens, and be read afterwards with the appliance quiet.

**`[PI3-FOUND-180]` No audio thread on this appliance has real-time priority,
and the usual ways of granting it do not work here.** Measured: `pipewire`'s
`data-loop.0` — the thread that encodes SBC and feeds A2DP — runs
`SCHED_OTHER` at priority 0 carrying `SCHED_RESET_ON_FORK`, which is the
fingerprint of a request made and refused. `rtkit-daemon` confirms it, once a
minute: *"Supervising 0 threads of 0 processes of 0 users."* The cause is a
hard limit: `/proc/<pipewire>/limits` reads `Max realtime priority 0 0`.

Three fixes were tried and **none of them worked**, which is the part worth
recording:

- Adding the account to the `pipewire` group, the usual advice. Debian's
  `@pipewire - rtprio 95` is applied by `pam_limits`, and **`/etc/pam.d/systemd-user`
  does not exist on this image**, so no PAM limits reach a systemd *user*
  session — which is where PipeWire runs.
- `LimitRTPRIO=95` as a drop-in on `user@.service`. `systemctl show` duly
  reported 95 for the instance, and the manager process still came up with a
  hard limit of 0.
- `DefaultLimitRTPRIO=95` in `/etc/systemd/system.conf`, with `daemon-reexec`
  and across a full reboot. The manager default read 95; PID 1 itself and the
  user manager both still read 0.

All three were reverted rather than left in place, because configuration that
looks like it grants real time and does not is worse than none. Whether RT is
even the right target is unproven: `pw-top` shows the audio thread using 2–4%
of its quantum with zero xruns in steady state, so it has considerable headroom
and would need a >20 ms scheduling delay to glitch. Establish that such delays
actually occur before spending more on this.

**The upower remedy recorded above never survived a reboot, and this is
why.** `systemctl enable --now upower` worked — for that session.
`upower.service` ships `WantedBy=graphical.target`, and this appliance boots
to `multi-user.target` with no display, so enabling it creates a want that is
never reached. Every boot since has come up with `upower` *enabled* and
*inactive*, and WirePlumber has logged
`Failed to get percentage from UPower: org.freedesktop.DBus.Error.NameHasNoOwner`
each time. The verb that actually holds on a headless appliance is
`systemctl add-wants multi-user.target upower.service`, which is now in place.

Worth noting what this did **not** turn out to explain: the error appears
exactly once per boot here, at about 13 s, not on the repeating ~2.5 minute
sweep that `[PI3-FOUND-030]` originally described, and the startup stutter
being investigated happens at 63-80 s. Fixed because it is a known hazard and
the fix is one line, not because it was the cause.

**`[PI3-FOUND-190]` What the startup stutter is not.** With a sampler running
unattended (`vaino-startup-sample`, no ssh connected), the window the listener
reported as stuttering — 63 s to 80 s — shows: swap counters static, 216 MB
available, the player using about 5% of one core, and zero xruns. The load
average sits near 1.4 but is a lagging average decaying from the library index
build at 27-50 s, not live work. So it is not memory, not swap, not CPU
starvation, and not the player missing its ring.

What remains is the radio. Wi-Fi and Bluetooth share one chip and one antenna
on a Pi Zero 2W, DHCP completed at 56.9 s and the NetworkManager dispatcher ran
57-67 s — overlapping the start of the stutter exactly — and no amount of CPU
headroom prevents an antenna being used by something else. Since that cannot be
removed, the lever is margin: the graph was running a 1024-frame quantum, about
21 ms, so any interruption longer than that glitches. `default.clock.min-quantum`
is now 2048, which doubles the tolerance to about 46 ms, at a latency cost that
is meaningless for music with no synchronised display. **Unverified against a
cold boot at the time of writing** — the change was made and confirmed live
(sink quantum 2048, zero xruns), but the boot it is meant to improve has not
been run yet.

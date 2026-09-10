# PI011: Two Speakers, and Where the Stutters Were

**Appliance Record — the answer, and a wrong answer on the way to it**

Split from [PI004](PI004-speaker-operation.md) on 2026-09-08. Concludes the
investigation begun in [PI009](PI009-the-silence-of-2026-09-08.md) and
continued in [PI010](PI010-startup-time-and-stutter.md).

**Read section 11 first.** This document reaches three different answers in
sequence and the last one supersedes the rest: the stuttering is decided by
**how the appliance was power-cycled**, and redialling the Bluetooth link once
after boot stops it. The placement finding of `[PI3-FOUND-320]` and the
quantum finding of `[PI3-FOUND-330]`, which sections 5 and 6 present as
causes, are both **confounded** by that variable and are no longer claimed.
The earlier sections are kept because each was the conclusion the evidence
genuinely supported at the time, and because the way each failed is the useful
part.

> **Related:** [PI004](PI004-speaker-operation.md) §0 for current understanding ·
> [PI003](PI003-choosing-a-speaker.md) for the speaker-choice design

---

## 1. Politeness that was not enforced, and a deadline cut on stale reasoning

**`[PI3-FOUND-250]` `IOSchedulingClass=idle` did nothing, because the
scheduler in use has no idea what it means.** The Director rebuild was made
idle-class so it would yield the card to the decoder `[PI3-FOUND-220]`. It
did not yield. Sampled at one-second resolution with nobody connected, its
block-device reads run:

```
mono   read_kb        vaino_jiffies
35.73   14516          28
44.16  136468         502
54.29  270824         997
```

**256 MB pulled off the SD card in about eighteen seconds — roughly 15 MB/s
sustained — while using half a core**, straight through the start of
playback, by a thread that was nominally at idle priority.

The cause is the elevator: `/sys/block/mmcblk0/queue/scheduler` was
`mq-deadline`, which implements deadlines and nothing else. **I/O priority
classes are implemented by BFQ and by essentially nothing else in a modern
kernel**, so `ioprio_set(IDLE)` on the rebuild thread was a request nobody
was listening to. Switching the card to `bfq` is what makes an I/O-priority
class mean anything at all here; without it, every such change in this
document is decoration.

Set through `tmpfiles.d` alongside the 512 KB readahead, so it survives a
reboot on an appliance that gets power-cut rather than shut down.

**What `bfq` did not do is fix the stutters, and the measurement says so.**
On the boot after the switch the rebuild still read ~255 MB at ~15 MB/s,
essentially unchanged — correctly, because BFQ's idle class defers only to
*competing* I/O, and the decoder was reading from the page cache rather than
the card, so there was nothing to defer to. The stutters had another cause
entirely `[PI3-FOUND-320]`. The change is kept because the reasoning holds —
an idle-class request that no scheduler honours is a bug whatever else is
true — but it is a correctness fix, not the remedy for anything audible.

**`[PI3-FOUND-260]` The boot gate's deadline was cut on reasoning that had
already expired.** `[PI3-FOUND-150]` shortened it from 45 s to 15 s because
the player took twenty seconds to start and the wait could hide inside that.
Then `[PI3-FOUND-210]` established that the twenty seconds were never the
player's own work — they were contention — and once `mpd` was made to yield,
the same work measured 120 ms. The premise was gone; the change outlived it.

What that cost, measured on the next boot: the speaker took 63 s to answer,
having been taken by another device `[PI3-FOUND-090]`; the gate gave up at
15 s; the player opened onto a dummy and played into it for twenty-seven
seconds until `vaino-speaker` reconnected and asked for a reopen. That boot
recorded **26.45 s of underrun, against 2.79 s** on a boot where the speaker
was present before the player started.

Restored to 60 s, with 45 s of first refusal for the chosen speaker. It exits
the instant that sink appears — 0.14 s on a healthy boot — so the higher
number costs nothing when things are well, and only stops the player
committing to a dummy when they are not. Starting early buys nothing here and
costs a reopen; starting late costs only silence that was silent anyway.

## 2. Two bugs that only appear with a second speaker

**`[PI3-FOUND-270]` "Use this one" used whichever speaker was listed first.**
The `use` verb set the default sink by taking the first non-dummy entry out of
`wpctl status`, which is right only while exactly one speaker is connected.
Connect a second and it points the default at whoever sorts first. Measured,
with both connected and the listener asking for the OontZ:

```
$ vaino-btctl use 08:EB:ED:26:14:12          # the OontZ
{"ok":true,"state":"connected","sink_node":"48"}   # node 48 is the MIDDLETON
```

It answered `ok:true` for doing the opposite of what it was asked, and the web
handler — which reopens the player's output on the strength of that `ok`
`[REQ-VIS-260]` — dutifully moved the audio to the speaker the listener had
just asked to leave. From the settings page it looked like the button did
nothing at all.

Now matched by the alias BlueZ holds for the requested address, which is what
WirePlumber names the node after — the same join `vaino-speaker` already uses
`[PI3-AIM-050]` — and it waits up to ten seconds for that sink, because
WirePlumber creates it a moment after BlueZ reports the connection. No match
is reported as `none` rather than papered over with somebody else's sink.

**`[PI3-FOUND-280]` The scripts were reading a database the player stopped
writing to.** `[IMPL-DBSPLIT-025]` moved everything the listener chooses into
`/var/vaino/listener.db`, leaving the catalog in `/srv/library/`. Both
`vaino-speaker` and `vaino-wait-sink` kept reading the pre-split
`/srv/library/vaino.db`, which still exists and still holds a stale
`speaker_address` row. **The two held the same address, so nothing looked
wrong for weeks.**

They stopped agreeing the instant a second speaker was chosen:

```
legacy /srv/library/vaino.db: 20:64:DE:CF:F3:AD     (MIDDLETON)
live   /var/vaino/listener.db: 08:EB:ED:26:14:12    (OontZ)
```

The keeper read MIDDLETON, saw the stream on the OontZ, concluded the routing
disagreed with the speaker "on record", and asked the player to reopen —
fighting the listener's own choice every thirty seconds on the authority of a
file nothing had written to in weeks. `vaino-wait-sink` had the same fault and
would have spent its boot waiting for the speaker they used to have.

Both now prefer `listener.db` when it exists and fall back to the pre-split
path when it does not, so one script serves a split appliance and an unsplit
one without being told which it is on. Verified: the keeper falls silent on a
tick after the speaker is changed, and the gate reports *"OontZ_Angle 3 U412
present after 0s"*.

**`[PI3-FOUND-290]` A second speaker does not fail to connect; it connects as
a headset.** The adapter carries one A2DP transport at a time. Ask for a
second speaker while the first is still connected and BlueZ does not refuse —
it negotiates HSP/HFP instead, which is mono over SCO and worthless for
music. Measured, having switched to the OontZ with the Middleton still
connected:

```
51. output_MONO  >  OontZ_Angle 3 U412:playback_MONO   [active]
55. OontZ_Angle 3 U412                                  ← a capture Source
$ busctl ... MediaTransport1 State        → no transport for the OontZ at all
```

The giveaway is the microphone: an A2DP sink has no source. Volume full,
nothing audible, and every layer reporting success — `[PI3-WHY-010]` wearing
another hat, and the same shape of fault as the very first symptom in this
document.

Freeing the transport is not sufficient on its own. Disconnecting the
Middleton left the OontZ exactly where it was, still mono with no transport,
because nothing renegotiates a live connection. It took a disconnect and
reconnect of the OontZ itself, after which:

```
dev_08_EB_ED_26_14_12/sep1/fd1  →  "active"
55. output_FL > OontZ:playback_FL   56. output_FR > OontZ:playback_FR
```

`use` now disconnects any *other* connected audio device first — which is
what the listener asked for anyway; nobody presses "use this one" meaning
"and also keep the last" — and reconnects the requested device only when it
is connected without a transport, so a speaker already carrying A2DP is left
strictly alone rather than having the audio interrupted by the verb meant to
deliver it.

## 3. A wrong answer, by substitution

**`[PI3-FOUND-300]` The periodic stutters were the Middleton, not the
appliance.** Every Pi-side instrument had been saying so for a while — the
player's underrun counter stopped climbing while the listener still heard
stutters, PipeWire reported zero xruns, the A2DP transport stayed `active`,
and Wi-Fi settled to three scans a boot — but "everything I can measure looks
fine" is not a diagnosis. Swapping the speaker is.

The OontZ was made the chosen speaker and the appliance power-cycled: audio
at **24.5 s**, and ninety seconds of listening with no audible stutter at all.
That boot also exercised most of what had been built — `vaino-db-recover`
rolled back a hot journal at 17.5 s, `vaino-wait-sink` found the right sink in
2 s, `session open` took 182 ms.

The decisive number is the comparison, not the absolute: **2.68 s of underrun
sounding clean**, against **1.64 s on a Middleton boot that stuttered
throughout**. More missing samples, less audible trouble. What the listener
heard on the Middleton was therefore not Vaino's underruns — those are a
couple of seconds around the Director rebuild, on any speaker, and inaudible.

The appliance's own faults were real and are fixed: 88-95 s to audio became
24.5 s, a power cut that wedged the player into 23 restarts now heals itself,
and a speaker that could not reconnect after a power cycle now does.

> **The conclusion drawn here was wrong; see `[PI3-FOUND-320]`.** This section
> ended "what remained belongs to the speaker, and no amount of work on this
> side will reach it." It did not belong to the speaker. The OontZ was clean
> because it was not being used as a shelf: the Pi was resting on the
> Middleton, and its shared Wi-Fi/Bluetooth antenna was being detuned and
> desensitised by the chassis and the speaker's own radio inches away. Two
> variables were changed — the speaker *and* the geometry — while one was
> believed to have been. Substitution answered the question it was asked; the
> inference drawn from it did not survive the next observation.

> **Method worth keeping.** Four causes were proposed and measured away —
> memory, swap, CPU starvation, SD contention — before substitution answered
> it in one power cycle. The instrument that mattered was `vaino-underruns`,
> not because its number was high but because it went *static while the
> symptom continued*, which is what put the fault downstream of the player. A
> counter that stops moving can be as informative as one that climbs.

## 4. Two speakers, and who gets to decide

**`[PI3-FOUND-310]` A choice the listener made was being overwritten by
whatever turned up.** Measured: the Middleton was selected in the settings
panel and the appliance power-cycled with the speaker left on.

```
30.3s  vaino-speaker: connected 20:64:DE:CF:F3:AD after 10s, asked reopen
50.7s  vaino-speaker: adopted 08:EB:ED:26:14:12 as the speaker (was 20:64:DE:CF:F3:AD)
71.2s  resuming playback            -- on the OontZ
```

The keeper connected the chosen speaker correctly. The OontZ — still trusted,
so BlueZ reaches for it unprompted — connected behind it, took the one A2DP
transport `[PI3-FOUND-290]`, and the Middleton dropped. The listener heard a
connect tone and a disconnect tone seconds apart. Then `[PI3-AIM-040]`'s
adopt-what-is-connected rule promoted the interloper and **destroyed the
record of the choice**, and the boot finished playing through the speaker
they had just navigated away from.

Adoption was right when it was written: a stale address was being paged every
thirty seconds and stalling the speaker that was actually playing. But
`[PI3-AIM-060]` fixed that injury at its source — nothing is paged while
audio reaches a real sink — which left adoption doing only harm. It now
happens **only when no speaker has been chosen at all**; a recorded choice
stands until the listener changes it, and an uninvited device is reported
rather than promoted.

**The other half is trust.** Trust means "reconnect to this without being
asked", and on an adapter that carries one A2DP transport only one speaker
should hold that. Choosing a speaker now withdraws the standing invitation
from the others: they stay **paired**, so `use` brings any of them back in
seconds, they simply stop letting themselves in `[PI3-WHY-040]`.

That withdrawal runs **last** in the verb, and the reason is a race worth
recording. Done first, it was silently undone: `vaino-speaker` trusts
whichever address is *stored*, the store still names the old speaker until
the caller records the new one — which happens only after the verb returns
ok — so a keeper tick landing mid-connect re-trusted the speaker just
withdrawn, and it was still auto-connecting on the next boot. Observed once,
diagnosed from the persisted `Trusted=` flag disagreeing with what the verb
had just done. Moved to the end, the window is the microseconds between that
line and the caller's write.

> **Verified.** The OontZ was re-trusted by hand and `use` run against the
> Middleton: the OontZ came back `Trusted=false` on disk, the Middleton
> `Trusted=true`, connected, `transport: "active"`, stream on MIDDLETON. A
> keeper tick with the OontZ deliberately connected alongside left
> `speaker_address` untouched.

## 5. The stutters were where the appliance was sitting

**`[PI3-FOUND-320]` The Pi was on top of the speaker.** Moving it about
eighteen inches away — same cable, same speaker, same everything else —
ended the periodic stuttering. Two power cycles, 240+ seconds of listening,
none heard at all. The radio numbers say why:

| | Pi resting on the speaker | Pi 18" away |
|---|---|---|
| Wi-Fi signal | -61 to -66 dBm | **-50 dBm** |
| Wi-Fi retry discards | 69, later 267 | **8** |
| audible stutter | every ~15 s | **none** |

Eleven to sixteen decibels, and retries down roughly thirty-fold. The Zero 2W
carries one PCB antenna shared by Wi-Fi and Bluetooth `[PI3-FOUND-010]`, and
resting it on the speaker put that antenna against a magnet, a driver, a
metal grille, and the speaker's own Bluetooth radio a couple of inches away.
Two transmitters in each other's near field desensitise each other; the
chassis detunes what is left.

**This is why every instrument said the machine was healthy.** Loss on the
air is invisible to all of them: the player's ring never missed a deadline
(`underrun_samples` static while the listener heard stutters), PipeWire
reported zero xruns, `MediaTransport1` stayed `active`, and the A2DP packets
were being handed to a controller that was duly transmitting them into a
degraded link. Nothing in software can see a packet that was sent and not
heard.

It also explains the substitution result in section 3 honestly. The OontZ
was clean not because the Middleton's electronics are at fault, but because
the OontZ was not being used as a shelf. **The conclusion in section 3 —
"what remains belongs to the speaker" — is wrong, and this supersedes it.**
What remained belonged to the geometry.

> **A caution for whoever measures this next.** The appliance is powered from
> the speaker's own USB port `[PI3-FOUND-090]`, by a cable short enough that
> setting the Pi on the speaker is the obvious tidy thing to do. It is the
> single worst place to put it. If stutters return, check where the box is
> before touching anything in this document.

**Underrun counts do not predict audibility, and should not be read as if
they do.** This boot recorded 11.58 s of `underrun_samples` and sounded
perfect; an earlier Middleton boot recorded 1.64 s and stuttered throughout.
The counter is honest about the ring running dry -- which happens during a
reopen, and around the Director rebuild -- but a dry ring during a routing
change is inaudible, while a healthy ring feeding a degraded radio is not.
Use it to tell "the player is starving" from "the player is fine", which is
what it settled `[PI3-FOUND-200]`, and not as a measure of quality.

## 6. Link quality is the predictor, and it is a number

**`[PI3-FOUND-340]` Moving the box was never the fix; improving the link
was.** Stuttering returned on 2026-09-09 with the appliance still eighteen
inches clear of the speaker. Everything else was eliminated by measurement:
the binary differed from the clean runs by a doc comment alone, the SBC
negotiation was byte-identical (`ay 4 17 21 2 53`), the transport was
`active`, AVRCP and sink volumes were at unity, the underrun counter was
**static**, and audio captured off the sink was continuous.

What differed was the radio, and it differs by a number worth writing down:

| | signal | stutters |
|---|---|---|
| Appliance resting on the speaker | -61 to -66 dBm | yes |
| Eighteen inches clear, clean runs | **-50 dBm** | none |
| Eighteen inches clear, stuttering | **-59 to -66 dBm** | yes |

Three points, and the symptom tracks the link rather than the distance. On
the stuttering boot the appliance had also associated with the *other* access
point of the pair `[PI3-FOUND-310]`'s roam-flap describes — same SSID,
different radio, worse result — which is one way the same eighteen inches can
buy a good link on Monday and a poor one on Tuesday.

**So `[PI3-FOUND-320]` is refined, not withdrawn.** Sitting on the speaker
reliably ruins the link and moving off it reliably helps, but neither is the
mechanism: what matters is how much of the one shared antenna Bluetooth gets,
and Wi-Fi conditions move that on their own.

**Retry counts were offered as half of this and are withdrawn.** They looked
discriminating — 8 on a clean run against 55 on a stuttering one — but
`/proc/net/wireless` reports retry discards as a **counter cumulative since
boot**, and those two numbers were read at different uptimes. Compared
properly as a rate, a stuttering boot on 2026-09-09 ran at 0.03 retries/s,
which is nothing. The figure never discriminated; it only looked as though it
did because the clock underneath it was moving.

> **Check it before listening.** `awk 'NR==3{print $4}' /proc/net/wireless`
> gives the signal level, which is instantaneous and therefore comparable
> between boots. **Around -50 dBm has sounded clean here; -59 to -66 has
> stuttered**, on four boots. A working threshold from few points, not a
> specification — but it is one number, it needs no audio, and it beats moving
> the box and hoping.

## 7. The power-cycle mode decides it, and nothing on the Pi explains why

**`[PI3-FOUND-350]` Two modes, measured back to back, 2026-09-09.** The
listener noticed that switching the speaker off — which cuts the Pi's supply,
since it is powered from the speaker's USB `[PI3-FOUND-090]` — correlated with
stuttering restarts, where power-cycling the Pi alone did not. Run as a
controlled pair with the appliance eighteen inches clear in both:

- **Mode A**, speaker switched off and on: connect at 35 s, then stutters at
  40, 55, 72, 87, 94, 110 and 119 s, continuing past 180 s before settling.
- **Mode B**, speaker left powered, Pi alone cycled: reconnect at 33 s, **no
  stuttering at all through 120 s+**.

The observation is real. **The explanation is not on this machine.** Sampled
unattended over an identical 35–120 s window:

| | Mode A (stuttered) | Mode B (clean) |
|---|---|---|
| Wi-Fi signal | **-58.6 dBm** | **-71.9 dBm** |
| Load average | 0.75 | **1.33** |
| Disk read | 1837 kB/s | **2333 kB/s** |
| Player CPU | 15.7 jiffies/s | 14.1 jiffies/s |

Every one of them points the wrong way. The clean boot had **thirteen
decibels worse signal**, higher load and more I/O than the boot that
stuttered.

**`[PI3-FOUND-340]` is therefore withdrawn.** Signal level looked like the
predictor across four boots and is not one; it was the second metric offered
here to fail, after retry counts. Both failed the same way — a number that
correlated across a handful of boots, presented as though it explained
something, and falsified by the first experiment designed to test it rather
than to confirm it.

What is left is the variable the Pi cannot see: in mode A **the speaker is
cold-booting too**, its own radio and audio stack initialising, while in mode
B it is settled and only the link is re-made. Nothing in `/proc`, `pw-top` or
the player's counters observes that.

> **Next step, and what it needs.** The closest available proxy is
> `bluetoothd`'s own view — transport state changes, AVDTP negotiation, and
> whether anything else attaches to the speaker while it boots. Capturing that
> across both modes needs the persistent journal turned back on
> (PI010 §3), because the volatile default destroyed mode A's log the moment
> mode B booted. Turn it on, repeat the pair, and compare the Bluetooth logs
> rather than the Pi's own health.

## 8. What the Bluetooth layer says about the two modes

**`[PI3-FOUND-360]` One measurable difference, and it is a connection
collision.** Both modes were captured with the persistent journal on. Filtered
of endpoint registration noise, the whole of what `bluetoothd` has to say:

```
Mode A (stuttered)                         Mode B (clean)
21.01 a2dp-sink connect failed:            20.87 a2dp-sink connect failed:
      Protocol not available                     Protocol not available
27.42 avdtp_connect_cb: Operation          (nothing)
      already in progress (114)
30.91 sep1/fd0: fd(29) ready               29.39 sep1/fd0: fd(29) ready
(nothing further, ever)                    (nothing further, ever)
```

Counted across each entire boot: **one AVDTP collision in mode A, none in
mode B**; one transport creation and one early profile failure in both.

Two things follow, and the second is the awkward one.

**The collision is real and mode-specific.** In mode A the speaker is
cold-booting, so both ends reach for each other at once and one attempt lands
on another already in flight — the same `Operation already in progress (114)`
signature that an over-eager chase produced in `[PI3-FOUND-140]`, arriving
this time from the speaker rather than from us.

**But it happens once, at 27 s, before audio starts at 32 s — and then
`bluetoothd` says nothing for the remaining three minutes** while the listener
hears stutters at 40, 55, 72, 87, 94, 110 and 119 s. So the collision does not
*cause* each stutter. The most it can be is the moment a link gets negotiated
badly and stays that way, which is a hypothesis this document has not yet
earned the right to state as fact.

**`[PI3-FOUND-370]` The trusted auto-connect fires before the endpoints
exist, in both modes.** `a2dp-sink profile connect failed ... Protocol not
available` at ~21 s is BlueZ acting on `Trusted=true` `[PI3-FOUND-130]` and
reaching for the speaker about two seconds *before* WirePlumber finishes
registering its A2DP endpoints at ~23 s. It is harmless here — something
reconnects later either way — but it means the trust-driven fast path, the one
that exists to avoid the power-up race, currently never succeeds on its first
attempt. Worth fixing on its own terms, and unrelated to the stutter, since
both modes do it identically.

**Baseline for the next comparison**, recorded from the clean mode B link:

| | |
|---|---|
| Configuration | `ay 4 17 21 2 53` — SBC, 48 kHz, joint stereo, 16 blocks, 8 subbands, loudness, bitpool 2–53 |
| State / Volume | `active` / 127 |
| ACL role | **CENTRAL** (`<` outgoing — the Pi initiated) |
| Underruns | 2.60 s |

> **The test that would settle it.** Read those same four values immediately
> after a mode A boot. If the negotiated configuration or the initiating role
> differs, the collision has a lasting consequence and there is something to
> fix. If they are identical, then a link that measures the same in every
> respect sounds different, and the difference is inside the speaker where
> nothing here can reach it.

## 9. The fix: redial the link outbound, once

**`[PI3-FOUND-380]` A link the speaker opened is worse than one we opened, and
it measures identically.** Reading the mode A link immediately after a boot,
against the mode B baseline of section 8:

| | Mode B (clean) | Mode A (stuttering) |
|---|---|---|
| Configuration | `ay 4 17 21 2 53` | `ay 4 17 21 2 53` |
| State / Codec / Volume | active / SBC / 127 | active / SBC / 127 |
| **ACL direction** | **`<` outgoing** | **`>` incoming** |

Every negotiated parameter is byte-identical. The one thing that differs is
who opened the link: in mode A the cold-booting speaker reaches out to its
last device; in mode B this appliance reaches out to a speaker already awake.

**Tested as a same-boot intervention, which is as controlled as this gets.**
On a boot stuttering on schedule, the link was disconnected and reconnected
outbound — nothing else touched. The direction flipped `>` to `<`, the
configuration did not change, **and the stuttering stopped**: none heard, and
the underrun counter flat across the following sixty seconds.

So `vaino-speaker` redialled once per boot when it found an inbound link,
leaving an outbound one alone. **That redial has since been removed entirely**
`[PI3-FOUND-450]`: the gap it cost turned out to be half a minute rather than
a few seconds, and it was paid on every boot. The paragraphs below are the
record of what it did while it was there.

> **What this is not.** Why an inbound link should sound worse is *not*
> established. The one visible correlate is the AVDTP collision of
> `[PI3-FOUND-360]`, which an outbound redial resolves by making the
> negotiation single-sided — but that is a plausible story, not a measurement,
> and two plausible stories have already been withdrawn from this document.
> What is established is narrower and worth stating exactly: **redialling
> outbound stops it, once, reproducibly on the boot it was tried.** One trial.

## 10. Both explanations refuted, and the experiment that remains

**`[PI3-FOUND-390]` The next mode A boot came up outbound, collision-free, and
stuttered anyway.** That is both hypotheses of section 9 gone in one
observation:

| | Mode B (clean) | Mode A, boot 1 | Mode A, boot 2 |
|---|---|---|---|
| ACL direction | `<` outbound | `>` inbound | **`<` outbound** |
| AVDTP collisions | 0 | 1 | **0** |
| Configuration | `ay 4 17 21 2 53` | same | same |
| Stuttered | no | yes | **yes** |

The keeper had connected it outbound itself at 36 s, so the redial built in
section 9 — conditioned on finding an inbound link — never even fired.

**And the trial that appeared to prove the redial was confounded.** It ran at
57 s, and a second attempt at 180 s; mode A settles on its own by about 180 s.
Neither trial separated "the redial fixed it" from "the speaker was going to
settle anyway". The second could not have: it was run at the settling point.

So nothing at the Bluetooth layer now distinguishes a stuttering boot from a
clean one. Configuration, codec, transport state, volume, direction and
collision count are all identical. The difference remains the one thing this
machine cannot observe: whether the speaker was cold-booted.

**What is left is an experiment, and it is written as one.** The redial now
fires once per boot unconditionally at around 50 s, well before the settling
point, because that is the only arrangement that can tell the two apart: if
stuttering stops at ~50 s rather than running to ~180 s, the redial does
something. If it runs to 180 s regardless, it does not, and the honest answer
becomes the listener's own workaround — power-cycle the Pi, not the speaker.

> **This costs a mode B boot a gap it did not previously pay**, and that is a
> real regression if the redial turns out to do nothing. It is deliberate, and
> it should be reverted the moment the experiment answers either way.

## 11. Where this ended, and what is still owed

**`[PI3-FOUND-400]` The redial works, three times out of three, and the cure
is uncomfortable.** *(Superseded: a fourth trial agreed, `[PI3-FOUND-440]`
measured what it did, and `[PI3-FOUND-450]` removed it for exactly the
discomfort this section names.)* On every mode A power cycle since it was made
unconditional, the boot comes up stuttering, the keeper redials at around 50 s,
and the audio is smooth afterwards. But the listener's summary is the honest
measure of it:

> *"the audio is smooth after a redial-reconnect, but that takes up to 95
> seconds post power to achieve and involves significant distracting
> start/stop/restart audio in the process."*

So it is a mitigation, not a fix. Audio arrives at ~37 s, stutters, is cut off
for several seconds around 50 s, returns, and only settles by ~95 s. Polling
BlueZ instead of sleeping on a fixed clock cut the interruption from about
eleven seconds to eight; the rest is inherent to tearing a link down and
rebuilding it.

**`[PI3-FOUND-410]` Two earlier findings are confounded by the power-cycle
mode and are withdrawn as causes.** Neither was tested against it, because it
was not known to be a variable at the time:

- **`[PI3-FOUND-320]`, placement.** The clean runs that established it were
  mode B — the listener's own note records the speaker staying powered, since
  it only announced the disconnect when the Pi came back. Mode B is clean at
  any placement. Sitting on the speaker does measurably cost 11–16 dB of
  signal, which is worth knowing, but signal level was itself refuted as a
  predictor of the stutter `[PI3-FOUND-390]`.
- **`[PI3-FOUND-330]`, the graph quantum.** Its evidence was "removed it, the
  stuttering returned" — on boots that were mode A and would have stuttered
  regardless. The quantum is left at 2048 because a wider margin is defensible
  on hardware this small, not because it is known to do anything.

**`[PI3-FOUND-420]` There is no instrument for this symptom.** *(True when
written; closed on 2026-09-10 by `vaino-hci-capture`, which does track it
`[PI3-FOUND-430]`. The table below remains accurate about everything above the
HCI layer, and is why the search had to go below it.)* Every detector tried
sits either before the loss or measures something that does not track it:

| Instrument | Verdict |
|---|---|
| `vaino-underruns` | Does not correlate — 11.58 s sounded perfect, 1.64 s stuttered |
| `pw-top` xruns | Always zero, clean and stuttering alike |
| Sink-monitor capture | Measures audio *before* SBC encode and transmit; always continuous |
| Signal level, retry counts | Both tried, both refuted |
| `debugfs` `hci0` | Configuration knobs only; no throughput or error counters |
| PipeWire `wchar` | Reads zero while playing; not a proxy |

**The listener's ear was the only detector**, which is why every hypothesis
here cost a three-minute listening test, why the data points are few and
noisy, and why five explanations were published and withdrawn. The advice that
closed this section -- budget for that, or build an instrument first -- was
taken: the instrument is `vaino-hci-capture` `[PI3-FOUND-430]`. The ear is
still what says *when* a stutter happened; the capture is what says how much
audio went missing while it did.

### `[PI3-FOUND-430]` The stutter has a signature, measured 2026-09-10

A Mode A power cycle stuttered on the usual ~15 s spacing. `vaino-hci-capture`
was started mid-train, at uptime 123.8, and ran 180 s -- so it covers a
stuttering stretch and a clean stretch on the same link in the same boot,
which is the control this investigation never had. Raw log:
`logs/hci-20260910T135004Z.log`.

| region (uptime) | ear | bytes/s | pkt/s |
| --- | --- | --- | --- |
| 124-137 | stuttering | 40994 | 65.6 |
| 144-171 | stuttering | 38302 | 61.0 |
| 180-221 | clean | 46120 | 75.6 |

**The deficit is real.** Over 144-171 the host handed the controller 17% less
audio than during clean playback: 4.4 seconds of audio missing out of 26.

**It ends when the listener says it ends.** Throughput reaches a flat 75-77
pkt/s at 179.3 and never dips again through the remaining 124 s. The listener
reported clear from about 185-190. **This is the first instrument on this
appliance whose reading correlates with the ear** `[PI3-FOUND-420]`, and it is
the reason the five withdrawn theories above were expensive: every one of them
was argued from ear-points alone.

**The 15-second period is in the packet data.** Deepest dips at 129.8/131.9
and 146.0/148.0 -- 16.1 s apart -- with shallower minima at 161-163 and 175.

**What it does not settle.** The shortfall is visible at HCI TX, before the
air, so the speaker is not the thing discarding audio. Whether the host is
starving the encoder or the controller is refusing packets is **not
distinguished here**, and the obvious-looking evidence is worthless: the
completions column runs at exactly 0.500 of TX in both regimes across 98
samples. That precision means the ratio is structural -- completions batched
two packets at a time -- so it says nothing about credit availability. It is
recorded here so that nobody later mistakes it for flow-control health.

**An anomaly, flagged and not used.** From 222.6 the completions column reads
zero for 80 consecutive seconds while TX stays at a healthy 75-77. A host
cannot transmit indefinitely without credits returning, so this is almost
certainly an artifact of the capture. The TX column is not implicated, but the
completions column should not be trusted until a second run either reproduces
this or does not.

Single run. `btmon` was running during it, at `Nice=10`; the stutters preceded
it and continued through it, so it did not cause them, but it cannot be ruled
out of their depth.

### `[PI3-FOUND-440]` What the redial does, in packets, measured 2026-09-10

The second Mode A cycle captured from boot, so it holds the redial itself.
Raw log: `logs/hci-20260910T135726Z.log`.

| region (uptime) | ear | bytes/s | pkt/s |
| --- | --- | --- | --- |
| 30.9-51.1 pre-redial | a couple of stutters | 44635 | 73.5 |
| 52.1-88.3 | silence | 0 | 0 |
| 89.3-199.2 post-redial | smooth | 46057 | 75.5 |

**The redial restores the full rate, and it holds.** 110 seconds after it, not
one dip. Before it, the mean was only 3% down but with local dips to about
36000 B/s at 49-51 -- which is what "a couple of stutters" looks like as
packets. Against `[PI3-FOUND-430]`'s 17% over a full train, severity tracks
the ear in both directions. `[PI3-FOUND-400]` was ear-only until now; it has a
mechanism.

**The redial waits on the wrong signal.** The gap was 36.2 s. `vaino-speaker.sh`
polls for `Connected: yes` with a ten-iteration ceiling, sleeps one second and
posts `reopen-output` -- so the reopen went out roughly 24 s before the
speaker was carrying audio. Audio came back clean anyway, but by a later
tick's routing check `[PI3-AIM-050]` rather than by design. What matters for
audio is the `MediaTransport1` state, not the ACL link, and `vaino-btctl`
already has `transport_state()` for it. **Not changed yet**: it would confound
a capture series in progress.

**Read rates as totals over elapsed time, never off one row.** In steady state
the per-second rows alternate about 68 and 85. That is not a dip. The tick
marker runs at 1.005-1.03 s rather than exactly 1 s, so counts alias across
the boundary while the pair-sum stays at 76/s. Every figure in this section is
total bytes divided by elapsed time.

**The completions anomaly reproduced, and was wrongly dismissed.** It was
called an artifact of a single capture when first seen. It appeared again here
with the same signature -- one partial second (16, against 15 the first time)
then zero for the rest of the run while TX stays at a healthy 75-77. Onsets
were 98.8 s and 135.1 s into their captures, both during settled playback,
neither overlapping a stutter, so nothing above depends on it. It is
repeatable behaviour of the capture or the stack, not noise, and reading the
raw `btmon` output either side of that second is a cheap next experiment.

### `[PI3-FOUND-450]` The redial is removed, on the listener's judgement

It worked. Four Mode A cycles came up smooth once it had fired, and
`[PI3-FOUND-440]` measured what it did: a flat 46057 B/s for 110 s with no
dips. It is the only intervention in this investigation that demonstrably
changed the symptom.

It was removed anyway, on 2026-09-10, because the cure is more disruptive than
the disease. It tears down working audio and rebuilds it, and the hole is not
small -- 36.2 s on the captured boot, against the eleven seconds the first
version was tuned down from -- and it is paid on every boot, including the
Mode B boots that never stuttered at all. A stutter every fifteen seconds is
irritating. Half a minute of silence in the middle of a track, every time, is
worse. That is a judgement about how the appliance should feel to use, which
is the listener's to make and not a measurement's.

**This is not a finding about the stutter.** The stutter is unfixed and will
return on Mode A boots. What is gone is a mitigation whose price was declined.

Two things kept, so this is not relearned: the reasoning stays in
`vaino-speaker.sh` where the code was, and anyone reinstating it must wait on
the `MediaTransport1` state rather than `Connected: yes` -- the removed
version posted `reopen-output` about 24 s before the speaker carried audio,
and recovered by a later tick's routing check `[PI3-AIM-050]` rather than by
design.

Removing it also buys the capture series something it wanted: a Mode A boot
recorded from power-on with a full stutter train and nothing intervening in
it.

### `[PI3-FOUND-460]` A full train, captured whole, 2026-09-10

The first Mode A boot recorded from power-on with nothing intervening -- the
redial having been removed the same afternoon `[PI3-FOUND-450]`. Raw log:
`logs/hci-20260910T140551Z.log`. Clean reference 45750 B/s / 74.8 pkt/s, from
the two smooth stretches of the earlier runs.

| region | % of clean | ear |
| --- | --- | --- |
| run 2 post-redial | 100.1 | smooth |
| run 1 post-settle | 99.9 | smooth |
| run 2 pre-redial | 94.7 | a couple of stutters |
| **run 3, no redial, whole window** | **87.1** | continuous, every ~15 s |
| run 1 stutter train | 83.4 | full train |

**Severity tracks the ear across five regions and three boots, both ways.**

**The deficit is present in the first packets and never clears.** Every
20-second block runs 86-94% of clean from audio start to the end of the
window. Over 170 s the appliance handed the controller 12.9% less audio than a
healthy link -- about 22 seconds of audio missing. Nothing arrives partway
through to cause it. **This is evidence against the library-load explanation**
(the second future path below), which predicts onset at playback rather than a
deficit already in place before the first note is audible. Not a refutation --
the load also begins before audio -- but the experiment now has a prediction
to fail.

**The ear and the minima agree event by event.** Fitting the wall-clock offset
against the listener's marks gives 4.0 s, which is firmware plus bootloader on
a Pi Zero 2W, and at that offset **7 of 9 marks fall within 2 s of a measured
throughput minimum**. Earlier runs agreed only on boundaries; this one agrees
stutter by stutter. Mean period 14.7 s. The unmatched marks are +99 and one of
the +136/140 pair; minima at 61, 75 and 179 fall in stretches the listener
summarised as "roughly 15 second intervals" rather than listing.

**The redial is confirmed by its absence.** With it: 45800 B/s flat, zero
dips, 110 s. Without it, same speaker, same mode, same day: 39840 B/s, dips
throughout, no recovery inside 180 s. That is as close to a controlled A/B as
this appliance allows, and it means the removal was a real trade rather than a
no-op.

**The completions anomaly reproduced a third time**, onset 136.0, same
signature -- one partial second then zero while TX stays healthy. Three for
three, always in settled playback, never overlapping a stutter.

**An arithmetic correction to the earlier figures.** Rates were being computed
by dividing n+1 samples' bytes by n intervals, which biases every figure high
-- most visibly a "105.7% of clean" tail block that was pure artifact. All
numbers in these sections now exclude the first sample's bytes. The earlier
findings move by under a percent and none of their conclusions change.

### `[PI3-FOUND-470]` Mode B measured, and the pair is matched

Mode B captured the same afternoon as `[PI3-FOUND-460]`, same speaker, same
code, no redial in either. Raw log: `logs/hci-20260910T141610Z-modeB.log`.

| | Mode A (run 3) | Mode B (run 4) |
| --- | --- | --- |
| whole window | 39840 B/s, 87.1% of clean | 45803 B/s, 100.1% |
| 20 s blocks | 86-94% throughout | 99.4-100.6% throughout |
| dips under 40000 B/s | every ~14.7 s | none |
| audio missing | ~22 s in 170 | none |
| ear | stuttering to +171 at least | smooth |

**The mode decides it, measured rather than heard.** That was the claim of
`[PI3-FOUND-350]`, argued from listening; the separation here is total rather
than marginal.

**The deficit and the audible stutter are one phenomenon.** It was worth
asking whether Mode B might carry dips the listener could not hear -- that
would have meant two things were being conflated. It carries none.

**It is decided at connection and does not change.** Mode B's first block is
already at 100.5%; Mode A's first block is already deficient; neither drifts
across 180 s. Whatever differs is established while the link is being set up
and then held. That is why every "something arrives later and disturbs it"
theory has failed to predict anything, the library load included.

**The completions anomaly is an instrument defect with a signature.** Across
the three captures that recorded from boot, its onset varies by 41 s of wall
clock and by 1.5% of cumulative packets:

    run 2   uptime 155.89   6541 packets transmitted
    run 3   uptime 135.99   6639 packets
    run 4   uptime 114.68   6577 packets

It fires at about 6600 packets, not at a time -- a counter filling or wrapping,
almost certainly in `btmon` or the monitor socket rather than in the
controller. Run 1 looked arbitrary only because its capture began mid-playback
and its count started from an unknown offset. Nothing in these findings rests
on the completions column, and now nothing needs to: what it measures is the
capture, not the link.

**Timing, for the record.** Mode B connected at +29 and audio began at +34,
against +35 and +39 for Mode A -- the speaker being already awake rather than
cold-booting alongside the Pi. The modes differ in more than one variable,
which is why the packet rate is the comparison that counts and the timings are
not.

### `[PI3-FOUND-480]` A silence that could be attributed, 2026-09-10

The Mode B cycle after `[PI3-FOUND-470]` connected and then played nothing.
Power on 2:23:45, connect tone 2:24:16, still silent at 2:25:25 with the
progress bar advancing and volume raised at both ends.

Every layer on the appliance reported success, which is the `[PI3-FOUND-030]`
signature exactly:

| Check | Reading |
| --- | --- |
| `wpctl` sink | MIDDLETON, vol 0.83 |
| `wpctl` streams | both channels linked to MIDDLETON playback, active |
| player `/audio/sink` | `{"sink":"MIDDLETON","dummy":false,"known":true}` |
| BlueZ | Connected: yes, Trusted: yes, codec SBC |
| connected devices | MIDDLETON only -- no interloper `[PI3-FOUND-090]` |

**And this time there was one more question to ask.** 373 `ACL Data TX`
packets in 5 s -- 74.6/s, indistinguishable from the clean Mode B run. The air
was carrying a healthy stream and the speaker was not making sound. **The
fault was in the speaker**, and no work on the Pi would have fixed it.

That attribution had never been possible before. PI009's silence had every
layer reporting success and no way to separate a truthful report from a lie;
the whole appliance-side supervisor exists because of it. A five-second packet
count now answers the question those five layers could not.

Recovery was a manual disconnect and reconnect -- both returned in under a
second -- followed by `reopen-output`. Audio returned immediately, and the
rate after was 375 packets in 5 s.

**Two honest limits on this entry.** The fault state was destroyed before the
transport's `State` and SBC `Configuration` were read: a path-extraction query
failed and restoring the listener's audio was chosen over digging. If it
recurs, those two properties are the first to capture, before touching
anything. And it revises `[PI3-FOUND-470]`: Mode B is reliably *not
stuttering*, which is narrower than reliably good. This was a different fault,
not a stutter, but the single-observation caution recorded there was
warranted.

### `[PI3-FOUND-490]` Role and the hop map, the two things never read

The question that produced this was the listener's: what is *this appliance*
doing to the state of the speaker? Three things, all read off the live link on
2026-09-10 with `vaino-linkstate`:

| | Healthy outbound link |
| --- | --- |
| `role` | **CENTRAL** |
| `direction` | outbound (`<`) |
| `afh` | `000000f47ffcffffff3f` -- 48 of 79 channels |
| `link_quality` / `rssi` / `tx_power` | 255 (max) / 0 (golden range) / 12 |
| `supervision_timeout` | 20000 ms |
| transport `State` / `Volume` / `Configuration` | active / **106** of 127 / `4 17 21 2 53` |
| wifi | **channel 1, 2412 MHz, 20 MHz wide** |

**What this appliance writes into the speaker.** The AVRCP absolute volume --
106 here -- which the speaker keeps across a reboot of the Pi, and which is
the first thing to read on a connected-but-silent fault `[PI3-FOUND-480]`. The
SBC configuration, long since exonerated as identical in both modes. And, when
it is CENTRAL, the AFH channel map.

**`role` is not `direction`, and only `role` was never checked.** Section 9's
mode comparison recorded `<` against `>` and section 8 called that "ACL role
CENTRAL (`<` outgoing -- the Pi initiated)". Those are two different fields.
Whoever initiates *starts* as CENTRAL, but either end may request a role
switch afterwards and BlueZ commonly permits one, so a Mode A link may have
been running with this Pi as PERIPHERAL on every boot with nobody looking.
`[PI3-FOUND-390]` refuted *direction* as the mechanism; it did not touch role.

**The hop map excludes exactly this Pi's WiFi channel.** Channels 0-25 are
disabled, and at 2402 + k MHz that is a refusal to hop anywhere in 2402-2427.
This Pi's WiFi sits on channel 1: 2412 MHz, 20 MHz wide, spanning 2402-2422.
The map was also observed to change between two reads minutes apart (52
channels, then 48), so the adaptation is live rather than fixed at connection.

> **The hypothesis, stated so it can be killed.** The CENTRAL sets the hop
> map. This Pi knows where its own WiFi radio is; the Middleton does not. So
> when the Pi is CENTRAL the speaker is handed a map that avoids a transmitter
> sitting inches away, and when the speaker is CENTRAL that protection does
> not exist. That would predict mode dependence, a deficit present in the
> first packets and never drifting `[PI3-FOUND-470]`, roughly three minutes of
> settling as the speaker's own blind assessment learns the bad channels, no
> correlation with any Pi-side metric, and physical separation appearing to
> help before it was ruled confounded `[PI3-FOUND-320]`.
>
> **It is a hypothesis. Five have been withdrawn from this document.** Its
> prediction is sharp: a stuttering Mode A boot reads `PERIPHERAL`, or shows
> the low channels enabled, or both. **A stuttering boot that reads CENTRAL
> with a 48-channel map excluding 2402-2427 kills it outright**, and that is
> the outcome to hope for as much as any, because it is the first prediction
> here that costs one command to test.

Run `sudo vaino-linkstate` immediately after a boot in each mode and diff.

**Withdrawn the same evening, by the test it named.** See
`[PI3-FOUND-500]` below.

### `[PI3-FOUND-500]` The hop-map hypothesis, killed in one boot

The hop-map hypothesis of `[PI3-FOUND-490]` predicted that a stuttering Mode A
boot would read `PERIPHERAL`, or show the low channels enabled, or both, and
said that a
stuttering boot reading CENTRAL with the WiFi band excluded would kill it
outright. That is exactly what happened, on the next Mode A cycle, 2026-09-10.

Twenty-two samples of `vaino-linkstate` across 225 s, alongside
`logs/hci-20260910T160818Z-modeA-linkstate.log`:

| | |
| --- | --- |
| `role CENTRAL` | 22 of 22 |
| channels 0-25 excluded (2402-2427 MHz) | 22 of 22 |
| `link_quality` 255, the maximum | 22 of 22 |
| `volume` 106 | unchanged throughout |
| throughput | 40199 B/s, **87.9% of clean** |
| ear | stutters from +45, continuing |

The Pi never became PERIPHERAL, the WiFi band was excluded at every sample,
and the audio was 12% short the whole time. **The hypothesis is withdrawn** --
the sixth in this document, and the first to cost one boot and one command
rather than a series of three-minute listening tests. That is the instrument
earning its cost.

**Three things the run gives us regardless.**

*AFH adapts in about ten seconds, not three minutes.* All 79 channels at
uptime 29.3, down to 44 by 38.6. Any story in which the three-minute settle is
the speaker slowly learning bad channels is dead alongside the main
hypothesis.

*The map never converges.* It moved between 41 and 50 channels for the whole
225 s -- `000000acbdfbdeffbc3f`, `000000ecefbbffffff3f`, `000000a4d7eb7dffff17`
-- so the radio is continuously reclassifying the upper band while the
stuttering continues regardless.

*The controller thinks the link is perfect.* `link_quality` read 255, its
maximum, at every sample, while 12% of the audio failed to get out. It joins
the table in `[PI3-FOUND-420]`: another instrument that reports health through
a fault the listener can hear.

**Where that leaves it.** Role, direction, codec, configuration, volume,
transport state, hop map, link quality, RSSI and transmit power have now all
been read on both a stuttering and a clean link, and every one of them is
either identical or reports perfect health. Nothing the Pi can see or set
distinguishes the two cases. Combined with `[PI3-FOUND-480]`'s attributable
silence, the evidence points inside the Middleton, where nothing here can
reach.

### `[PI3-FOUND-510]` The speaker remembers the band, and that is the difference

The listener's hypothesis, tested 2026-09-10: the Middleton learns this
room's interference, keeps it across a Mode B restart, and loses it across a
Mode A one. Two boots, same sweep, `vaino-linkstate` every 9 s.

| | Mode A (run 5) | Mode B (run 6) |
| --- | --- | --- |
| first sample with a link | 29.3 s, **79 channels, naive** | 32.7 s, **56 channels, low band already excluded** |
| by ~40 s | 44 ch; 2430, 2450, 2466-67, 2472, 2480 excluded | 52 ch, **the settled pattern exactly** |
| through ~220 s | wandering 41-50, up to ten upper exclusions | **stable 51-52, never more than one** |
| throughput | 40199 B/s, 87.9% of clean | 45790 B/s, **100.1%** |
| ear | stutters from +45, continuing | clean |
| logs | `logs/linkstate-...160818Z-modeA.log` | `logs/linkstate-...-modeB.log` |

**The Pi was power-cycled in both.** The listener confirmed it: power off,
Middleton drop tone, power on. So this appliance's controller began with no
channel assessment either way -- which run 5's naive 79-channel first sample
shows directly. **Mode B's adapted map at its first sample therefore cannot
have come from this machine.** The only other party on the link is the
speaker, and the mechanism the specification provides is peripheral channel
classification reporting, which a CENTRAL merges into the map it broadcasts.

**Role and direction are both eliminated, definitively.** Both boots read
CENTRAL at every sample. Direction came out the *opposite* way round from the
association recorded in section 9 -- this Mode A was outbound and stuttered,
this Mode B was inbound and was clean -- so a Mode B link can be inbound and
perfect. `[PI3-FOUND-390]` refuted direction on one boot; this refutes it in
the reverse configuration.

**The ordering favours cause over symptom.** In Mode A the naive map is
present at 29.3 s, before audio starts at about 38, and the deficit is in the
first packets `[PI3-FOUND-470]`. Map first, deficit second. It remains possible
that a bad link makes a controller blame channels, producing scatter as a
symptom -- but that story does not explain a map that is *already adapted*
before the audio begins.

**A fix this suggests, and it is the first to come from evidence.** The host
can write channel classification down to its controller
(`HCI_Set_AFH_Host_Channel_Classification`). If this appliance persisted its
settled map across reboots and applied it at startup, a Mode A boot could
begin adapted instead of naive, without depending on the speaker's memory at
all. Untried, and it should be treated as a hypothesis until a boot tests it.

**Limits, stated plainly.** One boot per mode for the map comparison. The
interference is environmental and varies, so a quiet stretch could flatter
Mode B. The ZigBee reading of section `[PI3-FOUND-500]` is still unconfirmed:
the network's channel is not presently known, and the exclusions that looked
like ZigBee centres remain suggestive rather than established.

### `[PI3-FOUND-520]` Seeding the map: the experiment, armed 2026-09-10

If the speaker's retained channel map is what makes a Mode B boot clean
`[PI3-FOUND-510]`, then this appliance does not need to depend on the
speaker's memory. The host can write its own classification down to its
controller with `HCI_Set_AFH_Host_Channel_Classification` (OGF 0x03, OCF
0x003F), and the controller ANDs that with what it learns. So: save a map
once, while the link is settled and sounding right, and hand it back on every
boot.

`vaino-afh-seed` does that in three verbs -- `save`, `apply`, `show` -- plus a
`boot` mode that applies seven times at ten-second intervals, because BlueZ
powers the adapter up after the unit is ordered to start and a controller
reset silently discards any classification written before it.

Seeded from the settled link on 2026-09-10:

    map 000000fcffffffffff3f -- 52 of 79 channels
      excluded ch 0-25 = 2402-2427 MHz     (this Pi's WiFi, channel 1)
      excluded ch 78-78 = 2480-2480 MHz

Verified end to end on the appliance: saved, decoded, applied, controller
accepted it, map unchanged afterwards as expected for a mask identical to the
one already in use.

> **The prediction, written before the run.** A Mode A power cycle with the
> seed enabled reads **closer to 100% than to 88%** on `vaino-hci-capture`,
> and the map's first sample shows the low band already excluded rather than
> all 79 channels enabled. **If it stutters at ~88% anyway, the hop map was a
> bystander and this comes back out.** Six theories have been withdrawn from
> this document; the seventh gets no more faith than its evidence.

**Two ways it could mislead, named in advance.** A quiet stretch of
environmental interference would flatter any Mode A boot, so a single clean
run is suggestive rather than conclusive -- the comparison that counts is
against run 5's 87.9%, on the same speaker in the same room. And the seed
cannot help with interference the saved map does not describe; if the ZigBee
reading of `[PI3-FOUND-500]` is right and those transmitters move channel,
a stale map is worth nothing.

**The risk it carries.** A saved map is a claim about one room. Seeded
elsewhere, or after the interference moves, it excludes channels that were
fine and costs hop diversity for no benefit. Hence `save` refusing anything
with fewer than the specification's 20 usable channels, `apply` re-validating
before it writes, and the unit shipping disabled.

**Ran the same evening. It failed its own test.** See `[PI3-FOUND-530]`.

### `[PI3-FOUND-530]` The seed worked, the audio did not, and the map is a symptom

The experiment armed in `[PI3-FOUND-520]` ran on a Mode A cycle the same
evening. Logs: `logs/hci-20260910T165525Z-modeA-seeded.log`,
`logs/linkstate-20260910T165525Z-modeA-seeded.log`.

**The mechanism worked.** The journal shows the retry design earning itself:
the first write at 16:55:16 was rejected because the adapter was not up yet,
and the second at 16:55:26 succeeded. A single write at boot would have
silently done nothing and the run would have proved nothing.

**Half the prediction was met.** First sample, uptime 34.3: 47 channels with
the low band already excluded, against run 5's naive 79.

**The other half failed outright.** 39770 B/s, **86.9% of clean**, against run
5's 87.9% and run 3's 87.1% -- statistically indistinguishable -- with stutters
on the usual spacing from +65. The prediction was "closer to 100% than to 88%".
**`[PI3-FOUND-520]` is withdrawn**, the seventh in this document, and
`vaino-afh-seed` was disabled on the appliance the same hour, as the prediction
required.

**But it disconfirms something specific, which is why it was worth running.**
Counting exclusion zones above channel 26:

| | first sample | mean upper zones |
| --- | --- | --- |
| run 5, Mode A naive | 79 ch, 0 | **6.5** |
| run 6, Mode B clean | 56 ch, 0 | **1.8** |
| run 7, Mode A seeded | 47 ch, 5 | **4.2** |

The seed supplied the **low** band. Within thirty seconds the controller had
added five upper-band zones of its own, and it averaged 4.2 across the run.
Mode B, sounding perfect, averaged 1.8.

So the low-band difference between the modes was never the mechanism, and the
causal reading of `[PI3-FOUND-510]` is disconfirmed: Mode A now starts with
exactly the low-band exclusion Mode B had and sounds exactly as bad. What
still separates the modes is the **upper**-band scatter, and since seeding
could not prevent the controller from generating it, the scatter looks like a
controller marking channels bad because packets are failing on them -- a
symptom -- rather than packets failing because channels went unmarked.

The measurement in `[PI3-FOUND-510]` stands: the speaker does carry adapted
state across a Mode B restart. What is withdrawn is the inference that this
state is what makes Mode B sound clean.

**Direction refuted a third time.** This Mode A came up **inbound**; run 5 was
outbound. Both stuttered, both read CENTRAL throughout.

### `[PI3-FOUND-570]` The Oontz sounds clean at the Middleton's stuttering rate

First cold-boot capture of the second speaker, Mode B, 2026-09-10. Logs:
`logs/hci-20260910T171700Z-oontz-modeB.log`,
`logs/linkstate-20260910T171700Z-oontz-modeB.log`.

| | Middleton | Oontz |
| --- | --- | --- |
| AFH channels | 41-50 | **20 -- the specification's minimum** |
| link quality | 255 (max) throughout | **95**, then 255 |
| SBC configuration | `4 17 21 2 53` | `4 17 21 2 45` |
| AVRCP volume | 106 | **absent** |
| throughput | 45750 clean / ~39900 stuttering | **39502** |
| ear | stutters on Mode A | **no stutters** |

**The absolute byte rate is not the predictor, and very nearly fooled me.**
39502 B/s is a clean Oontz. 39770 and 40199 B/s were stuttering Middletons.
The instrument measures a deficit *against that speaker's own baseline*, and a
figure from one speaker says nothing about another. The Oontz's lower rate is
explained by its codec, not by any fault: bitpool 45 against 53 predicts 84.9%
of the Middleton's rate and 39502/45750 is 86.3%.

**Flatness does not rescue it either.** Block-to-block variation was tried as a
speaker-independent measure and does not separate: run 4, clean, sits at 5.4%
while run 3, stuttering, sits at 6.8%. There is no absolute threshold and no
variance threshold. **Every future reading needs a baseline captured from that
speaker, sounding right.**

**Two more mechanisms fall out.** The Oontz hops on **20 channels**, the
fewest the specification permits, and sounds perfect -- so a restricted hop map
does not cause this, independently confirming `[PI3-FOUND-530]`. And its
controller reports link quality 95 where the Middleton reports a flawless 255
while stuttering, which finishes link quality as a predictor: it reads *worse*
on the speaker that works.

**What this is not.** It is not the Middleton-versus-environment test. Mode A
is defined by the speaker's power cut taking the Pi with it, which happens only
because the Pi runs off the Middleton's USB port. The equivalent for the Oontz
is deliberately cold-booting both together, and that has not been done.

### `[PI3-FOUND-580]` The completeness map: it is the Middleton

The listener closed the grid on 2026-09-10 by cold-booting the Oontz and the
Pi together -- the Mode A condition, reproduced deliberately for a speaker that
does not power the appliance. Logs: `logs/hci-20260910T172457Z-oontz-modeA.log`,
`logs/linkstate-20260910T172457Z-oontz-modeA.log`.

| speaker | mode | % of that speaker's own baseline | ear |
| --- | --- | --- | --- |
| Middleton | A | 87.1, 87.9, 86.9 | **stutters, 3 of 3** |
| Middleton | B | 100.1 | clean |
| Oontz | B | 100 (the baseline) | clean |
| **Oontz** | **A** | **100.2** | **clean** |

Every Oontz Mode A block sits between 98.4% and 101.1%. One cell of four
fails, and it fails every time.

**What this exonerates.** The room, and the interference in it: a second
speaker cold-booting in the same place at the same hour is flawless. The
cold-boot-together condition itself, which is identical for both. And this
appliance's own behaviour, which runs the same code path either way. Every
Pi-side explanation is now excluded by a control rather than by argument.

**What it implicates.** The Middleton, specifically, in how it comes up from
cold. `[PI3-FOUND-350]` said the power-cycle mode decides it, which was true
and incomplete: the mode decides it *for this speaker*.

**And the hop map dies a third time, most clearly here.** The Oontz cold-booted
to 54 channels and settled to 47 with scattered upper-band exclusions --
`000080bcdddefffbff3f`, the pattern that had looked like the fault signature --
and sounded perfect throughout. Scatter on a clean link is the cleanest
possible refutation `[PI3-FOUND-530]`. Its tight 20-channel map from the Mode B
run was an adapted state, not a property of the speaker.

**Controller health metrics now point the wrong way, consistently.** The Oontz
reported link quality 85-92 and RSSI -6 through clean audio; the Middleton
reports 255, the maximum, while stuttering.

**What this changes about the goal.** The stutter is a defect in one speaker's
cold-start behaviour, not something this appliance is doing wrong and not
something it can repair. The remaining choices are practical rather than
diagnostic: use the Oontz, use the Middleton in Mode B, or reinstate an
intervention like the redial `[PI3-FOUND-450]` that re-establishes the link
after the speaker has been awake a while -- which, read in this light, is
exactly what it was doing.

### `[PI3-FOUND-590]` Both speakers powered: three results in one sequence

The listener ran a mixed sequence on 2026-09-10 with both speakers in play.
Three separate things came out of it.

**1. The Middleton disturbs a link it is not part of.** At 5:32:01 the
Middleton was powered on while the Oontz was connected and playing. The Oontz
then stuttered -- small ones at 5:32:32, 5:32:49 and 5:33:13, a bigger one at
5:33:45 -- and was smooth again by 5:34:45. The Middleton was not connected to
anything during this; it was simply coming up.

That is a Middleton cold start degrading a **different speaker's** link in the
same room, and it corroborates `[PI3-FOUND-580]` from an angle the completeness
map could not reach: the speaker is not merely fragile on its own link, it is
an active disturbance in the band while it starts. It also fits the shape of
the Mode A fault, where the Middleton's cold start overlaps the appliance's.

**2. The fallback works, exercised for real.** The Oontz was powered down at
5:34:45. `speaker_address` still named the Oontz, and the Middleton was
untrusted -- so nothing but `[PI3-FOUND-560]` could have done it. Audio was
playing smoothly from the Middleton by 5:35:25, about forty seconds later,
with no intervention. First real exercise of the path that could not be tested
when it was written, because it refuses to run while anything is audible.

**3. Two connected audio devices, and silence from both.** At 5:35:45 the
Oontz was powered back on. Within seconds the audio stopped, both speakers
showed connected, and raising every volume did nothing. The diagnosis took one
command:

    < eSCO 08:EB:ED:26:14:12 handle 6 state 1 lm CENTRAL
         48. output_MONO  > OontZ_Angle 3 U412:playback_MONO  [active]

An eSCO link and a **MONO** port: the Oontz had been taken as an HSP/HFP
headset rather than an A2DP sink, which is `[PI3-FOUND-290]` returning under a
condition it was not found in. Zero `ACL Data TX` in five seconds confirmed
nothing was reaching the air at all.

**The recovery was a guess, and it is recorded as one.** `wpctl set-profile 53
0` was issued without checking what index 0 meant; it means **off**. It
restored audio only because PipeWire then fell back to the Middleton. It left
the Oontz holding an ACL link with a dead profile, which produced the next
finding, and a considered fix would have disconnected the device instead.

**And that dead profile exposed a real defect.** With the Oontz holding a link
but offering no sink, the keeper's routing check saw a mismatch it could never
resolve and demanded a reopen **every thirty seconds, indefinitely**, against a
device with nowhere to send audio -- while the stream was already playing
happily on the other speaker:

    17:39:34  08:EB:ED:26:14:12 is connected but the stream was on 'MIDDLETON' ... asked the player to reopen
    17:40:08  (same)
    17:40:41  (same)

`Connected: yes` means BlueZ has a link. It does not mean PipeWire has anywhere
to send audio. A reopen that cannot succeed is not a harmless no-op -- it is an
instruction to abandon working audio. The check now requires the target sink to
actually exist, and says so plainly when it does not. Verified under the
service's own user: `MIDDLETON` present, `OontZ_Angle 3 U412` absent, a
nonsense name absent.

**A smaller one fixed alongside.** The interloper marker used
`${XDG_RUNTIME_DIR:-/tmp}`, but that variable is *set* under a root run and
names `/run/user/0`, which does not exist -- so the fallback never triggered
and every write failed. It now tests the directory rather than the variable.

### `[PI3-AIM-070]` The design: audio sticks, and one speaker at a time

Stated by the listener on 2026-09-10, after a session with both speakers in
play:

> Once audio is established with one speaker, it should remain with that
> speaker even as other recognised speakers become available. Other speakers
> are only connected if the current one becomes unavailable.
>
> The user's selected speaker in the settings page would be the preferred
> choice when starting with multiple speakers available.

**What the keeper used to do instead.** The routing check compared the stream
against whichever speaker was *connected* and moved the stream whenever they
differed. So a chosen speaker returning mid-session would drag audio off a
speaker that was playing perfectly well. That is the right rule for deciding
where to send audio going nowhere, and the wrong one for audio already going
somewhere.

**Viability, not identity, is the question now.** A route is fine if it names a
sink PipeWire still offers; only when it stops being one does anything move.
Verified on the appliance: with the Middleton playing and the Oontz -- the
*chosen* speaker -- reconnected, the keeper ran silently and left the stream
where it was.

**The preference gets exactly one chance.** Only while the chosen speaker is
the connected one, only inside the first two minutes of uptime, and only once
per boot. The chase already implements most of the preference by going after
the chosen speaker first and falling back only once that fails
`[PI3-FOUND-560]`; this covers the gap where another speaker's sink appears
first and stickiness would then hold audio there all session. A preference
able to fire at any time would be the mid-track switch stickiness exists to
prevent.

### `[PI3-FOUND-600]` Two connected speakers is not degraded audio, it is none

Measured the same evening, with both powered and connected. Three five-second
samples of `ACL Data TX` with the Oontz holding a link alongside the playing
Middleton:

    0   0   0

Disconnect the Oontz, and the same measurement on the same link:

    374   375   374

Not degraded. **Stopped.** The listener's account matches to the second:
*"Oontz is connected, but silent"*, then *"now Middleton is audible again"*.

The link list says why:

    < ACL  20:64:DE:CF:F3:AD   (Middleton, playing)
    < ACL  08:EB:ED:26:14:12   (Oontz)
    < eSCO 08:EB:ED:26:14:12   (Oontz -- HSP/HFP)

The second device had taken an **eSCO** link as well as an ACL one -- the
HSP/HFP headset profile `[PI3-FOUND-290]` -- and a synchronous link does not
share the radio with A2DP, it pre-empts it.

So the keeper now disconnects any speaker holding a link while another is
playing, which `[PI3-AIM-070]` authorises directly. Guarded on the stream
actually playing on a real sink, so it cannot fire during startup while sinks
are still appearing, and it never targets the device the audio is going to --
verified by dry run against the live adapter, which selected the playing
Middleton as **KEEP** and nothing for disconnection.

**Not yet exercised on a real second connection.** The Oontz declined to
reconnect when the rule was deployed, so the disconnect branch has not fired
in anger. The half that could do damage -- picking the wrong device -- is the
half that was tested.

**This also explains the earlier interference report.** At 5:32 the listener
powered the Middleton on while the Oontz played, and heard four stutters over
two minutes `[PI3-FOUND-590]`. A second device negotiating profiles in the
band is not free, and at its worst -- a synchronous link -- it is total.

### `[PI3-FOUND-610]` An untrusted speaker knocks forever, and the machine wedged

The journal survived this one, because `Storage=persistent` had been left on
from an earlier investigation. It is the first time in this whole record that
a fault could be read after the fact rather than reconstructed.

**What the Middleton was doing.** From the moment it was powered on, every nine
seconds:

    17:53:10  bluetoothd: Authentication attempt without agent
    17:53:10  a2dp.c:auth_cb() Access denied: org.bluez.Error.Rejected
    17:53:19  (same)   17:53:28  (same)   17:53:38  (same)

**The mechanism is this appliance's own doing.** A trusted device is
authorised automatically; an untrusted one needs an agent to approve it, and no
agent is registered here. The Middleton read `Trusted: no` because the listener
had switched to the Oontz and `withdraw_others` untrusts everything but the
chosen speaker `[PI3-FOUND-130]`. So a powered-on, paired, untrusted speaker
knocks on the door every nine seconds and is refused every nine seconds,
indefinitely. Untrusting is not neutral: it converts a nearby speaker into a
permanent source of connection attempts.

**The one-speaker rule fired correctly, and could not win.** At 17:53:28 it did
exactly what it was built to do -- and the device it disconnected simply came
back on the next attempt.

**Then the machine wedged.** The journal ends at 17:53:38, three seconds before
the listener reported the Oontz going silent, and the appliance ran for
thirteen minutes answering ping while refusing every TCP connection -- ssh and
the web interface alike -- until it was power-cycled.

**No cause was established, and none is claimed.** The kernel logged no I/O
error, no OOM kill, no hung-task warning and no panic; ICMP is handled in the
kernel and kept working, which places the fault in userspace. Three guesses
were made during the incident -- WiFi interference, self-inflicted paging, and
a marginal radio link -- and all three were wrong: the loss cleared to 0% while
TCP stayed dead. **What actually stopped is unknown.**

**Which is the point of what was built next.** Whatever happened, the record
that would have shown the run-up was journald, and journald is userspace and
died with everything else. `vaino-vitals` `[PI3-FOUND-620]` writes elsewhere.

### `[PI3-FOUND-640]` The rules inverted themselves, and the policy got shorter

**What happened.** The Middleton was playing. The Oontz was powered on at
18:24. The journal:

    18:24:08  'OontZ_Angle 3 U412' is holding a link while 'MIDDLETON' is
              playing -- disconnecting it
    18:24:15  bluetoothd: a2dp_select_capabilities() Unable to select SEP
    18:24:41  'MIDDLETON' is holding a link while 'OontZ_Angle 3 U412' is
              playing -- disconnecting it

The rule fired correctly first, kicking the intruder off a playing incumbent.
The Oontz -- trusted, so it reconnects unprompted -- came straight back, A2DP
negotiation failed, and the Middleton's sink died with it. On the next tick the
viability test read the dead sink as licence to move the audio to the Oontz,
and the same rule then kicked the Middleton. **Every rule did exactly what it
said, and together they did the opposite of the design.**

**The error, precisely.** Making *viability* the test for keeping audio where
it is hands victory to any intruder capable of breaking the incumbent -- which,
on one adapter with one A2DP transport, is all of them. The aggressor wins by
breaking the thing it is competing with. Incumbency is an identity, not a
condition: a speaker whose sink was knocked out has not become unavailable, it
has been attacked.

**`[PI3-AIM-080]` The policy is now one sentence.**

> There is at most one speaker connected at a time. It is chosen when none is
> connected -- the listener's speaker first, then any other known one -- and it
> is replaced only when it goes away.

That sentence replaced four rules which had grown separately and had begun to
fight: a stickiness test, a viability test, a startup preference with its
once-per-boot marker, and a disconnect-the-others sweep. It is not a weaker
statement -- it produces the same intended behaviour, and it removes the
interactions that produced the inversion:

| Old rule | Where it went |
| --- | --- |
| stickiness | falls out: there is only ever one candidate |
| viability as licence to switch | deleted: a missing sink is waited for |
| startup preference, once-per-boot marker | falls out: preference applies when picking, which only happens when none is connected |
| disconnect-the-others sweep | kept, but as *enforce exactly one*, not as a reaction to what is playing |
| interloper reporting | deleted: an interloper is disconnected, not narrated |

`vaino-speaker.sh` went from 503 lines to 366 with no loss of behaviour.

**`[PI3-FOUND-630]` The race is closed at the source.** A keeper running
every thirty seconds cannot win against damage that takes five, so the refusal
now happens before a transport is handed over. `vaino-bt-agent` answers BlueZ's
authorisation calls: accept when there is no incumbent, accept when the caller
*is* the incumbent, refuse otherwise. Without an agent BlueZ has only two
reflexes -- authorise a trusted device automatically, refuse an untrusted one
forever `[PI3-FOUND-610]` -- and neither is what this appliance wants.

**It guards every audio profile, not just A2DP.** *(Written before the first
real test, and wrong: it missed the umbrella UUID that BlueZ actually asks
about, and the table below is therefore incomplete rather than reassuring.
See `[PI3-FOUND-660]`.)* The first version refused
A2DP alone, which would have left open the door the last failure came through:
the headset and hands-free profiles open a *synchronous* eSCO link, and a
synchronous link pre-empts A2DP rather than sharing with it `[PI3-FOUND-600]`.
Caught by testing the decision rather than assuming it. Verified against the
deployed agent:

    incumbent, a2dp .............. ALLOWED
    intruder, a2dp ............... REFUSED
    intruder, hands-free ......... REFUSED
    intruder, headset ............ REFUSED
    intruder, remote control ..... ALLOWED

**Not yet exercised in anger.** The refusal path is unit-tested against the
deployed agent and correct in all five cases, but no speaker has actually
knocked since it was installed -- the Middleton was powered down. The first
time two speakers are powered together is the real test.

*(It was, and it failed `[PI3-FOUND-660]`. The five cases were correct and
incomplete: each was fed a UUID drawn from the same list the code checks, so
the test shared the implementation's blind spot. The agent now carries
`vaino-bt-agent selftest`, whose UUIDs are transcribed by hand rather than read
from the code, and which was itself verified by reintroducing the bug and
watching it fail.)*

### `[PI3-FOUND-660]` The agent guarded the UUIDs nobody uses

First real test, 2026-09-10 19:11. The Oontz held the audio; the Middleton was
powered on. The agent was consulted and **allowed it**:

    19:11:09  authorise 20:64:DE:CF:F3:AD for 0000110d (not audio, allowing)
    19:11:31  keeper: MIDDLETON connected while OontZ_Angle 3 U412 holds the
              audio -- disconnecting it

`0000110d` is Advanced Audio Distribution -- the umbrella profile UUID, and the
one BlueZ actually authorises against. The guard listed `...110b` (A2DP sink)
and `...110a` (A2DP source), which are the names nobody asks for. **The agent
was useless in precisely the case it was written for**, and the keeper's
backstop cleaned up twenty seconds later.

The outcome was still correct -- the Oontz kept the audio and the listener
heard no interruption -- but by the slow path the agent exists to remove.

**Why the unit test did not catch it.** Every case was fed a UUID chosen from
the same list the code checks, so the test agreed with the code about which
names matter and both were wrong together. The five green results at
`[PI3-FOUND-640]` were real and meaningless. What caught it was one line of
journal from a speaker actually knocking, which is why *"not yet exercised in
anger"* was worth writing down rather than glossing.

Fixed by adding the umbrella UUID, retested, and the refusal path now covers
umbrella, sink, headset and hands-free while leaving remote control alone.
**Still not exercised in anger** -- the corrected agent has not yet met a real
knock either.

### `[PI3-FOUND-680]` The agent was unreachable, because trust is the enforcement

Second real test, 2026-09-10 19:22. The Middleton held the audio; the Oontz
was powered on. It connected at 19:22:18, **both speakers went silent**, and
the Middleton recovered at 19:22:53 when the keeper's backstop disconnected the
intruder -- thirty-five seconds of silence.

**The agent was never called.** Not refused, not consulted: BlueZ asks an agent
only about an **untrusted** device. A trusted one is authorised without anybody
being asked, and both speakers read `Trusted: yes`.

So the agent could never have worked as built, and the reasoning that removed
`withdraw_others` was backwards. `[PI3-FOUND-650]` called it redundant because
"the agent now refuses any audio profile from anything that is not holding the
audio". It cannot. **Trust is the enforcement; the agent only supplements it,
for the devices trust has already excluded.** Removing the untrusting removed
the mechanism and left the supplement addressing nobody.

**The correction is not simply to put it back.** The old version untrusted
everything but the *chosen* speaker, which is how a Mode A boot could find the
Middleton untrusted while it was the one playing. Trust now follows the
**audio**:

- the incumbent is trusted, so it may let itself back in;
- every other known speaker is untrusted, so its attempts reach the agent,
  which refuses them because it is not the incumbent;
- when nothing holds the audio, the listener's chosen speaker is trusted
  instead -- the one path that does not have to win the power-up race
  `[PI3-FOUND-130]`.

Verified on the appliance immediately after deploying: with the Middleton
playing, the tick logged *"untrusted OontZ_Angle 3 U412 -- only the speaker
holding the audio may let itself in"*, the two devices then read `Trusted: no`
and `Trusted: yes` respectively, and audio was undisturbed at 375 packets per
five seconds.

**Still unproven where it counts.** The agent has yet to refuse anything: the
first test found the wrong UUID list `[PI3-FOUND-660]`, the second found it was
never consulted, and both times the keeper's backstop did the work. A third
test -- powering a non-incumbent speaker on while another plays -- should now
show `REFUSE` in the agent's log within a second and no silence at all. Two
mechanisms have been fixed on the way to that, so it would be premature to
assume the third attempt is the one that works.

### Two paths worth taking, neither of them a fix

**The HCI layer.** `btmon` sees actual ACL data packets and the controller's
own flow control — around 75 packets/s during healthy playback. It is the one
layer between the SBC encoder and the air that has not been observed during a
stuttering boot, and the only remaining candidate for a real instrument. It is
privileged and heavy enough to perturb what it measures, which has already
happened twice here `[PI3-FOUND-170]`, `[PI3-FOUND-240]`, so it wants a
bounded startup capture rather than a permanent watcher.

That capture is now built: `vaino-hci-capture`, installed and disabled, one
fixed window, filtered to two line types before a byte is stored, the raw
stream kept in tmpfs, and aggregated only after the window closes. See PI010
section 3. It has now been run during a stuttering boot and it worked: see
`[PI3-FOUND-430]`. The path is no longer speculative, and the next reading
owed is a second run, to settle the completions anomaly and to catch a train
from boot rather than from the middle. What it decides is which side of the controller the loss sits on:
a dip or a gap in the per-second count means the packets are not getting out,
and a rate that stays flat through a stutter the listener can hear means the
loss is past the controller and inside the speaker. Either answer is progress.
This investigation has never had one.

**Suspending the library load entirely.** The Director rebuild reads ~256 MB
at ~15 MB/s while using half a core `[PI3-FOUND-250]`, overlapping exactly the
window in which the stuttering happens. Making it *fully* wait — not merely
yield, which `bfq` and per-thread idle priority already arrange — would say
whether the startup work contributes at all. It is not a shipping design: an
appliance that will not choose its next passage until minutes after boot has
traded one fault for another. But as an experiment it cleanly separates "the
link is bad" from "the machine is too busy to feed it", which nothing so far
has managed.

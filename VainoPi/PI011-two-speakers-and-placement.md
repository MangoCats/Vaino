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

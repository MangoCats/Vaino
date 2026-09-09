# PI011: Two Speakers, and Where the Stutters Were

**Appliance Record — the answer, and a wrong answer on the way to it**

Split from [PI004](PI004-speaker-operation.md) on 2026-09-08. Concludes the
investigation begun in [PI009](PI009-the-silence-of-2026-09-08.md) and
continued in [PI010](PI010-startup-time-and-stutter.md).

**The answer is `[PI3-FOUND-320]`, and it is not in software.** The appliance
was resting on the speaker, detuning the one antenna its Wi-Fi and Bluetooth
share. §2 reaches the wrong conclusion first and says so; it is kept because
it is the conclusion the evidence genuinely supported at the time.

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

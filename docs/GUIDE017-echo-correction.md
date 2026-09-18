# GUIDE017: Echo Correction — Rate and Offset

**Development Guidance — split from [GUIDE016](GUIDE016-echo-playback-plan-build.md) 2026-09-18**

Phase 5 on its own, because building it grew it past what belonged inside the
plan. Two errors, corrected in two places, for reasons that took some finding:
a **rate** is a slope and is trimmed continuously, an **offset** is a position
and is shed at a passage boundary. Confusing them is how a correction loop
acquires a fault that only shows after hours.

> **Related:** [GUIDE016](GUIDE016-echo-playback-plan-build.md) `[GDE-ECHO-330]` — the phase this follows · [LOG012](LOG012-phase-4-drift.md) `[LOG-P4-130]` — the +13.92 ppm this corrects · [LOG011](LOG011-the-lead-is-a-ring.md) `[LOG-ECHO-030]` — why depth cannot serve · [SPEC021](spec/SPEC021-echo-mode-control.md) `[SPEC-ECHO-030]` — the control that starts it

---

## 1. What is corrected, and where

**`[GDE-ECHO-340]` Rate and offset are different errors, corrected in different
places, and separating them is what makes the ring's depth stop mattering.** A
*rate* error accumulates slowly and is corrected by trimming: drop or duplicate
one frame on submission into the output ring, on the mixer thread, at whatever
interval the measured ppm calls for — one frame every 5.7 s at the ~4 ppm now
measured for `bose`↔`smartboardpc`, and every 1.4 s for `bose`↔`vainopi`, not
the one per minute this plan assumed from the discredited 0.4 ppm
`[GDE-ECHO-550]`.
That the correction is heard fifteen seconds later is irrelevant, because what
is being corrected is a slope, not a position. An *offset* error is corrected
only at a passage boundary, by opening the next passage a few milliseconds
adjusted, where it costs nothing and is inaudible by construction.

This split is why `[REQ-AUD-164]`'s buffer-depth trap does not bite here as it
did four times before `[GDE-ECHO-220]`: the one correction that would need to be
heard immediately is the one never applied mid-passage.

*Offset correction built 2026-09-18, and building it settled how it has to
work.* **Ring depth cannot do it.** For a gapless stream a sample's air time is
fixed by where sample 0 was placed plus the device rate; the next passage is
appended contiguously, so its alignment is inherited and changing the ring's
depth mid-stream shifts nothing. Depth decides alignment at a *fresh start* and
nowhere else, which is why `[LOG-ECHO-030]` holds at a join and why this
correction must move the **transition** instead.

*A first attempt moved the position inside the file — opening the next passage
a few milliseconds further in — and was one-directional, since there is no
negative position to open at.* The knob that actually serves is **where the
incoming passage sits inside the overlap**. Every transition has one, and
starting the passage earlier overlaps a few milliseconds more and catches up,
while starting it later overlaps a few less and waits. Nothing is skipped and
nothing is repeated, so it is inaudible in both directions and symmetric by
construction — there is no negative position, but there is always a slightly
smaller overlap. It is a signed nudge to `should_admit`'s threshold, clamped so
it can neither ask for audio that does not exist nor narrow through zero into a
gap, which would be a worse fault than the offset it was correcting.

**`[GDE-ECHO-345]` The anchor's precision, not the ear's, sets how well this can
work.** The residual is measured from a `DriftAnchor`, whose position comes from
`audible_ms` and therefore carries the output ring's own depth jitter — tens of
milliseconds `[LOG-P4-010]`. The deadband is 40 ms for that reason alone, well
above anything a listener would call aligned. Correcting below the noise is the
hunting `[GDE-ECHO-350]` exists to prevent, so **finer alignment needs a more
precise anchor, not a smaller deadband**: a position derived from the device's
own frame counter, which is sampled atomically with its timestamp and carries no
ring at all `[LOG-P4-010]`. That is the next thing worth building, and until it
exists a following node is audibly following rather than audibly one speaker.

A defect surfaced on the way. The residual compared the *timestamps* of two
anchors, which are never about the same sample — the master's crossed a network
and describes a moment already past. On a quiet network that difference is
mostly transport delay, so the loop would have chased the network. The local
reading is now carried along its own playback to the sample the master named
before the two are compared.

*Rate correction built 2026-09-18.* The trim is applied on the mixer thread,
one frame per interval, in `apply_trim` — which refuses rather than forces when
a block is too small to drop from or has no room for a duplicate, because a
trim skipped now simply happens a few milliseconds later and the schedule is a
rate, not a queue of debts.

**`[GDE-ECHO-355]` The trim runs on a fitted slope, never on the latest
residual.** A single reading carries the anchor's ring jitter `[LOG-P4-010]`,
so a position servo at the 500 us deadband the `Follower` was built with would
fire most seconds in whichever direction the noise last pointed. `RateEstimate`
fits a line across an hour instead, and the interval that slope calls for *is*
the rate limit — there is no second governor to disagree with it.

The window is set by arithmetic rather than taste: the slope's standard error
is `s / (sd(t) * sqrt(n))` for scatter `s` across span `T` with `sd(t) =
T/sqrt(12)`, which at 30 ms sampled twice a second is **5 ppm over ten minutes
and 0.35 ppm over an hour**. Ten minutes measures nothing useful about a 14 ppm
drift. An hour is also about the timescale the rate itself wanders on
`[LOG-P4-130]`, so averaging further would smooth a quantity that has moved.

Anything that *steps* the residual — an offset correction, a rejoin — clears
the window, because a straight line through a step reads as a large rate that
never happened: a 40 ms correction inside a half-hour window fits to about
33 ppm.

**`[GDE-ECHO-341]` Every correction must shrink the error it was given, which
means an offset is bitten off and never escalated.** Found by running two
nodes: `lempiplay3` sat a steady **0.9 s** behind `bose` while correcting
continuously and reducing it not at all.

The loop had two moves and reached for the wrong one. A shift spends the
transition's overlap and always reduces the offset by however much it spends. A
rejoin throws the node's first sample down afresh -- and *inherits whatever
error that placement carries*. When the residual exceeded what one transition
could absorb the loop escalated to a rejoin, the rejoin landed with the same
systematic bias, and the next measurement found the same offset again. It could
run for ever without converging, and did.

So a large offset is now corrected by taking the largest bite the transition
allows and coming back for the rest: 886 ms is two transitions at half a second
each, monotonically. `Rejoin` is reserved for a residual past five seconds,
where the node is probably not playing what it thinks it is and placing the
first sample afresh is the honest answer rather than a smaller correction.

**`[GDE-ECHO-343]` "Is it playing this?" is the wrong question to ask a
follower; "is it coming to this?" is the right one.** The mid-passage join was
guarded on the *currently playing* passage. But `current` is the **audible**
passage, and the output ring is some fifteen seconds deep `[LOG-ECHO-020]`, so
for a long window after a follower admits a passage it still names the previous
one. The master's anchor switches as soon as its own ring drains, which is
sooner.

So at every ordinary transition the guard saw a mismatch and joined into a
passage the node was already flowing into perfectly well -- and every join cuts
the ring and re-imposes the join bias `[GDE-ECHO-342]`. That is how a node held
a steady 0.9 s of lag *through a correction loop that was working*: the loop
shed the offset over a couple of transitions, and the next transition put it
straight back.

The queue is the rest of the answer. A passage already admitted but not yet
audible sits at the front of the published queue, ahead of what is still
merely queued, so asking "playing **or** coming" covers both the ring window
and the ordinary wait. A passage already coming needs no join, only patience.

**`[GDE-ECHO-342]` The join bias itself is real and not yet fixed.** A
commanded start lands late by a variable few hundred milliseconds to a second.
`skip_lead_ms` is compensated `[GDE-ECHO-337]`, but before cutting the ring the
engine synchronously tops the incoming decoder up, and that takes as long as it
takes -- "seconds", on an appliance seeking into a long capture
`[PI-CHR-075]`. A correction loop that always converges makes this survivable
rather than fatal, which is why it is recorded here and not patched in a hurry:
the durable fix is to place the incoming audio at a computed ring depth
`[placement]` rather than to fire early by a constant and hope.

**`[GDE-ECHO-350]` Hysteresis and a rate limit, or the loop will hunt.** Trim
only while the estimated offset exceeds a deadband comfortably larger than the
measurement noise, never more than one frame per correction interval, and never
on an estimate younger than the regression window. A correction loop that reacts
to its own measurement noise produces exactly the slow periodic wobble it was
built to remove.


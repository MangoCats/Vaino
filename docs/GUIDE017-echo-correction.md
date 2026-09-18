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
each, monotonically. `Rejoin` is reserved for a residual past the threshold in
`[GDE-ECHO-344]` -- 1.5 s, lowered from five once it was clear that five
seconds of nudging is forty minutes -- where the node is probably not playing
what it thinks it is and placing the first sample afresh is the honest answer
rather than a smaller correction.

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

**`[GDE-ECHO-347]` The loop's floor was its actuator's quantum, and the
deadband was smaller than it.** Measured on the fleet 2026-09-18, and this is
the number that explains what a listener still hears. After converging from
924 ms in two transitions, the residual sat and dithered:

    +441 -> admitted 440 ms early
     +51 -> admitted  50 ms early
     -43 -> admitted  43 ms late
     -44 -> admitted  43 ms late
     -49 -> admitted  48 ms late
     +43 -> admitted  42 ms early

A correction at *every* transition, each applied, each measuring back the same
size. Not noise -- every one of those figures is one mix chunk. The mixer runs
only with `MIN_SUBMIT` of room, so an incoming passage's first sample lands on
a chunk boundary and admission timing can only shift in steps of **46.4 ms** at
44.1 kHz. The loop had reached its actuator's resolution.

And it could never come to rest, because the deadband it was asked to reach
(40 ms) is **smaller than the smallest step it can take** (46 ms). That is the
same class as `[GDE-ECHO-344]` -- a threshold inside another mechanism's error
band -- with the mechanism being quantisation rather than noise, which is why
no amount of filtering would have found it.

Opening the passage part-way in is not quantised. Skipping `o` of the content
makes everything after it sound `o` earlier, to the millisecond, and a few tens
of milliseconds is inside the lead-in where nothing has begun. So the
correction now splits: **admission for the coarse part, origin for the fine**.
Being late needs only the fine knob. Being early goes back a whole chunk on the
coarse one and pulls the overshoot forward on the fine, since there is no
negative position to open at -- the asymmetry `[GDE-ECHO-340]` first ran into,
now put to work rather than worked around.

**`[GDE-ECHO-349]` Below the mix quantum only the frame trim will do, and it
was already there.** A frame is 23 us and the trim already drops or duplicates
one, inaudibly, on the mixer thread -- it was simply wired to *rate* alone.
Nothing about the actuator is rate-specific: N frames dropped is N/44100
seconds of position.

So the correction now hands over. Above one mix quantum a passage boundary
shifts the whole error at once `[GDE-ECHO-347]`. Below it the coarse step is
larger than the error, and the trim pays the remainder off one frame at a time.
The two share an actuator, so they are summed into a single interval rather
than run as two timers that would double the splice rate and argue about
direction.

The budget is **100 ppm** -- one part in ten thousand, inaudible as pitch by a
wide margin, about 4.4 frames a second so each splice is well clear of the
last, and a whole quantum shed in about eight minutes. Raising it is the
obvious way to converge faster and the obvious way to make it audible; that
wants a listening test rather than an argument.

**`[GDE-ECHO-348]` The endgame needs a measurement finer than the actuator it
drives, which a single reading is not.** One anchor reading carries tens of
milliseconds of ring jitter `[LOG-P4-010]`, so correcting a 10 ms position from
one would be chasing noise -- and the deadband could not be lowered below that
noise, which is why it sat at 40 ms and why 40 ms was smaller than the coarse
knob's own step.

A median over two minutes fixes it. Median, not mean, because the ring's
shortfall is bounded one side and not the other. The window is derived: filtered
noise falls as `1.25 s / sqrt(2T)` while drift accrued *during* the window grows
as `r·T`, and at 30 ms of scatter and 13.92 ppm they cross near 130 s at about
2 ms each. The deadband follows the filter down, from 40 ms to 8 ms. Anything
that steps the residual clears the window, exactly as it clears the rate fit.

**`[GDE-ECHO-351]` A log computed at a different point from the action is not
evidence of the action.** The coarse/fine split `[GDE-ECHO-347]` shipped with
the shift *consumed before the split that used it*, so the fine half was always
zero and **every late correction did nothing for four commits** -- while the
log line, which derived its own split a few lines earlier, dutifully reported
`500 ms into the passage` each time. A listener heard no change, the residual
did not move, and the log said the work was done.

The measurement that exposed it was not the log: `tools/echo_skew.sh` read
483, 544, 555, 512 ms across several minutes while corrections were "applied"
at every transition. A number that refuses to move under treatment is worth
more than a line that says treatment occurred.

The test written afterwards asserts on the **passage's actual origin** rather
than on a log line, and fails with `opened at 0 ms` against the old code. That
is the shape every test here should have taken: `[GDE-ECHO-343]`,
`[GDE-ECHO-344]` and this one all passed their unit tests while the system was
broken, because they asserted on what the code intended rather than on what
reached the audio.

**`[GDE-ECHO-346]` A fitted slope is the error in the correction, not the
drift, and applying it as an absolute settles at half.** The residual being
fitted is what remains *after* the current trim, so a loop that sends the fit
as the whole answer sets the trim to `R - A` when it already holds `A`. The
fixed point is `R/2` and the eigenvalue is -1: it sits at half-correction or
oscillates about it on the fit window's period.

At the +13.92 ppm measured for this pair `[LOG-P4-130]` that leaves ~7 ppm
uncorrected for ever -- **25 ms an hour**, which crosses the 40 ms offset
deadband every hour and a half, fires a shift, and clears the rate window,
starting the whole cycle again. That is a structural error and no amount of
threshold tuning reaches it.

Adding to what is applied is deadbeat in one window, and what remains is the
fit's own error rather than half the drift. The correction is then applied
**once per window and the window cleared**, because the plant's slope has just
stepped and a line across that step is not a slope. Shipped code sent it twice
a second.

**`[GDE-ECHO-344]` Two mechanisms, and the threshold between them must clear
the join bias.** `[GDE-ECHO-343]`'s guard was right about ordinary transitions
and wrong about everything else: suppressing the join whenever the node was
*coming* to that passage also removed the only thing that aligns a node
initially. After a restart a follower resumes its own programme, adopts the
master's queue, flows into the same passages -- and sits however far out it
happened to be, with 500 ms a transition to claw back. Measured at **4.8 s**,
which is forty minutes of nudging; the divergence got worse, not better.

Being *coming to a passage* is therefore only sufficient while the node is
roughly in the right place. Grossly out, it is playing the right passage at the
wrong moment, and only placing the first sample afresh fixes that.

The threshold between the two carries a constraint that is easy to miss: **it
must sit above the join bias.** A join lands a few hundred milliseconds to a
second late `[GDE-ECHO-342]`, so a threshold at or below that has every join
trigger the next one for ever. 1.5 s clears the worst observed bias with room,
so a join always lands *inside* the band and the nudges take it from there --
and is low enough that a node never faces forty minutes of nudging.

**`[GDE-ECHO-342]` The join bias, measured rather than assumed.** *Deferred
through several rounds and fixed 2026-09-18, when it became the whole of what
a listener could hear: a consistent ~900 ms, confirmed by ear, by
`tools/echo_skew.sh` (900/875/967 ms) and by the follower's own residual
(+845, +953) all at once.*

A commanded start goes through `skip`, which applies `skip_lead_ms` -- a
constant -- and then does the work: open the file, seek, build the resampler,
and top the decoder up so the overlay is not silence `[PI-CHR-075]`. That work
lands **directly on the air time**, and firing early by a constant cannot
compensate a variable. The residual of a join therefore *is* its preparation
time, identically; everything else in the arithmetic cancels.

The compensation is now measured. Each join times its own preparation and
folds it into a smoothed estimate, weighted towards history so one slow seek
moves the figure rather than replacing it, and clamped so a pathological join
cannot leave every later one firing seconds early. The first join on a cold
node guesses 400 ms and is wrong once; every join after it uses what this node
actually costs.

This is deliberately *not* the durable fix. That is to derive the placement
from the target air time at the moment the ring is cut -- after the work, not
before -- which `placement()` already computes and nothing calls. It was not
taken here because `cut_ring_to_incoming` is the real-time path every ordinary
user skip also uses, and a self-calibrating constant reaches most of the
benefit without touching it. If the measured preparation turns out to vary
widely *within* a node rather than between nodes, the estimate will not hold
and the durable fix becomes necessary.

**Superseded note:** the join bias itself is real and no longer unfixed. A
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


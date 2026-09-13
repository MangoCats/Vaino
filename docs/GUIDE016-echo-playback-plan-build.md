# GUIDE016: Echo Playback — Plan, Phases 3-6

**Development Guidance — split from [GUIDE009](GUIDE009-echo-playback-plan.md) 2026-09-13**

The phases that **build**, continuing [GUIDE009](GUIDE009-echo-playback-plan.md),
which holds the gating and the three measurement phases. Those are met
`[GDE-ECHO-520]`, so this is the live half of the plan and the half that grows.

> **Related:** [GUIDE009](GUIDE009-echo-playback-plan.md) `[GDE-ECHO-250]` — the gating and phases 0-2 · [GUIDE014](GUIDE014-echo-phase-status.md) `[GDE-ECHO-520]` — where each phase actually stands · [GUIDE010](GUIDE010-echo-node-capabilities.md) `[GDE-ECHO-450]` — the node roster these phases schedule against · [REQ003](spec/REQ003-audio-playback.md) — the buffer-depth rule every correction obeys · [SPEC011](spec/SPEC011-audio-path-supervisor.md) — the device lifecycle that invalidates a frame clock

---

## 1. Phase 3 — The wire

**`[GDE-ECHO-310]` A master broadcasts two kinds of anchor; it sends no
commands.** One kind is not enough, because a backward-looking statement cannot
serve a node whose presentation offset exceeds the master's `[GDE-ECHO-410]`:
such a node would have needed to submit *before the master did*, and learns so
too late to act.

**The schedule, forward-looking.** *Passage P, sample 0, will be heard at wall
time T.* Emitted when the master admits P to the mixer — which by
`[REQ-AUD-160]` and the ring's depth is roughly **15 s before anyone hears it**,
against an A2DP buffer of at most a few hundred ms. That lead is what makes an
arbitrary offset compensable, and it exists only because an echo node holds the
file locally and knows the queue `[GDE-ECHO-420]`. Each node schedules its own
submission at `T − presentation_offset`.

**The drift anchor, backward-looking.** *Sample N of P was heard at T*, repeated
periodically, carrying the master's measured ppm. Computed from `audible_ms`,
never `played_ms`, because those differ by the ring's depth and only one of them
describes sound `[REQ-AUD-164]`.

Cadence follows Phase 0's measured stability; twice a second for the drift
anchor, matching the existing snapshot rate, costs nothing new because it rides
the WebSocket the browser snapshot already uses. The schedule is emitted on
admission rather than on a clock.

**The binding constraint is lead time against offset spread, and it is never
close.** A node can only start as early as its foreknowledge allows; with ~15 s
of lead against a worst-case spread of a few hundred ms, the margin is roughly
sixty-fold. A design that instead treated the master's offset as zero would have
worked in one direction only.

**`[GDE-ECHO-320]` Every message is absolute, idempotent and independently
sufficient, so a lost one is harmless.** No deltas, no sequence-dependent state,
no acknowledgement, no retransmission. An echo node that misses a message simply
uses the next; one that receives a stale message discards it by its own
timestamp. This is what lets `[GDE-ECHO-020]`'s stateless master hold — and it
is the property that makes the transport choice uninteresting, which is the
point.

---

## 2. Phase 4 — Echo, measured but uncorrected

**`[GDE-ECHO-330]` Join only at a passage start, using the seek path that
already exists.** An echo node receiving an anchor for a passage it is not
playing computes the offset it would need and opens that passage there, through
the same `resume_at` path a restart already uses `[REQ-AUD-140]`. Mid-passage
joining was deferred here; `[GDE-ECHO-510]`'s rejoin case promotes it to
required, and it is skip plus a seek, both of which already exist.

Then measure and do not correct. Log the residual offset between the two nodes
continuously across many passages and compare it against Phase 1's predicted
drift.

**Gate.** Observed drift matches prediction within a factor of two. A mismatch
means something in the chain is not understood — and shipping a correction loop
over a misunderstood error is how a system acquires a fault that only appears
after hours, which is the hardest kind to find.

---

## 3. Phase 5 — Correction

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

**`[GDE-ECHO-350]` Hysteresis and a rate limit, or the loop will hunt.** Trim
only while the estimated offset exceeds a deadband comfortably larger than the
measurement noise, never more than one frame per correction interval, and never
on an estimate younger than the regression window. A correction loop that reacts
to its own measurement noise produces exactly the slow periodic wobble it was
built to remove.

---

## 4. Phase 6 — What happens when it goes wrong

**`[GDE-ECHO-360]` The frame clock is invalidated by more than it looks.** Each
of these voids the anchor and must force a rejoin at the next passage boundary
rather than a silent continuation on stale state:

| event | effect |
| :--- | :--- |
| device reopen `[SPEC-APS-010]` | frame counter and stream epoch both reset |
| underrun `[REQ-AUD-142]` | frames that were counted were never heard |
| pause | the device stops; the count stops with it |
| skip | the ring is cut `[REQ-AUD-158]`, so frames counted are discarded |
| master silent | the announced queue runs down, then the warm Director resumes — no timeout `[GDE-ECHO-500]` |

The last row is the important one for `[GDE-ECHO-020]`: an echo node whose
master disappears must return to being an ordinary player, not stop.

---

## 5. How it is tested

**`[GDE-ECHO-370]` Most of it is testable on one machine; the part that is not
must be honest about it.** The regression, the anchor arithmetic, the hysteresis
and every row of `[GDE-ECHO-360]` are unit-testable against a synthetic clock
that can be told to run fast — and should be, because they are the parts where a
sign error is invisible in listening and obvious in a test. What cannot be
faked is two real DACs over a real network, so the acceptance criterion
`[GDE-ECHO-260]` stays an integration measurement on the fleet, and the residual
offset is logged rather than merely observed, so a regression months later has a
baseline to fail against.

---

## 6. Explicitly not in v1

**`[GDE-ECHO-380]` Deferred, with reasons, so they are not mistaken for
oversights.** Bluetooth output as an echo node — deferred pending an
offset-stability measurement, **not excluded by category** `[GDE-ECHO-070]`;
mid-passage joining `[GDE-ECHO-330]`; Windows as an echo node, where the delay
term is an estimate `[GDE-ECHO-180]`; more than one master on a network; any
attempt to sync *volume*, which is a listening control and belongs to each room
`[REQ-AUD-152]`; and any Sendspin interoperation, which remains a separate
question answered on its own terms in
[SPIN001](../sendspin/SPIN001-protocol-and-integration-analysis.md).

---

## 7. Risk register

**`[GDE-ECHO-390]` What would actually sink this, ranked by how early it shows.**

| risk | shows in | if it happens |
| :--- | :--- | :--- |
| A node's ALSA gives no hardware timestamps | Phase 2 | that node cannot echo; `[GDE-ECHO-290]` makes it say so rather than drift silently |
| Wi-Fi cannot hold 1 ms wall-clock agreement | Phase 0 | anchor cadence rises, or wired-only is accepted |
| Measured drift contradicts Phase 1 | Phase 4 | stop; the chain is not understood `[GDE-ECHO-330]` |
| Crystal drifts with room temperature more than expected | Phase 4 | the trim loop absorbs it, which is what a loop is for |
| Correction loop hunts | Phase 5 | deadband widened `[GDE-ECHO-350]` |
| cpal panics on the audio thread | any | `[GDE-ECHO-190]`; do not increase how often that path is called |

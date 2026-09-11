# GUIDE009: Echo Playback — Development Plan

**Development Guidance — planned 2026-09-11, concluding [GUIDE008](GUIDE008-echo-playback-investigation.md)**

The build order for echo playback, with the gate that can stop each phase. The
plan is shaped by one fact from the investigation: the only drift figure in
evidence belongs to one machine `[GDE-ECHO-060]`, so the early phases produce
measurements and the design of the correction is not settled until it has them.
The timebase comes first, because every measurement after it is expressed
against it `[GDE-ECHO-305]`.

> **Related:** [GUIDE008](GUIDE008-echo-playback-investigation.md) — the findings this rests on · [GUIDE010](GUIDE010-echo-node-capabilities.md) — the node model Phase 1 fills in · [REQ003](spec/REQ003-audio-playback.md) — the buffer-depth rule every correction obeys · [SPEC011](spec/SPEC011-audio-path-supervisor.md) — the device lifecycle that invalidates a frame clock · [BOSE004](../BosePi/BOSE004-operating-health.md) — the independent instrument Phase 2 is validated against · [PI026](../VainoPi/PI026-startup-preflight.md) — the preflight shape Phase 0 borrows

---

## 1. How the plan is gated

**`[GDE-ECHO-250]` Six phases, each ending in a measurement that can stop the
project.** No phase begins while the previous one's gate is unmet. Phases 0–2
are useful on their own and leave the player better instrumented even if echo
playback is never built, which is deliberate: the expensive, irreversible work
is deferred behind the cheap, independently valuable work.

**`[GDE-ECHO-260]` "Done" is a sustained measurement, not a demonstration.**
The acceptance criterion for the whole effort is: **two nodes, playing one
programme, holding within 1 ms of each other for 24 unbroken hours across
passage changes, with the residual offset logged throughout.** One ms is chosen
because it is comfortably inside the comb-filtering threshold `[GDE-ECHO-050]`
and comfortably outside what `[GDE-ECHO-110]` predicts is achievable; a target
the prediction only just meets would not distinguish success from luck. A demo
that sounds right for one song proves nothing about drift and must not be
accepted as evidence `[GOV-SRC-020]`.

---

## 2. Phase 0 — The shared timebase

**`[GDE-ECHO-300]` chrony on every node, verified at boot, reported in the same
place as everything else.** Install and configure chrony across the fleet with a
common upstream, then extend the startup preflight `[PI-PRE-010]` with a check
that it is running and converged. What must be recorded is not chrony's own
estimate of its accuracy but the observed offset between nodes over a week —
`[GOV-SRC-020]` again: a daemon's self-report is a claim, not a measurement.

**`systemd-timesyncd` is the default, and it is the wrong tool here** — for a
specific reason rather than a general one: it is an SNTP client that corrects
the *time* without disciplining the *frequency*, which is precisely the half
`[GDE-ECHO-100]` needs. A node left on timesyncd satisfies the wall-clock
requirement and silently fails the frequency one.

Both x86 nodes shipped that way. **`teacherslounge` was moved to chrony on
2026-09-11** — installing it deactivates timesyncd automatically — and within a
minute reported `System time 0.000007303 seconds fast of NTP time`, frequency
0.556 ppm fast, residual −16.08 ppm still settling. That residual is the figure
to watch: it is what Phase 1 would otherwise mistake for DAC drift.
**`smartboardpc` is still on timesyncd** and is not yet a candidate for any
measurement that depends on frequency.

**`[GDE-ECHO-305]` The timebase precedes the measurement, and an earlier
revision of this plan had that backwards.** It listed measurement as Phase 0 and
the timebase as Phase 2, which cannot work: a node's ppm is expressed *against*
the clock discipline in force, so measuring first and installing chrony
afterwards invalidates the measurement without changing the hardware. The DAC
would not have moved; the ruler would.

Observed directly on `smartboardpc`, minutes apart: residual frequency
**+305.427 ppm, then −213.530 ppm, with skew still at 10⁶ ppm** — chrony's way
of saying it has no estimate yet. Any ppm figure taken in that window measures
chrony, not the hardware. The fleet must therefore be **entirely on chrony and
settled** before Phase 1 begins, and a mixed fleet — some nodes on chrony, some
on timesyncd — cannot be compared across at all `[GOV-SRC-020]`.

**Gate.** Node-to-node wall-clock agreement within 1 ms, sustained, across a
reboot of each node and across a Wi-Fi reconnect. If Wi-Fi proves too unstable
to hold that, the finding belongs in this document before any wire format is
designed, because it changes how often the anchor in Phase 3 must be resent.

---

## 3. Phase 1 — Measure every candidate node

**`[GDE-ECHO-270]` A per-node ppm campaign, using the instrument that already
exists, before any player code changes.** For each node that might participate,
read the running stream's frame counter against monotonic time exactly as
`[BOS-OPS-020]` did — the card found **by name**, per `[BOS-PWR-050]` — at two
points at least 24 hours apart, and compute ppm from the difference. Record
ambient temperature at both readings, because a crystal moves with it and a
single-temperature figure is what `[GDE-ECHO-060]` warns against.

Each node must yield **both halves of `[GDE-ECHO-400]`'s model**, not ppm
alone: its rate error, its presentation offset, whether that offset holds across
a device reopen and a reconnect, its clock ownership, its timestamp source, and
whether it has the library's native rate. An earlier revision of this phase
measured rate only, which would have left the offset column empty for every
node and the Bluetooth question unanswerable.

The roster is five nodes and one measurement `[GDE-ECHO-450]`. Rate error is
obtained by regression against the shared timebase; offset by ALSA's reported
delay plus a calibrated residual `[GDE-ECHO-430]`, with the acoustic
cross-correlation of `[GDE-ECHO-460]` as the independent check that ranks the
rest `[GOV-SRC-020]`.

**Gate.** Two or more wired nodes measured below 2 ppm relative: proceed as
planned. Any intended node above 20 ppm: proceed, but Phase 5 becomes mandatory
rather than optional, and `[GDE-ECHO-110]`'s "per-passage resync would have been
enough" conclusion is void for that pair. A node that cannot be measured at all
is not a candidate.

---

## 4. Phase 2 — The frame clock

**`[GDE-ECHO-280]` Count frames where they leave for the device, and nowhere
else.** In `player/src/output.rs`, `fill` gains three atomics alongside the
existing `Counts`: frames emitted since stream start, the disciplined wall-clock
reading taken in the same callback, and the ALSA delay as reported by
`playback − callback` `[GDE-ECHO-160]`. The callback's existing contract is
unchanged and non-negotiable — no allocation, no blocking, no lock — so these
are relaxed atomic stores of values already in hand, and the regression that
turns them into a rate estimate happens on an ordinary thread.

The callback signature changes from `move |out, _|` to bind the
`&OutputCallbackInfo` it currently discards `[GDE-ECHO-140]`. Only the delay
difference is read from it; the absolute instants are not consumed
`[GDE-ECHO-200]`.

**`[GDE-ECHO-290]` Publish whether the timestamps are real, and treat "unknown"
as unknown.** cpal silently substitutes a software clock when hardware
timestamps are unavailable `[GDE-ECHO-170]`, and offers no way to ask which
happened. Detect it by observation rather than by interrogation: if the reported
delay never varies across many callbacks, it is not coming from hardware.
Publish the verdict as a three-state fact — hardware, software, undetermined —
surfaced the way every other audio-path fact already is `[REQ-VIS-250]`. A node
reporting anything but *hardware* is not eligible to echo, and says so. Absent
is not zero `[GOV-SRC-040]`.

**Gate.** The in-process frame clock agrees with `/proc/asound`'s independent
counter within 1 ppm over 24 hours on `bose`. This is the point of building it
here rather than in Phase 3: two instruments, different mechanisms, one answer —
the same standard `[BOS-OPS-020]` held itself to. Disagreement means the new
instrument is wrong, and it is far cheaper to learn that now than to debug it
later through a network.

---

## 5. Phase 3 — The wire

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

## 6. Phase 4 — Echo, measured but uncorrected

**`[GDE-ECHO-330]` Join only at a passage start, using the seek path that
already exists.** An echo node receiving an anchor for a passage it is not
playing computes the offset it would need and opens that passage there, through
the same `resume_at` path a restart already uses `[REQ-AUD-140]`. Mid-passage
joining is explicitly out of scope for v1: it is the case where the ring's depth
is hardest to reason about, and nothing is learned by attempting it first.

Then measure and do not correct. Log the residual offset between the two nodes
continuously across many passages and compare it against Phase 1's predicted
drift.

**Gate.** Observed drift matches prediction within a factor of two. A mismatch
means something in the chain is not understood — and shipping a correction loop
over a misunderstood error is how a system acquires a fault that only appears
after hours, which is the hardest kind to find.

---

## 7. Phase 5 — Correction

**`[GDE-ECHO-340]` Rate and offset are different errors, corrected in different
places, and separating them is what makes the ring's depth stop mattering.** A
*rate* error accumulates slowly and is corrected by trimming: drop or duplicate
one frame on submission into the output ring, on the mixer thread, at whatever
interval the measured ppm calls for — roughly one frame per minute at 0.4 ppm.
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

## 8. Phase 6 — What happens when it goes wrong

**`[GDE-ECHO-360]` The frame clock is invalidated by more than it looks.** Each
of these voids the anchor and must force a rejoin at the next passage boundary
rather than a silent continuation on stale state:

| event | effect |
| :--- | :--- |
| device reopen `[SPEC-APS-010]` | frame counter and stream epoch both reset |
| underrun `[REQ-AUD-142]` | frames that were counted were never heard |
| pause | the device stops; the count stops with it |
| skip | the ring is cut `[REQ-AUD-158]`, so frames counted are discarded |
| master silent > 2 anchors | echo node resumes its own Director at the next boundary |

The last row is the important one for `[GDE-ECHO-020]`: an echo node whose
master disappears must return to being an ordinary player, not stop.

---

## 9. How it is tested

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

## 10. Explicitly not in v1

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

## 11. Risk register

**`[GDE-ECHO-390]` What would actually sink this, ranked by how early it shows.**

| risk | shows in | if it happens |
| :--- | :--- | :--- |
| A node's ALSA gives no hardware timestamps | Phase 2 | that node cannot echo; `[GDE-ECHO-290]` makes it say so rather than drift silently |
| Wi-Fi cannot hold 1 ms wall-clock agreement | Phase 0 | anchor cadence rises, or wired-only is accepted |
| Measured drift contradicts Phase 1 | Phase 4 | stop; the chain is not understood `[GDE-ECHO-330]` |
| Crystal drifts with room temperature more than expected | Phase 4 | the trim loop absorbs it, which is what a loop is for |
| Correction loop hunts | Phase 5 | deadband widened `[GDE-ECHO-350]` |
| cpal panics on the audio thread | any | `[GDE-ECHO-190]`; do not increase how often that path is called |

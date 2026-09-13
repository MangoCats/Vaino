# GUIDE014: Echo Playback — Phase Status and the Revised Critical Path

**Development Guidance — status as of 2026-09-13, revising [GUIDE009](GUIDE009-echo-playback-plan.md)**

[GUIDE009](GUIDE009-echo-playback-plan.md) is the plan and stays the plan. This
file is the status board it deliberately is not, written because three of its
six gates have now been answered and one of the answers **reorders the
remaining work**.

> **Related:** [GUIDE009](GUIDE009-echo-playback-plan.md) `[GDE-ECHO-250]` — the six phases and their gates · [LOG007](LOG007-drift-instrument-correction.md) `[LOG-FIX-030]` — the corrected drift figures this rests on · [GUIDE010](GUIDE010-echo-node-capabilities.md) `[GDE-ECHO-450]` — the node roster · [GUIDE013](GUIDE013-audio-stack-reporting.md) — how cpal reports timestamps

---

## 1. Where the phases stand

**`[GDE-ECHO-520]` Three gates answered, one of them twice.**

| phase | gate | status |
| :--- | :--- | :--- |
| 0 — shared timebase | node-to-node within 1 ms | **met**; fleet entirely on chrony, `smartboardpc` serving |
| 1 — measure every node | rate *and* offset per node | **rate half met, offset half blocked** |
| 2 — the frame clock | agrees with `/proc` within 1 ppm | **met at +0.33 ppm** `[LOG-FIX-050]` |
| 3 — the wire | — | not started, and **should not start yet** |
| 4 — echo uncorrected | drift matches prediction ×2 | not started |
| 5 — correction | — | not started, now **mandatory** `[LOG-FIX-040]` |
| 6 — failure handling | — | designed, not built |

Phase 0 completed when `bose` joined chrony on 2026-09-12 through
`overlayroot-chroot` `[IMPL-BOS-180]`. Both measured nodes are converged:
`bose` 1.0 ms fast of NTP with 5.5 ppm skew, `smartboardpc` 0.2 ms with 1.9.

Phase 2's gate appeared to fail by 13.19 ppm and did not. The reference was
circular, not the instrument `[LOG-FIX-010]`. The gate still earned its keep —
it caught a bad measurement before anything was designed on it, which is what
`[GDE-ECHO-280]` built it for — just not the bad measurement it was aimed at.

**Phase 1's rate half is answered and its gate fell in the middle.** No pair is
under 2 ppm relative and no node is over 20 ppm absolute: `bose` ≈+14,
`smartboardpc` +9.96, `vainopi` −2.09 `[LOG-FIX-030]`. Neither branch of the
gate applies cleanly, and the conclusion is the stricter one — **Phase 5 is
mandatory for every pair**, because no node is sub-ppm and clock ownership does
not predict rate accuracy `[LOG-FIX-040]`.

---

## 2. The blocker: the offset instrument reads zero

**`[GDE-ECHO-530]` Presentation offset is unmeasured on every node, and the
instrument for it is broken rather than merely absent.** The model in
`[GDE-ECHO-400]` has two halves, rate and offset. Only rate is in hand.

On `bose`, across every `clock:` line since the frame clock shipped, the delay
term reads **0**. The kernel, on the same PCM at the same time, does not:

| | |
| :--- | :--- |
| device the player opens | `hw:CARD=sndrpihifiberry,DEV=0` — raw hw, no `plug`, no `dmix` |
| `/proc` `delay`, sampled 8× over 3 s | 2760, 3048, 3320, 3572, 3588, 3860, 4124, 4396 |
| `delay + avail` | **4412 every time** — the ring, 100 ms |
| what the player records | `delay=0`, unchanged across 1.7 M callbacks |

So the quantity exists, is live, and swings ~1650 frames across each period
cycle. The player's path to it — cpal's `playback − callback`, which is
`status.get_delay()` converted to a duration `[GDE-ECHO-160]` — returns
nothing. The cause is not yet known and is not worth guessing at: the
plugin-chain explanation is already ruled out by the device name.

**`[GDE-ECHO-535]` The eligibility rule is therefore firing on a broken input,
and would disqualify the master.** A node whose reported delay never varies is
using software timestamps and may not echo, per `[GDE-ECHO-290]`. `bose`'s
delay never varies *as the player sees it*, so `bose` — the self-clocked I²S
node the whole design treats as the reference — currently reports `Software`
and is ineligible. The rule is right. Its input is wrong, and the kernel
evidence above suggests `bose` will classify as `Hardware` once the read is
fixed.

Absent is not zero `[GOV-SRC-040]`, and this is the case that principle was
written for: a constant 0 is indistinguishable from a real measurement of no
delay, and only the cross-check against `/proc` separated them.

---

## 3. The revised critical path

**`[GDE-ECHO-540]` Phase 2b comes before Phase 3, because the wire carries a
term nobody can currently measure.** Phase 3's forward schedule has each node
submitting at `T − presentation_offset` `[GDE-ECHO-310]`. With the offset
unmeasurable, the wire format could be built and even tested, but every node
would schedule against an assumed zero — which is exactly the "treated the
master's offset as zero" design that `[GDE-ECHO-410]` rejects, arrived at by
accident instead of by choice.

**Phase 2b — make the offset measurable.** Three routes, in the order they
should be tried:

1. **Find out why `get_delay()` yields 0 through cpal on a raw hw device.**
   Cheapest, and it fixes the reported verdict as a side effect. A short probe
   linked against the same `alsa` crate cpal uses, opening the same device and
   printing `status.get_delay()`, separates "the kernel will not tell cpal"
   from "cpal will not tell us" in a single run.
2. **Read `/proc` `delay` directly** as a documented fallback, ranked below the
   in-process read and *visible as a fallback* rather than silently equivalent
   `[GOV-SRC-040]`. It is a different process reading a different instant, so
   it is worse — but it is not zero.
3. **Acoustic calibration** `[GDE-ECHO-460]`. Already proven on this fleet:
   `teacherslounge`'s microphone gave `vainopi` −2.089 ppm ±0.477 from 130 s of
   clicks `[LOG-DRIFT-062]`, and ±0.10 ppm at ten minutes. It measures the
   whole path to the air, which is the quantity that actually matters, and it
   is the only route that works for `vainopi`, whose A2DP output has no
   `hw_ptr` at all `[GDE-ECHO-070]`. `tools/click_emit.py` and
   `tools/click_analyze.py` carry it, and `tools/test_click_analyze.py` checks
   the analyser against recordings whose answer is known -- including the
   spurious-edge-detection case that produced `[LOG-DRIFT-064]`'s
   +12,574 ppm.

Route 3 is not merely a fallback. It is the independent check that ranks the
other two `[GOV-SRC-020]`, and the offset question — whether a node's offset
holds across a device reopen and a Bluetooth reconnect `[GDE-ECHO-420]` — is
answerable *only* acoustically for the one node where it is most in doubt.

---

## 4. What the corrected drift changes downstream

**`[GDE-ECHO-550]` The trim loop's cadence was computed from the discredited
figure and is roughly forty times too slow.** The correction in
`[GDE-ECHO-340]` is sized at "roughly one frame per minute at 0.4 ppm". At the
drift actually measured:

| pair | relative | one trimmed frame every |
| :--- | ---: | ---: |
| `bose` ↔ `smartboardpc` | ~4 ppm | 5.7 s |
| `bose` ↔ `vainopi` | ~16 ppm | 1.4 s |

A single dropped or duplicated frame once a minute is inaudible by inspection.
**Once every 1.4 s is not obviously inaudible**, particularly on sustained
tonal material, and the plan should stop assuming it is. This does not change
the *architecture* — rate is still a slope corrected on submission, offset
still a position corrected at passage boundaries `[GDE-ECHO-340]` — but it
turns "drop a frame" into a choice between frame trimming and fractional
resampling, to be settled with a listening test in Phase 5 rather than by
assertion now.

The deadband in `[GDE-ECHO-350]` also needs a real number rather than an
adjective. The measurement noise it must sit outside is now known: hourly
scatter of 2.2 ppm on the frame clock, against 9.8 ppm for a `/proc` read
paired with a separately-read clock `[LOG-FIX-070]`.

---

## 5. What did not change

**`[GDE-ECHO-560]` The design survives its own worst measurement, which is the
useful thing to record.** None of the following moved:

- The two-message wire `[GDE-ECHO-310]` — forward schedule plus backward drift
  anchor — and the ~15 s of lead that makes an arbitrary offset compensable.
  The margin was sixty-fold against offset spread and still is; the correction
  changed rate, not lead.
- Absolute, idempotent, independently sufficient messages `[GDE-ECHO-320]`.
- The rejoin and going-independent semantics of `[GDE-ECHO-500]` and
  its `[GDE-ECHO-510]` companion.
- The buffer-depth discipline `[REQ-AUD-164]` and the reasons a frame clock is
  invalidated `[GDE-ECHO-360]`.
- The acceptance criterion `[GDE-ECHO-260]`: two nodes within 1 ms for 24
  unbroken hours. At ~4 ppm relative that is 4 µs/s of accumulation, so the
  criterion is unchanged and the trim loop is what meets it.

The one conclusion that inverted — `bose` as a clean sub-ppm reference — was
never load-bearing for the architecture, only for the question of whether
correction could be skipped. It cannot `[LOG-FIX-040]`.

---

## 6. Next, in order

**`[GDE-ECHO-570]` Four things, and the first is small.**

1. **Probe `get_delay()` on `bose`** against the same `alsa` crate cpal uses,
   on the same device. One binary, one run, and it decides between routes 1
   and 2 of `[GDE-ECHO-540]`.
2. **Fix or replace the delay read**, then let the verdict in `[GDE-ECHO-290]`
   re-run. Expect `bose` to become `Hardware`.
3. **Fill the offset column** of `[GDE-ECHO-450]` for `bose` and
   `smartboardpc` from the repaired instrument, and for `vainopi`
   acoustically — a 10-minute run with the Middleton beside
   `teacherslounge`'s microphone, which also calibrates the laptop's ADC and
   makes `vainopi`'s −2.089 absolute.
4. **Then Phase 3.** Not before: a wire built against an assumed-zero offset is
   the one design `[GDE-ECHO-410]` explicitly rejects.

A second drift window at a different ambient temperature `[LOG-DRIFT-060]` runs
free alongside all of this, since the sampler is already appending.

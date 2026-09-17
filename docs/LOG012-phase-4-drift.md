# LOG012: Phase 4's Drift, From the Player's Own Clock

**Experiment Record — 2026-09-17, in progress**

`[GDE-ECHO-330]`'s gate asks that observed drift match Phase 1's prediction
within a factor of two. This is that measurement, taken from the instrument the
player already carries rather than from anything built for the occasion.

> **Related:** [GUIDE016](GUIDE016-echo-playback-plan-build.md) `[GDE-ECHO-330]` — the gate this serves · [LOG007](LOG007-drift-instrument-correction.md) `[LOG-FIX-030]` — the electrical figure · [LOG008](LOG008-acoustic-calibration.md) `[LOG-CAL-110]` — the acoustic one · [LOG011](LOG011-the-lead-is-a-ring.md) `[LOG-ECHO-020]` — the ring this deliberately avoids

---

## 1. Why not the echo anchor

**`[LOG-P4-010]` The obvious instrument is the wrong one: the published anchor
carries the ring's jitter.** `DriftAnchor.sample` is derived from `audible_ms`,
which is `frames_mixed` less what is still sitting in the output ring. The ring
hovers near capacity but fluctuates by roughly a mixer chunk, and
`[LOG-ECHO-020]`'s admission leads scatter across 14.93–15.046 s — so the anchor
inherits tens of milliseconds of noise that has nothing to do with the clock.
Regressing it would need thousands of passages to average that away.

The `clock:` journal line has no such problem. `frames` and `at_nanos` are
stored together **inside one audio callback**, so the pair is near-atomic and
carries neither ring depth nor read skew `[LOG-FIX-070]`. It has been emitted
every five minutes on every node all along, which means the first result needed
no new code and no waiting.

`tools/drift_analyze.py --clock` reads those lines. It segments wherever the
counter stops being a clock — a device reopen resets `frames` to zero, a pause
stops it while wall time continues — because averaging through either invents a
rate error that did not happen `[SPEC-APS-010]`.

## 2. `bose`, 14.9 hours

| | |
| :--- | ---: |
| samples | 180, one unbroken segment |
| span | 53 701 s (14.9 h) |
| **rate error** | **+13.365 ppm** |
| hourly windows | n=15, mean +13.346, sd 2.157, range +9.91 … +16.95 |

**`[LOG-P4-020]` Three instruments sharing no code now agree on `bose` at about
+13 ppm.** The endpoint and least-squares fit agree to under 1 ppm, so the rate
is constant across the window:

| instrument | figure |
| :--- | ---: |
| `/proc` two-read, electrical `[LOG-FIX-030]` | +13.7 (range 12.97–14.33) |
| click track through the ADC `[LOG-CAL-110]` | +12.3 ± 0.2 |
| player's own frame clock, this run | **+13.365** |

The hourly sd of 2.157 ppm also reproduces `[LOG-FIX-030]`'s 2.87 from a
different instrument, which is the more useful half of the agreement: it says
the hour-to-hour wander is the clock's own and not an artefact of how it is
being watched. Any single hour is worth ±2 ppm and no more `[LOG-FIX-070]`.

## 2a. `lempiplay3`, 3.9 hours, and the pair

| | endpoint | **least squares** |
| :--- | ---: | ---: |
| `bose`, 15.0 h, n=181 | +14.000 | **+13.669 ppm** |
| `lempiplay3`, 3.9 h, n=46 | −0.262 | **+0.168 ppm** |
| **pair** | | **+13.50 ppm** |

**`[LOG-P4-060]` The pair is +13.50 ppm — one trimmed frame every 1.68 s.**
`lempiplay3`'s frame clock is essentially nominal, which is the whole of the
difference: `bose`'s HiFiBerry runs +13.7 and the Pi's own analog output runs
within a fifth of a ppm of rate. The pair figure is therefore almost entirely
`bose`'s crystal.

**`[LOG-P4-065]` Report the fit, not the endpoint, and this tool now prints
both.** The endpoint estimator uses two samples and inherits whatever those two
were doing. Adding a single sample to `bose`'s 15 h moved it from +13.365 to
+14.000 — 0.64 ppm from one reading — while the fit stayed near +13.7. The
tool showed only the endpoint unless the two disagreed by more than 1 ppm, which
hid the more robust number in exactly the case where nothing looks wrong.
`bose`'s fit of +13.669 now sits on top of the electrical +13.7 `[LOG-FIX-030]`.

**`[LOG-P4-070]` Short windows are worth far less than they look, and the error
bars quoted for them here were themselves too tight.** Three readings of the
same `lempiplay3` clock:

| window | figure |
| :--- | ---: |
| one 5-minute interval | −17.3 ppm |
| 45 minutes | +3.7 ppm |
| 3.9 hours | **+0.2 ppm** |

Nothing changed but the window. The 45-minute figure was published above with
an estimated ±3 ppm; it moved 3.5 ppm, so that bar was optimistic rather than
conservative. `lempiplay3`'s hourly sd is **4.270 ppm**, nearly twice `bose`'s
2.539, so this node needs several hours before a figure means anything. The
`frames`/`at_nanos` pair carries no read skew `[LOG-FIX-070]`, but
millisecond-scale jitter in where the clock is read within a callback is 20 ppm
over 300 s.

## 2b. The delay is where the two nodes actually differ

**`[LOG-P4-080]` `lempiplay3`'s reported delay varies by 9 ms; `bose`'s varies
by 0.09 ms.** Same field, same code, same five-minute cadence:

| node | n | mean | spread | sd |
| :--- | ---: | ---: | ---: | ---: |
| `bose`, HiFiBerry I²S | 181 over 15.0 h | 2042.9 | **4 fr (0.09 ms)** | 0.5 |
| `lempiplay3`, bcm2835 analog | 49 over 3.9 h | 1861.4 | **413 fr (9.37 ms)** | 108.1 |

This matters more than either rate figure. `[GDE-ECHO-420]` makes *variability*,
not magnitude, the disqualifying property, and the working assumption has been
that wired means stable and Bluetooth means variable. A wired analog output with
a hundred times `bose`'s delay variability does not fit that split, and 9 ms is
far above `[SPEC-DLY-020]`'s 1 ms control step — it is audible as image smear
between two speakers in one room, not a rounding concern.

It also bears on `[SPEC-DLY-040]`: on this node the "known delay" is a
distribution rather than a number, so a default taken from one reading would
store whatever the device happened to report that second.

**`[LOG-P4-085]` Four hours did not shrink it.** At 49 samples the spread is
413 frames against 13 samples' 400, and the sd only fell from 128.8 to 108.1.
This is the node's behaviour, not a startup transient, and it is the finding
that most threatens `bose`↔`lempiplay3` as an echo pair: trimming corrects a
*slope* `[GDE-ECHO-340]`, and 9 ms of delay wander is not one.

**`[LOG-P4-090]` The variation is structured, not noise, and 13 samples is too
few to say what it is.** The sequence runs 1639, 1655, 1670, 1686, 1704, 1702 —
a near-linear ramp of about 3 frames per minute — and then jumps to 1948. A ramp
followed by a step is the signature of something accumulating in a buffer and
being resynchronised, not of a noisy measurement. Worth a longer window before
anyone theorises; recorded here so the shape is not lost.

## 3. `lempiplay3` — accumulating

Restarted 2026-09-17 19:25 onto continuous power. Its journal accrues a `clock:`
line every five minutes with no intervention, so this section is a matter of
waiting rather than running anything.

**`[LOG-P4-030]` Its first two minutes are not usable and the tool drops them
for the right reason.** `lempiplay3` has no RTC `[GDE-ECHO-365]`, so it booted
with a restored Sep 15 time and NTP stepped it about two days forward 130 s in.
The `clock:` lines from before the step carry `at_nanos` two days behind those
after it. Because the tool reads `at_nanos` from the line rather than the
journal's own timestamp, the step appears as an enormous gap with almost no
frames in it, fails the implied-rate window, and becomes a segment boundary.
That is the correct handling, arrived at for a general reason rather than this
one.

## 4. Open

**`[LOG-P4-040]` The pair figure is the point, and it still needs
`lempiplay3`'s hours — 45 minutes is a first reading, not a gate.** Each node's rate here is measured against its own NTP-disciplined
system clock; both discipline to the same sources, so their *rate* agreement is
far below a ppm and differencing the two figures gives the relative drift echo
actually has to correct. Phase 1 predicts 7–14 ppm for `bose`↔`vainopi`
`[LOG-CAL-030]`, but nothing predicts `bose`↔`lempiplay3`: that pair has never
been measured, and this run establishes it for the first time.

**`[LOG-P4-050]` `bose`'s journal held 14.9 h against an uptime of 40 h.** The
segment is clean and the fit is sound, so this does not affect the figure, but
the missing 25 h were not explained — journal rotation and a `vaino` restart
inside the boot are both candidates and were not distinguished. It matters only
if a future run expects a full boot's worth of data to be there.

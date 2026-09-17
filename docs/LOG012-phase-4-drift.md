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

**`[LOG-P4-040]` The pair figure is the point, and it needs `lempiplay3`'s
hours.** Each node's rate here is measured against its own NTP-disciplined
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

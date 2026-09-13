# LOG008: Hearing `bose` Through `teacherslounge`

**Experiment Record — 2026-09-13**

Ten minutes of `bose`'s DAC recorded by `teacherslounge`'s microphone, to put a
second and independent instrument on the drift figures and to calibrate the
laptop's ADC — the reference `vainopi`'s only measurement was taken against
`[LOG-DRIFT-062]`.

> **Related:** [LOG007](LOG007-drift-instrument-correction.md) `[LOG-FIX-030]` — the electrical figure this is compared with · [LOG006](LOG006-echo-drift-measurement.md) `[LOG-DRIFT-062]` — `vainopi`'s run against the same microphone · [GUIDE014](GUIDE014-echo-phase-status.md) `[GDE-ECHO-540]` — the phase this serves

---

## 1. What was run

`vaino` stopped on `bose` (the `hw:` device is exclusive, and a resampler
anywhere in the chain would make this a measurement of the resampler);
`tools/click_emit.py`'s track played with `aplay` straight to
`hw:CARD=sndrpihifiberry,DEV=0` at the library's own 44100; `arecord` on
`teacherslounge` at `hw:0,0`, 48000, likewise raw. 600 clicks, one per second,
each a 12 ms Hann-windowed chirp from 1 to 6 kHz.

**`[LOG-CAL-010]` The first attempt heard nothing usable, and the instrument
said so instead of producing a number.** Two verification runs before the real
one: the first found 14 detections and no credible run at all, the second 5.
`bose` was already at 0 dB on both its mixers, so the only lever was click
energy — the chirp went from 4 ms at 0.5 amplitude to 12 ms at 0.9, about
+10 dB. That this failed loudly rather than quietly is the whole design
`[GOV-SRC-020]`: a fitted line through 14 spurious detections would have looked
like a measurement.

## 2. Result

| | |
| :--- | ---: |
| detections | 558 of ~615 |
| longest credible run | **426 consecutive, 425 s** |
| discarded outside the run | 132 |
| fitted spacing | 0.999988616 s |
| residual | **6.5 µs rms**, worst 13.9 µs |
| **`bose` − `teacherslounge` ADC** | **+11.384 ppm ± 0.003** |

**`[LOG-CAL-020]` On clean data the span estimator agrees, which is how you can
tell the data is clean.** It reads +11.389 against the fit's +11.384. On the
29 s verification run it read +6.733 against +10.209 — a 3.5 ppm disagreement
over a short window with a marginal signal. The two estimators converging is
evidence; either one alone is not `[LOG-DRIFT-064]`.

## 3. What this gives directly

**`[LOG-CAL-030]` `bose` ↔ `vainopi` is +13.47 ppm ± 0.48, and the microphone
cancels out of it.** Both nodes were measured against the *same* ADC, so
subtracting the two readings removes it entirely:

    bose − ADC        = +11.384 ± 0.003   (this run)
    vainopi − ADC     =  −2.089 ± 0.477   `[LOG-DRIFT-062]`
    bose − vainopi    = **+13.47 ± 0.48**

This is the assumption-free number, and it is the one echo actually needs: a
node pair's relative rate. It supersedes `[GDE-ECHO-550]`'s estimate of ~16 ppm
for this pair, which was arithmetic on two absolutes rather than a measurement
of the difference. One trimmed frame every **1.7 s**, not 1.4.

## 4. What it gives only with an assumption

Combined with the electrical figure for `bose` `[LOG-FIX-030]`:

    teacherslounge ADC = 14 − 11.384  ≈ **+2.6 ppm**  (± 0.7, all of it bose's)
    vainopi, absolute  = −2.089 + 2.6 ≈ **+0.5 ppm**  (± 0.9)

**`[LOG-CAL-040]` This does not by itself settle whether `bose` is +14 or
+0.43, and should not be reported as though it does.** An acoustic run compares
two clocks and has no third to appeal to. It is consistent with +14 if the
laptop's ADC is +2.6 ppm, which is an unremarkable figure for a consumer
codec — but it is equally consistent with the discredited +0.43 if that ADC is
−11 ppm, which is also within what a consumer crystal is quoted at. The
electrical argument in `[LOG-FIX-010]` is what rules the old figure out; this
run neither adds to nor subtracts from it.

What would settle it is in `[LOG-CAL-050]` below.

## 5. Open

**`[LOG-CAL-050]` Measure `teacherslounge`'s ADC electrically and every figure
here becomes absolute.** It is an ordinary Linux box with chrony and a hardware
capture PCM, so the two-read method `[LOG-DRIFT-015]` applies unchanged —
`hw_ptr` on `pcm0c` against `/proc/uptime`, through `tools/drift_analyze.py`.
That yields the ADC against NTP-disciplined time, and then this run's +11.384
gives `bose` acoustically, independently of the frame clock and of `/proc` on
`bose`. Two instruments sharing no code and no machine, which is the standard
`[GDE-ECHO-280]` was written to hold things to.

It needs a capture stream held open for hours on a laptop that sleeps, so it is
worth starting deliberately rather than opportunistically.

**`[LOG-CAL-060]` 132 detections fell outside the clean run.** The fit is
sound — 426 consecutive at 6.5 µs — but a fifth of the track was disturbed,
and the cause was not investigated. Room noise during the run is the obvious
candidate. It matters only if a future run needs the full window.

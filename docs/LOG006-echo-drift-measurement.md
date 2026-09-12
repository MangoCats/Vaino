# LOG006: Echo Playback — the Phase 1 Drift Campaign

**Experiment Record — opened and first two nodes read, 2026-09-12**

`[GUIDE009]`'s Phase 1 `[GDE-ECHO-270]`: measure each candidate node's clock
rate against the shared timebase, now that Phase 0's chrony is settled
`[GDE-ECHO-305]`. This file holds the **t₀ baselines**, so the campaign
survives the session that started it — a reading whose starting point lives
only in someone's scrollback is not a measurement.

> **Related:** [GUIDE009](GUIDE009-echo-playback-plan.md) — the phase this serves · [GUIDE010](GUIDE010-echo-node-capabilities.md) `[GDE-ECHO-450]` — the roster being filled in · [BOSE004](../BosePi/BOSE004-operating-health.md) `[BOS-OPS-020]` — the instrument, and the prior figure this supersedes

---

## 1. Two methods, because the nodes report differently

**`[LOG-DRIFT-010]` A node running Vaino can be read from one sample; a node
under `aplay` cannot.** Vaino opens ALSA through cpal, which calls
`set_tstamp_mode(true)` `[GDE-ECHO-150]`, so `pcm0p/sub0/status` carries a
populated `tstamp` and `trigger_time` — the window is self-contained in a
single read. `aplay` does not enable timestamping: its `tstamp` reads
`0.000000000`, confirmed on `smartboardpc` 2026-09-12.

So:

| node kind | method |
| :--- | :--- |
| running Vaino (`bose`) | one read: `(hw_ptr / rate) − (tstamp − trigger_time)` |
| `aplay` probe (`smartboardpc`) | two reads: `(Δhw_ptr / rate) − Δuptime` |

**`[LOG-DRIFT-015]` The delta method is not merely a fallback — for a probe it
is the correct one.** `aplay` prefills its buffer before the stream triggers,
so `hw_ptr` counts frames not yet heard and the trigger-anchored form reads
absurdly high on a short window (+1530 ppm at 18 s on Smart, which is prefill,
not drift). Differencing two reads cancels the prefill exactly.

**`[LOG-DRIFT-020]` What voids a reading.** Any stream restart, device reopen,
underrun or reboot resets `trigger_time`/`hw_ptr`. `bose` rebooted four times
on 2026-09-11, so its window starts after the last of them. Check `state:
RUNNING` and an unchanged `trigger_time` before trusting a second read.

---

## 2. t₀ baselines

**`[LOG-DRIFT-030]` `bose` — Vaino's own stream, card resolved by name
`[BOS-PWR-050]`.**

```
2026-09-12T00:17:01Z   card 1 (sndrpihifiberry)   state: RUNNING   owner: vaino
trigger_time: 5072.382283402
tstamp      : 7472.805062731
hw_ptr      : 105858716        rate 44100
temp        : 60.3'C           chrony residual: +0.016 ppm
```

Sanity estimate over the 40 minutes elapsed so far: **+0.675 ppm**
(quantisation 0.0094 ppm, so not the limit). Recorded as a plausibility check,
**not as the measurement** — it is roughly double `[BOS-OPS-020]`'s 0.35 ppm,
which is itself superseded because that figure was taken against a
`systemd-timesyncd` baseline `[GDE-ECHO-050]` and `bose` moved to chrony on
2026-09-11. The ruler changed; the DAC did not.

**`[LOG-DRIFT-040]` `smartboardpc` — a silent ALSA-direct probe, deliberately
not PipeWire.**

```
2026-09-12T00:19:37Z   card 1 (ATE1133)   state: RUNNING
uptime_before: 9463.50
hw_ptr       : 879744          rate 48000
uptime_after : 9463.51         (0.01 s read gap)
chrony residual: -0.001 ppm
```

Started with `aplay -D hw:ATE1133,0 -f S16_LE -r 48000 -c 2 /dev/zero`, which
is digital silence — the speaker stays quiet. **To stop it:**
`pkill -f "aplay -D hw:ATE1133"`.

Card **1** on purpose. PipeWire idles a stream on card 0 (the Burr-Brown),
which is not the device feeding the speaker `[SMT-AUD-020]`, and PipeWire does
its own adaptive resampling — so `hw_ptr` under it reflects PipeWire's
correction as much as the hardware. An indicative read of card 0 under
PipeWire gave **+17.79 ppm**; it is recorded here only so that a later, clean
figure can be compared against it, and it should not be quoted as Smart's
drift.

---

## 2a. Results, 2026-09-12

**`[LOG-DRIFT-045]` Both windows completed undisturbed, through the storm.**

| node | window | drift | instrument floor |
| :--- | ---: | ---: | ---: |
| `bose` — HiFiBerry DAC+ Pro, I²S, self-clocked | 21.01 h | **+0.432 ppm** | 0.0003 ppm (quantisation) |
| `smartboardpc` — ATE1133 USB, `ADAPTIVE`, host-slaved | 20.30 h | **+9.96 ppm** | ±0.096 ppm (read bracket) |
| **relative** | | **9.53 ppm** | |

`bose`'s `trigger_time` was unchanged across the window, so the stream never
restarted `[LOG-DRIFT-020]`. Smart's probe stayed alive for 22.9 h of uptime.

`bose` at +0.432 ppm sits close to `[BOS-OPS-020]`'s +0.35 ppm, which is
reassuring given that figure had a `systemd-timesyncd` baseline and this one is
against chrony — the ruler changed and the answer barely moved.

Smart's clean figure is **9.96 ppm, not the 17.79 ppm** the PipeWire card-0
read suggested `[LOG-DRIFT-040]`. Measuring the right card, ALSA-direct, was
worth doing: the indicative number was wrong by 80 %.

**`[LOG-DRIFT-048]` One hour would have been enough to decide.** The storm
prompted the question of how long a window must be. Smart's read-bracket floor
is ±1.94 ppm at one hour — ample to establish a ~10 ppm signal, which is the
only question that changes the design. The extra nineteen hours bought
precision (±0.096 ppm), not the decision.

---

## 3. What the readings settle

**`[LOG-DRIFT-050]` Per-passage resync does NOT suffice for this pair, and
`[GDE-ECHO-110]` is superseded for any pair including Smart.** At 9.53 ppm
relative:

| | drift |
| :--- | ---: |
| across a 4-minute passage | **2.29 ms** |
| across an hour | **34.3 ms** |

2.29 ms sits inside `[GDE-ECHO-050]`'s **1–5 ms comb-filtering band**, so a
listener hearing both would hear the colouration deepen across every passage
and snap back at each boundary — the changing artefact that is more noticeable
than a constant offset.

The favourable conclusion in `[GDE-ECHO-110]` was explicitly conditional on
both nodes being sub-ppm. `bose` is, at 0.432. Smart is not, at 9.96. **A
bose↔Smart pair therefore requires the continuous trim of Phase 5
`[GDE-ECHO-340]`, not a boundary resync.** A bose↔`teacherslounge` pair might
still qualify — teacherslounge is self-clocked like bose and unmeasured.

The two nodes are not measuring the same physical thing, which is the point of
`[GDE-ECHO-440]`: `bose`'s I²S DAC is self-clocked, so its number is a crystal.
Smart's USB endpoints are both `ADAPTIVE`, so the device follows the host and
its number is the Intel USB controller's frame clock against disciplined time.
Two different causes, neither predicting the other.

---

## 4. Open

**`[LOG-DRIFT-060]` Re-read over a longer window, and at a different room
temperature.** Both figures come from a single ~21 h window at one ambient.
`[GDE-ECHO-060]` warns that a crystal moves with temperature, and `bose` was at
60.3 °C at t₀ — a second window in different conditions is what would turn
these into a range rather than a point. A sampler now appends to
`/var/vaino/drift-samples.tsv` on `bose` every 5 minutes, on C so it survives a
reboot, which makes any later pair of rows a fresh window at no further
effort.

**`[LOG-DRIFT-070]` Three nodes still have no plan.** `vainopi`'s A2DP path
has no `hw_ptr` to read at all — its output is not a hardware PCM
`[GDE-ECHO-070]` — and `teacherslounge` and the desktop are unstarted.
Presentation offset `[GDE-ECHO-430]` is unmeasured everywhere, and the
acoustic cross-correlation `[GDE-ECHO-460]` that would give ground truth
independent of every software estimate has not been attempted.

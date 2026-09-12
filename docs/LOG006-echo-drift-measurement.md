# LOG006: Echo Playback — the Phase 1 Drift Campaign

**Experiment Record — opened 2026-09-12, readings in progress**

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

## 3. What the readings will settle

**`[LOG-DRIFT-050]` Whether per-passage resync suffices for a real pair.**
`[GDE-ECHO-110]` concluded it does — explicitly conditional on both nodes being
sub-ppm. `bose` plausibly is. If Smart's clean figure lands anywhere near
card 0's indicative +17.79 ppm, the pair drifts ~4 ms across a four-minute
passage, which is inside `[GDE-ECHO-050]`'s comb-filtering band, and that
conclusion fails for any pair including Smart.

The two nodes are not measuring the same physical thing, which is the point of
`[GDE-ECHO-440]`: `bose`'s I²S DAC is self-clocked, so its number is a crystal.
Smart's USB endpoints are both `ADAPTIVE`, so the device follows the host and
its number is the Intel USB controller's frame clock against disciplined time.
Two different causes, neither predicting the other.

---

## 4. Open

**`[LOG-DRIFT-060]` Take the second reads no earlier than 2026-09-13T00:20Z**,
confirming `state: RUNNING` and, for `bose`, an unchanged `trigger_time`
first. Record ambient temperature with each `[GDE-ECHO-270]`.

**`[LOG-DRIFT-070]` Three nodes still have no plan.** `vainopi`'s A2DP path
has no `hw_ptr` to read at all — its output is not a hardware PCM
`[GDE-ECHO-070]` — and `teacherslounge` and the desktop are unstarted.
Presentation offset `[GDE-ECHO-430]` is unmeasured everywhere, and the
acoustic cross-correlation `[GDE-ECHO-460]` that would give ground truth
independent of every software estimate has not been attempted.

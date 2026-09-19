# GUIDE027: What The Transition Achieved

**Development Guidance — written 2026-09-19, from a log line that could not
be wrong**

[GUIDE026](GUIDE026-placing-the-passage-exactly.md) `[GDE-ARC-051]` gave the
passage transition an uncapped actuator in both directions, and the engine
duly reported placing every correction in full. The field measurement
disagreed with the engine, repeatedly, and the engine had no way to be wrong.

> **Related:** [GUIDE026](GUIDE026-placing-the-passage-exactly.md) `[GDE-ARC-051]` — the actuator · [GUIDE025](GUIDE025-what-holds-the-alignment-back.md) `[GDE-ARC-043]` — the ratio this is a case of · [GOV002](GOV002-sources-of-truth.md) `[GOV-SRC-040]` — rank by measurement

---

## 1. A claim that restates its own input

**`[GDE-ARC-057]` `should_admit_nudged` fires on `remaining <= overlap +
nudge`, so it can only act at or *after* the instant it wants. A correction
that arrives with less of the outgoing passage left than it asked to spend is
truncated to whatever was left — and the engine computed its success line from
the request, so it reported the full figure either way.**

The line read:

```
echo-offset: passage 9914 asked for 565 ms earlier; placed by 552 ms of
overlap + 13 ms origin + 0 ms silence = 565 ms, 0 ms left to the frame trim
```

Every term after `placed by` comes from `spend_overlap_ms` applied to the
request. Nothing in it was observed. `0 ms left to the frame trim` is not a
finding that the correction landed; it is arithmetic restating that 552 + 13
= 565.

Measured against `tools/echo_skew.py` over the same transitions on
`lp3-wifi`, 2026-09-19:

| asked | skew step actually measured | delivered |
| ---: | ---: | ---: |
| 565 ms | 269 ms | 48 % |
| 295 ms | 18 ms | 6 % |
| 270 ms | 135 ms | 50 % |
| 133 ms | 84 ms | 63 % |
| 56 ms | 13 ms | 23 % |

The log claimed 100 % on every row. **Which end was wrong could not be
established from inside the engine**, because the engine held no measurement
of the achieved figure to compare against — the classic shape of
`[GDE-ARC-043]`, and the specific failure CLAUDE.md §6 exists to prevent:
verify the thing, not a proxy for it.

## 2. What was added

`achieved_shift_ms` takes the outgoing passage's **real remaining play time at
the instant admission fired**, captured in `admit_due` before `advance()`
removes the pair it was measured from. Without a nudge that instant is the
overlap, so the shift admission really bought is the difference:

```
achieved = (remaining − overlap) + origin − gap
```

`delivered_shift_ms` stays, and stays the right tool for *testing the
decision*: it reads the same clamp the actuator reads, so a test can follow a
correction from decision to intent. What it cannot do is disagree with the
request, which is exactly what was needed.

Three consequences, all of them the point:

- **The shortfall reaches the trim.** `echo_debt_frames` now takes
  `asked − achieved` rather than `asked − intended`, so a truncated
  correction is repaid instead of silently lost.
- **The log carries both numbers**, plus the remaining time and overlap it
  judged them from, so the next disagreement is diagnosable from one line.
- **A clean transition is not exact either, and now says so.** The engine's
  own test found a residue of about 2 ms — admission firing one evaluation
  late. Bounded by one mix block, 46 ms, and previously rounded away by the
  claim rather than by any rounding.

## 3. What this does not yet answer

**The mechanism is confirmed; its size in the field is not.** A shortfall
bounded by one mix block cannot by itself explain a 6 % delivery on a 295 ms
request — that needs the correction to have arrived with only tens of
milliseconds of the outgoing passage left. Plausible, because
`correct_offset` fires when the *master's* passage id changes, which is not
aligned with this node's own transition; but plausible is not measured.

The instrumentation is the experiment rather than the fix. What the deployed
`achieved` figures show — whether they track `delivered`, or fall short, and
by how much against `remaining` — decides whether the next change is to
**issue corrections earlier relative to this node's own boundary**, or to
accept the truncation and let the repaid debt close it.

Recording the open question here rather than guessing at it, because the
previous two changes in this series were each built on a reading that turned
out to measure something else.

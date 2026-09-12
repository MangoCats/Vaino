# GUIDE010: Echo Playback — What a Node Is, and What Its Audio Stack Will Tell You

**Development Guidance — split from [GUIDE008](GUIDE008-echo-playback-investigation.md) on 2026-09-11, which had reached the 300-line limit `[GOV-DOC-010]`**

Two things GUIDE008 needed and could not hold: the model that decides **whether
a given node can echo at all**, expressed in measured parameters rather than
device classes, and the `cpal`-level findings that say **what any node's audio
stack will honestly report** about itself.

> **Related:** [GUIDE008](GUIDE008-echo-playback-investigation.md) — the investigation this was carved out of · [GUIDE009](GUIDE009-echo-playback-plan.md) — the plan that measures every cell left empty here · [SMART001](../SmartPC/SMART001-survey.md) and [BOSE001](../BosePi/BOSE001-survey.md) — the per-machine surveys the roster draws on

---

## 1. A node is a set of measurements, not a kind of device

**`[GDE-ECHO-400]` Eligibility is a predicate over measured parameters.** An
allowlist of blessed hardware cannot survive its first unfamiliar node, and this
fleet acquired two unfamiliar nodes in a single afternoon. What decides whether
a machine can echo is not what it *is* but what is *known* about it:

| parameter | what it answers |
| :--- | :--- |
| `rate_error_ppm` | how fast it walks away, and in which direction |
| `presentation_offset` | submit-to-sound delay, in ms |
| `offset_stability` | whether that delay stays put |
| `clock_ownership` | self, host-slaved, or remote — *why* it drifts |
| `timestamp_source` | hardware, software, or none `[GDE-ECHO-290]` |
| `native_rates` | whether the library's rate survives untouched |

**`[GDE-ECHO-410]` Every node has a presentation offset, and no node is the
reference.** All participants agree on a common *sound time* and each schedules
backward by its own offset:

```
submit_time(node, sample N) = sound_time(sample N) − presentation_offset(node)
```

A master is the node whose **selection** is followed `[GDE-ECHO-020]`, not the
node whose latency defines the datum. This matters in the case that is otherwise
easy to get wrong: when the *master* is the high-latency node, a low-latency
echo node simply waits. Treating the master's offset as zero would have built a
system that works in one direction only.

**`[GDE-ECHO-420]` Offset magnitude is compensable; offset *variability* is what
disqualifies.** An echo node holds the audio on its own disk and knows the queue
ahead of time, so it can begin early by exactly its own offset — the capability a
streaming receiver categorically lacks, because it does not yet have the audio.
A fixed 250 ms is therefore a scheduling constant, not an obstacle.

This retires the categorical exclusion of Bluetooth. The real question for
`vainopi` is not *how large* its A2DP latency is but *how often and how far it
moves*, and whether a move can be detected. That is a measurement nobody has
taken, not a property anyone should assume.

**`[GDE-ECHO-430]` The offset has a measured part and a calibrated part, and
pretending otherwise would hide the larger one.** ALSA reports its own delay
`[GDE-ECHO-160]`, which for an I²S DAC is essentially the whole story. For A2DP
the sink's internal buffering is opaque to the host, so a residual remains that
the stack will not report. `presentation_offset` is therefore *measured delay +
calibrated residual*, the residual established once per node and re-established
whenever the link renegotiates. A node that reports only the measured half is
declaring a number it cannot support `[GOV-SRC-030]`.

**`[GDE-ECHO-440]` Clock ownership has three cases, and they drift for unrelated
reasons.** *Self-clocked* — an I²S DAC or an asynchronous USB device — drifts by
its own crystal. *Host-slaved* — an adaptive USB sink `[SMT-AUD-040]` — drifts
by the host controller's frame timing. *Remote* — A2DP — drifts by a clock in
another device entirely. Two nodes in different cases share no common cause, so
no measurement of one predicts the other. This is `[GDE-ECHO-060]` restated as
mechanism rather than caution.

---

## 2. The fleet as it actually stands

**`[GDE-ECHO-450]` Five nodes, five different situations, one measured cell.**
Probed 2026-09-11.

| node | output | clock | offset | ppm | timestamps |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `bose` | HiFiBerry DAC+ Pro, I²S `[PI-BOS-020]` | self | small, fixed | **+0.432** `[LOG-DRIFT-045]` | hardware (cpal enables it) |
| `teacherslounge` | Realtek ALC3246 analog | self | small, expected fixed | — | unknown |
| `smartboardpc` | ATE1133 USB, adaptive `[SMT-AUD-040]` | host-slaved | medium | **+9.96** `[LOG-DRIFT-045]` | ≥1 ms granularity |
| `vainopi` | A2DP to the Middleton | remote | large, renegotiates | — | unknown |
| desktop (`local`) | Windows WASAPI | self | unknown | — | estimate only `[GDE-ECHO-180]` |

Two numbers in the table are now measured, and they differ by a factor of 23 — one self-clocked, one host-slaved, which is `[GDE-ECHO-440]` arriving as data rather than argument. Three rows remain empty. That is the argument for
[GUIDE009](GUIDE009-echo-playback-plan.md)'s measurement phase existing, stated as a
table rather than as a worry.

**`[GDE-ECHO-470]` An echo node needs the same audio, and three nodes already
have it.** Surveyed 2026-09-11:

| node | library | audio files |
| :--- | :--- | ---: |
| desktop (`local`) | `C:\Users\Mango Cat\Music`, 44 G | 5,743 |
| `teacherslounge` | `/home/sw/Music`, 44 G | 5,743 — **identical to local** |
| `smartboardpc` | `/media/mango/PortableSSD/Media/Music`, 49 G | 5,754 — a variant, reconciled with local 2026-09-12 |
| `bose` | its own, 44 G `[BOSE001]` | — |

`teacherslounge` therefore needs no audio to become a playback node; it needs
chrony (installed 2026-09-11), a library database, and a service. Smart's
library sits on a volume that is **100 % full with 11 G spare**
`[SMT-STO-010]`, which is a provisioning problem of a different kind.

> **Smart was given a library on 2026-09-12, and the reconciliation is the
> lesson.** The local catalogue was snapshotted (`VACUUM INTO`, so a live WAL
> database could be copied consistently), transferred, paired with a fresh
> empty listener half from `schema.sql` → `split_database.py`, and bound with
> `relink` — which matches by **encoded-audio hash**, not by path
> `[SPEC012]`. Smart keeps its own listening history rather than inheriting
> the desktop's.
>
> Comparing the two collections **by path** claimed 877 audio files were
> missing. The hash said **35**. The other ~840 were already on Smart under
> different filenames, and copying them would have written ~1.7 GB of
> duplicates into that 11 G of headroom and left the library pointing at the
> wrong copies. `[GDE-ECHO-480]` said path comparison was unreliable; this is
> the number that says how unreliable. The 35 genuinely-absent files were five
> complete albums, copied into Smart's own root and structure, after which
> `relink --apply` bound **5,709 of 5,709** rows with zero missing.

**`[GDE-ECHO-480]` Comparing two libraries by path is unreliable, and failed
four distinct ways on one afternoon.** Establishing whether local and
`teacherslounge` differed produced three successive wrong answers before the
right one, each wrong for its own reason:

| apparent delta | actual cause |
| ---: | :--- |
| 685 files | the two sides sorted under different collations |
| 49 files | Windows cannot store a **trailing dot** in a directory name (`Garage Inc.`) |
| 21 files | a manual rename (`Born in the U.S.A.` → `Born in the USA`) |
| **0 files** | the truth — verified byte-identical at 5,171,539 bytes on a disputed track |

Comparing local against Smart added two more classes: a **colon** stripped from
a directory name, and an artist-folder convention (`The Beach Boys` against
`Beach Boys, The`) that alone accounted for roughly 150 apparent differences in
each direction. Normalised by album and track, the genuine delta is 36 files
one way and 12 the other.

Acting on any of the intermediate answers would have written hundreds of
duplicate files into differently-spelled directories. **`md5_encoded` is the
identity `[REQ-AUD-100]`, paths are not**, and `[SPEC012]`'s relink exists
precisely because a path is not portable between two machines. Any future
library reconciliation should hash rather than compare names; the figures above
are path-derived and are evidence of a difference, not proof of one.

**`[GDE-ECHO-460]` The fleet already owns an acoustic instrument, by
accident.** The ATE1133's *capture* endpoint is asynchronous mono at 48 kHz
`[SMT-AUD-050]`. A microphone on that input can record two speakers at once and
recover their true offset by cross-correlation — a measurement that depends on
no software estimate anywhere in either node's stack, and is therefore the
ground truth `[GOV-SRC-020]` asks for when ranking every other method. It is
also the only instrument here that can see `[GDE-ECHO-430]`'s uncalibrated
residual at all.

---

## 3. What the audio stack actually provides

Everything below was read out of `cpal` 0.15.3's own source rather than inferred
from its documentation, because the failure modes are all in the fallbacks.

**`[GDE-ECHO-140]` The callback is already handed a timestamp, and Vaino throws
it away.** In `player/src/output.rs`, all three sample-format arms are
`move |out, _| fill(...)` — the discarded `_` is `&cpal::OutputCallbackInfo`.
The hook needed for any of this already exists, at the one place
`[REQ-AUD-164]` says measurements must be taken.

**`[GDE-ECHO-150]` cpal's ALSA backend does enable hardware timestamping — in a
clock domain chrony deliberately does not discipline.** It calls
`set_tstamp_mode(true)` then `set_tstamp_type(TstampType::MonotonicRaw)`, and on
failure silently retries with `Monotonic`. `CLOCK_MONOTONIC` is frequency-slewed
by `adjtimex` and therefore *is* disciplined; `CLOCK_MONOTONIC_RAW` is not. So
cpal's timestamps arrive in one of two domains differing by the system crystal's
own error — tens of ppm, one to two orders of magnitude larger than the DAC
error being measured — and the API offers no way to learn which one was given.

**`[GDE-ECHO-160]` The absolute instants are not comparable across machines; the
difference between them is.** `StreamInstant` is measured from the stream's own
trigger, so two nodes' values share no epoch. But `playback.duration_since(callback)`
is computed as `frames_to_duration(status.get_delay())` — the genuine ALSA
hardware delay, expressed as a duration, and therefore domain-independent and
directly usable. **Take the difference, never the absolutes** is the whole rule,
and it is `[GOV-SRC-050]` in miniature: the two values answer different
questions.

**`[GDE-ECHO-170]` If hardware timestamps are unavailable, cpal substitutes a
software clock permanently and silently.** At stream open it probes
`get_htstamp()` once; on `(0, 0)` it stores an `Instant` and every later
timestamp becomes elapsed time since stream creation — a pure software monotonic
reading carrying **no information about the DAC at all**, in which drift is
definitionally invisible. Consumed unknowingly, that is a textbook
`[GOV-SRC-030]` breach: the weaker source answers in the same shape as the
stronger one, destroying the evidence that would expose it. Any use of these
timestamps must detect and declare the fallback.

**`[GDE-ECHO-180]` On Windows the delay term is an estimate by its own
admission.** cpal's WASAPI backend carries the comment that the returned
`playback` value "is an estimate that assumes audio is delivered immediately
after the callback." The desktop can therefore be a master or a controller, but
must not be assumed measurable as an echo node until someone measures it.

**`[GDE-ECHO-190]` There is a `panic!` on the audio thread in that path.**
cpal's `stream_timestamp` panics outright if `get_htstamp` precedes
`get_trigger_htstamp`, and two neighbouring `.expect()` calls abort on
`StreamInstant` range overflow. Vaino does not currently reach any of them
because it does not read the timestamp; a design that starts reading it inherits
them, and must not add a reason to call into that code more often.

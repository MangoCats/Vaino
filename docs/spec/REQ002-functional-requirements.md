# REQ002: Functional Requirements

**Requirements Specification — Tier 1**

What Vaino and Sampo must do. **Supersedes [REQ001](REQ001-system-requirements.md)**, a v1 artifact retained only until mined `[GDE-DIS-010]`.

Derived from six years of MuLibPlay production behaviour `[GDE-BMK-*]` and McRhythm's refined functional work `[GDE-MCR-050]`, inherited as [MCR-REQ001](../inherited/mcrhythm/MCR-REQ001-requirements.md). McRhythm's *requirements* are inherited; its *architecture* is rejected `[GDE-CHT-050]`.

> **Domain acronyms** are chosen to avoid McRhythm's REQ namespace entirely (`PI, CF, NET, UI, VER, PB, OFF, NF, CTL, SEL, QUE, AUTH, TECH, IPD, FLV, ART, PERS, HIST, ERR, XFD, DEF, AF, UQ, OV`), so a `grep` for a Vaino requirement cannot match inherited material `[INH-HAZ-020]`.

---

## 1. Audio Playback — `AUD`

**`[REQ-AUD-100]`** Play the user's audio files with the **decoded audio stream unaltered** `[SPEC-DF-020]`. Verifiable, not merely asserted: `md5_encoded` before and after any Vaino operation must match.

**`[REQ-AUD-110]`** Decode by **streaming**, never whole-file. Bounded per-passage buffers `[GDE-ARC-050]`. The library contains a 244.9-minute file that needs ~2.6 GB decoded `[GDE-V1-030]`; this requirement is what makes it playable at all.

**`[REQ-AUD-120]`** Play any passage as a span of a larger file — a DAO file holds up to 40 `[GDE-BMK-020]` — without decoding the portions outside it.

**`[REQ-AUD-122]` Passage boundaries are sample-accurate, not packet-accurate.** A decoder seek lands on a container packet and reports where it actually landed; the remainder must be discarded so `start_ms` means `start_ms`. Measured drift when the reported landing was ignored: **648 frames, 14.7 ms**, on every passage with a non-zero start. It is inaudible in isolation, which is precisely the danger — it silently shifts every trim point, and because the passage length is measured from the *requested* start, it drags the end boundary along with it.

**`[REQ-AUD-130]`** Crossfade between consecutive passages using their lead-in/lead-out points and gain `[SPEC-SC-040]`.

**`[REQ-AUD-140]`** Resume playback state across restart, including position within a passage `[SPEC-SC-098]`.

**`[REQ-AUD-142]` Playback has exactly two states: playing and paused.** There is no "stopped". Pausing halts only the *consumer*; decoders keep filling their buffers, so resuming is instant and the pipeline stays primed after the initial power-on fill. A brief underrun at first start is therefore expected and acceptable; if it proves audible, the remedy is to prime the output before commencing, not to add a third state.

> **Halting the consumer means stopping the output device, not merely declining to submit.** The ring holds ~14 s and the device callback drains it regardless, so a pause that only stops submission leaves the music playing for another fourteen seconds — observed directly: reported position climbed 49.7 s → 51.7 s while paused. Stopping the stream leaves the ring full, which is what makes resuming instant. Where a backend cannot pause, the caller must be told rather than assume silence.
>
> **Underruns are counted only while playing.** A paused player underruns continuously by design; counting those buries the fault the number exists to expose. Before this distinction the idle figure reached 479,232 samples (~5 s); measured during real playback it is 0.

**`[REQ-AUD-150]`** Audio output is **co-located with the server**. Remote devices control; they do not receive streams.

**`[REQ-AUD-152]` Master volume is applied at the output device, in the callback** — not to samples on their way into the ring. Anything applied before submission is heard only after everything already submitted has drained, so with a ~14 s ring the control appeared to lag by ten seconds or more: the knob was governing audio computed far ahead of the ear. Applying it in the callback means a change reaches samples that are already buffered but not yet heard, which is precisely the audio a listener expects a volume knob to affect.

> This is the same buffer-depth trap as pausing by declining to submit `[REQ-AUD-142]`, and it recurs for any control the listener expects to act *now*. The rule generalises: per-passage properties (gain `[SPEC-SC-040]`, crossfade) belong before the mixer, because each side of a crossfade carries its own level; listening controls belong at the device.
>
> The value crosses to the callback as an atomic, not behind the ring's mutex. The callback must never block, and must be able to change level even on a tick where it cannot take that lock.

**`[REQ-AUD-154]` The master fader is logarithmic: equal travel, equal decibels.** 64 dB of range, `amplitude = 10^((travel − 1) × 64 / 20)`, with the very bottom of the travel being true silence rather than −64 dB — a fader that cannot be closed is a fault. Loudness is perceived in ratios, so a linear amplitude fader spends its top half on differences barely distinguishable from full and crams everything audible into the bottom sliver.

| knob | linear (was) | logarithmic (is) |
|---:|---:|---:|
| 100 % | 1.000 | 1.000 — 0 dB |
| 75 % | 0.750 | 0.158 — −16 dB |
| 50 % | 0.500 | 0.025 — −32 dB |
| 25 % | 0.250 | 0.004 — −48 dB |
| 0 % | 0.000 | muted |

> The 64 dB figure is MuLibPlay's, carried over from six years of daily use: it ran a `-8192..=0` integer slider scaled by 1/128 into exactly this curve, likewise with a hard mute at the bottom.
>
> **Amplitude is the internal representation; travel is the listener's.** The engine, the device and the saved resume point all speak amplitude — it is what multiplies samples. Only the control speaks in travel, and the conversion happens once at each edge of the HTTP layer. The browser is told a position and a dB value and holds no copy of the curve, so there is one taper in the system rather than two that can drift.

> **Verification:** `[REQ-AUD-110]` and `[REQ-AUD-120]` are gated by an automated test playing the 244.9-minute file at ≤150 MB RSS and ≤500 ms skip latency `[GDE-ARC-050]`.
>
> `[REQ-AUD-140]` verified end-to-end on desktop hardware (48 kHz device, 44.1 kHz sources, 8,079-passage library): a run interrupted at ~16 s saved 15.01 s, and the next run resumed the same passage at 15.0 s and went on to save 25.01 s — position advancing *from* the resume point, not restarting. The 15.01 s figure is also the check on audible-versus-mixed position: had the mixed figure been saved it would have read ~29 s.

## 2. Program Director — `PD`

**`[REQ-PD-100]`** Select the next passage automatically, continuously, without user intervention. The queue never empties while eligible passages exist.

**`[REQ-PD-110]`** Implement MuLibPlay's weighting **as designed** `[GDE-PD-010..030]`: log-scale rotation, multiplicative artist-then-track eligibility, hard rotation block, linear recovery ramp, seasonal occasion multipliers, length bonus, and a `minWeightLimit` floor.

> **As designed, not as shipped.** This previously read "reproduce exactly". It changed when a variable shadowing was found in the shipped code: MuLibPlay's artist recovery ramp never reached the track weight, so a partially recovered artist has never damped its tracks `[SPEC-DIR-117]`. Vaino implements the ramp. MuLibPlay is a proven baseline, not a ceiling — six years of satisfactory listening is evidence the design is sound, not evidence that every behaviour of the binary is worth preserving.
>
> **Bit-identical reproduction is therefore no longer the acceptance test**, and could not be: the two now deliberately differ. The `GateOnly` coupling is retained so the divergence can be *measured* rather than assumed, which is the more useful check — it says how much the corrected ramp actually changes selection. Consistent with `[GDE-QUA-*]`, that measurement is diagnostic, never a pass/fail gate.

**`[REQ-PD-115]` Related recordings share a rotation.** Hearing a live take, a remaster or a compilation appearance suppresses the others `[SPEC-DIR-116]`. Every relation applies, each judged on its own play history, damped over a recovery window scaled by relation strength.

**`[REQ-PD-118]` Two master time scales — one for artists, one for tracks** — multiply every block and ramp duration `[SPEC-DIR-118]`. Range 0.0001–100.0000 to four decimal places, default 1.0000, at which they are exactly inert. They scale durations only, never weights, so *when* a passage becomes eligible is adjustable without touching *how much* it is wanted.

**`[REQ-PD-112]` Record every play, keyed by recording MBID.** Rotation is meaningless without it: an unrecorded play leaves a track as eligible as it was before, so a long session repeats what the algorithm exists to space out.

> Recorded at the **start** of playback, not on completion. Rotation spaces out what the listener has *encountered*, and a track skipped after ten seconds has been encountered — suppressing it for a while is the wanted behaviour. This also matches MuLibPlay, whose own note says the history structures update "as each new track finishes playing (or is put in the play queue)".
>
> Stored with `passage_id` **and** `mbid` `[SPEC-SC-095]`: the passage id is the convenience, the MBID is what survives a rescan that renumbers passages. An unidentified passage still records a play with a null MBID — it simply cannot contribute to rotation.
>
> A passage may legally hold a medley of several recordings, so the query selects the heaviest by a scalar subquery rather than a join, which would return that passage twice in every pool.

**`[REQ-PD-120]`** Select from **`radio` passages only** `[GDE-BMK-030]`.

**`[REQ-PD-130]`** Shape candidates by flavor distance `[SPEC-FD-040]` in two stages — prune against programme seeds, then order by similarity to the passage already queued — and apply randomness **last**, over the shaped pool `[GDE-PD-050]`.

**`[REQ-PD-140]`** Express a programme as **a list of exemplar passages**, not tuned parameters `[GDE-PD-040]`. "What should 10 AM sound like?" is answered by naming songs.

**`[REQ-PD-150]`** Honour user Likes and Dislikes as inputs to selection `[GDE-MCR-070]`. *How* Taste combines with seed shaping is open design `[GDE-OPN-030]`.

**`[REQ-PD-160]`** Degrade gracefully on partial flavor data: a passage with 11 known characteristics remains selectable alongside one with 71 `[SPEC-FD-040]`.

## 3. Visibility — `VIS`

Vaino's headline requirement `[GDE-CHT-030]`. Previously specified nowhere.

**`[REQ-VIS-100]` Why this track?** Every automatic selection exposes its full weight decomposition — artist weight, rotation block state, position on the recovery ramp, occasion multiplier, length bonus, distance to each seed, final rank, roulette position — **and the runners-up that lost**.

> **Status: the frequency half is delivered.** Every Director-chosen passage carries its full Stage-A decomposition — each term separately, never just the product — plus the five heaviest runners-up it beat, its share of the pool, and the pool's size and total weight. Written durably to `selection_decisions` and shown in the web UI. Terms sitting at ×1.0000 are dimmed rather than hidden: "this did not apply" is part of the answer.
>
> **Still missing:** distance to each seed, Taste effect, flow distance and roulette rank, all of which belong to stages B–D and need flavor distance `[SPEC-FD-040]`. Each stored record states which stages ran, so a decision recorded now cannot later be mistaken for a shaped one. A passage the Director did not choose — a resumed one, or one queued before the log began — reports that plainly rather than borrowing another passage's reasoning.

**`[REQ-VIS-110]` How was this identified?** Every ingest decision is a durable record: which stage matched, at what confidence, which candidates were rejected `[SPEC-SA-085]`. This is what converts an undocumented ritual `[GDE-BMK-050]` into a reviewable process.

**`[REQ-VIS-120]` Is this data trustworthy?** Every flavor value displays its provenance and measured accuracy `[SPEC-SC-070]`. A user must be able to see whether a value came from the dump, was locally computed at a stated error, or was entered by hand.

**`[REQ-VIS-130]`** Automatically computed boundaries, lead-in/lead-out points and gain are **reviewable and overridable** through a waveform view `[SPEC-SA-080]`. Manual edits outrank computed values permanently and are never silently recomputed.

**`[REQ-VIS-150]` The listening surface shows what is coming and what is in force.** The queue in play order, the active programme with a manual override `[SPEC-DIR-185]`, and master volume.

> Delivered 2026-08-13. Two implementation notes worth keeping:
>
> **Per-passage `gain_db` was read from the library and never applied to the audio** — it reached `QueueEntry`, was printed by `station`, and was silently dropped before the mixer. The library carries real values (median −3.0 dB), so tracks were playing at whatever level they were mastered at. It is now applied **per passage, before mixing**, so each side of a crossfade carries its own level; applying it after the mix would level the blend rather than the tracks, and the point is that they meet at a matched loudness. Master volume is applied last, over the mixed signal, because it is a listening level and not a property of any passage.
>
> **The Director is not `Sync`,** so the browser cannot reach it. Programme choice is written to a small shared cell that the engine reads on its next refill — an override therefore changes what is selected *next* rather than interrupting what is playing, which is the wanted behaviour. An unknown programme id is a 404 rather than a silent no-op.

**`[REQ-VIS-140]`** Long-running operations report real progress and are interruptible without loss `[REQ-LIB-130]`.

## 4. Library Building — `LIB` *(Sampo)*

**`[REQ-LIB-100]`** Induct new music without hand labour — the reason the project exists `[GDE-CHT-020]`.

**`[REQ-LIB-110]`** Segment a multi-track file into passages and identify each against MusicBrainz `[SPEC-SA-070]`.

**`[REQ-LIB-120]`** Compute flavor locally, with no dependence on any live external service `[GDE-FEX-027]`. AcousticBrainz's API died within seven months of a successful bulk query `[GDE-MCR-045]`.

**`[REQ-LIB-130]`** Import is **incremental, resumable and interruptible** `[GDE-CHT-045]`. Adding a handful of tracks is the common case; a full library scan is the exception.

**`[REQ-LIB-140]`** Never re-decode audio to improve a classifier. Lowlevel features are cached permanently `[SPEC-SC-080]`.

**`[REQ-LIB-145]` Repair `duration_ms` from the decoded length at ingest, wherever it disagrees.** `[SPEC-SC-030]` already specifies "decoded, not header-claimed", and the migrated library violates it: **29.2% of files differ from their decoded length by more than 5 s**, 3.2% *over*-state it, and one overstates by **38.4 minutes** `[GDE-FEX-106]`.

> This is not cosmetic. Segmentation used the inflated value to create a **phantom passage** in a tail that does not exist, and the player uses `duration_ms` for lead-out timing. A field this load-bearing being wrong on a quarter of the library will keep producing symptoms that look like unrelated bugs — the extraction failure that surfaced it looked at first like an ffmpeg fault.
>
> Repair it where it disagrees, rather than always: `ffprobe` costs ~50 ms, but rewriting a correct value is churn. Passages already derived from a wrong duration need re-checking, not just the file row.
>
> **Done 2026-08-13** by `tools/repair_durations.py`, over all 5,590 files:
>
> | | |
> | :--- | ---: |
> | durations wrong by >1 s | **1,621 (29.0%)** |
> | …over-stating the file | 270 |
> | error: median / p95 / max | **35.0 s** / 234.6 s / 2301.3 s |
> | passage ends past the real audio, clamped | **453** |
> | phantom passages deleted | 1 |
>
> A **median error of 35 seconds** is not encoder rounding. The 29.0% measured over the whole library matches the 29.2% sample estimate `[GDE-FEX-106]` exactly.
>
> One consequence worth recording: `lowlevel_cache` is keyed `(audio_md5, start_ms, end_ms)` `[SPEC-SC-080]`, so clamping an end **orphans that passage's cached features**. The features were still correct — extraction had already clamped the range before analysing — so 212 rows were **re-keyed rather than re-extracted**, each matching exactly one cache row on `(audio_md5, start_ms)`. Radio-passage coverage is now **8,078 of 8,078**. Any repair that moves a passage boundary must consider the cache key, or it silently discards work.

**`[REQ-LIB-150]`** Relocate a moved or renamed library by content, not path `[SPEC-SC-035]`.

## 5. Portability — `PORT`

**`[REQ-PORT-100]`** A Vaino installation with **no Sampo** can receive derived data and use every advanced feature `[SPEC-DF-080]`.

**`[REQ-PORT-110]`** Derived data travels by embedded tag, per-file sidecar, or whole-database migration — one payload schema across all three `[SPEC-DF-065]`.

**`[REQ-PORT-120]`** Listener state **never** travels with music `[SPEC-DF-055]`. The transport carries facts about the music, never facts about the listener.

**`[REQ-PORT-130]`** Imported metadata is verified before trust: recompute `audio_md5` and discard encoding-scope claims that disagree `[SPEC-DF-070]`.

**`[REQ-PORT-140]`** Writing tags to a user's files requires informed consent, and uses temp-file → verify → atomic replace so a failed write cannot damage the library `[SPEC-DF-092]`.

**`[REQ-PORT-150]`** Listener state is exported automatically on a schedule, integrity-checked before rotation, retained generationally `[SPEC-DF-094]`. It is the only irreplaceable data in the system.

## 6. Appliance — `HW`

**`[REQ-HW-100]`** Run continuously on a Raspberry Pi Zero 2W (512 MB) — **≤150 MB RSS** `[GDE-MCR-020]`. MuLibPlay uses 171 MB on a 1.8 GB Pi 4 `[GDE-BMK-010]`; Vaino must fit a third of the memory.

**`[REQ-HW-110]` Reach first audio quickly on power-up — best effort, and bounded by the output profile.** Management services may start afterwards.

This is deliberately **not** an absolute target, because the audio output channel determines what is achievable `[IMPL-PROF-010]`. A Bluetooth sink must associate before any audio can flow, and that cost is inherent to the channel rather than to Vaino. Two consequences:

- **`[REQ-HW-112]`** Where an output channel imposes an unavoidable startup delay, that delay is **accepted for that profile only**. It must not be allowed to set the standard for profiles that do not share it.
- **`[REQ-HW-114]`** Profiles without such a delay — I2S DAC, USB DAC, HDMI — must be configurable for the faster boot, **sacrificing Bluetooth capability** to do so. Fast boot and Bluetooth are alternatives, not a compromise to be split.

**`[REQ-HW-120]`** Survive repeated hard power loss without database corruption.

**`[REQ-HW-130]`** The player is portable and reaches ARM. Sampo need not `[SPEC-SA-018]`.

**`[REQ-HW-140]` Desktop and server hosts are first-class targets, not a by-product of the appliance.** Vaino runs on Windows, Linux and macOS as an ordinary application `[GDE-CHT-045]`. The Pi Zero 2W is the *constraining* target, not the only one.

**`[REQ-HW-145]` Every supported target is tested, not merely compiled.** `build/verify-targets.sh` runs the suite on Linux x86_64, Linux aarch64 (under emulation) and the host. Compiling is not testing: an audit found aarch64 had only ever been *built*, Linux x86_64 never built at all, and the suite only ever *run* on Windows.

**`[REQ-HW-147]` At least one verification must use a real audio device.** A null sink reports no device rate and therefore cannot detect a sample-rate fault. This is not hypothetical: playback ran **8.8% fast — about 1.5 semitones sharp** — because a 48 kHz device met a 44.1 kHz library with the resampler unwired, and every prior test had used a null sink.

## 7. Non-Requirements

**`[REQ-NEG-100]`** Vaino does **not** stream audio to remote devices `[REQ-AUD-150]`, require any live external service at playback time, or modify audio data `[REQ-AUD-100]`.

**`[REQ-NEG-110]`** Sampo does **not** play audio, run on the appliance, or hold listener state `[SPEC-SA-100]`.

**`[REQ-NEG-120]` Neither Vaino nor Sampo integrates with personal-cloud accounts.** No Google Drive, Gmail, Calendar, or equivalent from any vendor — not for storage, not for scheduling, not for identity. This is a scope boundary, not an unimplemented feature.

> Three reasons it stays closed. **Playback must not depend on a reachable service** `[REQ-NEG-100]`; an appliance that cannot play music because a token expired has failed at its only job. **Network cost is justified per data class** `[SPEC006 §B]` — identification earns its lookups because MBIDs cannot be derived locally; nothing in playback, selection, or flavor can make that case. **Listener history is the user's** `[SPEC-DF-090]`, which is why class-D export exists at all; routing it through a third-party account inverts that.
>
> The near-miss worth naming: occasion weighting `[REQ-PD-050]` is seasonal, computed from month and day against the system clock `[SPEC003 §3.3]`. It is *not* a calendar integration and must not become one — reading real appointments would make selection fail when a remote service is unreachable.
>
> Off-machine backup of a class-D export to cloud storage is a legitimate thing a **user** may choose to do with a file Vaino has already written. Vaino does not do it for them, and nothing in the system may assume it happened.

---

## 8. Coverage Gaps

Areas where [MCR-REQ001](../inherited/mcrhythm/MCR-REQ001-requirements.md) has substantial requirements that Vaino has **not yet adopted or rejected** — recorded so the gap is visible rather than accidental:

| McRhythm area | Status |
| :--- | :--- |
| User identity / authentication (`AUTH`, 13 reqs) | Undecided. MuLibPlay is single-user with no auth. |
| Multi-user coordination (`PERS`, `UQ`) | Undecided; likely out of scope for v1. |
| Network status & offline operation (`NET`, `OFF`, 66 reqs) | Partially implied by `[REQ-LIB-120]`; not enumerated. |
| Three build tiers Full/Lite/Minimal (`VER`, 19 reqs) | Superseded by the Vaino/Sampo split `[GDE-ARC-010]`. |
| Error handling (`ERR`, 5 reqs) | Not yet enumerated. |

---

**Traceability:** `[REQ-AUD-100..NEG-110]` · supersedes `REQ001` · derived from `[GDE-BMK-*]`, `[GDE-PD-*]`, `[GDE-CHT-*]`, inherited `MCR-REQ001`

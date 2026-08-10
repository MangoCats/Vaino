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

**`[REQ-AUD-150]`** Audio output is **co-located with the server**. Remote devices control; they do not receive streams.

> **Verification:** `[REQ-AUD-110]` and `[REQ-AUD-120]` are gated by an automated test playing the 244.9-minute file at ≤150 MB RSS and ≤500 ms skip latency `[GDE-ARC-050]`.
>
> `[REQ-AUD-140]` verified end-to-end on desktop hardware (48 kHz device, 44.1 kHz sources, 8,079-passage library): a run interrupted at ~16 s saved 15.01 s, and the next run resumed the same passage at 15.0 s and went on to save 25.01 s — position advancing *from* the resume point, not restarting. The 15.01 s figure is also the check on audible-versus-mixed position: had the mixed figure been saved it would have read ~29 s.

## 2. Program Director — `PD`

**`[REQ-PD-100]`** Select the next passage automatically, continuously, without user intervention. The queue never empties while eligible passages exist.

**`[REQ-PD-110]`** Reproduce MuLibPlay's weighting exactly `[GDE-PD-010..030]`: log-scale rotation, multiplicative artist-then-track eligibility, hard rotation block, linear recovery ramp, seasonal occasion multipliers, length bonus, and a `minWeightLimit` floor.

**`[REQ-PD-120]`** Select from **`radio` passages only** `[GDE-BMK-030]`.

**`[REQ-PD-130]`** Shape candidates by flavor distance `[SPEC-FD-040]` in two stages — prune against programme seeds, then order by similarity to the passage already queued — and apply randomness **last**, over the shaped pool `[GDE-PD-050]`.

**`[REQ-PD-140]`** Express a programme as **a list of exemplar passages**, not tuned parameters `[GDE-PD-040]`. "What should 10 AM sound like?" is answered by naming songs.

**`[REQ-PD-150]`** Honour user Likes and Dislikes as inputs to selection `[GDE-MCR-070]`. *How* Taste combines with seed shaping is open design `[GDE-OPN-030]`.

**`[REQ-PD-160]`** Degrade gracefully on partial flavor data: a passage with 11 known characteristics remains selectable alongside one with 71 `[SPEC-FD-040]`.

## 3. Visibility — `VIS`

Vaino's headline requirement `[GDE-CHT-030]`. Previously specified nowhere.

**`[REQ-VIS-100]` Why this track?** Every automatic selection exposes its full weight decomposition — artist weight, rotation block state, position on the recovery ramp, occasion multiplier, length bonus, distance to each seed, final rank, roulette position — **and the runners-up that lost**.

**`[REQ-VIS-110]` How was this identified?** Every ingest decision is a durable record: which stage matched, at what confidence, which candidates were rejected `[SPEC-SA-085]`. This is what converts an undocumented ritual `[GDE-BMK-050]` into a reviewable process.

**`[REQ-VIS-120]` Is this data trustworthy?** Every flavor value displays its provenance and measured accuracy `[SPEC-SC-070]`. A user must be able to see whether a value came from the dump, was locally computed at a stated error, or was entered by hand.

**`[REQ-VIS-130]`** Automatically computed boundaries, lead-in/lead-out points and gain are **reviewable and overridable** through a waveform view `[SPEC-SA-080]`. Manual edits outrank computed values permanently and are never silently recomputed.

**`[REQ-VIS-140]`** Long-running operations report real progress and are interruptible without loss `[REQ-LIB-130]`.

## 4. Library Building — `LIB` *(Sampo)*

**`[REQ-LIB-100]`** Induct new music without hand labour — the reason the project exists `[GDE-CHT-020]`.

**`[REQ-LIB-110]`** Segment a multi-track file into passages and identify each against MusicBrainz `[SPEC-SA-070]`.

**`[REQ-LIB-120]`** Compute flavor locally, with no dependence on any live external service `[GDE-FEX-027]`. AcousticBrainz's API died within seven months of a successful bulk query `[GDE-MCR-045]`.

**`[REQ-LIB-130]`** Import is **incremental, resumable and interruptible** `[GDE-CHT-045]`. Adding a handful of tracks is the common case; a full library scan is the exception.

**`[REQ-LIB-140]`** Never re-decode audio to improve a classifier. Lowlevel features are cached permanently `[SPEC-SC-080]`.

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

# SPEC007: Sampo Architecture

**Design Specification — Tier 2 · PROVISIONAL**

Structure of **Sampo**, the library builder that turns raw audio into everything `vaino.db` needs. Named for the *Kalevala*'s mill that ground abundance from raw material — a separate artifact that Väinämöinen depends on but never contains `[GDE-ARC-010]`.

> **Status.** §§1–5 rest on measured results and are stable. **§6 (segmentation & identification) is provisional** — the pipeline is specified, but the per-passage extraction question `[SPEC-SA-090]` is untested and could move it. Marked accordingly rather than asserted.

> **Related:** [GUIDE002 §2](../GUIDE002-rearchitecture-plan.md#2-architectural-decisions) · [GUIDE003](../GUIDE003-feature-extraction-strategy.md) · [SPEC006](SPEC006-data-flow-and-portability.md) · inherited [MCR-SPEC033 Album Matching](../inherited/mcrhythm/MCR-SPEC033-album_matching.md) · [MCR-SPEC025 Amplitude Analysis](../inherited/mcrhythm/MCR-SPEC025-amplitude_analysis.md)

---

## 1. Identity and Boundaries

**`[SPEC-SA-010]`** Sampo is a **separate project**: own repository, own licence, own platform envelope.

| | Value | Forced by |
| :--- | :--- | :--- |
| Language | Python | ML/fingerprinting ecosystem; McRhythm's 71K-line Rust ingest collapsed `[GDE-MCR-030]` |
| Licence | **AGPL-3.0** | Essentia is AGPL `[GDE-ARC-018]` |
| Platform | **x86 desktop only** | Essentia extractor has no ARM64 build `[GDE-FEX-137]` |
| Runs | On demand, never on the appliance | `[GDE-ARC-010]` |

**`[SPEC-SA-015]`** Sampo and Vaino communicate **only through the shared SQLite file**. No RPC, no shared process, no linked code. Nothing AGPL enters Vaino.

**`[SPEC-SA-018]` The x86 restriction is Essentia's alone.** Measured 2026-08-09: `fpcalc` ships ARM64 builds for Linux and macOS `[SPEC-SA-060]`. So if the extractor is ever built for ARM64, nothing else in the toolchain blocks it — the constraint is one binary deep, not architectural.

---

## 2. Pipeline

**`[SPEC-SA-020]`** One direction, resumable at every stage:

```
 audio file
    │
    ├─► [S1] scan .......... discover, hash, detect change
    ├─► [S2] segment ....... passage boundaries          ── PROVISIONAL §6
    ├─► [S3] identify ...... fpcalc → AcoustID → MusicBrainz
    ├─► [S4] extract ....... Essentia → lowlevel JSON     ── CACHED FOREVER
    ├─► [S5] classify ...... distilled models → 71-dim flavor
    ├─► [S6] amplitude ..... lead-in/lead-out, gain, segue points
    └─► [S7] publish ....... vaino.db + optional tags/sidecars
```

**`[SPEC-SA-025]` S4's output is cached permanently and is the pivot of the whole design.** Lowlevel extraction costs ~27 s/track `[GDE-FEX-062]` and is the only stage requiring audio decode. Everything downstream — classification, re-classification with better models, flavor regeneration — consumes cached features. **Improving a classifier must never re-decode a user's library** `[GDE-CHT-045]`. This is why S4 and S5 are separate stages rather than one.

**`[SPEC-SA-028]` Every stage is independently resumable and idempotent.** Import is incremental by default `[GDE-CHT-045]`: a 10,000-track library is an overnight job, but the common case is adding a handful of tracks. Interruption at any point must lose at most the in-flight item.

---

## 3. Feature Extraction (S4) — Settled

**`[SPEC-SA-030]`** Invoke AcousticBrainz's **own** published static binary as a subprocess — `essentia-extractor v2.1_beta2-1-ge3940c0`, verified running natively and producing valid output `[GDE-FEX-062]`. Not a reimplementation: the reference implementation itself, so there is no feature-extraction fidelity to validate `[GDE-FEX-090]`.

Subprocess invocation also keeps the AGPL boundary clean — aggregation rather than linking, though Sampo takes AGPL regardless `[GDE-ARC-018]`.

**`[SPEC-SA-035]`** The extractor emits `metadata.audio_properties.md5_encoded` for free. This is Vaino's `audio_md5` identity key `[SPEC-DF-030]`, verified stable across tag writes `[SPEC-DF-020]`.

---

## 4. Classification (S5) — Settled

**`[SPEC-SA-040]`** Distilled models map 928 lowlevel features → 71 highlevel dimensions, reproducing AcousticBrainz's classifiers without Gaia, without a build, and without reverse-engineering a binary format `[GDE-FEX-065]`. Median err/β **0.152** against the reference's own 0.210 self-inconsistency `[LOG-I5-020]` — though that floor is known to be too generous `[LOG-MET-030]`.

**`[SPEC-SA-045]` Model artifacts are raw fp32 arrays, not pickles.** Inference is numpy-only: three matmuls and a ReLU, portable to any language `[LOG-NEXT-050]`. sklearn pickles break across versions and these ship to machines we do not control. Bundles are self-describing — feature ordering, class names, training config, measured accuracy — so a stored model can be audited without the code that made it `[GDE-CHT-030]`.

**`[SPEC-SA-048]`** Model family is per-characteristic and not fixed by this specification; the store abstracts it. Currently 16 dedicated MLPs and 2 gradient-boosted `[LOG-I5-030]`, and further research `[LOG-NEXT-010]` may change the split without affecting anything here.

---

## 5. Identification (S3) — Toolchain Verified

**`[SPEC-SA-050]` The upstream services are alive.** Given that AcousticBrainz died between January and August 2026 `[GDE-MCR-045]`, this was checked rather than assumed (2026-08-09):

| Service | Status |
| :--- | :--- |
| **AcoustID API** | **Alive.** Returns structured JSON errors with validation codes — not the 500s/timeouts AcousticBrainz gives. |
| **MusicBrainz WS/2** | **Alive.** HTTP 200. |
| **Chromaprint** | **Actively maintained** — v1.6.1 released 2026-07-28. |

**`[SPEC-SA-055]` Requires an AcoustID API key.** A registration dependency Vaino does not otherwise have, and a single point of failure for identification. Rate limits apply; results must be cached in `vaino.db` so a re-run never re-queries.

**`[SPEC-SA-060]` Fingerprinting uses the prebuilt `fpcalc` binary as a subprocess** — same pattern as the extractor, no linking, no build. Official builds exist for `windows-x86_64`, `linux-x86_64`, **`linux-arm64`**, `macos-arm64`, `macos-x86_64`, `macos-universal`.

**`[SPEC-SA-065]` Licence, checked not assumed.** Chromaprint's own source is **MIT**; it bundles LGPL-2.1 FFmpeg code, so the project as a whole is distributed as LGPL-2.1 (upstream states an intent to reach MIT-only). LGPL imposes no additional obligation on Sampo, which is already AGPL. Subprocess invocation avoids linking questions entirely. **Note for redistribution:** upstream also flags the licence of whichever external FFT library a given binary was compiled against — must be confirmed before shipping `fpcalc` in a Sampo distribution.

---

## 6. Segmentation & Amplitude (S2, S6) — PROVISIONAL

**`[SPEC-SA-070]` Reproduce McRhythm's segmentation cascade.** It is the fix for MuLibPlay's undocumented manual induction `[GDE-BMK-050]`, and measured 93.0% album match / 96.0% boundary accuracy `[GDE-MCR-010]`. Specified in inherited [MCR-SPEC033](../inherited/mcrhythm/MCR-SPEC033-album_matching.md): grid search → DP assembly → RMS quiet-spot → extra merging, with 7-strategy MusicBrainz edition search and Stage-6 RMS boundary refinement.

**Reproduction requires its own requirements and specification work** — the inherited document describes McRhythm's implementation, not Vaino's contract. Scheduled as a P4 deliverable `[GDE-PHS-040]`, and those figures must be **independently re-verified**: at present they are simultaneously the target and the only evidence.

**`[SPEC-SA-075]` Trim points and segue frames are computed, not hand-placed.** McRhythm automated this and the work is inherited: [MCR-SPEC025](../inherited/mcrhythm/MCR-SPEC025-amplitude_analysis.md) defines lead-in/lead-out detection from an A-weighted RMS intensity envelope with quarter- and three-quarter-intensity thresholds and per-medium parameter presets. This supplies the **Radio** side of the Album/Radio duality `[GDE-BMK-030]`, which MuLibPlay only ever produced by hand.

**`[SPEC-SA-080]` Automatic placement is always reviewable and overridable.** A waveform view with draggable boundaries, lead-in/lead-out markers and gain, for both segmentation and amplitude results. Manual edits outrank computed values permanently `[SPEC-DF-070]` and are never silently recomputed. Automation sets the default; it does not remove the decision.

**`[SPEC-SA-085]` Every decision is recorded, not just logged** — which stage matched, at what confidence, which candidates were rejected `[GDE-CHT-030]`. This is what converts an undocumented ritual into a reviewable process.

**`[SPEC-SA-090]` OPEN — per-passage extraction is untested and may move this section.** Extraction must run **per passage**, not per file: a 40-track DAO file needs 40 extractor runs over 40 slices, which forces S2 before S4 and makes S4 consume sliced temporary audio. Consequences not yet measured:

- The ~27 s/track figure `[GDE-FEX-062]` came entirely from whole single-track files; slicing cost is unaccounted.
- Classifiers were distilled from features over **whole recordings**. Whether a short slice yields in-distribution features is unknown. MuLibPlay's Radio cuts run to a **12 s minimum**, with 2.5% under 90 s.
- A minimum-duration policy, a fallback, or per-length calibration may be needed — each with different architectural consequences.

Experiment scheduled after the current model retrain completes.

---

## 7. Deliberate Non-Goals

**`[SPEC-SA-100]`** Sampo does **not** play audio, serve the player UI, run on ARM, run on the appliance, or hold listener state `[SPEC-DF-055]`. It writes facts about music; Vaino owns everything about the listener.

**`[SPEC-SA-105]`** No microservices. McRhythm split ingest across services and reached 71,321 lines with duplicate parallel hierarchies `[GDE-MCR-030]`. Sampo is one process with staged, resumable steps until a measured constraint says otherwise `[GDE-FBD-050]`.

---

**Traceability:** `[SPEC-SA-010..105]` · derived from `[GDE-ARC-010]`, `[GDE-ARC-015]`, `[GDE-CHT-045]`, `[GDE-FEX-062]`, `[GDE-MCR-010]`

# GUIDE002: Vaino Re-Architecture Plan

**Development Guidance — Tier 0**

The ground-up re-think, re-specification and re-implementation plan for Vaino, derived from the measured evidence in [GUIDE001: Lineage & Lessons](GUIDE001-lineage-and-lessons.md). Every decision below cites the lesson that forced it.

> **Critical path:** local feature extraction is the first target. Its detailed strategy lives in [GUIDE003: Feature Extraction Strategy](GUIDE003-feature-extraction-strategy.md).

---

## 1. Design Charter

**`[GDE-CHT-010]` Match or beat MuLibPlay on every axis.** It runs at 171 MB RSS / 15% CPU on a Pi 4, serving a 44 GB library with a 4-hour DAO file, and it has not needed attention in six years. That is the bar.

**`[GDE-CHT-020]` Fix new-music induction.** This is the reason for the project. Today it is undocumented external manual labor `[GDE-BMK-050]`. McRhythm proved a 93%-accurate automated segmenter is achievable `[GDE-MCR-010]` — but segmentation is worthless without flavor data for what it finds, which is why extraction comes first `[GDE-PHS-000]`.

**`[GDE-CHT-030]` Make every process visible.** The headline requirement. Three things must be inspectable rather than inferred:
- **Why this track?** — the full weight decomposition behind each selection.
- **How was this identified?** — the ingest decision record for every passage.
- **Is this data trustworthy?** — provenance and a live accuracy scorecard for every descriptor.

**`[GDE-CHT-040]` Ruthless scope discipline.** McRhythm was killed by surface area, not by technology `[GDE-MCR-030]`. Every new module, service, or abstraction must justify itself against a measured constraint.

**`[GDE-CHT-045]` Vaino must eventually be distributable — self-contained, importing someone else's collection.** Not a personal appliance only. This is a first-class constraint, not a future nicety, because it forecloses whole classes of design:

- **No dependence on a bulk external corpus.** Shipping or referencing the 37 GB AcousticBrainz dump is not viable, and its API is dead `[GDE-MCR-045]`, so a recipient cannot fetch what they lack. Another user's library has unknown — plausibly poor — dump coverage.
- **Therefore local extraction is architecturally mandatory**, not merely preferable `[GDE-FEX-027]`. It is the only import path that works on a machine that has never seen our development data.
- **Import is incremental by default, not a one-time batch.** Users add tracks as they collect them, so import must be routine and repeatable rather than a first-run ordeal: resumable, interruptible, progress-reporting, and cheap to re-enter. A 10,000-track library is an acceptable overnight job at the measured 27 s/track, but it is the unrepresentative worst case — most users start at ~1,000 tracks or fewer `[GDE-FEX-135]`.
- **Never re-decode audio to improve a classifier.** Lowlevel features are cached permanently on first import; a better model later is a re-run of classification only, over cached features.

**`[GDE-CHT-050]` Inherit McRhythm's requirements; reject its architecture.** Its functional specifications are the most refined in the lineage `[GDE-MCR-050]` and become Vaino's primary functional reference. Its 6-service structure does not `[GDE-ARC-010]`.

---

## 2. Architectural Decisions

### `[GDE-ARC-010]` Two binaries, split by runtime constraint

Not six services. Not one monolith. The split follows the one boundary that physically exists — *what must run on a 512 MB appliance, and what never will*:

> **Sampo** — in the *Kalevala*, the mill forged by Ilmarinen the smith that ground out flour, salt and gold: abundance from raw material. A separate artifact, made by a different hand, that Väinämöinen nonetheless depends on and ultimately sails to reclaim. The naming carries the architecture: Sampo grinds raw audio into descriptive wealth, and it is its own entity — separate repository, separate licence, separate platform — that Vaino consumes but never contains.

| | **`vaino`** (player) | **`sampo`** (library builder) |
| :--- | :--- | :--- |
| **Runs on** | Pi Zero 2W + desktop, portable | Desktop **x86 only** |
| **Licence** | **MIT** | **AGPL-3.0** |
| **Does** | Playback, crossfade, Program Director, web UI, WebSocket | Scanning, fingerprinting, MusicBrainz, DAO segmentation, feature extraction, review UI |

**`[GDE-ARC-015]` Three independent boundaries fall on the same seam**, which is the strongest available evidence that the seam is real:

1. **Runtime** — what must fit in 512 MB versus what never runs on the appliance.
2. **Platform** — the Essentia extractor is x86-only `[GDE-FEX-137]`; the player must reach ARM.
3. **Licence** — Essentia is AGPL; the player must stay MIT and freely portable `[GDE-FEX-139]`.

They interoperate as a system but remain separate entities: separate processes, communicating only through the shared SQLite file. Nothing AGPL is ever linked into `vaino`.

**`[GDE-ARC-018]` Licence direction matters, so keep the shared code MIT.** The schema/DAO layer both binaries use stays MIT. MIT code may be incorporated into an AGPL work; the reverse is not true. `sampo` therefore takes on AGPL while `vaino` remains unaffected.

Note this is deliberately conservative: `sampo` *invokes* the extractor as a subprocess rather than linking it, which is generally aggregation rather than derivation. Relicensing anyway removes the question instead of arguing it. The distilled classifiers are separable in any case — they are models trained on AcousticBrainz data, not Essentia code `[GDE-FEX-139]`.

Per `[GDE-LES-050]`, no further decomposition without a measured constraint demanding it.

### `[GDE-ARC-020]` Rust for Vaino, Python for Sampo

**Player → Rust** (`symphonia` + `rubato` + `cpal` + `axum`). Justification is the 512 MB target: it requires the streaming decoder design `[GDE-LES-010]`, and McRhythm's `wkmp-ap` is ~27K lines of working, specified implementation of exactly that design `[GDE-MCR-020]` available to port. Python's whole-file decode is what broke v1 `[GDE-V1-030]`.

**Sampo → Python.** It never runs on the appliance, the Essentia / fingerprinting / MusicBrainz ecosystem lives there, and McRhythm's 71K-line Rust ingest service is precisely the component that collapsed `[GDE-MCR-030]`. Library building is bursty batch work where iteration speed matters far more than microseconds.

**Honest risk:** Rust is also what stalled McRhythm. The mitigation is scope, not language — one binary instead of six, with a proven design to port rather than invent.

### `[GDE-ARC-030]` Flavor is the full 71-dimension vector, extensible by the user

Adopt McRhythm's Musical Flavor model `[GDE-MCR-060]` rather than MuLibPlay's 11 scalars: 18 classifiers / 71 dimensions, binary and complex characteristics, plus user-defined characteristics computed identically. The user-defined mechanism replaces MuLibPlay's hardcoded `[C]`/`[W]`/`[S]`/`[K]` occasion multipliers `[GDE-PD-020]` with something general.

Preserve both documented asymmetries: **distance over intersecting characteristics**, **taste over the union centroid**.

Storage must accommodate partial vectors — many recordings will have 11 known dimensions (from `mulib.db`), some 71 (from the dumps), some locally computed. Per-characteristic provenance, not per-track.

### `[GDE-ARC-040]` Restore the relational entity model

Return to what MuLibPlay proved over six years, reconciled with McRhythm's entity definitions `[GDE-MCR-050]`:

```
files ──< passages >── recordings ──< artists
  │      (Album / Radio)      │
  │                        albums
  └── content signature (relocatable)

play_history · programs · flavor(+provenance) · taste · ingest_decisions
```

Mandatory properties:
- **`files.signature`** — content hash, so moving the library never breaks the database `[GDE-BMK-050]`.
- **Album/Radio passage duality** `[GDE-BMK-030]` — restored. The Program Director selects Radio passages only.
- **Per-characteristic provenance** — `acousticbrainz-dump-20220623` | `computed:<extractor>@<version>` | `manual` | `inherited:mulib`. Non-nullable. This alone would have caught `[GDE-V1-010]` on day one.
- **Do not create** `tempo`, `intensity`, `keyMood`, `darkLight`, `genre`, `themes` `[GDE-BMK-040]`.

### `[GDE-ARC-050]` Bounded audio buffers — a hard rule

Port `wkmp-ap`'s design directly `[GDE-MCR-020]`: per-passage `decoder → resampler → fader → ring buffer`, ~15 s / ~5.3 MB each, mixer into an output ring buffer.

**Enforcement:** an automated test that plays the 244.9-minute `GoodbyeYellowBrickRoad.mp3` and fails the build if RSS exceeds 150 MB or skip latency exceeds 500 ms. Not a guideline — a gate.

### `[GDE-ARC-060]` Predecessors are study material with a disposal date

`vaino.db`, the Vaino v1 Python implementation, the abandoned Go port, and the v1 specifications are **learning artifacts, not foundations**. They are retained only while they still teach something, then deleted outright — not archived, not partially ported, not left to confuse future readers `[GDE-LES-040]`.

Each carries an explicit open question recording what remains to be learned from it `[GDE-DIS-010]`. When that list empties, the artifact goes.

---

## 3. Phased Plan

Each phase is independently useful, independently testable, and reports a measurable result.

### `[GDE-PHS-000]` P0 — Local Feature Extraction ⭐ **FIRST TARGET**

**The highest-risk unknown in the project, and 729 tracks already depend on it.** That is the count of tracks in `vaino.db` with no counterpart in `mulib.db` — real music already in the library whose classification, and therefore whose induction into the selection system, has no trustworthy basis today.

It is also the dependency that makes everything else worth building: a segmenter that finds passages it cannot characterize has not solved the induction problem.

Full strategy in **[GUIDE003](GUIDE003-feature-extraction-strategy.md)**. In outline:
1. **Mirror the 2022-06-23 AcousticBrainz dumps immediately** `[GDE-MCR-045]` — the API died within seven months of a successful bulk query; the dumps are the last copy and are not guaranteed to persist.
2. Extract the library's recording MBIDs from the dumps → 71-dimension ground truth, vastly richer than `mulib.db`'s 11 `[GDE-MCR-060]`, and directly usable as production flavor data.
3. **Reproduce AcousticBrainz's own pipeline rather than approximate it** — Essentia `streaming_extractor_music` plus the published highlevel classifier models. The paired lowlevel+highlevel dumps allow verifying each stage independently.
4. Measure, analyze, iterate `[GDE-PHS-005]`.

> **Reports:** per-characteristic Pearson r against held-out ground truth, published as a standing scorecard in the UI `[GDE-CHT-030]`.

### `[GDE-PHS-005]` Extraction quality is best-effort and iterative, not pass/fail

There is **no ship/no-ship threshold.** The discipline being enforced is *measurement and honest reporting* `[GDE-LES-030]` — the absence of which is what let v1's failure hide `[GDE-V1-010]`. The discipline is not a number.

- A good score on the first attempt is a reason to analyze what worked and what didn't and **try again** — not a reason to stop.
- **Calibrate against the measured ceiling.** AcousticBrainz agrees with *itself* at only r ≈ 0.82 across submissions of the same recording `[GDE-FEX-085]`. Targets above that measure encoding noise, not extractor quality. Settling at **r ≈ 0.82 after genuinely exhausting the available ideas is not a failure — it is approximately as good as the target data permits.** Record the approaches tried and why each plateaued.
- Every iteration is logged with its approach, its per-characteristic scores, and its analysis of strengths and weaknesses. The iteration history is itself a deliverable.
- Whatever accuracy is reached, the value ships **with its provenance and its measured accuracy attached** `[GDE-ARC-030]`, so downstream consumers and the user can judge it.

### `[GDE-PHS-010]` P1 — Data Foundation

Schema per `[GDE-ARC-030..040]`, plus a **lossless importer from `mulib.db`**. The migration is the schema's first and best test: if the model cannot hold six years of real production data, it is the wrong model.

Three flavor sources now exist, in descending order of authority — the schema must hold all three with per-characteristic provenance `[GDE-ARC-030]`:

| Source | Coverage | Dims | Role |
| :--- | ---: | ---: | :--- |
| AcousticBrainz dump `[GDE-FEX-055]` | 8,001 recordings (93.7%) | **71** | Reference / ground truth `[SPEC-FD-150]` |
| `mulib.db` `abXxx` | 8,062 recordings | 11 | Fallback and cross-check only |
| Sampo local extraction | all | 71 | **Production values** `[SPEC-FD-145]` |

The dump is *not* the production source. Mixed provenance measurably degrades similarity `[SPEC-FD-140]`, so production flavor is uniformly locally extracted and the dump serves as the yardstick.

Otherwise carry over `[GDE-LES-060]` unchanged: 37,134 play events, 16,232 verified cut boundaries, 2,918 tuned rotation/recovery/restraint settings, 8 programs with their seed tracks. These have no other source.

> **Reports:** every non-dead field round-trips; row counts reconcile exactly; the 11-D vectors match to 1e-9; dimension coverage per recording.

### `[GDE-PHS-020]` P2 — The Player

Rust streaming engine ported from `wkmp-ap`. Decode, resample, crossfade, queue, WebSocket state push, minimal web UI.

> **Reports:** plays every file in the 44 GB library including the 244.9-minute DAO file, at **≤150 MB RSS** and **≤500 ms skip latency** `[GDE-ARC-050]`. Runs 72 hours unattended without leak or drift.

### `[GDE-PHS-030]` P3 — Program Director + Visibility

Port the MuLibPlay math exactly `[GDE-PD-010..050]` — log-scale rotation, multiplicative eligibility, seasonal occasions, length bonus, two-stage acoustic shaping, rank-decayed roulette. `src/audio/selector.py` `[GDE-V1-060]` is a good reference port and a useful cross-check.

Then extend toward McRhythm's model `[GDE-MCR-070]`: Like/Dislike with click-stacking and undo, Like-Taste and Dislike-Taste centroids, dislike-as-exclusion-filter. Note that McRhythm explicitly left the Taste→selection coupling undefined — that is Vaino's design work, not an inheritance.

Build the **"Why this track?" panel** `[GDE-CHT-030]`: artist weight, rotation block state, position on the recovery ramp, occasion multiplier, length bonus, distance to each seed, final rank and roulette position — plus the runners-up that lost.

> **Reports:** given a frozen play history and a fixed RNG seed, the port reproduces MuLibPlay's selections. Diverge only deliberately, and record why.

### `[GDE-PHS-040]` P4 — Ingest & DAO Segmentation

**Reproduce** McRhythm's cascade `[GDE-MCR-010]` — not merely port it. The inherited [MCR-SPEC033](inherited/mcrhythm/MCR-SPEC033-album_matching.md) describes *McRhythm's implementation*, not Vaino's contract, so reproduction has three deliverables in order:

1. **Requirements** — what Vaino demands of segmentation, independent of how McRhythm did it. Source material: MCR-SPEC033, MCR-IMPL005.
2. **Specification** — Vaino's own cascade spec: grid search → DP assembly → RMS quiet-spot → extra merging, 7-strategy MusicBrainz edition search, windowed-dB-profile optimization, Stage-6 RMS boundary refinement.
3. **Implementation + review UI** — waveform with draggable boundaries, lead-in/lead-out markers and gain `[SPEC-SA-080]`.

Trim points and segue frames are **computed**, from the inherited amplitude analysis [MCR-SPEC025](inherited/mcrhythm/MCR-SPEC025-amplitude_analysis.md) `[SPEC-SA-075]` — this supplies the Radio side of the Album/Radio duality `[GDE-BMK-030]` that MuLibPlay only ever produced by hand. Automatic placement is always reviewable and overridable; manual edits outrank computed values permanently.

Every decision persisted as an inspectable **ingest decision record** — which stage matched, at what confidence, which editions were considered and rejected `[GDE-CHT-030]`. This turns "undocumented ritual" into "reviewable process".

> **Reports:** measured against McRhythm's 93% album match / 96% mean boundary accuracy `[GDE-MCR-010]` on the 189 known DAO files. **These must be independently re-verified** — at present they are simultaneously the target and the only evidence, which is not a test.

### `[GDE-PHS-050]` P5 — Appliance

Fast boot to first audio, 3-partition resilient storage, Wall Art / kiosk display, Bluetooth and D/A HAT output.

> **Reports:** power-on to first audio under the target in [REQ001](spec/REQ001-system-requirements.md); survives repeated hard power loss without database corruption.

---

## 4. Forbidden Patterns

Violations are build failures or review rejections, not style opinions. Each is a scar from a specific measured failure.

| Rule | Because |
| :--- | :--- |
| **`[GDE-FBD-010]`** No whole-file decode into memory. Ever. | `[GDE-V1-030]` — 5.2 GB for one file |
| **`[GDE-FBD-020]`** No flavor value without per-characteristic provenance. | `[GDE-V1-010]` — hid a total extraction failure |
| **`[GDE-FBD-030]`** No ML output without a current measured accuracy figure attached. | `[GDE-V1-020]` — four defects, zero validation |
| **`[GDE-FBD-040]`** No two implementations of one component. Delete the loser. | `[GDE-MCR-030]`, `[GDE-V1-050]` |
| **`[GDE-FBD-050]`** No new service or process without a measured constraint requiring it. | `[GDE-MCR-030]` — six services, no benefit |
| **`[GDE-FBD-060]`** No schema field without a consumer at merge time. | `[GDE-BMK-040]` — six years of NULLs |
| **`[GDE-FBD-070]`** No re-deriving data that already exists and is correct. | `[GDE-LES-060]` |
| **`[GDE-FBD-080]`** No dependency on a live external service without a local mirror. | `[GDE-MCR-045]` — API died in seven months |

---

## 5. Disposal Register

Per `[GDE-ARC-060]`, predecessors are retained only while they still teach. Each entry is deleted when its column empties.

**`[GDE-DIS-010]` Outstanding learning value:**

| Artifact | Still to be learned from it |
| :--- | :--- |
| `vaino.db` | 74,299 play-history rows (vs MuLibPlay's 37,134) — reconcile the surplus and establish which are genuine. The 2,279 DAO slice boundaries — compare against McRhythm's segmenter output as a test case. The 729 novel tracks — the P0 target population. |
| `src/audio/selector.py` | Reference cross-check for the P3 port `[GDE-V1-060]`. |
| `src/db/dao_slicer.py`, `resolver.py`, `acoustic_resolver.py` | Whether any identification heuristic here outperforms McRhythm's cascade. Probably not — verify, then discard. |
| `go/` (3,481 lines) | **Nothing.** Incomplete, duplicative `[GDE-V1-050]`. **Delete now.** |
| `docs/spec/SPEC004-go-migration-guide.md` | **Nothing.** Delete with `go/`. |
| v1 `docs/spec/*` — including the uncommitted edits to `REQ001`, `SPEC002`, `SPEC003` and the untracked `SPEC004-go-migration-guide.md` | **Historical, informational, and on the disposal path.** These are working-tree remnants of v1 thinking, not live specifications. Retain only until every idea of value has been extracted into the current document set, then delete. `SPEC004-go-migration-guide.md` goes with `go/` — nothing to extract. |

**`[GDE-DIS-020]`** Deletion is deletion — removed from the working tree, recoverable from git history if ever needed. Do not leave `_old`, `_v1`, or `legacy` directories; that is exactly the pattern McRhythm's own debt analysis flagged as CRITICAL `[GDE-MCR-030]`.

---

## 6. Open Questions

1. ~~**`[GDE-OPN-010]` How complete is dump coverage?**~~ — **ANSWERED 2026-08-09: 93.7%** (8,001 of 8,542 recordings) `[GDE-FEX-055]`. Misses skew post-2013 by 2.3×, so a recipient's newer library will fare worse. Note this no longer sizes the extraction work — three separate findings made local extraction mandatory regardless `[GDE-FEX-025]`, `[GDE-FEX-027]`, `[SPEC-FD-140]`.
2. **`[GDE-OPN-020]` Rust for the player — confirm or reconsider?** The evidence supports it `[GDE-ARC-020]`, but McRhythm's stall is a real counter-signal worth weighing explicitly before P2 starts.
3. **`[GDE-OPN-030]` How should Taste feed the Program Director?** McRhythm specified the Taste model but explicitly deferred this coupling `[GDE-MCR-070]`. It interacts with MuLibPlay's seed-track shaping `[GDE-PD-050]`, which already does something similar by different means. Genuine new design work.
4. **`[GDE-OPN-040]` Which user-defined characteristics to define first?** `[GDE-ARC-030]` supports them generally; MuLibPlay's six years of use suggest christmas / winter / summer / kids are the proven ones `[GDE-PD-020]`.

---

**See also:** [GUIDE001: Lineage & Lessons](GUIDE001-lineage-and-lessons.md) · [GUIDE003: Feature Extraction Strategy](GUIDE003-feature-extraction-strategy.md) · [GOV001: Document Hygiene](GOV001-document-hygiene.md)

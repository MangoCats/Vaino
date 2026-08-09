# GUIDE001: Project Lineage & Lessons Learned

**Development Guidance — Tier 0**

Captures the measured state of the three predecessor systems (MuLibPlay, McRhythm/wkmp, Vaino v1) as of **2026-08-08**, and the evidence-backed lessons that constrain the Vaino re-implementation. Companion document: [GUIDE002: Re-Architecture Plan](GUIDE002-rearchitecture-plan.md).

> Every claim below is sourced from live inspection of the running deployment, the three repositories, and their databases. Measurements, not impressions.

---

## 1. The Lineage

| System | Language / Shape | Status | What it is |
| :--- | :--- | :--- | :--- |
| **MuLibPlay** | Qt5 C++, single process | **Live** on `pi@bose.lan` since 2020 | The working benchmark. Everything must equal or beat it. |
| **McRhythm / wkmp** | Rust, 6 microservices, ~100K LOC | Stalled Feb 2026 | Ambitious re-imagining. Solved DAO segmentation; drowned in scope. |
| **Vaino v1** | Python/FastAPI + partial Go port | Current repo | Quick stab at a simpler McRhythm. Plays audio; accuracy and latency are unacceptable. |

---

## 2. MuLibPlay — The Benchmark (what "good" means)

**`[GDE-BMK-010]` Live deployment measurements** (`pi@bose.lan`, Raspberry Pi 4, armv7l, 1884 MB RAM, kernel 5.15):

- Process RSS **171 MB**, 15.2% CPU, HTTP control UI on port 1160.
- Library: **44 GB / 7,238 files**, largest single file **340 MB / 244.9 minutes** (`GoodbyeYellowBrickRoad.mp3`).
- 6 years of continuous operation. Play history spans **2020-03-26 → present, 37,134 events**.

**`[GDE-BMK-020]` Database content** (`mulib.db`, 95 MB):

| Entity | Count | Note |
| :--- | ---: | :--- |
| tracks | 8,116 | |
| cuts | 16,232 | Exactly 2 per track — see `[GDE-BMK-030]` |
| files | 5,615 | 189 are true DAO files (>1 distinct track) |
| albums / artists | 675 / 470 | |
| playHistory | 37,134 | |
| AcousticBrainz vectors | 8,062 (99.3%) | Downloaded before shutdown. **Irreplaceable.** |
| rotation/recovery/restraint set | ~2,918 (36%) | Hand-tuned user preference |

Max tracks in one DAO file: **40** (`Purple.mp3`, `WhatsTheStoryMorningGlory.mp3`).

**`[GDE-BMK-030]` The Album/Radio cut duality — the single best schema idea in the lineage.**
Every track carries two `cuts` rows against the same file:
- **`Album`** — full boundaries, `gain = 1.0`, segue points equal to the hard boundaries.
- **`Radio`** — trimmed boundaries, distinct `startSegueFrame`/`endSegueFrame`, computed `gain` (~0.70–0.75 observed) for loudness matching.

The Program Director selects **only `Radio` cuts**. One recording, multiple presentations, one file. Vaino v1 discarded this; it must come back.

**`[GDE-BMK-040]` Dead schema — do not port.** The following `tracks` columns have been NULL for all 8,116 rows across six years: `tempo`, `intensity`, `keyMood`, `darkLight`, `genre`, `themes`. `quality`, `jts`, `popularity` have ≤10 rows. They were aspirational and never used. Six years of production is conclusive evidence.

**`[GDE-BMK-050]` The confirmed weak spot — there is no ingest.** `MaintController::scanFile` ([maintController.cpp:380](../../MuLibPlay/maintController.cpp)) does exactly one thing: hash a file, `SELECT * FROM files WHERE sig = :hash`, and update `filePath` if it matches. It **relocates known files**. It cannot induct new music. MuLibPlay contains no MusicBrainz client, no fingerprinting, no segmentation. The entire new-music process is external manual labor — which is precisely why it is undocumented, unrepeatable, and easily forgotten.

---

## 3. MuLibPlay's Selection Algorithm — Preserve Exactly

Source: [musicdirector.cpp](../../MuLibPlay/musicdirector.cpp). This algorithm produces the good selections. It is the most valuable transferable asset in the lineage after the data itself.

**`[GDE-PD-010]` Log-scale time encoding.** `rotationValueToSeconds(rv) = 10^rv × 3600`. So `-1.0` → 6 min, `0.0` → 1 hour, `1.0` → 10 hours. One tunable float spans four orders of magnitude.

**`[GDE-PD-020]` Multiplicative eligibility weighting**, applied to artists first, then tracks:
1. Base weight `w = 10^(-restraint)`.
2. **Hard block** if `now - lastPlay < rotationSeconds`.
3. **Linear recovery ramp** otherwise: `w *= (age - rotSecs) / recSecs`, clamped to `[0, 1]`.
4. Track weight multiplies its artist's weight. Related tracks (`relatedTracks`) also block and damp, scaled by relation strength.
5. Seasonal `occasionWeight()` multiplier — `[C]`hristmas, `[W]`inter, `[S]`ummer, `[K]`ids.
6. Drop below `minWeightLimit = 0.001`.

**`[GDE-PD-030]` Cut-level filters and length bonus.** Radio cuts only; reject `< 30 s`, `> 3600 s`, or starting `> 10800 s` into a file. Then `w *= sqrt(min(4.0, 180s / cutLength))` — a mild preference for ~3-minute cuts, capped at 2.0× for short ones.

**`[GDE-PD-040]` Programs are defined by seed tracks, not by tuned curves.** Eight time-of-day programs, each a list of 6–8 exemplar track IDs:

| Time | Program | | Time | Program |
| :--- | :--- | :-- | :--- | :--- |
| 04:00 | Soft | | 15:00 | Groove |
| 10:00 | Light | | 16:30 | Fun |
| 12:00 | Cool | | 19:00 | Prog |
| 14:00 | Loud | | 22:00 | Mellow |

"What should 10 AM sound like?" is answered by naming six songs. This is dramatically more usable than tuning eleven sliders, and it should be kept.

**`[GDE-PD-050]` Two-stage acoustic shaping over the 11-D AcousticBrainz vector** (`danceable, female, acoustic, aggressive, happy, party, relaxed, sad, bright, tonal, instrumental`; plain squared Euclidean distance, unweighted):
1. **Prune** — down-select ≤5 seeds (one per artist, least-recently-played), then remove the tracks *most unlike* each seed until the pool reaches ~1,000.
2. **Gather** — take the ~100 tracks *most like* each seed.
3. **Flow** — re-sort that pool by similarity to the **last track already in the queue**, so consecutive songs blend.
4. **Roulette** — take the top 100, apply rank decay `simWt *= 0.96` per position, then weighted-random pick.

Randomness is applied *last*, over an already-shaped pool. The character comes from the shaping; the surprise comes from the roulette.

---

## 4. McRhythm / wkmp — What Worked, What Killed It

**`[GDE-MCR-010]` The DAO segmenter genuinely works.** Measured over a 200-album test ([STAGE6_FULL_TEST_RESULTS_20260109.md](../../McRhythm/STAGE6_FULL_TEST_RESULTS_20260109.md)):

- **93.0%** album match rate (186/200); 7 genuine failures, 7 intentionally skipped.
- **96.0%** mean track-boundary match; **67.2%** of matched albums perfect (100%).
- Stage-6 RMS boundary refinement improved **114 of 186** albums with zero regressions.
- ~180× speedup by computing one windowed dB profile and filtering it 180 ways, instead of 180 audio scans. 87.6% of albums early-exit at a median of 4 parameter combinations tried out of 180.

Architecture: a cascade — parameter grid search → dynamic-programming assembly (over-segmented) → RMS quiet-spot (vinyl/cassette) → extra merging (bonus tracks) — plus a 7-strategy MusicBrainz edition search (CamelCase splitting, fuzzy edit distance, per-token fuzzy, album-only fallback). **This is the fix for MuLibPlay's weak spot and it already exists.**

**`[GDE-MCR-020]` The audio player design is correct.** [SPEC016](../../McRhythm/docs/SPEC016-decoder_buffer_design.md): per-passage `decoder → resampler → fader → ring buffer` chains, ~15 s buffered per passage (**5.3 MB** at 44.1 kHz stereo f32), mixer draining into an output ring buffer, target **≤150 MB total RSS** on a 512 MB Pi Zero 2W. Streaming, bounded, never whole-file.

**`[GDE-MCR-030]` What killed it: scope multiplied by six.** From the project's own [TECHNICAL_DEBT_ANALYSIS.md](../../McRhythm/TECHNICAL_DEBT_ANALYSIS.md):
- Six microservices (`ap`, `ui`, `pd`, `ai`, `le`, `dr`) each with its own HTTP server, all against one SQLite file.
- The ingest service alone reached **71,321 lines of Rust**.
- **Two complete parallel extractor hierarchies** (`src/extractors/` and `src/fusion/extractors/`) — flagged CRITICAL by the project's own analysis.
- Three coexisting pipeline generations (legacy, PLAN024, PLAN025) in the same tree.
- 2,253-line orchestrator; 141 gratuitous `clone()` calls; 26 files with no tests.

The technology was fine. The surface area was not. A 6-service split for a single-user appliance bought nothing and cost everything.

**`[GDE-MCR-040]` AcousticBrainz shutdown is the unsolved blocker.** With the service gone, descriptors for new music must be computed locally. Neither McRhythm nor Vaino v1 ever landed a validated local extractor. **This is the true critical-path problem of the whole lineage** — see [GUIDE003](GUIDE003-feature-extraction-strategy.md).

**`[GDE-MCR-045]` The live API died between January and August 2026 — measured.** McRhythm successfully queried `acousticbrainz.org` on **2026-01-01**, retrieving data for 2,427 of 2,664 recordings (91.1% coverage) ([acousticbrainz_coverage_report.md](../../McRhythm/acousticbrainz_coverage_report.md)). Retested **2026-08-08**: every request returns HTTP 500 or times out. That window has closed.

**The 2022-06-23 archival dumps are still served** from `data.metabrainz.org/pub/musicbrainz/acousticbrainz/dumps/` — `highlevel-json` and `lowlevel-json`, roughly 30 shards of 1–2 GB each. Given that the API died without notice, these should be treated as **also at risk and mirrored promptly**.

**`[GDE-MCR-050]` McRhythm's functional requirements are the most refined in the lineage — inherit them.** Unlike Vaino v1's specifications (written quickly, largely uncontested), McRhythm's received sustained investment and revision:

| Document | Lines | Contributes |
| :--- | ---: | :--- |
| [REQ001-requirements.md](../../McRhythm/docs/REQ001-requirements.md) | 731 | 15 revision commits. Covers ground Vaino's 116-line REQ001 does not: queue-empty behaviour, play history, network status, user identity, offline operation, error handling, library edge cases, three build tiers |
| [REQ002-entity_definitions.md](../../McRhythm/docs/REQ002-entity_definitions.md) | 247 | Passage / Song / Recording / Work entity model |
| [SPEC003-musical_flavor.md](../../McRhythm/docs/SPEC003-musical_flavor.md) | 188 | See `[GDE-MCR-060]` |
| [SPEC005-program_director.md](../../McRhythm/docs/SPEC005-program_director.md) | 471 | Selection algorithm design |
| [SPEC004-musical_taste.md](../../McRhythm/docs/SPEC004-musical_taste.md) · [SPEC006-like_dislike.md](../../McRhythm/docs/SPEC006-like_dislike.md) | 203 | Taste model, Like/Dislike semantics |

For scale: McRhythm's REQ001 alone (731 lines) is more than a third the size of Vaino's entire `docs/` tree (2,049 lines).

**`[GDE-MCR-060]` Musical Flavor is a genuine advance over MuLibPlay's 11 numbers.** [SPEC003](../../McRhythm/docs/SPEC003-musical_flavor.md) defines flavor as the **full AcousticBrainz highlevel vector — 18 classifiers, 71 dimensions**, not the 11 scalars MuLibPlay stores:

- **Binary characteristics** (2 dims summing to 1.0): `danceability`, `gender`, `mood_*`, `timbre`, `tonal_atonal`, `voice_instrumental`.
- **Complex characteristics** (3+ dims summing to 1.0): `genre_dortmund` (9), `genre_tzanetakis` (10), `genre_rosamerica` (8), `genre_electronic` (5), `ismir04_rhythm` (10), `moods_mirex` (5). **None of these exist in `mulib.db`.**
- **User-defined characteristics** — e.g. `user.christmas.all.{christmasy, not_christmasy}`, `user.seasonal_affinity.all.{winter, spring, summer, fall}` — computed and treated identically to the built-in ones. This is a clean generalization of MuLibPlay's hardcoded `[C]`/`[W]`/`[S]`/`[K]` occasion hack `[GDE-PD-020]`, and it is a better idea.

Two deliberate, documented asymmetries worth preserving:
- **Flavor distance uses only *intersecting* characteristics** — never assume missing data is zero when comparing two specific items.
- **Taste uses the *union* centroid** — build the broadest possible profile when aggregating many items.

**`[GDE-MCR-070]` The Like/Dislike model is well thought through.** [SPEC006](../../McRhythm/docs/SPEC006-like_dislike.md): two centroids (Like-Taste and Dislike-Taste) producing two ranked lists, with the dislike list usable as an exclusion filter — "well-liked yet potentially unexpected". Click-stacking within a 5-minute window increases weight; the opposite button acts as undo; a detail panel exposes and permits direct editing of the resulting float weights. Passage-level actions are distributed across constituent songs. Note its own caveat: how Taste feeds the Program Director is explicitly *left undefined*.

---

## 5. Vaino v1 — Measured Failures

**`[GDE-V1-010]` The descriptors were inherited, not computed.** Direct comparison of `vaino.db` against `mulib.db`, joined on MusicBrainz recording ID (7,648 tracks matched):

| Descriptor | n | Pearson r | MAE |
| :--- | ---: | ---: | ---: |
| ab_acoustic | 7,643 | 0.984 | 0.006 |
| ab_happy | 7,643 | 0.985 | 0.005 |
| ab_danceable | 7,643 | 0.981 | 0.007 |
| *(all 11 dimensions)* | 7,643 | **0.978 – 0.992** | **0.004 – 0.008** |

That is not agreement — that is **copying**. **7,481 of 8,215 rows (91%) are bit-identical** to MuLibPlay's values to within 1e-6. Only **292 rows (3.6%)** carry the ONNX extractor's signature (3-decimal rounding, `[0.02, 0.98]` clamp). The claim that Essentia/ONNX replaced AcousticBrainz was never tested at scale; the data looks excellent only because it was borrowed from its predecessor.

**`[GDE-V1-020]` The ONNX extractor is technically broken.** [src/audio/onnx_extractor.py](../../Vaino/src/audio/onnx_extractor.py), four independent defects, any one of which invalidates the output:

1. **Decimation is not resampling.** `samples[::int(sr/16000)]` gives `step = 2` for 44.1 kHz → **22,050 Hz**, then labels it 16,000 Hz. No anti-alias filter, so the result is aliased *and* time-scaled.
2. **A 3-second sample stands in for the whole track.** 187 frames × 256 hop ≈ 2.99 s, taken from the file's midpoint. AcousticBrainz aggregates over the entire recording.
3. **Wrong compression.** `log10(max(1e-5, magnitude))` on an unnormalized filterbank, where Essentia's `TensorflowInputMusiCNN` uses `log10(1 + 10000·x)` on normalized power mel. MusicNN receives out-of-distribution input.
4. **Discrimination discarded** by clamping to `[0.02, 0.98]` and rounding to 3 decimals.

**`[GDE-V1-030]` The lag has one cause: whole-file decode.** `AudioEngine._load_audio_file` calls `miniaudio.decode_file(file_path)` — the entire file into a NumPy array — while holding `self._lock`, from a synchronous FastAPI handler ([engine.py:226](../../Vaino/src/audio/engine.py)). Play/skip latency is therefore proportional to *whole-file* decode time.

For this library's largest file (244.9 min): **~2.6 GB** decoded at int16, **~5.2 GB** at float32. That is not slow on a Pi Zero 2W — it is impossible. Compare McRhythm's 5.3 MB streaming buffer `[GDE-MCR-020]`. **A ~1000× memory difference.**

**`[GDE-V1-040]` Schema regression.** A single flat `tracks` table with `start_offset_ms`/`end_offset_ms` replaced MuLibPlay's `files → cuts → tracks → artists/albums` model. Consequences: no Album/Radio duality `[GDE-BMK-030]`, no `files` entity, no content signature (so files cannot be relocated), `track_relations` **empty (0 rows)**, `musicbrainz_album_id` on only 2,448/8,216 (30%).

**`[GDE-V1-050]` Two half-implementations.** A Python implementation (4,472 lines) and an abandoned Go port (3,481 lines) of the same system coexist in one repo — the same duplication pattern that McRhythm flagged as CRITICAL `[GDE-MCR-030]`.

**`[GDE-V1-060]` What v1 got right — keep it.** `src/audio/selector.py` is a faithful, readable port of the MuLibPlay weighting math `[GDE-PD-010..030]`, including the log-scale rotation encoding and the seasonal occasion multipliers. WebSocket push already exists. These are worth carrying forward.

---

## 6. The Lessons, Distilled

| # | Lesson | Evidence |
| :--- | :--- | :--- |
| **`[GDE-LES-010]`** | **Never decode a whole file.** Streaming, bounded per-passage buffers are non-negotiable. | `[GDE-V1-030]` vs `[GDE-MCR-020]` |
| **`[GDE-LES-020]`** | **No descriptor value without recorded provenance.** Inherited and computed values became indistinguishable and hid a total extraction failure for months. | `[GDE-V1-010]` |
| **`[GDE-LES-030]`** | **Every ML substitution is measured against ground truth, continuously and publicly.** Not a one-time ship/no-ship gate — a standing scorecard that drives iteration. 7,648 tracks with both real AcousticBrainz values and local audio already exist as a labeled set. | `[GDE-V1-010]`, `[GDE-MCR-040]` |
| **`[GDE-LES-040]`** | **One implementation per component.** Parallel hierarchies killed McRhythm and are already reappearing in Vaino v1. | `[GDE-MCR-030]`, `[GDE-V1-050]` |
| **`[GDE-LES-050]`** | **Split by runtime constraint, not by fashion.** Six microservices for a single-user appliance bought nothing. | `[GDE-MCR-030]` |
| **`[GDE-LES-060]`** | **Migrate the data; never re-derive it.** 6 years of play history, 8,062 real AcousticBrainz vectors, 16,232 verified cut boundaries, 2,918 tuned preferences. | `[GDE-BMK-020]` |
| **`[GDE-LES-070]`** | **Don't port aspirational schema.** Six years of NULLs is a decisive verdict. | `[GDE-BMK-040]` |
| **`[GDE-LES-080]`** | **Keep the exemplar-based preference model**, and generalize it via user-defined flavor characteristics. Naming six songs beats tuning eleven sliders. | `[GDE-PD-040]`, `[GDE-MCR-060]` |
| **`[GDE-LES-090]`** | **Inherit McRhythm's functional requirements, not its architecture.** The requirements were refined over many revisions; the 6-service structure is what failed. | `[GDE-MCR-050]`, `[GDE-MCR-030]` |
| **`[GDE-LES-100]`** | **External data sources vanish without notice.** AcousticBrainz's API died within seven months of a successful bulk query. Mirror what you depend on, now. | `[GDE-MCR-045]` |

---

**Next:** [GUIDE002: Re-Architecture Plan](GUIDE002-rearchitecture-plan.md)

# GUIDE003: Feature Extraction Strategy

**Development Guidance — Tier 0 · P0 Critical Path**

Strategy for replacing AcousticBrainz with local analysis — the work that becomes **Sampo**, the separately-licensed x86 library builder `[GDE-ARC-010]`. This is the **first implementation target** `[GDE-PHS-000]` and the highest-risk unknown in the project. Derived from [GUIDE001](GUIDE001-lineage-and-lessons.md); phase context in [GUIDE002](GUIDE002-rearchitecture-plan.md).

---

## 1. Why This Comes First

**`[GDE-FEX-010]` Tracks already depending on it — two distinct populations, not one.** These are frequently confused; they overlap but neither contains the other:

| Population | Count | Definition |
| :--- | ---: | :--- |
| **Untrusted** | **729** | In `vaino.db` with no `mulib.db` counterpart — classification rests on nothing trustworthy today `[GDE-V1-010]`. Some *are* in the dump and can be repaired from it. |
| **Unreachable** | **522** | In `vaino.db` but absent from the AcousticBrainz dump `[GDE-FEX-056]`. No reference exists at any price; only local extraction can serve them. |

The union is the work; the *unreachable* set is the part no amount of harvesting can fix.

**`[GDE-FEX-020]` It gates the project's actual purpose.** A segmenter that finds passages it cannot characterize has not solved new-music induction `[GDE-CHT-020]`. Every subsequent phase either depends on flavor data or is less valuable without it.

**`[GDE-FEX-025]` Dump coverage does not reduce this priority — high coverage would be a *reprieve*, not a solution.** AcousticBrainz stopped accepting submissions in 2022 and its API is now dead `[GDE-MCR-045]`. Every future acquisition is permanently outside the dump. However well `[GDE-OPN-010]` turns out, local extraction is the only path that does not expire, so it is investigated on its merits and to its limit — not sized to whatever gap the dump happens to leave.

**`[GDE-FEX-027]` Distribution makes it mandatory, not merely preferable** `[GDE-CHT-045]`. A recipient importing their own collection cannot be asked to carry a 37 GB corpus, and cannot fetch what they lack because the API is dead `[GDE-MCR-045]`. Their coverage is unknown and plausibly poor. Local extraction is the only import path that works on a machine that has never seen our development data.

The size comparison settles it — though not where the tension actually lies:

| Shippable artifact | Size |
| :--- | ---: |
| Essentia static extractor (per platform) | **5 MB** |
| Distilled models, shared trunk + 18 heads | **2.5 MB** (1.2 MB fp16) |
| Distilled models, 18 dedicated | **44 MB** |
| *AcousticBrainz highlevel dump* | *37,000 MB* |

Worth stating plainly: **distribution size is not an argument against the dedicated per-characteristic models** that iterations 4–5 favour. At 44 MB they ship comfortably; it is the *dump* that is impossible, at ~840× the dedicated models and ~30,000× the shared one. The accuracy-vs-size question between model families stays open on its own merits `[LOG-NEXT-050]`.

**`[GDE-FEX-028]` And if local extraction merely matches the dump, local extraction wins outright** `[SPEC-FD-130]`. Similarity is a relative judgment within our library, so uniform scoring beats per-track fidelity: a library scored entirely by one model on our own files has zero encoding variance and common-mode model error, while a mixed library pays both an encoding and a model difference on every cross-provenance comparison. The dump is *itself* non-uniform — its ~0.210 err/β self-inconsistency `[GDE-FEX-085]` is exactly that.

**Measured 2026-08-09, and the prediction was half wrong** `[SPEC-FD-140]`. Mixing provenance costs ~8 points of top-1 retrieval — confirmed decisively, and it is the part that governs the design. But all-local did **not** beat all-dump (76.7% vs 77.9%); the student's approximation error slightly outweighs the encoding variance it avoids. The conclusion survives on the mixing penalty alone `[SPEC-FD-145]`: uniform-local is the only regime reachable in the general case, and it costs ~1 point against an all-dump library that cannot be built anyway.

**`[GDE-FEX-030]` It is the one problem no predecessor solved.** McRhythm specified local Essentia analysis and never landed it `[GDE-MCR-040]`. Vaino v1 claimed to have landed it and had not `[GDE-V1-010]`. Two projects have now been wrong about this in opposite directions, which is reason enough to attack it first and honestly.

---

## 2. The Target

**`[GDE-FEX-040]` Reproduce the full AcousticBrainz highlevel vector — 18 classifiers, 71 dimensions** `[GDE-MCR-060]`, not MuLibPlay's 11 scalars.

| Kind | Classifiers | Dims |
| :--- | :--- | ---: |
| **Binary** (2 dims, sum 1.0) | `danceability`, `gender`, `mood_acoustic`, `mood_aggressive`, `mood_electronic`, `mood_happy`, `mood_party`, `mood_relaxed`, `mood_sad`, `timbre`, `tonal_atonal`, `voice_instrumental` | 24 |
| **Complex** (3+ dims, sum 1.0) | `genre_dortmund` (9), `genre_tzanetakis` (10), `genre_rosamerica` (8), `genre_electronic` (5), `ismir04_rhythm` (10), `moods_mirex` (5) | 47 |

The 11 dimensions MuLibPlay stores map to one side of eleven binary classifiers (`abBright` ← `timbre.bright`, `abFemale` ← `gender.female`, `abInstrumental` ← `voice_instrumental.instrumental`, and so on). **All 47 complex dimensions are absent from `mulib.db`** and have no local ground truth `[GDE-FEX-060]`.

---

## 3. Strategy: Harvest, then Reproduce, then Approximate

Three tiers in strict priority order. Each is cheaper and more accurate than the one below it, so exhaust each before descending.

### `[GDE-FEX-050]` Tier 0 — Harvest the archived dumps ✅ **MIRRORED 2026-08-09**

**Status: complete.** 31 of 31 files downloaded and checksum-verified against MetaBrainz's published manifest — the full 30-shard highlevel dump plus the paired lowlevel sample, 41 GB. The archive is now held locally and no longer at the mercy of `data.metabrainz.org` `[GDE-FEX-120]`.

The urgency was justified: the AcousticBrainz live API worked for McRhythm on 2026-01-01 and returned HTTP 500 or timed out when retested on 2026-08-08 `[GDE-MCR-045]`. The dumps were the only remaining source:

```
https://data.metabrainz.org/pub/musicbrainz/acousticbrainz/dumps/
  acousticbrainz-highlevel-json-20220623/   ~30 shards, 1–2 GB each
  acousticbrainz-lowlevel-json-20220623/    ~30 shards, 1–2 GB each
```

Mirror both. The lowlevel dump is what makes Stage B validation possible `[GDE-FEX-090]`; the highlevel dump is the ground truth and the source of the `β_c` / `w_c` constants `[SPEC-FD-050]`. Given that the API vanished without notice, treat this as time-sensitive `[GDE-FBD-080]`.

**Revised role — validation, not necessarily production.** This document originally treated dump hits as finished flavor data needing no further work. `[SPEC-FD-150]` overturns that: whether the dump also *serves* as production values now depends on `[SPEC-FD-160]`. If uniform local scoring ranks similarity better, the entire library is extracted locally and the dump is kept purely as the yardstick. Harvesting is equally urgent either way — a yardstick that disappears cannot be recovered `[GDE-FEX-120]`.

Extract the library's ~7,900 recording MBIDs. Every hit is an exact 71-dimension reference vector.

#### `[GDE-FEX-055]` Coverage measured 2026-08-09 — `[GDE-OPN-010]` answered

Full harvest across all 31 files, **29,560,615 documents scanned**:

| | |
| :--- | ---: |
| Library recordings found | **8,001 of 8,542 — 93.7%** |
| …with more than one submission | 7,685 |
| Mean submissions per found recording | **77** (max 1,270) |
| Dimension values stored | 43,760,424 |

**`[GDE-FEX-056]` The gaps skew new, which is the population that matters for distribution.** Among `vaino.db`'s 7,912 MBIDs, 522 are absent. Their era profile differs sharply from the found set:

| Era | of found | of missing |
| :--- | ---: | ---: |
| pre-1980 | 7% | 3% |
| 1980–94 | 39% | 15% |
| 1995–2004 | 26% | 30% |
| 2005–12 | 12% | 19% |
| **2013+** | **8%** | **18%** |

Post-2013 material is **2.3× over-represented** among the misses, confirming the pattern McRhythm saw. Our library skews old; **a recipient's library will skew newer, so their coverage will be worse than 93.7%** — direct support for `[GDE-FEX-027]`.

**`[GDE-FEX-057]` Unexpected asset: 77 submissions per recording on average.** This was not anticipated and it cuts two ways.

*Favourably* — the constants can be recomputed on the library's own population rather than a generic sample. **Done 2026-08-09** `[LOG-I6-010]`, and the expected benefit did not materialise: averaging submissions tightens the floor by only **1.14×**, so the "77 submissions cancel the noise" reasoning was much weaker than assumed here.

*Against the earlier argument* — `[SPEC-FD-140]` reasoned that all-local might beat all-dump because the dump carries encoding variance. Averaging was expected to remove most of that. It does not (1.14×), so the dump remains substantially noisier than assumed on *both* readings — and measurably noisier on our library than on a generic sample: floor 0.359 versus 0.210 `[LOG-I6-010]`. The consistency and distribution arguments stand unaffected regardless.

**Note the superseded claim.** An earlier draft said "only the misses need Tier 1" and treated coverage `[GDE-OPN-010]` as sizing the whole document. Three later findings each independently overturn that: future acquisitions are permanently outside the dump `[GDE-FEX-025]`, distribution forbids depending on it `[GDE-FEX-027]`, and uniform local scoring may rank similarity better than mixed provenance does `[SPEC-FD-140]`. Coverage is still worth measuring — it bounds how much reference data validation has to work with — but it no longer sizes the work.

### `[GDE-FEX-060]` Tier 1 — Reproduce AcousticBrainz's pipeline, don't approximate it

**This is the central strategic correction over Vaino v1.** v1 tried to *approximate* AcousticBrainz using MusicNN embeddings and classification heads `[GDE-V1-020]` — different models, trained differently, producing outputs that are at best correlated with the target. AcousticBrainz's own pipeline is public:

```
audio ──[Essentia streaming_extractor_music]──> lowlevel features ──[SVM classifier models]──> highlevel vector
```

Running that same pipeline reproduces AcousticBrainz **by construction** rather than by resemblance. Correctness becomes a matter of matching a known reference, not of training something new.

#### `[GDE-FEX-062]` Verified 2026-08-08: the feature extractor needs no build at all

AcousticBrainz published **static binaries of the exact extractor it ran**, and they are still served:

```
https://data.metabrainz.org/pub/musicbrainz/acousticbrainz/extractors/
  essentia-extractor-v2.1_beta2-1-ge3940c0-win-i686.zip     (5 MB, sha1 verified)
  essentia-extractor-v2.1_beta2-linux-x86_64.tar.gz
  essentia-extractor-v2.1_beta2-linux-i686.tar.gz
  essentia-extractor-v2.1_beta2-2-gbb40004-osx.tar.gz
```

The Windows build was downloaded, checksum-verified, and **run successfully on library audio** — no Docker, no WSL, no compiler. It self-reports `extractor "music 1.0"`, `essentia 2.1-beta2`, git `v2.1_beta2-1-ge3940c0`: the exact vintage behind the dumps.

Throughput measured at **~27 s per track** single-threaded (12 tracks, 322 s). For the full 8,216-track library that is ~62 core-hours, trivially parallel; for the ~729 tracks that actually need it, under 6 core-hours.

The SVM classifier models are likewise published and downloaded: `essentia-extractor-svm_models-v2.1_beta5.tar.gz` (39 MB) — 18 classifiers, each with a Gaia `.history` transformation chain, a `.history.param` (RBF C-SVC, `C=11`, `gamma=-11`, `balanceClasses`), and a `.history.results.html` accuracy report.

**Consequence: `[GDE-FEX-060]` is already achieved for feature extraction.** We are not reproducing Essentia — we are running the reference implementation itself. Only the highlevel classification step remains open `[GDE-FEX-065]`.

#### `[GDE-FEX-065]` The one remaining gap: lowlevel → highlevel

No SVM-capable binary was ever published — the released extractors emit lowlevel features only, exactly as AcousticBrainz ran them (classification was a separate server-side step). So the whole remaining problem is one deterministic function: **436+ lowlevel scalars → 71 highlevel dimensions.** Three routes, in increasing attractiveness:

1. **Build Essentia + Gaia with SVM support.** The original assumption. Now clearly the *worst* option — it is the only one requiring the awkward Gaia build, and it buys nothing the others don't.
2. **Reimplement the Gaia inference chain.** The `.history` files are Qt `QDataStream` serializations of a `remove → select → normalize → PCA → SVM` pipeline. Tractable, exactly verifiable, but involves reverse-engineering a binary format.
3. **Distil the classifiers from the dumps.** ⭐ We do not need to *be* Gaia; we need to *predict what Gaia predicted*. The paired sample dumps already on disk supply ~88,816 recordings with both lowlevel features and AcousticBrainz's own highlevel outputs — a fully labelled dataset with a deterministic teacher, the easiest possible supervised setup. No binary format, no Gaia, no build.

Route 3 is recommended, with route 2 as the fallback if distillation plateaus. Both are verified identically `[GDE-FEX-090]`, and the data to attempt route 3 is already local.

#### `[GDE-FEX-067]` Route 2 surveyed 2026-08-10 — the chains are legible, and route 2 is the only route that reaches all 18

Route 3 was pursued and produced models for **11 binary characteristics only**. The six complex ones — `genre_dortmund`, `genre_electronic`, `genre_rosamerica`, `genre_tzanetakis`, `ismir04_rhythm`, `moods_mirex` — have no distilled model, and the one attempt on record sits at 1.87× the reproducibility floor `[LOG-NEXT-010]`. Those six are exactly what `[SPEC-FD-082]` predicts the library most needs.

All 18 Gaia `.history` files are on disk. `tools/gaia_history.py` reads them: a `QDataStream` with magic `0x6AEA723D` and length-prefixed UTF-16BE strings. Every chain is the same ten steps, varying only at step five:

```
remove → fixlength → remove → enumerate → {select|normalize|remove}
       → select → cleaner → normalize → svmtrain → select
```

**Two corrections to `[GDE-FEX-065]` above.** There is **no PCA stage** in any of the 18 — the pipeline was described as `remove → select → normalize → PCA → SVM` and the PCA does not exist. And the descriptor names are stored in full (`.lowlevel.silence_rate_20dB.max`, `.lowlevel.spectral_decrease.dmean`, …), matching the extractor's JSON keys exactly, so the `remove`/`select` steps need no guessing at all.

#### `[GDE-FEX-068]` The SVM models need no reverse engineering — they are libsvm text

The part that looked hardest is not serialised at all. Gaia stores each model under a `modelData` parameter as **the contents of a libsvm model file**, in the documented text format:

```
svm_type c_svc      kernel_type rbf     gamma 0.000488281
nr_class 2          total_sv 76         rho 2.52051
label 1 0           probA -3.10782      probB 0.304241
nr_sv 36 40
SV
245.3712498333288 1:0.45952198 2:0.51523113 …
```

All 18 extract cleanly, and **their class counts independently confirm they are the right models** — matching `[SPEC-FD-010]`'s table exactly:

| Classifier | classes | SVs | | Classifier | classes | SVs |
| :--- | ---: | ---: | :-- | :--- | ---: | ---: |
| `genre_dortmund` | 9 | 1,457 | | `moods_mirex` | 5 | 240 |
| `genre_tzanetakis` | 10 | 762 | | `genre_electronic` | 5 | 188 |
| `ismir04_rhythm` | 10 | 608 | | 12 binaries | 2 | 68–1,569 |
| `genre_rosamerica` | 8 | 329 | | | | |

Every model carries `probA`/`probB`, so the 0–1 values AcousticBrainz publishes are libsvm's Platt-scaled probability estimates — reproducible from the stored coefficients rather than needing to be inferred.

**One trap:** `danceability` uses a **polynomial** kernel while the other seventeen use RBF, and its `.param` file says RBF. Trust the model, not the parameter file.

Support-vector counts are small (68–1,569), so prediction costs nothing next to the ~27 s extraction that precedes it.

#### `[GDE-FEX-069]` The parameter tree is ordinary Qt `QVariant`

No bespoke format anywhere: `quint32` type, `quint8` isNull, payload, with the standard QMetaType ids — map 8, list 9, string 10, double 6. `read_variant` handles the six types Gaia actually uses and **raises on anything else rather than guessing**, because a wrong guess here yields plausible numbers, which is the worst possible failure mode for a transform chain.

The `normalize` step decodes fully. `coeffs` is a map of descriptor → `{a, b}`, each a list of doubles — length 1 for scalars, 36 for `.tonal.hpcp.*`, 24 for `.tonal.chords_histogram`. Normalisation is **`y = a·x + b`**: `tuning_frequency` carries `a=0.0402, b=-17.35`, mapping 440 Hz to 0.35 — min-max scaling written as scale-and-offset rather than as a range.

All 18 parse, at 372–875 dimensions each.

**A trap caught before it shipped:** six of the eighteen — `genre_dortmund`, `genre_electronic`, `mood_relaxed`, `moods_mirex`, `timbre`, `voice_instrumental` — normalize **twice**, once at step five and again before the SVM. An API returning "the" coefficients hands back the wrong step for a third of the classifiers, and the output still looks like reasonable numbers. `normalize_coeffs` therefore returns a **list** in chain order.

#### `[GDE-FEX-070a]` The record framing is *not* fully understood — and that is deliberate

An attempt at a sequential reader failed on all 18. The header is `magic, version, count, reserved`, and each record opens `QString name, QString applier`, then **two** `QVariantMap`s — not one. After those comes an empty map and then a region that could be a `QByteArray` of applier state or a mis-framed string.

Choosing between those by inspection is guesswork, and a sequential reader built on a guess would associate parameters with the **wrong steps** while still producing numbers that look entirely reasonable. That is the failure this whole exercise is most exposed to, so the reader was **removed rather than shipped**. Extraction locates parameters by name and occurrence instead, which works for all 18, and the 658-pair verification is what confirms the association is right.

Resolve the framing later if it becomes necessary. It is not necessary yet.

#### `[GDE-FEX-090a]` The verification set is complete

All **658** lowlevel JSONs have published highlevel beside them in `data/flavor-sample.db`, across all 18 characteristics. Note that recordings carry **multiple submissions** (0, 1, 2 …) with different values, and the lowlevel file is one specific submission — so a comparison must establish *which*, rather than assume submission 0. Comparing against every submission and reporting the best match will show it.

#### `[GDE-FEX-091]` First end-to-end run: close, and **not** correct

`tools/gaia_predict.py` applies the chain and compares against the reference in one tool, deliberately — a subtly wrong chain still emits plausible probabilities, so "it ran" is evidence of nothing. libsvm prediction is implemented: RBF and polynomial kernels, and the Platt sigmoid matching libsvm's own `sigmoid_predict`.

First run on `tonal_atonal` (2 classes, 76 SVs, one normalize step), 8 recordings:

| descriptor order | median abs error | best | worst |
| :--- | ---: | ---: | ---: |
| as-read | **0.0593** | 0.0087 | 0.3171 |
| sorted | 0.1055 | 0.0039 | 0.2614 |
| reversed | 0.1055 | 0.0039 | 0.2614 |

**This is not a match.** A correct reimplementation should agree to within floating-point noise, not 0.06. But it is also clearly not noise — random guessing against values in [0,1] would sit near 0.35 — so the chain is substantially right and specifically wrong.

**The concrete lead: the vector is 372 dimensions and the SVM's highest support-vector index is 380.** Eight dimensions are missing, so every distance is computed in the wrong space. The `enumerate` step converts string descriptors to numbers, and the lowlevel files carry exactly four musical strings — `tonal.key_key`, `tonal.key_scale`, `tonal.chords_key`, `tonal.chords_scale` — which accounts for four, not eight. Note also that the errors are generous by construction: the harness takes the best over both label orientations and all submissions, so the true error is at least this large.

#### `[GDE-FEX-092]` The eight dimensions are found — and they reveal a version mismatch that blocks route 2

The SVM's input list is **133 descriptors**: the 125 with normalize coefficients (372 dims) plus **8 enumerated string descriptors** (1 dim each) = **380**, matching the highest support-vector index exactly. The eight are:

```
.tonal.chords_key            .tonal.key_edma.key         .tonal.key_krumhansl.key
.tonal.chords_scale          .tonal.key_edma.scale       .tonal.key_krumhansl.scale
                             .tonal.key_temperley.key    .tonal.key_temperley.scale
```

`key_edma`, `key_krumhansl` and `key_temperley` are the **three-profile key estimation** of later Essentia. Neither input we have produces them:

| Artefact | Essentia version | Key descriptors emitted |
| :--- | :--- | :--- |
| AcousticBrainz's published highlevel | 2.1-**beta1** | `key_key`, `key_scale` only |
| The downloadable extractor `[GDE-FEX-062]` | 2.1-**beta2** | `key_key`, `key_scale` only |
| The downloadable SVM models | v2.1_**beta5** | **requires all eight** |

Measured: 0 of 40 archived lowlevel files carry `key_edma`, and neither do our own 12 extractions. **Six of the 380 input dimensions cannot be supplied by any data we can produce.**

Two consequences, and the second is worse than the first:

1. **Exact reproduction of AcousticBrainz's published values is impossible with these models.** They are a later vintage than the pipeline that produced the dumps, so `[GDE-FEX-090]`'s verification can never reach zero error. The 0.059 residual `[GDE-FEX-091]` is at least partly this.
2. **The models cannot be run correctly on our own library either**, because our extractor is beta2 and also lacks the three-profile keys. This is not a verification problem; it is an input problem.

**Route 2 is blocked pending a version question**, and the cheap test is whether AcousticBrainz published an earlier `svm_models` release matching the beta1/beta2 extractor — the URL pattern in `[GDE-FEX-062]` is known and the beta5 tarball was found by browsing that directory.

#### `[GDE-FEX-093]` Unblocked: the beta1 models exist, in a different directory

`data.metabrainz.org/pub/musicbrainz/acousticbrainz/**svm_models/**` — a sibling of `extractors/`, not inside it — holds a second, **earlier** set of the same 18 classifiers, stamped `accuracies_v2.1_beta1.html`. Different file sizes throughout (`tonal_atonal` 2 MB against the beta5 copy's 0.7 MB), so these are genuinely different models.

Measured on `tonal_atonal`, against one archived lowlevel file:

| | descriptors needed | present in the lowlevel | vector dims | highest SV index |
| :--- | ---: | ---: | ---: | ---: |
| beta5 (on disk) | 125 | **81 — 44 missing** | 372 | 380 |
| **beta1** (fetched) | 157 | **157 — 0 missing** | 666 | 670 |

The beta1 model's enumerated strings are exactly `.tonal.chords_key`, `.tonal.chords_scale`, `.tonal.key_key`, `.tonal.key_scale` — the four that both the archived lowlevel **and our own beta2 extractor** emit. The vintage matches. Route 2 is viable.

#### `[GDE-FEX-094]` The two normalize steps **compose**; they are not alternatives

`[GDE-FEX-069]` recorded that six chains normalize twice and treated the question as *which* to use. That was wrong: they apply **in sequence**.

`.lowlevel.spectral_spread.var` reads `2.197e12` raw. Step 0 (`a=3.10e-15, b=-4.29e-4`) takes it to `0.0064`; step 1 (`a=0.2565, b=+0.5`) takes that to `0.5016` — inside the support vectors' observed range of [0, 11]. Applying only step 1 to the raw value gives `5.6e11`, which makes `‖x‖²` about `3e23`, so every RBF kernel value underflows to zero and the classifier returns `-rho` for **every input**.

That failure is worth naming precisely because of how it presented: a constant output identical across all three candidate descriptor orderings. A constant classifier still produces a probability, and a plausible one — 0.8366 here.

**Current state — still not correct.** With composition applied, `tonal_atonal` over 12 recordings gives a median absolute error of **0.0956**, best **0.0003**, worst **0.267**. Predictions now vary, and some recordings match nearly exactly, which the constant version could never have done. But a correct reimplementation matches to floating-point noise throughout, and this does not.

#### `[GDE-FEX-095]` The layout is solved; the values are still wrong

The four enumerated dimensions were found without guessing. Scanning the support vectors for indices whose values are **integers greater than 1** — impossible for a normalised feature — returns exactly two: **547** and **664**, each carrying 0–11, which is twelve pitch classes. Sorting all 161 descriptors alphabetically and accumulating widths places `.tonal.chords_key` at **547** and `.tonal.key_key` at **664**, totalling **670**. Three independent facts agree, so the ordering is alphabetical over the union of numeric and string descriptors, and the enumerated dimensions are **not normalised** — the support vectors hold raw codes at those positions.

Vector dimensions now match the model exactly: 670 against 670.

**And it is still wrong.** Over 25 recordings with a fixed label orientation:

```
median 0.3660   exact (<0.001) 0/25   range 0.004 … 1.000
```

Errors reaching **1.000** are confident inversions, not numerical drift. A brief earlier reading of "best 0.0000" was an artefact of the harness taking the best over *both* label orientations as well as all submissions — generosity that flattered the result and hid this distribution. **The harness needs tightening before it is trusted further**: the label-to-class-name mapping should be resolved from the chain rather than tried both ways.

Two candidate causes, untested:

1. **The `cleaner` step is not implemented.** It sits between `select` and `normalize` in every chain and presumably drops or repairs degenerate descriptors. If it removes any, every subsequent index shifts — which fits a distribution of a few near-hits among many confident misses.
2. **The label-to-class mapping.** `label 1 0` says which model class is first, not whether "1" means `tonal` or `atonal`.

Neither is speculative work: both are readable from the chain.

#### `[GDE-FEX-096]` Chain order resolved; the enumeration maps are stored, and unread

Ordering the steps by file offset settles the sequence:

```
remove(16) → fixlength(29730) → remove(112618) → enumerate(153388)
  → normalize(162678) → select(951377) → cleaner(972078)
  → normalize(982013) → svmtrain(1071587) → select(1889620)
```

`cleaner` uses the `removedesc` applier, so it does drop descriptors — but **both** normalize steps carry 157, so nothing is dropped in this chain and cause (1) above is eliminated. Composing the two normalizes across the intervening `select`/`cleaner` is correct.

Immediately before `svmtrain` the chain stores, explicitly, **the enumeration maps and the class field**:

```
.tonal.key_scale     minor, major
.tonal.key_key       G#, G, F#, F, E, D#, D, C#, C, B, A#, A
.tonal.chords_scale  minor, major
.tonal.chords_key    G#, G, F#, F, E, D#, D, C#, C, B, A#, A
className            highlevel.tonal_atonal
```

**This contradicts the codes currently in the tool.** `[GDE-FEX-095]` assumed alphabetical (`A=0 … G#=11`); the stored order is descending. Reading the maps properly needs QVariant types **12** and **32**, which `read_variant` refuses rather than guesses — the same discipline that kept the container framing honest `[GDE-FEX-070a]`.

`gaia_predict.KEY_CODES` is therefore marked unverified in the source, and **no output of that module should be used until the stored maps are read**. Identifying those two type ids against the Qt version Gaia was built with is the next step, and it also yields the label-to-class mapping, since `className` sits in the same region.

#### `[GDE-FEX-097]` The enumeration maps are read — and the codes are arbitrary

The maps are **not** `QVariant`-framed, which is why the reader balked. They are a bare `quint32` count followed by that many `(QString, quint32)` pairs:

```
00 00 00 02   count = 2
00 00 00 0a "minor"  00 00 00 00      minor = 0
00 00 00 0a "major"  00 00 00 01      major = 1
```

Read properly, the codes are **arbitrary and differ per descriptor**:

| descriptor | codes |
| :--- | :--- |
| `.tonal.key_key` | G#=0, F=1, G=2, C=3, A#=4, A=5, E=6, C#=7, B=8, D=9, F#=10, D#=11 |
| `.tonal.chords_key` | G#=0, F=1, G=2, C=3, E=4, D=5, A=6, F#=7, B=8, C#=9, D#=10, A#=11 |
| both `_scale` | minor=0, major=1 |

The same twelve notes map differently in the two descriptors, so **no ordering rule would have produced them** — not alphabetical, not chromatic, not the order they appear in the file. Both of my guesses were wrong: `[GDE-FEX-095]`'s alphabetical keys, and a `major=0, minor=1` scale mapping that was exactly inverted. `gaia_history.enum_maps` now reads them from the chain.

**And the chain still does not reproduce.** With correct codes, over 25 recordings at a fixed orientation:

```
median 0.4289   exact (<0.001) 0/25   range 0.020 … 1.000
```

The generous metric improved (worst 0.267 → 0.176) while the strict median moved the wrong way. That combination says the residual error is **not** the enumerated dimensions — they are now right, and something larger is still wrong. A global label flip does not explain it either: the distribution is broad rather than bimodal, so predictions are decorrelated from truth for a substantial fraction rather than systematically inverted.

**Remaining candidates**, in the order worth testing:

1. The `select` step after `svmtrain` and the `addfield` step before it are unmodelled; `addfield` declares `className = highlevel.tonal_atonal`, so the label-to-class mapping lives there.
2. `.tonal.hpcp` and similar vector descriptors are 36-wide — whether their components are ordered as stored, and whether `fixlength` reorders them, is untested.
3. The first `remove` and `fixlength` steps are assumed inert because the normalize coefficients define the surviving set; that assumption has not been checked.

#### `[GDE-FEX-098]` The divergence, found in the literature: a **gaussianize** step my tooling was hiding

MTG's Gaia provides a `gaussianize` transformation, and its ChangeLog discusses serialising gaussianize histories — a transformation I had never accounted for. It is present in the chain, at offset 242175:

```
remove → fixlength → remove → enumerate → normalize → GAUSSIANIZE
  → select → cleaner → normalize → svmtrain → select
```

Applier `distribute`; `descriptorNames = ['lowlevel.*']`; parameter `distribution`, a **per-component** table keyed `.lowlevel.zerocrossingrate.var[0]`. The ~709 KB between it and the following `select` is that table — the bulk of the file.

Gaussianize maps each component through the training set's empirical distribution, so it is **non-linear and per-component**. It covers `lowlevel.*`, which is most of the vector. Omitting it leaves the majority of dimensions transformed by the wrong function entirely — which is precisely the observed signature: broad, decorrelated error, a few recordings near-right by luck, and no amount of fixing indices or codes moving the median.

**This also corrects `[GDE-FEX-094]`.** The "two normalize steps that compose" are the signature of a **gaussianize sandwich**: `normalize → gaussianize → normalize`. Composing the two normalises directly, as that entry concluded, silently deletes the non-linear middle. The classifiers with two normalizes are a superset of those with gaussianize, which is why the pattern looked like a normalisation quirk.

**How it stayed hidden for six commits is the more useful lesson.** `chain_summary` filtered step names against a `KNOWN` set, so any transformation not already in my vocabulary was omitted from every chain I printed — and the omitted one was the transformation that mattered. Refusing to guess at *values* `[GDE-FEX-070a]`, `[GDE-FEX-096]` was right and repeatedly paid off; assuming my *vocabulary* was complete was the same error wearing different clothes, and it was never checked because the output looked plausible. `chain_of` now reports every step and flags unrecognised ones rather than dropping them.

**Only 4 of 18 beta5 chains gaussianize** — `genre_dortmund`, `genre_electronic`, `moods_mirex`, `voice_instrumental` — and `tonal_atonal` is not among them there. But the **beta1** `tonal_atonal` is. Picking the smallest, simplest-looking classifier as the first test target happened to pick one carrying the extra non-linear step.

Next: fetch a beta1 classifier **without** gaussianize and verify the rest of the chain against it. That isolates the variable — if the linear path is otherwise correct it should reproduce to floating-point noise, which would confirm both the layout and the remaining machinery before gaussianize is implemented at all.

> **Sources:** [MTG/gaia ChangeLog](https://github.com/MTG/gaia/blob/master/ChangeLog) · [essentia gaiatransform.cpp](https://github.com/MTG/essentia/blob/master/src/algorithms/highlevel/gaiatransform.cpp) · [gaia normalize.cpp](https://github.com/MTG/gaia/blob/master/src/algorithms/normalize.cpp) · [Essentia music extractor docs](https://essentia.upf.edu/streaming_extractor_music.html)

### `[GDE-FEX-099]` ✅ ROUTE 2 VERIFIED — the chain reproduces AcousticBrainz exactly

Isolating the variable worked. Three beta1 classifiers **without** a gaussianize step, each against 120 archived recordings with published highlevel:

| classifier | exact (<0.001) | median error | p95 | max |
| :--- | ---: | ---: | ---: | ---: |
| `mood_acoustic` | 114/120 — 95% | 0.000065 | 0.0018 | 0.0033 |
| `mood_happy` | 104/120 — 87% | 0.000033 | 0.0024 | 0.0034 |
| `mood_party` | 95/120 — 79% | 0.000002 | 0.0024 | 0.0037 |

**Maximum error 0.0037 across 360 comparisons — no outliers, no inversions.** The residual is consistent with rounding in the published dump values rather than any error in the chain. This is reproduction, not approximation.

The corrected `chain_of` also confirms the structural rule with no exceptions among the four beta1 classifiers held: **one normalize ⇔ no gaussianize; two normalizes ⇔ a gaussianize sandwich.**

```
mood_acoustic  remove → fixlength → remove → enumerate → select → select
                 → cleaner → normalize → addfield → svmtrain → select
tonal_atonal   remove → fixlength → remove → enumerate → normalize → gaussianize
                 → select → cleaner → normalize → addfield → svmtrain → select
```

**What this validates, all at once:** the beta1 model vintage `[GDE-FEX-093]`, the alphabetical descriptor layout with enumerated strings interleaved `[GDE-FEX-095]`, the enum codes read from the chain `[GDE-FEX-097]`, `y = a·x + b` normalisation `[GDE-FEX-069]`, the libsvm RBF and Platt-sigmoid implementation `[GDE-FEX-068]`, and the harness itself. Every one of those was previously *asserted*; all are now *confirmed* by a single end-to-end result that could not have come out this way if any were wrong.

It also confirms the `[GDE-FEX-098]` diagnosis: the only thing wrong with `tonal_atonal` was the missing gaussianize.

**Remaining for full coverage of all 18:**

1. **Gaussianize**, for the chains that use it. In the beta5 set that is `genre_dortmund`, `genre_electronic`, `moods_mirex`, `voice_instrumental` — three of them among the six complex characteristics Vaino most needs `[SPEC-FD-082]`. Note the beta1 chains differ (beta1 `tonal_atonal` gaussianizes, beta5's does not), so the beta1 set must be surveyed rather than assumed.
2. **Pairwise coupling** for the six multi-class classifiers; only binary Platt is implemented.
3. Fetch the remaining beta1 classifiers (~100 MB).

None of these is exploratory any more. The chain is understood and proven; what is left is implementing two documented algorithms and downloading files.

#### `[GDE-FEX-100]` 11 of 18 classifiers reproduce, including three complex ones

All 18 beta1 models fetched (83 MB). The full survey:

| | needs gaussianize | multi-class |
| :--- | :--- | :--- |
| **7** | `genre_dortmund`, `genre_electronic`, `genre_rosamerica`, `mood_sad`, `timbre`, `tonal_atonal`, `voice_instrumental` | |
| **6** | | `genre_dortmund`, `genre_electronic`, `genre_rosamerica`, `genre_tzanetakis`, `ismir04_rhythm`, `moods_mirex` |

**Eight binary classifiers, 60 recordings each** — both kernels, since `danceability`, `mood_aggressive` and `mood_happy` are polynomial:

```
danceability 55/60   gender 44/60   mood_acoustic 59/60   mood_aggressive 56/60
mood_electronic 55/60   mood_happy 51/60   mood_party 45/60   mood_relaxed 56/60
median error 1.3e-5 … 7.4e-5      max across all eight: 0.0037
```

**Three multi-class classifiers**, via Wu–Lin–Weng pairwise coupling as libsvm implements it:

```
genre_tzanetakis  60/60 exact    ismir04_rhythm  60/60 exact    moods_mirex  60/60 exact
median 0.000000                  max 0.0000
```

The multi-class results are *exact* where the binary ones carry a ~0.003 residual, which supports the residual being rounding in the published values rather than anything in the chain.

**Three of the six complex characteristics — `genre_tzanetakis`, `ismir04_rhythm`, `moods_mirex` — are now reproducible**, and they are precisely what `[SPEC-FD-082]` predicts *Light* and *Groove* need.

One transcription note kept because it is the kind of thing that silently corrupts: `sv_coef` is stored here as `[support_vector][coefficient]`, transposed from libsvm's `[coefficient][support_vector]`. In `decision_values` the pair (i, j) uses `sv_coef[·][j-1]` over class i's block and `sv_coef[·][i]` over class j's — an asymmetry worth transcribing rather than reconstructing from intuition. It raised an `IndexError` rather than quietly producing wrong numbers, which was luck as much as design.

**Remaining: gaussianize, for the seven chains that use it** — including the other three complex characteristics.

#### `[GDE-FEX-101]` Gaussianize transcribed from Gaia's source — 12 of 18

Two guesses at the semantics were made and both were wrong: quantile-to-uniform made `tonal_atonal` worse (0.43 → 0.64), and quantile-through-inverse-normal-CDF worse again (0.82). Reading MTG's `distribute` applier settled it:

```
rank    = lower_bound(distribution, v)
rank    = clamp(rank, outliers, nPoints - outliers)
normIdx = rank / nPoints
out     = erfinv(2*normIdx - 1)
```

`erfinv(2q−1)` is the inverse normal CDF **scaled by 1/√2** — the factor both guesses missed, and one that matters greatly to an RBF kernel. Python has no `erfinv`; it is written through `NormalDist.inv_cdf`, which is the same function.

The stored tables are per component, keyed `.descriptor[i]`: **little-endian float32 inside a big-endian stream** — a raw memory dump — sorted ascending, with no count prefix. 484 components for most chains.

**`mood_sad` now reproduces**: 30/40 exact, median 0.000128, max 0.0030 — the same signature as the other verified classifiers. So the algorithm is right.

**Six do not**: `tonal_atonal`, `voice_instrumental`, `timbre`, `genre_dortmund`, `genre_electronic`, `genre_rosamerica`, all sitting near median 0.8.

The puzzle is sharp, which is the useful part. `mood_sad` and `tonal_atonal` are **structurally identical** on every axis checked: same chain (`normalize → gaussianize → select → cleaner → normalize`), same gaussianize scope (`lowlevel.*`), same 484 tables, same 157 descriptors in both normalize steps, and the two normalize steps cover the *same* descriptor set in each. Whatever differs is not the chain shape.

Next candidate: the `select` and `cleaner` steps between gaussianize and the second normalize are still unmodelled, and are assumed inert because the two normalizes cover identical sets. That assumption is now the least-tested thing standing — `mood_sad` may simply be the case where those steps happen to be no-ops.

> **Source:** [gaia `distribute` applier](https://github.com/MTG/gaia/blob/master/src/algorithms/distribute.cpp)

### `[GDE-FEX-102]` ✅ ALL 18 CLASSIFIERS REPRODUCE — route 2 is complete

The six that appeared broken were a **harness** fault, not a chain fault. Class names map to model labels **by value, not by position**: the class sorted at index `i` corresponds to model label `i`. I compared against `label[i]`. Where a model's labels read `[0, 1]` the two coincide and everything verified; where they read `[1, 0]` every prediction was scored against the wrong class:

```
tonal_atonal        0.812 → 0.000633      voice_instrumental  0.810 → 0.000739
timbre              0.776 → 0.000273
```

Full verification, 60 archived recordings each, against AcousticBrainz's published highlevel:

| classifier | exact | median | max | | classifier | exact | median | max |
| :--- | ---: | ---: | ---: | :-- | :--- | ---: | ---: | ---: |
| `genre_tzanetakis` | 60/60 | 0.000000 | 0.0000 | | `mood_electronic` | 55/60 | 0.000074 | 0.0029 |
| `ismir04_rhythm` | 60/60 | 0.000000 | 0.0000 | | `mood_aggressive` | 56/60 | 0.000013 | 0.0025 |
| `moods_mirex` | 60/60 | 0.000000 | 0.0000 | | `mood_relaxed` | 56/60 | 0.000046 | 0.0029 |
| `genre_electronic` | 60/60 | 0.000043 | 0.0003 | | `mood_happy` | 51/60 | 0.000042 | 0.0034 |
| `mood_acoustic` | 59/60 | 0.000064 | 0.0025 | | `genre_rosamerica` | 51/60 | 0.000561 | 0.0024 |
| `genre_dortmund` | 57/60 | 0.000009 | 0.0019 | | `mood_sad` | 48/60 | 0.000149 | 0.0034 |
| `mood_aggressive` | 56/60 | 0.000013 | 0.0025 | | `timbre` | 48/60 | 0.000283 | 0.0024 |
| `danceability` | 55/60 | 0.000044 | 0.0031 | | `mood_party` | 45/60 | 0.000014 | 0.0037 |
| `gender` | 44/60 | 0.000039 | 0.0027 | | `tonal_atonal` | 35/60 | 0.000672 | 0.0072 |
| | | | | | `voice_instrumental` | 36/60 | 0.000659 | 0.0070 |

**Maximum error across all eighteen: 0.0072.** Three reproduce exactly. `[GDE-FEX-065]`'s remaining gap — the one deterministic function from 436+ lowlevel scalars to 71 highlevel dimensions — is closed.

**The bookend worth keeping.** `[GDE-FEX-095]` warned that a verification tool which *flatters* is worse than none, because it gets believed. This was the mirror: a harness that was too **harsh** made a correct implementation look broken for six commits, and sent me looking for faults in chain structure that was right all along. Both failures are the same underlying error — trusting the comparison more than the thing compared. The check needs checking too.

#### `[GDE-FEX-103]` Class names are stored too, and the production path runs

`classMapping` is a plain `QStringList` beside `className` — index i is model
label value i, the mapping whose positional guess caused `[GDE-FEX-102]`. All 18
read cleanly, and all are already in sorted order, which retroactively confirms
the `sorted()` assumption the verification relied on.

`tools/gaia_classify.py` is the production path: load the 18 chains once (~5 s),
then classify any lowlevel JSON. Run against **our own beta2 extraction** rather
than the archive:

```
danceability not_danceable 0.857   mood_aggressive aggressive 0.987
genre_dortmund electronic  0.446   mood_happy      happy      0.998
tonal_atonal   atonal      0.925   voice_instrumental voice   0.980
```

Coherent, and named. **586 ms per track for all 18 classifiers** — so classifying
the whole 5,590-file library costs under an hour, negligible beside the ~27 s per
track the lowlevel extraction itself takes `[GDE-FEX-062]`. Extraction remains the
only expensive step, and it is the one that caches `[SPEC-SC-080]`.

**What Vaino can now do:** run all 18 AcousticBrainz classifiers locally, over any audio, from the published extractor and models, with values verified against AcousticBrainz's own output. That is uniform local provenance `[SPEC-FD-145]` with no accuracy penalty and no approximation — the outcome `[SPEC-FD-150]` argued for and could not previously reach.

Note what is *not* required: matching AcousticBrainz. `[SPEC-FD-145]` wants **uniform provenance**, not fidelity to an external reference. If a beta5-compatible extractor could be obtained instead, running it over the whole library would be equally acceptable — the constraint is that every track be scored the same way, not that the way match the dumps. That reframes the question from "reproduce AB" to "find any matched extractor/model pair we can run over everything".

**Route 2 is therefore promoted from fallback to the recommended path for the six complex characteristics.** Route 3's models remain the right answer for the 11 binaries they already cover.

### `[GDE-FEX-070]` Tier 2 — Approximate, only for what Tier 1 cannot reach

If Tier 1 proves impossible for some classifiers, fall back to modern embeddings (MusicNN, EffNet-Discogs) with heads fitted **against the harvested ground truth** `[GDE-FEX-050]` — which is exactly what v1 lacked. Same iteration protocol `[GDE-FEX-100]`, same provenance and accuracy labelling `[GDE-ARC-030]`.

Anything reaching production this way is tagged distinctly from Tier 0 and Tier 1 values, so its lower confidence is visible everywhere it is used `[GDE-FBD-030]`.

---

## 4. Validation

### `[GDE-FEX-080]` The ground-truth inventory

| Source | Recordings | Dims | Use |
| :--- | ---: | ---: | :--- |
| `mulib.db` `abXxx` columns | 8,062 | 11 | Available **today**, no download needed. Start here. |
| AcousticBrainz highlevel dump | TBD `[GDE-OPN-010]` | 71 | Primary ground truth once mirrored |
| AcousticBrainz lowlevel dump | same | — | Stage B input `[GDE-FEX-090]` |

Discipline: split into train/tune and **held-out** sets before the first experiment, and never report a tuned score as a held-out one.

### `[GDE-FEX-090]` Two-stage validation — the key methodological idea

Do not measure the pipeline end-to-end and stare at one correlation. The paired lowlevel + highlevel dumps allow isolating the two halves:

**Stage A — feature extraction. Now moot, and measured to be so.** Since we run the reference binary itself `[GDE-FEX-062]`, there is no reimplementation to validate. The defects that sank v1 `[GDE-V1-020]` cannot recur, because no preprocessing is being re-derived.

Attempting Stage A anyway proved the point and exposed a confound worth recording: across 12 library tracks with dump coverage, 363 of 436 shared scalar descriptors agreed within 5% (76 bit-identical), with `rhythm.bpm` matching to ~0.1%. The 73 divergent descriptors were dominated by `.min` statistics and higher-order moments — quantities a single differing frame can move sharply. **But the comparison is not controlled: 0 of 12 tracks shared source audio with AcousticBrainz** (`md5_encoded` differed on all, with track lengths differing by up to 1.8 s and bitrates from 32 to 256 kbps). Those numbers therefore measure *encode variance*, not extractor fidelity — the same confound that sets the reproducibility ceiling `[GDE-FEX-085]`. Since AcousticBrainz's original source files are not available, a controlled Stage A is impossible in principle, and unnecessary in practice.

**Stage B — classification. This is now the entire remaining problem** `[GDE-FEX-065]`. *Given AcousticBrainz's own lowlevel features, does my classifier reproduce AcousticBrainz's highlevel output?*

Requires **only the two dumps — no audio at all.** Runs fast, over far more recordings than the library contains, and is *exactly* verifiable: the teacher is deterministic, so a correct implementation should reproduce the dump near-perfectly (r ≈ 0.999). Unlike Stage A, no encode variance intrudes — both sides consume the identical feature vector.

This is the happy consequence of the confound above: all of the irreducible ~0.82 `[GDE-FEX-085]` lives in audio encoding, which we do not control and need not model; everything we *do* control is exactly checkable.

### `[GDE-FEX-085]` The reproducibility ceiling — measured, and it reframes the target

**AcousticBrainz does not agree with itself.** Roughly 9.5% of recordings in the dump carry more than one submission — the same MusicBrainz recording, a different rip or encoding, analyzed by the same pipeline. Comparing submission 0 against submission 1 across **8,461 such recordings** in the sample dump:

| Classifier | r | MAE | | Classifier | r | MAE |
| :--- | ---: | ---: | :-- | :--- | ---: | ---: |
| `mood_sad` | **0.936** | 0.038 | | `mood_relaxed` | 0.835 | 0.069 |
| `mood_acoustic` | 0.891 | 0.054 | | `tonal_atonal` | 0.801 | 0.094 |
| `gender` | 0.862 | 0.064 | | `mood_aggressive` | 0.795 | 0.060 |
| `mood_happy` | 0.832 | 0.071 | | `timbre` | 0.793 | 0.094 |
| | | | | `voice_instrumental` | 0.767 | 0.103 |
| | | | | `mood_party` | 0.760 | 0.079 |
| | | | | `danceability` | **0.749** | 0.107 |

**Mean r ≈ 0.82.** Independently corroborated: comparing `mulib.db`'s stored values against the dump for 924 shared recordings gives r = 0.72–0.87 — the same range, which also confirms the field mapping in `[GDE-FEX-040]` is correct and that the residual is submission variance, not a mapping error.

Three consequences, all of which change how this work should be judged:

1. **r ≈ 0.95 against per-recording AcousticBrainz labels is not attainable** — not through any extractor deficiency, but because the target label itself is only ~0.82 self-consistent. Pursuing it past the ceiling is chasing encoding noise.
2. **r ≈ 0.82 is approximately ceiling, not a shortfall.** An extractor at that level is statistically indistinguishable from being another legitimate AcousticBrainz submission. This is the strongest form of the point in `[GDE-PHS-005]`: the honest stopping condition is set by the data, not by ambition.
3. **The right test is distributional, not pointwise** `[GDE-FEX-095]`. Ask whether the spread of (local extractor − AcousticBrainz) differences matches the spread of (submission 0 − submission 1) differences. If the two distributions coincide, the extractor is as good as AcousticBrainz — which is the actual goal, and is a well-posed question, unlike "how close to 1.0 can r get".

Note the strong implication for `[GDE-FEX-090]`: **Stage B carries none of this variance.** Given identical lowlevel input the classifiers are deterministic, so a correct Stage B reimplementation should reproduce the dump's highlevel output near-exactly (r ≈ 0.999). All of the irreducible ~0.82 lives in Stage A. This makes the two-stage split more valuable, not less — it separates the part that must be perfect from the part that cannot be.

### `[GDE-FEX-095]` Metrics

- **Binary characteristics** — Pearson r per classifier, plus MAE. Directly comparable to the `mulib.db` baseline.
- **Complex characteristics** — per-dimension r is necessary but insufficient on a simplex. Report alongside it top-1 class agreement and a distributional measure (Jensen–Shannon divergence or cosine over the full simplex).
- **What actually matters downstream** is flavor *distance* — specified in [SPEC005](spec/SPEC005-flavor-distance.md), not raw per-dimension agreement. Report rank correlation between distances computed on reproduced vectors versus ground-truth vectors, over sampled recording pairs. A vector can be mediocre per-dimension and still rank neighbours correctly, and ranking neighbours is what the Program Director consumes `[GDE-PD-050]`.
- **Per-characteristic reliability feeds straight back into the metric** `[SPEC-FD-120]`: a locally extracted characteristic contributes to distance in proportion to its measured agreement, so a weak extractor degrades similarity gracefully rather than poisoning it.

### `[GDE-FEX-100]` Iteration protocol — best effort, no pass/fail gate

Per `[GDE-PHS-005]`, **there is no ship/no-ship threshold.** What is enforced is measurement and honest reporting `[GDE-LES-030]`; the absence of that is what let v1's failure hide for months. The discipline is the process, not a number.

Each iteration records:

1. **Approach** — what was tried and why it was expected to help.
2. **Scores** — per-characteristic, on held-out data, both stages.
3. **Analysis** — strengths and weaknesses of this approach specifically. Which classifiers did well, which lagged, and what the residuals suggest.
4. **Next hypothesis** — or, an explicit statement that ideas are exhausted.

Explicitly:
- **A good score is not a finish line.** Analyze what worked and what lagged, form a next hypothesis, and try again.
- **But calibrate ambition against the measured ceiling** `[GDE-FEX-085]`: AcousticBrainz self-agreement is r ≈ 0.82. Beyond roughly that level, per-dimension r is measuring encoding variance rather than extractor quality — switch to the distributional test `[GDE-FEX-095]` and to downstream distance-ranking, which are the metrics that still carry signal up there.
- **r ≈ 0.82 after genuinely exhausting the available ideas is a legitimate outcome — and per `[GDE-FEX-085]` it is very close to as good as the target data allows.** Record what was tried and why each approach plateaued.
- The iteration history is itself a deliverable — it is what makes the next person's (or the next year's) attempt cheaper.

### `[GDE-FEX-110]` Publish the scorecard

Current per-characteristic accuracy, per provenance tier, visible in the UI `[GDE-CHT-030]`. A user looking at a track should be able to see whether its flavor came from the dump exactly, was reproduced locally at r = 0.91, or was approximated at r = 0.78.

This is the direct structural answer to `[GDE-V1-010]`: with provenance and accuracy attached to every value, "the descriptors are all fine" can never again be true-looking and false.

---

## 5. Risks

| Risk | Assessment | Mitigation |
| :--- | :--- | :--- |
| **`[GDE-FEX-120]`** ~~Dumps disappear as the API did~~ — **RETIRED 2026-08-08** | Was the single most time-sensitive risk in the project | **Mirrored and verified: 31/31 files, 41 GB** `[GDE-FEX-050]`. The archive is local. Remaining exposure is ordinary data-loss risk on our own storage, not the loss of an irreplaceable external resource — so it is now a backup question, not a race. |
| **`[GDE-FEX-130]`** ~~Gaia / Essentia will not build~~ — **RESOLVED 2026-08-08** | Was rated moderate-high; now **near zero**. No build is required: AcousticBrainz's own static extractor binary runs natively here `[GDE-FEX-062]`, and the SVM models are published `[GDE-FEX-065]` | Route 3 (distillation) avoids Gaia entirely. Route 2 (reimplement the `.history` chain) is the fallback. Building Gaia is now the option of last resort, not the plan. |
| **`[GDE-FEX-140]`** Dump coverage is poor for newer music (the *unreachable* set `[GDE-FEX-010]`) | Moderate. Coverage was 91.1% on an older-skewing sample `[GDE-MCR-045]`; gaps clustered in post-2012 releases, soundtracks and niche genres — plausibly where new music lives | Measure first `[GDE-OPN-010]`. If coverage is poor, Tier 1 matters more, not less. |
| **`[GDE-FEX-135]`** ~~Import time~~ — **ACCEPTED, not a risk** | 27 s/track measured `[GDE-FEX-062]`. 10,000 tracks ≈ 9 h across 8 cores — an acceptable overnight job, and an unrepresentative worst case: most users start at ~1,000 tracks or fewer | **Incremental import is the normal mode**, not batch: users add tracks as they collect them `[GDE-CHT-045]`. Still cache lowlevel permanently — improving a classifier later must never re-decode anyone's audio. |
| **`[GDE-FEX-137]`** ~~No ARM64 extractor~~ — **RESOLVED by scope** | Published builds are win-i686, linux-i686, linux-x86_64, and an x86_64 macOS build from 2015 | **`sampo` is declared x86-only.** The player stays portable and reaches ARM `[GDE-ARC-015]`. Not solved — scoped out, deliberately. An ARM64 Essentia build is a later option, never a prerequisite. |
| **`[GDE-FEX-139]`** ~~Licensing~~ — **RESOLVED by separation** | Essentia is AGPL-3.0/commercial dual-licensed | **`sampo` is AGPL-3.0; `vaino` stays MIT** `[GDE-ARC-018]`. Separate processes, communicating only via the shared SQLite file; nothing AGPL is linked into the player. Conservative by design — subprocess invocation is likely aggregation anyway. The distilled classifiers remain separable: models trained on AcousticBrainz data, not Essentia code. |
| **`[GDE-FEX-150]`** No local ground truth for the 47 complex dimensions until the dumps are mirrored | Certain, by construction `[GDE-FEX-060]` | Tier 0 resolves it. Until then, restrict validation to the 11 binary dimensions `mulib.db` provides, and label results as partial. |
| **`[GDE-FEX-160]`** Music post-dating the 2022 dump has no ground truth, ever | Certain | This is precisely the population Tier 1 exists to serve. Its accuracy can only be inferred from held-out performance on older music — state that limitation rather than implying coverage. |

---

## 6. Defects to Not Repeat

From the v1 post-mortem `[GDE-V1-020]`. Each would be caught by Stage A `[GDE-FEX-090]`:

1. **Resample properly.** `samples[::2]` on 44.1 kHz yields 22,050 Hz — not 16,000 — aliased, and time-scaled. Use a real resampler with an anti-alias filter.
2. **Analyze the whole recording.** A single 3-second patch from the midpoint is a near-random sample of a track's character. AcousticBrainz aggregates across the entire recording.
3. **Match the reference preprocessing exactly.** Compression, normalization and filterbank definition are part of the model contract, not implementation detail.
4. **Do not clamp or round outputs.** `[0.02, 0.98]` clamping and 3-decimal rounding discard exactly the discrimination the distance metric depends on.
5. **Tag provenance on write** `[GDE-FBD-020]`. Had this existed, the fact that 91% of v1's values were inherited rather than computed would have been visible on day one instead of a year later.

---

**See also:** [GUIDE001: Lineage & Lessons](GUIDE001-lineage-and-lessons.md) · [GUIDE002: Re-Architecture Plan](GUIDE002-rearchitecture-plan.md) · McRhythm [SPEC003: Musical Flavor](inherited/mcrhythm/MCR-SPEC003-musical_flavor.md)

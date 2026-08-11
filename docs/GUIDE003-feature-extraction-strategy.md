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

The outstanding lead is the **four enumerated string dimensions** (666 built against 670 expected): their integer codes and their positions in the index order are still unknown, and every index after the first of them may be shifted.

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

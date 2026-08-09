# SPEC005: Flavor Distance & Song Similarity

**Design Specification — Tier 2**

Defines how the distance between two Musical Flavor vectors is computed. This metric is what the Program Director consumes when it prunes, gathers and orders candidates ([GDE-PD-050](../GUIDE001-lineage-and-lessons.md)), so its quality directly determines whether "most similar" actually finds the most similar-sounding track.

> **Related:** [GUIDE003: Feature Extraction Strategy](../GUIDE003-feature-extraction-strategy.md) · [SPEC003: Program Director Intelligence](SPEC003-program-director-intelligence.md) · McRhythm [SPEC003: Musical Flavor](../../../McRhythm/docs/SPEC003-musical_flavor.md)

---

## 1. The Problem With Naive Euclidean Distance

**`[SPEC-FD-010]`** A flavor vector is not 71 independent numbers. It is **18 characteristics**, each a probability distribution over its own classes summing to 1.0:

- **Binary** characteristics (K=2) — `danceability`, `gender`, `mood_*`, `timbre`, `tonal_atonal`, `voice_instrumental`. Twelve of them. Each carries **one** degree of freedom; the second class is `1 − p`.
- **Complex** characteristics (K≥3) — `genre_dortmund` (9), `genre_tzanetakis` (10), `ismir04_rhythm` (10), `genre_rosamerica` (8), `genre_electronic` (5), `moods_mirex` (5). Six of them, K−1 degrees of freedom each.

**`[SPEC-FD-020]`** Squared Euclidean distance over all 71 raw values is wrong in three independent ways:

1. **Binary characteristics are double-counted.** `danceable` and `not_danceable` differ by exactly equal and opposite amounts, so each binary contributes `2(Δp)²` rather than `(Δp)²`.
2. **High-K characteristics dominate by dimension count.** `genre_tzanetakis` contributes 10 terms; `mood_happy` contributes 2. Genre would swamp mood for no principled reason.
3. **Characteristics have different natural scales.** Measured mean between-recording distance ranges from 0.219 (`genre_tzanetakis`) to 0.690 (`genre_rosamerica`) — a 3× spread. Unnormalized, the wide-spreading characteristics dominate regardless of how much they actually say.

MuLibPlay sidestepped all three by using only 11 binary probabilities, one per characteristic `[GDE-PD-050]`. That is coherent, but it discards the six complex characteristics entirely — and those turn out to be among the most reliable `[SPEC-FD-050]`.

---

## 2. The Metric

**`[SPEC-FD-030]` Per-characteristic distance is total variation:**

```
TV_c(a, b) = ½ · Σ_k | a_c,k − b_c,k |
```

Chosen because it unifies the two kinds without a fudge factor:
- For **K=2** it reduces exactly to `|Δp|` — the binary case, correctly single-counted.
- For **K≥3** it is the probability mass that must move to turn one distribution into the other.
- It is bounded `[0, 1]`, is a true metric (obeys the triangle inequality), and is well-behaved at zero — unlike KL divergence, which diverges when a class has zero probability, which is common here.

**`[SPEC-FD-040]` Aggregate distance:**

```
              Σ_c∈S  w_c · ( TV_c(a,b) / β_c )
D(a, b)  =    ─────────────────────────────────
                      Σ_c∈S  w_c
```

where `S` is the set of characteristics **present in both vectors** — the intersection rule inherited from McRhythm's `[MFL-DIST-010]`: never assume a missing characteristic is zero, because partial vectors are normal `[GDE-ARC-030]`.

- **`β_c`** — the characteristic's natural scale, its mean between-recording total variation `[SPEC-FD-050]`. Dividing by it puts every characteristic on a comparable "how unusual is this difference" footing.
- **`w_c`** — the characteristic's measured reliability `[SPEC-FD-050]`.

Normalizing by `Σ w_c` keeps `D` comparable across pairs even when different characteristics are available, which is essential for partial vectors.

---

## 3. Reliability — Measured, Not Assumed

**`[SPEC-FD-050]`** AcousticBrainz stores multiple submissions for ~9.5% of recordings: the same MusicBrainz recording, a different rip or encoding, through the same pipeline. Two submissions of one recording are a **parallel measurement of the same underlying song**, which makes this the classical test–retest reliability setup.

```
                 mean within-recording TV     (submission 0 vs 1)
reliability  =  1 − ───────────────────────
                 mean between-recording TV    (random distinct recordings)
```

A characteristic whose two submissions differ as much as two random songs carries no information (reliability 0). One that is identical across submissions while separating random songs carries full information (reliability 1).

Measured over **8,463 multi-submission recordings** and 15,000 random pairs from the 2022-06-23 sample dump:

| Characteristic | K | β_c (scale) | w_c (reliability) | | Characteristic | K | β_c | w_c |
| :--- | --: | --: | --: | :-- | :--- | --: | --: | --: |
| `genre_electronic` | 5 | 0.4137 | **0.880** | | `mood_aggressive` | 2 | 0.2843 | 0.790 |
| `genre_dortmund` | 9 | 0.3908 | **0.880** | | `ismir04_rhythm` | 10 | 0.6225 | 0.794 |
| `mood_sad` | 2 | 0.3152 | 0.879 | | `moods_mirex` | 5 | 0.3543 | 0.789 |
| `genre_rosamerica` | 8 | 0.6903 | 0.869 | | `mood_happy` | 2 | 0.3283 | 0.782 |
| `mood_acoustic` | 2 | 0.4050 | 0.866 | | `timbre` | 2 | 0.4338 | 0.784 |
| `genre_tzanetakis` | 10 | 0.2186 | 0.819 | | `tonal_atonal` | 2 | 0.4283 | 0.782 |
| `gender` | 2 | 0.3244 | 0.804 | | `mood_relaxed` | 2 | 0.3115 | 0.780 |
| | | | | | `mood_electronic` | 2 | 0.3806 | 0.773 |
| | | | | | `voice_instrumental` | 2 | 0.4383 | 0.766 |
| | | | | | `mood_party` | 2 | 0.3197 | 0.752 |
| | | | | | `danceability` | 2 | 0.4297 | **0.751** |

**`[SPEC-FD-055]` The headline finding: four of the six complex characteristics rank among the seven most reliable.** `genre_electronic` and `genre_dortmund` (0.880) beat every mood classifier. MuLibPlay discarded all six. Including them is the single largest available improvement to similarity quality `[SPEC-FD-060]`.

The reliability spread is narrow (0.75–0.88), so weighting is a refinement rather than a transformation. Values are re-derived, not hand-tuned; regenerate them whenever the flavor source changes.

---

## 4. Validation

**`[SPEC-FD-060]`** Metric designs compared by retrieval: given submission 0 of a recording as the query, how well does each metric rank submission 1 of the *same* recording against 499 random distractors? 1,500 queries.

| Metric | top-1 | top-5 | MRR |
| :--- | ---: | ---: | ---: |
| MuLibPlay — 11 binary dims, squared Euclidean | 76.2% | 80.5% | 0.785 |
| Total variation, all 18 characteristics, unweighted | 79.9% | 83.7% | 0.819 |
| **Total variation, scale-normalized + reliability-weighted** | **81.5%** | 83.6% | **0.827** |

Read honestly: **most of the gain comes from using all 18 characteristics** (+3.7 pp top-1), not from the weighting scheme (a further +1.6 pp). The weighting earns its place — it is nearly free once `β_c` and `w_c` are measured — but the specification's real content is *use the complex characteristics MuLibPlay threw away*.

**`[SPEC-FD-070]` Limitation — this validates robustness, not perceptual similarity.** The test measures whether a metric recognizes the same recording through a different encoding. That is necessary but not sufficient for "finds the most similar-sounding track": a metric could score well here and still rank perceptually unrelated songs as close.

**`[SPEC-FD-080]` Planned perceptual validation.** MuLibPlay's eight programs each carry 6–8 hand-picked seed tracks `[GDE-PD-040]` — direct human judgments that these songs belong together. A sound metric should place same-program seeds closer than cross-program seeds. Roughly 50 tracks across 8 programs is a small sample, but it is genuine perceptual signal from the actual user. Run once dump coverage of the seed tracks is available.

---

## 5. Provenance Consistency Outranks Per-Track Accuracy

**`[SPEC-FD-130]` Similarity is a relative judgment, so uniformity of scoring matters more than fidelity to any external reference.**

Distance is only ever computed *between two tracks in this library*. Absolute agreement with AcousticBrainz is therefore instrumental, never terminal. Two regimes differ sharply:

- **Uniform provenance** — every track scored by the same model from the same decoder on our own files. Whatever error the model has is **common-mode**: it shifts both sides of every comparison and largely cancels. Encoding variance is *zero*, because there is only one encode of each file — ours.
- **Mixed provenance** — some tracks from the dump, some locally extracted. Every cross-provenance comparison pays *both* an encoding difference and a model difference, and the metric cannot tell that systematic offset apart from genuine musical difference. The library splits into two subpopulations that the distance function silently treats as musically distinct.

**`[SPEC-FD-140]` Measured 2026-08-09 — the mixed-provenance penalty is confirmed and large; the predicted local-beats-dump advantage is not.**

The `[SPEC-FD-060]` retrieval test, run under three provenance regimes over 1,500 queries against 500 candidates:

| Regime | top-1 | top-5 | MRR |
| :--- | ---: | ---: | ---: |
| **A** all-teacher (dump only) | **77.9%** | 81.2% | **0.796** |
| **B** all-student (local only) | 76.7% | 81.1% | 0.789 |
| **C** mixed (local query, dump library) | **69.6%** | 77.3% | 0.733 |

**Confirmed, decisively: mixing provenance costs ~8 points of top-1 accuracy** — worse than *either* pure regime, exactly as the metric's construction predicts. This is the finding that matters for design, and it is robust.

**Not confirmed: all-local was predicted to beat all-dump, and it did not** — 76.7% against 77.9%, marginally behind. The prediction rested on the dump's encoding variance `[GDE-FEX-085]` being a net handicap; measurement says the student's own approximation error slightly outweighs the encoding variance it avoids.

Two caveats bound that negative result, neither yet resolved:
- The student here is the **iteration-1 shared MLP** (median err/β 0.223), not the final per-characteristic selection (0.152) `[LOG-I5-030]`. A rerun with the final models would likely close or reverse a 1.2-point gap.
- `[GDE-FEX-057]` found the dump holds a mean of 77 submissions per library recording. Regime A used single submissions; **averaged references would make the dump stronger still**, cutting against a rerun's favour.

So the honest ordering is **A ≈ B ≫ C**, with A and B within noise of each other and the gap between them unresolved.

**`[SPEC-FD-145]` The design conclusion survives, but for a different reason than argued.** Uniform provenance is required not because local extraction is *better* than the dump, but because mixing is materially worse than either. Since a distributable Vaino cannot ship the dump `[GDE-CHT-045]` and future acquisitions can never be in it `[GDE-FEX-025]`, uniform-local is the only regime reachable in the general case — and it costs at most ~1 point against an all-dump library we could not build anyway.

**`[SPEC-FD-150]` Design consequence — the dumps' role changes.** They remain essential as **validation ground truth** and as the source of the `β_c` and `w_c` constants `[SPEC-FD-050]`, but they should **not** be the production flavor values. Extracting the entire library locally is preferable to using dump values for the covered 93.7% `[GDE-FEX-055]` and local values for the rest — that split is precisely regime C, the one measurably worst option `[SPEC-FD-140]`.

Note this also removes the awkwardness in `[SPEC-FD-120]`: with uniform provenance there is no per-track provenance weighting to apply, because every track has the same provenance.

**`[SPEC-FD-160]` Status: measured 2026-08-09** by `tools/provenance_consistency_test.py`. The mixed-provenance penalty is confirmed at ~8 points of top-1. The predicted all-local advantage is **not** confirmed and remains open, bounded by the two caveats in `[SPEC-FD-140]`.

---

## 6. Implementation Notes

**`[SPEC-FD-090]`** `β_c` and `w_c` are corpus constants, computed once per flavor source and stored alongside it — not recomputed per query.

**`[SPEC-FD-100]`** Characteristics whose classes fail to sum to 1.0 ± 1e-4 are flagged, per McRhythm's `[MFL-DEF-040]`. Verified clean on 21,636 of 21,636 characteristic instances in the sample dump.

**`[SPEC-FD-110]`** User-defined characteristics `[GDE-MCR-060]` participate identically: they are distributions summing to 1.0 and take part in `S` whenever present in both vectors. Their `β_c` and `w_c` cannot be measured from AcousticBrainz submissions and must be assigned — default `w_c = 1.0`, `β_c` = observed mean between-recording TV over the library.

**`[SPEC-FD-120]`** Locally extracted flavor values carry their own reliability, which is bounded above by the ceiling in `[GDE-FEX-085]`. Where a characteristic comes from local extraction rather than the dump, `w_c` should be scaled by the extractor's measured agreement for that characteristic, so less trustworthy data automatically contributes less.

---

**Traceability:** `[SPEC-FD-010..120]` · derived from `[GDE-MCR-060]`, `[GDE-FEX-085]`, `[GDE-PD-050]`

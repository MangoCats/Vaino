# SPEC005: Flavor Distance & Song Similarity

**Design Specification — Tier 2**

Defines how the distance between two Musical Flavor vectors is computed. This metric is what the Program Director consumes when it prunes, gathers and orders candidates ([GDE-PD-050](../GUIDE001-lineage-and-lessons.md)), so its quality directly determines whether "most similar" actually finds the most similar-sounding track.

> **Related:** [GUIDE003: Feature Extraction Strategy](../GUIDE003-feature-extraction-strategy.md) · [SPEC009: Program Director](SPEC009-program-director.md) · McRhythm [SPEC003: Musical Flavor](../inherited/mcrhythm/MCR-SPEC003-musical_flavor.md) *(link corrected 2026-08-30 — pointed at Vaino's own v1 `SPEC003-program-director-intelligence.md`, deleted per `[GDE-DIS-010]`; the live document is `SPEC009`)*

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

Measured over **8,463 multi-submission recordings** and 15,000 random pairs from the 2022-06-23 **sample** dump:

> ✅ **`[SPEC-FD-051]` Regenerated 2026-08-09 on the library's own 7,685 multi-submission recordings.** The table below is now **superseded** by `[SPEC-FD-052]`; it is retained because the comparison between the two is itself a finding.

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

**`[SPEC-FD-052]` Library-native constants — normative.** Measured over the library's own 7,685 multi-submission recordings. **Use these.**

| Characteristic | β_c | w_c | | Characteristic | β_c | w_c |
| :--- | --: | --: | :-- | :--- | --: | --: |
| `genre_electronic` | 0.3315 | **0.743** | | `ismir04_rhythm` | 0.4954 | 0.578 |
| `genre_dortmund` | 0.2721 | **0.742** | | `mood_happy` | 0.2867 | 0.568 |
| `mood_sad` | 0.2447 | **0.734** | | `mood_party` | 0.2850 | 0.545 |
| `genre_rosamerica` | 0.5477 | **0.706** | | `danceability` | 0.3441 | 0.526 |
| `mood_acoustic` | 0.3336 | **0.704** | | `voice_instrumental` | 0.3795 | 0.517 |
| `gender` | 0.3448 | 0.666 | | `genre_tzanetakis` | 0.1175 | 0.516 |
| `mood_aggressive` | 0.2562 | 0.624 | | `tonal_atonal` | 0.3760 | 0.498 |
| `mood_relaxed` | 0.3438 | 0.620 | | `timbre` | 0.3709 | 0.482 |
| `moods_mirex` | 0.2979 | 0.600 | | `mood_electronic` | 0.3107 | 0.454 |

**`[SPEC-FD-053]` Two differences from the generic sample, both consequential.**

*β is smaller for 16 of 18 characteristics* — the library is far more homogeneous in flavor than a random slice of AcousticBrainz, which is unsurprising for one person's collection. Distances are compressed, so the same raw difference reads as a larger normalized distance. This is also why MuLibPlay's pool parameters `[SPEC-DIR-200]` are better founded than they looked: they were tuned *on this library's own distance distribution*.

*Reliability is uniformly lower* — 0.45–0.74 here against 0.75–0.88 on the sample. AcousticBrainz is **less** self-consistent on our library, not more. The likely cause is popularity: our recordings carry a mean of 77 submissions from many different rips `[GDE-FEX-057]`, where a random sample's multi-submission recordings often have just two, frequently from similar sources. More submitters means more encoding diversity means more spread.

**`[SPEC-FD-055]` The headline finding survives regeneration: four of the six complex characteristics rank among the seven most reliable.** `genre_electronic` (0.743) and `genre_dortmund` (0.742) still beat every mood classifier, and `genre_rosamerica` sits fourth — the same ordering the generic sample gave. MuLibPlay discarded all six. Including them remains the single largest available improvement to similarity quality `[SPEC-FD-060]`.

The reliability spread is narrow (0.75–0.88), so weighting is a refinement rather than a transformation. Values are re-derived, not hand-tuned; regenerate them whenever the flavor source changes.

---

## 4. Validation

**`[SPEC-FD-060]`** Metric designs compared by retrieval: given submission 0 of a recording as the query, how well does each metric rank submission 1 of the *same* recording against 499 random distractors? 1,500 queries.

> **Not comparable with the table in §5.** This section varies the *metric* with provenance held constant; §5 `[SPEC-FD-140]` varies the *provenance* with the metric held constant. Different baselines and different questions — the two sets of percentages must not be read as a progression.

| Metric | top-1 | top-5 | MRR |
| :--- | ---: | ---: | ---: |
| MuLibPlay — 11 binary dims, squared Euclidean | 76.2% | 80.5% | 0.785 |
| Total variation, all 18 characteristics, unweighted | 79.9% | 83.7% | 0.819 |
| **Total variation, scale-normalized + reliability-weighted** | **81.5%** | 83.6% | **0.827** |

Read honestly: **most of the gain comes from using all 18 characteristics** (+3.7 pp top-1), not from the weighting scheme (a further +1.6 pp). The weighting earns its place — it is nearly free once `β_c` and `w_c` are measured — but the specification's real content is *use the complex characteristics MuLibPlay threw away*.

**`[SPEC-FD-070]` Limitation — this validates robustness, not perceptual similarity.** The test measures whether a metric recognizes the same recording through a different encoding. That is necessary but not sufficient for "finds the most similar-sounding track": a metric could score well here and still rank perceptually unrelated songs as close.

**`[SPEC-FD-080]` Perceptual validation — run 2026-08-10. The metric agrees with the listener.** MuLibPlay's eight programmes each carry 6–8 hand-picked seeds `[GDE-PD-040]` — direct human judgments that these songs belong together. All 49 have flavor.

| | mean distance | pairs |
| :--- | ---: | ---: |
| within programme | 0.9895 | 128 |
| across programmes | 1.1931 | 1,048 |
| **ratio** | **0.829** | |

**Same-programme seeds sit 17.1% closer than cross-programme seeds.** This is the first evidence that the metric tracks *perceptual* similarity rather than merely recognising a re-encode `[SPEC-FD-070]`, and it was obtained on 11 binary characteristics alone.

**`[SPEC-FD-082]` The per-programme spread is the more useful result**, because it points at what is missing:

| Programme | seeds | mean d | vs library mean |
| :--- | ---: | ---: | ---: |
| Loud | 6 | 0.784 | 66% |
| Soft | 6 | 0.841 | 71% |
| Mellow | 6 | 0.877 | 74% |
| Prog | 6 | 0.890 | 75% |
| Fun | 6 | 1.033 | 87% |
| Cool | 5 | 1.093 | 92% |
| Groove | 8 | 1.137 | 95% |
| Light | 6 | 1.168 | 98% |

The programmes that cohere are the ones the 11 binaries can express — *Loud* is largely `mood_aggressive` and `timbre`, *Soft* and *Mellow* largely `mood_relaxed` and `mood_acoustic`. The programmes that barely cohere at all — *Light* at 98% of the library mean, *Groove* at 95% — are the ones defined by **genre and rhythm**, which is precisely what the six absent complex characteristics carry `[SPEC-FD-085]`.

This is a concrete, testable prediction: extracting `genre_*`, `ismir04_rhythm` and `moods_mirex` should tighten *Light* and *Groove* markedly, and *Loud* and *Soft* comparatively little. Re-run this measurement after extraction; it is the cheapest available check on whether extraction bought anything.

**Caveat as before:** 49 seeds over 8 programmes is a small sample, and one listener's groupings are not a general perceptual standard. Treat 0.829 as encouraging, not as a validated figure.

**`[SPEC-FD-083]` Tested 2026-08-11 with the complex characteristics present — the prediction was right about *Light* and wrong overall.**

55 programme seeds were extracted locally and classified through all 18 reproduced Gaia chains `[GDE-FEX-102]`; 35 have both inherited and local flavor, so the comparison holds the seed set fixed and varies only the features.

| feature set | within | cross | ratio |
| :--- | ---: | ---: | ---: |
| inherited, 11 characteristics | 0.9957 | 1.2162 | **0.8187** |
| local, 18 characteristics | 0.9764 | 1.1387 | **0.8575** |

**Aggregate separation got worse, not better.** But the per-programme breakdown shows why, and it is not a flat refutation:

| programme | n | inherited 11 | local 18 | local, complex only |
| :--- | ---: | ---: | ---: | ---: |
| **Light** | 6 | 1.168 | 0.973 | **0.788** |
| Fun | 6 | 1.033 | 0.946 | 0.822 |
| Cool | 5 | 1.093 | 1.057 | 0.938 |
| Prog | 4 | 0.728 | 0.679 | 0.631 |
| Loud | 2 | 1.075 | 1.038 | 0.948 |
| **Mellow** | 6 | 0.877 | 1.088 | **1.388** |
| **Soft** | 5 | 0.914 | 0.953 | **1.155** |

*Light* — the worst-cohering programme, and the specific case `[SPEC-FD-082]` named — tightened from 1.168 to 0.788 on the complex characteristics alone. *Fun*, *Cool*, *Prog* and *Loud* improved too. **The aggregate fell because *Mellow* and *Soft* degraded sharply**, and those are precisely the programmes the mood binaries express well and genre/rhythm does not.

The honest reading: **the complex characteristics are not uniformly better, they are differently informative.** A metric weighting all 18 equally trades away what mood captures to gain what genre captures.

Three caveats, the second serious enough to require work before this is treated as settled:

1. **Small n** — 35 seeds, 2–6 per programme. *Loud* rests on a single pair.
2. **The constants are wrong for this data.** `β_c` and `w_c` were measured on *dump* values `[SPEC-FD-052]`, and `[SPEC-FD-090]` states they are per flavor source. They are applied here to *locally extracted* values, so every complex characteristic is scaled by a β measured on a different corpus. That alone could produce the *Mellow*/*Soft* degradation, and it must be re-derived on local values before the comparison means anything.
3. **The baseline is confounded** — inherited-11 versus local-18 differs in provenance as well as in feature count, so it is not a clean "more characteristics" test.

**Consequence for `[SPEC-DIR-200]`:** re-deriving the pool parameters should wait until the constants are re-derived, not merely until the vector grows.

**`[SPEC-FD-084]` Resolved 2026-08-13 on the fully extracted library — and the ratio was the wrong measure.**

The library is now uniformly local: **8,073 of 8,079 passages** extracted per passage and classified through all 18 reproduced chains `[GDE-FEX-102]`, 7,894 recordings, 0.07% loss.

*The constants were re-derived on local values* `[SPEC-FD-090]`, using the 163 recordings that appear in more than one passage as the test–retest set. **Local reliability is higher for every one of the 18:**

| | mean `w_c` | range |
| :--- | ---: | :--- |
| dump-derived `[SPEC-FD-052]` | 0.60 | 0.45 – 0.74 |
| **local** | **0.77** | 0.67 – 0.88 |

`timbre` 0.482 → 0.792, `mood_electronic` 0.454 → 0.746, `genre_tzanetakis` 0.517 → 0.669. This is `[GDE-FEX-028]`'s argument measured directly: the dump's low self-consistency came from ~77 submissions per recording across many rips `[GDE-FEX-057]`; one pipeline over our own files has far less within-recording variance. **Uniform local provenance is not merely equal to the dump — it is measurably more self-consistent.**

*The comparison, on the 48 seeds present in both, isolating each change:*

| | within | cross | ratio | **P@1** | **P@3** | **MRR** |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| inherited 11 + dump constants | 0.9924 | 1.1987 | 0.8279 | 0.188 | 0.236 | 0.406 |
| local 18 + dump constants | 0.9551 | 1.1235 | 0.8502 | **0.271** | 0.229 | **0.458** |
| local 18 + local constants | 0.8876 | 1.0362 | 0.8566 | **0.271** | **0.243** | 0.451 |

**The two measures disagree, and the ratio is the one to discard.** It worsens monotonically while retrieval improves by 44% relative on P@1 (chance is 0.106). The reason is visible in the columns: within-programme distance improved 10.6% and cross-programme improved 13.6%, so the *ratio* fell even though everything cohered better. A metric that compresses the whole space uniformly looks worse by ratio and is not worse.

**Retrieval is the measure that matches the consumer.** Stage B gathers the passages nearest each seed `[SPEC-DIR-145]`; it never computes a within/cross ratio. `[SPEC-FD-080]`'s ratio was a reasonable first proxy and should now be read alongside P@1/MRR rather than alone.

*Per programme, absolute cohesion, inherited → local+local:* seven of eight improved.

| | inh+dump | loc+local | | | inh+dump | loc+local |
| :--- | ---: | ---: | :-- | :--- | ---: | ---: |
| **Light** | 1.168 | **0.849** | | Prog | 0.876 | 0.715 |
| Groove | 1.137 | 1.051 | | Loud | 0.784 | 0.725 |
| Cool | 1.093 | 0.940 | | Soft | 0.841 | 0.811 |
| Fun | 1.033 | 0.855 | | **Mellow** | 0.877 | **0.973** |

*Light* — `[SPEC-FD-082]`'s named prediction — improved most, from worst-cohering to mid-pack. *Mellow* is the sole regression and remains unexplained.

**`[SPEC-FD-086]` Full-library picture, 2026-08-13.** `flavorcheck` over the extracted library:

| | before (inherited 11) | after (local 18) |
| :--- | ---: | ---: |
| subjects | 7,897 | **7,911** |
| **incomparable pairs** | 74 | **0** |
| malformed instances | 0 | 0 |
| median distance | 1.052 | 0.972 |

**Incomparable pairs fall to zero.** Previously 37 recordings carried only user characteristics and could not be compared to anything `[SPEC-FD-085]`; every recording now has the full vector, so no passage is unreachable by similarity. That is a plain correctness gain independent of any accuracy argument.

All 49 seeds now have flavor, against 35 with both provenances:

| programme | mean d | vs library | *(was, inherited 11)* |
| :--- | ---: | ---: | ---: |
| Loud | 0.725 | 70% | *0.784 — 66%* |
| Prog | 0.759 | 73% | *0.890 — 75%* |
| Soft | 0.811 | 78% | *0.841 — 71%* |
| **Light** | **0.849** | **82%** | *1.168 — 98%* |
| Fun | 0.855 | 83% | *1.033 — 87%* |
| Cool | 0.940 | 91% | *1.093 — 92%* |
| Mellow | 0.973 | 94% | *0.877 — 74%* |
| Groove | 1.051 | 102% | *1.137 — 95%* |

*Light* moves from worst-cohering to fourth. *Mellow* and *Groove* are the two that worsen relative to the library mean, and neither is explained.

**Caveat that bounds all of this: n = 48.** A P@1 difference of 0.083 is four seeds. The direction is consistent across three measures and the mechanism is understood, but this is not a statistically strong result, and `[SPEC-FD-080]`'s caveat stands — one listener's groupings are not a general perceptual standard.

---

## 5. Provenance Consistency Outranks Per-Recording Accuracy

**`[SPEC-FD-130]` Similarity is a relative judgment, so uniformity of scoring matters more than fidelity to any external reference.**

Distance is only ever computed *between two recordings in this library*. Absolute agreement with AcousticBrainz is therefore instrumental, never terminal. Two regimes differ sharply:

- **Uniform provenance** — every recording scored by the same model from the same decoder on our own files. Whatever error the model has is **common-mode**: it shifts both sides of every comparison and largely cancels. Encoding variance is *zero*, because there is only one encode of each file — ours.
- **Mixed provenance** — some recordings from the dump, some locally extracted. Every cross-provenance comparison pays *both* an encoding difference and a model difference, and the metric cannot tell that systematic offset apart from genuine musical difference. The library splits into two subpopulations that the distance function silently treats as musically distinct.

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

Note this also removes the awkwardness in `[SPEC-FD-120]`: with uniform provenance there is no per-recording provenance weighting to apply, because every recording has the same provenance.

**`[SPEC-FD-160]` Status: measured 2026-08-09** by `tools/provenance_consistency_test.py`. The mixed-provenance penalty is confirmed at ~8 points of top-1. The predicted all-local advantage is **not** confirmed and remains open, bounded by the two caveats in `[SPEC-FD-140]`.

---

## 5a. Future Direction — Entry and Exit Flavor

**`[SPEC-FD-170]` Not the current target. Recorded so it is not lost.**

Flow ordering `[SPEC-DIR-160]` currently matches the **whole-recording** flavor of a candidate against the **whole-recording** flavor of the passage already queued. But a handover is not a whole-recording event: what the listener actually hears is the *end* of one passage against the *start* of the next, and many recordings differ markedly at their edges — a quiet intro, a fade, a long outro, a false ending, a recording that opens sparse and closes dense.

The proposal: alongside the whole-recording vector, characterise the **first three minutes** and the **last three minutes** of each passage (where the passage is long enough to have them), and let flow match `exit(previous) → entry(next)` instead of `whole(previous) → whole(next)`.

That is a better model of the thing being optimised. Whole-recording similarity answers "do these two belong in the same programme"; edge similarity answers "does this transition work", and Stage C is asking the second question with an instrument built for the first.

**Three things to know before attempting it:**

1. **It roughly triples extraction cost.** Lowlevel features are *aggregates* over the analysed window, so an entry vector cannot be derived from a whole-recording extraction — it needs its own run. At `[GDE-FEX-104]`'s 6.4 s per audio-minute, adding two 3-minute windows per passage takes the library from ~64 to roughly ~190 core-hours. The `lowlevel_cache` key `(audio_md5, start_ms, end_ms)` `[SPEC-SC-080]` already accommodates the extra rows without schema change.
2. **`flavor.subject_kind` needs a third value**, or a segment discriminator. It currently allows `recording` and `passage`; entry/exit are neither.
3. **Short passages have no edges.** A 2-minute recording is its own entry and exit, so the metric must fall back to whole-recording rather than compare a 3-minute window against a 2-minute one. `[SPEC-FD-040]`'s intersection rule handles the shape of this, but the fallback should be explicit.

**Current design target remains a single flavor vector per passage.** This entry is a direction, not a commitment: it should be attempted only once whole-recording flow is in listening use and the transitions it produces can be judged against the ones edge-matching would produce.

---

## 6. Implementation Notes

**`[SPEC-FD-085]` Implemented 2026-08-10** in `player/src/director/flavor.rs`, measured by `flavorcheck`. Four findings, three of them limits on what the metric can currently do.

*Cost is not a constraint.* 7,897 subjects load in 0.14 s; a distance costs **0.1 µs**. Stage B weighing the whole library against five seeds is ~4 ms, so the pool parameters `[SPEC-DIR-200]` can be re-derived on merit rather than on budget. Vectors are stored flat with a `u64` presence bitmask; a map lookup per class would have dominated.

*`[SPEC-FD-100]` verified on the library itself* — 0 malformed characteristic instances, where the spec had previously verified only the sample dump.

*The six complex characteristics are absent from the library.* They have constants but **no values**: the migrated flavor is MuLibPlay's 11 binaries plus 4 user characteristics. So `[SPEC-FD-055]`'s headline — that including the complex characteristics is the single largest available improvement — **cannot be exercised until Sampo extracts them**. The schema is built from the data rather than the constants table, so they will appear automatically when it does, with no code change. Until then the metric runs on the same information MuLibPlay had, keeping only the scale-normalisation and reliability weighting worth ~+1.6 pp of the measured +5.3.

> ✅ **Superseded by `[SPEC-FD-084]`/`[SPEC-FD-086]`, 2026-08-13.** The condition above no longer holds: the library is now uniformly local, 8,073 of 8,079 passages extracted and classified through all 18 reproduced chains — the complex characteristics do carry values. Retained for the reasoning (cost, schema-driven appearance), not as a current statement of the library's contents.

*User-defined characteristics record only their positives, so they cannot yet contribute* `[SPEC-FD-110]`. Every holder of `user.christmas` has `christmasy = 1.0`; among its 41 holders it never varies, so its measured β is 0 and it is excluded — correctly, since a characteristic with no between-recording variation carries no discriminative information. **To participate in distance, non-members must carry an explicit 0.0.** As stored they are usable as occasion values `[SPEC-DIR-130]`, where only the positives matter, and unusable as flavor. 37 recordings carry *only* user characteristics and are therefore incomparable to the rest — reported as incomparable rather than as distant, per `[SPEC-FD-040]`.

**`[SPEC-FD-087]` Neighbour inspection, the only perceptual check available before `[SPEC-FD-080]`.** On 11 binary characteristics the nearest neighbours are already plausible:

| Query | Nearest |
| :--- | :--- |
| Fatboy Slim — *Santa Cruz* | Sneaker Pimps (0.314), Moby (0.328) |
| America — *Muskrat Love* | Springsteen — *I'm on Fire* (0.245), Peter, Paul & Mary (0.269) |
| Radiohead — *You* | Metallica — *Fuel* (0.194), Alice in Chains — *Would?* (0.205) |

The third is the interesting one: *You* is among Radiohead's heaviest early tracks, and the metric places it with hard rock rather than with the rest of their catalogue. That is the behaviour wanted — similarity by sound, not by artist. Distances span 0.221–2.391 with a median of 1.052. This is not evidence of perceptual validity, which `[SPEC-FD-080]` is for; it is evidence that nothing is obviously broken.

**`[SPEC-FD-090]`** `β_c` and `w_c` are corpus constants, computed once per flavor source and stored alongside it — not recomputed per query. A characteristic with no measured constant has `w_c = 1.0` and β estimated from a bounded sample of pairs **drawn from the subjects that carry it** — sampling the whole library would almost never draw two holders of a characteristic that sits on a few dozen recordings.

**`[SPEC-FD-100]`** Characteristics whose classes fail to sum to 1.0 ± 1e-4 are flagged, per McRhythm's `[MFL-DEF-040]`. Verified clean on 21,636 of 21,636 characteristic instances in the sample dump.

**`[SPEC-FD-110]`** User-defined characteristics `[GDE-MCR-060]` participate identically: they are distributions summing to 1.0 and take part in `S` whenever present in both vectors. Their `β_c` and `w_c` cannot be measured from AcousticBrainz submissions and must be assigned — default `w_c = 1.0`, `β_c` = observed mean between-recording TV over the library.

**`[SPEC-FD-120]`** *Superseded by `[SPEC-FD-145]`/`[SPEC-FD-150]`, 2026-08-13 — kept for its history, not as current design.* This originally proposed scaling `w_c` by a locally-extracted characteristic's own measured agreement, bounded above by the ceiling in `[GDE-FEX-085]`, so a less trustworthy characteristic contributed less. That assumed a library holding a mix of dump-sourced and locally-extracted values with different reliability per recording. Once provenance was made uniform-local `[SPEC-FD-084]`, the premise disappeared: every recording shares the same extractor, so there is no per-recording provenance signal left to scale by — `[SPEC-FD-150]` says this in as many words. `w_c` in `player/src/director/flavor.rs` is, correctly, a single corpus-wide reliability per characteristic (`[SPEC-FD-052]`), not a per-value one; `flavor.accuracy` (`[SPEC-SC-070]`) is populated but not read into distance, and that is no longer a gap.

---

**Traceability:** `[SPEC-FD-010..120]` · derived from `[GDE-MCR-060]`, `[GDE-FEX-085]`, `[GDE-PD-050]`

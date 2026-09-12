# GUIDE012: Blocking the Same Song Across Different Recordings

**Development Guidance — scoped 2026-09-11, from a live incident on `vainopi`**

The related-recording rotation of `[SPEC-DIR-116]` is fully implemented, fully
tested, and has never had a row to act on. This document scopes filling it from
MusicBrainz **Works**, and measures that option against the cheaper one it
would replace. Nothing is asserted here without a measurement behind it
`[GOV-SRC-020]`.

> **Related:** [SPEC037](spec/SPEC037-eligibility-and-frequency.md) `[SPEC-DIR-116]` — the rule this feeds · [SPEC008](spec/SPEC008-database-schema.md) — the `recording_relations` junction · [SPEC007](spec/SPEC007-sampo-architecture.md) — the S3 identification stage this extends · [GOV002](GOV002-sources-of-truth.md) — ranking by measurement

---

## 1. The gap, as it actually presented

**`[GDE-WRK-010]` A recording rotation keyed on the MBID blocks a *rendering*,
not a song.** On 2026-09-11 the Director queued "Funeral for a Friend / Love
Lies Bleeding" 29 minutes after playing it. Two passages of the same file,
same artist, same title, two different recording MBIDs — `49323205…` at 646.5 s
and `891cb542…` at 709.7 s — and so two independent rotation histories. The
second had never played, and nothing suppressed it.

That incident had a second, unrelated cause on the artist side, which is not
this document's subject. The recording-side cause is: **`recording_relations`
holds 0 rows**, so a passage can only ever be blocked by its own exact MBID.

**`[GDE-WRK-015]` Identification is not the problem.** All 8,330 radio passages
carry a recording id — 8,167 real MusicBrainz MBIDs and 163 synthetic
`local:audio:…` ids for unidentified segments. The MBID-equality block works
exactly as intended where it applies: 176 MBIDs span 368 radio passages, and
those pairs already share a rotation.

**`[GDE-WRK-020]` The exposed surface is 245 artist+title groups covering 500
distinct recordings** — songs the library holds more than once under MBIDs
that differ legitimately. A MusicBrainz *Recording* is a specific rendering;
a remaster, a 5.1 mix, an acoustic take and a live version are four Recordings
by design. The entity meaning "the same song" is the **Work**.

---

## 2. The player needs no changes

**`[GDE-WRK-030]` This is catalogue work, on Sampo's side of the line.** Stage A
already implements the rule — [`player/src/director/frequency.rs`](../player/src/director/frequency.rs)
blocks and damps on `Related`, [`player/src/director/library.rs`](../player/src/director/library.rs)
loads `recording_relations`, and `Census.related_blocked` already counts the
exclusions, so a fill becomes visible in the pool figures the moment it lands.
No player code is in scope.

---

## 3. Four constraints the existing schema imposes

**`[GDE-WRK-040]` The junction is asymmetric.** `recording_relations` is keyed
`(mbid, related_mbid)` in [`sql/schema.sql`](../sql/schema.sql) and the loader
indexes only on `mbid`. A work class of *N* recordings therefore needs
**N×(N−1) directed rows**, not N(N−1)/2. Both directions, or the block runs one
way only.

**`[GDE-WRK-045]` `strength` cannot soften a block, only speed a recovery.** In
`weigh`, a related recording inside the rotation window returns
`RelatedRotationBlock` *before* strength is read; strength then scales the
recovery window. `strength: 0.0` still blocks fully — it merely skips the
damping ramp. **Relating a pair is binary**: they share a full rotation, or
they do not. There is no "related, but gently."

**`[GDE-WRK-050]` Therefore covers are a policy decision taken before any row
is written.** A shared Work is exactly what links two artists' versions of one
song. Relating them means hearing one blocks the other for the recording's full
rotation, with no dial to soften it. Restricting the pass to same-artist-credit
classes is the conservative default and is what §6 recommends.

**`[GDE-WRK-055]` A multi-work recording would chain classes together.** Under
naive closure, a recording linking to works W1 and W2 pulls every performance
of both into one class, and through them each other's. Measured rarity below
makes this cheap to guard; the guard must be *excluding* the relation, since
`[GDE-WRK-045]` leaves no weaker setting.

---

## 4. Measured — what a Work pass actually buys

**`[GDE-WRK-060]` Method.** 559 recordings fetched from
`musicbrainz.org/ws/2/recording?inc=work-rels`, 2026-09-11, one request per
second with the same citizenship rules as
[`tools/fetch_releases.py`](../tools/fetch_releases.py): every one of the 500
recordings in a same-artist/same-title group, plus all 78 Elton John radio
recordings as a complete single-artist view. 556 returned, 3 errored.

**`[GDE-WRK-065]` Work coverage is 92.1%** — 512 of 556 recordings carry at
least one `performance` relation. The 44 without are dominated by instrumental
and new-age material, where nobody has created the Work.

Relation attributes: 658 plain, 171 `live`, 41 `cover`, 2 `partial`,
1 `instrumental`.

**`[GDE-WRK-070]` `[GDE-WRK-055]`'s chaining risk is negligible here: 1 recording
in 556 links to more than one work** — and that one is a Boccherini minuet
linked to two MusicBrainz works that are plainly the *same* work entered twice,
not a medley. Note that MusicBrainz models the incident's own medley,
"Funeral for a Friend / Love Lies Bleeding", as a **single** Work
(`ed2f3608…`), which is the behaviour this rule wants.

**`[GDE-WRK-075]` The artist+title heuristic is far more accurate than expected,
and that reverses the case for Work.** Of 244 groups:

| | groups |
| :--- | ---: |
| Work **confirms** the pairing | 214 |
| undecidable — a member has no Work at all | 26 |
| Work **contradicts** — disjoint works | **1** |
| not fetched | 3 |

Where Work can judge at all, artist+title is wrong **1 time in 215 — 0.5%**.
The single contradiction is two recordings titled "I Love You" against two
distinct work MBIDs both titled "I Love You", which is at least as likely to be
a duplicated Work upstream as a real distinction.

> An earlier reading of the partial data put this at ~14%. That was an artifact
> of counting *undecidable* groups as disagreements. They are not the same
> thing, and the corrected figure argues the opposite way.

**`[GDE-WRK-080]` Work's real advantage is recall, not precision — and it is
large.** Variants carry different titles, so artist+title cannot see them.
Measured across all 78 Elton John radio recordings:

| | classes | recordings covered | directed rows |
| :--- | ---: | ---: | ---: |
| artist+title | 9 | 18 | 18 |
| **Work** | **19** | **39** | **42** |

Work more than doubles both the classes found and the recordings brought under
a shared rotation. Every one of the 14 cross-title pairs the sample found is
of this shape: `Candle in the Wind (acoustic)` ↔ `Candle in the Wind`,
`Tower of Babel` ↔ `Tower of Babel (live)`.

**`[GDE-WRK-085]` Classes are small, so the table stays small.** Across the
whole 559-recording sample: 227 classes, 463 recordings covered, **490 directed
rows**, and every class is size 2 (218) or 3 (9) — about 2.2 rows per class.

Elton John's density — 0.54 directed rows per recording — is an **upper
region, not a rate**: that artist's radio passages include a four-hour deluxe
compilation carrying the album, its 5.1 mixes, bonus tracks and live versions.
Applied to 8,167 recordings it suggests low thousands of rows, and the full
pass is what would settle it.

**`[GDE-WRK-090]` Cross-artist collision is the one thing this sample cannot
measure.** Both populations are artist-scoped by construction, so the measured
0 cross-artist pairs means nothing. 41 `cover` attributes confirm covers are
present in the library. Whether two *different* library artists share a Work —
the case `[GDE-WRK-050]` turns into a four-day block — is answerable only by
the full-library pass.

---

## 5. The cheap option, which the measurement promotes

**`[GDE-WRK-100]` Artist+title needs no network and can land today.** It is a
local query over `recordings` and `recording_artists`, it closes the incident's
own case, and at 0.5% measured error `[GDE-WRK-075]` it is not the blunt
instrument it looked like. What it cannot do is see variants under different
titles, which is half the population `[GDE-WRK-080]`.

**`[GDE-WRK-105]` The two compose rather than compete**, because
`recording_relations.source` already distinguishes provenance. Write
`title:artist-exact` rows now; write `work:musicbrainz` rows when the crawl
finishes; drop the former where the latter supersedes it. Neither pass has to
wait for the other, and the listener gets the Elton John case fixed in an
afternoon instead of after an eleven-hour crawl.

---

## 6. What building it involves

**`[GDE-WRK-110]` Store the Work; derive the relations.** Add `works` and
`recording_works` rather than writing `recording_relations` straight from the
API. Every policy question in §3 will be revisited, and re-deriving relations
from a local table is instant where re-fetching is hours. It also keeps the
`source` column honest.

**`[GDE-WRK-115]` The cache cannot be mined; the pass must be fresh.**
`musicbrainz_cache` holds 8,747 rows, all `recording+releases` or
`release+recordings` — fetched without `inc=work-rels`, so no work data is in
them. A single pass with `inc=releases+work-rels` refreshes both at once and
makes the crawl serve two purposes.

**`[GDE-WRK-120]` Cost: 8,167 recordings.** A clean 1 req/s is 2.3 hours; the
rate actually observed on 2026-09-11 was ~4.9 s/request under throttling, so
budget **5–11 hours**. Resumable and cached, same shape as
`fetch_releases.py` — meant to be left running and interruptible.

**`[GDE-WRK-125]` Stages.**

| stage | depends on | why here |
| :--- | :--- | :--- |
| 1 · `works`/`recording_works` schema | — | nothing is testable without it |
| 2 · a new work-fetch tool, beside `fetch_releases.py` | 1 | read-only network, cannot damage anything |
| 3 · artist+title fill, `source='title:artist-exact'` | — | independent; lands first, fixes the live case |
| 4 · derive `work:musicbrainz` rows, same-artist only | 2 | the policy of `[GDE-WRK-050]` applied |
| 5 · measure cross-artist collisions, then decide | 4 | `[GDE-WRK-090]` — a decision against a number |

**`[GDE-WRK-130]` Transport already works, by accident.** `vainopi` carries the
whole 1.17 GB `library.db`, caches included, so relations ship with the file as
things stand. [`tools/payload.py`](../tools/payload.py) does **not** carry
`recording_relations`, so a move to bundle-only sync would need that gap closed
first.

---

## 7. Open

- **`[GDE-WRK-200]`** Cross-artist Work collisions are unmeasured `[GDE-WRK-090]`.
  Until they are, restrict to same-artist-credit classes.
- **`[GDE-WRK-210]`** The 7.9% of recordings with no Work `[GDE-WRK-065]` have no
  path to relation except artist+title or a hand edit.
- **`[GDE-WRK-220]`** Whether a `live` or `partial` performance should share a
  full rotation with the studio take is a listening judgement, not a data
  question, and `[GDE-WRK-045]` allows only yes or no.
- **`[GDE-WRK-230]`** Class sizes beyond this sample are unmeasured — the
  projection in `[GDE-WRK-085]` rests on one dense artist.

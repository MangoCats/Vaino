# GUIDE012: Blocking the Same Song Across Different Recordings

**Development Guidance — scoped 2026-09-11 from a live incident on `vainopi`;
figures replaced 2026-09-12 when the full-library crawl finished**

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
`local:audio:…` ids for unidentified segments. Those counts are `vainopi`'s;
the desktop catalogue the crawl actually read reaches 8,008, and the two halves
of the fleet are not identical `[GOV-SRC-030]` — a derivation run against one
does not describe the other. The MBID-equality block works
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
exclusions, so a fill shows up in the pool figures the moment it lands. No
player code is in scope.

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

**`[GDE-WRK-060]` Method.** Every recording reachable from a radio passage —
8,008 of them — fetched from `musicbrainz.org/ws/2/recording?inc=work-rels`,
one request per second with the same citizenship rules as
[`tools/fetch_releases.py`](../tools/fetch_releases.py). The crawl ran
2026-09-11 20:29 to 2026-09-12 05:36; 8,004 returned and 4 errored. A
559-recording probe sized the work first, and where its figures differed they
are noted — a sample drawn from *known-duplicated* songs is biased toward
exactly what it was measuring.

**`[GDE-WRK-065]` Work coverage is 89.0%** — 7,120 of 8,004 recordings carry at
least one `performance` relation. The 884 without are dominated by instrumental
and new-age material, where nobody has created the Work. *(The probe said
92.1%; the sample's bias explains the gap.)*

**`[GDE-WRK-070]` `[GDE-WRK-055]`'s chaining risk is real but small: 68
recordings of 8,004 — 0.85% — link to more than one Work.** The probe found 1
in 556 and called it negligible, which was the sample talking. 68 is still
small enough to guard by exclusion rather than design, but it is not zero and
a derivation step must handle it deliberately. Note that MusicBrainz models the
incident's own medley, "Funeral for a Friend / Love Lies Bleeding", as a
**single** Work (`ed2f3608…`), which is the behaviour this rule wants.

**`[GDE-WRK-075]` The artist+title heuristic is far more accurate than expected,
and that reverses the case for Work.** Of 244 groups:

| | groups |
| :--- | ---: |
| Work **confirms** the pairing | 214 |
| undecidable — a member has no Work at all | 26 |
| Work **contradicts** — disjoint works | **1** |
| not fetched | 3 |

Where Work can judge at all, artist+title is wrong **1 time in 215 — 0.5%**.
The lone contradiction is two recordings titled "I Love You" against two
distinct work MBIDs *also* both titled "I Love You" — likelier a duplicate
upstream than a real distinction.

> An earlier reading of the partial data put this at ~14%. That was an artifact
> of counting *undecidable* groups as disagreements. They are not the same
> thing, and the corrected figure argues the opposite way.

**`[GDE-WRK-080]` Work's real advantage is recall, not precision — and it is
large.** Variants carry different titles, so artist+title cannot see them.
Over the whole library, Work restricted to same-artist pairs `[GDE-WRK-095]`
against the heuristic it would replace:

| | classes | recordings covered | directed rows |
| :--- | ---: | ---: | ---: |
| artist+title | 250 | 509 | 536 |
| **Work, same-artist** | **407** | **904** | **1,260** |

**1.8× the recordings brought under a shared rotation**, and 2.4× the rows.
The gap is entirely variants under a different title:
`Candle in the Wind (acoustic)` ↔ `Candle in the Wind`,
`Tower of Babel` ↔ `Tower of Babel (live)`.

**`[GDE-WRK-085]` The table stays small: 495 classes over 1,074 recordings,
1,494 directed rows.** Most are pairs (422), then 60 of size 3 and 9 of size 4;
the tail runs longer than the probe suggested, which saw nothing above 3. The
largest are single-artist version sets — Moby's "Extreme Ways" ×10, Depeche
Mode's "Personal Jesus" ×5 — which is the mechanism working.

**`[GDE-WRK-090]` Cross-artist collision, measured: 88 of 495 classes — 17.8% —
span more than one artist**, and they account for 258 of the 1,494 directed
rows. The probe could not see this at all, being artist-scoped by construction.

These are exactly the case `[GDE-WRK-050]` warned turns into a four-day block:

| Work | recordings held |
| :--- | :--- |
| Blowin' in the Wind | Bob Dylan · Peter, Paul & Mary |
| Across the Universe | The Beatles · David Bowie · Rufus Wainwright |
| Let It Be | The Beatles · Nick Cave · Aretha Franklin |
| I Got You Babe | Sonny & Cher · Chrissie Hynde |

Relating those means hearing Aretha Franklin's "Let It Be" silences The
Beatles' for 4.2 days, with no dial to soften it `[GDE-WRK-045]`. **Same-artist
only**, and `[GDE-WRK-095]` says at what granularity.

**`[GDE-WRK-095]` Exclude cross-artist *pairs*, not the classes that contain
them.** A mixed class often holds a genuine same-artist pair alongside the
cover, and dropping the whole class discards it:

| policy | directed rows | recordings |
| :--- | ---: | ---: |
| drop any class with >1 artist | 1,234 | 878 |
| **keep same-artist pairs wherever they occur** | **1,260** | **904** |

26 rows over 26 recordings, and not marginal ones — Elton John's two "Goodbye
Yellow Brick Road", Led Zeppelin's studio and 1969 Paris live "You Shook Me",
Bob Marley's two "Waiting in Vain". Same safety either way: no cross-artist
pair is written.

---

## 5. The cheap option, which the measurement promotes

**`[GDE-WRK-100]` Artist+title needs no network and can land today.** It is a
local query over `recordings` and `recording_artists`, it closes the incident's
own case, and at 0.5% measured error `[GDE-WRK-075]` it is not the blunt
instrument it looked like. What it cannot do is see variants under different
titles, which is half the population `[GDE-WRK-080]`.

**`[GDE-WRK-105]` The two compose rather than compete**, because
`recording_relations.source` already distinguishes provenance. Write
`title:artist-exact` rows now and `work:musicbrainz` rows once stage 1 lands,
dropping the former where the latter supersedes it. Neither waits for the
other.

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

**`[GDE-WRK-120]` Cost, now measured rather than budgeted: nine hours.** The
crawl ran 2026-09-11 20:29 → 2026-09-12 05:36 for 7,448 outstanding recordings
— about 4.4 s each, well off the 1 req/s the published limit allows, because
MusicBrainz throttles. Budget a night, not an afternoon. Resumable and cached,
same shape as `fetch_releases.py`; committed per row, so an interrupt costs one
request.

**`[GDE-WRK-125]` Stages.**

| stage | state | why here |
| :--- | :--- | :--- |
| 1 · `works`/`recording_works` schema | open | nothing is testable without it |
| 2 · `tools/fetch_works.py`, cache only | **built 2026-09-11** | read-only network, cannot damage anything |
| 3 · the crawl | **done 2026-09-12** `[GDE-WRK-120]` | 8,004 of 8,008, cached in `data/work_relations.db` |
| 4 · measure cross-artist collisions | **done** `[GDE-WRK-090]` | a decision against a number, not a guess |
| 5 · artist+title fill, `source='title:artist-exact'` | open | independent of 1–4; fixes the live case on its own |
| 6 · derive `work:musicbrainz` rows, same-artist pairs | open, needs 1 | `[GDE-WRK-095]`, and a rule for `[GDE-WRK-070]`'s 68 |

**`[GDE-WRK-130]` Transport already works, by accident.** `vainopi` carries the
whole 1.17 GB `library.db`, caches included, so relations ship with the file as
things stand. [`tools/payload.py`](../tools/payload.py) does **not** carry
`recording_relations`, so a move to bundle-only sync would need that gap closed
first.

---

## 7. Open

- **`[GDE-WRK-200]`** *Resolved 2026-09-12* — cross-artist collisions are
  measured at 88 classes `[GDE-WRK-090]`, and the rule is same-artist **pairs**
  `[GDE-WRK-095]`. What remains open is whether a listener ever wants the
  cover related; `[GDE-WRK-045]` means that can only ever be all or nothing.
- **`[GDE-WRK-210]`** The 11.0% of recordings with no Work `[GDE-WRK-065]` have
  no path to relation except artist+title or a hand edit.
- **`[GDE-WRK-220]`** Whether a `live` or `partial` performance should share a
  full rotation with the studio take is a listening judgement, not a data
  question, and `[GDE-WRK-045]` allows only yes or no. 171 `live` relations in
  the probe's 559 say this is not a rare case.
- **`[GDE-WRK-230]`** The 68 multi-Work recordings `[GDE-WRK-070]` need a
  deliberate rule before any derivation runs; none is chosen here.

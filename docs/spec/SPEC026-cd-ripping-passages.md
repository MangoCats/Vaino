# SPEC026: CD Ripping — Passage Representation for Hidden Audio and Multi-Disc Sets

**Design Specification — Tier 2 · Designed, not yet built**

Two cases where a rip is not simply one passage per TOC track: real audio
hidden in a pregap or at `INDEX 00`, and a box set ripped disc-by-disc.
Both build directly on [SPEC025](SPEC025-cd-ripping.md)'s TOC read (§3)
and Disc ID lookup (§6) — split into its own document once the two
together pushed SPEC025 past `[GOV-DOC-010]`'s 300-line hard limit, not
because the subject is unrelated. Read SPEC025 first; this document only
covers what happens to the audio once §3/§6 have already run.

> **Status.** Requirements and specification only, per `[REQ-LIB-255..260]`.
> No code exists yet — same posture as [SPEC025](SPEC025-cd-ripping.md),
> which this depends on entirely; neither can be built independently of
> the other.

> **Related:** [SPEC025](SPEC025-cd-ripping.md) for the ripping mechanics
> this extends · [SPEC023](SPEC023-domain-vocabulary.md) for what a
> passage is and is not · [SPEC008 §3](SPEC008-database-schema.md#3-passages--the-albumradio-duality)
> for the passages schema, §3b for `release_recordings.disc`

---

## 1. Hidden and pregap-only audio — its own passage, optionally folded in too

**`[SPEC-RIP-090]` Real audio inside a `PREGAP` or at `INDEX 00` gets its
own passage, never a silent drop.** `[ENT-PASSAGE-010]` already defines a
passage as a span of one file with its own playback metadata, and
`[ENT-FILE-010]` already allows a file to hold many passages — a DAO
capture is the case the vocabulary was written for. A naive split at
`INDEX 01` boundaries alone would discard hidden audio as if it were the
silence a pregap ordinarily is; instead, when SPEC025 §3's read finds real
audio in that span (not silence), it becomes its own passage with
`boundary_src` set the same as its neighbours (`[SPEC-RIP-070]`),
addressable, playable, and reviewable exactly like any other.

**`[SPEC-RIP-092]` A second, folded-in passage may additionally span the
hidden audio together with the adjacent track, when that pairing is what
a listener would actually want played.** Nothing in the schema forces one
passage per span — two passages already may cover overlapping or adjacent
ranges of the same file with no conflict, so "the hidden intro alone" and
"the hidden intro plus the track it leads into, as one continuous
listen" coexist as two separate rows rather than competing
representations. The first is what makes the hidden audio findable and
playable on its own; the second is what makes it play the way the disc
evidently intended when skipping straight to the visible track would clip
a deliberate segue. Both, either, or neither may exist for a given hidden
span — nothing requires the second passage to be created.

---

## 2. Multi-disc sets — one file per disc, one Release, passage-per-track by default

**`[SPEC-RIP-094]` A box set ripped disc-by-disc is one file per disc, not
one file per track and not one concatenated file for the whole set.** Each
disc rips through SPEC025's own pipeline (§§2-4) independently; a
five-disc box is five `files` rows, each a DAO capture in exactly the
sense `[ENT-FILE-010]` already describes.

**`[SPEC-RIP-096]` All of a set's discs resolve to one MusicBrainz
Release — a multi-medium release, not N unrelated ones.** Each disc's own
Disc ID lookup (SPEC025 §6) is still performed per disc, but where it
resolves, every disc lands on the same `releases` row;
`release_recordings.disc` `[SPEC-SC-048]` — already present in the schema
for exactly this — carries which medium each track belongs to. No new
column or table is needed: a box set is the ordinary multi-disc shape
`release_recordings` already supports, not a special case.

**`[SPEC-RIP-098]` Default segmentation is unchanged: one passage per
track, per disc.** Each disc-file is split by its own TOC exactly as a
single-disc rip is (SPEC025 §3) — a box set changes how many files a rip
produces and how they resolve on MusicBrainz, not how a single disc's own
tracks become passages. Additional passages spanning a whole disc, or a
conventional "side" grouping some box sets still carry (a vinyl-style A/B
convention on an otherwise plain CD), may also be created — the same
wider-span-alongside-the-per-track-ones pattern §1 already established for
hidden audio folded into a neighbour, applied at a larger granularity
rather than a new mechanism.

---

## 3. Open

**`[SPEC-RIP-099]`** Genuinely undecided, left here rather than guessed at:

- **When the folded-in passage (§1) is warranted.** §1 decides the
  *representation*; which discs earn the second passage automatically (a
  short hidden intro that clearly leads into the next track) versus which
  don't (a long, self-contained hidden bonus track) is not yet designed —
  a threshold, a heuristic, or a plain user prompt at rip time are all
  still on the table.
- **Which grouping — whole-disc, side, both, or neither — a box set's
  optional wider passage (§2) defaults to**, and whether it is offered
  automatically or only on request; undesigned for the same reason as the
  item above.

`[SPEC025 §8](SPEC025-cd-ripping.md#8-open)` covers what remains open in
the ripping mechanics this document does not touch — multi-disc
rip-session UX, CD-TEXT vs. MusicBrainz precedence, and drive/hardware
failure modes.

---

**Traceability:** `[SPEC-RIP-090..099]` · derives `[REQ-LIB-255..260]` ·
extends [SPEC025](SPEC025-cd-ripping.md), `[SPEC008]` §3's
`release_recordings.disc` and `passages` schema, and
[SPEC023](SPEC023-domain-vocabulary.md)'s Passage/File definitions.

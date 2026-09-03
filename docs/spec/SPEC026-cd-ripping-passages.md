# SPEC026: CD Ripping — Passage Representation for Hidden Audio and Multi-Disc Sets

**Design Specification — Tier 2 · Designed, not yet built**

Two cases where a rip is not simply one passage per TOC track: real audio
hidden in a pregap or at `INDEX 00`, and a box set ripped disc-by-disc.
Both build directly on [SPEC025](SPEC025-cd-ripping.md)'s TOC read (§3)
and Disc ID lookup (§6) — split into its own document once the two
together pushed SPEC025 past `[GOV-DOC-010]`'s 300-line hard limit, not
because the subject is unrelated. Read SPEC025 first; this document only
covers what happens to the audio once §3/§6 have already run.

> **Status.** Requirements and specification only, per `[REQ-LIB-255..265]`.
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

**`[SPEC-RIP-093]` Creating the folded-in passage is the user's decision,
never an automatic heuristic.** No threshold reliably tells a short
hidden intro that leads into the next track apart from a long,
self-contained hidden bonus track — rather than guess, Sampo detects the
hidden span (`[SPEC-RIP-090]`) and surfaces it as a one-action offer at
review time ("also create a passage combining this with the next track?"),
the same discover-then-let-a-person-confirm shape `[SPEC-SUI-215]`'s
release-suggestion flow already takes: the tool finds the candidate and
writes nothing until the operator picks it. The console makes the choice
easy; it does not make the choice.

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

**`[SPEC-RIP-100]` Which wider passage to create, if any, is the user's
call, made easy rather than made automatically.** Whole-disc and per-side
groupings are offered as one-action choices once a disc's tracks are
segmented (§2) — not inferred from disc length, track count, or any other
heuristic, for the same reason `[SPEC-RIP-093]` keeps the folded-in-passage
decision with the person ripping rather than a threshold: nothing about
the audio itself says whether a listener wants a whole disc played as one
continuous passage. Zero, one, or several wider passages may exist per
disc; declining every offer leaves the ordinary per-track passages
exactly as segmented.

---

## 3. Open

**`[SPEC-RIP-102]`** Both wider-passage triggers (§1, §2) are now decided
— user's choice, offered rather than inferred (`[SPEC-RIP-093]`,
`[SPEC-RIP-100]`) — so nothing about *when* a wider passage gets created
remains open here. What the offer's own interaction looks like (a
checkbox at review time, a prompt during the rip itself, something else)
is first-build UI detail, not a design question this document exists to
settle.

`[SPEC025 §8](SPEC025-cd-ripping.md#8-open)` covers what remains open in
the ripping mechanics this document does not touch — multi-disc
rip-session UX, CD-TEXT vs. MusicBrainz precedence, and drive/hardware
failure modes.

---

**Traceability:** `[SPEC-RIP-090..102]` · derives `[REQ-LIB-255..265]` ·
extends [SPEC025](SPEC025-cd-ripping.md), `[SPEC008]` §3's
`release_recordings.disc` and `passages` schema, and
[SPEC023](SPEC023-domain-vocabulary.md)'s Passage/File definitions.

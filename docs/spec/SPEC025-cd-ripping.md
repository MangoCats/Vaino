# SPEC025: CD Ripping via a Disc's Own Table of Contents

**Design Specification — Tier 2 · Designed, not yet built**

A new ingest entry point for Sampo: rip a physical CD directly, rather than
starting from files already on disk. Where `[SPEC024](SPEC024-dao-segmentation-cascade.md)`
infers track boundaries from audio content because the original disc's own
data is long gone, this document is for the case where it isn't gone yet —
the disc is in the drive, and its own table of contents (TOC) states
boundaries to the sector rather than asking anything to be inferred.

> **Status.** Requirements and specification only, per `[REQ-LIB-220..245]`.
> No code exists yet. Flagged in the spec itself rather than left to the
> reader to assume: the ripping-tool choice (§2) is a real open question,
> not a settled implementation detail, because it was not tested against
> this project's own desktop platform before this document was written
> `[GOV-SRC-020]`.

> **Related:** [SPEC024](SPEC024-dao-segmentation-cascade.md) for the
> audio-content cascade this supersedes for a fresh rip, and remains the
> fallback for everything else · [SPEC007 §2](SPEC007-sampo-architecture.md#2-pipeline)
> for where this joins Sampo's pipeline · [SPEC008](SPEC008-database-schema.md)
> §3 for `boundary_src`'s existing `imported:` convention this reuses ·
> [ROADMAP §3](../ROADMAP.md#3-rearchitecture--whats-still-ahead)

---

## 1. Why a disc's own TOC beats inference

**`[SPEC-RIP-010]`** A disc-at-once rip's own TOC — read from the disc's
Q-subchannel, not guessed from the encoded audio afterward — states every
track's start, length, pregap and index points to the sector (1/75 s). That
is ground truth, not the best available estimate `[REQ-LIB-220]`: nothing
in `[SPEC024](SPEC024-dao-segmentation-cascade.md)`'s cascade — grid search,
DP assembly, the RMS fallback, extra-track merging — is needed when the
boundaries are already known rather than inferred. The cascade remains
exactly as necessary as before for everything a TOC cannot help with: vinyl
transfers, cassette transfers, live recordings, or audio that reached this
library already ripped or downloaded with no TOC of its own.

A second, larger win rides along with the first: the TOC's own sector
offsets are also a **MusicBrainz Disc ID** — a lookup that identifies the
exact pressing from disc geometry (§6), a stronger answer than any text or
audio-based match and one that arrives for free once a TOC exists at all.

---

## 2. The ripping tool — an open question, not a settled choice

**`[SPEC-RIP-020]`** Two real candidates, and this project's own desktop
platform has not yet decided between them:

| | cdrdao | Exact Audio Copy (EAC) |
| :--- | :--- | :--- |
| Platform | Linux-first; Windows builds exist, less actively exercised there | Windows-native |
| Output | `.toc` (text) + audio | `.cue` + log, equivalent boundary data |
| Read verification | `--paranoia-mode 0-3`, built in (§5) | "Secure mode," its own settings |
| Licence | GPL-2.0-or-later | Freeware, not open source |

Sampo already invokes external tools as subprocesses and nothing else —
`ffmpeg`, `fpcalc`, the Essentia extractor `[SPEC-SA-030]`, `[SPEC-SA-060]`
— so a GPL-2.0 tool run this way raises no licensing question distinct from
what `[GDE-ARC-018]` already settled for those. The open question is
practical, not architectural: which one actually runs cleanly on this
project's own Windows desktop, unverified as of this document.

**Designed for either without committing to one**: a thin adapter
boundary — "produce a TOC/cue-equivalent structure plus the ripped audio" —
with `cdrdao` as the reference implementation this document specifies
against (its `--paranoia-mode` scale is what `[REQ-LIB-245]`/§5 names
directly) and `EAC` as a documented alternative behind the same interface,
resolved by whichever build-and-test pass actually implements this. Do not
build for both before the platform question is answered — one working path
is the deliverable, not an abstraction exercised by a single caller
`[GDE-FBD-050]`.

---

## 3. Reading the TOC

**`[SPEC-RIP-030]`** cdrdao's own TOC positions are `MM:SS:FF` — minutes,
seconds, **frames**, 75 frames per second, the CD's own timebase, not
milliseconds. Converting is one function, tested on its own rather than
folded into a larger one, the same lesson McRhythm's own tick-based timing
already cost this project's lineage once (`[GDE-MCR-*]`):

```
ms = round((minutes * 60 + seconds) * 1000 + frames * 1000 / 75)
```

**`[SPEC-RIP-035]`** A `PREGAP` is silence *before* a track's own audio
begins, conventionally 2 seconds before track 2 onward and commonly absent
before track 1; an `INDEX` marks a sub-position inside a track (a hidden
interlude at `INDEX 00`, the audible start at `INDEX 01`). Both are read
from the TOC and kept distinct from the track's own `start_ms`/`end_ms` —
collapsing a pregap into the previous track's length would silently move a
boundary the disc itself did not draw there.

---

## 4. Encoding

**`[SPEC-RIP-040]`** cdrdao reads raw PCM; nothing library-facing consumes
that directly. A rip decodes once, to a working lossless file (WAV or
FLAC), then that working file is encoded to the library's own format and
discarded `[REQ-LIB-240]` — an archival lossless copy is opt-in, never kept
by default, the same "off unless asked" posture `[REQ-VIS-205]` already
holds for cue sheets, cover art and lyrics written outside Vaino's own
storage.

**`[SPEC-RIP-045]` Default library format: MP3, matching what the library
already is.** The existing library is overwhelmingly MP3 with some OGG
Vorbis mixed in; introducing FLAC as a third format for new rips would cost
every downstream tool (extraction, fingerprinting, the appliance's own
storage-budget math, `[REQ-HW-100]`) a third case to handle for no benefit
once the archival step is past. FLAC on the appliance specifically is
worth naming as a **non-goal**: it runs roughly 2-3x an equivalent-quality
lossy encode for no audible gain on that hardware, and the appliance's SD
card is the resource-constrained end of this system, not the desktop doing
the ripping.

---

## 5. Paranoia level

**`[SPEC-RIP-050]` A user-adjustable setting, 0-3, default 2**, per
`[REQ-LIB-245]`. Matches `cdrdao read-cd --paranoia-mode` exactly rather
than inventing a parallel scale:

| Level | Behaviour | Cost |
| ---: | :--- | :--- |
| 0 | No checking — copied directly from the drive | fastest |
| 1 | Overlapped reads, avoids jitter | + |
| 2 | **Level 1, plus verification of the read audio data (default)** | ++ |
| 3 | Level 2, plus scratch detection and repair | slowest |

cdrdao's own upstream default is 3 (full paranoia). Vaino's default is
**2**, a deliberate one-step departure: a personal collection is presumed
mostly undamaged, and level 3's scratch-detection-and-repair pass costs
materially more time for discs that will rarely need it. Level 3 stays one
setting away for a disc that is visibly scratched — the point is a sensible
default, not a ceiling. Stored as a Sampo console setting (its own sidecar,
alongside `remote_config`'s existing key/value shape `[SPEC-DF-*]`), never
a `vaino.db`/player table — this is purely a Sampo-side ripping preference,
not listener state.

---

## 6. Disc ID and MusicBrainz

**`[SPEC-RIP-060]`** The TOC's track count and sector offsets, sent to
MusicBrainz's own Disc ID lookup (`GET /ws/2/discid/<disc-id>?toc=...`),
resolve the exact release when the disc is already catalogued, or a fuzzy
TOC-based match otherwise. When it resolves, that release's own track
metadata — titles, artists, positions — is used directly `[REQ-LIB-235]`,
in place of `[SPEC-SA-070]`'s ordinary per-track AcoustID fingerprint
lookup: a Disc ID match identifies the *pressing itself* from disc
geometry, a stronger answer than a fingerprint's per-track guess.

**`[SPEC-RIP-065]`** When Disc ID resolves nothing — a self-burned
compilation, an unreleased recording, a disc MusicBrainz has never seen and
whose fuzzy match also comes back empty — the ordinary per-track AcoustID
path (`identify_recording()` in `tools/segment_dao.py`) is the fallback,
unchanged. Ripping never blocks on an unresolved disc; it degrades to
exactly the path a file with no TOC already takes.

---

## 7. Provenance

**`[SPEC-RIP-070]` `boundary_src = 'imported:cdrdao-toc'`** (or the
ripping tool actually resolved in §2) — an instance of `[SPEC008]`'s own
already-stated `'imported:<x>'` convention, not a new one invented for
this. Ranked above `computed:segment-cascade@v1` and below `manual`
`[REQ-LIB-225]`: a disc's own TOC is ground truth the cascade only
approximates, and a person's own correction still outranks both, the same
"never silently recompute over a human decision" rule `[SPEC-SC-045]`
already holds everywhere else.

**`[SPEC-RIP-075]`** The Disc ID match, when one is made, writes an
`ingest_decisions` row (`stage='rip'`) the same shape
`tools/choose_release.py`'s `stage='release_match'` rows already use —
outcome the matched release, confidence a margin where a fuzzy match
returned more than one candidate, detail the candidates considered. A rip
that resolved nothing writes nothing here; there is no decision to record,
only a fallback to the ordinary identification path `[SPEC-RIP-065]`
already covers.

---

## 8. Open

**`[SPEC-RIP-080]`** Genuinely undecided, left here rather than guessed at:

- **Which ripping tool actually runs cleanly on this project's own Windows
  desktop** (§2) — cdrdao or EAC, resolved only by testing, not by this
  document.
- **Hidden and pregap-only audio.** A disc with genuine audio inside a
  pregap or at `INDEX 00` (a hidden intro, a run-out groove recording) is
  real audio a naive track-by-`INDEX 01` split would drop. Whether Vaino
  represents it as its own passage, folds it into a neighbour, or leaves it
  out by default is undecided.
- **Multi-disc sets.** A box set ripped disc-by-disc needs its discs kept
  in the right order and its later discs distinguished from a second copy
  of the first — not designed here.
- **CD-TEXT versus MusicBrainz, when both exist and disagree.** A disc
  carrying its own CD-TEXT title/artist is a second source next to
  whatever Disc ID resolves; no ranking between them has been measured or
  decided, so per `[GOV-SRC-050]` this stays an open question rather than
  an assumed answer.
- **Drive/hardware failure modes.** A read error, a drive needing a disc
  swapped mid-rip, or no optical drive present at all `[REQ-LIB-230]`
  names the discipline (report, never guess) but not the exact UI for any
  of these — first real design work for a build pass, not a paper
  exercise.

---

**Traceability:** `[SPEC-RIP-010..080]` · derives `[REQ-LIB-220..245]` ·
extends `[SPEC-SC-045]`'s provenance ladder and `[SPEC008]`'s `imported:`
convention · complements, does not replace, `[SPEC024](SPEC024-dao-segmentation-cascade.md)`

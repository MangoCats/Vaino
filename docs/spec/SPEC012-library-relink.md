# SPEC012: Library Relink

**Design Specification — binding a transported library to a target's own paths**

Applies `[SPEC-DF-030]`'s identity keys to deployment. Companion:
[SPEC006: Data Flow & Portability](SPEC006-data-flow-and-portability.md).

---

## 1. Why this exists

**`[SPEC-RLK-010]` A database arrives knowing what its music *is*, and nothing
about where this machine keeps it.** `[SPEC-DF-030]` is explicit: `path` is
machine scope and **survives nothing — never transported**. The schema says so
on the column itself. Yet a library moved between machines needs 5,705 correct
paths before a note can be played, and they have to come from somewhere.

They come from the target, by asking each file what it is.

**`[SPEC-RLK-020]` The alternative was tried, and it is not sound.** The
2026-08-17 transfer rewrote every path on the *source* — `C:\Users\…\Music\` to
`/srv/library/audio/` — and shipped the result. It worked, and it worked for
reasons that are properties of that library rather than of the method:

| Assumption | Fails when |
|---|---|
| The target's tree matches the source's, exactly | anything is reorganised, renamed or re-foldered |
| Path case is preserved end to end | Windows (case-insensitive) feeds Linux (case-sensitive) |
| Unicode normalisation matches | macOS writes NFD, Linux stores NFC — **628 of this library's paths carry non-ASCII characters** |
| The target holds the whole library | it holds a subset, and every absent row is indistinguishable from a broken one |

The pre-flight check that "verified" all 5,705 files existed used
`os.path.exists` on Windows, which is **case-insensitive** — it would have
passed paths that 404 on the appliance. A second, byte-exact check found zero
mismatches, but only because the library is well kept. Nothing in the method
produced that result, and nothing in it would have reported the failure.

---

## 2. The mechanism

**`[SPEC-RLK-030]` Hash what is on the target; match it; write the path.**

    for each file under the audio root
        md5 = MD5(encoded audio stream, container and tags excluded)
        row = SELECT file_id FROM files WHERE audio_md5 = md5
        if row: UPDATE files SET path = <this file> WHERE file_id = row

This is MuLibPlay's `scanFile` `[GDE-BMK-050]`, which hashed a **file**. Vaino
hashes the **encoded stream with tags excluded**, which is the correction
`[SPEC-DF-030]` names: `[SPEC-DF-060]` uses embedded tags as a metadata
*transport*, so a file-level hash would orphan every row the moment metadata
was written back.

**`[SPEC-RLK-040]` Match by the narrowest key that fits `[SPEC-DF-040]`, then
stop.**

1. **`audio_md5`** — this exact encoding. Certain, and the only rung that
   justifies a silent update.
2. **`recording_mbid`** — the same recording, re-encoded. A different rip of a
   known recording is a *candidate*, not a match: passage boundaries, trim
   points and replay gain are encoding-scope and do not transfer to it
   `[SPEC-DF-040]`. Reported for a person to decide, never applied.
3. **Neither** — the file is unknown to this database. Ingest, not relink
   `[SPEC-RLK-090]`.

---

## 3. What it reports

**`[SPEC-RLK-050]` Five outcomes, all of them stated.** A relink that prints
"done" has told the operator nothing about a library that may be half bound.

| Outcome | Meaning | Action |
|---|---|---|
| **matched** | hashed, and the hash agrees with the path already held | none |
| **moved** | hashed, found elsewhere; path updated | none |
| **missing** | a row whose audio is nowhere on this machine | mark absent, keep the row |
| **corrupt** | bytes present where the row expects them, hash disagrees | report loudly; never bind |
| **unknown** | audio here that no row claims | list; ingest is a separate job |

**`[SPEC-RLK-055]` `corrupt` and `unknown` must not be confused.** A truncated
copy of a known file hashes to nothing the database recognises, so the naive
reading is "unknown music" — and a failed transfer would be reported as a
library discovery. They are separable: a row is **corrupt** rather than
**missing** when a file exists at the location it names and does not hash to
it, and a file is **unknown** rather than **corrupt** when nothing claims its
location. `size_bytes` sharpens the same call, a truncation being visible
without hashing at all.

This is the distinction that earns relink its second job, and getting it wrong
would make the check worse than none: a corrupted library reported as an
enlarged one.

**`[SPEC-RLK-060]` Missing is a state, not an error.** A Pi holding a subset of
a 44 GB library is a normal deployment, not a broken one. Those passages are
excluded from selection and say why, exactly as an unidentified passage still
plays but cannot contribute to rotation. What must never happen is a row
silently retaining a path that resolves to nothing, which is the failure
`[SPEC-RLK-020]` produces at scale.

---

## 4. Cost, measured

**`[SPEC-RLK-070]`** The schema records **~70 ms per file** for
`ffmpeg -i F -vn -c:a copy -f md5 -`, verified bit-identical to the extractor's
own value. No decode: the encoded packets are hashed as they are read, so the
work is I/O, not DSP.

Measured end to end on the development machine: **5,742 files hashed in 423 s**
— 74 ms each, matching the schema's figure almost exactly. On the appliance,
where the constraint is SD card read rather than CPU, a full relink should be
**under an hour**. Against the 11 hours the same library takes to arrive over
the Pi Zero 2 W's Wi-Fi, the relink is not a cost worth optimising.

The first end-to-end run against the real library, with the database carrying
target paths and the audio at its source location — the deployment case
exactly:

    5,705 rows, 5,743 audio files under the root
    moved     5,705      every row bound
    missing       0
    corrupt       0
    unknown      28      never indexed, including a scratch directory
    duplicate     9      literal copies, `X.mp3` beside `X_2.mp3`

**`[SPEC-RLK-080]` Hash with ffmpeg — because ffmpeg wrote the values we
hold, not because it is more correct.** *(Revised 2026-08-17, then corrected
the same day.)*

The first version of this entry recommended Symphonia; the second rejected it
on a 1% disagreement and read as though Symphonia had lost on merit. That
framing was wrong, and the correction matters more than the conclusion.

`audio_md5` is **Essentia's `md5_encoded`**, and Essentia's audio I/O is built
on FFmpeg/libav. The 5,705 stored values are therefore an ffmpeg-family
artefact. Measuring ffmpeg against them — 68 of 68 — is close to measuring a
tool against its own output. **It is evidence of shared lineage, not of
correctness.** Had Symphonia generated the references, Symphonia would score
100% and ffmpeg would fail on the same ~60 files.

**`[SPEC-RLK-085]` The two disagree about where the stream ends.** Measured:
both begin identically, immediately after the ID3v2 tag. The disputed files
end in stray `0xFF` bytes — the start of an MPEG sync word with no frame behind
it. Symphonia stops at the last complete decodable frame; ffmpeg includes the
remainder. ID3v1 handling is not uniform within ffmpeg either: one file's valid
`TAG` trailer was included in the hash, another's was stripped.

An attempt to write the rule down — *everything after ID3v2, to EOF, less an
ID3v1 trailer if present* — reproduced **36%** of the stored hashes. That
failure is the finding: there is no simple specification of what ffmpeg does,
only accumulated heuristics.

Neither reading is more valid; they answer different questions. Symphonia's is
the cleaner **identity** — only decodable audio, so two copies differing solely
in trailing junk are correctly the same recording. ffmpeg's is the stricter
**integrity** check, noticing trailing damage Symphonia ignores by design. Since
`[SPEC-RLK-140]` made relink an integrity check, ffmpeg suits — but that is a
reason found after the fact, not the reason it was chosen.

**`[SPEC-RLK-086]` The identity key is implementation-defined, and that is a
latent risk.** Both hashers are deterministic (three runs each, identical) and
neither involves floats or endianness, so both are stable across x86_64 and
aarch64. Platform is not the hazard.

**Version is.** Neither implements a standard. Ours is ffmpeg 8.0; Essentia's
bundled libav is considerably older. That they agree today is fortunate rather
than guaranteed, and `[SPEC-DF-030]` treats `audio_md5` as a stable identity
key when it is really "whatever the extractor's demuxer did". An ffmpeg upgrade
could in principle orphan rows, and nothing would report it as anything but
missing music.

Two consequences follow. The hasher is **not** a free choice: it must be the
one that produced the incumbent values, so switching means regenerating all
5,705 hashes atomically — a mixed database is silently broken. And the
generator's identity is worth recording alongside the values, so a future
disagreement can be diagnosed instead of discovered.

---

## 5. Boundaries and risks

**`[SPEC-RLK-090]` Relink is not ingest.** It binds rows that already exist. It
never creates one. MuLibPlay could relocate a known file and could not induct a
new one, and that gap is precisely why its new-music process was "undocumented,
unrepeatable, and easily forgotten" `[GDE-BMK-050]`. Vaino inherits the
relocation and must not inherit the boundary as a limit: `unknown` files are
reported so that ingest has somewhere to start.

**`[SPEC-RLK-100]` It touches one column.** `path`, and nothing else. Not
passages, not flavor, not history. A relink that can only rewrite where a file
lives cannot corrupt what is known about it, which is what makes it safe to run
unattended and repeatedly.

**`[SPEC-RLK-110]` Idempotent, and interruptible.** Running it twice changes
nothing the second time. Interrupted half way, the rows it reached are correct
and the rest are as they were — there is no state in which the library is
inconsistent with itself.

**`[SPEC-RLK-120]` The uniqueness constraint is load-bearing.** `audio_md5` is
`NOT NULL UNIQUE`, so two files with one hash are a duplicate on disk rather
than an ambiguity in the match. The relink reports the pair and binds the row to
one of them; it does not silently prefer whichever it walked into first.

---

## 6. Settled

**`[SPEC-RLK-130]` The shipped database keeps its paths when it is a plain
copy, and loses them when it is not.** *(Decided 2026-08-17.)*

Blanking paths makes `[SPEC-DF-030]`'s "never transported" enforceable rather
than advisory, which is the stronger position — but not at the price of
inventing a transformation step to achieve it. Where the database ships as a
straight file copy, the stale paths are harmless: relink overwrites every one
of them, and a wrong path that is about to be rewritten costs nothing to carry.

Where a database is **already** being customised for other reasons — pruning
caches, subsetting a library, stripping anything not wanted on an appliance —
the paths come out during that pass. The work is already being done; leaving
machine-scope data in a database that is being deliberately shaped for another
machine would be an omission rather than an efficiency.

The practical consequence is that the source-side path rewrite used on
2026-08-17 `[SPEC-RLK-020]` is **not** needed once relink exists. It was the
whole justification for touching the database at all, and it can go.

**`[SPEC-RLK-140]` Relink is also the integrity check.** *(Decided
2026-08-17.)* A hash that matches proves the bytes arrived intact. The walk pays
for that hash already; only the reporting was missing, and a transfer otherwise
has nothing checking it beyond rsync's own framing.

**This forbids the obvious optimisation.** A relink that skipped files whose
path already looked right would be faster and would verify nothing — `matched`
would mean "the path resolves", which is an assumption wearing the costume of a
result. Every file is hashed, every run.

A `--quick` mode that skips bound files may still be wanted for a large library
being relinked repeatedly. If it exists it must say, in its own output, that it
verified nothing: the failure this whole document exists to prevent is a check
that reports success without observing the thing it claims to check.

---

## 7. Decided, and deferred

**`[SPEC-RLK-150]` At the next re-extraction, Symphonia becomes the hash
authority and the ffmpeg dependency is retired.** *(Decided 2026-08-17. Not to
be done tonight, or on its own.)*

Not because ffmpeg is wrong — `[SPEC-RLK-080]` and `[SPEC-RLK-085]` establish
that it is not, and that the two readers merely disagree about trailing bytes
no listener will ever hear. The reason is **ownership of the implementation**.

`audio_md5` is an identity key that other tables are keyed on, and today its
meaning is defined by an external package that a routine `apt upgrade` may
change underneath the appliance `[SPEC-RLK-086]`. Symphonia is compiled into
the player: the definition would then ship with the binary, versioned with it,
and could not drift without a deliberate build. That is the difference between
an identity key the project *has* and one it *borrows*, and it is worth more
than either reading's claim to be the more correct description of an MP3.

Retiring ffmpeg also removes the appliance's only dependency on it — installed
solely for this hash, on a machine with no other use for a media framework.

**Why it waits for a re-extraction.** `audio_md5` is a key in four tables:
`files`, `lowlevel_cache`, `identification_cache` and `ingest_decisions` —
about 45,000 rows on the present library. Changing the hash means rewriting all
of them together, keyed by the value that is changing, and a half-applied
migration silently orphans every cache while looking merely cold. A
re-extraction regenerates those values anyway, so the migration cost collapses
to zero at exactly that moment and at no other.

**Three preconditions, none optional.**

1. **Close or accept Symphonia's coverage gap.** It failed to read 1 file of
   5,743 that ffmpeg read — a `.mp3` it probes as an unsupported wave format.
   0.017%, but under Symphonia's authority that file would have no identity at
   all, and it currently plays. Fix upstream, re-encode it, or record the
   exclusion deliberately.
2. **One implementation, not two `[GDE-FBD-040]`.** The hash is currently free:
   Essentia emits `md5_encoded` as a by-product of extraction `[SPEC-SA-035]`,
   and ingest is Python. Moving the authority to a Rust crate means deciding
   where ingest gets its hash — not leaving Python and the player each computing
   an identity key their own way.
3. **Record the generator alongside the values.** Whatever tool holds the
   authority, the database should say which, and which version. A future
   disagreement is then diagnosable rather than discovered as a library that has
   mysteriously gone missing.

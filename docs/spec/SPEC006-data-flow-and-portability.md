# SPEC006: Data Flow & Library Portability

**Design Specification — Tier 2**

How data originates, where it lives, and how Sampo's derived knowledge reaches a Vaino installation that has no Sampo. Covers the identity scheme that makes derived data re-bindable to audio on a foreign system.

> **Related:** [GUIDE002 §2 Architecture](../GUIDE002-rearchitecture-plan.md#2-architectural-decisions) · [SPEC005: Flavor Distance](SPEC005-flavor-distance.md) · [GUIDE003: Feature Extraction](../GUIDE003-feature-extraction-strategy.md)

---

## 1. Source of Truth

**`[SPEC-DF-010]` The user's music files are the only original data.** Everything else — identification, segmentation, flavor, loudness — is derived, and derivable again from the audio alone by Sampo. `vaino.db` is a cache of expensive derivations, never a source. Deleting it must cost time, never information.

**`[SPEC-DF-020]` The bit-exact guarantee, stated precisely.** The promise is **the decoded audio stream is never altered** — not that file bytes are never touched. This distinction is what makes `[SPEC-DF-060]` metadata transport possible at all.

Measured 2026-08-09, adding a 400-byte ID3 `TXXX` frame to an MP3:

| | before | after |
| :--- | :--- | :--- |
| whole-file MD5 | `04bf496b…` | `2d8f8194…` — **changed** |
| `md5_encoded` (decoded audio) | `f1556800a10f8d1f…` | `f1556800a10f8d1f…` — **unchanged** |

Container tags live outside the audio stream. So Vaino plays byte-identical audio whether or not Sampo has annotated the file, and **the guarantee is verifiable rather than merely asserted**: extract, tag, re-extract, compare `md5_encoded`.

---

## 2. Identity — Three Keys, Three Scopes

**`[SPEC-DF-030]`** Derived data binds to audio at three different scopes. Conflating them is what makes library migration brittle.

| Key | Scope | Source | Survives |
| :--- | :--- | :--- | :--- |
| **`audio_md5`** | *this exact encoding* | Essentia `md5_encoded`, free at extraction | re-tagging, renaming, moving, copying |
| **`recording_mbid`** | *this recording, any encoding* | MusicBrainz via AcoustID | re-encoding, different rip, different bitrate |
| **`file_path`** | *this machine only* | filesystem | nothing — never transported |

**`[SPEC-DF-035]` A local sequence number is a fourth key, and it may not cross an installation.** `passage_id` is an `INTEGER PRIMARY KEY` — efficient, and correct as a foreign-key target inside one database `[SPEC-SC-040]`. It is not an *identity*: a re-derivation renumbers passages `[REQ-LIB-160]`, so the same integer means a different passage on the same machine after a rebuild, and an unrelated one on any other machine.

The rule: **a local sequence number may be used only where the potential for confusion is structurally absent** — within a single database file, in one process's queries, as a foreign key. Anything that crosses an installation, a transport, or a rebuild keys by scope instead.

For a passage the portable key is **`(audio_md5, kind, start_ms, end_ms)`** — encoding scope, because a passage is a span of one exact encoding and its boundaries are class C `[SPEC-DF-050]`. This names a shape the schema already has rather than adding one: `passages_span` is exactly that uniqueness constraint, and `lowlevel_cache` is already keyed this way `[SPEC-SC-080]`.

What the rule prevents is silent by construction. A stale or foreign `passage_id` does not fail — it resolves to a real passage, which plays a real song under a real name, and nothing downstream can tell `[REQ-LIB-165]`.

**`[SPEC-DF-040]` Match by the narrowest key that fits the data.**

- **Recording-scope data** — flavor vector, MBIDs, artist/album/title, work relations. A property of *the music*, so it binds by `recording_mbid` and is valid for anyone holding that recording in any encoding.
- **Encoding-scope data** — DAO passage boundaries, Album/Radio trim points, segue frames, replay gain, decoded duration. A property of *this rip*, meaningless against a different encode, so it binds by `audio_md5`.
- **Machine-scope data** — `file_path`, mtime, size. Never leaves the installation.

`audio_md5` is the workhorse: it is exactly MuLibPlay's `files.sig` idea `[GDE-BMK-050]`, corrected. MuLibPlay could relocate a known file; hashing decoded audio additionally survives the tag writes that `[SPEC-DF-060]` depends on.

---

## 3. What Travels, and What Must Not

**`[SPEC-DF-050]`** Four data classes, with sharply different portability rules:

| Class | Examples | Travels? |
| :--- | :--- | :--- |
| **A · Derived facts** | flavor vector, `audio_md5`, decoded duration, replay gain, chromaprint | **Yes.** Deterministic from audio; expensive; identical for everyone. |
| **B · Identification** | recording/release MBIDs, artist, album, title | **Yes.** Costs network lookups and human correction. |
| **C · Segmentation** | DAO passage boundaries, Album/Radio duality, segue points | **Yes — the highest-value payload.** This is the manual labour that made induction painful `[GDE-BMK-050]`. Sharing it is the single biggest convenience win. |
| **D · Listener state** | rotation/recovery/restraint, likes/dislikes, play history, programs | **Never.** |

**`[SPEC-DF-055]` Class D never travels with music.** It describes *the listener*, not the music: personal, private, and meaningless to anyone else. A shared file carrying someone's play history would be both a privacy leak and noise. Listener state moves only by deliberate whole-`vaino.db` migration `[SPEC-DF-080]` or the class-D export `[SPEC-DF-090]`, both between a user's own machines.

**The rule:** *the transport carries facts about the music, never facts about the listener.*

---

## 4. Three Transports

**`[SPEC-DF-060]`** In increasing order of invasiveness. All three carry the same payload schema; they differ only in where it is written.

### T1 · Embedded tags — travels inside the file

Written into the container's native tag block, so the data survives any copy, move, or rename, with nothing to keep alongside it. This is the mechanism that makes a "straight, playable audio file" self-describing.

| Container | Mechanism |
| :--- | :--- |
| MP3 | ID3v2 `TXXX` frames |
| FLAC / Ogg | Vorbis comments |
| MP4 / M4A | `----:com.apple.iTunes:` freeform atoms |

Follow MusicBrainz Picard's existing conventions where they exist (`MUSICBRAINZ_TRACKID`, `MUSICBRAINZ_ALBUMID`, `ACOUSTID_ID`) so Vaino interoperates with the wider tooling ecosystem rather than inventing a private dialect. Vaino-specific payloads take a `VAINO_*` namespace.

Size is not a constraint: a JSON flavor payload is ~1–2 KB `[SPEC-DF-093]`, and a 40-passage DAO segmentation roughly 3 KB.

**Writes to the user's files, so it requires informed consent once, then proceeds by default** `[SPEC-DF-092]`. Tag writers can corrupt containers; every write is temp-file → verify `md5_encoded` → atomic replace, so the original is never mutated in place.

### T2 · Sidecar fragments — travels beside the file

One `.vaino.json` per audio file `[SPEC-DF-091]`. Zero risk to audio, no consent needed, works on read-only media and containers Vaino cannot tag. Lost if the audio is copied without it — acceptable, because this is the fallback for when T1 is unavailable rather than the primary route.

### T3 · Database migration — whole library at once

Ship `vaino.db` itself and rebind by `audio_md5` `[SPEC-DF-040]`. This is MuLibPlay's proven route, and the only one that can carry class D. For moving a library between a user's *own* machines.

**`[SPEC-DF-065]` T1 and T2 are the same payload in different envelopes.** One serializer, one parser, one schema version. Sampo emits; Vaino imports.

---

## 5. Trust and Conflict

**`[SPEC-DF-070]` Imported metadata is a claim, not a fact.** It may be stale, wrong, produced by an older Sampo, or hostile.

1. **Verify before trusting encoding-scope data.** Recompute `audio_md5`; if it disagrees with the embedded value, discard all class C data — it describes a different encode. Cheap and decisive.
2. **Class A/B may be accepted on `recording_mbid` alone**, since it is encoding-independent by construction.
3. **Provenance is mandatory** `[GDE-FBD-020]`. Every imported value records its origin — `sampo@<version>`, `acousticbrainz-dump-20220623`, `imported:<source>`, `manual`.
4. **Conflict resolution by provenance rank, then recency.** `manual` outranks everything; a newer Sampo model version outranks an older one; local outranks imported at equal rank. Never silently overwrite a user's correction.
5. **Never execute or interpolate imported strings.** Text fields are display data.

---

## 6. Deployment Topologies

**`[SPEC-DF-080]`**

| Topology | Flow |
| :--- | :--- |
| **Co-resident** (desktop) | Sampo scans → writes `vaino.db` → Vaino reads. No transport needed. |
| **Appliance** (Pi + desktop Sampo) | Sampo builds on desktop → T3 migration → Vaino rebinds by `audio_md5`. The Pi never runs Sampo `[GDE-ARC-010]`. |
| **Sharing** (foreign install) | Audio arrives with T1/T2 payload → Vaino imports class A/B/C after verification → advanced features work with **no Sampo present at all**. |
| **Recovery** | `vaino.db` lost. If T1 was used, re-import from the files themselves; otherwise re-derive with Sampo. Class D is lost unless separately backed up — the one asymmetry worth warning users about. |

The sharing row is the point of the whole design: it lets a Vaino-only installation benefit from segmentation and flavor work it could never perform itself, because Sampo is x86-only, AGPL, and heavyweight `[GDE-ARC-015]`.

---

## 7. Resolved Decisions

Settled 2026-08-09. Each replaces an open question; the reasoning is kept so a future reader can tell a decision from an accident.

**`[SPEC-DF-090]` Listener state is exported automatically, class D only.**
A small, private file on a schedule — never bundled with music `[SPEC-DF-055]`. Rationale: class D is the *only* irreplaceable data in the system, since everything else re-derives from audio `[SPEC-DF-010]`. It is also small: MuLibPlay's six years amount to 37,134 play events plus ~2,900 preference rows. A full `vaino.db` snapshot was rejected as mostly re-derivable cache (164 MB, largely cover art), and manual export as depending on remembering — which is precisely why six years of MuLibPlay history sits unprotected today.

The same export is the **sync unit between a user's own machines**, so the backup path is exercised in normal use rather than only in disaster.

**`[SPEC-DF-091]` Sidecars are per-file `.vaino.json`.**
Sidecars are the fallback for when embedding is impossible — read-only media, unsupported containers, or a user who declines tag writes. On an exception path robustness outranks tidiness, and a per-file sidecar survives single-file copies that a directory manifest would not. Directory clutter is accepted as the cost.

**`[SPEC-DF-092]` Embedded tags: informed consent once at first import, then default on.**
Not permanently opt-in. A default-off path receives little real-world exercise and rots, while self-describing files are the point of the design `[SPEC-DF-060]`. Not default-on-silent either: modifying a user's music library without asking is not justifiable even with verification.

Every write is **temp-file → verify → atomic replace**:
1. Copy to a temp file, write tags there.
2. Recompute `md5_encoded` on the temp file.
3. Replace the original **only** if it matches the pre-write value `[SPEC-DF-020]`.
4. On mismatch, discard the temp file, log, and mark the track tag-ineligible.

The original is never mutated in place, so a failed write cannot damage the library.

**`[SPEC-DF-093]` Payload is human-readable JSON.**
At ~1–2 KB per track the compact alternative saves ~1.5 KB and costs inspectability, exact float precision, and debuggability. A packed fp16 form would be opaque to every tool including our own. This is a system whose headline requirement is that its processes be visible `[GDE-CHT-030]`; an unreadable payload embedded in the user's own files contradicts that for no measurable gain. One format, one parser `[SPEC-DF-065]`.

---

## 8. Export & Consent Policy

Settled 2026-08-09, completing the decisions in §7.

**`[SPEC-DF-094]` Class-D export: daily, generational, integrity-checked before rotation.**

1. **Verify first.** Confirm `vaino.db` is readable and self-consistent (`PRAGMA integrity_check`, plus row-count and referential sanity on the class-D tables) **before** writing a new generation. A failed check aborts the rotation and raises an alert — it never overwrites a good backup with a bad one.
2. **Retain generations:** 30 daily + 12 monthly.
3. **Cost is negligible.** Class D is a few MB — MuLibPlay's six years is 37,134 play events, `vaino.db` holds 74,299 — so 42 generations stay well under 200 MB.

Rationale: storage is not the constraint here; **undetected corruption is**. A single rotating snapshot faithfully copies a corrupted database over the last good copy, and the loss is discovered only when someone looks. Depth plus a pre-rotation integrity check is what makes the backup trustworthy rather than merely present.

Rejected: *on clean shutdown* (a 24/7 appliance may never shut down cleanly — or only ever lose power, exporting nothing); *weekly* (shallower window to notice corruption before good generations age out). The **append-only journal** alternative is strictly more robust and remains the upgrade path if snapshots prove insufficient — it was declined only for the second write path it would add `[GDE-CHT-040]`.

**`[SPEC-DF-095]` Headless Sampo falls back to sidecar-only, with an explicit `--embed` override.**

Consent applies solely where Sampo runs. Vaino only ever *reads* tags, and the appliance never runs Sampo `[GDE-ARC-010]`, so the Pi never faces this question.

For a non-interactive Sampo run (scripted, batch, NAS) with no recorded consent:
1. Write `.vaino.json` sidecars `[SPEC-DF-091]` instead of touching audio.
2. Log the downgrade explicitly — silent degradation is its own failure `[GDE-CHT-030]`.
3. `--embed` grants consent for scripted use where the operator has decided.

The run still produces fully portable data `[SPEC-DF-065]`; only the envelope changes. Failing safe beats both aborting (turning a first scripted run into a diagnosis exercise) and assuming consent (modifying a user's music library on an inference).

---

**Traceability:** `[SPEC-DF-010..093]` · derived from `[GDE-BMK-050]`, `[GDE-ARC-010]`, `[GDE-CHT-045]`, `[GDE-FBD-020]`

# GOV001: Document Hygiene & Governance Standard

**Governance Specification — Tier 0**

This document establishes the official document hygiene standards, naming conventions, unique identifier taxonomies, and modularity principles for the **Vaino** project repository.

---

## 1. Document Modularity & Context Window Efficiency Rules

To ensure that both human contributors and AI coding assistants can quickly inspect specific specifications without consuming excessive context window capacity or wading through unrelated content, all project documentation MUST follow these core principles:

1. **`[GOV-DOC-010]` Focused Single-Purpose Documents**:
   - Every document MUST focus on a single domain or component.
   - Target file length is **100 to 250 lines** per document, and staying under 250 is **strongly encouraged**: it is the length at which a reader, or an agent loading the file into context, can hold the whole of it at once.
   - **The hard limit is 300 lines.** Above 300 a document MUST be split into sub-documents within appropriate folders (e.g., `docs/spec/`). Between 250 and 300 it is over the target and not in breach — a note, not a defect.
   - *Revised 2026-08-20, from a flat 250.* The 250-line target was doing two jobs and one of them badly: it named a good length **and** implied that exceeding it obliged a split. That put a spec's later, measured additions in competition with its earlier reasoning, and made "trim to fit" a reason to cut argument that had been written deliberately. A band separates the encouragement from the obligation, so the choice to keep something is no longer a choice to cut something else.

2. **`[GOV-DOC-030]` Inherited documents are segregated and prefixed**:
   - Material copied from predecessor projects lives only under `docs/inherited/`, carries an `MCR-` (or equivalent) filename prefix, and opens with a banner stating its class.
   - This is required because McRhythm and Vaino both number `SPEC003`–`SPEC006` with different meanings. See [inherited/README.md](inherited/README.md) `[INH-HAZ-010]`.
   - **`[GOV-DOC-040]` Enforced by `tools/check_docs.py`**, which also fails on new identifier collisions, dangling tags and unresolvable links. Run it before committing documentation changes.
   - **Searches over `REQ`/`ENT` must be scoped**: `grep -rn "REQ-AUD" docs/ --exclude-dir=inherited`. Inherited material defines 651 tags of its own `[INH-HAZ-020]`.

3. **Unique Grep-Searchable Identifiers**:
   - All requirements, design specs, entity definitions, and test cases MUST be assigned a unique, bracketed identifier tag (e.g., `[REQ-AUD-010]`, `[SPEC-AUD-020]`, `[UT-AUD-001]`).
   - Tags MUST be consistent across specifications, source code comments, and automated test names.

4. **Direct Markdown Hyperlinks**:
   - All references to other documents MUST use standard GitHub Markdown file links with explicit relative paths (e.g., `[SPEC001: Audio Engine](SPEC001-audio-engine.md)`).

5. **`[GOV-DOC-020]` Synchronous Specification & Test Maintenance Rule**:
   - Whenever an interactive conversation or prompt results in new code creation, architectural refinement, or settled design decisions, the corresponding formal requirements/specifications (`docs/spec/`) and automated test suites (`tests/`) MUST be updated synchronously within the same conversation turn.

---

## 2. Identifier Taxonomy Standard

All unique identifiers MUST use one of the following standardized prefixes:

| Category | Tag Format | Description | Example |
| :--- | :--- | :--- | :--- |
| **Requirements** | `[REQ-<DOMAIN>-<NUM>]` | System & functional requirements | `[REQ-AUD-010]`, `[REQ-DB-020]` |
| **Design Specifications** | `[SPEC-<DOMAIN>-<NUM>]` | Component & mathematical specifications | `[SPEC-AUD-040]`, `[SPEC-PD-010]` |
| **Entities & Data Models** | `[ENT-<NAME>-<NUM>]` | Core relational data entities & schemas | `[ENT-TRACK-010]`, `[ENT-PASSAGE-010]` |
| **Unit & Integration Tests**| `[UT-<DOMAIN>-<NUM>]` | Automated test suite assertions | `[UT-AUD-001]`, `[UT-DB-001]` |
| **Governance & Process** | `[GOV-<DOMAIN>-<NUM>]` | Repository rules & policies | `[GOV-DOC-010]` |
| **Development Guidance** | `[GDE-<DOMAIN>-<NUM>]` | Lessons learned, architectural rationale, forbidden patterns | `[GDE-LES-010]`, `[GDE-ARC-020]` |
| **Implementation Guides** | `[IMPL-<DOMAIN>-<NUM>]` | Step-by-step build and deployment procedures | `[IMPL-AUD-020]`, `[IMPL-STOR-030]` |
| **Experiment Records** | `[LOG-<DOMAIN>-<NUM>]` | Dated iteration history: approach, measured result, why it plateaued | `[LOG-I1-020]`, `[LOG-NEXT-010]` |
| **Inherited Material** | `[INH-<DOMAIN>-<NUM>]` | Provenance and classification of documents copied from predecessor projects | `[INH-HAZ-010]` |

### Domain Acronyms
- `AUD` — Audio Engine, Decoders, Slicing, Crossfading
- `DB` — Database, SQLite, Media Scanner
- `MB` — MusicBrainz, AcoustID, Chromaprint Fingerprinting
- `FE` — Feature Extraction, Essentia, LUFS Loudness
- `FD` — Flavor Distance, Song Similarity Metric
- `SC` — Database Schema, Relational Model
- `DIR` — Program Director, Selection Pipeline
- `SA` — Sampo Architecture, Pipeline Stages
- `VIS` — Visibility, Process Transparency
- `LIB` — Library Building, Ingest (Sampo)
- `PORT` — Portability, Metadata Transport
- `DF` — Data Flow, Identity Keys, Library Portability
- `PD` — Program Director, Auto-Playlist Selection Math
- `UI` — Web Server, REST API, WebSocket Protocol, Web UI
- `HW` — Embedded Target, RPi Zero 2W, Storage Partitioning
- `APS` — Audio Path Supervisor: output device, sink, speaker lifecycle
- `RLK` — Library relink: binding a transported library to a target's paths
- `SUI` — Sampo Console: the library-builder's own web interface (distinct from `UI`, which is the player's)
- `PL` — Derived-data payload: the one format carried by all three transports
- `MPD` — MPD integration: the Program Director as a guest in someone else's player
- `SRC` — Sources of truth: ranking two answers to one question, see [GOV002](GOV002-sources-of-truth.md)
- `PLAY` — What counts as a play, for every path that writes `listener_play_history`
- `LYR` — Lyrics: where they live, and how far they travel
- `BK` — Switching playback backends without stopping, see [SPEC018](spec/SPEC018-switching-backends.md)

### Development Guidance Domains (`GDE`)
- `BMK` — MuLibPlay benchmark measurements
- `PD` — MuLibPlay selection algorithm (preserved behaviour)
- `MCR` — McRhythm/wkmp findings
- `V1` — Vaino v1 measured failures
- `LES` — Distilled lessons
- `FEX` — Feature extraction strategy (P0 critical path)
- `CHT` / `ARC` / `PHS` / `FBD` / `DIS` / `OPN` — Charter, architecture decisions, phases, forbidden patterns, disposal register, open questions
- `AND` — Phone port strategy: routes onto a phone, and what each costs
- `IOS` — Phone port strategy, iOS-specific: store terms, audio stack, toolchain
- `CLD` — Hosted flavor service: what it would be, and whether it pays for itself
- `EXT` — Driving other players with Vaino's selection, and where that stops
- `BAK` — External backends: the measured cost of an MPD/OpenSubsonic adapter
- `MPD` — The Director as an MPD client: leverage, extension via stickers, mapping

---

## 3. Master Specification Search Index

Agents and developers can use standard grep commands to instantly locate specifications:

```bash
# Example: Search for all Audio Engine requirements
grep -rn "REQ-AUD" docs/

# Example: Search for Program Director scoring specifications
grep -rn "SPEC-PD" docs/
```

| Tag ID | Component | Location Document |
| :--- | :--- | :--- |
| `[GDE-BMK-*]` | MuLibPlay benchmark & selection algorithm | [GUIDE001-lineage-and-lessons.md](GUIDE001-lineage-and-lessons.md) |
| `[GDE-LES-*]` | Distilled lessons from all predecessors | [GUIDE001-lineage-and-lessons.md](GUIDE001-lineage-and-lessons.md#6-the-lessons-distilled) |
| `[GDE-ARC-*]` | Re-architecture decisions | [GUIDE002-rearchitecture-plan.md](GUIDE002-rearchitecture-plan.md#2-architectural-decisions) |
| `[GDE-PHS-*]` | Phased implementation plan | [GUIDE002-rearchitecture-plan.md](GUIDE002-rearchitecture-plan.md#3-phased-plan) |
| `[GDE-FBD-*]` | Forbidden patterns | [GUIDE002-rearchitecture-plan.md](GUIDE002-rearchitecture-plan.md#4-forbidden-patterns) |
| `[GDE-DIS-*]` | Predecessor disposal register | [GUIDE002-rearchitecture-plan.md](GUIDE002-rearchitecture-plan.md#5-disposal-register) |
| `[GDE-FEX-*]` | Feature extraction strategy (P0 critical path) | [GUIDE003-feature-extraction-strategy.md](GUIDE003-feature-extraction-strategy.md) |
| `[GDE-AND-*]` | Phone ports (Android, iOS): fork vs ground-up, and the licence that decides it | [GUIDE004-phone-port-strategy.md](GUIDE004-phone-port-strategy.md) |
| `[GDE-CLD-*]` | Hosted flavor lookup instead of Sampo on the device | [GUIDE005-flavor-service.md](GUIDE005-flavor-service.md) |
| `[GDE-EXT-*]` | The Director driving other players; why streaming is closed | [GUIDE006-director-as-a-guest.md](GUIDE006-director-as-a-guest.md) |
| `[GDE-BAK-*]` | Measured cost of an MPD / OpenSubsonic backend, and its containment | [GUIDE007-external-backends-investigation.md](GUIDE007-external-backends-investigation.md) |
| `[SPEC-MPD-050]` | Extending MPD through its sticker database, without patching it | [SPEC015-mpd-director.md](spec/SPEC015-mpd-director.md#4-extending-mpd-without-patching-mpd) |
| `[SPEC-MPD-060]` | Mapping a Vaino passage to an MPD URI | [SPEC015-mpd-director.md](spec/SPEC015-mpd-director.md#5-the-mapping-which-is-the-hard-part) |
| `[SPEC-MPD-092]` | MPD's measured protocol behaviour, as against its documentation | [SPEC016-mpd-protocol-findings.md](spec/SPEC016-mpd-protocol-findings.md) |
| `[SPEC-PLAY-*]` | When a play is written to history — one rule, every path | [SPEC017-what-counts-as-a-play.md](spec/SPEC017-what-counts-as-a-play.md) |
| `[IMPL-MPD-*]` | Prototyping the MPD Director: the plan, and what it measured | [IMPL004](IMPL004-mpd-prototype.md), [IMPL005](IMPL005-mpd-prototype-results.md) |
| `[SPEC-BK-*]` | Moving a session between Vaino's engine and MPD without stopping | [SPEC018-switching-backends.md](spec/SPEC018-switching-backends.md) |
| `[SPEC-LYR-*]` | Lyrics, and what a guest protocol will not carry | [SPEC019-lyrics.md](spec/SPEC019-lyrics.md) |
| `[IMPL-*]` | Pi Zero 2W appliance setup procedure | [IMPL001-appliance-setup.md](../VainoPi/IMPL001-appliance-setup.md) |
| `[IMPL-SUI-*]` | Sampo Console build order and per-stage claims | [IMPL003-sampo-console-build.md](IMPL003-sampo-console-build.md) |
| `[IMPL-MPD-*]` | MPD Director prototype: build order, riskiest part first | [IMPL004-mpd-prototype.md](IMPL004-mpd-prototype.md) |
| `[PI3-*]` | Speaker link: design and the player's contract | [PI003-choosing-a-speaker.md](../VainoPi/PI003-choosing-a-speaker.md) |
| `[PI3-FOUND-*]`, `[PI3-ROCKER-*]`, `[PI3-LED-*]` | What operating the speaker taught | [PI004-speaker-operation.md](../VainoPi/PI004-speaker-operation.md) |
| `[PI5-LIB-*]` | Getting the real library onto the appliance, and its cost | [PI005-appliance-library.md](../VainoPi/PI005-appliance-library.md) |
| `[LOG-I*-*]` | Extraction iteration history & measured results | [LOG001-extraction-iterations.md](LOG001-extraction-iterations.md) |
| `[INH-*]` | Inherited-document provenance register & hazards | [inherited/README.md](inherited/README.md) |
| `[SPEC-FD-030]` | Total-variation per-characteristic distance | [SPEC005-flavor-distance.md](spec/SPEC005-flavor-distance.md#2-the-metric) |
| `[SPEC-FD-050]` | Measured per-characteristic reliability & scale constants | [SPEC005-flavor-distance.md](spec/SPEC005-flavor-distance.md#3-reliability--measured-not-assumed) |
| `[REQ-*]` (AUD/PD/VIS/LIB/PORT/HW) | Functional requirements — supersedes REQ001 | [REQ002-functional-requirements.md](spec/REQ002-functional-requirements.md) |
| `[SPEC-DIR-100]` | Frequency vs character orthogonality | [SPEC009-program-director.md](spec/SPEC009-program-director.md#1-the-governing-idea) |
| `[SPEC-DIR-150]` | Where Like/Dislike Taste enters selection | [SPEC009-program-director.md](spec/SPEC009-program-director.md#4-stage-b--pool-shaping) |
| `[SPEC-SC-030]` | Identity spine: files / recordings / passages DDL | [SPEC008-database-schema.md](spec/SPEC008-database-schema.md#2-identity-spine) |
| `[SPEC-SC-060]` | Flavor storage: long/narrow, per-characteristic provenance | [SPEC008-database-schema.md](spec/SPEC008-database-schema.md#4-flavor) |
| `[SPEC-SA-020]` | Sampo pipeline stages S1-S7 | [SPEC007-sampo-architecture.md](spec/SPEC007-sampo-architecture.md#2-pipeline) |
| `[SPEC-SA-090]` | OPEN: per-passage extraction, untested | [SPEC007-sampo-architecture.md](spec/SPEC007-sampo-architecture.md#6-segmentation--amplitude-s2-s6--provisional) |
| `[SPEC-DF-030]` | Identity keys: audio_md5 / recording_mbid / file_path | [SPEC006-data-flow-and-portability.md](spec/SPEC006-data-flow-and-portability.md#2-identity--three-keys-three-scopes) |
| `[SPEC-DF-035]` | Local sequence numbers: when a `passage_id` may be used | [SPEC006-data-flow-and-portability.md](spec/SPEC006-data-flow-and-portability.md#2-identity--three-keys-three-scopes) |
| `[SPEC-DF-060]` | Metadata transports: embedded tags, sidecar, db migration | [SPEC006-data-flow-and-portability.md](spec/SPEC006-data-flow-and-portability.md#4-three-transports) |
| `[REQ-AUD-010]` | Gapless Audio File Decoding | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-AUD-020]` | Passage Timestamp Trimming | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-AUD-040]` | Dual-Buffer Crossfade Ramp Mixing | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-DB-020]` | Fast Incremental File Check | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#1-requirement-enumeration--mapping) |
| `[REQ-MB-010]` | Chromaprint Fingerprinting | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#22-metadata--musicbrainz-identifier-database) |
| `[REQ-PD-010]` | Candidate Fitness Scoring Model | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#23-program-director--selection-algorithm) |
| `[SPEC-APS-060]` | Audio path supervisor: one owner, one snapshot | [SPEC011-audio-path-supervisor.md](spec/SPEC011-audio-path-supervisor.md#3-the-design) |
| `[SPEC-RLK-030]` | Relink: hash the target, match audio_md5, write the path | [SPEC012-library-relink.md](spec/SPEC012-library-relink.md#2-the-mechanism) |
| `[SPEC-RLK-050]` | Relink outcomes: matched / moved / missing / unknown | [SPEC012-library-relink.md](spec/SPEC012-library-relink.md#3-what-it-reports) |
| `[SPEC-RLK-150]` | Deferred: Symphonia takes the hash at the next re-extraction | [SPEC012-library-relink.md](spec/SPEC012-library-relink.md#7-decided-and-deferred) |
| `[SPEC-SUI-020]` | Sampo's console against the player's browse page | [SPEC013-sampo-console.md](spec/SPEC013-sampo-console.md#2-identity-and-boundaries) |
| `[SPEC-SUI-055]` | Folder view: identity and completeness are two axes | [SPEC013-sampo-console.md](spec/SPEC013-sampo-console.md#32-folder--what-is-here-and-what-is-known-about-it) |
| `[SPEC-SUI-095]` | Export ships a class A/B/C bundle, not a database | [SPEC013-sampo-console.md](spec/SPEC013-sampo-console.md#5-export--new-music-to-a-remote-vaino) |
| `[SPEC-SUI-150]` | Handoff to the player is same-database-only; export is not | [SPEC013-sampo-console.md](spec/SPEC013-sampo-console.md#34-handoff--the-players-own-pages-inside-sampos-workflow) |
| `[SPEC-PL-020]` | Payload shape: two arrays, two binding scopes | [SPEC014-payload-schema.md](spec/SPEC014-payload-schema.md#2-shape--two-arrays-because-there-are-two-scopes) |
| `[SPEC-PL-060]` | Acceptance: required set plus unresolvable conflict | [SPEC014-payload-schema.md](spec/SPEC014-payload-schema.md#4-acceptance) |
| `[SPEC-PL-090]` | Measured payload size, and the SPEC-DF-093 correction | [SPEC014-payload-schema.md](spec/SPEC014-payload-schema.md#5-size-and-a-correction) |
| `[SPEC-APS-100]` | Supervisor migration order | [SPEC011-audio-path-supervisor.md](spec/SPEC011-audio-path-supervisor.md#4-migration-order) |
| `[SPEC-AUD-010]`| Audio Engine Trait Contracts | [SPEC001-audio-engine.md](spec/SPEC001-audio-engine.md#1-interface-trait-contracts-rust--python-specs) |
| `[SPEC-AUD-040]`| Mathematical Ramp Profiles | [SPEC001-audio-engine.md](spec/SPEC001-audio-engine.md#2-mathematical-ramp-profile-models) |
| ~~`[SPEC-DB-010]`~~ | Relational DDL & Indexes | ⚠️ **Dead entry.** `SPEC002-data-schema-and-ipc.md` contains no `[SPEC-*]` tags; this row was aspirational. The document is a v1 artifact on the disposal path `[GDE-DIS-010]`. |
| `[SPEC-PD-010]` | Acoustic Transition Flow Scoring | [SPEC003-program-director-intelligence.md](spec/SPEC003-program-director-intelligence.md#21-acoustic-transition-flow-s_flow) |
| ~~`[SPEC-RUST-010]`~~| Python to Rust Module Mapping | ⚠️ **Dead entry.** `SPEC004-rust-migration-guide.md` contains no `[SPEC-*]` tags; this row was aspirational. Superseded by `[GDE-ARC-020]`. |

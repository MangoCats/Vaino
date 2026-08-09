# GOV001: Document Hygiene & Governance Standard

**Governance Specification — Tier 0**

This document establishes the official document hygiene standards, naming conventions, unique identifier taxonomies, and modularity principles for the **Vaino** project repository.

---

## 1. Document Modularity & Context Window Efficiency Rules

To ensure that both human contributors and AI coding assistants can quickly inspect specific specifications without consuming excessive context window capacity or wading through unrelated content, all project documentation MUST follow these core principles:

1. **`[GOV-DOC-010]` Focused Single-Purpose Documents**:
   - Every document MUST focus on a single domain or component.
   - Target file length is **100 to 250 lines** per document. Large documents MUST be split into sub-documents within appropriate folders (e.g., `docs/spec/`).

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
| **Experiment Records** | `[LOG-<DOMAIN>-<NUM>]` | Dated iteration history: approach, measured result, why it plateaued | `[LOG-I1-020]`, `[LOG-NEXT-010]` |
| **Inherited Material** | `[INH-<DOMAIN>-<NUM>]` | Provenance and classification of documents copied from predecessor projects | `[INH-HAZ-010]` |

### Domain Acronyms
- `AUD` — Audio Engine, Decoders, Slicing, Crossfading
- `DB` — Database, SQLite, Media Scanner
- `MB` — MusicBrainz, AcoustID, Chromaprint Fingerprinting
- `FE` — Feature Extraction, Essentia, LUFS Loudness
- `FD` — Flavor Distance, Song Similarity Metric
- `DF` — Data Flow, Identity Keys, Library Portability
- `PD` — Program Director, Auto-Playlist Selection Math
- `UI` — Web Server, REST API, WebSocket Protocol, Web UI
- `HW` — Embedded Target, RPi Zero 2W, Storage Partitioning

### Development Guidance Domains (`GDE`)
- `BMK` — MuLibPlay benchmark measurements
- `PD` — MuLibPlay selection algorithm (preserved behaviour)
- `MCR` — McRhythm/wkmp findings
- `V1` — Vaino v1 measured failures
- `LES` — Distilled lessons
- `FEX` — Feature extraction strategy (P0 critical path)
- `CHT` / `ARC` / `PHS` / `FBD` / `DIS` / `OPN` — Charter, architecture decisions, phases, forbidden patterns, disposal register, open questions

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
| `[LOG-I*-*]` | Extraction iteration history & measured results | [LOG001-extraction-iterations.md](LOG001-extraction-iterations.md) |
| `[INH-*]` | Inherited-document provenance register & hazards | [inherited/README.md](inherited/README.md) |
| `[SPEC-FD-030]` | Total-variation per-characteristic distance | [SPEC005-flavor-distance.md](spec/SPEC005-flavor-distance.md#2-the-metric) |
| `[SPEC-FD-050]` | Measured per-characteristic reliability & scale constants | [SPEC005-flavor-distance.md](spec/SPEC005-flavor-distance.md#3-reliability--measured-not-assumed) |
| `[SPEC-DF-030]` | Identity keys: audio_md5 / recording_mbid / file_path | [SPEC006-data-flow-and-portability.md](spec/SPEC006-data-flow-and-portability.md#2-identity--three-keys-three-scopes) |
| `[SPEC-DF-060]` | Metadata transports: embedded tags, sidecar, db migration | [SPEC006-data-flow-and-portability.md](spec/SPEC006-data-flow-and-portability.md#4-three-transports) |
| `[REQ-AUD-010]` | Gapless Audio File Decoding | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-AUD-020]` | Passage Timestamp Trimming | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-AUD-040]` | Dual-Buffer Crossfade Ramp Mixing | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-DB-020]` | Fast Incremental File Check | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#1-requirement-enumeration--mapping) |
| `[REQ-MB-010]` | Chromaprint Fingerprinting | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#22-metadata--musicbrainz-identifier-database) |
| `[REQ-PD-010]` | Candidate Fitness Scoring Model | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#23-program-director--selection-algorithm) |
| `[SPEC-AUD-010]`| Audio Engine Trait Contracts | [SPEC001-audio-engine.md](spec/SPEC001-audio-engine.md#1-interface-trait-contracts-rust--python-specs) |
| `[SPEC-AUD-040]`| Mathematical Ramp Profiles | [SPEC001-audio-engine.md](spec/SPEC001-audio-engine.md#2-mathematical-ramp-profile-models) |
| ~~`[SPEC-DB-010]`~~ | Relational DDL & Indexes | ⚠️ **Dead entry.** `SPEC002-data-schema-and-ipc.md` contains no `[SPEC-*]` tags; this row was aspirational. The document is a v1 artifact on the disposal path `[GDE-DIS-010]`. |
| `[SPEC-PD-010]` | Acoustic Transition Flow Scoring | [SPEC003-program-director-intelligence.md](spec/SPEC003-program-director-intelligence.md#21-acoustic-transition-flow-s_flow) |
| ~~`[SPEC-RUST-010]`~~| Python to Rust Module Mapping | ⚠️ **Dead entry.** `SPEC004-rust-migration-guide.md` contains no `[SPEC-*]` tags; this row was aspirational. Superseded by `[GDE-ARC-020]`. |

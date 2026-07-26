# GOV001: Document Hygiene & Governance Standard

**Governance Specification — Tier 0**

This document establishes the official document hygiene standards, naming conventions, unique identifier taxonomies, and modularity principles for the **Vaino** project repository.

---

## 1. Document Modularity & Context Window Efficiency Rules

To ensure that both human contributors and AI coding assistants can quickly inspect specific specifications without consuming excessive context window capacity or wading through unrelated content, all project documentation MUST follow these core principles:

1. **Focused Single-Purpose Documents**:
   - Every document MUST focus on a single domain or component.
   - Target file length is **100 to 250 lines** per document. Large documents MUST be split into sub-documents within appropriate folders (e.g., `docs/spec/`).

2. **Unique Grep-Searchable Identifiers**:
   - All requirements, design specs, entity definitions, and test cases MUST be assigned a unique, bracketed identifier tag (e.g., `[REQ-AUD-010]`, `[SPEC-AUD-020]`, `[UT-AUD-001]`).
   - Tags MUST be consistent across specifications, source code comments, and automated test names.

3. **Direct Markdown Hyperlinks**:
   - All references to other documents MUST use standard GitHub Markdown file links with explicit relative paths (e.g., `[SPEC001: Audio Engine](SPEC001-audio-engine.md)`).

4. **Synchronous Specification & Test Maintenance Rule (`[GOV-DOC-020]`)**:
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

### Domain Acronyms
- `AUD` — Audio Engine, Decoders, Slicing, Crossfading
- `DB` — Database, SQLite, Media Scanner
- `MB` — MusicBrainz, AcoustID, Chromaprint Fingerprinting
- `FE` — Feature Extraction, Essentia, LUFS Loudness
- `PD` — Program Director, Auto-Playlist Selection Math
- `UI` — Web Server, REST API, WebSocket Protocol, Web UI
- `HW` — Embedded Target, RPi Zero 2W, Storage Partitioning

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
| `[REQ-AUD-010]` | Gapless Audio File Decoding | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-AUD-020]` | Passage Timestamp Trimming | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-AUD-040]` | Dual-Buffer Crossfade Ramp Mixing | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#21-audio-engine--pipeline) |
| `[REQ-DB-020]` | Fast Incremental File Check | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#1-requirement-enumeration--mapping) |
| `[REQ-MB-010]` | Chromaprint Fingerprinting | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#22-metadata--musicbrainz-identifier-database) |
| `[REQ-PD-010]` | Candidate Fitness Scoring Model | [REQ001-system-requirements.md](spec/REQ001-system-requirements.md#23-program-director--selection-algorithm) |
| `[SPEC-AUD-010]`| Audio Engine Trait Contracts | [SPEC001-audio-engine.md](spec/SPEC001-audio-engine.md#1-interface-trait-contracts-rust--python-specs) |
| `[SPEC-AUD-040]`| Mathematical Ramp Profiles | [SPEC001-audio-engine.md](spec/SPEC001-audio-engine.md#2-mathematical-ramp-profile-models) |
| `[SPEC-DB-010]` | Relational DDL & Indexes | [SPEC002-data-schema-and-ipc.md](spec/SPEC002-data-schema-and-ipc.md#1-database-relational-schema-sqlite-ddl) |
| `[SPEC-PD-010]` | Acoustic Transition Flow Scoring | [SPEC003-program-director-intelligence.md](spec/SPEC003-program-director-intelligence.md#21-acoustic-transition-flow-s_flow) |
| `[SPEC-RUST-010]`| Python to Rust Module Mapping | [SPEC004-rust-migration-guide.md](spec/SPEC004-rust-migration-guide.md#1-python-to-rust-module-mapping-matrix) |

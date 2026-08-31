# Vaino

> **Continuous Radio-Style Music Player Engine**  
> *"Born wise, ready to play, singing seamless melodies into the world."*

---

## 📻 Overview

**Vaino** is a continuous, automated music playback system designed to transform a music collection into a personal, 24/7 radio station experience. Rather than requiring users to manually construct playlists or stop between tracks, Vaino dynamically arranges songs, sound-bite clips, and long capture files (such as Disc-At-Once DAO files) into a seamless audio stream with custom crossfades, fade ramps, and trim points.

Vaino is built for both dedicated, low-power embedded appliances (such as a Raspberry Pi Zero 2W connected to high-fidelity audio equipment) and standard desktop computers (Windows, Linux, macOS).

---

## ⚡ Core Philosophy & Inspiration

Vaino draws its name and spiritual concept from **Väinämöinen**, the elemental bard and hero of the Finnish epic *Kalevala*:

1. **The Eternal Sage (Instant-On readiness)**:  
   Väinämöinen was born wise and ancient, ready to act without delay. Vaino is engineered for **instant readiness** upon power-on. On embedded hardware, the primary goal is getting the first song playing immediately, with web management services launching asynchronously as network services become available.

2. **The Singing Sorcerer (Automated Music Stream)**:  
   Rather than asking the user to manually control every track, Vaino acts as an automated digital shaman. It reads the context of the moment—listener preferences, play history, time of day, day of week, and day of year—along with high-level audio characteristics to curate a continuous, harmonious stream of sound.

Vaino is a fresh project and the spiritual successor to the abandoned `McRhythm` project. While drawing inspiration from `McRhythm`, Vaino operates with a clean slate, unconstrained by legacy design choices.

---

## 🔑 Key Features & Architecture Pillars

- **Continuous Audio Stream Engine**: Seamlessly transitions across standard single-track audio files, DAO (Disc-at-Once) full album captures, and quick sound-bite clips using configured trim points and custom crossfade ramp profiles.
- **Strict Co-Located Audio Output**: Where the Vaino server runs is strictly where audio streams out. Remote devices act as controllers, not streaming endpoints.
- **Dual Target Platforms**:
  - **Embedded Appliance**: Designed for 24-7 operation on Raspberry Pi Zero 2W with D/A HAT audio or Bluetooth, fast-boot audio priority, and a power-loss-resilient 3-partition storage strategy.
  - **Desktop Application**: Provides an identical playback and control experience on Windows, Linux, and macOS systems.
- **Web-Based Control & Wall Art Interface**:
  - **Quick Control**: Responsive control layout for queue editing, playlist shaping, and manual overrides on phone, tablet, or desktop browsers.
  - **Wall Art / Kiosk Mode**: Fullscreen visual display showcasing high-resolution album artwork, upcoming queue, clock, and ambient visual decorations for dedicated wall-mounted displays.
- **Context-Aware Playlist Intelligence**: Uses track metadata (MusicBrainz IDs), AudioBrainz-inspired high-level music descriptors, play history, and temporal context (time/day/season) to select optimal upcoming songs.

---

## 📚 Documentation Index

Detailed architectural and design specifications are organized in the [`docs/`](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs) folder. *(Index rebuilt 2026-08-30 — the previous version linked seven files describing a pre-rearchitecture plan that was never built; see `[GDE-DIS-010]` in GUIDE002.)*

**Start here:**
- 🚀 **[HOWTO.md](HOWTO.md)** — build and run Vaino and Sampo locally, today.
- 🧭 **[GUIDE001: Project Lineage & Lessons Learned](docs/GUIDE001-lineage-and-lessons.md)** — Measured state of MuLibPlay, McRhythm/wkmp and Vaino v1; the benchmark to beat, the selection algorithm to preserve, and the failures to never repeat.
- 🗺️ **[GUIDE002: Re-Architecture Plan](docs/GUIDE002-rearchitecture-plan.md)** — Design charter, architectural decisions, phased plan, forbidden patterns, and the predecessor disposal register. **This is the current plan** — it supersedes the phased Python-first roadmap this README used to link.

**Governance:**
- 📋 **[GOV001: Document Hygiene & Governance Standard](docs/GOV001-document-hygiene.md)** — identifier taxonomy (`REQ`, `SPEC`, `UT`, `ENT`), modularity rules, and master search index.
- ⚖️ **[GOV002: Sources of Truth](docs/GOV002-sources-of-truth.md)** — how to rank two disagreeing answers to the same question, and the register of rankings already made.
- 📥 **[Inherited Documents Register](docs/inherited/README.md)** — design material copied in from MuLibPlay and McRhythm, classified as active design input vs historical evidence, with collision hazards noted.
- 📜 **[LICENSING.md](LICENSING.md)** — **two licences in this one repository**: `player/` and `docs/` are MIT; `tools/` (Sampo) is AGPL-3.0-or-later, because it invokes Essentia. Read this before assuming the root `LICENSE` file covers everything.

**Design guidance:**
- 🔬 **[GUIDE003: Feature Extraction Strategy](docs/GUIDE003-feature-extraction-strategy.md)** — **P0 critical path.** Replacing AcousticBrainz: harvest the archived dumps, reproduce the pipeline, two-stage validation.
- 📱 **[GUIDE004: Phone Port Strategy](docs/GUIDE004-phone-port-strategy.md)** · 🌐 **[GUIDE005: Flavor Without Sampo](docs/GUIDE005-flavor-service.md)** · 🔌 **[GUIDE006: The Director as a Guest](docs/GUIDE006-director-as-a-guest.md)** · 📊 **[GUIDE007: External Backends](docs/GUIDE007-external-backends-investigation.md)**
- 📊 **[LOG001: Feature Extraction Iteration Log](docs/LOG001-extraction-iterations.md)** — dated record of every extraction attempt, its measured result, and why it plateaued.

**Current specifications (`docs/spec/`, ~22 documents):** start from [REQ002: Functional Requirements](docs/spec/REQ002-functional-requirements.md) (what Vaino and Sampo must do) and [SPEC008: Database Schema](docs/spec/SPEC008-database-schema.md) (the `vaino.db` DDL), then follow their own cross-links — [SPEC009 Program Director](docs/spec/SPEC009-program-director.md), [SPEC007 Sampo Architecture](docs/spec/SPEC007-sampo-architecture.md) *(provisional)*, [SPEC006 Data Flow & Portability](docs/spec/SPEC006-data-flow-and-portability.md), [SPEC005 Flavor Distance](docs/spec/SPEC005-flavor-distance.md), and onward through SPEC010–SPEC022 (identification review, the audio path supervisor, library relink, the Sampo console, the MPD backend, waveform editing). GOV001's master index is the fastest way to jump straight to a tag.

**Architecture and appliance:**
- 🏛️ **[System Architecture & Audio Pipeline](docs/architecture.md)** — describes what is actually built: the two-binary split, the backend seam, the audio path, the data model.
- ⚡ **[Embedded Hardware & Storage Resilience](VainoPi/embedded-hardware.md)** — Raspberry Pi Zero 2W spec, fast-boot sequence, and 3-partition layout.
- 🍓 **[IMPL001: Pi Zero 2W Appliance Setup](VainoPi/IMPL001-appliance-setup.md)** — step-by-step OS build for the appliance.

---

## 🛠️ Technology Stack

Two separate programs sharing one SQLite file, decided in [GUIDE002](docs/GUIDE002-rearchitecture-plan.md) and detailed in [architecture.md](docs/architecture.md) — not a phased Python-then-Rust migration:

- **`player/` — Vaino, the player. Rust from the start** (`symphonia` + `rubato` + `cpal` + `axum`), MIT-licensed, portable to the Pi Zero 2W (`≤150MB` RSS) and to desktop. Nothing AGPL is ever linked into it.
- **`tools/` — Sampo, the library builder. Python**, AGPL-3.0-or-later (because Essentia is), x86-desktop-only, invoked as a subprocess. Scanning, fingerprinting, MusicBrainz, DAO segmentation, feature extraction, review UI. Never runs on the appliance.

They interoperate only through the shared `vaino.db` file — no linked code, no RPC, no shared process in either direction.

---

## 🚀 Target Platforms & Roadmap

| Platform | Role | Priority |
| :--- | :--- | :--- |
| **Raspberry Pi Zero 2W** | Dedicated 24/7 Radio Station Appliance | **First Priority** |
| **Desktop PC (Win/Linux/macOS)** | Standard Computer Host / Player | **First Priority** |
| **Mobile (Android / iOS)** | Direct Native Mobile Host | *Future / Post-V1 (No early influence)* |

---

## 📜 License

Two licences, one repository: `player/`, `docs/`, `build/`, `sql/` and the root files are **MIT** ([LICENSE](LICENSE)); `tools/` (Sampo) is **AGPL-3.0-or-later** ([tools/LICENSE](tools/LICENSE)), because it invokes Essentia. See [LICENSING.md](LICENSING.md) for the full arrangement and why the direction only works one way.

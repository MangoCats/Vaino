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

Detailed architectural and design specifications are organized in the [`docs/`](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs) folder:

- 📋 **[GOV001: Document Hygiene & Governance Standard](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/GOV001-document-hygiene.md)** — Repository governance rules, unique identifier taxonomy (`REQ`, `SPEC`, `UT`, `ENT`), modularity principles, and master search index.
- 🏛️ **[System Architecture & Audio Pipeline](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/architecture.md)** — Core components, audio pipeline, trimming, crossfading, and audio locality rules.
- ⚡ **[Embedded Hardware & Storage Resilience](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/embedded-hardware.md)** — Raspberry Pi Zero 2W spec, fast-boot sequence, and 3-partition layout.
- 🎨 **[User Interface & Control Model](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/user-interface.md)** — Web server interface, Quick Control, and Wall Art / Kiosk mode specifications.
- 🗄️ **[Audio Database & Selection Intelligence](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/audio-database.md)** — Database schema concept, audio descriptors, and context-driven recommendation engine.
- ⚙️ **[Tech Stack & Microservices Investigation](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/tech-stack-investigation.md)** — Microservice partitioning model, technology stack comparisons (Rust, Go, Python, Hybrid), AcoustID/Chromaprint, and Essentia acoustic feature extraction strategy.
- 🗺️ **[End-to-End Development Roadmap](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/roadmap.md)** — Phased 7-stage roadmap from basic web-controlled player to full autonomous station engine.
- 🚀 **[Phase 1 Implementation Plan & Deliverables](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/phase1-plan.md)** — Concrete architecture, module specifications, deliverables, and acceptance criteria for Phase 1.
- 💰 **[Subscription & Technology Cost Breakdown](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/cost-estimate.md)** — Cost analysis for MusicBrainz, AcoustID, Essentia, and offline operations ($0/month).

### 📜 Formal Specification Hierarchy (`docs/spec/`)
- 📋 **[REQ001: System Requirements & Verification Matrix](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/spec/REQ001-system-requirements.md)** — Enumerated requirements (`[REQ-AUD-010]`, `[REQ-PD-010]`, `[REQ-HW-010]`) with unit testing & acceptance criteria.
- 🎵 **[SPEC001: Audio Engine & Pipeline Specification](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/spec/SPEC001-audio-engine.md)** — Rust/Python trait contracts, dual-buffer crossfade state machine, and ramp curve equations.
- 🗄️ **[SPEC002: Database Schema & IPC Protocol Specification](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/spec/SPEC002-data-schema-and-ipc.md)** — Relational DDL, WebSocket event schemas, and multi-user sync contracts.
- 🧙‍♂️ **[SPEC003: Program Director Selection Engine Specification](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/spec/SPEC003-program-director-intelligence.md)** — Candidate scoring math, time-of-day curves, acoustic feature distance, and anti-repetition decay formulas.
- 🦀 **[SPEC004: Rust Migration & Cross-Compilation Guide](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/spec/SPEC004-rust-migration-guide.md)** — 1-to-1 Python-to-Rust module mapping matrix, `<30MB` RAM allocation budget, and RPi Zero 2W ARM64 build target.

---

## 🛠️ Technology Stack Strategy

Vaino adopts a **Phased Evolutionary Engineering Strategy** (documented in [docs/tech-stack-investigation.md](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/tech-stack-investigation.md)):

- **Phase 1: Rapid Prototyping (Python / Hybrid)**: Fast R&D and validation of the SQLite schema, AcoustID fingerprinting (`pyacoustid`), Essentia acoustic feature extraction (`essentia-python`), auto-playlist selection algorithms, and the Web UI (FastAPI + WebSockets).
- **Phase 2: Production Engine Migration (Rust Core)**: Migration of the real-time Audio Playback Engine, Crossfader, and Web Server to **Rust** (`symphonia` + `rodio`/`cpal` + `axum`) to achieve <30MB RAM footprint, instant boot (<1s), and zero-GC audio streaming for 24/7 Raspberry Pi Zero 2W appliances.

---

## 🚀 Target Platforms & Roadmap

| Platform | Role | Priority |
| :--- | :--- | :--- |
| **Raspberry Pi Zero 2W** | Dedicated 24/7 Radio Station Appliance | **First Priority** |
| **Desktop PC (Win/Linux/macOS)** | Standard Computer Host / Player | **First Priority** |
| **Mobile (Android / iOS)** | Direct Native Mobile Host | *Future / Post-V1 (No early influence)* |

---

## 📜 License

This project is licensed under the **MIT License** — see the [LICENSE](file:///c:/Users/Mango%20Cat/Dev/Vaino/LICENSE) file for details.

# SPEC004: Rust Migration & Cross-Compilation Guide

**Design Specification — Tier 2**

This document defines the 1-to-1 code translation mapping, crate ecosystem selection, memory budget allocations, and cross-compilation strategy for migrating Vaino's Python reference implementation to **Rust** in **Phase 6**.

---

## 1. Python to Rust Module Mapping Matrix

| Python Reference Module | Target Rust Crate / Module | Responsibility |
| :--- | :--- | :--- |
| `src/audio/engine.py` (Decoder) | `symphonia` | Pure Rust audio file decoding (MP3, FLAC, WAV, OGG, M4A) |
| `src/audio/engine.py` (Playback) | `cpal` / `rodio` | Low-latency PCM stream output to ALSA, WASAPI, or CoreAudio |
| `src/db/database.py` | `rusqlite` (WAL mode) | Embedded SQLite connection management & batch transactions |
| `src/db/scanner.py` | `rayon` + `symphonia` | Parallel multi-threaded audio directory scanner & metadata parser |
| `src/server/app.py` | `axum` + `tokio` | High-performance async HTTP REST API & static file server |
| `src/server/websocket.py` | `axum::extract::ws` | WebSocket state broadcast connection manager |
| `main.py` | `src/main.rs` + `clap` | Binary entry point & CLI argument parsing |

---

## 2. Cargo Dependencies (`Cargo.toml`)

```toml
[package]
name = "vaino"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async Runtime & Web Server
tokio = { version = "1.35", features = ["full"] }
axum = { version = "0.7", features = ["ws"] }
tower-http = { version = "0.5", features = ["fs", "cors"] }

# Audio Decoding & Hardware Output
symphonia = { version = "0.5", features = ["all"] }
cpal = "0.15"

# Database & Serialization
rusqlite = { version = "0.30", features = ["bundled"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Parallelism & Utility
rayon = "1.8"
sha2 = "0.10"
clap = { version = "4.4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

---

## 3. Raspberry Pi Zero 2W Memory Budget Allocation (<30MB RSS)

On the Raspberry Pi Zero 2W (512MB RAM total), Vaino's Rust binary enforces strict memory bounds:

```
+------------------------------------------------------------------+
|                    RAST BINARY MEMORY BUDGET                      |
|                                                                  |
|  [ Tokio Async Runtime Heap ]   ──►  6.0 MB                      |
|  [ Symphonia PCM Decoded Ring ] ──►  8.0 MB (Dual-buffer 44.1kHz)|
|  [ Rusqlite Page Cache ]       ──►  4.0 MB (SQLite Cache)        |
|  [ Queue & State Allocation ]   ──►  2.0 MB                      |
|  [ Thread Stack & OS Overhead ] ──►  5.0 MB                      |
|  --------------------------------------------------------------  |
|  TOTAL TARGET RESIDENT MEMORY  ──► 25.0 MB (< 30.0 MB Max Limit) |
+------------------------------------------------------------------+
```

---

## 4. Cross-Compilation Workflow for RPi Zero 2W (ARM64)

Cross-compiling the single Rust binary for the Raspberry Pi Zero 2W target (`aarch64-unknown-linux-gnu`):

```bash
# 1. Install Rust target
rustup target add aarch64-unknown-linux-gnu

# 2. Build production release binary via cross
cargo build --target aarch64-unknown-linux-gnu --release

# 3. Deploy single binary to Raspberry Pi Zero 2W
scp target/aarch64-unknown-linux-gnu/release/vaino pi@vaino-station.local:/opt/vaino/bin/
```

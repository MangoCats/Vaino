# SPEC004: Go Migration & Cross-Compilation Guide for Raspberry Pi Zero 2W

**Design Specification — Tier 2**

This document defines the 1-to-1 code translation mapping, Go package ecosystem selection, memory budget allocations (<20MB RSS), concurrency model, and cross-compilation strategy for migrating Vaino's Python reference implementation to **Go** for production deployment on **Raspberry Pi Zero 2W** targets.

---

## 1. Executive Summary & Goals

On the Raspberry Pi Zero 2W (512MB RAM, quad-core 1.0GHz ARM Cortex-A53), Python's runtime overhead (~120MB+ RSS) consumes significant memory and CPU resources. 

Migrating to Go provides:
- **Low Resident Memory**: **< 20 MB RSS** total resident memory usage (~85% memory savings).
- **Instant Boot / Zero Cold Start**: Sub-50ms application startup time.
- **Lock-Free Concurrency**: Native Goroutines and Channels decouple audio rendering from HTTP/WebSocket web handling without Python GIL contention.
- **Single Static Binary**: Self-contained executable deployment (`aarch64-linux-gnu`) with zero external runtime or Python virtualenv dependencies.

---

## 2. Python to Go Module Translation Matrix

| Python Reference File | Target Go Package / Path | Primary Responsibility |
| :--- | :--- | :--- |
| `main.py` | `cmd/vaino/main.go` | Application entry point, flag parsing, signal handling |
| `src/audio/engine.py` | `pkg/audio/engine.go` | Low-latency PCM audio stream output to ALSA, dual ring buffer, volume scaling |
| `src/audio/selector.py` | `pkg/selector/director.go` | 4-pass candidate pool refining, 11D Euclidean distance, rotation/recovery/restraint math |
| `src/db/database.py` | `pkg/db/database.go` | Embedded SQLite database access, schema migrations, WAL mode transactions |
| `src/db/scanner.py` | `pkg/scanner/scanner.go` | Multi-goroutine audio library directory scanner & ID3/FLAC metadata parser |
| `src/server/app.py` | `pkg/server/router.go` | High-performance REST API endpoints (`go-chi/chi`) |
| `src/server/websocket.py` | `pkg/server/websocket.go` | Concurrent WebSocket client hub & state broadcaster (`gorilla/websocket`) |

---

## 3. Go Project Structure (`/` Root)

```text
vaino/
├── cmd/
│   └── vaino/
│       └── main.go               # Entry point
├── pkg/
│   ├── audio/
│   │   ├── engine.go             # Audio engine & ALSA player
│   │   └── decoder.go            # MP3/FLAC/WAV stream decoders
│   ├── db/
│   │   ├── database.go           # SQLite connection & schema migrations
│   │   └── schema.sql            # Embedded SQLite DDL
│   ├── scanner/
│   │   └── scanner.go            # Parallel directory scanner
│   ├── selector/
│   │   ├── director.go           # Program Director engine
│   │   └── math.go               # 11D Euclidean distance & rating math
│   └── server/
│       ├── router.go             # Chi REST API routes
│       └── websocket.go          # WebSocket hub & client connections
├── go.mod
├── go.sum
└── Makefile
```

---

## 4. Go Dependency Specifications (`go.mod`)

```go
module github.com/mangocats/vaino

go 1.22

require (
	// Async Web Server & Router
	github.com/go-chi/chi/v5 v5.0.12
	github.com/go-chi/cors v1.2.1
	github.com/gorilla/websocket v1.5.1

	// Database & Driver
	github.com/mattn/go-sqlite3 v1.14.22 // or modernc.org/sqlite for Cgo-free builds

	// Audio Playback & Metadata
	github.com/ebitengine/oto/v3 v3.2.0
	github.com/dhowden/tag v0.0.0-20230630033851-978a0926ee25
	github.com/hajimehoshi/go-mp3 v0.3.4

	// Concurrency & Utilities
	golang.org/x/sync v0.6.0
)
```

---

## 5. Raspberry Pi Zero 2W Memory Budget Allocation (<20MB RSS)

Memory layout allocation enforced under `GOGC=50`:

```
+-------------------------------------------------------------------+
|                     GO BINARY MEMORY BUDGET                       |
|                                                                   |
|  [ Goroutine Stacks & Runtime ]  ──►  3.0 MB                      |
|  [ Oto PCM Ring Buffer ]         ──►  4.0 MB (Dual-buffer 44.1kHz) |
|  [ SQLite Page Cache ]           ──►  4.0 MB (WAL Mode Cache)       |
|  [ Active Track & Ratings Cache ]──►  2.0 MB                      |
|  [ HTTP / WebSocket Buffers ]    ──►  2.0 MB                      |
|  ---------------------------------------------------------------  |
|  TOTAL RESIDENT MEMORY (RSS)     ──► 15.0 MB (< 20.0 MB Max Limit)  |
+-------------------------------------------------------------------+
```

---

## 6. Mathematical Equivalence Verification (Go Math Package)

In `pkg/selector/math.go`, Go float64 operations match the Python equations exactly:

### 11D Euclidean Distance
```go
// Calculate11DDistance computes normalized 11D Euclidean distance matching SPEC003.
func Calculate11DDistance(u, v []float64) float64 {
	if len(u) != 11 || len(v) != 11 {
		return 0.5
	}
	var sum float64
	for i := 0; i < 11; i++ {
		diff := u[i] - v[i]
		sum += diff * diff
	}
	return math.Sqrt(sum / 11.0)
}
```

### Rotation, Recovery & Restraint
```go
func RotationToSeconds(rv float64) float64 {
	return math.Pow(10.0, rv) * 3600.0
}

func CalculateRestraintWeight(restraint float64) float64 {
	return math.Pow(10.0, -restraint)
}

func CalculateRecoveryWeight(ageSec, rotSec, recSec float64) float64 {
	if ageSec <= rotSec {
		return 0.0
	}
	if ageSec >= (rotSec + recSec) || recSec <= 0 {
		return 1.0
	}
	return (ageSec - rotSec) / recSec
}
```

---

## 7. Cross-Compilation & Deployment Pipeline for Pi Zero 2W

Cross-compiling the static ARM64 binary from Windows/Linux to the Raspberry Pi Zero 2W (`aarch64-linux-gnu`):

```bash
# 1. Cross-compile Go ARM64 binary for Raspberry Pi Zero 2W
GOOS=linux GOARCH=arm64 CGO_ENABLED=1 CC=aarch64-linux-gnu-gcc go build -ldflags="-s -w" -o vaino-arm64 ./cmd/vaino

# 2. Deploy binary to Pi Zero 2W
scp vaino-arm64 pi@bose:/opt/vaino/bin/vaino

# 3. Systemd Service Unit (/etc/systemd/system/vaino.service)
# ExecStart=/opt/vaino/bin/vaino --db=/var/lib/vaino/vaino.db --music=/media/music
```

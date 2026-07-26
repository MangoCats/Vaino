# SPEC001: Audio Engine & Pipeline Specification

**Design Specification — Tier 2**

This document specifies the internal data structures, trait interfaces, state machines, and mathematical equations for the **Vaino Audio Playback Engine**, designed for direct mapping from Python to Rust.

---

## 1. Interface Trait Contracts (Rust / Python Specs)

### 1.1 `AudioDecoder` Trait
```rust
/// Interface for audio file decoding and sample extraction
pub trait AudioDecoder: Send + Sync {
    /// Opens an audio file and initializes decoding streams
    fn open(file_path: &Path) -> Result<Self, AudioError>;
    
    /// Decodes a frame chunk into 32-bit floating point PCM samples [-1.0, 1.0]
    fn decode_chunk(&mut self, max_samples: usize) -> Result<PCMBuffer, AudioError>;
    
    /// Seeks to a specific timestamp offset in milliseconds
    fn seek_ms(&mut self, offset_ms: u64) -> Result<(), AudioError>;
    
    /// Returns audio format properties
    fn properties(&self) -> AudioProperties;
}

pub struct AudioProperties {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
}
```

### 1.2 `Crossfader` Trait
```rust
/// Interface for dual-buffer audio crossfading and ramp mixing
pub trait Crossfader: Send + Sync {
    /// Configures the active transition between Track A (outgoing) and Track B (incoming)
    fn setup_transition(
        &mut self,
        track_a: Box<dyn AudioDecoder>,
        track_b: Box<dyn AudioDecoder>,
        crossfade_window_ms: u32,
        profile: RampProfile,
    );

    /// Generates the next mixed PCM frame chunk
    fn mix_next_chunk(&mut self, frame_count: usize) -> PCMBuffer;
}
```

---

## 2. Mathematical Ramp Profile Models

During crossfading or track fade-in/out, the gain $\alpha(t) \in [0.0, 1.0]$ at normalized time $t \in [0.0, 1.0]$ is computed using one of three mathematical curves:

```
        LINEAR                   EXPONENTIAL                 LOGISTIC S-CURVE
  1.0┌───────────/        1.0┌────────────/        1.0┌───────────--/
     │          /            │           /            │          /
     │         /             │          /             │         /
     │        /              │        _/              │        /
  0.0└───────/            0.0└──────_/             0.0└──────_/
     0.0       1.0           0.0       1.0            0.0       1.0
```

1. **Linear Ramp**:
   $$\alpha_{\text{linear}}(t) = t$$

2. **Exponential Ramp**:
   $$\alpha_{\text{exp}}(t) = t^2 \quad \text{or} \quad \alpha_{\text{exp}}(t) = \frac{e^{k \cdot t} - 1}{e^k - 1} \quad (k=2.0)$$

3. **Logistic S-Curve Ramp**:
   $$\alpha_{\text{scurve}}(t) = \frac{1}{1 + e^{-k \cdot (t - 0.5)}} \quad \text{normalized so } \alpha(0)=0.0, \alpha(1)=1.0$$

---

## 3. Dual-Buffer Crossfader State Machine

The audio engine manages two concurrent decoding buffers (`Track A` and `Track B`) during track transitions:

```
   [ Track A Playing ] ──► (Track A reaches EndOffset - CrossfadeWindow)
                                  │
                                  ▼
                   [ DUAL-BUFFER CROSSFADE STATE ]
             ├── Track A: Apply Fade-Out Gain (1 - α(t))
             └── Track B: Apply Fade-In Gain (α(t))
                                  │
                                  ▼
   [ Track A Stopped ] ──► [ Track B Becomes Active Primary Track ]
```

---

## 4. Unit Testing Specifications

To ensure mathematical precision across Python and Rust implementations, unit tests MUST verify the following assertions:

### Test Case `UT-AUD-001`: Buffer Volume Normalization
- **Input**: Integer 16-bit PCM sample `32767` and `-32768`.
- **Expected Output**: Float 32-bit sample `+0.999969` and `-1.000000`.

### Test Case `UT-AUD-002`: S-Curve Midpoint Symmetry
- **Input**: $t = 0.5$.
- **Expected Output**: $\alpha_{\text{scurve}}(0.5) = 0.5000 \pm 0.0001$.

### Test Case `UT-AUD-003`: Crossfade Energy Balance
- **Input**: Equal-power crossfade at $t = 0.5$ for two identical in-phase sine waves.
- **Expected Output**: Output RMS amplitude variance $< 0.05 \text{ dB}$.

# SPEC003: Program Director Selection Engine Specification

**Design Specification — Tier 2**

This document specifies the mathematical models, multi-stage selection pipeline, rotation/recovery algorithms, logarithmic restraint preference scaling, occasion weighting curves, and roulette-wheel weighted random sampling for Vaino's autonomous playlist selector (**"Singing Sorcerer"**).

---

## 1. Multi-Stage Selection Pipeline Overview

To provide maximum musical variety, acoustic flow, and user tunability, Vaino evaluates candidate tracks $k$ using a multi-pass pipeline combining **MuLibPlay multiplicative preference/cooldown weighting** with **11-Dimensional AcousticBrainz transition flow matching** and **roulette-wheel weighted random sampling**.

```
    ┌────────────────────────────────────────────────────────┐
    │  All Library Tracks / Passages                         │
    └───────────────────────────┬────────────────────────────┘
                                │
                                ▼
    ┌────────────────────────────────────────────────────────┐
    │  Pass 1: Multiplicative Weight & Rotation Filter       │
    │  Calculate W(k) = W_restraint * W_rec * W_occ * W_flow │
    │  Discard W(k) < 0.001 or hard rotation lockouts        │
    └───────────────────────────┬────────────────────────────┘
                                │
                                ▼
    ┌────────────────────────────────────────────────────────┐
    │  Pass 2: Active Program Seed Alignment                 │
    │  Down-select pool to N_excl (1000) using 11D distance  │
    │  between candidates and active station seed tracks     │
    └───────────────────────────┬────────────────────────────┘
                                │
                                ▼
    ┌────────────────────────────────────────────────────────┐
    │  Pass 3: Queue-Tail Acoustic Flow Re-sorting           │
    │  Re-sort candidates by 11D similarity to last track    │
    │  currently in active playback queue                    │
    └───────────────────────────┬────────────────────────────┘
                                │
                                ▼
    ┌────────────────────────────────────────────────────────┐
    │  Pass 4: Roulette-Wheel Weighted Random Sampling       │
    │  Pick next track probabilistically with position       │
    │  decay scaling (0.96^i) across candidate pool          │
    └───────────────────────────┬────────────────────────────┘
                                │
                                ▼
    ┌────────────────────────────────────────────────────────┐
    │  Selected Track Auto-Enqueued to Active Queue          │
    └────────────────────────────────────────────────────────┘
```

---

## 2. Multiplicative Candidate Weighting Formula

For any candidate track $k$, the total selection weight $W(k)$ is defined as:

$$W(k) = W_{\text{restraint, trk}}(k) \cdot W_{\text{restraint, art}}(k) \cdot W_{\text{rec, trk}}(k) \cdot W_{\text{rec, art}}(k) \cdot W_{\text{rec, rel}}(k) \cdot W_{\text{occasion}}(k) \cdot W_{\text{flow}}(k) \cdot \sqrt{\frac{180}{\text{duration\_sec}(k)}}$$

If $W(k) < 0.001$, candidate $k$ is deemed ineligible and discarded from the selection pool.

---

## 3. Component Mathematical Formulas

### 3.1 Exponential Rotation Hard Lockout & Linear Recovery Ramp

Every track and artist has log-scale parameters:
- **Rotation** ($rv_{\text{rot}} \in [0.0, 3.335]$): Base lockout time before a track or artist can be re-played.
- **Recovery** ($rv_{\text{rec}} \in [0.0, 3.335]$): Recovery window following rotation block during which selection probability ramps up.

Rotation and recovery values map to seconds via exponential power scaling:

$$T_{\text{rot}} = 10^{rv_{\text{rot}}} \cdot 3600 \quad \text{seconds}$$
$$T_{\text{rec}} = 10^{rv_{\text{rec}}} \cdot 3600 \quad \text{seconds}$$

*Standard Defaults*:
- Track Rotation: $rv_{\text{rot, trk}} = 0.0$ ($1.0\text{ hour}$)
- Track Recovery: $rv_{\text{rec, trk}} = 0.778$ ($6.0\text{ hours}$)
- Artist Rotation: $rv_{\text{rot, art}} = 0.778$ ($6.0\text{ hours}$)
- Artist Recovery: $rv_{\text{rec, art}} = 0.778$ ($6.0\text{ hours}$)

Let $\Delta t$ be the elapsed time in seconds since candidate track $k$ (or artist $A(k)$) last completed playback:

$$W_{\text{rec}}(\Delta t) = 
\begin{cases} 
0.0 & \text{if } \Delta t < T_{\text{rot}} \quad \text{(Hard Rotation Lockout)} \\
\frac{\Delta t - T_{\text{rot}}}{T_{\text{rec}}} & \text{if } T_{\text{rot}} \le \Delta t < (T_{\text{rot}} + T_{\text{rec}}) \quad \text{(Linear Recovery Ramp)} \\
1.0 & \text{if } \Delta t \ge (T_{\text{rot}} + T_{\text{rec}}) \quad \text{(Fully Recovered)}
\end{cases}$$

#### Related Track Lockout Propagation ($W_{\text{rec, rel}}$)
If candidate $k$ is linked to a related track $r$ (e.g., live vs. studio recording) with relationship weight $w_{\text{rel}}$, the lockout and recovery parameters are scaled by $w_{\text{rel}}$, preventing immediate repetition of variant performances.

---

### 3.2 Logarithmic Restraint Preference Scaling ($W_{\text{restraint}}$)

User preference tuning for tracks and artists is controlled via restraint parameters $rv_{\text{restraint}} \in [-1.0, 3.335]$:

$$W_{\text{restraint}} = 10^{-rv_{\text{restraint}}}$$

*Restraint Impact*:
- $rv = 0.0 \implies W = 1.0\times$ (Standard neutral weight)
- $rv = -0.3 \implies W \approx 2.0\times$ (High user preference / frequent rotation)
- $rv = +1.0 \implies W = 0.1\times$ (Low preference / rare rotation)
- $rv = +3.0 \implies W = 0.001\times$ (Near-total exclusion)

Artist restraint scales track restraint multiplicatively: $W_{\text{restraint, total}} = W_{\text{restraint, trk}} \cdot W_{\text{restraint, art}}$.

---

### 3.3 Occasion & Calendar Seasonal Weighting ($W_{\text{occasion}}$)

Tracks tagged with occasion codes evaluate calendar-sensitive multipliers based on current date (month $m$, day $d$):

#### Christmas Code `[C]`
- **January – October** ($m < 11$): $W_{\text{occasion}} = 10^{-6}$ (Effective silence)
- **November** ($m = 11$): Ramps smoothly from $\approx 0.05$ up to $1.0$:
  $$W_{\text{occasion}} = \left( \frac{25}{55 - d} \right)^3$$
- **December** ($m = 12, d \le 24$): Ramps from $1.0$ up to $5.0$ on Christmas Eve:
  $$W_{\text{occasion}} = \frac{5.0}{\sqrt{\max(1, 25 - d)}}$$
- **Christmas Day** ($m = 12, d = 25$): $W_{\text{occasion}} = 10.0$ (Peak holiday priority)
- **Post-Christmas** ($m = 12, d > 25$): Decay factor $W_{\text{occasion}} = \frac{-1.0}{25 - d}$

#### Seasonal Codes `[W]` (Winter), `[S]` (Summer), `[K]` (Kids)
- `[W]` Winter: Nov ($0.5\times$), Dec ($2.0\times$), Jan ($1.5\times$), Feb ($1.0\times$), Mar ($0.25\times$), Other ($10^{-6}\times$)
- `[S]` Summer: May ($0.5\times$), Jun ($2.0\times$), Jul ($1.5\times$), Aug ($1.0\times$), Other ($0.2\times$)
- `[K]` Kids Songs: Multiplied by global `kid_song_weight` ($10^{-6}$) unless Kids Mode active.

---

### 3.4 Acoustic Transition Flow & 11D Vector Distance ($W_{\text{flow}}$)

Transition smoothness between queue-tail track $C$ and candidate $k$ evaluates normalized Euclidean distance across 11 AcousticBrainz descriptor dimensions:

$$\vec{v} = \left[ \text{danceable}, \text{female}, \text{acoustic}, \text{aggressive}, \text{happy}, \text{party}, \text{relaxed}, \text{sad}, \text{bright}, \text{tonal}, \text{instrumental} \right]$$

$$D_{11\text{D}}(C, k) = \sqrt{\sum_{i=1}^{11} \left( v_{i, C} - v_{i, k} \right)^2}$$

$$W_{\text{flow}}(k) = 1.0 - \frac{\min\left(D_{11\text{D}}(C, k), \sqrt{11}\right)}{\sqrt{11}}$$

---

## 4. Multi-Pass Candidate Pool Refining & Selection Algorithm

### Pass 1: Primary Weight Evaluation
Query all catalog tracks, compute $W(k)$, and build eligible map $M_{\text{eligible}} = \{ k \mid W(k) \ge 0.001 \}$.

### Pass 2: Program Seed Down-Selection
1. Retrieve active program seed tracks $S = \{ s_1, s_2, \dots, s_n \}$ based on current clock time matching program start boundaries ($t_{\text{start}}$).
2. If pool size $|M_{\text{eligible}}| > N_{\text{excl}}$ (default 1000), calculate 11D acoustic distance $D_{11\text{D}}(s, k)$ from candidates to seed tracks.
3. Remove candidates with highest mean distance to seed tracks until pool size equals $N_{\text{excl}}$.

### Pass 3: Queue-Tail Acoustic Flow Re-sorting
1. Identify last track $C_{\text{tail}}$ currently in active `player_queue` (or active playing track if queue empty).
2. Sort remaining candidate pool in ascending order of 11D acoustic distance $D_{11\text{D}}(C_{\text{tail}}, k)$.

### Pass 4: Roulette-Wheel Weighted Selection
1. Take top $N_{\text{rand}}$ (default 100) candidates from sorted flow pool.
2. Apply position-based similarity decay scaling: $W_{\text{final}}(k_i) = W(k_i) \cdot 0.96^i$ for index $i \in [0, N_{\text{rand}}-1]$.
3. Compute total cumulative sum $W_{\text{sum}} = \sum_i W_{\text{final}}(k_i)$.
4. Draw uniform random float $r \in [0, W_{\text{sum}})$.
5. Iterate through candidates accumulating weight until cumulative sum $\ge r$; select and auto-enqueue that candidate track.

---

## 5. Unit Testing Specifications

### Test Case `UT-PD-010`: Hard Rotation Lockout
- **Input**: Track played 30 minutes ago with rotation $rv_{\text{rot}} = 0.0$ ($T_{\text{rot}} = 3600\text{s} = 1\text{ hr}$).
- **Expected Output**: $W_{\text{rec, trk}} = 0.0$, track excluded from candidate pool.

### Test Case `UT-PD-020`: Linear Recovery Ramp
- **Input**: Track played 4 hours ago with $T_{\text{rot}} = 1\text{ hr}$ and $T_{\text{rec}} = 6\text{ hrs}$.
- **Expected Output**: $\Delta t = 4\text{ hrs}$. $W_{\text{rec, trk}} = \frac{4 - 1}{6} = 0.50$.

### Test Case `UT-PD-030`: Restraint Logarithmic Scaling
- **Input**: Track $A$ ($rv_{\text{restraint}} = 0.0$), Track $B$ ($rv_{\text{restraint}} = 0.3$), Track $C$ ($rv_{\text{restraint}} = -0.3$).
- **Expected Output**: $W_{\text{restraint}}(A) = 1.0$, $W_{\text{restraint}}(B) \approx 0.50$, $W_{\text{restraint}}(C) \approx 2.0$.

### Test Case `UT-PD-040`: Christmas Seasonal Weighting
- **Input**: Christmas track tagged `[C]` evaluated on Sep 15, Dec 20, Dec 25.
- **Expected Output**: Sep 15 weight $\le 10^{-6}$; Dec 20 weight $\approx 2.23$; Dec 25 weight $= 10.0$.

### Test Case `UT-PD-050`: Roulette-Wheel Sampling Probability
- **Input**: Pool of 100 candidate tracks with varied weights. Run 10,000 selection trials.
- **Expected Output**: Empirical selection frequencies match candidate relative weight ratios within $\pm 3\%$ margin of error.

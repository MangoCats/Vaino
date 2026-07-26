# SPEC003: Program Director Selection Engine Specification

**Design Specification — Tier 2**

This document specifies the mathematical models, vector distance formulas, time-of-day energy curves, and anti-repetition cooldown algorithms for Vaino's autonomous playlist selector (**"Singing Sorcerer"**).

---

## 1. Mathematical Candidate Scoring Model

For any candidate track $k$ in the music library, the Program Director calculates a total fitness score $S(k)$:

$$S(k) = w_{\text{flow}} \cdot S_{\text{flow}}(k) + w_{\text{time}} \cdot S_{\text{time}}(k) + w_{\text{pref}} \cdot S_{\text{pref}}(k) - P_{\text{repeat}}(k)$$

The track with the highest score $S(k)$ is selected for auto-enqueueing.

---

## 2. Component Scoring Formulas

### 2.1 Acoustic Transition Flow ($S_{\text{flow}}$)
Measures the acoustic feature distance between current track $C$ and candidate $k$:

$$D(C, k) = \sqrt{ \left( \text{energy}_C - \text{energy}_k \right)^2 + \left( \text{valence}_C - \text{valence}_k \right)^2 + \left( \frac{\text{BPM}_C - \text{BPM}_k}{200} \right)^2 }$$

$$S_{\text{flow}}(k) = 1.0 - \min(1.0, D(C, k))$$

### 2.2 Time-of-Day Energy Curve ($S_{\text{time}}$)
Matches candidate energy to a target energy curve $E_{\text{target}}(h)$ for current hour $h \in [0, 23]$:

```
  Energy 1.0 ┌                    ┌───┐ (Afternoon Peak)
             │                   /     \
         0.5 │   ┌───┐ (Morning) /       \       ┌───┐ (Late Ambient)
             │  /     \_________/         \_____/     \
         0.0 └──┴─────┴─────────┴─────────┴─────┴─────┴────
                00:00   06:00     12:00   18:00   24:00 (Hour)
```

$$S_{\text{time}}(k) = 1.0 - \left| E_{\text{target}}(h) - \text{energy}_k \right|$$

### 2.3 Anti-Repetition Cooldown Penalty ($P_{\text{repeat}}$)
Applies exponentially decaying penalties based on elapsed time since candidate $k$ or artist $A(k)$ last played:

$$P_{\text{track}}(k) = 10.0 \cdot e^{-\lambda \cdot \Delta t_{\text{track}}}$$
$$P_{\text{artist}}(k) = 3.0 \cdot e^{-\lambda \cdot \Delta t_{\text{artist}}}$$

$$P_{\text{repeat}}(k) = P_{\text{track}}(k) + P_{\text{artist}}(k)$$

where $\Delta t$ is elapsed hours since last play, and decay factor $\lambda = 0.5$.

---

## 3. Unit Testing Specifications

### Test Case `UT-PD-001`: Transition Smoothness
- **Input**: Current track with $\text{energy}=0.5, \text{BPM}=120$. Candidates $A (\text{energy}=0.55, \text{BPM}=122)$ and $B (\text{energy}=0.95, \text{BPM}=180)$.
- **Expected Output**: $S_{\text{flow}}(A) > S_{\text{flow}}(B)$.

### Test Case `UT-PD-002`: Cooldown Penalty Decay
- **Input**: Track played $0.5\text{ hours}$ ago vs track played $24\text{ hours}$ ago.
- **Expected Output**: Penalty for recent track $> 100 \times$ penalty for 24h old track.

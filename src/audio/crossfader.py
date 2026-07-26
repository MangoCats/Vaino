# src/audio/crossfader.py
"""
[SPEC-AUD-040] Dual-Buffer Crossfader & Mathematical Ramp Curves
Implements Linear, Exponential, and Logistic S-Curve volume transition profiles
for continuous radio-style crossfading between Track A and Track B.
"""

import numpy as np
import logging

logger = logging.getLogger(__name__)

def calculate_ramp(frames: int, profile: str = "S_CURVE", fade_in: bool = True) -> np.ndarray:
    """
    Computes a 1D float32 gain array of length `frames` normalized from 0.0 to 1.0.
    If fade_in is False (fade-out), inverts the curve (1.0 to 0.0).
    """
    if frames <= 0:
        return np.ones(0, dtype=np.float32)

    t = np.linspace(0.0, 1.0, frames, dtype=np.float32)
    profile_upper = (profile or "S_CURVE").upper()

    if profile_upper == "LINEAR":
        gain = t
    elif profile_upper == "EXPONENTIAL":
        gain = t ** 2
    elif profile_upper == "S_CURVE":
        # Logistic S-curve centered at 0.5 with steepness k=6.0
        k = 6.0
        sigmoid = 1.0 / (1.0 + np.exp(-k * (t - 0.5)))
        # Normalize so sigmoid(0.0) == 0.0 and sigmoid(1.0) == 1.0
        s0 = 1.0 / (1.0 + np.exp(k * 0.5))
        s1 = 1.0 / (1.0 + np.exp(-k * 0.5))
        gain = ((sigmoid - s0) / (s1 - s0)).astype(np.float32)
    else:
        gain = t

    if not fade_in:
        gain = 1.0 - gain

    return gain

class DualBufferCrossfader:
    """
    [SPEC-AUD-040] Blends outgoing Track A and incoming Track B PCM sample arrays.
    """
    @staticmethod
    def mix_crossfade(
        buffer_a: np.ndarray,
        buffer_b: np.ndarray,
        profile: str = "S_CURVE"
    ) -> np.ndarray:
        """
        Mixes overlapping frames between buffer_a (fade-out) and buffer_b (fade-in).
        buffer_a and buffer_b MUST have shape (frames, channels).
        """
        frames = min(len(buffer_a), len(buffer_b))
        if frames <= 0:
            return np.zeros((0, buffer_a.shape[1] if buffer_a.ndim > 1 else 2), dtype=np.float32)

        fade_out_ramp = calculate_ramp(frames, profile=profile, fade_in=False)
        fade_in_ramp = calculate_ramp(frames, profile=profile, fade_in=True)

        channels = buffer_a.shape[1] if buffer_a.ndim > 1 else 1
        if channels > 1:
            fade_out_ramp = fade_out_ramp[:, np.newaxis]
            fade_in_ramp = fade_in_ramp[:, np.newaxis]

        mixed = (buffer_a[:frames] * fade_out_ramp) + (buffer_b[:frames] * fade_in_ramp)
        return mixed.astype(np.float32)

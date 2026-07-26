import os
import unittest
import numpy as np
from src.audio.crossfader import calculate_ramp, DualBufferCrossfader
from src.audio.engine import AudioEngine

class TestVainoPhase2(unittest.TestCase):
    def test_ramp_linear(self):
        ramp = calculate_ramp(frames=100, profile="LINEAR", fade_in=True)
        self.assertEqual(len(ramp), 100)
        self.assertAlmostEqual(ramp[0], 0.0, places=4)
        self.assertAlmostEqual(ramp[-1], 1.0, places=4)

    def test_ramp_scurve_midpoint(self):
        """[UT-AUD-002] Verify S-Curve midpoint symmetry alpha(0.5) == 0.5"""
        ramp = calculate_ramp(frames=101, profile="S_CURVE", fade_in=True)
        self.assertEqual(len(ramp), 101)
        self.assertAlmostEqual(ramp[50], 0.5, places=3)

    def test_crossfader_mixing(self):
        """[UT-AUD-003] Test dual-buffer PCM crossfade blending"""
        buf_a = np.ones((100, 2), dtype=np.float32)
        buf_b = np.ones((100, 2), dtype=np.float32)
        mixed = DualBufferCrossfader.mix_crossfade(buf_a, buf_b, profile="LINEAR")
        self.assertEqual(mixed.shape, (100, 2))
        # At midpoint 50%, linear fade out 0.5 + fade in 0.5 == 1.0 total gain
        self.assertAlmostEqual(mixed[50, 0], 1.0, places=3)

    def test_passage_trimming_offsets(self):
        """[REQ-AUD-020] [REQ-AUD-030] Test offset slicing on audio tracks"""
        music_file = r"C:\Users\Mango Cat\Music\Eagles\Hotel_California\(Eagles)Hotel_California-01-Hotel_California.mp3"
        if os.path.exists(music_file):
            track_trimmed = {
                "file_path": music_file,
                "title": "Hotel California Trimmed Segment",
                "start_offset_ms": 10000,  # Start 10s in
                "end_offset_ms": 20000,    # Stop 20s in (10s duration)
                "duration_ms": 10000
            }
            engine = AudioEngine()
            samples, sr, ch = engine._load_audio_file(track_trimmed)
            expected_frames = int(10.0 * sr)
            self.assertAlmostEqual(len(samples), expected_frames, delta=sr * 0.1)

    def test_unicode_pua_path_in_memory_decoding(self):
        """[REQ-AUD-020] Test Python open(rb) in-memory fallback for Unicode/PUA file path decoding"""
        music_file = r"C:\Users\Mango Cat\Music\Various Artists\I Am Sam Music From and Inspired by the Motion Picture\13 - We Can Work It Out.mp3"
        if os.path.exists(music_file):
            track = {
                "file_path": music_file,
                "title": "We Can Work It Out",
                "start_offset_ms": 0,
                "end_offset_ms": None
            }
            engine = AudioEngine()
            samples, sr, ch = engine._load_audio_file(track)
            self.assertGreater(len(samples), 0)
            self.assertEqual(sr, 44100)
            self.assertEqual(ch, 2)

if __name__ == "__main__":
    unittest.main()

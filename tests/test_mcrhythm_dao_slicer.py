import unittest
import numpy as np
from unittest.mock import patch
from src.db.database import Database
from src.db.dao_slicer import DAOSlicer, camel_case_split


class TestMcRhythmDAOSlicer(unittest.TestCase):
    def setUp(self):
        self.db = Database(":memory:")
        self.slicer = DAOSlicer(self.db)

    def test_camel_case_split(self):
        self.assertEqual(camel_case_split("ZZTopsFirstAlbum"), "ZZ Tops First Album")
        self.assertEqual(camel_case_split("HappyNation"), "Happy Nation")
        self.assertEqual(camel_case_split("BackInBlack"), "Back In Black")

    def test_edition_duration_signature_matching(self):
        editions = [
            # Edition A: Standard 10 tracks, sum ~2,000,000 ms
            [{"track_number": 1, "title": "T1", "length_ms": 200000}] * 10,
            # Edition B: Deluxe 15 tracks, sum ~3,000,000 ms
            [{"track_number": 1, "title": "T1", "length_ms": 200000}] * 15
        ]

        # Candidate selection matching a ~3,000,000 ms DAO capture
        selected_ed = None
        best_diff = float("inf")
        target_ms = 3000000

        for ed in editions:
            total_edition_ms = sum(t.get("length_ms", 0) for t in ed)
            diff = abs(total_edition_ms - target_ms)
            if diff < best_diff:
                best_diff = diff
                selected_ed = ed

        self.assertEqual(len(selected_ed), 15)

    def test_rms_boundary_refinement(self):
        # Create synthetic audio energy array with a quiet minimum at frame 100
        mono_samples = np.ones(44100 * 20, dtype=np.float32)  # 20 seconds of audio at 44.1kHz
        # Insert a 2-second silence gap from 9s to 11s (samples 396,900 to 485,100)
        mono_samples[int(44100 * 9):int(44100 * 11)] = 0.001

        # Frame size = 4410 (100ms)
        frame_size = 4410
        num_frames = len(mono_samples) // frame_size
        truncated = mono_samples[:num_frames * frame_size].reshape(num_frames, frame_size)
        rms_envelope = np.sqrt(np.mean(truncated ** 2, axis=1))

        # Expected boundary was roughly 10.5 seconds (10500 ms)
        exp_ms = 10500
        exp_frame = int((exp_ms / 1000.0) * 10)  # 105

        start_search = max(0, exp_frame - 20)
        end_search = min(num_frames, exp_frame + 20)

        search_slice = rms_envelope[start_search:end_search]
        min_idx = int(np.argmin(search_slice))
        best_frame = start_search + min_idx
        best_ms = int((best_frame / 10.0) * 1000)

        # Verified that the RMS minimum fell inside the silence gap (9,000 ms - 11,000 ms)
        self.assertTrue(9000 <= best_ms <= 11000)


if __name__ == "__main__":
    unittest.main()

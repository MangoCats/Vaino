import unittest
import numpy as np
from src.audio.essentia_extractor import EssentiaExtractor


class TestEssentiaExtractor(unittest.TestCase):
    def test_extract_high_level_empty(self):
        empty_samples = np.array([], dtype=np.float32)
        res = EssentiaExtractor.extract_high_level(empty_samples, 44100)
        self.assertIn("gender", res)
        self.assertIn("timbre", res)
        self.assertIn("genre_rosamerica", res)
        self.assertEqual(res["gender"]["female"], 0.5)

    def test_extract_high_level_synthetic_sine(self):
        sample_rate = 44100
        duration_sec = 2.0
        t = np.linspace(0, duration_sec, int(sample_rate * duration_sec), endpoint=False)
        sine_wave = (0.5 * np.sin(2 * np.pi * 440 * t)).astype(np.float32)

        res = EssentiaExtractor.extract_high_level(sine_wave, sample_rate)
        self.assertIn("gender", res)
        self.assertIn("timbre", res)
        self.assertIn("mood_aggressive", res)
        self.assertIn("mood_party", res)
        self.assertIn("genre_rosamerica", res)

        self.assertGreaterEqual(res["gender"]["female"], 0.0)
        self.assertLessEqual(res["gender"]["female"], 1.0)
        self.assertGreaterEqual(res["timbre"]["bright"], 0.0)
        self.assertLessEqual(res["timbre"]["bright"], 1.0)


if __name__ == "__main__":
    unittest.main()

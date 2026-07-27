import unittest
import numpy as np
from src.audio.onnx_extractor import ONNXHighLevelExtractor


class TestONNXExtractor(unittest.TestCase):
    def test_onnx_available(self):
        self.assertTrue(ONNXHighLevelExtractor.is_available())

    def test_extract_descriptors_synthetic_signal(self):
        sample_rate = 44100
        duration_sec = 1.0
        t = np.linspace(0, duration_sec, int(sample_rate * duration_sec), endpoint=False)
        sine_wave = (0.5 * np.sin(2 * np.pi * 220 * t)).astype(np.float32)

        res = ONNXHighLevelExtractor.extract_descriptors(sine_wave, sample_rate)
        self.assertIn("gender", res)
        self.assertIn("timbre", res)
        self.assertIn("mood_aggressive", res)
        self.assertIn("mood_party", res)
        self.assertIn("genre_rosamerica", res)
        self.assertTrue(res.get("is_onnx_model"))

        self.assertGreaterEqual(res["gender"]["female"], 0.0)
        self.assertLessEqual(res["gender"]["female"], 1.0)
        self.assertGreaterEqual(res["timbre"]["bright"], 0.0)
        self.assertLessEqual(res["timbre"]["bright"], 1.0)


if __name__ == "__main__":
    unittest.main()

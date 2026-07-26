import os
import unittest
from src.db.database import Database
from src.db.descriptors import AudioFeatureExtractor
from src.db.fingerprint import AudioFingerprinter

class TestVainoPhase4(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.db_path = "test_phase4.db"
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass
        cls.db = Database(db_path=cls.db_path)
        cls.sample_audio = r"C:\Users\Mango Cat\Music\Eagles\Hotel_California\(Eagles)Hotel_California-01-Hotel_California.mp3"

    @classmethod
    def tearDownClass(cls):
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass

    def test_feature_extraction(self):
        """[REQ-FE-010] Verify LUFS, BPM, energy feature extraction"""
        if os.path.exists(self.sample_audio):
            features = AudioFeatureExtractor.extract_features(self.sample_audio)
            self.assertIn("energy", features)
            self.assertIn("valence", features)
            self.assertIn("tempo_bpm", features)
            self.assertIn("loudness_lufs", features)
            self.assertGreater(features["energy"], 0.0)
            self.assertLessEqual(features["energy"], 1.0)

    def test_descriptor_db_persistence(self):
        """Verify DB persistence of track audio descriptors"""
        dummy_desc = {
            "energy": 0.75,
            "valence": 0.60,
            "danceability": 0.80,
            "acousticness": 0.10,
            "instrumentalness": 0.0,
            "speechiness": 0.05,
            "tempo_bpm": 124.0,
            "key_signature": "E Minor",
            "loudness_lufs": -9.5
        }
        self.db.upsert_track_descriptors("test_track_123", dummy_desc)
        retrieved = self.db.get_track_descriptors("test_track_123")
        self.assertIsNotNone(retrieved)
        self.assertEqual(retrieved["energy"], 0.75)
        self.assertEqual(retrieved["tempo_bpm"], 124.0)

if __name__ == "__main__":
    unittest.main()

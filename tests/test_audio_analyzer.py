# tests/test_audio_analyzer.py
import os
import tempfile
import unittest
import numpy as np
from src.db.database import Database
from src.audio.analyzer import AudioAnalyzer

class TestAudioAnalyzer(unittest.TestCase):
    def setUp(self):
        self.db_fd, self.db_path = tempfile.mkstemp(suffix=".db")
        self.db = Database(self.db_path)

        sample_dir = r"C:\Users\Mango Cat\Music\Eagles\Hotel_California"
        if os.path.exists(sample_dir):
            files = [os.path.join(sample_dir, f) for f in os.listdir(sample_dir) if f.endswith(".mp3")]
            self.audio_path = files[0]
            self.created_audio = False
        else:
            self.audio_fd, self.audio_path = tempfile.mkstemp(suffix=".wav")
            self.created_audio = True

        conn = self.db.get_connection()
        try:
            conn.execute(
                """
                INSERT INTO tracks (id, file_path, title, artist, album, duration_ms, file_format)
                VALUES ('test_tr_001', ?, 'Hotel California', 'Eagles', 'Hotel California', 300000, 'mp3')
                """,
                (self.audio_path,)
            )
            conn.commit()
        finally:
            conn.close()

    def tearDown(self):
        os.close(self.db_fd)
        if hasattr(self, "created_audio") and self.created_audio:
            os.close(self.audio_fd)
            try:
                os.remove(self.audio_path)
            except OSError:
                pass
        try:
            os.remove(self.db_path)
        except OSError:
            pass

    def test_audio_feature_extraction(self):
        """[REQ-AUD-050] Test single audio file feature extraction"""
        features = AudioAnalyzer.analyze_audio_file(self.audio_path)
        self.assertIsNotNone(features)
        self.assertIn("energy", features)
        self.assertIn("valence", features)
        self.assertIn("danceability", features)
        self.assertIn("loudness_lufs", features)
        self.assertIn("tempo_bpm", features)
        self.assertIn("key_signature", features)
        self.assertGreater(features["energy"], 0.0)

    def test_analyze_all_unprocessed_batch(self):
        """[REQ-AUD-050] Test parallel batch feature extraction and DB insertion"""
        analyzer = AudioAnalyzer(self.db)
        success_count, total = analyzer.analyze_all_unprocessed(limit=10, max_workers=2)
        self.assertEqual(success_count, 1)
        self.assertEqual(total, 1)

        # Verify record in track_audio_descriptors
        desc = self.db.get_track_descriptors("test_tr_001")
        self.assertIsNotNone(desc)
        self.assertEqual(desc["track_id"], "test_tr_001")
        self.assertGreater(desc["energy"], 0.0)

if __name__ == "__main__":
    unittest.main()

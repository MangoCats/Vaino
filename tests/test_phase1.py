import os
import unittest
from src.db.database import Database
from src.db.scanner import MediaScanner
from src.audio.engine import AudioEngine

class TestVainoPhase1(unittest.TestCase):
    def setUp(self):
        self.test_db_path = f"test_vaino_{self._testMethodName}.db"
        if os.path.exists(self.test_db_path):
            try:
                os.remove(self.test_db_path)
            except Exception:
                pass
        self.db = Database(db_path=self.test_db_path)
        self.music_dir = r"C:\Users\Mango Cat\Music\Eagles\Hotel_California"

    def tearDown(self):
        del self.db
        if os.path.exists(self.test_db_path):
            try:
                os.remove(self.test_db_path)
            except Exception:
                pass

    def test_database_initialization(self):
        count = self.db.get_total_track_count()
        self.assertEqual(count, 0)

    def test_media_scanner(self):
        if os.path.exists(self.music_dir):
            scanner = MediaScanner(db=self.db, music_dir=self.music_dir)
            count = scanner.scan_directory()
            self.assertGreater(count, 0)
            tracks = self.db.get_all_tracks(limit=10)
            self.assertGreater(len(tracks), 0)
            self.assertIn("Hotel", tracks[0]["title"])

    def test_audio_engine_status(self):
        engine = AudioEngine()
        status = engine.get_status()
        self.assertEqual(status["state"], "IDLE")
        self.assertEqual(status["volume"], 80)

if __name__ == "__main__":
    unittest.main()

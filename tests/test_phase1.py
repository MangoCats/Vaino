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
            total, updated, skipped = scanner.scan_directory()
            self.assertGreater(total, 0)
            tracks = self.db.get_all_tracks(limit=10)
            self.assertGreater(len(tracks), 0)
            self.assertIn("Hotel", tracks[0]["title"])

    def test_audio_engine_status(self):
        engine = AudioEngine()
        status = engine.get_status()
        self.assertEqual(status["state"], "IDLE")
        self.assertEqual(status["volume"], 80)

    def test_incremental_scan_benchmark(self):
        """[REQ-DB-020] Benchmark incremental scanning (<0.1s re-scan on cached files)"""
        import time
        if os.path.exists(self.music_dir):
            scanner = MediaScanner(db=self.db, music_dir=self.music_dir)
            # Initial scan -> populates file_mtime / file_size cache
            total, updated1, skipped1 = scanner.scan_directory()
            self.assertGreater(total, 0)

            # Benchmark second scan -> all files should be skipped via fast mtime/size check
            t_start = time.time()
            total2, updated2, skipped2 = scanner.scan_directory()
            elapsed = time.time() - t_start

            self.assertEqual(updated2, 0)
            self.assertEqual(skipped2, total)
            self.assertLess(elapsed, 0.1, f"Incremental rescan took {elapsed:.3f}s, exceeding 0.1s threshold")

if __name__ == "__main__":
    unittest.main()

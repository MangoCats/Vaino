import os
import unittest
import sqlite3
import tempfile
from src.db.database import Database
from src.db.acoustic_resolver import AcousticResolver
from src.audio.selector import ProgramDirector

class TestAcousticResolver(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self.tmp_dir.name, "test_vaino.db")
        self.mulib_path = os.path.join(self.tmp_dir.name, "test_mulib.db")

        # Initialize vaino test db
        self.db = Database(self.db_path)
        
        # Populate track in vaino db
        self.db.upsert_track({
            "id": "t1",
            "file_path": "Artist\\Album\\Track1.mp3",
            "file_format": "mp3",
            "title": "Test Track 1",
            "artist": "Test Artist",
            "album": "Test Album",
            "duration_ms": 180000,
            "musicbrainz_track_id": "mbid-12345"
        })

        # Initialize dummy mulib.db
        conn = sqlite3.connect(self.mulib_path)
        conn.execute("""
            CREATE TABLE tracks (
                id INTEGER PRIMARY KEY,
                mbidRecording TEXT,
                path TEXT,
                abAcoustic REAL, abAggressive REAL, abBright REAL, abDanceable REAL,
                abFemale REAL, abHappy REAL, abInstrumental REAL, abParty REAL,
                abRelaxed REAL, abSad REAL, abTonal REAL
            )
        """)
        conn.execute("""
            INSERT INTO tracks (mbidRecording, path, abAcoustic, abAggressive, abBright, abDanceable, abFemale, abHappy, abInstrumental, abParty, abRelaxed, abSad, abTonal)
            VALUES ('mbid-12345', 'Artist\\\\Album\\\\Track1.mp3', 0.85, 0.12, 0.65, 0.78, 0.92, 0.80, 0.05, 0.75, 0.60, 0.10, 0.88)
        """)
        conn.commit()
        conn.close()

    def tearDown(self):
        self.tmp_dir.cleanup()

    def test_tier1_mulib_transfer(self):
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        cur = conn.execute("SELECT * FROM tracks WHERE id = 't1'")
        row = cur.fetchone()

        res = AcousticResolver.resolve_track(row, conn, self.mulib_path)
        conn.close()

        self.assertIsNotNone(res)
        self.assertAlmostEqual(res["ab_acoustic"], 0.85)
        self.assertAlmostEqual(res["ab_female"], 0.92)

    def test_program_director_11d_distance(self):
        pd = ProgramDirector(self.db)
        desc1 = {
            "ab_acoustic": 0.8, "ab_aggressive": 0.1, "ab_bright": 0.6, "ab_danceable": 0.7,
            "ab_female": 0.9, "ab_happy": 0.8, "ab_instrumental": 0.1, "ab_party": 0.7,
            "ab_relaxed": 0.6, "ab_sad": 0.1, "ab_tonal": 0.8
        }
        desc2 = dict(desc1) # Identical -> distance = 0.0
        dist = pd.compute_acoustic_distance(desc1, desc2)
        self.assertAlmostEqual(dist, 0.0)

        desc3 = {
            "ab_acoustic": 0.2, "ab_aggressive": 0.8, "ab_bright": 0.1, "ab_danceable": 0.2,
            "ab_female": 0.1, "ab_happy": 0.1, "ab_instrumental": 0.9, "ab_party": 0.1,
            "ab_relaxed": 0.1, "ab_sad": 0.8, "ab_tonal": 0.2
        }
        dist_diff = pd.compute_acoustic_distance(desc1, desc3)
        self.assertGreater(dist_diff, 0.4)

if __name__ == "__main__":
    unittest.main()

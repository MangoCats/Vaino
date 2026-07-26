import os
import unittest
from src.db.database import Database
from src.audio.selector import ProgramDirector

class TestVainoPhase5(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.db_path = "test_phase5.db"
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass
        cls.db = Database(db_path=cls.db_path)
        cls.director = ProgramDirector(db=cls.db)

        # Seed test tracks
        cls.track_a = {
            "id": "track_a_001",
            "file_path": r"C:\path\to\track_a.mp3",
            "file_format": "MP3",
            "title": "Ambient Mellow Morning",
            "artist": "Artist Alpha",
            "album": "Album Alpha",
            "duration_ms": 180000
        }
        cls.track_b = {
            "id": "track_b_002",
            "file_path": r"C:\path\to\track_b.mp3",
            "file_format": "MP3",
            "title": "High Energy Afternoon Peak",
            "artist": "Artist Beta",
            "album": "Album Beta",
            "duration_ms": 200000
        }
        cls.db.upsert_track(cls.track_a)
        cls.db.upsert_track(cls.track_b)

        cls.db.upsert_track_descriptors("track_a_001", {"energy": 0.30, "valence": 0.40, "tempo_bpm": 85.0})
        cls.db.upsert_track_descriptors("track_b_002", {"energy": 0.85, "valence": 0.80, "tempo_bpm": 128.0})

    @classmethod
    def tearDownClass(cls):
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass

    def test_target_energy_curves(self):
        """[SPEC-PD-010] Verify time-of-day target energy curve mapping"""
        self.assertEqual(self.director.get_target_energy_for_hour(3), 0.30)   # Late night mellow
        self.assertEqual(self.director.get_target_energy_for_hour(14), 0.85)  # Afternoon peak

    def test_transition_flow_scoring(self):
        """[UT-PD-001] Test acoustic feature distance math"""
        desc_1 = {"energy": 0.50, "valence": 0.50, "tempo_bpm": 120.0}
        desc_similar = {"energy": 0.52, "valence": 0.51, "tempo_bpm": 122.0}
        desc_distant = {"energy": 0.95, "valence": 0.10, "tempo_bpm": 185.0}

        dist_similar = self.director.compute_acoustic_distance(desc_1, desc_similar)
        dist_distant = self.director.compute_acoustic_distance(desc_1, desc_distant)

        self.assertLess(dist_similar, dist_distant)

    def test_cooldown_penalty_decay(self):
        """[UT-PD-002] Verify exponential decay penalty formula"""
        history = [{"track_id": "track_a_001"}]
        penalty = self.director.calculate_cooldown_penalty(self.track_a, history)
        self.assertGreater(penalty, 5.0)

    def test_autonomous_track_selection(self):
        """[REQ-PD-010] Test Program Director auto-selection for afternoon peak (14:00)"""
        selected = self.director.select_next_track(
            current_track=self.track_a,
            candidate_pool=[self.track_a, self.track_b],
            current_hour=14
        )
        self.assertIsNotNone(selected)
        # Should select Track B because energy=0.85 matches 14:00 afternoon target 0.85
        self.assertEqual(selected["id"], "track_b_002")

if __name__ == "__main__":
    unittest.main()

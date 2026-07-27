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

    def test_musicbrainz_resolver_mock(self):
        """[REQ-MB-020A, REQ-MB-020B, REQ-MB-020C] Verify MusicBrainz identifier resolution and database linkage"""
        from src.db.resolver import MusicBrainzResolver
        from unittest.mock import patch

        # Insert a track lacking MBID
        self.db.upsert_track({
            "id": "mb_test_01",
            "file_path": r"C:\music\eagles_test.mp3",
            "file_format": "MP3",
            "title": "Hotel California",
            "artist": "Eagles",
            "album": "Hotel California",
            "duration_ms": 391000
        })

        resolver = MusicBrainzResolver(self.db)
        
        # Mock extract_embedded_mbid to simulate finding MBID via ID3 tag or AcoustID lookup
        mock_mbid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
        mock_sort = "Eagles, The"

        with patch.object(resolver, "extract_embedded_mbid", return_value=mock_mbid):
            with patch.object(resolver, "extract_embedded_artist_sort", return_value=mock_sort):
                resolved_count, skipped_count = resolver.resolve_all_unlinked(limit=100)
                self.assertGreaterEqual(resolved_count, 1)

        # Verify DB row updated with resolved MBID and artist_sort_name
        track = self.db.get_track_by_id("mb_test_01")
        self.assertEqual(track["musicbrainz_track_id"], mock_mbid)
        self.assertEqual(track["artist_sort_name"], "Eagles, The")

if __name__ == "__main__":
    unittest.main()

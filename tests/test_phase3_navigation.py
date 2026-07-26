import os
import unittest
from src.db.database import Database

class TestVainoPhase3Navigation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.db_path = "test_phase3_nav.db"
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass
        cls.db = Database(db_path=cls.db_path)

        # Seed sample tracks
        cls.db.upsert_track({
            "id": "t1", "file_path": r"C:\music\eagles_01.mp3", "file_format": "MP3",
            "title": "Hotel California", "artist": "Eagles", "album": "Hotel California",
            "year": 1976, "track_number": 1, "duration_ms": 391000
        })
        cls.db.upsert_track({
            "id": "t2", "file_path": r"C:\music\eagles_02.mp3", "file_format": "MP3",
            "title": "New Kid in Town", "artist": "Eagles", "album": "Hotel California",
            "year": 1976, "track_number": 2, "duration_ms": 304000
        })
        cls.db.upsert_track({
            "id": "t3", "file_path": r"C:\music\beatles_01.mp3", "file_format": "MP3",
            "title": "Come Together", "artist": "The Beatles", "album": "Abbey Road",
            "year": 1969, "track_number": 1, "duration_ms": 259000
        })

    @classmethod
    def tearDownClass(cls):
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass

    def test_get_all_artists(self):
        """[REQ-UI-020A] Test artist list grouping and album counts"""
        artists = self.db.get_all_artists()
        self.assertEqual(len(artists), 2)
        artist_names = [a["artist"] for a in artists]
        self.assertIn("Eagles", artist_names)
        self.assertIn("The Beatles", artist_names)

    def test_get_all_albums(self):
        """[REQ-UI-020A] Test album grid grouping with year and artist"""
        albums = self.db.get_all_albums()
        self.assertEqual(len(albums), 2)
        album_names = [al["album"] for al in albums]
        self.assertIn("Hotel California", album_names)
        self.assertIn("Abbey Road", album_names)

    def test_album_tracks_sorted_by_track_number(self):
        """[REQ-UI-020B] Test album tracklist sorting by track_number"""
        tracks = self.db.get_album_tracks("Hotel California")
        self.assertEqual(len(tracks), 2)
        self.assertEqual(tracks[0]["track_number"], 1)
        self.assertEqual(tracks[1]["track_number"], 2)
        self.assertEqual(tracks[0]["title"], "Hotel California")
        self.assertEqual(tracks[1]["title"], "New Kid in Town")

if __name__ == "__main__":
    unittest.main()

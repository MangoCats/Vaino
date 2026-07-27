import unittest
from unittest.mock import patch, MagicMock
from src.db.database import Database
from src.db.dao_slicer import DAOSlicer


class TestDAOSlicer(unittest.TestCase):
    def setUp(self):
        self.db = Database(":memory:")
        self.slicer = DAOSlicer(self.db)

    @patch("src.db.dao_slicer.DAOSlicer.fetch_musicbrainz_release_tracklist")
    def test_slice_dao_file(self, mock_tracklist):
        mock_tracklist.return_value = [
            {"track_number": 1, "title": "1984", "length_ms": 67000, "recording_mbid": "rec-1", "release_mbid": "rel-1"},
            {"track_number": 2, "title": "Jump", "length_ms": 241000, "recording_mbid": "rec-2", "release_mbid": "rel-1"},
            {"track_number": 3, "title": "Panama", "length_ms": 212000, "recording_mbid": "rec-3", "release_mbid": "rel-1"}
        ]

        dao_file = {
            "id": "dao_vh_1984",
            "file_path": "C:\\Music\\Van Halen\\1984.mp3",
            "file_format": "mp3",
            "title": "1984",
            "artist": "Van Halen",
            "album": "1984",
            "year": 1984,
            "duration_ms": 2009000,
            "file_size": 50000000,
            "has_cover_art": 1,
            "file_mtime": 1000.0
        }

        # Insert parent raw unsliced track into DB
        self.db.upsert_track(dao_file)

        passages = self.slicer.slice_dao_file(dao_file)
        self.assertEqual(passages, 3)

        # Check album tracks returned by DB
        tracks = self.db.get_album_tracks("1984", "Van Halen")
        self.assertEqual(len(tracks), 3)

        # Track 1
        t1 = tracks[0]
        self.assertEqual(t1["title"], "1984")
        self.assertEqual(t1["track_number"], 1)
        self.assertEqual(t1["start_offset_ms"], 0)
        self.assertEqual(t1["end_offset_ms"], 67000)
        self.assertEqual(t1["musicbrainz_track_id"], "rec-1")

        # Track 2 (Jump)
        t2 = tracks[1]
        self.assertEqual(t2["title"], "Jump")
        self.assertEqual(t2["track_number"], 2)
        self.assertEqual(t2["start_offset_ms"], 67000)
        self.assertEqual(t2["end_offset_ms"], 308000)
        self.assertEqual(t2["musicbrainz_track_id"], "rec-2")


if __name__ == "__main__":
    unittest.main()

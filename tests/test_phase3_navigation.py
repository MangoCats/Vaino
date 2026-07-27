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
            "title": "Hotel California", "artist": "Eagles", "artist_sort_name": "Eagles", "album": "Hotel California",
            "year": 1976, "track_number": 1, "duration_ms": 391000
        })
        cls.db.upsert_track({
            "id": "t2", "file_path": r"C:\music\eagles_02.mp3", "file_format": "MP3",
            "title": "New Kid in Town", "artist": "Eagles", "artist_sort_name": "Eagles", "album": "Hotel California",
            "year": 1976, "track_number": 2, "duration_ms": 304000
        })
        cls.db.upsert_track({
            "id": "t3", "file_path": r"C:\music\beatles_01.mp3", "file_format": "MP3",
            "title": "Come Together", "artist": "The Beatles", "artist_sort_name": "Beatles, The", "album": "Abbey Road",
            "year": 1969, "track_number": 1, "duration_ms": 259000
        })
        cls.db.upsert_track({
            "id": "t4", "file_path": r"C:\music\bruce_01.mp3", "file_format": "MP3",
            "title": "Born to Run", "artist": "Bruce Springsteen", "artist_sort_name": "Springsteen, Bruce",
            "album": "Born to Run", "year": 1975, "track_number": 1, "duration_ms": 270000
        })
        cls.db.upsert_track({
            "id": "t5", "file_path": r"C:\music\santana_smooth.mp3", "file_format": "MP3",
            "title": "Smooth", "artist": "Santana feat. Rob Thomas", "album": "Supernatural",
            "year": 1999, "track_number": 1, "duration_ms": 295000
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
        artist_names = [a["artist"] for a in artists]
        self.assertIn("Eagles", artist_names)
        self.assertIn("The Beatles", artist_names)
        self.assertIn("Bruce Springsteen", artist_names)
        self.assertIn("Santana", artist_names)
        self.assertIn("Rob Thomas", artist_names)
        self.assertNotIn("Santana feat. Rob Thomas", artist_names)

    def test_get_all_albums(self):
        """[REQ-UI-020A] Test album grid grouping with year and artist"""
        albums = self.db.get_all_albums()
        self.assertGreaterEqual(len(albums), 4)
        album_names = [al["album"] for al in albums]
        self.assertIn("Hotel California", album_names)
        self.assertIn("Abbey Road", album_names)
        self.assertIn("Born to Run", album_names)
        self.assertIn("Supernatural", album_names)

    def test_album_tracks_sorted_by_track_number(self):
        """[REQ-UI-020B] Test album tracklist sorting by track_number"""
        tracks = self.db.get_album_tracks("Hotel California")
        self.assertEqual(len(tracks), 2)
        self.assertEqual(tracks[0]["track_number"], 1)
        self.assertEqual(tracks[1]["track_number"], 2)

    def test_artist_sort_name_keying(self):
        """[REQ-MB-020D] Test artist_sort_name letter bar keying (Bruce Springsteen -> S, The Beatles -> B)"""
        artists_s = self.db.get_all_artists(letter="S")
        s_names = [a["artist"] for a in artists_s]
        self.assertIn("Bruce Springsteen", s_names)
        self.assertIn("Santana", s_names)

        artists_b = self.db.get_all_artists(letter="B")
        b_names = [a["artist"] for a in artists_b]
        self.assertIn("The Beatles", b_names)

    def test_individual_artist_decomposition(self):
        """[REQ-MB-020E, REQ-UI-020G] Test decomposition of 'Santana feat. Rob Thomas' into Santana and Rob Thomas portfolio entries"""
        albums_rt = self.db.get_all_albums(artist="Rob Thomas")
        self.assertEqual(len(albums_rt), 1)
        self.assertEqual(albums_rt[0]["album"], "Supernatural")

    def test_album_deduplication_by_title(self):
        """[REQ-UI-020G] Test that albums with varied track artist strings are grouped into a single album tile"""
        # Seed two tracks on same album with different raw track artist strings
        self.db.upsert_track({
            "id": "t6", "file_path": r"C:\music\rarities_01.mp3", "file_format": "MP3",
            "title": "Song A", "artist": "Sarah McLachlan", "album": "Rarities, B-Sides",
            "year": 1996, "track_number": 1, "duration_ms": 200000
        })
        self.db.upsert_track({
            "id": "t7", "file_path": r"C:\music\rarities_02.mp3", "file_format": "MP3",
            "title": "Song B", "artist": "Sarah McLachlan & Cyndi Lauper", "album": "Rarities, B-Sides",
            "year": 1996, "track_number": 2, "duration_ms": 210000
        })

        albums_sm = self.db.get_all_albums(artist="Sarah McLachlan")
        rarities_tiles = [al for al in albums_sm if al["album"] == "Rarities, B-Sides"]
        self.assertEqual(len(rarities_tiles), 1, "Rarities album was split into multiple duplicate tiles")
    def test_diacritic_normalization_motley_crue(self):
        """[REQ-UI-020I] Test that Mötley Crüe with diacritics appears under letter filter M"""
        self.db.upsert_track({
            "id": "t8", "file_path": r"C:\music\motley_01.mp3", "file_format": "MP3",
            "title": "Kickstart My Heart", "artist": "Mötley Crüe", "album": "Dr. Feelgood",
            "year": 1989, "track_number": 1, "duration_ms": 284000
        })
        artists_m = self.db.get_all_artists(letter="M")
        m_names = [a["artist"] for a in artists_m]
    def test_simple_minds_as_is_fallback(self):
        """[REQ-UI-020J] Test that artist without MusicBrainz sort tag uses name as-is (Simple Minds -> S)"""
        self.db.upsert_track({
            "id": "t9", "file_path": r"C:\music\simple_minds_01.mp3", "file_format": "MP3",
            "title": "Don't You (Forget About Me)", "artist": "Simple Minds", "album": "Once Upon a Time",
            "year": 1985, "track_number": 1, "duration_ms": 260000
        })
        artists_s = self.db.get_all_artists(letter="S")
        s_names = [a["artist"] for a in artists_s]
        self.assertIn("Simple Minds", s_names)

        artists_m = self.db.get_all_artists(letter="M")
        m_names = [a["artist"] for a in artists_m]
        self.assertNotIn("Simple Minds", m_names)

    def test_album_and_track_article_stripping(self):
        """[REQ-UI-020I, REQ-UI-020J] Test article stripping ('The', 'A', 'An') for album and track sort names"""
        from src.db.scanner import compute_sort_name
        self.db.upsert_track({
            "id": "t10", "file_path": r"C:\music\pink_floyd_01.mp3", "file_format": "MP3",
            "title": "Money", "title_sort_name": compute_sort_name("Money"),
            "artist": "Pink Floyd", "artist_sort_name": compute_sort_name("Pink Floyd"),
            "album": "The Dark Side of the Moon", "album_sort_name": compute_sort_name("The Dark Side of the Moon"),
            "year": 1973, "track_number": 6, "duration_ms": 382000
        })
        self.db.upsert_track({
            "id": "t11", "file_path": r"C:\music\beatles_hard.mp3", "file_format": "MP3",
            "title": "A Hard Day's Night", "title_sort_name": compute_sort_name("A Hard Day's Night"),
            "artist": "The Beatles", "artist_sort_name": compute_sort_name("The Beatles"),
            "album": "A Hard Day's Night", "album_sort_name": compute_sort_name("A Hard Day's Night"),
            "year": 1964, "track_number": 1, "duration_ms": 154000
        })
        self.db.upsert_track({
            "id": "t12", "file_path": r"C:\music\john_evening.mp3", "file_format": "MP3",
            "title": "An Evening With...", "title_sort_name": compute_sort_name("An Evening With..."),
            "artist": "John Denver", "artist_sort_name": compute_sort_name("John Denver"),
            "album": "An Evening With John Denver", "album_sort_name": compute_sort_name("An Evening With John Denver"),
            "year": 1975, "track_number": 1, "duration_ms": 240000
        })

        # "The Dark Side of the Moon" -> album_sort_name "Dark Side of the Moon, The" -> appears under D, NOT under T
        albums_d = self.db.get_all_albums(letter="D")
        d_album_names = [al["album"] for al in albums_d]
        self.assertIn("The Dark Side of the Moon", d_album_names)

        albums_t = self.db.get_all_albums(letter="T")
        t_album_names = [al["album"] for al in albums_t]
        self.assertNotIn("The Dark Side of the Moon", t_album_names)

        # "A Hard Day's Night" -> title_sort_name "Hard Day's Night, A" -> appears under H
        tracks_h = self.db.get_all_tracks(letter="H")
        h_track_titles = [tr["title"] for tr in tracks_h]
        self.assertIn("A Hard Day's Night", h_track_titles)

        # "An Evening With..." -> title_sort_name "Evening With..., An" -> appears under E
        tracks_e = self.db.get_all_tracks(letter="E")
        e_track_titles = [tr["title"] for tr in tracks_e]
        self.assertIn("An Evening With...", e_track_titles)

    def test_filtered_track_count_matches_get_all_tracks(self):
        """[REQ-UI-020K] Test that get_total_track_count matches len(get_all_tracks) exactly across letter/artist/query filters"""
        # Letter filter
        count_o = self.db.get_total_track_count(letter="O")
        tracks_o = self.db.get_all_tracks(limit=1000, letter="O")
        self.assertEqual(count_o, len(tracks_o))

        # Artist filter
        count_e = self.db.get_total_track_count(artist="Eagles")
        tracks_e = self.db.get_all_tracks(limit=1000, artist="Eagles")
        self.assertEqual(count_e, len(tracks_e))

if __name__ == "__main__":
    unittest.main()

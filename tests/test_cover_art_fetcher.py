import unittest
from unittest.mock import patch, MagicMock
from src.db.database import Database
from src.db.cover_art_fetcher import CoverArtFetcher


class TestCoverArtFetcher(unittest.TestCase):
    def setUp(self):
        self.db = Database(":memory:")
        self.fetcher = CoverArtFetcher(self.db)

    def test_save_and_get_album_cover_art(self):
        sample_png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01"
        album_id = self.db.save_album_cover_art(
            album_name="Hotel California",
            artist_name="Eagles",
            image_bytes=sample_png,
            mime_type="image/png",
            source="TEST"
        )
        self.assertTrue(album_id)

        res = self.db.get_album_cover_art("Hotel California", "Eagles")
        self.assertIsNotNone(res)
        img_data, mime = res
        self.assertEqual(img_data, sample_png)
        self.assertEqual(mime, "image/png")

    @patch("src.db.cover_art_fetcher.CoverArtFetcher._safe_http_get")
    def test_fetch_from_cover_art_archive_mock(self, mock_get):
        mock_png = b"\x89PNG\r\n\x1a\n" + b"x" * 1500
        mock_get.return_value = (mock_png, "image/png")

        res = self.fetcher.fetch_from_cover_art_archive("mock-mbid-12345")
        self.assertIsNotNone(res)
        img_bytes, mime = res
        self.assertEqual(img_bytes, mock_png)
        self.assertEqual(mime, "image/png")


if __name__ == "__main__":
    unittest.main()

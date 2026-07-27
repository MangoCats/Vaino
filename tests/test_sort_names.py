import unittest
from src.db.scanner import compute_sort_name, compute_artist_sort_name


class TestSortNames(unittest.TestCase):
    def test_strip_leading_special_chars_and_uppercase(self):
        # Leading quotes (strips ONLY from the front of the name string)
        self.assertEqual(compute_sort_name("'Hello'"), "HELLO'")
        self.assertEqual(compute_sort_name('"World"'), 'WORLD"')
        self.assertEqual(compute_sort_name("`Test`"), "TEST`")

        # Leading parentheses, brackets, braces (strips ONLY from the front of the name string)
        self.assertEqual(compute_sort_name("(The) Dark Side of the Moon"), "DARK SIDE OF THE MOON, THE")
        self.assertEqual(compute_sort_name("[1999] Party"), "1999] PARTY")
        self.assertEqual(compute_sort_name("{Special} Song"), "SPECIAL} SONG")

        # Leading punctuation (periods, commas, spaces)
        self.assertEqual(compute_sort_name("...And Justice for All"), "JUSTICE FOR ALL, AND")
        self.assertEqual(compute_sort_name("  The Beatles  "), "BEATLES, THE")
        self.assertEqual(compute_sort_name(",,,Simple Minds"), "SIMPLE MINDS")

    def test_diacritic_normalization_and_uppercase(self):
        self.assertEqual(compute_sort_name("Mötley Crüe"), "MOTLEY CRUE")
        self.assertEqual(compute_sort_name("Beyoncé"), "BEYONCE")

    def test_artist_sort_name(self):
        self.assertEqual(compute_artist_sort_name("Simple Minds"), "SIMPLE MINDS")
        self.assertEqual(compute_artist_sort_name("The Doors"), "DOORS, THE")
        self.assertEqual(compute_artist_sort_name("Bruce Springsteen", "Springsteen, Bruce"), "SPRINGSTEEN, BRUCE")


if __name__ == "__main__":
    unittest.main()

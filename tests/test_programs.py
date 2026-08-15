import os
import unittest
import tempfile
import sqlite3
from src.db.database import Database
from src.audio.selector import ProgramDirector

class TestPrograms(unittest.TestCase):
    def setUp(self):
        self.temp_db_fd, self.temp_db_path = tempfile.mkstemp(suffix=".db")
        self.db = Database(db_path=self.temp_db_path)
        self.director = ProgramDirector(self.db)

    def tearDown(self):
        os.close(self.temp_db_fd)
        if os.path.exists(self.temp_db_path):
            os.remove(self.temp_db_path)

    def test_program_crud_operations(self):
        # Create program
        p1 = self.db.save_program("Morning Mellow", "06:00", "track_1\ntrack_2")
        self.assertIsNotNone(p1)
        self.assertEqual(p1["name"], "Morning Mellow")
        self.assertEqual(p1["start_time"], "06:00")
        self.assertEqual(p1["track_ids"], "track_1\ntrack_2")

        # Get all programs
        progs = self.db.get_all_programs()
        self.assertEqual(len(progs), 1)

        # Update program
        updated = self.db.update_program(p1["id"], "Morning Warmup", "07:00", "track_1")
        self.assertEqual(updated["name"], "Morning Warmup")
        self.assertEqual(updated["start_time"], "07:00")

        # Delete program
        deleted = self.db.delete_program(p1["id"])
        self.assertTrue(deleted)
        self.assertEqual(len(self.db.get_all_programs()), 0)

    def test_active_program_time_slot_matching(self):
        # Setup 4 time slots
        self.db.save_program("Soft", "04:00")
        self.db.save_program("Cool", "12:00")
        self.db.save_program("Prog", "19:00")
        self.db.save_program("Mellow", "22:00")

        # Test at 05:30 -> Soft
        p1 = self.director.get_active_program("05:30")
        self.assertEqual(p1["name"], "Soft")

        # Test at 14:15 -> Cool
        p2 = self.director.get_active_program("14:15")
        self.assertEqual(p2["name"], "Cool")

        # Test at 21:00 -> Prog
        p3 = self.director.get_active_program("21:00")
        self.assertEqual(p3["name"], "Prog")

        # Test midnight boundary: at 02:00 -> Mellow (22:00)
        p4 = self.director.get_active_program("02:00")
        self.assertEqual(p4["name"], "Mellow")

if __name__ == "__main__":
    unittest.main()

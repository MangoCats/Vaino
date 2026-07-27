import os
import unittest
from src.db.database import Database
from src.audio.engine import AudioEngine

class TestVainoPhase4Queue(unittest.TestCase):
    def setUp(self):
        self.db_path = "test_queue.db"
        if os.path.exists(self.db_path):
            try: os.remove(self.db_path)
            except Exception: pass
        self.db = Database(self.db_path)
        self.engine = AudioEngine(db=None)

        # Seed test tracks
        self.t1 = {"id": "q1", "file_format": "MP3", "title": "Hotel California", "artist": "Eagles", "album": "Hotel California", "track_number": 1, "duration_ms": 390000, "file_path": r"C:\music\eagles1.mp3"}
        self.t2 = {"id": "q2", "file_format": "MP3", "title": "New Kid in Town", "artist": "Eagles", "album": "Hotel California", "track_number": 2, "duration_ms": 304000, "file_path": r"C:\music\eagles2.mp3"}
        self.t3 = {"id": "q3", "file_format": "MP3", "title": "Life in the Fast Lane", "artist": "Eagles", "album": "Hotel California", "track_number": 3, "duration_ms": 286000, "file_path": r"C:\music\eagles3.mp3"}
        self.t4 = {"id": "q4", "file_format": "MP3", "title": "Wasted Time", "artist": "Eagles", "album": "Hotel California", "track_number": 4, "duration_ms": 295000, "file_path": r"C:\music\eagles4.mp3"}

        self.db.upsert_track(self.t1)
        self.db.upsert_track(self.t2)
        self.db.upsert_track(self.t3)
        self.db.upsert_track(self.t4)

    def tearDown(self):
        if os.path.exists(self.db_path):
            try: os.remove(self.db_path)
            except Exception: pass

    def test_enqueue_single_track_and_priority(self):
        """[REQ-QUE-010, REQ-QUE-020] Test enqueuing single tracks to end and play next (priority index 0)"""
        self.engine.enqueue_track(self.t1, play_next=False)
        # First track becomes current_track
        self.assertEqual(self.engine.current_track["id"], "q1")
        self.assertEqual(len(self.engine.queue), 0)

        # Add t2 to end
        self.engine.enqueue_track(self.t2, play_next=False)
        self.assertEqual(len(self.engine.queue), 1)
        self.assertEqual(self.engine.queue[0]["id"], "q2")

        # Add t3 as Play Next (inserts at index 0)
        self.engine.enqueue_track(self.t3, play_next=True)
        self.assertEqual(len(self.engine.queue), 2)
        self.assertEqual(self.engine.queue[0]["id"], "q3")
        self.assertEqual(self.engine.queue[1]["id"], "q2")

    def test_enqueue_entire_album(self):
        """[REQ-QUE-020] Test enqueuing an entire album in track_number order"""
        tracks = [self.t4, self.t2, self.t1, self.t3]  # Out of order
        self.engine.enqueue_album(tracks, play_next=False)

        # First track (t1, track_number=1) becomes current_track
        self.assertEqual(self.engine.current_track["id"], "q1")
        # Remaining in queue: t2, t3, t4
        queued_ids = [t["id"] for t in self.engine.queue]
        self.assertEqual(queued_ids, ["q2", "q3", "q4"])

    def test_queue_reorder_and_remove(self):
        """[REQ-QUE-030] Test moving and removing items in queue"""
        self.engine.current_track = self.t1
        self.engine.queue = [dict(self.t2), dict(self.t3), dict(self.t4)]

        # Move t4 (index 2) to top (index 0)
        moved = self.engine.move_in_queue(from_index=2, to_index=0)
        self.assertTrue(moved)
        queued_ids = [t["id"] for t in self.engine.queue]
        self.assertEqual(queued_ids, ["q4", "q2", "q3"])

        # Remove t2 (index 1)
        removed = self.engine.remove_from_queue(index=1)
        self.assertTrue(removed)
        queued_ids = [t["id"] for t in self.engine.queue]
        self.assertEqual(queued_ids, ["q4", "q3"])

        # Clear queue
        self.engine.clear_queue()
        self.assertEqual(len(self.engine.queue), 0)

    def test_track_history_and_skip_back(self):
        """[REQ-QUE-040] Test history stack memory and skip_back() previous track retrieval"""
        self.engine.current_track = self.t1
        self.engine.queue = [dict(self.t2), dict(self.t3)]

        # Advance to t2 (skip)
        self.engine.history_stack.append(self.t1)
        self.engine.current_track = self.engine.queue.pop(0)
        self.assertEqual(self.engine.current_track["id"], "q2")
        self.assertEqual(len(self.engine.history_stack), 1)

        # Skip back to t1
        self.engine.skip_back()
        self.assertEqual(self.engine.current_track["id"], "q1")
        self.assertEqual(len(self.engine.history_stack), 0)
        # t2 should be back at index 0 of queue
        self.assertEqual(self.engine.queue[0]["id"], "q2")

    def test_large_queue_clear_and_status_sync(self):
        """[REQ-QUE-030] Test queue behavior with 500 items and clearing all items"""
        large_queue = [{"id": f"q_{i}", "file_format": "MP3", "title": f"Track {i}", "artist": "Various", "album": "Compilation", "duration_ms": 180000, "file_path": r"C:\music\track.mp3"} for i in range(500)]
        self.engine.current_track = large_queue[0]
        self.engine.queue = large_queue[1:]

        self.assertEqual(len(self.engine.queue), 499)
        status = self.engine.get_status()
        self.assertEqual(status["queue_length"], 499)

        # Clear queue
        self.engine.clear_queue()
        self.assertEqual(len(self.engine.queue), 0)
        status_cleared = self.engine.get_status()
        self.assertEqual(status_cleared["queue_length"], 0)

if __name__ == "__main__":
    unittest.main()

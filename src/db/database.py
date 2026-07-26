import sqlite3
import os
from typing import List, Dict, Any, Optional

class Database:
    def __init__(self, db_path: str = "vaino.db"):
        self.db_path = db_path
        self._init_db()

    def get_connection(self) -> sqlite3.Connection:
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def _init_db(self):
        schema_path = os.path.join(os.path.dirname(__file__), "schema.sql")
        with open(schema_path, "r", encoding="utf-8") as f:
            schema_sql = f.read()
        
        conn = self.get_connection()
        try:
            conn.executescript(schema_sql)
            conn.commit()
        finally:
            conn.close()

    def upsert_track(self, track_data: Dict[str, Any]):
        sql = """
        INSERT INTO tracks (
            id, file_path, file_format, title, artist, album,
            year, track_number, duration_ms, start_offset_ms,
            end_offset_ms, has_cover_art
        ) VALUES (
            :id, :file_path, :file_format, :title, :artist, :album,
            :year, :track_number, :duration_ms, :start_offset_ms,
            :end_offset_ms, :has_cover_art
        ) ON CONFLICT(file_path) DO UPDATE SET
            title=excluded.title,
            artist=excluded.artist,
            album=excluded.album,
            year=excluded.year,
            track_number=excluded.track_number,
            duration_ms=excluded.duration_ms,
            has_cover_art=excluded.has_cover_art;
        """
        conn = self.get_connection()
        try:
            conn.execute(sql, track_data)
            conn.commit()
        finally:
            conn.close()

    def get_all_tracks(self, limit: int = 500, offset: int = 0, query: Optional[str] = None) -> List[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            if query:
                q = f"%{query}%"
                sql = """
                SELECT * FROM tracks 
                WHERE title LIKE ? OR artist LIKE ? OR album LIKE ?
                ORDER BY artist, album, track_number, title
                LIMIT ? OFFSET ?
                """
                cursor = conn.execute(sql, (q, q, q, limit, offset))
            else:
                sql = """
                SELECT * FROM tracks 
                ORDER BY artist, album, track_number, title
                LIMIT ? OFFSET ?
                """
                cursor = conn.execute(sql, (limit, offset))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

    def get_track_by_id(self, track_id: str) -> Optional[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM tracks WHERE id = ?", (track_id,))
            row = cursor.fetchone()
            return dict(row) if row else None
        finally:
            conn.close()

    def get_total_track_count(self) -> int:
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT COUNT(*) as cnt FROM tracks")
            row = cursor.fetchone()
            return row["cnt"] if row else 0
        finally:
            conn.close()

    def record_play_history(self, track_id: str, completed: bool = True):
        conn = self.get_connection()
        try:
            conn.execute(
                "INSERT INTO play_history (track_id, completed) VALUES (?, ?)",
                (track_id, 1 if completed else 0)
            )
            conn.commit()
        finally:
            conn.close()

import sqlite3
import os
from typing import List, Dict, Any, Optional, Tuple

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
            conn.execute("PRAGMA journal_mode = WAL")
            conn.execute("PRAGMA synchronous = NORMAL")
            conn.executescript(schema_sql)
            
            # Migration check for existing databases
            cursor = conn.execute("PRAGMA table_info(tracks)")
            columns = [row["name"] for row in cursor.fetchall()]
            if "file_mtime" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN file_mtime REAL DEFAULT 0")
            if "file_size" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN file_size INTEGER DEFAULT 0")
            conn.commit()
        finally:
            conn.close()

    def get_existing_file_map(self) -> Dict[str, Tuple[float, int]]:
        """Returns {file_path: (file_mtime, file_size)} for fast incremental scanning."""
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT file_path, file_mtime, file_size FROM tracks")
            return {row["file_path"]: (row["file_mtime"] or 0.0, row["file_size"] or 0) for row in cursor.fetchall()}
        finally:
            conn.close()

    def upsert_track(self, track_data: Dict[str, Any]):
        self.upsert_tracks_batch([track_data])

    def upsert_tracks_batch(self, tracks_data: List[Dict[str, Any]]):
        if not tracks_data:
            return
        
        # Ensure all required named parameter bindings exist in each dict
        sanitized = []
        for t in tracks_data:
            d = dict(t)
            d.setdefault("year", None)
            d.setdefault("track_number", None)
            d.setdefault("start_offset_ms", 0)
            d.setdefault("end_offset_ms", None)
            d.setdefault("has_cover_art", 0)
            d.setdefault("file_mtime", 0.0)
            d.setdefault("file_size", 0)
            sanitized.append(d)

        sql = """
        INSERT INTO tracks (
            id, file_path, file_format, title, artist, album,
            year, track_number, duration_ms, start_offset_ms,
            end_offset_ms, has_cover_art, file_mtime, file_size
        ) VALUES (
            :id, :file_path, :file_format, :title, :artist, :album,
            :year, :track_number, :duration_ms, :start_offset_ms,
            :end_offset_ms, :has_cover_art, :file_mtime, :file_size
        ) ON CONFLICT(file_path) DO UPDATE SET
            title=excluded.title,
            artist=excluded.artist,
            album=excluded.album,
            year=excluded.year,
            track_number=excluded.track_number,
            duration_ms=excluded.duration_ms,
            has_cover_art=excluded.has_cover_art,
            file_mtime=excluded.file_mtime,
            file_size=excluded.file_size;
        """
        conn = self.get_connection()
        try:
            conn.execute("PRAGMA synchronous = NORMAL")
            conn.executemany(sql, sanitized)
            conn.commit()
        finally:
            conn.close()

    def delete_tracks_by_paths(self, file_paths: List[str]):
        if not file_paths:
            return
        conn = self.get_connection()
        try:
            conn.executemany("DELETE FROM tracks WHERE file_path = ?", [(p,) for p in file_paths])
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

    def get_total_track_count(self, query: Optional[str] = None) -> int:
        conn = self.get_connection()
        try:
            if query:
                q = f"%{query}%"
                cursor = conn.execute("SELECT COUNT(*) as cnt FROM tracks WHERE title LIKE ? OR artist LIKE ? OR album LIKE ?", (q, q, q))
            else:
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

    def upsert_track_descriptors(self, track_id: str, desc: Dict[str, Any]):
        sql = """
        INSERT INTO track_audio_descriptors (
            track_id, energy, valence, danceability, acousticness,
            instrumentalness, speechiness, tempo_bpm, key_signature, loudness_lufs
        ) VALUES (
            :track_id, :energy, :valence, :danceability, :acousticness,
            :instrumentalness, :speechiness, :tempo_bpm, :key_signature, :loudness_lufs
        ) ON CONFLICT(track_id) DO UPDATE SET
            energy=excluded.energy,
            valence=excluded.valence,
            danceability=excluded.danceability,
            acousticness=excluded.acousticness,
            tempo_bpm=excluded.tempo_bpm,
            loudness_lufs=excluded.loudness_lufs;
        """
        data = dict(desc)
        data["track_id"] = track_id
        data.setdefault("energy", 0.5)
        data.setdefault("valence", 0.5)
        data.setdefault("danceability", 0.5)
        data.setdefault("acousticness", 0.5)
        data.setdefault("instrumentalness", 0.5)
        data.setdefault("speechiness", 0.1)
        data.setdefault("tempo_bpm", 120.0)
        data.setdefault("key_signature", "C Major")
        data.setdefault("loudness_lufs", -14.0)

        conn = self.get_connection()
        try:
            conn.execute(sql, data)
            conn.commit()
        finally:
            conn.close()

    def get_track_descriptors(self, track_id: str) -> Optional[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM track_audio_descriptors WHERE track_id = ?", (track_id,))
            row = cursor.fetchone()
            return dict(row) if row else None
        finally:
            conn.close()


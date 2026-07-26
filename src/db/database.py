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
            if "musicbrainz_track_id" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN musicbrainz_track_id TEXT")
            if "musicbrainz_album_id" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN musicbrainz_album_id TEXT")
            if "artist_sort_name" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN artist_sort_name TEXT")
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
            d.setdefault("artist_sort_name", None)
            sanitized.append(d)

        sql = """
        INSERT INTO tracks (
            id, file_path, file_format, title, artist, album,
            year, track_number, duration_ms, start_offset_ms,
            end_offset_ms, has_cover_art, file_mtime, file_size,
            artist_sort_name
        ) VALUES (
            :id, :file_path, :file_format, :title, :artist, :album,
            :year, :track_number, :duration_ms, :start_offset_ms,
            :end_offset_ms, :has_cover_art, :file_mtime, :file_size,
            :artist_sort_name
        ) ON CONFLICT(id) DO UPDATE SET
            file_path=excluded.file_path,
            title=excluded.title,
            artist=excluded.artist,
            album=excluded.album,
            year=excluded.year,
            track_number=excluded.track_number,
            duration_ms=excluded.duration_ms,
            start_offset_ms=excluded.start_offset_ms,
            end_offset_ms=excluded.end_offset_ms,
            has_cover_art=excluded.has_cover_art,
            file_mtime=excluded.file_mtime,
            file_size=excluded.file_size,
            artist_sort_name=COALESCE(excluded.artist_sort_name, tracks.artist_sort_name);
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

    def get_all_tracks(self, limit: int = 500, offset: int = 0, query: Optional[str] = None, artist: Optional[str] = None, album: Optional[str] = None, letter: Optional[str] = None) -> List[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            if artist:
                where_clauses.append("artist = ?")
                params.append(artist)
            if album:
                where_clauses.append("album = ?")
                params.append(album)
            if letter:
                if letter == "#":
                    where_clauses.append("(title GLOB '[0-9]*' OR artist GLOB '[0-9]*' OR COALESCE(artist_sort_name, artist) GLOB '[0-9]*')")
                else:
                    l = f"{letter}%"
                    where_clauses.append("(title LIKE ? OR artist LIKE ? OR COALESCE(artist_sort_name, artist) LIKE ?)")
                    params.extend([l, l, l])
            if query:
                q = f"%{query}%"
                where_clauses.append("(title LIKE ? OR artist LIKE ? OR album LIKE ? OR artist_sort_name LIKE ?)")
                params.extend([q, q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT * FROM tracks 
            {where_str}
            ORDER BY artist, album, CASE WHEN track_number IS NULL OR track_number = 0 THEN 999 ELSE track_number END ASC, title
            LIMIT ? OFFSET ?
            """
            params.extend([limit, offset])
            cursor = conn.execute(sql, tuple(params))
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

    def get_all_artists(self, limit: int = 200, query: Optional[str] = None, letter: Optional[str] = None) -> List[Dict[str, Any]]:
        """[REQ-MB-020D, REQ-UI-020E] Returns distinct artists matched by MusicBrainz artist_sort_name prefix key."""
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            if letter:
                if letter == "#":
                    where_clauses.append("COALESCE(artist_sort_name, artist) GLOB '[0-9]*'")
                else:
                    where_clauses.append("COALESCE(artist_sort_name, artist) LIKE ?")
                    params.append(f"{letter}%")
            if query:
                q = f"%{query}%"
                where_clauses.append("(artist LIKE ? OR album LIKE ? OR artist_sort_name LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT artist, MIN(COALESCE(artist_sort_name, artist)) as artist_sort_name,
                   COUNT(DISTINCT album) as album_count, COUNT(*) as track_count,
                   COALESCE(MAX(CASE WHEN has_cover_art = 1 THEN id END), MIN(id)) as sample_track_id
            FROM tracks
            {where_str}
            GROUP BY artist
            ORDER BY MIN(COALESCE(artist_sort_name, artist)) ASC
            LIMIT ?
            """
            params.append(limit)
            cursor = conn.execute(sql, tuple(params))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

    def get_all_albums(self, limit: int = 200, query: Optional[str] = None, artist: Optional[str] = None, letter: Optional[str] = None) -> List[Dict[str, Any]]:
        """[REQ-UI-020E] Returns distinct albums starting with selected letter or matching artist/query."""
        conn = self.get_connection()
        try:
            params = []
            where_clauses = []
            if artist:
                where_clauses.append("artist = ?")
                params.append(artist)
            if letter:
                if letter == "#":
                    where_clauses.append("(album GLOB '[0-9]*' OR artist GLOB '[0-9]*' OR COALESCE(artist_sort_name, artist) GLOB '[0-9]*')")
                else:
                    l = f"{letter}%"
                    where_clauses.append("(album LIKE ? OR artist LIKE ? OR COALESCE(artist_sort_name, artist) LIKE ?)")
                    params.extend([l, l, l])
            if query:
                q = f"%{query}%"
                where_clauses.append("(album LIKE ? OR artist LIKE ? OR artist_sort_name LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT album, artist, MIN(year) as year, COUNT(*) as track_count,
                   COALESCE(MAX(CASE WHEN has_cover_art = 1 THEN id END), MIN(id)) as sample_track_id
            FROM tracks
            {where_str}
            GROUP BY album, artist
            ORDER BY artist ASC, album ASC
            LIMIT ?
            """
            params.append(limit)
            cursor = conn.execute(sql, tuple(params))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

    def get_album_tracks(self, album_name: str, artist_name: Optional[str] = None) -> List[Dict[str, Any]]:
        """[REQ-UI-020B] Returns all tracks in an album sorted strictly by track_number."""
        conn = self.get_connection()
        try:
            if artist_name:
                sql = """
                SELECT * FROM tracks
                WHERE album = ? AND artist = ?
                ORDER BY CASE WHEN track_number IS NULL OR track_number = 0 THEN 999 ELSE track_number END ASC, title ASC
                """
                cursor = conn.execute(sql, (album_name, artist_name))
            else:
                sql = """
                SELECT * FROM tracks
                WHERE album = ?
                ORDER BY CASE WHEN track_number IS NULL OR track_number = 0 THEN 999 ELSE track_number END ASC, title ASC
                """
                cursor = conn.execute(sql, (album_name,))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

    def get_total_track_count(self, query: Optional[str] = None, artist: Optional[str] = None, album: Optional[str] = None, letter: Optional[str] = None) -> int:
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            if artist:
                where_clauses.append("artist = ?")
                params.append(artist)
            if album:
                where_clauses.append("album = ?")
                params.append(album)
            if letter:
                if letter == "#":
                    where_clauses.append("(title GLOB '[0-9]*' OR artist GLOB '[0-9]*')")
                else:
                    l = f"{letter}%"
                    where_clauses.append("(title LIKE ? OR artist LIKE ?)")
                    params.extend([l, l])
            if query:
                q = f"%{query}%"
                where_clauses.append("(title LIKE ? OR artist LIKE ? OR album LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"SELECT COUNT(*) as cnt FROM tracks {where_str}"
            cursor = conn.execute(sql, tuple(params))
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


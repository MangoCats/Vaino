import sqlite3
import os
from typing import List, Dict, Any, Optional, Tuple

class Database:
    def __init__(self, db_path: str = "vaino.db"):
        self.db_path = db_path
        self._init_db()

    def get_connection(self) -> sqlite3.Connection:
        if self.db_path == ":memory:":
            if not hasattr(self, "_mem_conn") or self._mem_conn is None:
                self._mem_conn = sqlite3.connect(":memory:", check_same_thread=False)
                self._mem_conn.row_factory = sqlite3.Row
            return self._mem_conn
        conn = sqlite3.connect(self.db_path, check_same_thread=False)
        conn.row_factory = sqlite3.Row
        return conn

    def close_connection(self, conn: sqlite3.Connection):
        if self.db_path != ":memory:":
            conn.close()

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
            if "album_sort_name" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN album_sort_name TEXT")
            if "title_sort_name" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN title_sort_name TEXT")

            ab_cols = [
                "ab_acoustic", "ab_aggressive", "ab_bright", "ab_danceable",
                "ab_female", "ab_happy", "ab_instrumental", "ab_party",
                "ab_relaxed", "ab_sad", "ab_tonal"
            ]
            for col in ab_cols:
                if col not in columns:
                    conn.execute(f"ALTER TABLE tracks ADD COLUMN {col} REAL")

            if "play_count" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN play_count INTEGER DEFAULT 0")
            if "last_played_at" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN last_played_at DATETIME DEFAULT NULL")
            if "rotation" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN rotation REAL DEFAULT 0.0")
            if "recovery" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN recovery REAL DEFAULT 0.778")
            if "restraint" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN restraint REAL DEFAULT 0.0")
            if "profanity" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN profanity REAL DEFAULT 0.0")
            if "occasions" not in columns:
                conn.execute("ALTER TABLE tracks ADD COLUMN occasions TEXT DEFAULT NULL")

            cursor = conn.execute("PRAGMA table_info(track_audio_descriptors)")
            desc_cols = [row["name"] for row in cursor.fetchall()]
            if "essentia_json" not in desc_cols:
                conn.execute("ALTER TABLE track_audio_descriptors ADD COLUMN essentia_json TEXT")

            conn.execute("""
                CREATE TABLE IF NOT EXISTS artist_ratings (
                    artist_id TEXT PRIMARY KEY,
                    artist_name TEXT NOT NULL UNIQUE,
                    artist_sort_name TEXT NOT NULL,
                    play_count INTEGER DEFAULT 0,
                    last_played_at DATETIME DEFAULT NULL,
                    rotation REAL DEFAULT 0.778,
                    recovery REAL DEFAULT 0.778,
                    restraint REAL DEFAULT 0.0,
                    streak_length REAL DEFAULT 0.0,
                    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );
            """)

            conn.execute("""
                CREATE TABLE IF NOT EXISTS track_relations (
                    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                    related_track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                    relationship_weight REAL DEFAULT 1.0,
                    PRIMARY KEY (track_id, related_track_id)
                );
            """)

            conn.execute("""
                CREATE TABLE IF NOT EXISTS album_cover_art (
                    album_id TEXT PRIMARY KEY,
                    album_name TEXT NOT NULL,
                    artist_name TEXT,
                    image_data BLOB NOT NULL,
                    mime_type TEXT NOT NULL,
                    source TEXT NOT NULL,
                    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );
            """)
            conn.execute("CREATE INDEX IF NOT EXISTS idx_album_cover_art_album ON album_cover_art(album_name);")

            conn.execute("""
                CREATE TABLE IF NOT EXISTS programs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    start_time TEXT NOT NULL,
                    track_ids TEXT
                );
            """)

            # Backfill/re-compute sort names for existing DB rows to enforce article stripping
            cursor = conn.execute("SELECT id, title, artist, album, artist_sort_name, album_sort_name, title_sort_name FROM tracks")
            rows = cursor.fetchall()
            if rows:
                from .scanner import compute_sort_name, compute_artist_sort_name
                updates = []
                for row in rows:
                    cur_art_sort = row["artist_sort_name"]
                    cur_alb_sort = row["album_sort_name"]
                    cur_ttl_sort = row["title_sort_name"]

                    art_sort = compute_artist_sort_name(row["artist"], cur_art_sort)
                    alb_sort = compute_sort_name(row["album"])
                    ttl_sort = compute_sort_name(row["title"])

                    if alb_sort != cur_alb_sort or ttl_sort != cur_ttl_sort or art_sort != cur_art_sort:
                        updates.append((art_sort, alb_sort, ttl_sort, row["id"]))

                if updates:
                    conn.executemany(
                        "UPDATE tracks SET artist_sort_name = ?, album_sort_name = ?, title_sort_name = ? WHERE id = ?",
                        updates
                    )

            conn.commit()
        finally:
            self.close_connection(conn)

    def get_existing_file_map(self) -> Dict[str, Tuple[float, int]]:
        """Returns {file_path: (file_mtime, file_size)} for fast incremental scanning."""
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT file_path, file_mtime, file_size FROM tracks")
            return {row["file_path"]: (row["file_mtime"] or 0.0, row["file_size"] or 0) for row in cursor.fetchall()}
        finally:
            self.close_connection(conn)

    def upsert_track(self, track_data: Dict[str, Any]):
        self.upsert_tracks_batch([track_data])

    def upsert_tracks_batch(self, tracks_data: List[Dict[str, Any]]):
        if not tracks_data:
            return
        
        # Ensure all required named parameter bindings exist in each dict
        sanitized = []
        ab_cols = [
            "ab_acoustic", "ab_aggressive", "ab_bright", "ab_danceable",
            "ab_female", "ab_happy", "ab_instrumental", "ab_party",
            "ab_relaxed", "ab_sad", "ab_tonal"
        ]
        for t in tracks_data:
            d = dict(t)
            d.setdefault("album", None)
            d.setdefault("year", None)
            d.setdefault("track_number", None)
            d.setdefault("start_offset_ms", 0)
            d.setdefault("end_offset_ms", None)
            d.setdefault("has_cover_art", 0)
            d.setdefault("file_mtime", 0.0)
            d.setdefault("file_size", 0)
            d.setdefault("artist_sort_name", None)
            d.setdefault("album_sort_name", None)
            d.setdefault("title_sort_name", None)
            d.setdefault("musicbrainz_track_id", None)
            d.setdefault("musicbrainz_album_id", None)
            for c in ab_cols:
                d.setdefault(c, None)
            sanitized.append(d)

        sql = """
        INSERT INTO tracks (
            id, file_path, file_format, title, artist, album,
            year, track_number, duration_ms, start_offset_ms,
            end_offset_ms, has_cover_art, file_mtime, file_size,
            artist_sort_name, album_sort_name, title_sort_name,
            musicbrainz_track_id, musicbrainz_album_id,
            ab_acoustic, ab_aggressive, ab_bright, ab_danceable,
            ab_female, ab_happy, ab_instrumental, ab_party,
            ab_relaxed, ab_sad, ab_tonal
        ) VALUES (
            :id, :file_path, :file_format, :title, :artist, :album,
            :year, :track_number, :duration_ms, :start_offset_ms,
            :end_offset_ms, :has_cover_art, :file_mtime, :file_size,
            :artist_sort_name, :album_sort_name, :title_sort_name,
            :musicbrainz_track_id, :musicbrainz_album_id,
            :ab_acoustic, :ab_aggressive, :ab_bright, :ab_danceable,
            :ab_female, :ab_happy, :ab_instrumental, :ab_party,
            :ab_relaxed, :ab_sad, :ab_tonal
        ) ON CONFLICT(id) DO UPDATE SET
            file_path=excluded.file_path,
            file_format=excluded.file_format,
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
            artist_sort_name=COALESCE(excluded.artist_sort_name, tracks.artist_sort_name),
            album_sort_name=COALESCE(excluded.album_sort_name, tracks.album_sort_name),
            title_sort_name=COALESCE(excluded.title_sort_name, tracks.title_sort_name),
            musicbrainz_track_id=COALESCE(excluded.musicbrainz_track_id, tracks.musicbrainz_track_id),
            musicbrainz_album_id=COALESCE(excluded.musicbrainz_album_id, tracks.musicbrainz_album_id),
            ab_acoustic=COALESCE(excluded.ab_acoustic, tracks.ab_acoustic),
            ab_aggressive=COALESCE(excluded.ab_aggressive, tracks.ab_aggressive),
            ab_bright=COALESCE(excluded.ab_bright, tracks.ab_bright),
            ab_danceable=COALESCE(excluded.ab_danceable, tracks.ab_danceable),
            ab_female=COALESCE(excluded.ab_female, tracks.ab_female),
            ab_happy=COALESCE(excluded.ab_happy, tracks.ab_happy),
            ab_instrumental=COALESCE(excluded.ab_instrumental, tracks.ab_instrumental),
            ab_party=COALESCE(excluded.ab_party, tracks.ab_party),
            ab_relaxed=COALESCE(excluded.ab_relaxed, tracks.ab_relaxed),
            ab_sad=COALESCE(excluded.ab_sad, tracks.ab_sad),
            ab_tonal=COALESCE(excluded.ab_tonal, tracks.ab_tonal);
        """
        conn = self.get_connection()
        try:
            conn.execute("PRAGMA synchronous = NORMAL")
            conn.executemany(sql, sanitized)
            
            # Decompose and insert track_artists junction entries [REQ-MB-020E]
            from .scanner import split_artists
            artist_rows = []
            track_ids = [t["id"] for t in sanitized]
            for t in sanitized:
                track_id = t["id"]
                raw_artist = t.get("artist", "")
                embedded_sort = t.get("artist_sort_name")
                individual_artists = split_artists(raw_artist, embedded=embedded_sort)
                for a_name, a_sort in individual_artists:
                    artist_rows.append((track_id, a_name, a_sort))

            if track_ids:
                placeholders = ",".join("?" for _ in track_ids)
                conn.execute(f"DELETE FROM track_artists WHERE track_id IN ({placeholders})", track_ids)
            if artist_rows:
                conn.executemany(
                    "INSERT OR IGNORE INTO track_artists (track_id, artist_name, artist_sort_name) VALUES (?, ?, ?)",
                    artist_rows
                )
            conn.commit()
        finally:
            self.close_connection(conn)

    def delete_tracks_by_paths(self, file_paths: List[str]):
        if not file_paths:
            return
        conn = self.get_connection()
        try:
            conn.executemany("DELETE FROM tracks WHERE file_path = ?", [(p,) for p in file_paths])
            conn.commit()
        finally:
            self.close_connection(conn)

    def get_all_tracks(self, limit: int = 500, offset: int = 0, query: Optional[str] = None, artist: Optional[str] = None, album: Optional[str] = None, letter: Optional[str] = None) -> List[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            join_clause = ""
            if artist:
                join_clause = "JOIN track_artists ta ON t.id = ta.track_id"
                where_clauses.append("(ta.artist_name = ? OR t.artist = ?)")
                params.extend([artist, artist])
            if album:
                where_clauses.append("t.album = ?")
                params.append(album)
            if letter:
                if letter == "#":
                    where_clauses.append("COALESCE(t.title_sort_name, t.title) GLOB '[0-9]*'")
                else:
                    l = f"{letter}%"
                    where_clauses.append("COALESCE(t.title_sort_name, t.title) LIKE ?")
                    params.append(l)
            if query:
                q = f"%{query}%"
                where_clauses.append("(t.title LIKE ? OR t.artist LIKE ? OR t.album LIKE ? OR t.artist_sort_name LIKE ?)")
                params.extend([q, q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            if album:
                order_clause = "ORDER BY CASE WHEN t.track_number IS NULL OR t.track_number = 0 THEN 999 ELSE t.track_number END ASC, COALESCE(t.title_sort_name, t.title) ASC"
            else:
                order_clause = "ORDER BY COALESCE(t.title_sort_name, t.title) ASC, t.artist ASC"

            sql = f"""
            SELECT DISTINCT t.* FROM tracks t
            {join_clause}
            {where_str}
            {order_clause}
            LIMIT ? OFFSET ?
            """
            params.extend([limit, offset])
            cursor = conn.execute(sql, tuple(params))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            self.close_connection(conn)

    def get_track_by_id(self, track_id: str) -> Optional[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM tracks WHERE id = ?", (track_id,))
            row = cursor.fetchone()
            return dict(row) if row else None
        finally:
            self.close_connection(conn)

    def get_total_artist_count(self, query: Optional[str] = None, letter: Optional[str] = None) -> int:
        """[REQ-UI-020I] Returns total distinct individual artist count matching letter/query filters."""
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            if letter:
                if letter == "#":
                    where_clauses.append("ta.artist_sort_name GLOB '[0-9]*'")
                else:
                    where_clauses.append("ta.artist_sort_name LIKE ?")
                    params.append(f"{letter}%")
            if query:
                q = f"%{query}%"
                where_clauses.append("(ta.artist_name LIKE ? OR t.album LIKE ? OR ta.artist_sort_name LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT COUNT(DISTINCT ta.artist_name)
            FROM track_artists ta
            JOIN tracks t ON ta.track_id = t.id
            {where_str}
            """
            cursor = conn.execute(sql, tuple(params))
            row = cursor.fetchone()
            return row[0] if row else 0
        finally:
            self.close_connection(conn)

    def get_all_artists(self, limit: int = 100, offset: int = 0, query: Optional[str] = None, letter: Optional[str] = None) -> List[Dict[str, Any]]:
        """[REQ-MB-020E, REQ-UI-020G, REQ-UI-020I] Returns paginated distinct individual artists from track_artists junction table."""
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            if letter:
                if letter == "#":
                    where_clauses.append("ta.artist_sort_name GLOB '[0-9]*'")
                else:
                    where_clauses.append("ta.artist_sort_name LIKE ?")
                    params.append(f"{letter}%")
            if query:
                q = f"%{query}%"
                where_clauses.append("(ta.artist_name LIKE ? OR t.album LIKE ? OR ta.artist_sort_name LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT ta.artist_name as artist,
                   MIN(ta.artist_sort_name) as artist_sort_name,
                   COUNT(DISTINCT t.album) as album_count,
                   COUNT(DISTINCT t.id) as track_count,
                   COALESCE(MAX(CASE WHEN t.has_cover_art = 1 THEN t.id END), MIN(t.id)) as sample_track_id
            FROM track_artists ta
            JOIN tracks t ON ta.track_id = t.id
            {where_str}
            GROUP BY ta.artist_name
            ORDER BY MIN(ta.artist_sort_name) ASC
            LIMIT ? OFFSET ?
            """
            params.extend([limit, offset])
            cursor = conn.execute(sql, tuple(params))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            self.close_connection(conn)

    def get_total_album_count(self, query: Optional[str] = None, artist: Optional[str] = None, letter: Optional[str] = None) -> int:
        """[REQ-UI-020I] Returns total distinct album count matching artist/letter/query filters."""
        conn = self.get_connection()
        try:
            params = []
            where_clauses = []
            join_clause = ""
            if artist:
                join_clause = "JOIN track_artists ta ON t.id = ta.track_id"
                where_clauses.append("(ta.artist_name = ? OR t.artist = ?)")
                params.extend([artist, artist])
            if letter:
                if letter == "#":
                    where_clauses.append("COALESCE(t.album_sort_name, t.album) GLOB '[0-9]*'")
                else:
                    l = f"{letter}%"
                    where_clauses.append("COALESCE(t.album_sort_name, t.album) LIKE ?")
                    params.append(l)
            if query:
                q = f"%{query}%"
                where_clauses.append("(t.album LIKE ? OR t.artist LIKE ? OR t.artist_sort_name LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT COUNT(DISTINCT t.album)
            FROM tracks t
            {join_clause}
            {where_str}
            """
            cursor = conn.execute(sql, tuple(params))
            row = cursor.fetchone()
            return row[0] if row else 0
        finally:
            self.close_connection(conn)

    def get_all_albums(self, limit: int = 100, offset: int = 0, query: Optional[str] = None, artist: Optional[str] = None, letter: Optional[str] = None) -> List[Dict[str, Any]]:
        """[REQ-UI-020G, REQ-UI-020I] Returns paginated distinct deduplicated albums sorted by album_sort_name."""
        conn = self.get_connection()
        try:
            params = []
            where_clauses = []
            join_clause = ""
            if artist:
                join_clause = "JOIN track_artists ta ON t.id = ta.track_id"
                where_clauses.append("(ta.artist_name = ? OR t.artist = ?)")
                params.extend([artist, artist])
            if letter:
                if letter == "#":
                    where_clauses.append("COALESCE(t.album_sort_name, t.album) GLOB '[0-9]*'")
                else:
                    l = f"{letter}%"
                    where_clauses.append("COALESCE(t.album_sort_name, t.album) LIKE ?")
                    params.append(l)
            if query:
                q = f"%{query}%"
                where_clauses.append("(t.album LIKE ? OR t.artist LIKE ? OR t.artist_sort_name LIKE ?)")
                params.extend([q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"""
            SELECT t.album,
                   MIN(COALESCE(t.album_sort_name, t.album)) as album_sort_name,
                   COALESCE(MAX(CASE WHEN t.artist = ? THEN t.artist END), MIN(t.artist)) as artist,
                   MIN(t.year) as year,
                   COUNT(DISTINCT t.id) as track_count,
                   COALESCE(MAX(CASE WHEN t.has_cover_art = 1 THEN t.id END), MIN(t.id)) as sample_track_id
            FROM tracks t
            {join_clause}
            {where_str}
            GROUP BY t.album
            ORDER BY MIN(COALESCE(t.album_sort_name, t.album)) ASC
            LIMIT ? OFFSET ?
            """
            all_params = [artist if artist else ""]
            all_params.extend(params)
            all_params.extend([limit, offset])
            cursor = conn.execute(sql, tuple(all_params))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            self.close_connection(conn)

    def get_album_tracks(self, album_name: str, artist_name: Optional[str] = None) -> List[Dict[str, Any]]:
        """[REQ-UI-020B] Returns all tracks in an album sorted strictly by track_number and passage offset."""
        conn = self.get_connection()
        try:
            # Check if sliced passage tracks exist for this album
            if artist_name:
                cursor = conn.execute("SELECT COUNT(*) FROM tracks WHERE album = ? AND artist = ? AND end_offset_ms IS NOT NULL", (album_name, artist_name))
            else:
                cursor = conn.execute("SELECT COUNT(*) FROM tracks WHERE album = ? AND end_offset_ms IS NOT NULL", (album_name,))
            has_passages = cursor.fetchone()[0] > 0

            where_clause = "album = ?"
            params = [album_name]
            if artist_name:
                where_clause += " AND artist = ?"
                params.append(artist_name)
            
            if has_passages:
                where_clause += " AND end_offset_ms IS NOT NULL"

            sql = f"""
            SELECT * FROM tracks
            WHERE {where_clause}
            ORDER BY CASE WHEN track_number IS NULL OR track_number = 0 THEN 999 ELSE track_number END ASC, start_offset_ms ASC, title ASC
            """
            cursor = conn.execute(sql, tuple(params))
            return [dict(row) for row in cursor.fetchall()]
        finally:
            self.close_connection(conn)

    def get_total_track_count(self, query: Optional[str] = None, artist: Optional[str] = None, album: Optional[str] = None, letter: Optional[str] = None) -> int:
        conn = self.get_connection()
        try:
            where_clauses = []
            params = []
            join_clause = ""
            if artist:
                join_clause = "JOIN track_artists ta ON t.id = ta.track_id"
                where_clauses.append("(ta.artist_name = ? OR t.artist = ?)")
                params.extend([artist, artist])
            if album:
                where_clauses.append("t.album = ?")
                params.append(album)
            if letter:
                if letter == "#":
                    where_clauses.append("COALESCE(t.title_sort_name, t.title) GLOB '[0-9]*'")
                else:
                    l = f"{letter}%"
                    where_clauses.append("COALESCE(t.title_sort_name, t.title) LIKE ?")
                    params.append(l)
            if query:
                q = f"%{query}%"
                where_clauses.append("(t.title LIKE ? OR t.artist LIKE ? OR t.album LIKE ? OR t.artist_sort_name LIKE ?)")
                params.extend([q, q, q, q])

            where_str = ("WHERE " + " AND ".join(where_clauses)) if where_clauses else ""
            sql = f"SELECT COUNT(DISTINCT t.id) as cnt FROM tracks t {join_clause} {where_str}"
            cursor = conn.execute(sql, tuple(params))
            row = cursor.fetchone()
            return row["cnt"] if row else 0
        finally:
            self.close_connection(conn)

    def record_play_history(self, track_id: str, completed: bool = True):
        conn = self.get_connection()
        try:
            conn.execute(
                "INSERT INTO play_history (track_id, completed) VALUES (?, ?)",
                (track_id, 1 if completed else 0)
            )
            conn.commit()
        finally:
            self.close_connection(conn)

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
            self.close_connection(conn)

    def upsert_track_descriptors_batch(self, batch: List[Tuple[str, Dict[str, Any]]]):
        if not batch:
            return
        sql = """
        INSERT INTO track_audio_descriptors (
            track_id, energy, valence, danceability, acousticness,
            instrumentalness, speechiness, tempo_bpm, key_signature, loudness_lufs, essentia_json
        ) VALUES (
            :track_id, :energy, :valence, :danceability, :acousticness,
            :instrumentalness, :speechiness, :tempo_bpm, :key_signature, :loudness_lufs, :essentia_json
        )
        ON CONFLICT(track_id) DO UPDATE SET
            energy = excluded.energy,
            valence = excluded.valence,
            danceability = excluded.danceability,
            acousticness = excluded.acousticness,
            instrumentalness = excluded.instrumentalness,
            speechiness = excluded.speechiness,
            tempo_bpm = excluded.tempo_bpm,
            key_signature = excluded.key_signature,
            loudness_lufs = excluded.loudness_lufs,
            essentia_json = excluded.essentia_json
        """
        records = []
        for track_id, d in batch:
            rec = dict(d)
            rec["track_id"] = track_id
            rec.setdefault("energy", 0.5)
            rec.setdefault("valence", 0.5)
            rec.setdefault("danceability", 0.5)
            rec.setdefault("acousticness", 0.5)
            rec.setdefault("instrumentalness", 0.5)
            rec.setdefault("speechiness", 0.1)
            rec.setdefault("tempo_bpm", 120.0)
            rec.setdefault("key_signature", "C Major")
            rec.setdefault("loudness_lufs", -14.0)
            rec.setdefault("essentia_json", None)
            records.append(rec)

        conn = self.get_connection()
        try:
            conn.executemany(sql, records)
            conn.commit()
        finally:
            self.close_connection(conn)

    def get_track_descriptors(self, track_id: str) -> Optional[Dict[str, Any]]:
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM track_audio_descriptors WHERE track_id = ?", (track_id,))
            row = cursor.fetchone()
            res = dict(row) if row else {}

            if res.get("essentia_json"):
                try:
                    import json
                    res["essentia"] = json.loads(res["essentia_json"])
                except Exception:
                    res["essentia"] = None

            # Also fetch 11D AcousticBrainz features from tracks table if available
            t_row = conn.execute("""
                SELECT ab_acoustic, ab_aggressive, ab_bright, ab_danceable,
                       ab_female, ab_happy, ab_instrumental, ab_party,
                       ab_relaxed, ab_sad, ab_tonal
                FROM tracks WHERE id = ?
            """, (track_id,)).fetchone()

            if t_row:
                for k in t_row.keys():
                    if res.get(k) is None and t_row[k] is not None:
                        res[k] = float(t_row[k])

            return res if res else None
        finally:
            self.close_connection(conn)

    def save_player_state(self, current_track_id: Optional[str], playback_state: str, volume: int):
        """Persists current playing track ID, playback state, and master volume."""
        conn = self.get_connection()
        try:
            conn.execute(
                """
                INSERT INTO player_state (id, current_track_id, playback_state, volume, updated_at)
                VALUES (1, ?, ?, ?, CURRENT_TIMESTAMP)
                ON CONFLICT(id) DO UPDATE SET
                    current_track_id = excluded.current_track_id,
                    playback_state = excluded.playback_state,
                    volume = excluded.volume,
                    updated_at = CURRENT_TIMESTAMP
                """,
                (current_track_id, playback_state, volume)
            )
            conn.commit()
        finally:
            self.close_connection(conn)

    def get_player_state(self) -> Optional[Dict[str, Any]]:
        """Retrieves persisted player state."""
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM player_state WHERE id = 1")
            row = cursor.fetchone()
            return dict(row) if row else None
        finally:
            self.close_connection(conn)

    def save_player_queue(self, track_ids: List[str]):
        """Persists ordered list of queued track IDs."""
        conn = self.get_connection()
        try:
            conn.execute("DELETE FROM player_queue")
            if track_ids:
                conn.executemany(
                    "INSERT INTO player_queue (track_id) VALUES (?)",
                    [(tid,) for tid in track_ids]
                )
            conn.commit()
        finally:
            self.close_connection(conn)

    def get_player_queue_tracks(self) -> List[Dict[str, Any]]:
        """Retrieves ordered queue track records."""
        conn = self.get_connection()
        try:
            cursor = conn.execute(
                """
                SELECT t.*
                FROM player_queue pq
                JOIN tracks t ON pq.track_id = t.id
                ORDER BY pq.queue_order ASC
                """
            )
            return [dict(row) for row in cursor.fetchall()]
        finally:
            self.close_connection(conn)

    def save_album_cover_art(self, album_name: str, artist_name: Optional[str], image_bytes: bytes, mime_type: str, source: str = "MUSICBRAINZ") -> str:
        """Saves or updates album cover art blob in album_cover_art table."""
        import hashlib
        album_id = hashlib.md5(f"{artist_name or ''}||{album_name}".lower().encode("utf-8")).hexdigest()
        conn = self.get_connection()
        try:
            conn.execute(
                """
                INSERT INTO album_cover_art (album_id, album_name, artist_name, image_data, mime_type, source, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ON CONFLICT(album_id) DO UPDATE SET
                    image_data = excluded.image_data,
                    mime_type = excluded.mime_type,
                    source = excluded.source,
                    updated_at = CURRENT_TIMESTAMP
                """,
                (album_id, album_name, artist_name, image_bytes, mime_type, source)
            )
            # Update tracks for this album so has_cover_art = 1
            conn.execute(
                "UPDATE tracks SET has_cover_art = 1 WHERE album = ?",
                (album_name,)
            )
            conn.commit()
            return album_id
        finally:
            self.close_connection(conn)

    def get_album_cover_art(self, album_name: str, artist_name: Optional[str] = None) -> Optional[Tuple[bytes, str]]:
        """Retrieves album cover art (image_bytes, mime_type) from album_cover_art table."""
        import hashlib
        album_id = hashlib.md5(f"{artist_name or ''}||{album_name}".lower().encode("utf-8")).hexdigest()
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT image_data, mime_type FROM album_cover_art WHERE album_id = ?", (album_id,))
            row = cursor.fetchone()
            if row:
                return row["image_data"], row["mime_type"]

            # Fallback by album_name match
            cursor = conn.execute("SELECT image_data, mime_type FROM album_cover_art WHERE album_name = ? LIMIT 1", (album_name,))
            row = cursor.fetchone()
            if row:
                return row["image_data"], row["mime_type"]
            return None
        finally:
            self.close_connection(conn)

    def get_all_programs(self) -> list:
        """Retrieves all time-slot programs ordered by start_time."""
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM programs ORDER BY start_time ASC")
            return [dict(row) for row in cursor.fetchall()]
        finally:
            self.close_connection(conn)

    def get_program_by_id(self, program_id: int) -> Optional[dict]:
        """Retrieves a single program by ID."""
        conn = self.get_connection()
        try:
            cursor = conn.execute("SELECT * FROM programs WHERE id = ?", (program_id,))
            row = cursor.fetchone()
            return dict(row) if row else None
        finally:
            self.close_connection(conn)

    def save_program(self, name: str, start_time: str, track_ids: str = "") -> dict:
        """Creates a new program slot."""
        conn = self.get_connection()
        try:
            cursor = conn.execute(
                "INSERT INTO programs (name, start_time, track_ids) VALUES (?, ?, ?)",
                (name, start_time, track_ids)
            )
            conn.commit()
            return self.get_program_by_id(cursor.lastrowid)
        finally:
            self.close_connection(conn)

    def update_program(self, program_id: int, name: str, start_time: str, track_ids: str = "") -> Optional[dict]:
        """Updates an existing program slot."""
        conn = self.get_connection()
        try:
            conn.execute(
                "UPDATE programs SET name = ?, start_time = ?, track_ids = ? WHERE id = ?",
                (name, start_time, track_ids, program_id)
            )
            conn.commit()
            return self.get_program_by_id(program_id)
        finally:
            self.close_connection(conn)

    def delete_program(self, program_id: int) -> bool:
        """Deletes a program slot."""
        conn = self.get_connection()
        try:
            cursor = conn.execute("DELETE FROM programs WHERE id = ?", (program_id,))
            conn.commit()
            return cursor.rowcount > 0
        finally:
            self.close_connection(conn)

    def import_mulib_programs(self, mulib_path: str = r"C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db") -> int:
        """Imports default time-slot programs from mulib.db."""
        if not os.path.exists(mulib_path):
            return 0

        conn = self.get_connection()
        try:
            mconn = sqlite3.connect(mulib_path)
            mconn.row_factory = sqlite3.Row
            mcur = mconn.execute("SELECT * FROM programs")
            m_progs = mcur.fetchall()
            
            imported_count = 0
            for mp in m_progs:
                name = mp["name"]
                start_time = mp["startTime"]
                raw_tids = mp["trackList"] or ""
                
                # Parse numeric track IDs from mulib
                m_track_ids = [int(t.strip("[] ")) for t in raw_tids.split("\n") if t.strip() and t.strip("[] ").isdigit()]
                
                # Find matching Vaino track IDs for these mulib track IDs
                vaino_track_ids = []
                for mtid in m_track_ids:
                    c_res = mconn.execute("""
                        SELECT f.filePath, t.mbidRecording, t.name
                        FROM tracks t
                        JOIN cuts c ON c.trackId = t.trackId
                        JOIN files f ON f.fileId = c.fileId
                        WHERE t.trackId = ?
                    """, (mtid,)).fetchone()
                    
                    if c_res:
                        mbid = c_res["mbidRecording"]
                        fpath = c_res["filePath"]
                        v_row = None
                        if mbid:
                            v_row = conn.execute("SELECT id FROM tracks WHERE musicbrainz_track_id = ?", (mbid,)).fetchone()
                        if not v_row and fpath:
                            tail = os.path.basename(fpath)
                            v_row = conn.execute("SELECT id FROM tracks WHERE file_path LIKE ?", (f"%{tail}%",)).fetchone()
                        if v_row:
                            vaino_track_ids.append(v_row["id"])

                track_ids_str = "\n".join(vaino_track_ids)
                
                conn.execute(
                    "INSERT INTO programs (name, start_time, track_ids) VALUES (?, ?, ?) ON CONFLICT(name) DO UPDATE SET start_time=excluded.start_time, track_ids=excluded.track_ids",
                    (name, start_time, track_ids_str)
                )
                imported_count += 1

            mconn.close()
            conn.commit()
            return imported_count
        except Exception as e:
            logger.warning(f"Error importing mulib programs: {e}")
            return 0
        finally:
            self.close_connection(conn)

    def import_mulib_preferences(self, mulib_path: str = r"C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db") -> dict:
        """
        [REQ-UI-030] Imports user preferences (rotation, recovery, restraint, profanity, occasions,
        artist ratings, play history, track relations) from mulib.db into Vaino.
        """
        if not os.path.exists(mulib_path):
            return {"error": f"File not found: {mulib_path}"}

        vconn = self.get_connection()
        try:
            mconn = sqlite3.connect(mulib_path)
            mconn.row_factory = sqlite3.Row

            m_tracks = mconn.execute("SELECT * FROM tracks").fetchall()
            mulib_to_vaino_track: Dict[int, str] = {}

            v_tracks = vconn.execute("SELECT id, title, artist, file_path, musicbrainz_track_id FROM tracks").fetchall()
            v_by_mbid = {r["musicbrainz_track_id"]: r["id"] for r in v_tracks if r["musicbrainz_track_id"]}
            v_by_title_artist = {(r["title"].lower(), r["artist"].lower()): r["id"] for r in v_tracks if r["title"] and r["artist"]}
            v_by_filetail = {os.path.basename(r["file_path"]).lower(): r["id"] for r in v_tracks if r["file_path"]}

            m_cuts = mconn.execute("""
                SELECT c.cutId, c.trackId, f.filePath, t.mbidRecording, t.name as title
                FROM cuts c
                JOIN tracks t ON c.trackId = t.trackId
                LEFT JOIN files f ON c.fileId = f.fileId
            """).fetchall()
            
            mulib_cut_to_vaino_track: Dict[int, str] = {}
            for c in m_cuts:
                cid = c["cutId"]
                mtid = c["trackId"]
                mbid = c["mbidRecording"]
                fpath = c["filePath"]
                
                v_tid = None
                if mbid and mbid in v_by_mbid:
                    v_tid = v_by_mbid[mbid]
                elif fpath:
                    ftail = os.path.basename(fpath).lower()
                    if ftail in v_by_filetail:
                        v_tid = v_by_filetail[ftail]

                if v_tid:
                    mulib_cut_to_vaino_track[cid] = v_tid
                    if mtid:
                        mulib_to_vaino_track[mtid] = v_tid

            for mt in m_tracks:
                mtid = mt["trackId"]
                if mtid in mulib_to_vaino_track:
                    continue
                mbid = mt["mbidRecording"] or mt["mbid"]
                title = mt["name"]
                
                v_tid = None
                if mbid and mbid in v_by_mbid:
                    v_tid = v_by_mbid[mbid]
                elif title:
                    for (t_lower, a_lower), tid in v_by_title_artist.items():
                        if t_lower == title.lower():
                            v_tid = tid
                            break
                if v_tid:
                    mulib_to_vaino_track[mtid] = v_tid

            # 2. Track Ratings & 11D Acoustic Features
            t_updates = []
            for mt in m_tracks:
                mtid = mt["trackId"]
                if mtid in mulib_to_vaino_track:
                    v_tid = mulib_to_vaino_track[mtid]
                    rot = float(mt["rotation"] or 0.0)
                    rec = float(mt["recovery"] if mt["recovery"] is not None else 0.778)
                    res = float(mt["restraint"] or 0.0)
                    prof = float(mt["profanity"] or 0.0)
                    occ = mt["occasions"] or None
                    
                    ab_ac = float(mt["abAcoustic"]) if mt["abAcoustic"] is not None else None
                    ab_ag = float(mt["abAggressive"]) if mt["abAggressive"] is not None else None
                    ab_br = float(mt["abBright"]) if mt["abBright"] is not None else None
                    ab_da = float(mt["abDanceable"]) if mt["abDanceable"] is not None else None
                    ab_fe = float(mt["abFemale"]) if mt["abFemale"] is not None else None
                    ab_ha = float(mt["abHappy"]) if mt["abHappy"] is not None else None
                    ab_in = float(mt["abInstrumental"]) if mt["abInstrumental"] is not None else None
                    ab_pa = float(mt["abParty"]) if mt["abParty"] is not None else None
                    ab_re = float(mt["abRelaxed"]) if mt["abRelaxed"] is not None else None
                    ab_sa = float(mt["abSad"]) if mt["abSad"] is not None else None
                    ab_to = float(mt["abTonal"]) if mt["abTonal"] is not None else None

                    t_updates.append((rot, rec, res, prof, occ, ab_ac, ab_ag, ab_br, ab_da, ab_fe, ab_ha, ab_in, ab_pa, ab_re, ab_sa, ab_to, v_tid))

            if t_updates:
                vconn.executemany("""
                    UPDATE tracks
                    SET rotation = ?, recovery = ?, restraint = ?, profanity = ?, occasions = ?,
                        ab_acoustic = COALESCE(?, ab_acoustic),
                        ab_aggressive = COALESCE(?, ab_aggressive),
                        ab_bright = COALESCE(?, ab_bright),
                        ab_danceable = COALESCE(?, ab_danceable),
                        ab_female = COALESCE(?, ab_female),
                        ab_happy = COALESCE(?, ab_happy),
                        ab_instrumental = COALESCE(?, ab_instrumental),
                        ab_party = COALESCE(?, ab_party),
                        ab_relaxed = COALESCE(?, ab_relaxed),
                        ab_sad = COALESCE(?, ab_sad),
                        ab_tonal = COALESCE(?, ab_tonal)
                    WHERE id = ?
                """, t_updates)

            # 3. Artist Ratings
            m_artists = mconn.execute("SELECT * FROM artists").fetchall()
            a_updates = []
            import hashlib
            for ma in m_artists:
                a_name = ma["name"]
                if not a_name:
                    continue
                rot = float(ma["rotation"] if ma["rotation"] is not None else 0.778)
                rec = float(ma["recovery"] if ma["recovery"] is not None else 0.778)
                res = float(ma["restraint"] or 0.0)
                strk = float(ma["streakLength"] or 0.0)
                sort_name = ma["sortName"] or a_name.upper()
                a_id = hashlib.md5(a_name.encode("utf-8")).hexdigest()[:16]

                a_updates.append((a_id, a_name, sort_name, rot, rec, res, strk))

            if a_updates:
                vconn.executemany("""
                    INSERT INTO artist_ratings (artist_id, artist_name, artist_sort_name, rotation, recovery, restraint, streak_length)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(artist_name) DO UPDATE SET
                        rotation = excluded.rotation,
                        recovery = excluded.recovery,
                        restraint = excluded.restraint,
                        streak_length = excluded.streak_length,
                        updated_at = CURRENT_TIMESTAMP
                """, a_updates)

            # 4. Play History
            from datetime import datetime, timezone
            m_history = mconn.execute("SELECT * FROM playHistory ORDER BY time ASC").fetchall()

            track_play_counts: Dict[str, int] = {}
            track_last_played: Dict[str, str] = {}
            artist_play_counts: Dict[str, int] = {}
            artist_last_played: Dict[str, str] = {}

            h_inserts = []
            for h in m_history:
                ts = h["time"]
                cid = h["cutId"]
                mbid = h["mbid"]
                
                v_tid = None
                if mbid and mbid in v_by_mbid:
                    v_tid = v_by_mbid[mbid]
                elif cid in mulib_cut_to_vaino_track:
                    v_tid = mulib_cut_to_vaino_track[cid]

                if v_tid:
                    iso_str = datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
                    h_inserts.append((v_tid, iso_str))
                    
                    track_play_counts[v_tid] = track_play_counts.get(v_tid, 0) + 1
                    track_last_played[v_tid] = iso_str

                    t_row = vconn.execute("SELECT artist FROM tracks WHERE id = ?", (v_tid,)).fetchone()
                    if t_row and t_row["artist"]:
                        art_name = t_row["artist"]
                        artist_play_counts[art_name] = artist_play_counts.get(art_name, 0) + 1
                        artist_last_played[art_name] = iso_str

            if h_inserts:
                vconn.executemany("INSERT INTO play_history (track_id, played_at, completed) VALUES (?, ?, 1)", h_inserts)

            t_count_updates = [(cnt, track_last_played[tid], tid) for tid, cnt in track_play_counts.items()]
            if t_count_updates:
                vconn.executemany("UPDATE tracks SET play_count = ?, last_played_at = ? WHERE id = ?", t_count_updates)

            for art_name, cnt in artist_play_counts.items():
                a_id = hashlib.md5(art_name.encode("utf-8")).hexdigest()[:16]
                last_played = artist_last_played[art_name]
                vconn.execute("""
                    INSERT INTO artist_ratings (artist_id, artist_name, artist_sort_name, play_count, last_played_at)
                    VALUES (?, ?, ?, ?, ?)
                    ON CONFLICT(artist_name) DO UPDATE SET
                        play_count = artist_ratings.play_count + excluded.play_count,
                        last_played_at = MAX(COALESCE(artist_ratings.last_played_at, ''), excluded.last_played_at),
                        updated_at = CURRENT_TIMESTAMP
                """, (a_id, art_name, art_name.upper(), cnt, last_played))

            # 5. Track Relations
            rel_inserts = []
            for mt in m_tracks:
                mtid = mt["trackId"]
                rel_raw = mt["relatedTracks"]
                if mtid in mulib_to_vaino_track and rel_raw:
                    v_tid1 = mulib_to_vaino_track[mtid]
                    r_ids = [int(x.strip("[] ")) for x in rel_raw.split(",") if x.strip("[] ").isdigit()]
                    for r_mtid in r_ids:
                        if r_mtid in mulib_to_vaino_track:
                            v_tid2 = mulib_to_vaino_track[r_mtid]
                            if v_tid1 != v_tid2:
                                rel_inserts.append((v_tid1, v_tid2, 1.0))

            if rel_inserts:
                vconn.executemany("INSERT OR IGNORE INTO track_relations (track_id, related_track_id, relationship_weight) VALUES (?, ?, ?)", rel_inserts)

            vconn.commit()
            mconn.close()
            return {
                "status": "SUCCESS",
                "mapped_tracks": len(mulib_to_vaino_track),
                "mapped_cuts": len(mulib_cut_to_vaino_track),
                "tracks_updated": len(t_updates),
                "artists_updated": len(a_updates),
                "history_imported": len(h_inserts),
                "track_play_counts_updated": len(track_play_counts),
                "artist_play_counts_updated": len(artist_play_counts),
                "relations_imported": len(rel_inserts)
            }
        except Exception as e:
            logger.warning(f"Error importing mulib preferences: {e}")
            return {"status": "ERROR", "error": str(e)}
        finally:
            self.close_connection(vconn)

    # ----------------------------------------------------
    # Play Tracking & Tunable Ratings Methods [REQ-PD-040..080]
    # ----------------------------------------------------
    def record_play(self, track_id: str, play_time: Optional[float] = None):
        """
        [REQ-PD-060] Records completion of a track play, updating play_count and last_played_at
        timestamps for both the track and its associated artist(s).
        """
        import time as _time
        import hashlib
        from datetime import datetime, timezone
        
        if play_time is None:
            play_time = _time.time()

        iso_time = datetime.fromtimestamp(play_time, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S")

        conn = self.get_connection()
        try:
            # 1. Update tracks table
            conn.execute(
                "UPDATE tracks SET play_count = COALESCE(play_count, 0) + 1, last_played_at = ? WHERE id = ?",
                (iso_time, track_id)
            )

            # 2. Insert into play_history
            conn.execute(
                "INSERT INTO play_history (track_id, played_at, completed) VALUES (?, ?, 1)",
                (track_id, iso_time)
            )

            # 3. Fetch track artist(s)
            t_row = conn.execute("SELECT artist, artist_sort_name FROM tracks WHERE id = ?", (track_id,)).fetchone()
            if t_row:
                artist_name = t_row["artist"]
                art_sort = t_row["artist_sort_name"] or artist_name
                
                # Check for decomposed individual artists
                ta_rows = conn.execute("SELECT artist_name, artist_sort_name FROM track_artists WHERE track_id = ?", (track_id,)).fetchall()
                artists_to_update = [(r["artist_name"], r["artist_sort_name"]) for r in ta_rows] if ta_rows else [(artist_name, art_sort)]

                for a_name, a_sort in artists_to_update:
                    a_id = hashlib.md5(a_name.encode("utf-8")).hexdigest()[:16]
                    conn.execute("""
                        INSERT INTO artist_ratings (artist_id, artist_name, artist_sort_name, play_count, last_played_at, rotation, recovery, restraint)
                        VALUES (?, ?, ?, 1, ?, 0.778, 0.778, 0.0)
                        ON CONFLICT(artist_name) DO UPDATE SET
                            play_count = COALESCE(artist_ratings.play_count, 0) + 1,
                            last_played_at = excluded.last_played_at,
                            updated_at = CURRENT_TIMESTAMP
                    """, (a_id, a_name, a_sort, iso_time))

            conn.commit()
        finally:
            self.close_connection(conn)

    def get_track_ratings(self, track_id: str) -> Optional[dict]:
        """Returns ratings dictionary for specified track."""
        conn = self.get_connection()
        try:
            row = conn.execute("SELECT id, title, artist, album, play_count, last_played_at, rotation, recovery, restraint, profanity, occasions FROM tracks WHERE id = ?", (track_id,)).fetchone()
            return dict(row) if row else None
        finally:
            self.close_connection(conn)

    def update_track_ratings(
        self,
        track_id: str,
        rotation: Optional[float] = None,
        recovery: Optional[float] = None,
        restraint: Optional[float] = None,
        profanity: Optional[float] = None,
        occasions: Optional[str] = None
    ) -> Optional[dict]:
        """Updates track rating sliders/fields."""
        conn = self.get_connection()
        try:
            cur = conn.execute("SELECT rotation, recovery, restraint, profanity, occasions FROM tracks WHERE id = ?", (track_id,)).fetchone()
            if not cur:
                return None
            
            new_rot = rotation if rotation is not None else cur["rotation"]
            new_rec = recovery if recovery is not None else cur["recovery"]
            new_res = restraint if restraint is not None else cur["restraint"]
            new_prof = profanity if profanity is not None else cur["profanity"]
            new_occ = occasions if occasions is not None else cur["occasions"]

            conn.execute(
                "UPDATE tracks SET rotation = ?, recovery = ?, restraint = ?, profanity = ?, occasions = ? WHERE id = ?",
                (new_rot, new_rec, new_res, new_prof, new_occ, track_id)
            )
            conn.commit()
            return self.get_track_ratings(track_id)
        finally:
            self.close_connection(conn)

    def get_artist_ratings(self, artist_name: str) -> dict:
        """Returns ratings dictionary for an artist (creating default entry if missing)."""
        import hashlib
        conn = self.get_connection()
        try:
            row = conn.execute("SELECT * FROM artist_ratings WHERE artist_name = ?", (artist_name,)).fetchone()
            if row:
                return dict(row)
            
            a_id = hashlib.md5(artist_name.encode("utf-8")).hexdigest()[:16]
            return {
                "artist_id": a_id,
                "artist_name": artist_name,
                "artist_sort_name": artist_name.upper(),
                "play_count": 0,
                "last_played_at": None,
                "rotation": 0.778,
                "recovery": 0.778,
                "restraint": 0.0,
                "streak_length": 0.0
            }
        finally:
            self.close_connection(conn)

    def update_artist_ratings(
        self,
        artist_name: str,
        rotation: Optional[float] = None,
        recovery: Optional[float] = None,
        restraint: Optional[float] = None,
        streak_length: Optional[float] = None
    ) -> dict:
        """Updates or inserts artist rating sliders/fields."""
        import hashlib
        conn = self.get_connection()
        try:
            cur = conn.execute("SELECT * FROM artist_ratings WHERE artist_name = ?", (artist_name,)).fetchone()
            a_id = hashlib.md5(artist_name.encode("utf-8")).hexdigest()[:16]
            a_sort = artist_name.upper()

            if cur:
                new_rot = rotation if rotation is not None else cur["rotation"]
                new_rec = recovery if recovery is not None else cur["recovery"]
                new_res = restraint if restraint is not None else cur["restraint"]
                new_strk = streak_length if streak_length is not None else cur["streak_length"]

                conn.execute(
                    "UPDATE artist_ratings SET rotation = ?, recovery = ?, restraint = ?, streak_length = ?, updated_at = CURRENT_TIMESTAMP WHERE artist_name = ?",
                    (new_rot, new_rec, new_res, new_strk, artist_name)
                )
            else:
                new_rot = rotation if rotation is not None else 0.778
                new_rec = recovery if recovery is not None else 0.778
                new_res = restraint if restraint is not None else 0.0
                new_strk = streak_length if streak_length is not None else 0.0

                conn.execute(
                    "INSERT INTO artist_ratings (artist_id, artist_name, artist_sort_name, rotation, recovery, restraint, streak_length) VALUES (?, ?, ?, ?, ?, ?, ?)",
                    (a_id, artist_name, a_sort, new_rot, new_rec, new_res, new_strk)
                )

            conn.commit()
            return self.get_artist_ratings(artist_name)
        finally:
            self.close_connection(conn)

    def get_all_artist_ratings(self) -> Dict[str, dict]:
        """Returns map of artist_name to ratings dict."""
        conn = self.get_connection()
        try:
            rows = conn.execute("SELECT * FROM artist_ratings").fetchall()
            return {r["artist_name"]: dict(r) for r in rows}
        finally:
            self.close_connection(conn)

    def get_related_tracks(self, track_id: str) -> List[Tuple[str, float]]:
        """Returns list of (related_track_id, relationship_weight) for specified track."""
        conn = self.get_connection()
        try:
            rows = conn.execute("SELECT related_track_id, relationship_weight FROM track_relations WHERE track_id = ?", (track_id,)).fetchall()
            return [(r["related_track_id"], float(r["relationship_weight"])) for r in rows]
        finally:
            self.close_connection(conn)

    def add_track_relation(self, track_id: str, related_track_id: str, relationship_weight: float = 1.0):
        """Adds a bi-directional related track link."""
        conn = self.get_connection()
        try:
            conn.execute(
                "INSERT INTO track_relations (track_id, related_track_id, relationship_weight) VALUES (?, ?, ?) ON CONFLICT(track_id, related_track_id) DO UPDATE SET relationship_weight=excluded.relationship_weight",
                (track_id, related_track_id, relationship_weight)
            )
            conn.execute(
                "INSERT INTO track_relations (track_id, related_track_id, relationship_weight) VALUES (?, ?, ?) ON CONFLICT(track_id, related_track_id) DO UPDATE SET relationship_weight=excluded.relationship_weight",
                (related_track_id, track_id, relationship_weight)
            )
            conn.commit()
        finally:
            self.close_connection(conn)



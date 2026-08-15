-- Vaino SQLite Database Schema

CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    file_format TEXT NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT,
    year INTEGER,
    track_number INTEGER,
    duration_ms INTEGER NOT NULL,
    start_offset_ms INTEGER DEFAULT 0,
    end_offset_ms INTEGER DEFAULT NULL,
    has_cover_art BOOLEAN DEFAULT 0,
    file_mtime REAL DEFAULT 0,
    file_size INTEGER DEFAULT 0,
    musicbrainz_track_id TEXT,
    musicbrainz_album_id TEXT,
    artist_sort_name TEXT,
    album_sort_name TEXT,
    title_sort_name TEXT,
    ab_acoustic REAL,
    ab_aggressive REAL,
    ab_bright REAL,
    ab_danceable REAL,
    ab_female REAL,
    ab_happy REAL,
    ab_instrumental REAL,
    ab_party REAL,
    ab_relaxed REAL,
    ab_sad REAL,
    ab_tonal REAL,
    play_count INTEGER DEFAULT 0,
    last_played_at DATETIME DEFAULT NULL,
    rotation REAL DEFAULT 0.0,
    recovery REAL DEFAULT 0.778,
    restraint REAL DEFAULT 0.0,
    profanity REAL DEFAULT 0.0,
    occasions TEXT DEFAULT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

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

CREATE TABLE IF NOT EXISTS track_relations (
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    related_track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    relationship_weight REAL DEFAULT 1.0,
    PRIMARY KEY (track_id, related_track_id)
);

CREATE TABLE IF NOT EXISTS track_audio_descriptors (
    track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    energy REAL,
    valence REAL,
    danceability REAL,
    acousticness REAL,
    instrumentalness REAL,
    speechiness REAL,
    tempo_bpm REAL,
    key_signature TEXT,
    loudness_lufs REAL,
    essentia_json TEXT
);

CREATE TABLE IF NOT EXISTS play_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id TEXT REFERENCES tracks(id),
    played_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed BOOLEAN DEFAULT 1
);

CREATE TABLE IF NOT EXISTS programs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    start_time TEXT NOT NULL,
    track_ids TEXT
);

CREATE INDEX IF NOT EXISTS idx_tracks_file_path ON tracks(file_path);
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);


CREATE TABLE IF NOT EXISTS track_artists (
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    artist_name TEXT NOT NULL,
    artist_sort_name TEXT NOT NULL,
    artist_mbid TEXT,
    PRIMARY KEY (track_id, artist_name)
);

CREATE INDEX IF NOT EXISTS idx_track_artists_artist ON track_artists(artist_name);
CREATE INDEX IF NOT EXISTS idx_track_artists_sort ON track_artists(artist_sort_name);
CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
CREATE INDEX IF NOT EXISTS idx_history_played_at ON play_history(played_at);

CREATE TABLE IF NOT EXISTS player_queue (
    queue_order INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS player_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_track_id TEXT REFERENCES tracks(id) ON DELETE SET NULL,
    playback_state TEXT DEFAULT 'IDLE',
    volume INTEGER DEFAULT 80,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS album_cover_art (
    album_id TEXT PRIMARY KEY,
    album_name TEXT NOT NULL,
    artist_name TEXT,
    image_data BLOB NOT NULL,
    mime_type TEXT NOT NULL,
    source TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_album_cover_art_album ON album_cover_art(album_name);

-- Vaino SQLite Database Schema

CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL UNIQUE,
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
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
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
    loudness_lufs REAL
);

CREATE TABLE IF NOT EXISTS play_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id TEXT REFERENCES tracks(id),
    played_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed BOOLEAN DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);
CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
CREATE INDEX IF NOT EXISTS idx_history_played_at ON play_history(played_at);

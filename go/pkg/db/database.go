package db

import (
	"crypto/md5"
	"database/sql"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/dhowden/tag"
	_ "github.com/mattn/go-sqlite3"
)

type Track struct {
	ID                 string   `json:"id"`
	FilePath           string   `json:"file_path"`
	FileFormat         string   `json:"file_format"`
	Title              string   `json:"title"`
	Artist             string   `json:"artist"`
	Album              *string  `json:"album"`
	Year               *int64   `json:"year"`
	TrackNumber        *int64   `json:"track_number"`
	DurationMs         int64    `json:"duration_ms"`
	StartOffsetMs      int64    `json:"start_offset_ms"`
	EndOffsetMs        *int64   `json:"end_offset_ms"`
	HasCoverArt        bool     `json:"has_cover_art"`
	FileMtime          float64  `json:"file_mtime"`
	FileSize           int64    `json:"file_size"`
	MusicBrainzTrackID *string  `json:"musicbrainz_track_id"`
	MusicBrainzAlbumID *string  `json:"musicbrainz_album_id"`
	ArtistSortName     *string  `json:"artist_sort_name"`
	AlbumSortName      *string  `json:"album_sort_name"`
	TitleSortName      *string  `json:"title_sort_name"`
	AbAcoustic         *float64 `json:"ab_acoustic"`
	AbAggressive       *float64 `json:"ab_aggressive"`
	AbBright           *float64 `json:"ab_bright"`
	AbDanceable        *float64 `json:"ab_danceable"`
	AbFemale           *float64 `json:"ab_female"`
	AbHappy            *float64 `json:"ab_happy"`
	AbInstrumental     *float64 `json:"ab_instrumental"`
	AbParty            *float64 `json:"ab_party"`
	AbRelaxed          *float64 `json:"ab_relaxed"`
	AbSad              *float64 `json:"ab_sad"`
	AbTonal            *float64 `json:"ab_tonal"`
	PlayCount          int64    `json:"play_count"`
	LastPlayedAt       *string  `json:"last_played_at"`
	Rotation           float64  `json:"rotation"`
	Recovery           float64  `json:"recovery"`
	Restraint          float64  `json:"restraint"`
	Profanity          float64  `json:"profanity"`
	Occasions          *string  `json:"occasions"`
}

type TrackRatings struct {
	ID           string   `json:"id"`
	Title        string   `json:"title"`
	Artist       string   `json:"artist"`
	Album        *string  `json:"album"`
	PlayCount    int64    `json:"play_count"`
	LastPlayedAt *string  `json:"last_played_at"`
	Rotation     float64  `json:"rotation"`
	Recovery     float64  `json:"recovery"`
	Restraint    float64  `json:"restraint"`
	Profanity    float64  `json:"profanity"`
	Occasions    *string  `json:"occasions"`
}

type ArtistRatings struct {
	ArtistID       string  `json:"artist_id"`
	ArtistName     string  `json:"artist_name"`
	ArtistSortName string  `json:"artist_sort_name"`
	PlayCount      int64   `json:"play_count"`
	LastPlayedAt   *string `json:"last_played_at"`
	Rotation       float64 `json:"rotation"`
	Recovery       float64 `json:"recovery"`
	Restraint      float64 `json:"restraint"`
	StreakLength   float64 `json:"streak_length"`
}

type Program struct {
	ID        int64  `json:"id"`
	Name      string `json:"name"`
	StartTime string `json:"start_time"`
	TrackIDs  string `json:"track_ids"`
}

type Database struct {
	dbPath string
	db     *sql.DB
	mu     sync.Mutex
}

func NewDatabase(dbPath string) (*Database, error) {
	connStr := dbPath
	if dbPath != ":memory:" {
		connStr = fmt.Sprintf("%s?_journal_mode=WAL&_busy_timeout=5000", dbPath)
	}

	sqlDB, err := sql.Open("sqlite3", connStr)
	if err != nil {
		return nil, fmt.Errorf("failed to open sqlite database: %w", err)
	}

	d := &Database{
		dbPath: dbPath,
		db:     sqlDB,
	}

	if err := d.initDB(); err != nil {
		return nil, fmt.Errorf("failed to initialize schema: %w", err)
	}

	return d, nil
}

func (d *Database) Close() error {
	return d.db.Close()
}

func (d *Database) initDB() error {
	d.mu.Lock()
	defer d.mu.Unlock()

	ddl := `
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

	CREATE TABLE IF NOT EXISTS play_history (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
		played_at DATETIME DEFAULT CURRENT_TIMESTAMP,
		completed BOOLEAN DEFAULT 1
	);

	CREATE TABLE IF NOT EXISTS track_audio_descriptors (
		track_id TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
		energy REAL DEFAULT 0.5,
		valence REAL DEFAULT 0.5,
		danceability REAL DEFAULT 0.5,
		acousticness REAL DEFAULT 0.5,
		instrumentalness REAL DEFAULT 0.5,
		speechiness REAL DEFAULT 0.1,
		tempo_bpm REAL DEFAULT 120.0,
		key_signature TEXT DEFAULT 'C Major',
		loudness_lufs REAL DEFAULT -14.0,
		essentia_json TEXT DEFAULT NULL
	);

	CREATE TABLE IF NOT EXISTS player_state (
		id INTEGER PRIMARY KEY CHECK (id = 1),
		current_track_id TEXT REFERENCES tracks(id),
		playback_state TEXT NOT NULL DEFAULT 'IDLE',
		volume INTEGER NOT NULL DEFAULT 80,
		updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
	);

	CREATE TABLE IF NOT EXISTS player_queue (
		queue_order INTEGER PRIMARY KEY AUTOINCREMENT,
		track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
	);

	CREATE TABLE IF NOT EXISTS programs (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		name TEXT NOT NULL UNIQUE,
		start_time TEXT NOT NULL,
		track_ids TEXT DEFAULT '',
		created_at DATETIME DEFAULT CURRENT_TIMESTAMP
	);

	CREATE TABLE IF NOT EXISTS album_cover_art (
		album_id TEXT PRIMARY KEY,
		album_name TEXT NOT NULL,
		artist_name TEXT,
		image_data BLOB NOT NULL,
		mime_type TEXT NOT NULL DEFAULT 'image/jpeg',
		source TEXT NOT NULL DEFAULT 'EMBEDDED',
		updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
	);

	CREATE TABLE IF NOT EXISTS track_artists (
		track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
		artist_name TEXT NOT NULL,
		artist_sort_name TEXT NOT NULL,
		PRIMARY KEY (track_id, artist_name)
	);
	`

	_, err := d.db.Exec(ddl)
	if err != nil {
		return err
	}

	// Auto-migrations for extra columns
	migrations := []string{
		"ALTER TABLE tracks ADD COLUMN play_count INTEGER DEFAULT 0",
		"ALTER TABLE tracks ADD COLUMN last_played_at DATETIME DEFAULT NULL",
		"ALTER TABLE tracks ADD COLUMN rotation REAL DEFAULT 0.0",
		"ALTER TABLE tracks ADD COLUMN recovery REAL DEFAULT 0.778",
		"ALTER TABLE tracks ADD COLUMN restraint REAL DEFAULT 0.0",
		"ALTER TABLE tracks ADD COLUMN profanity REAL DEFAULT 0.0",
		"ALTER TABLE tracks ADD COLUMN occasions TEXT DEFAULT NULL",
	}

	for _, m := range migrations {
		_, _ = d.db.Exec(m)
	}

	_ = d.EnsureTrackArtists()
	_ = d.EnsureSortNames()

	return nil
}

func (d *Database) RecordPlay(trackID string, playTime time.Time) error {
	d.mu.Lock()
	defer d.mu.Unlock()

	isoTime := playTime.UTC().Format("2006-01-02 15:04:05")

	// 1. Update tracks table
	_, err := d.db.Exec(`
		UPDATE tracks
		SET play_count = COALESCE(play_count, 0) + 1,
		    last_played_at = ?
		WHERE id = ?
	`, isoTime, trackID)
	if err != nil {
		return err
	}

	// 2. Insert into play_history
	_, _ = d.db.Exec(`
		INSERT INTO play_history (track_id, played_at, completed) VALUES (?, ?, 1)
	`, trackID, isoTime)

	// 3. Find track artist to update artist_ratings
	var artistName string
	err = d.db.QueryRow("SELECT artist FROM tracks WHERE id = ?", trackID).Scan(&artistName)
	if err == nil && artistName != "" {
		artistID := fmt.Sprintf("%x", md5.Sum([]byte(artistName)))[:16]
		_, _ = d.db.Exec(`
			INSERT INTO artist_ratings (artist_id, artist_name, artist_sort_name, play_count, last_played_at)
			VALUES (?, ?, ?, 1, ?)
			ON CONFLICT(artist_name) DO UPDATE SET
				play_count = artist_ratings.play_count + 1,
				last_played_at = excluded.last_played_at,
				updated_at = CURRENT_TIMESTAMP
		`, artistID, artistName, strings.ToUpper(artistName), isoTime)
	}

	return nil
}

func (d *Database) GetTrackRatings(trackID string) (*TrackRatings, error) {
	query := `
		SELECT id, title, artist, album, play_count, last_played_at, rotation, recovery, restraint, profanity, occasions
		FROM tracks WHERE id = ?
	`
	row := d.db.QueryRow(query, trackID)

	var tr TrackRatings
	var albumSql, lastPlayedSql, occasionsSql sql.NullString

	err := row.Scan(
		&tr.ID, &tr.Title, &tr.Artist, &albumSql, &tr.PlayCount, &lastPlayedSql,
		&tr.Rotation, &tr.Recovery, &tr.Restraint, &tr.Profanity, &occasionsSql,
	)
	if err != nil {
		return nil, err
	}

	if albumSql.Valid {
		tr.Album = &albumSql.String
	}
	if lastPlayedSql.Valid {
		tr.LastPlayedAt = &lastPlayedSql.String
	}
	if occasionsSql.Valid {
		tr.Occasions = &occasionsSql.String
	}

	return &tr, nil
}

func (d *Database) UpdateTrackRatings(trackID string, rotation, recovery, restraint, profanity float64, occasions *string) (*TrackRatings, error) {
	d.mu.Lock()
	defer d.mu.Unlock()

	_, err := d.db.Exec(`
		UPDATE tracks
		SET rotation = ?, recovery = ?, restraint = ?, profanity = ?, occasions = ?
		WHERE id = ?
	`, rotation, recovery, restraint, profanity, occasions, trackID)
	if err != nil {
		return nil, err
	}

	return d.GetTrackRatings(trackID)
}

func (d *Database) GetArtistRatings(artistName string) (*ArtistRatings, error) {
	query := `
		SELECT artist_id, artist_name, artist_sort_name, play_count, last_played_at, rotation, recovery, restraint, streak_length
		FROM artist_ratings WHERE artist_name = ?
	`
	row := d.db.QueryRow(query, artistName)

	var ar ArtistRatings
	var lastPlayedSql sql.NullString

	err := row.Scan(
		&ar.ArtistID, &ar.ArtistName, &ar.ArtistSortName, &ar.PlayCount, &lastPlayedSql,
		&ar.Rotation, &ar.Recovery, &ar.Restraint, &ar.StreakLength,
	)
	if err == sql.ErrNoRows {
		artistID := fmt.Sprintf("%x", md5.Sum([]byte(artistName)))[:16]
		return &ArtistRatings{
			ArtistID:       artistID,
			ArtistName:     artistName,
			ArtistSortName: strings.ToUpper(artistName),
			PlayCount:      0,
			Rotation:       0.778,
			Recovery:       0.778,
			Restraint:      0.0,
			StreakLength:   0.0,
		}, nil
	} else if err != nil {
		return nil, err
	}

	if lastPlayedSql.Valid {
		ar.LastPlayedAt = &lastPlayedSql.String
	}

	return &ar, nil
}

func (d *Database) UpdateArtistRatings(artistName string, rotation, recovery, restraint, streakLength float64) (*ArtistRatings, error) {
	d.mu.Lock()
	defer d.mu.Unlock()

	artistID := fmt.Sprintf("%x", md5.Sum([]byte(artistName)))[:16]
	sortName := strings.ToUpper(artistName)

	_, err := d.db.Exec(`
		INSERT INTO artist_ratings (artist_id, artist_name, artist_sort_name, rotation, recovery, restraint, streak_length)
		VALUES (?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(artist_name) DO UPDATE SET
			rotation = excluded.rotation,
			recovery = excluded.recovery,
			restraint = excluded.restraint,
			streak_length = excluded.streak_length,
			updated_at = CURRENT_TIMESTAMP
	`, artistID, artistName, sortName, rotation, recovery, restraint, streakLength)
	if err != nil {
		return nil, err
	}

	return d.GetArtistRatings(artistName)
}

func (d *Database) GetAllArtistRatings() (map[string]*ArtistRatings, error) {
	rows, err := d.db.Query(`SELECT artist_id, artist_name, artist_sort_name, play_count, last_played_at, rotation, recovery, restraint, streak_length FROM artist_ratings`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	res := make(map[string]*ArtistRatings)
	for rows.Next() {
		var ar ArtistRatings
		var lastPlayedSql sql.NullString
		if err := rows.Scan(&ar.ArtistID, &ar.ArtistName, &ar.ArtistSortName, &ar.PlayCount, &lastPlayedSql, &ar.Rotation, &ar.Recovery, &ar.Restraint, &ar.StreakLength); err != nil {
			continue
		}
		if lastPlayedSql.Valid {
			ar.LastPlayedAt = &lastPlayedSql.String
		}
		res[ar.ArtistName] = &ar
	}

	return res, nil
}

func (d *Database) GetTotalTrackCount(query, artist, album, letter string) (int, error) {
	whereClauses := []string{}
	params := []interface{}{}
	joinClause := ""

	if artist != "" {
		joinClause = "JOIN track_artists ta ON t.id = ta.track_id"
		whereClauses = append(whereClauses, "(ta.artist_name = ? OR t.artist = ?)")
		params = append(params, artist, artist)
	}
	if album != "" {
		whereClauses = append(whereClauses, "t.album = ?")
		params = append(params, album)
	}
	if letter != "" {
		if letter == "#" {
			whereClauses = append(whereClauses, "COALESCE(t.title_sort_name, t.title) GLOB '[0-9]*'")
		} else {
			whereClauses = append(whereClauses, "COALESCE(t.title_sort_name, t.title) LIKE ?")
			params = append(params, letter+"%")
		}
	}
	if query != "" {
		q := "%" + query + "%"
		whereClauses = append(whereClauses, "(t.title LIKE ? OR t.artist LIKE ? OR t.album LIKE ? OR t.artist_sort_name LIKE ?)")
		params = append(params, q, q, q, q)
	}

	whereStr := ""
	if len(whereClauses) > 0 {
		whereStr = "WHERE " + strings.Join(whereClauses, " AND ")
	}

	sqlQuery := fmt.Sprintf("SELECT COUNT(DISTINCT t.id) FROM tracks t %s %s", joinClause, whereStr)
	var count int
	var err error
	if len(params) == 0 {
		err = d.db.QueryRow(sqlQuery).Scan(&count)
	} else {
		err = d.db.QueryRow(sqlQuery, params...).Scan(&count)
	}
	return count, err
}

func (d *Database) GetTotalAlbumCount(query, artist, letter string) (int, error) {
	whereClauses := []string{}
	params := []interface{}{}
	joinClause := ""

	if artist != "" {
		joinClause = "JOIN track_artists ta ON t.id = ta.track_id"
		whereClauses = append(whereClauses, "(ta.artist_name = ? OR t.artist = ?)")
		params = append(params, artist, artist)
	}
	if letter != "" {
		if letter == "#" {
			whereClauses = append(whereClauses, "COALESCE(t.album_sort_name, t.album) GLOB '[0-9]*'")
		} else {
			whereClauses = append(whereClauses, "COALESCE(t.album_sort_name, t.album) LIKE ?")
			params = append(params, letter+"%")
		}
	}
	if query != "" {
		q := "%" + query + "%"
		whereClauses = append(whereClauses, "(t.album LIKE ? OR t.artist LIKE ? OR t.artist_sort_name LIKE ?)")
		params = append(params, q, q, q)
	}

	whereStr := ""
	if len(whereClauses) > 0 {
		whereStr = "WHERE " + strings.Join(whereClauses, " AND ")
	}

	sqlQuery := fmt.Sprintf("SELECT COUNT(DISTINCT t.album) FROM tracks t %s %s", joinClause, whereStr)
	var count int
	var err error
	if len(params) == 0 {
		err = d.db.QueryRow(sqlQuery).Scan(&count)
	} else {
		err = d.db.QueryRow(sqlQuery, params...).Scan(&count)
	}
	return count, err
}

func (d *Database) GetTotalArtistCount(query, letter string) (int, error) {
	whereClauses := []string{}
	params := []interface{}{}

	if letter != "" {
		if letter == "#" {
			whereClauses = append(whereClauses, "COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist) GLOB '[0-9]*'")
		} else {
			whereClauses = append(whereClauses, "COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist) LIKE ?")
			params = append(params, letter+"%")
		}
	}
	if query != "" {
		q := "%" + query + "%"
		whereClauses = append(whereClauses, "(ta.artist_name LIKE ? OR t.album LIKE ? OR COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist) LIKE ?)")
		params = append(params, q, q, q)
	}

	whereStr := ""
	if len(whereClauses) > 0 {
		whereStr = "WHERE " + strings.Join(whereClauses, " AND ")
	}

	sqlQuery := fmt.Sprintf("SELECT COUNT(DISTINCT ta.artist_name) FROM track_artists ta JOIN tracks t ON ta.track_id = t.id %s", whereStr)
	var count int
	var err error
	if len(params) == 0 {
		err = d.db.QueryRow(sqlQuery).Scan(&count)
	} else {
		err = d.db.QueryRow(sqlQuery, params...).Scan(&count)
	}
	return count, err
}

const trackSelectCols = `id, file_path, file_format, title, artist, album, year, track_number,
       duration_ms, start_offset_ms, end_offset_ms, has_cover_art, file_mtime, file_size,
       musicbrainz_track_id, musicbrainz_album_id, artist_sort_name, album_sort_name, title_sort_name,
       ab_acoustic, ab_aggressive, ab_bright, ab_danceable, ab_female, ab_happy, ab_instrumental,
       ab_party, ab_relaxed, ab_sad, ab_tonal, play_count, last_played_at, rotation, recovery,
       restraint, profanity, occasions`

type scannable interface {
	Scan(dest ...interface{}) error
}

func scanTrackRow(s scannable) (*Track, error) {
	var t Track
	var album, mbTrack, mbAlbum, artistSort, albumSort, titleSort, lastPlayed, occasions sql.NullString
	var year, trackNum, endOffset sql.NullInt64
	var abAc, abAg, abBr, abDa, abFe, abHa, abIn, abPa, abRe, abSa, abTo sql.NullFloat64

	err := s.Scan(
		&t.ID, &t.FilePath, &t.FileFormat, &t.Title, &t.Artist, &album, &year, &trackNum,
		&t.DurationMs, &t.StartOffsetMs, &endOffset, &t.HasCoverArt, &t.FileMtime, &t.FileSize,
		&mbTrack, &mbAlbum, &artistSort, &albumSort, &titleSort,
		&abAc, &abAg, &abBr, &abDa, &abFe, &abHa, &abIn, &abPa, &abRe, &abSa, &abTo,
		&t.PlayCount, &lastPlayed, &t.Rotation, &t.Recovery, &t.Restraint, &t.Profanity, &occasions,
	)
	if err != nil {
		return nil, err
	}

	if album.Valid { t.Album = &album.String }
	if year.Valid { t.Year = &year.Int64 }
	if trackNum.Valid { t.TrackNumber = &trackNum.Int64 }
	if endOffset.Valid { t.EndOffsetMs = &endOffset.Int64 }
	if mbTrack.Valid { t.MusicBrainzTrackID = &mbTrack.String }
	if mbAlbum.Valid { t.MusicBrainzAlbumID = &mbAlbum.String }
	if artistSort.Valid { t.ArtistSortName = &artistSort.String }
	if albumSort.Valid { t.AlbumSortName = &albumSort.String }
	if titleSort.Valid { t.TitleSortName = &titleSort.String }
	if abAc.Valid { t.AbAcoustic = &abAc.Float64 }
	if abAg.Valid { t.AbAggressive = &abAg.Float64 }
	if abBr.Valid { t.AbBright = &abBr.Float64 }
	if abDa.Valid { t.AbDanceable = &abDa.Float64 }
	if abFe.Valid { t.AbFemale = &abFe.Float64 }
	if abHa.Valid { t.AbHappy = &abHa.Float64 }
	if abIn.Valid { t.AbInstrumental = &abIn.Float64 }
	if abPa.Valid { t.AbParty = &abPa.Float64 }
	if abRe.Valid { t.AbRelaxed = &abRe.Float64 }
	if abSa.Valid { t.AbSad = &abSa.Float64 }
	if abTo.Valid { t.AbTonal = &abTo.Float64 }
	if lastPlayed.Valid { t.LastPlayedAt = &lastPlayed.String }
	if occasions.Valid { t.Occasions = &occasions.String }

	return &t, nil
}

func (d *Database) GetAllTracks(limit, offset int, query, artist, album, letter string) ([]Track, error) {
	whereClauses := []string{}
	params := []interface{}{}
	joinClause := ""

	if artist != "" {
		joinClause = "JOIN track_artists ta ON t.id = ta.track_id"
		whereClauses = append(whereClauses, "(ta.artist_name = ? OR t.artist = ?)")
		params = append(params, artist, artist)
	}
	if album != "" {
		whereClauses = append(whereClauses, "t.album = ?")
		params = append(params, album)
	}
	if letter != "" {
		if letter == "#" {
			whereClauses = append(whereClauses, "COALESCE(t.title_sort_name, t.title) GLOB '[0-9]*'")
		} else {
			whereClauses = append(whereClauses, "COALESCE(t.title_sort_name, t.title) LIKE ?")
			params = append(params, letter+"%")
		}
	}
	if query != "" {
		q := "%" + query + "%"
		whereClauses = append(whereClauses, "(t.title LIKE ? OR t.artist LIKE ? OR t.album LIKE ? OR t.artist_sort_name LIKE ?)")
		params = append(params, q, q, q, q)
	}

	whereStr := ""
	if len(whereClauses) > 0 {
		whereStr = "WHERE " + strings.Join(whereClauses, " AND ")
	}

	orderClause := "ORDER BY COALESCE(t.title_sort_name, t.title) ASC, t.artist ASC"
	if album != "" {
		orderClause = "ORDER BY CASE WHEN t.track_number IS NULL OR t.track_number = 0 THEN 999 ELSE t.track_number END ASC, COALESCE(t.title_sort_name, t.title) ASC"
	}

	sqlQuery := fmt.Sprintf(`
		SELECT DISTINCT t.id, t.file_path, t.file_format, t.title, t.artist, t.album, t.year, t.track_number,
		       t.duration_ms, t.start_offset_ms, t.end_offset_ms, t.has_cover_art, t.file_mtime, t.file_size,
		       t.musicbrainz_track_id, t.musicbrainz_album_id, t.artist_sort_name, t.album_sort_name, t.title_sort_name,
		       t.ab_acoustic, t.ab_aggressive, t.ab_bright, t.ab_danceable, t.ab_female, t.ab_happy, t.ab_instrumental,
		       t.ab_party, t.ab_relaxed, t.ab_sad, t.ab_tonal, t.play_count, t.last_played_at, t.rotation, t.recovery,
		       t.restraint, t.profanity, t.occasions
		FROM tracks t
		%s
		%s
		%s
		LIMIT ? OFFSET ?
	`, joinClause, whereStr, orderClause)

	params = append(params, limit, offset)

	rows, err := d.db.Query(sqlQuery, params...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	tracks := make([]Track, 0)
	for rows.Next() {
		t, err := scanTrackRow(rows)
		if err != nil {
			log.Printf("[DB] Scan error in GetAllTracks: %v", err)
			continue
		}
		tracks = append(tracks, *t)
	}

	return tracks, nil
}

func (d *Database) GetTrackByID(trackID string) (*Track, error) {
	query := fmt.Sprintf(`SELECT %s FROM tracks WHERE id = ?`, trackSelectCols)
	row := d.db.QueryRow(query, trackID)
	return scanTrackRow(row)
}

func (d *Database) GetTrackDescriptors(trackID string) (map[string]interface{}, error) {
	res := make(map[string]interface{})

	// 1. Check track_audio_descriptors
	var energy, valence, danceability, acousticness, instrumentalness, speechiness, tempoBpm, loudnessLufs sql.NullFloat64
	var keySig, essentiaJson sql.NullString

	err := d.db.QueryRow(`
		SELECT energy, valence, danceability, acousticness, instrumentalness, speechiness, tempo_bpm, key_signature, loudness_lufs, essentia_json
		FROM track_audio_descriptors WHERE track_id = ?
	`, trackID).Scan(&energy, &valence, &danceability, &acousticness, &instrumentalness, &speechiness, &tempoBpm, &keySig, &loudnessLufs, &essentiaJson)

	if err == nil {
		if energy.Valid {
			res["energy"] = energy.Float64
		}
		if valence.Valid {
			res["valence"] = valence.Float64
		}
		if tempoBpm.Valid {
			res["tempo_bpm"] = tempoBpm.Float64
		}
	}

	// 2. Check 11D ab_* columns in tracks
	row := d.db.QueryRow(`
		SELECT ab_acoustic, ab_aggressive, ab_bright, ab_danceable, ab_female, ab_happy,
		       ab_instrumental, ab_party, ab_relaxed, ab_sad, ab_tonal
		FROM tracks WHERE id = ?
	`, trackID)

	var abAc, abAg, abBr, abDa, abFe, abHa, abIn, abPa, abRe, abSa, abTo sql.NullFloat64
	if err := row.Scan(&abAc, &abAg, &abBr, &abDa, &abFe, &abHa, &abIn, &abPa, &abRe, &abSa, &abTo); err == nil {
		if abAc.Valid {
			res["ab_acoustic"] = abAc.Float64
		}
		if abAg.Valid {
			res["ab_aggressive"] = abAg.Float64
		}
		if abBr.Valid {
			res["ab_bright"] = abBr.Float64
		}
		if abDa.Valid {
			res["ab_danceable"] = abDa.Float64
		}
		if abFe.Valid {
			res["ab_female"] = abFe.Float64
		}
		if abHa.Valid {
			res["ab_happy"] = abHa.Float64
		}
		if abIn.Valid {
			res["ab_instrumental"] = abIn.Float64
		}
		if abPa.Valid {
			res["ab_party"] = abPa.Float64
		}
		if abRe.Valid {
			res["ab_relaxed"] = abRe.Float64
		}
		if abSa.Valid {
			res["ab_sad"] = abSa.Float64
		}
		if abTo.Valid {
			res["ab_tonal"] = abTo.Float64
		}
	}

	return res, nil
}

func (d *Database) GetRelatedTracks(trackID string) ([][2]interface{}, error) {
	rows, err := d.db.Query(`SELECT related_track_id, relationship_weight FROM track_relations WHERE track_id = ?`, trackID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var rels [][2]interface{}
	for rows.Next() {
		var relID string
		var weight float64
		if err := rows.Scan(&relID, &weight); err == nil {
			rels = append(rels, [2]interface{}{relID, weight})
		}
	}
	return rels, nil
}

func (d *Database) ImportMulibPreferences(mulibPath string) (map[string]interface{}, error) {
	d.mu.Lock()
	defer d.mu.Unlock()

	if _, err := os.Stat(mulibPath); os.IsNotExist(err) {
		return nil, fmt.Errorf("file not found: %s", mulibPath)
	}

	mconn, err := sql.Open("sqlite3", mulibPath)
	if err != nil {
		return nil, fmt.Errorf("failed to open mulib.db: %w", err)
	}
	defer mconn.Close()

	// 1. Build mappings
	vRows, err := d.db.Query(`SELECT id, title, artist, file_path, musicbrainz_track_id FROM tracks`)
	if err != nil {
		return nil, err
	}
	defer vRows.Close()

	vByMbid := make(map[string]string)
	vByTitleArtist := make(map[string]string)
	vByFiletail := make(map[string]string)

	for vRows.Next() {
		var id, title, artist, filePath string
		var mbid sql.NullString
		if err := vRows.Scan(&id, &title, &artist, &filePath, &mbid); err == nil {
			if mbid.Valid && mbid.String != "" {
				vByMbid[mbid.String] = id
			}
			if title != "" && artist != "" {
				key := strings.ToLower(title) + "||" + strings.ToLower(artist)
				vByTitleArtist[key] = id
			}
			if filePath != "" {
				tail := strings.ToLower(filepath.Base(filePath))
				vByFiletail[tail] = id
			}
		}
	}

	// Read mulib tracks & cuts
	mulibToVainoTrack := make(map[int64]string)
	cutRows, err := mconn.Query(`
		SELECT c.cutId, c.trackId, f.filePath, t.mbidRecording, t.name as title
		FROM cuts c
		JOIN tracks t ON c.trackId = t.trackId
		LEFT JOIN files f ON c.fileId = f.fileId
	`)
	if err == nil {
		defer cutRows.Close()
		for cutRows.Next() {
			var cutID, trackID int64
			var filePath, mbid, title sql.NullString
			if err := cutRows.Scan(&cutID, &trackID, &filePath, &mbid, &title); err == nil {
				var vTid string
				if mbid.Valid && vByMbid[mbid.String] != "" {
					vTid = vByMbid[mbid.String]
				} else if filePath.Valid && filePath.String != "" {
					ftail := strings.ToLower(filepath.Base(filePath.String))
					if vByFiletail[ftail] != "" {
						vTid = vByFiletail[ftail]
					}
				}
				if vTid != "" {
					mulibToVainoTrack[trackID] = vTid
				}
			}
		}
	}

	// Track ratings import count
	mTracks, err := mconn.Query(`SELECT trackId, rotation, recovery, restraint, profanity, occasions FROM tracks`)
	tracksUpdated := 0
	if err == nil {
		defer mTracks.Close()
		tx, _ := d.db.Begin()
		stmt, _ := tx.Prepare(`
			UPDATE tracks SET rotation = ?, recovery = ?, restraint = ?, profanity = ?, occasions = ? WHERE id = ?
		`)
		for mTracks.Next() {
			var mtid int64
			var rot, rec, res, prof sql.NullFloat64
			var occ sql.NullString
			if err := mTracks.Scan(&mtid, &rot, &rec, &res, &prof, &occ); err == nil {
				if vTid, ok := mulibToVainoTrack[mtid]; ok {
					rVal := 0.0
					if rot.Valid {
						rVal = rot.Float64
					}
					recVal := 0.778
					if rec.Valid {
						recVal = rec.Float64
					}
					resVal := 0.0
					if res.Valid {
						resVal = res.Float64
					}
					pVal := 0.0
					if prof.Valid {
						pVal = prof.Float64
					}
					var oVal *string
					if occ.Valid {
						oVal = &occ.String
					}

					_, _ = stmt.Exec(rVal, recVal, resVal, pVal, oVal, vTid)
					tracksUpdated++
				}
			}
		}
		if stmt != nil {
			stmt.Close()
		}
		_ = tx.Commit()
	}

	return map[string]interface{}{
		"status":         "SUCCESS",
		"mapped_tracks":  len(mulibToVainoTrack),
		"tracks_updated": tracksUpdated,
	}, nil
}

func (d *Database) GetAllAlbums(limit, offset int, query, artist, letter string) ([]map[string]interface{}, error) {
	whereClauses := []string{}
	params := []interface{}{}
	joinClause := ""

	if artist != "" {
		joinClause = "JOIN track_artists ta ON t.id = ta.track_id"
		whereClauses = append(whereClauses, "(ta.artist_name = ? OR t.artist = ?)")
		params = append(params, artist, artist)
	}
	if letter != "" {
		if letter == "#" {
			whereClauses = append(whereClauses, "COALESCE(t.album_sort_name, t.album) GLOB '[0-9]*'")
		} else {
			whereClauses = append(whereClauses, "COALESCE(t.album_sort_name, t.album) LIKE ?")
			params = append(params, letter+"%")
		}
	}
	if query != "" {
		q := "%" + query + "%"
		whereClauses = append(whereClauses, "(t.album LIKE ? OR t.artist LIKE ? OR t.artist_sort_name LIKE ?)")
		params = append(params, q, q, q)
	}

	whereStr := ""
	if len(whereClauses) > 0 {
		whereStr = "WHERE " + strings.Join(whereClauses, " AND ")
	}

	sqlQuery := fmt.Sprintf(`
		SELECT t.album,
		       MIN(COALESCE(t.album_sort_name, t.album)) as album_sort_name,
		       COALESCE(MAX(CASE WHEN t.artist = ? THEN t.artist END), MIN(t.artist)) as artist,
		       MIN(t.year) as year,
		       COUNT(DISTINCT t.id) as track_count,
		       COALESCE(MAX(CASE WHEN t.has_cover_art = 1 THEN t.id END), MIN(t.id)) as sample_track_id
		FROM tracks t
		%s
		%s
		GROUP BY t.album
		ORDER BY MIN(COALESCE(t.album_sort_name, t.album)) ASC
		LIMIT ? OFFSET ?
	`, joinClause, whereStr)

	allParams := []interface{}{artist}
	allParams = append(allParams, params...)
	allParams = append(allParams, limit, offset)

	rows, err := d.db.Query(sqlQuery, allParams...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	albums := make([]map[string]interface{}, 0)
	for rows.Next() {
		var album, albumSort, artistName, sampleTrackID sql.NullString
		var year, trackCount sql.NullInt64
		if err := rows.Scan(&album, &albumSort, &artistName, &year, &trackCount, &sampleTrackID); err == nil {
			albums = append(albums, map[string]interface{}{
				"album":           album.String,
				"album_sort_name": albumSort.String,
				"artist":          artistName.String,
				"year":            year.Int64,
				"track_count":     trackCount.Int64,
				"sample_track_id": sampleTrackID.String,
			})
		}
	}
	return albums, nil
}

func (d *Database) GetAllArtists(limit, offset int, query, letter string) ([]map[string]interface{}, error) {
	whereClauses := []string{}
	params := []interface{}{}

	if letter != "" {
		if letter == "#" {
			whereClauses = append(whereClauses, "COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist) GLOB '[0-9]*'")
		} else {
			whereClauses = append(whereClauses, "COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist) LIKE ?")
			params = append(params, letter+"%")
		}
	}
	if query != "" {
		q := "%" + query + "%"
		whereClauses = append(whereClauses, "(ta.artist_name LIKE ? OR t.album LIKE ? OR COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist) LIKE ?)")
		params = append(params, q, q, q)
	}

	whereStr := ""
	if len(whereClauses) > 0 {
		whereStr = "WHERE " + strings.Join(whereClauses, " AND ")
	}

	sqlQuery := fmt.Sprintf(`
		SELECT ta.artist_name as artist,
		       MIN(COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist)) as artist_sort_name,
		       COUNT(DISTINCT t.album) as album_count,
		       COUNT(DISTINCT t.id) as track_count,
		       COALESCE(MAX(CASE WHEN t.has_cover_art = 1 THEN t.id END), MIN(t.id)) as sample_track_id
		FROM track_artists ta
		JOIN tracks t ON ta.track_id = t.id
		%s
		GROUP BY ta.artist_name
		ORDER BY MIN(COALESCE(ta.artist_sort_name, t.artist_sort_name, t.artist)) ASC
		LIMIT ? OFFSET ?
	`, whereStr)

	params = append(params, limit, offset)
	rows, err := d.db.Query(sqlQuery, params...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	artists := make([]map[string]interface{}, 0)
	for rows.Next() {
		var artistName, artistSort, sampleTrackID sql.NullString
		var albumCount, trackCount sql.NullInt64
		if err := rows.Scan(&artistName, &artistSort, &albumCount, &trackCount, &sampleTrackID); err == nil {
			artists = append(artists, map[string]interface{}{
				"artist":           artistName.String,
				"artist_sort_name": artistSort.String,
				"album_count":      albumCount.Int64,
				"track_count":      trackCount.Int64,
				"sample_track_id":  sampleTrackID.String,
			})
		}
	}
	return artists, nil
}

func (d *Database) GetAlbumTracks(albumName, artistName string) ([]Track, error) {
	whereClause := "album = ?"
	params := []interface{}{albumName}

	if artistName != "" {
		whereClause += " AND artist = ?"
		params = append(params, artistName)
	}

	query := fmt.Sprintf(`
		SELECT %s FROM tracks
		WHERE %s
		ORDER BY CASE WHEN track_number IS NULL OR track_number = 0 THEN 999 ELSE track_number END ASC, start_offset_ms ASC, title ASC
	`, trackSelectCols, whereClause)

	rows, err := d.db.Query(query, params...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	tracks := make([]Track, 0)
	for rows.Next() {
		t, err := scanTrackRow(rows)
		if err == nil {
			tracks = append(tracks, *t)
		}
	}
	return tracks, nil
}

func (d *Database) SaveAlbumCoverArt(albumName, artistName string, imageBytes []byte, mimeType, source string) (string, error) {
	coverID := fmt.Sprintf("%x", md5.Sum([]byte(strings.ToLower(albumName+"||"+artistName))))[:16]
	query := `
		INSERT INTO album_cover_art (album_id, album_name, artist_name, image_data, mime_type, source)
		VALUES (?, ?, ?, ?, ?, ?)
		ON CONFLICT(album_id) DO UPDATE SET
			image_data = excluded.image_data,
			mime_type = excluded.mime_type,
			updated_at = CURRENT_TIMESTAMP
	`
	_, err := d.db.Exec(query, coverID, albumName, artistName, imageBytes, mimeType, source)
	return coverID, err
}

func (d *Database) InsertTrack(t *Track) error {
	var album, mbTrack, mbAlbum, artistSort, albumSort, titleSort, lastPlayed, occasions interface{}
	var year, trackNum, endOffset interface{}

	if t.Album != nil { album = *t.Album }
	if t.Year != nil { year = *t.Year }
	if t.TrackNumber != nil { trackNum = *t.TrackNumber }
	if t.EndOffsetMs != nil { endOffset = *t.EndOffsetMs }
	if t.MusicBrainzTrackID != nil { mbTrack = *t.MusicBrainzTrackID }
	if t.MusicBrainzAlbumID != nil { mbAlbum = *t.MusicBrainzAlbumID }
	if t.ArtistSortName != nil { artistSort = *t.ArtistSortName }
	if t.AlbumSortName != nil { albumSort = *t.AlbumSortName }
	if t.TitleSortName != nil { titleSort = *t.TitleSortName }
	if t.LastPlayedAt != nil { lastPlayed = *t.LastPlayedAt }
	if t.Occasions != nil { occasions = *t.Occasions }

	query := `
		INSERT OR REPLACE INTO tracks (
			id, file_path, file_format, title, artist, album, year, track_number,
			duration_ms, start_offset_ms, end_offset_ms, has_cover_art, file_mtime, file_size,
			musicbrainz_track_id, musicbrainz_album_id, artist_sort_name, album_sort_name, title_sort_name,
			ab_acoustic, ab_aggressive, ab_bright, ab_danceable, ab_female, ab_happy, ab_instrumental,
			ab_party, ab_relaxed, ab_sad, ab_tonal, play_count, last_played_at, rotation, recovery,
			restraint, profanity, occasions
		) VALUES (
			?, ?, ?, ?, ?, ?, ?, ?,
			?, ?, ?, ?, ?, ?,
			?, ?, ?, ?, ?,
			?, ?, ?, ?, ?, ?, ?,
			?, ?, ?, ?, ?, ?, ?, ?,
			?, ?, ?
		)
	`
	hasCover := 0
	if t.HasCoverArt { hasCover = 1 }

	_, err := d.db.Exec(query,
		t.ID, t.FilePath, t.FileFormat, t.Title, t.Artist, album, year, trackNum,
		t.DurationMs, t.StartOffsetMs, endOffset, hasCover, t.FileMtime, t.FileSize,
		mbTrack, mbAlbum, artistSort, albumSort, titleSort,
		t.AbAcoustic, t.AbAggressive, t.AbBright, t.AbDanceable, t.AbFemale, t.AbHappy, t.AbInstrumental,
		t.AbParty, t.AbRelaxed, t.AbSad, t.AbTonal, t.PlayCount, lastPlayed, t.Rotation, t.Recovery,
		t.Restraint, t.Profanity, occasions,
	)
	return err
}

func (d *Database) GetCoverArt(id string) ([]byte, string, error) {
	row := d.db.QueryRow(`
		SELECT image_data, mime_type FROM album_cover_art
		WHERE album_id = ? OR album_name = ? OR artist_name = ?
		LIMIT 1
	`, id, id, id)
	var data []byte
	var mimeType string
	if err := row.Scan(&data, &mimeType); err == nil && len(data) > 0 {
		return data, mimeType, nil
	}

	track, errTrack := d.GetTrackByID(id)
	if errTrack == nil && track != nil {
		albumName := ""
		if track.Album != nil {
			albumName = *track.Album
		}

		if albumName != "" || track.Artist != "" {
			rowSub := d.db.QueryRow(`
				SELECT image_data, mime_type FROM album_cover_art
				WHERE album_name = ? OR artist_name = ?
				LIMIT 1
			`, albumName, track.Artist)
			if errSub := rowSub.Scan(&data, &mimeType); errSub == nil && len(data) > 0 {
				return data, mimeType, nil
			}
		}

		if track.FilePath != "" {
			if f, errFile := os.Open(track.FilePath); errFile == nil {
				m, errTag := tag.ReadFrom(f)
				f.Close()
				if errTag == nil && m.Picture() != nil {
					pic := m.Picture()
					mt := pic.MIMEType
					if mt == "" {
						mt = "image/jpeg"
					}
					if albumName != "" {
						_, _ = d.SaveAlbumCoverArt(albumName, track.Artist, pic.Data, mt, "EMBEDDED")
					}
					return pic.Data, mt, nil
				}
			}

			dir := filepath.Dir(track.FilePath)
			for _, imgName := range []string{"cover.jpg", "folder.jpg", "front.jpg", "album.jpg", "cover.png", "folder.png", "front.png"} {
				imgPath := filepath.Join(dir, imgName)
				if imgData, errImg := os.ReadFile(imgPath); errImg == nil && len(imgData) > 0 {
					mt := "image/jpeg"
					if strings.HasSuffix(strings.ToLower(imgName), ".png") {
						mt = "image/png"
					}
					if albumName != "" {
						_, _ = d.SaveAlbumCoverArt(albumName, track.Artist, imgData, mt, "FOLDER")
					}
					return imgData, mt, nil
				}
			}
		}
	}

	var sampleFilePath string
	var sampleAlbum string
	var sampleArtist string
	rowSample := d.db.QueryRow(`
		SELECT file_path, COALESCE(album, ''), artist FROM tracks
		WHERE album = ? OR artist = ?
		LIMIT 1
	`, id, id)
	if errSample := rowSample.Scan(&sampleFilePath, &sampleAlbum, &sampleArtist); errSample == nil && sampleFilePath != "" {
		if f, errFile := os.Open(sampleFilePath); errFile == nil {
			m, errTag := tag.ReadFrom(f)
			f.Close()
			if errTag == nil && m.Picture() != nil {
				pic := m.Picture()
				mt := pic.MIMEType
				if mt == "" {
					mt = "image/jpeg"
				}
				if sampleAlbum != "" {
					_, _ = d.SaveAlbumCoverArt(sampleAlbum, sampleArtist, pic.Data, mt, "EMBEDDED")
				}
				return pic.Data, mt, nil
			}
		}

		dir := filepath.Dir(sampleFilePath)
		for _, imgName := range []string{"cover.jpg", "folder.jpg", "front.jpg", "album.jpg", "cover.png", "folder.png", "front.png"} {
			imgPath := filepath.Join(dir, imgName)
			if imgData, errImg := os.ReadFile(imgPath); errImg == nil && len(imgData) > 0 {
				mt := "image/jpeg"
				if strings.HasSuffix(strings.ToLower(imgName), ".png") {
					mt = "image/png"
				}
				if sampleAlbum != "" {
					_, _ = d.SaveAlbumCoverArt(sampleAlbum, sampleArtist, imgData, mt, "FOLDER")
				}
				return imgData, mt, nil
			}
		}
	}

	return nil, "", fmt.Errorf("no cover art found for %s", id)
}

func (d *Database) GetAlbumCoverArt(albumID string) ([]byte, string, error) {
	return d.GetCoverArt(albumID)
}

func (d *Database) GetAllPrograms() ([]Program, error) {
	rows, err := d.db.Query(`SELECT id, name, start_time, COALESCE(track_ids, '') FROM programs ORDER BY start_time ASC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	programs := make([]Program, 0)
	for rows.Next() {
		var p Program
		if err := rows.Scan(&p.ID, &p.Name, &p.StartTime, &p.TrackIDs); err == nil {
			programs = append(programs, p)
		}
	}
	return programs, nil
}

func (d *Database) GetProgramByID(id int64) (*Program, error) {
	row := d.db.QueryRow(`SELECT id, name, start_time, COALESCE(track_ids, '') FROM programs WHERE id = ?`, id)
	var p Program
	err := row.Scan(&p.ID, &p.Name, &p.StartTime, &p.TrackIDs)
	if err != nil {
		return nil, err
	}
	return &p, nil
}

func (d *Database) SaveProgram(name, startTime, trackIDs string) (*Program, error) {
	res, err := d.db.Exec(`
		INSERT INTO programs (name, start_time, track_ids)
		VALUES (?, ?, ?)
		ON CONFLICT(name) DO UPDATE SET
			start_time = excluded.start_time,
			track_ids = excluded.track_ids
	`, name, startTime, trackIDs)
	if err != nil {
		return nil, err
	}
	id, _ := res.LastInsertId()
	return d.GetProgramByID(id)
}

func (d *Database) UpdateProgram(id int64, name, startTime, trackIDs string) (*Program, error) {
	_, err := d.db.Exec(`UPDATE programs SET name = ?, start_time = ?, track_ids = ? WHERE id = ?`, name, startTime, trackIDs, id)
	if err != nil {
		return nil, err
	}
	return d.GetProgramByID(id)
}

func (d *Database) DeleteProgram(id int64) error {
	_, err := d.db.Exec(`DELETE FROM programs WHERE id = ?`, id)
	return err
}

func (d *Database) ImportMuLibPrograms(mulibPath string) (int, error) {
	if mulibPath == "" {
		mulibPath = `C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db`
	}
	if _, err := os.Stat(mulibPath); os.IsNotExist(err) {
		defaults := []struct {
			name, startTime string
		}{
			{"Overnight Ambient", "00:00"},
			{"Morning Light", "06:00"},
			{"Midday Groove", "12:00"},
			{"Drive Time", "17:00"},
			{"Night Moods", "22:00"},
		}
		count := 0
		for _, def := range defaults {
			_, err := d.SaveProgram(def.name, def.startTime, "")
			if err == nil {
				count++
			}
		}
		return count, nil
	}

	count := 0
	if mdb, err := sql.Open("sqlite", mulibPath); err == nil {
		if rows, errQuery := mdb.Query(`SELECT name, startTime, trackList FROM programs`); errQuery == nil {
			for rows.Next() {
				var name, startTime sql.NullString
				var trackList sql.NullString
				if err := rows.Scan(&name, &startTime, &trackList); err == nil && name.Valid && startTime.Valid {
					_, errSave := d.SaveProgram(name.String, startTime.String, trackList.String)
					if errSave == nil {
						count++
					}
				}
			}
			rows.Close()
		}
		mdb.Close()
	}

	if count == 0 {
		defaults := []struct {
			name, startTime string
		}{
			{"Overnight Ambient", "00:00"},
			{"Morning Light", "06:00"},
			{"Midday Groove", "12:00"},
			{"Drive Time", "17:00"},
			{"Night Moods", "22:00"},
		}
		for _, def := range defaults {
			_, err := d.SaveProgram(def.name, def.startTime, "")
			if err == nil {
				count++
			}
		}
	}

	return count, nil
}

func ComputeSortName(rawName string) string {
	raw := strings.TrimSpace(rawName)
	if raw == "" {
		return ""
	}

	text := strings.TrimLeft(raw, " \t\n\r'\"`()[]{},.")
	if text == "" {
		text = raw
	}

	lower := strings.ToLower(text)
	var res string
	if strings.HasPrefix(lower, "the ") {
		res = strings.TrimSpace(text[4:]) + ", The"
	} else if strings.HasPrefix(lower, "a ") {
		res = strings.TrimSpace(text[2:]) + ", A"
	} else if strings.HasPrefix(lower, "an ") {
		res = strings.TrimSpace(text[3:]) + ", An"
	} else {
		res = text
	}

	return strings.ToUpper(res)
}

func (d *Database) EnsureTrackArtists() error {
	var count int
	_ = d.db.QueryRow(`SELECT COUNT(*) FROM track_artists`).Scan(&count)
	if count == 0 {
		rows, err := d.db.Query(`SELECT id, artist FROM tracks WHERE artist IS NOT NULL AND artist != ''`)
		if err == nil {
			type taItem struct{ id, artist, sortName string }
			items := make([]taItem, 0)
			for rows.Next() {
				var id, artist string
				if errScan := rows.Scan(&id, &artist); errScan == nil {
					items = append(items, taItem{id: id, artist: artist, sortName: ComputeSortName(artist)})
				}
			}
			rows.Close()

			if len(items) > 0 {
				tx, errTx := d.db.Begin()
				if errTx == nil {
					stmt, _ := tx.Prepare(`INSERT OR IGNORE INTO track_artists (track_id, artist_name, artist_sort_name) VALUES (?, ?, ?)`)
					for _, item := range items {
						_, _ = stmt.Exec(item.id, item.artist, item.sortName)
					}
					stmt.Close()
					_ = tx.Commit()
				}
			}
		}
	}
	return nil
}

func (d *Database) EnsureSortNames() error {
	rows, err := d.db.Query(`SELECT id, title, artist, COALESCE(album, '') FROM tracks WHERE artist_sort_name IS NULL OR artist_sort_name = '' OR album_sort_name IS NULL OR album_sort_name = '' OR title_sort_name IS NULL OR title_sort_name = ''`)
	if err == nil {
		type trackSortUpdate struct {
			id, artSort, albSort, ttlSort string
		}
		updates := make([]trackSortUpdate, 0)
		for rows.Next() {
			var id, title, artist, album string
			if errScan := rows.Scan(&id, &title, &artist, &album); errScan == nil {
				updates = append(updates, trackSortUpdate{
					id:      id,
					artSort: ComputeSortName(artist),
					albSort: ComputeSortName(album),
					ttlSort: ComputeSortName(title),
				})
			}
		}
		rows.Close()

		if len(updates) > 0 {
			tx, errTx := d.db.Begin()
			if errTx == nil {
				stmt, _ := tx.Prepare(`UPDATE tracks SET artist_sort_name = ?, album_sort_name = ?, title_sort_name = ? WHERE id = ?`)
				for _, u := range updates {
					_, _ = stmt.Exec(u.artSort, u.albSort, u.ttlSort, u.id)
				}
				stmt.Close()
				_ = tx.Commit()
			}
		}
	}

	taRows, errTa := d.db.Query(`SELECT track_id, artist_name FROM track_artists WHERE artist_sort_name IS NULL OR artist_sort_name = ''`)
	if errTa == nil {
		type taUpdate struct {
			trackID, artistName, artSort string
		}
		updates := make([]taUpdate, 0)
		for taRows.Next() {
			var tid, aname string
			if errScan := taRows.Scan(&tid, &aname); errScan == nil {
				updates = append(updates, taUpdate{
					trackID:    tid,
					artistName: aname,
					artSort:    ComputeSortName(aname),
				})
			}
		}
		taRows.Close()

		if len(updates) > 0 {
			tx, errTx := d.db.Begin()
			if errTx == nil {
				stmt, _ := tx.Prepare(`UPDATE track_artists SET artist_sort_name = ? WHERE track_id = ? AND artist_name = ?`)
				for _, u := range updates {
					_, _ = stmt.Exec(u.artSort, u.trackID, u.artistName)
				}
				stmt.Close()
				_ = tx.Commit()
			}
		}
	}

	return nil
}


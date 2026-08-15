package db

import (
	"testing"
	"time"
)

func TestDatabaseOperations(t *testing.T) {
	d, err := NewDatabase(":memory:")
	if err != nil {
		t.Fatalf("failed memory db: %v", err)
	}
	defer d.Close()

	// Insert test track directly
	_, err = d.db.Exec(`
		INSERT INTO tracks (id, file_path, file_format, title, artist, album, duration_ms, rotation, recovery, restraint)
		VALUES ('t101', '/music/song1.mp3', 'MP3', 'Song One', 'Artist Alpha', 'Album One', 180000, 0.0, 0.778, 0.0)
	`)
	if err != nil {
		t.Fatalf("failed inserting test track: %v", err)
	}

	// Test GetTrackRatings
	tr, err := d.GetTrackRatings("t101")
	if err != nil {
		t.Fatalf("failed GetTrackRatings: %v", err)
	}
	if tr.Title != "Song One" {
		t.Errorf("expected Title Song One, got %s", tr.Title)
	}

	// Test UpdateTrackRatings
	updated, err := d.UpdateTrackRatings("t101", 1.0, 0.5, 0.4, 0.0, nil)
	if err != nil {
		t.Fatalf("failed UpdateTrackRatings: %v", err)
	}
	if updated.Restraint != 0.4 {
		t.Errorf("expected restraint 0.4, got %f", updated.Restraint)
	}

	// Test RecordPlay
	err = d.RecordPlay("t101", time.Now())
	if err != nil {
		t.Fatalf("failed RecordPlay: %v", err)
	}

	trPost, _ := d.GetTrackRatings("t101")
	if trPost.PlayCount != 1 {
		t.Errorf("expected PlayCount 1, got %d", trPost.PlayCount)
	}

	// Test ArtistRatings
	ar, err := d.GetArtistRatings("Artist Alpha")
	if err != nil {
		t.Fatalf("failed GetArtistRatings: %v", err)
	}
	if ar.ArtistName != "Artist Alpha" {
		t.Errorf("expected Artist Alpha, got %s", ar.ArtistName)
	}
}

func TestVainoDBQueries(t *testing.T) {
	d, err := NewDatabase("c:\\Users\\Mango Cat\\Dev\\Vaino\\vaino.db")
	if err != nil {
		t.Skipf("skipping vaino.db test: %v", err)
		return
	}
	defer d.Close()

	totalTracks, err := d.GetTotalTrackCount("", "", "", "")
	if err != nil {
		t.Fatalf("GetTotalTrackCount error: %v", err)
	}
	t.Logf("Total tracks in vaino.db: %d", totalTracks)

	tracks, err := d.GetAllTracks(10, 0, "", "", "", "")
	if err != nil {
		t.Fatalf("GetAllTracks error: %v", err)
	}
	t.Logf("Fetched %d tracks from vaino.db", len(tracks))

	totalAlbums, err := d.GetTotalAlbumCount("", "", "")
	if err != nil {
		t.Fatalf("GetTotalAlbumCount error: %v", err)
	}
	t.Logf("Total albums in vaino.db: %d", totalAlbums)

	albums, err := d.GetAllAlbums(10, 0, "", "", "")
	if err != nil {
		t.Fatalf("GetAllAlbums error: %v", err)
	}
	t.Logf("Fetched %d albums from vaino.db", len(albums))

	totalArtists, err := d.GetTotalArtistCount("", "")
	if err != nil {
		t.Fatalf("GetTotalArtistCount error: %v", err)
	}
	t.Logf("Total artists in vaino.db: %d", totalArtists)

	artists, err := d.GetAllArtists(10, 0, "", "")
	if err != nil {
		t.Fatalf("GetAllArtists error: %v", err)
	}
	t.Logf("Fetched %d artists from vaino.db", len(artists))
	if len(artists) > 0 {
		t.Logf("First artist object: %+v", artists[0])
	}
	if len(tracks) > 0 {
		t.Logf("First track title: %s, artist: %s, duration: %d", tracks[0].Title, tracks[0].Artist, tracks[0].DurationMs)
	}

	alTracks, err := d.GetAlbumTracks("Takin' It to the Streets", "")
	if err != nil {
		t.Fatalf("GetAlbumTracks error: %v", err)
	}
	t.Logf("GetAlbumTracks Takin' It to the Streets fetched %d tracks", len(alTracks))
	if len(alTracks) > 0 {
		t.Logf("First album track title: %s, artist: %s", alTracks[0].Title, alTracks[0].Artist)
	}
}

func TestGetCoverArtResolver(t *testing.T) {
	d, err := NewDatabase(":memory:")
	if err != nil {
		t.Fatalf("failed memory db: %v", err)
	}
	defer d.Close()

	_, errSave := d.SaveAlbumCoverArt("Takin' It to the Streets", "The Doobie Brothers", []byte("FAKE_JPEG_BYTES"), "image/jpeg", "TEST")
	if errSave != nil {
		t.Fatalf("failed SaveAlbumCoverArt: %v", errSave)
	}

	data, mimeType, errGet := d.GetCoverArt("Takin' It to the Streets")
	if errGet != nil || string(data) != "FAKE_JPEG_BYTES" || mimeType != "image/jpeg" {
		t.Errorf("expected FAKE_JPEG_BYTES image/jpeg, got data=%s, mime=%s, err=%v", string(data), mimeType, errGet)
	}
}

func TestComputeSortNameAndAlphaFilter(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"The Beatles", "BEATLES, THE"},
		{"A Flock of Seagulls", "FLOCK OF SEAGULLS, A"},
		{"An Endless Sporadic", "ENDLESS SPORADIC, AN"},
		{"'Til Tuesday", "TIL TUESDAY"},
		{"10,000 Maniacs", "10,000 MANIACS"},
	}

	for _, tt := range tests {
		actual := ComputeSortName(tt.input)
		if actual != tt.expected {
			t.Errorf("ComputeSortName(%q) = %q; expected %q", tt.input, actual, tt.expected)
		}
	}
}

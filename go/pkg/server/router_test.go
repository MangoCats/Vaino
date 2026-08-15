package server

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/mangocats/vaino/pkg/audio"
	"github.com/mangocats/vaino/pkg/db"
)

func setupTestEnvironment(t *testing.T) (*db.Database, *audio.AudioEngine, http.Handler) {
	d, err := db.NewDatabase(":memory:")
	if err != nil {
		t.Fatalf("failed memory db: %v", err)
	}

	// Insert test track with single quotes in album name
	alb := "Takin' It to the Streets"
	yr := int64(1976)
	trNum := int64(1)
	t1 := &db.Track{
		ID:            "tr_doobie_1",
		FilePath:      "/music/doobie/track1.mp3",
		FileFormat:    "MP3",
		Title:         "It Keeps You Running",
		Artist:        "The Doobie Brothers",
		Album:         &alb,
		Year:          &yr,
		TrackNumber:   &trNum,
		DurationMs:    240000,
		TitleSortName: nil,
	}
	if err := d.InsertTrack(t1); err != nil {
		t.Fatalf("failed to insert test track: %v", err)
	}

	engine := audio.NewAudioEngine(d)
	r := NewRouter(d, engine)
	return d, engine, r
}

func TestRouterEndpoints(t *testing.T) {
	d, _, r := setupTestEnvironment(t)
	defer d.Close()

	// 1. GET /api/v1/status
	req := httptest.NewRequest("GET", "/api/v1/status", nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected status 200, got %d", rec.Code)
	}

	var status map[string]interface{}
	_ = json.Unmarshal(rec.Body.Bytes(), &status)
	if status["state"] != "IDLE" {
		t.Errorf("expected state IDLE, got %v", status["state"])
	}

	// 2. POST /api/v1/player/volume
	reqVol := httptest.NewRequest("POST", "/api/v1/player/volume", strings.NewReader(`{"volume":65}`))
	recVol := httptest.NewRecorder()
	r.ServeHTTP(recVol, reqVol)

	if recVol.Code != http.StatusOK {
		t.Errorf("expected status 200 for volume, got %d", recVol.Code)
	}
}

func TestJSONPrimitiveSerialization(t *testing.T) {
	d, _, r := setupTestEnvironment(t)
	defer d.Close()

	req := httptest.NewRequest("GET", "/api/v1/library/tracks?limit=10", nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected status 200, got %d", rec.Code)
	}

	var body map[string]interface{}
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("failed to parse JSON response: %v", err)
	}

	tracks, ok := body["tracks"].([]interface{})
	if !ok || len(tracks) == 0 {
		t.Fatalf("expected tracks list in response")
	}

	firstTrack := tracks[0].(map[string]interface{})
	if albumStr, isString := firstTrack["album"].(string); !isString || albumStr != "Takin' It to the Streets" {
		t.Errorf("expected album string primitive, got: %v", firstTrack["album"])
	}
	if yearNum, isNum := firstTrack["year"].(float64); !isNum || int(yearNum) != 1976 {
		t.Errorf("expected year number primitive 1976, got: %v", firstTrack["year"])
	}
}

func TestAlbumURLUnescaping(t *testing.T) {
	d, _, r := setupTestEnvironment(t)
	defer d.Close()

	// Test URL encoded single quote (%27) in album name
	req := httptest.NewRequest("GET", "/api/v1/library/albums/Takin%27%20It%20to%20the%20Streets/tracks", nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected status 200, got %d", rec.Code)
	}

	var body map[string]interface{}
	_ = json.Unmarshal(rec.Body.Bytes(), &body)

	tracks, ok := body["tracks"].([]interface{})
	if !ok || len(tracks) != 1 {
		t.Errorf("expected 1 album track after URL unescaping, got %d", len(tracks))
	}
}

func TestPlayerPlayAndQueueAdd(t *testing.T) {
	d, _, r := setupTestEnvironment(t)
	defer d.Close()

	// POST /api/v1/player/play?track_id=tr_doobie_1
	reqPlay := httptest.NewRequest("POST", "/api/v1/player/play?track_id=tr_doobie_1", nil)
	recPlay := httptest.NewRecorder()
	r.ServeHTTP(recPlay, reqPlay)

	if recPlay.Code != http.StatusOK {
		t.Fatalf("expected status 200 for player play, got %d", recPlay.Code)
	}

	var statusPlay map[string]interface{}
	_ = json.Unmarshal(recPlay.Body.Bytes(), &statusPlay)
	curTrack, _ := statusPlay["current_track"].(map[string]interface{})
	if curTrack == nil || curTrack["id"] != "tr_doobie_1" {
		t.Errorf("expected current track ID tr_doobie_1, got: %v", curTrack)
	}
	if statusPlay["duration_ms"] == nil {
		t.Errorf("expected 'duration_ms' in status play response")
	}
	if statusPlay["elapsed_ms"] == nil {
		t.Errorf("expected 'elapsed_ms' in status play response")
	}

	// POST /api/v1/queue/add (track)
	reqQueue := httptest.NewRequest("POST", "/api/v1/queue/add", strings.NewReader(`{"track_id":"tr_doobie_1","play_next":false}`))
	recQueue := httptest.NewRecorder()
	r.ServeHTTP(recQueue, reqQueue)

	if recQueue.Code != http.StatusOK {
		t.Fatalf("expected status 200 for queue add, got %d", recQueue.Code)
	}

	var statusQueue map[string]interface{}
	_ = json.Unmarshal(recQueue.Body.Bytes(), &statusQueue)
	queueArr, ok := statusQueue["queue"].([]interface{})
	if !ok {
		t.Fatalf("expected 'queue' array in status response, got: %v", statusQueue["queue"])
	}
	if len(queueArr) == 0 {
		t.Errorf("expected non-empty queue array after queue add, got 0 items")
	}

	// DELETE /api/v1/queue/clear
	reqClear := httptest.NewRequest("DELETE", "/api/v1/queue/clear", nil)
	recClear := httptest.NewRecorder()
	r.ServeHTTP(recClear, reqClear)
	if recClear.Code != http.StatusOK {
		t.Fatalf("expected status 200 for queue clear, got %d", recClear.Code)
	}
}

func TestCoverArtFallbackSVG(t *testing.T) {
	d, _, r := setupTestEnvironment(t)
	defer d.Close()

	req := httptest.NewRequest("GET", "/api/v1/art/non_existent_id", nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected status 200 SVG fallback, got %d", rec.Code)
	}
	if ct := rec.Header().Get("Content-Type"); ct != "image/svg+xml" {
		t.Errorf("expected Content-Type image/svg+xml, got %s", ct)
	}
}

func TestProgramsEndpoints(t *testing.T) {
	d, _, r := setupTestEnvironment(t)
	defer d.Close()

	// 1. GET /api/v1/programs (auto-imports default programs)
	req := httptest.NewRequest("GET", "/api/v1/programs", nil)
	rec := httptest.NewRecorder()
	r.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected status 200 for programs, got %d", rec.Code)
	}

	var body map[string]interface{}
	_ = json.Unmarshal(rec.Body.Bytes(), &body)
	progs, ok := body["programs"].([]interface{})
	if !ok || len(progs) == 0 {
		t.Fatalf("expected non-empty programs array, got: %v", body["programs"])
	}

	// 2. POST /api/v1/programs (create new program)
	reqCreate := httptest.NewRequest("POST", "/api/v1/programs", strings.NewReader(`{"name":"Sunset Chill","start_time":"19:00","track_ids":""}`))
	recCreate := httptest.NewRecorder()
	r.ServeHTTP(recCreate, reqCreate)

	if recCreate.Code != http.StatusOK {
		t.Fatalf("expected status 200 for create program, got %d", recCreate.Code)
	}

	var newProg map[string]interface{}
	_ = json.Unmarshal(recCreate.Body.Bytes(), &newProg)
	if newProg["name"] != "Sunset Chill" {
		t.Errorf("expected name Sunset Chill, got %v", newProg["name"])
	}
}

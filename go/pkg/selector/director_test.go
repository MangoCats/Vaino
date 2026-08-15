package selector

import (
	"math"
	"testing"
	"time"

	"github.com/mangocats/vaino/pkg/db"
)

func TestRotationToSeconds(t *testing.T) {
	if RotationToSeconds(0.0) != 3600.0 {
		t.Errorf("expected 3600.0, got %f", RotationToSeconds(0.0))
	}
	if RotationToSeconds(1.0) != 36000.0 {
		t.Errorf("expected 36000.0, got %f", RotationToSeconds(1.0))
	}
}

func TestCalculateRecoveryWeight(t *testing.T) {
	rotSec := 3600.0
	recSec := 21600.0

	// Hard lockout
	if CalculateRecoveryWeight(1800.0, rotSec, recSec) != 0.0 {
		t.Errorf("expected 0.0 during lockout")
	}

	// Linear ramp halfway
	ramp := CalculateRecoveryWeight(3600.0+10800.0, rotSec, recSec)
	if math.Abs(ramp-0.5) > 1e-5 {
		t.Errorf("expected 0.5 ramp, got %f", ramp)
	}

	// Fully recovered
	if CalculateRecoveryWeight(30000.0, rotSec, recSec) != 1.0 {
		t.Errorf("expected 1.0 recovered")
	}
}

func TestCalculateRestraintWeight(t *testing.T) {
	if CalculateRestraintWeight(0.0) != 1.0 {
		t.Errorf("expected 1.0")
	}
	if math.Abs(CalculateRestraintWeight(0.3)-0.50118) > 1e-3 {
		t.Errorf("expected ~0.50118")
	}
}

func TestCalculate11DDistance(t *testing.T) {
	u := map[string]interface{}{
		"ab_acoustic": 0.5, "ab_aggressive": 0.5, "ab_bright": 0.5, "ab_danceable": 0.5,
		"ab_female": 0.5, "ab_happy": 0.5, "ab_instrumental": 0.5, "ab_party": 0.5,
		"ab_relaxed": 0.5, "ab_sad": 0.5, "ab_tonal": 0.5,
	}
	v := map[string]interface{}{
		"ab_acoustic": 0.5, "ab_aggressive": 0.5, "ab_bright": 0.5, "ab_danceable": 0.5,
		"ab_female": 0.5, "ab_happy": 0.5, "ab_instrumental": 0.5, "ab_party": 0.5,
		"ab_relaxed": 0.5, "ab_sad": 0.5, "ab_tonal": 0.5,
	}

	dist := Calculate11DDistance(u, v)
	if dist != 0.0 {
		t.Errorf("expected 0.0 distance for identical vectors, got %f", dist)
	}
}

func TestProgramDirectorSelection(t *testing.T) {
	database, err := db.NewDatabase(":memory:")
	if err != nil {
		t.Fatalf("failed memory db: %v", err)
	}
	defer database.Close()

	pd := NewProgramDirector(database)

	t1 := db.Track{
		ID:         "t1",
		Title:      "Song 1",
		Artist:     "Artist 1",
		DurationMs: 180000,
		Rotation:   0.0,
		Recovery:   0.778,
	}
	t2 := db.Track{
		ID:         "t2",
		Title:      "Song 2",
		Artist:     "Artist 2",
		DurationMs: 200000,
		Rotation:   0.0,
		Recovery:   0.778,
	}

	pool := []db.Track{t1, t2}

	// Select next track
	sel, err := pd.SelectNextTrack(nil, pool, nil, nil)
	if err != nil {
		t.Fatalf("selection failed: %v", err)
	}
	if sel.ID != "t1" && sel.ID != "t2" {
		t.Errorf("invalid track selected: %v", sel)
	}

	// Lockout t1 with recent play
	_ = database.RecordPlay("t1", time.Now())

	// t1 is locked out, t2 must be selected
	sel2, _ := pd.SelectNextTrack(&t1, pool, nil, nil)
	if sel2.ID != "t2" {
		t.Errorf("expected t2 selected due to t1 rotation lockout, got %s", sel2.ID)
	}
}

func TestGetActiveProgramTimeSlot(t *testing.T) {
	d, err := db.NewDatabase(":memory:")
	if err != nil {
		t.Fatalf("failed memory db: %v", err)
	}
	defer d.Close()

	_, _ = d.SaveProgram("Overnight Ambient", "00:00", "")
	_, _ = d.SaveProgram("Morning Light", "06:00", "")
	_, _ = d.SaveProgram("Midday Groove", "12:00", "")
	_, _ = d.SaveProgram("Fun", "16:30", "")
	_, _ = d.SaveProgram("Night Moods", "22:00", "")

	pd := NewProgramDirector(d)

	// Test at 18:08 (should select "Fun" at 16:30)
	t1808 := time.Date(2026, 7, 30, 18, 8, 0, 0, time.UTC)
	prog, err := pd.GetActiveProgram(t1808)
	if err != nil || prog == nil {
		t.Fatalf("expected active program, got error: %v", err)
	}
	if prog.Name != "Fun" {
		t.Errorf("expected active program 'Fun' at 18:08, got '%s'", prog.Name)
	}

	// Test at 05:15 (should select "Overnight Ambient" at 00:00)
	t0515 := time.Date(2026, 7, 30, 5, 15, 0, 0, time.UTC)
	prog05, _ := pd.GetActiveProgram(t0515)
	if prog05 != nil && prog05.Name != "Overnight Ambient" {
		t.Errorf("expected active program 'Overnight Ambient' at 05:15, got '%s'", prog05.Name)
	}
}

func TestQueueReplenishExcludesExistingAndQueueTracks(t *testing.T) {
	database, err := db.NewDatabase(":memory:")
	if err != nil {
		t.Fatalf("failed memory db: %v", err)
	}
	defer database.Close()

	pd := NewProgramDirector(database)

	t1 := db.Track{ID: "t1", Title: "Track 1", Artist: "Artist A", DurationMs: 180000, Recovery: 0.778}
	t2 := db.Track{ID: "t2", Title: "Track 2", Artist: "Artist B", DurationMs: 180000, Recovery: 0.778}
	t3 := db.Track{ID: "t3", Title: "Track 3", Artist: "Artist C", DurationMs: 180000, Recovery: 0.778}

	pool := []db.Track{t1, t2, t3}

	excludeIDs := map[string]bool{"t1": true, "t2": true}
	sel, err := pd.SelectNextTrack(nil, pool, nil, excludeIDs)
	if err != nil {
		t.Fatalf("selection failed: %v", err)
	}
	if sel.ID != "t3" {
		t.Errorf("expected t3 selected when t1 and t2 excluded, got %s", sel.ID)
	}
}

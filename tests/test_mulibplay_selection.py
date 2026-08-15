# tests/test_mulibplay_selection.py
"""
Unit and integration tests for MuLibPlay-equivalent song selection algorithms,
rotation lockouts, recovery ramps, restraint scaling, occasion weighting,
play tracking, and rating REST API endpoints.
"""

import time
import pytest
from fastapi.testclient import TestClient

from src.db.database import Database
from src.audio.engine import AudioEngine
from src.audio.selector import (
    ProgramDirector,
    rotation_to_seconds,
    calculate_recovery_weight,
    calculate_restraint_weight,
    calculate_occasion_weight,
    calculate_track_length_modifier
)
from src.server.app import create_app


@pytest.fixture
def memory_db():
    db = Database(":memory:")
    # Seed mock tracks
    t1 = {
        "id": "t101",
        "file_path": "/music/song1.mp3",
        "file_format": "MP3",
        "title": "Song One",
        "artist": "Artist Alpha",
        "album": "Album One",
        "duration_ms": 180000,
        "rotation": 0.0,      # 1 hr
        "recovery": 0.778,    # 6 hrs
        "restraint": 0.0,
        "profanity": 0.0,
        "occasions": None
    }
    t2 = {
        "id": "t102",
        "file_path": "/music/song2.mp3",
        "file_format": "MP3",
        "title": "Song Two",
        "artist": "Artist Beta",
        "album": "Album Two",
        "duration_ms": 210000,
        "rotation": 0.0,
        "recovery": 0.778,
        "restraint": 0.3,     # reduced preference (~0.5x)
        "profanity": 0.0,
        "occasions": None
    }
    t3 = {
        "id": "t103",
        "file_path": "/music/xmas_song.mp3",
        "file_format": "MP3",
        "title": "Holiday Cheer",
        "artist": "Artist Alpha",
        "album": "Holiday Album",
        "duration_ms": 150000,
        "rotation": 0.0,
        "recovery": 0.778,
        "restraint": 0.0,
        "profanity": 0.0,
        "occasions": "[C]"    # Christmas
    }
    db.upsert_tracks_batch([t1, t2, t3])
    return db


def test_rotation_to_seconds():
    assert rotation_to_seconds(0.0) == 3600.0        # 1 hour
    assert rotation_to_seconds(1.0) == 36000.0       # 10 hours
    assert abs(rotation_to_seconds(0.778) - 21594.0) < 100.0 # ~6 hours


def test_recovery_weight_ramp():
    rot_sec = 3600.0   # 1 hr
    rec_sec = 21600.0  # 6 hrs

    # Hard lockout window
    assert calculate_recovery_weight(1800.0, rot_sec, rec_sec) == 0.0
    assert calculate_recovery_weight(3600.0, rot_sec, rec_sec) == 0.0

    # Linear recovery ramp
    # At age = 3600 + 10800 (halfway through recovery window), ramp should be 0.5
    ramp_half = calculate_recovery_weight(3600.0 + 10800.0, rot_sec, rec_sec)
    assert abs(ramp_half - 0.5) < 1e-5

    # Fully recovered
    assert calculate_recovery_weight(3600.0 + 21600.0, rot_sec, rec_sec) == 1.0
    assert calculate_recovery_weight(50000.0, rot_sec, rec_sec) == 1.0


def test_restraint_weight():
    assert calculate_restraint_weight(0.0) == 1.0
    assert abs(calculate_restraint_weight(0.3) - 0.50118) < 1e-3
    assert abs(calculate_restraint_weight(-0.3) - 1.99526) < 1e-3
    assert calculate_restraint_weight(1.0) == 0.1


def test_occasion_weight_christmas():
    # September (non-Christmas month)
    sep_time = time.mktime((2026, 9, 15, 12, 0, 0, 0, 0, 0))
    w_sep = calculate_occasion_weight("[C]", current_time=sep_time)
    assert w_sep < 1e-5

    # Dec 25 (Christmas Day)
    xmas_time = time.mktime((2026, 12, 25, 12, 0, 0, 0, 0, 0))
    w_xmas = calculate_occasion_weight("[C]", current_time=xmas_time)
    assert w_xmas == 10.0


def test_track_length_modifier():
    # 3-minute track (180s) -> modifier 1.0
    assert calculate_track_length_modifier(180000) == 1.0
    # Short track (45s) -> modifier 2.0 (max bonus)
    assert calculate_track_length_modifier(45000) == 2.0


def test_record_play_and_ratings(memory_db):
    db = memory_db
    t1 = db.get_track_by_id("t101")
    assert t1["play_count"] == 0 or t1["play_count"] is None

    # Record play
    now = time.time()
    db.record_play("t101", play_time=now)

    ratings = db.get_track_ratings("t101")
    assert ratings["play_count"] == 1
    assert ratings["last_played_at"] is not None

    # Verify artist ratings created
    a_ratings = db.get_artist_ratings("Artist Alpha")
    assert a_ratings["play_count"] == 1
    assert a_ratings["last_played_at"] is not None

    # Update track ratings
    updated = db.update_track_ratings("t101", restraint=0.5, rotation=1.0)
    assert updated["restraint"] == 0.5
    assert updated["rotation"] == 1.0

    # Update artist ratings
    a_updated = db.update_artist_ratings("Artist Alpha", restraint=-0.2)
    assert a_updated["restraint"] == -0.2


def test_program_director_selection_with_ratings(memory_db):
    db = memory_db
    pd = ProgramDirector(db)

    # 1. Initially select next track
    selected = pd.select_next_track()
    assert selected is not None
    assert selected["id"] in ["t101", "t102", "t103"]

    # 2. Record recent play for t101 -> Should trigger rotation lockout
    now = time.time()
    db.record_play("t101", play_time=now)

    # 3. Select next track with current_track = t101
    t101 = db.get_track_by_id("t101")
    next_sel = pd.select_next_track(current_track=t101)
    assert next_sel is not None
    # t101 must be locked out by recent play
    assert next_sel["id"] != "t101"


def test_ratings_rest_api(memory_db):
    db = memory_db
    engine = AudioEngine(db=db)
    app = create_app(db=db, audio_engine=engine, scanner=None)
    client = TestClient(app)

    # GET track ratings
    res = client.get("/api/v1/ratings/track/t101")
    assert res.status_code == 200
    data = res.json()
    assert data["id"] == "t101"
    assert data["rotation"] == 0.0

    # PUT track ratings
    res_put = client.put("/api/v1/ratings/track/t101", json={"restraint": 0.4, "rotation": 0.5})
    assert res_put.status_code == 200
    assert res_put.json()["restraint"] == 0.4
    assert res_put.json()["rotation"] == 0.5

    # GET artist ratings
    res_art = client.get("/api/v1/ratings/artist/Artist%20Alpha")
    assert res_art.status_code == 200
    assert res_art.json()["artist_name"] == "Artist Alpha"

    # PUT artist ratings
    res_art_put = client.put("/api/v1/ratings/artist/Artist%20Alpha", json={"restraint": -0.3, "rotation": 1.0})
    assert res_art_put.status_code == 200
    assert res_art_put.json()["restraint"] == -0.3
    assert res_art_put.json()["rotation"] == 1.0

    # GET all artist ratings
    res_all_art = client.get("/api/v1/ratings/artists")
    assert res_all_art.status_code == 200
    assert "Artist Alpha" in res_all_art.json()

    # POST import-mulib
    res_import = client.post("/api/v1/ratings/import-mulib", json={"mulib_path": r"C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db"})
    assert res_import.status_code == 200
    imp_data = res_import.json()
    assert imp_data["status"] == "SUCCESS"


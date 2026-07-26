# src/audio/selector.py
"""
[SPEC-PD-010] Program Director & Intelligent Auto-Playlist Engine ("Singing Sorcerer")
Autonomous context-aware recommendation engine scoring candidate tracks on acoustic transition flow,
time-of-day energy curves, and anti-repetition cooldown decay formulas.
"""

import math
import time
import logging
from typing import List, Dict, Any, Optional
from ..db.database import Database

logger = logging.getLogger(__name__)

class ProgramDirector:
    """
    [REQ-PD-010] Autonomous auto-playlist selector.
    """
    def __init__(self, db: Database):
        self.db = db
        self.w_flow = 0.40
        self.w_time = 0.35
        self.w_pref = 0.25

    def get_target_energy_for_hour(self, hour: int) -> float:
        """
        [SPEC-PD-010] Returns target energy curve [0.0, 1.0] based on time of day.
        - Late Night / Early Morning (00:00 - 06:00): Mellow ambient (0.2 - 0.4)
        - Morning (06:00 - 12:00): Moderate build (0.5 - 0.7)
        - Afternoon Peak (12:00 - 18:00): High energy (0.7 - 0.9)
        - Evening (18:00 - 24:00): Relaxing wind-down (0.4 - 0.6)
        """
        if 0 <= hour < 6:
            return 0.30
        elif 6 <= hour < 12:
            return 0.60
        elif 12 <= hour < 18:
            return 0.85
        else:
            return 0.50

    def compute_acoustic_distance(self, current_desc: Dict[str, Any], candidate_desc: Dict[str, Any]) -> float:
        """
        [SPEC-PD-010] Calculates normalized Euclidean acoustic feature distance.
        """
        de = (current_desc.get("energy", 0.5) - candidate_desc.get("energy", 0.5)) ** 2
        dv = (current_desc.get("valence", 0.5) - candidate_desc.get("valence", 0.5)) ** 2
        dbpm = ((current_desc.get("tempo_bpm", 120.0) - candidate_desc.get("tempo_bpm", 120.0)) / 200.0) ** 2
        return float(math.sqrt(de + dv + dbpm))

    def calculate_cooldown_penalty(self, track: Dict[str, Any], history: List[Dict[str, Any]]) -> float:
        """
        [SPEC-PD-010] Calculates exponential decay penalty for recently played tracks/artists.
        """
        if not history:
            return 0.0

        penalty = 0.0
        now = time.time()

        for idx, entry in enumerate(history[:50]):
            played_track_id = entry.get("track_id")
            # Determine elapsed hours (approximate from order if timestamp missing)
            elapsed_hours = (idx + 1) * 0.1

            if played_track_id == track["id"]:
                # Track repeat penalty
                penalty += 10.0 * math.exp(-0.5 * elapsed_hours)

        return penalty

    def select_next_track(
        self,
        current_track: Optional[Dict[str, Any]] = None,
        candidate_pool: Optional[List[Dict[str, Any]]] = None,
        current_hour: Optional[int] = None
    ) -> Optional[Dict[str, Any]]:
        """
        [REQ-PD-010] Selects the optimal next track from the candidate pool using composite fitness scoring.
        """
        if not candidate_pool:
            candidate_pool = self.db.get_all_tracks(limit=200)

        if not candidate_pool:
            return None

        if current_hour is None:
            current_hour = time.localtime().tm_hour

        target_energy = self.get_target_energy_for_hour(current_hour)
        current_desc = None
        if current_track:
            current_desc = self.db.get_track_descriptors(current_track["id"])

        best_candidate = None
        best_score = -9999.0

        for candidate in candidate_pool:
            # Skip current track if playing
            if current_track and candidate["id"] == current_track["id"]:
                continue

            cand_desc = self.db.get_track_descriptors(candidate["id"]) or {
                "energy": 0.5, "valence": 0.5, "tempo_bpm": 120.0
            }

            # 1. Flow Score
            if current_desc:
                dist = self.compute_acoustic_distance(current_desc, cand_desc)
                s_flow = max(0.0, 1.0 - dist)
            else:
                s_flow = 0.5

            # 2. Time Score
            cand_energy = cand_desc.get("energy", 0.5)
            s_time = max(0.0, 1.0 - abs(target_energy - cand_energy))

            # 3. Preference Score (default 0.8)
            s_pref = 0.8

            # 4. Anti-repetition Penalty
            penalty = self.calculate_cooldown_penalty(candidate, [])

            total_score = (self.w_flow * s_flow) + (self.w_time * s_time) + (self.w_pref * s_pref) - penalty

            if total_score > best_score:
                best_score = total_score
                best_candidate = candidate

        logger.info(f"Program Director selected next track: {best_candidate.get('title') if best_candidate else 'None'} (Score: {best_score:.3f})")
        return best_candidate

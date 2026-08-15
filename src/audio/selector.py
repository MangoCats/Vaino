# src/audio/selector.py
"""
[SPEC-PD-010] Program Director & Intelligent Auto-Playlist Engine ("Singing Sorcerer")
Autonomous context-aware recommendation engine scoring candidate tracks using multi-stage
MuLibPlay multiplicative preference/cooldown weighting, 11D acoustic transition flow matching,
and roulette-wheel weighted random sampling.
"""

import math
import time
import random
import logging
from datetime import datetime, timezone
from typing import List, Dict, Any, Optional, Tuple
from ..db.database import Database

logger = logging.getLogger(__name__)

AB_KEYS = [
    "ab_acoustic", "ab_aggressive", "ab_bright", "ab_danceable",
    "ab_female", "ab_happy", "ab_instrumental", "ab_party",
    "ab_relaxed", "ab_sad", "ab_tonal"
]

def rotation_to_seconds(rv: float) -> float:
    """Converts log-scale rotation/recovery value rv into seconds: 10^rv * 3600."""
    return (10.0 ** float(rv)) * 3600.0

def calculate_recovery_weight(age_seconds: float, rot_seconds: float, rec_seconds: float) -> float:
    """Calculates linear recovery ramp weight [0.0, 1.0] following hard rotation lockout."""
    if age_seconds <= rot_seconds:
        return 0.0
    if age_seconds >= (rot_seconds + rec_seconds):
        return 1.0
    if rec_seconds <= 0:
        return 1.0
    return (age_seconds - rot_seconds) / rec_seconds

def calculate_restraint_weight(restraint_rv: float) -> float:
    """Calculates logarithmic restraint multiplier: 10^-restraint."""
    return 10.0 ** (-float(restraint_rv))

def calculate_occasion_weight(occasions_str: Optional[str], current_time: Optional[float] = None) -> float:
    """Calculates calendar/seasonal occasion multiplier for tagged tracks ([C], [W], [S], [K])."""
    if not occasions_str:
        return 1.0

    weight = 1.0
    dt = datetime.fromtimestamp(current_time if current_time else time.time(), tz=timezone.utc)
    month = dt.month
    day = dt.day

    if "[C]" in occasions_str:  # Christmas
        if month < 11:
            weight *= 0.000001
        elif month == 11:
            days_to_xmas = 55 - day
            weight *= (25.0 / float(days_to_xmas)) ** 3.0
        else:  # December
            days_to_xmas = 25 - day
            if days_to_xmas == 0:
                weight *= 10.0
            elif days_to_xmas < 0:
                weight *= -1.0 / float(days_to_xmas)
            else:
                weight *= 5.0 / math.sqrt(float(days_to_xmas))

    if "[W]" in occasions_str:  # Winter
        if month == 11:
            weight *= 0.5
        elif month == 12:
            weight *= 2.0
        elif month == 1:
            weight *= 1.5
        elif month == 2:
            weight *= 1.0
        elif month == 3:
            weight *= 0.25
        else:
            weight *= 0.000001

    if "[S]" in occasions_str:  # Summer
        if month == 5:
            weight *= 0.5
        elif month == 6:
            weight *= 2.0
        elif month == 7:
            weight *= 1.5
        elif month == 8:
            weight *= 1.0
        else:
            weight *= 0.2

    if "[K]" in occasions_str:  # Kids' Songs
        weight *= 0.000001

    return weight

def calculate_track_length_modifier(duration_ms: int) -> float:
    """Applies length ratio modifier sqrt(180s / duration_s), capped at max 2.0x bonus."""
    sec = max(1.0, float(duration_ms) / 1000.0)
    ratio = 180.0 / sec
    if ratio > 4.0:
        ratio = 4.0
    return math.sqrt(ratio)


class ProgramDirector:
    """
    [REQ-PD-010..080] Autonomous multi-pass playlist selector matching MuLibPlay algorithms.
    """
    def __init__(self, db: Database):
        self.db = db
        self.w_flow = 0.40
        self.w_time = 0.35
        self.w_pref = 0.25
        self.use_clock_autoselect = True

        # Candidate pool tuning limits (matching MuLibPlay defaults)
        self.excl_pool_sz = 1000
        self.rand_pool_sz = 100
        self.min_weight_limit = 0.001

    def parse_iso_time(self, iso_str: Optional[str]) -> Optional[float]:
        """Parses ISO timestamp string into epoch seconds."""
        if not iso_str:
            return None
        try:
            dt = datetime.strptime(iso_str, "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)
            return dt.timestamp()
        except Exception:
            return None

    def get_active_program(self, current_time_str: Optional[str] = None) -> Optional[dict]:
        """Selects active time-slot program closest to current clock time in past."""
        programs = self.db.get_all_programs()
        if not programs:
            return None

        if current_time_str is None:
            ltime = time.localtime()
            current_time_str = f"{ltime.tm_hour:02d}:{ltime.tm_min:02d}"

        try:
            ch, cm = map(int, current_time_str.split(":"))
            t_now = ch * 60 + cm
        except Exception:
            return programs[0]

        best_program = None
        min_delta = 24 * 60 + 1

        for prog in programs:
            try:
                sh, sm = map(int, prog["start_time"].split(":"))
                t_start = sh * 60 + sm
                delta = t_now - t_start
                if delta < 0:
                    delta += 24 * 60
                if delta < min_delta:
                    min_delta = delta
                    best_program = prog
            except Exception:
                continue

        return best_program

    def compute_program_target_vector(self, program: dict) -> Optional[Dict[str, float]]:
        """Calculates mean 11D AcousticBrainz feature vector across program seed tracks."""
        if not program or not program.get("track_ids"):
            return None

        track_ids = [tid.strip() for tid in program["track_ids"].split("\n") if tid.strip()]
        if not track_ids:
            return None

        feature_sums = {k: 0.0 for k in AB_KEYS}
        feature_counts = {k: 0 for k in AB_KEYS}

        conn = self.db.get_connection()
        try:
            for tid in track_ids:
                row = conn.execute("SELECT * FROM tracks WHERE id = ?", (tid,)).fetchone()
                if row:
                    d = dict(row)
                    for k in AB_KEYS:
                        val = d.get(k)
                        if val is not None:
                            try:
                                feature_sums[k] += float(val)
                                feature_counts[k] += 1
                            except (ValueError, TypeError):
                                pass
        finally:
            self.db.close_connection(conn)

        target_vec = {}
        for k in AB_KEYS:
            if feature_counts[k] > 0:
                target_vec[k] = feature_sums[k] / feature_counts[k]
            else:
                target_vec[k] = 0.5

        return target_vec

    def compute_acoustic_distance(self, current_desc: Dict[str, Any], candidate_desc: Dict[str, Any]) -> float:
        """Calculates normalized 11D Euclidean acoustic feature distance."""
        if all(k in current_desc and current_desc[k] is not None for k in AB_KEYS) and \
           all(k in candidate_desc and candidate_desc[k] is not None for k in AB_KEYS):
            sq_sum = sum((float(current_desc[k]) - float(candidate_desc[k])) ** 2 for k in AB_KEYS)
            return float(math.sqrt(sq_sum / 11.0))

        # Fallback 3D
        de = (current_desc.get("energy", 0.5) - candidate_desc.get("energy", 0.5)) ** 2
        dv = (current_desc.get("valence", 0.5) - candidate_desc.get("valence", 0.5)) ** 2
        dbpm = ((current_desc.get("tempo_bpm", 120.0) - candidate_desc.get("tempo_bpm", 120.0)) / 200.0) ** 2
        return float(math.sqrt(de + dv + dbpm))

    def get_target_energy_for_hour(self, hour: int) -> float:
        """[SPEC-PD-010] Returns target energy curve [0.0, 1.0] based on time of day."""
        if 0 <= hour < 6:
            return 0.30
        elif 6 <= hour < 12:
            return 0.60
        elif 12 <= hour < 18:
            return 0.85
        else:
            return 0.50

    def calculate_cooldown_penalty(self, track: Dict[str, Any], history: List[Dict[str, Any]]) -> float:
        """Calculates exponential decay penalty for recently played tracks."""
        if not history:
            return 0.0

        penalty = 0.0
        for idx, entry in enumerate(history[:50]):
            played_track_id = entry.get("track_id")
            elapsed_hours = (idx + 1) * 0.1
            if played_track_id == track["id"]:
                penalty += 10.0 * math.exp(-0.5 * elapsed_hours)

        return penalty

    def compute_candidate_weight(
        self,
        candidate: Dict[str, Any],
        artist_ratings_map: Dict[str, dict],
        now_sec: float,
        current_desc: Optional[Dict[str, Any]] = None,
        target_energy: Optional[float] = None
    ) -> float:
        """
        [REQ-PD-010..050] Computes composite selection weight W(k) matching MuLibPlay formula.
        """
        # 1. Track Restraint
        t_restraint = float(candidate.get("restraint") or 0.0)
        w_t_restraint = calculate_restraint_weight(t_restraint)

        # 2. Artist Restraint, Rotation, Recovery
        artist_name = candidate.get("artist", "")
        art_ratings = artist_ratings_map.get(artist_name, {})
        a_restraint = float(art_ratings.get("restraint") or 0.0)
        w_a_restraint = calculate_restraint_weight(a_restraint)

        a_rot_rv = float(art_ratings.get("rotation") if art_ratings.get("rotation") is not None else 0.778)
        a_rec_rv = float(art_ratings.get("recovery") if art_ratings.get("recovery") is not None else 0.778)
        a_rot_sec = rotation_to_seconds(a_rot_rv)
        a_rec_sec = rotation_to_seconds(a_rec_rv)

        a_last_played = self.parse_iso_time(art_ratings.get("last_played_at"))
        if a_last_played is not None:
            a_age = now_sec - a_last_played
            w_a_rec = calculate_recovery_weight(a_age, a_rot_sec, a_rec_sec)
        else:
            w_a_rec = 1.0

        if w_a_rec <= 0.0:
            return 0.0  # Artist rotation lockout

        # 3. Track Rotation & Recovery
        t_rot_rv = float(candidate.get("rotation") if candidate.get("rotation") is not None else 0.0)
        t_rec_rv = float(candidate.get("recovery") if candidate.get("recovery") is not None else 0.778)
        t_rot_sec = rotation_to_seconds(t_rot_rv)
        t_rec_sec = rotation_to_seconds(t_rec_rv)

        t_last_played = self.parse_iso_time(candidate.get("last_played_at"))
        if t_last_played is not None:
            t_age = now_sec - t_last_played
            w_t_rec = calculate_recovery_weight(t_age, t_rot_sec, t_rec_sec)
        else:
            w_t_rec = 1.0

        if w_t_rec <= 0.0:
            return 0.0  # Track rotation lockout

        # 4. Related Track Rotation Lockout
        w_rel_rec = 1.0
        related_links = self.db.get_related_tracks(candidate["id"]) if self.db else []
        if related_links:
            for rel_id, rel_wt in related_links:
                rel_trk = self.db.get_track_by_id(rel_id)
                if rel_trk and rel_trk.get("last_played_at"):
                    rel_last = self.parse_iso_time(rel_trk["last_played_at"])
                    if rel_last is not None:
                        rel_age = now_sec - rel_last
                        r_rec = calculate_recovery_weight(rel_age, t_rot_sec * rel_wt, t_rec_sec * rel_wt)
                        if r_rec < w_rel_rec:
                            w_rel_rec = r_rec
        if w_rel_rec <= 0.0:
            return 0.0

        # 5. Occasion & Length Weight
        w_occasion = calculate_occasion_weight(candidate.get("occasions"), current_time=now_sec)
        w_length = calculate_track_length_modifier(candidate.get("duration_ms", 180000))

        # 6. Flow Weight
        cand_desc = self.db.get_track_descriptors(candidate["id"]) if self.db else None
        if current_desc and cand_desc:
            dist = self.compute_acoustic_distance(current_desc, cand_desc)
            w_flow = max(0.0, 1.0 - dist)
        else:
            w_flow = 1.0

        # 7. Target Energy Curve Match
        if target_energy is not None and cand_desc and cand_desc.get("energy") is not None:
            w_time = max(0.01, 1.0 - abs(target_energy - float(cand_desc["energy"])))
        else:
            w_time = 1.0

        total_weight = w_t_restraint * w_a_restraint * w_t_rec * w_a_rec * w_rel_rec * w_occasion * w_flow * w_length * w_time
        return total_weight

    def select_next_track(
        self,
        current_track: Optional[Dict[str, Any]] = None,
        candidate_pool: Optional[List[Dict[str, Any]]] = None,
        current_hour: Optional[int] = None
    ) -> Optional[Dict[str, Any]]:
        """
        [REQ-PD-070] Multi-pass candidate pool refining and roulette-wheel weighted random sampling.
        """
        if not candidate_pool:
            candidate_pool = self.db.get_all_tracks(limit=500)

        if not candidate_pool:
            return None

        now_sec = time.time()
        artist_ratings_map = self.db.get_all_artist_ratings()

        current_desc = None
        if current_track:
            current_desc = self.db.get_track_descriptors(current_track["id"])

        target_energy = self.get_target_energy_for_hour(current_hour) if current_hour is not None else None

        # Pass 1: Compute candidate weights and filter eligible pool
        weighted_candidates: List[Tuple[Dict[str, Any], float]] = []
        for candidate in candidate_pool:
            if current_track and candidate["id"] == current_track["id"]:
                continue

            wt = self.compute_candidate_weight(
                candidate=candidate,
                artist_ratings_map=artist_ratings_map,
                now_sec=now_sec,
                current_desc=current_desc,
                target_energy=target_energy
            )
            if wt >= self.min_weight_limit:
                weighted_candidates.append((candidate, wt))

        if not weighted_candidates:
            # Fallback to un-locked candidate if pool is strictly locked out
            return candidate_pool[0]

        # Pass 2: Program Seed Alignment
        active_prog = self.get_active_program() if self.use_clock_autoselect else None
        prog_target_vec = self.compute_program_target_vector(active_prog) if active_prog else None

        if prog_target_vec and len(weighted_candidates) > self.excl_pool_sz:
            # Prune candidates farthest from program target vector
            with_dist = []
            for cand, wt in weighted_candidates:
                c_desc = self.db.get_track_descriptors(cand["id"]) or {}
                d = self.compute_acoustic_distance(prog_target_vec, c_desc)
                with_dist.append((d, cand, wt))
            with_dist.sort(key=lambda x: x[0])
            weighted_candidates = [(cand, wt) for d, cand, wt in with_dist[:self.excl_pool_sz]]

        # Pass 3: Queue-Tail Acoustic Flow Re-sorting
        if current_desc:
            flow_sorted = []
            for cand, wt in weighted_candidates:
                c_desc = self.db.get_track_descriptors(cand["id"]) or {}
                d = self.compute_acoustic_distance(current_desc, c_desc)
                flow_sorted.append((d, cand, wt))
            flow_sorted.sort(key=lambda x: x[0])
            candidate_list = [(cand, wt) for d, cand, wt in flow_sorted[:self.rand_pool_sz]]
        else:
            random.shuffle(weighted_candidates)
            candidate_list = weighted_candidates[:self.rand_pool_sz]

        # Pass 4: Roulette-Wheel Weighted Random Sampling with position decay (0.96^i)
        sample_weights = []
        decay = 1.0
        for cand, wt in candidate_list:
            final_wt = wt * decay
            sample_weights.append(final_wt)
            decay *= 0.96

        total_weight_sum = sum(sample_weights)
        if total_weight_sum <= 0:
            return candidate_list[0][0]

        target_r = random.uniform(0.0, total_weight_sum)
        cumulative = 0.0
        for idx, (cand, wt) in enumerate(candidate_list):
            cumulative += sample_weights[idx]
            if cumulative >= target_r:
                log_prog = active_prog['name'] if active_prog else 'Default'
                logger.info(f"Program Director selected track '{cand.get('title')}' by '{cand.get('artist')}' (Slot: {log_prog}, Weight: {wt:.4f})")
                return cand

        return candidate_list[0][0]

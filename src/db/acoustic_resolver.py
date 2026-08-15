import os
import json
import sqlite3
import urllib.request
import urllib.error
import logging
from typing import Dict, Any, Optional

logger = logging.getLogger(__name__)

MULIB_DEFAULT_PATH = r"C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db"
AB_COLS = [
    "ab_acoustic", "ab_aggressive", "ab_bright", "ab_danceable",
    "ab_female", "ab_happy", "ab_instrumental", "ab_party",
    "ab_relaxed", "ab_sad", "ab_tonal"
]

class AcousticResolver:
    """Resolves 11-Dimensional AcousticBrainz features via 3-Tier fallback hierarchy."""

    @classmethod
    def resolve_track(cls, track_row: sqlite3.Row, db_conn: sqlite3.Connection, mulib_path: str = MULIB_DEFAULT_PATH) -> Optional[Dict[str, float]]:
        """
        Resolves 11D features for a single track row.
        Returns dict of {col_name: float_val} or None if failed.
        """
        d = dict(track_row)

        # Check if already populated
        has_all = all(d.get(c) is not None for c in AB_COLS)
        if has_all:
            return {c: float(d[c]) for c in AB_COLS}

        # Tier 1: Ground Truth transfer from mulib.db
        res = cls._try_tier1_mulib(d, mulib_path)
        if res:
            logger.info(f"Tier 1 (mulib.db) resolved features for '{d.get('title')}'")
            cls._update_db_track(db_conn, d['id'], res)
            return res

        # Tier 2: AcousticBrainz API lookup by MBID
        mbid = d.get('musicbrainz_track_id')
        if mbid:
            res = cls._try_tier2_api(mbid)
            if res:
                logger.info(f"Tier 2 (AcousticBrainz API) resolved features for '{d.get('title')}'")
                cls._update_db_track(db_conn, d['id'], res)
                return res

        # Tier 3: Local audio ONNX Machine Learning extraction
        file_path = d.get('file_path')
        if file_path and os.path.exists(file_path):
            res = cls._try_tier3_onnx(file_path)
            if res:
                logger.info(f"Tier 3 (Local ONNX) resolved features for '{d.get('title')}'")
                cls._update_db_track(db_conn, d['id'], res)
                return res

        return None

    @classmethod
    def _try_tier1_mulib(cls, d: dict, mulib_path: str) -> Optional[Dict[str, float]]:
        """Tier 1: Look up pre-calculated values in mulib.db."""
        if not os.path.exists(mulib_path):
            return None

        mbid = d.get('musicbrainz_track_id')
        rel_path = d.get('file_path', '')
        
        # Extract relative tail path for matching (e.g. 'Artist\Album\Track.mp3')
        path_parts = rel_path.replace('/', '\\').split('\\')
        tail_path = "\\".join(path_parts[-2:]) if len(path_parts) >= 2 else rel_path

        try:
            mconn = sqlite3.connect(mulib_path)
            mconn.row_factory = sqlite3.Row
            row = None
            
            if mbid:
                cur = mconn.execute(
                    "SELECT abAcoustic, abAggressive, abBright, abDanceable, abFemale, abHappy, abInstrumental, abParty, abRelaxed, abSad, abTonal FROM tracks WHERE mbidRecording = ?",
                    (mbid,)
                )
                row = cur.fetchone()

            if not row and tail_path:
                tail_posix = tail_path.replace('\\', '/')
                cur = mconn.execute("""
                    SELECT t.abAcoustic, t.abAggressive, t.abBright, t.abDanceable, t.abFemale, t.abHappy,
                           t.abInstrumental, t.abParty, t.abRelaxed, t.abSad, t.abTonal
                    FROM tracks t
                    JOIN cuts c ON c.trackId = t.trackId
                    JOIN files f ON f.fileId = c.fileId
                    WHERE f.filePath LIKE ? OR f.filePath LIKE ?
                """, (f"%{tail_path}%", f"%{tail_posix}%"))
                row = cur.fetchone()

            mconn.close()

            if row and row["abAcoustic"] is not None:
                return {
                    "ab_acoustic": float(row["abAcoustic"]),
                    "ab_aggressive": float(row["abAggressive"]),
                    "ab_bright": float(row["abBright"]),
                    "ab_danceable": float(row["abDanceable"]),
                    "ab_female": float(row["abFemale"]),
                    "ab_happy": float(row["abHappy"]),
                    "ab_instrumental": float(row["abInstrumental"]),
                    "ab_party": float(row["abParty"]),
                    "ab_relaxed": float(row["abRelaxed"]),
                    "ab_sad": float(row["abSad"]),
                    "ab_tonal": float(row["abTonal"])
                }
        except Exception as e:
            logger.warning(f"Error querying mulib.db for Tier 1: {e}")

        return None

    @classmethod
    def _try_tier2_api(cls, mbid: str) -> Optional[Dict[str, float]]:
        """Tier 2: Query AcousticBrainz API via urllib."""
        url = f"http://acousticbrainz.org/api/v1/{mbid}/high-level"
        req = urllib.request.Request(url, headers={"User-Agent": "Vaino/1.0"})
        try:
            with urllib.request.urlopen(req, timeout=5.0) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode('utf-8'))
                    hl = data.get('highlevel', {})
                    return cls._extract_ab_dict_from_highlevel(hl)
        except Exception as e:
            logger.debug(f"AcousticBrainz API query failed for {mbid}: {e}")
        return None

    @classmethod
    def _try_tier3_onnx(cls, file_path: str) -> Optional[Dict[str, float]]:
        """Tier 3: Local audio file decode + ONNX ML extractor."""
        try:
            import miniaudio
            import numpy as np
            decoded = None
            try:
                decoded = miniaudio.decode_file(file_path)
            except Exception:
                try:
                    with open(file_path, "rb") as f:
                        decoded = miniaudio.decode_file(f.read())
                except Exception:
                    pass

            if decoded is not None and hasattr(decoded, "samples") and len(decoded.samples) > 0:
                raw_samples = np.frombuffer(decoded.samples, dtype=np.int16).astype(np.float32) / 32768.0
                sr = decoded.sample_rate

                if len(raw_samples) > 0:
                    from src.audio.onnx_extractor import ONNXHighLevelExtractor
                    hl = ONNXHighLevelExtractor.extract_descriptors(raw_samples, sr)
                    return cls._extract_ab_dict_from_highlevel(hl)
        except Exception as e:
            logger.warning(f"Local ONNX extraction failed for '{file_path}': {e}")
        return None

    @classmethod
    def _extract_ab_dict_from_highlevel(cls, hl: dict) -> Dict[str, float]:
        def get_val(section: str, key: str) -> float:
            sub = hl.get(section, {})
            if isinstance(sub, dict) and "all" in sub:
                return float(sub["all"].get(key, 0.5))
            elif isinstance(sub, dict):
                return float(sub.get(key, 0.5))
            return 0.5

        return {
            "ab_acoustic": get_val("mood_acoustic", "acoustic"),
            "ab_aggressive": get_val("mood_aggressive", "aggressive"),
            "ab_bright": get_val("timbre", "bright"),
            "ab_danceable": get_val("danceability", "danceable"),
            "ab_female": get_val("gender", "female"),
            "ab_happy": get_val("mood_happy", "happy"),
            "ab_instrumental": get_val("voice_instrumental", "instrumental"),
            "ab_party": get_val("mood_party", "party"),
            "ab_relaxed": get_val("mood_relaxed", "relaxed"),
            "ab_sad": get_val("mood_sad", "sad"),
            "ab_tonal": get_val("tonal_atonal", "tonal")
        }

    @classmethod
    def _update_db_track(cls, conn: sqlite3.Connection, track_id: str, ab_vals: Dict[str, float]):
        sql = """
        UPDATE tracks SET
            ab_acoustic = ?, ab_aggressive = ?, ab_bright = ?, ab_danceable = ?,
            ab_female = ?, ab_happy = ?, ab_instrumental = ?, ab_party = ?,
            ab_relaxed = ?, ab_sad = ?, ab_tonal = ?
        WHERE id = ?
        """
        conn.execute(sql, (
            ab_vals["ab_acoustic"], ab_vals["ab_aggressive"], ab_vals["ab_bright"], ab_vals["ab_danceable"],
            ab_vals["ab_female"], ab_vals["ab_happy"], ab_vals["ab_instrumental"], ab_vals["ab_party"],
            ab_vals["ab_relaxed"], ab_vals["ab_sad"], ab_vals["ab_tonal"],
            track_id
        ))
        conn.commit()

    @classmethod
    def batch_resolve_library(cls, db_path: str, mulib_path: str = MULIB_DEFAULT_PATH) -> int:
        """Batch resolves and populates missing 11D features for all tracks in library."""
        conn = sqlite3.connect(db_path)
        conn.row_factory = sqlite3.Row
        cur = conn.execute("SELECT * FROM tracks")
        rows = cur.fetchall()
        
        resolved_count = 0
        for row in rows:
            res = cls.resolve_track(row, conn, mulib_path)
            if res:
                resolved_count += 1

        conn.close()
        return resolved_count

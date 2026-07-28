# src/db/dao_slicer.py
"""
[REQ-AUD-030] [REQ-MB-020] Automated DAO Passage Slicer, Tracklist Alignment & RMS Boundary Refinement
Incorporates McRhythm algorithms:
1. 6-Strategy Multi-Query MusicBrainz Release Search (CamelCase splitting, unquoted, fuzzy ~1, album-only)
2. Edition Track-Count & Duration Signature Matching
3. RMS Low-Energy / Silence Boundary Refinement for sample-accurate track boundaries
"""

import os
import re
import json
import time
import urllib.request
import urllib.parse
import urllib.error
import logging
import numpy as np
import miniaudio
from typing import Dict, Any, List, Optional, Tuple
from .database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

USER_AGENT = "VainoAudioPlayer/1.0 ( https://github.com/MangoCats/Vaino )"


def camel_case_split(text: str) -> str:
    """Splits CamelCase strings (e.g., 'ZZTopsFirstAlbum' -> 'ZZ Tops First Album')."""
    if not text:
        return ""
    # Insert space before capital letters preceded by lowercase
    s1 = re.sub(r'([a-z0-9])([A-Z])', r'\1 \2', text)
    # Insert space between acronyms and capitalized words
    s2 = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1 \2', s1)
    return s2.strip()


class DAOSlicer:
    def __init__(self, db: Database):
        self.db = db
        self._last_req_time = 0.0

    def _rate_limit_throttle(self):
        """Enforces minimum 1.25s delay between requests per MusicBrainz API policy."""
        now = time.time()
        elapsed = now - self._last_req_time
        if elapsed < 1.25:
            time.sleep(1.25 - elapsed)
        self._last_req_time = time.time()

    def _safe_http_get(self, url: str, max_retries: int = 2) -> Optional[bytes]:
        """Sends HTTP GET request with MusicBrainz rate-limit throttle and backoff retries."""
        for attempt in range(max_retries + 1):
            self._rate_limit_throttle()
            try:
                req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
                with urllib.request.urlopen(req, timeout=10.0) as resp:
                    if resp.status == 200:
                        return resp.read()
            except urllib.error.HTTPError as e:
                if e.code in (429, 503):
                    logger.warning(f"MusicBrainz rate-limited (HTTP {e.code}) on {url}. Retrying in 3s... (attempt {attempt+1})")
                    time.sleep(3.0)
                else:
                    logger.debug(f"HTTP {e.code} for {url}")
                    break
            except Exception as e:
                logger.debug(f"HTTP GET error for {url}: {e}")
                break
        return None

    def fetch_musicbrainz_release_tracklist(self, artist: str, album: str, dao_duration_ms: Optional[int] = None) -> Optional[List[Dict[str, Any]]]:
        """
        [REQ-MB-020] McRhythm 6-Strategy Search & Duration Signature Matching:
        Queries MusicBrainz across multiple fallback strategies and selects the release edition
        whose cumulative duration signature closest matches the DAO capture.
        """
        if not album or not album.strip():
            return None

        clean_album = album.strip()
        clean_artist = artist.strip() if artist else ""
        split_album = camel_case_split(clean_album)
        split_artist = camel_case_split(clean_artist)

        # McRhythm Multi-Strategy Search Fallback Queries
        search_queries = []

        # Strategy 1: Standard Quoted Search
        if clean_artist and clean_artist.lower() != "unknown artist":
            search_queries.append(f'release:"{clean_album}" AND artist:"{clean_artist}"')
        else:
            search_queries.append(f'release:"{clean_album}"')

        # Strategy 2: CamelCase Split Search (if different)
        if (split_album != clean_album or split_artist != clean_artist) and clean_artist.lower() != "unknown artist":
            search_queries.append(f'release:"{split_album}" AND artist:"{split_artist}"')

        # Strategy 3: Unquoted Search
        if clean_artist and clean_artist.lower() != "unknown artist":
            search_queries.append(f'release:{clean_album} AND artist:{clean_artist}')

        # Strategy 4: Fuzzy ~1 Edit Search
        clean_alb_token = clean_album.split()[0] if clean_album else ""
        clean_art_token = clean_artist.split()[0] if clean_artist else ""
        if clean_alb_token and clean_art_token and clean_artist.lower() != "unknown artist":
            search_queries.append(f'release:{clean_alb_token}~1 AND artist:{clean_art_token}~1')

        # Strategy 5 & 6: Album-Only Fallback Search
        search_queries.append(f'release:"{clean_album}"')
        if split_album != clean_album:
            search_queries.append(f'release:"{split_album}"')
        search_queries.append(f'release:{clean_album}')

        candidate_editions = []

        for q_idx, query in enumerate(search_queries, 1):
            url = "https://musicbrainz.org/ws/2/release/"
            params = {"query": query, "fmt": "json", "limit": 5}
            full_url = f"{url}?{urllib.parse.urlencode(params)}"

            res = self._safe_http_get(full_url)
            if not res:
                continue

            try:
                data = json.loads(res.decode("utf-8"))
                releases = data.get("releases", [])
                for rel in releases:
                    release_id = rel.get("id")
                    if release_id:
                        tracklist = self.fetch_release_details(release_id)
                        if tracklist:
                            candidate_editions.append(tracklist)

                if candidate_editions:
                    logger.debug(f"Strategy {q_idx} ('{query}') returned {len(candidate_editions)} candidate release editions.")
                    break
            except Exception as e:
                logger.debug(f"Strategy {q_idx} error: {e}")

        if not candidate_editions:
            return None

        # Select Best Matching Edition using McRhythm Duration Signature Matching
        if dao_duration_ms and len(candidate_editions) > 1:
            best_edition = None
            best_diff = float("inf")

            for ed in candidate_editions:
                total_edition_ms = sum(t.get("length_ms", 0) for t in ed)
                diff = abs(total_edition_ms - dao_duration_ms)
                if diff < best_diff:
                    best_diff = diff
                    best_edition = ed

            if best_edition:
                return best_edition

        return candidate_editions[0]

    def fetch_release_details(self, release_id: str) -> Optional[List[Dict[str, Any]]]:
        url = f"https://musicbrainz.org/ws/2/release/{release_id}"
        params = {"inc": "recordings", "fmt": "json"}
        full_url = f"{url}?{urllib.parse.urlencode(params)}"

        res = self._safe_http_get(full_url)
        if not res:
            return None

        try:
            data = json.loads(res.decode("utf-8"))
            media = data.get("media", [])
            if media and "tracks" in media[0]:
                tracklist = []
                for idx, tr in enumerate(media[0]["tracks"], 1):
                    rec = tr.get("recording", {})
                    tracklist.append({
                        "track_number": idx,
                        "title": tr.get("title") or rec.get("title"),
                        "length_ms": tr.get("length") or rec.get("length", 180000),
                        "recording_mbid": rec.get("id"),
                        "release_mbid": release_id
                    })
                return tracklist
        except Exception as e:
            logger.warning(f"MusicBrainz release details failed for {release_id}: {e}")

        return None

    def refine_passage_boundaries_rms(self, file_path: str, raw_offsets_ms: List[int], search_window_sec: float = 15.0) -> List[int]:
        """
        [McRhythm Stage 6] RMS Low-Energy Boundary Refinement:
        Calculates 100ms RMS energy envelope across audio file and snaps expected boundary
        offsets to the nearest local RMS minimum (inter-track silence gap).
        """
        if len(raw_offsets_ms) <= 2 or not os.path.exists(file_path):
            return raw_offsets_ms

        try:
            decoded = None
            try:
                decoded = miniaudio.decode_file(file_path)
            except Exception:
                with open(file_path, "rb") as fp:
                    decoded = miniaudio.decode(fp.read())

            if not decoded or decoded.sample_rate <= 0:
                return raw_offsets_ms

            sr = decoded.sample_rate
            ch = decoded.nchannels
            raw_samples = np.frombuffer(decoded.samples, dtype=np.int16).astype(np.float32) / 32768.0
            if ch > 1:
                mono = raw_samples.reshape(-1, ch).mean(axis=1)
            else:
                mono = raw_samples

            # 100ms RMS frame size (0.1s)
            frame_size = int(sr * 0.1)
            num_frames = len(mono) // frame_size
            if num_frames < 10:
                return raw_offsets_ms

            # Compute RMS energy per 100ms frame
            truncated = mono[:num_frames * frame_size].reshape(num_frames, frame_size)
            rms_envelope = np.sqrt(np.mean(truncated ** 2, axis=1))

            refined = list(raw_offsets_ms)

            # Search window in 100ms frames (15s = 150 frames)
            window_frames = int(search_window_sec * 10)

            # Refine interior track boundaries (exclude 0 and final end offset)
            for i in range(1, len(raw_offsets_ms) - 1):
                exp_ms = raw_offsets_ms[i]
                exp_frame = int((exp_ms / 1000.0) * 10) # 10 frames per second

                start_search = max(0, exp_frame - window_frames)
                end_search = min(num_frames, exp_frame + window_frames)

                if start_search < end_search:
                    search_slice = rms_envelope[start_search:end_search]
                    min_idx = int(np.argmin(search_slice))
                    best_frame = start_search + min_idx
                    best_ms = int((best_frame / 10.0) * 1000)
                    refined[i] = best_ms

            return refined
        except Exception as e:
            logger.debug(f"RMS boundary refinement skipped for {file_path}: {e}")
            return raw_offsets_ms

    def slice_dao_file(self, dao_track: Dict[str, Any], enable_rms_refinement: bool = True) -> int:
        """
        [REQ-AUD-030] Generates individual track passages for a DAO continuous album capture file,
        applying McRhythm multi-strategy release search and RMS boundary refinement.
        """
        artist = dao_track["artist"]
        album = dao_track["album"]
        file_path = dao_track["file_path"]
        file_format = dao_track["file_format"]
        dao_duration_ms = dao_track.get("duration_ms")

        tracklist = self.fetch_musicbrainz_release_tracklist(artist, album, dao_duration_ms=dao_duration_ms)
        if not tracklist:
            logger.info(f"No MusicBrainz tracklist found for DAO capture: {artist} - {album}")
            return 0

        # Calculate initial raw expected millisecond offsets
        raw_offsets_ms = [0]
        curr = 0
        for tr in tracklist:
            dur = tr.get("length_ms") or 180000
            curr += dur
            raw_offsets_ms.append(curr)

        # Apply McRhythm Stage 6 RMS silence boundary refinement if audio file exists
        if enable_rms_refinement and os.path.exists(file_path):
            offsets_ms = self.refine_passage_boundaries_rms(file_path, raw_offsets_ms)
        else:
            offsets_ms = raw_offsets_ms

        passages_created = 0

        for idx, tr in enumerate(tracklist):
            track_num = tr["track_number"]
            title = tr["title"]
            start_offset_ms = offsets_ms[idx]
            end_offset_ms = offsets_ms[idx + 1]
            passage_duration = max(1000, end_offset_ms - start_offset_ms)

            mbid = tr.get("recording_mbid")
            release_mbid = tr.get("release_mbid")

            passage_id = f"{dao_track['id']}_p{track_num:02d}"

            passage_record = {
                "id": passage_id,
                "file_path": file_path,  # Points to the same underlying DAO audio file
                "file_format": file_format,
                "title": title,
                "artist": artist,
                "album": album,
                "year": dao_track.get("year"),
                "track_number": track_num,
                "duration_ms": passage_duration,
                "start_offset_ms": start_offset_ms,
                "end_offset_ms": end_offset_ms,
                "has_cover_art": dao_track.get("has_cover_art", 0),
                "file_mtime": dao_track.get("file_mtime", 0.0),
                "file_size": dao_track.get("file_size", 0),
                "musicbrainz_track_id": mbid,
                "musicbrainz_album_id": release_mbid
            }

            self.db.upsert_track(passage_record)
            passages_created += 1

        logger.info(f"Sliced DAO album [{artist} - {album}] into {passages_created} passage tracks!")
        return passages_created

    def process_all_dao_files(self, limit: int = 100) -> int:
        """
        Finds true DAO continuous album captures in DB and slices them into passage tracks.
        """
        conn = self.db.get_connection()
        try:
            cursor = conn.execute("""
                SELECT id, file_path, file_format, title, artist, album, year, duration_ms, file_size, has_cover_art, file_mtime
                FROM tracks
                WHERE duration_ms > 900000 AND file_size > 26214400 AND (start_offset_ms = 0 OR start_offset_ms IS NULL) AND end_offset_ms IS NULL
                LIMIT ?
            """, (limit,))
            dao_files = [dict(row) for row in cursor.fetchall()]
        finally:
            self.db.close_connection(conn)

        logger.info(f"Found {len(dao_files)} unsliced DAO album files ready for passage slicing...")
        total_sliced = 0
        for dao in dao_files:
            count = self.slice_dao_file(dao)
            total_sliced += count

        return total_sliced


if __name__ == "__main__":
    db = Database("vaino.db")
    slicer = DAOSlicer(db)
    slicer.process_all_dao_files(limit=10)

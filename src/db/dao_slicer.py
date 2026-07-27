# src/db/dao_slicer.py
"""
[REQ-AUD-030] [REQ-MB-020] Automated DAO Passage Slicer & Tracklist Alignment
Queries MusicBrainz release tracklists for continuous DAO album captures and generates
individual passage track records in `vaino.db` with exact `start_offset_ms` and `end_offset_ms`.
"""

import os
import json
import time
import urllib.request
import urllib.parse
import urllib.error
import logging
from typing import Dict, Any, List, Optional, Tuple
from .database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

USER_AGENT = "VainoAudioPlayer/1.0 ( https://github.com/MangoCats/Vaino )"


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

    def fetch_musicbrainz_release_tracklist(self, artist: str, album: str) -> Optional[List[Dict[str, Any]]]:
        """
        [REQ-MB-020] Queries MusicBrainz API for official release tracklist and track durations.
        """
        if not album or not album.strip():
            return None

        clean_album = album.strip()
        clean_artist = artist.strip() if artist else ""

        if clean_artist and clean_artist.lower() != "unknown artist":
            query = f'release:"{clean_album}" AND artist:"{clean_artist}"'
        else:
            query = f'release:"{clean_album}"'

        url = "https://musicbrainz.org/ws/2/release/"
        params = {"query": query, "fmt": "json", "limit": 3}
        full_url = f"{url}?{urllib.parse.urlencode(params)}"

        res = self._safe_http_get(full_url)
        if not res:
            return None

        try:
            data = json.loads(res.decode("utf-8"))
            releases = data.get("releases", [])
            for rel in releases:
                release_id = rel.get("id")
                if release_id:
                    tracklist = self.fetch_release_details(release_id)
                    if tracklist:
                        return tracklist
        except Exception as e:
            logger.warning(f"MusicBrainz release search failed for {artist} - {album}: {e}")

        return None

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

    def slice_dao_file(self, dao_track: Dict[str, Any]) -> int:
        """
        [REQ-AUD-030] Generates individual track passages for a DAO continuous album capture file.
        """
        artist = dao_track["artist"]
        album = dao_track["album"]
        file_path = dao_track["file_path"]
        file_format = dao_track["file_format"]

        tracklist = self.fetch_musicbrainz_release_tracklist(artist, album)
        if not tracklist:
            logger.info(f"No MusicBrainz tracklist found for DAO capture: {artist} - {album}")
            return 0

        current_offset_ms = 0
        passages_created = 0

        for tr in tracklist:
            track_num = tr["track_number"]
            title = tr["title"]
            duration = tr["length_ms"] or 180000
            end_offset_ms = current_offset_ms + duration
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
                "duration_ms": duration,
                "start_offset_ms": current_offset_ms,
                "end_offset_ms": end_offset_ms,
                "has_cover_art": dao_track.get("has_cover_art", 0),
                "file_mtime": dao_track.get("file_mtime", 0.0),
                "file_size": dao_track.get("file_size", 0),
                "musicbrainz_track_id": mbid,
                "musicbrainz_album_id": release_mbid
            }

            self.db.upsert_track(passage_record)
            passages_created += 1
            current_offset_ms = end_offset_ms

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

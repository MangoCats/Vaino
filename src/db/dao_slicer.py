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
import logging
from typing import Dict, Any, List, Optional, Tuple
from .database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class DAOSlicer:
    def __init__(self, db: Database):
        self.db = db

    def fetch_musicbrainz_release_tracklist(self, artist: str, album: str) -> Optional[List[Dict[str, Any]]]:
        """
        [REQ-MB-020] Queries MusicBrainz API for official release tracklist and track durations.
        """
        query = f'artist:"{artist}" AND release:"{album}"'
        url = "https://musicbrainz.org/ws/2/release/"
        params = {"query": query, "fmt": "json", "limit": 1}
        full_url = f"{url}?{urllib.parse.urlencode(params)}"

        try:
            req = urllib.request.Request(full_url, headers={"User-Agent": "Vaino/0.1.0 ( contact@vaino.org )"})
            with urllib.request.urlopen(req, timeout=6) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode("utf-8"))
                    releases = data.get("releases", [])
                    if releases:
                        release_id = releases[0]["id"]
                        return self.fetch_release_details(release_id)
        except Exception as e:
            logger.warning(f"MusicBrainz release search failed for {artist} - {album}: {e}")

        return None

    def fetch_release_details(self, release_id: str) -> Optional[List[Dict[str, Any]]]:
        url = f"https://musicbrainz.org/ws/2/release/{release_id}"
        params = {"inc": "recordings", "fmt": "json"}
        full_url = f"{url}?{urllib.parse.urlencode(params)}"

        try:
            req = urllib.request.Request(full_url, headers={"User-Agent": "Vaino/0.1.0 ( contact@vaino.org )"})
            with urllib.request.urlopen(req, timeout=6) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode("utf-8"))
                    media = data.get("media", [])
                    if media and "tracks" in media[0]:
                        tracklist = []
                        for idx, tr in enumerate(media[0]["tracks"], 1):
                            rec = tr.get("recording", {})
                            tracklist.append({
                                "track_number": idx,
                                "title": tr.get("title") or rec.get("title"),
                                "length_ms": tr.get("length") or rec.get("length", 180000),
                                "recording_mbid": rec.get("id")
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
                "musicbrainz_track_id": mbid
            }

            self.db.upsert_track(passage_record)
            passages_created += 1
            current_offset_ms = end_offset_ms

        logger.info(f"Sliced DAO album [{artist} - {album}] into {passages_created} passage tracks!")
        return passages_created

    def process_all_dao_files(self, limit: int = 10) -> int:
        """
        Finds true DAO continuous album captures in DB and slices them into passage tracks.
        """
        conn = self.db.get_connection()
        try:
            cursor = conn.execute("""
                SELECT id, file_path, file_format, title, artist, album, year, duration_ms, file_size, has_cover_art, file_mtime
                FROM tracks
                WHERE duration_ms > 900000 AND file_size > 26214400 AND start_offset_ms = 0 AND end_offset_ms IS NULL
                LIMIT ?
            """, (limit,))
            dao_files = [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

        logger.info(f"Found {len(dao_files)} DAO album files ready for passage slicing...")
        total_sliced = 0
        for dao in dao_files:
            count = self.slice_dao_file(dao)
            total_sliced += count
            time.sleep(1.0)  # Rate limit for MusicBrainz API

        return total_sliced

if __name__ == "__main__":
    db = Database("vaino.db")
    slicer = DAOSlicer(db)
    slicer.process_all_dao_files(limit=5)

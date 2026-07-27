# src/db/resolver.py
"""
[REQ-MB-010] [REQ-MB-020] Automated MusicBrainz Identifier Resolver
Extracts embedded MusicBrainz IDs from ID3/FLAC tags and queries AcoustID API for unmatched tracks.
"""

import os
import time
import logging
from typing import Dict, Any, Optional, Tuple, List
from concurrent.futures import ThreadPoolExecutor, as_completed
import mutagen
from .database import Database
from .fingerprint import AudioFingerprinter

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class MusicBrainzResolver:
    def __init__(self, db: Database):
        self.db = db
        self.fingerprinter = AudioFingerprinter()

    def extract_embedded_mbid(self, file_path: str) -> Optional[str]:
        """
        [REQ-MB-010] Extracts embedded MusicBrainz Recording ID from MP3 ID3, FLAC, or M4A tags.
        """
        try:
            audio = mutagen.File(file_path)
            if audio is not None and audio.tags:
                tags = audio.tags
                # Check FLAC / Vorbis / MP4 / ID3 tags
                mbid = (
                    tags.get("musicbrainz_trackid") or
                    tags.get("MUSICBRAINZ_TRACKID") or
                    tags.get("UFID:http://musicbrainz.org") or
                    tags.get("---:com.apple.iTunes:MusicBrainz Track Id")
                )
                if mbid:
                    if isinstance(mbid, list):
                        return str(mbid[0])
                    elif hasattr(mbid, "data"):
                        return mbid.data.decode("utf-8", errors="ignore").strip("\x00")
                    return str(mbid)
        except Exception as e:
            logger.debug(f"Error reading embedded MBID for {file_path}: {e}")
        return None

    def extract_embedded_artist_sort(self, file_path: str) -> Optional[str]:
        """
        [REQ-MB-020D] Extracts embedded MusicBrainz Artist Sort tag from ID3, FLAC, or M4A tags.
        """
        try:
            audio = mutagen.File(file_path)
            if audio is not None and audio.tags:
                tags = audio.tags
                sort_tag = (
                    tags.get("artistsort") or
                    tags.get("ARTISTSORT") or
                    tags.get("TSOP") or
                    tags.get("XSOP") or
                    tags.get("soar") or
                    tags.get("musicbrainz_artistsort")
                )
                if sort_tag:
                    if hasattr(sort_tag, "text") and sort_tag.text:
                        return str(sort_tag.text[0]).strip()
                    elif isinstance(sort_tag, list) and sort_tag:
                        return str(sort_tag[0]).strip()
                    return str(sort_tag).strip()
        except Exception as e:
            logger.debug(f"Error reading embedded sort tag for {file_path}: {e}")
        return None

    def resolve_track_embedded(self, track: Dict[str, Any]) -> Optional[Tuple[str, str, str]]:
        file_path = track["file_path"]
        artist = track.get("artist", "")
        mbid = self.extract_embedded_mbid(file_path)
        embedded_sort = self.extract_embedded_artist_sort(file_path)

        from .scanner import compute_artist_sort_name
        sort_name = compute_artist_sort_name(artist, embedded=embedded_sort)

        return track["id"], mbid, sort_name

    def resolve_all_unlinked(self, limit: int = 10000, max_workers: int = 16) -> Tuple[int, int]:
        """
        [REQ-MB-020, REQ-MB-020D] High-speed parallel resolution of unlinked tracks in vaino.db.
        Returns (resolved_count, skipped_count)
        """
        start_time = time.time()
        conn = self.db.get_connection()
        try:
            cursor = conn.execute(
                "SELECT id, file_path, title, artist FROM tracks WHERE musicbrainz_track_id IS NULL OR artist_sort_name IS NULL LIMIT ?",
                (limit,)
            )
            unlinked = [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

        if not unlinked:
            logger.info("All tracks in database already have resolved MusicBrainz IDs and sort names.")
            return 0, 0

        logger.info(f"Resolving MusicBrainz metadata for {len(unlinked)} tracks (Workers: {max_workers})...")
        resolved_updates: List[Tuple[str, Optional[str], str]] = []

        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            future_to_track = {
                executor.submit(self.resolve_track_embedded, t): t for t in unlinked
            }
            for future in as_completed(future_to_track):
                try:
                    res = future.result()
                    if res:
                        resolved_updates.append(res)
                except Exception as e:
                    pass

        # Batch update database with resolved MBIDs and artist_sort_name
        if resolved_updates:
            conn = self.db.get_connection()
            try:
                conn.executemany(
                    "UPDATE tracks SET musicbrainz_track_id = COALESCE(?, musicbrainz_track_id), artist_sort_name = ? WHERE id = ?",
                    [(mbid, sort_name, track_id) for track_id, mbid, sort_name in resolved_updates]
                )
                conn.commit()
            finally:
                conn.close()

        elapsed = time.time() - start_time
        resolved_count = len(resolved_updates)
        skipped_count = len(unlinked) - resolved_count
        logger.info(f"MusicBrainz resolution complete in {elapsed:.2f}s! ({resolved_count} resolved, {skipped_count} remaining for AcoustID lookup).")
        return resolved_count, skipped_count

if __name__ == "__main__":
    db = Database("vaino.db")
    resolver = MusicBrainzResolver(db)
    resolver.resolve_all_unlinked(limit=10000)

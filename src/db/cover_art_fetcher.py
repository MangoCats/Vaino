"""
Automated Cover Art Fetcher & Resolver Module
Resolves album cover art from:
1. Local album directory image files (cover.jpg, folder.jpg, album.jpg, front.jpg)
2. MusicBrainz Release MBID & Cover Art Archive (https://coverartarchive.org)
3. MusicBrainz Release Search API
"""

import os
import re
import time
import urllib.request
import urllib.parse
import urllib.error
import json
import logging
from typing import Optional, Tuple, List, Dict, Any
from .database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

USER_AGENT = "VainoAudioPlayer/1.0 ( https://github.com/MangoCats/Vaino )"


class CoverArtFetcher:
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

    def _safe_http_get(self, url: str, max_retries: int = 2) -> Optional[Tuple[bytes, str]]:
        """Sends HTTP GET request with MusicBrainz rate-limit throttle and backoff retries."""
        for attempt in range(max_retries + 1):
            self._rate_limit_throttle()
            try:
                req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
                with urllib.request.urlopen(req, timeout=10.0) as resp:
                    if resp.status == 200:
                        content_type = resp.headers.get("Content-Type", "image/jpeg")
                        data = resp.read()
                        return data, content_type
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

    def scan_folder_for_art(self, file_path: str) -> Optional[Tuple[bytes, str]]:
        """Scans local folder of track for cover image files."""
        if not file_path or not os.path.exists(file_path):
            return None

        folder = os.path.dirname(file_path)
        art_filenames = [
            "cover.jpg", "cover.jpeg", "cover.png",
            "folder.jpg", "folder.jpeg", "folder.png",
            "album.jpg", "album.jpeg", "album.png",
            "front.jpg", "front.jpeg", "front.png"
        ]

        # 1. Exact match search
        for name in art_filenames:
            candidate = os.path.join(folder, name)
            if os.path.isfile(candidate) and os.path.getsize(candidate) > 0:
                try:
                    ext = os.path.splitext(name)[1].lower()
                    mime = "image/png" if ext == ".png" else "image/jpeg"
                    with open(candidate, "rb") as f:
                        return f.read(), mime
                except Exception:
                    pass

        # 2. Pattern search for any .jpg / .png image in directory
        try:
            for fname in os.listdir(folder):
                if fname.lower().endswith((".jpg", ".jpeg", ".png")):
                    full = os.path.join(folder, fname)
                    if os.path.isfile(full) and os.path.getsize(full) > 0:
                        ext = os.path.splitext(fname)[1].lower()
                        mime = "image/png" if ext == ".png" else "image/jpeg"
                        with open(full, "rb") as f:
                            return f.read(), mime
        except Exception:
            pass

        return None

    def fetch_from_cover_art_archive(self, mbid: str, is_release_group: bool = False) -> Optional[Tuple[bytes, str]]:
        """Downloads front cover image from Cover Art Archive by Release or Release Group MBID."""
        if not mbid or not mbid.strip():
            return None

        endpoint = "release-group" if is_release_group else "release"
        url = f"https://coverartarchive.org/{endpoint}/{mbid.strip()}/front-500"
        alt_url = f"https://coverartarchive.org/{endpoint}/{mbid.strip()}/front"

        for target_url in [url, alt_url]:
            res = self._safe_http_get(target_url)
            if res and len(res[0]) > 1000:
                return res

        return None

    def search_musicbrainz_and_fetch_art(self, album_name: str, artist_name: Optional[str]) -> Optional[Tuple[bytes, str]]:
        """Queries MusicBrainz API for album release MBID and fetches front cover art."""
        if not album_name or not album_name.strip():
            return None

        clean_album = album_name.strip()
        clean_artist = artist_name.strip() if artist_name else ""

        # Construct MusicBrainz search query
        if clean_artist and clean_artist.lower() != "unknown artist":
            query = f'release:"{clean_album}" AND artist:"{clean_artist}"'
        else:
            query = f'release:"{clean_album}"'

        mb_url = f"https://musicbrainz.org/ws/2/release/?query={urllib.parse.quote(query)}&fmt=json&limit=3"

        res = self._safe_http_get(mb_url)
        if not res:
            return None

        try:
            data = json.loads(res[0].decode("utf-8"))
            releases = data.get("releases", [])
            for r in releases:
                mbid = r.get("id")
                if mbid:
                    art = self.fetch_from_cover_art_archive(mbid, is_release_group=False)
                    if art:
                        return art
                    
                # Try release-group MBID
                rg = r.get("release-group", {})
                rg_mbid = rg.get("id")
                if rg_mbid:
                    art = self.fetch_from_cover_art_archive(rg_mbid, is_release_group=True)
                    if art:
                        return art
        except Exception as e:
            logger.debug(f"MusicBrainz JSON parse error for {album_name}: {e}")

        return None

    def fetch_art_via_recording_mbid(self, recording_mbid: str) -> Optional[Tuple[bytes, str]]:
        """Queries MusicBrainz Recording API for Release / Release Group MBID and fetches Cover Art Archive image."""
        if not recording_mbid or not recording_mbid.strip():
            return None

        url = f"https://musicbrainz.org/ws/2/recording/{recording_mbid.strip()}?inc=releases+release-groups&fmt=json"
        res = self._safe_http_get(url)
        if not res:
            return None

        try:
            data = json.loads(res[0].decode("utf-8"))
            releases = data.get("releases", [])
            for r in releases:
                rel_mbid = r.get("id")
                rg_mbid = r.get("release-group", {}).get("id")

                if rel_mbid:
                    art = self.fetch_from_cover_art_archive(rel_mbid, is_release_group=False)
                    if art:
                        return art

                if rg_mbid:
                    art = self.fetch_from_cover_art_archive(rg_mbid, is_release_group=True)
                    if art:
                        return art
        except Exception as e:
            logger.debug(f"Error fetching release art for recording MBID {recording_mbid}: {e}")

        return None

    def resolve_album_art(self, album_name: str, artist_name: Optional[str], sample_file_path: Optional[str] = None, recording_mbid: Optional[str] = None) -> Optional[Tuple[bytes, str]]:
        """
        Resolves album art using priority fallback:
        1. Database stored album_cover_art
        2. Local directory cover files
        3. Recording MBID -> Cover Art Archive
        4. MusicBrainz Search API -> Cover Art Archive
        """
        # 1. Check SQLite album_cover_art table
        stored = self.db.get_album_cover_art(album_name, artist_name)
        if stored:
            return stored

        # 2. Scan local folder
        if sample_file_path:
            local_art = self.scan_folder_for_art(sample_file_path)
            if local_art:
                img_bytes, mime = local_art
                self.db.save_album_cover_art(album_name, artist_name, img_bytes, mime, source="FOLDER")
                return local_art

        # 3. Query Cover Art Archive via Recording MBID
        if recording_mbid:
            rec_art = self.fetch_art_via_recording_mbid(recording_mbid)
            if rec_art:
                img_bytes, mime = rec_art
                self.db.save_album_cover_art(album_name, artist_name, img_bytes, mime, source="MUSICBRAINZ_MBID")
                return rec_art

        # 4. Query MusicBrainz / Cover Art Archive via Search
        mb_art = self.search_musicbrainz_and_fetch_art(album_name, artist_name)
        if mb_art:
            img_bytes, mime = mb_art
            self.db.save_album_cover_art(album_name, artist_name, img_bytes, mime, source="MUSICBRAINZ_SEARCH")
            return mb_art

        return None

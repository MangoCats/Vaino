import os
import time
import hashlib
import logging
from typing import Optional, Dict, Any, Tuple, Set, List
from concurrent.futures import ThreadPoolExecutor, as_completed
import mutagen
from mutagen.mp4 import MP4, MP4Cover
from .database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

SUPPORTED_EXTENSIONS = {".mp3", ".flac", ".wav", ".ogg", ".m4a"}

import unicodedata

def normalize_diacritics(text: str) -> str:
    """Removes diacritics and accents (e.g. Mötley Crüe -> Motley Crue, Beyoncé -> Beyonce)."""
    if not text:
        return ""
    nfkd = unicodedata.normalize('NFD', text)
    return "".join(c for c in nfkd if unicodedata.category(c) != 'Mn')

KNOWN_SINGLE_GROUPS = {
    "mötley crüe", "motley crue", "blue öyster cult", "blue oyster cult",
    "pink floyd", "led zeppelin", "deep purple", "black sabbath",
    "lynyrd skynyrd", "judas priest", "iron maiden", "jethro tull",
    "fleetwood mac", "def leppard", "bad company", "dire straits",
    "earth, wind & fire", "earth wind & fire",
    "sam the sham & the pharaohs", "sam the sham and the pharaohs",
    "emerson, lake & palmer", "emerson lake & palmer",
    "crosby, stills, nash & young", "crosby, stills & nash",
    "sly & the family stone", "sly and the family stone",
    "the mamas & the papas", "kurtis blow", "hall & oates", "daryl hall & john oates",
    "katrina and the waves", "huey lewis & the news", "huey lewis and the news",
    "hootie & the blowfish", "hootie and the blowfish", "kc & the sunshine band",
    "gladys knight & the pips", "mott the hoople", "spartak", "iron & wine",
    "ziggy marley & the melody makers", "bob marley & the wailers",
    "tom petty and the heartbreakers", "tom petty & the heartbreakers",
    "joan jett & the blackhearts", "joan jett and the blackhearts"
}

def compute_artist_sort_name(artist: str, embedded: Optional[str] = None) -> str:
    """[REQ-MB-020D, REQ-UI-020I] Computes canonical MusicBrainz sort name for an artist, normalized for A-Z filtering."""
    if embedded and str(embedded).strip():
        return normalize_diacritics(str(embedded).strip())
    
    if not artist or not str(artist).strip():
        return "Unknown Artist"

    artist_str = str(artist).strip()
    if artist_str.lower() in KNOWN_SINGLE_GROUPS or normalize_diacritics(artist_str.lower()) in KNOWN_SINGLE_GROUPS:
        return normalize_diacritics(artist_str)

    if "," in artist_str:
        return normalize_diacritics(artist_str)

    if artist_str.lower().startswith("the "):
        res = f"{artist_str[4:]}, The"
    elif artist_str.lower().startswith("a "):
        res = f"{artist_str[2:]}, A"
    else:
        parts = artist_str.split()
        if len(parts) == 2 and not any(p.endswith(".") for p in parts):
            res = f"{parts[1]}, {parts[0]}"
        else:
            res = artist_str

    return normalize_diacritics(res)

def split_artists(artist_str: str) -> List[Tuple[str, str]]:
    """
    [REQ-MB-020E, REQ-UI-020G] Decomposes combined artist strings into individual (artist_name, artist_sort_name) tuples.
    Handles 'feat.', 'ft.', 'featuring', 'with', 'vs.', '/', '&', and 'and' separators while preserving canonical groups.
    """
    if not artist_str or not str(artist_str).strip():
        return [("Unknown Artist", "Unknown Artist")]

    raw = str(artist_str).strip()
    if raw.lower() in KNOWN_SINGLE_GROUPS:
        return [(raw, compute_artist_sort_name(raw))]

    import re
    pattern = r'\s+(?:feat\.?|ft\.?|featuring|with|vs\.?|\/|\&|and)\s+'
    parts = re.split(pattern, raw, flags=re.IGNORECASE)

    results = []
    for p in parts:
        cleaned = p.strip()
        if not cleaned:
            continue
        sub_parts = [sp.strip() for sp in re.split(r'[\/\&]', cleaned) if sp.strip()]
        for sp in sub_parts:
            sub_sub = re.split(r'\s+feat\.?\s+', sp, flags=re.IGNORECASE)
            for sss in sub_sub:
                name = sss.strip()
                if name:
                    sort_name = compute_artist_sort_name(name)
                    if (name, sort_name) not in results:
                        results.append((name, sort_name))

    return results if results else [(raw, compute_artist_sort_name(raw))]

class MediaScanner:
    def __init__(self, db: Database, music_dir: str):
        self.db = db
        self.music_dir = os.path.abspath(music_dir)
        self._folder_art_cache: Dict[str, Optional[str]] = {}

    def generate_track_id(self, file_path: str) -> str:
        rel_path = os.path.relpath(file_path, self.music_dir)
        return hashlib.sha256(rel_path.encode("utf-8")).hexdigest()[:16]

    def find_folder_art(self, file_path: str) -> Optional[str]:
        folder = os.path.dirname(file_path)
        if folder in self._folder_art_cache:
            return self._folder_art_cache[folder]

        art_names = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png", "Hotelcalifornia.jpg"]
        found_art = None
        try:
            for name in os.listdir(folder):
                if name.lower() in [n.lower() for n in art_names]:
                    found_art = os.path.join(folder, name)
                    break
                if not found_art and name.lower().endswith((".jpg", ".png", ".jpeg")):
                    found_art = os.path.join(folder, name)
        except Exception:
            pass

        self._folder_art_cache[folder] = found_art
        return found_art

    def extract_cover_art_bytes(self, file_path: str) -> Optional[Tuple[bytes, str]]:
        try:
            audio = mutagen.File(file_path)
            if audio is not None:
                if hasattr(audio, "tags") and audio.tags is not None:
                    for key in audio.tags.keys():
                        if key.startswith("APIC:"):
                            apic = audio.tags[key]
                            return apic.data, apic.mime
                if hasattr(audio, "pictures") and audio.pictures:
                    pic = audio.pictures[0]
                    return pic.data, pic.mime
                if isinstance(audio, MP4) and "covr" in audio.tags:
                    covers = audio.tags["covr"]
                    if covers:
                        cover = covers[0]
                        mime = "image/png" if cover.imageformat == MP4Cover.FORMAT_PNG else "image/jpeg"
                        return bytes(cover), mime
        except Exception as e:
            logger.debug(f"Error checking embedded art for {file_path}: {e}")

        folder_art_path = self.find_folder_art(file_path)
        if folder_art_path and os.path.exists(folder_art_path):
            try:
                ext = os.path.splitext(folder_art_path)[1].lower()
                mime = "image/png" if ext == ".png" else "image/jpeg"
                with open(folder_art_path, "rb") as f:
                    return f.read(), mime
            except Exception as e:
                logger.debug(f"Error reading folder art {folder_art_path}: {e}")

        return None

    def check_has_art(self, file_path: str) -> bool:
        """Fast check if artwork exists without loading full image bytes."""
        folder_art = self.find_folder_art(file_path)
        if folder_art:
            return True
        try:
            audio = mutagen.File(file_path)
            if audio is not None:
                if hasattr(audio, "tags") and audio.tags is not None:
                    for key in audio.tags.keys():
                        if key.startswith("APIC:"):
                            return True
                if hasattr(audio, "pictures") and audio.pictures:
                    return True
                if isinstance(audio, MP4) and "covr" in audio.tags and audio.tags["covr"]:
                    return True
        except Exception:
            pass
        return False

    def parse_track_metadata(self, file_path: str, stat_info: Optional[os.stat_result] = None) -> Dict[str, Any]:
        ext = os.path.splitext(file_path)[1].lower()
        track_id = self.generate_track_id(file_path)
        if stat_info is None:
            stat_info = os.stat(file_path)

        has_art = self.check_has_art(file_path)

        filename = os.path.splitext(os.path.basename(file_path))[0]
        parent_folder = os.path.basename(os.path.dirname(file_path))

        title = filename
        artist = parent_folder if parent_folder else "Unknown Artist"
        album = parent_folder if parent_folder else "Unknown Album"
        year = None
        track_number = None
        duration_ms = 0

        try:
            audio = mutagen.File(file_path)
            if audio is not None:
                if hasattr(audio.info, "length"):
                    duration_ms = int(audio.info.length * 1000)

                tags = audio.tags
                if tags:
                    title_tag = tags.get("title") or tags.get("TIT2")
                    artist_tag = tags.get("artist") or tags.get("TPE1")
                    album_tag = tags.get("album") or tags.get("TALB")
                    date_tag = tags.get("date") or tags.get("TDRC") or tags.get("TYER")
                    track_tag = tags.get("tracknumber") or tags.get("TRCK")
                    artist_sort_tag = (
                        tags.get("artistsort") or
                        tags.get("ARTISTSORT") or
                        tags.get("TSOP") or
                        tags.get("XSOP") or
                        tags.get("soar") or
                        tags.get("musicbrainz_artistsort")
                    )

                    if title_tag:
                        title = str(title_tag[0])
                    if artist_tag:
                        artist = str(artist_tag[0])
                    if album_tag:
                        album = str(album_tag[0])
                    if date_tag:
                        try:
                            year = int(str(date_tag[0])[:4])
                        except ValueError:
                            pass
                    if track_tag:
                        try:
                            track_str = str(track_tag[0]).split("/")[0]
                            track_number = int(track_str)
                        except ValueError:
                            pass
        except Exception as e:
            logger.warning(f"Error parsing metadata for {file_path}: {e}")

        embedded_sort = None
        if artist_sort_tag:
            if hasattr(artist_sort_tag, "text") and artist_sort_tag.text:
                embedded_sort = str(artist_sort_tag.text[0])
            elif isinstance(artist_sort_tag, list) and artist_sort_tag:
                embedded_sort = str(artist_sort_tag[0])
            else:
                embedded_sort = str(artist_sort_tag)

        artist_sort_name = compute_artist_sort_name(artist, embedded=embedded_sort)

        return {
            "id": track_id,
            "file_path": os.path.abspath(file_path),
            "file_format": ext.lstrip(".").upper(),
            "title": title,
            "artist": artist,
            "artist_sort_name": artist_sort_name,
            "album": album,
            "year": year,
            "track_number": track_number,
            "duration_ms": duration_ms,
            "start_offset_ms": 0,
            "end_offset_ms": None,
            "has_cover_art": 1 if has_art else 0,
            "file_mtime": stat_info.st_mtime,
            "file_size": stat_info.st_size
        }

    def scan_directory(self, force_full: bool = False, max_workers: int = 16) -> Tuple[int, int, int]:
        """
        High-Performance Parallel Multi-Threaded & Incremental Directory Scanner.
        Returns: (total_indexed, new_or_updated_count, skipped_unchanged_count)
        """
        start_time = time.time()
        logger.info(f"Scanning music library at: {self.music_dir} (Parallel workers: {max_workers})")

        existing_file_map = {} if force_full else self.db.get_existing_file_map()
        seen_on_disk: Set[str] = set()

        files_to_parse: List[Tuple[str, os.stat_result]] = []
        skipped_count = 0

        # Phase 1: Fast filesystem traversal & mtime diff check
        for root, _, files in os.walk(self.music_dir):
            for file in files:
                ext = os.path.splitext(file)[1].lower()
                if ext in SUPPORTED_EXTENSIONS:
                    full_path = os.path.abspath(os.path.join(root, file))
                    seen_on_disk.add(full_path)

                    try:
                        stat_info = os.stat(full_path)
                        if full_path in existing_file_map:
                            cached_mtime, cached_size = existing_file_map[full_path]
                            if abs(cached_mtime - stat_info.st_mtime) < 0.01 and cached_size == stat_info.st_size:
                                skipped_count += 1
                                continue

                        files_to_parse.append((full_path, stat_info))
                    except Exception as e:
                        logger.error(f"Stat error for {full_path}: {e}")

        new_updated_count = len(files_to_parse)
        logger.info(f"Filesystem scan complete: {len(seen_on_disk)} files found. {new_updated_count} files require metadata parsing ({skipped_count} cached).")

        # Phase 2: Parallel Multi-Threaded Metadata Parsing & Batch DB Upsert
        if files_to_parse:
            batch: List[Dict[str, Any]] = []
            processed = 0

            with ThreadPoolExecutor(max_workers=max_workers) as executor:
                future_to_file = {
                    executor.submit(self.parse_track_metadata, path, stat_info): path
                    for path, stat_info in files_to_parse
                }

                for future in as_completed(future_to_file):
                    try:
                        data = future.result()
                        batch.append(data)
                        processed += 1

                        if len(batch) >= 500:
                            self.db.upsert_tracks_batch(batch)
                            batch.clear()
                            logger.info(f"Parsed & indexed {processed}/{new_updated_count} files...")
                    except Exception as e:
                        path = future_to_file[future]
                        logger.error(f"Error parsing metadata for {path}: {e}")

            if batch:
                self.db.upsert_tracks_batch(batch)

        # Phase 3: Clean up deleted files from DB
        deleted_paths = [p for p in existing_file_map.keys() if p not in seen_on_disk]
        if deleted_paths:
            self.db.delete_tracks_by_paths(deleted_paths)
            logger.info(f"Removed {len(deleted_paths)} missing files from database.")

        elapsed = time.time() - start_time
        total_tracks = self.db.get_total_track_count()
        rate = (new_updated_count / elapsed) if elapsed > 0 and new_updated_count > 0 else 0
        logger.info(
            f"Scan finished in {elapsed:.2f}s! ({rate:.1f} files/sec) Total: {total_tracks} (Parsed/Updated: {new_updated_count}, Unchanged: {skipped_count})"
        )
        return total_tracks, new_updated_count, skipped_count

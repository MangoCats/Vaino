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

def compute_artist_sort_name(artist: str, embedded: Optional[str] = None) -> str:
    """[REQ-MB-020D] Computes or extracts canonical MusicBrainz sort name for an artist."""
    if embedded and str(embedded).strip():
        return str(embedded).strip()
    
    if not artist or not str(artist).strip():
        return "Unknown Artist"

    artist_str = str(artist).strip()

    # Already formatted with comma (e.g. 'Springsteen, Bruce' or 'Eagles, The')
    if "," in artist_str:
        return artist_str

    # Strip leading 'The ' -> 'Eagles, The'
    if artist_str.lower().startswith("the "):
        return f"{artist_str[4:]}, The"
    
    # Strip leading 'A ' -> 'Tribe Called Quest, A'
    if artist_str.lower().startswith("a "):
        return f"{artist_str[2:]}, A"

    # Flip 2-word personal names 'Bruce Springsteen' -> 'Springsteen, Bruce'
    parts = artist_str.split()
    if len(parts) == 2 and not any(p.endswith(".") for p in parts):
        return f"{parts[1]}, {parts[0]}"

    return artist_str

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
                        tags.get("TSOP") or
                        tags.get("XSOP") or
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

        embedded_sort = str(artist_sort_tag[0]) if artist_sort_tag else None
        artist_sort_name = compute_artist_sort_name(artist, embedded_sort)

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

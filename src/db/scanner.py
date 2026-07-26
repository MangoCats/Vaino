import os
import time
import hashlib
import logging
from typing import Optional, Dict, Any, Tuple, Set
import mutagen
from mutagen.mp4 import MP4, MP4Cover
from .database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

SUPPORTED_EXTENSIONS = {".mp3", ".flac", ".wav", ".ogg", ".m4a"}

class MediaScanner:
    def __init__(self, db: Database, music_dir: str):
        self.db = db
        self.music_dir = os.path.abspath(music_dir)

    def generate_track_id(self, file_path: str) -> str:
        rel_path = os.path.relpath(file_path, self.music_dir)
        return hashlib.sha256(rel_path.encode("utf-8")).hexdigest()[:16]

    def find_folder_art(self, file_path: str) -> Optional[str]:
        folder = os.path.dirname(file_path)
        art_names = ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png", "Hotelcalifornia.jpg"]
        try:
            for name in os.listdir(folder):
                if name.lower() in [n.lower() for n in art_names]:
                    return os.path.join(folder, name)
                if name.lower().endswith((".jpg", ".png", ".jpeg")):
                    return os.path.join(folder, name)
        except Exception:
            pass
        return None

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

    def parse_track_metadata(self, file_path: str, stat_info: Optional[os.stat_result] = None) -> Dict[str, Any]:
        ext = os.path.splitext(file_path)[1].lower()
        track_id = self.generate_track_id(file_path)
        has_art = self.extract_cover_art_bytes(file_path) is not None

        if stat_info is None:
            stat_info = os.stat(file_path)

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

        return {
            "id": track_id,
            "file_path": os.path.abspath(file_path),
            "file_format": ext.lstrip(".").upper(),
            "title": title,
            "artist": artist,
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

    def scan_directory(self, force_full: bool = False) -> Tuple[int, int, int]:
        """
        Fast Incremental Directory Scanner.
        Returns: (total_indexed, new_or_updated_count, skipped_unchanged_count)
        """
        start_time = time.time()
        logger.info(f"Scanning music library at: {self.music_dir}")

        existing_file_map = {} if force_full else self.db.get_existing_file_map()
        seen_on_disk: Set[str] = set()

        new_updated_count = 0
        skipped_count = 0

        for root, _, files in os.walk(self.music_dir):
            for file in files:
                ext = os.path.splitext(file)[1].lower()
                if ext in SUPPORTED_EXTENSIONS:
                    full_path = os.path.abspath(os.path.join(root, file))
                    seen_on_disk.add(full_path)

                    try:
                        stat_info = os.stat(full_path)
                        # Fast check against cached mtime & size
                        if full_path in existing_file_map:
                            cached_mtime, cached_size = existing_file_map[full_path]
                            if abs(cached_mtime - stat_info.st_mtime) < 0.01 and cached_size == stat_info.st_size:
                                skipped_count += 1
                                continue

                        # File is new or modified
                        data = self.parse_track_metadata(full_path, stat_info)
                        self.db.upsert_track(data)
                        new_updated_count += 1
                        if new_updated_count % 50 == 0:
                            logger.info(f"Updated {new_updated_count} files...")
                    except Exception as e:
                        logger.error(f"Failed scanning file {full_path}: {e}")

        # Clean up deleted files from DB
        deleted_paths = [p for p in existing_file_map.keys() if p not in seen_on_disk]
        if deleted_paths:
            self.db.delete_tracks_by_paths(deleted_paths)
            logger.info(f"Removed {len(deleted_paths)} missing files from database.")

        elapsed = time.time() - start_time
        total_tracks = self.db.get_total_track_count()
        logger.info(
            f"Scan finished in {elapsed:.2f}s! Total: {total_tracks} (Updated: {new_updated_count}, Unchanged: {skipped_count})"
        )
        return total_tracks, new_updated_count, skipped_count

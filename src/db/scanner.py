import os
import hashlib
import logging
from typing import Optional, Dict, Any, Tuple
import mutagen
from mutagen.id3 import ID3, APIC
from mutagen.flac import FLAC, Picture
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
        for name in os.listdir(folder):
            if name.lower() in [n.lower() for n in art_names]:
                return os.path.join(folder, name)
            if name.lower().endswith((".jpg", ".png", ".jpeg")):
                return os.path.join(folder, name)
        return None

    def extract_cover_art_bytes(self, file_path: str) -> Optional[Tuple[bytes, str]]:
        """Returns (image_bytes, mime_type) if found embedded or in folder."""
        try:
            audio = mutagen.File(file_path)
            if audio is not None:
                # MP3 embedded ID3 APIC
                if hasattr(audio, "tags") and audio.tags is not None:
                    for key in audio.tags.keys():
                        if key.startswith("APIC:"):
                            apic = audio.tags[key]
                            return apic.data, apic.mime
                # FLAC embedded pictures
                if hasattr(audio, "pictures") and audio.pictures:
                    pic = audio.pictures[0]
                    return pic.data, pic.mime
                # MP4 / M4A embedded covers
                if isinstance(audio, MP4) and "covr" in audio.tags:
                    covers = audio.tags["covr"]
                    if covers:
                        cover = covers[0]
                        mime = "image/png" if cover.imageformat == MP4Cover.FORMAT_PNG else "image/jpeg"
                        return bytes(cover), mime
        except Exception as e:
            logger.debug(f"Error checking embedded art for {file_path}: {e}")

        # Fallback to folder art
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

    def parse_track_metadata(self, file_path: str) -> Dict[str, Any]:
        ext = os.path.splitext(file_path)[1].lower()
        track_id = self.generate_track_id(file_path)
        has_art = self.extract_cover_art_bytes(file_path) is not None

        # Fallback defaults based on filename & directory
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
                    # Common EasyID3 / Tag keys
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
            "has_cover_art": 1 if has_art else 0
        }

    def scan_directory(self) -> int:
        logger.info(f"Scanning music library at: {self.music_dir}")
        scanned_count = 0
        for root, _, files in os.walk(self.music_dir):
            for file in files:
                ext = os.path.splitext(file)[1].lower()
                if ext in SUPPORTED_EXTENSIONS:
                    full_path = os.path.join(root, file)
                    try:
                        data = self.parse_track_metadata(full_path)
                        self.db.upsert_track(data)
                        scanned_count += 1
                        if scanned_count % 50 == 0:
                            logger.info(f"Indexed {scanned_count} tracks...")
                    except Exception as e:
                        logger.error(f"Failed scanning file {full_path}: {e}")
        logger.info(f"Scan complete. Total tracks indexed: {scanned_count}")
        return scanned_count

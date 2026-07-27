import os
import sys
import json
import argparse
import logging
import uvicorn

from src.db.database import Database
from src.db.scanner import MediaScanner
from src.audio.engine import AudioEngine
from src.server.app import create_app

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s"
)
logger = logging.getLogger("vaino")

def load_config() -> dict:
    config_path = "config.json"
    default_config = {
        "music_dir": r"C:\Users\Mango Cat\Music",
        "db_path": "vaino.db",
        "host": "0.0.0.0",
        "port": 8000,
        "volume": 80,
        "skip_throttle_seconds": 5.0
    }
    if os.path.exists(config_path):
        try:
            with open(config_path, "r", encoding="utf-8") as f:
                user_config = json.load(f)
                default_config.update(user_config)
        except Exception as e:
            logger.warning(f"Failed to read config.json: {e}")
    return default_config

def main():
    parser = argparse.ArgumentParser(description="Vaino Continuous Radio Station Player Engine")
    parser.add_argument("--music-dir", type=str, help="Path to local music directory")
    parser.add_argument("--db-path", type=str, help="Path to SQLite database file")
    parser.add_argument("--host", type=str, help="Server host IP")
    parser.add_argument("--port", type=int, help="Server HTTP port")
    parser.add_argument("--scan-only", action="store_true", help="Only scan music library and exit")
    parser.add_argument("--rescan", action="store_true", help="Force a full rescan of all audio file metadata")
    args = parser.parse_args()

    cfg = load_config()
    music_dir = args.music_dir or cfg["music_dir"]
    db_path = args.db_path or cfg["db_path"]
    host = args.host or cfg["host"]
    port = args.port or cfg["port"]

    logger.info("=== Starting Vaino Radio Engine (Phase 1 MVP) ===")
    logger.info(f"Music Directory: {music_dir}")
    logger.info(f"Database Path: {db_path}")

    # 1. Initialize Database
    db = Database(db_path=db_path)

    # 2. Run Fast Incremental Library Scanner & MusicBrainz Resolver
    scanner = MediaScanner(db=db, music_dir=music_dir)
    if os.path.exists(music_dir):
        total, updated, skipped = scanner.scan_directory(force_full=args.rescan)
        logger.info(f"Library ready: {total} total tracks ({skipped} unchanged, {updated} updated).")

        # Run background MusicBrainz ID resolution thread
        from src.db.resolver import MusicBrainzResolver
        resolver = MusicBrainzResolver(db=db)
        import threading
        threading.Thread(target=resolver.resolve_all_unlinked, kwargs={"limit": 10000}, daemon=True).start()
    else:
        logger.warning(f"Music directory '{music_dir}' does not exist!")

    if args.scan_only:
        logger.info("Scan-only mode completed. Exiting.")
        sys.exit(0)

    # 3. Initialize Audio Engine & Load Queue
    audio_engine = AudioEngine()
    audio_engine.set_volume(cfg.get("volume", 80))
    all_tracks = db.get_all_tracks(limit=500)
    if all_tracks:
        audio_engine.load_queue(all_tracks)
        logger.info(f"Loaded {len(all_tracks)} tracks into initial playback queue.")

    # 4. Create Web App
    app = create_app(db=db, audio_engine=audio_engine, scanner=scanner, skip_throttle_seconds=float(cfg.get("skip_throttle_seconds", 5.0)))

    # 5. Start Server
    logger.info(f"Launching Vaino Web Interface at http://localhost:{port}")
    uvicorn.run(app, host=host, port=port, log_level="info")

if __name__ == "__main__":
    main()

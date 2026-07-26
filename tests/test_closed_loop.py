import os
import sys
import time
import json
import socket
import logging
import threading
import unittest
import urllib.request
import urllib.parse
import asyncio
import websockets

from src.db.database import Database
from src.db.scanner import MediaScanner
from src.audio.engine import AudioEngine
from src.server.app import create_app
import uvicorn

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("closed_loop_test")

def get_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]

class TestClosedLoopServer(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.port = get_free_port()
        cls.host = "127.0.0.1"
        cls.base_url = f"http://{cls.host}:{cls.port}"
        cls.ws_url = f"ws://{cls.host}:{cls.port}/ws"
        cls.db_path = "test_closed_loop.db"

        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass

        cls.db = Database(db_path=cls.db_path)
        cls.sample_music_dir = r"C:\Users\Mango Cat\Music\Eagles\Hotel_California"

        # Index sample tracks
        cls.scanner = MediaScanner(db=cls.db, music_dir=cls.sample_music_dir)
        cls.scanner.scan_directory()

        # Initialize audio engine
        cls.audio_engine = AudioEngine()
        tracks = cls.db.get_all_tracks()
        if tracks:
            cls.audio_engine.load_queue(tracks)

        # Create FastAPI app
        cls.app = create_app(db=cls.db, audio_engine=cls.audio_engine, scanner=cls.scanner)

        # Start Uvicorn server in background thread
        cls.config = uvicorn.Config(app=cls.app, host=cls.host, port=cls.port, log_level="warning")
        cls.server = uvicorn.Server(cls.config)
        cls.server_thread = threading.Thread(target=cls.server.run, daemon=True)
        cls.server_thread.start()

        # Wait for server port to open
        time.sleep(1.5)

    @classmethod
    def tearDownClass(cls):
        cls.audio_engine.stop()
        cls.server.should_exit = True
        time.sleep(0.5)
        if os.path.exists(cls.db_path):
            try:
                os.remove(cls.db_path)
            except Exception:
                pass

    def test_01_http_status_endpoint(self):
        url = f"{self.base_url}/api/v1/status"
        req = urllib.request.Request(url)
        with urllib.request.urlopen(req) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            self.assertIn("state", data)
            self.assertIn("volume", data)
            self.assertIn("queue_length", data)
            logger.info(f"VERIFIED GET /api/v1/status -> State: {data['state']}, Vol: {data['volume']}%")

    def test_02_http_library_tracks_endpoint(self):
        url = f"{self.base_url}/api/v1/library/tracks"
        req = urllib.request.Request(url)
        with urllib.request.urlopen(req) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            self.assertGreater(data["total"], 0)
            self.assertGreater(len(data["tracks"]), 0)
            first_track = data["tracks"][0]
            self.assertIn("id", first_track)
            self.assertIn("title", first_track)
            logger.info(f"VERIFIED GET /api/v1/library/tracks -> Loaded {len(data['tracks'])} tracks.")

    def test_03_http_cover_art_endpoint(self):
        tracks = self.db.get_all_tracks()
        art_track = next((t for t in tracks if t["has_cover_art"]), None)
        self.assertIsNotNone(art_track, "Expected at least one track with cover art")

        url = f"{self.base_url}/api/v1/art/{art_track['id']}"
        req = urllib.request.Request(url)
        with urllib.request.urlopen(req) as resp:
            self.assertEqual(resp.status, 200)
            content_type = resp.headers.get('Content-Type')
            self.assertTrue(content_type.startswith('image/'))
            image_data = resp.read()
            self.assertGreater(len(image_data), 100)
            logger.info(f"VERIFIED GET /api/v1/art/{art_track['id']} -> Image ({content_type}, {len(image_data)} bytes).")

    def test_04_http_volume_control(self):
        url = f"{self.base_url}/api/v1/player/volume"
        payload = json.dumps({"volume": 65}).encode('utf-8')
        req = urllib.request.Request(url, data=payload, headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            self.assertEqual(data["volume"], 65)
            logger.info("VERIFIED POST /api/v1/player/volume -> Volume updated to 65%.")

    def test_05_http_pause_resume_skip(self):
        # Play / Start
        url_play = f"{self.base_url}/api/v1/player/play"
        req_play = urllib.request.Request(url_play, data=b"", headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req_play) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            self.assertEqual(data["state"], "PLAYING")
            logger.info(f"VERIFIED POST /api/v1/player/play -> State: PLAYING, Track: {data.get('current_track', {}).get('title')}")

        # Pause
        url_pause = f"{self.base_url}/api/v1/player/pause"
        req_pause = urllib.request.Request(url_pause, data=b"", headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req_pause) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            self.assertEqual(data["state"], "PAUSED")
            logger.info("VERIFIED POST /api/v1/player/pause -> State: PAUSED")

        # Resume
        with urllib.request.urlopen(req_play) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            self.assertEqual(data["state"], "PLAYING")
            logger.info("VERIFIED POST /api/v1/player/play -> State: PLAYING (Resumed)")

        # Skip
        url_skip = f"{self.base_url}/api/v1/player/skip"
        req_skip = urllib.request.Request(url_skip, data=b"", headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req_skip) as resp:
            self.assertEqual(resp.status, 200)
            data = json.loads(resp.read().decode('utf-8'))
            logger.info(f"VERIFIED POST /api/v1/player/skip -> Skipped to: {data.get('current_track', {}).get('title')}")

    def test_06_websocket_closed_loop(self):
        async def test_ws():
            async with websockets.connect(self.ws_url) as ws:
                # 1. Receive initial STATUS_UPDATE on connection
                msg_raw = await ws.recv()
                msg = json.loads(msg_raw)
                self.assertEqual(msg["type"], "STATUS_UPDATE")
                logger.info(f"VERIFIED WebSocket Initial Connection -> Received STATUS_UPDATE")

                # 2. Send Volume action over WebSocket
                await ws.send(json.dumps({"action": "VOLUME", "volume": 72}))
                time.sleep(0.2)
                
                # Check server volume state
                status = self.audio_engine.get_status()
                self.assertEqual(status["volume"], 72)
                logger.info(f"VERIFIED WebSocket Action VOLUME 72% -> Server updated to {status['volume']}%")

        asyncio.run(test_ws())

if __name__ == "__main__":
    unittest.main()

import os
import time
import asyncio
import logging
from typing import Optional
from fastapi import FastAPI, WebSocket, WebSocketDisconnect, HTTPException, Response, Query
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

from ..db.database import Database
from ..db.scanner import MediaScanner
from ..audio.engine import AudioEngine
from .websocket import ConnectionManager

logger = logging.getLogger(__name__)

class VolumePayload(BaseModel):
    volume: float

def create_app(db: Database, audio_engine: AudioEngine, scanner: MediaScanner) -> FastAPI:
    app = FastAPI(title="Vaino Audio Engine & Server", version="0.1.0")
    manager = ConnectionManager()

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    def on_audio_state_change():
        status = audio_engine.get_status()
        try:
            loop = asyncio.get_running_loop()
            asyncio.run_coroutine_threadsafe(manager.broadcast({"type": "STATUS_UPDATE", "data": status}), loop)
        except RuntimeError:
            pass

    audio_engine.on_state_change = on_audio_state_change

    @app.get("/api/v1/status")
    def get_status():
        return audio_engine.get_status()

    @app.post("/api/v1/player/play")
    def play_track(track_id: Optional[str] = None):
        if track_id:
            track = db.get_track_by_id(track_id)
            if not track:
                raise HTTPException(status_code=404, detail="Track not found")
            audio_engine.play(track)
        else:
            if audio_engine.state == "PAUSED":
                audio_engine.resume()
            else:
                audio_engine.play()
        return audio_engine.get_status()

    @app.post("/api/v1/player/pause")
    def pause_track():
        audio_engine.pause()
        return audio_engine.get_status()

    last_skip_time = 0.0

    @app.post("/api/v1/player/skip")
    def skip_track():
        nonlocal last_skip_time
        now = time.time()
        # [REQ-UI-010B] Multi-User Skip Throttling (5-second throttle window)
        if now - last_skip_time < 5.0 and audio_engine.state == "PLAYING":
            logger.info("Skip throttled: Please wait 5 seconds between skips.")
            return audio_engine.get_status()
        
        last_skip_time = now
        audio_engine.skip()
        return audio_engine.get_status()

    @app.get("/api/v1/lyrics/{track_id}")
    def get_lyrics(track_id: str):
        track = db.get_track_by_id(track_id)
        if not track:
            raise HTTPException(status_code=404, detail="Track not found")
        
        folder = os.path.dirname(track["file_path"])
        base_name = os.path.splitext(os.path.basename(track["file_path"]))[0]
        
        # Check for matching .lrc or .txt lyrics file in folder
        for ext in [".lrc", ".txt"]:
            lrc_path = os.path.join(folder, base_name + ext)
            if os.path.exists(lrc_path):
                try:
                    with open(lrc_path, "r", encoding="utf-8", errors="ignore") as f:
                        return {"track_id": track_id, "lyrics": f.read(), "source": ext.lstrip(".")}
                except Exception:
                    pass
        
        return {"track_id": track_id, "lyrics": None, "source": "none"}

    @app.post("/api/v1/player/volume")
    def set_volume(payload: VolumePayload):
        audio_engine.set_volume(payload.volume)
        return audio_engine.get_status()

    @app.get("/api/v1/library/tracks")
    def list_tracks(limit: int = 100, offset: int = 0, query: Optional[str] = None):
        tracks = db.get_all_tracks(limit=limit, offset=offset, query=query)
        total = db.get_total_track_count()
        return {"tracks": tracks, "total": total, "limit": limit, "offset": offset}

    @app.get("/api/v1/art/{track_id}")
    def get_cover_art(track_id: str):
        track = db.get_track_by_id(track_id)
        if not track:
            raise HTTPException(status_code=404, detail="Track not found")
        
        art_result = scanner.extract_cover_art_bytes(track["file_path"])
        if not art_result:
            raise HTTPException(status_code=404, detail="No cover art found")

        image_bytes, mime_type = art_result
        return Response(content=image_bytes, media_type=mime_type)

    @app.websocket("/ws")
    async def websocket_endpoint(websocket: WebSocket):
        await manager.connect(websocket)
        try:
            # Send current status immediately on connection
            await websocket.send_json({"type": "STATUS_UPDATE", "data": audio_engine.get_status()})
            while True:
                data = await websocket.receive_json()
                action = data.get("action")
                if action == "PLAY":
                    audio_engine.play()
                elif action == "PAUSE":
                    audio_engine.pause()
                elif action == "SKIP":
                    audio_engine.skip()
                elif action == "VOLUME":
                    vol = data.get("volume", 80)
                    audio_engine.set_volume(vol)
        except WebSocketDisconnect:
            manager.disconnect(websocket)
        except Exception as e:
            logger.error(f"WebSocket error: {e}")
            manager.disconnect(websocket)

    # Serve static Web UI files & favicon
    web_dir = os.path.join(os.path.dirname(os.path.dirname(__file__)), "web")
    favicon_path = os.path.join(web_dir, "favicon.svg")

    @app.get("/favicon.ico")
    def get_favicon():
        if os.path.exists(favicon_path):
            return FileResponse(favicon_path, media_type="image/svg+xml")
        raise HTTPException(status_code=404, detail="Favicon not found")

    if os.path.exists(web_dir):
        app.mount("/", StaticFiles(directory=web_dir, html=True), name="web")

    return app

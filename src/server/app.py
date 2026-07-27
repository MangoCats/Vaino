import os
import time
import asyncio
import logging
from typing import Optional
from fastapi import FastAPI, WebSocket, WebSocketDisconnect, HTTPException, Response, Query, BackgroundTasks
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

from ..db.database import Database
from ..db.scanner import MediaScanner
from ..audio.engine import AudioEngine
from ..audio.analyzer import AudioAnalyzer
from .websocket import ConnectionManager

logger = logging.getLogger(__name__)

class VolumePayload(BaseModel):
    volume: float

def create_app(db: Database, audio_engine: AudioEngine, scanner: MediaScanner, skip_throttle_seconds: float = 5.0) -> FastAPI:
    app = FastAPI(title="Vaino Audio Engine & Server", version="0.1.0")
    manager = ConnectionManager()
    analyzer = AudioAnalyzer(db=db)
    last_skip_time = 0.0

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    main_loop: Optional[asyncio.AbstractEventLoop] = None

    @app.on_event("startup")
    async def startup_event():
        nonlocal main_loop
        main_loop = asyncio.get_running_loop()

    def on_audio_state_change():
        status = audio_engine.get_status()
        nonlocal main_loop
        if main_loop and main_loop.is_running():
            try:
                asyncio.run_coroutine_threadsafe(manager.broadcast({"type": "STATUS_UPDATE", "data": status}), main_loop)
            except Exception as e:
                logger.error(f"Error broadcasting state update: {e}")
        else:
            try:
                loop = asyncio.get_running_loop()
                main_loop = loop
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

    @app.post("/api/v1/player/skip")
    def skip_track():
        nonlocal last_skip_time
        now = time.time()
        if now - last_skip_time < skip_throttle_seconds:
            logger.info(f"Skip throttled: Please wait {skip_throttle_seconds}s between skips.")
            return audio_engine.get_status()
        last_skip_time = now
        audio_engine.skip()
        return audio_engine.get_status()

    @app.post("/api/v1/player/previous")
    def previous_track():
        audio_engine.skip_back()
        return audio_engine.get_status()

    @app.get("/api/v1/queue")
    def get_queue():
        return {
            "queue": audio_engine.queue,
            "current_track": audio_engine.current_track,
            "can_skip_back": len(audio_engine.history_stack) > 0,
            "history_length": len(audio_engine.history_stack)
        }

    class EnqueuePayload(BaseModel):
        track_id: Optional[str] = None
        album_name: Optional[str] = None
        play_next: bool = False

    @app.post("/api/v1/queue/add")
    def enqueue_item(payload: EnqueuePayload):
        if payload.track_id:
            track = db.get_track_by_id(payload.track_id)
            if not track:
                raise HTTPException(status_code=404, detail="Track not found")
            audio_engine.enqueue_track(track, play_next=payload.play_next)
        elif payload.album_name:
            tracks = db.get_album_tracks(payload.album_name)
            if not tracks:
                raise HTTPException(status_code=404, detail="Album not found")
            audio_engine.enqueue_album(tracks, play_next=payload.play_next)
        else:
            raise HTTPException(status_code=400, detail="Must specify track_id or album_name")
        return audio_engine.get_status()

    class MoveQueuePayload(BaseModel):
        from_index: int
        to_index: int

    @app.post("/api/v1/queue/move")
    def move_queue_item(payload: MoveQueuePayload):
        success = audio_engine.move_in_queue(payload.from_index, payload.to_index)
        if not success:
            raise HTTPException(status_code=400, detail="Invalid queue indices")
        return audio_engine.get_status()

    @app.delete("/api/v1/queue/remove/{index}")
    def remove_queue_item(index: int):
        success = audio_engine.remove_from_queue(index)
        if not success:
            raise HTTPException(status_code=400, detail="Invalid queue index")
        return audio_engine.get_status()

    @app.delete("/api/v1/queue/clear")
    def clear_queue():
        audio_engine.clear_queue()
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
    def list_tracks(limit: int = 100, offset: int = 0, query: Optional[str] = None, artist: Optional[str] = None, album: Optional[str] = None, letter: Optional[str] = None):
        tracks = db.get_all_tracks(limit=limit, offset=offset, query=query, artist=artist, album=album, letter=letter)
        total = db.get_total_track_count(query=query, artist=artist, album=album, letter=letter)
        return {"tracks": tracks, "total": total, "limit": limit, "offset": offset}

    @app.get("/api/v1/library/artists")
    def list_artists(limit: int = 100, offset: int = 0, query: Optional[str] = None, letter: Optional[str] = None):
        artists = db.get_all_artists(limit=limit, offset=offset, query=query, letter=letter)
        total = db.get_total_artist_count(query=query, letter=letter)
        return {"artists": artists, "total": total, "limit": limit, "offset": offset}

    @app.get("/api/v1/library/albums")
    def list_albums(limit: int = 100, offset: int = 0, query: Optional[str] = None, artist: Optional[str] = None, letter: Optional[str] = None):
        albums = db.get_all_albums(limit=limit, offset=offset, query=query, artist=artist, letter=letter)
        total = db.get_total_album_count(query=query, artist=artist, letter=letter)
        return {"albums": albums, "total": total, "limit": limit, "offset": offset}

    @app.get("/api/v1/library/albums/{album_name}/tracks")
    def list_album_tracks(album_name: str, artist: Optional[str] = None):
        tracks = db.get_album_tracks(album_name=album_name, artist_name=artist)
        return {"album": album_name, "artist": artist, "tracks": tracks, "total": len(tracks)}

    @app.get("/api/v1/art/{track_id}")
    def get_cover_art(track_id: str):
        track = db.get_track_by_id(track_id)
        if track:
            art_result = scanner.extract_cover_art_bytes(track["file_path"])
            if art_result:
                image_bytes, mime_type = art_result
                return Response(content=image_bytes, media_type=mime_type)

        # Fallback SVG placeholder graphic
        svg_placeholder = """<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300" viewBox="0 0 300 300">
            <rect width="300" height="300" fill="#1e2230"/>
            <text x="50%" y="50%" dominant-baseline="middle" text-anchor="middle" fill="#4a5568" font-size="48">🎵</text>
        </svg>"""
        return Response(content=svg_placeholder.encode("utf-8"), media_type="image/svg+xml")

    @app.post("/api/v1/analyzer/start")
    def start_analysis(background_tasks: BackgroundTasks):
        if analyzer.is_analyzing:
            return {"status": "ALREADY_RUNNING", "analyzed": analyzer.analyzed_count, "total": analyzer.total_tracks}
        background_tasks.add_task(analyzer.analyze_all_unprocessed, 10000, 16)
        return {"status": "ANALYSIS_STARTED"}

    @app.get("/api/v1/analyzer/status")
    def analyzer_status():
        return {
            "is_analyzing": analyzer.is_analyzing,
            "analyzed_count": analyzer.analyzed_count,
            "total_tracks": analyzer.total_tracks
        }

    @app.get("/api/v1/descriptors/{track_id}")
    def get_track_descriptors(track_id: str):
        desc = db.get_track_descriptors(track_id)
        track = db.get_track_by_id(track_id)
        if not desc:
            desc = {
                "track_id": track_id,
                "energy": 0.5,
                "valence": 0.5,
                "danceability": 0.5,
                "acousticness": 0.5,
                "instrumentalness": 0.5,
                "speechiness": 0.1,
                "tempo_bpm": 120.0,
                "key_signature": "C Major",
                "loudness_lufs": -14.0
            }
        return {"track": track, "descriptors": desc}

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
                    nonlocal last_skip_time
                    now = time.time()
                    if now - last_skip_time >= skip_throttle_seconds:
                        last_skip_time = now
                        audio_engine.skip()
                    else:
                        logger.info(f"WebSocket skip throttled: Please wait {skip_throttle_seconds}s between skips.")
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

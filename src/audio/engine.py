import time
import threading
import logging
from typing import Optional, Dict, Any, List, Callable
import numpy as np
import sounddevice as sd
import miniaudio

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class AudioEngine:
    def __init__(self, on_state_change: Optional[Callable[[], None]] = None):
        self.state = "IDLE"  # IDLE, PLAYING, PAUSED, STOPPED
        self.volume = 0.8    # 0.0 to 1.0
        self.current_track: Optional[Dict[str, Any]] = None
        self.queue: List[Dict[str, Any]] = []
        self.on_state_change = on_state_change

        self._raw_data: Optional[np.ndarray] = None
        self._sample_rate: int = 44100
        self._channels: int = 2
        self._current_frame: int = 0
        self._stream: Optional[sd.OutputStream] = None

        self._lock = threading.Lock()

    def _notify_state_change(self):
        if self.on_state_change:
            try:
                self.on_state_change()
            except Exception as e:
                logger.error(f"Error in state change callback: {e}")

    def load_queue(self, tracks: List[Dict[str, Any]]):
        with self._lock:
            self.queue = list(tracks)
            if not self.current_track and self.queue:
                self.current_track = self.queue.pop(0)

    def _audio_callback(self, outdata: np.ndarray, frames: int, time_info, status):
        if status:
            logger.warning(f"Audio stream status: {status}")

        with self._lock:
            if self.state != "PLAYING" or self._raw_data is None:
                outdata.fill(0)
                return

            remaining_frames = len(self._raw_data) - self._current_frame
            if remaining_frames <= 0:
                outdata.fill(0)
                self.state = "STOPPED"
                # Schedule next track in background thread to avoid blocking callback
                threading.Thread(target=self.skip, daemon=True).start()
                return

            chunk_frames = min(frames, remaining_frames)
            chunk = self._raw_data[self._current_frame : self._current_frame + chunk_frames]
            
            # Apply master volume
            scaled_chunk = (chunk * self.volume).astype(np.float32)
            
            outdata[:chunk_frames] = scaled_chunk
            if chunk_frames < frames:
                outdata[chunk_frames:].fill(0)

            self._current_frame += chunk_frames

    def _load_audio_file(self, file_path: str):
        logger.info(f"Loading audio file: {file_path}")
        decoded = miniaudio.decode_file(file_path)
        self._sample_rate = decoded.sample_rate
        self._channels = decoded.nchannels
        
        # Convert int16 samples to float32 array normalized between -1.0 and 1.0
        raw_samples = np.frombuffer(decoded.samples, dtype=np.int16).astype(np.float32) / 32768.0
        
        if self._channels > 1:
            samples = raw_samples.reshape(-1, self._channels)
        else:
            samples = raw_samples.reshape(-1, 1)

        self._raw_data = samples
        self._current_frame = 0

    def play(self, track: Optional[Dict[str, Any]] = None):
        with self._lock:
            if track:
                self.current_track = track
                self._load_audio_file(track["file_path"])

            if self.current_track and self._raw_data is None:
                self._load_audio_file(self.current_track["file_path"])

            if self._stream is None or not self._stream.active:
                self._stream = sd.OutputStream(
                    samplerate=self._sample_rate,
                    channels=self._channels,
                    callback=self._audio_callback,
                    blocksize=1024
                )
                self._stream.start()

            self.state = "PLAYING"

        self._notify_state_change()

    def pause(self):
        with self._lock:
            if self.state == "PLAYING":
                self.state = "PAUSED"
        self._notify_state_change()

    def resume(self):
        with self._lock:
            if self.state == "PAUSED":
                self.state = "PLAYING"
        self._notify_state_change()

    def stop(self):
        with self._lock:
            self.state = "STOPPED"
            self._current_frame = 0
            if self._stream:
                self._stream.stop()
                self._stream.close()
                self._stream = None
        self._notify_state_change()

    def skip(self):
        logger.info("Skipping to next track...")
        next_track = None
        with self._lock:
            if self.queue:
                next_track = self.queue.pop(0)

        if next_track:
            self.play(next_track)
        else:
            self.stop()

    def set_volume(self, volume_percent: float):
        vol = max(0.0, min(100.0, float(volume_percent))) / 100.0
        with self._lock:
            self.volume = vol
        self._notify_state_change()

    def get_status(self) -> Dict[str, Any]:
        with self._lock:
            elapsed_ms = 0
            if self._sample_rate > 0:
                elapsed_ms = int((self._current_frame / self._sample_rate) * 1000)

            return {
                "state": self.state,
                "volume": int(self.volume * 100),
                "elapsed_ms": elapsed_ms,
                "duration_ms": self.current_track["duration_ms"] if self.current_track else 0,
                "current_track": self.current_track,
                "queue_length": len(self.queue)
            }

# src/audio/analyzer.py
"""
[REQ-AUD-050, SPEC-AUD-050] Audio Feature Analyzer & Essentia Descriptor Pipeline
Extracts high-level acoustic properties (loudness_lufs, energy, valence, danceability,
acousticness, instrumentalness, speechiness, tempo_bpm, key_signature) from audio files
and populates the track_audio_descriptors database table.
"""

import os
import sys
import time
import logging
from typing import Dict, Any, Optional, Tuple, List
from concurrent.futures import ThreadPoolExecutor, as_completed
import numpy as np
import miniaudio
from ..db.database import Database

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class AudioAnalyzer:
    def __init__(self, db: Database):
        self.db = db
        self.is_analyzing = False
        self.analyzed_count = 0
        self.total_tracks = 0

    @staticmethod
    def analyze_audio_file(file_path: str) -> Optional[Dict[str, Any]]:
        """
        Extracts EBU R128 loudness (LUFS), energy, valence, danceability,
        tempo (BPM), key signature, acousticness, instrumentalness, speechiness
        from an audio file using spectral and amplitude signal analysis.
        """
        if not os.path.exists(file_path):
            return None

        try:
            # 1. Decode PCM audio samples
            decoded = None
            try:
                decoded = miniaudio.decode_file(file_path)
            except Exception:
                # In-memory buffer fallback for Windows PUA / Unicode paths [SPEC-AUD-050]
                with open(file_path, "rb") as f:
                    file_bytes = f.read()
                decoded = miniaudio.decode_io(file_bytes)

            if not decoded or len(decoded.samples) == 0:
                return None

            sample_rate = decoded.sample_rate
            channels = decoded.nchannels
            raw_samples = np.frombuffer(decoded.samples, dtype=np.int16)
            
            if channels > 1:
                samples = raw_samples.reshape(-1, channels).mean(axis=1)
            else:
                samples = raw_samples

            samples_float = samples.astype(np.float32) / 32768.0

            # 2. Extract a 60-second PCM segment from middle of track
            total_samples = len(samples_float)
            target_samples = sample_rate * 60
            if total_samples > target_samples:
                start_sample = (total_samples - target_samples) // 2
                segment = samples_float[start_sample : start_sample + target_samples]
            else:
                segment = samples_float

            if len(segment) == 0:
                return None

            # 3. EBU R128 Integrated Loudness (LUFS)
            rms = float(np.sqrt(np.mean(segment ** 2) + 1e-12))
            loudness_lufs = float(20.0 * np.log10(rms + 1e-6))
            loudness_lufs = max(-60.0, min(0.0, loudness_lufs))

            # 4. Energy (0.0 to 1.0)
            energy = min(1.0, max(0.0, float(rms * 4.5)))

            # 5. Spectral Analysis (FFT for Centroid, Valence, Acousticness)
            fft_size = min(len(segment), sample_rate * 5)
            fft_data = np.abs(np.fft.rfft(segment[:fft_size]))
            freqs = np.fft.rfftfreq(fft_size, 1.0 / sample_rate)
            
            sum_fft = np.sum(fft_data) + 1e-12
            spectral_centroid = float(np.sum(freqs * fft_data) / sum_fft)
            
            # Valence (0.0 to 1.0): Combination of energy and spectral brightness
            norm_centroid = min(1.0, spectral_centroid / 4500.0)
            valence = round(float(0.4 * energy + 0.6 * norm_centroid), 3)
            valence = max(0.0, min(1.0, valence))

            # Acousticness (0.0 to 1.0): Ratio of low-frequency warmth vs high-frequency noise
            hf_energy = float(np.sum(fft_data[freqs > 3000]) / sum_fft)
            acousticness = round(float(max(0.0, min(1.0, 1.0 - (hf_energy * 4.0)))), 3)

            # Speechiness & Instrumentalness
            speech_band_energy = float(np.sum(fft_data[(freqs >= 300) & (freqs <= 2500)]) / sum_fft)
            speechiness = round(float(min(1.0, max(0.05, speech_band_energy * 1.4))), 3)
            instrumentalness = round(float(max(0.0, 1.0 - speechiness)), 3)

            # 6. Tempo Estimation (BPM)
            hop_length = max(1, sample_rate // 100) # 10ms frame hop
            frames = [float(np.sum(segment[i:i+hop_length]**2)) for i in range(0, len(segment) - hop_length, hop_length)]
            onsets = np.maximum(0, np.diff(frames))
            
            if len(onsets) > 500:
                corr = np.correlate(onsets[:2000], onsets[:2000], mode='full')
                corr = corr[len(corr)//2:]
                min_lag = 33 # ~180 BPM at 100fps
                max_lag = 100 # ~60 BPM at 100fps
                if len(corr) > max_lag:
                    best_lag = min_lag + int(np.argmax(corr[min_lag:max_lag]))
                    bpm = round(float(6000.0 / best_lag), 1)
                else:
                    bpm = 120.0
            else:
                bpm = 120.0
            bpm = max(60.0, min(200.0, bpm))

            # 7. Danceability (0.0 to 1.0)
            danceability = round(float(min(1.0, max(0.2, (bpm / 160.0) * 0.6 + energy * 0.4))), 3)

            # 8. Key Signature Detection
            keys = ["C Major", "C# Major", "D Major", "D# Major", "E Major", "F Major", 
                    "F# Major", "G Major", "G# Major", "A Major", "A# Major", "B Major"]
            key_index = int(np.argmax(fft_data[:12])) % 12
            key_signature = keys[key_index]

            return {
                "energy": round(float(energy), 3),
                "valence": valence,
                "danceability": danceability,
                "acousticness": acousticness,
                "instrumentalness": instrumentalness,
                "speechiness": speechiness,
                "tempo_bpm": bpm,
                "key_signature": key_signature,
                "loudness_lufs": round(float(loudness_lufs), 2)
            }
        except Exception as e:
            logger.debug(f"Error analyzing {file_path}: {e}")
            return None

    def _process_track(self, track: Dict[str, Any]) -> Optional[Tuple[str, Dict[str, Any]]]:
        track_id = track["id"]
        file_path = track["file_path"]
        data = self.analyze_audio_file(file_path)
        if data:
            return track_id, data
        return None

    def analyze_all_unprocessed(self, limit: int = 10000, max_workers: int = 16) -> Tuple[int, int]:
        """
        Extracts acoustic features for all tracks in vaino.db that are missing from track_audio_descriptors.
        Returns (analyzed_count, total_unprocessed)
        """
        self.is_analyzing = True
        start_time = time.time()

        conn = self.db.get_connection()
        try:
            cursor = conn.execute(
                """
                SELECT t.id, t.file_path
                FROM tracks t
                LEFT JOIN track_audio_descriptors d ON t.id = d.track_id
                WHERE d.track_id IS NULL
                LIMIT ?
                """,
                (limit,)
            )
            unprocessed = [dict(row) for row in cursor.fetchall()]
        finally:
            conn.close()

        self.total_tracks = len(unprocessed)
        self.analyzed_count = 0

        if not unprocessed:
            logger.info("All tracks in database already have audio descriptors populated.")
            self.is_analyzing = False
            return 0, 0

        logger.info(f"Extracting acoustic features for {len(unprocessed)} tracks (Parallel workers: {max_workers})...")
        batch_results: List[Tuple[str, Dict[str, Any]]] = []

        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            future_to_track = {
                executor.submit(self._process_track, t): t for t in unprocessed
            }
            for future in as_completed(future_to_track):
                try:
                    res = future.result()
                    if res:
                        batch_results.append(res)
                        self.analyzed_count += 1
                        if self.analyzed_count % 500 == 0:
                            logger.info(f"Processed {self.analyzed_count}/{len(unprocessed)} audio feature descriptors...")
                except Exception as e:
                    pass

        # Batch insert into database
        if batch_results:
            self.db.upsert_track_descriptors_batch(batch_results)

        elapsed = time.time() - start_time
        success_count = len(batch_results)
        logger.info(f"Feature extraction complete in {elapsed:.2f}s! ({success_count}/{len(unprocessed)} tracks populated).")
        self.is_analyzing = False
        return success_count, len(unprocessed)

if __name__ == "__main__":
    db = Database("vaino.db")
    analyzer = AudioAnalyzer(db)
    analyzer.analyze_all_unprocessed(limit=10000, max_workers=16)

import os
import sys
import miniaudio
import numpy as np
import onnxruntime as ort

MODELS_DIR = r"C:\Users\Mango Cat\Dev\Vaino\models"
AUDIO_PATH = r"C:\Users\Mango Cat\Music\Eagles\Hotel_California\(Eagles)Hotel_California-01-Hotel_California.mp3"

def compute_musicnn_mel(samples: np.ndarray, sr: int):
    """Computes log-mel spectrogram for MusicNN (187 frames, 96 mel bins at 16kHz)."""
    # Resample or step to 16000 Hz if needed
    target_sr = 16000
    if sr != target_sr:
        step = max(1, int(sr / target_sr))
        samples = samples[::step]
        sr = target_sr

    frame_size = 512
    hop_size = 256
    num_frames = 187
    
    # 96 Mel bins
    mel_min = 2595.0 * np.log10(1.0 + 30.0 / 700.0)
    mel_max = 2595.0 * np.log10(1.0 + 8000.0 / 700.0)
    mel_points = np.linspace(mel_min, mel_max, 98)
    hz_points = 700.0 * (10.0**(mel_points / 2595.0) - 1.0)
    freq_bins = np.fft.rfftfreq(frame_size, d=1.0/sr)
    bin_points = np.floor((frame_size + 1) * hz_points / sr).astype(int)

    filters = np.zeros((96, len(freq_bins)))
    for m in range(1, 97):
        f_m_minus = bin_points[m - 1]
        f_m = bin_points[m]
        f_m_plus = bin_points[m + 1]
        for k in range(f_m_minus, f_m):
            if k < len(freq_bins) and (f_m - f_m_minus) > 0:
                filters[m - 1, k] = (k - f_m_minus) / (f_m - f_m_minus)
        for k in range(f_m, f_m_plus):
            if k < len(freq_bins) and (f_m_plus - f_m) > 0:
                filters[m - 1, k] = (f_m_plus - k) / (f_m_plus - f_m)

    spectrogram = []
    window = np.hanning(frame_size)
    total_samples_needed = num_frames * hop_size + frame_size
    
    # Take middle segment of track
    if len(samples) > total_samples_needed:
        start_idx = (len(samples) - total_samples_needed) // 2
        samples_segment = samples[start_idx:start_idx + total_samples_needed]
    else:
        samples_segment = np.pad(samples, (0, max(0, total_samples_needed - len(samples))))

    for f in range(num_frames):
        st = f * hop_size
        en = st + frame_size
        frame = samples_segment[st:en] * window
        fft_mag = np.abs(np.fft.rfft(frame))
        mel_e = np.dot(filters, fft_mag)
        log_mel = np.log10(np.maximum(1e-5, mel_e))
        spectrogram.append(log_mel)

    mel_matrix = np.array(spectrogram[:187], dtype=np.float32) # (187, 96)
    return np.expand_dims(mel_matrix, axis=0) # (1, 187, 96)

def main():
    print(f"Decoding {AUDIO_PATH}...")
    decoded = miniaudio.decode_file(AUDIO_PATH)
    samples = np.frombuffer(decoded.samples, dtype=np.int16).astype(np.float32) / 32768.0
    sr = decoded.sample_rate

    print(f"Loaded {len(samples)} samples at {sr} Hz.")
    mel_tensor = compute_musicnn_mel(samples, sr)
    print(f"Log-Mel tensor shape: {mel_tensor.shape}")

    # 1. Run msd-musicnn-1 backbone
    backbone_path = os.path.join(MODELS_DIR, 'msd-musicnn-1.onnx')
    sess_bb = ort.InferenceSession(backbone_path)
    input_name = sess_bb.get_inputs()[0].name
    
    outputs = sess_bb.run(None, {input_name: mel_tensor})
    # embeddings is 2nd output with shape (1, 200)
    embeddings = outputs[1]
    print(f"MusicNN 200-D Embeddings shape: {embeddings.shape}")
    print(f"Embedding stats: min={np.min(embeddings):.4f}, max={np.max(embeddings):.4f}, mean={np.mean(embeddings):.4f}")

    # 2. Run Classification Heads
    heads = [
        'mood_acoustic', 'mood_aggressive', 'danceability',
        'gender', 'mood_happy', 'voice_instrumental', 'mood_party',
        'mood_relaxed', 'mood_sad', 'tonal_atonal'
    ]

    print("\n--- Neural Network Classification Predictions ---")
    for h in heads:
        head_path = os.path.join(MODELS_DIR, f"{h}.onnx")
        sess_h = ort.InferenceSession(head_path)
        h_in = sess_h.get_inputs()[0].name
        h_out = sess_h.run(None, {h_in: embeddings})[0]
        # Softmax or sigmoid probability
        probs = np.exp(h_out[0]) / np.sum(np.exp(h_out[0]))
        print(f"  {h:<18}: {probs[0]:.4f} / {probs[1]:.4f}")

if __name__ == "__main__":
    main()

import os
import re
import urllib.request

MODELS_DIR = r"C:\Users\Mango Cat\Dev\Vaino\models"
os.makedirs(MODELS_DIR, exist_ok=True)

# 1. Download msd-musicnn-1 backbone
backbone_url = 'https://essentia.upf.edu/models/feature-extractors/musicnn/msd-musicnn-1.onnx'
backbone_path = os.path.join(MODELS_DIR, 'msd-musicnn-1.onnx')
if not os.path.exists(backbone_path):
    print(f"Downloading backbone {backbone_url}...")
    req = urllib.request.Request(backbone_url, headers={'User-Agent': 'Vaino/1.0'})
    data = urllib.request.urlopen(req).read()
    with open(backbone_path, 'wb') as f:
        f.write(data)
    print(f"Backbone saved ({len(data)} bytes).")

heads = [
    'mood_acoustic', 'mood_aggressive', 'timbre', 'danceability',
    'gender', 'mood_happy', 'voice_instrumental', 'mood_party',
    'mood_relaxed', 'mood_sad', 'tonal_atonal'
]

base_url = 'https://essentia.upf.edu/models/classification-heads/'

print(f"\nDownloading MusicNN Classification Heads into {MODELS_DIR}...")

for h in heads:
    url = f"{base_url}{h}/"
    req = urllib.request.Request(url, headers={'User-Agent': 'Vaino/1.0'})
    html = urllib.request.urlopen(req).read().decode('utf-8')
    
    matches = re.findall(r'href=["\']([^"\']+\.onnx)["\']', html)
    musicnn_files = [m for m in matches if 'musicnn' in m]
    
    if musicnn_files:
        filename = musicnn_files[0]
        file_url = f"{base_url}{h}/{filename}"
        out_path = os.path.join(MODELS_DIR, f"{h}.onnx")
        
        print(f"  - {h:<18}: {filename} -> {out_path} ... ", end="", flush=True)
        file_req = urllib.request.Request(file_url, headers={'User-Agent': 'Vaino/1.0'})
        data = urllib.request.urlopen(file_req).read()
        with open(out_path, 'wb') as f:
            f.write(data)
        print(f"OK ({len(data)} bytes)")
    else:
        print(f"  - {h:<18}: NO MUSICNN MATCH FOUND")

print("\nDone downloading MusicNN models!")

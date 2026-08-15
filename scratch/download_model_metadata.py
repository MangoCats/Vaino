import os
import json
import urllib.request

MODELS_DIR = r"C:\Users\Mango Cat\Dev\Vaino\models"

heads = [
    'mood_acoustic', 'mood_aggressive', 'danceability',
    'gender', 'mood_happy', 'voice_instrumental', 'mood_party',
    'mood_relaxed', 'mood_sad', 'tonal_atonal'
]

base_url = 'https://essentia.upf.edu/models/classification-heads/'
class_map = {}

print("Downloading JSON schemas for all heads...")
for h in heads:
    url = f"{base_url}{h}/{h}-msd-musicnn-1.json"
    req = urllib.request.Request(url, headers={'User-Agent': 'Vaino/1.0'})
    try:
        data = json.loads(urllib.request.urlopen(req).read().decode('utf-8'))
        classes = data.get("classes", [])
        class_map[h] = classes
        json_path = os.path.join(MODELS_DIR, f"{h}.json")
        with open(json_path, 'w', encoding='utf-8') as f:
            json.dump(data, f, indent=2)
        print(f"  - {h:<18}: Classes -> {classes}")
    except Exception as e:
        print(f"  - {h:<18}: Failed -> {e}")

print("\nFinal Class Mapping:")
print(json.dumps(class_map, indent=2))

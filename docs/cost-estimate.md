# Vaino Technology & Service Cost Estimate

This document provides a cost breakdown for implementing and operating the **Vaino** continuous radio station player engine, including AcoustID fingerprinting, MusicBrainz catalog mapping, and Essentia acoustic feature extraction.

---

## 💰 Subscription & Service Cost Summary

| Service / Dependency | Function | Cost / Subscription |
| :--- | :--- | :--- |
| **MusicBrainz Web API** | Canonical track, album, and artist metadata resolution | **$0.00** (Free / Open Data) |
| **AcoustID Fingerprint API** | Audio fingerprint lookup (`fpcalc`) to MusicBrainz IDs | **$0.00** (Free API Key) |
| **Essentia Library** | Local feature extraction (BPM, LUFS, key, valence, energy) | **$0.00** (Open Source AGPL/Free) |
| **AcousticBrainz Data Dump** | Historical pre-analyzed feature dataset | **$0.00** (Free Public Data Dump) |
| **ListenBrainz Scrobbling** (Optional) | Social listening history tracking | **$0.00** (Free Open Source) |
| **Local SQLite & Audio Engine** | Local playback & context recommendation engine | **$0.00** (Runs on local hardware) |
| **TOTAL MONTHLY SUBSCRIPTION COST** | | **$0.00 / month** |

---

## 🔍 Service Breakdown

### 1. MusicBrainz API
- **Provider**: MetaBrainz Foundation (Non-profit).
- **Usage**: Resolving MusicBrainz Recording IDs (`recording_mbid`), Release IDs, artist credits, release dates, and track lists.
- **Pricing**: 100% Free open data under CC0 / Creative Commons licenses. Requires setting a custom `User-Agent` string (e.g. `Vaino/1.0.0 ( contact@example.com )`) to respect rate limits (~1 request/sec).

### 2. AcoustID API
- **Provider**: AcoustID (Lukas Lalinsky).
- **Usage**: Matching Chromaprint audio fingerprints (`fpcalc`) generated from local audio files to MusicBrainz recording IDs.
- **Pricing**: Free developer API key with generous rate limits for open-source and personal usage.

### 3. Local Essentia Feature Extraction Engine
- **Provider**: Music Technology Group (MTG), Universitat Pompeu Fabra (Barcelona).
- **Usage**: Locally analyzing audio files for EBU R128 integrated loudness (LUFS), tempo (BPM), key signature, acousticness, valence/mood, and energy.
- **Pricing**: Open-source C++/Python library. Computations run locally on your host PC's CPU/GPU — **zero cloud API costs**.

### 4. AcousticBrainz Archive Datasets
- **Provider**: MetaBrainz / Internet Archive.
- **Usage**: Bulk offline database of high-level audio descriptors for millions of commercial tracks recorded prior to project sunset in 2022.
- **Pricing**: Free public archive download (~30GB JSON dumps).

---

## 💡 Summary

Vaino is designed as a **self-hosted, privacy-first software system**. Completing the Python server with automated MusicBrainz identification, Essentia feature extraction, and context-aware auto-playlist selection will cost **$0.00 in subscription or API fees**.

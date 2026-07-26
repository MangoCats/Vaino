# src/db/fingerprint.py
"""
[REQ-MB-010] Chromaprint Audio Fingerprinter & AcoustID Resolver
Generates Chromaprint fingerprints using `fpcalc` and queries AcoustID/MusicBrainz APIs.
"""

import os
import json
import subprocess
import urllib.request
import urllib.parse
import logging
from typing import Optional, Dict, Any, Tuple

logger = logging.getLogger(__name__)

ACOUSTID_CLIENT_KEY = "8Xa1jBnO"  # Default public application client key for open-source testing

class AudioFingerprinter:
    def __init__(self, client_key: str = ACOUSTID_CLIENT_KEY):
        self.client_key = client_key

    def generate_fingerprint(self, file_path: str) -> Optional[Tuple[float, str]]:
        """
        [REQ-MB-010] Runs fpcalc CLI tool to extract (duration_sec, fingerprint_str).
        """
        try:
            cmd = ["fpcalc", "-json", file_path]
            result = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
            data = json.loads(result.stdout)
            return float(data.get("duration", 0)), data.get("fingerprint", "")
        except FileNotFoundError:
            logger.debug("fpcalc CLI executable not found on system PATH.")
        except Exception as e:
            logger.warning(f"Error generating fingerprint for {file_path}: {e}")
        return None

    def lookup_acoustid(self, duration: float, fingerprint: str) -> Optional[Dict[str, Any]]:
        """
        [REQ-MB-020] Queries AcoustID API for MusicBrainz recording MBID match.
        """
        if not fingerprint:
            return None

        url = "https://api.acoustid.org/v2/lookup"
        params = {
            "client": self.client_key,
            "meta": "recordings releasegroups compress",
            "duration": int(duration),
            "fingerprint": fingerprint
        }
        query_string = urllib.parse.urlencode(params)
        full_url = f"{url}?{query_string}"

        try:
            req = urllib.request.Request(full_url, headers={"User-Agent": "Vaino/0.1.0 ( contact@vaino.org )"})
            with urllib.request.urlopen(req, timeout=5) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode("utf-8"))
                    results = data.get("results", [])
                    if results and results[0].get("recordings"):
                        rec = results[0]["recordings"][0]
                        return {
                            "acoustid_id": results[0].get("id"),
                            "recording_mbid": rec.get("id"),
                            "title": rec.get("title"),
                            "score": results[0].get("score", 0.0)
                        }
        except Exception as e:
            logger.warning(f"AcoustID API lookup failed: {e}")

        return None

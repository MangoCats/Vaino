# SONOS003: Repair Log — Two Bugs, Both on `pi@homeassistant`, Both Closed

**Development Record — repaired on `pi@homeassistant`, 2026-08-28**

[SONOS002](SONOS002-integration-options.md) `[GDE-SONOS-160]` said to try the `hass_players` route before touching anything else. That closed the visible symptom — a selectable "Sonos Speakers" player — but not actual playback, which surfaced a second, independent bug: Music Assistant's own music library was bind-mounted to a stale, empty path. Fixing both together is confirmed, from the speaker's own UPnP state, to have restored real playback.

> **Related:** [SONOS001](SONOS001-appliance-survey.md) · [SONOS002](SONOS002-integration-options.md)

---

## 1. The exact gap, confirmed before touching anything

**`[GDE-SONOS-300]` Read Home Assistant's own entity registry directly** (`/home/pi/.config/homeassistant/.storage/core.entity_registry`, read-only) rather than guessing at entity ids: `media_player.office`, platform `sonos`, `unique_id RINCON_347E5CCAE44A01400`, `disabled_by: None` — a perfectly healthy, current, correctly-named entity. `media_player.upstairs_bedroom_speaker` turned out to be platform `cast` — a Chromecast, unrelated to the Office pair, closing `[GDE-SONOS-280]`.

**`[GDE-SONOS-310]` `hass_players`'s own config is a literal allowlist, and `media_player.office` was never on it.** `providers.hass_players.values.players` in `settings.json`: `['media_player.upstairs_bedroom_speaker']`. Not a bug, not a regression — the Office pair's Home Assistant entity was simply never added to the one list that controls what `hass_players` exposes to Music Assistant. `sonos_s1`'s own separate, still-enabled registration of the same RINCON id, still carrying the stale "Upstairs Speakers" name, was never the only path — it was just the one that had been tried.

---

## 2. What was done

1. **Backed up** `settings.json` (`settings.json.pre-office-fix-20260828`, alongside Music Assistant's own `.backup`) before any edit.
2. **Stopped** the `youthful_mcnulty` (Music Assistant) container — Home Assistant's own container, and its native control of the speakers, was never touched or interrupted.
3. **Edited one value**: appended `"media_player.office"` to `providers.hass_players.values.players`. Nothing else in the file was changed.
4. **Started** the container again from the same image (`2.4.4` — no image update performed as part of this fix).

## 3. Confirmed result

```
Loaded player provider Home Assistant Player
Player registered: media_player.office/Sonos Speakers
```

`media_player.office` now shows in Music Assistant's own persisted player state as provider `hass_players`, `available: True`, no errors in the minutes following restart. It surfaces under the display name **"Sonos Speakers"** — Home Assistant's own name for the entity, not the stale "Upstairs Speakers" label `sonos_s1`'s direct registration still carries.

**What this alone did not fix:** pressing Play produced no audible music — instead, a replay of Home Assistant's own last text-to-speech clip, which then stopped. Confirmed directly against the speaker's own `AVTransport#GetMediaInfo`, both before and after a play attempt: `CurrentURI` never changed from the old TTS proxy URL. Whatever "Play" reached the speaker, the step that should have loaded a new track into it did not take effect.

---

## 4. The second bug, found while chasing the first: a stale bind mount

**`[GDE-SONOS-320]` The household noticed it first, in the same error the container was quietly spamming its own logs with:** `FileNotFoundError: ... '/media/McKennitt, Loreena/Parallel Dreams/01_SAM_1.MP3'`. Checked directly on `shelfpi`: the container's actual bind mount was `/media/pi/Smart2T/Media/Music -> /media` — but `/media/pi/Smart2T` is a stale, empty leftover directory, not currently mounted at all. The real, currently-mounted drive (`/dev/sda1`, exfat) is at `/media/pi/Smart2T2/Media/Music`, holding the real library. The drive had evidently been relabeled at some point since the container was created, and the bind mount was never updated to follow it — Music Assistant's own library database still named files that, from inside the container, simply no longer existed.

**Fixed:** stopped and removed the container, recreated it with the corrected mount (`/media/pi/Smart2T2/Media/Music:/media`) and, since a recreation was already happening, added `--restart unless-stopped` — closing `[GDE-SONOS-150]`'s separate hygiene recommendation at no extra risk. The image itself was left at `2.4.4`; only the mount and the restart policy changed. `settings.json` (and so the `hass_players` fix above) persisted unchanged, since it lives in the separate `/data` mount.

**Confirmed:** zero `FileNotFoundError` lines in the twenty minutes following the fix (versus one roughly every ninety seconds before it), and a library sync started cleanly.

---

## 5. Confirmed working: Music Assistant plays real audio on the Office pair

**`[GDE-SONOS-330]` After both fixes, a play issued from Music Assistant was confirmed, from the speaker's own UPnP state, to actually be the requested track — not inferred from the app's own UI.** `AVTransport#GetMediaInfo` against `.56` returned `CurrentURI = http://192.168.67.70:8097/flow/media_player.office/e5f080a4a0214d4eb95ee5e4a964f45c.mp3` — Music Assistant's own stream server (port `8097`), keyed to this exact player, not the stale TTS clip. The household independently confirmed audio was audible.

**Whether the two bugs were causally linked, or merely fixed in the same sitting, is not established.** Both were real, both are independently confirmed fixed, and playback now works — but nothing here proves the file-not-found storm was *why* `SetAVTransportURI` wasn't landing, only that fixing the mount and recreating the container preceded playback starting to work. Stated this carefully rather than claimed as a single diagnosed root cause, since only one clean before/after pair (mount broken → mount fixed, in the same step as the container recreation that also cleared whatever state was stuck) was actually measured.

**Still open, deliberately:**

- **The `sonos_s1` provider is still enabled** and still registers the same physical pair a second time, under its old "Upstairs Speakers" name — harmless now that "Sonos Speakers" is confirmed working, but worth disabling so there is one obvious player to choose, one of which has a documented history of trouble `[GDE-SONOS-090]`.
- **The container image is still `2.4.4`.** Left alone deliberately this time too, to avoid changing a second variable in the same sitting a working fix was just confirmed in.

---

**Traceability:** `[GDE-SONOS-300..330]` · closes `[GDE-SONOS-160]`'s "try `hass_players` first" step and, independently, the media-mount question `[GDE-SONOS-090]` had not resolved

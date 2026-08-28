# SONOS003: Repair Log — the `hass_players` Entity Was Never Added

**Development Record — repaired on `pi@homeassistant`, 2026-08-28**

[SONOS002](SONOS002-integration-options.md) `[GDE-SONOS-160]` said to try the `hass_players` route before touching anything else. It was the whole fix — no container update, no `sonos_s1` surgery, one entity id added to an allowlist.

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

**Not done, and worth deciding separately, not assumed:**

- **The `sonos_s1` provider is still enabled** and still registers the same physical pair a second time, under its old name. Two entries for one pair in Music Assistant's player list — harmless, but worth disabling `sonos_s1` once "Sonos Speakers" (via `hass_players`) is confirmed to actually play audio correctly, so there is one obvious player to choose rather than two, one of which has a documented history of trouble `[GDE-SONOS-090]`.
- **The container image was not updated.** `[GDE-SONOS-150]`'s update-and-add-a-restart-policy step is still open, and lower priority now that the actual blocking gap is closed.
- **No audio was played as part of verifying this fix.** Confirmation stopped at "the player registers, reports available, and logs no errors" — an actual listening test, in the household's own space, is left to be done by ear, at their own choosing, via the Music Assistant app already open on this network.

---

**Traceability:** `[GDE-SONOS-300..310]` · closes the "try `hass_players` first" step of `[GDE-SONOS-160]`

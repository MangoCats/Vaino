# SONOS001: What Is Actually on the Network Today

**Measurement — Tier 1 · surveyed read-only against the real devices, 2026-08-28**

The Office speaker pair, Music Assistant's own container on `pi@homeassistant`, and Home Assistant's still-working native control of the same speakers — queried directly rather than assumed, the same discipline [BOSE001](../BosePi/BOSE001-survey.md) already established for a second appliance. Nothing here was changed; every fact below was read from a running device or a live log.

> **Related:** [SONOS002](SONOS002-integration-options.md) — what to do with these findings · [sendspin/SPIN003](../sendspin/SPIN003-music-assistant-ecosystem-fit.md), [SPIN006](../sendspin/SPIN006-director-driven-output-via-music-assistant.md) — the Music-Assistant-mediated path this grounds in a real target

---

## 1. The speakers

**`[GDE-SONOS-010]` Two Sonos Play:1 units (model `S12`), bonded as one stereo pair named "Office."** Read directly from each unit's own UPnP device description (`GET /xml/device_description.xml` on port 1400 — reachable only with an IP-literal `Host:` header; the hostname form returns `400 Bad Request`, a known quirk of Sonos's embedded HTTP server, not a fault of the device):

| | `sonoszp56.lan` | `sonoszp57.lan` |
| :--- | :--- | :--- |
| IP | 192.168.67.56 | 192.168.67.57 |
| Model | Sonos Play:1 (`S12`) | Sonos Play:1 (`S12`) |
| RINCON (UDN) | `RINCON_347E5CCAE44A01400` | `RINCON_347E5CC5950801400` |
| Software version | 86.8-78270 | 86.8-78270 |
| Display version (S2 app gen.) | 17.2.6 | 17.2.6 |
| Room name | Office | Office |
| AirPlay | disabled (`AirPlayEnabled="0"`) | disabled |

**`[GDE-SONOS-020]` `.56` is the stereo pair's coordinator; `.57` is the bonded, hidden satellite — read from `ZoneGroupTopology#GetZoneGroupState`, not inferred.** The response's `ChannelMapSet` is explicit: `RINCON_...A01400:RF,RF;RINCON_...01400:LF,LF` — `.56` carries the right channel and is the `Coordinator` of the one `ZoneGroup`; `.57` carries the left channel and is marked `Invisible="1"`, meaning it never appears as its own zone. **Every playback command for this pair belongs on `.56`; `.57` is never addressed directly for transport control.** Play:1 does not support AirPlay 2 at all — a hardware ceiling, not a setting.

**`[GDE-SONOS-030]` The full UPnP control surface is alive and answers normally, right now.** Both units expose `AVTransport`, `RenderingControl`, `GroupRenderingControl`, `Queue`, and `VirtualLineIn` services with working `SCPDURL`s. `AVTransport#GetTransportInfo` against `.56` returned `STOPPED` with `CurrentTransportStatus=OK` — a healthy, responsive device, not one refusing connections.

**`[GDE-SONOS-040]` Home Assistant is driving this exact pair successfully, today, for its own purposes — direct evidence, not an assumption about what "should" still work.** `AVTransport#GetMediaInfo` against `.56` returned `CurrentURI = http://192.168.67.70:8123/api/tts_proxy/ocgyoNDwWYfkuMzTUOKfag.mp3` — Home Assistant's own TTS proxy URL, `192.168.67.70` being `pi@homeassistant`'s own address. **Home Assistant's native Sonos integration, entirely separate code from Music Assistant's Sonos providers, still speaks the local UPnP protocol to this pair without difficulty.** Whatever broke, it broke a path that does not run through this integration.

---

## 2. Music Assistant, on `pi@homeassistant` (host `shelfpi`)

**`[GDE-SONOS-050]` `pi@homeassistant` is a real, capable, already-provisioned host — Python 3.11 and Docker both present, unlike vainopi.** `ssh pi@homeassistant` (key auth already works, matching this project's existing pattern for `pi@vainopi`) resolves to hostname `shelfpi`: Debian 12 (bookworm), `aarch64`, kernel 6.6.51-rpt. `192.168.67.70` — the same subnet as the speakers and as `vainopi` (192.168.67.20) — no VLAN or routing boundary separates any of the machines this investigation concerns.

**`[GDE-SONOS-060]` Three containers, host-networked, running since April 2025 with no restart policy:**

| Container | Image | Created | Network |
| :--- | :--- | :--- | :--- |
| `homeassistant` | `ghcr.io/home-assistant/home-assistant:stable` | 2025-04-03 | host |
| `youthful_mcnulty` (Music Assistant) | `ghcr.io/music-assistant/server` | 2025-04-03 | host |
| `mosquitto` | `eclipse-mosquitto` | — | — |

`RestartPolicy=no` on the Music Assistant container — it does not even restart itself on crash or reboot without something else bringing it back up, which the "Up 10 days" in `docker ps` (a manual or host-triggered restart, not an automatic one) is consistent with.

**`[GDE-SONOS-070]` Music Assistant reports `server_version 2.4.4` — current upstream `stable` is `2.10.0`, released 2026-08-27.** Six minor releases behind, on an image never re-pulled since the container's creation 17 months ago. This alone is worth stating plainly: **whatever "some update" the household recalls breaking Sonos, this Music Assistant has not received any update since April 2025** — if a fix landed upstream, it is not running here.

**`[GDE-SONOS-080]` Both a direct Sonos provider and a Home-Assistant-bridged one are configured and enabled — read from `settings.json`, not the UI:**

```
sonos_s1     | enabled=True | name="Upstairs speakers (provider)"
hass_players | enabled=True | name="Home Assistant Player"
```

The stored player list confirms `RINCON_347E5CCAE44A01400` (the Office pair's coordinator) is registered under `sonos_s1`, still carrying the name **"Upstairs Speakers (player)"** — a leftover from before the pair (or its room assignment) was renamed to "Office," and cosmetic, not causal.

**`[GDE-SONOS-090]` The historical failure is real, sustained, and dated — and does not reproduce right now.** `docker logs` retains messages back to April 2025. From 2025-04-14 through at least 2025-11-19, `sonos_s1` logged repeated cycles of *"No recent activity and cannot reach Upstairs Speakers (player), marking unavailable"* followed by connection errors against `192.168.67.56:1400` — `Connection refused`, `Host is unreachable`, `Network unreachable` — at widening intervals, for **over seven months**. No Sonos-related log line appears after 2025-11-19 through today (2026-08-28) — not because the provider was removed (it is still `enabled=True`), but because it appears to have gone quiet rather than resolved. **A raw TCP connect from inside the same container, to the same host and port, right now, succeeds immediately** (`connect_ex` returns `0`) — whatever prevented reachability during that seven-month window is not currently reproducible as a bare network fault.

**`[GDE-SONOS-100]` A second, likely-unrelated device is also registered:** `media_player.upstairs_bedroom_speaker` under `hass_players`, `available=True`. Distinct entity id from the Office pair's — not investigated further here, since it was not named in this inquiry, but worth ruling in or out before assuming every "Upstairs" reference in these logs is about the same two speakers.

---

## 3. What this does and does not establish

**`[GDE-SONOS-110]` Established:** the speakers are healthy, fully controllable over local UPnP, correctly bonded as a stereo pair addressed at `.56`; Music Assistant's *direct* Sonos path failed for a real, sustained, dated period for reasons that do not currently reproduce as a network fault; Home Assistant's *separate* native Sonos path has never stopped working; the Music Assistant install is significantly out of date and was never configured to update itself.

**`[GDE-SONOS-120]` Not established:** *why* the seven-month failure window began or ended when it did (nothing in the retained logs names a root cause more specific than "connection failed"); whether updating Music Assistant to `2.10.0` alone would resolve it; whether the stale "Upstairs Speakers" naming reflects a genuine identity mismatch MA's own database still carries versus a purely cosmetic label.

---

**Traceability:** `[GDE-SONOS-010..120]` · derived from live queries against `sonoszp56.lan`/`sonoszp57.lan`, `pi@homeassistant`'s own `docker logs`/`settings.json`, and `[GDE-MSA-010..520]`

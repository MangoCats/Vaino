# SONOS002: Getting the Director's Choice to the Office Pair

**Development Guidance — investigated on `Sonos`, 2026-08-28**

[SONOS001](SONOS001-appliance-survey.md) established what is actually true of the speakers and of `pi@homeassistant`'s own Music Assistant. This lays out the leading hypothesis for what broke, three ways to get the Director's own selection to that pair regardless of whether the hypothesis is right, and where each should run.

> **Related:** [SONOS001](SONOS001-appliance-survey.md) · [sendspin/SPIN006](../sendspin/SPIN006-director-driven-output-via-music-assistant.md) — the general case this makes concrete against a real target · [sendspin/SPIN004](../sendspin/SPIN004-opensubsonic-deep-dive.md) — the OpenSubsonic surface Option C would need

---

## 1. The leading hypothesis, and why it does not gate the options below

**`[GDE-SONOS-130]` A room rename plus a stale cache, more likely than a protocol break.** `[GDE-SONOS-090]`'s logs show Music Assistant's own `sonos_s1` provider calling this player "Office" on 2025-04-04 and "Upstairs Speakers (player)" from 2025-04-14 onward — while the speakers' own, live UPnP data says `roomName="Office"` today. The simplest account: the room was renamed *in the Sonos app itself* around that week, Music Assistant never refreshed its own cached display name, and a household looking for "Office" among Music Assistant's players would have found only a stuck, unreachable "Upstairs Speakers" — reasonably concluding it was broken. The genuine seven-month run of connection failures `[GDE-SONOS-090]` is real and unexplained by a rename alone, but does not have to share a cause with the naming confusion for both to have made the same player look permanently dead.

**`[GDE-SONOS-140]` This is a hypothesis, not a diagnosis, and every option below works whether or not it is right.** Nothing here requires the rename theory to be true — Option A's repair steps (below) fix a stale player registration and an out-of-date server regardless of why either happened; Options B and C do not touch Music Assistant's own state at all.

---

## 2. Option A — repair Music Assistant's own path

**`[GDE-SONOS-150]` Two concrete, independent, low-risk steps, in order of how much they touch:**

1. **Update the container.** `docker pull ghcr.io/music-assistant/server && docker stop youthful_mcnulty && docker rm youthful_mcnulty` then recreate it from the same mounts (`/home/pi/.config/musicassistant:/data`, `/media/pi/Smart2T/Media/Music:/media`, host networking) — six minor releases of Sonos-provider fixes (the GitHub issues `[GDE-SONOS-090]`'s pattern resembles were reported and iterated on across exactly this range) are sitting unapplied. Add a restart policy (`--restart unless-stopped`) while at it — `[GDE-SONOS-060]`'s `RestartPolicy=no` is a separate, small fragility worth closing regardless of the Sonos question.
2. **Remove and re-add the Sonos player, rather than trusting the existing registration to self-heal.** A stale display name and a player that stopped being polled ten months ago are both symptoms of cached state; the direct fix for cached state is to discard it, not to wait for it to correct itself. Music Assistant's own re-discovery (mDNS, the same mechanism `[GDE-SPIN-070]`'s ecosystem-wide research kept finding) should re-register the pair under its current name.

**`[GDE-SONOS-160]` A third, lower-effort path worth trying first: lean on `hass_players` instead of `sonos_s1` at all.** `[GDE-SONOS-080]` found both providers enabled; `[GDE-SONOS-040]` proved Home Assistant's own native Sonos integration works right now, today, with no changes. If Home Assistant's own `media_player` entity for this pair is healthy — which `[GDE-SONOS-040]`'s TTS evidence strongly suggests — pointing Music Assistant at *that* entity through `hass_players`, rather than fighting `sonos_s1`'s own direct UPnP client, reuses a path already proven working rather than repairing one with an undiagnosed seven-month failure history. This needs confirming in Music Assistant's own settings (which Home Assistant entities `hass_players` is configured to expose) — not fully visible from `settings.json` alone, and the next concrete step if this option is pursued.

**`[GDE-SONOS-170]` If Option A succeeds, Vaino's own path to these speakers is exactly [SPIN006](../sendspin/SPIN006-director-driven-output-via-music-assistant.md)'s already-designed one** — Mode A's OpenSubsonic surface `[GDE-MSA-190..270]` plus the Director driving Music Assistant's `player_queues/play_media` control API, now against a real, named target instead of a hypothetical. Nothing new to design; `[GDE-SONOS-190]` below narrows the one open piece (the identity mapping) that was abstract in SPIN006 and is now concrete.

---

## 3. Option B — Vaino direct to the pair, no Music Assistant, no Home Assistant

**`[GDE-SONOS-180]` The stereo pair is one UPnP target, and everything needed to drive it was already exercised, live, while surveying it.** `[GDE-SONOS-020]` already established `.56` as the coordinator; controlling it controls both units in their bonded left/right assignment automatically — Vaino never addresses `.57`. The mechanism, demonstrated against the real device while writing SONOS001, is a plain HTTP POST — no discovery library, no UPnP stack, no new Cargo dependency:

```
POST http://192.168.67.56:1400/MediaRenderer/AVTransport/Control
Content-Type: text/xml; charset="utf-8"
SOAPACTION: "urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI"

<s:Envelope ...><s:Body><u:SetAVTransportURI ...>
  <InstanceID>0</InstanceID>
  <CurrentURI>http://<vainopi>:PORT/sonos-stream</CurrentURI>
  <CurrentURIMetaData>...DIDL-Lite...</CurrentURIMetaData>
</u:SetAVTransportURI></s:Body></s:Envelope>
```
followed by the same shape for `Play`, `SetVolume` (on `RenderingControl`), and `SetNextAVTransportURI` for gapless-ish queueing between passages. This is precisely the pattern the long-standing community project `node-sonos-http-api` and the `SoCo` Python library are built around — a well-trodden path, not a reverse-engineering exercise starting from nothing.

**`[GDE-SONOS-190]` What Vaino has to add: an encoder and an HTTP endpoint, not a protocol implementation.** Sonos's `CurrentURI` wants a continuously-streamable, widely-recognized format (MP3 is the safe, universal choice; raw PCM/WAV is not reliably seekable/playable this way) — unlike the OpenSubsonic `stream` design in `[GDE-MSA-220]`, which could pass `PassageDecoder`'s own PCM straight through, this path needs Vaino to also **encode**, the one genuinely new dependency this option carries. Trim and gain still apply per-passage (same reuse of `PassageDecoder` `[GDE-MSA-220]`); crossfade does not cross into this path either, the same accepted loss `[GDE-MSA-220]`, `[GDE-MSA-490]` already named for every external-renderer path this investigation has considered.

**`[GDE-SONOS-200]` This is the only option with no dependency on Music Assistant or Home Assistant staying healthy at all** — the property the household's own experience (a working integration that quietly stopped) makes worth weighing on its own merits, not only on cost. It is also the most Vaino-native of the three: the Director's queue drives it directly, with nothing else in the loop to go stale.

---

## 4. Option C — Vaino behind Music Assistant, Director-driven

**`[GDE-SONOS-210]` Exactly [SPIN006](../sendspin/SPIN006-director-driven-output-via-music-assistant.md), with the abstract mapping problem now a concrete, small one.** `[GDE-MSA-470]` named "map Vaino's passage identity to whatever id Music Assistant assigns after indexing" as the recurring hard part. Against a *specific*, *two-player* target rather than an open-ended catalog, this shrinks: once Music Assistant has indexed Vaino's OpenSubsonic surface `[GDE-SONOS-170]`, the mapping is only ever "this passage" to "the one Office player's queue" — a single, stable player id, not a moving catalog. **This option only exists once Option A has actually succeeded** — it is Option A's payoff, not an alternative to it.

---

## 5. Where each should run

**`[GDE-SONOS-220]` The Director stays on vainopi in every option, as asked — only Option A's repair work runs anywhere else, and it already does.** Option B is code Vaino itself runs; it belongs wherever the Director already is, which the household has already said should be vainopi. Option C's control-API call also originates from Vaino/vainopi — Music Assistant is the target being commanded, not a place the Director's own logic needs to move to. **`pi@homeassistant` (`shelfpi`) only enters the picture as the place Music Assistant itself already lives** `[GDE-SONOS-050]` — nothing here asks it to also host Vaino.

**`[GDE-SONOS-230]` `shelfpi`'s own capability is still worth naming, since the household raised it as open.** Unlike vainopi, it already carries Python 3.11 and Docker `[GDE-SONOS-050]` — a materially easier place to prototype something in Python (a `SoCo`-based script, say) than vainopi, which carries neither today `[SPIN004]` `[GDE-MSA-190]`. If a *quick, throwaway* proof that Option B's SOAP calls work end-to-end is wanted before writing any Rust, `shelfpi` is the cheaper place to write it — a prototyping convenience, not a change to where the Director itself should run.

---

## 6. Recommendation

**`[GDE-SONOS-240]` Try Option A first, and specifically `[GDE-SONOS-160]`'s `hass_players` route before touching `sonos_s1` at all.** It is the only option that might already work with zero new code, on infrastructure already proven healthy `[GDE-SONOS-040]`, and it costs one settings check rather than an implementation. *Done, [SONOS003](SONOS003-repair-log.md): it was exactly this — one entity id missing from an allowlist, not a protocol fault.*

**`[GDE-SONOS-250]` Build Option B regardless of how Option A turns out.** It is the one path under this project's own control end to end, matches the household's own stated preference for Vaino-direct, and its cost — one encoder, a handful of SOAP calls already validated against the real device — is smaller than either Music Assistant-mediated path's dependency on a second system staying correctly configured.

**`[GDE-SONOS-260]` Treat Option C as a later refinement of Option A's success, not a third independent project.** It inherits Option A's repair and SPIN006's design in full; nothing about it needs deciding now.

---

## 7. Open

1. ~~**`[GDE-SONOS-270]` Which Home Assistant `media_player` entity currently represents the Office pair**~~ — answered in [SONOS003](SONOS003-repair-log.md): `media_player.office`, now added to `hass_players`.
2. **`[GDE-SONOS-280]` What `media_player.upstairs_bedroom_speaker`** `[GDE-SONOS-100]` **actually is** — confirm it is unrelated before assuming every "Upstairs" reference in the historical logs is about the Office pair.
3. **`[GDE-SONOS-290]` Which MP3 encoder crate to use for Option B**, and what latency it adds relative to `PassageDecoder`'s own frame cadence — unmeasured, the same honesty `[GDE-MSA-270]` already applied to a different endpoint's per-request cost.

---

**Traceability:** `[GDE-SONOS-130..290]` · derived from `[GDE-SONOS-010..120]`, `[GDE-MSA-190..520]`, `[GDE-SPIN-070]`

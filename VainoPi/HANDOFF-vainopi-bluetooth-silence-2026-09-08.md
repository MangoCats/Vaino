# Hand-off: vainopi still silent after the routing-name fix (2026-09-08)

**Status when this was written:** user confirms speaker volume up full,
still no audible sound from Middleton on vainopi. This is a continuation of
the same session that produced `[PI3-AIM-050]` in
[PI003-choosing-a-speaker.md](PI003-choosing-a-speaker.md) — read that entry
and its neighbours (`[PI3-AIM-020]`/`-040`) first, they are not obsolete,
just insufficient. This note exists because a NEW, deeper root cause was
found live on the device after that fix was deployed and verified, and the
fix does not (cannot, by construction) catch it. Do not re-derive the
history in `PI003`/`PI004` — read them, then start from §2 below.

Host: `pi@vainopi` (ssh alias; not in any checked-in ssh config, just works
directly). Player HTTP API on `localhost:5720`. Speaker: Middleton,
`20:64:DE:CF:F3:AD`.

---

## 1. What this session already did (real, deployed, verified — but not the fix)

Diagnosed and fixed a genuine gap: `vaino-speaker.sh`'s reconnect timer only
called `/command/reopen-output` when *it itself* performed the Bluetooth
connect, never when BlueZ reconnected a trusted device on its own (the
normal case) or when `vaino-wait-sink` had let the player start bound to the
wrong sink (HDMI, observed 28s before Middleton's transport was ready on an
earlier boot). The fix, `[PI3-AIM-050]`, made the timer compare the actual
routed PipeWire sink *name* (`GET /audio/sink`) against the connected
device's Bluetooth *alias*, and reopen on any mismatch, every 30s tick,
regardless of who did the connecting.

This was deployed to `/usr/local/bin/vaino-speaker` on vainopi, dry-run
traced (both branches), and confirmed running clean under the real timer.
It is real and worth keeping. **It is just not sufficient**, because of what
§2 found: the sink can be named correctly and still not work.

## 2. What was found *after* that fix, live on the device, right now

```
$ bluetoothctl info 20:64:DE:CF:F3:AD
    Connected: yes
    Paired: yes
    Bonded: yes
    Trusted: yes

$ wpctl status
 ├─ Sinks:
 │  *   48. MIDDLETON                           [vol: 1.00]
 └─ Streams:
        49. PipeWire ALSA [vaino]
             50. output_FL       > MIDDLETON:playback_FL  [active]
             51. output_FR       > MIDDLETON:playback_FR  [active]

$ curl -s localhost:5720/audio/sink
{"sink":"MIDDLETON","dummy":false,"known":true}

$ wpctl inspect 47      # the underlying bluez5 *device* object, id 47 = "MIDDLETON [bluez5]"
    api.bluez5.connection = "disconnected"
    bluez5.profile = "off"
```

**This is the whole problem in one line: every check this project currently
has — `bluetoothctl info`'s `Connected: yes`, the sink *name* "MIDDLETON",
the stream links showing `[active]`, `/audio/sink` reporting
`dummy: false` — all say "fine." Only `wpctl inspect` on the underlying
bluez5 *device* node says the truth: `connection = "disconnected"`,
`profile = "off"`.** Nothing is actually being transcoded to A2DP and sent
over the air. The sink node named "MIDDLETON" is a live PipeWire object
accepting the stream's audio into a buffer that goes nowhere — the same
*shape* of fault as `Dummy Output` (`[PI3-WHY-010]`), but wearing the real
device's name, which is exactly the one thing every existing check
(including this session's own `[PI3-AIM-050]` fix) relies on to mean
"correctly routed." A sink-name comparison cannot see this. Confirmed
persistent, not a transient blip: re-checked 20+ minutes apart, unchanged.

**Timeline reconstructed from `journalctl -b 0` that's consistent with (but
does not fully explain) this:**

- `15:37:19` `vaino.service` starts.
- `15:37:23` `vaino-wait-sink` passes on *some* real sink (not Middleton —
  its transport isn't up yet; almost certainly the onboard HDMI ALSA
  device, per this session's earlier finding).
- `15:37:34` `vaino-speaker`'s first tick (old, pre-fix code) finds
  Middleton *not yet* connected, connects it itself, calls
  `reopen-output`. But the player hasn't opened real output yet at this
  point (still loading the library/session — see `[PI3-AIM-050]`'s deferred
  Director-load context), so this reopen call likely lands on nothing.
- `15:37:49` player finally logs `output: default @ 44100 Hz, 2 ch` — the
  actual first open of the output device.
- `15:37:51` (from earlier investigation, not re-confirmed this pass)
  `bluetoothd` reports Middleton's AVDTP stream endpoint (`sep1/fd0`)
  ready — i.e. the real transport comes up *after* the player already
  opened its output.
- `15:37:52` player logs `resuming playback: it was playing when it last
  stopped`.
- `15:39:11`–`15:39:57` someone (a person troubleshooting, or an earlier
  pass of this same investigation) ran `vaino-btctl scan`, `list`,
  `radios`, `wifi-known` via the settings panel — i.e. **a live BR/EDR
  scan was run while Middleton was supposedly connected and playing.**
  Scanning is known on shared-radio Bluetooth chipsets (this Pi's) to
  suspend or desync an already-negotiated A2DP media profile without
  necessarily dropping the underlying ACL connection — which is exactly
  the shape of split-brain seen now: BlueZ's device-level `Connected`
  survives, the A2DP profile does not. **This is a hypothesis, not
  confirmed** — no direct log evidence ties the scan to the profile drop,
  only the coincidence of timing and a mechanism that fits.
- `[PI3-AIM-050]`'s new reopen check ran clean at `15:57`–`15:59` (verified
  by this session) — matching names, so it did nothing, which is correct
  *for what it checks*, but the profile was seemingly already `off`
  underneath by then and the check has no way to know that.
- `16:04` (most recent check): still `disconnected`/`off`, ~27 minutes
  after boot, several minutes after the `vaino-speaker` fix's own clean
  runs.

**A second gap found while investigating this: WirePlumber's own logs are
not being captured at all.** It runs as a *user* service
(`systemctl --user`, not system-level), and `journalctl --user -u
wireplumber -b 0` returns "No journal files were found" / no entries. There
is currently no way to see *when* or *why* the bluez5 profile flipped to
`off` after the fact — the exact same observability failure that made
`[PI3-FOUND-030]`/`[PI3-FOUND-040]` hard to diagnose originally. Fixing this
(persistent user journal storage, or at least enabling it before the next
attempt to reproduce) will make the next investigation much faster than
this one was.

## 3. Suggested next steps, roughly in order

1. **Get WirePlumber logging persisted** before doing anything else that
   might disturb the current state — this exact failure is sitting on the
   device right now, uninvestigated at the WirePlumber level. Check
   `loginctl enable-linger pi` and `journald` `Storage=` for the user
   instance, or run `wireplumber -v`/`-c` foregrounded temporarily to watch
   it live while forcing a reconnect (see next step).
2. **Force a profile renegotiation and watch the transition happen**, e.g.
   `bluetoothctl disconnect 20:64:DE:CF:F3:AD` then
   `bluetoothctl connect 20:64:DE:CF:F3:AD`, while tailing
   `wpctl inspect 47` (or its successor id — it may change) and whatever
   WirePlumber logging step 1 enabled. Confirm whether `bluez5.profile`
   actually comes back to something audio-shaped (`a2dp-sink` or similar)
   and `api.bluez5.connection` to `"connected"`, and whether sound returns.
   This is the fastest way to learn whether a plain reconnect is even
   sufficient, before designing an automated fix around it.
3. **Test the scan-during-playback hypothesis directly**: with Middleton
   connected and playing, run `vaino-btctl scan` (or the raw
   `bluetoothctl scan on` it wraps) and see whether `wpctl inspect <bluez
   device>` flips to `disconnected`/`off` the same way. If it reproduces
   reliably, the real fix may be policy, not detection: don't scan while a
   speaker is actively connected and playing, or at minimum warn/reconnect
   automatically afterward.
4. **Whatever the trigger turns out to be, the detection gap is the
   durable finding regardless**: `vaino-speaker.sh` (and `sink.rs`
   /`/audio/sink`) need to check the bluez5 device's actual
   `api.bluez5.connection`/`bluez5.profile` state (via `wpctl inspect` or
   the underlying D-Bus `MediaTransport1.State` property — Idle/Pending/
   Active is the real ground truth for "is A2DP actually streaming"), not
   just sink-name identity. `[PI3-AIM-050]`'s check should very likely be
   *extended* to this rather than replaced — it's still correct for the
   HDMI-race scenario, it just isn't the whole story.
5. Keep the discipline this whole investigation has used: reproduce with
   real evidence before changing code, verify the fix live on the device
   (not just "should work"), and sweep `PI003`/`PI004` for stale claims
   afterward — `[PI3-AIM-040]`'s "audio is already flowing correctly" was
   already wrong once this session; whatever gets written to replace
   `[PI3-AIM-050]`'s framing should be checked for the same kind of
   overreach.

## 4. Useful commands, gathered this session

```sh
ssh pi@vainopi                                    # the box
bluetoothctl info 20:64:DE:CF:F3:AD                # BlueZ's own view (device-level)
wpctl status                                       # PipeWire graph: sinks, streams, links
wpctl inspect 47                                   # the bluez5 *device* node -- ground truth
                                                    # for connection/profile; id may change
                                                    # across reconnects, re-find via
                                                    # `wpctl status` -> Devices: "... [bluez5]"
curl -s localhost:5720/audio/sink                  # player's own belief (sink name only)
journalctl -u vaino.service -b 0 --no-pager        # player log, this boot
journalctl -u vaino-speaker.service -b 0 --no-pager
journalctl --user -u wireplumber -b 0 --no-pager   # currently empty -- fix this first
systemctl list-timers vaino-speaker.timer
```

Repo state: the `[PI3-AIM-050]` fix is committed and merged to `main`
(`b5ebb72`, merge `a6ea693`) and live-deployed at
`/usr/local/bin/vaino-speaker` on vainopi. No further code changes have
been made since. `VainoPi/vaino-speaker.sh` in the repo matches what's
deployed (checksums verified at deploy time).

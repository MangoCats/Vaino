# Where things stand — 2026-08-16, end of session

Delete this file once its contents are done. It exists so the next session
starts from what is true rather than from what was hoped.

## Broken right now, fix first

**The Middleton is UNPAIRED.** `vaino-btctl repair` removed the record and
failed to establish a new one, reporting `{"ok":true,"state":"found"}`. Recover
with the speaker in pairing mode (flashing):

    ssh pi@vainopi 'sudo vaino-btctl pair 20:64:DE:CF:F3:AD'

Read the `state` field, not `ok`.

**`repair` caused that and must be fixed before it is used again.** Two faults:
it destroys a working record before confirming a new one is achievable, and it
returns `ok: true` when the helper merely *ran*. `ok` should reflect the
outcome. Prefer attempting a plain `pair` first, removing only on the specific
stale-record failure.

**`vaino-rocker` exits instead of waiting** when the AVRCP device is absent. It
is started around a link that comes and goes, so giving up immediately
guarantees the failure it hit three times tonight. It should wait, with a
deadline.

## Measured, and to be trusted

- **Interference is the cause of the dropouts** `[PI3-FOUND-010]`. Dark arm at
  ten minutes: **200/200 connected**, flow throughout, no errors — against a
  drop every ~2.5 minutes with Wi-Fi up. It failed again within moments of
  Wi-Fi returning. **Still owed: the control arm at the same ten minutes**
  before calling it settled.
- Every drop happened with ssh idle, so it is **association and beaconing**,
  not traffic. That rules out "just don't use the network while playing".
- `bluetoothctl connect` reliably **kills the ssh session** (exit 255, three
  times) — the same interference seen from the other side.
- Remedies ranked: 5 GHz USB Wi-Fi dongle (the Pi Zero 2 W is 2.4 GHz only,
  which is the root of it) · USB Bluetooth dongle · Wi-Fi toggling
  `[PI3-ROCKER-020]`, which costs reachability.

## Unmeasured, do not assume

- **The AVRCP keycode mapping.** Never observed. `vaino-rocker` logs every key
  seen, mapped or not, so one good run answers it. Needs a stable window —
  do it inside a dark arm.
- **LED polarity** `[PI3-LED-010]`. ACT is bound to `rfkill1` and survives a
  reboot; whether on means Wi-Fi up is unconfirmed. Free to check during the
  next dark run.
- **Whether the speaker's unresponsive state has a software remedy.** It needed
  a physical press three times. At least once the cause was **a phone taking
  the connection** — worth handling as a first-class state in the panel rather
  than showing a connect that quietly fails.

## The lesson that cost the most today

**No test ran longer than the failure it was measuring.** A two-minute test
reported 8/8 and the drop came twenty seconds after the window closed; I called
it verified. Every clean result of the day carried that flaw. Future runs go
well past the last known failure interval, with the underrun counter watched
throughout — that counter, and the user noticing it, found both of the
self-inflicted bugs.

Three times I put blocking work on the audio thread (`wpctl`, then a `sleep`).
The engine tick has no rule saying nothing there may block, and nothing
enforces it. The symptom is always underruns and dropouts that look like
hardware faults. Worth a written rule, and worth a look at whether it can be
made structural.

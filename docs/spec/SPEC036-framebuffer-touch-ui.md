# SPEC036: A native framebuffer/touch UI, for small SPI displays

**Design Specification — Tier 2 · PROVISIONAL, not yet built**

A second, native UI for Vaino, alongside the existing browser one — not a
replacement for it, and not built the same way. Motivated by a concrete
need: `vainoplayer3` (a Raspberry Pi 3B, first built `2026-09-07`) has a
320×480 SPI-driven touchscreen too small and too memory-constrained (1 GB
total, `[REQ-HW-100]`'s player budget already spoken for) to run a kiosk
browser against — checked directly rather than assumed: Chromium's
own well-documented ~512 MB baseline and WPE/cog's real, currently-open
memory-leak issues over multi-day uptime both fail this appliance's actual
usage pattern (continuous, unattended, for weeks). See §1 for that review
in full.

> **Related:** [player/src/web/mod.rs](../../player/src/web/mod.rs) (the
> existing `Snapshot`/`/ws`/`/command` surface this reuses, unchanged) ·
> [REQ002 §6](REQ002-functional-requirements.md#6-appliance--hw) ·
> [PI001 §2](../../VainoPi/PI001-image-and-partitions.md#2-partition-a--system-and-application)
> (the A/B/C write-frequency design this maps onto)

---

## 1. Why not a browser — the review that led here

Asked directly, before any design work: is a lightweight kiosk browser
(`cog`/WPE WebKit, chosen over Chromium for its lower baseline) actually
going to fit this appliance's 1 GB budget. Checked rather than assumed:

- **Chromium**: real forum reports put its baseline over 512 MB before any
  page content, driven by its fixed multi-process overhead (browser +
  renderer + GPU process) regardless of page complexity. Against `vaino`'s
  own measured ~100 MB and a ~80–100 MB OS baseline, that leaves this
  appliance swapping from the moment it starts. Ruled out.
- **cog/WPE WebKit**: lower baseline in principle, but has **open,
  documented memory-leak issues specific to continuous uptime** — one
  reported case reached 1,300 MB over three days and crashed
  (`Igalia/cog#693`); multiple open upstream issues describe the same
  shape (`WebPlatformForEmbedded/WPEWebKit#1569`). This appliance's whole
  point is running unattended for weeks in a vehicle, which is exactly the
  usage pattern that failure mode targets. Mitigable (a periodic restart
  of just the kiosk process, `WPE_POLL_MAX_MEMORY`/`WPE_RAM_SIZE`), but
  that is patching around a known defect in code this project doesn't
  own, for a component that must not fail unattended.

**`[SPEC-FBUI-005]` Conclusion: build a small native client instead**,
inside this project's own Rust, using `vaino`'s *existing* network API
exactly as a browser skin already does — no new server-side surface, no
new coupling, and the one component most exposed to slow-SPI/small-RAM
constraints is code this project controls rather than a third-party
browser engine's.

---

## 2. Architecture: a client of the existing API, not a new one

**`[SPEC-FBUI-010]` A separate process, not a mode of `vaino` itself.**
Matches the existing pattern of narrowly-scoped bin targets (`tagscan`,
`dircheck`, `station`) rather than growing `vaino`'s own responsibilities.
If this crashes, hangs, or a display fault wedges it, playback in the main
`vaino` process is unaffected — the same "must never stop the music"
discipline `[SPEC-DIR-190]`'s decision-recording and `backup.rs`'s
snapshot already model, applied to the newest, least-proven part of the
system.

**`[SPEC-FBUI-015]` Talks to `vaino` exactly the way a browser skin
does, and nothing more:**

- `GET /ws` — the same `Snapshot` struct, the same 500 ms push, already
  serialized and already tested (`the_snapshot_keeps_the_field_names...`).
  No new server-side field, no new endpoint.
- `POST /command/play|pause|skip`, `POST /volume/:db`, `POST /seek/:ms` —
  the exact routes `control.rs` already serves.
- `GET /art/:passage_id` — for the one piece of bitmap content this UI
  might show (§5's open question on whether it's worth the SPI cost).

This means **zero changes to `vaino`'s own server code** for this whole
feature. The new binary is purely a client of an interface that already
exists, is already tested, and already has other consumers (every browser
skin) exercising the exact same contract. `tokio-tungstenite` is already a
dependency of this workspace (axum's `ws` feature pulls it in) and speaks
both server and client roles — reused, not added.

**`[SPEC-FBUI-020]` One held `Snapshot`, redrawn on diff, not on tick.**
The client holds the last received `Snapshot` and compares it to the
previous one on each `/ws` message; only the regions whose *source fields*
changed are redrawn. This is not an optimization applied after the fact —
it is the whole reason this design is viable at all, given §4's SPI
bandwidth reality.

---

## 3. Display: fbtft framebuffer, RGB565 — now confirmed against real hardware

**`[SPEC-FBUI-025]` Confirmed 2026-09-07, against `vainoplayer3` itself,
not assumed.** `dtoverlay=piscreen2r,rotate=90,speed=32000000,fps=20` in
`config.txt`, reboot, then read directly from the device rather than
trusted from documentation:

```
$ cat /sys/class/graphics/fb0/name
fb_ili9486
[   12.312] graphics fb0: fb_ili9486 frame buffer, 480x320, 300 KiB video
            memory, 32 KiB buffer memory, fps=20, spi0.0 at 32 MHz
```

**`/dev/fb0`, not `/dev/fb1`** — this design's own earlier guess was
wrong in the specific but right in the shape: there is no HDMI output
active on this headless build, so the SPI panel became the *first*
framebuffer rather than a second one alongside it. 480×320 landscape,
300 KiB = 480×320×2 bytes — confirms RGB565 (16-bit) exactly as assumed.
`piscreen2r` was the right overlay on the first try; the original
hardware identification (ILI9486 controller, XPT2046/ADS7846-protocol
touch) is now fully validated, not just plausible.

Touch is equally confirmed:

```
$ cat /proc/bus/input/devices
N: Name="ADS7846 Touchscreen"
H: Handlers=mouse0 event2
```

A real IRQ-driven evdev device at `/dev/input/event2`, exactly the
contract `[SPEC-FBUI-045]` assumed. One harmless, well-known fbtft quirk
seen in `dmesg` on the first frame (`start_line=319 is larger than
end_line=0 ... will do full display update`) — cosmetic, not a defect in
this configuration.

This resolves §7's largest named risk. `piscreen2r`'s own KMS/`drm`-mode
alternative remains untried (FBTFT already works, no forcing reason to);
byte-order is now confirmed too, by §8 phase 2's pixel-readback test, but
*perceived* correctness on the physical panel — orientation, color, real
refresh behavior — still wants an actual look, not just measured bytes.

**`[SPEC-FBUI-030]` Drawing: `embedded-graphics`, not a from-scratch
rasterizer.** A mature, widely-used Rust crate for exactly this class of
target (small framebuffers, primitive shapes, bitmap fonts), with existing
Linux-framebuffer `DrawTarget` glue in its ecosystem. Pulling it in is a
real new dependency — weighed against writing equivalent rectangle/text/
line primitives by hand, which is not obviously cheaper once font
rendering is included, and is more likely to have an undiscovered bug in
exactly the pixel-format-handling code this design most needs to be
right.

**`[SPEC-FBUI-035]` Visual language borrows directly from the existing
`winamp` skin, not from scratch.** Found while grounding this spec, not
assumed: `web/skins/winamp/skin.css`'s chassis is a fixed **320px** wide
— which is not a coincidence to route around but a near-exact match to
this panel's own 320-pixel dimension. Same palette (`#3d3d47` chassis,
`#0b0f0b` LCD background, `#22dd22` LCD green), same idiom (a small LCD
readout for title/artist/position, flat transport buttons) — reimplemented
as native primitives, not by parsing or rendering the CSS itself.

---

## 4. The SPI bandwidth budget, stated as a real constraint

**`[SPEC-FBUI-040]` A full-panel redraw is the most expensive thing this
UI can do, and this design avoids doing it more than once per track.**
fbtft over SPI at the panel's rated speed (32 MHz per the original
hardware identification) has well-documented low full-frame refresh rates
— this is a property of the *bus*, not of how efficient the drawing code
is, and no amount of Rust performance work changes it. Consequences this
design commits to:

- The position/duration text and any progress bar update on a timer
  (matching the browser's own 500 ms-ish cadence is unnecessary here;
  once per second is enough for a number a person glances at, not reads
  continuously) and touch **only their own small screen region**, never
  the whole frame.
- Title/artist/album redraw **only on track change** — exactly what the
  diff rule above already limits this to.
- **Album art (`/art/:passage_id`) is the one open question, not a
  committed feature of v1.** A full-color bitmap blit is the single most
  expensive possible SPI operation this panel could be asked to do, and
  doing it once per track (not continuously) may still be worth the cost
  — but this is explicitly deferred to be measured against real hardware
  rather than assumed acceptable. See §7's open items.

---

## 5. Touch: raw evdev, calibrated by this project, not by X11/libinput

**`[SPEC-FBUI-045]` Reads the XPT2046 driver's raw evdev device directly**
(via the `evdev` crate or hand-rolled `ioctl`/`read` — a well-documented,
simple wire format), rather than depending on X11/libinput's own mature
calibration tooling — because there is no X11 or Wayland compositor in
this design at all; running one just to get its calibration UI would
reintroduce exactly the memory/complexity cost this whole design exists to
avoid.

**`[SPEC-FBUI-050]` A one-time (re-triggerable) calibration routine.**
Displays crosshair targets at known screen coordinates, records the raw
touch event at each, computes a linear affine transform (the standard
resistive-touchscreen calibration algorithm — well-established, not
invented here) from raw ADC space to screen pixels. Stored as a small flat
file, not a database table: this is a single-purpose, host-specific
setting with no relational structure to it, the same reasoning that keeps
`run-local.sh` a flat file rather than a config table.

**`[SPEC-FBUI-055]` Storage location follows the existing A/B/C write-
frequency design, not a new convention.** The calibration file is written
once (at calibration time) and read on every subsequent start — squarely
a C-partition (state) artifact by `[PI-PART-020]`'s own ordering
principle, alongside `listener.db`. Proposed path:
`/var/vaino/touch-calibration.toml`. The binary itself is an A-partition
(system) artifact, deployed and locked down exactly like `vaino` is.

---

## 6. Deployment: mapped onto the existing appliance model, not new

**`[SPEC-FBUI-060]`** A `systemd` unit alongside `vaino.service`,
`Restart=always` matching its pattern, but **not started or enabled until
the hardware bring-up from the parent conversation is independently
confirmed working** — bringing up a service that crash-loops against
hardware nobody has proven responds yet would just be noise obscuring the
actual bring-up signal. Runs as the same `pi` user `vaino` does (needs
`/dev/fb1` and the touch `/dev/input/eventN` device permissions, the usual
`video`/`input` group membership rather than root).

---

## 7. Reviewed for gaps before any code — real ones found

Asked of this design itself, the same discipline `[IMPL002]`'s own review
passes used, before treating this as ready to build:

- ~~**The hardware identity itself is still open**~~ **Resolved
  2026-09-07, per `[SPEC-FBUI-025]`** — was the single largest risk in
  this document when this review was written; struck through rather than
  deleted so this section's own history stays legible.
- **Non-ASCII text is a real gap, not a hypothetical one.** This library
  has real, non-ASCII artist/title names (checked against this project's
  own data, not assumed) — `embedded-graphics`'s bundled fonts are small
  fixed bitmap sets, typically ASCII-only. A v1 that silently drops or
  mangles a non-ASCII title is a real, known-in-advance defect, not an
  edge case to discover later. Needs a decision before `[SPEC-FBUI-030]`
  is implemented: a broader bitmap font (the `u8g2-fonts` crate covers
  much more of Unicode at a similar footprint to `embedded-graphics`'s own
  fonts) is the likely answer, not a real TTF rasterizer, which would
  reintroduce real memory/CPU cost this design is trying to avoid.
- **Reconnection behavior needs its own explicit design, not an
  afterthought.** `vaino` restarting (a crash, an update) must not leave
  this UI showing stale, confidently-wrong state — `[SPEC-FBUI-020]`'s
  diff-based redraw needs an explicit "no connection" state distinct from
  "connected and nothing changed," or a `vaino` restart could leave the
  display showing the last song it played, silently, indefinitely.
- **Touch hit-testing regions are not yet specified concretely** — this
  document describes the visual language and the data flow, not exact
  pixel rectangles for play/pause/skip/seek. That is real remaining design
  work, appropriately deferred until `[SPEC-FBUI-025]`'s hardware
  questions are answered (a hit-test grid designed against the wrong
  orientation or the wrong overlay's rotation convention is wasted work).
- **What happens before calibration has ever run** — first boot, no
  calibration file present. `[SPEC-FBUI-050]`'s routine needs to be the
  thing that runs automatically in that case, not a separate manual step
  someone has to remember, or the appliance's first real boot shows a
  screen that cannot be usefully touched.

None of these are reasons not to build this — they are exactly the kind
of gap this review step exists to find before code makes them expensive to
fix. Folded into §8's phasing below rather than left as loose ends.

---

## 8. Implementation plan, phased against the open items above

1. ~~Hardware bring-up~~ **Done, 2026-09-07** — `/dev/fb0` (`fb_ili9486`,
   480×320, RGB565) and `/dev/input/event2` (`ADS7846 Touchscreen`) both
   confirmed against real hardware, per `[SPEC-FBUI-025]`.
2. ~~A minimal `DrawTarget` + connection test~~ **Done, 2026-09-07** —
   `player/src/bin/fbui.rs` (feature-gated `fbui`, `bose`/`vainopi` builds
   never compile it), a `ClientSnapshot` naming only the fields this phase
   draws rather than making the real `Snapshot` `Deserialize` too
   (`[SPEC-FBUI-015]`'s "zero server-side changes" held in practice, not
   just on paper). Deployed and run as a real systemd service on
   `vainoplayer3` (`After=vaino.service`), stable, not crash-looping.
   Verified past "it didn't crash" — read `/dev/fb0` back after a render
   and counted actual pixel bytes: 496 pixels matching `LCD_GREEN`'s exact
   RGB565 encoding (real text was drawn, not garbage) against ~153,040
   matching `LCD_BG`'s (the whole-screen fill), out of 153,600 total. The
   `embedded-graphics` → RGB565 → byte-order → real-framebuffer pipeline
   works end to end. **Not yet confirmed by looking at the physical
   screen** — byte-level readback proves the pixels are correct, not that
   the panel displays them the way a person would expect (orientation,
   perceived color, refresh behavior all still want an actual look).
3. **Calibration routine**, run automatically when no calibration file
   exists — resolves §7's "first boot" gap by construction rather than by
   remembering a manual step.
4. **Transport UI**: play/pause/skip/volume/seek, hit-tested against
   concrete regions decided once §8.1's real orientation is known, wired
   to the existing `/command`/`/volume`/`/seek` routes.
5. **Reconnection handling**: an explicit "disconnected" render state,
   tested by actually killing and restarting `vaino` mid-session and
   confirming the display says so rather than freezing on stale data.
6. **Non-ASCII font decision**, resolved with real library data (titles
   this project's own catalog actually has) rather than a synthetic test
   string.
7. **Album art**, only after 1–6 are proven — measured against real SPI
   hardware (how long one full bitmap blit actually takes) before deciding
   whether it belongs in this UI at all.

Not started. This document is the plan; `[SPEC-FBUI-025]`'s hardware
question is the one thing every step after it actually depends on.

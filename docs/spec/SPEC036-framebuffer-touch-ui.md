# SPEC036: A native framebuffer/touch UI, for small SPI displays

**Design Specification — Tier 2 · Built, deployed, and running** — tagged
`testedInTruck`: confirmed functional in a real vehicle install (a 2019
Ram Classic Tradesman's center console, AUX-in to the factory head unit),
not just on a bench.

A second, native UI for Vaino, alongside the existing browser one — not a
replacement for it, and not built the same way. Motivated by a concrete
need: `vainoplayer3` (a Raspberry Pi 3B, first built `2026-09-07`) has a
480×320 SPI-driven touchscreen too small and too memory-constrained (1 GB
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
seen in `dmesg` (`start_line=319 is larger than end_line=0 ... will do
full display update`) — cosmetic, not a defect in this configuration, and
confirmed pre-existing rather than caused by any later phase: `dmesg -T`
timestamps put it at 19:38-19:40 on this boot, over an hour before album
art (§8 phase 7) was ever deployed. Only 7 total log lines exist despite
far more redraws than that having happened since boot, so the kernel is
clearly rate-limiting the *message*, not the underlying fallback -- it
likely fires on every `render()` call, given `render()` already rewrites
virtually the whole panel every time (§8 phase 7's own measurement). Its
visible symptom (observed 2026-09-07, once album art existed to make it
obvious): a brief whole-screen white cast on a large content change like
a track skip, resolving to the correct frame immediately after. Not
noticed on smaller changes (a position-bar tick) because old and new
frames look nearly identical against each other, not because the
underlying driver fallback fires less often for them. A kernel-driver-
level cosmetic artifact, not something fixable from this binary's own
code; `piscreen2r`'s untried `drm`/KMS alternative (next paragraph) is
the only real candidate for eliminating it, and has not been pursued.

This resolves §7's largest named risk. `piscreen2r`'s own KMS/`drm`-mode
alternative remains untried (FBTFT already works, no forcing reason to);
byte-order is now confirmed too, by §8 phase 2's pixel-readback test, but
*perceived* correctness on the physical panel — orientation, color, real
refresh behavior — still wants an actual look, not just measured bytes,
**since confirmed directly**: real green text on a black background, seen
on the physical unit.

**`[SPEC-FBUI-027]` The kernel's own framebuffer console shares this
device, and must be tamed rather than removed.** Found the hard way, not
designed for up front: `/dev/fb0` is also Linux's text console (`fbcon`),
because `cmdline.txt` binds `console=tty1` to it. Removing that binding
looked like the obvious fix for a login-prompt cursor stomping on
`fbui`'s output — instead it hung `vainoplayer3` at boot, reproducibly,
three times in a row, with no filesystem corruption on either the ext4
`SYSTEM` partition (`e2fsck`-clean) or the f2fs `STATE` partition (full
read-only check passed, checkpoint recorded a proper `unmount`) to explain
it — something in this image's boot sequence depends on a VT console
existing at all, even an unused one. `console=tty1` stays. The two actual
symptoms it caused are instead solved without touching boot config:

- The login prompt itself: `getty@tty1.service` disabled (`systemctl
  disable --now`) — confirmed durable by finding no unit symlink at all
  under `/etc/systemd/system` after a reboot, not just believing the
  command's own output.
- `fbcon`'s VT cursor, which keeps blinking even with no getty attached
  (a property of the console layer itself, not of whatever reads from
  it): turned off live with `TERM=linux setterm -cursor off > /dev/tty1`,
  made durable via `fbui.service`'s own `ExecStartPre` (a `+`-prefixed
  line, since `/dev/tty1` isn't writable by the `pi` user the rest of the
  unit runs as) — tying the fix to the unit that actually owns the
  display, rather than a separate boot-order-dependent script.

One narrow, cosmetic residual: for the few seconds between `fbcon`
claiming the console during boot and `fbui`'s own first paint, boot text
can in principle still flash on screen once. Self-heals immediately
(`fbui` unconditionally repaints the whole panel on start) and has not
needed chasing further.

**`[SPEC-FBUI-030]` Drawing: `embedded-graphics`, not a from-scratch
rasterizer.** A mature, widely-used Rust crate for exactly this class of
target (small framebuffers, primitive shapes, bitmap fonts), with existing
Linux-framebuffer `DrawTarget` glue in its ecosystem. Pulling it in is a
real new dependency — weighed against writing equivalent rectangle/text/
line primitives by hand, which is not obviously cheaper once font
rendering is included, and is more likely to have an undiscovered bug in
exactly the pixel-format-handling code this design most needs to be
right.

**`[SPEC-FBUI-035]` Visual language borrows directly from an existing web
skin, not from scratch — originally WinAmp, since revised into two
themes, one per page.** Found while grounding this spec, not assumed:
`web/skins/winamp/skin.css`'s chassis is a fixed **320px** wide — which is
not a coincidence to route around but a near-exact match to this panel's
own 320-pixel dimension. Same palette (`#3d3d47` chassis, `#0b0f0b` LCD
background, `#22dd22` LCD green), same idiom (a small LCD readout for
title/artist/position, flat transport buttons) — reimplemented as native
primitives, not by parsing or rendering the CSS itself. This is still
Settings' own theme entirely, and remains the gear icon's on both pages.

**Now Playing re-themed to MuLibPlay on 2026-09-08**, on direct request:
`web/skins/mulibplay/skin.css`'s own real, measured palette (`#000000`
"black ground", `#cccccc` text, `#2010a0` filled buttons, `#d0a000` gold
for whichever programme is active, `#7ab7ff` progress fill) rather than
WinAmp green, once the button grid grew from four transport buttons to
ten (two rows of five: play/pause, skip, all 8 programmes — §9). Filled
and rounded rather than outlined, the same simplification a low-resolution
rendering of that skin needs regardless of this project's own reasons for
it (a thin outline stroke does not read clearly at 480×320): confirmed
directly, not assumed, after an outlined first attempt read as illegible
on the physical panel. The literal hex values from the desktop skin
needed further darkening beyond that for this specific panel's own
real, measured low contrast, and button labels moved to a bold ASCII
font — both again confirmed by looking at the physical screen rather than
trusting the numbers on paper. See §9 for the full account.

---

## 4. The SPI bandwidth budget, stated as a real constraint

**`[SPEC-FBUI-040]` A full-panel redraw is the most expensive thing this
UI can do, and this design avoids doing it more than necessary.** fbtft
over SPI — originally assumed to matter at the bus's rated 32 MHz, but
this design ended up committing to full-panel redraws anyway (below), so
the number that actually governs refresh cost is whatever
`piscreen2r`'s `speed=` is configured to, not a fixed original figure.
**Lowered to 16 MHz on 2026-09-08**, after real vehicle use surfaced
intermittent color corruption (a "white-washed, almost inverted" flicker,
worse on large content changes like a track skip) consistent with SPI
signal-integrity trouble at the higher rate on this board's actual
wiring — confirmed as the fix by direct observation before/after, not
just plausible reasoning. Refresh is visibly slower as a result; nothing
about this UI's own usage pattern (an update every few seconds at most,
never continuous motion) needs the faster rate back.

**Region-limited redraw was the original plan; full-panel redraw is what
was actually built,** once §8 phase 2 measured the real cost of the
alternative: `render()`'s own CPU-side work stayed under 20ms even
redrawing the *entire* 480×320 canvas on every call, an order of
magnitude below anything a person would perceive as lag. Given that,
tracking exactly which sub-regions changed and redrawing only those
would have added real code and real risk (partial-redraw bugs are a
classic source of stale-looking UI) for a saving that never mattered in
practice. What *is* still true from the original plan:

- The position/duration text and progress bar update on a timer — not
  every push (matching the browser's ~500ms cadence is unnecessary here),
  throttled instead to **once per 5 seconds** for a position-only change
  (`differs_ignoring_position`, added once queue/programme/volume state
  gave the diff rule more to ignore); anything else still redraws the
  full panel at once.
- Title/artist/album still only actually *change* on track change — the
  diff rule's job, not a region-redraw's.
- **Album art was the one open question, and is no longer one.** §8
  phase 7 measured it and built it: a full-color bitmap blit adds a
  small, bounded cost to the full-panel redraw this design already pays
  for on every real change, not a new separate expensive operation.

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
- ~~**What happens before calibration has ever run**~~ **Resolved
  2026-09-07** — `fbui` checks for `[SPEC-FBUI-055]`'s file at startup and
  runs `[SPEC-FBUI-050]`'s routine automatically when it's missing; no
  separate manual step exists to forget.

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
   works end to end. **Confirmed by looking at the physical screen**, not
   just by byte-level readback: real green text on black, as designed.
   Getting there also surfaced and resolved `[SPEC-FBUI-027]` (the console
   shares this device) — a real boot-hang risk found and fixed along the
   way, not merely a cosmetic finish.
3. ~~Calibration routine~~ **Done, 2026-09-07** — the standard 3-point
   affine solve (`[SPEC-FBUI-050]`, Vidales' algorithm, the same one
   tslib/X11 evtouch use), raw evdev hand-parsed rather than a new crate
   (`[SPEC-FBUI-045]`). The math is unit-tested against a synthetic
   rotated/scaled transform (recovers a held-out 4th point exactly, the
   check the calibration literature itself recommends) independent of any
   hardware, then run for real: three genuine touches on `vainoplayer3`
   produced distinct, internally-consistent raw readings (raw_x fell as
   screen_x rose, raw_y rose sharply with screen_y — the axis remix
   `rotate=90` predicts) and a saved `/var/vaino/touch-calibration.toml`
   with finite, sane coefficients. Confirmed re-triggerable (`fbui
   --calibrate`) and confirmed idempotent the other way too — a plain
   restart finds the file and logs `using existing calibration` rather
   than re-prompting.
4. ~~Transport UI~~ **Done, 2026-09-07, mostly confirmed** — four
   buttons (`VOL-`, play/pause, skip, `VOL+`) plus a tap-to-seek position
   bar, hit-tested against concrete pixel regions now that §8.1's
   orientation is known. Wired to the exact three real command names
   `control.rs` serves — checked against the handler itself rather than
   assumed: `play`, `pause`, `skip` (there is deliberately no `prev` or
   `stop`, `[REQ-AUD-142]`) — plus `/volume/:db` (±3dB per tap, computed
   from the last known `volume_db`) and `/seek/:ms`. Posted over a
   hand-rolled HTTP/1.1 request on a raw `TcpStream` rather than a new
   client crate, since `reqwest` is explicitly appliance-excluded
   (`Cargo.toml`'s own `sampo-support` reasoning) and `hyper`'s client
   builder would be more code than three fire-and-forget local requests
   need. Touch runs on its own OS thread, joined with the websocket
   stream via `tokio::select!` — physically confirmed working: skip,
   volume, and play/pause all produced their real effect when tapped on
   `vainoplayer3`. **Seek is not yet confirmed** — this library is 31
   radio passages, all `duration_ms == 0`, and the code correctly declines
   to send a seek for those (`if duration_ms > 0`), so a tap there is
   correctly inert but untested against real seekable content. Left open
   rather than claimed.
5. ~~Reconnection handling~~ **Done, 2026-09-07** — `render_disconnected`
   replaces `render`'s old "connect before first try" placeholder (which
   only covered the first-ever connection, not a later drop) and now fires
   on every disconnect, with `last` reset to `None` alongside it. That
   reset is the actual fix, not the message alone: without it, a `vaino`
   restart that happened to come back with an identical snapshot would
   fail `[SPEC-FBUI-020]`'s diff check and leave the disconnected screen
   up forever despite real data flowing again. Tested exactly as planned
   — `systemctl restart vaino.service` mid-session on `vainoplayer3` — and
   confirmed by framebuffer readback, not just log lines: the green-pixel
   count dropped from 7,186 (full transport UI) to 508 (the two-line
   disconnected message, no buttons) within one push interval of the
   restart, then returned to 7,287 within `fbui`'s own 3-second retry once
   `vaino` was back.
6. ~~Non-ASCII font decision~~ **Done, 2026-09-07** — resolved with real
   library data, not a synthetic test string: queried a production
   `vaino.db` for actual non-ASCII artist/title text and found accented
   Latin (`Björk`, `Mötley Crüe`, `Fernando Mendonça`) far outnumbered by
   curly quotes/apostrophes and Unicode dashes (`Guns N'Roses`, `The
   Go‐Go's`) — checked, not assumed. `u8g2_font_9x15_t_symbols`
   (`[SPEC-FBUI-030]`, revised) covers the first group directly, confirmed
   by actually calling `render()` against it rather than trusting the
   font's name; `normalize_for_display` maps the second, measured group to
   plain ASCII. Both paths pinned down as a permanent regression test
   (`tests::font_coverage_matches_real_library_data`) and confirmed live:
   a real production title/artist pair temporarily substituted into
   `vainoplayer3`'s currently-playing passage, rendered with no errors in
   `fbui`'s log, and visually confirmed legible on the physical screen —
   not just proven not to crash.
7. ~~Album art~~ **Done, 2026-09-07.** The measurement came first: a raw
   write of a full 307,200-byte frame into `/dev/fb0`'s mmap takes ~0.09ms
   (negligible), `render()`'s own CPU-side cost stayed under 20ms across
   every real redraw during normal playback, and the SPI transfer itself
   is governed by `piscreen2r`'s already-configured `fps=20` (~50ms/frame)
   independent of this binary — since `render()` already touches virtually
   every pixel on every call, a thumbnail adds a small bounded cost to an
   already-paid full-panel redraw, not a new expensive operation. Built on
   that basis: `image` (default features off, `jpeg`+`png` only — real
   production art is never anything else, per `media_type_for` in
   `tags.rs`) decodes whatever `GET /art/:passage_id` returns, resized
   once per passage change to a fixed 100x100 buffer (doubled from an
   initial 50x50 after seeing it on the physical screen), blitted via
   `FbDisplay::put_pixel` directly. `passage_id` added to `ClientSnapshot`
   for exactly the purpose the real `Snapshot` already documents it for.
   Fetching uses a second hand-rolled client, `http_get` (a
   `Connection: close` GET, read to EOF — simpler than parsing
   `Content-Length` for a loopback request to a server this project
   controls), keyed and cached per `passage_id` so a passage confirmed to
   have no art (most of this all-radio sample library) is not re-requested
   on every push. Confirmed against real hardware, not just compiled: a
   real 29KB JPEG fetched, decoded, and resized in 32.6ms on
   `vainoplayer3`; 1,373 distinct colors read back from the 100x100 art
   region (real photo content, not a solid fill); and confirmed visually
   on the physical screen twice, once at each size.
   One real, pre-existing cosmetic artifact this made newly *visible*
   without being its cause, run down before deciding not to pursue it
   further: `dmesg -T` shows the well-known
   `fbtft`/`fb_ili9486` `start_line=319 is larger than end_line=0` full-
   display-update fallback (`[SPEC-FBUI-025]`'s own §3 already named it
   "cosmetic") firing at 19:38-19:40 on this boot -- over an hour before
   album art was ever deployed, so it predates and is not caused by this
   phase. It likely fires on every `render()` call (only 7 log lines exist
   total despite far more redraws than that, so the kernel is rate-
   limiting the *message*, not the fallback), but only became visible to
   the eye -- a brief whole-screen white cast on a large content change
   like a skip, resolving immediately after -- once album art gave a track
   change a large, high-contrast frame to visibly differ from. Not pursued
   further per explicit instruction; `piscreen2r`'s untried `drm`/KMS mode
   remains the one real candidate for eliminating it, should it matter later.

Phases 1–7 done and verified against real hardware, per each entry above.
This document's original seven-phase plan is complete; §9 covers real
work that came after it.

---

## 9. Beyond the seven-phase plan

The original plan closed with a single-page, WinAmp-themed transport UI.
Real use — including the vehicle install itself — asked for more, all
done and verified against real `vainoplayer3` hardware between
2026-09-07 and 2026-09-08:

- **Two pages, not one.** A gear icon (top-right, both pages, drawn last
  so its own backdrop always wins over whatever text runs underneath it)
  toggles between Now Playing and a new Settings page. Splitting was
  necessary, not stylistic: volume control, all 8 programmes, and
  per-track queue editing do not fit one 480×320 screen alongside album
  art and transport controls.
- **A 2×5 button grid**: play/pause, skip, then all 8 of `SPEC009`'s own
  programmes, the currently active one highlighted (a filled gold button
  rather than an outline — "which programme is active" has no other
  indicator on this screen). Never sends `auto`: this appliance has
  no realtime clock and a truck's driving hours have no relationship to a
  schedule tuned for home listening, so only a specific programme id is
  ever posted to `/program/:id`. Persistence for this was a real,
  necessary backend fix, not a UI-only change — `[SPEC-DIR-185]`,
  `SPEC009` §6.
- **Settings**: volume +/− (moved off Now Playing to make room for the
  programme grid) and the upcoming queue, each entry with the same
  sooner/remove/later actions `/queue/:qid/:action` already serves every
  browser skin — found already built and reused, not a new server-side
  route.
- **Album name.** A third line under title/artist, from the real
  `Snapshot`'s own `album` field, blank rather than a placeholder string
  when unknown.
- **MuLibPlay re-theme, contrast, and weight** — `[SPEC-FBUI-035]` above
  has the full account: darker button fill and progress-bar track than
  the desktop skin's own literal values, a bold ASCII font for button
  labels (`Cool`, `Fun`, `Mellow`, `Soft`, `Prog`, `Light`, `Loud`,
  `Groove` — `vainoplayer3`'s own real programme names, not synthetic
  ones), both fixes made only after the first attempt was reported too
  low-contrast to read on the physical panel.
- **Album art's resize filter was a real, found bug, not a style
  choice.** `image`'s `FilterType::Triangle` blends source pixels in
  their stored gamma-encoded (sRGB) values rather than in linear light,
  which systematically biases a large downscale brighter than correct —
  reported as art looking "washed out, too white." Real production art
  is confirmed plain JPEG (ruling out a PNG-alpha explanation), and this
  UI's own flat-filled colors are never interpolated at all, so a
  resize-blending bias was the one thing left that explained it
  happening only on album art. Switched to `FilterType::Nearest`, which
  picks one source pixel untouched per output pixel and so cannot be
  biased by a blend that never happens — a real fix, not a full
  correct-and-proper linear-light resize, but the right-sized one for a
  100×100 thumbnail on a panel this coarse. A residual, panel-hardware
  viewing-angle washout remains and is not a software defect.
- **`/dev/fb0` and `/dev/input/event2` are not fixed paths anymore.**
  Disabling `vc4-kms-v3d` for boot speed (`[REQ-HW-100]`'s audio-first
  priority, next bullet) removed the hand-off that used to keep the
  legacy firmware framebuffer (`BCM2708 FB`, 32bpp) from ever claiming a
  device node at all — it now claims `fb0` first on boot, bumping the
  real panel to `fb1`, and evdev's own enumeration order shifted the
  touchscreen from `event2` to `event0` the same way. A fixed path had
  already broken once from a config change this file had no part in;
  `FbDisplay::find_panel_path`/`find_touch_path` now find both by their
  real driver/device name (`fb_ili9486`, `ADS7846 Touchscreen`) instead,
  surviving the next such shift rather than breaking on it again.
- **Boot-to-audio time, halved.** Prompted directly: "is vainoplayer3's
  boot sequence optimized to reach audio playout as soon as possible,"
  answered honestly (no) and then fixed, in priority order the appliance
  actually needs (audio, then touch display, then network; HDMI never).
  Real measurement, not estimation: ~28.5s power-on-to-audio before,
  ~12.4s after. Two independent fixes, not one:
  - **OS boot**: `cloud-init` (its one-time job long done, still costing
    5+ seconds every boot for nothing) and Bluetooth (unused on this
    install) disabled; `vc4-kms-v3d` disabled (HDMI never used, and it
    was actively probing for a display every boot regardless).
    `fsck.repair=yes` was deliberately left alone — trading this
    appliance's own power-interruption protection for boot speed is not
    a trade worth making.
  - **`vaino`'s own startup**: `Session::open()` was loading the Program
    Director's full flavor index (578,523 rows) synchronously before
    playback could resume, measured at ~13-15s, even though resuming an
    already-known passage needs no selection at all. Moved onto the same
    background-rebuild machinery `[SPEC-DIR-185]`'s persistence fix
    already reuses for a live `/library/reload` — including its existing
    SD-card-contention-aware queue-depth gate, not a new one invented for
    this. Selection runs on the pre-existing frequency-only fallback
    (the same one a library with no Director tables at all already used)
    for the few seconds until the background build lands.

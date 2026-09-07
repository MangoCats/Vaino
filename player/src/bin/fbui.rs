//! A native framebuffer UI for small SPI touchscreens `[SPEC036]`.
//!
//! Phase 2 of that spec's plan: prove the pixel format and orientation
//! assumptions against real hardware before building anything else on top
//! of them. This binary does exactly one thing -- connect to `vaino`'s
//! *existing* `/ws`, the same push every browser skin already reads, and
//! draw the title/artist it carries onto the confirmed `/dev/fb0`
//! (`fb_ili9486`, 480x320, RGB565 -- `[SPEC-FBUI-025]`, measured against
//! `vainoplayer3` directly, not assumed).
//!
//! Deliberately not the real `Snapshot` from `web::mod` -- that type is
//! `Serialize`-only (the server never deserializes its own push), and
//! giving it `Deserialize` too would be a change to code this design's
//! whole premise is to leave untouched `[SPEC-FBUI-015]`. `ClientSnapshot`
//! below names only the handful of fields this phase actually draws;
//! serde ignores every field it doesn't ask for, so the real `Snapshot`
//! growing a field is not a breaking change here.
//!
//! Phase 3 adds touch calibration `[SPEC-FBUI-050]`: reads the ADS7846
//! touch device's raw evdev stream directly `[SPEC-FBUI-045]` (a
//! hand-rolled 24-byte `struct input_event` parse -- the documented,
//! stable ABI on 64-bit Linux -- rather than pulling in an `evdev` crate
//! for a format this small), walks the person through three on-screen
//! targets, and solves the standard 3-point affine calibration (Vidales'
//! algorithm: the same one tslib/X11 evtouch use) rather than assuming
//! any particular rotation or axis convention. That matters here because
//! `piscreen2r`'s `rotate=90` is applied by the *display* driver; the
//! touch controller has no idea it happened, so an assumed mapping would
//! have to hardcode this panel's specific rotation. An empirical
//! screen-to-raw correspondence absorbs whatever rotation, mirroring, or
//! skew actually exists between them, without needing to know what it is.
//!
//! Phase 4 wires that calibration to an actual transport UI: play/pause,
//! skip, +-3dB volume, and tap-to-seek on the position bar, hit-tested
//! against the concrete regions `render` draws and posted to `vaino`'s
//! existing `/command/:name`, `/volume/:db`, `/seek/:ms` routes
//! `[SPEC-FBUI-015]` -- the exact three real command names `control.rs`
//! serves (`play`, `pause`, `skip`; there is deliberately no "prev" or
//! "stop" `[REQ-AUD-142]`), not assumed ones. Touch is read on its own OS
//! thread and joined with the websocket stream via `tokio::select!` in
//! `main`, rather than sharing `fbui`'s single async worker thread with a
//! blocking evdev read.
//!
//! Phase 5 fixes the reconnection gap `[SPEC036]` §7 named: every drop
//! (not just the first-ever connect) now shows `render_disconnected` and
//! resets the held snapshot, so an identical post-restart push can't be
//! mistaken by `[SPEC-FBUI-020]`'s diff check for "nothing changed."
//!
//! Phase 6 replaces `embedded-graphics`'s bundled ASCII-only fonts with
//! `u8g2-fonts`' `u8g2_font_9x15_t_symbols` -- chosen by measuring real
//! titles/artists (queried from a production `vaino.db`, not invented):
//! it covers the accented Latin that shows up constantly ("Bj\u{f6}rk",
//! "Mendon\u{e7}a"), confirmed by actually calling `render()` against it
//! rather than trusting the font name (`tests::font_coverage_matches_real_library_data`).
//! It does *not* cover the curly quotes/apostrophes and Unicode hyphens
//! that turn out to be the single most common non-ASCII character in this
//! library ("Guns N\u{2019}Roses", "The Go\u{2010}Go\u{2019}s") -- no
//! bitmap font checked carried both without jumping to `unifont`'s much
//! larger, uglier glyphs for a whole library's worth of tracks that
//! mostly don't need it. `normalize_for_display` maps that specific,
//! measured set to its plain-ASCII look-alike instead: the semantic
//! content of a title is unaffected by a curly apostrophe rendering as a
//! straight one.
//!
//!     fbui [--calibrate] [ws://host:port/ws]   (default: ws://127.0.0.1:5720/ws)

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use framebuffer::Framebuffer;
use futures_util::StreamExt;
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};
use u8g2_fonts::FontRenderer;

/// Only the fields this phase draws. Unknown incoming fields are ignored by
/// serde automatically -- the real `Snapshot` carries dozens more.
#[derive(serde::Deserialize, Default, Clone, PartialEq)]
struct ClientSnapshot {
    playing: bool,
    title: Option<String>,
    artist: Option<String>,
    position_ms: u64,
    duration_ms: u64,
    volume_db: f32,
}

/// Wraps the real Linux framebuffer device so `embedded-graphics` can draw
/// into it directly. Reads the panel's *actual* reported geometry via the
/// standard `FBIOGET_VSCREENINFO`/`FBIOGET_FSCREENINFO` ioctls (what the
/// `framebuffer` crate does internally) rather than hardcoding the
/// 480x320/RGB565 this project already measured -- a future config change
/// to `piscreen2r`'s params should not silently desync from a number typed
/// into this file.
struct FbDisplay {
    fb: Framebuffer,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
}

impl FbDisplay {
    fn open(path: &str) -> Result<Self, String> {
        let fb = Framebuffer::new(path).map_err(|e| format!("open {path}: {e:?}"))?;
        let width = fb.var_screen_info.xres;
        let height = fb.var_screen_info.yres;
        let bits_per_pixel = fb.var_screen_info.bits_per_pixel;
        if bits_per_pixel != 16 {
            // This design's whole pixel-writing path below assumes RGB565
            // (2 bytes/pixel) because that is what was actually measured
            // against vainoplayer3's hardware [SPEC-FBUI-025]. A different
            // bit depth here means a different overlay or a different
            // config than what this was built and tested against -- refuse
            // rather than write pixels in the wrong format silently.
            return Err(format!(
                "expected 16bpp (RGB565), got {bits_per_pixel}bpp -- this binary's pixel \
                 writer was built against the confirmed vainoplayer3 config and does not \
                 know how to draw into this format"
            ));
        }
        Ok(Self { fb, width, height, bytes_per_pixel: (bits_per_pixel / 8) as u32 })
    }

    fn put_pixel(&mut self, x: u32, y: u32, color: Rgb565) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = ((y * self.width + x) * self.bytes_per_pixel) as usize;
        // RGB565, little-endian -- the standard fbdev byte order for this
        // depth. Confirmed correct by physically looking at the panel
        // `[SPEC-FBUI-027]`: real green text on black, as designed.
        let raw = color.into_storage();
        let bytes = raw.to_le_bytes();
        self.fb.frame[offset] = bytes[0];
        self.fb.frame[offset + 1] = bytes[1];
    }
}

impl OriginDimensions for FbDisplay {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for FbDisplay {
    type Color = Rgb565;
    type Error = std::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x >= 0 && point.y >= 0 {
                self.put_pixel(point.x as u32, point.y as u32, color);
            }
        }
        Ok(())
    }
}

/// The WinAmp-derived palette `[SPEC-FBUI-035]` -- same values as
/// `web/skins/winamp/skin.css`'s `--lcd-bg`/`--lcd`, so the two UIs read as
/// the same player rather than coincidentally similar ones.
const LCD_BG: Rgb565 = Rgb565::new(1, 3, 1); // ~#0b0f0b at 5/6/5 bit depth
const LCD_GREEN: Rgb565 = Rgb565::new(4, 59, 4); // ~#22dd22

/// `[SPEC-FBUI-030]`, revised for Phase 6: `embedded-graphics`'s own
/// bundled fonts are ASCII-only, which real library data (not a synthetic
/// test string) shows is not enough. `u8g2_font_9x15_t_symbols` matches
/// this file's original 9x15 glyph size and covers the accented Latin
/// that appears constantly in real titles/artists -- checked by actually
/// rendering, not by trusting the font's name
/// (`tests::font_coverage_matches_real_library_data`).
const TEXT_FONT: FontRenderer = FontRenderer::new::<u8g2_fonts::fonts::u8g2_font_9x15_t_symbols>();

/// Maps the specific, measured set of punctuation this library's titles
/// use that `TEXT_FONT` does not carry a glyph for -- curly quotes and
/// apostrophes, and every dash-like character in Unicode's General
/// Punctuation block, plus the Hawaiian ʻokina (a modifier letter, not a
/// quote mark, but visually and phonetically closest to one) -- to their
/// plain-ASCII look-alikes. A title's meaning survives a curly apostrophe
/// rendering as a straight one; a missing-glyph box or a render error
/// would be the actually-wrong outcome here.
fn normalize_for_display(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{02bb}' | '\u{02bc}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            '\u{2010}'..='\u{2015}' => '-',
            other => other,
        })
        .collect()
}

/// Renders one line of already-`normalize_for_display`-cleaned text at a
/// given left/baseline anchor. Errors are logged, not propagated --
/// `FbDisplay`'s own `DrawTarget::Error` is `Infallible`, so the only way
/// this can fail is a glyph `TEXT_FONT` genuinely lacks; that should be
/// caught by `normalize_for_display` and the coverage test before it ever
/// reaches here, so surfacing it loudly if it somehow doesn't is more
/// useful than a silent `.unwrap_or(())`.
fn draw_text(display: &mut FbDisplay, text: &str, x: i32, y: i32) {
    if let Err(e) = TEXT_FONT.render(text, Point::new(x, y), VerticalPosition::Baseline, FontColor::Transparent(LCD_GREEN), display) {
        eprintln!("fbui: could not render {text:?}: {e:?}");
    }
}

fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

// ---------------------------------------------------------------------
// Transport UI `[SPEC036]` §8 phase 4: play/pause/skip/volume/seek, hit-
// tested against concrete regions now that orientation is confirmed
// `[SPEC-FBUI-025]` rather than guessed at.
// ---------------------------------------------------------------------

const SEEKBAR_Y0: i32 = 58;
const SEEKBAR_Y1: i32 = 76;
const POS_TEXT_Y: i32 = 96;
const BUTTON_Y0: i32 = 112;
const BUTTON_Y1: i32 = 270;
/// Four buttons, evenly spread across the panel's 480px width with visible
/// gaps between them -- sized well above a fingertip's real contact area,
/// not just legible text, since this is what a person actually presses.
const BUTTON_XS: [(i32, i32); 4] = [(0, 110), (123, 233), (246, 356), (369, 479)];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Zone {
    VolDown,
    PlayPause,
    Skip,
    VolUp,
    Seek,
}

/// Maps an already-calibrated screen point to whichever control (if any)
/// it lands on. Takes screen coordinates, not raw touch ADC values --
/// calibration `[SPEC-FBUI-050]` is applied by the caller first, so this
/// function never needs to know this panel's rotation or axis convention.
fn hit_test(sx: f64, sy: f64) -> Option<Zone> {
    let (x, y) = (sx as i32, sy as i32);
    if (SEEKBAR_Y0..=SEEKBAR_Y1).contains(&y) {
        return Some(Zone::Seek);
    }
    if (BUTTON_Y0..=BUTTON_Y1).contains(&y) {
        for (i, &(x0, x1)) in BUTTON_XS.iter().enumerate() {
            if x >= x0 && x <= x1 {
                return Some([Zone::VolDown, Zone::PlayPause, Zone::Skip, Zone::VolUp][i]);
            }
        }
    }
    None
}

fn draw_button(display: &mut FbDisplay, x0: i32, x1: i32, label: &str) -> Result<(), std::convert::Infallible> {
    Rectangle::new(Point::new(x0, BUTTON_Y0), Size::new((x1 - x0) as u32, (BUTTON_Y1 - BUTTON_Y0) as u32))
        .into_styled(PrimitiveStyle::with_stroke(LCD_GREEN, 2))
        .draw(display)?;
    let center = Point::new((x0 + x1) / 2, (BUTTON_Y0 + BUTTON_Y1) / 2);
    if let Err(e) = TEXT_FONT.render_aligned(
        label,
        center,
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(LCD_GREEN),
        display,
    ) {
        eprintln!("fbui: could not render button {label:?}: {e:?}");
    }
    Ok(())
}

/// Clears the LCD panel area and redraws everything -- title/artist, the
/// tap-to-seek progress bar, position/volume, and the four transport
/// buttons. The whole-region redraw `[SPEC-FBUI-020]` says to do only on
/// the fields that actually changed, which `main`'s own diff against the
/// last state already guarantees by only calling this when something did.
fn render(display: &mut FbDisplay, snap: &ClientSnapshot) -> Result<(), std::convert::Infallible> {
    let w = display.width as i32;
    Rectangle::new(Point::zero(), Size::new(display.width, display.height))
        .into_styled(PrimitiveStyle::with_fill(LCD_BG))
        .draw(display)?;

    let title = snap.title.as_deref().map(normalize_for_display);
    let artist = snap.artist.as_deref().map(normalize_for_display);
    draw_text(display, title.as_deref().unwrap_or("(nothing playing)"), 8, 20);
    draw_text(display, artist.as_deref().unwrap_or(""), 8, 40);

    let (bar_x0, bar_x1) = (8, w - 8);
    Rectangle::new(
        Point::new(bar_x0, SEEKBAR_Y0),
        Size::new((bar_x1 - bar_x0) as u32, (SEEKBAR_Y1 - SEEKBAR_Y0) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(LCD_GREEN, 1))
    .draw(display)?;
    if snap.duration_ms > 0 {
        let frac = (snap.position_ms as f64 / snap.duration_ms as f64).clamp(0.0, 1.0);
        let fill_w = (((bar_x1 - bar_x0 - 2) as f64) * frac).round() as u32;
        if fill_w > 0 {
            Rectangle::new(
                Point::new(bar_x0 + 1, SEEKBAR_Y0 + 1),
                Size::new(fill_w, (SEEKBAR_Y1 - SEEKBAR_Y0 - 2) as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(LCD_GREEN))
            .draw(display)?;
        }
    }

    let pos = format!(
        "{} {} / {}  {:+.0}dB",
        if snap.playing { ">" } else { "||" },
        fmt_time(snap.position_ms),
        fmt_time(snap.duration_ms),
        snap.volume_db,
    );
    draw_text(display, &pos, 8, POS_TEXT_Y);

    draw_button(display, BUTTON_XS[0].0, BUTTON_XS[0].1, "VOL-")?;
    draw_button(display, BUTTON_XS[1].0, BUTTON_XS[1].1, if snap.playing { "PAUSE" } else { "PLAY" })?;
    draw_button(display, BUTTON_XS[2].0, BUTTON_XS[2].1, "SKIP")?;
    draw_button(display, BUTTON_XS[3].0, BUTTON_XS[3].1, "VOL+")?;
    Ok(())
}

/// An explicit, unmistakably different screen for "not connected to
/// `vaino` right now" `[SPEC036]` §7's reconnection gap -- no buttons, no
/// seek bar, none of `render`'s controls that would be silently inert if
/// tapped while there is nothing to send them to. Deliberately not just
/// `render` called with a default/empty `ClientSnapshot`: that would read
/// as "connected, and nothing happens to be playing," which is a real,
/// different state a person could otherwise not tell apart from this one.
fn render_disconnected(display: &mut FbDisplay) -> Result<(), std::convert::Infallible> {
    let (w, h) = (display.width as i32, display.height as i32);
    Rectangle::new(Point::zero(), Size::new(display.width, display.height))
        .into_styled(PrimitiveStyle::with_fill(LCD_BG))
        .draw(display)?;
    let center_x = w / 2;
    for (line, y) in [("-- disconnected --", h / 2 - 12), ("reconnecting...", h / 2 + 12)] {
        if let Err(e) = TEXT_FONT.render_aligned(
            line,
            Point::new(center_x, y),
            VerticalPosition::Baseline,
            HorizontalAlignment::Center,
            FontColor::Transparent(LCD_GREEN),
            display,
        ) {
            eprintln!("fbui: could not render {line:?}: {e:?}");
        }
    }
    Ok(())
}

/// Fire-and-forget POST to `vaino`'s existing control API `[SPEC-FBUI-015]`
/// -- a hand-rolled HTTP/1.1 request over a raw TCP socket rather than a
/// client crate. `hyper` is already in this workspace's dependency tree
/// (`axum` pulls it in), but wiring its client builder for three
/// fire-and-forget local requests is more code than the lines below, for
/// no behavior this UI needs -- same "every dependency is a memory
/// decision" reasoning `Cargo.toml` already states for `reqwest`.
async fn http_post(addr: String, path: String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = match tokio::net::TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fbui: {path} failed to connect to {addr}: {e}");
            return;
        }
    };
    let req = format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    if let Err(e) = stream.write_all(req.as_bytes()).await {
        eprintln!("fbui: {path} failed to send: {e}");
        return;
    }
    let mut buf = [0u8; 32];
    let _ = stream.read(&mut buf).await; // drain enough for a clean close; response body unused
}

/// `ws://host:port/ws` -> `host:port`, so touch commands go to the same
/// `vaino` this UI's websocket is already talking to, without a second
/// address to keep in sync by hand.
fn http_addr_from_ws_url(ws_url: &str) -> String {
    let after_scheme = ws_url.split_once("://").map(|(_, rest)| rest).unwrap_or(ws_url);
    after_scheme.split('/').next().unwrap_or(after_scheme).to_string()
}

// ---------------------------------------------------------------------
// Touch `[SPEC-FBUI-045]`: raw evdev, hand-parsed rather than a new crate.
// ---------------------------------------------------------------------

/// Confirmed against real hardware -- `ADS7846 Touchscreen` enumerates
/// here on `vainoplayer3` (`[SPEC-FBUI-025]`'s own touch confirmation).
const TOUCH_DEVICE: &str = "/dev/input/event2";

/// Where the calibration this module produces gets stored -- a C-partition
/// (state) artifact by `[SPEC-FBUI-055]`'s own reasoning, alongside
/// `listener.db`.
const CALIBRATION_PATH: &str = "/var/vaino/touch-calibration.toml";

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const BTN_TOUCH: u16 = 0x14a;

struct RawEvent {
    ev_type: u16,
    code: u16,
    value: i32,
}

/// `struct input_event` on 64-bit Linux is a fixed, documented 24-byte
/// layout: `struct timeval` (two `i64`s here, 16 bytes) followed by
/// `u16 type, u16 code, i32 value` (8 bytes). Parsed field-by-field from
/// raw bytes rather than an `unsafe` struct transmute -- same ABI, no
/// reliance on repr/padding guesses holding on whatever target this
/// cross-compiles for next.
fn read_event(file: &mut std::fs::File) -> std::io::Result<RawEvent> {
    use std::io::Read;
    let mut buf = [0u8; 24];
    file.read_exact(&mut buf)?;
    Ok(RawEvent {
        ev_type: u16::from_ne_bytes([buf[16], buf[17]]),
        code: u16::from_ne_bytes([buf[18], buf[19]]),
        value: i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]),
    })
}

struct TouchDevice {
    file: std::fs::File,
}

impl TouchDevice {
    fn open(path: &str) -> std::io::Result<Self> {
        Ok(Self { file: std::fs::File::open(path)? })
    }

    /// Blocks until one complete press-then-release is seen and returns the
    /// raw (x, y) reading, averaged over every sample reported while held
    /// down -- a resistive panel's ADC jitters visibly sample to sample,
    /// and calibration accuracy depends on this being stable, not on
    /// whichever single sample happened to arrive first or last.
    fn wait_for_tap(&mut self) -> std::io::Result<(f64, f64)> {
        let (mut cur_x, mut cur_y) = (0i32, 0i32);
        let (mut sum_x, mut sum_y, mut n) = (0i64, 0i64, 0i64);
        let mut touching = false;
        loop {
            let ev = read_event(&mut self.file)?;
            match (ev.ev_type, ev.code) {
                (t, c) if t == EV_ABS && c == ABS_X => cur_x = ev.value,
                (t, c) if t == EV_ABS && c == ABS_Y => cur_y = ev.value,
                (t, c) if t == EV_KEY && c == BTN_TOUCH => {
                    let now = ev.value != 0;
                    if now && !touching {
                        sum_x = 0;
                        sum_y = 0;
                        n = 0;
                    } else if !now && touching {
                        return Ok(if n > 0 {
                            (sum_x as f64 / n as f64, sum_y as f64 / n as f64)
                        } else {
                            // A tap quick enough that no ABS sample landed
                            // between press and release -- fall back to the
                            // last known position rather than blocking
                            // forever waiting for a sample that isn't coming.
                            (cur_x as f64, cur_y as f64)
                        });
                    }
                    touching = now;
                }
                (t, _) if t == EV_SYN && touching => {
                    sum_x += cur_x as i64;
                    sum_y += cur_y as i64;
                    n += 1;
                }
                _ => {}
            }
        }
    }
}

/// The standard 3-point affine touch calibration (screen = raw * matrix),
/// solved exactly from three known correspondences `[SPEC-FBUI-050]`. Not
/// invented here -- the same algorithm behind tslib and X11's evtouch.
/// Handles rotation, mirroring, scale and translation all at once, which
/// is exactly what's needed given the display's `rotate=90` is invisible
/// to the touch controller (see this file's module doc comment).
struct AffineCalibration {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl AffineCalibration {
    fn from_three_points(screen: [(f64, f64); 3], raw: [(f64, f64); 3]) -> Self {
        let [(xs1, ys1), (xs2, ys2), (xs3, ys3)] = screen;
        let [(xr1, yr1), (xr2, yr2), (xr3, yr3)] = raw;

        let delta = (xr1 - xr3) * (yr2 - yr3) - (xr2 - xr3) * (yr1 - yr3);

        let a = ((xs1 - xs3) * (yr2 - yr3) - (xs2 - xs3) * (yr1 - yr3)) / delta;
        let b = ((xr1 - xr3) * (xs2 - xs3) - (xr2 - xr3) * (xs1 - xs3)) / delta;
        let c = (yr1 * (xr3 * xs2 - xr2 * xs3) + yr2 * (xr1 * xs3 - xr3 * xs1)
            + yr3 * (xr2 * xs1 - xr1 * xs2))
            / delta;

        let d = ((ys1 - ys3) * (yr2 - yr3) - (ys2 - ys3) * (yr1 - yr3)) / delta;
        let e = ((xr1 - xr3) * (ys2 - ys3) - (xr2 - xr3) * (ys1 - ys3)) / delta;
        let f = (yr1 * (xr3 * ys2 - xr2 * ys3) + yr2 * (xr1 * ys3 - xr3 * ys1)
            + yr3 * (xr2 * ys1 - xr1 * ys2))
            / delta;

        Self { a, b, c, d, e, f }
    }

    fn apply(&self, raw_x: f64, raw_y: f64) -> (f64, f64) {
        (self.a * raw_x + self.b * raw_y + self.c, self.d * raw_x + self.e * raw_y + self.f)
    }

    /// A human-readable flat file, not a database table `[SPEC-FBUI-055]`
    /// -- six floats have no relational structure worth a schema for.
    /// `.toml`-named and `key = value`-shaped so it reads as valid TOML to
    /// a person or a future tool, without this binary needing the `toml`
    /// crate just to write six numbers `[REQ-HW-140]`'s "every dependency
    /// is a memory decision" reasoning.
    fn save(&self, path: &str) -> std::io::Result<()> {
        let contents = format!(
            "# Vaino touch calibration [SPEC-FBUI-050].\n\
             # Regenerate with `fbui --calibrate`; do not hand-edit.\n\
             a = {}\nb = {}\nc = {}\nd = {}\ne = {}\nf = {}\n",
            self.a, self.b, self.c, self.d, self.e, self.f
        );
        std::fs::write(path, contents)
    }

    fn load(path: &str) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut v = std::collections::HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, val) = line.split_once('=')?;
            v.insert(key.trim().to_string(), val.trim().parse::<f64>().ok()?);
        }
        Some(Self {
            a: *v.get("a")?,
            b: *v.get("b")?,
            c: *v.get("c")?,
            d: *v.get("d")?,
            e: *v.get("e")?,
            f: *v.get("f")?,
        })
    }
}

/// Inset from the physical edges, not at them -- a resistive panel's own
/// accuracy degrades right at the bezel, a well-known property of the
/// technology rather than anything specific to this board. Three points
/// forming a wide triangle (not collinear), as the algorithm requires.
const CAL_POINTS: [(i32, i32); 3] = [(48, 32), (432, 32), (240, 288)];

fn draw_crosshair(display: &mut FbDisplay, x: i32, y: i32, index: usize) -> Result<(), std::convert::Infallible> {
    let (w, h) = (display.width as i32, display.height as i32);
    Rectangle::new(Point::zero(), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(LCD_BG))
        .draw(display)?;
    let stroke = PrimitiveStyle::with_stroke(LCD_GREEN, 2);
    Line::new(Point::new(x - 10, y), Point::new(x + 10, y)).into_styled(stroke).draw(display)?;
    Line::new(Point::new(x, y - 10), Point::new(x, y + 10)).into_styled(stroke).draw(display)?;
    draw_text(display, &format!("calibration {}/{} -- touch the +", index + 1, CAL_POINTS.len()), 8, h - 12);
    Ok(())
}

/// Walks the person through `CAL_POINTS`, one crosshair at a time, and
/// solves the affine transform from what they actually touched.
/// Re-triggerable `[SPEC-FBUI-050]`: nothing here is one-shot besides the
/// file it writes, so running this again with `--calibrate` simply
/// overwrites a stale or wrong calibration.
fn run_calibration(display: &mut FbDisplay, touch: &mut TouchDevice) -> std::io::Result<AffineCalibration> {
    let mut raw_pts = [(0.0, 0.0); 3];
    for (i, &(sx, sy)) in CAL_POINTS.iter().enumerate() {
        let _ = draw_crosshair(display, sx, sy, i);
        let (rx, ry) = touch.wait_for_tap()?;
        println!("fbui: calibration point {} -- screen ({sx},{sy}) -> raw ({rx},{ry})", i + 1);
        raw_pts[i] = (rx, ry);
    }
    let screen_pts = CAL_POINTS.map(|(x, y)| (x as f64, y as f64));
    Ok(AffineCalibration::from_three_points(screen_pts, raw_pts))
}

#[cfg(test)]
mod tests {
    use super::AffineCalibration;

    /// Synthetic hardware stand-in: a forward map with a 90-degree-style
    /// axis swap plus scale and offset, the same *shape* of transform
    /// `piscreen2r`'s rotate=90 plus an uncalibrated touch controller would
    /// actually produce. Calibrating from three points generated by this
    /// map, then applying the result to a fourth, held-out point, must
    /// recover that fourth point's real screen coordinates -- this is
    /// exactly the "verify at a 4th point" check the calibration
    /// literature recommends, run here without needing real hardware.
    fn forward(sx: f64, sy: f64) -> (f64, f64) {
        (2.0 * sy + 100.0, 3.0 * sx + 50.0)
    }

    #[test]
    fn recovers_a_rotated_scaled_transform() {
        let screen = [(48.0, 32.0), (432.0, 32.0), (240.0, 288.0)];
        let raw = screen.map(|(x, y)| forward(x, y));
        let cal = AffineCalibration::from_three_points(screen, raw);

        let (test_sx, test_sy) = (150.0, 200.0);
        let (test_rx, test_ry) = forward(test_sx, test_sy);
        let (got_sx, got_sy) = cal.apply(test_rx, test_ry);

        assert!((got_sx - test_sx).abs() < 1e-6, "x: got {got_sx}, want {test_sx}");
        assert!((got_sy - test_sy).abs() < 1e-6, "y: got {got_sy}, want {test_sy}");
    }

    /// A no-op sink -- only whether `TEXT_FONT.render` returns `Ok`/`Err`
    /// matters here, per the crate's own documented default behavior:
    /// "unknown chars will return an error" unless
    /// `with_ignore_unknown_chars` is set (it is not, for `TEXT_FONT`).
    struct NullDisplay;
    impl embedded_graphics::prelude::OriginDimensions for NullDisplay {
        fn size(&self) -> embedded_graphics::prelude::Size {
            embedded_graphics::prelude::Size::new(64, 64)
        }
    }
    impl embedded_graphics::prelude::DrawTarget for NullDisplay {
        type Color = embedded_graphics::pixelcolor::Rgb565;
        type Error = std::convert::Infallible;
        fn draw_iter<I>(&mut self, _pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
        {
            Ok(())
        }
    }

    fn font_renders(ch: char) -> bool {
        use embedded_graphics::prelude::Point;
        let mut nd = NullDisplay;
        super::TEXT_FONT
            .render(ch.to_string().as_str(), Point::new(0, 20), super::VerticalPosition::Baseline, super::FontColor::Transparent(super::LCD_GREEN), &mut nd)
            .is_ok()
    }

    /// Grounds `[SPEC-FBUI-030]`'s font choice in real data instead of a
    /// synthetic test string, and pins it down as a regression test: these
    /// exact characters were queried from a production `vaino.db`'s real
    /// artist/title text (`Bj\u{f6}rk`, `M\u{f6}tley Cr\u{fc}e`, `Fernando
    /// Mendon\u{e7}a`, `Auli\u{2bb}i Cravalho`, `Guns N\u{2019} Roses`,
    /// `The Go\u{2010}Go\u{2019}s`, a `\u{201c}`-quoted nickname). Split
    /// exactly the way the actual code path splits them: the accented
    /// Latin group must render directly (this is what justified choosing
    /// `u8g2_font_9x15_t_symbols` over plain ASCII), and the punctuation
    /// group must NOT render directly (this is what justifies
    /// `normalize_for_display` existing at all) but must render once
    /// normalized.
    #[test]
    fn font_coverage_matches_real_library_data() {
        for ch in ['\u{f6}', '\u{fc}', '\u{e9}', '\u{e7}'] {
            assert!(font_renders(ch), "expected TEXT_FONT to cover accented Latin {ch:?} directly");
        }
        for ch in ['\u{2018}', '\u{2019}', '\u{02bb}', '\u{201c}', '\u{2010}', '\u{2014}'] {
            assert!(!font_renders(ch), "expected TEXT_FONT to lack {ch:?} -- if this now fails, a font upgrade may let normalize_for_display drop this mapping");
            let normalized = super::normalize_for_display(&ch.to_string());
            assert!(font_renders(normalized.chars().next().unwrap()), "normalize_for_display({ch:?}) = {normalized:?} still doesn't render");
        }
    }

    #[test]
    fn save_and_load_round_trip() {
        let cal = AffineCalibration { a: 1.5, b: -0.25, c: 10.0, d: 0.1, e: 2.0, f: -5.0 };
        let path = std::env::temp_dir().join("fbui-test-calibration.toml");
        let path = path.to_str().unwrap();
        cal.save(path).unwrap();
        let loaded = AffineCalibration::load(path).expect("load");
        std::fs::remove_file(path).ok();
        assert_eq!((cal.a, cal.b, cal.c, cal.d, cal.e, cal.f), (loaded.a, loaded.b, loaded.c, loaded.d, loaded.e, loaded.f));
    }
}

#[tokio::main]
async fn main() {
    let mut force_calibrate = false;
    let mut url = None;
    for arg in std::env::args().skip(1) {
        if arg == "--calibrate" {
            force_calibrate = true;
        } else {
            url = Some(arg);
        }
    }
    let url = url.unwrap_or_else(|| "ws://127.0.0.1:5720/ws".to_string());

    let mut display = match FbDisplay::open("/dev/fb0") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("fbui: {e}");
            std::process::exit(1);
        }
    };
    println!("fbui: {}x{} framebuffer opened", display.width, display.height);

    // Calibrate once, re-triggerable, before anything else touches the
    // screen `[SPEC-FBUI-050]` -- resolves the "what happens before
    // calibration has ever run" gap `[SPEC036]` §7 named: first boot has
    // no file at CALIBRATION_PATH, so this always runs then, with no
    // separate manual step for anyone to forget. Run synchronously here,
    // before any async work starts (no websocket connection exists yet,
    // nothing else is scheduled) -- blocking the sole worker thread on a
    // blocking file read is exactly correct at this specific point, not
    // a shortcut around doing it properly.
    let calibration: Option<AffineCalibration> = if force_calibrate
        || AffineCalibration::load(CALIBRATION_PATH).is_none()
    {
        match TouchDevice::open(TOUCH_DEVICE) {
            Ok(mut touch) => match run_calibration(&mut display, &mut touch) {
                Ok(cal) => {
                    match cal.save(CALIBRATION_PATH) {
                        Ok(()) => println!("fbui: calibration saved to {CALIBRATION_PATH}"),
                        Err(e) => eprintln!("fbui: could not save calibration: {e}"),
                    }
                    Some(cal)
                }
                Err(e) => {
                    eprintln!("fbui: calibration aborted: {e}");
                    None
                }
            },
            Err(e) => {
                eprintln!("fbui: could not open {TOUCH_DEVICE} for calibration: {e}");
                None
            }
        }
    } else {
        println!("fbui: using existing calibration at {CALIBRATION_PATH}");
        AffineCalibration::load(CALIBRATION_PATH)
    };

    // Touch is read on its own OS thread, blocking on the same evdev
    // protocol calibration above already used, and forwarded to `main`'s
    // async loop over a channel -- a blocking file read has no place
    // sharing this binary's one async worker thread once that loop also
    // has a websocket to service concurrently, unlike calibration's
    // one-time, nothing-else-running use of the same call above.
    let (touch_tx, mut touch_rx) = tokio::sync::mpsc::unbounded_channel::<(f64, f64)>();
    std::thread::spawn(move || loop {
        match TouchDevice::open(TOUCH_DEVICE) {
            Ok(mut td) => loop {
                match td.wait_for_tap() {
                    Ok(pos) => {
                        if touch_tx.send(pos).is_err() {
                            return; // main has exited; nothing left to report to
                        }
                    }
                    Err(e) => {
                        eprintln!("fbui: touch read error: {e}");
                        break;
                    }
                }
            },
            Err(e) => eprintln!("fbui: could not open {TOUCH_DEVICE}: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    });

    // An explicit "disconnected" render before ever trying to connect, so
    // a `vaino` that is not up yet shows an honest state rather than
    // whatever garbage was in video memory at boot -- the same state
    // `render_disconnected` also shows on every later drop, resolving
    // `[SPEC036]` §7's reconnection-state gap for both cases now, not
    // just this first one.
    let _ = render_disconnected(&mut display);

    let http_addr = http_addr_from_ws_url(&url);
    let mut last: Option<ClientSnapshot> = None;
    loop {
        println!("fbui: connecting to {url}");
        let ws = match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                eprintln!("fbui: connect failed: {e}; retrying in 3s");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        println!("fbui: connected");
        let (_, mut read) = ws.split();

        // Any taps that queued up while disconnected are stale by the time
        // a connection succeeds -- a button press firing a command the
        // instant reconnection completes would be surprising, not useful.
        while touch_rx.try_recv().is_ok() {}

        loop {
            tokio::select! {
                msg = read.next() => {
                    let msg = match msg {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            eprintln!("fbui: ws error: {e}");
                            break;
                        }
                        None => break, // stream ended
                    };
                    let text = match msg.into_text() {
                        Ok(t) => t,
                        Err(_) => continue, // not a text frame; nothing this phase reads
                    };
                    let snap: ClientSnapshot = match serde_json::from_str(&text) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("fbui: snapshot parse failed: {e}");
                            continue;
                        }
                    };
                    if last.as_ref() != Some(&snap) {
                        if let Err(e) = render(&mut display, &snap) {
                            // Infallible today, per DrawTarget::Error above --
                            // kept as a real match rather than `.unwrap()` so
                            // a future fallible backend (the `drm` path
                            // `[SPEC-FBUI-025]` leaves open) fails loudly
                            // here instead of panicking.
                            eprintln!("fbui: render failed: {e:?}");
                        }
                        last = Some(snap);
                    }
                }
                Some((rx, ry)) = touch_rx.recv() => {
                    let Some(cal) = &calibration else { continue };
                    let (sx, sy) = cal.apply(rx, ry);
                    match hit_test(sx, sy) {
                        Some(Zone::PlayPause) => {
                            let playing = last.as_ref().map(|s| s.playing).unwrap_or(false);
                            let name = if playing { "pause" } else { "play" };
                            tokio::spawn(http_post(http_addr.clone(), format!("/command/{name}")));
                        }
                        Some(Zone::Skip) => {
                            tokio::spawn(http_post(http_addr.clone(), "/command/skip".to_string()));
                        }
                        Some(Zone::VolUp) => {
                            let db = last.as_ref().map(|s| s.volume_db).unwrap_or(0.0) + 3.0;
                            tokio::spawn(http_post(http_addr.clone(), format!("/volume/{db}")));
                        }
                        Some(Zone::VolDown) => {
                            let db = last.as_ref().map(|s| s.volume_db).unwrap_or(0.0) - 3.0;
                            tokio::spawn(http_post(http_addr.clone(), format!("/volume/{db}")));
                        }
                        Some(Zone::Seek) => {
                            if let Some(s) = &last {
                                if s.duration_ms > 0 {
                                    let frac = ((sx - 8.0) / (display.width as f64 - 16.0)).clamp(0.0, 1.0);
                                    let ms = (frac * s.duration_ms as f64).round() as u64;
                                    tokio::spawn(http_post(http_addr.clone(), format!("/seek/{ms}")));
                                }
                            }
                        }
                        None => {}
                    }
                }
            }
        }
        eprintln!("fbui: disconnected; reconnecting in 3s");
        let _ = render_disconnected(&mut display);
        // Cleared, not carried over: the next successful snapshot must
        // render even if it happens to be identical to the last one shown
        // before this drop -- otherwise the diff check in the loop above
        // would see no change and leave this disconnected screen up
        // indefinitely despite `vaino` being back and pushing real data
        // `[SPEC036]` §7's reconnection-state gap, the actual bug this
        // phase exists to close, not just the missing message on its own.
        last = None;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

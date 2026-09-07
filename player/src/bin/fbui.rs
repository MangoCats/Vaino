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
//!     fbui [ws://host:port/ws]   (default: ws://127.0.0.1:5720/ws)

use embedded_graphics::mono_font::ascii::FONT_9X15;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use framebuffer::Framebuffer;
use futures_util::StreamExt;

/// Only the fields this phase draws. Unknown incoming fields are ignored by
/// serde automatically -- the real `Snapshot` carries dozens more.
#[derive(serde::Deserialize, Default, Clone, PartialEq)]
struct ClientSnapshot {
    playing: bool,
    title: Option<String>,
    artist: Option<String>,
    position_ms: u64,
    duration_ms: u64,
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
        // depth. Unverified against a physically-observed image at the
        // time this was written; `[SPEC-FBUI-025]`'s own review names this
        // exact byte-order question as still open.
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

fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Clears the LCD panel area and redraws title/artist/position -- the
/// whole-region redraw `[SPEC-FBUI-020]` says to do only on the fields
/// that actually changed, which `main`'s own diff against the last state
/// already guarantees by only calling this when something did.
fn render(display: &mut FbDisplay, snap: &ClientSnapshot) -> Result<(), std::convert::Infallible> {
    let (w, h) = (display.width as i32, display.height as i32);
    Rectangle::new(Point::zero(), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(LCD_BG))
        .draw(display)?;

    let style = MonoTextStyle::new(&FONT_9X15, LCD_GREEN);
    let title = snap.title.as_deref().unwrap_or("(nothing playing)");
    let artist = snap.artist.as_deref().unwrap_or("");
    let pos = format!(
        "{} {} / {}",
        if snap.playing { ">" } else { "||" },
        fmt_time(snap.position_ms),
        fmt_time(snap.duration_ms)
    );

    Text::new(title, Point::new(8, 20), style).draw(display)?;
    Text::new(artist, Point::new(8, 40), style).draw(display)?;
    Text::new(&pos, Point::new(8, h - 12), style).draw(display)?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let url = std::env::args().nth(1).unwrap_or_else(|| "ws://127.0.0.1:5720/ws".to_string());

    let mut display = match FbDisplay::open("/dev/fb0") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("fbui: {e}");
            std::process::exit(1);
        }
    };
    println!("fbui: {}x{} framebuffer opened", display.width, display.height);

    // An explicit "disconnected" render before ever trying to connect, so
    // a `vaino` that is not up yet shows an honest state rather than
    // whatever garbage was in video memory at boot `[SPEC-FBUI]` §7's
    // reconnection-state gap, addressed here for the first (connect) case;
    // the reconnect-after-drop case is not yet handled by this phase.
    let _ = render(&mut display, &ClientSnapshot::default());

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
        while let Some(msg) = read.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("fbui: ws error: {e}");
                    break;
                }
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
                    // Infallible today, per DrawTarget::Error above -- kept
                    // as a real match rather than `.unwrap()` so a future
                    // fallible backend (the `drm` path `[SPEC-FBUI-025]`
                    // leaves open) fails loudly here instead of panicking.
                    eprintln!("fbui: render failed: {e:?}");
                }
                last = Some(snap);
            }
        }
        eprintln!("fbui: disconnected; reconnecting in 3s");
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

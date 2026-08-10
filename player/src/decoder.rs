//! Bounded streaming decoder for a single passage.
//!
//! Opens a file, seeks to the passage start, and yields decoded frames a packet
//! at a time. It **never** holds more than one decoded packet plus the caller's
//! buffer, so memory is independent of passage length -- decoding minute 240 of
//! a 245-minute file costs exactly what decoding minute 1 costs.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    Symphonia(SymError),
    NoAudioTrack,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "io: {e}"),
            DecodeError::Symphonia(e) => write!(f, "decode: {e}"),
            DecodeError::NoAudioTrack => write!(f, "no audio track in container"),
        }
    }
}

/// A passage being decoded: a span of one file, not the whole file.
pub struct PassageDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    pub sample_rate: u32,
    pub channels: usize,
    /// Interleaved f32 for the packet just decoded. Reused every call, so the
    /// allocation happens once regardless of passage length.
    scratch: Vec<f32>,
    frames_emitted: u64,
    frame_limit: Option<u64>,
}

impl PassageDecoder {
    /// Open `path` and seek to `start_ms`, decoding at most `end_ms - start_ms`.
    /// `end_ms == None` means "to end of file".
    pub fn open(path: &Path, start_ms: u64, end_ms: Option<u64>) -> Result<Self, DecodeError> {
        let file = File::open(path).map_err(DecodeError::Io)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(DecodeError::Symphonia)?;
        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(DecodeError::NoAudioTrack)?;
        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(DecodeError::Symphonia)?;

        // Seeking is what makes a 40-track DAO file tractable: minute 240 costs
        // the same as minute 1 [REQ-AUD-120].
        if start_ms > 0 {
            let t = Time::from(std::time::Duration::from_millis(start_ms));
            format
                .seek(SeekMode::Accurate, SeekTo::Time { time: t, track_id: Some(track_id) })
                .map_err(DecodeError::Symphonia)?;
        }

        let frame_limit = end_ms.map(|e| {
            let span_ms = e.saturating_sub(start_ms);
            (span_ms as f64 * sample_rate as f64 / 1000.0) as u64
        });

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            scratch: Vec::new(),
            frames_emitted: 0,
            frame_limit,
        })
    }

    /// Decode the next packet. Returns interleaved f32 frames, or `None` at end
    /// of passage. The slice borrows internal scratch and is valid until the
    /// next call -- callers copy into their own ring buffer.
    pub fn next(&mut self) -> Result<Option<&[f32]>, DecodeError> {
        if let Some(limit) = self.frame_limit {
            if self.frames_emitted >= limit {
                return Ok(None);
            }
        }
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                // Clean end of stream, plus the ResetRequired a chained-header
                // stream can raise at a boundary.
                Err(SymError::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None)
                }
                Err(SymError::ResetRequired) => return Ok(None),
                Err(e) => return Err(DecodeError::Symphonia(e)),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    // Disjoint field borrows: `buf` borrows self.decoder while
                    // scratch is a different field, which is legal where a
                    // `&mut self` method would not be.
                    fill_scratch(&mut self.scratch, self.channels, &buf);
                    let mut n = self.scratch.len() / self.channels;
                    if let Some(limit) = self.frame_limit {
                        let remaining = limit.saturating_sub(self.frames_emitted) as usize;
                        if n > remaining {
                            n = remaining;
                            self.scratch.truncate(n * self.channels);
                        }
                    }
                    self.frames_emitted += n as u64;
                    return Ok(Some(&self.scratch));
                }
                // A corrupt packet mid-file should skip, not abort playback: the
                // rest of the passage is still good.
                Err(SymError::DecodeError(_)) => continue,
                Err(e) => return Err(DecodeError::Symphonia(e)),
            }
        }
    }

    pub fn frames_emitted(&self) -> u64 {
        self.frames_emitted
    }
}

/// Interleave a decoded packet into `scratch`, converting to f32.
///
/// A free function rather than a method: the caller holds a borrow of
/// `self.decoder` through `buf`, so a `&mut self` receiver would conflict.
fn fill_scratch(scratch: &mut Vec<f32>, ch: usize, buf: &AudioBufferRef<'_>) {
    let frames = buf.frames();
    scratch.clear();
    scratch.resize(frames * ch, 0.0);

    // symphonia hands back planar buffers; the audio path wants interleaved,
    // and doing the conversion here keeps it in one place.
    macro_rules! planar {
        ($b:expr, $conv:expr) => {{
            let avail = $b.spec().channels.count();
            for c in 0..ch.min(avail) {
                let src = $b.chan(c);
                for (i, s) in src.iter().enumerate().take(frames) {
                    scratch[i * ch + c] = $conv(*s);
                }
            }
        }};
    }

    match buf {
        AudioBufferRef::F32(b) => planar!(b, |s: f32| s),
        AudioBufferRef::S16(b) => planar!(b, |s: i16| s as f32 / 32_768.0),
        AudioBufferRef::S32(b) => planar!(b, |s: i32| s as f32 / 2_147_483_648.0),
        AudioBufferRef::U8(b) => planar!(b, |s: u8| (s as f32 - 128.0) / 128.0),
        // Remaining formats are rare in this library; emit silence rather than
        // panicking so playback continues.
        _ => scratch.iter_mut().for_each(|v| *v = 0.0),
    }
}

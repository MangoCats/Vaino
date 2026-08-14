//! Ring buffer and mixer.
//!
//! Deliberately small. Per `[XFD-ORTH-020]` the mixer performs **no** gain
//! calculation: fades are applied by [`crate::fade`] before audio is buffered,
//! so mixing a crossfade is addition. Resisting the urge to put "just a little"
//! gain logic here is what keeps the fade math in one place.

use crate::fade::Fade;

/// Fixed-capacity FIFO of interleaved f32 samples.
///
/// Capacity is set once and never grows -- that is the whole point
/// `[GDE-FBD-010]`. Writes that would exceed capacity are truncated and
/// reported, so a producer outrunning the consumer is visible rather than
/// silently ballooning memory.
pub struct RingBuffer {
    buf: Box<[f32]>,
    head: usize,
    len: usize,
}

impl RingBuffer {
    pub fn new(capacity_samples: usize) -> Self {
        Self { buf: vec![0.0; capacity_samples].into_boxed_slice(), head: 0, len: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn free(&self) -> usize {
        self.buf.len() - self.len
    }

    /// Append what fits. Returns the number of samples written, which is less
    /// than `src.len()` when the buffer is full.
    pub fn write(&mut self, src: &[f32]) -> usize {
        let n = src.len().min(self.free());
        let cap = self.buf.len();
        let tail = (self.head + self.len) % cap;
        let first = n.min(cap - tail);
        self.buf[tail..tail + first].copy_from_slice(&src[..first]);
        if n > first {
            self.buf[..n - first].copy_from_slice(&src[first..n]);
        }
        self.len += n;
        n
    }

    /// Remove up to `dst.len()` samples into `dst`, returning how many.
    /// Keep at most `n` samples and discard everything queued behind them.
    ///
    /// The ring is a head and a length, so dropping the tail is one assignment
    /// -- no copying, no reallocation, safe to call from under the output lock.
    /// Returns what is left.
    pub fn truncate(&mut self, n: usize) -> usize {
        self.len = self.len.min(n);
        self.len
    }

    pub fn read(&mut self, dst: &mut [f32]) -> usize {
        let n = dst.len().min(self.len);
        let cap = self.buf.len();
        let first = n.min(cap - self.head);
        dst[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
        if n > first {
            dst[first..n].copy_from_slice(&self.buf[..n - first]);
        }
        self.head = (self.head + n) % cap;
        self.len -= n;
        n
    }

    /// Add up to `dst.len()` samples into `dst` rather than overwriting.
    ///
    /// This is the crossfade primitive: two passages summed into one output
    /// `[XFD-BEH-C1-020]`. Samples are consumed either way.
    pub fn mix_into(&mut self, dst: &mut [f32]) -> usize {
        let n = dst.len().min(self.len);
        let cap = self.buf.len();
        for (i, out) in dst.iter_mut().enumerate().take(n) {
            *out += self.buf[(self.head + i) % cap];
        }
        self.head = (self.head + n) % cap;
        self.len -= n;
        n
    }
}

/// One passage's buffered, already-faded audio.
pub struct Stream {
    pub ring: RingBuffer,
    /// Frames written so far, which is the fade's position. Tracked here
    /// because the fade is applied on the way *in*.
    pub frames_written: u64,
    pub fade: Fade,
    pub channels: usize,
    /// No more audio will be produced; the stream ends when the ring drains.
    pub finished: bool,
}

impl Stream {
    pub fn new(capacity_samples: usize, channels: usize, fade: Fade) -> Self {
        Self {
            ring: RingBuffer::new(capacity_samples),
            frames_written: 0,
            fade,
            channels,
            finished: false,
        }
    }

    /// Apply this stream's fade to `samples` and buffer the result.
    ///
    /// The only path by which audio enters a stream, so audio in the ring is
    /// always already faded -- the invariant the mixer relies on.
    pub fn push(&mut self, samples: &mut [f32]) -> usize {
        self.fade.apply(samples, self.channels, self.frames_written);
        let written = self.ring.write(samples);
        self.frames_written += (written / self.channels.max(1)) as u64;
        written
    }

    pub fn is_exhausted(&self) -> bool {
        self.finished && self.ring.is_empty()
    }
}

/// Sum active streams into `out`, returning how many samples had a contributor.
///
/// A free function, not a struct: the mixer needs no state, and making it own
/// its streams forced callers to hand ownership away just to mix -- which made
/// feeding a decoder into an already-mixing stream impossible. Streams stay
/// with whoever is filling them; mixing borrows them for a block.
///
/// Holds no gain, no curves and no timing policy: those belong to the fader and
/// the queue. Keeping it this dumb is what makes crossfade testable in one place.
/// Takes any iterator of `&mut Stream` rather than a slice, so callers that
/// keep a stream beside its decoder can pass `.iter_mut().map(|(_, s)| s)`
/// instead of shuffling ownership to satisfy the signature.
pub fn mix<'a, I>(streams: I, out: &mut [f32]) -> usize
where
    I: IntoIterator<Item = &'a mut Stream>,
{
    out.iter_mut().for_each(|s| *s = 0.0);
    let mut filled = 0;
    for s in streams {
        filled = filled.max(s.ring.mix_into(out));
    }
    filled
}

/// Drop streams that have finished and drained.
pub fn retain_active(streams: &mut Vec<Stream>) {
    streams.retain(|s| !s.is_exhausted());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fade::{Curve, Fade};

    #[test]
    fn ring_wraps_without_loss() {
        let mut r = RingBuffer::new(8);
        assert_eq!(r.write(&[1.0, 2.0, 3.0, 4.0, 5.0]), 5);
        let mut out = [0.0; 3];
        assert_eq!(r.read(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        // wraps past the physical end
        assert_eq!(r.write(&[6.0, 7.0, 8.0, 9.0, 10.0]), 5);
        let mut all = [0.0; 7];
        assert_eq!(r.read(&mut all), 7);
        assert_eq!(all, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
    }

    #[test]
    fn ring_never_exceeds_capacity() {
        let mut r = RingBuffer::new(4);
        assert_eq!(r.write(&[1.0; 10]), 4, "must truncate, not grow");
        assert_eq!(r.len(), 4);
        assert_eq!(r.capacity(), 4);
    }

    #[test]
    fn mix_sums_two_streams() {
        let mut streams: Vec<Stream> = [0.25f32, 0.5]
            .iter()
            .map(|v| {
                let mut s = Stream::new(16, 2, Fade::none());
                s.push(&mut vec![*v; 8]);
                s.finished = true;
                s
            })
            .collect();
        let mut out = [0.0; 8];
        assert_eq!(mix(streams.iter_mut(), &mut out), 8);
        assert!(out.iter().all(|x| (*x - 0.75).abs() < 1e-6), "got {out:?}");
    }

    #[test]
    fn exhausted_streams_are_dropped() {
        let mut s = Stream::new(8, 2, Fade::none());
        s.push(&mut vec![1.0; 4]);
        s.finished = true;
        let mut streams = vec![s];
        let mut out = [0.0; 8];
        mix(streams.iter_mut(), &mut out);
        retain_active(&mut streams);
        assert!(streams.is_empty(), "drained stream must not linger");
    }

    #[test]
    fn audio_is_faded_on_the_way_in_not_at_mix_time() {
        let mut s = Stream::new(64, 1, Fade { curve: Curve::Linear, frames: 8, fade_in: true });
        let mut block = vec![1.0f32; 8];
        s.push(&mut block);
        let mut out = [0.0; 8];
        s.ring.read(&mut out);
        assert!(out[0] < out[7], "ring must already hold faded audio");
        assert!(out[0].abs() < 1e-6, "fade-in starts silent");
    }

    #[test]
    fn silence_where_no_stream_contributes() {
        let mut out = [9.9; 4];
        assert_eq!(mix(std::iter::empty(), &mut out), 0);
        assert!(out.iter().all(|s| *s == 0.0), "must clear, not leave stale audio");
    }
}

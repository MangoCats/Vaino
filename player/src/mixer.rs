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

    /// Discard everything buffered, keeping the allocation.
    ///
    /// For recovery after an output failure: what the ring holds then is audio
    /// mixed for a moment already several seconds gone, and playing it out on
    /// reconnection would replay that moment `[IMPL-AUD-020]`. The samples are
    /// left in place because nothing reads past `len`.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
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

    /// Keep at most `n` samples and discard everything queued behind them.
    ///
    /// The ring is a head and a length, so dropping the tail is one assignment
    /// -- no copying, no reallocation, safe to call from under the output lock.
    /// Returns what is left.
    pub fn truncate(&mut self, n: usize) -> usize {
        self.len = self.len.min(n);
        self.len
    }

    /// Remove up to `dst.len()` samples into `dst`, returning how many.
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

    /// The buffered samples, as the two contiguous runs a ring stores them in.
    ///
    /// For editing audio that has already been submitted -- which is the only
    /// way to reach it, since the callback owns everything downstream. The
    /// second run is the wrapped part and is empty unless the data straddles
    /// the end of the backing store.
    pub fn as_mut_slices(&mut self) -> (&mut [f32], &mut [f32]) {
        let cap = self.buf.len();
        let first = self.len.min(cap - self.head);
        let (start, from_head) = self.buf.split_at_mut(self.head);
        (&mut from_head[..first], &mut start[..self.len - first])
    }

    /// Add `src` into the buffer starting `offset` samples from the read point,
    /// summing where audio is already queued and appending past the end.
    ///
    /// This is how a passage is laid OVER audio that has already been
    /// submitted `[REQ-AUD-162]`. `write` cannot do it: that appends, and the
    /// whole point is to overlap. Any gap between the current end and `offset`
    /// is filled with silence, so the offset means what it says even when the
    /// buffer holds less than that.
    ///
    /// Returns the samples placed, which is short of `src.len()` only when the
    /// buffer runs out of capacity.
    pub fn mix_at(&mut self, offset: usize, src: &[f32]) -> usize {
        let cap = self.buf.len();
        while self.len < offset && self.len < cap {
            self.buf[(self.head + self.len) % cap] = 0.0;
            self.len += 1;
        }
        let mut placed = 0;
        for (i, s) in src.iter().enumerate() {
            let pos = offset + i;
            if pos < self.len {
                let p = (self.head + pos) % cap;
                self.buf[p] += *s;
            } else if self.len < cap {
                let p = (self.head + self.len) % cap;
                self.buf[p] = *s;
                self.len += 1;
            } else {
                break;
            }
            placed += 1;
        }
        placed
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

    /// The overlay sums where audio is already queued and appends past the end
    /// -- one call spanning both, because the incoming passage straddles the
    /// end of what the outgoing one left behind.
    #[test]
    fn mix_at_sums_over_existing_audio_and_appends_past_it() {
        let mut r = RingBuffer::new(64);
        r.write(&[1.0; 20]);
        let placed = r.mix_at(15, &[0.5; 10]);
        assert_eq!(placed, 10);
        assert_eq!(r.len(), 25, "five summed, five appended");
        let mut out = [0.0f32; 25];
        r.read(&mut out);
        assert_eq!(out[14], 1.0, "before the overlay, untouched");
        assert_eq!(out[15], 1.5, "summed, not overwritten");
        assert_eq!(out[19], 1.5);
        assert_eq!(out[20], 0.5, "past the end, appended");
    }

    /// The offset must mean the same thing after the buffer has wrapped, which
    /// it will have done: the output ring runs for the life of the process.
    #[test]
    fn mix_at_is_correct_across_the_wrap() {
        let mut r = RingBuffer::new(16);
        r.write(&[9.0; 12]);
        let mut sink = [0.0f32; 10];
        r.read(&mut sink); // head now at 10, two samples left
        r.write(&[1.0; 8]); // straddles the end of the backing store
        assert_eq!(r.len(), 10);
        let placed = r.mix_at(4, &[0.5; 4]);
        assert_eq!(placed, 4);
        let mut out = [0.0f32; 10];
        r.read(&mut out);
        assert_eq!(out[3], 1.0, "before the overlay");
        assert_eq!(out[4], 1.5, "summed across the wrap");
        assert_eq!(out[7], 1.5);
        assert_eq!(out[8], 1.0, "after it");
    }

    /// A gap between the end of the audio and the offset is silence, so the
    /// offset means what it says even when the ring is nearly empty.
    #[test]
    fn mix_at_pads_when_the_offset_is_beyond_the_end() {
        let mut r = RingBuffer::new(32);
        r.write(&[1.0; 2]);
        r.mix_at(5, &[0.5; 3]);
        assert_eq!(r.len(), 8);
        let mut out = [0.0f32; 8];
        r.read(&mut out);
        assert_eq!(&out[2..5], &[0.0, 0.0, 0.0], "silence, not stale audio");
        assert_eq!(out[5], 0.5);
    }

    /// The shape of a skip: the outgoing passage falls away, the incoming one
    /// arrives part-way down, and for the difference they are summed. This is
    /// the property that distinguishes it from a stop followed by a start.
    #[test]
    fn a_skip_transition_overlaps_the_two_passages() {
        let mut r = RingBuffer::new(64);
        r.write(&[1.0; 40]);
        assert_eq!(r.truncate(20), 20, "the backlog is cut to the fade");
        let fade = Fade { curve: Curve::Linear, frames: 20, fade_in: false };
        {
            let (front, back) = r.as_mut_slices();
            fade.apply(front, 1, 0);
            let wrapped_at = front.len() as u64;
            fade.apply(back, 1, wrapped_at);
        }
        r.mix_at(5, &[0.5; 25]);

        let mut out = [0.0f32; 30];
        assert_eq!(r.read(&mut out), 30);
        assert!((out[0] - 1.0).abs() < 1e-6, "outgoing starts where it was");
        assert!((out[4] - 0.8).abs() < 1e-6, "and is alone until the lead");
        assert!((out[5] - (0.75 + 0.5)).abs() < 1e-6, "then the two are summed");
        assert!((out[19] - (0.05 + 0.5)).abs() < 1e-6, "still summed at the end");
        assert!((out[20] - 0.5).abs() < 1e-6, "outgoing gone, incoming alone");
        // The outgoing part must fall monotonically the whole way.
        let outgoing: Vec<f32> = (0..5).map(|i| out[i]).collect();
        assert!(outgoing.windows(2).all(|w| w[1] < w[0]), "{outgoing:?}");
    }
    use crate::fade::{Curve, Fade};

    #[test]
    fn ring_clear_discards_stale_audio_and_still_wraps() {
        let mut r = RingBuffer::new(8);
        r.write(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let mut out = [0.0; 3];
        r.read(&mut out);                 // leave head mid-buffer, as in life
        r.clear();
        assert_eq!(r.len(), 0);
        let mut nothing = [9.9; 4];
        assert_eq!(r.read(&mut nothing), 0, "cleared ring must yield nothing");
        assert_eq!(nothing, [9.9; 4], "and must not touch the caller's buffer");

        // The reason this matters: after an output failure the ring holds audio
        // from a moment now several seconds gone, and playing it on
        // reconnection is a stutter rather than a gap `[IMPL-AUD-020]`. A clear
        // that only reset the length would replay it on the next wrap.
        assert_eq!(r.write(&[1.0; 8]), 8, "full capacity available after clear");
        let mut all = [0.0; 8];
        assert_eq!(r.read(&mut all), 8);
        assert_eq!(all, [1.0; 8], "no stale samples resurface");
    }

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

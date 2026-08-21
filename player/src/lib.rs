//! Vaino audio player.
//!
//! The single hard rule this crate exists to enforce: **audio is never decoded
//! whole** `[GDE-FBD-010]`. Vaino v1 called `miniaudio.decode_file()` and pulled
//! entire files into memory; this library's largest file is 244.9 minutes, which
//! is ~2.6 GB decoded at int16 and ~5.2 GB at f32 `[GDE-V1-030]`. That is not
//! slow on a 512 MB Pi Zero 2W, it is impossible.
//!
//! Everything here is built around a fixed-capacity buffer per passage
//! (~15 s, ~5.3 MB at 44.1 kHz stereo f32) `[GDE-ARC-050]`, so memory is a
//! function of how many passages are open, never of how long they are.

pub mod backup;
pub mod bundle;
pub mod db;
pub mod director;
pub mod decoder;
pub mod engine;
pub mod fade;
pub mod mixer;
pub mod mpd;
pub mod output;
pub mod path;
pub mod playback;
pub mod queue;
pub mod relink;
pub mod bluetooth;
pub mod sink;
pub mod session;
pub mod tags;
pub mod web;
pub mod resample;
pub mod scrobble;

/// Frames of audio buffered per passage. 15 s at 44.1 kHz.
///
/// Sized from McRhythm's measured design `[GDE-MCR-020]`: 44100 * 2ch * 4 bytes
/// * 15 s = 5.29 MB, against a =<150 MB total-process target `[REQ-HW-100]`.
pub const BUFFER_FRAMES: usize = 44_100 * 15;

/// How much of the queue a display is sent.
///
/// Not how much is queued -- the Director keeps whatever depth it was given.
/// This is how far ahead anyone can usefully read, and it bounds the size of a
/// snapshot that goes out twice a second to every connected browser.
pub const QUEUE_SHOWN: usize = 12;

/// Free space below which a passage's decoder is topped up again, in frames.
///
/// Small enough that the check is cheap and the buffer never sits nearly empty,
/// large enough that a decode yields more than it costs to ask for.
pub const DECODE_TOPUP_FRAMES: usize = 4096;

/// Most tracks one browse request will answer with `[REQ-VIS-180]`.
///
/// A library of 8,000 passages returns in 80 ms, but the number is sent to the
/// browser rather than assumed there, so the page can say "showing the first
/// 2,000" without a second copy of this constant to fall out of step.
pub const BROWSE_LIMIT: usize = 2_000;

/// How long Skip takes to fade the outgoing passage out `[REQ-AUD-158]`.
///
/// The listener is hearing audio mixed up to a ring's depth ago, so a skip can
/// only be prompt if what was already submitted is cut short. This is how much
/// of it survives, and over how long it falls away.
pub const SKIP_FADE_MS: u64 = 2_000;
pub const SKIP_FADE_MAX_MS: u64 = 10_000;

/// How long after a skip the next passage begins its normal fade-in
/// `[REQ-AUD-162]`.
///
/// Shorter than the fade-out, so the two overlap and are summed for the
/// difference -- 1.5 s with both at their defaults. The overlap is what makes a
/// skip sound like a transition rather than a stop followed by a start, and it
/// costs nothing extra because the incoming passage is already decoded
/// `[REQ-AUD-160]`.
pub const SKIP_LEAD_MS: u64 = 500;
/// How often the resume point is written `[REQ-VIS-155]`.
///
/// Every write lands on the appliance's most volatile partition
/// `[PI-C-010]`, and this is the only one that happens continuously and
/// unattended -- so it is the write rate that decides how much of that
/// partition's life is spent with a write in flight.
///
/// Five seconds rather than one, which is what it was. The cost of the longer
/// interval is bounded and small: at most this much playback position is lost
/// to a power cut, and the *interesting* transitions -- passage change, pause,
/// resume -- bypass the throttle entirely and are written the moment they
/// happen. So the setting trades a few seconds of position, never an event.
pub const RESUME_SAVE_MS: u64 = 5_000;
pub const RESUME_SAVE_MIN_MS: u64 = 1_000;
pub const RESUME_SAVE_MAX_MS: u64 = 300_000;

/// How long a *skipped* passage is held out of selection `[SPEC-PLAY-050]`.
///
/// 156 hours is six and a half days: long enough that a rejected passage does
/// not return within the week, and offset from a whole week so it does not
/// come back on the same day at the same time.
pub const SKIP_SUPPRESS_H: u64 = 156;
/// Zero is a legitimate setting: it turns skip suppression off entirely.
pub const SKIP_SUPPRESS_MIN_H: u64 = 0;
pub const SKIP_SUPPRESS_MAX_H: u64 = 8_760; // a year

pub const SKIP_LEAD_MIN_MS: u64 = 100;
pub const SKIP_LEAD_MAX_MS: u64 = 2_000;

/// Peak resident memory of this process, in bytes.
///
/// Deliberately dependency-free: the memory bound is the property under test,
/// so measuring it should not itself pull in allocations or crates.
pub fn peak_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }
    #[cfg(windows)]
    {
        // PROCESS_MEMORY_COUNTERS.PeakWorkingSetSize via psapi, declared inline
        // rather than taking a windows-sys dependency for one struct.
        #[repr(C)]
        #[derive(Default)]
        struct Pmc {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        // GetProcessMemoryInfo lives in psapi; kernel32 alone does not export it.
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> isize;
        }
        #[link(name = "psapi")]
        extern "system" {
            fn GetProcessMemoryInfo(h: isize, c: *mut Pmc, cb: u32) -> i32;
        }
        let mut pmc = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Default::default() };
        let ok = unsafe {
            GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb)
        };
        if ok != 0 { Some(pmc.peak_working_set_size as u64) } else { None }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

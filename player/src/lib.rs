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

pub mod db;
pub mod decoder;
pub mod engine;
pub mod fade;
pub mod mixer;
pub mod output;
pub mod queue;
pub mod resample;

/// Frames of audio buffered per passage. 15 s at 44.1 kHz.
///
/// Sized from McRhythm's measured design `[GDE-MCR-020]`: 44100 * 2ch * 4 bytes
/// * 15 s = 5.29 MB, against a =<150 MB total-process target `[REQ-HW-100]`.
pub const BUFFER_FRAMES: usize = 44_100 * 15;

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

//! The smallest MPD client that answers a question `[SPEC015]`.
//!
//! `std::net` and nothing else, which is the claim `[SPEC-MPD-070]` makes about
//! this whole direction: MPD's protocol is line-oriented text over TCP, so
//! speaking it costs a dependency-free module rather than an HTTP stack.
//!
//! Deliberately incomplete. This is what stage 0 needs — connect, send, read a
//! response, parse `key: value` records `[IMPL-MPD-010]` — and the commands
//! that change anything are not here because stage 0 writes to nothing.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

pub struct Mpd {
    out: TcpStream,
    inp: BufReader<TcpStream>,
    /// The `OK MPD <version>` greeting, kept because the protocol level is the
    /// thing to target rather than the newest release `[SPEC-MPD-080]`.
    pub version: String,
}

impl Mpd {
    pub fn connect(addr: &str) -> Result<Self, String> {
        let out = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
        let inp = BufReader::new(out.try_clone().map_err(|e| e.to_string())?);
        let mut m = Mpd { out, inp, version: String::new() };
        let mut greeting = String::new();
        m.inp.read_line(&mut greeting).map_err(|e| e.to_string())?;
        if !greeting.starts_with("OK MPD ") {
            return Err(format!("not an MPD greeting: {}", greeting.trim()));
        }
        m.version = greeting.trim_start_matches("OK MPD ").trim().to_string();
        Ok(m)
    }

    /// Send one command and collect its response lines.
    ///
    /// MPD terminates a response with `OK` or `ACK …`; anything else is a
    /// `key: value` line. An `ACK` is returned as an error rather than as data,
    /// because a caller that treats a refusal as an empty result will report a
    /// library of nothing and call it success.
    pub fn cmd(&mut self, command: &str) -> Result<Vec<String>, String> {
        writeln!(self.out, "{command}").map_err(|e| e.to_string())?;
        self.out.flush().map_err(|e| e.to_string())?;
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let n = self.inp.read_line(&mut line).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed mid-response".into());
            }
            let line = line.trim_end_matches(['\n', '\r']).to_string();
            if line == "OK" {
                return Ok(lines);
            }
            if let Some(err) = line.strip_prefix("ACK ") {
                return Err(err.to_string());
            }
            lines.push(line);
        }
    }

    /// Every song MPD knows, with its tags.
    ///
    /// Records are delimited by the `file:` key, which the protocol guarantees
    /// comes first for each song.
    pub fn songs(&mut self) -> Result<Vec<Song>, String> {
        let lines = self.cmd("listallinfo")?;
        let mut out: Vec<Song> = Vec::new();
        for line in lines {
            let Some((key, value)) = line.split_once(": ") else { continue };
            if key == "file" {
                out.push(Song { uri: value.to_string(), ..Default::default() });
                continue;
            }
            let Some(song) = out.last_mut() else { continue };
            match key {
                "MUSICBRAINZ_TRACKID" => song.recording_mbid = Some(value.to_string()),
                "Title" => song.title = Some(value.to_string()),
                "Artist" => song.artist = Some(value.to_string()),
                "duration" => song.duration_s = value.parse().ok(),
                _ => {}
            }
        }
        Ok(out)
    }
}

/// One song as MPD describes it. Only the fields the mapping ladder consults.
#[derive(Debug, Default, Clone)]
pub struct Song {
    /// Relative to MPD's `music_directory`, forward slashes, and **the thing
    /// Vaino must learn to name** `[SPEC-MPD-060]`.
    pub uri: String,
    /// Picard writes the *recording* MBID here despite the name, which is what
    /// makes this the second rung rather than a dead end.
    pub recording_mbid: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_s: Option<f64>,
}

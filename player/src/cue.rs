//! Cue sheets, so a guest can name a passage inside a capture `[SPEC-MPD-056]`.
//!
//! A DAO capture carries one set of album-level tags, so MPD names every
//! passage inside it after the file — 34.1% of this library `[SPEC-MPD-052]`.
//! MPD *does* read cue sheets, and exposes each cue track as its own song with
//! its own title. Writing one beside a capture names every passage in it, for
//! every client, with no sticker support required.
//!
//! **This writes into the listener's music folder**, which is why it is off by
//! default and asked for explicitly `[REQ-VIS-205]`. Nothing else in Vaino puts
//! a file there.
//!
//! Two things measured before choosing this shape:
//!
//! * MPD applies a cue track's boundaries **as a range** and reports the right
//!   title, so `rangeid` is neither needed nor safe — it overwrites the range
//!   with offsets into the *file*.
//! * MPD **ignores `INDEX 00`**, so a track ends where the next one begins.
//!   Against radio spans that is a median 4.8 s of the next track's lead-in.
//!   The backend already ends an unhonoured span itself `[SPEC-MPD-096]`, and
//!   that machinery bounds this the same way.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// What a generation run did, for the person who asked for it.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub written: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub failed: Vec<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} cue sheet(s) written, {} already current, {} skipped",
            self.written, self.unchanged, self.skipped
        );
        if !self.failed.is_empty() {
            s.push_str(&format!(", {} failed", self.failed.len()));
        }
        s
    }
}

/// `MM:SS:FF`, 75 frames to the second, as the format requires.
fn cue_time(ms: i64) -> String {
    let ms = ms.max(0);
    let secs = ms / 1000;
    let frames = (((ms % 1000) * 75 + 500) / 1000).min(74);
    format!("{:02}:{:02}:{:02}", secs / 60, secs % 60, frames)
}

/// Cue strings are double-quoted with no escape, so a quote inside one ends it.
fn cue_str(s: &str) -> String {
    s.replace('"', "'").replace(['\r', '\n'], " ")
}

/// One capture's sheet, or `None` when there is nothing worth naming.
fn sheet(file: &Path, album: &str, tracks: &[(i64, String, String)]) -> Option<String> {
    // A file with one passage is already named by its own tags; a sheet would
    // add a layer and say nothing new.
    if tracks.len() < 2 {
        return None;
    }
    let name = file.file_name()?.to_string_lossy().to_string();
    let kind = match file.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_uppercase()) {
        Some(e) if e == "MP3" => "MP3",
        Some(e) if e == "FLAC" || e == "OGG" => "WAVE",
        _ => "WAVE",
    };
    let performer = tracks.first().map(|t| t.2.clone()).unwrap_or_default();
    let mut out = String::new();
    out.push_str("REM Written by Vaino. Tracks are its radio passages.\n");
    if !performer.is_empty() {
        out.push_str(&format!("PERFORMER \"{}\"\n", cue_str(&performer)));
    }
    out.push_str(&format!("TITLE \"{}\"\n", cue_str(album)));
    out.push_str(&format!("FILE \"{}\" {}\n", cue_str(&name), kind));
    for (i, (start_ms, title, artist)) in tracks.iter().enumerate() {
        out.push_str(&format!("  TRACK {:02} AUDIO\n", i + 1));
        let t = if title.is_empty() { format!("Track {}", i + 1) } else { title.clone() };
        out.push_str(&format!("    TITLE \"{}\"\n", cue_str(&t)));
        if !artist.is_empty() {
            out.push_str(&format!("    PERFORMER \"{}\"\n", cue_str(artist)));
        }
        out.push_str(&format!("    INDEX 01 {}\n", cue_time(*start_ms)));
    }
    Some(out)
}

/// Write a cue sheet beside every capture in the library.
///
/// Idempotent: a sheet whose content already matches is left alone, so running
/// this twice touches nothing and a listener toggling the setting does not
/// rewrite their folder.
pub fn generate(conn: &Connection, dry_run: bool) -> Result<Report, String> {
    let mut by_file: HashMap<String, (String, Vec<(i64, String, String)>)> = HashMap::new();
    let mut q = conn
        .prepare(
            "SELECT f.path, \
                COALESCE(t.album, ''), \
                p.start_ms, \
                COALESCE((SELECT r.title FROM passage_recordings pr \
                          JOIN recordings r ON r.mbid = pr.mbid \
                          WHERE pr.passage_id = p.passage_id LIMIT 1), ''), \
                COALESCE((SELECT a.name FROM passage_recordings pr \
                          JOIN recording_artists ra ON ra.mbid = pr.mbid \
                          JOIN artists a ON a.mbid = ra.artist_mbid \
                          WHERE pr.passage_id = p.passage_id \
                          ORDER BY ra.weight DESC LIMIT 1), '') \
             FROM passages p JOIN files f USING(file_id) \
             LEFT JOIN file_tags t ON t.file_id = f.file_id \
             WHERE p.kind = 'radio' ORDER BY f.path, p.start_ms",
        )
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for (path, album, start, title, artist) in rows.flatten() {
        let e = by_file.entry(path).or_insert_with(|| (album, Vec::new()));
        e.1.push((start, title, artist));
    }

    let mut rep = Report::default();
    for (path, (album, tracks)) in by_file {
        let audio = PathBuf::from(&path);
        let Some(text) = sheet(&audio, &album, &tracks) else {
            rep.skipped += 1;
            continue;
        };
        let cue = audio.with_extension("cue");
        // Never overwrite someone else's sheet. A cue file that Vaino did not
        // write may be the reason the library is arranged as it is.
        if let Ok(existing) = std::fs::read_to_string(&cue) {
            if existing == text {
                rep.unchanged += 1;
                continue;
            }
            if !existing.starts_with("REM Written by Vaino") {
                rep.skipped += 1;
                continue;
            }
        }
        if dry_run {
            rep.written += 1;
            continue;
        }
        match std::fs::write(&cue, &text) {
            Ok(()) => rep.written += 1,
            Err(e) => rep.failed.push(format!("{}: {e}", cue.display())),
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks() -> Vec<(i64, String, String)> {
        vec![
            (5_030, "Test for Echo".into(), "Rush".into()),
            (361_586, "Driven".into(), "Rush".into()),
        ]
    }

    /// 75 frames to the second, and never 75 — which is the next second.
    #[test]
    fn cue_times_are_minutes_seconds_frames() {
        assert_eq!(cue_time(0), "00:00:00");
        assert_eq!(cue_time(5_030), "00:05:02");
        assert_eq!(cue_time(361_586), "06:01:44");
        assert_eq!(cue_time(999), "00:00:74", "never rolls to frame 75");
        assert_eq!(cue_time(-5), "00:00:00", "and never goes backwards");
    }

    /// A quote inside a title would end the field early; the format has no
    /// escape for it.
    #[test]
    fn a_quote_in_a_title_cannot_break_the_field() {
        let t = vec![
            (0, "He said \"go\"".into(), "X".into()),
            (1_000, "b".into(), "X".into()),
        ];
        let out = sheet(Path::new("a.mp3"), "Al\"bum", &t).unwrap();
        assert_eq!(out.matches('"').count() % 2, 0, "quotes stay balanced");
        assert!(!out.contains("said \"go\""));
    }

    /// A single-passage file is already named by its own tags.
    #[test]
    fn a_file_with_one_passage_gets_no_sheet() {
        assert!(sheet(Path::new("a.mp3"), "Album", &tracks()[..1]).is_none());
        assert!(sheet(Path::new("a.mp3"), "Album", &tracks()).is_some());
    }

    #[test]
    fn a_sheet_names_every_track_and_points_at_the_audio() {
        let out = sheet(Path::new("/m/Rush/TestForEcho.mp3"), "Test For Echo", &tracks()).unwrap();
        assert!(out.contains("FILE \"TestForEcho.mp3\" MP3"), "the audio, not the path");
        assert!(out.contains("TRACK 01 AUDIO") && out.contains("TRACK 02 AUDIO"));
        assert!(out.contains("TITLE \"Driven\""));
        assert!(out.contains("INDEX 01 06:01:44"));
        assert!(out.starts_with("REM Written by Vaino"), "so it is recognisable later");
    }
}

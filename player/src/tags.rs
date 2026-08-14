//! What the audio file says about itself.
//!
//! MusicBrainz is the preferred source for what a passage is called
//! `[REQ-VIS-170]`, but it does not know everything the library needs. Album
//! names live at the Release level and Vaino's `releases` table is empty until
//! Sampo fills it; cover art is not in the database at all. Both are, however,
//! usually sitting in the file's own tags.
//!
//! So this is the fallback, and only the fallback. It is read from the file
//! rather than fetched, because playback must not depend on a live external
//! service `[REQ-NEG-100]` -- the Cover Art Archive is exactly the kind of
//! dependency that requirement exists to forbid.

use std::path::Path;

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey, StandardVisualKey};
use symphonia::core::probe::Hint;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

impl Tags {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.artist.is_none() && self.album.is_none()
    }
}

/// Embedded cover art: its media type and its bytes.
pub struct Artwork {
    pub media_type: String,
    pub data: Vec<u8>,
}

fn open(path: &Path) -> Option<symphonia::core::probe::ProbeResult> {
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()
}

/// Read title, artist and album. Never fails: an unreadable or untagged file
/// yields empty tags, because missing metadata must not stop a passage playing.
pub fn read(path: &Path) -> Tags {
    let mut tags = Tags::default();
    let Some(mut probed) = open(path) else { return tags };

    // Two places to look, and both matter. ID3v2 on an MP3 is found by the
    // probe before the format reader exists; Vorbis comments in a FLAC belong
    // to the format itself. Taking only one silently loses half the library.
    let mut take = |rev: &symphonia::core::meta::MetadataRevision| {
        for tag in rev.tags() {
            let slot = match tag.std_key {
                Some(StandardTagKey::TrackTitle) => &mut tags.title,
                Some(StandardTagKey::Artist) => &mut tags.artist,
                Some(StandardTagKey::Album) => &mut tags.album,
                _ => continue,
            };
            if slot.is_none() {
                let v = tag.value.to_string().trim().to_string();
                if !v.is_empty() {
                    *slot = Some(v);
                }
            }
        }
    };

    if let Some(rev) = probed.format.metadata().skip_to_latest() {
        take(rev);
    }
    if let Some(rev) = probed.metadata.get().as_ref().and_then(|m| m.current()) {
        take(rev);
    }
    tags
}

/// The embedded cover, if there is one.
///
/// Front cover by preference; failing that, whatever picture is there. A file
/// carrying a back cover and a band photo and nothing else should still show
/// something rather than nothing.
pub fn artwork(path: &Path) -> Option<Artwork> {
    let mut probed = open(path)?;

    let pick = |rev: &symphonia::core::meta::MetadataRevision| -> Option<Artwork> {
        let visuals = rev.visuals();
        let front = visuals
            .iter()
            .find(|v| v.usage == Some(StandardVisualKey::FrontCover));
        let chosen = front.or_else(|| visuals.first())?;
        Some(Artwork {
            media_type: chosen.media_type.clone(),
            data: chosen.data.to_vec(),
        })
    };

    if let Some(rev) = probed.format.metadata().skip_to_latest() {
        if let Some(a) = pick(rev) {
            return Some(a);
        }
    }
    probed.metadata.get().as_ref().and_then(|m| m.current()).and_then(pick)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Missing metadata must never be an error: a passage with no tags plays
    /// exactly as well as one with them.
    #[test]
    fn an_unreadable_file_yields_empty_tags_rather_than_failing() {
        let t = read(Path::new("no-such-file.mp3"));
        assert!(t.is_empty());
        assert!(artwork(Path::new("no-such-file.mp3")).is_none());
    }
}

//! What a generation run did, said the same way every time `[PI3-API-030]`.
//!
//! Four modules write files into folders Vaino does not own — [`crate::cue`],
//! [`crate::covers`], [`crate::lyrics_cache`], [`crate::lyrics_sidecar`] — and
//! each grew its own report. The shape was identical and the **words were not**:
//! one called an untouched file `skipped`, another `kept`, a third
//! `left as they were`, all meaning the same thing, while `covers` used
//! `had_cover` for it and `skipped` for something else entirely.
//!
//! A listener reads those sentences one after another on the settings page. They
//! should differ where the runs differ and nowhere else, so the sentence is
//! built once here and each module supplies only its own nouns and reasons.

/// A tally of one run over a folder.
#[derive(Debug, Default, PartialEq)]
pub struct Written {
    /// Files created.
    pub written: usize,
    /// Files already holding exactly what this would have written. The measure
    /// of idempotence: a second run should report everything here.
    pub unchanged: usize,
    /// Candidates deliberately not written, counted by reason, in the order the
    /// reasons were first given.
    ///
    /// **Reasons, not a single number.** "473 skipped" tells a listener nothing
    /// about whether their library is fine or their settings are wrong.
    pub passed: Vec<(&'static str, usize)>,
    /// What went wrong, named. Failures are rare and specific, so they are
    /// listed rather than counted.
    pub failed: Vec<String>,
}

impl Written {
    pub fn wrote(&mut self) {
        self.written += 1;
    }

    pub fn already_current(&mut self) {
        self.unchanged += 1;
    }

    /// Count one candidate against a reason it was not written.
    ///
    /// `why` completes the phrase "*N* …", so it reads as
    /// `"75 shared a folder"`.
    pub fn passed_over(&mut self, why: &'static str) {
        match self.passed.iter_mut().find(|(w, _)| *w == why) {
            Some((_, n)) => *n += 1,
            None => self.passed.push((why, 1)),
        }
    }

    pub fn failed(&mut self, what: String) {
        self.failed.push(what);
    }

    /// How many of `why` were passed over. For tests, which should assert on
    /// the reason rather than on a position in a list.
    pub fn count_of(&self, why: &str) -> usize {
        self.passed.iter().find(|(w, _)| *w == why).map_or(0, |(_, n)| *n)
    }

    /// One sentence, for the person who asked for the run.
    ///
    /// `noun` is what was written, singular — "cue sheet", "cover", "song".
    /// Silent about failures when there were none, because a trailing
    /// "0 failed" reads as an invitation to worry.
    pub fn summary(&self, noun: &str) -> String {
        let mut s = format!("{} {noun}(s) written, {} already current", self.written, self.unchanged);
        for (why, n) in &self.passed {
            s.push_str(&format!(", {n} {why}"));
        }
        if !self.failed.is_empty() {
            s.push_str(&format!(", {} failed", self.failed.len()));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reasons keep the order they were first given, so a summary does not
    /// reshuffle itself between runs.
    #[test]
    fn reasons_are_counted_and_keep_their_order() {
        let mut r = Written::default();
        r.passed_over("shared a folder");
        r.passed_over("without art");
        r.passed_over("shared a folder");

        assert_eq!(r.passed, vec![("shared a folder", 2), ("without art", 1)]);
        assert_eq!(r.count_of("shared a folder"), 2);
        assert_eq!(r.count_of("never mentioned"), 0);
    }

    #[test]
    fn the_sentence_names_every_reason() {
        let mut r = Written::default();
        for _ in 0..3 {
            r.wrote();
        }
        r.already_current();
        r.passed_over("shared a folder");

        assert_eq!(
            r.summary("cover"),
            "3 cover(s) written, 1 already current, 1 shared a folder"
        );
    }

    /// Nothing to say about failures is said by not saying it.
    #[test]
    fn failures_appear_only_when_there_are_some() {
        let mut r = Written::default();
        r.wrote();
        assert!(!r.summary("file").contains("failed"));
        r.failed("C:/x/y.cue: access denied".into());
        assert!(r.summary("file").ends_with(", 1 failed"));
    }
}

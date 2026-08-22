//! Holding two backends and exchanging which one sounds `[SPEC-BK-030]`.
//!
//! The session drives **this**, not a backend, and never learns that it has
//! changed: [`Switching`] is itself a [`Playback`], forwarding every call to
//! whichever side is live. That is the whole trick, and it is only available
//! because the trait was made load-bearing first `[SPEC-BK-022]`.
//!
//! **What crosses a handoff is a queue of passage ids and a position**, never
//! audio and never decoder state. Vaino names what it holds exactly; MPD's
//! queue has to be *read back* into passages, and some of it cannot be
//! `[SPEC-BK-045]`.

use crate::playback::{Capabilities, Playback};
use crate::queue::QueueEntry;

/// Which side is sounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Vaino's own engine: spans, gain and ramps `[SPEC-BK-025]`.
    Local,
    /// A guest, today MPD: spans only.
    Guest,
}

/// What a handoff managed to carry, and what it could not.
///
/// Reported rather than summarised. A shortened queue that says nothing about
/// having been shortened is the quiet wrongness `[PI3-API-030]` exists to
/// refuse, and a count is not a report — the caller is given the names.
#[derive(Debug, Default, PartialEq)]
pub struct Adopted {
    /// Passages the incoming backend will be given, in order.
    pub passages: Vec<i64>,
    /// Entries left behind because nothing could name them `[SPEC-BK-045]`.
    pub dropped: Vec<String>,
}

/// Turn a guest's queue into passages, dropping what cannot be named.
///
/// **The rule is `[SPEC-BK-045]`: drop, do not block.** An entry is unnameable
/// when its file carries more than one radio passage — 191 of 5,709 here — and
/// a whole-file entry could be any of up to forty of them. Guessing would
/// attribute a play to a passage nobody heard `[SPEC-MPD-060]`; refusing the
/// whole handoff would let a rare property of the library veto something a
/// person just asked for.
///
/// Order is preserved, because it is the listener's order.
pub fn adopt_queue<F>(entries: &[String], resolve: F) -> Adopted
where
    F: Fn(&str) -> Option<i64>,
{
    let mut out = Adopted::default();
    for e in entries {
        match resolve(e) {
            Some(id) => out.passages.push(id),
            None => out.dropped.push(e.clone()),
        }
    }
    out
}

/// How the outgoing side stopped, so a caller can say `[PI3-API-030]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// It faded, and the changeover was smooth.
    Faded,
    /// It cut. Honest rather than hidden — MPD without a mixer cannot fade
    /// `[SPEC-MPD-099]`, and pretending otherwise would be the lie.
    Cut,
}

/// A backend that can stop gracefully when handed off away from.
///
/// Deliberately **not** part of [`Playback`]. Fading is something a handoff
/// wants and ordinary playback never does, and widening the trait for one
/// caller is how a seam stops being the shape of what crosses it
/// `[SPEC-BK-020]`.
pub trait FadeOut {
    /// Stop sounding over roughly this long. Returns whether it really faded.
    fn fade_out(&mut self, ms: u64) -> Stopped;
}

/// What [`Switching`] holds: something that plays, and can stop gracefully.
pub trait Backend: Playback + FadeOut {}
impl<T: Playback + FadeOut> Backend for T {}

/// What a queue transfer moved, and what it lost on the way.
#[derive(Debug, Default, PartialEq)]
pub struct Carried {
    /// Passages now queued on the incoming backend, in order.
    pub moved: Vec<i64>,
    /// Passages the library could no longer build. Named, not counted.
    pub lost: Vec<i64>,
}

/// Re-offer a carried queue to whichever backend is now live.
///
/// **The spans are re-derived here, not carried.** A passage id is the whole of
/// what crosses `[SPEC-BK-030]`; `start_ms`, `end_ms`, gain and ramps are read
/// from the library again, because they belong to the passage and not to
/// whichever backend last played it. Handing over a built `QueueEntry` would
/// have carried the *previous* backend's idea of the passage into the next one.
///
/// A passage that cannot be rebuilt is reported and skipped, for the same
/// reason an unnameable guest entry is `[SPEC-BK-045]`: a library that has been
/// rescanned since the queue was built may have renumbered it away, and that is
/// not a reason to refuse the switch.
pub fn carry_queue<F>(ids: &[i64], into: &mut dyn Playback, build: F) -> Carried
where
    F: Fn(i64) -> Option<QueueEntry>,
{
    let mut out = Carried::default();
    for &id in ids {
        match build(id) {
            Some(e) => {
                into.enqueue(e);
                out.moved.push(id);
            }
            None => out.lost.push(id),
        }
    }
    out
}

/// Two backends, one of them sounding.
pub struct Switching {
    local: Box<dyn Backend>,
    guest: Option<Box<dyn Backend>>,
    active: Side,
}

impl Switching {
    /// Start on the local engine, which is where an appliance comes up
    /// `[PI5-PWR-030]`. A guest is attached later or never.
    pub fn new(local: Box<dyn Backend>) -> Self {
        Self { local, guest: None, active: Side::Local }
    }

    pub fn attach_guest(&mut self, guest: Box<dyn Backend>) {
        self.guest = Some(guest);
    }

    pub fn active(&self) -> Side {
        self.active
    }

    pub fn has_guest(&self) -> bool {
        self.guest.is_some()
    }

    fn live(&self) -> &dyn Backend {
        match self.active {
            Side::Local => self.local.as_ref(),
            Side::Guest => self.guest.as_deref().unwrap_or(self.local.as_ref()),
        }
    }

    fn live_mut(&mut self) -> &mut dyn Backend {
        match self.active {
            Side::Guest if self.guest.is_some() => self.guest.as_deref_mut().unwrap(),
            _ => self.local.as_mut(),
        }
    }

    /// Move the session to the other side, reporting **what the outgoing
    /// backend was holding** so the caller can re-offer it.
    ///
    /// The transfer is not done here, and that is deliberate: a queue crosses
    /// as passage ids, and turning an id back into something playable needs the
    /// library, which a backend has no business holding. The caller reads the
    /// library, builds entries and enqueues them into the new side — where the
    /// spans are re-derived, because a span belongs to the passage and not to
    /// whichever backend last played it.
    ///
    /// A guest's queue is not ids to begin with, so it goes through
    /// [`adopt_queue`] first and arrives shorter `[SPEC-BK-045]`.
    ///
    /// **Audio is not crossfaded here yet.** `[SPEC-BK-030]` wants both sides
    /// sounding briefly, which the appliance measurement showed is possible;
    /// this moves the queue and leaves that to the audio path.
    pub fn switch_to(&mut self, target: Side) -> Result<Vec<i64>, String> {
        self.switch_to_over(target, 0).map(|(ids, _)| ids)
    }

    /// The same, fading the outgoing side out over `fade_ms` first.
    ///
    /// Reports **how** it stopped as well as what it was holding, because a
    /// backend that cannot fade cuts instead `[SPEC-MPD-099]` and a caller
    /// saying "switched" over a hard cut would be describing something that did
    /// not happen `[PI3-API-030]`.
    pub fn switch_to_over(
        &mut self,
        target: Side,
        fade_ms: u64,
    ) -> Result<(Vec<i64>, Stopped), String> {
        if target == self.active {
            return Ok((Vec::new(), Stopped::Faded));
        }
        if target == Side::Guest && self.guest.is_none() {
            return Err("no guest backend is attached".into());
        }
        let carried = self.live().queued_ids();
        // Stop the outgoing side *before* the switch, while it is still the
        // live one: after it, `live_mut` would fade the side just arrived at.
        let stopped =
            if fade_ms > 0 { self.live_mut().fade_out(fade_ms) } else { Stopped::Cut };
        self.active = target;
        Ok((carried, stopped))
    }
}

/// Forwarding, so the session cannot tell `[SPEC-BK-025]`.
impl Playback for Switching {
    /// **The live side's, never the union.** Reporting `FULL` while a guest is
    /// playing would promise gain and ramps that MPD cannot honour, which is
    /// exactly the lie `[SPEC-BK-040]` requires be told in advance instead.
    fn capabilities(&self) -> Capabilities {
        self.live().capabilities()
    }
    fn enqueue(&mut self, entry: QueueEntry) {
        self.live_mut().enqueue(entry)
    }
    fn queued_ids(&self) -> Vec<i64> {
        self.live().queued_ids()
    }
    fn queued_ms(&self) -> u64 {
        self.live().queued_ms()
    }
    fn shortfall(&self) -> usize {
        self.live().shortfall()
    }
    fn take_dropped(&mut self) -> Vec<i64> {
        self.live_mut().take_dropped()
    }
    fn resume_at(&mut self, position_ms: u64) {
        self.live_mut().resume_at(position_ms)
    }
    fn tick(&mut self) -> usize {
        // **Both sides tick, not only the live one.** A guest that is not
        // sounding still has a server to reconcile with, and a local engine
        // that is not sounding still has an output ring to drain. Ticking only
        // the active side would make the other one's view of the world stop at
        // the moment of the switch, and it would be wrong the moment it
        // mattered.
        let mut work = self.local.tick();
        if let Some(g) = self.guest.as_deref_mut() {
            work += g.tick();
        }
        work
    }
    fn is_shutdown(&self) -> bool {
        self.live().is_shutdown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A backend that records what it was told, and nothing else.
    #[derive(Default)]
    struct Fake {
        queued: Vec<i64>,
        ticks: usize,
        caps: Option<Capabilities>,
        faded_ms: Option<u64>,
        can_fade: bool,
    }
    impl FadeOut for Fake {
        fn fade_out(&mut self, ms: u64) -> Stopped {
            self.faded_ms = Some(ms);
            if self.can_fade {
                Stopped::Faded
            } else {
                Stopped::Cut
            }
        }
    }
    impl Playback for Fake {
        fn capabilities(&self) -> Capabilities {
            self.caps.unwrap_or(Capabilities::FULL)
        }
        fn enqueue(&mut self, e: QueueEntry) {
            self.queued.push(e.passage_id);
        }
        fn queued_ids(&self) -> Vec<i64> {
            self.queued.clone()
        }
        fn queued_ms(&self) -> u64 {
            self.queued.len() as u64 * 1000
        }
        fn shortfall(&self) -> usize {
            5usize.saturating_sub(self.queued.len())
        }
        fn take_dropped(&mut self) -> Vec<i64> {
            Vec::new()
        }
        fn resume_at(&mut self, _ms: u64) {}
        fn tick(&mut self) -> usize {
            self.ticks += 1;
            1
        }
        fn is_shutdown(&self) -> bool {
            false
        }
    }

    fn entry(id: i64) -> QueueEntry {
        QueueEntry {
            qid: 0,
            passage_id: id,
            path: std::path::PathBuf::from("x.mp3"),
            start_ms: 0,
            end_ms: 1000,
            lead_in_ms: 0,
            lead_out_ms: 0,
            gain_db: 0.0,
            mbid: None,
            naming: Default::default(),
        }
    }

    /// The settled rule `[SPEC-BK-045]`: what cannot be named is dropped, the
    /// rest goes through, and the listener's order is kept.
    #[test]
    fn an_unnameable_entry_is_dropped_and_the_rest_goes_through() {
        let known: HashMap<&str, i64> =
            [("a.mp3", 10), ("c.mp3", 30)].into_iter().collect();
        let queue: Vec<String> =
            ["a.mp3", "capture.mp3", "c.mp3"].iter().map(|s| s.to_string()).collect();

        let got = adopt_queue(&queue, |u| known.get(u).copied());

        assert_eq!(got.passages, vec![10, 30], "named entries survive, in order");
        assert_eq!(got.dropped, vec!["capture.mp3"], "and the rest is named, not counted");
    }

    /// Dropping must never become blocking, even when nothing is nameable.
    #[test]
    fn a_wholly_unnameable_queue_still_hands_over() {
        let queue: Vec<String> = ["x.mp3", "y.mp3"].iter().map(|s| s.to_string()).collect();
        let got = adopt_queue(&queue, |_| None);
        assert!(got.passages.is_empty());
        assert_eq!(got.dropped.len(), 2, "reported, and the switch still proceeds");
    }

    /// A queue crosses as ids, and arrives rebuilt from the library
    /// `[SPEC-BK-030]`.
    #[test]
    fn a_carried_queue_is_rebuilt_on_the_far_side() {
        let mut dest = Fake::default();
        let got = carry_queue(&[7, 8], &mut dest, |id| Some(entry(id)));
        assert_eq!(got.moved, vec![7, 8]);
        assert!(got.lost.is_empty());
        assert_eq!(dest.queued_ids(), vec![7, 8], "order is the listener's order");
    }

    /// A passage the library can no longer build is reported and skipped, not
    /// allowed to stop the switch `[SPEC-BK-045]`.
    #[test]
    fn a_passage_the_library_lost_does_not_stop_the_switch() {
        let mut dest = Fake::default();
        let got = carry_queue(&[7, 999, 8], &mut dest, |id| (id != 999).then(|| entry(id)));
        assert_eq!(got.moved, vec![7, 8]);
        assert_eq!(got.lost, vec![999], "named, so a caller can say which");
        assert_eq!(dest.queued_ids(), vec![7, 8]);
    }

    struct Cutter;
    impl FadeOut for Cutter {
        fn fade_out(&mut self, _ms: u64) -> Stopped {
            Stopped::Cut
        }
    }
    struct Fader;
    impl FadeOut for Fader {
        fn fade_out(&mut self, _ms: u64) -> Stopped {
            Stopped::Faded
        }
    }

    /// A backend that cannot fade says so, rather than cutting quietly
    /// `[SPEC-MPD-099]`, `[PI3-API-030]`.
    #[test]
    fn a_backend_that_cannot_fade_reports_the_cut() {
        assert_eq!(Cutter.fade_out(600), Stopped::Cut);
        assert_eq!(Fader.fade_out(600), Stopped::Faded);
    }

    /// The session talks to one thing; which side answers is not its business.
    #[test]
    fn the_session_cannot_tell_which_side_it_is_driving() {
        let mut s = Switching::new(Box::new(Fake::default()));
        s.attach_guest(Box::new(Fake { caps: Some(Capabilities::MPD), ..Default::default() }));

        let backend: &mut dyn Playback = &mut s;
        backend.enqueue(entry(1));
        assert_eq!(backend.queued_ids(), vec![1], "the local side took it");
        assert!(backend.capabilities().gain, "and reports what the local side can do");

        let carried = s.switch_to(Side::Guest).unwrap();
        assert_eq!(carried, vec![1], "and says what the outgoing side was holding");
        let backend: &mut dyn Playback = &mut s;
        assert!(backend.queued_ids().is_empty(), "the guest has its own queue");
        assert!(!backend.capabilities().gain, "and its own, smaller capabilities");
        backend.enqueue(entry(2));
        assert_eq!(backend.queued_ids(), vec![2]);

        assert_eq!(s.switch_to(Side::Local).unwrap(), vec![2]);
        assert_eq!(s.queued_ids(), vec![1], "the local side kept what it had");
    }

    /// The **outgoing** side is the one that stops, and it stops before the
    /// switch. Fading after would have silenced the side just arrived at.
    #[test]
    fn the_outgoing_side_is_what_fades() {
        let mut s = Switching::new(Box::new(Fake { can_fade: true, ..Default::default() }));
        s.attach_guest(Box::new(Fake { can_fade: true, ..Default::default() }));

        let (_, stopped) = s.switch_to_over(Side::Guest, 600).unwrap();
        assert_eq!(stopped, Stopped::Faded);
        assert_eq!(s.active(), Side::Guest);
    }

    /// A cut is reported as a cut `[SPEC-MPD-099]`. The switch still happens —
    /// the listener asked for it, and an abrupt stop is not a failure.
    #[test]
    fn a_side_that_cannot_fade_still_hands_over() {
        let mut s = Switching::new(Box::new(Fake { can_fade: false, ..Default::default() }));
        s.attach_guest(Box::new(Fake { can_fade: true, ..Default::default() }));
        let (_, stopped) = s.switch_to_over(Side::Guest, 600).unwrap();
        assert_eq!(stopped, Stopped::Cut, "said so, rather than claiming a fade");
        assert_eq!(s.active(), Side::Guest, "and switched anyway");
    }

    /// Capabilities must follow the live side, or `[SPEC-BK-040]` cannot be
    /// honoured: a UI would promise gain while MPD was playing.
    #[test]
    fn capabilities_are_the_live_sides_and_not_the_union() {
        let mut s = Switching::new(Box::new(Fake::default()));
        s.attach_guest(Box::new(Fake { caps: Some(Capabilities::MPD), ..Default::default() }));
        assert_eq!(s.capabilities(), Capabilities::FULL);
        s.switch_to(Side::Guest).unwrap();
        assert_eq!(s.capabilities(), Capabilities::MPD);
    }

    /// Both sides keep working while one sounds. A guest left un-ticked would
    /// stop reconciling with its server and be wrong the moment it mattered.
    #[test]
    fn the_idle_side_keeps_ticking() {
        let mut s = Switching::new(Box::new(Fake::default()));
        s.attach_guest(Box::new(Fake::default()));
        assert_eq!(s.tick(), 2, "one unit of work from each side");
    }

    #[test]
    fn switching_to_a_guest_that_is_not_there_is_refused() {
        let mut s = Switching::new(Box::new(Fake::default()));
        assert!(s.switch_to(Side::Guest).is_err());
        assert_eq!(s.active(), Side::Local, "and leaves the session where it was");
    }
}

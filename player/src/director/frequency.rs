//! Stage A: eligibility and frequency — *how often may this play?*
//!
//! Pure arithmetic over times and tuning values. No database, no clock, no
//! randomness: `now` is a parameter, so every case here is directly testable
//! and the six-years-proven behaviour can be pinned rather than described.
//!
//! Frequency never consults flavor, Taste or seeds `[SPEC-DIR-100]`. Keeping
//! that separation is what lets the Why-this-track panel tell two legible
//! stories instead of one opaque product `[SPEC-DIR-190]`.

/// Defaults from `[SPEC-DIR-120]`. They matter more than they look: only 36%
/// of MuLibPlay tracks ever received tuned values, so most selection runs on
/// exactly these numbers.
pub const DEF_ROTATION_TRK: f64 = 2.0; // 4.2 days
pub const DEF_RECOVERY_TRK: f64 = 2.6; // 16.6 days
pub const DEF_ROTATION_ART: f64 = 1.0; // 10 hours
pub const DEF_RECOVERY_ART: f64 = 1.0; // 10 hours
pub const DEF_RESTRAINT: f64 = 0.0; // ×1.0

/// `[SPEC-DIR-195]` proven values.
pub const MIN_WEIGHT: f64 = 0.001;
pub const MIN_LENGTH_S: f64 = 30.0;
pub const MAX_LENGTH_S: f64 = 3600.0;
pub const MAX_DEPTH_S: f64 = 10800.0;
pub const LENGTH_MIDPOINT_S: f64 = 180.0;
pub const LENGTH_CAP: f64 = 4.0;

/// Log-scale time encoding `[SPEC-DIR-110]`: one float spans four orders of
/// magnitude, which is why a single slider can mean "six minutes" or "41 days".
pub fn seconds(v: f64) -> f64 {
    10f64.powf(v) * 3600.0
}

/// The recovery ramp, transcribed from MuLibPlay's `recoveryWeight`.
///
/// Blocked below `rot`, fully recovered at `rot + rec`, linear between. The
/// boundaries are inclusive exactly as shipped -- `age == rot` is still zero --
/// because the acceptance test is reproducing its selections `[REQ-PD-110]`.
pub fn recovery_weight(age_s: f64, rot_s: f64, rec_s: f64) -> f64 {
    if age_s <= rot_s {
        return 0.0;
    }
    if age_s >= rot_s + rec_s {
        return 1.0;
    }
    (age_s - rot_s) / rec_s
}

/// Per-subject tuning, for an artist or a recording `[SPEC-SC-*]`.
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    pub rotation: f64,
    pub recovery: f64,
    pub restraint: f64,
}

impl Tuning {
    pub fn track_defaults() -> Self {
        Self { rotation: DEF_ROTATION_TRK, recovery: DEF_RECOVERY_TRK, restraint: DEF_RESTRAINT }
    }
    pub fn artist_defaults() -> Self {
        Self { rotation: DEF_ROTATION_ART, recovery: DEF_RECOVERY_ART, restraint: DEF_RESTRAINT }
    }
}

/// Whether the artist's recovery weight multiplies into the track's.
///
/// It reads as an implementation detail and is not one. MuLibPlay's shipped
/// code declares a second `weight` inside the artist-eligibility block, which
/// shadows the outer one; the line that multiplies in the artist weight writes
/// to a variable never read again. So **as shipped, the artist ramp does not
/// affect the track weight at all** -- an artist blocks, but a partially
/// recovered artist does not damp. See `[SPEC-DIR-117]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistCoupling {
    /// Reproduce the shipped behaviour, bug included. Required by the P3
    /// acceptance test `[REQ-PD-110]`: a faithful port must be faithful.
    AsShipped,
    /// The documented intent `[SPEC-DIR-115]` step 4: a recovering artist damps
    /// its tracks. Almost certainly what was meant, never what ran.
    AsSpecified,
}

/// Everything that produced the weight, kept as separate terms.
///
/// Returned rather than logged, because `[SPEC-DIR-190]` requires the decision
/// to be reconstructable and a single float cannot be. Exclusions carry their
/// reason for the same purpose: "why did this *not* play" is half the question.
#[derive(Debug, Clone, PartialEq)]
pub struct Weighing {
    pub artist_weight: f64,
    pub artist_blocked: bool,
    pub track_restraint: f64,
    pub track_ramp: f64,
    pub length_bonus: f64,
    pub occasion: f64,
    pub weight: f64,
    pub excluded: Option<Exclusion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    ArtistRotationBlock,
    TrackRotationBlock,
    TooShort,
    TooLong,
    TooDeep,
    BelowMinWeight,
}

impl Weighing {
    fn excluded(reason: Exclusion) -> Self {
        Self {
            artist_weight: 0.0,
            artist_blocked: reason == Exclusion::ArtistRotationBlock,
            track_restraint: 0.0,
            track_ramp: 0.0,
            length_bonus: 0.0,
            occasion: 1.0,
            weight: 0.0,
            excluded: Some(reason),
        }
    }
    pub fn is_eligible(&self) -> bool {
        self.excluded.is_none()
    }
}

/// One passage's frequency inputs.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub length_s: f64,
    /// How far into its file the passage starts — a DAO file holds up to 40,
    /// and the deep ones are rarely what a listener means by "a song".
    pub depth_s: f64,
    pub track: Tuning,
    pub artist: Tuning,
    /// Seconds since this recording last played; `None` if never.
    pub track_age_s: Option<f64>,
    /// Seconds since anything by this artist last played; `None` if never.
    pub artist_age_s: Option<f64>,
    /// Seasonal multiplier from `[SPEC-DIR-130]`, already resolved to a number
    /// by the occasion curve. Kept as an input so this module stays free of
    /// the clock.
    pub occasion: f64,
}

/// The artist pass, then the track pass `[SPEC-DIR-115]`.
///
/// Never-played is not the same as played-long-ago only in that it skips the
/// ramp entirely: with no history there is nothing to recover from, so the
/// weight is the restraint term alone. That is what makes a fresh library
/// degrade to uniform random rather than to silence `[SPEC-DIR-158]`.
pub fn weigh(c: &Candidate, coupling: ArtistCoupling) -> Weighing {
    // Passage filters first: they are free, and they are the reason a 90-minute
    // live set does not enter the roulette [SPEC-DIR-125].
    if c.length_s < MIN_LENGTH_S {
        return Weighing::excluded(Exclusion::TooShort);
    }
    if c.length_s > MAX_LENGTH_S {
        return Weighing::excluded(Exclusion::TooLong);
    }
    if c.depth_s > MAX_DEPTH_S {
        return Weighing::excluded(Exclusion::TooDeep);
    }

    // --- artist pass ---
    let art_rot = seconds(c.artist.rotation);
    let art_rec = seconds(c.artist.recovery);
    let mut artist_weight = 10f64.powf(-c.artist.restraint);
    if let Some(age) = c.artist_age_s {
        if age < art_rot {
            return Weighing::excluded(Exclusion::ArtistRotationBlock);
        }
        artist_weight *= recovery_weight(age, art_rot, art_rec);
    }

    // --- track pass ---
    let trk_rot = seconds(c.track.rotation);
    let trk_rec = seconds(c.track.recovery);
    let track_restraint = 10f64.powf(-c.track.restraint);
    let mut track_ramp = 1.0;
    if let Some(age) = c.track_age_s {
        if age < trk_rot {
            return Weighing::excluded(Exclusion::TrackRotationBlock);
        }
        // Guarded exactly as shipped: the ramp is applied only inside the
        // recovery window, and returns 1.0 outside it anyway.
        if age < trk_rot + trk_rec {
            track_ramp = recovery_weight(age, trk_rot, trk_rec);
        }
    }

    let length_bonus = length_bonus(c.length_s);

    let mut weight = track_restraint * track_ramp * length_bonus * c.occasion;
    if coupling == ArtistCoupling::AsSpecified {
        weight *= artist_weight;
    }

    let mut w = Weighing {
        artist_weight,
        artist_blocked: false,
        track_restraint,
        track_ramp,
        length_bonus,
        occasion: c.occasion,
        weight,
        excluded: None,
    };
    // Strictly greater, as shipped: `if (weight > minWeightLimit)`.
    if !(weight > MIN_WEIGHT) {
        weight = 0.0;
        w.weight = weight;
        w.excluded = Some(Exclusion::BelowMinWeight);
    }
    w
}

/// A mild preference for passages near three minutes `[SPEC-DIR-125]`.
/// Capped at 2× so a 20-second fragment cannot dominate the pool.
pub fn length_bonus(length_s: f64) -> f64 {
    (LENGTH_CAP.min(LENGTH_MIDPOINT_S / length_s.max(f64::EPSILON))).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: f64 = 3600.0;
    const DAY: f64 = 24.0 * HOUR;

    /// The encoding the whole stage rests on. Pinned against the worked
    /// examples in the spec so a change to the formula cannot pass silently.
    #[test]
    fn log_scale_matches_the_specified_anchors() {
        assert!((seconds(0.0) - HOUR).abs() < 1e-6, "0.0 is one hour");
        assert!((seconds(-1.0) - 360.0).abs() < 1e-6, "-1.0 is six minutes");
        assert!((seconds(2.0) - 4.1666 * DAY).abs() < 0.01 * DAY, "2.0 is ~4.2 days");
        assert!((seconds(3.0) - 41.666 * DAY).abs() < 0.1 * DAY, "3.0 is ~41 days");
    }

    #[test]
    fn the_recovery_ramp_is_linear_between_its_boundaries() {
        let (rot, rec) = (100.0, 400.0);
        assert_eq!(recovery_weight(50.0, rot, rec), 0.0, "inside rotation is blocked");
        assert_eq!(recovery_weight(100.0, rot, rec), 0.0, "the boundary is inclusive, as shipped");
        assert!((recovery_weight(300.0, rot, rec) - 0.5).abs() < 1e-9, "halfway");
        assert_eq!(recovery_weight(500.0, rot, rec), 1.0, "fully recovered");
        assert_eq!(recovery_weight(9e9, rot, rec), 1.0, "and stays there");
    }

    fn candidate() -> Candidate {
        Candidate {
            length_s: LENGTH_MIDPOINT_S,
            depth_s: 0.0,
            track: Tuning::track_defaults(),
            artist: Tuning::artist_defaults(),
            track_age_s: None,
            artist_age_s: None,
            occasion: 1.0,
        }
    }

    /// Cold start: no history, no tuning. Everything must weigh the same, or
    /// a fresh library is not uniform random but silently biased.
    #[test]
    fn a_never_played_library_is_uniform() {
        let w = weigh(&candidate(), ArtistCoupling::AsShipped);
        assert!(w.is_eligible());
        assert!((w.weight - 1.0).abs() < 1e-9, "weight was {}", w.weight);
        assert_eq!(w.track_ramp, 1.0, "no history means no ramp to be on");
    }

    #[test]
    fn a_recent_play_blocks_the_track() {
        let mut c = candidate();
        c.track_age_s = Some(seconds(DEF_ROTATION_TRK) - 1.0);
        assert_eq!(weigh(&c, ArtistCoupling::AsShipped).excluded, Some(Exclusion::TrackRotationBlock));
    }

    #[test]
    fn a_recent_play_by_the_artist_blocks_the_track() {
        let mut c = candidate();
        c.artist_age_s = Some(seconds(DEF_ROTATION_ART) - 1.0);
        let w = weigh(&c, ArtistCoupling::AsShipped);
        assert_eq!(w.excluded, Some(Exclusion::ArtistRotationBlock));
        assert!(w.artist_blocked, "the panel must be able to say which block fired");
    }

    /// Mid-ramp the weight is partial, and the decomposition must show WHERE on
    /// the ramp -- "0.5" is not an explanation, "halfway through recovery" is.
    #[test]
    fn a_partly_recovered_track_is_damped_and_says_so() {
        let mut c = candidate();
        let (rot, rec) = (seconds(DEF_ROTATION_TRK), seconds(DEF_RECOVERY_TRK));
        c.track_age_s = Some(rot + rec / 2.0);
        let w = weigh(&c, ArtistCoupling::AsShipped);
        assert!((w.track_ramp - 0.5).abs() < 1e-9, "ramp {}", w.track_ramp);
        assert!((w.weight - 0.5).abs() < 1e-9);
    }

    /// The shadowing bug, pinned. As shipped a half-recovered ARTIST leaves the
    /// track weight untouched; as specified it halves it. Both are asserted so
    /// neither can drift into the other unnoticed [SPEC-DIR-117].
    #[test]
    fn the_artist_ramp_reaches_the_track_only_as_specified() {
        let mut c = candidate();
        let (rot, rec) = (seconds(DEF_ROTATION_ART), seconds(DEF_RECOVERY_ART));
        c.artist_age_s = Some(rot + rec / 2.0);

        let shipped = weigh(&c, ArtistCoupling::AsShipped);
        assert!((shipped.artist_weight - 0.5).abs() < 1e-9, "artist is half recovered");
        assert!((shipped.weight - 1.0).abs() < 1e-9,
                "as shipped the artist ramp does not reach the track weight");

        let intended = weigh(&c, ArtistCoupling::AsSpecified);
        assert!((intended.weight - 0.5).abs() < 1e-9, "as specified it damps the track");
    }

    #[test]
    fn restraint_spans_boost_to_suppression() {
        let mut c = candidate();
        c.track.restraint = -0.939; // the observed maximum boost
        assert!((weigh(&c, ArtistCoupling::AsShipped).weight - 8.687).abs() < 0.01);
        c.track.restraint = 5.0; // "never again"
        assert_eq!(weigh(&c, ArtistCoupling::AsShipped).excluded, Some(Exclusion::BelowMinWeight));
    }

    #[test]
    fn the_length_bonus_favours_three_minutes_and_caps_at_two() {
        assert!((length_bonus(180.0) - 1.0).abs() < 1e-9, "the midpoint is neutral");
        assert!((length_bonus(45.0) - 2.0).abs() < 1e-9, "4x capped, so sqrt is 2x");
        assert!((length_bonus(1.0) - 2.0).abs() < 1e-9, "the cap holds however short");
        assert!(length_bonus(720.0) < 1.0, "long passages are mildly disfavoured");
    }

    #[test]
    fn passage_filters_reject_outside_the_playable_range() {
        let mut c = candidate();
        c.length_s = 29.0;
        assert_eq!(weigh(&c, ArtistCoupling::AsShipped).excluded, Some(Exclusion::TooShort));
        c.length_s = 3601.0;
        assert_eq!(weigh(&c, ArtistCoupling::AsShipped).excluded, Some(Exclusion::TooLong));
        c.length_s = 180.0;
        c.depth_s = 10801.0;
        assert_eq!(weigh(&c, ArtistCoupling::AsShipped).excluded, Some(Exclusion::TooDeep));
    }

    /// The occasion multiplier must remain a separate visible term, not be
    /// folded into the weight product [SPEC-DIR-130].
    #[test]
    fn the_occasion_multiplier_stays_legible() {
        let mut c = candidate();
        c.occasion = 3.9; // christmasy 0.9 against a 4.2 curve, in December
        let w = weigh(&c, ArtistCoupling::AsShipped);
        assert!((w.weight - 3.9).abs() < 1e-9);
        assert_eq!(w.occasion, 3.9, "the panel needs the term, not just the product");
    }

    /// The exclusion test is strictly greater, as shipped -- a weight of
    /// exactly min_weight does not qualify.
    #[test]
    fn the_minimum_weight_boundary_is_exclusive() {
        let mut c = candidate();
        c.track.restraint = -(MIN_WEIGHT.log10()); // weight lands exactly on 0.001
        let w = weigh(&c, ArtistCoupling::AsShipped);
        assert_eq!(w.excluded, Some(Exclusion::BelowMinWeight), "weight was {}", w.weight);
    }
}

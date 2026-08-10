//! The Program Director: what plays next, and why `[SPEC009]`.
//!
//! Four stages, kept in separate modules because they answer different
//! questions and must not be allowed to blur into each other `[SPEC-DIR-100]`:
//!
//! ```text
//!   A. frequency ... how often may this play?   (rotation, recovery, history)
//!   B. shaping ..... does this fit right now?   (seeds, Taste, flavor)
//!   C. flow ........ does it follow what is queued?
//!   D. roulette .... weighted random over the shaped pool
//! ```
//!
//! Only Stage A exists so far. Stages B and C need flavor distance
//! `[SPEC-FD-040]`, and D needs a settled pool size `[SPEC-DIR-200]`.

pub mod frequency;
pub mod library;

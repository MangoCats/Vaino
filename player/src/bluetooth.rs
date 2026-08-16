//! Speaker selection, fronting the privileged helper `[PI3-API-050]`.
//!
//! Every BlueZ operation goes through `sudo vaino-btctl`, which holds the
//! privilege and enforces its own argument checks `[PI-SET-030]`. This module
//! is the other half of that boundary: it decides which verbs exist, and it
//! validates the address again before spending a subprocess on it.
//!
//! Checking twice is deliberate. The helper's check is the one that must not
//! be bypassed; this one exists so a malformed address from a browser is a 400
//! with an explanation rather than a non-zero exit nobody reads, and so the
//! rule survives someone later calling this module from somewhere new.

use std::process::Command;

const HELPER: &str = "/usr/local/bin/vaino-btctl";

/// The verbs the web surface may invoke. An enum rather than a string passed
/// through, so an unknown verb cannot reach the helper at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    List,
    Scan,
    Pair,
    Repair,
    Use,
    Forget,
    Status,
}

impl Verb {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "list" => Verb::List,
            "scan" => Verb::Scan,
            "pair" => Verb::Pair,
            "repair" => Verb::Repair,
            "use" => Verb::Use,
            "forget" => Verb::Forget,
            "status" => Verb::Status,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Verb::List => "list",
            Verb::Scan => "scan",
            Verb::Pair => "pair",
            Verb::Repair => "repair",
            Verb::Use => "use",
            Verb::Forget => "forget",
            Verb::Status => "status",
        }
    }

    /// Does this verb name a device?
    pub fn needs_address(self) -> bool {
        !matches!(self, Verb::List | Verb::Scan)
    }
}

/// A Bluetooth device address, in the one form the helper accepts.
///
/// Anchored, fixed length, uppercase hex. Deliberately not a lenient parse
/// that normalises what it is given: the value is about to become an argument
/// to a privileged program, and the useful property is that anything not
/// already exactly right is refused rather than repaired into something
/// plausible.
pub fn is_address(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 17 {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| {
        if i % 3 == 2 {
            *c == b':'
        } else {
            c.is_ascii_digit() || (b'A'..=b'F').contains(c)
        }
    })
}

/// Run a verb. `Err` carries a message fit to show a listener.
pub fn run(verb: Verb, address: Option<&str>) -> Result<serde_json::Value, String> {
    if verb.needs_address() {
        match address {
            Some(a) if is_address(a) => {}
            _ => return Err("not a device address".into()),
        }
    }
    let mut cmd = Command::new("sudo");
    cmd.arg("-n").arg(HELPER).arg(verb.as_str());
    if let Some(a) = address.filter(|_| verb.needs_address()) {
        cmd.arg(a);
    }
    let out = cmd.output().map_err(|e| format!("helper not available: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    // The helper reports its own failures as JSON with ok:false, so a parse
    // failure here means something else went wrong -- most likely the sudoers
    // rule is missing, which is worth saying plainly rather than as a blank.
    serde_json::from_str(text.trim()).map_err(|_| {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("password") || err.contains("not allowed") {
            "helper is not permitted to run; the sudoers rule is missing".into()
        } else {
            format!("helper gave no usable answer: {}", err.trim())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_speaker_we_use() {
        assert!(is_address("20:64:DE:CF:F3:AD"));
    }

    #[test]
    fn refuses_anything_that_is_not_exactly_an_address() {
        // The case that matters: an argument bound for a privileged program.
        assert!(!is_address("20:64:DE:CF:F3:AD; rm -rf /"));
        assert!(!is_address("20:64:DE:CF:F3"), "too short");
        assert!(!is_address("20-64-DE-CF-F3-AD"), "wrong separator");
        assert!(!is_address("20:64:de:cf:f3:ad"), "lowercase is not normalised");
        assert!(!is_address(""));
        assert!(!is_address("../../etc/passwd"));
    }

    #[test]
    fn unknown_verbs_do_not_exist() {
        assert_eq!(Verb::parse("destroy"), None);
        assert_eq!(Verb::parse("remove"), None);
        assert_eq!(Verb::parse("use"), Some(Verb::Use));
    }

    #[test]
    fn only_the_listing_verbs_go_without_a_device() {
        assert!(!Verb::List.needs_address());
        assert!(!Verb::Scan.needs_address());
        for v in [Verb::Pair, Verb::Repair, Verb::Use, Verb::Forget, Verb::Status] {
            assert!(v.needs_address(), "{v:?} must name a device");
        }
    }

    #[test]
    fn a_verb_needing_a_device_refuses_a_bad_one_before_spawning() {
        assert!(run(Verb::Use, Some("nonsense")).is_err());
        assert!(run(Verb::Use, None).is_err());
    }
}

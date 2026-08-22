//! Stamp the build with the commit it came from `[REQ-VIS-200]`.
//!
//! A running player must be able to say which source it is, because the
//! alternative is asking a person to remember — and the moment that matters is
//! exactly the moment nobody does: a control that is missing, a fix that seems
//! not to have landed, an appliance that was deployed to twice.
//!
//! **The dirty marker earns its keep.** A hash alone says which commit the tree
//! was *at*, not what was compiled: a build from an edited tree is not that
//! commit, and reporting it as one would be a confident wrong answer of exactly
//! the kind `[PI3-API-030]` refuses.
//!
//! Absent git, or a source tarball with no repository, is not a failure. The
//! version still exists; the hash becomes `unknown`, which is honest and lets
//! the build proceed.

use std::process::Command;

fn main() {
    // Rerun when HEAD moves. Without this the stamp is baked once and then
    // quietly lies for every later build.
    for p in [".git/HEAD", ".git/index", "../.git/HEAD", "../.git/index"] {
        if std::path::Path::new(p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }

    let hash = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into());

    // `--porcelain` prints one line per changed path and nothing at all for a
    // clean tree, which is the whole test.
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let stamp = if dirty { format!("{hash}+dirty") } else { hash };
    println!("cargo:rustc-env=VAINO_GIT={stamp}");
}

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

use std::path::Path;
use std::process::Command;

/// Ask git something, or `None` if git or the repository is absent.
fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn watch(p: &Path) {
    if p.exists() {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}

fn main() {
    // **Naming any input replaces cargo's default**, which is to re-run this
    // whenever a file in the package changes. That default is what kept the
    // dirty marker true, so listing only the git files silently traded one kind
    // of staleness for another: HEAD was watched, the working tree was not, and
    // a build from edited sources went on reporting a clean tree. Both halves
    // have to be named now that either is.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");

    // Asked for rather than guessed at: the repository is a directory up from
    // this crate today, but a worktree or a submodule puts it somewhere else
    // entirely, and a guess that misses simply stops watching without saying so.
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        let git_dir = Path::new(&dir);
        // HEAD moves on checkout; the branch's own ref file moves on commit;
        // the index moves on both. Watching all three covers every way the
        // answer below can change without a source file changing.
        watch(&git_dir.join("HEAD"));
        watch(&git_dir.join("index"));
        if let Some(head_ref) = git(&["rev-parse", "--symbolic-full-name", "HEAD"]) {
            watch(&git_dir.join(head_ref));
        }
    }

    let hash = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());

    // **A diff, not a status, and line endings do not count as a difference.**
    // The appliance binary is cross-compiled in a Linux container against a
    // bind-mounted Windows checkout, where the worktree is CRLF and the
    // container's git has no `autocrlf` to undo it: `status --porcelain` there
    // reports 104 phantom modifications, so every appliance build stamped
    // `+dirty` no matter how clean the tree was. A stamp that always says
    // `+dirty` says nothing, and it said it on the one machine where nobody can
    // check by looking.
    //
    // `--ignore-cr-at-eol` agrees with the Windows host, which is the test.
    //
    // It answers for the **repository**, while what is watched above is this
    // package. An edit to something the player does not compile — a document,
    // another crate — therefore marks the tree dirty without rebuilding this
    // binary, and the stamp will say so only at the next rebuild. That is the
    // right way round: the stamp describes the sources the binary was built
    // from, and those have not changed.
    let dirty = Command::new("git")
        .args(["diff", "--quiet", "--ignore-cr-at-eol", "HEAD"])
        .status()
        .ok()
        .map(|st| !st.success())
        .unwrap_or(false);

    let stamp = if dirty { format!("{hash}+dirty") } else { hash };
    println!("cargo:rustc-env=VAINO_GIT={stamp}");
}

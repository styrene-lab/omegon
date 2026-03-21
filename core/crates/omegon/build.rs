//! Build script — bakes git metadata into the binary so every build is
//! uniquely identifiable via `omegon --version`.
//!
//! Output format: `omegon 0.14.0 (3a4b5c6 2026-03-21)`
//!
//! For RC builds tagged `v0.14.1-rc.1`, cargo-release sets the Cargo.toml
//! version to `0.14.1-rc.1` which clap renders directly. The git sha and
//! date provide the second axis of identity — same RC tag, different commit
//! means a rebuild is visible.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    // Short SHA (7 chars)
    let sha = git(&["rev-parse", "--short=7", "HEAD"])
        .unwrap_or_else(|| "unknown".into());

    // Dirty flag — any tracked modifications or staged changes
    let dirty = git(&["status", "--porcelain"])
        .map(|s| if s.is_empty() { "" } else { "-dirty" })
        .unwrap_or("");

    // Commit date (not author date — survives rebase)
    let date = git(&["log", "-1", "--format=%cd", "--date=short"])
        .unwrap_or_else(|| "unknown".into());

    // Describe — includes tag distance for RC awareness
    // e.g. "v0.14.0" (on tag), "v0.14.0-12-g3a4b5c6" (12 commits past tag)
    let describe = git(&["describe", "--tags", "--always"])
        .unwrap_or_else(|| sha.clone());

    println!("cargo:rustc-env=OMEGON_GIT_SHA={sha}{dirty}");
    println!("cargo:rustc-env=OMEGON_BUILD_DATE={date}");
    println!("cargo:rustc-env=OMEGON_GIT_DESCRIBE={describe}");

    // Only re-run when git state changes (HEAD moves or index changes)
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}

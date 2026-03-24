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

    // Check if git describe matches Cargo.toml version (tag is on HEAD).
    // If so, the git: line in --version is redundant — suppress it.
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let tag_matches = describe == format!("v{cargo_version}")
        || describe.strip_prefix('v') == Some(&cargo_version);
    let describe_display = if tag_matches {
        String::new()
    } else {
        format!("\ngit: {describe}")
    };

    println!("cargo:rustc-env=OMEGON_GIT_SHA={sha}{dirty}");
    println!("cargo:rustc-env=OMEGON_BUILD_DATE={date}");
    println!("cargo:rustc-env=OMEGON_GIT_DESCRIBE={describe_display}");

    // Only re-run when the commit changes (HEAD moves), not on every
    // git status/stage/stash. Watching .git/index causes full recompiles
    // on every incremental build because the index changes constantly.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    // Also watch the ref that HEAD points to (e.g. refs/heads/main)
    if let Some(head_ref) = git(&["symbolic-ref", "--short", "HEAD"]) {
        let ref_path = format!("../../.git/refs/heads/{head_ref}");
        println!("cargo:rerun-if-changed={ref_path}");
    }
}

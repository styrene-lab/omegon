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
    let sha = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());

    // Dirty flag — tracked modifications or staged changes only.
    // Ignored local runtime state (for example `.omegon/runtime/`) must not taint
    // version reporting, because workspace leases are intentionally machine-local.
    let dirty = git(&[
        "status",
        "--porcelain",
        "--untracked-files=no",
        "--ignored=no",
    ])
        .map(|s| if s.is_empty() { "" } else { "-dirty" })
        .unwrap_or("");

    // Commit date (not author date — survives rebase)
    let date =
        git(&["log", "-1", "--format=%cd", "--date=short"]).unwrap_or_else(|| "unknown".into());

    // Describe — includes tag distance for RC awareness
    // e.g. "v0.14.0" (on tag), "v0.14.0-12-g3a4b5c6" (12 commits past tag)
    let describe = git(&["describe", "--tags", "--always"]).unwrap_or_else(|| sha.clone());

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

    // Compute the next milestone version from CARGO_PKG_VERSION.
    // - RC build  "0.15.3-rc.1" → next milestone is "0.15.3" (strip suffix)
    // - Stable    "0.15.2"       → next milestone is "0.15.3" (bump patch)
    let next_version = {
        let v = &cargo_version;
        if let Some(base) = v.split('-').next() {
            if v.contains("-rc") {
                // Already targeting this version; the stable is the base
                base.to_string()
            } else {
                // Stable — next milestone is patch+1
                let parts: Vec<&str> = base.splitn(3, '.').collect();
                if parts.len() == 3 {
                    let patch: u32 = parts[2].parse().unwrap_or(0);
                    format!("{}.{}.{}", parts[0], parts[1], patch + 1)
                } else {
                    base.to_string()
                }
            }
        } else {
            v.clone()
        }
    };

    println!("cargo:rustc-env=OMEGON_GIT_SHA={sha}{dirty}");
    println!("cargo:rustc-env=OMEGON_BUILD_DATE={date}");
    println!("cargo:rustc-env=OMEGON_GIT_DESCRIBE={describe_display}");
    println!("cargo:rustc-env=OMEGON_NEXT_VERSION={next_version}");

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

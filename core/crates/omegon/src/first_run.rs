//! Side-effect-free first-launch detection for startup splash policy.
//!
//! Interactive settings come from the shared profile/CLI bootstrap. A fresh
//! install must not prompt for a legacy posture or persist an implicit choice.

use std::path::Path;

/// Whether this launch has no saved user or project profile.
pub fn is_first_launch(cwd: &Path) -> bool {
    // Skip for child processes
    if std::env::var("OMEGON_CHILD").is_ok() {
        return false;
    }
    // Prompt flags imply non-interactive intent.
    if std::env::args().any(|a| a == "--prompt" || a == "--prompt-file") {
        return false;
    }
    // First run = no global profile
    let has_global = dirs::home_dir()
        .map(|h| h.join(".omegon/profile.json").exists())
        .unwrap_or(false);
    // Also check project-level
    let has_project = crate::setup::find_project_root(cwd)
        .join(".omegon/profile.json")
        .exists();
    !has_global && !has_project
}

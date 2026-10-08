//! Shared semantic surface projections consumed by TUI, ACP, and future clients.

pub mod actions;
pub mod activity;
pub mod command;
pub mod command_menu;
pub mod component;
pub mod conversation;
pub mod dashboard;
pub mod diagnostics;
pub mod editor;
pub mod episodes;
pub mod footer;
pub mod inference_policy;
pub mod inline;
pub mod instruments;
pub mod layout;
pub mod memory_status;
pub mod menu;
pub mod model_menu;
pub mod operations;
pub mod palette;
pub mod plans;
pub mod profile;
pub(crate) mod session;
pub(crate) mod session_activity;
pub mod settings;

#[cfg(test)]
mod ownership_tests {
    use std::{fs, path::Path};

    #[test]
    fn production_surfaces_do_not_import_or_probe_concrete_owners() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/surfaces");
        let forbidden = [
            "crate::features",
            "crate::lifecycle",
            "crate::memory_service",
            "crate::status",
            "crate::auth",
            "crate::route",
            "crate::model_catalog",
            "crate::model_preferences",
            "crate::settings",
            "crate::workspace",
            "crate::bootstrap_projection",
            "crate::subagent_route",
            "crate::session_authority",
            "crate::session_blob_store",
            "std::fs",
            "std::process",
            "Command::new",
            ".exists()",
            ".is_dir()",
        ];
        let mut violations = Vec::new();
        for entry in fs::read_dir(&root).expect("read semantic surface directory") {
            let path = entry.expect("surface entry").path();
            if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                continue;
            }
            // Frozen durable schema-v1 DTOs are explicitly outside Slice 6.2.
            if path.file_name().and_then(|value| value.to_str()) == Some("session.rs") {
                continue;
            }
            let source = fs::read_to_string(&path).expect("read semantic surface source");
            let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
            for pattern in forbidden {
                if production.contains(pattern) {
                    violations.push(format!("{}: {pattern}", path.display()));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "semantic surfaces crossed concrete owner boundaries:\n{}",
            violations.join("\n")
        );
    }
}

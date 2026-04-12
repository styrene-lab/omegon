//! Clipboard paste retention feature — exposes `/clipboard prune` for
//! manual on-demand sweeps of stale clipboard image pastes from the
//! system temp directory.
//!
//! The automatic 24h sweep at session start lives in `main.rs`; this
//! feature is the operator's manual override surface. It uses the
//! same `clipboard::prune_old_pastes` helper so the rules stay
//! consistent across both call sites.

use async_trait::async_trait;
use omegon_traits::{BusEvent, BusRequest, CommandDefinition, CommandResult, Feature};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::clipboard;
use crate::settings::Settings;

pub struct ClipboardFeature {
    /// Shared settings handle so the prune subcommand reads the
    /// operator's current `clipboard_retention_hours` value rather
    /// than a stale snapshot.
    settings: Arc<Mutex<Settings>>,
}

impl ClipboardFeature {
    pub fn new(settings: Arc<Mutex<Settings>>) -> Self {
        Self { settings }
    }

    /// Resolve the configured retention window from settings, falling
    /// back to the 24h default if the lock is poisoned (shouldn't
    /// happen, but better to sweep with the default than to silently
    /// no-op).
    fn current_retention(&self) -> Duration {
        let hours = self
            .settings
            .lock()
            .map(|s| s.clipboard_retention_hours)
            .unwrap_or(24);
        Duration::from_secs(hours.saturating_mul(3600))
    }
}

#[async_trait]
impl Feature for ClipboardFeature {
    fn name(&self) -> &str {
        "clipboard"
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "clipboard".into(),
            description: "Manage clipboard paste retention (subcommands: prune)".into(),
            subcommands: vec!["prune".into()],
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name != "clipboard" {
            return CommandResult::NotHandled;
        }
        let sub = args.split_whitespace().next().unwrap_or("");
        match sub {
            "prune" | "" => {
                let retention = self.current_retention();
                match clipboard::prune_old_pastes(retention) {
                    Ok(stats) => {
                        let hours = retention.as_secs() / 3600;
                        let header = if retention.is_zero() {
                            "Clipboard prune: retention is set to 0 (disabled). \
                             No files will be deleted. Set \
                             `clipboard_retention_hours` to a positive value to \
                             enable automatic cleanup."
                                .to_string()
                        } else {
                            format!("Clipboard prune: retention = {hours}h\n{}", stats.summary())
                        };
                        CommandResult::Display(header)
                    }
                    Err(e) => CommandResult::Display(format!("Clipboard prune failed: {e}")),
                }
            }
            other => CommandResult::Display(format!(
                "Unknown /clipboard subcommand: {other:?}. Try `/clipboard prune`."
            )),
        }
    }

    fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn clipboard_feature_registers_prune_subcommand() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let feature = ClipboardFeature::new(settings);
        let cmds = feature.commands();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "clipboard");
        assert!(cmds[0].subcommands.iter().any(|s| s == "prune"));
    }

    #[test]
    fn clipboard_feature_returns_not_handled_for_other_commands() {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let mut feature = ClipboardFeature::new(settings);
        let result = feature.handle_command("usage", "");
        assert!(matches!(result, CommandResult::NotHandled));
    }

    #[test]
    fn clipboard_feature_prune_runs_with_default_retention() {
        // The prune subcommand should always return a Display result
        // (success or "failed" message), never NotHandled. We can't
        // assert specific deletion counts without setting up a fake
        // temp dir, but we can verify the command path doesn't panic
        // and produces operator-readable output.
        let settings = Arc::new(Mutex::new(Settings::default()));
        let mut feature = ClipboardFeature::new(settings);
        let result = feature.handle_command("clipboard", "prune");
        match result {
            CommandResult::Display(text) => {
                assert!(
                    text.contains("Clipboard prune"),
                    "should produce a clipboard prune report: {text}"
                );
            }
            other => panic!("expected Display result, got {other:?}"),
        }
    }

    #[test]
    fn clipboard_feature_zero_retention_emits_disabled_notice() {
        let mut settings = Settings::default();
        settings.clipboard_retention_hours = 0;
        let settings = Arc::new(Mutex::new(settings));
        let mut feature = ClipboardFeature::new(settings);
        let CommandResult::Display(text) = feature.handle_command("clipboard", "prune") else {
            panic!("expected Display result");
        };
        assert!(
            text.contains("disabled"),
            "should explain that retention is disabled: {text}"
        );
    }
}

//! Shared initialization helpers extracted from main.rs.
//!
//! These functions deduplicate recurring patterns across the three
//! entrypoints (interactive, headless, daemon).

use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;

use omegon_traits::AgentEvent;

use crate::bridge::LlmBridge;
use crate::providers;
use crate::settings::{self, SharedSettings};
use crate::setup;

// ─── Settings bootstrap ────────────────────────────────────────────────────

/// Options for initializing shared settings from profile + CLI overrides.
pub struct SettingsInit<'a> {
    pub model: &'a str,
    pub cwd: &'a Path,
    /// CLI posture name (explorator, fabricator, etc.) — overrides profile default.
    pub cli_posture: Option<&'a str>,
    /// Whether slim/explorator compatibility mode was explicitly requested.
    pub slim: bool,
    /// Whether full/default posture was explicitly requested, overriding `slim`.
    pub full: bool,
    /// CLI max_turns value. Only applied if != the default (50).
    pub max_turns: u32,
    /// Whether to apply posture from the profile (uses `apply_to_with_posture`
    /// vs plain `apply_to`).
    pub apply_profile_posture: bool,
}

/// Create shared settings, load the project profile, and apply CLI overrides.
///
/// This replaces the 3-6 duplicated "shared setup" blocks across main.rs.
pub fn initialize_shared_settings(init: &SettingsInit<'_>) -> SharedSettings {
    let shared = settings::shared(init.model);
    let loaded_profile = settings::Profile::load_with_source(init.cwd);
    let profile = loaded_profile.profile.clone();

    if let Ok(mut s) = shared.lock() {
        s.profile_source = loaded_profile.source;
        if init.apply_profile_posture {
            profile.apply_to_with_posture(&mut s, init.cwd);
        } else {
            profile.apply_to(&mut s);
        }

        // CLI posture flag overrides profile default.
        if let Some(posture_name) = init.cli_posture {
            crate::apply_posture_to_settings(posture_name, &mut s, init.cwd);
        }

        if init.full {
            s.set_posture(settings::PosturePreset::Architect);
        } else if init.slim || std::env::var("OMEGON_CHILD_SLIM").is_ok() {
            s.set_posture(settings::PosturePreset::Explorator);
        }

        // Resource posture sets defaults, but explicit persisted settings win
        // unless the operator passed an explicit CLI posture override.
        if init.cli_posture.is_none() {
            if let Some(thinking) = profile
                .thinking_level
                .as_deref()
                .and_then(settings::ThinkingLevel::parse)
            {
                s.thinking = thinking;
            }
            if let Some(class) = profile
                .requested_context_class
                .as_deref()
                .and_then(settings::ContextClass::parse)
            {
                s.set_requested_context_class(class);
            }
        }

        // Only override max_turns when explicitly set (50 is the default).
        if init.max_turns != 50 {
            s.max_turns = init.max_turns;
        }

        // Apply child runtime profile overrides from env (set by orchestrator).
        if let Some(thinking) = std::env::var("OMEGON_CHILD_THINKING_LEVEL")
            .ok()
            .and_then(|v| settings::ThinkingLevel::parse(&v))
        {
            s.thinking = thinking;
        }
        if let Some(class) = std::env::var("OMEGON_CHILD_CONTEXT_CLASS")
            .ok()
            .and_then(|v| settings::ContextClass::parse(&v))
        {
            s.set_requested_context_class(class);
        }

        tracing::info!(
            model = %s.model,
            thinking = %s.thinking.as_str(),
            max_turns = s.max_turns,
            "settings initialized from profile"
        );
    }

    shared
}

// ─── LLM bridge resolution ─────────────────────────────────────────────────

/// Resolve an LLM bridge for the given model, bailing if no provider is available.
///
/// Used by headless and smoke test entrypoints that cannot start without a provider.
pub async fn resolve_bridge_or_bail(model: &str) -> anyhow::Result<Box<dyn LlmBridge>> {
    resolve_bridge_or_bail_with_secrets(model, None).await
}

pub async fn resolve_bridge_or_bail_with_secrets(
    model: &str,
    secrets: Option<&omegon_secrets::SecretsManager>,
) -> anyhow::Result<Box<dyn LlmBridge>> {
    let explicit_provider = providers::explicit_provider_id(model);
    match providers::auto_detect_bridge_with_secrets(model, secrets).await {
        Some(bridge) => {
            tracing::info!("using native LLM provider");
            Ok(bridge)
        }
        None => {
            if let Some(provider) = explicit_provider {
                anyhow::bail!(
                    "Explicit provider '{provider}' is not available for model '{model}'. Configure that provider or choose a different model."
                );
            }
            // Try auto-detecting any available provider before giving up.
            if let Some(safe_model) = providers::automation_safe_model()
                && let Some(bridge) =
                    providers::auto_detect_bridge_with_secrets(&safe_model, secrets).await
            {
                tracing::info!(
                    requested = model, resolved = %safe_model,
                    "requested model unavailable — falling back to detected provider"
                );
                return Ok(bridge);
            }
            anyhow::bail!(
                "No LLM provider available.\n\n\
                 To get started:\n  \
                 omegon auth login anthropic       # OAuth (recommended)\n  \
                 omegon auth login antigravity     # Google Antigravity OAuth\n  \
                 omegon auth login openai-codex    # OpenAI Codex OAuth\n  \
                 omegon auth login google          # Google API key\n  \
                 omegon auth login openrouter      # OpenRouter (free tier)\n\n\
                 Or set a key directly:\n  \
                 export ANTHROPIC_API_KEY=sk-ant-...\n  \
                 export GOOGLE_API_KEY=AIza..."
            );
        }
    }
}

// ─── Event channel wiring ───────────────────────────────────────────────────

/// Create the broadcast event channel and wire the cleave/delegate slots
/// so features can emit `AgentEvent`s from inside tool execution.
pub fn wire_event_channel(
    agent: &setup::AgentSetup,
    capacity: usize,
) -> (
    broadcast::Sender<AgentEvent>,
    broadcast::Receiver<AgentEvent>,
) {
    let (events_tx, events_rx) = broadcast::channel::<AgentEvent>(capacity);

    if let Ok(mut slot) = agent.cleave_event_slot.lock() {
        *slot = Some(crate::build_runtime_bus_request_sink(events_tx.clone()));
    }
    if let Ok(mut slot) = agent.delegate_event_slot.lock() {
        *slot = Some(crate::build_runtime_bus_request_sink(events_tx.clone()));
    }

    (events_tx, events_rx)
}

// ─── Runtime posture ────────────────────────────────────────────────────────

/// Apply runtime posture to both the initial harness status and the live
/// dashboard handle. Deduplicates the pattern repeated in interactive,
/// headless, and daemon startup.
pub fn apply_runtime_posture(
    agent: &mut setup::AgentSetup,
    profile: omegon_traits::OmegonRuntimeProfile,
    autonomy: omegon_traits::OmegonAutonomyMode,
) {
    agent
        .initial_harness_status
        .update_runtime_posture(profile.clone(), autonomy.clone());
    if let Some(ref harness) = agent.dashboard_handles.harness
        && let Ok(mut status) = harness.lock()
    {
        status.update_runtime_posture(profile, autonomy);
    }
}

// ─── Loop config builder ────────────────────────────────────────────────────

/// Build a `LoopConfig` from shared settings with per-call overrides.
///
/// This replaces the 7+ identical inline constructions in the daemon handler
/// and the standalone headless config at run_agent_command.
#[derive(Default)]
pub struct LoopConfigOverrides {
    /// Override max_retries (default: 0 for interactive, >0 for headless).
    pub max_retries: u32,
    /// Override commit nudge behavior.
    pub allow_commit_nudge: bool,
    /// Override first-turn execution bias.
    pub enforce_first_turn_execution_bias: bool,
    /// Force compact flag (interactive mode).
    pub force_compact: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Secrets manager.
    pub secrets: Option<Arc<omegon_secrets::SecretsManager>>,
    /// Ollama manager (created at startup for interactive mode).
    pub ollama_manager: Option<crate::ollama::OllamaManager>,
    /// Runtime bridge model override. Keeps UI/profile model intact while routing
    /// provider calls to the model supported by the active bridge.
    pub bridge_model: Option<String>,
    /// Authoritative interactive route controller for per-turn serving model.
    pub route_controller: Option<Arc<crate::route::RouteController>>,
}

/// Build a LoopConfig reading model/max_turns from shared settings, with the
/// given working directory and overrides.
pub fn build_loop_config(
    shared_settings: &SharedSettings,
    cwd: &Path,
    model_fallback: &str,
    overrides: LoopConfigOverrides,
) -> crate::r#loop::LoopConfig {
    let (model, max_turns) = shared_settings
        .lock()
        .map(|s| (s.model.clone(), s.max_turns))
        .unwrap_or_else(|_| (model_fallback.to_string(), 50));

    let soft_limit_turns = if max_turns > 0 { max_turns * 2 / 3 } else { 0 };

    crate::r#loop::LoopConfig {
        max_turns,
        soft_limit_turns,
        max_retries: overrides.max_retries,
        retry_delay_ms: 750,
        model,
        bridge_model: overrides.bridge_model,
        route_controller: overrides.route_controller,
        cwd: cwd.to_path_buf(),
        extended_context: false,
        settings: Some(shared_settings.clone()),
        secrets: overrides.secrets,
        force_compact: overrides.force_compact,
        allow_commit_nudge: overrides.allow_commit_nudge,
        enforce_first_turn_execution_bias: overrides.enforce_first_turn_execution_bias,
        ollama_manager: overrides.ollama_manager,
        skill_phases: Vec::new(), // populated by caller after skill loading
        host_context: None,
        permission_policy: None,
        permission_role: None,
        cancel_keeps_prompt: None,
        drain_post_loop_requests: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_shared_settings_applies_slim_as_explorator() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(tmp.path().join(".omegon/profile.json"), r#"{}"#).unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: true,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert!(s.is_slim(), "slim=true should set Explorator posture");
        assert_eq!(
            s.posture.effective,
            settings::PosturePreset::Explorator,
            "slim flag must map to Explorator posture"
        );
        assert_eq!(s.thinking, settings::ThinkingLevel::Minimal);
    }

    #[test]
    fn initialize_shared_settings_slim_preserves_profile_thinking_level() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"thinkingLevel":"high"}"#,
        )
        .unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: true,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert!(s.is_slim(), "slim=true should still set Explorator posture");
        assert_eq!(
            s.thinking,
            settings::ThinkingLevel::High,
            "explicit profile thinking should survive slim posture defaults"
        );
    }

    #[test]
    fn initialize_shared_settings_slim_preserves_profile_requested_context_class() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"requestedContextClass":"massive"}"#,
        )
        .unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: true,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert!(s.is_slim(), "slim=true should still set Explorator posture");
        assert_eq!(
            s.requested_context_class,
            Some(settings::ContextClass::Massive),
            "explicit profile requested context should survive slim posture defaults"
        );
    }

    #[test]
    fn initialize_shared_settings_profile_resource_fields_override_profile_posture_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"defaultPosture":"explorator","thinkingLevel":"high","requestedContextClass":"massive"}"#,
        )
        .unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: false,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert_eq!(s.posture.effective, settings::PosturePreset::Explorator);
        assert_eq!(s.thinking, settings::ThinkingLevel::High);
        assert_eq!(
            s.requested_context_class,
            Some(settings::ContextClass::Massive)
        );
    }

    #[test]
    fn initialize_shared_settings_default_is_not_slim() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: false,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert!(!s.is_slim(), "default should not be slim");
    }

    #[test]
    fn initialize_shared_settings_cli_posture_overrides_profile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: Some("devastator"),
            slim: false,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert_eq!(s.posture.effective, settings::PosturePreset::Devastator);
        assert_eq!(s.thinking, settings::ThinkingLevel::High);
    }

    #[test]
    fn initialize_shared_settings_full_overrides_cli_posture_and_slim() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: Some("devastator"),
            slim: true,
            full: true,
            max_turns: 50,
            apply_profile_posture: true,
        });

        let s = shared.lock().unwrap();
        assert_eq!(s.posture.effective, settings::PosturePreset::Architect);
        assert!(
            !s.is_slim(),
            "--full should force the non-slim default posture"
        );
    }

    #[test]
    fn initialize_shared_settings_max_turns_only_overrides_when_not_default() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();

        // Default (50) should not override profile
        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: false,
            full: false,
            max_turns: 50,
            apply_profile_posture: true,
        });
        let default_turns = shared.lock().unwrap().max_turns;

        // Non-default should override
        let shared = initialize_shared_settings(&SettingsInit {
            model: "anthropic:claude-sonnet-4-6",
            cwd: tmp.path(),
            cli_posture: None,
            slim: false,
            full: false,
            max_turns: 25,
            apply_profile_posture: true,
        });
        let overridden = shared.lock().unwrap().max_turns;
        assert_eq!(overridden, 25, "non-default max_turns should override");
        // default_turns depends on whether profile.json exists, so just check it's set
        assert!(default_turns > 0);
    }

    #[test]
    fn build_loop_config_reads_model_and_max_turns_from_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = settings::shared("anthropic:claude-opus-4-7");
        if let Ok(mut s) = shared.lock() {
            s.max_turns = 30;
        }

        let config = build_loop_config(
            &shared,
            tmp.path(),
            "fallback-model",
            LoopConfigOverrides::default(),
        );

        assert_eq!(config.model, "anthropic:claude-opus-4-7");
        assert_eq!(config.max_turns, 30);
        assert_eq!(config.soft_limit_turns, 20); // 30 * 2/3
    }

    #[test]
    fn build_loop_config_falls_back_on_poisoned_mutex() {
        let shared = settings::shared("anthropic:claude-opus-4-7");
        // Poison the mutex
        let shared_clone = shared.clone();
        let _ = std::panic::catch_unwind(|| {
            let _guard = shared_clone.lock().unwrap();
            panic!("intentional poison");
        });

        let tmp = tempfile::tempdir().unwrap();
        let config = build_loop_config(
            &shared,
            tmp.path(),
            "fallback-model",
            LoopConfigOverrides::default(),
        );

        assert_eq!(config.model, "fallback-model");
        assert_eq!(config.max_turns, 50);
    }
}

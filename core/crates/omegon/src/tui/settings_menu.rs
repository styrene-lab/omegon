//! Settings/menu scaffolding for TUI runtime configuration.
//!
//! This module is the migration seam for moving settings-related selectors and,
//! eventually, the full settings surface out of `tui::mod`. It owns setting
//! descriptors and selector-option projections; `tui::mod` remains responsible
//! for presenting the selector and dispatching confirmed changes.

use super::selector;

/// What the active selector is editing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SelectorKind {
    Model,
    ModelGrade,
    ModelProvider,
    ModelPolicy,
    ThinkingLevel,
    ContextClass,
    Persona,
    Tone,
    SecretAction,
    SecretName,
    LoginProvider,
    VaultConfigure,
    UpdateChannel,
    WorkspaceRole,
    WorkspaceKind,
    Preferences,
    ToolDetail,
    MaxTurns,
}

/// Coarse settings categories for the future settings surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingCategory {
    RuntimeProfile,
    ModelProvider,
    Ui,
    AutonomySafety,
    MemoryContext,
    Integrations,
    UpdatesDiagnostics,
}

/// UI editor shape for a setting descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingEditorKind {
    Choice,
    Toggle,
    Text,
    Number,
    CommandAction,
    ReadOnly,
}

/// Mutation route for applying a setting change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingMutationRoute {
    LocalUi,
    SharedSettings,
    RuntimeCommand { command: &'static str },
    ExternalAction { command: &'static str },
    ReadOnly,
}

/// Declarative metadata for one operator-editable setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub category: SettingCategory,
    pub editor: SettingEditorKind,
    pub route: SettingMutationRoute,
}

pub(crate) const THINKING_DESCRIPTOR: SettingDescriptor = SettingDescriptor {
    id: "runtime.thinking",
    label: "Thinking level",
    category: SettingCategory::RuntimeProfile,
    editor: SettingEditorKind::Choice,
    route: SettingMutationRoute::RuntimeCommand { command: "think" },
};

pub(crate) const CONTEXT_DESCRIPTOR: SettingDescriptor = SettingDescriptor {
    id: "runtime.context_class",
    label: "Context class",
    category: SettingCategory::MemoryContext,
    editor: SettingEditorKind::Choice,
    route: SettingMutationRoute::RuntimeCommand { command: "context" },
};

fn sel_opt(value: &str, label: &str, desc: &str, current: &str) -> selector::SelectOption {
    selector::SelectOption {
        value: value.to_string(),
        label: label.to_string(),
        description: desc.to_string(),
        active: value == current,
    }
}

#[cfg(test)]
pub(crate) fn build_model_selector_options(
    current: &str,
    anthropic_auth: Option<(String, bool)>,
    openai_auth: Option<(String, bool)>,
    openai_codex_auth: Option<(String, bool)>,
) -> Vec<selector::SelectOption> {
    let mut options: Vec<selector::SelectOption> = Vec::new();

    if let Some((_, is_oauth)) = anthropic_auth {
        let auth = if is_oauth { "oauth" } else { "api key" };
        options.push(sel_opt(
            "anthropic:claude-sonnet-4-6",
            "Sonnet 4.6",
            &format!("Anthropic · balanced · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "anthropic:claude-opus-4-6",
            "Opus 4.6",
            &format!("Anthropic · strongest · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "anthropic:claude-haiku-4-5-20251001",
            "Haiku 4.5",
            &format!("Anthropic · fast · cheap · 200k · {auth}"),
            current,
        ));
    }

    if let Some((_, is_oauth)) = openai_auth {
        let auth = if is_oauth { "oauth" } else { "api key" };
        options.push(sel_opt(
            "openai:gpt-5.4",
            "GPT-5.4",
            &format!("OpenAI API · frontier · 1M · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai:o3",
            "o3",
            &format!("OpenAI API · reasoning · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai:o4-mini",
            "o4-mini",
            &format!("OpenAI API · fast reasoning · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai:gpt-4.1",
            "GPT-4.1",
            &format!("OpenAI API · coding · 1M · {auth}"),
            current,
        ));
    }

    if let Some((_, is_oauth)) = openai_codex_auth {
        let auth = if is_oauth { "oauth" } else { "api key" };
        options.push(sel_opt(
            "openai-codex:gpt-5.4",
            "GPT-5.4",
            &format!("ChatGPT/Codex · GPT route · 1M · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai-codex:gpt-5.4-mini",
            "GPT-5.4 mini",
            &format!("ChatGPT/Codex · fast coding · 1M · {auth}"),
            current,
        ));
    }

    options
}

pub(crate) fn thinking_selector_options(
    current: crate::settings::ThinkingLevel,
) -> Vec<selector::SelectOption> {
    crate::settings::ThinkingLevel::all()
        .iter()
        .map(|level| selector::SelectOption {
            value: level.as_str().to_string(),
            label: format!("{} {}", level.icon(), level.as_str()),
            description: match level {
                crate::settings::ThinkingLevel::Off => "Servitor — no extended thinking".into(),
                crate::settings::ThinkingLevel::Minimal => "Functionary — ~2k token budget".into(),
                crate::settings::ThinkingLevel::Low => "Adept — ~5k token budget".into(),
                crate::settings::ThinkingLevel::Medium => "Magos — ~10k token budget".into(),
                crate::settings::ThinkingLevel::High => "Archmagos — ~50k token budget".into(),
            },
            active: *level == current,
        })
        .collect()
}

pub(crate) fn context_class_selector_options(
    current: crate::settings::ContextClass,
) -> Vec<selector::SelectOption> {
    crate::settings::ContextClass::all()
        .iter()
        .map(|class| selector::SelectOption {
            value: class.short().to_string(),
            label: class.label().to_string(),
            description: match class {
                crate::settings::ContextClass::Compact => "Standard sessions".into(),
                crate::settings::ContextClass::Standard => "Extended analysis".into(),
                crate::settings::ContextClass::Extended => "Large codebase".into(),
                crate::settings::ContextClass::Massive => "Massive context".into(),
            },
            active: *class == current,
        })
        .collect()
}

pub(crate) fn preferences_selector_options(
    settings: &crate::settings::Settings,
) -> Vec<selector::SelectOption> {
    let dirs = if settings.trusted_directories.is_empty() {
        "none".to_string()
    } else {
        settings.trusted_directories.len().to_string()
    };

    vec![
        selector::SelectOption {
            value: "model".into(),
            label: "Model".into(),
            description: format!("Current: {}", settings.model),
            active: false,
        },
        selector::SelectOption {
            value: "thinking".into(),
            label: "Thinking Level".into(),
            description: format!("Current: {}", settings.thinking.as_str()),
            active: false,
        },
        selector::SelectOption {
            value: "context".into(),
            label: "Context Class".into(),
            description: format!("Current: {}", settings.context_class.label()),
            active: false,
        },
        selector::SelectOption {
            value: "detail".into(),
            label: "Tool Density".into(),
            description: format!("Current: {}", settings.tool_detail.as_str()),
            active: false,
        },
        selector::SelectOption {
            value: "persona".into(),
            label: "Persona".into(),
            description: "Activate or change persona".into(),
            active: false,
        },
        selector::SelectOption {
            value: "tone".into(),
            label: "Tone".into(),
            description: "Activate or change tone".into(),
            active: false,
        },
        selector::SelectOption {
            value: "permissions".into(),
            label: "Permissions".into(),
            description: format!("Configured: {dirs}"),
            active: false,
        },
        selector::SelectOption {
            value: "update".into(),
            label: "Update Channel".into(),
            description: format!(
                "Current: {} (auto: {})",
                settings.update_channel,
                if settings.auto_update { "on" } else { "off" }
            ),
            active: false,
        },
    ]
}

pub(crate) fn max_turns_selector_options(current: u32) -> Vec<selector::SelectOption> {
    [0, 10, 25, 50, 100, 200]
        .into_iter()
        .map(|turns| selector::SelectOption {
            value: turns.to_string(),
            label: if turns == 0 {
                "unlimited".into()
            } else {
                turns.to_string()
            },
            description: if turns == 0 {
                "No autonomous turn cap".into()
            } else {
                format!("Stop after {turns} autonomous turns")
            },
            active: turns == current,
        })
        .collect()
}


pub(crate) fn model_grade_selector_options(current: &str) -> Vec<selector::SelectOption> {
    ["F", "D", "C", "B", "A", "S"]
        .into_iter()
        .map(|grade| selector::SelectOption {
            value: grade.to_string(),
            label: format!("Grade {grade}"),
            description: match grade {
                "S" => "Maximum capability / strongest available route".to_string(),
                "A" | "B" => "Frontier-class routing intent".to_string(),
                "C" | "D" => "Mid-tier/local-friendly routing intent".to_string(),
                "F" => "Leaf/cheap routing intent".to_string(),
                _ => unreachable!(),
            },
            active: current.eq_ignore_ascii_case(grade),
        })
        .collect()
}

pub(crate) fn model_provider_selector_options(current: &str) -> Vec<selector::SelectOption> {
    [
        ("auto", "Auto", "Let the route controller choose a policy-compliant provider"),
        ("local", "Local", "Prefer local providers such as Ollama"),
        ("upstream", "Upstream", "Avoid local providers and use hosted upstream providers"),
        ("anthropic", "Anthropic", "Route specifically to Anthropic when available"),
        ("openai-codex", "OpenAI Codex", "Route specifically to OpenAI Codex when available"),
        ("openai", "OpenAI", "Route specifically to OpenAI API when available"),
        ("openrouter", "OpenRouter", "Route specifically to OpenRouter when available"),
        ("google", "Google Gemini", "Route specifically to Google Gemini when available"),
        ("ollama", "Ollama", "Route specifically to local Ollama when available"),
    ]
    .into_iter()
    .map(|(value, label, description)| selector::SelectOption {
        value: value.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        active: current.eq_ignore_ascii_case(value),
    })
    .collect()
}

pub(crate) fn model_policy_selector_options(current: &str) -> Vec<selector::SelectOption> {
    [
        ("exact", "Exact", "Require the requested grade exactly"),
        ("minimum", "Minimum", "Allow the requested grade or better"),
        ("nearest", "Nearest", "Allow nearest policy-compliant fallback"),
    ]
    .into_iter()
    .map(|(value, label, description)| selector::SelectOption {
        value: value.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        active: current.eq_ignore_ascii_case(value),
    })
    .collect()
}

pub(crate) fn tool_detail_selector_options(
    current: crate::settings::ToolDetail,
) -> Vec<selector::SelectOption> {
    vec![
        selector::SelectOption {
            value: "lean".into(),
            label: "Lean".into(),
            description: "One-liner per tool. Minimal noise.".into(),
            active: current == crate::settings::ToolDetail::Lean,
        },
        selector::SelectOption {
            value: "compact".into(),
            label: "Compact".into(),
            description: "2-3 lines: name + summary + short result.".into(),
            active: current == crate::settings::ToolDetail::Compact,
        },
        selector::SelectOption {
            value: "detailed".into(),
            label: "Detailed".into(),
            description: "Full args and results. Default.".into(),
            active: current == crate::settings::ToolDetail::Detailed,
        },
        selector::SelectOption {
            value: "verbose".into(),
            label: "Verbose".into(),
            description: "Maximum output. For debugging.".into(),
            active: current == crate::settings::ToolDetail::Verbose,
        },
    ]
}

pub(crate) fn update_channel_selector_options(current: &str) -> Vec<selector::SelectOption> {
    [
        crate::update::UpdateChannel::Stable,
        crate::update::UpdateChannel::Nightly,
    ]
    .into_iter()
    .map(|channel| selector::SelectOption {
        value: channel.as_str().to_string(),
        label: channel.as_str().to_string(),
        description: match channel {
            crate::update::UpdateChannel::Stable => "Stable releases".to_string(),
            crate::update::UpdateChannel::Nightly => "Nightly builds from main".to_string(),
        },
        active: current == channel.as_str(),
    })
    .collect()
}

pub(crate) fn workspace_role_selector_options() -> Vec<selector::SelectOption> {
    [
        crate::workspace::types::WorkspaceRole::Primary,
        crate::workspace::types::WorkspaceRole::Feature,
        crate::workspace::types::WorkspaceRole::CleaveChild,
        crate::workspace::types::WorkspaceRole::Benchmark,
        crate::workspace::types::WorkspaceRole::Release,
        crate::workspace::types::WorkspaceRole::Exploratory,
        crate::workspace::types::WorkspaceRole::ReadOnly,
    ]
    .into_iter()
    .map(|role| selector::SelectOption {
        value: role.as_str().to_string(),
        label: role.as_str().to_string(),
        description: format!("Set workspace role to {}", role.as_str()),
        active: false,
    })
    .collect()
}

pub(crate) fn workspace_kind_selector_options() -> Vec<selector::SelectOption> {
    [
        crate::workspace::types::WorkspaceKind::Code,
        crate::workspace::types::WorkspaceKind::Vault,
        crate::workspace::types::WorkspaceKind::Knowledge,
        crate::workspace::types::WorkspaceKind::Spec,
        crate::workspace::types::WorkspaceKind::Mixed,
        crate::workspace::types::WorkspaceKind::Generic,
    ]
    .into_iter()
    .map(|kind| selector::SelectOption {
        value: kind.as_str().to_string(),
        label: kind.as_str().to_string(),
        description: format!("Set workspace kind to {}", kind.as_str()),
        active: false,
    })
    .collect()
}

/// Result of applying a selected settings value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingApplyOutcome {
    Thinking(crate::settings::ThinkingLevel),
    ContextClass(crate::settings::ContextClass),
    ToolDetail(crate::settings::ToolDetail),
    UpdateChannel(crate::update::UpdateChannel),
    WorkspaceRole(crate::workspace::types::WorkspaceRole),
    WorkspaceKind(crate::workspace::types::WorkspaceKind),
    MaxTurns(u32),
    Invalid { label: &'static str, value: String },
}

impl SettingApplyOutcome {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Thinking(level) => format!("Thinking → {} {}", level.icon(), level.as_str()),
            Self::ContextClass(class) => format!("Context policy → {}", class.label()),
            Self::ToolDetail(mode) => format!("Tool density → {}", mode.as_str()),
            Self::UpdateChannel(channel) => format!(
                "Update channel set to {}. Rechecking for updates now.",
                channel.as_str()
            ),
            Self::WorkspaceRole(role) => format!("Workspace role → {}", role.as_str()),
            Self::WorkspaceKind(kind) => format!("Workspace kind → {}", kind.as_str()),
            Self::MaxTurns(max_turns) => format!(
                "Max turns → {}",
                if *max_turns == 0 {
                    "unlimited".to_string()
                } else {
                    max_turns.to_string()
                }
            ),
            Self::Invalid { label, value } => format!("Unknown {label}: {value}"),
        }
    }
}

pub(crate) fn apply_max_turns_selection(value: &str) -> SettingApplyOutcome {
    value
        .parse::<u32>()
        .map(SettingApplyOutcome::MaxTurns)
        .unwrap_or_else(|_| SettingApplyOutcome::Invalid {
            label: "max turns",
            value: value.to_string(),
        })
}

pub(crate) fn apply_thinking_selection(value: &str) -> SettingApplyOutcome {
    crate::settings::ThinkingLevel::parse(value)
        .map(SettingApplyOutcome::Thinking)
        .unwrap_or_else(|| SettingApplyOutcome::Invalid {
            label: "level",
            value: value.to_string(),
        })
}

pub(crate) fn apply_context_class_selection(value: &str) -> SettingApplyOutcome {
    crate::settings::ContextClass::parse(value)
        .map(SettingApplyOutcome::ContextClass)
        .unwrap_or_else(|| SettingApplyOutcome::Invalid {
            label: "context class",
            value: value.to_string(),
        })
}

pub(crate) fn apply_tool_detail_selection(value: &str) -> SettingApplyOutcome {
    crate::settings::ToolDetail::parse(value)
        .map(SettingApplyOutcome::ToolDetail)
        .unwrap_or_else(|| SettingApplyOutcome::Invalid {
            label: "density",
            value: value.to_string(),
        })
}

pub(crate) fn apply_update_channel_selection(value: &str) -> SettingApplyOutcome {
    crate::update::UpdateChannel::parse(value)
        .map(SettingApplyOutcome::UpdateChannel)
        .unwrap_or_else(|| SettingApplyOutcome::Invalid {
            label: "update channel",
            value: value.to_string(),
        })
}

pub(crate) fn apply_workspace_role_selection(value: &str) -> SettingApplyOutcome {
    crate::workspace::types::WorkspaceRole::parse(value)
        .map(SettingApplyOutcome::WorkspaceRole)
        .unwrap_or_else(|| SettingApplyOutcome::Invalid {
            label: "workspace role",
            value: value.to_string(),
        })
}

pub(crate) fn apply_workspace_kind_selection(value: &str) -> SettingApplyOutcome {
    crate::workspace::types::WorkspaceKind::parse(value)
        .map(SettingApplyOutcome::WorkspaceKind)
        .unwrap_or_else(|| SettingApplyOutcome::Invalid {
            label: "workspace kind",
            value: value.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_descriptor_routes_through_runtime_command() {
        assert_eq!(THINKING_DESCRIPTOR.id, "runtime.thinking");
        assert_eq!(THINKING_DESCRIPTOR.editor, SettingEditorKind::Choice);
        assert_eq!(
            THINKING_DESCRIPTOR.route,
            SettingMutationRoute::RuntimeCommand { command: "think" }
        );
    }

    #[test]
    fn thinking_options_mark_current_level_active() {
        let options = thinking_selector_options(crate::settings::ThinkingLevel::High);
        assert_eq!(options.iter().filter(|option| option.active).count(), 1);
        assert!(
            options
                .iter()
                .any(|option| option.value == "high" && option.active)
        );
    }

    #[test]
    fn context_options_mark_current_class_active() {
        let options = context_class_selector_options(crate::settings::ContextClass::Extended);
        assert_eq!(options.iter().filter(|option| option.active).count(), 1);
        assert!(
            options
                .iter()
                .any(|option| option.value == "Extended" && option.active)
        );
    }

    #[test]
    fn preferences_options_summarize_current_settings() {
        let settings = crate::settings::Settings::default();
        let options = preferences_selector_options(&settings);

        assert!(options.iter().any(|option| option.value == "model"));
        assert!(options.iter().any(|option| {
            option.value == "thinking"
                && option.description == format!("Current: {}", settings.thinking.as_str())
        }));
        assert!(options.iter().any(|option| {
            option.value == "permissions" && option.description == "Configured: none"
        }));
    }

    #[test]
    fn tool_detail_options_mark_current_density_active() {
        let options = tool_detail_selector_options(crate::settings::ToolDetail::Compact);

        assert_eq!(options.iter().filter(|option| option.active).count(), 1);
        assert!(
            options
                .iter()
                .any(|option| option.value == "compact" && option.active)
        );
    }

    #[test]
    fn update_channel_options_mark_current_channel_active() {
        let options = update_channel_selector_options("nightly");

        assert_eq!(options.iter().filter(|option| option.active).count(), 1);
        assert!(
            options
                .iter()
                .any(|option| option.value == "nightly" && option.active)
        );
    }

    #[test]
    fn workspace_options_include_known_values() {
        let roles = workspace_role_selector_options();
        let kinds = workspace_kind_selector_options();

        assert!(roles.iter().any(|option| option.value == "primary"));
        assert!(roles.iter().any(|option| option.value == "cleave-child"));
        assert!(kinds.iter().any(|option| option.value == "code"));
        assert!(kinds.iter().any(|option| option.value == "mixed"));
    }

    #[test]
    fn applying_thinking_selection_parses_valid_level() {
        let outcome = apply_thinking_selection("high");
        assert_eq!(
            outcome,
            SettingApplyOutcome::Thinking(crate::settings::ThinkingLevel::High)
        );
        assert_eq!(outcome.message(), "Thinking → ◉ high");
    }

    #[test]
    fn applying_context_selection_parses_valid_class() {
        let outcome = apply_context_class_selection("Extended");
        assert_eq!(
            outcome,
            SettingApplyOutcome::ContextClass(crate::settings::ContextClass::Extended)
        );
        assert_eq!(outcome.message(), "Context policy → Extended (400k)");
    }

    #[test]
    fn applying_tool_detail_selection_parses_valid_density() {
        let outcome = apply_tool_detail_selection("compact");
        assert_eq!(
            outcome,
            SettingApplyOutcome::ToolDetail(crate::settings::ToolDetail::Compact)
        );
        assert_eq!(outcome.message(), "Tool density → compact");
    }

    #[test]
    fn max_turns_options_and_selection_support_unlimited() {
        let options = max_turns_selector_options(0);
        assert!(
            options.iter().any(|option| {
                option.value == "0" && option.label == "unlimited" && option.active
            })
        );

        let outcome = apply_max_turns_selection("100");
        assert_eq!(outcome, SettingApplyOutcome::MaxTurns(100));
        assert_eq!(outcome.message(), "Max turns → 100");

        let unlimited = apply_max_turns_selection("0");
        assert_eq!(unlimited.message(), "Max turns → unlimited");
    }

    #[test]
    fn applying_update_channel_selection_parses_valid_channel() {
        let outcome = apply_update_channel_selection("nightly");
        assert_eq!(
            outcome,
            SettingApplyOutcome::UpdateChannel(crate::update::UpdateChannel::Nightly)
        );
        assert_eq!(
            outcome.message(),
            "Update channel set to nightly. Rechecking for updates now."
        );
    }

    #[test]
    fn applying_workspace_role_selection_parses_valid_role() {
        let outcome = apply_workspace_role_selection("cleave-child");
        assert_eq!(
            outcome,
            SettingApplyOutcome::WorkspaceRole(crate::workspace::types::WorkspaceRole::CleaveChild)
        );
        assert_eq!(outcome.message(), "Workspace role → cleave-child");
    }

    #[test]
    fn applying_workspace_kind_selection_parses_valid_kind() {
        let outcome = apply_workspace_kind_selection("mixed");
        assert_eq!(
            outcome,
            SettingApplyOutcome::WorkspaceKind(crate::workspace::types::WorkspaceKind::Mixed)
        );
        assert_eq!(outcome.message(), "Workspace kind → mixed");
    }

    #[test]
    fn invalid_setting_selection_reports_label_and_value() {
        let outcome = apply_tool_detail_selection("nope");
        assert_eq!(
            outcome,
            SettingApplyOutcome::Invalid {
                label: "density",
                value: "nope".into()
            }
        );
        assert_eq!(outcome.message(), "Unknown density: nope");
    }

}

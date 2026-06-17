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
    ThinkingLevel,
    ContextClass,
    Persona,
    Tone,
    SecretName,
    LoginProvider,
    VaultConfigure,
    UpdateChannel,
    WorkspaceRole,
    WorkspaceKind,
    Preferences,
    ToolDetail,
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

/// Result of applying a selected settings value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingApplyOutcome {
    Thinking(crate::settings::ThinkingLevel),
    ContextClass(crate::settings::ContextClass),
    Invalid { label: &'static str, value: String },
}

impl SettingApplyOutcome {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Thinking(level) => format!("Thinking → {} {}", level.icon(), level.as_str()),
            Self::ContextClass(class) => format!("Context policy → {}", class.label()),
            Self::Invalid { label, value } => format!("Unknown {label}: {value}"),
        }
    }
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
        assert!(options.iter().any(|option| option.value == "high" && option.active));
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
}

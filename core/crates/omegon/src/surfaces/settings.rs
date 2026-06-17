//! Renderer-neutral settings surface projection.
//!
//! TUI, ACP, CLI, web, and agent-facing tools should render settings from this
//! semantic projection instead of each surface rebuilding its own settings view.

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSurfaceProjection {
    pub tabs: Vec<SettingsTabProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsTabProjection {
    pub id: String,
    pub label: String,
    pub rows: Vec<SettingsRowProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsRowProjection {
    pub id: String,
    pub label: String,
    pub value: String,
    pub description: String,
    pub route: SettingsMutationRouteProjection,
    pub persistence: SettingsPersistenceProjection,
    pub editor: SettingsEditorProjection,
    pub status: SettingsStatusProjection,
    pub choices: Vec<SettingsChoiceProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsChoiceProjection {
    pub value: String,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsEditorProjection {
    Choice,
    Toggle,
    Text,
    Number,
    Action,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsStatusProjection {
    Normal,
    Warning,
    Error,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsMutationRouteProjection {
    RuntimeCommand,
    SharedSettings,
    LocalUi,
    ExternalAction,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPersistenceProjection {
    RuntimeOnly,
    PersistedProfile,
    ProjectPolicy,
    External,
    ReadOnly,
}

impl SettingsSurfaceProjection {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            tabs: vec![
                SettingsTabProjection {
                    id: "runtime".into(),
                    label: "Runtime".into(),
                    rows: vec![
                        choice_row(
                            "runtime.model",
                            "Model",
                            &settings.model,
                            "Active model route for future turns",
                            SettingsMutationRouteProjection::RuntimeCommand,
                            SettingsPersistenceProjection::RuntimeOnly,
                            vec![],
                        ),
                        choice_row(
                            "runtime.thinking",
                            "Thinking",
                            &format!("{} {}", settings.thinking.icon(), settings.thinking.as_str()),
                            "Reasoning budget requested from capable providers",
                            SettingsMutationRouteProjection::RuntimeCommand,
                            SettingsPersistenceProjection::RuntimeOnly,
                            crate::settings::ThinkingLevel::all()
                                .iter()
                                .map(|level| SettingsChoiceProjection {
                                    value: level.as_str().into(),
                                    label: format!("{} {}", level.icon(), level.as_str()),
                                    active: *level == settings.thinking,
                                })
                                .collect(),
                        ),
                        choice_row(
                            "runtime.context_class",
                            "Context class",
                            settings.context_class.label(),
                            &format!("Context window: {} tokens", settings.context_window),
                            SettingsMutationRouteProjection::RuntimeCommand,
                            SettingsPersistenceProjection::RuntimeOnly,
                            crate::settings::ContextClass::all()
                                .iter()
                                .map(|class| SettingsChoiceProjection {
                                    value: class.short().into(),
                                    label: class.label().into(),
                                    active: *class == settings.context_class,
                                })
                                .collect(),
                        ),
                        row(
                            "runtime.max_turns",
                            "Max turns",
                            &settings.max_turns.to_string(),
                            "Maximum autonomous turns for the current run",
                            SettingsMutationRouteProjection::SharedSettings,
                            SettingsPersistenceProjection::RuntimeOnly,
                            SettingsEditorProjection::Number,
                        ),
                    ],
                },
                SettingsTabProjection {
                    id: "ui".into(),
                    label: "UI".into(),
                    rows: vec![choice_row(
                        "ui.tool_detail",
                        "Tool display",
                        settings.tool_detail.as_str(),
                        "Density of tool-call summaries in interactive surfaces",
                        SettingsMutationRouteProjection::SharedSettings,
                        SettingsPersistenceProjection::PersistedProfile,
                        ["lean", "compact", "detailed", "verbose"]
                            .into_iter()
                            .map(|value| SettingsChoiceProjection {
                                value: value.into(),
                                label: value.into(),
                                active: value == settings.tool_detail.as_str(),
                            })
                            .collect(),
                    )],
                },
                SettingsTabProjection {
                    id: "workspace".into(),
                    label: "Workspace".into(),
                    rows: vec![
                        row(
                            "workspace.trusted_directories",
                            "Trusted dirs",
                            &trusted_dir_value(settings),
                            "Directories outside the workspace allowed without repeated prompts",
                            SettingsMutationRouteProjection::RuntimeCommand,
                            SettingsPersistenceProjection::PersistedProfile,
                            SettingsEditorProjection::Action,
                        ),
                        row(
                            "workspace.sandbox",
                            "Sandbox",
                            if settings.sandbox { "enabled" } else { "disabled" },
                            "Run delegate/cleave children inside OCI isolation when available",
                            SettingsMutationRouteProjection::RuntimeCommand,
                            SettingsPersistenceProjection::PersistedProfile,
                            SettingsEditorProjection::Toggle,
                        ),
                    ],
                },
                SettingsTabProjection {
                    id: "updates".into(),
                    label: "Updates".into(),
                    rows: vec![
                        choice_row(
                            "updates.channel",
                            "Channel",
                            &settings.update_channel,
                            "Release stream used by update checks",
                            SettingsMutationRouteProjection::SharedSettings,
                            SettingsPersistenceProjection::PersistedProfile,
                            ["stable", "nightly"]
                                .into_iter()
                                .map(|value| SettingsChoiceProjection {
                                    value: value.into(),
                                    label: value.into(),
                                    active: value == settings.update_channel,
                                })
                                .collect(),
                        ),
                        row(
                            "updates.auto_update",
                            "Auto update",
                            if settings.auto_update { "on" } else { "off" },
                            "Install discovered updates between sessions",
                            SettingsMutationRouteProjection::SharedSettings,
                            SettingsPersistenceProjection::PersistedProfile,
                            SettingsEditorProjection::Toggle,
                        ),
                    ],
                },
            ],
        }
    }

    pub fn render_markdown(&self) -> String {
        let mut out = String::from("## Current Harness Settings\n");
        for tab in &self.tabs {
            out.push_str("\n### ");
            out.push_str(&tab.label);
            out.push('\n');
            for row in &tab.rows {
                out.push_str("\n- **");
                out.push_str(&row.label);
                out.push_str("**: ");
                out.push_str(&row.value);
                if !row.description.is_empty() {
                    out.push_str(" — ");
                    out.push_str(&row.description);
                }
            }
            out.push('\n');
        }
        out
    }
}

fn row(
    id: &str,
    label: &str,
    value: &str,
    description: &str,
    route: SettingsMutationRouteProjection,
    persistence: SettingsPersistenceProjection,
    editor: SettingsEditorProjection,
) -> SettingsRowProjection {
    SettingsRowProjection {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        description: description.into(),
        route,
        persistence,
        editor,
        status: SettingsStatusProjection::Normal,
        choices: vec![],
    }
}

fn choice_row(
    id: &str,
    label: &str,
    value: &str,
    description: &str,
    route: SettingsMutationRouteProjection,
    persistence: SettingsPersistenceProjection,
    choices: Vec<SettingsChoiceProjection>,
) -> SettingsRowProjection {
    SettingsRowProjection {
        editor: SettingsEditorProjection::Choice,
        choices,
        ..row(
            id,
            label,
            value,
            description,
            route,
            persistence,
            SettingsEditorProjection::Choice,
        )
    }
}

fn trusted_dir_value(settings: &Settings) -> String {
    if settings.trusted_directories.is_empty() {
        "none".into()
    } else {
        settings.trusted_directories.len().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_contains_settings_tabs() {
        let settings = Settings::new("test-model");
        let projection = SettingsSurfaceProjection::from_settings(&settings);

        assert!(projection.tabs.iter().any(|tab| tab.id == "runtime"));
        assert!(projection.tabs.iter().any(|tab| tab.id == "ui"));
        assert!(projection.tabs.iter().any(|tab| tab.id == "workspace"));
        assert!(projection.tabs.iter().any(|tab| tab.id == "updates"));
    }

    #[test]
    fn projection_contains_choice_metadata() {
        let settings = Settings::new("test-model");
        let projection = SettingsSurfaceProjection::from_settings(&settings);
        let runtime = projection.tabs.iter().find(|tab| tab.id == "runtime").unwrap();
        let thinking = runtime.rows.iter().find(|row| row.id == "runtime.thinking").unwrap();

        assert_eq!(thinking.editor, SettingsEditorProjection::Choice);
        assert!(thinking.choices.iter().any(|choice| choice.active));
    }

    #[test]
    fn markdown_renders_projection_rows() {
        let settings = Settings::new("test-model");
        let markdown = SettingsSurfaceProjection::from_settings(&settings).render_markdown();

        assert!(markdown.contains("Current Harness Settings"));
        assert!(markdown.contains("**Model**: test-model"));
        assert!(markdown.contains("### UI"));
        assert!(markdown.contains("**Tool display**"));
    }
}

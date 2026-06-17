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
            tabs: vec![SettingsTabProjection {
                id: "runtime".into(),
                label: "Runtime".into(),
                rows: vec![
                    SettingsRowProjection {
                        id: "runtime.model".into(),
                        label: "Model".into(),
                        value: settings.model.clone(),
                        description: "Active model route for future turns".into(),
                        route: SettingsMutationRouteProjection::RuntimeCommand,
                        persistence: SettingsPersistenceProjection::RuntimeOnly,
                    },
                    SettingsRowProjection {
                        id: "runtime.thinking".into(),
                        label: "Thinking".into(),
                        value: format!("{} {}", settings.thinking.icon(), settings.thinking.as_str()),
                        description: "Reasoning budget requested from capable providers".into(),
                        route: SettingsMutationRouteProjection::RuntimeCommand,
                        persistence: SettingsPersistenceProjection::RuntimeOnly,
                    },
                    SettingsRowProjection {
                        id: "runtime.context_class".into(),
                        label: "Context class".into(),
                        value: settings.context_class.label().to_string(),
                        description: format!("Context window: {} tokens", settings.context_window),
                        route: SettingsMutationRouteProjection::RuntimeCommand,
                        persistence: SettingsPersistenceProjection::RuntimeOnly,
                    },
                    SettingsRowProjection {
                        id: "runtime.max_turns".into(),
                        label: "Max turns".into(),
                        value: settings.max_turns.to_string(),
                        description: "Maximum autonomous turns for the current run".into(),
                        route: SettingsMutationRouteProjection::SharedSettings,
                        persistence: SettingsPersistenceProjection::RuntimeOnly,
                    },
                    SettingsRowProjection {
                        id: "ui.tool_detail".into(),
                        label: "Tool display".into(),
                        value: settings.tool_detail.as_str().to_string(),
                        description: "Density of tool-call summaries in interactive surfaces".into(),
                        route: SettingsMutationRouteProjection::SharedSettings,
                        persistence: SettingsPersistenceProjection::PersistedProfile,
                    },
                ],
            }],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_contains_runtime_settings() {
        let settings = Settings::new("test-model");
        let projection = SettingsSurfaceProjection::from_settings(&settings);

        assert_eq!(projection.tabs[0].id, "runtime");
        assert!(projection.tabs[0].rows.iter().any(|row| {
            row.id == "runtime.model" && row.value == "test-model"
        }));
        assert!(projection.tabs[0].rows.iter().any(|row| row.id == "ui.tool_detail"));
    }

    #[test]
    fn markdown_renders_projection_rows() {
        let settings = Settings::new("test-model");
        let markdown = SettingsSurfaceProjection::from_settings(&settings).render_markdown();

        assert!(markdown.contains("Current Harness Settings"));
        assert!(markdown.contains("**Model**: test-model"));
        assert!(markdown.contains("**Tool display**"));
    }
}

//! Widget types and events for extension UI rendering.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Widget declaration — from RPC response or manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetDeclaration {
    pub id: String,
    pub label: String,
    pub kind: String,  // "stateful" | "ephemeral"
    pub renderer: String,  // "timeline", "tree", "table", etc.
    #[serde(default)]
    pub description: String,
}

/// Events from extension widget server (via TCP)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WidgetEvent {
    /// Update widget state (stateful widgets)
    #[serde(rename = "update")]
    Update {
        widget_id: String,
        data: Value,
        #[serde(default)]
        title: Option<String>,
    },
    /// Show ephemeral modal
    #[serde(rename = "show_modal")]
    ShowModal {
        widget_id: String,
        data: Value,
        #[serde(default)]
        auto_dismiss_ms: Option<u64>,
    },
    /// Request action from user
    #[serde(rename = "action_required")]
    ActionRequired {
        widget_id: String,
        actions: Vec<String>,
    },
}

/// Widget state held by TUI
#[derive(Debug, Clone)]
pub struct ExtensionTabWidget {
    pub widget_id: String,
    pub label: String,
    pub renderer: String,
    pub current_data: Value,
    pub kind: String,  // "stateful" | "ephemeral"
}

impl ExtensionTabWidget {
    pub fn new(
        widget_id: String,
        label: String,
        renderer: String,
        kind: String,
    ) -> Self {
        Self {
            widget_id,
            label,
            renderer,
            current_data: Value::Object(Default::default()),
            kind,
        }
    }

    pub fn update(&mut self, data: Value) {
        self.current_data = data;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_declaration_serialization() {
        let widget = WidgetDeclaration {
            id: "timeline".to_string(),
            label: "Timeline".to_string(),
            kind: "stateful".to_string(),
            renderer: "timeline".to_string(),
            description: "Work timeline".to_string(),
        };

        let json = serde_json::to_string(&widget).unwrap();
        let deserialized: WidgetDeclaration = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "timeline");
    }

    #[test]
    fn widget_event_update_parsing() {
        let json = r#"{"type":"update","widget_id":"timeline","data":{},"title":"Updated"}"#;
        let event: WidgetEvent = serde_json::from_str(json).unwrap();
        match event {
            WidgetEvent::Update { widget_id, title, .. } => {
                assert_eq!(widget_id, "timeline");
                assert_eq!(title, Some("Updated".to_string()));
            }
            _ => panic!("expected Update event"),
        }
    }
}

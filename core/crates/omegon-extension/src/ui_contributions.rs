//! Declarative operator-surface contribution protocol types.
//!
//! Extensions declare commands, passive status items, and host-managed surfaces.
//! The host validates these declarations against the extension manifest and owns
//! rendering, routing, placement, focus, and policy.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiContributionSet {
    pub version: u16,
    pub namespace: UiNamespace,
    #[serde(default)]
    pub contributions: Vec<UiContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiNamespace {
    pub requested: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiContribution {
    Command(CommandContribution),
    StatusItem(StatusItemContribution),
    Surface(SurfaceContribution),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
    pub slash: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_schema: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusItemContribution {
    pub id: String,
    pub title: String,
    pub refresh_tool: String,
    pub refresh_interval_ms: u64,
    pub template: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRendering {
    Host,
    Delegated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfacePlacement {
    SidePane,
    BottomPane,
    Modal,
    NewTab,
    External,
    BackgroundSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceContribution {
    pub id: String,
    pub title: String,
    pub surface_type: String,
    pub rendering: SurfaceRendering,
    #[serde(default)]
    pub preferred_placements: Vec<SurfacePlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<PrimitiveView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "primitive", rename_all = "snake_case")]
pub enum PrimitiveView {
    List(ListPrimitiveView),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListPrimitiveView {
    pub data_tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ListItemTemplate>,
    #[serde(default)]
    pub actions: Vec<PrimitiveAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItemTemplate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimitiveAction {
    pub id: String,
    pub title: String,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reader_delegated_surface_round_trips() {
        let value = json!({
            "version": 1,
            "namespace": {"requested": "reader", "fallback": "omegon-reader"},
            "contributions": [{
                "kind": "surface",
                "id": "reader",
                "title": "Reader",
                "surface_type": "document_reader",
                "rendering": "delegated",
                "preferred_placements": ["side_pane", "new_tab", "external", "background_session"],
                "open_tool": "reader_open",
                "status_tool": "reader_status"
            }]
        });
        let parsed: UiContributionSet = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed.namespace.requested, "reader");
        match &parsed.contributions[0] {
            UiContribution::Surface(surface) => {
                assert_eq!(surface.rendering, SurfaceRendering::Delegated);
                assert_eq!(surface.surface_type, "document_reader");
                assert_eq!(surface.preferred_placements[0], SurfacePlacement::SidePane);
                assert!(surface.view.is_none());
            }
            other => panic!("expected surface, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    #[test]
    fn scratchpad_host_list_surface_round_trips() {
        let value = json!({
            "version": 1,
            "namespace": {"requested": "scratchpad"},
            "contributions": [{
                "kind": "surface",
                "id": "scratchpad",
                "title": "Scratchpad",
                "surface_type": "primitive_view",
                "rendering": "host",
                "preferred_placements": ["side_pane", "modal"],
                "view": {
                    "primitive": "list",
                    "data_tool": "scratchpad_list",
                    "item": {"title": "{title}", "subtitle": "{body_preview}", "badge": "{tag_count}"},
                    "actions": [{"id": "open", "title": "Open", "tool": "scratchpad_get", "args": {"id": "{id}"}}]
                }
            }]
        });
        let parsed: UiContributionSet = serde_json::from_value(value.clone()).unwrap();
        match &parsed.contributions[0] {
            UiContribution::Surface(surface) => {
                assert_eq!(surface.rendering, SurfaceRendering::Host);
                match surface.view.as_ref().unwrap() {
                    PrimitiveView::List(list) => {
                        assert_eq!(list.data_tool, "scratchpad_list");
                        assert_eq!(list.actions[0].tool, "scratchpad_get");
                    }
                }
            }
            other => panic!("expected surface, got {other:?}"),
        }
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
}

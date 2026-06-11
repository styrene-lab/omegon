use async_trait::async_trait;
use omegon_headroom::{
    CompressionInput, ContentKind, HeadroomPolicy, HeadroomRef, HeadroomStorePolicy,
};
use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::settings::SharedSettings;

pub struct HeadroomFeature {
    settings: SharedSettings,
    store: crate::tools::headroom_support::SharedHeadroomStore,
}

impl HeadroomFeature {
    pub fn new(
        settings: SharedSettings,
        store: crate::tools::headroom_support::SharedHeadroomStore,
    ) -> Self {
        Self { settings, store }
    }
}

#[async_trait]
impl Feature for HeadroomFeature {
    fn name(&self) -> &str {
        "headroom"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::headroom::HEADROOM_COMPRESS.into(),
                label: "headroom_compress".into(),
                description: "Manually compress text through Omegon's experimental native headroom compressor. Stores exact originals behind CCR retrieval handles when reversible mode is enabled.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Text to compress." },
                        "source": { "type": "string", "description": "Optional source label for stats/retrieval metadata." },
                        "kind_hint": {
                            "type": "string",
                            "enum": ["json", "code", "log", "diff", "markdown", "plain_text"],
                            "description": "Optional content kind override. Omit to auto-detect."
                        },
                        "min_bytes": { "type": "integer", "minimum": 1, "description": "Optional per-call compression threshold." },
                        "target_bytes": { "type": "integer", "minimum": 1, "description": "Optional per-call compressed target size." },
                        "force": { "type": "boolean", "description": "Compress even when the experimental runtime gate is off." }
                    },
                    "required": ["text"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::headroom::HEADROOM_RETRIEVE.into(),
                label: "headroom_retrieve".into(),
                description: "Retrieve an exact original payload previously stored by headroom_compress.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Headroom retrieval id, e.g. hr:<sha256-prefix>." },
                        "max_bytes": { "type": "integer", "minimum": 1, "description": "Optional maximum bytes to return from the original." }
                    },
                    "required": ["id"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::TargetedRepoInspection],
            },
            ToolDefinition {
                name: crate::tool_registry::headroom::HEADROOM_STATS.into(),
                label: "headroom_stats".into(),
                description: "Report experimental native headroom settings and session CCR store statistics.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::headroom::HEADROOM_COMPRESS => self.compress(args),
            crate::tool_registry::headroom::HEADROOM_RETRIEVE => self.retrieve(args),
            crate::tool_registry::headroom::HEADROOM_STATS => self.stats(),
            other => Ok(error_result(&format!("unknown headroom tool: {other}"))),
        }
    }
}

impl HeadroomFeature {
    fn compress(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
            return Ok(error_result("missing required string field: text"));
        };
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("manual")
            .to_owned();
        let kind_hint = match args.get("kind_hint").and_then(|v| v.as_str()) {
            Some(value) => match parse_content_kind(value) {
                Some(kind) => Some(kind),
                None => return Ok(error_result(&format!("unknown kind_hint: {value}"))),
            },
            None => None,
        };

        let settings = self.settings.lock().unwrap().headroom.clone();
        if !force && !settings.effective_enabled() {
            return Ok(error_result(
                "headroom compression is experimental and currently off; use harness_settings action=set_headroom_compression value=manual/on, or pass force=true for this explicit call",
            ));
        }

        let min_bytes = args
            .get("min_bytes")
            .and_then(|v| v.as_u64())
            .map(|value| value as usize)
            .unwrap_or(settings.min_bytes);
        let target_bytes = args
            .get("target_bytes")
            .and_then(|v| v.as_u64())
            .map(|value| value as usize)
            .unwrap_or(settings.target_bytes);
        let policy = HeadroomPolicy {
            enabled: true,
            min_bytes,
            target_bytes,
            reversible: settings.reversible,
            ..HeadroomPolicy::default()
        };

        let mut store = self.store.lock().unwrap();
        store.set_policy(HeadroomStorePolicy {
            max_objects: settings.max_store_objects,
            max_original_bytes: settings.max_store_bytes,
        });
        let output = store.compress(CompressionInput {
            kind_hint,
            source,
            text: text.to_owned(),
            policy,
        });
        let store_stats = store.stats();
        drop(store);

        let ref_details = output.original_ref.as_ref().map(reference_json);
        let text = if output.compressed {
            output.text.clone()
        } else {
            format!(
                "[headroom: not compressed; original_bytes={} min_bytes={}]\n{}",
                output.stats.original_bytes, min_bytes, output.text
            )
        };

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details: json!({
                "headroom": {
                    "compressed": output.compressed,
                    "content_kind": output.content_kind,
                    "stats": output.stats,
                    "original_ref": ref_details,
                    "settings": {
                        "mode": settings.mode.as_str(),
                        "reversible": settings.reversible,
                        "collect_metrics": settings.collect_metrics,
                        "local_model": settings.local_model,
                    },
                    "store": store_stats,
                }
            }),
        })
    }

    fn retrieve(&self, args: Value) -> anyhow::Result<ToolResult> {
        let Some(id) = args.get("id").and_then(|v| v.as_str()) else {
            return Ok(error_result("missing required string field: id"));
        };
        let max_bytes = args
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .map(|value| value as usize);
        let store = self.store.lock().unwrap();
        let stored = match store.retrieve(id) {
            Ok(stored) => stored.clone(),
            Err(err) => return Ok(error_result(&err.to_string())),
        };
        drop(store);

        let mut text = stored.text.clone();
        let truncated = max_bytes.is_some_and(|limit| text.len() > limit);
        if let Some(limit) = max_bytes {
            text = truncate_utf8(&text, limit);
        }
        Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details: json!({
                "headroom": {
                    "retrieved": true,
                    "truncated": truncated,
                    "reference": reference_json(&stored.reference),
                    "source": stored.source,
                }
            }),
        })
    }

    fn stats(&self) -> anyhow::Result<ToolResult> {
        let settings = self.settings.lock().unwrap().headroom.clone();
        let mut store = self.store.lock().unwrap();
        store.set_policy(HeadroomStorePolicy {
            max_objects: settings.max_store_objects,
            max_original_bytes: settings.max_store_bytes,
        });
        let store_stats = store.stats();
        let objects = store
            .objects()
            .map(|stored| {
                json!({
                    "id": stored.reference.id,
                    "bytes": stored.reference.bytes,
                    "content_kind": stored.reference.content_kind,
                    "source": stored.source,
                })
            })
            .collect::<Vec<_>>();
        let out = format!(
            "## Headroom Stats\n\n- **Enabled**: {}\n- **Mode**: {}\n- **Reversible**: {}\n- **Metrics**: {}\n- **Min bytes**: {}\n- **Target bytes**: {}\n- **Local model**: {}\n- **Stored originals**: {}\n- **Stored original bytes**: {}\n- **Store max objects**: {}\n- **Store max bytes**: {}\n- **Evicted originals**: {}",
            settings.enabled,
            settings.mode.as_str(),
            settings.reversible,
            settings.collect_metrics,
            settings.min_bytes,
            settings.target_bytes,
            settings.local_model.as_deref().unwrap_or("deterministic"),
            store_stats.objects,
            store_stats.original_bytes,
            store_stats.max_objects,
            store_stats.max_original_bytes,
            store_stats.evicted_count,
        );
        Ok(ToolResult {
            content: vec![ContentBlock::Text { text: out }],
            details: json!({
                "headroom": {
                    "settings": {
                        "enabled": settings.enabled,
                        "mode": settings.mode.as_str(),
                        "effective_enabled": settings.effective_enabled(),
                        "reversible": settings.reversible,
                        "collect_metrics": settings.collect_metrics,
                        "min_bytes": settings.min_bytes,
                        "target_bytes": settings.target_bytes,
                        "max_store_objects": settings.max_store_objects,
                        "max_store_bytes": settings.max_store_bytes,
                        "local_model": settings.local_model,
                    },
                    "store": {
                        "stats": store_stats,
                        "items": objects,
                    }
                }
            }),
        })
    }
}

fn parse_content_kind(value: &str) -> Option<ContentKind> {
    match value {
        "json" => Some(ContentKind::Json),
        "code" => Some(ContentKind::Code),
        "log" => Some(ContentKind::Log),
        "diff" => Some(ContentKind::Diff),
        "markdown" => Some(ContentKind::Markdown),
        "plain_text" | "plain" | "text" => Some(ContentKind::PlainText),
        _ => None,
    }
}

fn reference_json(reference: &HeadroomRef) -> Value {
    json!({
        "id": reference.id,
        "sha256": reference.sha256,
        "bytes": reference.bytes,
        "content_kind": reference.content_kind,
    })
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!(
        "{}\n[headroom: retrieved output clipped to max_bytes={max_bytes}]",
        &text[..boundary]
    )
}

fn error_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!("Error: {text}"),
        }],
        details: json!({ "error": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HeadroomCompressionMode, HeadroomRuntimeConfig, Settings};
    use std::sync::{Arc, Mutex as StdMutex};

    fn feature_with_headroom(mode: HeadroomCompressionMode) -> HeadroomFeature {
        feature_with_config(HeadroomRuntimeConfig {
            enabled: !matches!(mode, HeadroomCompressionMode::Off),
            mode,
            min_bytes: 1,
            target_bytes: 1024,
            ..HeadroomRuntimeConfig::default()
        })
    }

    fn feature_with_config(headroom: HeadroomRuntimeConfig) -> HeadroomFeature {
        let mut settings = Settings::new("test-model");
        settings.headroom = headroom;
        HeadroomFeature::new(
            Arc::new(StdMutex::new(settings)),
            crate::tools::headroom_support::new_shared_store(),
        )
    }

    fn result_text(result: &ToolResult) -> &str {
        match &result.content[0] {
            ContentBlock::Text { text } => text,
            _ => panic!("expected text result"),
        }
    }

    #[test]
    fn exposes_three_tools() {
        let feature = feature_with_headroom(HeadroomCompressionMode::Manual);
        let tools = feature.tools();
        let names = tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&crate::tool_registry::headroom::HEADROOM_COMPRESS));
        assert!(names.contains(&crate::tool_registry::headroom::HEADROOM_RETRIEVE));
        assert!(names.contains(&crate::tool_registry::headroom::HEADROOM_STATS));
    }

    #[test]
    fn rejects_compress_when_gate_off_without_force() {
        let feature = feature_with_headroom(HeadroomCompressionMode::Off);
        let result = feature.compress(json!({"text": "hello"})).unwrap();
        assert!(result_text(&result).contains("currently off"));
    }

    #[test]
    fn compress_and_retrieve_round_trip() {
        let feature = feature_with_headroom(HeadroomCompressionMode::Manual);
        let text = (0..200)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = feature
            .compress(json!({"text": text, "kind_hint": "plain_text"}))
            .unwrap();
        let reference_id = result.details["headroom"]["original_ref"]["id"]
            .as_str()
            .expect("reference id")
            .to_owned();
        let retrieved = feature.retrieve(json!({"id": reference_id})).unwrap();
        assert!(result_text(&retrieved).contains("line 199"));
    }

    #[test]
    fn stats_reports_store_budget_and_evictions() {
        let feature = feature_with_config(HeadroomRuntimeConfig {
            enabled: true,
            mode: HeadroomCompressionMode::Manual,
            min_bytes: 1,
            target_bytes: 1024,
            max_store_objects: 1,
            max_store_bytes: 1024 * 1024,
            ..HeadroomRuntimeConfig::default()
        });
        let first = large_text("first");
        let second = large_text("second");
        feature
            .compress(json!({"text": first, "kind_hint": "log", "source": "first"}))
            .unwrap();
        feature
            .compress(json!({"text": second, "kind_hint": "log", "source": "second"}))
            .unwrap();

        let stats = feature.stats().unwrap();
        let text = result_text(&stats);
        assert!(text.contains("Store max objects"));
        assert!(text.contains("Evicted originals"));
        assert_eq!(
            stats.details["headroom"]["store"]["stats"]["max_objects"],
            1
        );
        assert_eq!(
            stats.details["headroom"]["store"]["stats"]["evicted_count"],
            1
        );
    }

    #[test]
    fn retrieve_reports_unknown_after_eviction() {
        let feature = feature_with_config(HeadroomRuntimeConfig {
            enabled: true,
            mode: HeadroomCompressionMode::Manual,
            min_bytes: 1,
            target_bytes: 1024,
            max_store_objects: 1,
            max_store_bytes: 1024 * 1024,
            ..HeadroomRuntimeConfig::default()
        });
        let first = feature
            .compress(json!({"text": large_text("first"), "kind_hint": "log"}))
            .unwrap();
        let first_id = first.details["headroom"]["original_ref"]["id"]
            .as_str()
            .expect("first id")
            .to_owned();
        feature
            .compress(json!({"text": large_text("second"), "kind_hint": "log"}))
            .unwrap();

        let retrieved = feature.retrieve(json!({"id": first_id})).unwrap();
        assert!(result_text(&retrieved).contains("unknown headroom object"));
    }
}

fn large_text(label: &str) -> String {
    (0..200)
        .map(|i| format!("{label} info line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

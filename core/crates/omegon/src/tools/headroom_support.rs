use std::sync::{Arc, Mutex};

use omegon_headroom::{
    CompressionInput, ContentKind, HeadroomPolicy, HeadroomStorePolicy, InMemoryHeadroomStore,
};
use serde_json::{Value, json};

pub type SharedHeadroomStore = Arc<Mutex<InMemoryHeadroomStore>>;

pub fn new_shared_store() -> SharedHeadroomStore {
    Arc::new(Mutex::new(InMemoryHeadroomStore::default()))
}

#[derive(Debug, Clone)]
pub struct HeadroomToolTextResult {
    pub text: String,
    pub details: Option<Value>,
}

pub fn maybe_compress_tool_text(
    settings: Option<&crate::settings::SharedSettings>,
    store: Option<&SharedHeadroomStore>,
    source: &str,
    kind_hint: Option<ContentKind>,
    text: String,
) -> HeadroomToolTextResult {
    let original_bytes = text.len();

    let Some(settings) = settings else {
        return HeadroomToolTextResult {
            text,
            details: None,
        };
    };
    let headroom = match settings.lock() {
        Ok(settings) => settings.headroom.clone(),
        Err(_) => {
            return HeadroomToolTextResult {
                text,
                details: None,
            };
        }
    };
    if !headroom.effective_enabled()
        || !matches!(headroom.mode, crate::settings::HeadroomCompressionMode::On)
    {
        return HeadroomToolTextResult {
            text,
            details: None,
        };
    }
    let effective_kind = kind_hint.unwrap_or_else(|| omegon_headroom::detect_kind(&text));
    let effective_kind_name = content_kind_name(effective_kind);
    if !headroom.allows_auto_kind_name(effective_kind_name) {
        return HeadroomToolTextResult {
            text,
            details: Some(json!({
                "compressed": false,
                "reason": "content_kind_not_auto_enabled",
                "original_bytes": original_bytes,
                "content_kind": effective_kind,
                "auto_allowed": false,
                "auto_kinds": headroom.auto_kinds,
                "mode": headroom.mode.as_str(),
            })),
        };
    }
    if text.len() < headroom.min_bytes {
        return HeadroomToolTextResult {
            text,
            details: Some(json!({
                "compressed": false,
                "reason": "below_min_bytes",
                "original_bytes": original_bytes,
                "content_kind": effective_kind,
                "auto_allowed": true,
                "min_bytes": headroom.min_bytes,
                "mode": headroom.mode.as_str(),
            })),
        };
    }
    let Some(store) = store else {
        return HeadroomToolTextResult {
            text,
            details: Some(json!({
                "compressed": false,
                "reason": "missing_store",
                "original_bytes": original_bytes,
                "mode": headroom.mode.as_str(),
            })),
        };
    };

    let policy = HeadroomPolicy {
        enabled: true,
        min_bytes: headroom.min_bytes,
        target_bytes: headroom.target_bytes,
        reversible: headroom.reversible,
        ..HeadroomPolicy::default()
    };
    let mut store = match store.lock() {
        Ok(store) => store,
        Err(_) => {
            return HeadroomToolTextResult {
                text,
                details: Some(json!({
                    "compressed": false,
                    "reason": "store_lock_poisoned",
                    "original_bytes": original_bytes,
                    "mode": headroom.mode.as_str(),
                })),
            };
        }
    };
    store.set_policy(HeadroomStorePolicy {
        max_objects: headroom.max_store_objects,
        max_original_bytes: headroom.max_store_bytes,
    });
    let output = store.compress(CompressionInput {
        kind_hint: Some(effective_kind),
        source: source.to_owned(),
        text,
        policy,
    });
    let store_stats = store.stats();
    drop(store);

    HeadroomToolTextResult {
        text: output.text,
        details: Some(json!({
            "compressed": output.compressed,
            "content_kind": output.content_kind,
            "stats": output.stats,
            "provider": output.provider,
            "original_ref": output.original_ref,
            "mode": headroom.mode.as_str(),
            "reversible": headroom.reversible,
            "store": store_stats,
        })),
    }
}

fn content_kind_name(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Json => "json",
        ContentKind::Code => "code",
        ContentKind::Log => "log",
        ContentKind::Diff => "diff",
        ContentKind::Markdown => "markdown",
        ContentKind::PlainText => "plain_text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{HeadroomCompressionMode, HeadroomRuntimeConfig, Settings};
    use std::sync::{Arc, Mutex as StdMutex};

    fn settings_with_headroom(
        mut headroom: HeadroomRuntimeConfig,
    ) -> crate::settings::SharedSettings {
        headroom.enabled = true;
        headroom.mode = HeadroomCompressionMode::On;
        headroom.min_bytes = 1;
        headroom.target_bytes = 1024;
        let mut settings = Settings::new("test-model");
        settings.headroom = headroom;
        Arc::new(StdMutex::new(settings))
    }

    fn large_json() -> String {
        let rows = (0..120)
            .map(|i| format!(r#"{{"id":{i},"status":"ok","message":"heartbeat accepted"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    #[test]
    fn auto_kind_gate_allows_json_by_default() {
        let settings = settings_with_headroom(HeadroomRuntimeConfig::default());
        let store = new_shared_store();
        let result = maybe_compress_tool_text(
            Some(&settings),
            Some(&store),
            "json",
            Some(ContentKind::Json),
            large_json(),
        );
        let details = result.details.expect("headroom details");
        assert_eq!(details["compressed"], true);
        assert_eq!(details["content_kind"], "json");
    }

    #[test]
    fn auto_kind_gate_blocks_markdown_by_default() {
        let settings = settings_with_headroom(HeadroomRuntimeConfig::default());
        let store = new_shared_store();
        let text = (0..120)
            .map(|i| format!("# Heading {i}\nbody"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = maybe_compress_tool_text(
            Some(&settings),
            Some(&store),
            "markdown",
            Some(ContentKind::Markdown),
            text,
        );
        let details = result.details.expect("headroom details");
        assert_eq!(details["compressed"], false);
        assert_eq!(details["reason"], "content_kind_not_auto_enabled");
        assert_eq!(details["auto_allowed"], false);
    }

    #[test]
    fn auto_kind_gate_allows_markdown_when_configured() {
        let headroom = HeadroomRuntimeConfig {
            auto_kinds: vec!["markdown".into()],
            ..HeadroomRuntimeConfig::default()
        };
        let settings = settings_with_headroom(headroom);
        let store = new_shared_store();
        let text = (0..120)
            .map(|i| format!("# Heading {i}\nbody"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = maybe_compress_tool_text(
            Some(&settings),
            Some(&store),
            "markdown",
            Some(ContentKind::Markdown),
            text,
        );
        let details = result.details.expect("headroom details");
        assert_eq!(details["compressed"], true);
        assert_eq!(details["content_kind"], "markdown");
    }
}

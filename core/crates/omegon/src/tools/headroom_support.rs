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
    if text.len() < headroom.min_bytes {
        return HeadroomToolTextResult {
            text,
            details: Some(json!({
                "compressed": false,
                "reason": "below_min_bytes",
                "original_bytes": original_bytes,
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
        kind_hint,
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
            "original_ref": output.original_ref,
            "mode": headroom.mode.as_str(),
            "reversible": headroom.reversible,
            "store": store_stats,
        })),
    }
}

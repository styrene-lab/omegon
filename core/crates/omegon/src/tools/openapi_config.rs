//! Loads OpenAPI tool configurations from `.omegon/openapi.toml`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::openapi::OpenApiConfig;

/// Per-entry TOML representation (mirrors `OpenApiConfig` but with raw strings).
#[derive(Debug, Deserialize)]
struct RawEntry {
    spec: String,
    auth: String,
    secret: String,
    base_url_override: Option<String>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    confirm: Vec<String>,
    #[serde(default)]
    read_only: bool,
}

/// Load all OpenAPI configs from `<project_root>/.omegon/openapi.toml`.
///
/// Returns an empty vec if the file is missing. Logs a warning and returns
/// empty if the file exists but cannot be parsed.
pub fn load_openapi_configs(project_root: &Path) -> Vec<(String, OpenApiConfig)> {
    let config_path = project_root.join(".omegon/openapi.toml");

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "failed to read openapi config");
            return Vec::new();
        }
    };

    let raw: HashMap<String, RawEntry> = match toml::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "failed to parse openapi config");
            return Vec::new();
        }
    };

    raw.into_iter()
        .map(|(name, entry)| {
            let spec = resolve_spec(&entry.spec, project_root);
            let config = OpenApiConfig {
                spec,
                auth: entry.auth,
                secret: entry.secret,
                base_url_override: entry.base_url_override,
                allow: entry.allow,
                confirm: entry.confirm,
                read_only: entry.read_only,
            };
            (name, config)
        })
        .collect()
}

/// Resolve a spec path: URLs are kept as-is, relative paths are resolved
/// against `project_root`.
fn resolve_spec(spec: &str, project_root: &Path) -> String {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        spec.to_string()
    } else {
        project_root.join(spec).display().to_string()
    }
}

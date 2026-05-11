//! Loads OpenAPI tool configurations from `.omegon/openapi.toml` and
//! auto-discovers spec files in `.omegon/apis/`.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::openapi::OpenApiConfig;

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

/// Load OpenAPI configs from two sources:
///
/// 1. `.omegon/openapi.toml` — explicit per-API config (auth, filters, etc.)
/// 2. `.omegon/apis/*.{yaml,json}` — auto-discovered specs with convention-based
///    auth: filename becomes the prefix, auth from `{PREFIX}_API_KEY` env var.
///
/// Explicit configs take precedence: if a name appears in both, the TOML
/// entry wins and the auto-discovered file is skipped.
pub fn load_openapi_configs(project_root: &Path) -> Vec<(String, OpenApiConfig)> {
    let mut configs = load_from_toml(project_root);
    let explicit_names: std::collections::HashSet<String> =
        configs.iter().map(|(n, _)| n.clone()).collect();

    for (name, config) in discover_api_dir(project_root) {
        if !explicit_names.contains(&name) {
            configs.push((name, config));
        }
    }

    configs
}

fn load_from_toml(project_root: &Path) -> Vec<(String, OpenApiConfig)> {
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

fn discover_api_dir(project_root: &Path) -> Vec<(String, OpenApiConfig)> {
    let api_dir = project_root.join(".omegon").join("apis");
    let entries = match std::fs::read_dir(&api_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut configs = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" && ext != "json" {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let secret_env = format!("{}_API_KEY", name.to_uppercase().replace('-', "_"));

        configs.push((
            name,
            OpenApiConfig {
                spec: path.display().to_string(),
                auth: "bearer".into(),
                secret: secret_env,
                base_url_override: None,
                allow: Vec::new(),
                confirm: Vec::new(),
                read_only: false,
            },
        ));
    }
    configs
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

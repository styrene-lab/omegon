//! Plugin system — load external extensions from TOML manifests.
//!
//! Plugins are declared as `~/.omegon/plugins/<name>/plugin.toml` manifests.
//! Each plugin can provide:
//! - **Tools** — backed by HTTP endpoint calls
//! - **Context** — injected into the agent's system prompt
//! - **Event forwarding** — agent events POSTed to external endpoints
//!
//! Plugins activate conditionally based on marker files (e.g., `.scribe`)
//! or environment variables. Inactive plugins are never loaded.
//!
//! This is the extension API contract for all external integrations.
//! The contract is: TOML manifest + HTTP endpoints. Language-agnostic.

pub mod manifest;
pub mod http_feature;

use manifest::PluginManifest;
use http_feature::HttpPluginFeature;
use std::path::{Path, PathBuf};

/// Discover and load active plugins for the given working directory.
/// Returns a list of Features ready to register with the EventBus.
pub fn discover_plugins(cwd: &Path) -> Vec<Box<dyn omegon_traits::Feature>> {
    let plugin_dirs = plugin_search_paths();
    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();

    for dir in &plugin_dirs {
        if !dir.is_dir() { continue; }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let plugin_dir = entry.path();
            if !plugin_dir.is_dir() { continue; }

            let manifest_path = plugin_dir.join("plugin.toml");
            if !manifest_path.exists() { continue; }

            match load_plugin(&manifest_path, cwd) {
                Ok(Some(feature)) => {
                    tracing::info!(
                        plugin = feature.name(),
                        path = %manifest_path.display(),
                        "loaded plugin"
                    );
                    features.push(feature);
                }
                Ok(None) => {
                    // Plugin exists but not active for this directory
                    tracing::debug!(
                        path = %manifest_path.display(),
                        "plugin not active for current project"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        path = %manifest_path.display(),
                        error = %e,
                        "failed to load plugin"
                    );
                }
            }
        }
    }

    features
}

/// Load a single plugin from its manifest file.
/// Returns None if the plugin is not active for the given cwd.
fn load_plugin(manifest_path: &Path, cwd: &Path) -> anyhow::Result<Option<Box<dyn omegon_traits::Feature>>> {
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: PluginManifest = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("invalid plugin manifest {}: {e}", manifest_path.display()))?;

    // Check activation
    if !manifest.activation.is_active(cwd) {
        return Ok(None);
    }

    Ok(Some(Box::new(HttpPluginFeature::new(manifest))))
}

/// Search paths for plugin directories (in priority order).
fn plugin_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. ~/.omegon/plugins/ (user-level)
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".omegon").join("plugins"));
    }

    // 2. .omegon/plugins/ (project-level)
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".omegon").join("plugins"));
    }

    // 3. OMEGON_PLUGIN_DIR env var
    if let Ok(dir) = std::env::var("OMEGON_PLUGIN_DIR") {
        paths.push(PathBuf::from(dir));
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = discover_plugins(dir.path());
        assert!(plugins.is_empty());
    }

    #[test]
    fn discover_active_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".omegon").join("plugins").join("test-plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        // Create marker file in cwd
        std::fs::write(dir.path().join(".marker"), "").unwrap();

        // Create plugin manifest
        std::fs::write(plugins_dir.join("plugin.toml"), r#"
            [plugin]
            name = "test"
            description = "Test plugin"

            [activation]
            marker_files = [".marker"]

            [[tools]]
            name = "test_tool"
            description = "does nothing"
            endpoint = "http://localhost:9999/noop"
        "#).unwrap();

        // Discover with the project dir as cwd — plugin should activate
        // We need to set OMEGON_PLUGIN_DIR since discover_plugins looks at ~/.omegon
        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", dir.path().join(".omegon").join("plugins")); }
        let plugins = discover_plugins(dir.path());
        unsafe { std::env::remove_var("OMEGON_PLUGIN_DIR"); }

        assert_eq!(plugins.len(), 1, "should discover the active plugin");
        assert_eq!(plugins[0].name(), "test");
        assert_eq!(plugins[0].tools().len(), 1);
    }

    #[test]
    fn discover_inactive_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".omegon").join("plugins").join("test-plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        // No marker file — plugin should NOT activate
        std::fs::write(plugins_dir.join("plugin.toml"), r#"
            [plugin]
            name = "test"

            [activation]
            marker_files = [".nope"]
        "#).unwrap();

        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", dir.path().join(".omegon").join("plugins")); }
        let plugins = discover_plugins(dir.path());
        unsafe { std::env::remove_var("OMEGON_PLUGIN_DIR"); }

        assert!(plugins.is_empty(), "inactive plugin should not load");
    }

    #[test]
    fn invalid_manifest_warns_not_crashes() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".omegon").join("plugins").join("bad");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "not valid toml {{{}}}").unwrap();

        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", dir.path().join(".omegon").join("plugins")); }
        let plugins = discover_plugins(dir.path());
        unsafe { std::env::remove_var("OMEGON_PLUGIN_DIR"); }

        assert!(plugins.is_empty(), "invalid manifest should not crash");
    }
}

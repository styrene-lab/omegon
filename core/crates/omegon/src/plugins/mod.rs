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

pub mod armory;
pub mod manifest;
pub mod http_feature;
pub mod mcp;
pub mod registry;

use manifest::PluginManifest;
use http_feature::HttpPluginFeature;
use omegon_traits::Feature;
use std::path::{Path, PathBuf};

/// Discover and load active plugins for the given working directory.
/// Returns a list of Features ready to register with the EventBus.
///
/// Handles both legacy HTTP-only manifests and armory-style manifests
/// (with MCP servers, script tools, OCI tools, etc.).
pub async fn discover_plugins(cwd: &Path) -> Vec<Box<dyn omegon_traits::Feature>> {
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

            // Try armory-style manifest first (has plugin.type field),
            // fall back to legacy HTTP-only manifest.
            match load_armory_plugin(&manifest_path, cwd).await {
                Ok(Some(mut loaded)) => {
                    for f in loaded.drain(..) {
                        tracing::info!(
                            plugin = f.name(),
                            path = %manifest_path.display(),
                            "loaded armory plugin"
                        );
                        features.push(f);
                    }
                }
                Ok(None) => {
                    // Not active or not armory-style — try legacy
                    match load_legacy_plugin(&manifest_path, cwd) {
                        Ok(Some(feature)) => {
                            tracing::info!(
                                plugin = feature.name(),
                                path = %manifest_path.display(),
                                "loaded legacy plugin"
                            );
                            features.push(feature);
                        }
                        Ok(None) => {
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
                Err(e) => {
                    tracing::warn!(
                        path = %manifest_path.display(),
                        error = %e,
                        "failed to load armory plugin"
                    );
                }
            }
        }
    }

    // Also discover MCP servers from project-level config
    let project_mcp = discover_project_mcp_servers(cwd).await;
    features.extend(project_mcp);

    features
}

/// Load an armory-style plugin (persona/tone/skill/extension with MCP servers).
/// Returns None if the manifest isn't armory-style or the plugin isn't active.
async fn load_armory_plugin(
    manifest_path: &Path,
    _cwd: &Path,
) -> anyhow::Result<Option<Vec<Box<dyn omegon_traits::Feature>>>> {
    let content = std::fs::read_to_string(manifest_path)?;

    // Check if this looks like an armory manifest (has [plugin] with type field).
    // If the content contains `type =` under `[plugin]`, it's armory-style.
    // If it doesn't, fall through to legacy gracefully.
    let is_armory = content.contains("[plugin]") && content.contains("type =");
    let manifest = match armory::ArmoryManifest::parse(&content) {
        Ok(m) => m,
        Err(e) if is_armory => {
            // Looks like an armory manifest with a syntax error — surface it
            anyhow::bail!("armory manifest parse error in {}: {e}", manifest_path.display());
        }
        Err(_) => return Ok(None), // Genuinely not armory-style
    };

    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();

    // Connect MCP servers if declared
    if !manifest.mcp_servers.is_empty() {
        let mcp_feature = mcp::McpFeature::connect(
            &manifest.plugin.name,
            &manifest.mcp_servers,
        ).await?;

        if !mcp_feature.tools().is_empty() {
            features.push(Box::new(mcp_feature));
        }
    }

    // TODO: Load script-backed tools, OCI tools, context entries
    // These will be additional Feature implementations wired here.

    if features.is_empty() && manifest.mcp_servers.is_empty() {
        return Ok(None);
    }

    Ok(Some(features))
}

/// Load a legacy HTTP-only plugin manifest.
fn load_legacy_plugin(
    manifest_path: &Path,
    cwd: &Path,
) -> anyhow::Result<Option<Box<dyn omegon_traits::Feature>>> {
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: PluginManifest = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("invalid plugin manifest {}: {e}", manifest_path.display()))?;

    if !manifest.activation.is_active(cwd) {
        return Ok(None);
    }

    Ok(Some(Box::new(HttpPluginFeature::new(manifest))))
}

/// Discover MCP servers declared in project-level config files.
/// Checks: .omegon/mcp.toml, opencode.json (for compatibility), .mcp.json
async fn discover_project_mcp_servers(cwd: &Path) -> Vec<Box<dyn omegon_traits::Feature>> {
    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();

    // Check .omegon/mcp.toml (native Omegon MCP config)
    let mcp_config_path = cwd.join(".omegon").join("mcp.toml");
    if mcp_config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&mcp_config_path) {
            if let Ok(servers) = toml::from_str::<std::collections::HashMap<String, mcp::McpServerConfig>>(&content) {
                match mcp::McpFeature::connect("project-mcp", &servers).await {
                    Ok(feature) if !feature.tools().is_empty() => {
                        tracing::info!(
                            servers = servers.len(),
                            tools = feature.tools().len(),
                            "loaded project MCP servers from .omegon/mcp.toml"
                        );
                        features.push(Box::new(feature));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to connect project MCP servers");
                    }
                }
            }
        }
    }

    features
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

    #[tokio::test]
    async fn discover_in_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = discover_plugins(dir.path()).await;
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn discover_active_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".omegon").join("plugins").join("test-plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        // Create marker file in cwd
        std::fs::write(dir.path().join(".marker"), "").unwrap();

        // Create plugin manifest (legacy HTTP-only style)
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

        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", dir.path().join(".omegon").join("plugins")); }
        let plugins = discover_plugins(dir.path()).await;
        unsafe { std::env::remove_var("OMEGON_PLUGIN_DIR"); }

        assert_eq!(plugins.len(), 1, "should discover the active plugin");
        assert_eq!(plugins[0].name(), "test");
        assert_eq!(plugins[0].tools().len(), 1);
    }

    #[tokio::test]
    async fn discover_inactive_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".omegon").join("plugins").join("test-plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();

        std::fs::write(plugins_dir.join("plugin.toml"), r#"
            [plugin]
            name = "test"

            [activation]
            marker_files = [".nope"]
        "#).unwrap();

        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", dir.path().join(".omegon").join("plugins")); }
        let plugins = discover_plugins(dir.path()).await;
        unsafe { std::env::remove_var("OMEGON_PLUGIN_DIR"); }

        assert!(plugins.is_empty(), "inactive plugin should not load");
    }

    #[tokio::test]
    async fn invalid_manifest_warns_not_crashes() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join(".omegon").join("plugins").join("bad");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "not valid toml {{{}}}").unwrap();

        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", dir.path().join(".omegon").join("plugins")); }
        let plugins = discover_plugins(dir.path()).await;
        unsafe { std::env::remove_var("OMEGON_PLUGIN_DIR"); }

        assert!(plugins.is_empty(), "invalid manifest should not crash");
    }
}

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
pub mod armory_feature;
pub mod http_feature;
pub mod manifest;
pub mod mcp;
pub mod persona_loader;
pub mod registry;

use http_feature::HttpPluginFeature;
use manifest::PluginManifest;
use omegon_traits::Feature;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct PluginSelectionFilter {
    pub enabled_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
}

impl PluginSelectionFilter {
    pub fn allows(&self, plugin_name: &str) -> bool {
        if self
            .disabled_extensions
            .iter()
            .any(|name| name == plugin_name)
        {
            return false;
        }
        if self.enabled_extensions.is_empty() {
            return true;
        }
        self.enabled_extensions
            .iter()
            .any(|name| name == plugin_name)
    }
}

/// Discover and load active plugins for the given working directory.
/// Returns a list of Features ready to register with the EventBus.
///
/// Handles both legacy HTTP-only manifests and armory-style manifests
/// (with MCP servers, script tools, OCI tools, etc.).
pub async fn discover_plugins(
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
) -> Vec<Box<dyn omegon_traits::Feature>> {
    discover_plugins_filtered(cwd, secrets, &PluginSelectionFilter::default()).await
}

pub async fn discover_plugins_filtered(
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    filter: &PluginSelectionFilter,
) -> Vec<Box<dyn omegon_traits::Feature>> {
    let plugin_dirs = plugin_search_paths(cwd);
    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();

    for dir in &plugin_dirs {
        if !dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let plugin_dir = entry.path();
            if !plugin_dir.is_dir() {
                continue;
            }
            let plugin_name = entry.file_name().to_string_lossy().to_string();
            if !filter.allows(&plugin_name) {
                continue;
            }

            let manifest_path = plugin_dir.join("plugin.toml");
            if !manifest_path.exists() {
                continue;
            }

            // Try armory-style manifest first (has plugin.type field),
            // fall back to legacy HTTP-only manifest.
            match load_armory_plugin(&manifest_path, cwd, secrets).await {
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
    let project_mcp = discover_project_mcp_servers(cwd, secrets).await;
    features.extend(project_mcp);

    features
}

/// Load an armory-style plugin (persona/tone/skill/extension with MCP servers).
/// Returns None if the manifest isn't armory-style or the plugin isn't active.
async fn load_armory_plugin(
    manifest_path: &Path,
    _cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
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
            anyhow::bail!(
                "armory manifest parse error in {}: {e}",
                manifest_path.display()
            );
        }
        Err(_) => return Ok(None), // Genuinely not armory-style
    };

    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();

    // Connect MCP servers if declared
    if !manifest.mcp_servers.is_empty() {
        let mcp_feature =
            mcp::McpFeature::connect(&manifest.plugin.name, &manifest.mcp_servers, secrets).await?;

        if !mcp_feature.tools().is_empty() {
            features.push(Box::new(mcp_feature));
        }
    }

    // Load script-backed and OCI tools via ArmoryFeature
    let plugin_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manifest has no parent directory"))?;
    if let Some(armory_feature) =
        armory_feature::ArmoryFeature::from_manifest(&manifest, plugin_root).await
    {
        let tool_count = armory_feature.tools().len();
        tracing::info!(
            plugin = manifest.plugin.name,
            tools = tool_count,
            "loaded armory executable tools"
        );
        features.push(Box::new(armory_feature));
    }

    if features.is_empty() {
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
async fn discover_project_mcp_servers(
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
) -> Vec<Box<dyn omegon_traits::Feature>> {
    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();

    // Check .omegon/mcp.toml (native Omegon MCP config)
    let mcp_config_path = cwd.join(".omegon").join("mcp.toml");
    if mcp_config_path.exists()
        && let Ok(content) = std::fs::read_to_string(&mcp_config_path)
        && let Ok(servers) =
            toml::from_str::<std::collections::HashMap<String, mcp::McpServerConfig>>(&content)
    {
        match mcp::McpFeature::connect("project-mcp", &servers, secrets).await {
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

    features
}

/// Search paths for plugin directories (in priority order).
fn plugin_search_paths(cwd: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. ~/.omegon/plugins/ (user-level)
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".omegon").join("plugins"));
    }

    // 2. <cwd>/.omegon/plugins/ (project-level for the targeted workspace)
    paths.push(cwd.join(".omegon").join("plugins"));

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
        let plugins = discover_plugins(dir.path(), None).await;
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn discover_plugins_filtered_honors_enabled_extensions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".marker"), "").unwrap();
        let plugins_root = dir.path().join(".omegon").join("plugins");

        let alpha = plugins_root.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::write(
            alpha.join("plugin.toml"),
            r#"
            [plugin]
            name = "Alpha Plugin"
            description = "Alpha test plugin"

            [activation]
            marker_files = [".marker"]

            [[tools]]
            name = "alpha_tool"
            description = "does alpha"
            endpoint = "http://localhost:9999/alpha"
        "#,
        )
        .unwrap();
        let beta = plugins_root.join("beta");
        std::fs::create_dir_all(&beta).unwrap();
        std::fs::write(
            beta.join("plugin.toml"),
            r#"
            [plugin]
            name = "Beta Plugin"
            description = "Beta test plugin"

            [activation]
            marker_files = [".marker"]

            [[tools]]
            name = "beta_tool"
            description = "does beta"
            endpoint = "http://localhost:9999/beta"
        "#,
        )
        .unwrap();

        let filter = PluginSelectionFilter {
            enabled_extensions: vec!["alpha".into()],
            disabled_extensions: vec![],
        };
        let plugins = discover_plugins_filtered(dir.path(), None, &filter).await;
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name(), "Alpha Plugin");
    }

    /// Test helper: load a single plugin from a test directory using load_legacy_plugin.
    /// Avoids unsafe env var manipulation that causes flaky tests in parallel runners.
    #[test]
    fn load_legacy_plugin_active() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("test-plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(dir.path().join(".marker"), "").unwrap();

        std::fs::write(
            plugins_dir.join("plugin.toml"),
            r#"
            [plugin]
            name = "test"
            description = "Test plugin"

            [activation]
            marker_files = [".marker"]

            [[tools]]
            name = "test_tool"
            description = "does nothing"
            endpoint = "http://localhost:9999/noop"
        "#,
        )
        .unwrap();

        let result = load_legacy_plugin(
            &plugins_dir.join("plugin.toml"),
            dir.path(), // cwd has .marker
        )
        .unwrap();

        assert!(result.is_some(), "should load active plugin");
        let feature = result.unwrap();
        assert_eq!(feature.name(), "test");
        assert_eq!(feature.tools().len(), 1);
    }

    #[test]
    fn load_legacy_plugin_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("test-plugin");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        // No .marker file — plugin should not activate

        std::fs::write(
            plugins_dir.join("plugin.toml"),
            r#"
            [plugin]
            name = "test"
            [activation]
            marker_files = [".nope"]
        "#,
        )
        .unwrap();

        let result = load_legacy_plugin(&plugins_dir.join("plugin.toml"), dir.path()).unwrap();

        assert!(result.is_none(), "inactive plugin should not load");
    }

    #[test]
    fn load_legacy_plugin_invalid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("bad");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("plugin.toml"), "not valid toml {{{}}}").unwrap();

        let result = load_legacy_plugin(&plugins_dir.join("plugin.toml"), dir.path());

        assert!(result.is_err(), "invalid manifest should return error");
    }
}

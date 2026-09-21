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
pub(crate) mod tool_capabilities;

use http_feature::HttpPluginFeature;
use manifest::PluginManifest;
use omegon_traits::Feature;
use std::path::{Path, PathBuf};

use crate::contribution_loading::GuardedContributionDirectory;

const MAX_PLUGIN_ENTRIES: usize = 10_000;
const MAX_PLUGIN_MANIFEST_BYTES: usize = 4 * 1024 * 1024;

fn dynamic_preflight(
    plugin_name: &str,
    manifest_name: &[u8],
    content: &str,
    snapshot: Option<&crate::contribution_loading::ContributionSnapshot>,
) -> anyhow::Result<omegon_traits::RuntimeDynamicContributionPreflight> {
    let id = omegon_traits::RuntimeContributionId::new(format!("plugin:{plugin_name}"))
        .map_err(|error| anyhow::anyhow!(error))?;
    let (source_kind, operations, effects) = if manifest_name == b"plugin.pkl" {
        (
            omegon_traits::RuntimeDynamicSourceKind::PluginManifestEvaluation,
            vec![omegon_traits::RuntimeProbeOperation::EvaluateManifest],
            vec![
                omegon_traits::RuntimeEffect::FilesystemRead,
                omegon_traits::RuntimeEffect::ProcessSpawn,
                omegon_traits::RuntimeEffect::NetworkAccess,
                omegon_traits::RuntimeEffect::SecretDelivery,
            ],
        )
    } else if let Ok(manifest) = armory::ArmoryManifest::parse(content) {
        if manifest
            .context
            .as_ref()
            .is_some_and(|context| context.script.is_some())
            || manifest
                .tools
                .iter()
                .any(|tool| tool.is_script() || tool.is_oci())
        {
            (
                omegon_traits::RuntimeDynamicSourceKind::PluginScript,
                vec![
                    omegon_traits::RuntimeProbeOperation::GenerateContext,
                    omegon_traits::RuntimeProbeOperation::DiscoverCapabilities,
                ],
                vec![
                    omegon_traits::RuntimeEffect::FilesystemRead,
                    omegon_traits::RuntimeEffect::ProcessSpawn,
                    omegon_traits::RuntimeEffect::NetworkAccess,
                    omegon_traits::RuntimeEffect::SecretDelivery,
                ],
            )
        } else if !manifest.mcp_servers.is_empty() {
            let http = manifest
                .mcp_servers
                .values()
                .any(|server| server.url.is_some());
            (
                if http {
                    omegon_traits::RuntimeDynamicSourceKind::McpHttp
                } else {
                    omegon_traits::RuntimeDynamicSourceKind::McpProcess
                },
                vec![
                    omegon_traits::RuntimeProbeOperation::Connect,
                    omegon_traits::RuntimeProbeOperation::DiscoverCapabilities,
                ],
                vec![
                    omegon_traits::RuntimeEffect::ProcessSpawn,
                    omegon_traits::RuntimeEffect::NetworkAccess,
                    omegon_traits::RuntimeEffect::SecretDelivery,
                ],
            )
        } else {
            (
                omegon_traits::RuntimeDynamicSourceKind::PluginHttp,
                vec![omegon_traits::RuntimeProbeOperation::GenerateContext],
                vec![omegon_traits::RuntimeEffect::NetworkAccess],
            )
        }
    } else {
        (
            omegon_traits::RuntimeDynamicSourceKind::PluginHttp,
            vec![omegon_traits::RuntimeProbeOperation::DiscoverCapabilities],
            vec![omegon_traits::RuntimeEffect::NetworkAccess],
        )
    };
    let source_digest = if let Some(snapshot) = snapshot {
        crate::dynamic_admission::digest_path(snapshot.path())?
    } else {
        crate::dynamic_admission::digest_bytes(content.as_bytes())
    };
    Ok(omegon_traits::RuntimeDynamicContributionPreflight {
        schema_version: omegon_traits::RUNTIME_DYNAMIC_PREFLIGHT_SCHEMA_VERSION,
        id,
        source_digest,
        source_kind,
        protocol: omegon_traits::RuntimeProtocolRange::new(1, 1)
            .map_err(|error| anyhow::anyhow!(error))?,
        minimum_dependencies: Vec::new(),
        requested_trust: omegon_traits::RuntimeTrustRequest::OperatorManaged,
        requested_confinement: omegon_traits::RuntimeConfinementRequest::HostProcess,
        probe: omegon_traits::RuntimeProbeRequirements {
            operations,
            timeout_ms: 30_000,
            requested_effects: effects,
        },
    })
}

pub struct AdmittedPlugins {
    features: Vec<Box<dyn omegon_traits::Feature>>,
    admissions: Vec<GuardedContributionDirectory>,
    mcp_supervisors: Vec<mcp::McpSupervisor>,
}

struct LoadedPlugin {
    features: Vec<Box<dyn omegon_traits::Feature>>,
    mcp_supervisors: Vec<mcp::McpSupervisor>,
}

pub(crate) struct GuardedPluginScope {
    pub(crate) scope: &'static str,
    pub(crate) display_root: PathBuf,
    pub(crate) admission: GuardedContributionDirectory,
}

pub(crate) fn open_guarded_plugin_scopes(cwd: &Path, home: &Path) -> Vec<GuardedPluginScope> {
    open_guarded_plugin_scopes_with_health(cwd, home, &mut Vec::new())
}

fn open_guarded_plugin_scopes_with_health(
    cwd: &Path,
    home: &Path,
    health: &mut Vec<crate::contribution_health::ScopeHealth>,
) -> Vec<GuardedPluginScope> {
    use crate::contribution_health::{ContributionKind::Plugins, LoadingState, ScopeHealth};
    let mut scopes = Vec::new();
    let project_root = crate::setup::find_project_root(cwd);
    for (root, components, scope) in [
        (home, &[b"plugins".as_slice()][..], "user"),
        (
            project_root.as_path(),
            &[b".omegon".as_slice(), b"plugins".as_slice()][..],
            "project",
        ),
    ] {
        let display_root = if scope == "user" {
            home.join("plugins")
        } else {
            project_root.join(".omegon/plugins")
        };
        match GuardedContributionDirectory::open(
            root,
            components,
            home,
            omegon_maintenance_contracts::ContributionKind::Plugin,
            scope,
        ) {
            Ok(Some(admission)) => {
                let display_root = components
                    .iter()
                    .fold(root.to_path_buf(), |path, component| {
                        #[cfg(unix)]
                        {
                            use std::os::unix::ffi::OsStrExt;
                            path.join(std::ffi::OsStr::from_bytes(component))
                        }
                        #[cfg(not(unix))]
                        {
                            path.join(String::from_utf8_lossy(component).as_ref())
                        }
                    });
                scopes.push(GuardedPluginScope {
                    scope,
                    display_root,
                    admission,
                });
            }
            Ok(None) => health.push(ScopeHealth::new(
                Plugins,
                scope,
                &display_root,
                LoadingState::Absent,
            )),
            Err(error) => {
                health.push(ScopeHealth::blocked(Plugins, scope, &display_root, &error));
                tracing::warn!(scope, error = %error, "plugin discovery scope failed closed");
            }
        }
    }
    scopes
}

impl AdmittedPlugins {
    pub fn publish<R>(self, publish: impl FnOnce(Vec<Box<dyn omegon_traits::Feature>>) -> R) -> R {
        let Self {
            features,
            admissions,
            mcp_supervisors: _,
        } = self;
        let result = publish(features);
        drop(admissions);
        result
    }

    pub(crate) fn publish_candidate<R>(
        self,
        publish: impl FnOnce(Vec<Box<dyn omegon_traits::Feature>>) -> R,
    ) -> (
        R,
        Vec<mcp::McpSupervisor>,
        Vec<GuardedContributionDirectory>,
    ) {
        let result = publish(self.features);
        (result, self.mcp_supervisors, self.admissions)
    }
}

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
) -> AdmittedPlugins {
    discover_plugins_filtered(cwd, secrets, &PluginSelectionFilter::default()).await
}

pub async fn discover_plugins_filtered(
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    filter: &PluginSelectionFilter,
) -> AdmittedPlugins {
    discover_plugins_filtered_with_inventory(
        cwd,
        secrets,
        filter,
        crate::contribution_lifecycle::DynamicContributionInventory::default(),
    )
    .await
}

pub(crate) async fn discover_plugins_filtered_with_inventory(
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    filter: &PluginSelectionFilter,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
) -> AdmittedPlugins {
    use crate::contribution_health::{ContributionKind::Plugins, LoadingState, ScopeHealth};
    let mut health = Vec::new();
    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();
    let mut admissions = Vec::new();
    let mut mcp_supervisors = Vec::new();
    let profile = crate::settings::Profile::load(cwd);
    let dynamic_admission =
        crate::dynamic_admission::DynamicAdmissionPolicy::from_profile(&profile);

    match crate::paths::omegon_home() {
        Ok(home) => {
            for scope in open_guarded_plugin_scopes_with_health(cwd, &home, &mut health) {
                let scope_name = scope.scope;
                let root = scope.display_root.clone();
                match discover_guarded_plugins(
                    scope,
                    cwd,
                    secrets,
                    filter,
                    &dynamic_admission,
                    &inventory,
                    &mut health,
                )
                .await
                {
                    Ok(Some((mut loaded, admission))) => {
                        health.push(ScopeHealth::new(
                            Plugins,
                            scope_name,
                            &root,
                            LoadingState::Loaded {
                                count: loaded.features.len(),
                            },
                        ));
                        features.append(&mut loaded.features);
                        mcp_supervisors.append(&mut loaded.mcp_supervisors);
                        admissions.push(admission);
                    }
                    Ok(None) => health.push(ScopeHealth::new(
                        Plugins,
                        scope_name,
                        &root,
                        LoadingState::Loaded { count: 0 },
                    )),
                    Err(error) => {
                        health.push(ScopeHealth::blocked(Plugins, scope_name, &root, &error));
                        tracing::warn!(scope = scope_name, error = %error, "plugin discovery scope failed closed");
                    }
                }
            }
        }
        Err(error) => {
            health.push(ScopeHealth::blocked(
                Plugins,
                "home",
                Path::new("~/.omegon/plugins"),
                &error,
            ));
            tracing::warn!(error = %error, "canonical plugin discovery failed closed");
        }
    }

    inventory.loading_health.replace_kind(Plugins, health);

    // Also discover MCP servers from project-level config
    let (project_mcp, mut project_mcp_supervisors) =
        discover_project_mcp_servers(cwd, secrets, &dynamic_admission, &inventory).await;
    features.extend(project_mcp);
    mcp_supervisors.append(&mut project_mcp_supervisors);

    AdmittedPlugins {
        features,
        admissions,
        mcp_supervisors,
    }
}

#[cfg(unix)]
async fn discover_guarded_plugins(
    scope: GuardedPluginScope,
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    filter: &PluginSelectionFilter,
    dynamic_admission: &crate::dynamic_admission::DynamicAdmissionPolicy,
    inventory: &crate::contribution_lifecycle::DynamicContributionInventory,
    health: &mut Vec<crate::contribution_health::ScopeHealth>,
) -> anyhow::Result<Option<(LoadedPlugin, GuardedContributionDirectory)>> {
    use crate::contribution_health::{ContributionKind::Plugins, ScopeHealth};
    use std::os::unix::ffi::OsStrExt;
    let scope_name = scope.scope;

    let GuardedPluginScope {
        display_root,
        admission,
        ..
    } = scope;
    let mut entries = admission.entry_names(MAX_PLUGIN_ENTRIES)?;
    entries.sort();
    let mut features = Vec::new();
    let mut mcp_supervisors = Vec::new();

    for raw_name in entries {
        if crate::contribution_loading::is_internal_contribution_entry(&raw_name) {
            continue;
        }
        if !admission.allows(&raw_name)? {
            tracing::info!(
                path = %display_root.join(std::ffi::OsStr::from_bytes(&raw_name)).display(),
                "excluded denied plugin"
            );
            continue;
        }
        let Ok(plugin_name) = std::str::from_utf8(&raw_name) else {
            continue;
        };
        if !filter.allows(plugin_name) {
            continue;
        }
        let Some(plugin_dir) = admission.open_child_directory(&raw_name)? else {
            continue;
        };
        let (manifest_name, content) = if let Some(content) =
            crate::contribution_loading::read_file_at(
                &plugin_dir,
                b"plugin.pkl",
                MAX_PLUGIN_MANIFEST_BYTES,
            )? {
            (b"plugin.pkl".as_slice(), content)
        } else if let Some(content) = crate::contribution_loading::read_file_at(
            &plugin_dir,
            b"plugin.toml",
            MAX_PLUGIN_MANIFEST_BYTES,
        )? {
            (b"plugin.toml".as_slice(), content)
        } else {
            continue;
        };
        let manifest_path = display_root
            .join(std::ffi::OsStr::from_bytes(&raw_name))
            .join(std::ffi::OsStr::from_bytes(manifest_name));
        let content = match String::from_utf8(content) {
            Ok(content) => content,
            Err(error) => {
                tracing::warn!(path = %manifest_path.display(), error = %error, "failed to load plugin");
                health.push(ScopeHealth::blocked(
                    Plugins,
                    &format!("{scope_name}/{plugin_name}"),
                    &manifest_path,
                    &error.into(),
                ));
                continue;
            }
        };
        let snapshot = if manifest_name == b"plugin.pkl"
            || armory::ArmoryManifest::parse(&content).is_ok_and(|manifest| {
                manifest.context.is_some()
                    || manifest
                        .tools
                        .iter()
                        .any(|tool| tool.is_script() || tool.is_oci())
            }) {
            let snapshot = match crate::contribution_loading::snapshot_contribution_directory(
                &plugin_dir,
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    tracing::warn!(path = %manifest_path.display(), error = %error, "failed to snapshot plugin");
                    health.push(ScopeHealth::blocked(
                        Plugins,
                        &format!("{scope_name}/{plugin_name}"),
                        &manifest_path,
                        &error,
                    ));
                    continue;
                }
            };
            if manifest_name == b"plugin.pkl"
                && let Err(error) = std::fs::write(snapshot.path().join("plugin.pkl"), &content)
            {
                tracing::warn!(path = %manifest_path.display(), error = %error, "failed to seal admitted Pkl manifest");
                health.push(ScopeHealth::blocked(
                    Plugins,
                    &format!("{scope_name}/{plugin_name}"),
                    &manifest_path,
                    &error.into(),
                ));
                continue;
            }
            Some(snapshot)
        } else {
            None
        };
        let preflight =
            match dynamic_preflight(plugin_name, manifest_name, &content, snapshot.as_ref()) {
                Ok(preflight) => preflight,
                Err(error) => {
                    tracing::warn!(plugin = plugin_name, %error, "plugin static preflight failed");
                    health.push(ScopeHealth::blocked(
                        Plugins,
                        &format!("{scope_name}/{plugin_name}"),
                        &manifest_path,
                        &error,
                    ));
                    continue;
                }
            };
        let candidate = match inventory.discover(preflight) {
            Ok(candidate) => candidate,
            Err(error) => {
                tracing::warn!(plugin = plugin_name, %error, "plugin static inventory rejected candidate");
                health.push(ScopeHealth::blocked(
                    Plugins,
                    &format!("{scope_name}/{plugin_name}"),
                    &manifest_path,
                    &error,
                ));
                continue;
            }
        };
        let candidate_id = candidate.preflight.id.clone();
        let trust_admission = match inventory.admit(&candidate, dynamic_admission) {
            Ok(admission) => admission,
            Err(error) => {
                tracing::warn!(plugin = plugin_name, %error, "plugin trust admission denied before execution");
                health.push(ScopeHealth::blocked(
                    Plugins,
                    &format!("{scope_name}/{plugin_name}"),
                    &manifest_path,
                    &error,
                ));
                continue;
            }
        };
        match load_plugin_manifest(
            &manifest_path,
            &content,
            cwd,
            secrets,
            snapshot,
            trust_admission,
        )
        .await
        {
            Ok(mut loaded) => {
                if loaded.features.is_empty() && loaded.mcp_supervisors.is_empty() {
                    inventory.absent(&candidate_id);
                    continue;
                }
                inventory.ready(&candidate_id);
                features.append(&mut loaded.features);
                mcp_supervisors.append(&mut loaded.mcp_supervisors);
            }
            Err(error) => {
                inventory.quarantine(&candidate_id, error.to_string());
                tracing::warn!(path = %manifest_path.display(), error = %error, "failed to load plugin");
                health.push(ScopeHealth::blocked(
                    Plugins,
                    &format!("{scope_name}/{plugin_name}"),
                    &manifest_path,
                    &error,
                ));
            }
        }
    }

    Ok(Some((
        LoadedPlugin {
            features,
            mcp_supervisors,
        },
        admission,
    )))
}

#[cfg(not(unix))]
async fn discover_guarded_plugins(
    _scope: GuardedPluginScope,
    _cwd: &Path,
    _secrets: Option<&omegon_secrets::SecretsManager>,
    _filter: &PluginSelectionFilter,
    _dynamic_admission: &crate::dynamic_admission::DynamicAdmissionPolicy,
    _inventory: &crate::contribution_lifecycle::DynamicContributionInventory,
    _health: &mut Vec<crate::contribution_health::ScopeHealth>,
) -> anyhow::Result<Option<(LoadedPlugin, GuardedContributionDirectory)>> {
    anyhow::bail!("guarded plugin discovery requires Unix")
}

async fn load_plugin_manifest(
    display_path: &Path,
    content: &str,
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    snapshot: Option<crate::contribution_loading::ContributionSnapshot>,
    trust_admission: crate::dynamic_admission::DynamicAdmissionPermit,
) -> anyhow::Result<LoadedPlugin> {
    let snapshot = snapshot.map(std::sync::Arc::new);
    if let Some(loaded) = load_armory_plugin(
        display_path,
        content,
        secrets,
        snapshot.as_ref(),
        &trust_admission,
    )
    .await?
    {
        for feature in &loaded.features {
            tracing::info!(plugin = feature.name(), path = %display_path.display(), "loaded armory plugin");
        }
        return Ok(loaded);
    }
    let snapshot_manifest;
    let manifest_path = if let Some(snapshot) = &snapshot {
        snapshot_manifest = snapshot.path().join(
            display_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("plugin manifest has no basename"))?,
        );
        snapshot_manifest.as_path()
    } else {
        display_path
    };
    let legacy = load_legacy_plugin_with_content(manifest_path, content, cwd, trust_admission)?;
    if let Some(feature) = legacy {
        tracing::info!(plugin = feature.name(), path = %display_path.display(), "loaded legacy plugin");
        Ok(LoadedPlugin {
            features: vec![feature],
            mcp_supervisors: Vec::new(),
        })
    } else {
        tracing::debug!(path = %display_path.display(), "plugin not active for current project");
        Ok(LoadedPlugin {
            features: Vec::new(),
            mcp_supervisors: Vec::new(),
        })
    }
}

/// Load an armory-style plugin (persona/tone/skill/extension with MCP servers).
/// Returns None if the manifest isn't armory-style or the plugin isn't active.
async fn load_armory_plugin(
    manifest_path: &Path,
    content: &str,
    secrets: Option<&omegon_secrets::SecretsManager>,
    snapshot: Option<&std::sync::Arc<crate::contribution_loading::ContributionSnapshot>>,
    trust_admission: &crate::dynamic_admission::DynamicAdmissionPermit,
) -> anyhow::Result<Option<LoadedPlugin>> {
    // Check if this looks like an armory manifest (has [plugin] with type field).
    // If the content contains `type =` under `[plugin]`, it's armory-style.
    // If it doesn't, fall through to legacy gracefully.
    let is_armory = content.contains("[plugin]") && content.contains("type =");
    let manifest = match armory::ArmoryManifest::parse(content) {
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
    let mut mcp_supervisors = Vec::new();

    // Connect MCP servers if declared
    if !manifest.mcp_servers.is_empty() {
        let mcp_feature = mcp::McpFeature::connect(
            &manifest.plugin.name,
            &manifest.mcp_servers,
            secrets,
            trust_admission.clone(),
        )
        .await?;
        mcp_supervisors.push(mcp_feature.supervisor());

        if !mcp_feature.tools().is_empty() {
            features.push(Box::new(mcp_feature));
        }
    }

    // Load script-backed and OCI tools via ArmoryFeature
    let needs_snapshot = manifest.context.is_some()
        || manifest
            .tools
            .iter()
            .any(|tool| tool.is_script() || tool.is_oci());
    let armory_feature = if needs_snapshot {
        let snapshot = snapshot.ok_or_else(|| anyhow::anyhow!("plugin snapshot is unavailable"))?;
        armory_feature::ArmoryFeature::from_manifest_snapshot(
            &manifest,
            std::sync::Arc::clone(snapshot),
            trust_admission.clone(),
        )
        .await
    } else {
        None
    };
    if let Some(armory_feature) = armory_feature {
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

    Ok(Some(LoadedPlugin {
        features,
        mcp_supervisors,
    }))
}

/// Load a legacy HTTP-only plugin manifest.
#[cfg(test)]
fn load_legacy_plugin(
    manifest_path: &Path,
    cwd: &Path,
) -> anyhow::Result<Option<Box<dyn omegon_traits::Feature>>> {
    let content = std::fs::read_to_string(manifest_path)?;
    let preflight = dynamic_preflight(
        manifest_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("legacy"),
        manifest_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .as_bytes(),
        &content,
        None,
    )?;
    let permit = crate::dynamic_admission::DynamicAdmissionPermit::for_test(preflight);
    load_legacy_plugin_with_content(manifest_path, &content, cwd, permit)
}

fn load_legacy_plugin_with_content(
    manifest_path: &Path,
    content: &str,
    cwd: &Path,
    trust_admission: crate::dynamic_admission::DynamicAdmissionPermit,
) -> anyhow::Result<Option<Box<dyn omegon_traits::Feature>>> {
    trust_admission.validate()?;
    let manifest: PluginManifest = if manifest_path.extension().is_some_and(|e| e == "pkl") {
        rpkl::from_config_with_options(manifest_path, crate::pkl_modules::omegon_eval_options())
            .map_err(|e| {
                anyhow::anyhow!("invalid plugin manifest {}: {e}", manifest_path.display())
            })?
    } else {
        toml::from_str(content).map_err(|e| {
            anyhow::anyhow!("invalid plugin manifest {}: {e}", manifest_path.display())
        })?
    };

    if !manifest.activation.is_active(cwd) {
        return Ok(None);
    }

    Ok(Some(Box::new(HttpPluginFeature::new_admitted(
        manifest,
        trust_admission,
    ))))
}

/// Discover MCP servers declared in project-level config files.
/// Checks: .omegon/mcp.toml, opencode.json (for compatibility), .mcp.json
async fn discover_project_mcp_servers(
    cwd: &Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    dynamic_admission: &crate::dynamic_admission::DynamicAdmissionPolicy,
    inventory: &crate::contribution_lifecycle::DynamicContributionInventory,
) -> (
    Vec<Box<dyn omegon_traits::Feature>>,
    Vec<mcp::McpSupervisor>,
) {
    let mut features: Vec<Box<dyn omegon_traits::Feature>> = Vec::new();
    let mut mcp_supervisors = Vec::new();

    // Check .omegon/mcp.toml (native Omegon MCP config)
    let mcp_config_path = cwd.join(".omegon").join("mcp.toml");
    if mcp_config_path.exists()
        && let Ok(content) = std::fs::read_to_string(&mcp_config_path)
        && let Ok(servers) =
            toml::from_str::<std::collections::HashMap<String, mcp::McpServerConfig>>(&content)
    {
        let preflight = mcp::dynamic_preflight("mcp:project", &content, &servers);
        let candidate = preflight.and_then(|preflight| inventory.discover(preflight));
        let candidate_id = candidate
            .as_ref()
            .ok()
            .map(|candidate| candidate.preflight.id.clone());
        let trust_admission =
            candidate.and_then(|candidate| inventory.admit(&candidate, dynamic_admission));
        let Ok(trust_admission) = trust_admission else {
            tracing::warn!("project MCP trust admission denied before connection");
            return (features, mcp_supervisors);
        };
        match mcp::McpFeature::connect("project-mcp", &servers, secrets, trust_admission).await {
            Ok(feature) if !feature.tools().is_empty() => {
                if let Some(candidate_id) = &candidate_id {
                    inventory.ready(candidate_id);
                }
                mcp_supervisors.push(feature.supervisor());
                tracing::info!(
                    servers = servers.len(),
                    tools = feature.tools().len(),
                    "loaded project MCP servers from .omegon/mcp.toml"
                );
                features.push(Box::new(feature));
            }
            Ok(_) => {}
            Err(e) => {
                if let Some(candidate_id) = &candidate_id {
                    inventory.quarantine(candidate_id, e.to_string());
                }
                tracing::warn!(error = %e, "failed to connect project MCP servers");
            }
        }
    }

    (features, mcp_supervisors)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        home: Option<std::ffi::OsString>,
        plugin_dir: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn isolate(home: &Path) -> Self {
            let guard = Self {
                home: std::env::var_os("OMEGON_HOME"),
                plugin_dir: std::env::var_os("OMEGON_PLUGIN_DIR"),
            };
            unsafe {
                std::env::set_var("OMEGON_HOME", home);
                std::env::remove_var("OMEGON_PLUGIN_DIR");
            }
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(prev) = self.home.take() {
                    std::env::set_var("OMEGON_HOME", prev);
                } else {
                    std::env::remove_var("OMEGON_HOME");
                }
                if let Some(prev) = self.plugin_dir.take() {
                    std::env::set_var("OMEGON_PLUGIN_DIR", prev);
                } else {
                    std::env::remove_var("OMEGON_PLUGIN_DIR");
                }
            }
        }
    }

    #[tokio::test]
    async fn discover_in_empty_dir() {
        let _lock = crate::test_support::env::lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(dir.path());
        let plugins = discover_plugins(dir.path(), None).await;
        plugins.publish(|features| assert!(features.is_empty()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn untrusted_plugin_and_project_mcp_do_not_execute_during_discovery() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        let marker = project.path().join("executed");
        let plugin = project.path().join(".omegon/plugins/untrusted");
        std::fs::create_dir_all(plugin.join("tools")).unwrap();
        std::fs::write(
            plugin.join("plugin.toml"),
            r#"
[plugin]
type = "extension"
id = "dev.test.untrusted"
name = "Untrusted"
version = "1.0.0"
description = "must not execute"

[context]
runner = "bash"
script = "tools/run.sh"

[[tools]]
name = "untrusted_tool"
description = "must not execute"
runner = "bash"
script = "tools/run.sh"
"#,
        )
        .unwrap();
        let script = plugin.join("tools/run.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        std::fs::create_dir_all(project.path().join(".omegon")).unwrap();
        std::fs::write(
            project.path().join(".omegon/mcp.toml"),
            format!("[untrusted]\ncommand = \"{}\"\n", script.display()),
        )
        .unwrap();

        discover_plugins(project.path(), None)
            .await
            .publish(|features| assert!(features.is_empty()));
        assert!(!marker.exists(), "denied dynamic code must not run");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discover_plugins_filtered_honors_enabled_extensions() {
        let _lock = crate::test_support::env::lock_async().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(dir.path());
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
        trust_plugin_code(dir.path(), &["alpha"]);

        let filter = PluginSelectionFilter {
            enabled_extensions: vec!["alpha".into()],
            disabled_extensions: vec![],
        };
        let plugins = discover_plugins_filtered(dir.path(), None, &filter).await;
        plugins.publish(|features| {
            assert_eq!(features.len(), 1);
            assert_eq!(features[0].name(), "Alpha Plugin");
        });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn environment_plugin_directory_is_excluded_from_startup() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        write_plugin(external.path(), "external-plugin");
        // SAFETY: this test holds the shared process-environment test lock.
        unsafe { std::env::set_var("OMEGON_PLUGIN_DIR", external.path()) };

        discover_plugins(project.path(), None)
            .await
            .publish(|features| assert!(features.is_empty()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_armory_script_executes_from_admitted_snapshot() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        let plugin = project.path().join(".omegon/plugins/snapshot");
        std::fs::create_dir_all(plugin.join("tools")).unwrap();
        std::fs::write(
            plugin.join("plugin.toml"),
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.snapshot"
            name = "Snapshot"
            version = "1.0.0"
            description = "snapshot test"

            [[tools]]
            name = "snapshot_tool"
            description = "reports its source"
            runner = "bash"
            script = "tools/run.sh"
        "#,
        )
        .unwrap();
        trust_plugin_code(project.path(), &["snapshot"]);
        let script = plugin.join("tools/run.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '{\"result\":\"ORIGINAL\",\"error\":null}\\n'\n",
        )
        .unwrap();

        let admitted = discover_plugins(project.path(), None).await;
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '{\"result\":\"MUTATED\",\"error\":null}\\n'\n",
        )
        .unwrap();
        let bus = admitted.publish(|features| {
            let mut bus = crate::bus::EventBus::new();
            for feature in features {
                bus.register(feature);
            }
            bus.finalize();
            bus
        });

        let result = bus
            .execute_tool(
                "snapshot_tool",
                "snapshot-call",
                serde_json::json!({}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        let rendered = format!("{result:?}");
        assert!(rendered.contains("ORIGINAL"));
        assert!(!rendered.contains("MUTATED"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_plugin_discovery_excludes_exact_denied_basename() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        write_plugin(&project.path().join(".omegon/plugins"), "denied");
        write_plugin(&project.path().join(".omegon/plugins"), "allowed");
        trust_plugin_code(project.path(), &["denied", "allowed"]);
        deny_plugin(
            project.path(),
            &[b".omegon", b"plugins"],
            home.path(),
            "project",
            b"denied",
        );

        discover_plugins(project.path(), None)
            .await
            .publish(|features| {
                assert_eq!(features.len(), 1);
                assert_eq!(features[0].name(), "allowed");
            });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn contribution_health_plugin_malformed_scope_isolated() {
        use std::io::Write;

        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        write_plugin(&home.path().join("plugins"), "user-plugin");
        write_plugin(&project.path().join(".omegon/plugins"), "project-plugin");
        trust_plugin_code(project.path(), &["user-plugin", "project-plugin"]);
        let authority = initialize_plugin_scope(
            project.path(),
            &[b".omegon", b"plugins"],
            home.path(),
            "project",
        );
        let state_path = home
            .path()
            .join("maintain/v1/deny")
            .join(authority.to_hex())
            .join("state.json");
        let mut state = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(state_path)
            .unwrap();
        state.write_all(b"{not-json").unwrap();
        state.sync_all().unwrap();

        let inventory = crate::contribution_lifecycle::DynamicContributionInventory::default();
        discover_plugins_filtered_with_inventory(
            project.path(),
            None,
            &PluginSelectionFilter::default(),
            inventory.clone(),
        )
        .await
        .publish(|features| {
            assert_eq!(features.len(), 1);
            assert_eq!(features[0].name(), "user-plugin");
        });
        let health = inventory.loading_health.snapshot();
        assert!(
            health
                .iter()
                .any(|scope| scope.scope == "project" && scope.is_blocked())
        );
        assert!(
            health
                .iter()
                .any(|scope| scope.scope == "user" && !scope.is_blocked())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_plugin_discovery_holds_locks_through_publication() {
        use omegon_maintenance_contracts::{LockMode, MaintenanceStateV1, ProtocolLock};

        let _lock = crate::test_support::env::lock_async().await;
        let home_path = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home_path.path());
        write_plugin(&home_path.path().join("plugins"), "user-plugin");
        write_plugin(&project.path().join(".omegon/plugins"), "project-plugin");
        let user_authority = plugin_scope_key(&home_path.path().join("plugins"), "user");
        let project_authority =
            plugin_scope_key(&project.path().join(".omegon/plugins"), "project");
        let home = omegon_maintenance_contracts::open_secure_root(home_path.path()).unwrap();
        let state = MaintenanceStateV1::bootstrap(
            &home,
            omegon_maintenance_contracts::path_identity(&home).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .unwrap();
        let admitted = discover_plugins(project.path(), None).await;

        admitted.publish(|features| {
            let mut bus = crate::bus::EventBus::new();
            for feature in features {
                bus.register(feature);
            }
            bus.finalize();
            for authority in [user_authority, project_authority] {
                let lock_name = format!("contribution-{authority}.lock");
                assert!(
                    ProtocolLock::acquire_at(
                        &state.locks,
                        lock_name.as_bytes(),
                        LockMode::Exclusive,
                        false,
                        true,
                    )
                    .is_err()
                );
            }
        });
        for authority in [user_authority, project_authority] {
            let lock_name = format!("contribution-{authority}.lock");
            assert!(
                ProtocolLock::acquire_at(
                    &state.locks,
                    lock_name.as_bytes(),
                    LockMode::Exclusive,
                    false,
                    true,
                )
                .is_ok()
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_persona_catalog_excludes_denied_entries_and_holds_scope_locks() {
        use omegon_maintenance_contracts::{LockMode, MaintenanceStateV1, ProtocolLock};

        let _lock = crate::test_support::env::lock_async().await;
        let home_path = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home_path.path());
        write_persona(
            &home_path.path().join("plugins"),
            "user-persona",
            "USER_PERSONA",
        );
        write_persona(
            &project.path().join(".omegon/plugins"),
            "denied-persona",
            "DENIED_PERSONA",
        );
        write_persona(
            &project.path().join(".omegon/plugins"),
            "project-persona",
            "PROJECT_PERSONA",
        );
        deny_plugin(
            project.path(),
            &[b".omegon", b"plugins"],
            home_path.path(),
            "project",
            b"denied-persona",
        );
        let user_authority = plugin_scope_key(&home_path.path().join("plugins"), "user");
        let project_authority =
            plugin_scope_key(&project.path().join(".omegon/plugins"), "project");
        let home = omegon_maintenance_contracts::open_secure_root(home_path.path()).unwrap();
        let state = MaintenanceStateV1::bootstrap(
            &home,
            omegon_maintenance_contracts::path_identity(&home).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .unwrap();

        crate::plugins::persona_loader::with_available(project.path(), |personas, tones| {
            assert!(tones.is_empty());
            assert_eq!(personas.len(), 2);
            let directives = personas
                .iter()
                .filter_map(|persona| persona.persona())
                .map(|persona| persona.directive.as_str())
                .collect::<Vec<_>>();
            assert!(
                directives
                    .iter()
                    .any(|directive| directive.contains("USER_PERSONA"))
            );
            assert!(
                directives
                    .iter()
                    .any(|directive| directive.contains("PROJECT_PERSONA"))
            );
            assert!(
                !directives
                    .iter()
                    .any(|directive| directive.contains("DENIED_PERSONA"))
            );
            for authority in [user_authority, project_authority] {
                let lock_name = format!("contribution-{authority}.lock");
                assert!(
                    ProtocolLock::acquire_at(
                        &state.locks,
                        lock_name.as_bytes(),
                        LockMode::Exclusive,
                        false,
                        true,
                    )
                    .is_err()
                );
            }
        });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_persona_catalog_skips_nested_symlink_content() {
        use std::os::unix::fs::symlink;

        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        let plugins = project.path().join(".omegon/plugins");
        write_persona(&plugins, "valid", "VALID_PERSONA");
        write_persona(&plugins, "linked", "REPLACED_PERSONA");
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), "OUTSIDE_PERSONA").unwrap();
        std::fs::remove_file(plugins.join("linked/PERSONA.md")).unwrap();
        symlink(outside.path(), plugins.join("linked/PERSONA.md")).unwrap();

        crate::plugins::persona_loader::with_available(project.path(), |personas, _| {
            assert_eq!(personas.len(), 1);
            assert_eq!(personas[0].id, "dev.test.valid");
            assert!(
                personas[0]
                    .persona()
                    .unwrap()
                    .directive
                    .contains("VALID_PERSONA")
            );
        });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_persona_catalog_rejects_duplicate_ids_across_scopes() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        write_persona_with_id(
            &home.path().join("plugins"),
            "user-copy",
            "dev.test.duplicate",
            "USER_DUPLICATE",
        );
        write_persona_with_id(
            &project.path().join(".omegon/plugins"),
            "project-copy",
            "dev.test.duplicate",
            "PROJECT_DUPLICATE",
        );

        crate::plugins::persona_loader::with_available(project.path(), |personas, _| {
            assert!(personas.is_empty());
        });
        assert!(
            crate::plugins::persona_loader::delete_persona(project.path(), "dev.test.duplicate",)
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_persona_catalog_isolates_malformed_project_scope() {
        use std::io::Write;

        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        write_persona(&home.path().join("plugins"), "user-persona", "USER_PERSONA");
        write_persona(
            &project.path().join(".omegon/plugins"),
            "project-persona",
            "PROJECT_PERSONA",
        );
        let authority = initialize_plugin_scope(
            project.path(),
            &[b".omegon", b"plugins"],
            home.path(),
            "project",
        );
        let state_path = home
            .path()
            .join("maintain/v1/deny")
            .join(authority.to_hex())
            .join("state.json");
        let mut state = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(state_path)
            .unwrap();
        state.write_all(b"{not-json").unwrap();
        state.sync_all().unwrap();

        crate::plugins::persona_loader::with_available(project.path(), |personas, _| {
            assert_eq!(personas.len(), 1);
            assert_eq!(personas[0].id, "dev.test.user-persona");
        });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_persona_catalog_uses_project_root_from_nested_workspace_path() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        std::fs::write(project.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        write_persona(
            &project.path().join(".omegon/plugins"),
            "root-persona",
            "ROOT_PERSONA",
        );
        let nested = project.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();

        crate::plugins::persona_loader::with_available(&nested, |personas, _| {
            assert_eq!(personas.len(), 1);
            assert_eq!(personas[0].id, "dev.test.root-persona");
        });
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn persona_mutations_use_canonical_guarded_user_scope() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(home.path());
        let path = crate::plugins::persona_loader::create_user_persona(
            project.path(),
            "mutable",
            "Mutable",
            "before",
            None,
            &[],
            "INITIAL_DIRECTIVE",
        )
        .unwrap();
        assert_eq!(path, home.path().join("plugins/mutable"));
        std::fs::write(path.join(".persona-state"), "preserve").unwrap();
        crate::plugins::persona_loader::update_persona(
            project.path(),
            "user.mutable",
            crate::plugins::persona_loader::PersonaUpdate {
                directive: Some("UPDATED_DIRECTIVE"),
                description: Some("after"),
                ..Default::default()
            },
        )
        .unwrap();
        crate::plugins::persona_loader::with_available(project.path(), |personas, _| {
            let persona = personas
                .iter()
                .find(|persona| persona.id == "user.mutable")
                .unwrap();
            assert_eq!(persona.description, "after");
            assert_eq!(persona.persona().unwrap().directive, "UPDATED_DIRECTIVE");
        });
        assert_eq!(
            std::fs::read_to_string(path.join(".persona-state")).unwrap(),
            "preserve"
        );
        crate::plugins::persona_loader::delete_persona(project.path(), "user.mutable").unwrap();
        assert!(!path.exists());
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

    #[cfg(unix)]
    fn write_plugin(directory: &Path, name: &str) {
        let plugin = directory.join(name);
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("plugin.toml"),
            format!(
                "[plugin]\nname = \"{name}\"\n\n[activation]\nalways = true\n\n[[tools]]\nname = \"{name}_tool\"\ndescription = \"test\"\nendpoint = \"http://localhost:9999/test\"\n"
            ),
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn trust_plugin_code(project: &Path, names: &[&str]) {
        let omegon = project.join(".omegon");
        std::fs::create_dir_all(&omegon).unwrap();
        let trusted: Vec<String> = names.iter().map(|name| format!("plugin:{name}")).collect();
        std::fs::write(
            omegon.join("profile.json"),
            serde_json::to_vec(&serde_json::json!({
                "permissions": {"trustedContributionCode": trusted}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn write_persona(directory: &Path, name: &str, directive: &str) {
        write_persona_with_id(directory, name, &format!("dev.test.{name}"), directive);
    }

    #[cfg(unix)]
    fn write_persona_with_id(directory: &Path, name: &str, id: &str, directive: &str) {
        let plugin = directory.join(name);
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("PERSONA.md"), directive).unwrap();
        std::fs::write(
            plugin.join("plugin.toml"),
            format!(
                "[plugin]\ntype = \"persona\"\nid = \"{id}\"\nname = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"test\"\n\n[persona.identity]\ndirective = \"PERSONA.md\"\n"
            ),
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn initialize_plugin_scope(
        root: &Path,
        components: &[&[u8]],
        home: &Path,
        scope: &str,
    ) -> omegon_maintenance_contracts::AuthorityKey {
        GuardedContributionDirectory::open(
            root,
            components,
            home,
            omegon_maintenance_contracts::ContributionKind::Plugin,
            scope,
        )
        .unwrap()
        .unwrap()
        .scope_key()
    }

    #[cfg(unix)]
    fn plugin_scope_key(
        directory: &Path,
        scope: &str,
    ) -> omegon_maintenance_contracts::AuthorityKey {
        let directory = std::fs::File::open(directory).unwrap();
        let parent = omegon_maintenance_contracts::path_identity(&directory).unwrap();
        omegon_maintenance_contracts::scope_key(
            omegon_maintenance_contracts::ContributionKind::Plugin.as_str(),
            scope,
            parent.key,
        )
    }

    #[cfg(unix)]
    fn deny_plugin(
        root: &Path,
        components: &[&[u8]],
        home_path: &Path,
        scope: &str,
        raw_name: &[u8],
    ) {
        use omegon_maintenance_contracts::{
            AuthorityKey, ContributionKind, DenyRecordV1, DenyState, DenyStateV1, SCHEMA_VERSION,
            derive_key, entry_key, open_secure_dir_at, replace_record_at,
        };
        use sha2::{Digest, Sha256};

        let authority = initialize_plugin_scope(root, components, home_path, scope);
        let home = omegon_maintenance_contracts::open_secure_root(home_path).unwrap();
        let state = omegon_maintenance_contracts::MaintenanceStateV1::bootstrap(
            &home,
            omegon_maintenance_contracts::path_identity(&home).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .unwrap();
        let deny_directory = open_secure_dir_at(&state.deny, authority.to_hex().as_bytes())
            .unwrap()
            .unwrap();
        let kind = ContributionKind::Plugin;
        let entry = entry_key(kind.as_str(), authority, raw_name);
        let request_id = "00000000-0000-0000-0000-000000000001";
        let record = DenyRecordV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "deny".into(),
            record_id: derive_key(
                "deny",
                &[
                    authority.as_bytes(),
                    entry.as_bytes(),
                    request_id.as_bytes(),
                ],
            ),
            scope_key: authority,
            contribution_kind: kind,
            entry_key: entry,
            raw_name_digest: AuthorityKey::from_bytes(Sha256::digest(raw_name).into()),
            generation: 1,
            state: DenyState::Denied,
            request_id: request_id.into(),
            created_at: "2026-08-18T00:00:00Z".into(),
        };
        let deny = DenyStateV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "deny_state".into(),
            record_id: derive_key("deny-state", &[authority.as_bytes(), &1_u64.to_be_bytes()]),
            scope_key: authority,
            generation: 1,
            entries: [(entry.to_hex(), record)].into(),
        };
        replace_record_at(&deny_directory, b"state.json", &deny, "deny-plugin-test").unwrap();
    }
}

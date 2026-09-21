//! Agent setup — shared initialization for headless and interactive modes.
//!
//! Builds the EventBus with all features registered, plus the ContextManager
//! and ConversationState needed for the agent loop.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use omegon_memory::EmbeddingService as _; // bring trait methods into scope

use crate::bus::EventBus;
use crate::context::ContextManager;
use crate::conversation::ConversationState;
use crate::features;
use crate::prompt;
use crate::session;
use crate::tools;

pub(crate) fn register_work_aggregation(
    bus: &mut EventBus,
) -> features::work_aggregation::WorkSnapshotPublisher {
    let (feature, publisher) = features::work_aggregation::WorkAggregationFeature::pending();
    bus.register(Box::new(feature));
    publisher
}

/// Summary of a resumed session, surfaced to the TUI for the welcome brief.
#[derive(Debug, Clone)]
pub struct ResumeInfo {
    pub session_id: String,
    pub turns: u32,
    pub description: String,
    pub last_prompt_snippet: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceStartupState {
    pub lease: crate::workspace::types::WorkspaceLease,
    pub admission: crate::workspace::types::AdmissionOutcome,
}

/// Renderer-neutral state captured during setup for an interactive surface's
/// first projection.
#[derive(Default)]
pub struct InteractiveInitialState {
    pub total_facts: usize,
    pub focused_node: Option<crate::runtime_state::FocusedNodeSummary>,
    pub active_changes: Vec<crate::runtime_state::ChangeSummary>,
    pub workspace_status: Option<String>,
}

/// Everything needed to run an agent loop.
pub struct AgentSetup {
    /// The event bus — owns all features. The loop dispatches tools and
    /// emits events through the bus.
    pub bus: EventBus,
    /// Immutable repository-work service captured from the accepted boot generation.
    pub(crate) work_snapshot: Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>,
    /// Stateless behavior policy captured with its accepted service identity.
    pub(crate) behavior_policy: Option<crate::behavior::BehaviorPolicyBinding>,
    /// Boot-captured exact-generation lifecycle service binding.
    pub(crate) lifecycle_binding: crate::lifecycle_service::LifecycleBinding,
    /// Boot-captured exact-generation memory service binding.
    pub(crate) memory_binding: crate::memory_service::MemoryBinding,
    /// Boot-captured exact-generation context/compaction planning binding.
    pub(crate) context_compaction: crate::context_compaction_service::ContextCompactionBinding,
    /// Boot-captured exact-generation repository Git/JJ binding.
    pub(crate) git_binding: crate::git_service::GitBinding,
    /// Stable session id for the current live conversation. Fresh sessions
    /// get a generated id at startup; resumed sessions reuse their saved id.
    pub session_id: String,
    pub(crate) session_view_binding: crate::session_consumers::SessionViewBinding,
    /// Instance identifier for runtime state isolation (`tui-{pid}`, `acp-{pid}`, etc.).
    pub instance_id: String,
    /// Durable v1 runtime ownership and heartbeat lifecycle.
    pub runtime_ownership: crate::workspace::runtime::RuntimeOwnership,
    /// Skill activation/resolution events produced while loading startup augments.
    pub startup_skill_activation_events: Vec<omegon_traits::SkillActivationEvent>,
    /// Shared context metrics — updated each turn, read by ContextProvider
    pub context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    /// Typed read-only context-pack service for host/operator surfaces.
    pub context_service: std::sync::Arc<crate::features::context::ContextProvider>,
    /// Shared command channel — set by main after TUI init
    pub command_tx: crate::features::context::SharedCommandTx,
    pub context_manager: ContextManager,
    pub conversation: ConversationState,
    pub cwd: PathBuf,
    /// Single shared owner for the active inference inventory generation.
    pub inference_runtime: crate::inference_runtime::InferenceRuntimeState,
    /// One-shot route binding for the boot-published model-budget feature.
    pub(crate) model_budget_route: Option<crate::features::model_budget::ModelBudgetRouteBinding>,
    /// Secrets manager — redaction, guards, recipes.
    pub secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    /// Resolved web auth state for the embedded dashboard.
    pub web_auth_state: crate::web::WebAuthState,
    /// Resolved startup-approved secret env pairs for child/headless runs.
    pub session_secret_env: Vec<(String, String)>,
    /// Snapshot of lifecycle + memory state at startup for TUI pre-population.
    pub(crate) startup_snapshot: StartupSnapshot,
    /// Phase tracking from loaded skills — used by the loop to detect
    /// premature completion.
    pub skill_phases: Vec<crate::skills::SkillPhaseInfo>,
    /// Shared handles for live dashboard updates.
    pub dashboard_handles: crate::runtime_state::RuntimeStateHandles,
    /// Initial harness status assembled at startup.
    /// The agent loop broadcasts this as AgentEvent::HarnessStatusChanged
    /// when the events channel is created.
    pub initial_harness_status: crate::status::HarnessStatus,
    /// Present when a prior session was loaded; None for fresh starts.
    pub resume_info: Option<ResumeInfo>,
    /// Validated legacy/catalog metadata retained for one-way compatibility import.
    pub(crate) resume_meta: Option<crate::session::SessionMeta>,
    /// Startup-local workspace ownership metadata.
    pub workspace_state: WorkspaceStartupState,
    /// One generation owner for native extension, MCP, and manifest resources.
    pub(crate) dynamic_contributions:
        crate::contribution_lifecycle::DynamicContributionGenerationOwner,
    /// Process-local diagnostic and one-shot replacement controls for the published generation.
    pub(crate) dynamic_contribution_control:
        crate::contribution_lifecycle::DynamicContributionControl,
    /// Effective component activation policy captured for this boot generation.
    pub(crate) component_policy: crate::component_policy::ResolvedComponentPolicy,
    /// Deterministic contribution omissions implied by the captured policy.
    pub(crate) component_dependency_policy:
        crate::contribution_graph::ComponentDependencyPolicyPlan,
    /// Extension widgets discovered during setup — passed to TUI for rendering.
    pub extension_widgets: Vec<crate::extensions::ExtensionTabWidget>,
    /// Extension deployment metadata discovered during startup.
    pub extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
    /// Loaded extension RPC handles keyed by extension id/name for ACP control-plane calls.
    pub extension_rpc_handles:
        std::collections::BTreeMap<String, crate::extensions::ExtensionPollingHandle>,
    /// Extension widget event receivers discovered during setup.
    pub widget_receivers: Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
    /// Slot the AgentEvent broadcast sender gets written into once main.rs
    /// has constructed the channel. The cleave feature reads this slot when
    /// emitting `AgentEvent::Decomposition*` events from inside its tool
    /// execution path. See `features::cleave::CleaveEventSlot`.
    pub cleave_event_slot: features::cleave::CleaveEventSlot,
    /// Same concept for delegate/scout worker events.
    pub delegate_event_slot: features::delegate::DelegateEventSlot,
    /// Polling handles for extensions that provide `vox_route`.
    /// Used by the daemon to start the vox event bridge.
    pub vox_polling_handles: Vec<crate::extensions::ExtensionPollingHandle>,
    /// Notification receivers for voice-capable extensions.
    pub voice_notification_receivers:
        Vec<tokio::sync::mpsc::UnboundedReceiver<crate::extensions::ExtensionNotification>>,
    /// Idle notification pumps for voice-capable extensions.
    pub voice_polling_handles: Vec<crate::extensions::ExtensionPollingHandle>,
}

pub(crate) async fn finalize_agent_error<T>(
    agent: &mut AgentSetup,
    error: anyhow::Error,
) -> anyhow::Result<T> {
    let report = agent.bus.shutdown_managed_services().await;
    let dynamic_failures = agent.dynamic_contributions.shutdown().await;
    if report.all_resources_settled() && dynamic_failures.is_empty() {
        Err(error)
    } else {
        Err(error.context(format!(
            "runtime resources did not settle while finalizing error: managed={report:?}; dynamic={dynamic_failures:?}"
        )))
    }
}

fn register_model_budget(
    bus: &mut EventBus,
    settings: Option<&crate::settings::SharedSettings>,
) -> Option<crate::features::model_budget::ModelBudgetRouteBinding> {
    settings.map(|settings| {
        let feature = features::model_budget::ModelBudget::new(settings.clone());
        let binding = feature.route_binding();
        bus.register(Box::new(feature));
        binding
    })
}

pub(crate) async fn finalize_managed_error<T>(
    bus: &mut EventBus,
    error: anyhow::Error,
) -> anyhow::Result<T> {
    let report = bus.shutdown_managed_services().await;
    if report.all_resources_settled() {
        Err(error)
    } else {
        Err(error.context(format!(
            "managed services did not settle while finalizing error: {report:?}"
        )))
    }
}

/// Runtime-substrate inventory captured at startup or before a future substrate refresh.
///
/// This is intentionally small and copyable: it lets operator-facing surfaces
/// describe what the runtime-discovered substrate contains without taking
/// ownership of process handles, receivers, or internal routing state. Future
/// refresh code should build and validate a candidate generation first, then
/// promote that candidate only after it succeeds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeSubstrateInventory {
    pub skill_activation_events: usize,
    pub extension_widgets: usize,
    pub extension_metadata_entries: usize,
    pub extension_rpc_handles: usize,
    pub widget_receivers: usize,
    pub vox_polling_handles: usize,
    pub voice_notification_receivers: usize,
    pub voice_polling_handles: usize,
}

impl RuntimeSubstrateInventory {
    pub fn from_agent_setup(setup: &AgentSetup) -> Self {
        Self {
            skill_activation_events: setup.startup_skill_activation_events.len(),
            extension_widgets: setup.extension_widgets.len(),
            extension_metadata_entries: setup.extension_metadata.len(),
            extension_rpc_handles: setup.extension_rpc_handles.len(),
            widget_receivers: setup.widget_receivers.len(),
            vox_polling_handles: setup.vox_polling_handles.len(),
            voice_notification_receivers: setup.voice_notification_receivers.len(),
            voice_polling_handles: setup.voice_polling_handles.len(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeSubstrateRefreshCandidate {
    pub inventory: RuntimeSubstrateInventory,
    pub extension_candidates: usize,
    pub extension_candidate_names: Vec<String>,
    pub skipped_by_policy: usize,
    pub disabled_extensions: usize,
    pub invalid_manifests: Vec<String>,
}

fn apply_initial_memory_status(
    harness_status: &mut crate::status::HarnessStatus,
    status: crate::status::MemoryStatus,
    binding_available: bool,
    warning: Option<String>,
) {
    harness_status.update_memory(status);
    if !binding_available || warning.is_some() {
        harness_status.memory_available = false;
        harness_status.memory_warning = warning;
    }
}

/// Build a runtime substrate refresh candidate inventory without mutating live runtime state.
///
/// This function does not spawn extension subprocesses or register live
/// features. The supervisor-owned refresh path uses this inventory to select
/// candidates before it stages and publishes changed generations.
pub fn runtime_substrate_refresh_candidate(
    cwd: &Path,
) -> anyhow::Result<RuntimeSubstrateRefreshCandidate> {
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut dry_run = RuntimeSubstrateRefreshCandidate::default();
    dry_run.inventory.skill_activation_events = crate::skills::list_structured()
        .map(|entries| entries.into_iter().filter(|entry| entry.reloadable).count())
        .unwrap_or_default();

    let ext_dir = crate::paths::omegon_home()?.join("extensions");
    if !ext_dir.exists() {
        return Ok(dry_run);
    }

    let profile = crate::settings::Profile::load(&cwd);
    let env_enabled = crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS");
    let env_disabled = crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS");

    for entry in std::fs::read_dir(&ext_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.toml");
        if !manifest_path.exists() {
            continue;
        }
        let ext_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        if !profile
            .extensions
            .permits(&ext_name, &env_enabled, &env_disabled)
        {
            dry_run.skipped_by_policy += 1;
            continue;
        }
        if extension_state_disabled(&path) {
            dry_run.disabled_extensions += 1;
            continue;
        }
        match crate::extensions::ExtensionManifest::from_extension_dir(&path) {
            Ok(manifest) => {
                dry_run.extension_candidates += 1;
                dry_run.extension_candidate_names.push(ext_name);
                dry_run.inventory.extension_metadata_entries += 1;
                dry_run.inventory.extension_rpc_handles += 1;
                dry_run.inventory.widget_receivers += 1;
                dry_run.inventory.extension_widgets += manifest.widgets.len();
                if manifest.capabilities.voice {
                    dry_run.inventory.voice_notification_receivers += 1;
                    dry_run.inventory.voice_polling_handles += 1;
                }
            }
            Err(err) => dry_run.invalid_manifests.push(format!("{ext_name}: {err}")),
        }
    }

    Ok(dry_run)
}

/// Pre-computed state gathered during setup for TUI initial display.
pub(crate) struct StartupSnapshot {
    pub total_facts: usize,
    pub lifecycle: LifecycleSnapshot,
}

/// Snapshot of design-tree + openspec state, extracted before boxing the provider.
#[derive(Default)]
pub(crate) struct LifecycleSnapshot {
    pub focused_node: Option<crate::runtime_state::FocusedNodeSummary>,
    pub active_changes: Vec<crate::runtime_state::ChangeSummary>,
}

impl LifecycleSnapshot {
    fn from_managed(host: &crate::runtime_state::LifecycleHostHandle) -> Self {
        let Ok(observation) = host.observe() else {
            return Self::default();
        };
        let Some(repository) = observation.repository else {
            return Self::default();
        };
        let focused_node = observation.focus.node_id.as_ref().and_then(|id| {
            repository.design.nodes.get(id).map(|node| {
                let sections = repository.sections.get(id);
                let assumptions = node.assumption_count();
                let decisions = sections
                    .map(|sections| {
                        sections
                            .decisions
                            .iter()
                            .filter(|decision| decision.status == "decided")
                            .count()
                    })
                    .unwrap_or(0);
                let readiness = sections
                    .map(|sections| sections.readiness_score())
                    .unwrap_or(0.0);
                crate::runtime_state::FocusedNodeSummary {
                    id: node.id.clone(),
                    title: node.title.clone(),
                    status: node.status,
                    open_questions: node.open_questions.len().saturating_sub(assumptions),
                    assumptions,
                    decisions,
                    readiness,
                    openspec_change: node.openspec_change.clone(),
                }
            })
        });
        let active_changes = repository
            .lifecycle
            .openspec
            .changes
            .iter()
            .map(|change| crate::runtime_state::ChangeSummary {
                name: change.name.clone(),
                stage: change.lifecycle_state.clone(),
                done_tasks: change.done_tasks,
                total_tasks: change.total_tasks,
            })
            .collect();

        Self {
            focused_node,
            active_changes,
        }
    }
}

pub(crate) fn project_memory_dir_if_initialized(project_root: &Path) -> Option<std::path::PathBuf> {
    // Canonical: ai/memory/, fallback: .omegon/memory/. Ordinary startup must
    // not create either path; /init is the explicit project-scaffold boundary.
    let ai = project_root.join("ai").join("memory");
    let omegon = project_root.join(".omegon").join("memory");
    if ai.exists() {
        Some(ai)
    } else if omegon.exists() {
        Some(omegon)
    } else {
        None
    }
}

pub(crate) fn ensure_project_memory_store_ready(
    db_path: &Path,
) -> anyhow::Result<Option<omegon_memory::sqlite::MemoryMigrationResult>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let status = omegon_memory::sqlite::SqliteBackend::status(db_path)?;
    match status.schema_version {
        omegon_memory::sqlite::MEMORY_SCHEMA_VERSION => {
            omegon_memory::sqlite::SqliteBackend::reconcile_current_default_mind(db_path)?;
            Ok(None)
        }
        version if omegon_memory::sqlite::LEGACY_MEMORY_SCHEMA_VERSIONS.contains(&version) => {
            let plan = omegon_memory::sqlite::SqliteBackend::plan_migration(db_path)?;
            let result = omegon_memory::sqlite::SqliteBackend::apply_migration(&plan)?;
            let verified = omegon_memory::sqlite::SqliteBackend::status(db_path)?;
            if verified.schema_version != omegon_memory::sqlite::MEMORY_SCHEMA_VERSION {
                anyhow::bail!(
                    "memory migration verification expected schema v{}, found v{}",
                    omegon_memory::sqlite::MEMORY_SCHEMA_VERSION,
                    verified.schema_version
                );
            }
            Ok(Some(result))
        }
        version => anyhow::bail!(
            "unsupported memory schema v{version} at {}; run `omegon memory migrate --status --path {}` and restore a supported v5-v7 backup or upgrade Omegon",
            db_path.display(),
            db_path.display()
        ),
    }
}

async fn managed_setup_error(bus: &mut EventBus, error: anyhow::Error) -> anyhow::Error {
    let report = bus.shutdown_managed_services().await;
    if report.all_resources_settled() {
        error
    } else {
        error.context(format!(
            "published managed-service cleanup did not settle: {report:?}"
        ))
    }
}

async fn published_setup_error(
    bus: &mut EventBus,
    dynamic: &mut crate::contribution_lifecycle::DynamicContributionGenerationOwner,
    error: anyhow::Error,
) -> anyhow::Error {
    let failures = dynamic.shutdown().await;
    let error = managed_setup_error(bus, error).await;
    if failures.is_empty() {
        error
    } else {
        error.context(format!(
            "published dynamic-contribution cleanup degraded: {}",
            failures.join("; ")
        ))
    }
}

impl AgentSetup {
    /// Initialize the event bus, tools, memory, lifecycle context, and conversation.
    pub async fn new(
        cwd: &Path,
        resume: Option<Option<&str>>,
        settings: Option<crate::settings::SharedSettings>,
    ) -> anyhow::Result<Self> {
        Self::new_with_safety(
            cwd,
            resume,
            settings,
            std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok(),
        )
        .await
    }

    pub async fn new_with_safety(
        cwd: &Path,
        resume: Option<Option<&str>>,
        settings: Option<crate::settings::SharedSettings>,
        dangerously_bypass_permissions: bool,
    ) -> anyhow::Result<Self> {
        Self::new_with_safety_and_mode(
            cwd,
            resume,
            settings,
            dangerously_bypass_permissions,
            "agent",
        )
        .await
    }

    pub async fn new_with_safety_and_mode(
        cwd: &Path,
        resume: Option<Option<&str>>,
        settings: Option<crate::settings::SharedSettings>,
        dangerously_bypass_permissions: bool,
        runtime_mode: &str,
    ) -> anyhow::Result<Self> {
        let cwd = std::fs::canonicalize(cwd)?;
        let is_child = std::env::var("OMEGON_CHILD").is_ok();
        let omegon_home = crate::paths::omegon_home()?;
        let component_policy =
            crate::component_policy::resolve_product_boot_policy(&cwd, &omegon_home)?;
        let component_dependency_policy = product_component_dependency_plan(&component_policy)?;

        // ─── Secrets manager ────────────────────────────────────────────
        let secrets_dir = crate::paths::omegon_home().unwrap_or_else(|_| cwd.join(".omegon"));
        let secrets = match omegon_secrets::SecretsManager::new(&secrets_dir) {
            Ok(s) => std::sync::Arc::new(s),
            Err(e) => {
                tracing::warn!("Failed to initialize secrets manager: {e}");
                std::sync::Arc::new(
                    omegon_secrets::SecretsManager::new(&std::env::temp_dir())
                        .expect("fallback secrets manager"),
                )
            }
        };
        // Normal startup is metadata-only for Keychain-backed secrets. Provider
        // clients and extension operations resolve their credentials lazily at
        // the explicit operation boundary; eagerly warming manifest/plugin
        // declarations causes one macOS authorization dialog per Keychain item
        // after ad-hoc development rebuilds.
        let selected_provider = settings
            .as_ref()
            .and_then(|settings| settings.lock().ok())
            .map(|guard| crate::providers::infer_provider_id(&guard.model));
        tracing::info!(
            selected_provider = ?selected_provider,
            child = is_child,
            "startup secret resolution deferred until use"
        );

        // Vault authentication is also deferred. A token recipe may itself be
        // Keychain-backed, so initializing the authenticated client here would
        // violate the zero-prompt startup invariant. Explicit Vault/secret
        // operations initialize or authenticate at their own boundary.
        let mut session_secret_env = secrets.session_env();
        let pre_hydrated_env_len = session_secret_env.len();
        if let Some(provider) = selected_provider.as_deref() {
            hydrate_selected_provider_auth_env_from_auth_json(
                provider,
                &mut session_secret_env,
                &secrets,
            );
        }
        for (idx, (name, value)) in session_secret_env.iter().enumerate() {
            if idx >= pre_hydrated_env_len
                || omegon_secrets::is_refreshable_oauth_secret_env(name.as_str())
            {
                // Provider auth copied from auth.json and refreshable OAuth
                // session tokens are only for child/delegate inheritance. Do
                // not promote them into this process environment: env
                // credentials have resolver priority over auth.json and would
                // freeze a shared disk credential into a per-process stale token.
                continue;
            }
            // SAFETY: setup runs before provider detection for this process; exporting
            // startup-resolved non-provider secrets here makes the active runtime see
            // the same credential surface as child/headless runs.
            unsafe { std::env::set_var(name, value) };
        }

        // Web auth secret: Try to load from preflight cache; fall back to ephemeral.
        // OMEGON_WEB_AUTH_SECRET is NOT preflighted (see above), so we'll get
        // an ephemeral root and will prompt for keychain access only if the user
        // actually performs a web search (on-demand).
        let web_auth_state = if let Some((_, secret)) = session_secret_env
            .iter()
            .find(|(name, _)| name == crate::web::WEB_AUTH_SECRET_NAME)
        {
            crate::web::WebAuthState::from_resolved_root(
                secret.clone(),
                crate::web::WebAuthSource::Keyring,
            )
        } else {
            // Not in preflight cache — generate ephemeral for this session.
            // Will upgrade to persistent keyring value on first web search.
            crate::web::WebAuthState::ephemeral_generated("session-generated".into())
        };
        let session_secret_diag = secrets.session_diagnostics();
        tracing::info!(
            warmed = session_secret_diag.len(),
            names = ?session_secret_diag
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            exported = session_secret_env.len(),
            child = is_child,
            "startup secret preflight summary"
        );
        tracing::debug!(diagnostics = ?session_secret_diag, "startup secret diagnostics");

        let project_root = find_project_root(&cwd);
        let mut bus = EventBus::new();
        bus.set_project_root(project_root.clone());
        let deferred_session_view = crate::session_consumers::DeferredSessionViewBinding::default();

        let boundary = if let Some(ref s) = settings {
            tools::WorkspaceBoundary::new(cwd.clone()).with_settings(s.clone())
        } else {
            tools::WorkspaceBoundary::new(cwd.clone())
        };

        // ─── Feature tool providers ─────────────────────────────────────
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "web-search",
            Box::new(tools::web_search::WebSearchProvider::with_secrets(
                secrets.clone(),
            )),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "local-inference",
            Box::new(tools::local_inference::LocalInferenceProvider::new()),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "view",
            Box::new(tools::view::ViewProvider::new(
                cwd.clone(),
                boundary.clone(),
            )),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "render",
            Box::new(tools::render::RenderProvider::new()),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "secret-tools",
            Box::new(tools::secret_tools::SecretToolsProvider::new(
                secrets.clone(),
            )),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "variable-tools",
            Box::new(tools::variable_tools::VariableToolsProvider),
        )));

        let openapi_configs = tools::openapi_config::load_openapi_configs(&project_root);
        if !openapi_configs.is_empty() {
            match tools::openapi::OpenApiToolProvider::from_configs(openapi_configs) {
                Ok(provider) => {
                    tracing::info!(
                        tools = provider.tool_count(),
                        "OpenAPI tool provider compiled"
                    );
                    bus.register(Box::new(features::adapter::ToolAdapter::new(
                        "openapi-tools",
                        Box::new(provider),
                    )));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to compile OpenAPI specs — skipping");
                }
            }
        }

        let codex_integration = crate::codex_config::load(&project_root);
        let codex_vault_path = codex_integration
            .as_ref()
            .map(|c| crate::codex_config::resolve_vault_path(&project_root, c));

        // ─── Memory ─────────────────────────────────────────────────────
        let mind = omegon_memory::sqlite::PRIMENSUS_MIND.to_string();
        let memory_dir = project_memory_dir_if_initialized(&project_root);
        let db_path = memory_dir.as_ref().map(|dir| dir.join("facts.db"));
        let jsonl_path = memory_dir.as_ref().map(|dir| dir.join("facts.jsonl"));

        let mut initial_memory_status = crate::status::MemoryStatus {
            total_facts: 0,
            active_facts: 0,
            project_facts: 0,
            persona_facts: 0,
            working_facts: 0,
            episodes: 0,
            edges: 0,
            active_persona_mind: None,
        };
        let mut memory_warning: Option<String> = None;
        let memory_binding = crate::memory_service::MemoryBinding::default();
        let memory_vault_config = match (codex_vault_path.as_ref(), codex_integration.as_ref()) {
            (Some(path), Some(integration)) => {
                match crate::memory_service::MemoryVaultConfigV1::validated(
                    path.clone(),
                    &integration.memory,
                ) {
                    Ok(config) => Some(config),
                    Err(error) => {
                        tracing::warn!(%error, "Codex vault memory synchronization disabled");
                        None
                    }
                }
            }
            _ => None,
        };
        let mut embed_service: Option<std::sync::Arc<dyn omegon_memory::EmbeddingService>> = None;
        if let Some(db_path) = db_path.as_ref() {
            if !is_child && let Some(migration) = ensure_project_memory_store_ready(db_path)? {
                tracing::warn!(
                    source_version = migration.source_version,
                    target_version = migration.target_version,
                    backup = %migration.backup.display(),
                    facts = migration.fact_count,
                    episodes = migration.episode_count,
                    "migrated legacy project memory store before startup"
                );
            }
            tracing::info!(mind = %mind, db = %db_path.display(), child = is_child, "managed memory candidate configured");
            // Skip the probe in child processes — the async HTTP request blocks
            // single-threaded runtimes (ACP, delegate children).
            if !is_child {
                let profile = crate::settings::Profile::load(&cwd);
                let svc = crate::embedding::OllamaEmbeddingService::from_config(
                    profile.embed_url.as_deref(),
                    profile.embed_model.as_deref(),
                );
                embed_service = if svc.probe().await {
                    tracing::info!(
                        url = svc.base_url(),
                        model = svc.model_name(),
                        "embedding service available — hybrid search enabled"
                    );
                    Some(std::sync::Arc::new(svc)
                        as std::sync::Arc<dyn omegon_memory::EmbeddingService>)
                } else {
                    #[cfg(feature = "local-embeddings")]
                    {
                        match crate::local_embedding::LocalEmbeddingService::from_default_dir() {
                            Ok(local_svc) => {
                                tracing::info!(
                                    model = local_svc.model_name(),
                                    "local ONNX embedding service loaded — hybrid search enabled"
                                );
                                Some(std::sync::Arc::new(local_svc)
                                    as std::sync::Arc<dyn omegon_memory::EmbeddingService>)
                            }
                            Err(_) => {
                                tracing::info!(
                                    "embedding service not reachable and no local model — FTS-only recall"
                                );
                                None
                            }
                        }
                    }
                    #[cfg(not(feature = "local-embeddings"))]
                    {
                        tracing::info!("embedding service not reachable — FTS-only recall");
                        None
                    }
                };
            }
        } else {
            tracing::info!(
                root = %project_root.display(),
                "project memory not initialized — skipping durable project memory backend; run /init to create ai/memory"
            );
            memory_warning = Some(
                "Project memory is not initialized — run `/init` to create `ai/memory/` for durable project facts."
                    .to_string(),
            );
        }
        let mut memory_feature =
            features::memory::MemoryFeature::new(memory_binding.clone(), mind.clone())
                .with_status_root(project_root.clone());
        if let Some(ref service) = embed_service {
            memory_feature = memory_feature
                .with_embed_service(service.clone())
                .with_extraction_model("anthropic:claude-haiku-4-5-20251001".into());
        }
        bus.register(Box::new(memory_feature));
        bus.register_internal_tool(crate::tool_registry::memory::MEMORY_STORE, "memory");

        // ─── Lifecycle (design-tree + openspec) ──────────────────────────
        // Use project root (git repo root), not cwd — docs/ and openspec/
        // live at the repo root, which may differ from cwd when running
        // from a subdirectory like core/.
        let lifecycle_binding = crate::lifecycle_service::LifecycleBinding::default();
        let lifecycle_host =
            crate::runtime_state::LifecycleHostHandle::new(lifecycle_binding.clone());
        let mut lifecycle_feature = features::lifecycle::LifecycleFeature::managed(
            &project_root,
            lifecycle_binding.clone(),
            lifecycle_host.clone(),
        );
        if let Some(ref vp) = codex_vault_path
            && codex_integration
                .as_ref()
                .is_some_and(|c| c.design_tree.enabled)
        {
            lifecycle_feature = lifecycle_feature.with_codex_vault(vp.clone());
            tracing::info!(vault = %vp.display(), "Codex vault sync enabled for design tree");
        }
        bus.register(Box::new(lifecycle_feature));

        // Declare the immutable work service in the initial boot graph. Its
        // snapshot is populated from the managed lifecycle observation before
        // setup publishes any consumer handles.
        let work_snapshot_publisher = register_work_aggregation(&mut bus);

        bus.register(Box::new(
            features::behavior_policy::BehaviorPolicyHostFeature,
        ));
        bus.register(Box::new(
            features::behavior_policy::BehaviorPolicyFeature::default(),
        ));
        let context_compaction =
            crate::context_compaction_service::ContextCompactionBinding::default();
        bus.register(Box::new(
            crate::context_compaction_service::ContextCompactionFeature,
        ));
        let git_binding = crate::git_service::GitBinding::default();
        bus.register(Box::new(crate::git_service::GitFeature));

        // ─── Sandbox setting (read once, shared by cleave + delegate) ──
        let sandbox = settings
            .as_ref()
            .and_then(|s| s.lock().ok())
            .map(|s| s.sandbox)
            .unwrap_or(false);

        // ─── Cleave + delegate shared inference runtime ────────────────
        let inference_runtime = crate::inference_runtime::InferenceRuntimeState::new(&project_root);
        // Local manifests and cached evidence must be admitted before the first
        // route is selected. Network discovery must not gate their visibility.
        let startup_report = inference_runtime.refresh().await;
        inference_runtime
            .record_refresh_report(&startup_report)
            .await;
        // Startup discovery is deliberately backgrounded: catalog reads project
        // the persisted cache immediately and never wait on provider networks.
        // The non-forced pass respects per-endpoint TTL; successful updates are
        // merged into the shared runtime snapshot for routing/delegate users.
        let startup_discovery = inference_runtime.clone();
        tokio::spawn(async move {
            let diagnostics = startup_discovery.refresh_discovery(false).await;
            let report = startup_discovery.refresh().await;
            startup_discovery.record_refresh_report(&report).await;
            if !diagnostics.is_empty() {
                tracing::debug!(
                    diagnostics = ?diagnostics,
                    "startup inference discovery retained last-known-good endpoint results"
                );
            }
        });

        // ─── Cleave (decomposition + dispatch) ─────────────────────────
        let mut cleave_feature = features::cleave::CleaveFeature::new_with_safety(
            &cwd,
            session_secret_env.clone(),
            sandbox,
            dangerously_bypass_permissions,
        );
        cleave_feature = cleave_feature
            .with_inference_runtime(inference_runtime.clone())
            .with_secrets(secrets.clone())
            .with_git(git_binding.clone());
        if let Some(settings) = settings.as_ref() {
            cleave_feature = cleave_feature.with_settings(settings.clone());
        }
        let cleave_handle = cleave_feature.shared_progress();
        // Capture the event-sender slot before bus.register consumes the
        // typed feature. main.rs writes the AgentEvent broadcast sender
        // into this slot once the channel exists, after which the cleave
        // feature can emit DecompositionStarted/ChildCompleted/Completed.
        let cleave_event_slot = cleave_feature.event_sender_slot();
        bus.register(Box::new(cleave_feature));

        // ─── Codescan (codebase_search / codebase_index) ──────────────
        let codescan_decision = component_policy.component("core:codescan");
        let codescan_binding =
            crate::codescan_service::CodescanBinding::from_component_decision(codescan_decision);
        bus.register(Box::new(crate::codescan_service::CodescanFeature::new(
            project_root.clone(),
            codescan_binding.clone(),
        )));
        if codescan_decision.is_some_and(|decision| !decision.enabled) {
            bus.set_policy_denied_tools([
                crate::tool_registry::codescan::CODEBASE_SEARCH,
                crate::tool_registry::codescan::CODEBASE_INDEX,
            ]);
        }
        // ─── Delegate (subagent system) ─────────────────────────────────
        let agents = crate::features::delegate::scan_agents(&cwd);
        let mut delegate_feature = features::delegate::DelegateFeature::new_with_safety(
            &cwd,
            agents,
            sandbox,
            dangerously_bypass_permissions,
        );
        delegate_feature = delegate_feature
            .with_inference_runtime(inference_runtime.clone())
            .with_secrets(secrets.clone());
        if let Some(settings) = settings.as_ref() {
            delegate_feature = delegate_feature.with_settings(settings.clone());
        }

        // Probe provider inventory so the delegate catalog is available
        // for context injection (lets the orchestrator see available models).
        if !is_child {
            let mut inventory = crate::routing::ProviderInventory::probe();
            inventory.probe_ollama().await;
            let inventory = std::sync::Arc::new(tokio::sync::RwLock::new(inventory));
            delegate_feature = delegate_feature.with_inventory(inventory);
        }

        let delegate_handle = delegate_feature.progress_handle();
        let delegate_tasks = delegate_feature.result_store_handle();
        let delegate_event_slot = delegate_feature.event_sender_slot();
        bus.register(Box::new(delegate_feature));

        // ─── Session log (context injection) ────────────────────────────
        bus.register(Box::new(
            features::session_log::SessionLog::new(&cwd)
                .with_session_binding(deferred_session_view.clone())
                .with_lifecycle(lifecycle_host.clone()),
        ));

        // ─── Audit log (structured JSONL trail for postmortem) ──────────
        let audit_session = std::env::var("OMEGON_SESSION_ID").unwrap_or_else(|_| {
            format!(
                "{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            )
        });
        bus.register(Box::new(
            features::audit_log::AuditLog::new(&cwd, &audit_session)
                .with_session_binding(deferred_session_view.clone()),
        ));

        // ─── Mutation (evolutionary skill/diagnostic creation) ───────────
        bus.register(Box::new(features::mutation::MutationFeature::new(
            crate::paths::omegon_home()?,
        )));

        // ─── Usage advisory (/usage from captured provider telemetry) ───
        bus.register(Box::new(features::usage::UsageFeature::new()));

        // ─── Prompt library (/prompt registry-native command surface) ───
        bus.register(Box::new(
            features::prompt::PromptFeature::with_workspace_root(cwd.clone()),
        ));
        bus.register(Box::new(features::loop_jobs::LoopFeature::new(
            &project_root,
        )));

        // ─── User command aliases (explicit prompt-targeted slash surfaces) ───
        bus.register(Box::new(
            features::user_commands::UserCommandFeature::load_for_workspace(&cwd),
        ));

        // ─── Clipboard paste retention (/clipboard prune) ────────────────
        // Manual on-demand sweep surface for clipboard image pastes.
        // The automatic 24h sweep at session start lives in main.rs;
        // this feature is the operator's override for forcing a sweep
        // mid-session. Both call paths share `clipboard::prune_old_pastes`.
        if let Some(ref settings) = settings {
            bus.register(Box::new(features::clipboard::ClipboardFeature::new(
                settings.clone(),
            )));
        }

        // ─── Model budget (grade intent + thinking) ───────────────────
        let model_budget_route = register_model_budget(&mut bus, settings.as_ref());

        // ─── Tool management ─────────────────────────────────────────────
        let manage_tools = features::manage_tools::ManageTools::new();
        let tool_admission = manage_tools.admission_handle();
        let tool_inventory = manage_tools.inventory_handle();
        bus.register(Box::new(manage_tools));

        // ─── Auth (credential probing + status) ───────────────────────
        let auth_feature = features::auth::AuthFeature::new();
        let auth_feature = if let Some(ref settings) = settings {
            auth_feature.with_settings(settings.clone())
        } else {
            auth_feature
        };
        bus.register(Box::new(auth_feature));

        // ─── Native features ────────────────────────────────────────────
        // ─── Persona system ────────────────────────────────────────────
        let mut persona_registry =
            crate::plugins::registry::AugmentRegistry::new(crate::prompt::load_lex_imperialis());
        persona_registry.loading_health = bus.contribution_health();
        let child_skills = crate::parse_csv_env("OMEGON_CHILD_SKILLS");
        if child_skills.is_empty() {
            persona_registry.load_skills(&cwd);
        } else {
            persona_registry.load_skills_subset(&cwd, &child_skills);
        }

        // Skill path declarations are admission requests, never persistent grants.
        let skill_trusted_paths = crate::skills::collect_trusted_paths(persona_registry.skills());
        if !skill_trusted_paths.is_empty() {
            tracing::info!(paths = ?skill_trusted_paths, "skills requested external paths; operator admission is required");
        }

        // ─── Extract skill phase info for completion tracking ──────────
        let skill_phases = crate::skills::collect_phase_info(persona_registry.skills());
        if !skill_phases.is_empty() {
            tracing::info!(
                count = skill_phases.len(),
                final_phases = ?skill_phases.iter().map(|p| &p.final_phase_label).collect::<Vec<_>>(),
                "loaded skill phase tracking"
            );
        }

        // ─── Activate startup persona/tone from child env or profile ────
        let startup_profile = crate::settings::Profile::load(&cwd);
        if let Some(persona_name) = std::env::var("OMEGON_CHILD_PERSONA")
            .ok()
            .or_else(|| startup_profile.persona.clone())
        {
            activate_startup_persona(&mut persona_registry, &cwd, &persona_name);
        }
        if let Some(tone_name) = startup_profile.tone.clone() {
            activate_startup_tone(&mut persona_registry, &cwd, &tone_name);
        }

        let shared_augment_registry =
            features::persona::SharedAugmentRegistry::new(persona_registry);
        bus.register(Box::new(
            features::persona::PersonaFeature::with_workspace_root(
                shared_augment_registry.clone(),
                cwd.clone(),
            ),
        ));
        bus.register_internal_tool(crate::tool_registry::persona::SWITCH_PERSONA, "persona");
        bus.register_internal_tool(crate::tool_registry::persona::SWITCH_TONE, "persona");
        bus.register(Box::new(features::skills::SkillsFeature::new(
            shared_augment_registry,
            cwd.clone(),
            crate::paths::omegon_home()?,
            child_skills,
        )));

        if let Some(ref settings) = settings {
            bus.register(Box::new(features::harness_settings::HarnessSettings::new(
                settings.clone(),
                project_root.clone(),
            )));
        }
        bus.register(Box::new(features::auto_compact::AutoCompact::new()));
        bus.register(Box::new(features::terminal_title::TerminalTitle::new(
            &cwd.to_string_lossy(),
        )));
        bus.register(Box::new(features::version_check::VersionCheck::new(env!(
            "CARGO_PKG_VERSION"
        ))));

        // ─── Context management provider ───────────────────────────────
        let context_metrics = features::context::SharedContextMetrics::new();
        let command_tx = features::context::new_shared_command_tx();
        let context_service =
            std::sync::Arc::new(features::context::ContextProvider::new_with_sources(
                context_metrics.clone(),
                command_tx.clone(),
                settings.clone(),
                Some(lifecycle_host.clone()),
                memory_binding.clone(),
                mind.clone(),
                Some(codescan_binding.clone()),
            ));
        bus.register(Box::new(context_service.as_ref().clone()));

        // ─── Operator-installed extensions (RPC + OCI) ────────────────
        // All extensions, including bundled ones (scribe-rpc), are discovered here
        let dynamic_inventory =
            crate::contribution_lifecycle::DynamicContributionInventory::with_loading_health(
                bus.contribution_health(),
            );
        let DiscoveredExtensions {
            extension_supervisors,
            extension_widgets,
            widget_receivers,
            vox_polling_handles,
            voice_notification_receivers,
            voice_polling_handles,
            extension_metadata,
            extension_rpc_handles,
            admission: extension_admission,
            discovery_attempts: _,
        } = match discover_and_register_extensions_with_policy(
            &cwd,
            &project_root,
            &mut bus,
            std::sync::Arc::clone(&secrets),
            dynamic_inventory.clone(),
            &component_policy,
        )
        .await
        {
            Ok(discovered) => discovered,
            Err(e) => {
                tracing::warn!("extension discovery failed: {}", e);
                DiscoveredExtensions::empty()
            }
        };

        // ─── Core tools (bash, read, write, edit, commit; hidden internal change) ──
        let core_tools = tools::CoreTools::with_git(cwd.clone(), git_binding.clone());
        let core_tools = if let Some(ref s) = settings {
            core_tools.with_settings(s.clone())
        } else {
            core_tools
        };
        let work_snapshot_slot = std::sync::Arc::new(std::sync::OnceLock::new());
        let core_tools = core_tools
            .with_secrets(secrets.clone())
            .with_work_snapshot_slot(std::sync::Arc::clone(&work_snapshot_slot));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(core_tools),
        )));
        // Register internal tools that the dispatch layer calls but the LLM never sees.
        bus.register_internal_tool(crate::tool_registry::core::TRUST_DIRECTORY, "core-tools");

        // ─── External plugins (TOML manifests) ────────────────────────
        let plugin_filter = crate::plugins::PluginSelectionFilter {
            enabled_extensions: crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS"),
            disabled_extensions: crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS"),
        };
        let plugins = crate::plugins::discover_plugins_filtered_with_inventory(
            &cwd,
            Some(secrets.as_ref()),
            &plugin_filter,
            dynamic_inventory.clone(),
        )
        .await;
        match crate::lifecycle_service::start_candidate(project_root.clone()).await {
            Ok(candidate) => bus.stage_managed_generation("lifecycle", candidate)?,
            Err(error) => tracing::warn!(
                %error,
                "managed lifecycle startup failed; lifecycle tools remain declared but unavailable"
            ),
        }
        match crate::context_compaction_service::start_candidate().await {
            Ok(candidate) => bus.stage_managed_generation("context-compaction", candidate)?,
            Err(error) => tracing::warn!(
                %error,
                "context/compaction startup failed; compaction planning is unavailable"
            ),
        }
        if project_root.join(".git").exists() || project_root.join(".jj").exists() {
            match crate::git_service::start_candidate(project_root.clone()).await {
                Ok(candidate) => bus.stage_managed_generation("git", candidate)?,
                Err(error) => tracing::warn!(
                    %error,
                    "managed Git startup failed; Git-backed operations are unavailable"
                ),
            }
        }
        if let Some(project_path) = db_path.clone() {
            let global_path = Some(crate::paths::user_config_dir().join("global-memory.db"))
                .filter(|path| path.is_file());
            let project_jsonl_path = jsonl_path
                .clone()
                .expect("project DB and JSONL paths derive from the same memory root");
            match crate::memory_service::start_candidate(
                crate::memory_service::MemoryWorkerConfig {
                    project_memory_root: memory_dir
                        .clone()
                        .expect("project memory paths derive from an initialized root"),
                    project_db_path: project_path,
                    project_jsonl_path,
                    global_db_path: global_path,
                    vault: memory_vault_config,
                    startup_sync_enabled: !is_child,
                },
            )
            .await
            {
                Ok(candidate) => bus.stage_managed_generation("memory", candidate)?,
                Err(error) => tracing::warn!(
                    %error,
                    "managed memory startup failed; memory binding remains unavailable"
                ),
            }
        }
        let (publication, mcp_supervisors, plugin_admissions) =
            plugins.publish_candidate(|plugins| {
                for plugin in plugins {
                    bus.register(plugin);
                }

                // Freeze declarations, validate and plan the candidate graph, then
                // publish legacy caches only from the accepted graph while plugin
                // admission locks remain held.
                bus.try_finalize_managed()
            });
        let mut dynamic_contributions =
            crate::contribution_lifecycle::DynamicContributionGenerationOwner::new(
                dynamic_inventory,
            );
        for supervisor in extension_supervisors {
            dynamic_contributions.own_extension(supervisor);
        }
        for supervisor in mcp_supervisors {
            dynamic_contributions.own_mcp(supervisor);
        }
        dynamic_contributions.stage();
        let publication_result = publication.await;
        if let Err(error) = publication_result {
            let cleanup_failures = dynamic_contributions.reject(error.to_string()).await;
            drop(plugin_admissions);
            drop(extension_admission);
            if cleanup_failures.is_empty() {
                return Err(error);
            }
            return Err(error.context(format!(
                "candidate cleanup degraded: {}",
                cleanup_failures.join("; ")
            )));
        }
        dynamic_contributions.publish();
        if let Err(error) = codescan_binding.capture(
            extension_rpc_handles
                .get(crate::codescan_service::CODESCAN_EXTENSION)
                .cloned(),
        ) {
            return Err(published_setup_error(&mut bus, &mut dynamic_contributions, error).await);
        }
        if let Err(error) = lifecycle_binding.capture(&bus) {
            return Err(published_setup_error(&mut bus, &mut dynamic_contributions, error).await);
        }
        if let Err(error) = memory_binding.capture(&bus) {
            return Err(published_setup_error(&mut bus, &mut dynamic_contributions, error).await);
        }
        if let Err(error) = context_compaction.capture(&bus) {
            return Err(published_setup_error(&mut bus, &mut dynamic_contributions, error).await);
        }
        if let Err(error) = git_binding.capture(&bus) {
            return Err(published_setup_error(&mut bus, &mut dynamic_contributions, error).await);
        }
        let git_snapshot = match git_binding
            .invoke(crate::git_service::GitRequest::Snapshot {
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
        {
            Ok(crate::git_service::GitResponse::Snapshot(snapshot)) => Some(snapshot),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(?error, "managed Git observation is unavailable");
                None
            }
        };
        if memory_binding.available() {
            match memory_binding
                .invoke(crate::memory_service::MemoryRequestV1::ManagedStatus {
                    scope: crate::memory_service::MemoryScopeV1::Project,
                    mind: mind.clone(),
                    cancellation: tokio_util::sync::CancellationToken::new(),
                })
                .await
            {
                Ok(crate::memory_service::MemoryResponseV1 {
                    payload: crate::memory_service::MemoryPayloadV1::ManagedStatus(status),
                    ..
                }) => {
                    let authority = status.authority.clone();
                    let index_state = status.index_state;
                    initial_memory_status = status.into();
                    crate::status::update_managed_memory_status(
                        crate::status::ManagedMemoryStatusSnapshot {
                            project_root: project_root.clone(),
                            available: true,
                            warning: None,
                            status: initial_memory_status.clone(),
                            authority,
                            index_state,
                        },
                    );
                }
                Ok(_) => {
                    memory_warning = Some("memory:startup_status_invalid_response".into());
                    tracing::warn!("managed memory readiness returned unexpected statistics");
                }
                Err(error) => {
                    memory_warning = Some("memory:startup_status_unavailable".into());
                    tracing::warn!(?error, "managed memory readiness statistics unavailable");
                }
            }
        }
        if !memory_binding.available() || memory_warning.is_some() {
            crate::status::update_managed_memory_status(
                crate::status::ManagedMemoryStatusSnapshot {
                    project_root: project_root.clone(),
                    available: false,
                    warning: memory_warning.clone(),
                    status: initial_memory_status.clone(),
                    authority: crate::memory_service::ManagedMemoryAuthorityV1::None,
                    index_state: crate::memory_service::ManagedMemoryIndexStateV1::Unknown,
                },
            );
        }
        if lifecycle_binding.available()
            && let Err(error) = lifecycle_host
                .refresh(
                    crate::lifecycle::read_model::SnapshotOptions::default(),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
        {
            tracing::warn!(error = %error, "managed lifecycle startup observation unavailable");
        }
        let lifecycle_observation = match lifecycle_host.observe() {
            Ok(observation) => observation.repository,
            Err(error) => {
                return Err(managed_setup_error(
                    &mut bus,
                    anyhow::anyhow!("managed lifecycle observation failed: {error:?}"),
                )
                .await);
            }
        };
        if let Some(observation) = lifecycle_observation {
            let snapshot =
                features::work_aggregation::WorkAggregationFeature::snapshot_from_observation(
                    observation,
                )
                .await;
            if let Err(error) = work_snapshot_publisher.publish(snapshot) {
                return Err(managed_setup_error(&mut bus, error).await);
            }
        }
        let lifecycle_snapshot = LifecycleSnapshot::from_managed(&lifecycle_host);
        drop(plugin_admissions);
        drop(extension_admission);
        let work_snapshot = match features::work_aggregation::capture_work_snapshot(&bus) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(managed_setup_error(&mut bus, error).await),
        };
        let behavior_policy = match features::behavior_policy::capture_behavior_policy(&bus) {
            Ok(policy) => policy,
            Err(error) => return Err(managed_setup_error(&mut bus, error).await),
        };
        if let Some(snapshot) = work_snapshot.as_ref()
            && let Err(error) = work_snapshot_slot
                .set(std::sync::Arc::clone(snapshot))
                .map_err(|_| anyhow::anyhow!("work snapshot slot was already initialized"))
        {
            return Err(managed_setup_error(&mut bus, error).await);
        }

        // Wire ManageTools state so runtime filtering and list output reflect
        // the bus's finalized model-visible tool cache.
        bus.set_tool_admission_policy(tool_admission.clone());
        bus.set_tool_inventory(tool_inventory.clone());

        // ─── Default tool profile — disable rarely-used tools ───────────
        {
            let (slim_mode, mut posture_disabled, posture_enabled, profile_terminal_tool) =
                settings
                    .as_ref()
                    .and_then(|s| {
                        s.lock().ok().map(|g| {
                            (
                                g.is_slim(),
                                g.posture_disabled_tools.clone(),
                                g.posture_enabled_tools.clone(),
                                g.terminal_tool,
                            )
                        })
                    })
                    .unwrap_or_else(|| (false, Vec::new(), Vec::new(), true));
            if !profile_terminal_tool {
                posture_disabled.push(crate::tool_registry::core::TERMINAL.into());
            } else if let Err(reason) = crate::tools::terminal::runtime_available() {
                tracing::warn!(
                    reason,
                    "terminal tool unavailable; disabling model-facing terminal tool"
                );
                posture_disabled.push(crate::tool_registry::core::TERMINAL.into());
            }
            bus.apply_operator_tool_profile(slim_mode, &posture_disabled, &posture_enabled);
            let mut disabled = tool_admission.lock().unwrap();
            tracing::info!(
                disabled = disabled.len(),
                slim = slim_mode,
                "default tool profile applied — use manage_tools to re-enable"
            );
            let child_enabled_tools = crate::parse_csv_env("OMEGON_CHILD_ENABLED_TOOLS");
            let child_disabled_tools = crate::parse_csv_env("OMEGON_CHILD_DISABLED_TOOLS");
            for tool in child_enabled_tools {
                disabled.remove(&tool);
            }
            for tool in child_disabled_tools {
                disabled.insert(tool);
            }
        }

        // ─── Assemble harness status (bootstrap probe) ──────────────────
        let mut harness_status = crate::status::HarnessStatus::assemble(&project_root);

        // Account for the active runtime profile before rendering bootstrap.
        // `HarnessStatus::assemble()` starts from conservative defaults; the
        // profile/model/settings are the authoritative source for route,
        // context, thinking, and capability orientation.
        if let Some(settings) = settings.as_ref()
            && let Ok(settings_guard) = settings.lock()
        {
            harness_status.update_from_settings(&settings_guard);
        }

        // Probe all authentication providers
        let auth_status = crate::auth::probe_all_providers().await;
        harness_status.providers = crate::auth::auth_status_to_provider_statuses(&auth_status);
        harness_status.annotate_provider_runtime_health();

        // Populate MCP/plugin info from discovered features
        harness_status.update_from_bus(&bus);
        if let Ok(skills) = crate::skills::list_structured() {
            harness_status.installed_plugins.extend(
                skills
                    .into_iter()
                    .filter(|skill| skill.installed || skill.project_local)
                    .map(|skill| crate::status::PluginSummary {
                        id: skill.id.unwrap_or_else(|| skill.name.clone()),
                        name: skill.name,
                        plugin_type: "skill".into(),
                        version: skill.version.unwrap_or_default(),
                        description: skill.description,
                    }),
            );
        }
        if let Ok(extensions_dir) = crate::extension_cli::extensions_dir()
            && let Ok(extensions) =
                crate::capabilities::extensions::list_installed_extension_capabilities_from_dir(
                    &extensions_dir,
                )
        {
            harness_status
                .installed_plugins
                .extend(
                    extensions
                        .into_iter()
                        .map(|extension| crate::status::PluginSummary {
                            id: extension.name.clone(),
                            name: extension.name,
                            plugin_type: "extension".into(),
                            version: extension.version,
                            description: extension.description,
                        }),
                );
        }
        harness_status.web_auth_mode = Some(web_auth_state.mode_name().to_string());
        harness_status.web_auth_source = Some(web_auth_state.source_name().to_string());

        // Populate memory stats from the initial count captured during DB load
        apply_initial_memory_status(
            &mut harness_status,
            initial_memory_status.clone(),
            memory_binding.available(),
            memory_warning.clone(),
        );
        harness_status.update_bootstrap_expectations();

        tracing::info!(
            providers = harness_status.providers.len(),
            mcp = harness_status.mcp_servers.len(),
            inference = harness_status.inference_backends.len(),
            container = harness_status.container_runtime.is_some(),
            facts = harness_status.memory.total_facts,
            web_auth_mode = harness_status.web_auth_mode.as_deref().unwrap_or("unknown"),
            web_auth_source = harness_status
                .web_auth_source
                .as_deref()
                .unwrap_or("unknown"),
            "harness status assembled"
        );

        // The TUI prints its compact summary after route/bridge resolution in
        // main. Explicit status and other interactive consumers keep diagnostics.
        if runtime_mode != "tui" && std::io::stderr().is_terminal() {
            let use_color = std::env::var("NO_COLOR").is_err();
            eprint!(
                "{}",
                crate::bootstrap_projection::render_bootstrap(&harness_status, use_color)
            );
        }

        // Emit BusEvent for features
        bus.emit_harness_status(&harness_status);

        // ─── System prompt + context ────────────────────────────────────
        // Build the base prompt from bus tool definitions.
        // Slim and constrained modes use compact schemas (stripped parameter
        // descriptions) to reduce token overhead by ~30-40%.
        let (slim_mode, current_model) = settings
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| (g.is_slim(), g.model.clone())))
            .unwrap_or((false, String::new()));
        let model_tier = crate::routing::infer_model_grade_band(&current_model);
        let prompt_mode = if matches!(
            model_tier,
            crate::routing::CapabilityGradeBand::Mid | crate::routing::CapabilityGradeBand::Leaf
        ) {
            prompt::PromptMode::Constrained
        } else if slim_mode {
            prompt::PromptMode::Slim
        } else {
            prompt::PromptMode::Full
        };
        let compact_schemas = true; // Always compact — stripped descriptions don't affect model behavior
        let tool_defs = bus.tool_definitions_mode(compact_schemas);
        let tool_count = tool_defs.len();
        let tool_tokens: usize = tool_defs
            .iter()
            .map(|t| {
                let schema = serde_json::to_string(&t.parameters).unwrap_or_default();
                (t.name.len() + t.description.len() + schema.len()) / 4
            })
            .sum();
        let automation_level = settings.as_ref().and_then(|settings| {
            settings
                .lock()
                .ok()
                .map(|settings| settings.automation_level)
        });
        let assembly = match automation_level {
            Some(level) => prompt::build_base_prompt_for_mode_with_subagent_policy(
                &cwd,
                &tool_defs,
                prompt_mode,
                crate::autonomy::subagent_policy_for_automation(level),
            ),
            None => prompt::build_base_prompt_for_mode(&cwd, &tool_defs, prompt_mode),
        };
        let base_prompt = match assembly {
            Ok(assembly) => assembly.prompt,
            Err(error) => {
                return Err(
                    published_setup_error(&mut bus, &mut dynamic_contributions, error).await,
                );
            }
        };
        let prompt_tokens = base_prompt.len() / 4;

        tracing::info!(
            tool_count,
            tool_tokens,
            prompt_tokens,
            compact = compact_schemas,
            mode = ?prompt_mode,
            "token budget: {} tools ~{}tok, system prompt ~{}tok",
            tool_count, tool_tokens, prompt_tokens,
        );

        // Context providers: the bus collects context from features, but we
        // still need the ContextManager for the injection pipeline (TTL decay,
        // budget management, priority sorting). Pass no standalone providers —
        // the bus will provide context via collect_context().
        let mut context_manager = ContextManager::new(base_prompt, vec![]);
        // Wire embedding service for semantic context relevance scoring
        if let Some(svc) = embed_service {
            context_manager.set_embed_service(svc);
        }

        // ─── Conversation ───────────────────────────────────────────────
        let mut resume_info: Option<ResumeInfo> = None;
        let mut resume_meta: Option<crate::session::SessionMeta> = None;
        let mut conversation = if let Some(resume_arg) = resume {
            let resume_id = resume_arg;
            // find_session returns the .json path; meta lives at .meta.json
            match session::find_session(&cwd, resume_id) {
                Some(path) => {
                    tracing::info!(path = %path.display(), "Resuming session");
                    match session::load_for_resume(&cwd, &path) {
                        Ok((conv, meta)) => {
                            crate::checkpoint::diagnose_startup_consistency(
                                &path,
                                &meta.session_id,
                            );

                            let description = crate::session::session_display_description(&meta);
                            resume_info = Some(ResumeInfo {
                                session_id: meta.session_id.clone(),
                                turns: meta.turns,
                                description,
                                last_prompt_snippet: meta.last_prompt_snippet.clone(),
                                created_at: meta.created_at.clone(),
                            });
                            resume_meta = Some(meta);
                            conv
                        }
                        Err(session::ResumeLoadError::Snapshot(e)) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "Failed to load session — starting fresh"
                            );
                            eprintln!(
                                "⚠ Could not restore session ({}). Starting fresh.\n  \
                                 Cause: {e}\n  \
                                 The saved session may be from an older version.",
                                path.display()
                            );
                            ConversationState::new()
                        }
                        Err(error @ session::ResumeLoadError::Authority(_)) => {
                            return Err(managed_setup_error(&mut bus, error.into()).await);
                        }
                    }
                }
                None => {
                    if resume_id.is_some() {
                        tracing::warn!("No matching session found — starting fresh");
                    }
                    ConversationState::new()
                }
            }
        } else {
            ConversationState::new()
        };

        if slim_mode {
            conversation.set_slim_mode(true);
        }

        let workspace_kind = crate::workspace::infer::infer_workspace_kind(&cwd);
        let workspace_project_root = find_project_root(&cwd);
        let project_id = crate::workspace::runtime::workspace_id_from_path(&workspace_project_root);
        let existing_workspace_lease = crate::workspace::runtime::read_workspace_lease(&cwd)
            .ok()
            .flatten();
        let existing_heartbeat = existing_workspace_lease.as_ref().and_then(|lease| {
            crate::workspace::runtime::heartbeat_epoch_secs(&lease.last_heartbeat)
        });
        let startup_session_id_hint = existing_workspace_lease
            .as_ref()
            .and_then(|lease| lease.owner_session_id.clone())
            .or_else(|| resume_info.as_ref().map(|info| info.session_id.clone()))
            .unwrap_or_else(crate::session::allocate_session_id);
        let workspace_admission_request = crate::workspace::types::WorkspaceAdmissionRequest {
            requested_role: crate::workspace::types::WorkspaceRole::Primary,
            requested_kind: workspace_kind,
            requested_mutability: crate::workspace::types::Mutability::Mutable,
            session_id: Some(startup_session_id_hint.clone()),
            action: crate::workspace::types::WorkspaceActionKind::SessionStart,
        };
        let workspace_admission = crate::workspace::admission::classify_admission(
            existing_workspace_lease.as_ref(),
            &workspace_admission_request,
            chrono::Utc::now().timestamp(),
            existing_heartbeat,
        );
        let workspace_lease = crate::workspace::types::WorkspaceLease {
            project_id: project_id.clone(),
            workspace_id: crate::workspace::runtime::workspace_id_from_path(&cwd),
            label: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.label.clone())
                .unwrap_or_else(|| "primary".into()),
            path: cwd.display().to_string(),
            backend_kind: crate::workspace::types::WorkspaceBackendKind::LocalDir,
            vcs_ref: git_snapshot.as_ref().map(|snapshot| {
                crate::workspace::types::WorkspaceVcsRef {
                    vcs: if snapshot.is_jj {
                        "jj".into()
                    } else {
                        "git".into()
                    },
                    branch: snapshot.branch.clone(),
                    revision: None,
                    remote: Some("origin".into()),
                }
            }),
            bindings: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.bindings.clone())
                .unwrap_or_default(),
            branch: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.branch.clone())
                .or_else(|| {
                    git_snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.branch.clone())
                })
                .unwrap_or_else(|| "(unknown)".into()),
            role: crate::workspace::types::WorkspaceRole::Primary,
            workspace_kind,
            mutability: crate::workspace::types::Mutability::Mutable,
            owner_session_id: Some(startup_session_id_hint.clone()),
            owner_agent_id: Some("omegon-local".into()),
            created_at: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.created_at.clone())
                .unwrap_or_else(crate::workspace::runtime::current_timestamp),
            last_heartbeat: crate::workspace::runtime::current_timestamp(),
            archived: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.archived)
                .unwrap_or(false),
            archived_at: existing_workspace_lease
                .as_ref()
                .and_then(|lease| lease.archived_at.clone()),
            archive_reason: existing_workspace_lease
                .as_ref()
                .and_then(|lease| lease.archive_reason.clone()),
            parent_workspace_id: existing_workspace_lease
                .as_ref()
                .and_then(|lease| lease.parent_workspace_id.clone()),
            source: "operator".into(),
        };
        let workspace_summary = crate::workspace::types::WorkspaceSummary {
            workspace_id: workspace_lease.workspace_id.clone(),
            label: workspace_lease.label.clone(),
            path: workspace_lease.path.clone(),
            backend_kind: workspace_lease.backend_kind,
            vcs_ref: workspace_lease.vcs_ref.clone(),
            bindings: workspace_lease.bindings.clone(),
            branch: workspace_lease.branch.clone(),
            role: workspace_lease.role,
            workspace_kind: workspace_lease.workspace_kind,
            mutability: workspace_lease.mutability,
            owner_session_id: workspace_lease.owner_session_id.clone(),
            last_heartbeat: workspace_lease.last_heartbeat.clone(),
            archived: workspace_lease.archived,
            archived_at: workspace_lease.archived_at.clone(),
            archive_reason: workspace_lease.archive_reason.clone(),
            stale: false,
        };
        let mut workspace_registry = crate::workspace::runtime::read_workspace_registry(&cwd)
            .ok()
            .flatten()
            .unwrap_or(crate::workspace::types::WorkspaceRegistry {
                project_id: project_id.clone(),
                repo_root: workspace_project_root.display().to_string(),
                workspaces: vec![],
            });
        workspace_registry.project_id = project_id;
        workspace_registry.repo_root = workspace_project_root.display().to_string();
        workspace_registry
            .workspaces
            .retain(|workspace| workspace.path != workspace_lease.path);
        workspace_registry.workspaces.push(workspace_summary);
        // Prune stale instance directories from previous runs before claiming ours.
        let pruned = crate::workspace::runtime::prune_stale_instances(&cwd);
        if !pruned.is_empty() {
            tracing::debug!(?pruned, "pruned stale instance directories");
        }
        let session_id = resume_info
            .as_ref()
            .map(|r| r.session_id.clone())
            .unwrap_or_else(|| startup_session_id_hint.clone());
        // Setup owns the canonical session identity. Every host receives this
        // event before the completed setup can execute a memory tool.
        bus.emit(&omegon_traits::BusEvent::SessionStart {
            cwd: cwd.clone(),
            session_id: session_id.clone(),
        });
        let session_snapshot = match crate::session_consumers::snapshot_path(&cwd, &session_id) {
            Some(path) => path,
            None => {
                return Err(managed_setup_error(
                    &mut bus,
                    anyhow::anyhow!("cannot determine interactive session path"),
                )
                .await);
            }
        };
        let session_view_binding =
            crate::session_consumers::SessionViewBinding::new(session_snapshot, session_id.clone());
        deferred_session_view.bind(session_view_binding.clone());

        let runtime_ownership =
            match crate::workspace::runtime::RuntimeOwnership::start(&cwd, runtime_mode) {
                Ok(ownership) => ownership,
                Err(error) => {
                    let managed_error = managed_setup_error(&mut bus, error).await;
                    let cleanup_failures = dynamic_contributions.shutdown().await;
                    if cleanup_failures.is_empty() {
                        return Err(managed_error);
                    }
                    return Err(managed_error.context(format!(
                        "published startup candidate cleanup degraded: {}",
                        cleanup_failures.join("; ")
                    )));
                }
            };
        bus.bind_runtime_ownership_retention(runtime_ownership.retention_flag());
        let instance_id = runtime_ownership.runtime_id().to_string();
        let _ =
            crate::workspace::runtime::write_workspace_lease(&cwd, &instance_id, &workspace_lease);
        let _ = crate::workspace::runtime::write_workspace_registry(&cwd, &workspace_registry);
        let workspace_state = WorkspaceStartupState {
            lease: workspace_lease,
            admission: workspace_admission,
        };

        let startup_snapshot = StartupSnapshot {
            total_facts: initial_memory_status.total_facts,
            lifecycle: lifecycle_snapshot,
        };

        let initial_harness_status = harness_status;

        let dynamic_contribution_control = dynamic_contributions.control();
        Ok(Self {
            bus,
            work_snapshot,
            behavior_policy,
            lifecycle_binding: lifecycle_binding.clone(),
            memory_binding: memory_binding.clone(),
            context_compaction: context_compaction.clone(),
            git_binding: git_binding.clone(),
            session_id,
            session_view_binding,
            instance_id,
            runtime_ownership,
            startup_skill_activation_events: Vec::new(),
            context_metrics,
            context_service,
            command_tx,
            context_manager,
            conversation,
            inference_runtime,
            model_budget_route,
            cwd,
            secrets: secrets.clone(),
            web_auth_state,
            session_secret_env,
            resume_info,
            resume_meta,
            workspace_state,
            startup_snapshot,
            initial_harness_status: initial_harness_status.clone(),
            dynamic_contributions,
            dynamic_contribution_control,
            component_policy,
            component_dependency_policy,
            extension_widgets,
            extension_metadata,
            extension_rpc_handles,
            widget_receivers,
            dashboard_handles: crate::runtime_state::RuntimeStateHandles::new(
                lifecycle_host,
                Some(cleave_handle),
                Some(delegate_handle),
                Some(delegate_tasks),
                Some(std::sync::Arc::new(std::sync::Mutex::new(
                    initial_harness_status.clone(),
                ))),
            ),
            cleave_event_slot,
            delegate_event_slot,
            vox_polling_handles,
            voice_notification_receivers,
            voice_polling_handles,
            skill_phases,
        })
    }

    /// Gather initial state for an interactive surface so its first projection
    /// has real setup data.
    pub fn interactive_initial_state(&self) -> InteractiveInitialState {
        InteractiveInitialState {
            total_facts: self.startup_snapshot.total_facts,
            focused_node: self.startup_snapshot.lifecycle.focused_node.clone(),
            active_changes: self.startup_snapshot.lifecycle.active_changes.clone(),
            workspace_status: Some(format!(
                "Workspace {} ({}) [{:?}/{:?}] backend={} owner={} admission={:?}",
                self.workspace_state.lease.workspace_id,
                self.workspace_state.lease.label,
                self.workspace_state.lease.role,
                self.workspace_state.lease.workspace_kind,
                self.workspace_state.lease.backend_kind.as_str(),
                self.workspace_state
                    .lease
                    .owner_session_id
                    .as_deref()
                    .unwrap_or("(none)"),
                self.workspace_state.admission
            )),
        }
    }
}

/// Find the project root for Omegon-local state.
///
/// Git discovery is intentionally bounded away from the user's home directory:
/// a `$HOME/.git` repository must not capture arbitrary child workspaces and
/// make `.omegon/`, memory, status, or generated git commands operate against
/// the wrong tree.
pub fn find_project_root(cwd: &Path) -> PathBuf {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut dir = cwd.clone();
    let mut nearest_soft_marker = if has_soft_project_marker(&cwd) {
        Some(cwd.clone())
    } else {
        None
    };

    loop {
        if has_non_git_hard_project_marker(&dir) {
            return dir;
        }

        let git_path = dir.join(".git");
        if git_path.is_dir() {
            if is_home_ancestor_repo(&dir, &cwd) {
                return nearest_soft_marker.unwrap_or(cwd);
            }
            return dir;
        }
        if git_path.is_file() {
            if is_home_ancestor_repo(&dir, &cwd) {
                return nearest_soft_marker.unwrap_or(cwd);
            }
            if let Ok(content) = std::fs::read_to_string(&git_path)
                && let Some(gitdir) = content.strip_prefix("gitdir: ")
            {
                let gitdir = gitdir.trim();
                let gitdir_path = if Path::new(gitdir).is_absolute() {
                    PathBuf::from(gitdir)
                } else {
                    dir.join(gitdir)
                };
                if let Some(repo) = gitdir_path
                    .parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                {
                    return repo.to_path_buf();
                }
            }
            return dir;
        }
        if nearest_soft_marker.is_none() && has_soft_project_marker(&dir) {
            nearest_soft_marker = Some(dir.clone());
        }
        if !dir.pop() {
            break;
        }
    }
    nearest_soft_marker.unwrap_or(cwd)
}

pub fn git_ceiling_directory(cwd: &Path) -> Option<PathBuf> {
    find_project_root(cwd).parent().map(Path::to_path_buf)
}

fn has_soft_project_marker(dir: &Path) -> bool {
    [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "Justfile",
        "justfile",
    ]
    .iter()
    .any(|marker| dir.join(marker).exists())
}

fn has_non_git_hard_project_marker(dir: &Path) -> bool {
    [".jj", ".codex", "AGENTS.md"]
        .iter()
        .any(|marker| dir.join(marker).exists())
}

fn is_home_ancestor_repo(repo_root: &Path, cwd: &Path) -> bool {
    cwd != repo_root
        && dirs::home_dir()
            .and_then(|home| home.canonicalize().ok())
            .is_some_and(|home| repo_root == home)
}

/// Scan installed extension manifests and collect all declared secret names.
/// Called during the startup preflight phase — before extensions are spawned —
/// so keyring-backed secrets are warmed into the session cache in time.
fn collect_extension_secret_requirements(cwd: &Path) -> Vec<String> {
    let ext_dir = match crate::paths::omegon_home() {
        Ok(home) => home.join("extensions"),
        Err(_) => return vec![],
    };
    if !ext_dir.exists() {
        return vec![];
    }
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(&ext_dir) else {
        return vec![];
    };
    let profile = crate::settings::Profile::load(cwd);
    let dynamic_admission =
        crate::dynamic_admission::DynamicAdmissionPolicy::from_profile(&profile);
    let env_enabled = crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS");
    let env_disabled = crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let ext_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        if !profile
            .extensions
            .permits(ext_name, &env_enabled, &env_disabled)
        {
            tracing::debug!(
                extension = ext_name,
                "extension skipped during secret preflight"
            );
            continue;
        }
        if extension_state_disabled(&path) {
            tracing::debug!(
                extension = ext_name,
                "disabled extension skipped during secret preflight"
            );
            continue;
        }
        if let Ok(manifest) = crate::extensions::ExtensionManifest::from_extension_dir(&path) {
            let admitted = crate::extensions::dynamic_preflight(&manifest, &path)
                .and_then(|preflight| dynamic_admission.admit(preflight).map(|_| ()));
            if let Err(error) = admitted {
                tracing::debug!(extension = ext_name, %error, "untrusted extension skipped during secret preflight");
                continue;
            }
            for name in manifest.secrets.required {
                tracing::debug!(
                    extension = %path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    secret = %name,
                    "extension declared required secret"
                );
                names.push(name);
            }
            // Required extension secrets are preflighted because the extension
            // cannot start correctly without them. Optional secrets are resolved
            // lazily during extension spawn/use; eagerly resolving them forces
            // avoidable macOS Keychain prompts after each ad-hoc rebuilt binary.
        }
    }
    names
}

fn hydrate_selected_provider_auth_env_from_auth_json(
    provider_id: &str,
    session_secret_env: &mut Vec<(String, String)>,
    secrets: &omegon_secrets::SecretsManager,
) {
    let Some(provider) = crate::auth::provider_by_id(provider_id) else {
        return;
    };
    let Some(primary_env) = provider.env_vars.first().copied() else {
        return;
    };
    if session_secret_env
        .iter()
        .any(|(name, _)| name == primary_env)
    {
        return;
    }

    // Startup reads only Omegon-owned auth.json. External CLI adoption is an
    // explicit login/first-use concern and must not fan out into Keychain/UI
    // interaction while the application is merely starting.
    let Some(creds) = crate::auth::read_credentials(provider.auth_key) else {
        return;
    };
    if creds.cred_type == "oauth" && creds.is_expired() {
        tracing::debug!(
            provider = provider.id,
            env = primary_env,
            "skipping expired provider OAuth env hydration from auth.json"
        );
        return;
    }

    secrets.register_redaction_secret(primary_env, &creds.access);
    secrets.register_redaction_secret(&format!("{}_AUTH_JSON_ACCESS", provider.id), &creds.access);
    secrets.register_redaction_secret(
        &format!("{}_AUTH_JSON_REFRESH", provider.id),
        &creds.refresh,
    );
    if let Some(account_id) = crate::auth::read_credential_extra(provider.auth_key, "accountId") {
        secrets.register_redaction_secret(
            &format!("{}_AUTH_JSON_ACCOUNT_ID", provider.id),
            &account_id,
        );
    }
    session_secret_env.push((primary_env.to_string(), creds.access));
    tracing::info!(
        provider = provider.id,
        env = primary_env,
        source = "auth.json",
        "hydrated selected provider auth env"
    );
}

/// Scan plugin manifests and project MCP config for `{VAR_NAME}` template references.
/// Called during the startup preflight phase so vault-backed secrets used by MCP
/// servers (e.g. `env = { MY_TOKEN = "{MY_TOKEN}" }`) are warmed before plugins connect.
fn collect_plugin_secret_requirements(cwd: &std::path::Path) -> Vec<String> {
    let mut names = Vec::new();

    // Helper: extract {VAR_NAME} references from a string
    fn extract_templates(s: &str, out: &mut Vec<String>) {
        let mut i = 0;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'{'
                && let Some(end) = s[i + 1..].find('}')
            {
                let var = &s[i + 1..i + 1 + end];
                if !var.is_empty() && var.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
                    out.push(var.to_string());
                }
                i += end + 2;
                continue;
            }
            i += 1;
        }
    }

    // Helper: scan a HashMap<String, McpServerConfig> for env template vars
    fn scan_servers(
        servers: &std::collections::HashMap<String, crate::plugins::mcp::McpServerConfig>,
        out: &mut Vec<String>,
    ) {
        for config in servers.values() {
            for value in config.env.values() {
                extract_templates(value, out);
            }
        }
    }

    // 1. User-level plugin manifests: ~/.omegon/plugins/*/plugin.toml
    let plugin_dirs: Vec<std::path::PathBuf> = [
        crate::paths::omegon_home().ok().map(|h| h.join("plugins")),
        Some(cwd.join(".omegon/plugins")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for dir in &plugin_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let manifest_path = entry.path().join("plugin.toml");
            let Ok(content) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            // Try armory-style manifest (has MCP servers)
            if let Ok(manifest) = crate::plugins::armory::ArmoryManifest::parse(&content) {
                scan_servers(&manifest.mcp_servers, &mut names);
            }
        }
    }

    // 2. Project-level MCP config: {cwd}/.omegon/mcp.toml
    let mcp_toml = cwd.join(".omegon/mcp.toml");
    if let Ok(content) = std::fs::read_to_string(&mcp_toml)
        && let Ok(servers) = toml::from_str::<
            std::collections::HashMap<String, crate::plugins::mcp::McpServerConfig>,
        >(&content)
    {
        scan_servers(&servers, &mut names);
    }

    // Deduplicate
    names.sort_unstable();
    names.dedup();
    tracing::debug!(
        count = names.len(),
        names = ?names,
        "plugin MCP env template vars collected for preflight"
    );
    names
}
fn extension_secret_names(manifest: &crate::extensions::ExtensionManifest) -> Vec<String> {
    let mut names = manifest.secrets.required.clone();
    for name in &manifest.secrets.optional {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

async fn resolve_extension_secrets(
    manifest: &crate::extensions::ExtensionManifest,
    secrets: &omegon_secrets::SecretsManager,
) -> Vec<(String, String)> {
    let mut resolved = Vec::new();
    for name in extension_secret_names(manifest) {
        if let Some(value) = secrets.resolve_async(&name).await {
            resolved.push((name, value));
        }
    }
    resolved
}

///
/// Resolves declared secrets at the enabled extension's spawn boundary and
/// extension via `bootstrap_secrets` RPC — never via subprocess environment.
struct DiscoveredExtensions {
    extension_supervisors: Vec<std::sync::Arc<crate::extensions::ExtensionSupervisor>>,
    extension_widgets: Vec<crate::extensions::ExtensionTabWidget>,
    widget_receivers: Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
    vox_polling_handles: Vec<crate::extensions::ExtensionPollingHandle>,
    voice_notification_receivers:
        Vec<tokio::sync::mpsc::UnboundedReceiver<crate::extensions::ExtensionNotification>>,
    voice_polling_handles: Vec<crate::extensions::ExtensionPollingHandle>,
    extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
    extension_rpc_handles:
        std::collections::BTreeMap<String, crate::extensions::ExtensionPollingHandle>,
    admission: Option<crate::contribution_loading::GuardedContributionDirectory>,
    discovery_attempts: Vec<String>,
}

pub(crate) struct KernelCompositionSetup {
    pub(crate) bus: crate::bus::EventBus,
    pub(crate) dynamic_contributions:
        crate::contribution_lifecycle::DynamicContributionGenerationOwner,
    pub(crate) dynamic_control: crate::contribution_lifecycle::DynamicContributionControl,
    pub(crate) component_policy: crate::component_policy::ResolvedComponentPolicy,
    pub(crate) component_dependency_policy:
        crate::contribution_graph::ComponentDependencyPolicyPlan,
    _extension_admission: Option<crate::contribution_loading::GuardedContributionDirectory>,
}

pub(crate) async fn setup_kernel_composition(cwd: &Path) -> anyhow::Result<KernelCompositionSetup> {
    let project_root = find_project_root(cwd);
    let home = crate::paths::omegon_home()?;
    let component_policy = crate::component_policy::resolve_product_boot_policy(cwd, &home)?;
    let component_dependency_policy = product_component_dependency_plan(&component_policy)?;
    let mut bus = crate::bus::EventBus::new();
    bus.set_project_root(project_root.clone());
    let codescan_decision = component_policy.component("core:codescan");
    let codescan_binding =
        crate::codescan_service::CodescanBinding::from_component_decision(codescan_decision);
    bus.register(Box::new(crate::codescan_service::CodescanFeature::new(
        project_root.clone(),
        codescan_binding.clone(),
    )));
    if codescan_decision.is_some_and(|decision| !decision.enabled) {
        bus.set_policy_denied_tools([
            crate::tool_registry::codescan::CODEBASE_SEARCH,
            crate::tool_registry::codescan::CODEBASE_INDEX,
        ]);
    }
    bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
        "core-tools",
        Box::new(crate::tools::CoreTools::with_git(
            cwd.to_path_buf(),
            crate::git_service::GitBinding::default(),
        )),
    )));
    bus.register_internal_tool(crate::tool_registry::core::TRUST_DIRECTORY, "core-tools");

    let inventory =
        crate::contribution_lifecycle::DynamicContributionInventory::with_loading_health(
            bus.contribution_health(),
        );
    let secrets = std::sync::Arc::new(omegon_secrets::SecretsManager::new(&home.join("secrets"))?);
    let DiscoveredExtensions {
        extension_supervisors,
        extension_rpc_handles,
        admission,
        ..
    } = discover_and_register_extensions_with_policy(
        cwd,
        &project_root,
        &mut bus,
        secrets,
        inventory.clone(),
        &component_policy,
    )
    .await?;
    let mut dynamic_contributions =
        crate::contribution_lifecycle::DynamicContributionGenerationOwner::new(inventory);
    for supervisor in extension_supervisors {
        dynamic_contributions.own_extension(supervisor);
    }
    dynamic_contributions.stage();
    if let Err(error) = bus.try_finalize_managed().await {
        let cleanup = dynamic_contributions.reject(error.to_string()).await;
        return Err(error.context(format!("kernel candidate cleanup: {cleanup:?}")));
    }
    dynamic_contributions.publish();
    codescan_binding.capture(
        extension_rpc_handles
            .get(crate::codescan_service::CODESCAN_EXTENSION)
            .cloned(),
    )?;
    let dynamic_control = dynamic_contributions.control();
    Ok(KernelCompositionSetup {
        bus,
        dynamic_contributions,
        dynamic_control,
        component_policy,
        component_dependency_policy,
        _extension_admission: admission,
    })
}

impl DiscoveredExtensions {
    fn empty() -> Self {
        Self {
            extension_supervisors: vec![],
            extension_widgets: vec![],
            widget_receivers: vec![],
            vox_polling_handles: vec![],
            voice_notification_receivers: vec![],
            voice_polling_handles: vec![],
            extension_metadata: Default::default(),
            extension_rpc_handles: Default::default(),
            admission: None,
            discovery_attempts: Vec::new(),
        }
    }
}

async fn discover_and_register_extensions(
    cwd: &Path,
    project_root: &Path,
    bus: &mut crate::bus::EventBus,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
) -> anyhow::Result<DiscoveredExtensions> {
    let home = crate::paths::omegon_home()?;
    let policy = crate::component_policy::resolve_product_boot_policy(cwd, &home)?;
    product_component_dependency_plan(&policy)?;
    discover_and_register_extensions_with_policy(
        cwd,
        project_root,
        bus,
        secrets,
        inventory,
        &policy,
    )
    .await
}

fn product_component_dependency_plan(
    policy: &crate::component_policy::ResolvedComponentPolicy,
) -> anyhow::Result<crate::contribution_graph::ComponentDependencyPolicyPlan> {
    let codescan = omegon_traits::RuntimeContributionId::new("extension:omegon-codescan")
        .expect("product codescan contribution id is valid");
    let denied = policy
        .component("core:codescan")
        .is_some_and(|decision| !decision.enabled)
        .then_some(codescan.clone());
    crate::contribution_graph::apply_component_dependency_policy([codescan], denied, [], [])
}

async fn discover_and_register_extensions_with_policy(
    cwd: &Path,
    project_root: &Path,
    bus: &mut crate::bus::EventBus,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
    component_policy: &crate::component_policy::ResolvedComponentPolicy,
) -> anyhow::Result<DiscoveredExtensions> {
    use crate::contribution_health::{ContributionKind::Extensions, LoadingState, ScopeHealth};
    let health = inventory.loading_health.clone();
    let root = crate::paths::omegon_home()
        .map(|home| home.join("extensions"))
        .unwrap_or_else(|_| PathBuf::from("~/.omegon/extensions"));
    let mut outcomes = Vec::new();
    let result = discover_and_register_extensions_with_policy_inner(
        cwd,
        project_root,
        bus,
        secrets,
        inventory,
        component_policy,
        &mut outcomes,
    )
    .await;
    let outcome = match &result {
        Ok(discovered) => ScopeHealth::new(
            Extensions,
            "user",
            &root,
            if discovered.admission.is_none() {
                LoadingState::Absent
            } else {
                LoadingState::Loaded {
                    count: discovered.extension_metadata.len(),
                }
            },
        ),
        Err(error) => ScopeHealth::blocked(Extensions, "user", &root, error),
    };
    outcomes.push(outcome);
    health.replace_kind(Extensions, outcomes);
    result
}

async fn discover_and_register_extensions_with_policy_inner(
    cwd: &Path,
    project_root: &Path,
    bus: &mut crate::bus::EventBus,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
    component_policy: &crate::component_policy::ResolvedComponentPolicy,
    outcomes: &mut Vec<crate::contribution_health::ScopeHealth>,
) -> anyhow::Result<DiscoveredExtensions> {
    use crate::contribution_health::{ContributionKind::Extensions, ScopeHealth};
    let home = crate::paths::omegon_home()?;
    let ext_dir = home.join("extensions");
    let admission = crate::contribution_loading::GuardedContributionDirectory::open(
        &home,
        &[b"extensions"],
        &home,
        omegon_maintenance_contracts::ContributionKind::Extension,
        "user",
    )?;

    let profile = crate::settings::Profile::load(cwd);
    let dynamic_admission =
        crate::dynamic_admission::DynamicAdmissionPolicy::from_profile(&profile);
    let env_enabled = crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS");
    let env_disabled = crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS");
    let mut count = 0;
    let mut extension_supervisors = Vec::new();
    let mut extension_widgets = vec![];
    let mut widget_receivers = vec![];
    let mut vox_polling_handles = vec![];
    let mut voice_notification_receivers = vec![];
    let mut voice_polling_handles = vec![];
    let mut extension_metadata = std::collections::BTreeMap::new();
    let mut extension_rpc_handles = std::collections::BTreeMap::new();
    let mut candidates = Vec::new();
    let codescan_enabled = component_policy
        .component("core:codescan")
        .is_none_or(|decision| decision.enabled);
    let mut operator_codescan_present = false;
    let release_codescan_dir = codescan_enabled
        .then(release_coupled_codescan_dir)
        .flatten();
    let mut discovery_attempts = Vec::new();
    let mut raw_names = match admission.as_ref() {
        Some(admission) => admission.entry_names(10_000)?,
        None => Vec::new(),
    };
    raw_names.sort();
    for raw_name in raw_names {
        let admission = admission
            .as_ref()
            .expect("extension names require an admitted extension directory");
        if crate::contribution_loading::is_internal_contribution_entry(&raw_name)
            || !admission.allows(&raw_name)?
        {
            continue;
        }
        let Ok(ext_name) = std::str::from_utf8(&raw_name) else {
            continue;
        };
        if ext_name == crate::codescan_service::CODESCAN_EXTENSION && !codescan_enabled {
            tracing::info!(
                component = "core:codescan",
                "release-coupled component omitted by boot policy"
            );
            continue;
        }
        if !profile
            .extensions
            .permits(ext_name, &env_enabled, &env_disabled)
        {
            tracing::debug!(extension = ext_name, "extension skipped by profile policy");
            continue;
        }
        discovery_attempts.push(ext_name.to_string());
        let Some(directory) = admission.open_child_directory(&raw_name)? else {
            continue;
        };
        if ext_name == crate::codescan_service::CODESCAN_EXTENSION
            && release_codescan_dir.is_some()
            && crate::contribution_loading::read_file_at(
                &directory,
                b".omegon-release-coupled",
                1024,
            )?
            .is_some()
        {
            tracing::info!(
                extension = ext_name,
                "installed release supersedes development codescan copy"
            );
            continue;
        }
        if crate::contribution_loading::read_file_at(&directory, b"manifest.toml", 1024 * 1024)?
            .is_none()
        {
            continue;
        }
        let snapshot = std::sync::Arc::new(
            crate::contribution_loading::snapshot_contribution_directory(&directory)?,
        );
        if extension_state_disabled(snapshot.path()) {
            tracing::debug!(extension = ext_name, "disabled extension skipped");
            continue;
        }
        let manifest = match crate::extensions::ExtensionManifest::from_extension_dir(
            snapshot.path(),
        ) {
            Ok(manifest) => manifest,
            Err(error) => {
                tracing::warn!(extension = ext_name, %error, "invalid extension manifest skipped");
                outcomes.push(ScopeHealth::blocked(
                    Extensions,
                    &format!("user/{ext_name}"),
                    &ext_dir.join(ext_name),
                    &error,
                ));
                continue;
            }
        };
        if manifest.extension.name != ext_name {
            tracing::warn!(
                extension = ext_name,
                manifest_name = %manifest.extension.name,
                "extension directory and manifest identities differ"
            );
            outcomes.push(ScopeHealth::blocked(
                Extensions,
                &format!("user/{ext_name}"),
                &ext_dir.join(ext_name),
                &anyhow::anyhow!("extension directory and manifest identities differ"),
            ));
            continue;
        }
        let preflight = match crate::extensions::dynamic_preflight(&manifest, snapshot.path()) {
            Ok(preflight) => preflight,
            Err(error) => {
                tracing::warn!(extension = ext_name, %error, "extension static preflight failed");
                outcomes.push(ScopeHealth::blocked(
                    Extensions,
                    &format!("user/{ext_name}"),
                    &ext_dir.join(ext_name),
                    &error,
                ));
                continue;
            }
        };
        let candidate = match inventory.discover(preflight) {
            Ok(candidate) => candidate,
            Err(error) => {
                tracing::warn!(extension = ext_name, %error, "extension static inventory rejected candidate");
                outcomes.push(ScopeHealth::blocked(
                    Extensions,
                    &format!("user/{ext_name}"),
                    &ext_dir.join(ext_name),
                    &error,
                ));
                continue;
            }
        };
        let trust_admission = match inventory.admit(&candidate, &dynamic_admission) {
            Ok(admission) => admission,
            Err(error) => {
                if ext_name == crate::codescan_service::CODESCAN_EXTENSION {
                    inventory.forget_rejected(&candidate.preflight.id);
                }
                tracing::warn!(extension = ext_name, %error, "extension trust admission denied before execution");
                outcomes.push(ScopeHealth::blocked(
                    Extensions,
                    &format!("user/{ext_name}"),
                    &ext_dir.join(ext_name),
                    &error,
                ));
                continue;
            }
        };
        operator_codescan_present |= ext_name == crate::codescan_service::CODESCAN_EXTENSION;
        candidates.push((
            ext_name.to_string(),
            ext_dir.join(ext_name),
            snapshot,
            manifest,
            trust_admission,
            candidate.preflight.id,
            false,
        ));
    }

    if !operator_codescan_present
        && profile.extensions.permits(
            crate::codescan_service::CODESCAN_EXTENSION,
            &env_enabled,
            &env_disabled,
        )
        && let Some(release_dir) = release_codescan_dir
    {
        let bundled_root = release_dir.clone();
        let bundled = (|| -> anyhow::Result<_> {
            let source = std::fs::File::open(&release_dir)?;
            let snapshot = std::sync::Arc::new(
                crate::contribution_loading::snapshot_contribution_directory(&source)?,
            );
            let manifest =
                crate::extensions::ExtensionManifest::from_extension_dir(snapshot.path())?;
            if manifest.extension.name != crate::codescan_service::CODESCAN_EXTENSION {
                anyhow::bail!("release-coupled codescan manifest identity is invalid");
            }
            let preflight = crate::extensions::dynamic_preflight(&manifest, snapshot.path())?;
            let candidate = inventory.discover(preflight)?;
            let trust_admission = inventory.admit_kernel_release(&candidate)?;
            Ok((
                crate::codescan_service::CODESCAN_EXTENSION.to_string(),
                release_dir,
                snapshot,
                manifest,
                trust_admission,
                candidate.preflight.id,
                true,
            ))
        })();
        match bundled {
            Ok(candidate) => candidates.push(candidate),
            Err(error) => {
                tracing::warn!(%error, "release-coupled codescan extension skipped");
                outcomes.push(ScopeHealth::blocked(
                    Extensions,
                    "release/omegon-codescan",
                    &bundled_root,
                    &error,
                ));
            }
        }
    }

    for (ext_name, state_dir, snapshot, manifest, trust_admission, candidate_id, release_coupled) in
        candidates
    {
        // Spawning an enabled extension is its explicit operation boundary:
        // resolve declared credentials on demand here, then deliver them only
        // through bootstrap_secrets RPC. Discovery/status paths remain
        // metadata-only and therefore cannot trigger secure-store access.
        let resolved_secrets = resolve_extension_secrets(&manifest, secrets.as_ref()).await;

        // Try to spawn this extension
        let spawned = if release_coupled {
            crate::extensions::spawn_from_release_snapshot(
                snapshot,
                trust_admission,
                inventory.clone(),
                project_root,
                &resolved_secrets,
            )
            .await
        } else {
            crate::extensions::spawn_from_admitted_snapshot(
                snapshot,
                &state_dir,
                trust_admission,
                inventory.clone(),
                project_root,
                &resolved_secrets,
            )
            .await
        };
        match spawned {
            Ok(spawned) => {
                inventory.ready(&candidate_id);
                let tool_count = spawned.feature.tools().len();
                let widget_count = spawned.widgets.len();
                tracing::info!(
                    name = %ext_name,
                    path = %state_dir.display(),
                    tools = tool_count,
                    widgets = widget_count,
                    "discovered and spawned extension"
                );
                extension_supervisors.push(spawned.supervisor.clone());
                // Collect vox polling handle if present
                if let Some(handle) = spawned.vox_polling_handle {
                    vox_polling_handles.push(handle);
                }
                if let Some(handle) = spawned.voice_polling_handle {
                    voice_polling_handles.push(handle);
                }
                if let Some(rx) = spawned.voice_notification_rx {
                    voice_notification_receivers.push(rx);
                }
                extension_metadata.insert(
                    ext_name.clone(),
                    crate::extensions::metadata_with_sdk_compatibility(
                        spawned.metadata,
                        &spawned.sdk_compatibility,
                    ),
                );
                extension_rpc_handles.insert(ext_name, spawned.rpc_polling_handle);
                bus.register(spawned.feature);
                // Collect widgets and receivers for TUI
                extension_widgets.extend(spawned.widgets);
                widget_receivers.push(spawned.widget_rx);
                count += 1;
            }
            Err(e) => {
                inventory.quarantine(&candidate_id, e.to_string());
                tracing::warn!(
                    name = %ext_name,
                    path = %state_dir.display(),
                    error = %e,
                    "failed to spawn extension"
                );
                outcomes.push(ScopeHealth::blocked(
                    Extensions,
                    &format!(
                        "{}/{ext_name}",
                        if release_coupled { "release" } else { "user" }
                    ),
                    &state_dir,
                    &e,
                ));
            }
        }
    }

    if count > 0 {
        tracing::info!(count = count, "extension discovery complete");
    }

    Ok(DiscoveredExtensions {
        extension_supervisors,
        extension_widgets,
        widget_receivers,
        vox_polling_handles,
        voice_notification_receivers,
        voice_polling_handles,
        extension_metadata,
        extension_rpc_handles,
        admission,
        discovery_attempts,
    })
}

pub(crate) enum InstalledExtensionReplacement {
    Unchanged,
    Changed(crate::contribution_lifecycle::StagedDynamicExtensionGeneration),
}

pub(crate) async fn stage_installed_extension_replacement(
    cwd: &Path,
    name: &str,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
) -> anyhow::Result<InstalledExtensionReplacement> {
    stage_installed_extension_replacement_from_home(
        cwd,
        &crate::paths::omegon_home()?,
        name,
        secrets,
        inventory,
    )
    .await
}

pub(crate) async fn stage_installed_extension_replacement_from_home(
    cwd: &Path,
    home: &Path,
    name: &str,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
) -> anyhow::Result<InstalledExtensionReplacement> {
    use crate::contribution_health::{ContributionKind::Extensions, LoadingState, ScopeHealth};
    let health = inventory.loading_health.clone();
    let scope = format!("user/{name}");
    let root = home.join("extensions").join(name);
    let result =
        stage_installed_extension_replacement_from_home_inner(cwd, home, name, secrets, inventory)
            .await;
    health.replace_scope(match &result {
        Ok(_) => ScopeHealth::new(Extensions, &scope, &root, LoadingState::Loaded { count: 1 }),
        Err(error) => ScopeHealth::blocked(Extensions, &scope, &root, error),
    });
    result
}

async fn stage_installed_extension_replacement_from_home_inner(
    cwd: &Path,
    home: &Path,
    name: &str,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    inventory: crate::contribution_lifecycle::DynamicContributionInventory,
) -> anyhow::Result<InstalledExtensionReplacement> {
    let id = omegon_traits::RuntimeContributionId::new(format!("extension:{name}"))
        .map_err(|error| anyhow::anyhow!(error))?;
    let admission = crate::contribution_loading::GuardedContributionDirectory::open(
        home,
        &[b"extensions"],
        home,
        omegon_maintenance_contracts::ContributionKind::Extension,
        "user",
    )?
    .ok_or_else(|| anyhow::anyhow!("installed extension directory is unavailable"))?;
    if !admission.allows(name.as_bytes())? {
        anyhow::bail!("extension `{name}` is not an admitted installed contribution");
    }
    let directory = admission
        .open_child_directory(name.as_bytes())?
        .ok_or_else(|| anyhow::anyhow!("extension `{name}` is not installed"))?;
    let snapshot = std::sync::Arc::new(
        crate::contribution_loading::snapshot_contribution_directory(&directory)?,
    );
    if extension_state_disabled(snapshot.path()) {
        anyhow::bail!("extension `{name}` is disabled");
    }
    let profile = crate::settings::Profile::load(cwd);
    let env_enabled = crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS");
    let env_disabled = crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS");
    if !profile
        .extensions
        .permits(name, &env_enabled, &env_disabled)
    {
        anyhow::bail!("extension `{name}` is disabled by the active profile");
    }
    let manifest = crate::extensions::ExtensionManifest::from_extension_dir(snapshot.path())?;
    if manifest.extension.name != name {
        anyhow::bail!(
            "extension directory `{name}` declares manifest identity `{}`",
            manifest.extension.name
        );
    }
    let preflight = crate::extensions::dynamic_preflight(&manifest, snapshot.path())?;
    if inventory.active_source_digest(&id).as_deref() == Some(&preflight.source_digest) {
        return Ok(InstalledExtensionReplacement::Unchanged);
    }
    let candidate = inventory.discover(preflight)?;
    let permit = inventory.admit(
        &candidate,
        &crate::dynamic_admission::DynamicAdmissionPolicy::from_profile(&profile),
    )?;
    let resolved_secrets = resolve_extension_secrets(&manifest, secrets.as_ref()).await;
    let spawned = crate::extensions::spawn_from_admitted_snapshot(
        snapshot,
        &home.join("extensions").join(name),
        permit,
        inventory.clone(),
        &find_project_root(cwd),
        &resolved_secrets,
    )
    .await
    .inspect_err(|error| inventory.quarantine(&id, error.to_string()))?;
    if !spawned.widgets.is_empty()
        || spawned.vox_polling_handle.is_some()
        || spawned.voice_polling_handle.is_some()
        || spawned.voice_notification_rx.is_some()
    {
        let failures = crate::extensions::shutdown_supervisors(
            std::slice::from_ref(&spawned.supervisor),
            std::time::Duration::from_millis(500),
        )
        .await;
        inventory.quarantine(
            &id,
            "changed generation requires restart to replace widget or voice side channels",
        );
        anyhow::bail!(
            "extension `{name}` requires `/runtime restart` to replace widget or voice side channels{}",
            if failures.is_empty() {
                String::new()
            } else {
                format!("; candidate cleanup: {}", failures.join("; "))
            }
        );
    }
    inventory.ready(&id);
    Ok(InstalledExtensionReplacement::Changed(
        crate::contribution_lifecycle::StagedDynamicExtensionGeneration::new(
            spawned.feature,
            spawned.supervisor,
            inventory,
        ),
    ))
}

pub(crate) fn release_coupled_codescan_dir() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let executable = std::fs::canonicalize(executable).ok()?;
    let binary_dir = executable.parent()?;
    [
        binary_dir.join("share/omegon/extensions/omegon-codescan"),
        binary_dir.join("../share/omegon/extensions/omegon-codescan"),
    ]
    .into_iter()
    .find_map(|directory| {
        let directory = directory
            .is_dir()
            .then(|| std::fs::canonicalize(directory).ok())
            .flatten()?;
        let generation = directory.ancestors().nth(4)?;
        crate::installed_release::validate_product_component(generation)
            .is_ok()
            .then_some(directory)
    })
}

fn extension_state_disabled(path: &Path) -> bool {
    crate::extensions::ExtensionState::load(path)
        .is_ok_and(|state| !state.enabled || state.stability.auto_disabled)
}

fn activate_startup_persona(
    registry: &mut crate::plugins::registry::AugmentRegistry,
    cwd: &Path,
    persona_name: &str,
) {
    let target = persona_name.to_lowercase();
    crate::plugins::persona_loader::with_available(cwd, |personas, _| {
        if let Some(loaded) = personas
            .iter()
            .find(|p| p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target))
            .and_then(|available| available.persona())
            .cloned()
        {
            tracing::info!(persona = %loaded.name, "activating startup persona");
            registry.activate_persona(loaded);
        } else {
            tracing::warn!(persona = %persona_name, "startup persona not found");
        }
    });
}

fn memory_status_from_stats(
    stats: omegon_memory::backend::MemoryStats,
) -> crate::status::MemoryStatus {
    crate::status::MemoryStatus {
        total_facts: stats.total_facts,
        active_facts: stats.active_facts,
        project_facts: stats.active_facts,
        persona_facts: 0,
        working_facts: 0,
        episodes: stats.episodes,
        edges: stats.edges,
        active_persona_mind: None,
    }
}

fn activate_startup_tone(
    registry: &mut crate::plugins::registry::AugmentRegistry,
    cwd: &Path,
    tone_name: &str,
) {
    let target = tone_name.to_lowercase();
    crate::plugins::persona_loader::with_available(cwd, |_, tones| {
        if let Some(loaded) = tones
            .iter()
            .find(|t| t.name.to_lowercase() == target || t.id.to_lowercase().contains(&target))
            .and_then(|available| available.tone())
            .cloned()
        {
            tracing::info!(tone = %loaded.name, "activating startup tone");
            registry.activate_tone(loaded);
        } else {
            tracing::warn!(tone = %tone_name, "startup tone not found");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_managed_status_overrides_a_live_binding_in_initial_harness_status() {
        let mut harness = crate::status::HarnessStatus::default();
        apply_initial_memory_status(
            &mut harness,
            crate::status::MemoryStatus {
                total_facts: 9,
                ..Default::default()
            },
            true,
            Some("memory:startup_status_unavailable".into()),
        );
        assert!(!harness.memory_available);
        assert_eq!(
            harness.memory_warning.as_deref(),
            Some("memory:startup_status_unavailable")
        );
    }

    #[test]
    fn setup_emits_canonical_session_identity_before_returning_tools() {
        let production = include_str!("setup.rs")
            .split_once("#[cfg(test)]\nmod tests")
            .unwrap()
            .0;
        let memory_registration = production
            .find("bus.register(Box::new(memory_feature))")
            .expect("memory feature registration");
        let session_start = production
            .find("bus.emit(&omegon_traits::BusEvent::SessionStart")
            .expect("setup-owned SessionStart");
        let setup_return = production.rfind("Ok(Self {").expect("AgentSetup return");
        assert!(memory_registration < session_start);
        assert!(session_start < setup_return);
    }

    #[test]
    fn model_budget_route_binding_preserves_the_accepted_composition() {
        let mut bus = EventBus::new();
        let settings = crate::settings::shared("openai:gpt-5.6");
        let binding = register_model_budget(&mut bus, Some(&settings)).unwrap();
        bus.try_finalize().unwrap();
        let accepted_generation = bus.composition_generation_id().unwrap().clone();
        let controller = std::sync::Arc::new(crate::route::RouteController::new(
            crate::route::ProviderRoute::Serving {
                model: "openai:gpt-5.6".into(),
            },
            Box::new(crate::bridge::MockBridge { events: Vec::new() }),
            None,
        ));

        binding.bind(controller).unwrap();

        assert_eq!(
            bus.composition_generation_id(),
            Some(&accepted_generation),
            "late route binding must not stage or publish a replacement graph"
        );
    }

    #[test]
    fn production_memory_consumers_have_no_direct_durable_owner() {
        fn visit(directory: &Path, findings: &mut Vec<String>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, findings);
                    continue;
                }
                if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                    continue;
                }
                let relative = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap()
                    .to_string_lossy();
                if matches!(
                    relative.as_ref(),
                    "src/memory_service.rs" | "src/migrate.rs"
                ) {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                let production = source
                    .split_once("#[cfg(test)]\nmod tests")
                    .map_or(source.as_str(), |(production, _)| production);
                for forbidden in [
                    "MemoryBackend",
                    "SqliteBackend::open",
                    "vault_sync::import_",
                    "vault_sync::materialize_",
                    "vault_sync::reinforce_",
                    ".import_jsonl(",
                    ".export_jsonl(",
                    ".store_embedding(",
                    "SELECT COUNT(*) FROM facts",
                ] {
                    if production.contains(forbidden) {
                        findings.push(format!("{relative}: {forbidden}"));
                    }
                }
                if relative != "src/setup.rs" && production.contains("SqliteBackend") {
                    findings.push(format!("{relative}: SqliteBackend import, alias, or open"));
                }
                let owns_project_memory_path = [
                    "facts.db",
                    "global-memory.db",
                    "join(\"ai\").join(\"memory\")",
                    "join(\".omegon\").join(\"memory\")",
                ]
                .iter()
                .any(|marker| production.contains(marker));
                if owns_project_memory_path
                    && (production.contains("rusqlite::Connection")
                        || production.contains("Connection::open")
                        || production.contains("use rusqlite"))
                {
                    findings.push(format!(
                        "{relative}: rusqlite project-memory connection or alias"
                    ));
                }
                let lines = production.lines().collect::<Vec<_>>();
                for (line, _) in lines
                    .iter()
                    .enumerate()
                    .filter(|(_, line)| line.contains("tokio::spawn"))
                {
                    let window =
                        lines[line.saturating_sub(10)..(line + 30).min(lines.len())].join("\n");
                    if window.contains("MemoryRequestV1::ApplyMutation")
                        || window.contains("MemoryRequestV1::ApplyToolMutation")
                        || window.contains("VaultSessionEnd")
                    {
                        findings.push(format!("{relative}: detached memory persistence task"));
                    }
                }
            }
        }

        let mut findings = Vec::new();
        visit(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut findings,
        );
        assert!(
            findings.is_empty(),
            "direct memory persistence owners: {findings:?}"
        );

        let feature = include_str!("features/memory.rs")
            .split_once("#[cfg(test)]\nmod tests")
            .map_or(include_str!("features/memory.rs"), |(production, _)| {
                production
            });
        for forbidden in [
            "MemoryBackend",
            "SqliteBackend",
            "tokio::spawn",
            "fn backend(",
        ] {
            assert!(
                !feature.contains(forbidden),
                "memory feature retained forbidden owner or detached task: {forbidden}"
            );
        }
    }

    #[test]
    fn production_compaction_consumers_have_no_direct_planner_or_ambient_lookup() {
        fn visit(directory: &Path, findings: &mut Vec<String>) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, findings);
                    continue;
                }
                if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                    continue;
                }
                let relative = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .unwrap()
                    .to_string_lossy();
                if matches!(
                    relative.as_ref(),
                    "src/context_compaction_service.rs" | "src/conversation.rs"
                ) {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                let production = source
                    .split_once("#[cfg(test)]\nmod tests")
                    .map_or(source.as_str(), |(production, _)| production);
                for forbidden in [
                    ".build_compaction_payload(",
                    ".build_compaction_payload_keeping_recent(",
                    "managed_service::<ContextCompactionService>",
                ] {
                    if production.contains(forbidden) {
                        findings.push(format!("{relative}: {forbidden}"));
                    }
                }
            }
        }

        let mut findings = Vec::new();
        visit(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut findings,
        );
        assert!(
            findings.is_empty(),
            "direct context/compaction planning bypasses: {findings:?}"
        );

        for (source, marker) in [
            (
                include_str!("interactive_coordinator.rs"),
                "loop_config.compatibility.context_compaction = runtime_state.context_compaction.clone()",
            ),
            (
                include_str!("acp_worker.rs"),
                "context_compaction: agent_setup.context_compaction.clone()",
            ),
            (
                include_str!("sentry/executor.rs"),
                "loop_config.compatibility.context_compaction = agent.context_compaction.clone()",
            ),
        ] {
            assert!(
                source.contains(marker),
                "normal loop host lost boot-captured context/compaction binding: {marker}"
            );
        }
        let main = include_str!("main.rs");
        assert!(
            main.matches("compatibility.context_compaction").count() >= 3,
            "daemon, headless, and bounded main hosts must transfer the captured binding"
        );
    }

    #[tokio::test]
    async fn finalize_managed_error_settles_memory_worker_and_writer() {
        let directory = tempfile::tempdir().unwrap();
        let mut bus = EventBus::new();
        bus.set_project_root(directory.path().to_path_buf());
        bus.register(Box::new(crate::memory_service::MemoryDeclarationFeature));
        let candidate =
            crate::memory_service::start_candidate(crate::memory_service::MemoryWorkerConfig {
                project_memory_root: directory.path().to_path_buf(),
                project_db_path: directory.path().join("facts.db"),
                project_jsonl_path: directory.path().join("facts.jsonl"),
                global_db_path: None,
                vault: None,
                startup_sync_enabled: false,
            })
            .await
            .unwrap();
        bus.stage_managed_generation("memory", candidate).unwrap();
        bus.try_finalize_managed().await.unwrap();

        let error =
            finalize_managed_error::<()>(&mut bus, anyhow::anyhow!("representative failure"))
                .await
                .unwrap_err();
        assert_eq!(error.to_string(), "representative failure");
        let report = bus.shutdown_managed_services().await;
        assert!(report.all_resources_settled(), "{report:?}");
    }

    #[test]
    fn managed_readiness_stats_drive_initial_memory_status() {
        let status = memory_status_from_stats(omegon_memory::backend::MemoryStats {
            total_facts: 7,
            active_facts: 5,
            episodes: 3,
            edges: 2,
            ..Default::default()
        });
        assert_eq!(status.total_facts, 7);
        assert_eq!(status.active_facts, 5);
        assert_eq!(status.project_facts, 5);
        assert_eq!(status.episodes, 3);
        assert_eq!(status.edges, 2);

        let source = include_str!("setup.rs");
        let capture = source.find("memory_binding.capture(&bus)").unwrap();
        let readiness_status = source[capture..]
            .find("MemoryRequestV1::ManagedStatus")
            .map(|offset| capture + offset)
            .unwrap();
        let publish = source[capture..]
            .find("apply_initial_memory_status")
            .map(|offset| capture + offset)
            .unwrap();
        assert!(capture < readiness_status && readiness_status < publish);
    }

    struct ExtensionEnvGuard {
        omegon_home: Option<std::ffi::OsString>,
        user_home: Option<std::ffi::OsString>,
        child_component_denies: Option<std::ffi::OsString>,
    }

    impl ExtensionEnvGuard {
        fn isolate(home: &Path) -> Self {
            let omegon_home = std::env::var_os("OMEGON_HOME");
            let user_home = std::env::var_os("HOME");
            let child_component_denies =
                std::env::var_os(crate::component_policy::CHILD_COMPONENT_DENIES_ENV);
            // SAFETY: guarded extension tests hold the shared environment lock.
            unsafe {
                std::env::set_var("OMEGON_HOME", home);
                std::env::set_var("HOME", home);
                std::env::remove_var(crate::component_policy::CHILD_COMPONENT_DENIES_ENV);
            }
            Self {
                omegon_home,
                user_home,
                child_component_denies,
            }
        }
    }

    impl Drop for ExtensionEnvGuard {
        fn drop(&mut self) {
            // SAFETY: guarded extension tests hold the shared environment lock.
            unsafe {
                if let Some(previous) = self.omegon_home.take() {
                    std::env::set_var("OMEGON_HOME", previous);
                } else {
                    std::env::remove_var("OMEGON_HOME");
                }
                if let Some(previous) = self.user_home.take() {
                    std::env::set_var("HOME", previous);
                } else {
                    std::env::remove_var("HOME");
                }
                if let Some(previous) = self.child_component_denies.take() {
                    std::env::set_var(
                        crate::component_policy::CHILD_COMPONENT_DENIES_ENV,
                        previous,
                    );
                } else {
                    std::env::remove_var(crate::component_policy::CHILD_COMPONENT_DENIES_ENV);
                }
            }
        }
    }

    #[cfg(unix)]
    struct ReleaseCodescanFixture {
        directory: PathBuf,
        lock: PathBuf,
    }

    #[cfg(unix)]
    impl ReleaseCodescanFixture {
        fn install() -> Self {
            use std::os::unix::fs::PermissionsExt;

            let executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
            let directory = executable
                .parent()
                .unwrap()
                .join("share/omegon/extensions/omegon-codescan");
            assert!(
                !directory.exists(),
                "refusing to replace existing release-coupled fixture at {}",
                directory.display()
            );

            let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
            let source = repository.join("extensions/omegon-codescan");
            let source_binary = source.join("target/release/omegon-codescan");
            assert!(
                source_binary.is_file(),
                "build the real codescan extension before running this ignored test: {}",
                source_binary.display()
            );
            let binary = directory.join("target/release/omegon-codescan");
            std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
            std::fs::copy(
                source.join("manifest.toml"),
                directory.join("manifest.toml"),
            )
            .unwrap();
            std::fs::copy(source_binary, &binary).unwrap();
            let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&binary, permissions).unwrap();
            use sha2::{Digest, Sha256};
            let manifest = directory.join("manifest.toml");
            let lock = directory
                .ancestors()
                .nth(4)
                .unwrap()
                .join("share/omegon/components/core-codescan.lock.json");
            std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
            let evidence = omegon_maintenance_contracts::ProductComponentLockV1 {
                schema_version: 1,
                component_id: "core:codescan".into(),
                wire_manifest_id: "omegon-codescan".into(),
                manifest_path: "share/omegon/extensions/omegon-codescan/manifest.toml".into(),
                manifest_digest: omegon_maintenance_contracts::AuthorityKey::from_bytes(
                    Sha256::digest(std::fs::read(&manifest).unwrap()).into(),
                ),
                executable_path:
                    "share/omegon/extensions/omegon-codescan/target/release/omegon-codescan"
                        .into(),
                executable_digest: omegon_maintenance_contracts::AuthorityKey::from_bytes(
                    Sha256::digest(std::fs::read(&binary).unwrap()).into(),
                ),
                target: crate::installed_release::compiled_target().into(),
                protocol_minimum: 1,
                protocol_maximum: 1,
                protocol_version: 1,
                fallback: "typed_unavailable".into(),
                signing_identity: omegon_maintenance_contracts::SigningIdentityV1 {
                    issuer: "https://token.actions.githubusercontent.com".into(),
                    workflow_identity: "https://github.com/styrene-lab/omegon/.github/workflows/release.yml@refs/tags/vtest".into(),
                    verification: "required".into(),
                },
            };
            std::fs::write(
                &lock,
                omegon_maintenance_contracts::canonical_json(&evidence).unwrap(),
            )
            .unwrap();
            Self { directory, lock }
        }

        fn enable_conformance_barrier(&self, control: &Path) {
            use std::io::Write;

            let mut manifest = std::fs::OpenOptions::new()
                .append(true)
                .open(self.directory.join("manifest.toml"))
                .unwrap();
            writeln!(
                manifest,
                "\n[runtime.env]\nOMEGON_CODESCAN_CONFORMANCE_DIR = {:?}",
                control.display().to_string()
            )
            .unwrap();
            let mut evidence: omegon_maintenance_contracts::ProductComponentLockV1 =
                serde_json::from_slice(&std::fs::read(&self.lock).unwrap()).unwrap();
            use sha2::{Digest, Sha256};
            evidence.manifest_digest = omegon_maintenance_contracts::AuthorityKey::from_bytes(
                Sha256::digest(std::fs::read(self.directory.join("manifest.toml")).unwrap()).into(),
            );
            std::fs::write(
                &self.lock,
                omegon_maintenance_contracts::canonical_json(&evidence).unwrap(),
            )
            .unwrap();
        }

        fn remove(&mut self) {
            if self.directory.exists() {
                std::fs::remove_dir_all(&self.directory).unwrap();
            }
            if self.lock.exists() {
                std::fs::remove_file(&self.lock).unwrap();
            }
        }
    }

    #[cfg(unix)]
    impl Drop for ReleaseCodescanFixture {
        fn drop(&mut self) {
            self.remove();
        }
    }

    #[cfg(unix)]
    fn directory_snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn visit(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
            if !directory.is_dir() {
                return;
            }
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(root, &path, files);
                } else {
                    files.push((
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(path).unwrap(),
                    ));
                }
            }
        }

        let mut files = Vec::new();
        visit(root, root, &mut files);
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    #[cfg(unix)]
    async fn wait_for_json(path: &Path) -> serde_json::Value {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match std::fs::read(path) {
                    Ok(bytes) => return serde_json::from_slice(&bytes).unwrap(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    Err(error) => panic!("could not read {}: {error}", path.display()),
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {}", path.display()))
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "slow: requires the release-built omegon-codescan process"]
    async fn release_coupled_codescan_traverses_discovery_and_host_binding() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = ExtensionEnvGuard::isolate(home.path());
        unsafe {
            std::env::remove_var("OMEGON_CHILD_ENABLED_EXTENSIONS");
            std::env::remove_var("OMEGON_CHILD_DISABLED_EXTENSIONS");
        }
        std::fs::write(
            project.path().join("acceptance_fixture.rs"),
            "pub fn release_discovery_acceptance_needle() -> bool { true }",
        )
        .unwrap();
        let mut release = ReleaseCodescanFixture::install();
        let cancellation_control = home.path().join("codescan-conformance");
        std::fs::create_dir_all(&cancellation_control).unwrap();
        release.enable_conformance_barrier(&cancellation_control);

        let binding = crate::codescan_service::CodescanBinding::default();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::codescan_service::CodescanFeature::new(
            project.path().to_path_buf(),
            binding.clone(),
        )));
        let inventory = crate::contribution_lifecycle::DynamicContributionInventory::default();
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap());
        let mut discovered = discover_and_register_extensions(
            project.path(),
            project.path(),
            &mut bus,
            secrets,
            inventory.clone(),
        )
        .await
        .unwrap();

        let metadata = &discovered.extension_metadata[crate::codescan_service::CODESCAN_EXTENSION];
        assert_eq!(
            metadata["extension_info"]["name"], "omegon-codescan",
            "{metadata}"
        );
        assert_eq!(metadata["capabilities"]["codescan"], true);
        assert_eq!(metadata["sdk_compatibility"]["status"], "supported");
        let handle = discovered
            .extension_rpc_handles
            .get(crate::codescan_service::CODESCAN_EXTENSION)
            .cloned()
            .expect("release discovery must expose the codescan RPC handle");
        binding.capture(Some(handle)).unwrap();

        let mut owner = crate::contribution_lifecycle::DynamicContributionGenerationOwner::new(
            inventory.clone(),
        );
        for supervisor in discovered.extension_supervisors.drain(..) {
            owner.own_extension(supervisor);
        }
        owner.stage();
        bus.try_finalize_managed().await.unwrap();
        owner.publish();
        let evidence = inventory.evidence();
        assert_eq!(evidence.len(), 1, "{evidence:?}");
        assert_eq!(
            evidence[0].state,
            crate::contribution_lifecycle::DiscoveredContributionState::Published
        );

        let indexed = bus
            .execute_tool(
                "codebase_index",
                "codescan-acceptance-index",
                serde_json::json!({"invalidate": true}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(indexed.details["code_chunks"].as_u64().unwrap() > 0);
        let searched = bus
            .execute_tool(
                "codebase_search",
                "codescan-acceptance-search",
                serde_json::json!({
                    "query": "release_discovery_acceptance_needle",
                    "scope": "code",
                    "max_results": 5
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            searched.details["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["file"] == "acceptance_fixture.rs"),
            "{searched:?}"
        );
        assert_eq!(
            searched.details["service_provenance"]["extension"],
            "omegon-codescan"
        );
        assert_eq!(
            searched.details["service_provenance"]["source_digest"],
            evidence[0].candidate.preflight.source_digest
        );
        assert!(searched.details["service_provenance"]["pid"].is_u64());
        assert_eq!(
            bus.tool_provenance("codebase_search"),
            omegon_traits::ToolProvenance::BuiltIn
        );

        let database_root = project.path().join(".omegon");
        let before_cancel = directory_snapshot(&database_root);
        std::fs::write(cancellation_control.join("arm"), b"").unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let cancellation = binding.execute(
            omegon_codescan_contracts::CodescanOperationV1::Index(
                omegon_codescan_contracts::IndexRequestV1 { invalidate: true },
            ),
            cancel.clone(),
        );
        tokio::pin!(cancellation);
        let started_path = cancellation_control.join("started.json");
        let started = tokio::select! {
            result = &mut cancellation => panic!("codescan request ended before worker barrier: {result:?}"),
            started = wait_for_json(&started_path) => started,
        };
        cancel.cancel();
        let error = tokio::time::timeout(std::time::Duration::from_secs(1), &mut cancellation)
            .await
            .expect("host cancellation did not settle")
            .unwrap_err();
        assert_eq!(error.code(), "request:cancelled");
        let outcome_path = cancellation_control.join("outcome.json");
        let outcome = wait_for_json(&outcome_path).await;
        assert_eq!(outcome["request_id"], started["request_id"]);
        assert_eq!(outcome["code"], "cancelled");
        assert_eq!(directory_snapshot(&database_root), before_cancel);
        std::fs::remove_file(cancellation_control.join("arm")).unwrap();
        let status = discovered.extension_rpc_handles[crate::codescan_service::CODESCAN_EXTENSION]
            .rpc_call(
                omegon_codescan_contracts::CODESCAN_STATUS_METHOD,
                serde_json::json!({}),
            )
            .await
            .unwrap();
        assert_eq!(status["ready"], true);

        assert!(owner.shutdown().await.is_empty());
        let report = bus.shutdown_managed_services().await;
        assert!(report.all_resources_settled(), "{report:?}");
        drop(discovered);
        release.remove();

        let absent_binding = crate::codescan_service::CodescanBinding::default();
        let mut absent_bus = crate::bus::EventBus::new();
        absent_bus.register(Box::new(crate::codescan_service::CodescanFeature::new(
            project.path().to_path_buf(),
            absent_binding.clone(),
        )));
        let absent_inventory =
            crate::contribution_lifecycle::DynamicContributionInventory::default();
        let absent = discover_and_register_extensions(
            project.path(),
            project.path(),
            &mut absent_bus,
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap()),
            absent_inventory,
        )
        .await
        .unwrap();
        assert!(
            !absent
                .extension_rpc_handles
                .contains_key(crate::codescan_service::CODESCAN_EXTENSION)
        );
        absent_binding.capture(None).unwrap();
        absent_bus.try_finalize_managed().await.unwrap();
        let unavailable = absent_bus
            .execute_tool(
                "codebase_search",
                "codescan-acceptance-absent",
                serde_json::json!({"query": "anything"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(unavailable.details["available"], false);
        assert_eq!(unavailable.details["code"], "service:unavailable");
        assert!(absent_bus.has_registered_tool("codebase_index"));
        let report = absent_bus.shutdown_managed_services().await;
        assert!(report.all_resources_settled(), "{report:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "slow: requires the release-built omegon-codescan process"]
    async fn release_codescan_can_be_enabled_on_restart_without_reinstall() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = ExtensionEnvGuard::isolate(home.path());
        unsafe {
            std::env::remove_var("OMEGON_CHILD_ENABLED_EXTENSIONS");
            std::env::remove_var("OMEGON_CHILD_DISABLED_EXTENSIONS");
        }
        std::fs::create_dir_all(project.path().join(".omegon")).unwrap();
        std::fs::write(
            project.path().join(".omegon/profile.json"),
            r#"{"components":{"core:codescan":{"enabled":false}}}"#,
        )
        .unwrap();
        std::fs::write(
            project.path().join("restart_fixture.rs"),
            "pub fn codescan_restart_without_reinstall_needle() -> bool { true }",
        )
        .unwrap();
        let mut release = ReleaseCodescanFixture::install();
        let packaged_before = directory_snapshot(&release.directory);

        let mut denied = setup_kernel_composition(project.path()).await.unwrap();
        assert_eq!(denied.component_dependency_policy.omitted.len(), 1);
        assert!(denied.bus.has_registered_tool("codebase_search"));
        assert!(
            denied
                .bus
                .tool_definitions()
                .iter()
                .all(|definition| definition.name != "codebase_search")
        );
        let disabled = denied
            .bus
            .execute_tool(
                "codebase_search",
                "restart-disabled",
                serde_json::json!({"query": "anything"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(disabled.details["code"], "service:disabled");
        assert!(denied.dynamic_contributions.shutdown().await.is_empty());
        let report = denied.bus.shutdown_managed_services().await;
        assert!(report.all_resources_settled(), "{report:?}");
        assert_eq!(directory_snapshot(&release.directory), packaged_before);

        std::fs::write(
            project.path().join(".omegon/profile.json"),
            r#"{"components":{"core:codescan":{"enabled":true}}}"#,
        )
        .unwrap();
        let mut enabled = setup_kernel_composition(project.path()).await.unwrap();
        assert_eq!(directory_snapshot(&release.directory), packaged_before);
        let indexed = enabled
            .bus
            .execute_tool(
                "codebase_index",
                "restart-index",
                serde_json::json!({"invalidate": true}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(indexed.details["code_chunks"].as_u64().unwrap() > 0);
        let searched = enabled
            .bus
            .execute_tool(
                "codebase_search",
                "restart-search",
                serde_json::json!({
                    "query": "codescan_restart_without_reinstall_needle",
                    "scope": "code",
                    "max_results": 5
                }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            searched.details["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["file"] == "restart_fixture.rs")
        );
        assert!(enabled.dynamic_contributions.shutdown().await.is_empty());
        let report = enabled.bus.shutdown_managed_services().await;
        assert!(report.all_resources_settled(), "{report:?}");
        release.remove();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn denied_release_codescan_does_not_probe_spawn_or_mutate_workspace() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = ExtensionEnvGuard::isolate(home.path());
        unsafe {
            std::env::remove_var("OMEGON_CHILD_ENABLED_EXTENSIONS");
            std::env::remove_var("OMEGON_CHILD_DISABLED_EXTENSIONS");
            std::env::remove_var(crate::component_policy::CHILD_COMPONENT_DENIES_ENV);
        }
        std::fs::create_dir_all(project.path().join(".omegon")).unwrap();
        std::fs::write(
            project.path().join(".omegon/profile.json"),
            r#"{"components":{"core:codescan":{"enabled":false}}}"#,
        )
        .unwrap();
        std::fs::write(project.path().join("fixture.txt"), b"unchanged").unwrap();

        let executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let release_dir = executable
            .parent()
            .unwrap()
            .join("share/omegon/extensions/omegon-codescan");
        assert!(!release_dir.exists());
        let launch_marker = home.path().join("codescan-process-started");
        let binary = release_dir.join("target/release/omegon-codescan");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(
            release_dir.join("manifest.toml"),
            include_bytes!("../../../../extensions/omegon-codescan/manifest.toml"),
        )
        .unwrap();
        std::fs::write(
            &binary,
            format!("#!/bin/sh\ntouch {:?}\nexit 1\n", launch_marker),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&binary, permissions).unwrap();
        let mut release = ReleaseCodescanFixture {
            lock: release_dir.join("nonexistent-component-lock.json"),
            directory: release_dir,
        };

        let generic = home.path().join("extensions/unrelated-extension");
        std::fs::create_dir_all(&generic).unwrap();
        std::fs::write(generic.join("manifest.toml"), b"not valid toml").unwrap();
        let before = directory_snapshot(project.path());
        let policy =
            crate::component_policy::resolve_product_boot_policy(project.path(), home.path())
                .unwrap();
        let binding = crate::codescan_service::CodescanBinding::from_component_decision(
            policy.component("core:codescan"),
        );
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::codescan_service::CodescanFeature::new(
            project.path().to_path_buf(),
            binding.clone(),
        )));
        bus.set_policy_denied_tools(["codebase_search", "codebase_index"]);
        let inventory = crate::contribution_lifecycle::DynamicContributionInventory::default();
        let discovered = discover_and_register_extensions_with_policy(
            project.path(),
            project.path(),
            &mut bus,
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap()),
            inventory,
            &policy,
        )
        .await
        .unwrap();

        assert!(discovered.extension_supervisors.is_empty());
        assert!(discovered.extension_rpc_handles.is_empty());
        assert_eq!(discovered.discovery_attempts, vec!["unrelated-extension"]);
        assert_eq!(directory_snapshot(project.path()), before);
        assert!(
            !launch_marker.exists(),
            "denied codescan process was spawned"
        );
        assert!(generic.join("manifest.toml").is_file());
        binding.capture(None).unwrap();
        bus.try_finalize_managed().await.unwrap();
        assert!(bus.tool_definitions().iter().all(|definition| {
            !matches!(
                definition.name.as_str(),
                "codebase_search" | "codebase_index"
            )
        }));
        assert!(bus.has_registered_tool("codebase_search"));
        assert!(bus.has_registered_tool("codebase_index"));
        for (index, surface) in [
            omegon_traits::RuntimeSurface::Cli,
            omegon_traits::RuntimeSurface::Acp,
        ]
        .into_iter()
        .enumerate()
        {
            let result = bus
                .invoke_tool(
                    "codebase_search",
                    &format!("disabled-codescan-direct-{index}"),
                    serde_json::json!({"query": "anything"}),
                    tokio_util::sync::CancellationToken::new(),
                    crate::invocation_service::InvocationScope {
                        principal: "operator".into(),
                        principal_class: omegon_traits::RuntimePrincipalClass::Operator,
                        surface,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(result.details["code"], "service:disabled");
            assert_eq!(result.details["component_id"], "core:codescan");
            assert_eq!(
                result.details["determining_policy_source"]["kind"],
                "selected-profile"
            );
        }
        let report = bus.shutdown_managed_services().await;
        assert!(report.all_resources_settled(), "{report:?}");
        release.remove();
    }

    fn with_auth_env_lock<T>(f: impl FnOnce() -> T + std::panic::UnwindSafe) -> T {
        let _guard = crate::auth::TEST_AUTH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let result = std::panic::catch_unwind(f);
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    #[tokio::test]
    async fn extension_secret_resolution_executes_deferred_recipe_at_spawn_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = omegon_secrets::SecretsManager::new(dir.path()).unwrap();
        secrets
            .set_recipe("OMADA_TEST_SECRET", "cmd:printf omada-secret")
            .unwrap();
        assert_eq!(secrets.resolve_cached("OMADA_TEST_SECRET"), None);

        let manifest_toml = r#"
[extension]
name = "recipe-backed-extension"
version = "0.1.0"
description = "fixture"

[runtime]
type = "native"
binary = "fixture"

[secrets]
required = ["OMADA_TEST_SECRET"]
"#;
        std::fs::write(dir.path().join("manifest.toml"), manifest_toml).unwrap();
        let manifest =
            crate::extensions::ExtensionManifest::from_extension_dir(dir.path()).unwrap();

        let resolved = resolve_extension_secrets(&manifest, &secrets).await;

        assert_eq!(
            resolved,
            vec![("OMADA_TEST_SECRET".to_string(), "omada-secret".to_string())]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn contribution_health_extension_identity_mismatch_and_recovery() {
        use crate::contribution_health::{ContributionKind::Extensions, LoadingState};
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = ExtensionEnvGuard::isolate(home.path());
        std::fs::create_dir_all(home.path().join("extensions")).unwrap();
        std::fs::create_dir_all(project.path().join(".omegon")).unwrap();
        std::fs::write(
            project.path().join(".omegon/profile.json"),
            r#"{"components":{"core:codescan":{"enabled":false}}}"#,
        )
        .unwrap();
        drop(
            crate::contribution_loading::GuardedContributionDirectory::open(
                home.path(),
                &[b"extensions"],
                home.path(),
                omegon_maintenance_contracts::ContributionKind::Extension,
                "user",
            )
            .unwrap(),
        );
        let state_path = home.path().join("maintain/v1/state.json");
        let original = std::fs::read(&state_path).unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(&original).unwrap();
        state["home"]["device"] = serde_json::json!(state["home"]["device"].as_u64().unwrap() + 1);
        std::fs::write(
            &state_path,
            omegon_maintenance_contracts::canonical_json(&state).unwrap(),
        )
        .unwrap();
        let continuity = home.path().join("maintain/v1/home-continuity.json");
        if continuity.exists() {
            std::fs::remove_file(continuity).unwrap();
        }
        let mut bus = crate::bus::EventBus::new();
        let inventory =
            crate::contribution_lifecycle::DynamicContributionInventory::with_loading_health(
                bus.contribution_health(),
            );
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap());
        let failed = discover_and_register_extensions(
            project.path(),
            project.path(),
            &mut bus,
            secrets.clone(),
            inventory.clone(),
        )
        .await;
        assert!(failed.is_err());
        let blocked = inventory.loading_health.snapshot();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].kind, Extensions);
        assert!(
            matches!(&blocked[0].outcome, LoadingState::Blocked { code, .. } if code == "home_identity_mismatch")
        );
        // Restore only the deliberately modified fixture; production guards are unchanged.
        std::fs::write(&state_path, original).unwrap();
        let recovered = discover_and_register_extensions(
            project.path(),
            project.path(),
            &mut bus,
            secrets.clone(),
            inventory.clone(),
        )
        .await
        .unwrap();
        drop(recovered);
        assert!(matches!(
            inventory.loading_health.snapshot()[0].outcome,
            LoadingState::Loaded { count: 0 }
        ));
        let malformed = home.path().join("extensions/malformed");
        std::fs::create_dir_all(&malformed).unwrap();
        std::fs::write(malformed.join("manifest.toml"), "not valid toml").unwrap();
        drop(
            discover_and_register_extensions(
                project.path(),
                project.path(),
                &mut bus,
                secrets.clone(),
                inventory.clone(),
            )
            .await
            .unwrap(),
        );
        assert!(
            inventory
                .loading_health
                .snapshot()
                .iter()
                .any(|scope| scope.scope == "user/malformed" && scope.is_blocked())
        );
        std::fs::remove_dir_all(home.path().join("extensions")).unwrap();
        drop(
            discover_and_register_extensions(
                project.path(),
                project.path(),
                &mut bus,
                secrets,
                inventory.clone(),
            )
            .await
            .unwrap(),
        );
        let health = inventory.loading_health.snapshot();
        assert_eq!(health.len(), 1);
        assert!(matches!(health[0].outcome, LoadingState::Absent));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn guarded_extension_discovery_excludes_denied_entry_and_holds_scope_lock() {
        use omegon_maintenance_contracts::{LockMode, MaintenanceStateV1, ProtocolLock};
        use std::os::unix::fs::symlink;

        let _lock = crate::test_support::env::lock_async().await;
        let home_path = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = ExtensionEnvGuard::isolate(home_path.path());
        let denied = home_path.path().join("extensions/denied");
        std::fs::create_dir_all(&denied).unwrap();
        std::fs::write(denied.join("manifest.toml"), "not valid toml").unwrap();
        let linked_source = tempfile::tempdir().unwrap();
        symlink(
            linked_source.path(),
            home_path.path().join("extensions/linked-local"),
        )
        .unwrap();
        deny_extension(home_path.path(), b"denied");
        let authority = extension_scope_key(&home_path.path().join("extensions"));
        let home = omegon_maintenance_contracts::open_secure_root(home_path.path()).unwrap();
        let state = MaintenanceStateV1::bootstrap(
            &home,
            omegon_maintenance_contracts::path_identity(&home).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .unwrap();
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home_path.path()).unwrap());
        let mut bus = crate::bus::EventBus::new();

        let discovered = discover_and_register_extensions(
            project.path(),
            project.path(),
            &mut bus,
            secrets,
            crate::contribution_lifecycle::DynamicContributionInventory::default(),
        )
        .await
        .unwrap();
        assert!(discovered.extension_metadata.is_empty());
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
        drop(discovered);
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

    #[cfg(unix)]
    #[tokio::test]
    async fn malformed_extension_deny_state_fails_scope_closed() {
        use std::io::Write;

        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let _env = ExtensionEnvGuard::isolate(home.path());
        std::fs::create_dir_all(home.path().join("extensions/example")).unwrap();
        std::fs::write(
            home.path().join("extensions/example/manifest.toml"),
            "not valid toml",
        )
        .unwrap();
        let authority = initialize_extension_scope(home.path());
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
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap());
        let mut bus = crate::bus::EventBus::new();

        assert!(
            discover_and_register_extensions(
                project.path(),
                project.path(),
                &mut bus,
                secrets,
                crate::contribution_lifecycle::DynamicContributionInventory::default(),
            )
            .await
            .is_err()
        );
    }

    #[cfg(unix)]
    fn initialize_extension_scope(home: &Path) -> omegon_maintenance_contracts::AuthorityKey {
        crate::contribution_loading::GuardedContributionDirectory::open(
            home,
            &[b"extensions"],
            home,
            omegon_maintenance_contracts::ContributionKind::Extension,
            "user",
        )
        .unwrap()
        .unwrap()
        .scope_key()
    }

    #[cfg(unix)]
    fn extension_scope_key(directory: &Path) -> omegon_maintenance_contracts::AuthorityKey {
        let directory = std::fs::File::open(directory).unwrap();
        let parent = omegon_maintenance_contracts::path_identity(&directory).unwrap();
        omegon_maintenance_contracts::scope_key(
            omegon_maintenance_contracts::ContributionKind::Extension.as_str(),
            "user",
            parent.key,
        )
    }

    #[cfg(unix)]
    fn deny_extension(home_path: &Path, raw_name: &[u8]) {
        use omegon_maintenance_contracts::{
            AuthorityKey, ContributionKind, DenyRecordV1, DenyState, DenyStateV1, SCHEMA_VERSION,
            derive_key, entry_key, open_secure_dir_at, replace_record_at,
        };
        use sha2::{Digest, Sha256};

        let authority = initialize_extension_scope(home_path);
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
        let kind = ContributionKind::Extension;
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
            created_at: "2026-08-19T00:00:00Z".into(),
        };
        let deny = DenyStateV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "deny_state".into(),
            record_id: derive_key("deny-state", &[authority.as_bytes(), &1_u64.to_be_bytes()]),
            scope_key: authority,
            generation: 1,
            entries: [(entry.to_hex(), record)].into(),
        };
        replace_record_at(&deny_directory, b"state.json", &deny, "deny-extension-test").unwrap();
    }

    #[test]
    fn explicit_project_marker_wins_over_parent_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let child = dir.path().join("child-workspace");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("AGENTS.md"), "instructions").unwrap();

        assert_eq!(find_project_root(&child), child.canonicalize().unwrap());
    }

    #[test]
    fn git_repo_still_wins_for_unmarked_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let child = dir.path().join("src/bin");
        std::fs::create_dir_all(&child).unwrap();

        assert_eq!(
            find_project_root(&child),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn git_repo_wins_over_nested_build_manifest_markers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let member = dir.path().join("core/crates/omegon");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(member.join("Cargo.toml"), "[package]\nname = \"omegon\"\n").unwrap();

        assert_eq!(
            find_project_root(&member),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn git_repo_wins_over_nested_omegon_state_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let member = dir.path().join("core");
        std::fs::create_dir_all(member.join(".omegon")).unwrap();
        std::fs::write(member.join(".omegon/profile.json"), "{}").unwrap();

        assert_eq!(
            find_project_root(&member),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn selected_provider_auth_hydration_skips_expired_oauth_and_unselected_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        let expired = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 1_000;
        std::fs::write(
            &auth_path,
            serde_json::json!({
                "openai-codex": {
                    "type": "oauth",
                    "access": "expired-codex-token",
                    "refresh": "refresh-token",
                    "expires": expired
                },
                "brave": {
                    "type": "api-key",
                    "access": "brave-token",
                    "refresh": "",
                    "expires": u64::MAX
                }
            })
            .to_string(),
        )
        .unwrap();

        with_auth_env_lock(|| {
            let original = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
            unsafe { std::env::set_var("OMEGON_AUTH_JSON_PATH", &auth_path) };
            let secrets = omegon_secrets::SecretsManager::new(dir.path()).expect("secrets manager");
            let mut session_secret_env = Vec::new();
            hydrate_selected_provider_auth_env_from_auth_json(
                "openai-codex",
                &mut session_secret_env,
                &secrets,
            );
            unsafe {
                match original {
                    Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                    None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
                }
            }

            assert!(
                session_secret_env.is_empty(),
                "expired selected OAuth and unrelated credentials must not be hydrated"
            );
        });
    }

    #[test]
    fn selected_provider_auth_hydration_reads_only_selected_internal_credential() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        std::fs::write(
            &auth_path,
            serde_json::json!({
                "brave": {
                    "type": "api-key",
                    "access": "brave-token",
                    "refresh": "",
                    "expires": u64::MAX
                },
                "tavily": {
                    "type": "api-key",
                    "access": "tavily-token",
                    "refresh": "",
                    "expires": u64::MAX
                }
            })
            .to_string(),
        )
        .unwrap();

        with_auth_env_lock(|| {
            let original = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
            unsafe { std::env::set_var("OMEGON_AUTH_JSON_PATH", &auth_path) };
            let secrets = omegon_secrets::SecretsManager::new(dir.path()).expect("secrets manager");
            let mut session_secret_env = Vec::new();
            hydrate_selected_provider_auth_env_from_auth_json(
                "brave",
                &mut session_secret_env,
                &secrets,
            );
            unsafe {
                match original {
                    Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                    None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
                }
            }

            assert_eq!(
                session_secret_env,
                vec![("BRAVE_API_KEY".into(), "brave-token".into())]
            );
        });
    }

    #[test]
    fn git_ceiling_is_parent_of_selected_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child-workspace");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("AGENTS.md"), "instructions").unwrap();

        assert_eq!(
            git_ceiling_directory(&child),
            child
                .canonicalize()
                .unwrap()
                .parent()
                .map(Path::to_path_buf)
        );
    }

    #[test]
    fn git_ceiling_preserves_parent_repo_for_unmarked_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let child = dir.path().join("src/bin");
        std::fs::create_dir_all(&child).unwrap();

        assert_eq!(
            git_ceiling_directory(&child),
            dir.path()
                .canonicalize()
                .unwrap()
                .parent()
                .map(Path::to_path_buf)
        );
    }
}

#[cfg(test)]
mod init_gating_tests {
    use super::*;
    use omegon_memory::MemoryBackend as _;

    #[test]
    fn startup_migrates_supported_legacy_memory_before_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.db");
        let backend = omegon_memory::SqliteBackend::open(&path).unwrap();
        drop(backend);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("DELETE FROM schema_version", []).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (6, datetime('now'))",
            [],
        )
        .unwrap();
        drop(conn);

        let result = ensure_project_memory_store_ready(&path).unwrap().unwrap();
        assert_eq!(result.source_version, 6);
        assert_eq!(
            result.target_version,
            omegon_memory::sqlite::MEMORY_SCHEMA_VERSION
        );
        assert!(result.backup.exists());
        assert_eq!(
            omegon_memory::SqliteBackend::status(&path)
                .unwrap()
                .schema_version,
            omegon_memory::sqlite::MEMORY_SCHEMA_VERSION
        );
        assert!(ensure_project_memory_store_ready(&path).unwrap().is_none());
        drop(omegon_memory::SqliteBackend::open(&path).unwrap());
    }

    #[test]
    fn startup_reconciles_post_migration_default_records_to_primensus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.db");
        let backend = omegon_memory::SqliteBackend::open(&path).unwrap();
        drop(backend);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO minds (name, description, created_at) VALUES ('default', 'Stale post-v7 caller', datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO episodes (id, mind, title, narrative, date, created_at) VALUES ('stray-session', 'default', 'session', 'post-migration session', date('now'), datetime('now'))",
            [],
        )
        .unwrap();
        drop(conn);

        assert!(ensure_project_memory_store_ready(&path).unwrap().is_none());
        let conn = rusqlite::Connection::open(&path).unwrap();
        let mind: String = conn
            .query_row(
                "SELECT mind FROM episodes WHERE id = 'stray-session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mind, omegon_memory::sqlite::PRIMENSUS_MIND);
    }

    #[test]
    fn startup_rejects_unsupported_memory_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.db");
        let backend = omegon_memory::SqliteBackend::open(&path).unwrap();
        drop(backend);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("DELETE FROM schema_version", []).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (4, datetime('now'))",
            [],
        )
        .unwrap();
        drop(conn);

        let error = ensure_project_memory_store_ready(&path).unwrap_err();
        assert!(error.to_string().contains("unsupported memory schema v4"));
    }

    #[test]
    fn project_memory_dir_absent_without_init_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        assert!(project_memory_dir_if_initialized(dir.path()).is_none());
        assert!(!dir.path().join("ai").exists());
        assert!(!dir.path().join(".omegon").exists());
    }

    #[tokio::test]
    async fn legacy_memory_root_reopens_the_same_persisted_store() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_root = dir.path().join(".omegon").join("memory");
        std::fs::create_dir_all(&legacy_root).unwrap();
        assert_eq!(
            project_memory_dir_if_initialized(dir.path()),
            Some(legacy_root.clone())
        );

        let db_path = legacy_root.join("facts.db");
        let backend = omegon_memory::SqliteBackend::open(&db_path).unwrap();
        let stored = backend
            .store_fact(omegon_memory::StoreFact {
                mind: omegon_memory::sqlite::PRIMENSUS_MIND.into(),
                content: "Legacy-root reopen fixture".into(),
                section: omegon_memory::Section::Architecture,
                decay_profile: omegon_memory::DecayProfileName::Standard,
                source: Some("test".into()),
            })
            .await
            .unwrap();
        drop(backend);

        let reopened = omegon_memory::SqliteBackend::open(&db_path).unwrap();
        assert_eq!(
            reopened
                .get_fact(&stored.fact.id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "Legacy-root reopen fixture"
        );
    }

    #[test]
    fn project_memory_dir_prefers_existing_ai_memory() {
        let dir = tempfile::tempdir().unwrap();
        let ai_memory = dir.path().join("ai/memory");
        std::fs::create_dir_all(&ai_memory).unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon/memory")).unwrap();
        assert_eq!(
            project_memory_dir_if_initialized(dir.path()),
            Some(ai_memory)
        );
    }

    #[test]
    fn project_memory_dir_uses_legacy_omegon_memory_when_ai_absent() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_memory = dir.path().join(".omegon/memory");
        std::fs::create_dir_all(&legacy_memory).unwrap();
        assert_eq!(
            project_memory_dir_if_initialized(dir.path()),
            Some(legacy_memory)
        );
    }
}

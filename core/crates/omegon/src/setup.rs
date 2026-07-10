//! Agent setup — shared initialization for headless and interactive modes.
//!
//! Builds the EventBus with all features registered, plus the ContextManager
//! and ConversationState needed for the agent loop.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use omegon_memory::EmbeddingService as _; // bring trait methods into scope
use omegon_memory::MemoryBackend as _;

use crate::bus::EventBus;
use crate::context::ContextManager;
use crate::conversation::ConversationState;
use crate::features;
use crate::lifecycle;
use crate::prompt;
use crate::session;
use crate::tools;

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

/// Everything needed to run an agent loop.
pub struct AgentSetup {
    /// The event bus — owns all features. The loop dispatches tools and
    /// emits events through the bus.
    pub bus: EventBus,
    /// Stable session id for the current live conversation. Fresh sessions
    /// get a generated id at startup; resumed sessions reuse their saved id.
    pub session_id: String,
    /// Instance identifier for runtime state isolation (`tui-{pid}`, `acp-{pid}`, etc.).
    pub instance_id: String,
    /// Skill activation/resolution events produced while loading startup augments.
    pub startup_skill_activation_events: Vec<omegon_traits::SkillActivationEvent>,
    /// Shared context metrics — updated each turn, read by ContextProvider
    pub context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    /// Shared command channel — set by main after TUI init
    pub command_tx: crate::features::context::SharedCommandTx,
    pub context_manager: ContextManager,
    pub conversation: ConversationState,
    pub cwd: PathBuf,
    /// Single shared owner for the active inference inventory generation.
    pub inference_runtime: crate::inference_runtime::InferenceRuntimeState,
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
    pub dashboard_handles: crate::tui::dashboard::DashboardHandles,
    /// Initial harness status assembled at startup.
    /// The agent loop broadcasts this as AgentEvent::HarnessStatusChanged
    /// when the events channel is created.
    pub initial_harness_status: crate::status::HarnessStatus,
    /// Present when a prior session was loaded; None for fresh starts.
    pub resume_info: Option<ResumeInfo>,
    /// Startup-local workspace ownership metadata.
    pub workspace_state: WorkspaceStartupState,
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
    pub skipped_by_policy: usize,
    pub disabled_extensions: usize,
    pub invalid_manifests: Vec<String>,
}

/// Build a runtime substrate refresh candidate inventory without mutating live runtime state.
///
/// This intentionally does not spawn extension subprocesses or register live
/// features. It verifies the filesystem/profile side of extension discovery so
/// `/runtime restart` can report whether a candidate refresh is plausible
/// before the later promotion implementation exists.
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
pub(crate) struct LifecycleSnapshot {
    pub focused_node: Option<crate::tui::dashboard::FocusedNodeSummary>,
    pub active_changes: Vec<crate::tui::dashboard::ChangeSummary>,
}

impl LifecycleSnapshot {
    fn from_lifecycle_feature(lf: &features::lifecycle::LifecycleFeature) -> Self {
        let focused_node = {
            let lp = lf.provider();
            lp.focused_node_id().and_then(|id| {
                lp.get_node(id).map(|n| {
                    let sections = lifecycle::design::read_node_sections(n);
                    let assumptions = n.assumption_count();
                    let decisions_count = sections
                        .as_ref()
                        .map(|s| s.decisions.iter().filter(|d| d.status == "decided").count())
                        .unwrap_or(0);
                    let readiness = sections
                        .as_ref()
                        .map(|s| s.readiness_score())
                        .unwrap_or(0.0);
                    crate::tui::dashboard::FocusedNodeSummary {
                        id: n.id.clone(),
                        title: n.title.clone(),
                        status: n.status,
                        open_questions: n.open_questions.len() - assumptions,
                        assumptions,
                        decisions: decisions_count,
                        readiness,
                        openspec_change: n.openspec_change.clone(),
                    }
                })
            })
        };

        let active_changes: Vec<_> = lf
            .read_handle()
            .openspec_snapshot(Default::default())
            .map(|snapshot| {
                snapshot
                    .changes
                    .into_iter()
                    .map(|c| crate::tui::dashboard::ChangeSummary {
                        name: c.name,
                        stage: c.lifecycle_state,
                        done_tasks: c.done_tasks,
                        total_tasks: c.total_tasks,
                    })
                    .collect()
            })
            .unwrap_or_default();

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
        let instance_id = crate::paths::instance_id("agent");
        let cwd = std::fs::canonicalize(cwd)?;
        // Canonical project root — extensions read this instead of
        // embedder-specific env vars (FLYNT_VAULT, CODEX_VAULT).
        unsafe { std::env::set_var("OMEGON_PROJECT_ROOT", &cwd) };
        let is_child = std::env::var("OMEGON_CHILD").is_ok();

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
        let mut preflight = std::collections::BTreeSet::<String>::new();
        if let Some(settings) = settings.as_ref()
            && let Ok(guard) = settings.lock()
        {
            let provider = crate::providers::infer_provider_id(&guard.model);
            // Add only the FIRST env var per provider (highest priority auth method),
            // but only when the shared auth store cannot already satisfy the active
            // route. This avoids an extra macOS Keychain prompt on every ad-hoc
            // rebuilt binary when auth.json already has valid OAuth for the model
            // (for example, gpt-* routes backed by openai-codex OAuth).
            if !crate::auth::provider_connected_for_model(&guard.model)
                && let Some(env_var) = crate::auth::provider_env_vars(&provider).first()
            {
                preflight.insert((*env_var).to_string());
            }
        }
        // Extension-declared required secrets — read manifests early so keyring-backed
        // secrets (e.g. GITHUB_TOKEN) are resolved before extension subprocesses spawn.
        for name in collect_extension_secret_requirements(&cwd) {
            preflight.insert(name);
        }
        // Plugin MCP env templates — scan {VAR_NAME} references so vault-backed
        // secrets referenced in plugin.toml / mcp.toml are warmed before plugins connect.
        for name in collect_plugin_secret_requirements(&cwd) {
            preflight.insert(name);
        }
        // Web search API keys are resolved LAZILY — only when the web_search
        // tool actually fires. Eagerly preflighting them causes 3 macOS Keychain
        // prompts on every launch for ad-hoc signed dev builds (signature changes
        // on each rebuild, invalidating "Always Allow" ACLs).
        // NOTE: OMEGON_WEB_AUTH_SECRET is NOT preflighted here.
        // Web browsing auth is only needed on-demand during web search.
        // Resolving it lazily avoids an extra keychain prompt at startup.
        tracing::info!(
            requested = preflight.len(),
            names = ?preflight,
            child = is_child,
            "startup secret preflight plan"
        );

        // Initialize vault client BEFORE preflight so vault: recipes can resolve.
        // Fail-open: vault unavailability is warned, not fatal — keyring/env still work.
        if let Err(e) = secrets.init_vault(&secrets_dir).await {
            tracing::warn!(error = %e, "vault init failed — vault: recipes will return None");
        }

        // Async preflight resolves ALL recipe types including vault:.
        // Replaces the sync preflight_session_cache() which silently skips vault recipes.
        secrets.preflight_session_cache_async(preflight).await;
        crate::auth::import_discovered_provider_credentials();
        let mut session_secret_env = secrets.session_env();
        let pre_hydrated_env_len = session_secret_env.len();
        hydrate_provider_auth_env_from_auth_json(&mut session_secret_env, &secrets);
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

        let mut bus = EventBus::new();

        let project_root = find_project_root(&cwd);

        // ─── Repo model (git state tracking) ────────────────────────────
        let repo_model = if project_root.join(".git").exists() || project_root.join(".jj").exists()
        {
            match omegon_git::RepoModel::discover(&project_root) {
                Ok(Some(model)) => {
                    tracing::info!(
                        repo = %model.repo_path().display(),
                        branch = model.branch().as_deref().unwrap_or("(detached)"),
                        submodules = model.submodules().len(),
                        "RepoModel active"
                    );
                    Some(model)
                }
                Ok(None) => {
                    tracing::debug!("not inside a git repo — RepoModel disabled");
                    None
                }
                Err(e) => {
                    tracing::warn!("git repo discovery failed: {e} — RepoModel disabled");
                    None
                }
            }
        } else {
            tracing::debug!(
                project = %project_root.display(),
                "selected project root is not a VCS root — RepoModel disabled"
            );
            None
        };

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

        let _codex_integration = crate::codex_config::load(&project_root);
        let codex_integration = crate::codex_config::load(&project_root);
        let codex_vault_path = codex_integration
            .as_ref()
            .map(|c| crate::codex_config::resolve_vault_path(&project_root, c));

        // ─── Memory ─────────────────────────────────────────────────────
        let mind = "default".to_string();
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

        let mut context_memory_backend: Option<std::sync::Arc<dyn omegon_memory::MemoryBackend>> =
            None;
        let mut context_memory_mind: Option<String> = None;
        let mut context_embed_service: Option<std::sync::Arc<dyn omegon_memory::EmbeddingService>> =
            None;

        if let Some(db_path) = db_path.as_ref() {
            match omegon_memory::SqliteBackend::open(db_path) {
                Ok(backend) => {
                    tracing::info!(mind = %mind, db = %db_path.display(), child = is_child, "memory backend loaded");

                    if let Ok(stats) = backend.stats(&mind).await {
                        initial_memory_status = crate::status::MemoryStatus {
                            total_facts: stats.total_facts,
                            active_facts: stats.active_facts,
                            project_facts: stats.active_facts,
                            persona_facts: 0,
                            working_facts: 0,
                            episodes: stats.episodes,
                            edges: stats.edges,
                            active_persona_mind: None,
                        };
                        tracing::info!(
                            facts = initial_memory_status.active_facts,
                            episodes = initial_memory_status.episodes,
                            edges = initial_memory_status.edges,
                            "memory snapshot for TUI"
                        );
                    }

                    // Import JSONL if database is empty (but not in child processes)
                    if !is_child {
                        let stats = backend.stats(&mind).await.ok();
                        if stats.as_ref().is_none_or(|s| s.active_facts == 0)
                            && jsonl_path.as_ref().is_some_and(|path| path.exists())
                            && let Some(jsonl_path) = jsonl_path.as_ref()
                            && let Ok(jsonl) = std::fs::read_to_string(jsonl_path)
                        {
                            match backend.import_jsonl(&jsonl).await {
                                Ok(import) => {
                                    tracing::info!(
                                        imported = import.imported,
                                        "imported facts.jsonl"
                                    )
                                }
                                Err(e) => tracing::warn!("JSONL import failed: {e}"),
                            }
                        }
                    }

                    // Register MemoryFeature with Arc<dyn MemoryBackend>
                    let memory_backend: std::sync::Arc<dyn omegon_memory::MemoryBackend> =
                        std::sync::Arc::new(backend);
                    context_memory_backend = Some(memory_backend.clone());
                    context_memory_mind = Some(mind.clone());

                    // ── Embedding service (optional, for hybrid search) ──
                    // Skip the probe in child processes — the async HTTP request blocks
                    // single-threaded runtimes (ACP, delegate children).
                    let embed_service: Option<std::sync::Arc<dyn omegon_memory::EmbeddingService>> =
                        if is_child {
                            None
                        } else {
                            let profile = crate::settings::Profile::load(&cwd);
                            let svc = crate::embedding::OllamaEmbeddingService::from_config(
                                profile.embed_url.as_deref(),
                                profile.embed_model.as_deref(),
                            );
                            if svc.probe().await {
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
                                    match crate::local_embedding::LocalEmbeddingService::from_default_dir()
                            {
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
                                    tracing::info!(
                                        "embedding service not reachable — FTS-only recall"
                                    );
                                    None
                                }
                            }
                        }; // end if is_child else probe

                    let mut memory_feature =
                        features::memory::MemoryFeature::new(memory_backend, mind);
                    if let Some(ref svc) = embed_service {
                        memory_feature = memory_feature.with_embed_service(svc.clone());
                        context_embed_service = Some(svc.clone());
                    }
                    if let Some(ref vp) = codex_vault_path {
                        memory_feature = memory_feature.with_codex_vault(vp.clone());
                        tracing::info!(vault = %vp.display(), "Codex vault sync enabled for memory");
                    }
                    if embed_service.is_some() {
                        memory_feature = memory_feature
                            .with_extraction_model("anthropic:claude-haiku-4-5-20251001".into());
                    }
                    bus.register(Box::new(memory_feature));
                }
                Err(err) => {
                    let warning = format!(
                        "Memory backend unavailable — memory_* tools disabled ({})",
                        db_path.display()
                    );
                    tracing::error!(db = %db_path.display(), error = %err, "memory backend unavailable — memory_* tools disabled");
                    memory_warning = Some(warning);
                }
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

        // ─── Lifecycle (design-tree + openspec) ──────────────────────────
        // Use project root (git repo root), not cwd — docs/ and openspec/
        // live at the repo root, which may differ from cwd when running
        // from a subdirectory like core/.
        let mut lifecycle_feature = features::lifecycle::LifecycleFeature::new(&project_root);
        if let Some(ref vp) = codex_vault_path
            && codex_integration
                .as_ref()
                .is_some_and(|c| c.design_tree.enabled)
        {
            lifecycle_feature = lifecycle_feature.with_codex_vault(vp.clone());
            tracing::info!(vault = %vp.display(), "Codex vault sync enabled for design tree");
        }
        let lifecycle_snapshot = LifecycleSnapshot::from_lifecycle_feature(&lifecycle_feature);
        let lifecycle_handle = lifecycle_feature.read_handle();
        bus.register(Box::new(lifecycle_feature));

        // ─── Sandbox setting (read once, shared by cleave + delegate) ──
        let sandbox = settings
            .as_ref()
            .and_then(|s| s.lock().ok())
            .map(|s| s.sandbox)
            .unwrap_or(false);

        // ─── Cleave + delegate shared inference runtime ────────────────
        let inference_runtime = crate::inference_runtime::InferenceRuntimeState::new(&project_root);

        // ─── Cleave (decomposition + dispatch) ─────────────────────────
        let mut cleave_feature = features::cleave::CleaveFeature::new_with_safety(
            &cwd,
            session_secret_env.clone(),
            sandbox,
            dangerously_bypass_permissions,
        );
        cleave_feature = cleave_feature.with_inference_runtime(inference_runtime.clone());
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
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "codescan",
            Box::new(tools::codebase_search::CodescanProvider::new(
                project_root.clone(),
            )),
        )));

        // ─── Delegate (subagent system) ─────────────────────────────────
        let agents = crate::features::delegate::scan_agents(&cwd);
        let mut delegate_feature = features::delegate::DelegateFeature::new_with_safety(
            &cwd,
            agents,
            sandbox,
            dangerously_bypass_permissions,
        );
        delegate_feature = delegate_feature.with_inference_runtime(inference_runtime.clone());
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
        let delegate_event_slot = delegate_feature.event_sender_slot();
        bus.register(Box::new(delegate_feature));

        // ─── Session log (context injection) ────────────────────────────
        bus.register(Box::new(features::session_log::SessionLog::new(&cwd)));

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
        bus.register(Box::new(features::audit_log::AuditLog::new(
            &cwd,
            &audit_session,
        )));

        // ─── Mutation (evolutionary skill/diagnostic creation) ───────────
        bus.register(Box::new(features::mutation::MutationFeature::new(
            crate::paths::omegon_home()?,
        )));

        // ─── Usage advisory (/usage from captured provider telemetry) ───
        bus.register(Box::new(features::usage::UsageFeature::new()));

        // ─── Prompt library (/prompt registry-native command surface) ───
        bus.register(Box::new(features::prompt::PromptFeature::new()));
        bus.register(Box::new(features::loop_jobs::LoopFeature::new(
            &project_root,
        )));

        // ─── User command aliases (explicit prompt-targeted slash surfaces) ───
        bus.register(Box::new(features::user_commands::UserCommandFeature::load()));

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
        if let Some(ref settings) = settings {
            bus.register(Box::new(features::model_budget::ModelBudget::new(
                settings.clone(),
            )));
        }

        // ─── Tool management ─────────────────────────────────────────────
        let manage_tools = features::manage_tools::ManageTools::new();
        let disabled_handle = manage_tools.disabled_handle();
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
        let child_skills = crate::parse_csv_env("OMEGON_CHILD_SKILLS");
        if child_skills.is_empty() {
            persona_registry.load_skills(&cwd);
        } else {
            persona_registry.load_skills_subset(&cwd, &child_skills);
        }

        // ─── Auto-trust paths declared in skills ─────────────────────────
        // Skills can declare `trusted_paths` in their frontmatter for directories
        // they need to read/write outside the workspace. Auto-add to settings
        // so the user isn't prompted on every run and delegates inherit them.
        let skill_trusted_paths = crate::skills::collect_trusted_paths(persona_registry.skills());
        if !skill_trusted_paths.is_empty()
            && let Some(ref s) = settings
            && let Ok(mut settings_guard) = s.lock()
        {
            let mut added = Vec::new();
            for path in &skill_trusted_paths {
                if !settings_guard.trusted_directories.contains(path) {
                    settings_guard.trusted_directories.push(path.clone());
                    added.push(path.clone());
                }
            }
            if !added.is_empty() {
                tracing::info!(
                    paths = ?added,
                    "auto-trusted paths from skill frontmatter"
                );
                let mut profile = crate::settings::Profile::load(&cwd);
                profile.capture_from(&settings_guard);
                let _ = profile.save(&cwd);
            }
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
            activate_startup_persona(&mut persona_registry, &persona_name);
        }
        if let Some(tone_name) = startup_profile.tone.clone() {
            activate_startup_tone(&mut persona_registry, &tone_name);
        }

        let shared_augment_registry =
            features::persona::SharedAugmentRegistry::new(persona_registry);
        bus.register(Box::new(features::persona::PersonaFeature::new(
            shared_augment_registry.clone(),
        )));
        bus.register(Box::new(features::skills::SkillsFeature::new(
            shared_augment_registry,
        )));

        if let Some(ref settings) = settings {
            bus.register(Box::new(features::harness_settings::HarnessSettings::new(
                settings.clone(),
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
        bus.register(Box::new(
            features::context::ContextProvider::new_with_sources(
                context_metrics.clone(),
                command_tx.clone(),
                settings.clone(),
                Some(lifecycle_handle.provider()),
                context_memory_backend.clone(),
                context_memory_mind.clone(),
                Some(project_root.clone()),
            ),
        ));

        // ─── Operator-installed extensions (RPC + OCI) ────────────────
        // All extensions, including bundled ones (scribe-rpc), are discovered here
        let (
            extension_widgets,
            widget_receivers,
            vox_polling_handles,
            voice_notification_receivers,
            voice_polling_handles,
            extension_metadata,
            extension_rpc_handles,
            nex_delegation_executor,
        ) = match discover_and_register_extensions(&cwd, &mut bus, std::sync::Arc::clone(&secrets))
            .await
        {
            Ok((
                widgets,
                receivers,
                handles,
                voice_receivers,
                voice_handles,
                metadata,
                rpc_handles,
                nex_executor,
            )) => (
                widgets,
                receivers,
                handles,
                voice_receivers,
                voice_handles,
                metadata,
                rpc_handles,
                nex_executor,
            ),
            Err(e) => {
                tracing::warn!("extension discovery failed: {}", e);
                (
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    Default::default(),
                    Default::default(),
                    None,
                )
            }
        };

        // ─── Core tools (bash, read, write, edit, commit; hidden internal change) ──
        let core_tools = if let Some(ref model) = repo_model {
            tools::CoreTools::with_repo_model(cwd.clone(), model.clone())
        } else {
            tools::CoreTools::new(cwd.clone())
        };
        let core_tools = if let Some(ref s) = settings {
            core_tools.with_settings(s.clone())
        } else {
            core_tools
        };
        let nex_delegations = crate::nex::substrate::read_only_delegations(&extension_metadata);
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(core_tools),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "nex-substrate",
            Box::new({
                let provider = tools::nex_substrate::NexSubstrateProvider::new(cwd.clone())
                    .with_boundary(boundary.clone())
                    .with_delegations(nex_delegations);
                if let Some(executor) = nex_delegation_executor {
                    provider.with_executor(executor)
                } else {
                    provider
                }
            }),
        )));
        // Register internal tools that the dispatch layer calls but the LLM never sees.
        bus.register_internal_tool(crate::tool_registry::core::TRUST_DIRECTORY, "core-tools");

        // ─── External plugins (TOML manifests) ────────────────────────
        let plugin_filter = crate::plugins::PluginSelectionFilter {
            enabled_extensions: crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS"),
            disabled_extensions: crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS"),
        };
        let plugins =
            crate::plugins::discover_plugins_filtered(&cwd, Some(secrets.as_ref()), &plugin_filter)
                .await;
        for plugin in plugins {
            bus.register(plugin);
        }

        // ─── Finalize bus (caches tool/command definitions) ─────────────
        bus.finalize();

        // Wire ManageTools state so runtime filtering and list output reflect
        // the bus's finalized model-visible tool cache.
        bus.set_disabled_tools(disabled_handle.clone());
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
            let mut disabled = disabled_handle.lock().unwrap();
            tracing::info!(
                disabled = disabled.len(),
                slim = slim_mode,
                "default tool profile applied — use manage_tools to re-enable"
            );
            let child_enabled_tools = crate::parse_csv_env("OMEGON_CHILD_ENABLED_TOOLS");
            let child_disabled_tools = crate::parse_csv_env("OMEGON_CHILD_DISABLED_TOOLS");
            if !child_enabled_tools.is_empty() {
                disabled.retain(|tool| !child_enabled_tools.iter().any(|enabled| enabled == tool));
            }
            for tool in child_disabled_tools {
                disabled.insert(tool);
            }
        }

        // ─── Assemble harness status (bootstrap probe) ──────────────────
        let mut harness_status = crate::status::HarnessStatus::assemble();

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
        harness_status.update_memory(initial_memory_status.clone());
        if initial_memory_status.active_facts == 0 {
            // update_memory() marks memory_available=true even for an empty-but-working backend;
            // if startup failed earlier, restore the unavailable state and carry the warning.
            if let Some(ref warning) = memory_warning {
                harness_status.memory_available = false;
                harness_status.memory_warning = Some(warning.clone());
            }
        }
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

        // Print bootstrap panel if running interactively
        let use_color = std::io::stderr().is_terminal() && std::env::var("NO_COLOR").is_err();
        if use_color || std::io::stderr().is_terminal() {
            let panel = crate::tui::bootstrap::render_bootstrap(&harness_status, use_color);
            eprint!("{panel}");
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
        let base_prompt = settings
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.automation_level))
            .map(|level| {
                prompt::build_base_prompt_for_mode_with_subagent_policy(
                    &cwd,
                    &tool_defs,
                    prompt_mode,
                    crate::autonomy::subagent_policy_for_automation(level),
                )
                .prompt
            })
            .unwrap_or_else(|| {
                prompt::build_base_prompt_for_mode(&cwd, &tool_defs, prompt_mode).prompt
            });
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
        if let Some(svc) = context_embed_service {
            context_manager.set_embed_service(svc);
        }

        // ─── Conversation ───────────────────────────────────────────────
        let mut resume_info: Option<ResumeInfo> = None;
        let mut conversation = if let Some(resume_arg) = resume {
            let resume_id = resume_arg;
            // find_session returns the .json path; meta lives at .meta.json
            match session::find_session(&cwd, resume_id) {
                Some(path) => {
                    tracing::info!(path = %path.display(), "Resuming session");
                    match ConversationState::load_session(&path) {
                        Ok(conv) => {
                            // Read the companion meta file to populate the resumption brief
                            let meta_path = path.with_extension("meta.json");
                            if let Ok(json) = std::fs::read_to_string(&meta_path)
                                && let Ok(meta) =
                                    serde_json::from_str::<session::SessionMeta>(&json)
                            {
                                // ── Checkpoint consistency verification ──
                                if let Some(latest_cp) =
                                    crate::checkpoint::read_last_checkpoint(&meta.session_id)
                                {
                                    let cp_turns = latest_cp.intent.stats_turns;
                                    let session_turns = meta.turns;
                                    if cp_turns > session_turns {
                                        tracing::warn!(
                                            session_turns,
                                            checkpoint_turns = cp_turns,
                                            session_id = %meta.session_id,
                                            "checkpoint is ahead of session file — \
                                             session may be stale (crash during prior run?)"
                                        );
                                    } else {
                                        tracing::debug!(
                                            session_turns,
                                            checkpoint_turns = cp_turns,
                                            "checkpoint consistent with session"
                                        );
                                    }
                                }

                                let description =
                                    crate::session::session_display_description(&meta);
                                resume_info = Some(ResumeInfo {
                                    session_id: meta.session_id,
                                    turns: meta.turns,
                                    description,
                                    last_prompt_snippet: meta.last_prompt_snippet,
                                    created_at: meta.created_at,
                                });
                            }
                            conv
                        }
                        Err(e) => {
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
            vcs_ref: repo_model
                .as_ref()
                .map(|model| crate::workspace::types::WorkspaceVcsRef {
                    vcs: "git".into(),
                    branch: model.branch(),
                    revision: None,
                    remote: Some("origin".into()),
                }),
            bindings: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.bindings.clone())
                .unwrap_or_default(),
            branch: existing_workspace_lease
                .as_ref()
                .map(|lease| lease.branch.clone())
                .or_else(|| repo_model.as_ref().and_then(|model| model.branch()))
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

        let session_id = resume_info
            .as_ref()
            .map(|r| r.session_id.clone())
            .unwrap_or_else(|| startup_session_id_hint.clone());

        let initial_harness_status = harness_status;

        Ok(Self {
            bus,
            session_id,
            instance_id,
            startup_skill_activation_events: Vec::new(),
            context_metrics,
            command_tx,
            context_manager,
            conversation,
            inference_runtime,
            cwd,
            secrets: secrets.clone(),
            web_auth_state,
            session_secret_env,
            resume_info,
            workspace_state,
            startup_snapshot,
            initial_harness_status: initial_harness_status.clone(),
            extension_widgets,
            extension_metadata,
            extension_rpc_handles,
            widget_receivers,
            dashboard_handles: crate::tui::dashboard::DashboardHandles {
                lifecycle: Some(lifecycle_handle),
                cleave: Some(cleave_handle),
                delegate: Some(delegate_handle),
                session: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::tui::dashboard::SharedSessionStats::default(),
                )),
                harness: Some(std::sync::Arc::new(std::sync::Mutex::new(
                    initial_harness_status.clone(),
                ))),
            },
            cleave_event_slot,
            delegate_event_slot,
            vox_polling_handles,
            voice_notification_receivers,
            voice_polling_handles,
            skill_phases,
        })
    }

    /// Gather initial state for the TUI so the first frame has real data.
    pub fn initial_tui_state(&self) -> crate::tui::TuiInitialState {
        crate::tui::TuiInitialState {
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

fn hydrate_provider_auth_env_from_auth_json(
    session_secret_env: &mut Vec<(String, String)>,
    secrets: &omegon_secrets::SecretsManager,
) {
    // Hydrate credentials for ALL providers that have stored auth, not just the
    // parent session's model. Children (cleave, delegate) may use any provider
    // and need the corresponding API keys in their inherited environment.
    for provider in crate::auth::PROVIDERS {
        let Some(primary_env) = provider.env_vars.first().copied() else {
            continue;
        };
        if session_secret_env
            .iter()
            .any(|(name, _)| name == primary_env)
        {
            continue;
        }
        let mut source = "auth.json";
        let creds = match crate::auth::read_credentials(provider.auth_key) {
            Some(creds) if creds.cred_type == "oauth" && creds.is_expired() => {
                tracing::debug!(
                    provider = provider.id,
                    env = primary_env,
                    "stored provider OAuth credential expired; trying external adoption before env hydration"
                );
                source = "external";
                crate::auth::adopt_external_credentials(provider.auth_key)
            }
            Some(creds) => Some(creds),
            None => {
                source = "external";
                crate::auth::adopt_external_credentials(provider.auth_key)
            }
        };

        if let Some(creds) = creds {
            secrets.register_redaction_secret(primary_env, &creds.access);
            secrets.register_redaction_secret(
                &format!("{}_AUTH_JSON_ACCESS", provider.id),
                &creds.access,
            );
            secrets.register_redaction_secret(
                &format!("{}_AUTH_JSON_REFRESH", provider.id),
                &creds.refresh,
            );
            if let Some(account_id) =
                crate::auth::read_credential_extra(provider.auth_key, "accountId")
            {
                secrets.register_redaction_secret(
                    &format!("{}_AUTH_JSON_ACCOUNT_ID", provider.id),
                    &account_id,
                );
            }
            session_secret_env.push((primary_env.to_string(), creds.access));
            tracing::info!(
                provider = provider.id,
                env = primary_env,
                source,
                "hydrated provider auth env"
            );
        }
    }
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
///
/// Resolves declared secrets from the session cache and delivers them to each
/// extension via `bootstrap_secrets` RPC — never via subprocess environment.
async fn discover_and_register_extensions(
    cwd: &Path,
    bus: &mut crate::bus::EventBus,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
) -> anyhow::Result<(
    Vec<crate::extensions::ExtensionTabWidget>,
    Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
    Vec<crate::extensions::ExtensionPollingHandle>,
    Vec<tokio::sync::mpsc::UnboundedReceiver<crate::extensions::ExtensionNotification>>,
    Vec<crate::extensions::ExtensionPollingHandle>,
    std::collections::BTreeMap<String, serde_json::Value>,
    std::collections::BTreeMap<String, crate::extensions::ExtensionPollingHandle>,
    Option<std::sync::Arc<dyn crate::tools::nex_substrate::NexDelegationExecutor>>,
)> {
    let ext_dir = crate::paths::omegon_home()?.join("extensions");

    if !ext_dir.exists() {
        tracing::debug!("extension directory not found: {}", ext_dir.display());
        return Ok((
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
            Default::default(),
            Default::default(),
            None,
        ));
    }

    let profile = crate::settings::Profile::load(cwd);
    let env_enabled = crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS");
    let env_disabled = crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS");
    let mut count = 0;
    let mut extension_widgets = vec![];
    let mut widget_receivers = vec![];
    let mut vox_polling_handles = vec![];
    let mut voice_notification_receivers = vec![];
    let mut voice_polling_handles = vec![];
    let mut extension_metadata = std::collections::BTreeMap::new();
    let mut extension_rpc_handles = std::collections::BTreeMap::new();
    let mut nex_delegation_executor: Option<
        std::sync::Arc<dyn crate::tools::nex_substrate::NexDelegationExecutor>,
    > = None;
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
            .unwrap_or("unknown");
        if !profile
            .extensions
            .permits(ext_name, &env_enabled, &env_disabled)
        {
            tracing::debug!(extension = ext_name, "extension skipped by profile policy");
            continue;
        }
        if extension_state_disabled(&path) {
            tracing::debug!(extension = ext_name, "disabled extension skipped");
            continue;
        }

        // Resolve declared secrets from session cache — these were preflighted
        // at startup so no new Keychain prompts happen here.
        // Use resolve_async so vault: recipes (which require an async client) work.
        let resolved_secrets: Vec<(String, String)> = {
            if let Ok(manifest) = crate::extensions::ExtensionManifest::from_extension_dir(&path) {
                let mut pairs = Vec::new();
                for name in &manifest.secrets.required {
                    if let Some(v) = secrets.resolve_async(name).await {
                        pairs.push((name.clone(), v));
                    }
                }
                pairs
            } else {
                vec![]
            }
        };

        // Try to spawn this extension
        match crate::extensions::spawn_from_manifest(&path, &resolved_secrets).await {
            Ok(spawned) => {
                let tool_count = spawned.feature.tools().len();
                let widget_count = spawned.widgets.len();
                tracing::info!(
                    name = ext_name,
                    path = %path.display(),
                    tools = tool_count,
                    widgets = widget_count,
                    "discovered and spawned extension"
                );
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
                    ext_name.to_string(),
                    crate::extensions::metadata_with_sdk_compatibility(
                        spawned.metadata,
                        &spawned.sdk_compatibility,
                    ),
                );
                extension_rpc_handles.insert(ext_name.to_string(), spawned.rpc_polling_handle);
                if nex_delegation_executor.is_none() {
                    nex_delegation_executor = spawned.nex_delegation_executor.map(|executor| {
                        executor
                            as std::sync::Arc<
                                dyn crate::tools::nex_substrate::NexDelegationExecutor,
                            >
                    });
                }
                bus.register(spawned.feature);
                // Collect widgets and receivers for TUI
                extension_widgets.extend(spawned.widgets);
                widget_receivers.push(spawned.widget_rx);
                count += 1;
            }
            Err(e) => {
                let ext_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                tracing::warn!(
                    name = ext_name,
                    path = %path.display(),
                    error = %e,
                    "failed to spawn extension"
                );
            }
        }
    }

    if count > 0 {
        tracing::info!(count = count, "extension discovery complete");
    }

    Ok((
        extension_widgets,
        widget_receivers,
        vox_polling_handles,
        voice_notification_receivers,
        voice_polling_handles,
        extension_metadata,
        extension_rpc_handles,
        nex_delegation_executor,
    ))
}

fn extension_state_disabled(path: &Path) -> bool {
    crate::extensions::ExtensionState::load(path)
        .is_ok_and(|state| !state.enabled || state.stability.auto_disabled)
}

fn activate_startup_persona(
    registry: &mut crate::plugins::registry::AugmentRegistry,
    persona_name: &str,
) {
    let (personas, _) = crate::plugins::persona_loader::scan_available();
    let target = persona_name.to_lowercase();
    if let Some(available) = personas
        .iter()
        .find(|p| p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target))
    {
        match crate::plugins::persona_loader::load_persona(&available.path) {
            Ok(loaded) => {
                tracing::info!(persona = %loaded.name, "activating startup persona");
                registry.activate_persona(loaded);
            }
            Err(e) => {
                tracing::warn!(persona = %persona_name, error = %e, "startup persona load failed");
            }
        }
    } else {
        tracing::warn!(persona = %persona_name, "startup persona not found");
    }
}

fn activate_startup_tone(
    registry: &mut crate::plugins::registry::AugmentRegistry,
    tone_name: &str,
) {
    let (_, tones) = crate::plugins::persona_loader::scan_available();
    let target = tone_name.to_lowercase();
    if let Some(available) = tones
        .iter()
        .find(|t| t.name.to_lowercase() == target || t.id.to_lowercase().contains(&target))
    {
        match crate::plugins::persona_loader::load_tone(&available.path) {
            Ok(loaded) => {
                tracing::info!(tone = %loaded.name, "activating startup tone");
                registry.activate_tone(loaded);
            }
            Err(e) => {
                tracing::warn!(tone = %tone_name, error = %e, "startup tone load failed");
            }
        }
    } else {
        tracing::warn!(tone = %tone_name, "startup tone not found");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn provider_auth_hydration_skips_expired_oauth_credentials() {
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
                    "expires": expired,
                    "accountId": "acct_123"
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
            hydrate_provider_auth_env_from_auth_json(&mut session_secret_env, &secrets);
            unsafe {
                match original {
                    Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                    None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
                }
            }

            assert!(
                !session_secret_env
                    .iter()
                    .any(|(name, value)| name == "CHATGPT_OAUTH_TOKEN"
                        && value == "expired-codex-token"),
                "expired Codex OAuth must not be inherited by child sessions"
            );
            assert!(
                session_secret_env
                    .iter()
                    .any(|(name, value)| name == "BRAVE_API_KEY" && value == "brave-token"),
                "static credentials should still be hydrated"
            );
        });
    }

    #[test]
    fn provider_auth_hydration_adopts_fresh_external_codex_when_internal_is_expired() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        let home = dir.path().join("home");
        let codex_dir = home.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let expired = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 1_000;
        let fresh_external_access = format!(
            "e30.{}.sig",
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                serde_json::json!({
                    "exp": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        + 3600
                })
                .to_string()
            )
        );
        std::fs::write(
            &auth_path,
            serde_json::json!({
                "openai-codex": {
                    "type": "oauth",
                    "access": "expired-codex-token",
                    "refresh": "expired-refresh-token",
                    "expires": expired,
                    "accountId": "acct_expired"
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            codex_dir.join("auth.json"),
            serde_json::json!({
                "tokens": {
                    "access_token": fresh_external_access,
                    "refresh_token": "fresh-external-refresh",
                    "account_id": "acct_external"
                }
            })
            .to_string(),
        )
        .unwrap();

        with_auth_env_lock(|| {
            let original_auth = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
            let original_home = std::env::var("HOME").ok();
            unsafe {
                std::env::set_var("OMEGON_AUTH_JSON_PATH", &auth_path);
                std::env::set_var("HOME", &home);
            }
            let secrets = omegon_secrets::SecretsManager::new(dir.path()).expect("secrets manager");
            let mut session_secret_env = Vec::new();
            hydrate_provider_auth_env_from_auth_json(&mut session_secret_env, &secrets);
            unsafe {
                match original_auth {
                    Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                    None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
                }
                match original_home {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
            }

            assert!(
                session_secret_env
                    .iter()
                    .any(|(name, value)| name == "CHATGPT_OAUTH_TOKEN"
                        && value == &fresh_external_access),
                "fresh external Codex OAuth should be hydrated when internal auth is expired"
            );
            let persisted: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&auth_path).unwrap()).unwrap();
            assert_eq!(
                persisted
                    .pointer("/openai-codex/accountId")
                    .and_then(|v| v.as_str()),
                Some("acct_external")
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

    #[test]
    fn project_memory_dir_absent_without_init_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        assert!(project_memory_dir_if_initialized(dir.path()).is_none());
        assert!(!dir.path().join("ai").exists());
        assert!(!dir.path().join(".omegon").exists());
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

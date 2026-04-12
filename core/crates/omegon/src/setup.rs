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
    /// Shared context metrics — updated each turn, read by ContextProvider
    pub context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    /// Shared command channel — set by main after TUI init
    pub command_tx: crate::features::context::SharedCommandTx,
    pub context_manager: ContextManager,
    pub conversation: ConversationState,
    pub cwd: PathBuf,
    /// Secrets manager — redaction, guards, recipes.
    pub secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    /// Resolved web auth state for the embedded dashboard.
    pub web_auth_state: crate::web::WebAuthState,
    /// Resolved startup-approved secret env pairs for child/headless runs.
    pub session_secret_env: Vec<(String, String)>,
    /// Snapshot of lifecycle + memory state at startup for TUI pre-population.
    pub(crate) startup_snapshot: StartupSnapshot,
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
    /// Widget event receivers — one per discovered extension.
    pub widget_receivers: Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
    /// Slot the AgentEvent broadcast sender gets written into once main.rs
    /// has constructed the channel. The cleave feature reads this slot when
    /// emitting `AgentEvent::Decomposition*` events from inside its tool
    /// execution path. See `features::cleave::CleaveEventSlot`.
    pub cleave_event_slot: features::cleave::CleaveEventSlot,
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
        let lp = lf.provider();
        let focused_node = lp.focused_node_id().and_then(|id| {
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
        });

        let active_changes: Vec<_> = lp
            .changes()
            .iter()
            .filter(|c| !matches!(c.stage, lifecycle::types::ChangeStage::Archived))
            .map(|c| crate::tui::dashboard::ChangeSummary {
                name: c.name.clone(),
                stage: c.stage,
                done_tasks: c.done_tasks,
                total_tasks: c.total_tasks,
            })
            .collect();

        Self {
            focused_node,
            active_changes,
        }
    }
}

impl AgentSetup {
    /// Initialize the event bus, tools, memory, lifecycle context, and conversation.
    pub async fn new(
        cwd: &Path,
        resume: Option<Option<&str>>,
        settings: Option<crate::settings::SharedSettings>,
    ) -> anyhow::Result<Self> {
        let cwd = std::fs::canonicalize(cwd)?;
        let is_child = std::env::var("OMEGON_CHILD").is_ok();

        // ─── Secrets manager ────────────────────────────────────────────
        let secrets_dir = dirs::home_dir()
            .unwrap_or_else(|| cwd.clone())
            .join(".omegon");
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
            // Add only the FIRST env var per provider (highest priority auth method).
            // e.g., for Anthropic: ANTHROPIC_OAUTH_TOKEN preferred over ANTHROPIC_API_KEY.
            // This avoids multiple keychain prompts for alternatives we won't use.
            if let Some(env_var) = crate::auth::provider_env_vars(&provider).first() {
                preflight.insert((*env_var).to_string());
            }
        }
        // Extension-declared required secrets — read manifests early so keyring-backed
        // secrets (e.g. GITHUB_TOKEN) are resolved before extension subprocesses spawn.
        for name in collect_extension_secret_requirements() {
            preflight.insert(name);
        }
        // Plugin MCP env templates — scan {VAR_NAME} references so vault-backed
        // secrets referenced in plugin.toml / mcp.toml are warmed before plugins connect.
        for name in collect_plugin_secret_requirements(&cwd) {
            preflight.insert(name);
        }
        // Web search API keys — preflight so hydrate_process_env() populates them
        // and available_providers() sees them via env::var(). No keychain prompt
        // if no recipe is configured for a given key.
        for key in &["BRAVE_API_KEY", "TAVILY_API_KEY", "SERPER_API_KEY"] {
            preflight.insert((*key).to_string());
        }
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
        let mut session_secret_env = secrets.session_env();
        hydrate_provider_auth_env_from_auth_json(settings.as_ref(), &mut session_secret_env);
        for (name, value) in &session_secret_env {
            // SAFETY: setup runs before provider detection for this process; exporting
            // startup-resolved secrets here makes the active provider path see the
            // same credential surface as child/headless runs.
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

        // ─── Repo model (git state tracking) ────────────────────────────
        let repo_model = match omegon_git::RepoModel::discover(&cwd) {
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
        };

        // ─── Core tools (bash, read, write, edit, change, speculate, commit) ──
        let core_tools = if let Some(ref model) = repo_model {
            tools::CoreTools::with_repo_model(cwd.clone(), model.clone())
        } else {
            tools::CoreTools::new(cwd.clone())
        };
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(core_tools),
        )));

        // ─── Feature tool providers ─────────────────────────────────────
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "web-search",
            Box::new(tools::web_search::WebSearchProvider::new()),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "local-inference",
            Box::new(tools::local_inference::LocalInferenceProvider::new()),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "view",
            Box::new(tools::view::ViewProvider::new(cwd.clone())),
        )));
        bus.register(Box::new(features::adapter::ToolAdapter::new(
            "render",
            Box::new(tools::render::RenderProvider::new()),
        )));

        // ─── Memory ─────────────────────────────────────────────────────
        let mind = "default".to_string();
        let project_root = find_project_root(&cwd);
        let memory_dir = {
            // Canonical: ai/memory/, fallback: .omegon/memory/
            let ai = project_root.join("ai").join("memory");
            let omegon = project_root.join(".omegon").join("memory");
            if omegon.exists() && !ai.exists() {
                omegon
            } else {
                ai
            }
        };
        let _ = std::fs::create_dir_all(&memory_dir);
        let db_path = memory_dir.join("facts.db");
        let jsonl_path = memory_dir.join("facts.jsonl");

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

        let mut context_memory_backend: Option<std::sync::Arc<dyn omegon_memory::MemoryBackend>> = None;
        let mut context_memory_mind: Option<String> = None;

        if let Ok(backend) = omegon_memory::SqliteBackend::open(&db_path) {
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
                    && jsonl_path.exists()
                    && let Ok(jsonl) = std::fs::read_to_string(&jsonl_path)
                {
                    match backend.import_jsonl(&jsonl).await {
                        Ok(import) => {
                            tracing::info!(imported = import.imported, "imported facts.jsonl")
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
            let embed_service: Option<
                std::sync::Arc<dyn omegon_memory::EmbeddingService>,
            > = {
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
                    Some(std::sync::Arc::new(svc))
                } else {
                    tracing::info!("embedding service not reachable — FTS-only recall");
                    None
                }
            };

            let mut memory_feature =
                features::memory::MemoryFeature::new(memory_backend, mind);
            if let Some(svc) = embed_service {
                memory_feature = memory_feature.with_embed_service(svc);
            }
            bus.register(Box::new(memory_feature));
        } else {
            let warning = format!(
                "Memory backend unavailable — memory_* tools disabled ({})",
                db_path.display()
            );
            tracing::error!(db = %db_path.display(), "memory backend unavailable — memory_* tools disabled");
            memory_warning = Some(warning);
        }

        // ─── Lifecycle (design-tree + openspec) ──────────────────────────
        // Use project root (git repo root), not cwd — docs/ and openspec/
        // live at the repo root, which may differ from cwd when running
        // from a subdirectory like core/.
        let lifecycle_feature = features::lifecycle::LifecycleFeature::new(&project_root);
        let lifecycle_snapshot = LifecycleSnapshot::from_lifecycle_feature(&lifecycle_feature);
        let lifecycle_handle = lifecycle_feature.shared_provider();
        bus.register(Box::new(lifecycle_feature));

        // ─── Cleave (decomposition + dispatch) ─────────────────────────
        let cleave_feature = features::cleave::CleaveFeature::new(&cwd, session_secret_env.clone());
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
        bus.register(Box::new(features::delegate::DelegateFeature::new(
            &cwd, agents,
        )));

        // ─── Session log (context injection) ────────────────────────────
        bus.register(Box::new(features::session_log::SessionLog::new(&cwd)));

        // ─── Usage advisory (/usage from captured provider telemetry) ───
        bus.register(Box::new(features::usage::UsageFeature::new()));

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

        // ─── Model budget (tier switching + thinking) ───────────────────
        if let Some(ref settings) = settings {
            bus.register(Box::new(features::model_budget::ModelBudget::new(
                settings.clone(),
            )));
        }

        // ─── Tool management ─────────────────────────────────────────────
        let manage_tools = features::manage_tools::ManageTools::new();
        let disabled_handle = manage_tools.disabled_handle();
        bus.register(Box::new(manage_tools));

        // ─── Auth (credential probing + status) ───────────────────────
        bus.register(Box::new(features::auth::AuthFeature::new()));

        // ─── Native features ────────────────────────────────────────────
        // ─── Persona system ────────────────────────────────────────────
        let mut persona_registry =
            crate::plugins::registry::PluginRegistry::new(crate::prompt::load_lex_imperialis());
        let child_skills = crate::parse_csv_env("OMEGON_CHILD_SKILLS");
        if child_skills.is_empty() {
            persona_registry.load_skills(&cwd);
        } else {
            persona_registry.load_skills_subset(&cwd, &child_skills);
        }

        // ─── Activate startup persona (child or headless --persona) ────
        if let Ok(persona_name) = std::env::var("OMEGON_CHILD_PERSONA") {
            let (personas, _) = crate::plugins::persona_loader::scan_available();
            let target = persona_name.to_lowercase();
            if let Some(available) = personas.iter().find(|p| {
                p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target)
            }) {
                match crate::plugins::persona_loader::load_persona(&available.path) {
                    Ok(loaded) => {
                        tracing::info!(persona = %loaded.name, "activating startup persona");
                        persona_registry.activate_persona(loaded);
                    }
                    Err(e) => {
                        tracing::warn!(persona = %persona_name, error = %e, "startup persona load failed");
                    }
                }
            } else {
                tracing::warn!(persona = %persona_name, "startup persona not found");
            }
        }

        bus.register(Box::new(features::persona::PersonaFeature::new(
            persona_registry,
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
        bus.register(Box::new(features::context::ContextProvider::new_with_sources(
            context_metrics.clone(),
            command_tx.clone(),
            Some(lifecycle_handle.clone()),
            context_memory_backend.clone(),
            context_memory_mind.clone(),
            Some(project_root.clone()),
        )));

        // ─── Operator-installed extensions (RPC + OCI) ────────────────
        // All extensions, including bundled ones (scribe-rpc), are discovered here
        let (extension_widgets, widget_receivers) =
            match discover_and_register_extensions(&mut bus, std::sync::Arc::clone(&secrets)).await
            {
                Ok((widgets, receivers)) => (widgets, receivers),
                Err(e) => {
                    tracing::warn!("extension discovery failed: {}", e);
                    (vec![], vec![])
                }
            };

        // ─── External plugins (TOML manifests) ────────────────────────
        let plugin_filter = crate::plugins::PluginSelectionFilter {
            enabled_extensions: crate::parse_csv_env("OMEGON_CHILD_ENABLED_EXTENSIONS"),
            disabled_extensions: crate::parse_csv_env("OMEGON_CHILD_DISABLED_EXTENSIONS"),
        };
        let plugins = crate::plugins::discover_plugins_filtered(
            &cwd,
            Some(secrets.as_ref()),
            &plugin_filter,
        )
        .await;
        for plugin in plugins {
            bus.register(plugin);
        }

        // ─── Finalize bus (caches tool/command definitions) ─────────────
        bus.finalize();

        // Wire disabled-tools handle so tool_definitions() filters at runtime
        bus.set_disabled_tools(disabled_handle.clone());

        // ─── Default tool profile — disable rarely-used tools ───────────
        // These tools are available via manage_tools enable but don't need
        // to consume input tokens on every request.
        {
            use crate::tool_registry as reg;
            let slim_mode = settings
                .as_ref()
                .and_then(|s| s.lock().ok().map(|g| g.slim_mode))
                .unwrap_or(false);
            let mut disabled = disabled_handle.lock().unwrap();
            // Speculation tools — only needed when explicitly exploring
            disabled.insert(reg::core::SPECULATE_START.into());
            disabled.insert(reg::core::SPECULATE_CHECK.into());
            disabled.insert(reg::core::SPECULATE_COMMIT.into());
            disabled.insert(reg::core::SPECULATE_ROLLBACK.into());
            // Render/image tools — most sessions don't need them
            disabled.insert(reg::render::RENDER_DIAGRAM.into());
            disabled.insert(reg::render::GENERATE_IMAGE_LOCAL.into());
            // Persona/tone switching — rarely used mid-session
            disabled.insert(reg::persona::SWITCH_PERSONA.into());
            disabled.insert(reg::persona::SWITCH_TONE.into());
            disabled.insert(reg::persona::LIST_PERSONAS.into());
            // Delegate system — advanced multi-agent, not default
            disabled.insert(reg::delegate::DELEGATE.into());
            disabled.insert(reg::delegate::DELEGATE_RESULT.into());
            disabled.insert(reg::delegate::DELEGATE_STATUS.into());
            // Auth probing — used at startup, not mid-conversation
            disabled.insert(reg::auth::AUTH_STATUS.into());
            // Harness settings — internal, rarely agent-called
            disabled.insert(reg::harness_settings::HARNESS_SETTINGS.into());
            // Memory tools that are rarely called directly
            disabled.insert(reg::memory::MEMORY_INGEST_LIFECYCLE.into());
            disabled.insert(reg::memory::MEMORY_CONNECT.into());
            disabled.insert(reg::memory::MEMORY_SEARCH_ARCHIVE.into());
            if slim_mode {
                disabled.insert(reg::web_search::WEB_SEARCH.into());
                disabled.insert(reg::local_inference::ASK_LOCAL_MODEL.into());
                disabled.insert(reg::local_inference::LIST_LOCAL_MODELS.into());
                disabled.insert(reg::local_inference::MANAGE_OLLAMA.into());
                disabled.insert(reg::memory::MEMORY_STORE.into());
                disabled.insert(reg::memory::MEMORY_RECALL.into());
                disabled.insert(reg::memory::MEMORY_QUERY.into());
                disabled.insert(reg::memory::MEMORY_ARCHIVE.into());
                disabled.insert(reg::memory::MEMORY_SUPERSEDE.into());
                disabled.insert(reg::memory::MEMORY_FOCUS.into());
                disabled.insert(reg::memory::MEMORY_RELEASE.into());
                disabled.insert(reg::memory::MEMORY_EPISODES.into());
                disabled.insert(reg::memory::MEMORY_COMPACT.into());
                disabled.insert(reg::lifecycle::DESIGN_TREE.into());
                disabled.insert(reg::lifecycle::DESIGN_TREE_UPDATE.into());
                disabled.insert(reg::lifecycle::OPENSPEC_MANAGE.into());
                disabled.insert(reg::lifecycle::LIFECYCLE_DOCTOR.into());
                disabled.insert(reg::cleave::CLEAVE_ASSESS.into());
                disabled.insert(reg::cleave::CLEAVE_RUN.into());
                disabled.insert(reg::codescan::CODEBASE_INDEX.into());
                disabled.insert(reg::session_log::SESSION_LOG.into());
                // OM research mode keeps local repo inspection and direct shell validation,
                // but drops heavier orchestration/meta-control surfaces by default.
                disabled.insert(reg::core::WHOAMI.into());
                disabled.insert(reg::core::SERVE.into());
                disabled.insert(reg::core::CHRONOS.into());
                disabled.insert(reg::view::VIEW.into());
                disabled.insert(reg::context::REQUEST_CONTEXT.into());
                disabled.insert(reg::context::CONTEXT_COMPACT.into());
                disabled.insert(reg::context::CONTEXT_CLEAR.into());
                disabled.insert(reg::model_budget::SET_MODEL_TIER.into());
                disabled.insert(reg::model_budget::SWITCH_TO_OFFLINE_DRIVER.into());
                disabled.insert(reg::model_budget::SET_THINKING_LEVEL.into());
            }
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

        // Probe all authentication providers
        let auth_status = crate::auth::probe_all_providers().await;
        harness_status.providers = crate::auth::auth_status_to_provider_statuses(&auth_status);
        harness_status.annotate_provider_runtime_health();

        // Populate MCP/plugin info from discovered features
        harness_status.update_from_bus(&bus);
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
        // Build the base prompt from bus tool definitions (not the old tools vec)
        let tool_defs = bus.tool_definitions();
        let slim_mode = settings
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.slim_mode))
            .unwrap_or(false);
        let base_prompt = prompt::build_base_prompt_with_breakdown(&cwd, &tool_defs, slim_mode).prompt;

        // Context providers: the bus collects context from features, but we
        // still need the ContextManager for the injection pipeline (TTL decay,
        // budget management, priority sorting). Pass no standalone providers —
        // the bus will provide context via collect_context().
        let context_manager = ContextManager::new(base_prompt, vec![]);

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
                            if let Ok(json) = std::fs::read_to_string(&meta_path) {
                                if let Ok(meta) =
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

                                    resume_info = Some(ResumeInfo {
                                        session_id: meta.session_id,
                                        turns: meta.turns,
                                        last_prompt_snippet: meta.last_prompt_snippet,
                                        created_at: meta.created_at,
                                    });
                                }
                            }
                            conv
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "Failed to load session (format may be from an older version) — starting fresh"
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
        let existing_heartbeat = existing_workspace_lease
            .as_ref()
            .and_then(|lease| crate::workspace::runtime::heartbeat_epoch_secs(&lease.last_heartbeat));
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
            vcs_ref: repo_model.as_ref().map(|model| crate::workspace::types::WorkspaceVcsRef {
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
        let _ = crate::workspace::runtime::write_workspace_lease(&cwd, &workspace_lease);
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
            context_metrics,
            command_tx,
            context_manager,
            conversation,
            cwd,
            secrets: secrets.clone(),
            web_auth_state,
            session_secret_env,
            resume_info,
            workspace_state,
            startup_snapshot,
            initial_harness_status: initial_harness_status.clone(),
            extension_widgets,
            widget_receivers,
            dashboard_handles: crate::tui::dashboard::DashboardHandles {
                lifecycle: Some(lifecycle_handle),
                cleave: Some(cleave_handle),
                session: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::tui::dashboard::SharedSessionStats::default(),
                )),
                harness: Some(std::sync::Arc::new(std::sync::Mutex::new(
                    initial_harness_status.clone(),
                ))),
            },
            cleave_event_slot,
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

/// Find the project root by walking up from cwd looking for .git.
pub fn find_project_root(cwd: &Path) -> PathBuf {
    let mut dir = cwd.to_path_buf();
    loop {
        let git_path = dir.join(".git");
        if git_path.is_dir() {
            return dir;
        }
        if git_path.is_file() {
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
        if !dir.pop() {
            break;
        }
    }
    cwd.to_path_buf()
}

/// Scan installed extension manifests and collect all declared secret names.
/// Called during the startup preflight phase — before extensions are spawned —
/// so keyring-backed secrets are warmed into the session cache in time.
fn collect_extension_secret_requirements() -> Vec<String> {
    let ext_dir = match dirs::home_dir() {
        Some(h) => h.join(".omegon/extensions"),
        None => return vec![],
    };
    if !ext_dir.exists() {
        return vec![];
    }
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(&ext_dir) else {
        return vec![];
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
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
            // Optional secrets are preflighted too — extension degrades gracefully if absent,
            // but we still want the keyring prompt to happen at the startup boundary,
            // not mid-session.
            for name in manifest.secrets.optional {
                names.push(name);
            }
        }
    }
    names
}

fn hydrate_provider_auth_env_from_auth_json(
    settings: Option<&crate::settings::SharedSettings>,
    session_secret_env: &mut Vec<(String, String)>,
) {
    let provider = settings
        .and_then(|s| s.lock().ok().map(|g| crate::providers::infer_provider_id(&g.model)));
    let Some(provider) = provider else {
        return;
    };
    let env_vars = crate::auth::provider_env_vars(&provider);
    let Some(primary_env) = env_vars.first().copied() else {
        return;
    };
    if session_secret_env.iter().any(|(name, _)| name == primary_env) {
        return;
    }
    let auth_key = crate::auth::auth_json_key(&provider);
    if let Some(creds) = crate::auth::read_credentials(auth_key) {
        session_secret_env.push((primary_env.to_string(), creds.access));
        tracing::info!(provider, env = primary_env, "hydrated provider auth env from auth.json");
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
            if bytes[i] == b'{' {
                if let Some(end) = s[i + 1..].find('}') {
                    let var = &s[i + 1..i + 1 + end];
                    if !var.is_empty()
                        && var.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    {
                        out.push(var.to_string());
                    }
                    i += end + 2;
                    continue;
                }
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
        dirs::home_dir().map(|h| h.join(".omegon/plugins")),
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
    if let Ok(content) = std::fs::read_to_string(&mcp_toml) {
        if let Ok(servers) = toml::from_str::<
            std::collections::HashMap<String, crate::plugins::mcp::McpServerConfig>,
        >(&content)
        {
            scan_servers(&servers, &mut names);
        }
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
    bus: &mut crate::bus::EventBus,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
) -> anyhow::Result<(
    Vec<crate::extensions::ExtensionTabWidget>,
    Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
)> {
    let ext_dir = dirs::home_dir()
        .map(|h| h.join(".omegon/extensions"))
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;

    if !ext_dir.exists() {
        tracing::debug!("extension directory not found: {}", ext_dir.display());
        return Ok((vec![], vec![]));
    }

    let mut count = 0;
    let mut extension_widgets = vec![];
    let mut widget_receivers = vec![];
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

        // Resolve declared secrets from session cache — these were preflighted
        // at startup so no new Keychain prompts happen here.
        // Use resolve_async so vault: recipes (which require an async client) work.
        let resolved_secrets: Vec<(String, String)> = {
            if let Ok(manifest) = crate::extensions::ExtensionManifest::from_extension_dir(&path) {
                let mut pairs = Vec::new();
                for name in manifest
                    .secrets
                    .required
                    .iter()
                    .chain(manifest.secrets.optional.iter())
                {
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
                let ext_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                let tool_count = spawned.feature.tools().len();
                let widget_count = spawned.widgets.len();
                tracing::info!(
                    name = ext_name,
                    path = %path.display(),
                    tools = tool_count,
                    widgets = widget_count,
                    "discovered and spawned extension"
                );
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

    Ok((extension_widgets, widget_receivers))
}

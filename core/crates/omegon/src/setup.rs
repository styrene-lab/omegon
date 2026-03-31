//! Agent setup — shared initialization for headless and interactive modes.
//!
//! Builds the EventBus with all features registered, plus the ContextManager
//! and ConversationState needed for the agent loop.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use omegon_memory::MemoryBackend as _; // bring trait methods into scope

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

/// Everything needed to run an agent loop.
pub struct AgentSetup {
    /// The event bus — owns all features. The loop dispatches tools and
    /// emits events through the bus.
    pub bus: EventBus,
    /// Shared context metrics — updated each turn, read by ContextProvider
    pub context_metrics: std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
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
            preflight.extend(
                crate::auth::provider_env_vars(&provider)
                    .iter()
                    .map(|s| (*s).to_string()),
            );
        }
        tracing::info!(
            requested = preflight.len(),
            names = ?preflight,
            child = is_child,
            "startup secret preflight plan"
        );
        secrets.preflight_session_cache(preflight);
        let session_secret_env = secrets.session_env();
        let web_auth_state = crate::web::resolve_web_auth_state(&secrets, "".into()).await;
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
                tracing::info!(facts = initial_memory_status.active_facts, episodes = initial_memory_status.episodes, edges = initial_memory_status.edges, "memory snapshot for TUI");
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
            bus.register(Box::new(features::memory::MemoryFeature::new(
                memory_backend,
                mind,
            )));
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
        let persona_registry =
            crate::plugins::registry::PluginRegistry::new(crate::prompt::load_lex_imperialis());
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
        bus.register(Box::new(features::context::ContextProvider::new(context_metrics.clone(), command_tx.clone())));

        // ─── Operator-installed extensions (RPC + OCI) ────────────────
        // All extensions, including bundled ones (scribe-rpc), are discovered here
        if let Err(e) = discover_and_register_extensions(&mut bus).await {
            tracing::warn!("extension discovery failed: {}", e);
        }

        // ─── External plugins (TOML manifests) ────────────────────────
        let plugins = crate::plugins::discover_plugins(&cwd).await;
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
            tracing::info!(
                disabled = disabled.len(),
                "default tool profile applied — use manage_tools to re-enable"
            );
        }

        // ─── Assemble harness status (bootstrap probe) ──────────────────
        let mut harness_status = crate::status::HarnessStatus::assemble();

        // Probe all authentication providers
        let auth_status = crate::auth::probe_all_providers().await;
        harness_status.providers = crate::auth::auth_status_to_provider_statuses(&auth_status);

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
            web_auth_source = harness_status.web_auth_source.as_deref().unwrap_or("unknown"),
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
        let base_prompt = prompt::build_base_prompt(&cwd, &tool_defs);

        // Context providers: the bus collects context from features, but we
        // still need the ContextManager for the injection pipeline (TTL decay,
        // budget management, priority sorting). Pass no standalone providers —
        // the bus will provide context via collect_context().
        let context_manager = ContextManager::new(base_prompt, vec![]);

        // ─── Conversation ───────────────────────────────────────────────
        let mut resume_info: Option<ResumeInfo> = None;
        let conversation = if let Some(resume_arg) = resume {
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
                                if let Ok(meta) = serde_json::from_str::<session::SessionMeta>(&json) {
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

        let startup_snapshot = StartupSnapshot {
            total_facts: initial_memory_status.total_facts,
            lifecycle: lifecycle_snapshot,
        };

        let initial_harness_status = harness_status;

        Ok(Self {
            bus,
            context_metrics,
            command_tx,
            context_manager,
            conversation,
            cwd,
            secrets: secrets.clone(),
            web_auth_state,
            session_secret_env,
            resume_info,
            startup_snapshot,
            initial_harness_status,
            dashboard_handles: crate::tui::dashboard::DashboardHandles {
                lifecycle: Some(lifecycle_handle),
                cleave: Some(cleave_handle),
                session: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::tui::dashboard::SharedSessionStats::default(),
                )),
                harness: None, // Will be set by the TUI when harness events are received
            },
        })
    }

    /// Gather initial state for the TUI so the first frame has real data.
    pub fn initial_tui_state(&self) -> crate::tui::TuiInitialState {
        crate::tui::TuiInitialState {
            total_facts: self.startup_snapshot.total_facts,
            focused_node: self.startup_snapshot.lifecycle.focused_node.clone(),
            active_changes: self.startup_snapshot.lifecycle.active_changes.clone(),
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

/// Discover and spawn operator-installed extensions from ~/.omegon/extensions/.
/// Each subdirectory with a manifest.toml is treated as an extension.
async fn discover_and_register_extensions(bus: &mut crate::bus::EventBus) -> anyhow::Result<()> {
    let ext_dir = dirs::home_dir()
        .map(|h| h.join(".omegon/extensions"))
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;

    if !ext_dir.exists() {
        tracing::debug!("extension directory not found: {}", ext_dir.display());
        return Ok(());
    }

    let mut count = 0;
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

        // Try to spawn this extension
        match crate::extensions::spawn_from_manifest(&path).await {
            Ok(feature) => {
                let ext_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                let tool_count = feature.tools().len();
                tracing::info!(
                    name = ext_name,
                    path = %path.display(),
                    tools = tool_count,
                    "discovered and spawned extension"
                );
                bus.register(feature);
                count += 1;
            }
            Err(e) => {
                let ext_name = path.file_name()
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

    Ok(())
}

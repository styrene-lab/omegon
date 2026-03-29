//! Omegon — Rust-native agent loop and lifecycle engine.
#![allow(dead_code)] // Phase 0 scaffold — fields/methods used as implementation fills in
//!
//! Phase 0: Headless agent loop for cleave children and standalone use.
//! Phase 1: Process owner with TUI bridge subprocess.
//! Phase 2: Native TUI rendering.
//! Phase 3: Native LLM provider clients.

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod auth;
mod bridge;
pub mod bus;
mod cleave;
mod context;
pub mod features;
mod migrate;
mod smoke;
mod switch;
mod update;

mod conversation;
mod lifecycle;
mod r#loop;
mod ollama;
mod plugin_cli;
mod plugins;
mod prompt;
mod providers;
pub mod routing;
mod session;
pub mod settings;
mod setup;
mod startup;
pub mod status;
pub mod tool_registry;
mod tools;
mod tui;
pub mod util;
mod web;

use bridge::LlmBridge;
use omegon_traits::AgentEvent;

/// Short version: `0.14.0 (3a4b5c6 2026-03-21)`
const fn build_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        " (",
        env!("OMEGON_GIT_SHA"),
        " ",
        env!("OMEGON_BUILD_DATE"),
        ")",
    )
}

/// Long version for `--version`: includes git describe only when tag doesn't match.
/// build.rs sets OMEGON_GIT_DESCRIBE to "" when tag matches Cargo version,
/// or "\ngit: v0.14.1-rc.15-125-gad5428c" when they diverge.
const fn build_long_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        " (",
        env!("OMEGON_GIT_SHA"),
        " ",
        env!("OMEGON_BUILD_DATE"),
        ")",
        env!("OMEGON_GIT_DESCRIBE"),
    )
}

#[derive(Parser)]
#[command(
    name = "omegon",
    about = "Omegon — AI coding agent",
    version = build_version(),
    long_version = build_long_version(),
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Working directory
    #[arg(short, long, default_value = ".", global = true)]
    cwd: PathBuf,

    /// Model identifier (provider:model format)
    #[arg(
        short,
        long,
        default_value = "anthropic:claude-sonnet-4-6",
        global = true
    )]
    model: String,

    // ── Agent mode args (used when no subcommand) ───────────────────────
    /// Prompt to execute (headless mode)
    #[arg(short, long)]
    prompt: Option<String>,

    /// Read prompt from a file instead of CLI argument
    #[arg(long)]
    prompt_file: Option<PathBuf>,

    /// Maximum turns before forced stop (0 = no limit)
    #[arg(long, default_value = "50")]
    max_turns: u32,

    /// Max retries on transient LLM errors
    #[arg(long, default_value = "3")]
    max_retries: u32,

    /// Resume a specific session by ID prefix. Without a value, resumes the
    /// most recent session (this is the default — omegon always resumes).
    #[arg(long)]
    resume: Option<Option<String>>,

    /// Start a fresh session, ignoring any saved history for this directory.
    #[arg(long)]
    fresh: bool,

    /// Disable session auto-save on exit.
    #[arg(long)]
    no_session: bool,

    /// Skip the splash screen animation on startup.
    #[arg(long)]
    no_splash: bool,

    /// Start with the tutorial overlay active (demo mode).
    #[arg(long)]
    tutorial: bool,

    /// Run headless smoke tests — validates operator features work end-to-end.
    /// Requires LLM auth (any provider) or local inference (Ollama).
    #[arg(long)]
    smoke: bool,

    /// Queue an initial prompt in the TUI (interactive mode, not headless).
    /// The prompt is sent automatically after startup. The TUI stays open.
    #[arg(long)]
    initial_prompt: Option<String>,

    /// Like --initial-prompt but reads from a file.
    #[arg(long)]
    initial_prompt_file: Option<PathBuf>,

    /// Override context class (squad/maniple/clan/legion).
    #[arg(long)]
    context_class: Option<String>,

    /// Log level: error, warn, info, debug, trace. Overrides RUST_LOG.
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    /// Write logs to a file in addition to stderr.
    #[arg(long, global = true)]
    log_file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum AuthAction {
    /// Show authentication status for all providers.
    Status,
    /// Log in to a provider (OAuth or API key depending on provider).
    Login {
        /// Provider to log in to (anthropic, openai, or openai-codex). Default: anthropic.
        #[arg(default_value = "anthropic")]
        provider: String,
    },
    /// Log out from a provider (removes stored credentials).
    Logout {
        /// Provider to log out from.
        provider: String,
    },
    /// Unlock encrypted secrets store.
    Unlock,
}

#[derive(Subcommand)]
enum Commands {
    /// Run interactive TUI session — ratatui-based terminal interface.
    Interactive,

    /// Unified authentication management.
    /// Usage: omegon auth <status|login|logout|unlock> [provider]
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// Log in to a provider. Defaults to Anthropic.
    /// Usage: omegon-agent login [anthropic|openai|openai-codex]
    /// DEPRECATED: Use `omegon auth login` instead.
    #[command(hide = true)]
    Login {
        /// Provider to log in to (anthropic, openai, or openai-codex). Default: anthropic.
        #[arg(default_value = "anthropic")]
        provider: String,
    },

    /// Migrate settings from another CLI agent tool.
    /// Usage: omegon-agent migrate [auto|claude-code|pi|codex|cursor|aider|continue|copilot|windsurf]
    Migrate {
        /// Source to migrate from. "auto" detects all available tools.
        #[arg(default_value = "auto")]
        source: String,
    },

    /// Manage plugins — install, list, remove, update.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },

    /// Run a cleave orchestration — dispatch multiple agent children in parallel.
    Cleave {
        /// Path to the plan JSON file
        #[arg(long)]
        plan: String,

        /// The directive (task description)
        #[arg(long)]
        directive: String,

        /// Workspace directory for worktrees and state.
        /// If workspace/state.json exists, it is loaded and resumed
        /// (preserving TS-written worktree paths and task files).
        #[arg(long)]
        workspace: String,

        /// Maximum parallel children
        #[arg(long, default_value = "4")]
        max_parallel: usize,

        /// Per-child wall-clock timeout in seconds
        #[arg(long, default_value = "900")]
        timeout: u64,

        /// Per-child idle timeout in seconds (no stderr output = stalled)
        #[arg(long, default_value = "180")]
        idle_timeout: u64,

        /// Max turns per child agent
        #[arg(long, default_value = "50")]
        max_turns: u32,
    },

    /// Switch between Omegon versions (download, install, activate).
    /// Usage: omegon switch [VERSION]
    Switch {
        /// Version to switch to (e.g. "0.14.1-rc.12"). Omit for interactive picker.
        version: Option<String>,

        /// List installed versions
        #[arg(long)]
        list: bool,

        /// Switch to the latest stable release
        #[arg(long)]
        latest: bool,

        /// Switch to the latest release candidate
        #[arg(long)]
        latest_rc: bool,
    },

    /// Audit design-tree state for suspicious lifecycle drift.
    #[command(hide = true)]
    Doctor,
}

#[derive(Subcommand)]
enum PluginAction {
    /// Install a plugin from a git URL or local path.
    Install {
        /// Git URL or local directory path containing plugin.toml.
        uri: String,
    },
    /// List installed plugins.
    List,
    /// Remove an installed plugin.
    Remove {
        /// Plugin directory name.
        name: String,
    },
    /// Update installed plugins (git pull).
    Update {
        /// Plugin name to update. Omit to update all.
        name: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ─── Logging setup ──────────────────────────────────────────────────
    // Priority: RUST_LOG env > --log-level flag > "info" default
    // Interactive mode: no subcommand (default) or explicit `interactive`.
    // In both cases ratatui owns stderr — tracing must go to file only.
    let is_interactive = matches!(cli.command, Some(Commands::Interactive) | None)
        && cli.prompt.is_none()
        && cli.prompt_file.is_none();
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    // Interactive mode: tracing MUST NOT go to stderr (ratatui owns it).
    // Logs go to --log-file or ~/.config/omegon/omegon.log as default.
    // Headless mode: stderr is fine.
    let _guard: Option<tracing_appender::non_blocking::WorkerGuard>;

    if is_interactive {
        let log_path = cli.log_file.clone().unwrap_or_else(|| {
            let dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config/omegon");
            let _ = std::fs::create_dir_all(&dir);
            dir.join("omegon.log")
        });
        let dir = log_path.parent().unwrap_or(Path::new("."));
        let name = log_path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("omegon.log");
        let file_appender = tracing_appender::rolling::never(dir, name);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        _guard = Some(guard);

        let file_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_ansi(false)
            .with_writer(non_blocking);

        // No stderr layer in interactive mode
        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .init();
    } else if let Some(ref log_path) = cli.log_file {
        let dir = log_path.parent().unwrap_or(Path::new("."));
        let name = log_path
            .file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("omegon.log");
        let file_appender = tracing_appender::rolling::never(dir, name);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        _guard = Some(guard);

        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_writer(std::io::stderr);
        let file_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_ansi(false)
            .with_writer(non_blocking);

        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer)
            .init();
    } else {
        _guard = None;
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_writer(std::io::stderr);

        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .init();
    }

    match cli.command {
        Some(Commands::Plugin { ref action }) => {
            match action {
                PluginAction::Install { uri } => plugin_cli::install(uri)?,
                PluginAction::List => plugin_cli::list()?,
                PluginAction::Remove { name } => plugin_cli::remove(name)?,
                PluginAction::Update { name } => plugin_cli::update(name.as_deref())?,
            }
            Ok(())
        }
        Some(Commands::Interactive) => run_interactive_command(&cli).await,
        Some(Commands::Migrate { ref source }) => {
            let cwd = std::fs::canonicalize(&cli.cwd)?;
            let report = migrate::run(source, &cwd);
            println!("{}", report.summary());
            Ok(())
        }
        Some(Commands::Auth { ref action }) => run_auth_command(action).await,
        Some(Commands::Login { ref provider }) => {
            // Backward compatibility - redirect to new auth login command
            eprintln!("Warning: 'login' command is deprecated. Use 'omegon auth login' instead.");
            run_auth_login(provider).await
        }
        Some(Commands::Cleave {
            ref plan,
            ref directive,
            ref workspace,
            max_parallel,
            timeout,
            idle_timeout,
            max_turns,
        }) => {
            run_cleave_command(
                &cli,
                Path::new(plan),
                directive,
                Path::new(workspace),
                max_parallel,
                timeout,
                idle_timeout,
                max_turns,
            )
            .await
        }
        Some(Commands::Switch {
            version,
            list,
            latest,
            latest_rc,
        }) => {
            if list {
                switch::list_versions().await
            } else if latest {
                switch::switch_to_latest(false).await
            } else if latest_rc {
                switch::switch_to_latest(true).await
            } else if let Some(ver) = version {
                switch::switch_to_version(&ver).await
            } else {
                switch::interactive_picker().await
            }
        }
        Some(Commands::Doctor) => run_doctor_command(&cli).await,
        None => {
            // No subcommand: interactive if no --prompt, headless if --prompt given
            if cli.smoke {
                run_smoke_command(&cli).await
            } else if cli.prompt.is_some() || cli.prompt_file.is_some() {
                run_agent_command(&cli).await
            } else {
                run_interactive_command(&cli).await
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_cleave_command(
    cli: &Cli,
    plan_path: &Path,
    directive: &str,
    workspace: &Path,
    max_parallel: usize,
    timeout: u64,
    idle_timeout: u64,
    max_turns: u32,
) -> anyhow::Result<()> {
    let repo_path = std::fs::canonicalize(&cli.cwd)?;
    let plan_json = std::fs::read_to_string(plan_path)?;
    let plan = cleave::CleavePlan::from_json(&plan_json)?;

    tracing::info!(
        children = plan.children.len(),
        max_parallel,
        model = %cli.model,
        "cleave orchestration starting"
    );

    // Resolve self binary path for spawning children
    let agent_binary = std::env::current_exe()?;
    let agent_setup = setup::AgentSetup::new(&repo_path, None, None).await?;

    let config = cleave::orchestrator::CleaveConfig {
        agent_binary,
        bridge_path: PathBuf::new(), // Legacy — not used by native dispatch
        node: String::new(),
        model: cli.model.clone(),
        max_parallel,
        timeout_secs: timeout,
        idle_timeout_secs: idle_timeout,
        max_turns,
        inventory: None,
        inherited_env: agent_setup.session_secret_env.clone(),
        progress_sink: cleave::progress::stdout_progress_sink(),
    };

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::warn!("Interrupted — cancelling cleave");
        cancel_clone.cancel();
    });

    let result =
        cleave::run_cleave(&plan, directive, &repo_path, workspace, &config, cancel).await?;

    // Print report
    eprintln!("\n## Cleave Report: {}", result.state.run_id);
    eprintln!("**Duration:** {:.0}s", result.duration_secs);
    eprintln!();

    let completed = result
        .state
        .children
        .iter()
        .filter(|c| c.status == cleave::state::ChildStatus::Completed)
        .count();
    let failed = result
        .state
        .children
        .iter()
        .filter(|c| c.status == cleave::state::ChildStatus::Failed)
        .count();
    eprintln!(
        "**Children:** {} completed, {} failed of {}",
        completed,
        failed,
        result.state.children.len()
    );
    eprintln!();

    for child in &result.state.children {
        let icon = match child.status {
            cleave::state::ChildStatus::Completed => "✓",
            cleave::state::ChildStatus::Failed => "✗",
            cleave::state::ChildStatus::Running => "⏳",
            cleave::state::ChildStatus::Pending => "○",
        };
        let dur = child
            .duration_secs
            .map(|d| format!(" ({:.0}s)", d))
            .unwrap_or_default();
        eprintln!("  {} **{}**{}: {:?}", icon, child.label, dur, child.status);
        if let Some(err) = &child.error {
            eprintln!("    Error: {}", err);
        }
    }

    eprintln!("\n### Merge Results");
    for (label, outcome) in &result.merge_results {
        match outcome {
            cleave::orchestrator::MergeOutcome::Success => eprintln!("  ✓ {} merged", label),
            cleave::orchestrator::MergeOutcome::Conflict(d) => {
                eprintln!("  ✗ {} CONFLICT: {}", label, d.lines().next().unwrap_or(""))
            }
            cleave::orchestrator::MergeOutcome::Failed(d) => {
                eprintln!("  ✗ {} FAILED: {}", label, d.lines().next().unwrap_or(""))
            }
            cleave::orchestrator::MergeOutcome::Skipped(reason) => {
                eprintln!("  ○ {} skipped ({})", label, reason)
            }
        }
    }

    // Post-merge guardrails (CLI only — TS wrapper runs its own)
    let all_merged = result
        .merge_results
        .iter()
        .all(|(_, o)| matches!(o, cleave::orchestrator::MergeOutcome::Success));
    if all_merged && failed == 0 {
        let checks = cleave::guardrails::discover_guardrails(&repo_path);
        if !checks.is_empty() {
            let report = cleave::guardrails::run_guardrails(&repo_path, &checks);
            eprintln!("\n### Post-Merge Guardrails\n{report}");
        }
    }

    // Exit with error if any children failed
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

async fn run_doctor_command(cli: &Cli) -> anyhow::Result<()> {
    let cwd = std::fs::canonicalize(&cli.cwd)?;
    let repo_root = setup::find_project_root(&cwd);
    let findings = lifecycle::doctor::audit_repo(&repo_root);

    if findings.is_empty() {
        println!("✓ No suspicious lifecycle drift found.");
        return Ok(());
    }

    println!("Lifecycle doctor: {} finding(s)\n", findings.len());
    for f in findings {
        println!("- {} [{}]", f.node_id, f.kind.as_str());
        println!("  {}", f.title);
        println!("  {}", f.detail);
    }
    Ok(())
}

async fn run_interactive_command(cli: &Cli) -> anyhow::Result<()> {
    tracing::info!(model = %cli.model, "omegon interactive starting");

    // Check .omegon-version — show in bootstrap panel (before TUI takes over stderr)
    if let Some(warning) = switch::check_version_file_warning(&cli.cwd) {
        eprintln!("{warning}");
    }

    // ─── Shared state (created early so features can reference it) ────
    let shared_settings = settings::shared(&cli.model);

    // Load project profile → apply to settings (model, thinking, max_turns)
    let profile = settings::Profile::load(&cli.cwd);
    if let Ok(mut s) = shared_settings.lock() {
        profile.apply_to(&mut s);
        // CLI flags override profile
        if cli.max_turns != 50 {
            // 50 is the default — only override if explicitly set
            s.max_turns = cli.max_turns;
        }
        tracing::info!(
            model = %s.model, thinking = %s.thinking.as_str(),
            max_turns = s.max_turns, "settings initialized from profile"
        );
    }

    // ─── Shared setup ───────────────────────────────────────────────────
    // Default: resume most recent session. --fresh overrides. --resume <id> pins a specific one.
    let resume: Option<Option<&str>> = if cli.fresh {
        None
    } else if let Some(ref r) = cli.resume {
        Some(r.as_deref())
    } else {
        Some(None) // try most recent
    };
    let mut agent = setup::AgentSetup::new(&cli.cwd, resume, Some(shared_settings.clone())).await?;

    // ─── LLM provider ──────────────────────────────────────────────────
    // Native Rust clients by default. --bridge flag forces the Node.js subprocess.
    // ─── LLM provider (native Rust clients only) ─────────────────────
    let requested_start_model = shared_settings
        .lock()
        .ok()
        .map(|s| s.model.clone())
        .unwrap_or_else(|| cli.model.clone());
    let resolved_cli_model = providers::resolve_execution_model_spec(&requested_start_model)
        .await
        .unwrap_or_else(|| requested_start_model.clone());
    if resolved_cli_model != requested_start_model {
        tracing::info!(requested = %requested_start_model, resolved = %resolved_cli_model, "resolved startup model to executable provider");
        if let Ok(mut s) = shared_settings.lock() {
            s.set_model(&resolved_cli_model);
        }
    }

    let mut provider_connected = true;
    let bridge: Box<dyn LlmBridge> = match providers::auto_detect_bridge(&resolved_cli_model).await
    {
        Some(native) => {
            tracing::info!("using native LLM provider");
            native
        }
        None => {
            tracing::warn!(
                "no LLM provider available — TUI will start but messages will fail until /login"
            );
            provider_connected = false;
            Box::new(bridge::NullBridge)
        }
    };
    // Update settings with provider status before TUI reads it
    if let Ok(mut s) = shared_settings.lock() {
        s.provider_connected = provider_connected;
    }
    let bridge: Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>> =
        Arc::new(tokio::sync::RwLock::new(bridge));

    // ─── Event channel ──────────────────────────────────────────────────
    let (events_tx, events_rx) = broadcast::channel::<AgentEvent>(256);
    let (command_tx, mut command_rx) = tokio::sync::mpsc::channel::<tui::TuiCommand>(16);
    let pending_compact = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let web_command_tx = command_tx.clone(); // For forwarding web dashboard commands

    // Broadcast initial HarnessStatus — bridges BusEvent (emitted in setup)
    // to AgentEvent (consumed by TUI + WebSocket)
    if let Ok(status_json) = serde_json::to_value(&agent.initial_harness_status) {
        let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json });
    }

    // ─── Shared state ─────────────────────────────────────────────────
    let shared_cancel: tui::SharedCancel = std::sync::Arc::new(std::sync::Mutex::new(None));

    // ─── Probe provider for authoritative model limits ──────────────
    // The route matrix is a static fallback. The /v1/models endpoint
    // returns the real context window for the selected model.
    {
        let selected_model = shared_settings
            .lock()
            .ok()
            .map(|s| s.model.clone())
            .unwrap_or_else(|| resolved_cli_model.clone());
        let model_id = selected_model
            .split_once(':')
            .map(|(_, model)| model)
            .unwrap_or(&selected_model);
        let provider = crate::providers::infer_provider_id(&selected_model);
        if provider == "anthropic" {
            if let Some(limits) = auth::probe_anthropic_model_limits(model_id).await {
                if let Ok(mut s) = shared_settings.lock() {
                    let old = s.context_window;
                    s.context_window = limits.max_input_tokens;
                    s.context_class = settings::ContextClass::from_tokens(limits.max_input_tokens);
                    if old != limits.max_input_tokens {
                        tracing::info!(
                            old,
                            new = limits.max_input_tokens,
                            "context window updated from /v1/models"
                        );
                    }
                }
            }
        }
    }

    let is_oauth = shared_settings
        .lock()
        .ok()
        .map(|s| crate::providers::infer_provider_id(&s.model))
        .and_then(|provider| providers::resolve_api_key_sync(&provider))
        .is_some_and(|(_, oauth)| oauth);

    // ─── Apply CLI overrides ──────────────────────────────────────────
    if let Some(ref class_str) = cli.context_class {
        if let Ok(mut s) = shared_settings.lock() {
            match class_str.to_lowercase().as_str() {
                "squad" => {
                    s.context_class = settings::ContextClass::Squad;
                    s.context_window = 200_000;
                }
                "maniple" => {
                    s.context_class = settings::ContextClass::Maniple;
                    s.context_window = 500_000;
                }
                "clan" => {
                    s.context_class = settings::ContextClass::Clan;
                    s.context_window = 680_000;
                }
                "legion" => {
                    s.context_class = settings::ContextClass::Legion;
                    s.context_window = 1_000_000;
                }
                _ => tracing::warn!("Unknown context class: {class_str}"),
            }
            s.apply_context_mode();
            tracing::info!(class = %class_str, window = s.context_window, "context class override applied");
        }
    }

    // ─── Launch TUI ─────────────────────────────────────────────────────
    let initial = agent.initial_tui_state();
    // Extract bus command definitions for the TUI command palette
    let bus_commands: Vec<omegon_traits::CommandDefinition> = agent
        .bus
        .command_definitions()
        .iter()
        .map(|(_, def)| def.clone())
        .collect();

    // Resolve initial prompt (--initial-prompt or --initial-prompt-file)
    let initial_prompt = match (&cli.initial_prompt, &cli.initial_prompt_file) {
        (Some(p), _) => Some(p.clone()),
        (_, Some(path)) => std::fs::read_to_string(path).ok(),
        _ => None,
    };

    let tui_config = tui::TuiConfig {
        cwd: agent.cwd.to_string_lossy().to_string(),
        is_oauth,
        initial,
        no_splash: cli.no_splash,
        bus_commands,
        dashboard_handles: agent.dashboard_handles.clone(),
        initial_prompt,
        start_tutorial: cli.tutorial,
        resume_info: agent.resume_info.clone(),
    };
    let tui_cancel = shared_cancel.clone();
    let tui_settings = shared_settings.clone();
    let tui_handle = tokio::spawn(async move {
        if let Err(e) =
            tui::run_tui(events_rx, command_tx, tui_config, tui_cancel, tui_settings).await
        {
            tracing::error!("TUI error: {e}");
        }
    });

    // ─── Emit session start to bus features ────────────────────────────
    agent.bus.emit(&omegon_traits::BusEvent::SessionStart {
        cwd: agent.cwd.clone(),
        session_id: "interactive".into(),
    });
    // Drain any requests from session_start handlers
    for request in agent.bus.drain_requests() {
        if let omegon_traits::BusRequest::Notify { message, .. } = request {
            let _ = events_tx.send(AgentEvent::SystemNotification { message });
        }
    }

    // ─── Interactive agent loop ─────────────────────────────────────────
    loop {
        let cmd = match command_rx.recv().await {
            Some(cmd) => cmd,
            None => break,
        };

        match cmd {
            tui::TuiCommand::Quit => break,

            tui::TuiCommand::SetModel(model) => {
                tracing::info!(model = %model, "model switched via /model command");

                let requested_model = model.clone();
                let effective_model = providers::resolve_execution_model_spec(&requested_model)
                    .await
                    .unwrap_or_else(|| requested_model.clone());

                // Detect provider change — swap bridge if needed
                let (old_model, old_provider) = shared_settings
                    .lock()
                    .ok()
                    .map(|s| {
                        (
                            s.model.clone(),
                            crate::providers::infer_provider_id(&s.model),
                        )
                    })
                    .unwrap_or_else(|| (String::new(), String::new()));
                let new_provider = crate::providers::infer_provider_id(&effective_model);

                if let Ok(mut s) = shared_settings.lock() {
                    s.set_model(&effective_model);
                    // Persist to project profile
                    let mut profile = settings::Profile::load(&agent.cwd);
                    profile.capture_from(&s);
                    let _ = profile.save(&agent.cwd);
                }

                if effective_model != requested_model {
                    let provider_label = crate::auth::provider_by_id(&new_provider)
                        .map(|p| p.display_name)
                        .unwrap_or(new_provider.as_str());
                    let _ = events_tx.send(AgentEvent::SystemNotification {
                        message: format!(
                            "Requested {requested_model}; using executable route {effective_model} via {provider_label}."
                        ),
                    });
                }

                // If provider changed, re-detect and hot-swap the bridge
                if old_provider != new_provider {
                    tracing::info!(
                        old = %old_provider, new = %new_provider,
                        "provider changed — re-detecting bridge"
                    );
                    // Bridge swap is awaited (not spawned) to prevent a race
                    // where the user sends a message before the new bridge is
                    // installed, causing the old provider to receive requests
                    // with the new model name.
                    let provider = crate::providers::infer_provider_id(&effective_model);
                    if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await
                    {
                        let mut guard = bridge.write().await;
                        *guard = new_bridge;
                        if let Ok(mut s) = shared_settings.lock() {
                            s.provider_connected = true;
                        }
                        let provider_label = crate::auth::provider_by_id(&provider)
                            .map(|p| p.display_name)
                            .unwrap_or(provider.as_str());
                        tracing::info!("bridge hot-swapped for provider {}", provider);
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: format!(
                                "Provider switched to {provider_label} ({effective_model})."
                            ),
                        });
                    } else {
                        if let Ok(mut s) = shared_settings.lock() {
                            s.provider_connected = false;
                        }
                        let provider_label = crate::auth::provider_by_id(&provider)
                            .map(|p| p.display_name)
                            .unwrap_or(provider.as_str());
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: format!(
                                "⚠ No credentials for {provider_label}. Use /login to authenticate."
                            ),
                        });
                    }
                } else if old_model != effective_model {
                    let provider_label = crate::auth::provider_by_id(&new_provider)
                        .map(|p| p.display_name)
                        .unwrap_or(new_provider.as_str());
                    let _ = events_tx.send(AgentEvent::SystemNotification {
                        message: format!(
                            "Model switched to {effective_model} via {provider_label}."
                        ),
                    });
                }
            }

            tui::TuiCommand::Compact => {
                tracing::info!("manual compaction requested");

                let bridge_guard = bridge.read().await;
                let stream_options = {
                    let s = shared_settings.lock().unwrap();
                    crate::bridge::StreamOptions {
                        model: Some(s.model.clone()),
                        reasoning: Some(s.thinking.as_str().to_string()),
                        extended_context: false,
                    }
                };
                if let Some((payload, _evict_count)) = agent.conversation.build_compaction_payload()
                {
                    match r#loop::compact_via_llm(bridge_guard.as_ref(), &payload, &stream_options)
                        .await
                    {
                        Ok(summary) => {
                            agent.conversation.apply_compaction(summary);
                            let est = agent.conversation.estimate_tokens();
                            if let Ok(s) = shared_settings.lock() {
                                let ctx_window = s.context_window;
                                if ctx_window > 0 {
                                    let _ = events_tx.send(AgentEvent::TurnEnd {
                                        turn: agent.conversation.intent.stats.turns,
                                        estimated_tokens: est,
                                    });
                                }
                            }
                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                message: "Compaction completed immediately.".into(),
                            });
                        }
                        Err(e) => {
                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                message: format!("Compaction failed: {e}"),
                            });
                        }
                    }
                } else {
                    let _ = events_tx.send(AgentEvent::SystemNotification {
                        message: "Nothing eligible to compact yet.".into(),
                    });
                }
            }

            tui::TuiCommand::ListSessions => {
                let sessions = session::list_sessions(&agent.cwd);
                let text = if sessions.is_empty() {
                    "No saved sessions for this directory.".to_string()
                } else {
                    let lines: Vec<String> = sessions
                        .iter()
                        .take(10)
                        .map(|s| {
                            format!(
                                "  {} — {} turns, {} tools — {}",
                                s.meta.session_id,
                                s.meta.turns,
                                s.meta.tool_calls,
                                s.meta.last_prompt_snippet
                            )
                        })
                        .collect();
                    format!("Recent sessions:\n{}", lines.join("\n"))
                };
                // Send back to TUI as a system message
                let _ = events_tx.send(AgentEvent::AgentEnd);
                tracing::info!("{text}");
            }

            tui::TuiCommand::NewSession => {
                // Save the current session before resetting
                if !cli.no_session {
                    let rid = agent.resume_info.as_ref().map(|r| r.session_id.as_str());
                    let _ = session::save_session(&agent.conversation, &agent.cwd, rid);
                }
                agent.conversation = crate::conversation::ConversationState::new();
                agent.resume_info = None;
                let _ = events_tx.send(AgentEvent::SessionReset);
            }

            tui::TuiCommand::StartWebDashboard => {
                let web_state = web::WebState::with_auth_state(
                    agent.dashboard_handles.clone(),
                    events_tx.clone(),
                    agent.web_auth_state.clone(),
                );
                let token = web_state.web_auth.issue_query_token();
                match web::start_server(web_state, 7842).await {
                    Ok((addr, web_cmd_rx)) => {
                        let url = format!("http://{addr}/?token={token}");
                        tui::open_browser(&url);
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: format!("Dashboard started at {url}"),
                        });
                        // Spawn a task to forward web commands into the main TUI command channel
                        let cmd_tx_clone = web_command_tx.clone();
                        let cancel_clone = shared_cancel.clone();
                        tokio::spawn(async move {
                            let mut rx = web_cmd_rx;
                            while let Some(web_cmd) = rx.recv().await {
                                let tui_cmd = match web_cmd {
                                    web::WebCommand::UserPrompt(text) => {
                                        tui::TuiCommand::UserPrompt(text)
                                    }
                                    web::WebCommand::SlashCommand { name, args } => {
                                        tui::TuiCommand::BusCommand { name, args }
                                    }
                                    web::WebCommand::Cancel => {
                                        if let Ok(guard) = cancel_clone.lock()
                                            && let Some(ref cancel) = *guard
                                        {
                                            cancel.cancel();
                                        }
                                        continue;
                                    }
                                };
                                if cmd_tx_clone.send(tui_cmd).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }
                    Err(e) => {
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: format!("Failed to start dashboard: {e}"),
                        });
                    }
                }
            }

            tui::TuiCommand::BusCommand { name, args } => {
                // Handle special auth commands directly
                if name == "secrets" {
                    let parts: Vec<&str> = args.splitn(3, ' ').collect();
                    let message = match parts.first().copied().unwrap_or("") {
                        "list" | "" => {
                            let names = agent.secrets.list_recipes();
                            let mut out = String::new();
                            if names.is_empty() {
                                out.push_str("No secrets stored.\n");
                            } else {
                                out.push_str(&format!("🔐 Secrets ({})\n\n", names.len()));
                                for (name, recipe) in &names {
                                    out.push_str(&format!("  {name:<24} {recipe}\n"));
                                }
                                out.push('\n');
                            }
                            out.push_str("Common secrets:\n");
                            out.push_str("  /secrets set GITHUB_TOKEN cmd:gh auth token    always fresh from CLI\n");
                            out.push_str("  /secrets set NPM_TOKEN cmd:npm token get       always fresh from CLI\n");
                            out.push_str("  /secrets set AWS_SECRET env:AWS_SECRET_ACCESS_KEY  from environment\n\n");
                            out.push_str("API keys (no CLI available — store directly):\n");
                            out.push_str(
                                "  /secrets set OPENROUTER_KEY sk-or-...          free cloud AI\n",
                            );
                            out.push_str("  /secrets set ANTHROPIC_API_KEY sk-ant-...      Anthropic API\n\n");
                            out.push_str("Retrieve or remove:\n");
                            out.push_str("  /secrets get GITHUB_TOKEN\n");
                            out.push_str("  /secrets delete GITHUB_TOKEN");
                            out
                        }
                        "set" => {
                            if parts.len() < 3 {
                                "Usage: /secrets set NAME VALUE\n\n\
                                 Dynamic (preferred — always fresh):\n\
                                 \x20 /secrets set GITHUB_TOKEN cmd:gh auth token\n\
                                 \x20 /secrets set NPM_TOKEN cmd:npm token get\n\
                                 \x20 /secrets set K8S_TOKEN cmd:kubectl get secret...\n\n\
                                 From environment:\n\
                                 \x20 /secrets set AWS_SECRET env:AWS_SECRET_ACCESS_KEY\n\n\
                                 Direct value (only when no CLI exists):\n\
                                 \x20 /secrets set OPENROUTER_KEY sk-or-v1-abc..."
                                    .into()
                            } else {
                                let secret_name = parts[1];
                                let secret_value = parts[2];
                                let result = if secret_value.contains(':')
                                    && ["env:", "cmd:", "vault:", "keyring:", "file:"]
                                        .iter()
                                        .any(|p| secret_value.starts_with(p))
                                {
                                    agent.secrets.set_recipe(secret_name, secret_value)
                                } else {
                                    agent.secrets.set_keyring_secret(secret_name, secret_value)
                                };
                                match result {
                                    Ok(()) => format!(
                                        "✓ Secret '{secret_name}' stored (encrypted in OS keyring).\n  The agent will redact this value from all output."
                                    ),
                                    Err(e) => format!("Error storing secret: {e}"),
                                }
                            }
                        }
                        "get" => {
                            if parts.len() < 2 {
                                "Usage: /secrets get NAME".into()
                            } else {
                                let secret_name = parts[1];
                                match agent.secrets.resolve(secret_name) {
                                    Some(val) => format!("🔓 {secret_name} = {val}"),
                                    None => format!(
                                        "Secret '{secret_name}' not found.\n  Use /secrets to see stored secrets."
                                    ),
                                }
                            }
                        }
                        "delete" => {
                            if parts.len() < 2 {
                                "Usage: /secrets delete NAME".into()
                            } else {
                                let secret_name = parts[1];
                                match agent.secrets.delete_recipe(secret_name) {
                                    Ok(()) => format!("✓ Secret '{secret_name}' deleted."),
                                    Err(e) => format!("Error: {e}"),
                                }
                            }
                        }
                        sub => format!("Unknown: /secrets {sub}\n\nType /secrets to see usage."),
                    };
                    let _ = events_tx.send(AgentEvent::SystemNotification { message });
                } else if name.starts_with("auth_") {
                    match name.as_str() {
                        "auth_status" => {
                            let status = auth::probe_all_providers().await;
                            let message = format_auth_status(&status);
                            let _ = events_tx.send(AgentEvent::SystemNotification { message });
                        }
                        "auth_login" => {
                            let provider = args.trim();
                            let provider = if provider.is_empty() {
                                "anthropic"
                            } else {
                                provider
                            };

                            // Run the login in a background task. Progress updates go
                            // through SystemNotification instead of eprintln (which
                            // would corrupt the ratatui display).
                            let events_tx_clone = events_tx.clone();
                            let progress_tx = events_tx.clone();
                            let provider_clone = provider.to_string();
                            let bridge_clone = bridge.clone();
                            let model_for_redetect = shared_settings
                                .lock()
                                .ok()
                                .map(|s| s.model.clone())
                                .unwrap_or_else(|| cli.model.clone());
                            let settings_for_login = shared_settings.clone();
                            tokio::spawn(async move {
                                let progress: auth::LoginProgress = Box::new(move |msg| {
                                    let _ = progress_tx.send(AgentEvent::SystemNotification {
                                        message: msg.to_string(),
                                    });
                                });
                                let result = match provider_clone.as_str() {
                                    "anthropic" | "claude" => {
                                        auth::login_anthropic_with_progress(progress).await
                                    }
                                    "openai-codex" | "chatgpt" | "codex" => {
                                        auth::login_openai_with_progress(progress).await
                                    }
                                    "openai" => Err(anyhow::anyhow!(
                                        "OpenAI API login in the TUI uses hidden API-key entry. Run /login and choose OpenAI API, or set OPENAI_API_KEY."
                                    )),
                                    _ => Err(anyhow::anyhow!(
                                        "Unknown provider: {}. Use: anthropic, openai, openai-codex",
                                        provider_clone
                                    )),
                                };
                                let provider_label = crate::auth::provider_by_id(&provider_clone)
                                    .map(|p| p.display_name)
                                    .unwrap_or(provider_clone.as_str())
                                    .to_string();
                                let message = match &result {
                                    Ok(_) => {
                                        format!("✓ Successfully logged in to {provider_label}")
                                    }
                                    Err(e) => format!("❌ Login failed: {}", e),
                                };
                                let _ = events_tx_clone
                                    .send(AgentEvent::SystemNotification { message });

                                // Hot-swap the bridge after login succeeds using the current model intent.
                                if result.is_ok() {
                                    let effective_model = providers::resolve_execution_model_spec(
                                        &model_for_redetect,
                                    )
                                    .await
                                    .unwrap_or(model_for_redetect.clone());
                                    if let Some(new_bridge) =
                                        providers::auto_detect_bridge(&effective_model).await
                                    {
                                        let mut guard = bridge_clone.write().await;
                                        *guard = new_bridge;
                                        if let Ok(mut s) = settings_for_login.lock() {
                                            s.set_model(&effective_model);
                                            s.provider_connected = true;
                                        }
                                        tracing::info!("bridge hot-swapped after successful login");
                                        let _ =
                                            events_tx_clone.send(AgentEvent::SystemNotification {
                                                message: format!(
                                                    "Provider connected — active route {}.",
                                                    effective_model
                                                ),
                                            });
                                    }
                                }
                            });
                        }
                        "auth_logout" => {
                            let provider = args.trim();
                            if provider.is_empty() {
                                let _ = events_tx.send(AgentEvent::SystemNotification {
                                    message: "Error: Provider required for logout".to_string(),
                                });
                            } else {
                                let message = match auth::logout_provider(provider) {
                                    Ok(()) => format!("✓ Logged out from {}", provider),
                                    Err(e) => format!("❌ Logout failed: {}", e),
                                };
                                let _ = events_tx.send(AgentEvent::SystemNotification { message });
                            }
                        }
                        "auth_unlock" => {
                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                message: "🔒 Secrets store unlock not yet implemented".to_string(),
                            });
                        }
                        _ => {
                            // Unknown auth command - fall through to bus
                            let result = agent.bus.dispatch_command(&name, &args);
                            match result {
                                omegon_traits::CommandResult::Display(msg) => {
                                    let _ = events_tx
                                        .send(AgentEvent::SystemNotification { message: msg });
                                }
                                omegon_traits::CommandResult::Handled => {
                                    tracing::debug!(cmd = %name, "bus command handled silently");
                                }
                                omegon_traits::CommandResult::NotHandled => {
                                    tracing::warn!(cmd = %name, "bus command not handled by any feature");
                                }
                            }
                        }
                    }
                } else {
                    // Regular bus command
                    let result = agent.bus.dispatch_command(&name, &args);
                    match result {
                        omegon_traits::CommandResult::Display(msg) => {
                            // Send back to TUI as a system notification (not into LLM conversation)
                            let _ = events_tx.send(AgentEvent::SystemNotification { message: msg });
                        }
                        omegon_traits::CommandResult::Handled => {
                            tracing::debug!(cmd = %name, "bus command handled silently");
                        }
                        omegon_traits::CommandResult::NotHandled => {
                            tracing::warn!(cmd = %name, "bus command not handled by any feature");
                        }
                    }
                }
                // Drain any requests generated by the command
                for request in agent.bus.drain_requests() {
                    match request {
                        omegon_traits::BusRequest::Notify { message, .. } => {
                            let _ = events_tx.send(AgentEvent::SystemNotification { message });
                        }
                        omegon_traits::BusRequest::InjectSystemMessage { content } => {
                            agent.conversation.push_user(format!("[System: {content}]"));
                        }
                        omegon_traits::BusRequest::RequestCompaction => {
                            tracing::info!("Bus: compaction requested");
                        }
                        omegon_traits::BusRequest::RefreshHarnessStatus => {
                            // Re-assemble and broadcast
                            let status = crate::status::HarnessStatus::assemble();
                            if let Ok(json) = serde_json::to_value(&status) {
                                let _ = events_tx
                                    .send(AgentEvent::HarnessStatusChanged { status_json: json });
                            }
                        }
                    }
                }
            }

            tui::TuiCommand::UserPromptWithImages(text, image_paths) => {
                // Encode images and attach to the next LLM call
                let mut images = Vec::new();
                for path in &image_paths {
                    if let Ok(data) = std::fs::read(path) {
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
                        let media_type = match ext {
                            "jpg" | "jpeg" => "image/jpeg",
                            "gif" => "image/gif",
                            "webp" => "image/webp",
                            "bmp" => "image/bmp",
                            "tiff" | "tif" => "image/tiff",
                            _ => "image/png",
                        };
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        images.push(crate::bridge::ImageAttachment {
                            data: b64,
                            media_type: media_type.to_string(),
                        });
                    }
                }
                // Push user text (images go through the LLM message separately)
                agent.conversation.push_user(text.clone());
                // Store images for the next LLM call
                agent.conversation.pending_images = images;

                // Read current settings for this turn
                let (model, max_turns) = {
                    let s = shared_settings.lock().unwrap();
                    (s.model.clone(), s.max_turns)
                };

                let loop_config = r#loop::LoopConfig {
                    max_turns,
                    soft_limit_turns: if max_turns > 0 { max_turns * 2 / 3 } else { 0 },
                    max_retries: cli.max_retries,
                    retry_delay_ms: 2000,
                    model,
                    cwd: agent.cwd.clone(),
                    extended_context: false,
                    settings: Some(shared_settings.clone()),
                    secrets: Some(agent.secrets.clone()),
                    force_compact: Some(pending_compact.clone()),
                };

                let cancel = CancellationToken::new();
                if let Ok(mut guard) = shared_cancel.lock() {
                    *guard = Some(cancel.clone());
                }

                let bridge_guard = bridge.read().await;
                if let Err(e) = r#loop::run(
                    bridge_guard.as_ref(),
                    &mut agent.bus,
                    &mut agent.context_manager,
                    &mut agent.conversation,
                    &events_tx,
                    cancel,
                    &loop_config,
                )
                .await
                {
                    drop(bridge_guard); // release before error handling
                    let user_msg = format_agent_error(&e);
                    tracing::error!("Agent loop error: {e}");
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: user_msg });
                    let _ = events_tx.send(AgentEvent::AgentEnd);
                }

                if let Ok(mut guard) = shared_cancel.lock() {
                    guard.take();
                }
            }

            tui::TuiCommand::UserPrompt(text) => {
                agent.conversation.push_user(text);

                // Read current settings for this turn
                let (model, max_turns) = {
                    let s = shared_settings.lock().unwrap();
                    (s.model.clone(), s.max_turns)
                };

                let loop_config = r#loop::LoopConfig {
                    max_turns,
                    soft_limit_turns: if max_turns > 0 { max_turns * 2 / 3 } else { 0 },
                    max_retries: cli.max_retries,
                    retry_delay_ms: 2000,
                    model,
                    cwd: agent.cwd.clone(),
                    extended_context: false,
                    settings: Some(shared_settings.clone()),
                    secrets: Some(agent.secrets.clone()),
                    force_compact: Some(pending_compact.clone()),
                };

                let cancel = CancellationToken::new();
                if let Ok(mut guard) = shared_cancel.lock() {
                    *guard = Some(cancel.clone());
                }

                let bridge_guard = bridge.read().await;
                if let Err(e) = r#loop::run(
                    bridge_guard.as_ref(),
                    &mut agent.bus,
                    &mut agent.context_manager,
                    &mut agent.conversation,
                    &events_tx,
                    cancel,
                    &loop_config,
                )
                .await
                {
                    drop(bridge_guard);
                    // Surface a concise error to the user, not the raw JSON blob
                    let user_msg = format_agent_error(&e);
                    tracing::error!("Agent loop error: {e}");
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: user_msg });
                    // The loop emits AgentEnd on success but not on error —
                    // emit it here so the TUI exits the "working" state.
                    let _ = events_tx.send(AgentEvent::AgentEnd);
                }

                if let Ok(mut guard) = shared_cancel.lock() {
                    guard.take();
                }
            }
        }
    }

    // Save session + profile
    if !cli.no_session
        && let Err(e) = session::save_session(
            &agent.conversation,
            &agent.cwd,
            agent.resume_info.as_ref().map(|r| r.session_id.as_str()),
        )
    {
        tracing::debug!("Session save failed: {e}");
    }
    // Always persist profile on exit (captures thinking level changes, etc.)
    if let Ok(s) = shared_settings.lock() {
        let mut profile = settings::Profile::load(&agent.cwd);
        profile.capture_from(&s);
        let _ = profile.save(&agent.cwd);
    }

    bridge.read().await.shutdown().await;
    tui_handle.abort();
    Ok(())
}

/// Format an agent loop error into a concise user-facing message.
/// Extracts the meaningful part from API error JSON blobs.
fn format_agent_error(e: &anyhow::Error) -> String {
    let raw = format!("{e}");
    // Try to extract the "message" field from Anthropic/OpenAI error JSON
    if let Some(start) = raw.find("\"message\":\"") {
        let rest = &raw[start + 11..];
        if let Some(end) = rest.find('"') {
            return format!("⚠ API error: {}", &rest[..end]);
        }
    }
    // Try to extract a status code
    if let Some(start) = raw.find("status=") {
        let rest = &raw[start..];
        if let Some(end) = rest.find(' ') {
            return format!("⚠ {}", &rest[..end.min(40)]);
        }
    }
    // Fallback: truncate
    let truncated = crate::util::truncate_str(&raw, 500);
    format!("⚠ {truncated}")
}

async fn run_smoke_command(cli: &Cli) -> anyhow::Result<()> {
    eprintln!("omegon {} — smoke test mode", env!("CARGO_PKG_VERSION"));

    // ─── LLM provider (native Rust clients only) ─────────────────────
    let bridge: Box<dyn bridge::LlmBridge> = match providers::auto_detect_bridge(&cli.model).await {
        Some(native) => native,
        None => {
            anyhow::bail!(
                "No LLM provider available. Set ANTHROPIC_API_KEY or another provider credential."
            );
        }
    };
    let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(bridge));

    let exit_code = smoke::run(bridge).await;
    std::process::exit(exit_code);
}

async fn run_agent_command(cli: &Cli) -> anyhow::Result<()> {
    tracing::info!(model = %cli.model, "omegon-agent starting");

    // Resolve prompt from --prompt or --prompt-file
    let prompt_text = match (&cli.prompt, &cli.prompt_file) {
        (Some(p), _) => p.clone(),
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read prompt file {}: {}", path.display(), e))?,
        (None, None) => {
            eprintln!("Usage: omegon-agent --prompt \"<task>\" [--cwd <path>]");
            eprintln!("       omegon-agent --prompt-file <path> [--cwd <path>]");
            eprintln!(
                "       omegon-agent cleave --plan <plan.json> --directive \"<task>\" --workspace <dir>"
            );
            eprintln!();
            eprintln!("Headless coding agent — executes a task and exits.");
            std::process::exit(1);
        }
    };

    // ─── Shared setup ───────────────────────────────────────────────────
    let shared_settings = settings::shared(&cli.model);
    let profile = settings::Profile::load(&cli.cwd);
    if let Ok(mut s) = shared_settings.lock() {
        profile.apply_to(&mut s);
        if cli.max_turns != 50 {
            s.max_turns = cli.max_turns;
        }
    }

    let resume = cli.resume.as_ref().map(|r| r.as_deref());
    let mut agent = setup::AgentSetup::new(&cli.cwd, resume, Some(shared_settings.clone())).await?;
    agent.conversation.push_user(prompt_text.clone());

    // ─── Build loop config ──────────────────────────────────────────────
    let loop_config = r#loop::LoopConfig {
        max_turns: cli.max_turns,
        soft_limit_turns: if cli.max_turns > 0 {
            cli.max_turns * 2 / 3
        } else {
            0
        },
        max_retries: cli.max_retries,
        retry_delay_ms: 2000,
        model: cli.model.clone(),
        cwd: agent.cwd.clone(),
        extended_context: false, // headless uses standard context
        settings: Some(shared_settings.clone()),
        secrets: Some(agent.secrets.clone()),
        force_compact: None,
    };

    // ─── LLM provider (native Rust clients only) ─────────────────────
    let bridge: Box<dyn LlmBridge> = match providers::auto_detect_bridge(&cli.model).await {
        Some(native) => {
            tracing::info!("using native LLM provider");
            native
        }
        None => {
            anyhow::bail!(
                "No LLM provider available. Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or another provider credential."
            );
        }
    };

    // ─── Event channel ──────────────────────────────────────────────────
    let (events_tx, mut events_rx) = broadcast::channel::<AgentEvent>(256);

    // ─── Event printer (headless mode: print to stderr) ─────────────────
    tokio::spawn(async move {
        while let Ok(event) = events_rx.recv().await {
            match event {
                AgentEvent::TurnStart { turn } => {
                    tracing::info!("── Turn {turn} ──");
                }
                AgentEvent::MessageChunk { text } => {
                    eprint!("{text}");
                }
                AgentEvent::ThinkingChunk { text } => {
                    eprint!("\x1b[2m{text}\x1b[0m");
                }
                AgentEvent::ToolStart { name, .. } => {
                    tracing::info!("→ {name}");
                }
                AgentEvent::ToolEnd {
                    id: _,
                    result,
                    is_error,
                } => {
                    let status = if is_error { "✗" } else { "✓" };
                    let text = result
                        .content
                        .first()
                        .map(|c| match c {
                            omegon_traits::ContentBlock::Text { text } => {
                                if text.len() > 200 {
                                    crate::util::truncate(&text, 200)
                                } else {
                                    text.clone()
                                }
                            }
                            omegon_traits::ContentBlock::Image { .. } => "[image]".into(),
                        })
                        .unwrap_or_default();
                    tracing::info!("  {status} {text}");
                }
                AgentEvent::TurnEnd { turn, .. } => {
                    tracing::info!("── Turn {turn} complete ──");
                }
                AgentEvent::AgentEnd => {
                    tracing::info!("Agent complete");
                }
                _ => {}
            }
        }
    });

    // ─── Run the loop ───────────────────────────────────────────────────
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::warn!("Interrupted — cancelling");
        cancel_clone.cancel();
    });

    let result = r#loop::run(
        bridge.as_ref(),
        &mut agent.bus,
        &mut agent.context_manager,
        &mut agent.conversation,
        &events_tx,
        cancel,
        &loop_config,
    )
    .await;

    // ─── Save session ────────────────────────────────────────────────────
    if !cli.no_session {
        if agent.cwd.join(".cleave-prompt.md").exists() {
            // Cleave child: save to worktree-local file
            let session_path = agent.cwd.join(".cleave-session.json");
            if let Err(e) = agent.conversation.save_session(&session_path) {
                tracing::debug!("Cleave session save failed (non-fatal): {e}");
            }
        } else {
            // Standalone agent: save to ~/.config/omegon/sessions/
            match session::save_session(
                &agent.conversation,
                &agent.cwd,
                agent.resume_info.as_ref().map(|r| r.session_id.as_str()),
            ) {
                Ok(path) => tracing::info!(path = %path.display(), "Session saved"),
                Err(e) => tracing::debug!("Session save failed (non-fatal): {e}"),
            }
        }
    }

    // Graceful bridge shutdown
    bridge.shutdown().await;

    match &result {
        Ok(()) => {
            if let Some(last_text) = agent.conversation.last_assistant_text() {
                println!("{last_text}");
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }

    result
}

async fn run_auth_command(action: &AuthAction) -> anyhow::Result<()> {
    match action {
        AuthAction::Status => {
            let status = auth::probe_all_providers().await;
            println!("{}", format_auth_status(&status));
            Ok(())
        }
        AuthAction::Login { provider } => run_auth_login(provider).await,
        AuthAction::Logout { provider } => match auth::logout_provider(provider) {
            Ok(()) => {
                println!("✓ Logged out from {provider}");
                Ok(())
            }
            Err(e) => {
                eprintln!("Logout failed: {e}");
                std::process::exit(1);
            }
        },
        AuthAction::Unlock => {
            // TODO: Implement secrets store unlock
            eprintln!("Secrets store unlock not yet implemented");
            std::process::exit(1);
        }
    }
}

/// Direct API key login — for providers without OAuth (OpenRouter, etc.)
/// Prompts for the key on stdin, stores in auth.json.
async fn login_api_key(
    provider: &str,
    env_var: &str,
    keys_url: &str,
) -> anyhow::Result<auth::OAuthCredentials> {
    eprintln!("Login to {provider}:");
    eprintln!("  1. Open {keys_url}");
    eprintln!("  2. Create or copy your API key");
    eprintln!("  3. Paste it below (input is hidden)");
    eprintln!();
    eprint!("API key: ");

    // Read key without echo (rpassword hides input on TTYs)
    let key = rpassword::read_password().unwrap_or_else(|_| {
        // Fallback for non-TTY (piped input, CI)
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf).unwrap_or(0);
        buf.trim().to_string()
    });

    if key.is_empty() {
        anyhow::bail!("No API key provided");
    }

    let creds = auth::OAuthCredentials {
        cred_type: "api-key".into(),
        access: key,
        refresh: String::new(),
        expires: u64::MAX, // API keys don't expire
    };
    auth::write_credentials(provider, &creds)?;

    // Also set the env var for the current session so the provider resolves immediately
    // SAFETY: single-threaded at this point in startup — no other threads reading env vars
    unsafe {
        std::env::set_var(env_var, &creds.access);
    }

    eprintln!("✓ {provider} API key stored. Active for this session and future sessions.");
    Ok(creds)
}

async fn run_auth_login(provider: &str) -> anyhow::Result<()> {
    let result = match provider {
        "anthropic" | "claude" => auth::login_anthropic().await,
        "openai-codex" | "chatgpt" | "codex" => auth::login_openai().await,
        "openai" => {
            login_api_key(
                "openai",
                "OPENAI_API_KEY",
                "https://platform.openai.com/api-keys",
            )
            .await
        }
        "openrouter" => {
            login_api_key(
                "openrouter",
                "OPENROUTER_API_KEY",
                "https://openrouter.ai/keys",
            )
            .await
        }
        _ => {
            eprintln!(
                "Unknown provider: {provider}. Use: anthropic, openai, openai-codex, openrouter"
            );
            std::process::exit(1);
        }
    };
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Login failed: {e}");
            std::process::exit(1);
        }
    }
}

fn format_auth_status(status: &auth::AuthStatus) -> String {
    let mut lines = vec!["Authentication Status:".to_string()];

    for provider in &status.providers {
        let icon = match provider.status {
            auth::ProviderAuthStatus::Authenticated => "✓",
            auth::ProviderAuthStatus::Expired => "⚠",
            auth::ProviderAuthStatus::Missing => "✗",
            auth::ProviderAuthStatus::Error => "❌",
        };

        let auth_type = if provider.is_oauth {
            "oauth"
        } else {
            "api-key"
        };
        let display_name = auth::provider_by_id(&provider.name)
            .map(|p| p.display_name)
            .unwrap_or(provider.name.as_str());
        let mut line = format!("  {icon} {:<16} {auth_type}", display_name);

        if let Some(ref details) = provider.details {
            line.push_str(&format!(" ({details})"));
        }

        lines.push(line);
    }

    if !status.vault.is_empty() || !status.secrets.is_empty() || !status.mcp.is_empty() {
        lines.push(String::new());

        if !status.vault.is_empty() {
            lines.push("Vault:".to_string());
            for vault_info in &status.vault {
                lines.push(format!(
                    "  {} {}",
                    if vault_info.accessible { "✓" } else { "✗" },
                    vault_info.addr
                ));
            }
        }

        if !status.secrets.is_empty() {
            lines.push("Secrets Store:".to_string());
            for secret_info in &status.secrets {
                lines.push(format!(
                    "  {} {}",
                    if secret_info.unlocked { "🔓" } else { "🔒" },
                    secret_info.store
                ));
            }
        }

        if !status.mcp.is_empty() {
            lines.push("MCP Servers:".to_string());
            for mcp_info in &status.mcp {
                lines.push(format!(
                    "  {} {}",
                    if mcp_info.connected { "✓" } else { "✗" },
                    mcp_info.server
                ));
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_agent_error_extracts_message() {
        let raw = r#"Anthropic 400 Bad Request: {"type":"error","error":{"type":"invalid_request_error","message":"Input should be a valid dictionary"}}"#;
        let e = anyhow::anyhow!("{raw}");
        let result = format_agent_error(&e);
        assert!(
            result.contains("Input should be a valid dictionary"),
            "got: {result}"
        );
    }

    #[test]
    fn format_agent_error_truncates_long() {
        let long = "x".repeat(500);
        let e = anyhow::anyhow!("{long}");
        let result = format_agent_error(&e);
        assert!(
            result.len() < 600,
            "should truncate, got len {}",
            result.len()
        );
    }

    #[test]
    fn format_agent_error_extracts_status() {
        let e = anyhow::anyhow!("status=429 Too Many Requests blah blah");
        let result = format_agent_error(&e);
        assert!(result.contains("status=429"), "got: {result}");
    }

    #[test]
    fn cli_auth_commands_parse_correctly() {
        // Test the auth status command
        let cli = Cli::try_parse_from(vec!["omegon", "auth", "status"])
            .expect("should parse auth status");
        match cli.command.unwrap() {
            Commands::Auth { action } => {
                match action {
                    AuthAction::Status => {} // expected
                    _ => panic!("Expected Status action"),
                }
            }
            _ => panic!("Expected Auth command"),
        }

        // Test auth login with provider
        let cli = Cli::try_parse_from(vec!["omegon", "auth", "login", "anthropic"])
            .expect("should parse auth login");
        match cli.command.unwrap() {
            Commands::Auth { action } => match action {
                AuthAction::Login { provider } => {
                    assert_eq!(provider, "anthropic");
                }
                _ => panic!("Expected Login action"),
            },
            _ => panic!("Expected Auth command"),
        }

        // Test auth logout
        let cli = Cli::try_parse_from(vec!["omegon", "auth", "logout", "openai-codex"])
            .expect("should parse auth logout");
        match cli.command.unwrap() {
            Commands::Auth { action } => match action {
                AuthAction::Logout { provider } => {
                    assert_eq!(provider, "openai-codex");
                }
                _ => panic!("Expected Logout action"),
            },
            _ => panic!("Expected Auth command"),
        }

        // Test auth unlock
        let cli = Cli::try_parse_from(vec!["omegon", "auth", "unlock"])
            .expect("should parse auth unlock");
        match cli.command.unwrap() {
            Commands::Auth { action } => {
                match action {
                    AuthAction::Unlock => {} // expected
                    _ => panic!("Expected Unlock action"),
                }
            }
            _ => panic!("Expected Auth command"),
        }
    }

    #[test]
    fn backward_compat_login_command_still_works() {
        // Test that the deprecated login command still parses
        let cli = Cli::try_parse_from(vec!["omegon", "login", "anthropic"])
            .expect("should parse legacy login");
        match cli.command.unwrap() {
            Commands::Login { provider } => {
                assert_eq!(provider, "anthropic");
            }
            _ => panic!("Expected Login command"),
        }
    }
}

//! Omegon — Rust-native agent loop and lifecycle engine.
#![allow(dead_code)] // Phase 0 scaffold — fields/methods used as implementation fills in
//!
//! Phase 0: Headless agent loop for cleave children and standalone use.
//! Phase 1: Process owner with TUI bridge subprocess.
//! Phase 2: Native TUI rendering.
//! Phase 3: Native LLM provider clients.

use clap::{Parser, Subcommand};
use std::collections::VecDeque;
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
mod cleave_smoke;
mod context;
mod shadow_context;
pub mod extensions;
pub mod features;
mod ipc;
mod migrate;
mod skills;
mod smoke;
mod switch;
mod update;
mod upstream_errors;
mod usage;

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
use tokio::sync::oneshot;

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

    /// Soft exhaustion threshold for transient LLM errors (0 = infinite).
    /// After this many consecutive transient failures, exit with code 2
    /// so the cleave orchestrator can try a fallback provider.
    #[arg(long, default_value = "100")]
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

    /// Run deterministic cleave smoke tests without live provider calls.
    /// Uses injected child outcomes to verify cleave orchestration/reporting.
    #[arg(long)]
    smoke_cleave: bool,

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

    /// Set by Ollama when launching via `ollama launch omegon`.
    /// Signals that we're running as an Ollama integration.
    #[arg(long, global = true)]
    ollama_integration: bool,

    /// Ollama-provided model (set by `ollama launch omegon --model <model>`).
    /// Overrides --model if present.
    #[arg(long, global = true)]
    ollama_model: Option<String>,

    /// Auto-confirm prompts (set by `ollama launch omegon --yes`).
    #[arg(short = 'y', long, global = true)]
    yes: bool,
}

#[derive(Subcommand)]
enum AuthAction {
    /// Show authentication status for all providers.
    Status,
    /// Log in to a provider (OAuth or API key depending on provider).
    Login {
        /// Provider to log in to (anthropic, openai, openai-codex, openrouter, or ollama-cloud). Default: anthropic.
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

    /// Run a persistent local daemon/control-plane for long-lived agents and supervisors.
    Serve {
        /// Preferred localhost control port.
        #[arg(long, default_value = "7842")]
        control_port: u16,

        /// Require the exact control port instead of auto-falling back.
        #[arg(long)]
        strict_port: bool,
    },

    /// Run an embedded localhost control-plane for external supervisors.
    #[command(hide = true)]
    Embedded {
        /// Preferred localhost control port.
        #[arg(long, default_value = "7842")]
        control_port: u16,

        /// Require the exact control port instead of auto-falling back.
        #[arg(long)]
        strict_port: bool,
    },

    /// Unified authentication management.
    /// Usage: omegon auth <status|login|logout|unlock> [provider]
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// Log in to a provider. Defaults to Anthropic.
    /// Usage: omegon-agent login [anthropic|openai|openai-codex|openrouter|ollama-cloud]
    /// DEPRECATED: Use `omegon auth login` instead.
    #[command(hide = true)]
    Login {
        /// Provider to log in to (anthropic, openai, openai-codex, openrouter, or ollama-cloud). Default: anthropic.
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

    /// Manage bundled skills — list available skills and install them to ~/.omegon/skills/.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },

    /// Hidden benchmark-oriented commands used by the local comparison harness.
    #[command(hide = true)]
    Bench {
        #[command(subcommand)]
        action: BenchAction,
    },
}

#[derive(Subcommand)]
enum SkillsAction {
    /// List bundled skills and their installation status.
    List,
    /// Install all bundled skills to ~/.omegon/skills/.
    Install,
}

#[derive(Subcommand)]
enum BenchAction {
    /// Run a single benchmark task prompt headlessly and emit usage JSON.
    RunTask {
        /// Prompt text to execute.
        #[arg(long)]
        prompt: String,

        /// Path to write benchmark usage/result JSON.
        #[arg(long)]
        usage_json: PathBuf,
    },
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

fn parse_csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn child_preloaded_files() -> Vec<PathBuf> {
    std::env::var("OMEGON_CHILD_PRELOADED_FILES")
        .ok()
        .map(|raw| {
            raw.split(':')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut cli = Cli::parse();

    // ─── Ollama integration detection ────────────────────────────────────
    // When launched via `ollama launch omegon`, the --ollama-model flag
    // (set by Ollama) should override the --model CLI flag.
    if cli.ollama_integration {
        if let Some(ref model) = cli.ollama_model {
            cli.model = model.clone();
            tracing::info!(model = %model, "using Ollama-provided model");
        }
    }

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
        Some(Commands::Serve {
            control_port,
            strict_port,
        }) => run_embedded_command(control_port, strict_port).await,
        Some(Commands::Embedded {
            control_port,
            strict_port,
        }) => run_embedded_command(control_port, strict_port).await,
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
        Some(Commands::Skills { ref action }) => match action {
            SkillsAction::List => skills::cmd_list().map_err(Into::into),
            SkillsAction::Install => skills::cmd_install().map_err(Into::into),
        },
        Some(Commands::Bench { ref action }) => match action {
            BenchAction::RunTask { prompt, usage_json } => {
                let mut bench_cli = Cli {
                    command: None,
                    cwd: cli.cwd.clone(),
                    model: cli.model.clone(),
                    prompt: Some(prompt.clone()),
                    prompt_file: None,
                    max_turns: cli.max_turns,
                    max_retries: cli.max_retries,
                    resume: cli.resume.clone(),
                    fresh: cli.fresh,
                    no_session: cli.no_session,
                    no_splash: cli.no_splash,
                    tutorial: cli.tutorial,
                    smoke: false,
                    smoke_cleave: false,
                    initial_prompt: None,
                    initial_prompt_file: None,
                    context_class: cli.context_class.clone(),
                    log_level: cli.log_level.clone(),
                    log_file: cli.log_file.clone(),
                    ollama_integration: cli.ollama_integration,
                    ollama_model: cli.ollama_model.clone(),
                    yes: cli.yes,
                };
                bench_cli.prompt_file = None;
                run_agent_command(&bench_cli, Some(usage_json.clone())).await
            }
        },
        None => {
            // No subcommand: interactive if no --prompt, headless if --prompt given
            if let Some(warning) = anthropic_subscription_automation_warning(&cli) {
                eprintln!("warning: {warning}");
            }

            if cli.smoke {
                run_smoke_command(&cli).await
            } else if cli.smoke_cleave {
                cleave_smoke::run(&cli).await
            } else if cli.prompt.is_some() || cli.prompt_file.is_some() {
                run_agent_command(&cli, None).await
            } else {
                run_interactive_command(&cli).await
            }
        }
    }
}

#[derive(serde::Serialize)]
struct EmbeddedStartupEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    schema_version: u32,
    pid: u32,
    http_base: String,
    startup_url: String,
    health_url: String,
    ready_url: String,
    ws_url: String,
    auth_mode: String,
    auth_source: String,
}

async fn run_embedded_command(control_port: u16, strict_port: bool) -> anyhow::Result<()> {
    let state = web::WebState::new(
        crate::tui::dashboard::DashboardHandles::default(),
        tokio::sync::broadcast::channel::<AgentEvent>(32).0,
    );

    let (startup, _cmd_rx) =
        web::start_server_with_options(state, control_port, strict_port).await?;
    let event = EmbeddedStartupEvent {
        event_type: "omegon.startup",
        schema_version: startup.schema_version,
        pid: std::process::id(),
        http_base: startup.http_base.clone(),
        startup_url: startup.startup_url.clone(),
        health_url: startup.health_url.clone(),
        ready_url: startup.ready_url.clone(),
        ws_url: startup.ws_url.clone(),
        auth_mode: startup.auth_mode.clone(),
        auth_source: startup.auth_source.clone(),
    };
    println!("{}", serde_json::to_string(&event)?);

    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn anthropic_subscription_automation_warning(cli: &Cli) -> Option<String> {
    let is_automated =
        cli.smoke || cli.smoke_cleave || cli.prompt.is_some() || cli.prompt_file.is_some();
    if !is_automated {
        return None;
    }

    use crate::providers::AnthropicCredentialMode;
    let provider = cli.model.split(':').next().unwrap_or("anthropic");
    let targets_anthropic =
        provider == "anthropic" || provider == "claude" || cli.model.contains("claude");
    if !targets_anthropic
        || crate::providers::anthropic_credential_mode() != AnthropicCredentialMode::OAuthOnly
    {
        return None;
    }

    Some(
        "Anthropic subscription credentials are active for an automated/headless Anthropic run. \
Anthropic's Consumer Terms may prohibit this kind of non-human access for Claude.ai / Claude Pro \
credentials. Omegon is proceeding because operator agency wins, but the risk is yours. \
For unrestricted automation, use ANTHROPIC_API_KEY instead. Reference: https://www.anthropic.com/legal/consumer-terms"
            .to_string(),
    )
}

fn ensure_clean_cleave_repo(repo_path: &Path) -> anyhow::Result<()> {
    let status = omegon_git::status::query_status(repo_path)?;
    if status.is_clean {
        return Ok(());
    }

    let mut paths: Vec<String> = status.entries.iter().map(|e| e.path.clone()).collect();
    paths.sort();
    anyhow::bail!(
        "cleave preflight failed: repository has uncommitted changes. Commit, stash, or clean these paths before cleaving: {}",
        paths.join(", ")
    );
}

pub(crate) fn summarize_cleave_child_statuses(
    children: &[cleave::state::ChildState],
) -> (usize, usize, usize, usize) {
    let mut completed = 0;
    let mut failed = 0;
    let mut upstream_exhausted = 0;
    let mut unfinished = 0;

    for child in children {
        match child.status {
            cleave::state::ChildStatus::Completed => completed += 1,
            cleave::state::ChildStatus::Failed => failed += 1,
            cleave::state::ChildStatus::UpstreamExhausted => upstream_exhausted += 1,
            cleave::state::ChildStatus::Running | cleave::state::ChildStatus::Pending => {
                unfinished += 1
            }
        }
    }

    (completed, failed, upstream_exhausted, unfinished)
}

pub(crate) fn format_cleave_merge_result(
    child: Option<&cleave::state::ChildState>,
    label: &str,
    outcome: &cleave::orchestrator::MergeOutcome,
) -> String {
    match outcome {
        cleave::orchestrator::MergeOutcome::Success => format!("  ✓ {label} merged"),
        cleave::orchestrator::MergeOutcome::NoChanges => {
            if let Some(child) = child {
                match child.status {
                    cleave::state::ChildStatus::UpstreamExhausted => {
                        format!("  ⚡ {label} upstream exhausted (no repo changes to merge)")
                    }
                    cleave::state::ChildStatus::Failed => {
                        format!("  ✗ {label} failed (no repo changes to merge)")
                    }
                    cleave::state::ChildStatus::Pending | cleave::state::ChildStatus::Running => {
                        format!("  ○ {label} incomplete (no repo changes to merge)")
                    }
                    cleave::state::ChildStatus::Completed => {
                        format!("  ○ {label} completed (no changes)")
                    }
                }
            } else {
                format!("  ○ {label} completed (no changes)")
            }
        }
        cleave::orchestrator::MergeOutcome::Conflict(d) => {
            format!("  ✗ {label} CONFLICT: {}", d.lines().next().unwrap_or(""))
        }
        cleave::orchestrator::MergeOutcome::Failed(d) => {
            format!("  ✗ {label} FAILED: {}", d.lines().next().unwrap_or(""))
        }
        cleave::orchestrator::MergeOutcome::Skipped(reason) => {
            format!("  ○ {label} skipped ({reason})")
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
    ensure_clean_cleave_repo(&repo_path)?;
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
        injected_env: Vec::new(),
        child_runtime: crate::cleave::CleaveChildRuntimeProfile::default(),
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
        cleave::run_cleave(&plan, directive, &repo_path, workspace, &config, cancel, None)
            .await?;

    // Print report
    eprintln!("\n## Cleave Report: {}", result.state.run_id);
    eprintln!("**Duration:** {:.0}s", result.duration_secs);
    eprintln!();

    let (completed, failed, upstream_exhausted, unfinished) =
        summarize_cleave_child_statuses(&result.state.children);
    eprintln!(
        "**Children:** {} completed, {} failed, {} upstream exhausted, {} unfinished of {}",
        completed,
        failed,
        upstream_exhausted,
        unfinished,
        result.state.children.len()
    );
    eprintln!();

    for child in &result.state.children {
        let icon = match child.status {
            cleave::state::ChildStatus::Completed => "✓",
            cleave::state::ChildStatus::Failed => "✗",
            cleave::state::ChildStatus::UpstreamExhausted => "⚡",
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
        let child = result.state.children.iter().find(|c| c.label == *label);
        eprintln!("{}", format_cleave_merge_result(child, label, outcome));
    }

    // Post-merge guardrails (CLI only — TS wrapper runs its own)
    let all_merged = result.merge_results.iter().all(|(_, o)| {
        matches!(
            o,
            cleave::orchestrator::MergeOutcome::Success
                | cleave::orchestrator::MergeOutcome::NoChanges
        )
    });
    if all_merged && failed == 0 && upstream_exhausted == 0 && unfinished == 0 {
        let checks = cleave::guardrails::discover_guardrails(&repo_path);
        if !checks.is_empty() {
            let report = cleave::guardrails::run_guardrails(&repo_path, &checks);
            eprintln!("\n### Post-Merge Guardrails\n{report}");
        }
    }

    // Exit with error if any children did not complete successfully.
    if failed > 0 || upstream_exhausted > 0 || unfinished > 0 {
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
        if let Ok(child_thinking) = std::env::var("OMEGON_CHILD_THINKING_LEVEL") {
            if let Some(level) = crate::settings::ThinkingLevel::parse(&child_thinking) {
                s.thinking = level;
            }
        }
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

    // LLM provider ──────────────────────────────────────────────────────
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

    // Wire command_tx to ContextProvider for tool dispatch
    if let Ok(mut shared_tx) = agent.command_tx.lock() {
        *shared_tx = Some(command_tx.clone());
    }

    let pending_compact = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let web_command_tx = command_tx.clone(); // For forwarding web dashboard commands
    let ipc_command_tx = command_tx.clone(); // For forwarding IPC commands

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
                    if cli.context_class.is_none() {
                        s.context_class = settings::ContextClass::from_tokens(limits.max_input_tokens);
                    }
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
                "squad" => s.set_requested_context_class(settings::ContextClass::Squad),
                "maniple" => s.set_requested_context_class(settings::ContextClass::Maniple),
                "clan" => s.set_requested_context_class(settings::ContextClass::Clan),
                "legion" => s.set_requested_context_class(settings::ContextClass::Legion),
                _ => tracing::warn!("Unknown context class: {class_str}"),
            }
            tracing::info!(class = %class_str, "requested context class policy applied");
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

    let login_prompt_tx: std::sync::Arc<
        tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>,
    > = std::sync::Arc::new(tokio::sync::Mutex::new(None));
    let extension_widgets = std::mem::take(&mut agent.extension_widgets);
    let widget_receivers = std::mem::take(&mut agent.widget_receivers);
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
        login_prompt_tx: login_prompt_tx.clone(),
        extension_widgets,
        widget_receivers,
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

    // ─── IPC server (native Auspex/host control plane) ────────────────
    let ipc_cancel = tokio_util::sync::CancellationToken::new();
    {
        let ipc_cfg = ipc::IpcServerConfig::from_cwd(
            &agent.cwd,
            env!("CARGO_PKG_VERSION"),
            &agent.session_id,
        );
        ipc::start_ipc_server(
            ipc_cfg,
            agent.dashboard_handles.clone(),
            events_tx.clone(),
            ipc_command_tx,
            shared_cancel.clone(),
            ipc_cancel.clone(),
        );
    }

    // ─── Emit session start to bus features ────────────────────────────
    agent.bus.emit(&omegon_traits::BusEvent::SessionStart {
        cwd: agent.cwd.clone(),
        session_id: agent.session_id.clone(),
    });
    // Drain any requests from session_start handlers
    for request in agent.bus.drain_requests() {
        match request {
            omegon_traits::BusRequest::Notify { message, .. } => {
                let _ = events_tx.send(AgentEvent::SystemNotification { message });
            }
            omegon_traits::BusRequest::AutoStoreFact { .. } => {} // no-op: memory not ready yet
            _ => {}
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
                        ..Default::default()
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

                                // Update metrics
                                if let Ok(mut metrics) = agent.context_metrics.lock() {
                                    metrics.update(
                                        est,
                                        ctx_window,
                                        &s.effective_requested_class().label(),
                                        s.thinking.as_str(),
                                    );
                                }

                                if ctx_window > 0 {
                                    let system_prompt = agent.context_manager.build_system_prompt(
                                        agent.conversation.last_user_prompt(),
                                        &agent.conversation,
                                    );
                                    let llm_messages = agent.conversation.build_llm_view();
                                    let context_composition = crate::r#loop::compute_context_composition(
                                        &system_prompt,
                                        &llm_messages,
                                        &agent.bus.tool_definitions(),
                                        ctx_window,
                                    );
                                    let _ = events_tx.send(AgentEvent::TurnEnd {
                                        turn: agent.conversation.intent.stats.turns,
                                        model: None,
                                        provider: None,
                                        estimated_tokens: est,
                                        context_window: ctx_window,
                                        context_composition,
                                        actual_input_tokens: 0,
                                        actual_output_tokens: 0,
                                        cache_read_tokens: 0,
                                        provider_telemetry: None,
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
                        message: "Nothing eligible to compact yet — compaction only summarizes older turns after the decay window.".into(),
                    });
                }
            }

            tui::TuiCommand::ContextStatus => {
                let est = agent.conversation.estimate_tokens();
                let settings = shared_settings.lock().unwrap();
                let ctx_window = settings.context_window;
                let pct = if ctx_window > 0 {
                    ((est as f64 / ctx_window as f64) * 100.0).min(100.0) as u32
                } else {
                    0
                };
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: format!(
                        "Context: {}/{} tokens ({}%)\nPolicy: {}\nModel: {}\nThinking: {}",
                        est,
                        ctx_window,
                        pct,
                        settings.effective_requested_class().label(),
                        settings.context_class.label(),
                        settings.thinking.as_str()
                    ),
                });
            }

            tui::TuiCommand::ContextCompact => {
                tracing::info!("context compaction requested via /context compact");

                let bridge_guard = bridge.read().await;
                let stream_options = {
                    let s = shared_settings.lock().unwrap();
                    crate::bridge::StreamOptions {
                        model: Some(s.model.clone()),
                        reasoning: Some(s.thinking.as_str().to_string()),
                        extended_context: false,
                        ..Default::default()
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

                            // Update metrics
                            let settings = shared_settings.lock().unwrap();
                            if let Ok(mut metrics) = agent.context_metrics.lock() {
                                metrics.update(
                                    est,
                                    settings.context_window,
                                    &settings.effective_requested_class().label(),
                                    settings.thinking.as_str(),
                                );
                            }

                            // Send authoritative context snapshot to TUI/web consumers.
                            let _ = events_tx.send(AgentEvent::ContextUpdated {
                                tokens: est as u64,
                                context_window: settings.context_window as u64,
                                context_class: settings.effective_requested_class().label().to_string(),
                                thinking_level: settings.thinking.as_str().to_string(),
                            });

                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                message: format!("Context compressed. Now using {est} tokens."),
                            });
                        }
                        Err(e) => {
                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                message: format!("Compression failed: {e}"),
                            });
                        }
                    }
                } else {
                    let _ = events_tx.send(AgentEvent::SystemNotification {
                        message: "Nothing to compress yet — compaction only summarizes older turns after the decay window.".into(),
                    });
                }
            }

            tui::TuiCommand::ContextClear => {
                tracing::info!("context clear requested via /context clear");
                // Same as /new: save session, reset conversation
                if !cli.no_session {
                    let _ = session::save_session(
                        &agent.conversation,
                        &agent.cwd,
                        Some(agent.session_id.as_str()),
                    );
                }
                agent.conversation = crate::conversation::ConversationState::new();
                agent.session_id = crate::session::allocate_session_id();
                agent.resume_info = None;

                // Reset metrics — extract context_window in single lock scope to avoid deadlock
                let context_window = if let Ok(mut metrics) = agent.context_metrics.lock() {
                    let context_window = metrics.context_window;
                    metrics.update(0, context_window, "Squad", "off");
                    context_window
                } else {
                    200_000
                };

                // Send authoritative context snapshot to TUI/web consumers.
                let _ = events_tx.send(AgentEvent::ContextUpdated {
                    tokens: 0,
                    context_window: context_window as u64,
                    context_class: "Squad".to_string(),
                    thinking_level: "off".to_string(),
                });

                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: "Context cleared. Starting fresh conversation.".into(),
                });
                let _ = events_tx.send(AgentEvent::SessionReset);
            }

            tui::TuiCommand::ListSessions => {
                let text = list_sessions_message(&agent.cwd);
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: text.clone(),
                });
                let _ = events_tx.send(AgentEvent::AgentEnd);
                tracing::info!("{text}");
            }

            tui::TuiCommand::NewSession => {
                // Save the current session before resetting
                if !cli.no_session {
                    let _ = session::save_session(
                        &agent.conversation,
                        &agent.cwd,
                        Some(agent.session_id.as_str()),
                    );
                }
                agent.conversation = crate::conversation::ConversationState::new();
                agent.session_id = crate::session::allocate_session_id();
                agent.resume_info = None;
                let _ = events_tx.send(AgentEvent::SessionReset);
            }

            tui::TuiCommand::StartWebDashboard => {
                let web_state = web::WebState::with_auth_state(
                    agent.dashboard_handles.clone(),
                    events_tx.clone(),
                    agent.web_auth_state.clone(),
                );
                match web::start_server(web_state, 7842).await {
                    Ok((startup, web_cmd_rx)) => {
                        if let Ok(startup_json) = serde_json::to_value(&startup) {
                            let _ =
                                events_tx.send(AgentEvent::WebDashboardStarted { startup_json });
                        }
                        let url = format!("http://{}/?token={}", startup.addr, startup.token);
                        tui::open_browser(&url);
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: format!(
                                "Dashboard started at {url} (auth: {} via {})",
                                startup.auth_mode, startup.auth_source
                            ),
                        });
                        // Spawn a task to forward web commands into the main TUI command channel
                        let cmd_tx_clone = web_command_tx.clone();
                        let cancel_clone = shared_cancel.clone();
                        tokio::spawn(async move {
                            let mut rx = web_cmd_rx;
                            while let Some(web_cmd) = rx.recv().await {
                                let tui_cmd = match web_cmd {
                                    web::WebCommand::UserPrompt(text) => {
                                        tui::TuiCommand::SubmitPrompt(crate::tui::PromptSubmission {
                                            text,
                                            image_paths: Vec::new(),
                                            submitted_by: "web-dashboard".to_string(),
                                            via: "websocket",
                                        })
                                    }
                                    web::WebCommand::SlashCommand {
                                        name,
                                        args,
                                        respond_to,
                                    } => {
                                        tui::TuiCommand::RunSlashCommand {
                                            name,
                                            args,
                                            respond_to,
                                        }
                                    }
                                    web::WebCommand::Cancel => {
                                        if let Ok(guard) = cancel_clone.lock()
                                            && let Some(ref cancel) = *guard
                                        {
                                            cancel.cancel();
                                        }
                                        continue;
                                    }
                                    web::WebCommand::CancelCleaveChild { label, respond_to } => {
                                        tui::TuiCommand::RunSlashCommand {
                                            name: "cleave".to_string(),
                                            args: format!("cancel {label}"),
                                            respond_to,
                                        }
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

            tui::TuiCommand::RunSlashCommand {
                name,
                args,
                respond_to,
            } => {
                let response = execute_remote_slash_command(
                    &mut agent,
                    &events_tx,
                    &shared_settings,
                    &bridge,
                    &login_prompt_tx,
                    &cli,
                    &name,
                    &args,
                )
                .await;
                if let Some(reply) = respond_to {
                    let _ = reply.send(response);
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
                } else if name == "context_request" {
                    let tool_args = if args.trim_start().starts_with('{') {
                        match serde_json::from_str::<serde_json::Value>(&args) {
                            Ok(value)
                                if value.get("requests").and_then(|v| v.as_array()).is_some() =>
                            {
                                value
                            }
                            Ok(_) | Err(_) => {
                                let _ = events_tx.send(AgentEvent::SystemNotification {
                                    message: "Usage: /context request <kind> <query> or /context request {\"requests\":[...]}".to_string(),
                                });
                                continue;
                            }
                        }
                    } else {
                        let (kind, query) = args.split_once(' ').unwrap_or((args.as_str(), ""));
                        if kind.trim().is_empty() || query.trim().is_empty() {
                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                message: "Usage: /context request <kind> <query> or /context request {\"requests\":[...]}".to_string(),
                            });
                            continue;
                        }

                        serde_json::json!({
                            "requests": [{
                                "kind": kind.trim(),
                                "query": query.trim(),
                                "reason": "Operator-requested direct context inspection from slash command"
                            }]
                        })
                    };

                    let message = match agent
                        .bus
                        .execute_tool(
                            crate::tool_registry::context::REQUEST_CONTEXT,
                            "tui-context-request",
                            tool_args,
                            tokio_util::sync::CancellationToken::new(),
                        )
                        .await
                    {
                        Ok(result) => result
                            .content
                            .iter()
                            .filter_map(|c| match c {
                                omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                        Err(e) => format!("Context request failed: {e}"),
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
                            let prompt_tx_for_login = events_tx.clone();
                            let login_prompt_slot = login_prompt_tx.clone();
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
                                let prompt: auth::LoginPrompt = Box::new(move |msg| {
                                    let slot = login_prompt_slot.clone();
                                    let tx = prompt_tx_for_login.clone();
                                    Box::pin(async move {
                                        let (otx, orx) = tokio::sync::oneshot::channel();
                                        {
                                            let mut guard = slot.lock().await;
                                            *guard = Some(otx);
                                        }
                                        let _ = tx
                                            .send(AgentEvent::SystemNotification { message: msg });
                                        orx.await
                                            .map_err(|_| anyhow::anyhow!("Login prompt cancelled"))
                                    })
                                });
                                let result = match provider_clone.as_str() {
                                    "anthropic" | "claude" => {
                                        auth::login_anthropic_with_callbacks(progress, prompt).await
                                    }
                                    "openai-codex" | "chatgpt" | "codex" => {
                                        auth::login_openai_with_callbacks(progress, prompt).await
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
                let cmd_requests = agent.bus.drain_requests();
                for request in cmd_requests {
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
                            let mut status = crate::status::HarnessStatus::assemble();
                            let auth_status = auth::probe_all_providers().await;
                            status.providers =
                                crate::auth::auth_status_to_provider_statuses(&auth_status);
                            status.annotate_provider_runtime_health();
                            status.update_from_bus(&agent.bus);
                            if let Ok(json) = serde_json::to_value(&status) {
                                let _ = events_tx
                                    .send(AgentEvent::HarnessStatusChanged { status_json: json });
                            }
                        }
                        omegon_traits::BusRequest::AutoStoreFact {
                            section,
                            content,
                            source,
                        } => {
                            let args =
                                serde_json::json!({ "content": content, "section": section });
                            if let Err(e) = agent
                                .bus
                                .execute_tool(
                                    "memory_store",
                                    "auto_ingest",
                                    args,
                                    tokio_util::sync::CancellationToken::new(),
                                )
                                .await
                            {
                                tracing::debug!(source, "auto-store fact skipped: {e}");
                            }
                        }
                    }
                }
            }

            tui::TuiCommand::SubmitPrompt(prompt) => {
                if prompt.image_paths.is_empty() {
                    agent.conversation.push_user(prompt.text.clone());

                    // Read current settings for this turn
                    let (model, max_turns) = {
                        let s = shared_settings.lock().unwrap();
                        (s.model.clone(), s.max_turns)
                    };

                    let loop_config = r#loop::LoopConfig {
                        max_turns,
                        soft_limit_turns: if max_turns > 0 { max_turns * 2 / 3 } else { 0 },
                        max_retries: 0,
                        retry_delay_ms: 750,
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
                        let user_msg = format_agent_error(&e);
                        tracing::error!("Agent loop error: {e}");
                        let _ = events_tx.send(AgentEvent::SystemNotification { message: user_msg });
                        let _ = events_tx.send(AgentEvent::AgentEnd);
                    }

                    if let Ok(mut guard) = shared_cancel.lock() {
                        guard.take();
                    }
                } else {
                    let image_paths = prompt.image_paths;
                    let text = prompt.text;
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
                                source_path: Some(path.display().to_string()),
                            });
                        }
                    }
                    agent
                        .conversation
                        .push_user_with_images(text.clone(), images);

                    let (model, max_turns) = {
                        let s = shared_settings.lock().unwrap();
                        (s.model.clone(), s.max_turns)
                    };

                    let loop_config = r#loop::LoopConfig {
                        max_turns,
                        soft_limit_turns: if max_turns > 0 { max_turns * 2 / 3 } else { 0 },
                        max_retries: 0,
                        retry_delay_ms: 750,
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
                        let user_msg = format_agent_error(&e);
                        tracing::error!("Agent loop error: {e}");
                        let _ = events_tx.send(AgentEvent::SystemNotification { message: user_msg });
                        let _ = events_tx.send(AgentEvent::AgentEnd);
                    }

                    {
                        let est = agent.conversation.estimate_tokens();
                        let settings = shared_settings.lock().unwrap();
                        if let Ok(mut metrics) = agent.context_metrics.lock() {
                            metrics.update(
                                est,
                                settings.context_window,
                                &settings.effective_requested_class().label(),
                                settings.thinking.as_str(),
                            );
                        }
                        let _ = events_tx.send(AgentEvent::ContextUpdated {
                            tokens: est as u64,
                            context_window: settings.context_window as u64,
                            context_class: settings.effective_requested_class().label().to_string(),
                            thinking_level: settings.thinking.as_str().to_string(),
                        });
                    }

                    if let Ok(mut guard) = shared_cancel.lock() {
                        guard.take();
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
                            source_path: Some(path.display().to_string()),
                        });
                    }
                }
                // Push user text and images together so attachments survive
                // compaction, role alternation, save/resume, and provider translation.
                agent
                    .conversation
                    .push_user_with_images(text.clone(), images);

                // Read current settings for this turn
                let (model, max_turns) = {
                    let s = shared_settings.lock().unwrap();
                    (s.model.clone(), s.max_turns)
                };

                let loop_config = r#loop::LoopConfig {
                    max_turns,
                    soft_limit_turns: if max_turns > 0 { max_turns * 2 / 3 } else { 0 },
                    max_retries: 0, // TUI: retry indefinitely, operator switches manually
                    retry_delay_ms: 750,
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

                // Update context metrics + notify TUI/web consumers after turn completion.
                {
                    let est = agent.conversation.estimate_tokens();
                    let settings = shared_settings.lock().unwrap();
                    if let Ok(mut metrics) = agent.context_metrics.lock() {
                        metrics.update(
                            est,
                            settings.context_window,
                            &settings.effective_requested_class().label(),
                            settings.thinking.as_str(),
                        );
                    }
                    let _ = events_tx.send(AgentEvent::ContextUpdated {
                        tokens: est as u64,
                        context_window: settings.context_window as u64,
                        context_class: settings.effective_requested_class().label().to_string(),
                        thinking_level: settings.thinking.as_str().to_string(),
                    });
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
            Some(agent.session_id.as_str()),
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
    let provider = provider_label_from_error(&raw);
    let upstream_class = crate::upstream_errors::classify_upstream_error_for_provider(
        provider.as_deref().unwrap_or("upstream"),
        &raw,
    );
    if matches!(
        upstream_class,
        crate::upstream_errors::UpstreamErrorClass::ProviderOverloaded
            | crate::upstream_errors::UpstreamErrorClass::Upstream5xx
            | crate::upstream_errors::UpstreamErrorClass::Timeout
            | crate::upstream_errors::UpstreamErrorClass::StalledStream
            | crate::upstream_errors::UpstreamErrorClass::NetworkConnect
            | crate::upstream_errors::UpstreamErrorClass::NetworkReset
            | crate::upstream_errors::UpstreamErrorClass::Dns
            | crate::upstream_errors::UpstreamErrorClass::DecodeBody
            | crate::upstream_errors::UpstreamErrorClass::BridgeDropped
            | crate::upstream_errors::UpstreamErrorClass::ResponseIncomplete
            | crate::upstream_errors::UpstreamErrorClass::ResponseCancelled
    ) {
        let who = provider_display_name(provider.as_deref().unwrap_or("upstream"));
        let status_hint = provider_status_hint(provider.as_deref().unwrap_or("upstream"));
        return format!(
            "⚠ Upstream error ({who}) — provider-side failure. Retry later or check {status_hint}."
        );
    }
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

fn provider_label_from_error(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    if lower.contains("codex") {
        Some("openai-codex".to_string())
    } else if lower.contains("openai") {
        Some("openai".to_string())
    } else if lower.contains("anthropic") || lower.contains("claude") {
        Some("anthropic".to_string())
    } else if lower.contains("openrouter") {
        Some("openrouter".to_string())
    } else if lower.contains("groq") {
        Some("groq".to_string())
    } else if lower.contains("mistral") {
        Some("mistral".to_string())
    } else if lower.contains("cerebras") {
        Some("cerebras".to_string())
    } else if lower.contains("ollama") {
        Some("ollama".to_string())
    } else {
        None
    }
}

fn provider_display_name(provider: &str) -> &'static str {
    crate::auth::PROVIDERS
        .iter()
        .find(|p| p.id == provider)
        .map(|p| p.display_name)
        .unwrap_or("the provider")
}

fn provider_status_hint(provider: &str) -> &'static str {
    match provider {
        "openai" | "openai-codex" => "status.openai.com",
        "anthropic" => "status.anthropic.com",
        _ => "the provider status page",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeActorKind {
    Tui,
    Auspex,
    IpcClient,
    WebClient,
    DaemonEvent,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeActor {
    kind: RuntimeActorKind,
    label: String,
}

impl RuntimeActor {
    fn tui() -> Self {
        Self {
            kind: RuntimeActorKind::Tui,
            label: "local-tui".to_string(),
        }
    }

    fn auspex() -> Self {
        Self {
            kind: RuntimeActorKind::Auspex,
            label: "auspex".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlSurface {
    Tui,
    Ipc,
    WebSocket,
    HttpEventIngress,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptEnvelope {
    id: u64,
    text: String,
    image_paths: Vec<PathBuf>,
    submitted_by: RuntimeActor,
    via: ControlSurface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveTurnPhase {
    Running,
    Cancelling {
        requested_by: RuntimeActor,
        via: ControlSurface,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveTurnMeta {
    runtime_turn_id: u64,
    prompt: PromptEnvelope,
    phase: ActiveTurnPhase,
}

#[derive(Debug, Default)]
struct InteractiveRuntimeSupervisor {
    queue: VecDeque<PromptEnvelope>,
    active_turn: Option<ActiveTurnMeta>,
    next_prompt_id: u64,
    next_runtime_turn_id: u64,
}

impl InteractiveRuntimeSupervisor {
    fn enqueue_prompt(
        &mut self,
        text: String,
        image_paths: Vec<PathBuf>,
        actor: RuntimeActor,
        via: ControlSurface,
    ) -> u64 {
        self.next_prompt_id += 1;
        let prompt_id = self.next_prompt_id;
        self.queue.push_back(PromptEnvelope {
            id: prompt_id,
            text,
            image_paths,
            submitted_by: actor,
            via,
        });
        prompt_id
    }

    fn queue_depth(&self) -> usize {
        self.queue.len()
    }

    fn is_busy(&self) -> bool {
        self.active_turn.is_some()
    }

    fn maybe_start_next_turn(&mut self) -> Option<ActiveTurnMeta> {
        if self.active_turn.is_some() {
            return None;
        }
        let prompt = self.queue.pop_front()?;
        self.next_runtime_turn_id += 1;
        let active = ActiveTurnMeta {
            runtime_turn_id: self.next_runtime_turn_id,
            prompt,
            phase: ActiveTurnPhase::Running,
        };
        self.active_turn = Some(active.clone());
        Some(active)
    }

    fn request_cancel(
        &mut self,
        actor: RuntimeActor,
        via: ControlSurface,
    ) -> Option<&ActiveTurnMeta> {
        let active = self.active_turn.as_mut()?;
        if matches!(active.phase, ActiveTurnPhase::Running) {
            active.phase = ActiveTurnPhase::Cancelling {
                requested_by: actor,
                via,
            };
        }
        self.active_turn.as_ref()
    }

    fn complete_active_turn(&mut self) -> Option<ActiveTurnMeta> {
        self.active_turn.take()
    }
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
struct BenchmarkUsageSummary {
    model: Option<String>,
    provider: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    cache_tokens: u64,
    estimated_tokens: usize,
    context_window: usize,
    context_composition: omegon_traits::ContextComposition,
    provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
}

impl BenchmarkUsageSummary {
    fn observe_turn(
        &mut self,
        model: Option<String>,
        provider: Option<String>,
        estimated_tokens: usize,
        context_window: usize,
        context_composition: omegon_traits::ContextComposition,
        actual_input_tokens: u64,
        actual_output_tokens: u64,
        cache_read_tokens: u64,
        provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    ) {
        self.model = model;
        self.provider = provider;
        self.input_tokens = self.input_tokens.saturating_add(actual_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(actual_output_tokens);
        self.cache_tokens = self.cache_tokens.saturating_add(cache_read_tokens);
        self.estimated_tokens = self.estimated_tokens.saturating_add(estimated_tokens);
        self.context_window = context_window;
        self.context_composition = context_composition;
        self.provider_telemetry = provider_telemetry;
    }
}

fn write_benchmark_usage_json(path: &Path, summary: &BenchmarkUsageSummary) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(summary)?)?;
    Ok(())
}

async fn run_agent_command(cli: &Cli, usage_json: Option<PathBuf>) -> anyhow::Result<()> {
    tracing::info!(model = %cli.model, "omegon-agent starting");

    if maybe_run_injected_cleave_smoke_child(&cli.cwd)? {
        return Ok(());
    }

    // Resolve prompt from --prompt or --prompt-file
    let prompt_text = match (&cli.prompt, &cli.prompt_file) {
        (Some(p), _) => p.clone(),
        (None, Some(path)) => {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                cli.cwd.join(path)
            };
            std::fs::read_to_string(&resolved).map_err(|e| {
                anyhow::anyhow!("Failed to read prompt file {}: {}", resolved.display(), e)
            })?
        }
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
        retry_delay_ms: 750,
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
    let benchmark_summary = std::sync::Arc::new(std::sync::Mutex::new(BenchmarkUsageSummary::default()));
    let benchmark_summary_task = std::sync::Arc::clone(&benchmark_summary);

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
                    name: _,
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
                AgentEvent::TurnEnd {
                    turn,
                    model,
                    provider,
                    estimated_tokens,
                    context_window,
                    context_composition,
                    actual_input_tokens,
                    actual_output_tokens,
                    cache_read_tokens,
                    provider_telemetry,
                } => {
                    if let Ok(mut summary) = benchmark_summary_task.lock() {
                        summary.observe_turn(
                            model,
                            provider,
                            estimated_tokens,
                            context_window,
                            context_composition,
                            actual_input_tokens,
                            actual_output_tokens,
                            cache_read_tokens,
                            provider_telemetry,
                        );
                    }
                    if actual_input_tokens > 0 || actual_output_tokens > 0 {
                        tracing::info!(
                            "── Turn {turn} complete — in:{actual_input_tokens} out:{actual_output_tokens} ──"
                        );
                    } else {
                        tracing::info!("── Turn {turn} complete ──");
                    }
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

    if let Some(path) = usage_json.as_ref() {
        let summary = benchmark_summary
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        write_benchmark_usage_json(path, &summary)?;
    }

    match &result {
        Ok(()) => {
            if let Some(last_text) = agent.conversation.last_assistant_text() {
                println!("{last_text}");
            }
        }
        Err(e) => {
            if r#loop::is_upstream_exhausted(&e) {
                // Exit 2 signals the cleave orchestrator (and any supervisor) that this
                // child failed due to upstream provider exhaustion, not a logic error.
                // The orchestrator may retry with a cross-provider fallback.
                eprintln!("upstream exhausted: {e}");
                std::process::exit(2);
            }
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }

    result
}

fn maybe_run_injected_cleave_smoke_child(cwd: &Path) -> anyhow::Result<bool> {
    let Some(mode) = std::env::var("OMEGON_CLEAVE_SMOKE_CHILD_MODE").ok() else {
        return Ok(false);
    };

    if let Ok(rel_path) = std::env::var("OMEGON_CLEAVE_SMOKE_WRITE_FILE") {
        let path = cwd.join(rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, "smoke child wrote this file\n")?;
    }

    match mode.as_str() {
        "success-noop" => {
            println!("simulated cleave smoke child success (noop)");
            Ok(true)
        }
        "success-dirty" => {
            println!("simulated cleave smoke child success (dirty)");
            Ok(true)
        }
        "report-runtime" => {
            let shared_settings = settings::shared("anthropic:claude-sonnet-4-6");
            let agent = tokio::runtime::Handle::current().block_on(async {
                setup::AgentSetup::new(cwd, None, Some(shared_settings.clone())).await
            })?;
            let status = agent.initial_harness_status.clone();
            let tool_names: Vec<String> = agent
                .bus
                .tool_definitions()
                .into_iter()
                .map(|t| t.name)
                .collect();
            let settings_guard = shared_settings.lock().ok();
            let selected_model = settings_guard
                .as_ref()
                .map(|s| s.model.clone())
                .unwrap_or_else(|| "unknown".into());
            let selected_provider = crate::providers::infer_provider_id(&selected_model);
            let preloaded_files = child_preloaded_files()
                .into_iter()
                .map(|path| {
                    let resolved = cwd.join(&path);
                    let content = std::fs::read_to_string(&resolved).unwrap_or_default();
                    serde_json::json!({
                        "path": path.display().to_string(),
                        "resolved": resolved.display().to_string(),
                        "content": content,
                    })
                })
                .collect::<Vec<_>>();
            let report = serde_json::json!({
                "mode": "report-runtime",
                "model": selected_model,
                "provider": selected_provider,
                "tool_names": tool_names,
                "plugin_names": status.installed_plugins.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                "active_persona_skills": status.active_persona.as_ref().map(|p| p.activated_skills.clone()).unwrap_or_default(),
                "requested_skill_filter": parse_csv_env("OMEGON_CHILD_SKILLS"),
                "preloaded_files": preloaded_files,
                "context_class": status.context_class,
                "thinking_level": status.thinking_level,
            });
            println!("{}", serde_json::to_string(&report)?);
            Ok(true)
        }
        "fail" => {
            eprintln!("Error: simulated cleave smoke child failure");
            std::process::exit(1);
        }
        "upstream-exhausted" => {
            eprintln!(
                "upstream exhausted: 100 consecutive transient failures over 0s: simulated smoke exhaustion"
            );
            std::process::exit(2);
        }
        other => anyhow::bail!("unknown OMEGON_CLEAVE_SMOKE_CHILD_MODE: {other}"),
    }
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

fn list_sessions_message(cwd: &Path) -> String {
    let sessions = session::list_sessions(cwd);
    if sessions.is_empty() {
        "No saved sessions for this directory.".to_string()
    } else {
        let lines: Vec<String> = sessions
            .iter()
            .take(10)
            .map(|s| {
                format!(
                    "  {} — {} turns, {} tools — {}",
                    s.meta.session_id, s.meta.turns, s.meta.tool_calls, s.meta.last_prompt_snippet
                )
            })
            .collect();
        format!("Recent sessions:\n{}", lines.join("\n"))
    }
}

async fn execute_remote_slash_command(
    agent: &mut setup::AgentSetup,
    events_tx: &broadcast::Sender<AgentEvent>,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    login_prompt_tx: &std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
    cli: &Cli,
    name: &str,
    args: &str,
) -> omegon_traits::SlashCommandResponse {
    use crate::tui::{canonical_slash_command, CanonicalSlashCommand};
    use omegon_traits::SlashCommandResponse;

    let Some(command) = canonical_slash_command(name, args) else {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} is interactive-only or unavailable via remote slash execution."
            )),
        };
    };

    match command {
        CanonicalSlashCommand::ModelList => {
            let catalog = crate::tui::model_catalog::ModelCatalog::discover();
            let mut output = String::from("Available models:\n");
            for (provider_name, models) in &catalog.providers {
                output.push_str(&format!("\n{}:\n", provider_name));
                for model in models {
                    output.push_str(&format!("  {} ({})\n", model.name, model.id));
                }
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(output),
            }
        }
        CanonicalSlashCommand::SetModel(requested_model) => {
            let effective_model = providers::resolve_execution_model_spec(&requested_model)
                .await
                .unwrap_or_else(|| requested_model.clone());
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
                let mut profile = settings::Profile::load(&agent.cwd);
                profile.capture_from(&s);
                let _ = profile.save(&agent.cwd);
            }
            let mut messages = Vec::new();
            if effective_model != requested_model {
                let provider_label = crate::auth::provider_by_id(&new_provider)
                    .map(|p| p.display_name)
                    .unwrap_or(new_provider.as_str());
                messages.push(format!(
                    "Requested {requested_model}; using executable route {effective_model} via {provider_label}."
                ));
            }
            if old_provider != new_provider {
                let provider = crate::providers::infer_provider_id(&effective_model);
                if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await {
                    let mut guard = bridge.write().await;
                    *guard = new_bridge;
                    if let Ok(mut s) = shared_settings.lock() {
                        s.provider_connected = true;
                    }
                    let provider_label = crate::auth::provider_by_id(&provider)
                        .map(|p| p.display_name)
                        .unwrap_or(provider.as_str());
                    messages.push(format!(
                        "Provider switched to {provider_label} ({effective_model})."
                    ));
                } else {
                    if let Ok(mut s) = shared_settings.lock() {
                        s.provider_connected = false;
                    }
                    let provider_label = crate::auth::provider_by_id(&provider)
                        .map(|p| p.display_name)
                        .unwrap_or(provider.as_str());
                    messages.push(format!(
                        "⚠ No credentials for {provider_label}. Use /login to authenticate."
                    ));
                }
            } else if old_model != effective_model {
                let provider_label = crate::auth::provider_by_id(&new_provider)
                    .map(|p| p.display_name)
                    .unwrap_or(new_provider.as_str());
                messages.push(format!(
                    "Model switched to {effective_model} via {provider_label}."
                ));
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(messages.join("\n")),
            }
        }
        CanonicalSlashCommand::SetThinking(level) => {
            if let Ok(mut s) = shared_settings.lock() {
                s.thinking = level;
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(format!("Thinking → {} {}", level.icon(), level.as_str())),
            }
        }
        CanonicalSlashCommand::ContextStatus => {
            let est = agent.conversation.estimate_tokens();
            let settings = shared_settings.lock().unwrap();
            let ctx_window = settings.context_window;
            let pct = if ctx_window > 0 {
                ((est as f64 / ctx_window as f64) * 100.0).min(100.0) as u32
            } else {
                0
            };
            SlashCommandResponse {
                accepted: true,
                output: Some(format!(
                    "Context: {}/{} tokens ({}%)\nPolicy: {}\nModel: {}\nThinking: {}",
                    est,
                    ctx_window,
                    pct,
                    settings.effective_requested_class().label(),
                    settings.context_class.label(),
                    settings.thinking.as_str()
                )),
            }
        }
        CanonicalSlashCommand::ContextCompact => {
            let bridge_guard = bridge.read().await;
            let stream_options = {
                let s = shared_settings.lock().unwrap();
                crate::bridge::StreamOptions {
                    model: Some(s.model.clone()),
                    reasoning: Some(s.thinking.as_str().to_string()),
                    extended_context: false,
                    ..Default::default()
                }
            };
            if let Some((payload, _)) = agent.conversation.build_compaction_payload() {
                match r#loop::compact_via_llm(bridge_guard.as_ref(), &payload, &stream_options).await
                {
                    Ok(summary) => {
                        agent.conversation.apply_compaction(summary);
                        let est = agent.conversation.estimate_tokens();
                        let settings = shared_settings.lock().unwrap();
                        if let Ok::<
                            std::sync::MutexGuard<'_, crate::features::context::SharedContextMetrics>,
                            _,
                        >(mut metrics) = agent.context_metrics.lock()
                        {
                            metrics.update(
                                est,
                                settings.context_window,
                                &settings.effective_requested_class().label(),
                                settings.thinking.as_str(),
                            );
                        }
                        let _ = events_tx.send(AgentEvent::ContextUpdated {
                            tokens: est as u64,
                            context_window: settings.context_window as u64,
                            context_class: settings.effective_requested_class().label().to_string(),
                            thinking_level: settings.thinking.as_str().to_string(),
                        });
                        SlashCommandResponse {
                            accepted: true,
                            output: Some(format!("Context compressed. Now using {est} tokens.")),
                        }
                    }
                    Err(e) => SlashCommandResponse {
                        accepted: false,
                        output: Some(format!("Compression failed: {e}")),
                    },
                }
            } else {
                SlashCommandResponse {
                    accepted: true,
                    output: Some(
                        "Nothing to compress yet — compaction only summarizes older turns after the decay window."
                            .to_string(),
                    ),
                }
            }
        }
        CanonicalSlashCommand::ContextClear => {
            if !cli.no_session {
                let _ = session::save_session(
                    &agent.conversation,
                    &agent.cwd,
                    Some(agent.session_id.as_str()),
                );
            }
            agent.conversation = crate::conversation::ConversationState::new();
            agent.session_id = crate::session::allocate_session_id();
            agent.resume_info = None;
            let context_window = if let Ok(mut metrics) = agent.context_metrics.lock() {
                let context_window = metrics.context_window;
                metrics.update(0, context_window, "Squad", "off");
                context_window
            } else {
                200_000
            };
            let _ = events_tx.send(AgentEvent::ContextUpdated {
                tokens: 0,
                context_window: context_window as u64,
                context_class: "Squad".to_string(),
                thinking_level: "off".to_string(),
            });
            let _ = events_tx.send(AgentEvent::SessionReset);
            SlashCommandResponse {
                accepted: true,
                output: Some("Context cleared. Starting fresh conversation.".to_string()),
            }
        }
        CanonicalSlashCommand::ContextRequest { kind, query } => {
            let args = serde_json::json!({
                "requests": [{
                    "kind": kind,
                    "query": query,
                    "reason": "Operator-requested direct context inspection from slash command"
                }]
            });
            match agent
                .bus
                .execute_tool(
                    crate::tool_registry::context::REQUEST_CONTEXT,
                    "slash-context-request",
                    args,
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
            {
                Ok(result) => {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    SlashCommandResponse {
                        accepted: true,
                        output: Some(text),
                    }
                }
                Err(e) => SlashCommandResponse {
                    accepted: false,
                    output: Some(format!("Context request failed: {e}")),
                },
            }
        }
        CanonicalSlashCommand::ContextRequestJson(raw) => {
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(args) if args.get("requests").and_then(|v| v.as_array()).is_some() => {
                    match agent
                        .bus
                        .execute_tool(
                            crate::tool_registry::context::REQUEST_CONTEXT,
                            "slash-context-request",
                            args,
                            tokio_util::sync::CancellationToken::new(),
                        )
                        .await
                    {
                        Ok(result) => {
                            let text = result
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n\n");
                            SlashCommandResponse {
                                accepted: true,
                                output: Some(text),
                            }
                        }
                        Err(e) => SlashCommandResponse {
                            accepted: false,
                            output: Some(format!("Context request failed: {e}")),
                        },
                    }
                }
                _ => SlashCommandResponse {
                    accepted: false,
                    output: Some(
                        "Usage: /context request <kind> <query> or /context request {\"requests\":[...]}".to_string(),
                    ),
                },
            }
        }
        CanonicalSlashCommand::SetContextClass(class) => {
            if let Ok(mut s) = shared_settings.lock() {
                s.set_requested_context_class(class);
                let mut profile = settings::Profile::load(&agent.cwd);
                profile.capture_from(&s);
                let _ = profile.save(&agent.cwd);
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(format!("Context policy → {} (model capacity unchanged)", class.label())),
            }
        }
        CanonicalSlashCommand::NewSession => {
            if !cli.no_session {
                let _ = session::save_session(
                    &agent.conversation,
                    &agent.cwd,
                    Some(agent.session_id.as_str()),
                );
            }
            agent.conversation = crate::conversation::ConversationState::new();
            agent.session_id = crate::session::allocate_session_id();
            agent.resume_info = None;
            let _ = events_tx.send(AgentEvent::SessionReset);
            SlashCommandResponse {
                accepted: true,
                output: Some("Started a fresh session.".to_string()),
            }
        }
        CanonicalSlashCommand::ListSessions => SlashCommandResponse {
            accepted: true,
            output: Some(list_sessions_message(&agent.cwd)),
        },
        CanonicalSlashCommand::AuthStatus => {
            let status = auth::probe_all_providers().await;
            SlashCommandResponse {
                accepted: true,
                output: Some(format_auth_status(&status)),
            }
        }
        CanonicalSlashCommand::AuthUnlock => SlashCommandResponse {
            accepted: true,
            output: Some("🔒 Secrets store unlock not yet implemented".to_string()),
        },
        CanonicalSlashCommand::AuthLogin(provider) => {
            let provider = provider.trim();
            let provider = if provider.is_empty() { "anthropic" } else { provider };
            if provider == "openai" {
                return SlashCommandResponse {
                    accepted: false,
                    output: Some(
                        "OpenAI API login is interactive-only in the TUI. Use /login in the terminal session or set OPENAI_API_KEY."
                            .to_string(),
                    ),
                };
            }
            if login_prompt_tx.lock().await.is_some() {
                return SlashCommandResponse {
                    accepted: false,
                    output: Some("Login is already waiting for interactive input in the TUI.".to_string()),
                };
            }
            let events_tx_clone = events_tx.clone();
            let progress_tx = events_tx.clone();
            let prompt_tx_for_login = events_tx.clone();
            let login_prompt_slot = login_prompt_tx.clone();
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
                let prompt: auth::LoginPrompt = Box::new(move |msg| {
                    let slot = login_prompt_slot.clone();
                    let tx = prompt_tx_for_login.clone();
                    Box::pin(async move {
                        let (otx, orx) = tokio::sync::oneshot::channel();
                        {
                            let mut guard = slot.lock().await;
                            *guard = Some(otx);
                        }
                        let _ = tx.send(AgentEvent::SystemNotification { message: msg });
                        orx.await
                            .map_err(|_| anyhow::anyhow!("Login prompt cancelled"))
                    })
                });
                let result = match provider_clone.as_str() {
                    "anthropic" | "claude" => {
                        auth::login_anthropic_with_callbacks(progress, prompt).await
                    }
                    "openai-codex" | "chatgpt" | "codex" => {
                        auth::login_openai_with_callbacks(progress, prompt).await
                    }
                    "openai" => Err(anyhow::anyhow!(
                        "OpenAI API login in the TUI uses hidden API-key entry. Run /login and choose OpenAI API, or set OPENAI_API_KEY."
                    )),
                    "openrouter" => Err(anyhow::anyhow!(
                        "OpenRouter login in the TUI uses hidden API-key entry. Run /login and choose OpenRouter, or set OPENROUTER_API_KEY."
                    )),
                    "ollama-cloud" => Err(anyhow::anyhow!(
                        "Ollama Cloud login in the TUI uses hidden API-key entry. Run /login and choose Ollama Cloud, or set OLLAMA_API_KEY."
                    )),
                    _ => Err(anyhow::anyhow!(
                        "Unknown provider: {}. Use: anthropic, openai, openai-codex, openrouter, ollama-cloud",
                        provider_clone
                    )),
                };
                let provider_label = crate::auth::provider_by_id(&provider_clone)
                    .map(|p| p.display_name)
                    .unwrap_or(provider_clone.as_str())
                    .to_string();
                let message = match &result {
                    Ok(_) => format!("✓ Successfully logged in to {provider_label}"),
                    Err(e) => format!("❌ Login failed: {}", e),
                };
                let _ = events_tx_clone.send(AgentEvent::SystemNotification { message });
                if result.is_ok() {
                    let effective_model = providers::resolve_execution_model_spec(&model_for_redetect)
                        .await
                        .unwrap_or(model_for_redetect.clone());
                    if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await {
                        let mut guard = bridge_clone.write().await;
                        *guard = new_bridge;
                        if let Ok(mut s) = settings_for_login.lock() {
                            s.set_model(&effective_model);
                            s.provider_connected = true;
                        }
                        let _ = events_tx_clone.send(AgentEvent::SystemNotification {
                            message: format!("Provider connected — active route {}.", effective_model),
                        });
                    }
                }
            });
            SlashCommandResponse {
                accepted: true,
                output: Some(format!(
                    "Login started for {provider}. Complete any interactive prompts in the TUI."
                )),
            }
        }
        CanonicalSlashCommand::AuthLogout(provider) => {
            let message = match auth::logout_provider(&provider) {
                Ok(()) => format!("✓ Logged out from {}", provider),
                Err(e) => format!("❌ Logout failed: {}", e),
            };
            SlashCommandResponse {
                accepted: true,
                output: Some(message),
            }
        }
    }
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
        "ollama-cloud" => {
            login_api_key(
                "ollama-cloud",
                "OLLAMA_API_KEY",
                "https://ollama.com/settings/keys",
            )
            .await
        }
        _ => {
            eprintln!(
                "Unknown provider: {provider}. Use: anthropic, openai, openai-codex, openrouter, ollama-cloud"
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
    use clap::CommandFactory;
    use tempfile::tempdir;

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
    fn format_agent_error_collapses_openai_provider_side_failures() {
        let e = anyhow::anyhow!("LLM error: Codex 520: error code: 520");
        let result = format_agent_error(&e);
        assert!(result.contains("Upstream error (OpenAI/Codex)"), "got: {result}");
        assert!(result.contains("status.openai.com"), "got: {result}");
        assert!(!result.contains("error code: 520"), "got: {result}");
    }

    #[test]
    fn interactive_runtime_supervisor_starts_first_prompt_fifo() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            Vec::new(),
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
        );

        let active = supervisor
            .maybe_start_next_turn()
            .expect("first queued prompt should start");

        assert_eq!(active.runtime_turn_id, 1);
        assert_eq!(active.prompt.text, "first");
        assert_eq!(active.prompt.submitted_by.kind, RuntimeActorKind::Tui);
        assert_eq!(supervisor.queue_depth(), 1);
        assert!(supervisor.is_busy());
    }

    #[test]
    fn interactive_runtime_supervisor_cancel_records_actor_identity() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
        );
        supervisor.maybe_start_next_turn().expect("active turn");

        let active = supervisor
            .request_cancel(RuntimeActor::auspex(), ControlSurface::Ipc)
            .expect("cancel should target active turn");

        match &active.phase {
            ActiveTurnPhase::Cancelling { requested_by, via } => {
                assert_eq!(requested_by.kind, RuntimeActorKind::Auspex);
                assert_eq!(requested_by.label, "auspex");
                assert_eq!(*via, ControlSurface::Ipc);
            }
            other => panic!("expected cancelling phase, got {other:?}"),
        }
        assert!(supervisor.is_busy());
    }

    #[test]
    fn interactive_runtime_supervisor_remains_busy_until_completion() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
        );
        supervisor.maybe_start_next_turn().expect("active turn");
        supervisor.request_cancel(RuntimeActor::tui(), ControlSurface::Tui);

        assert!(supervisor.is_busy(), "cancel request must not imply idle");

        let completed = supervisor.complete_active_turn().expect("completed turn");
        assert_eq!(completed.prompt.text, "first");
        assert!(!supervisor.is_busy(), "busy clears only after completion");
    }

    #[test]
    fn interactive_runtime_supervisor_starts_next_queued_turn_after_completion() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            vec![PathBuf::from("/tmp/paste.png")],
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
        );

        supervisor.maybe_start_next_turn().expect("first active turn");
        supervisor.complete_active_turn().expect("first completion");
        let active = supervisor
            .maybe_start_next_turn()
            .expect("second queued prompt should start");

        assert_eq!(active.runtime_turn_id, 2);
        assert_eq!(active.prompt.text, "second");
        assert_eq!(active.prompt.image_paths, vec![PathBuf::from("/tmp/paste.png")]);
        assert_eq!(active.prompt.submitted_by.kind, RuntimeActorKind::Auspex);
        assert_eq!(supervisor.queue_depth(), 0);
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
    fn remote_slash_login_is_classified_as_interactive_only_for_openai_api() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut agent = rt.block_on(setup::AgentSetup::new(Path::new("."), None, None)).unwrap();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(
            Box::new(crate::bridge::NullBridge) as Box<dyn LlmBridge>
        ));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let response = rt.block_on(execute_remote_slash_command(
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "login",
            "openai",
        ));

        assert!(!response.accepted);
        assert!(response.output.unwrap().contains("interactive-only"));
    }

    #[test]
    fn remote_slash_logout_defaults_to_anthropic() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut agent = rt.block_on(setup::AgentSetup::new(Path::new("."), None, None)).unwrap();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(
            Box::new(crate::bridge::NullBridge) as Box<dyn LlmBridge>
        ));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let response = rt.block_on(execute_remote_slash_command(
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "logout",
            "",
        ));

        assert!(response.accepted);
        assert!(response.output.unwrap().contains("anthropic"));
    }

    #[test]
    fn embedded_command_parses_control_plane_flags() {
        let cli = Cli::try_parse_from(vec![
            "omegon",
            "embedded",
            "--control-port",
            "7842",
            "--strict-port",
        ])
        .expect("should parse embedded command");

        match cli.command.unwrap() {
            Commands::Embedded {
                control_port,
                strict_port,
            } => {
                assert_eq!(control_port, 7842);
                assert!(strict_port);
            }
            _ => panic!("Expected Embedded command"),
        }
    }

    #[test]
    fn serve_command_parses_control_plane_flags() {
        let cli = Cli::try_parse_from(vec![
            "omegon",
            "serve",
            "--control-port",
            "7842",
            "--strict-port",
        ])
        .expect("should parse serve command");

        match cli.command.unwrap() {
            Commands::Serve {
                control_port,
                strict_port,
            } => {
                assert_eq!(control_port, 7842);
                assert!(strict_port);
            }
            _ => panic!("Expected Serve command"),
        }
    }

    #[test]
    fn auth_login_help_lists_all_supported_non_oauth_providers() {
        let mut cmd = Cli::command();
        let auth_cmd = cmd
            .find_subcommand_mut("auth")
            .expect("auth command must exist");
        let login_cmd = auth_cmd
            .find_subcommand_mut("login")
            .expect("auth login command must exist");
        let help = login_cmd.render_help().to_string();

        assert!(
            help.contains("openrouter"),
            "auth login help should mention openrouter: {help}"
        );
        assert!(
            help.contains("ollama-cloud"),
            "auth login help should mention ollama-cloud: {help}"
        );
    }

    #[test]
    fn relative_prompt_file_is_resolved_from_cwd() {
        let cwd = tempfile::tempdir().unwrap();
        let prompts = cwd.path().join("prompts");
        std::fs::create_dir_all(&prompts).unwrap();
        let prompt_path = prompts.join("task.md");
        std::fs::write(&prompt_path, "hello from prompt file").unwrap();

        let cli = Cli::try_parse_from(vec![
            "omegon",
            "--cwd",
            cwd.path().to_str().unwrap(),
            "--prompt-file",
            "prompts/task.md",
        ])
        .unwrap();

        let resolved = if cli.prompt_file.as_ref().unwrap().is_absolute() {
            cli.prompt_file.as_ref().unwrap().clone()
        } else {
            cli.cwd.join(cli.prompt_file.as_ref().unwrap())
        };

        let prompt = std::fs::read_to_string(resolved).unwrap();
        assert_eq!(prompt, "hello from prompt file");
    }

    #[test]
    fn relative_prompt_file_under_child_worktree_can_be_read() {
        let root = tempfile::tempdir().unwrap();
        let worktree = root.path().join("child-worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        let prompt_path = worktree.join(".cleave-prompt.md");
        std::fs::write(&prompt_path, "child prompt").unwrap();

        let cli = Cli::try_parse_from(vec![
            "omegon",
            "--cwd",
            worktree.to_str().unwrap(),
            "--prompt-file",
            ".cleave-prompt.md",
        ])
        .unwrap();

        let resolved = cli.cwd.join(cli.prompt_file.as_ref().unwrap());
        let prompt = std::fs::read_to_string(resolved).unwrap();
        assert_eq!(prompt, "child prompt");
    }

    #[test]
    fn cleave_status_summary_counts_terminal_and_non_terminal_states() {
        let children = vec![
            cleave::state::ChildState {
                child_id: 0,
                label: "done".to_string(),
                description: String::new(),
                scope: vec![],
                depends_on: vec![],
                status: cleave::state::ChildStatus::Completed,
                error: None,
                branch: None,
                worktree_path: None,
                backend: "native".to_string(),
                execute_model: None,
                provider_id: None,
                duration_secs: None,
                stdout: None,
                runtime: None,
                pid: None,
                started_at_unix_ms: None,
                last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        },
            cleave::state::ChildState {
                child_id: 1,
                label: "failed".to_string(),
                description: String::new(),
                scope: vec![],
                depends_on: vec![],
                status: cleave::state::ChildStatus::Failed,
                error: Some("boom".to_string()),
                branch: None,
                worktree_path: None,
                backend: "native".to_string(),
                execute_model: None,
                provider_id: None,
                duration_secs: None,
                stdout: None,
                runtime: None,
                pid: None,
                started_at_unix_ms: None,
                last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        },
            cleave::state::ChildState {
                child_id: 2,
                label: "exhausted".to_string(),
                description: String::new(),
                scope: vec![],
                depends_on: vec![],
                status: cleave::state::ChildStatus::UpstreamExhausted,
                error: Some("429".to_string()),
                branch: None,
                worktree_path: None,
                backend: "native".to_string(),
                execute_model: None,
                provider_id: None,
                duration_secs: None,
                stdout: None,
                runtime: None,
                pid: None,
                started_at_unix_ms: None,
                last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        },
            cleave::state::ChildState {
                child_id: 3,
                label: "pending".to_string(),
                description: String::new(),
                scope: vec![],
                depends_on: vec![],
                status: cleave::state::ChildStatus::Pending,
                error: None,
                branch: None,
                worktree_path: None,
                backend: "native".to_string(),
                execute_model: None,
                provider_id: None,
                duration_secs: None,
                stdout: None,
                runtime: None,
                pid: None,
                started_at_unix_ms: None,
                last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        },
        ];

        let (completed, failed, upstream_exhausted, unfinished) =
            summarize_cleave_child_statuses(&children);
        assert_eq!(
            (completed, failed, upstream_exhausted, unfinished),
            (1, 1, 1, 1)
        );
    }

    #[test]
    fn cleave_merge_result_reports_upstream_exhaustion_honestly() {
        let child = cleave::state::ChildState {
            child_id: 0,
            label: "noop-docs".to_string(),
            description: String::new(),
            scope: vec![],
            depends_on: vec![],
            status: cleave::state::ChildStatus::UpstreamExhausted,
            error: Some("429".to_string()),
            branch: None,
            worktree_path: None,
            backend: "native".to_string(),
            execute_model: None,
            provider_id: None,
            duration_secs: None,
            stdout: None,
            runtime: None,
            pid: None,
            started_at_unix_ms: None,
            last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        };

        let line = format_cleave_merge_result(
            Some(&child),
            "noop-docs",
            &cleave::orchestrator::MergeOutcome::NoChanges,
        );
        assert!(
            line.contains("upstream exhausted"),
            "unexpected line: {line}"
        );
        assert!(
            !line.contains("completed (no changes)"),
            "line should not claim completion: {line}"
        );
    }

    #[test]
    fn hidden_bench_run_task_cli_parses() {
        let cli = Cli::try_parse_from([
            "omegon",
            "bench",
            "run-task",
            "--prompt",
            "benchmark prompt",
            "--usage-json",
            "usage.json",
        ])
        .expect("bench run-task should parse");

        match cli.command.unwrap() {
            Commands::Bench {
                action: BenchAction::RunTask { prompt, usage_json },
            } => {
                assert_eq!(prompt, "benchmark prompt");
                assert_eq!(usage_json, PathBuf::from("usage.json"));
            }
            _ => panic!("wrong command parsed"),
        }
    }

    #[test]
    fn benchmark_usage_summary_accumulates_run_totals() {
        let mut summary = BenchmarkUsageSummary::default();
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            321,
            200_000,
            omegon_traits::ContextComposition {
                system_tokens: 100,
                tool_schema_tokens: 50,
                conversation_tokens: 75,
                memory_tokens: 10,
                tool_history_tokens: 20,
                thinking_tokens: 30,
                free_tokens: 199_715,
            },
            123,
            45,
            6,
            None,
        );
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            111,
            200_000,
            omegon_traits::ContextComposition {
                system_tokens: 120,
                tool_schema_tokens: 55,
                conversation_tokens: 90,
                memory_tokens: 12,
                tool_history_tokens: 24,
                thinking_tokens: 36,
                free_tokens: 199_663,
            },
            77,
            9,
            4,
            None,
        );

        assert_eq!(summary.input_tokens, 200);
        assert_eq!(summary.output_tokens, 54);
        assert_eq!(summary.cache_tokens, 10);
        assert_eq!(summary.estimated_tokens, 432);
        assert_eq!(summary.context_window, 200_000);
        assert_eq!(
            summary.context_composition,
            omegon_traits::ContextComposition {
                system_tokens: 120,
                tool_schema_tokens: 55,
                conversation_tokens: 90,
                memory_tokens: 12,
                tool_history_tokens: 24,
                thinking_tokens: 36,
                free_tokens: 199_663,
            }
        );
    }

    #[test]
    fn benchmark_usage_json_writer_persists_summary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bench").join("usage.json");
        let summary = BenchmarkUsageSummary {
            model: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            input_tokens: 123,
            output_tokens: 45,
            cache_tokens: 6,
            estimated_tokens: 321,
            context_window: 200_000,
            context_composition: omegon_traits::ContextComposition {
                system_tokens: 100,
                tool_schema_tokens: 50,
                conversation_tokens: 75,
                memory_tokens: 10,
                tool_history_tokens: 20,
                thinking_tokens: 30,
                free_tokens: 199_715,
            },
            provider_telemetry: None,
        };

        write_benchmark_usage_json(&path, &summary).unwrap();
        let written: BenchmarkUsageSummary =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written, summary);
    }

    fn anthropic_subscription_automation_warning_only_for_headless_anthropic_oauth() {
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::set_var("ANTHROPIC_OAUTH_TOKEN", "subscription-token");
        }
        let cli = Cli::try_parse_from(vec!["omegon", "--prompt", "hello"]).unwrap();
        let warning = anthropic_subscription_automation_warning(&cli)
            .expect("expected warning for headless anthropic oauth");
        assert!(warning.contains("operator agency wins"), "got: {warning}");
        assert!(warning.contains("ANTHROPIC_API_KEY"), "got: {warning}");

        let openai_cli = Cli::try_parse_from(vec![
            "omegon",
            "--model",
            "openai:gpt-4o",
            "--prompt",
            "hello",
        ])
        .unwrap();
        assert!(anthropic_subscription_automation_warning(&openai_cli).is_none());

        let interactive_cli = Cli::try_parse_from(vec!["omegon"]).unwrap();
        assert!(anthropic_subscription_automation_warning(&interactive_cli).is_none());

        unsafe {
            std::env::remove_var("ANTHROPIC_OAUTH_TOKEN");
        }
    }

    #[test]
    fn cleave_preflight_allows_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::fs::write(dir.path().join("README.md"), "hi\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        let result = ensure_clean_cleave_repo(dir.path());
        assert!(
            result.is_ok(),
            "clean repo should pass preflight: {result:?}"
        );
    }

    #[test]
    fn cleave_preflight_blocks_dirty_repo_and_lists_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::fs::write(dir.path().join("README.md"), "hi\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        std::fs::write(dir.path().join("dirty.txt"), "nope\n").unwrap();

        let err = ensure_clean_cleave_repo(dir.path())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cleave preflight failed"),
            "unexpected error: {err}"
        );
        assert!(
            err.contains("dirty.txt"),
            "missing dirty path in error: {err}"
        );
    }
}

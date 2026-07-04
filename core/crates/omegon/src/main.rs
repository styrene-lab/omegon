//! Omegon — Rust-native agent loop and lifecycle engine.
#![allow(dead_code)] // Phase 0 scaffold — fields/methods used as implementation fills in
//!
//! Phase 0: Headless agent loop for cleave children and standalone use.
//! Phase 1: Process owner with TUI bridge subprocess.
//! Phase 2: Native TUI rendering.
//! Phase 3: Native LLM provider clients.

use crate::conversation::PlanAction;
use clap::{Args, Parser, Subcommand};
use crossterm::ExecutableCommand;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{EnterAlternateScreen, disable_raw_mode, enable_raw_mode};
use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[cfg(test)]
pub(crate) static GLOBAL_TEST_ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

#[allow(clippy::await_holding_refcell_ref)] // single-threaded LocalSet — no concurrent mutations
mod acp;
mod acp_plan_tasks;
mod acp_worker;
mod auth;
mod autonomy;
mod backend;
mod behavior;
mod bootstrap;
mod bridge;
pub mod bus;
mod cleave;
mod cleave_smoke;
mod clipboard;
pub mod code_act;
pub mod code_act_proxy;
pub mod code_act_sandbox;
mod codex_config;
mod command_registry;
mod context;
mod control;
mod control_actions;
mod control_runtime;
mod control_tls;
mod embedding;
mod execution_substrate;
pub mod extensions;
pub mod features;
pub(crate) mod filelock;
mod first_run;
mod host_context;
mod ipc;
#[cfg(feature = "local-embeddings")]
mod local_embedding;
mod migrate;
mod shadow_context;
mod skills;
mod smoke;
mod surfaces;
mod switch;
mod task_spawn;
mod tdd;
mod test_support;
pub mod tool_schema;
mod update;
mod upstream_errors;
mod usage;
mod workspace;

mod agent_manifest;
mod armory;
mod bundle_verify;
pub mod capabilities;
mod catalog;
mod checkpoint;
mod child_agent;
mod conversation;
mod eval;
mod evidence;
mod extension_cli;
mod extension_registry;
mod lifecycle;
mod r#loop;
mod model_registry;
mod mqtt_bridge;
mod ollama;
mod packages;
mod paths;
mod permissions;
mod pkl_modules;
mod plan;
mod plugin_cli;
mod plugins;
mod project_rules;
mod prompt;
mod prompts;
mod providers;
mod route;
pub mod routing;
mod secret_cli;
mod sentry;
mod session;
mod session_router;
pub mod settings;
mod setup;
mod smoke_surface;
mod startup;
pub mod status;
mod task_tree;
pub mod tool_registry;
mod tools;
mod triggers;
mod tui;
mod ui_runtime;
pub mod util;
mod web;
mod workflow;

pub mod nex;

use anyhow::Context;
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

    /// Override context class (compact/standard/extended/massive).
    #[arg(long)]
    context_class: Option<String>,

    /// Activate a persona by name at startup (headless/child mode).
    #[arg(long)]
    persona: Option<String>,

    /// Enable slim runtime mode — reduce prompt and tool surface for quick interactive work.
    #[arg(long)]
    slim: bool,

    /// Force the default full/architect posture, overriding profile defaults and --slim.
    #[arg(long)]
    full: bool,

    /// Set the behavioral posture. Built-in: explorator/fabricator/architect/devastator.
    /// Custom postures can be defined in ~/.omegon/postures/<name>.pkl.
    /// Architect (default): orchestrator mode — plan and delegate to local models.
    /// Explorator: lean direct-execution mode (implies --slim).
    /// Fabricator: balanced — small tasks inline, larger ones delegated.
    /// Devastator: maximum-force deep reasoning.
    #[arg(long, value_parser = parse_posture)]
    posture: Option<String>,

    /// Shorthand for --posture architect.
    #[arg(long, conflicts_with = "posture")]
    architect: bool,

    /// Shorthand for --posture fabricator.
    #[arg(long, conflicts_with = "posture")]
    fabricator: bool,

    /// Shorthand for --posture explorator (implies --slim).
    #[arg(long, conflicts_with = "posture")]
    explorator: bool,

    /// Shorthand for --posture devastator.
    #[arg(long, conflicts_with = "posture")]
    devastator: bool,

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

    /// Disable all filesystem boundary checks. The agent can read/write
    /// anywhere on the host filesystem without permission prompts.
    /// Use for quick untethered work when you trust the model and want
    /// zero friction. Incompatible with --sandboxed/--oci.
    #[arg(long, conflicts_with = "sandboxed")]
    dangerously_bypass_permissions: bool,

    /// Run the entire omegon session inside an OCI container. The current
    /// directory is mounted at /work, everything else is kernel-isolated.
    /// Use for adversarial testing or when you want hard enforcement even
    /// in interactive mode. Requires podman or docker.
    #[arg(long, alias = "oci", conflicts_with = "dangerously_bypass_permissions")]
    sandboxed: bool,

    /// OCI image to use with --oci/--sandboxed.
    #[arg(long, global = true)]
    oci_image: Option<String>,

    /// OCI runtime to use with --oci/--sandboxed (podman or docker).
    #[arg(long, global = true)]
    oci_runtime: Option<String>,
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
enum TddAction {
    /// Watch a command and emit deterministic red→green TDD savepoint events.
    Watch {
        /// Filename extension to watch, such as rs, js, py, go, java, or txt.
        #[arg(short, long, alias = "ext")]
        filetype: Option<String>,

        /// Path to watch, relative to --cwd. Defaults to the current directory.
        #[arg(long = "watch", value_name = "PATH")]
        watch_paths: Vec<PathBuf>,

        /// OpenSpec change to attribute savepoints to.
        #[arg(long)]
        change: Option<String>,

        /// OpenSpec scenario id to attribute savepoints to.
        #[arg(long)]
        scenario: Option<String>,

        /// Task id to attribute savepoints to.
        #[arg(long)]
        task: Option<String>,

        /// Run once to establish baseline and exit.
        #[arg(long)]
        once: bool,

        /// Emit the baseline run as a raw savepoint event.
        #[arg(long)]
        emit_baseline: bool,

        /// Persist failing runs as explicit failure evidence.
        #[arg(long)]
        persist_failures: bool,

        /// Kill the test command after this many seconds and classify it as failing.
        #[arg(long)]
        timeout_secs: Option<u64>,

        /// Command to run. Place after --.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// Query recorded TDD evidence.
    Evidence {
        /// Command hash to query.
        #[arg(long)]
        command_hash: Option<String>,

        /// OpenSpec change to query.
        #[arg(long)]
        change: Option<String>,

        /// OpenSpec scenario id to query.
        #[arg(long)]
        scenario: Option<String>,

        /// Task id to query.
        #[arg(long)]
        task: Option<String>,

        /// Current worktree diff hash for stale-pass detection.
        #[arg(long)]
        current_diff_hash: Option<String>,

        /// Compute the current worktree diff hash for stale-pass detection.
        #[arg(long)]
        current: bool,

        /// Scope paths used when computing --current.
        #[arg(long = "scope", value_name = "PATH")]
        scopes: Vec<PathBuf>,

        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProjectRulesAction {
    /// Check project rules against current evidence and OpenSpec read models.
    Check {
        /// Evaluation context, such as default, local, ci, release, or archive.
        #[arg(long, default_value = "default")]
        context: String,

        /// Emit JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
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

        /// Agent manifest to load from catalog (id or path to bundle dir).
        #[arg(long)]
        agent: Option<String>,

        /// Trusted Auspex local web proxy identity JSON file.
        #[arg(long, value_name = "PATH")]
        web_trusted_proxy_identity: Option<PathBuf>,

        /// Require matching trusted proxy identity headers on web principal routes.
        #[arg(long)]
        require_web_proxy_identity: bool,

        #[command(flatten)]
        tls: ControlTlsArgs,
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

        /// Trusted Auspex local web proxy identity JSON file.
        #[arg(long, value_name = "PATH")]
        web_trusted_proxy_identity: Option<PathBuf>,

        /// Require matching trusted proxy identity headers on web principal routes.
        #[arg(long)]
        require_web_proxy_identity: bool,

        #[command(flatten)]
        tls: ControlTlsArgs,
    },

    /// Unified authentication management.
    /// Usage: omegon auth <status|login|logout|unlock> [provider]
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// Migrate settings from another CLI agent tool.
    /// Usage: omegon migrate [auto|claude-code|pi|codex|cursor|aider|continue|copilot|windsurf]
    Migrate {
        /// Source to migrate from. "auto" detects all available tools.
        #[arg(default_value = "auto")]
        source: String,
    },

    /// Evaluate an agent bundle against a test suite. Produces a score card.
    Eval {
        /// Agent manifest to evaluate (id or path to bundle dir).
        #[arg(long)]
        agent: String,

        /// Path to eval suite TOML file.
        #[arg(long)]
        suite: String,

        /// Override the model for this eval run. Run the same suite with
        /// different models to avoid overfitting to a single provider.
        #[arg(long)]
        model_override: Option<String>,
    },

    /// Manage plugins — install, list, remove, update.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },

    /// Manage extensions — install, list, remove, update, enable, disable.
    Extension {
        #[command(subcommand)]
        action: ExtensionAction,
    },

    /// Browse the upstream Armory for extensions, plugins, skills, and agents.
    Armory {
        #[command(subcommand)]
        action: ArmoryAction,
    },

    /// Manage secrets — set, list, delete.
    Secret {
        #[command(subcommand)]
        action: SecretAction,
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

        /// Deprecated — use --latest. Kept for backward compatibility.
        #[arg(long, hide = true)]
        latest_rc: bool,
    },

    /// Run a bounded headless task — process a prompt, emit structured output, exit.
    /// Designed for k8s Jobs/CronJobs, CI pipelines, and scripted automation.
    ///
    /// Accepts a task spec file (TOML) as positional argument, or inline flags.
    /// Task spec fields can be overridden by flags.
    ///
    /// Exit codes: 0=completed, 1=error, 2=upstream exhausted, 3=timeout
    ///
    /// Examples:
    ///   omegon run task.toml
    ///   omegon run --prompt "Review open PRs" --max-turns 10
    ///   omegon run task.toml --model anthropic:claude-opus-4-6
    Run {
        /// Task spec file (TOML). Declares prompt, bounds, agent settings, output.
        /// All fields can be overridden by flags.
        task_spec: Option<PathBuf>,

        /// Task prompt (inline). Overrides task spec.
        #[arg(long)]
        prompt: Option<String>,

        /// Task prompt from file. Overrides task spec.
        #[arg(long)]
        prompt_file: Option<PathBuf>,

        /// Write structured JSON result to this path (default: stdout). Overrides task spec.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Maximum agent turns. Overrides task spec.
        #[arg(long)]
        max_turns: Option<u32>,

        /// Wall-clock timeout in seconds. Overrides task spec.
        #[arg(long)]
        timeout: Option<u64>,

        /// Total token budget (input + output). Overrides task spec.
        #[arg(long)]
        token_budget: Option<u64>,

        /// Agent manifest (Pkl file or bundle directory).
        #[arg(long)]
        manifest: Option<String>,
    },

    /// Run deterministic, language-agnostic TDD red→green savepoint workflows.
    Tdd {
        #[command(subcommand)]
        action: TddAction,
    },

    /// Evaluate project-scoped rules against evidence and lifecycle read models.
    ProjectRules {
        #[command(subcommand)]
        action: ProjectRulesAction,
    },

    /// Manage Ollama integration — register, status, diagnostics.
    /// Usage: omegon ollama <register|unregister|status>
    Ollama {
        #[command(subcommand)]
        action: OllamaAction,
    },

    /// Run as an ACP (Agent Client Protocol) agent server.
    /// Default: stdio transport (for editor subprocess spawning).
    /// With --listen: WebSocket transport (for remote/k8s deployment).
    Acp {
        /// Agent manifest to load from catalog (id or path to bundle dir).
        #[arg(long)]
        agent: Option<String>,
        /// Listen on a network address instead of stdio.
        /// Starts a WebSocket server at ws://<addr>/acp, or wss:// with TLS, with health probes.
        /// Example: --listen 0.0.0.0:7842
        #[arg(long)]
        listen: Option<String>,

        #[command(flatten)]
        tls: ControlTlsArgs,
    },

    /// Manage local embedding models for semantic memory search.
    Embedding {
        #[command(subcommand)]
        action: EmbeddingAction,
    },

    /// Run the sentry autonomous task executor — long-running process that
    /// evaluates triggers, claims tasks from a board, and executes them.
    Sentry {
        /// Path to sentry.toml config file.
        #[arg(long, default_value = "sentry.toml")]
        config: std::path::PathBuf,

        /// Control plane HTTP port.
        #[arg(long, default_value = "7842")]
        control_port: u16,

        /// Require the exact control port instead of auto-falling back.
        #[arg(long)]
        strict_port: bool,
    },

    /// Audit design-tree state for suspicious lifecycle drift.
    #[command(hide = true)]
    Doctor,

    /// Manage skill bundles — inspect, import, install, and diagnose user/project/extension skills.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },

    /// Manage bundled agents — list available agents and install them to ~/.omegon/catalog/.
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },

    /// Manage personas — list, create, and delete agent personas.
    Persona {
        #[command(subcommand)]
        action: PersonaAction,
    },

    /// Hidden benchmark-oriented commands used by the local comparison harness.
    #[command(hide = true)]
    Bench {
        #[command(subcommand)]
        action: BenchAction,
    },

    /// Manage project tasks — list, create, update, and delete tasks.
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },

    /// Manage Nex sandbox profiles — init, list, inspect.
    Nex {
        #[command(subcommand)]
        action: NexAction,
    },
}

#[derive(Clone, Debug, Default, Args)]
struct ControlTlsArgs {
    /// PEM certificate chain for the control-plane TLS listener.
    #[arg(long = "rpc-tls-cert", alias = "control-tls-cert", value_name = "PATH")]
    cert: Option<PathBuf>,

    /// PEM private key for the control-plane TLS listener.
    #[arg(long = "rpc-tls-key", alias = "control-tls-key", value_name = "PATH")]
    key: Option<PathBuf>,

    /// Optional PEM client CA bundle. When set, client certificates are required.
    #[arg(
        long = "rpc-tls-client-ca",
        alias = "control-tls-client-ca",
        value_name = "PATH"
    )]
    client_ca: Option<PathBuf>,
}

impl ControlTlsArgs {
    fn into_config(self) -> anyhow::Result<Option<control_tls::ControlTlsConfig>> {
        match (self.cert, self.key, self.client_ca) {
            (None, None, None) => Ok(None),
            (Some(cert_chain_path), Some(private_key_path), client_ca_path) => {
                Ok(Some(control_tls::ControlTlsConfig {
                    cert_chain_path,
                    private_key_path,
                    client_ca_path,
                }))
            }
            (None, None, Some(_)) => {
                anyhow::bail!("--rpc-tls-client-ca requires --rpc-tls-cert and --rpc-tls-key")
            }
            _ => anyhow::bail!("--rpc-tls-cert and --rpc-tls-key must be provided together"),
        }
    }
}

#[derive(Subcommand)]
enum NexAction {
    /// Generate a starter .omegon/nex/project.toml for this project.
    Init,
    /// List available sandbox profiles (built-in + custom).
    List,
    /// Show details of a specific profile.
    Inspect {
        /// Profile name or hash prefix.
        name: String,
    },
    /// Export a profile as a docker-compose.yml service definition.
    Compose {
        /// Profile name or hash prefix.
        name: String,
        /// Service name in the compose file (defaults to profile name).
        #[arg(long)]
        service: Option<String>,
    },
    /// Export a Kubernetes/Cilium NetworkPolicy for egress filtering.
    /// Use in clusters where iptables is unavailable (eBPF CNI, service mesh).
    #[command(name = "networkpolicy")]
    NetworkPolicy {
        /// Profile name (uses its egress filter), or "sandboxed" for the
        /// default --sandboxed allowlist.
        #[arg(default_value = "sandboxed")]
        source: String,
    },
    /// Check container runtime availability.
    Status,
}

#[derive(Subcommand)]
enum SkillsAction {
    /// List skills — bundled, user-installed, and project-local.
    List,
    /// Diagnose local/project skill ecosystems and onboarding readiness.
    Doctor,
    /// Install all bundled skills, or install one skill from the upstream armory.
    Install {
        /// Armory skill name or skills/<name>. Omit to install bundled skills.
        name: Option<String>,
    },
    /// Show details of a specific skill.
    Get {
        /// Skill name (e.g., "rust", "security").
        name: String,
    },
    /// Create a new skill from a SKILL.md file.
    Create {
        /// Skill name (kebab-case).
        name: String,
        /// Path to SKILL.md content file.
        #[arg(long)]
        content: std::path::PathBuf,
        /// Install as project-local skill (in .omegon/skills/) instead of user-global.
        #[arg(long)]
        project_local: bool,
    },
    /// Import a Claude/Omegon skill bundle directory or SKILL.md file.
    Import {
        /// Path to a skill directory containing SKILL.md, or a SKILL.md file.
        path: std::path::PathBuf,
        /// Import as project-local skill under .omegon/skills/ instead of user-global.
        #[arg(long)]
        project: bool,
        /// Overwrite an existing imported skill bundle.
        #[arg(long)]
        force: bool,
    },
    /// Delete a skill by name.
    Delete {
        /// Skill name to delete.
        name: String,
    },
}

#[derive(Subcommand)]
enum CatalogAction {
    /// List bundled agents and their installation status.
    List,
    /// Install agents — fetches from upstream armory, falls back to bundled.
    Install {
        /// Skip upstream fetch and install the bundled (binary-embedded) copies only.
        /// Use this on airgapped systems or when you don't want network access.
        #[arg(long)]
        offline: bool,
    },
    /// Remove a catalog agent by ID.
    Remove {
        /// Agent ID (e.g., "styrene.coding-agent").
        id: String,
    },
}

#[derive(Subcommand)]
enum ArmoryAction {
    /// Browse upstream Armory inventory.
    Browse {
        /// Filter by kind.
        #[arg(long, value_enum, default_value_t = armory::ArmoryKind::All)]
        kind: armory::ArmoryKind,
        /// Search query matching id, name, description, category, or manifest id.
        query: Option<String>,
        /// Emit JSON instead of a terminal summary.
        #[arg(long)]
        json: bool,
    },
    /// Search upstream Armory inventory.
    Search {
        /// Search query matching id, name, description, category, or manifest id.
        query: String,
        /// Filter by kind.
        #[arg(long, value_enum, default_value_t = armory::ArmoryKind::All)]
        kind: armory::ArmoryKind,
        /// Emit JSON instead of a terminal summary.
        #[arg(long)]
        json: bool,
    },
    /// Install an item from the upstream Armory.
    Install {
        /// Item name, scoped path (skills/security), URL, or extension source.
        target: String,
        /// Force a specific install surface.
        #[arg(long, value_enum)]
        kind: Option<armory::ArmoryKind>,
    },
}

#[derive(Subcommand)]
enum PersonaAction {
    /// List available personas and tones.
    List,
    /// Create a new persona from a directive file.
    Create {
        /// Persona name.
        name: String,
        /// Path to PERSONA.md directive file.
        #[arg(long)]
        directive: std::path::PathBuf,
        /// One-line description.
        #[arg(long, default_value = "")]
        description: String,
        /// Badge emoji.
        #[arg(long)]
        badge: Option<String>,
    },
    /// Delete a persona by ID.
    Delete {
        /// Persona ID (e.g., "user.my-persona").
        id: String,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    /// List all project tasks.
    List,
    /// Show a specific task.
    Get {
        /// Task ID (slug).
        id: String,
    },
    /// Create a new task.
    Create {
        /// Task title.
        title: String,
        /// Task body (prompt/description). If omitted, reads from stdin.
        #[arg(long)]
        body: Option<String>,
        /// Priority: low, medium, high, critical.
        #[arg(long, default_value = "medium")]
        priority: String,
        /// Link to a design node.
        #[arg(long)]
        design_node: Option<String>,
        /// Link to an openspec change.
        #[arg(long)]
        openspec: Option<String>,
        /// Comma-separated tags.
        #[arg(long)]
        tags: Option<String>,
        /// Comma-separated dependency task IDs.
        #[arg(long)]
        depends_on: Option<String>,
    },
    /// Update a task's status.
    Status {
        /// Task ID (slug).
        id: String,
        /// New status: todo, in_progress, done, blocked, failed.
        status: String,
    },
    /// Delete a task.
    Delete {
        /// Task ID (slug).
        id: String,
    },
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

        /// Enable slim runtime mode for this benchmark run.
        #[arg(long)]
        slim: bool,
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

#[derive(Subcommand)]
enum ExtensionAction {
    /// Scaffold a new extension project with manifest, Cargo.toml, and src/main.rs.
    Init {
        /// Extension name (lowercase, alphanumeric + hyphens).
        name: String,
    },
    /// Install an extension by name from the armory, or from a git URL, tarball URL, or local path.
    Install {
        /// Extension name, git URL, tarball URL, or local directory path.
        uri: String,
        /// Pin to a specific version (only for armory registry installs).
        #[arg(long)]
        version: Option<String>,
    },
    /// List installed extensions (use --available to show all armory extensions).
    List {
        /// Show all available extensions from the armory, not just installed.
        #[arg(long)]
        available: bool,
    },
    /// Search available extensions in the armory registry.
    Search {
        /// Search query (matches name, description, category). Omit to list all.
        query: Option<String>,
    },
    /// Remove an installed extension.
    Remove {
        /// Extension directory name.
        name: String,
    },
    /// Update installed extensions (git pull).
    Update {
        /// Extension name to update. Omit to update all.
        name: Option<String>,
    },
    /// Enable a disabled extension.
    Enable {
        /// Extension name.
        name: String,
    },
    /// Disable an extension (prevents spawning on next startup).
    Disable {
        /// Extension name.
        name: String,
    },
}

#[derive(Subcommand)]
enum SecretAction {
    /// Store a secret value or recipe.
    Set {
        /// Secret name (e.g. GITHUB_TOKEN, VOX_DISCORD_BOT_TOKEN).
        name: String,
        /// Raw secret value. Stored in system keyring. Omit to read from stdin.
        value: Option<String>,
        /// Recipe form instead of raw value (env:VAR, cmd:..., vault:path#key, file:/path).
        #[arg(long)]
        recipe: Option<String>,
        /// Read secret value from stdin (avoids shell history / ps exposure).
        #[arg(long)]
        stdin: bool,
    },
    /// List configured secrets (values are never shown).
    List,
    /// Delete a secret and its recipe.
    Delete {
        /// Secret name to delete.
        name: String,
    },
}

#[derive(Subcommand)]
enum EmbeddingAction {
    /// Download an embedding model for local semantic search.
    /// Default: all-MiniLM-L6-v2 (22M params, 384-dim, ~80MB).
    Download {
        /// Model name (HuggingFace repo ID). Default: sentence-transformers/all-MiniLM-L6-v2
        #[arg(long, default_value = "sentence-transformers/all-MiniLM-L6-v2")]
        model: String,
    },
    /// Show status of local embedding models.
    Status,
    /// Compute embeddings for all facts that don't have one yet.
    Backfill,
}

#[derive(Subcommand)]
enum OllamaAction {
    /// Register omegon as an Ollama launch integration.
    /// Creates ~/.ollama/integrations/omegon.json so `ollama launch omegon` works.
    Register,
    /// Remove the Ollama launch integration registration.
    Unregister,
    /// Show Ollama status: reachability, models, VRAM, registration state.
    Status,
}

/// Build a typed `BusRequestSink` that the runtime hands to features
/// (currently just cleave) so they can communicate with the broadcast
/// channel via the `BusRequest` contract instead of holding a
/// `broadcast::Sender<AgentEvent>` directly.
///
/// Today the only variant the sink translates is
/// `BusRequest::EmitAgentEvent`, which forwards onto the broadcast
/// channel. Other `BusRequest` variants pushed through here are dropped
/// silently — features can still surface them through the conventional
/// `Vec<BusRequest>` return path from `on_event`. As more long-running
/// features need mid-execution communication, additional variants get
/// handled here.
fn build_runtime_bus_request_sink(
    events_tx: tokio::sync::broadcast::Sender<AgentEvent>,
) -> omegon_traits::BusRequestSink {
    omegon_traits::BusRequestSink::from_fn(move |request| {
        if let omegon_traits::BusRequest::EmitAgentEvent { event } = request {
            let _ = events_tx.send(*event);
        }
    })
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

async fn resolve_current_model_intent_route(
    route_controller: &std::sync::Arc<route::RouteController>,
) -> Option<route::RouteSnapshot> {
    let mut inventory = routing::ProviderInventory::probe();
    inventory.probe_ollama().await;
    let intent = route_controller.snapshot().await.intent;
    let candidate = route::select_candidate_for_intent(&intent, &inventory)?;
    let target = format!("{}:{}", candidate.provider_id, candidate.model_id);
    let bridge = providers::auto_detect_bridge(&target).await?;
    route_controller
        .resolve_route_from_intent_candidate(candidate, bridge)
        .await
        .ok()
}

fn persist_model_intent(
    cwd: &std::path::Path,
    intent: &crate::route::ModelIntent,
) -> anyhow::Result<()> {
    let mut profile = settings::Profile::load(cwd);
    profile.model_intent = Some(settings::ProfileModelIntent::from_route_intent(intent));
    profile.save(cwd)
}

fn parse_bool_env(name: &str) -> Option<bool> {
    let raw = std::env::var(name).ok()?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enabled" => Some(true),
        "0" | "false" | "no" | "off" | "disabled" => Some(false),
        _ => None,
    }
}

fn maybe_start_mqtt_bridge(
    cwd: &Path,
    instance_id: String,
    events_tx: broadcast::Sender<AgentEvent>,
) -> Option<mqtt_bridge::MqttBridgeHandle> {
    let profile = settings::Profile::load(cwd);
    let mqtt = &profile.integrations.mqtt;
    let enabled = parse_bool_env("OMEGON_MQTT_ENABLED")
        .or_else(|| parse_bool_env("OMEGON_MQTT"))
        .or(mqtt.enabled)
        .unwrap_or(false);
    if !enabled {
        tracing::debug!("MQTT bridge disabled by profile/default policy");
        return None;
    }

    let broker_host = std::env::var("OMEGON_MQTT_HOST")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| mqtt.broker_host.clone())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let broker_port = std::env::var("OMEGON_MQTT_PORT")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .or(mqtt.broker_port)
        .unwrap_or(mqtt_bridge::DEFAULT_BROKER_PORT);

    Some(mqtt_bridge::start_mqtt_bridge(
        mqtt_bridge::MqttBridgeConfig {
            instance_id,
            broker_host,
            broker_port,
            ..Default::default()
        },
        events_tx,
    ))
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

/// Global startup instant — set once at process start.
static STARTUP_INSTANT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Get time since process start in milliseconds.
pub fn startup_elapsed_ms() -> u64 {
    STARTUP_INSTANT
        .get()
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = STARTUP_INSTANT.set(std::time::Instant::now());
    let mut cli = Cli::parse();

    if cli.dangerously_bypass_permissions {
        // SAFETY: called before spawning any threads that read this var.
        unsafe { std::env::set_var("OMEGON_BYPASS_PERMISSIONS", "1") };
        eprintln!(
            "⚠ --dangerously-bypass-permissions: all filesystem boundary \
             checks disabled. The agent can read/write anywhere."
        );
    }

    // If --sandboxed/--oci is set and we're NOT already inside a container,
    // re-exec the entire omegon session inside an OCI container.
    if cli.sandboxed
        && std::env::var("OMEGON_INSIDE_SANDBOX").is_err()
        && std::env::var("OMEGON_INSIDE_OCI").is_err()
    {
        return run_sandboxed(&cli).await;
    } else if cli.sandboxed {
        anyhow::bail!(
            "--oci/--sandboxed was requested while already running inside an OCI container"
        );
    }

    // When launched via `ollama launch omegon`, the --ollama-model flag
    // (set by Ollama) should override the --model CLI flag.
    if cli.ollama_integration
        && let Some(ref model) = cli.ollama_model
    {
        cli.model = model.clone();
        tracing::info!(model = %model, "using Ollama-provided model");
    }

    // Priority: RUST_LOG env > --log-level flag > "info" default.
    // Interactive mode owns stderr via ratatui. ACP stdio owns stdout/stdin
    // for JSON-RPC, and some clients surface stderr in transcripts, so keep
    // stdio ACP logs file-only as well. ACP websocket/server mode can log to
    // stderr because stdout is not the protocol transport there.
    let is_interactive = matches!(cli.command, Some(Commands::Interactive) | None)
        && cli.prompt.is_none()
        && cli.prompt_file.is_none();
    let is_acp_stdio = matches!(cli.command, Some(Commands::Acp { listen: None, .. }));
    let logs_file_only = is_interactive || is_acp_stdio;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    // File-only modes: tracing MUST NOT go to stderr. Interactive TUI owns
    // stderr, and ACP stdio clients may serialize stderr as user-visible text.
    // Logs go to --log-file or ~/.config/omegon/omegon.log as default.
    // Other headless modes: stderr is fine.
    let _guard: Option<tracing_appender::non_blocking::WorkerGuard>;

    if logs_file_only {
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
                PluginAction::Install { uri } => {
                    plugin_cli::install(uri)?;
                }
                PluginAction::List => plugin_cli::list()?,
                PluginAction::Remove { name } => plugin_cli::remove(name)?,
                PluginAction::Update { name } => plugin_cli::update(name.as_deref())?,
            }
            Ok(())
        }
        Some(Commands::Extension { ref action }) => {
            match action {
                ExtensionAction::Init { name } => extension_cli::init(name)?,
                ExtensionAction::Install { uri, version } => {
                    armory::install_extension(uri, version.as_deref()).await?;
                }
                ExtensionAction::List { available } => {
                    if *available {
                        extension_registry::list_available().await?
                    } else {
                        extension_cli::list()?
                    }
                }
                ExtensionAction::Search { query } => {
                    extension_registry::search(query.as_deref()).await?
                }
                ExtensionAction::Remove { name } => extension_cli::remove(name)?,
                ExtensionAction::Update { name } => extension_cli::update(name.as_deref())?,
                ExtensionAction::Enable { name } => extension_cli::enable(name)?,
                ExtensionAction::Disable { name } => extension_cli::disable(name)?,
            }
            Ok(())
        }
        Some(Commands::Armory { ref action }) => {
            match action {
                ArmoryAction::Browse { kind, query, json } => {
                    armory::cmd_browse(*kind, query.as_deref(), *json, &cli.cwd).await?
                }
                ArmoryAction::Search { query, kind, json } => {
                    armory::cmd_browse(*kind, Some(query), *json, &cli.cwd).await?
                }
                ArmoryAction::Install { target, kind } => {
                    let kind = kind
                        .map(armory::ArmoryInstallKind::from)
                        .unwrap_or(armory::ArmoryInstallKind::Auto);
                    armory::cmd_install(target, kind, &cli.cwd).await?
                }
            }
            Ok(())
        }
        Some(Commands::Secret { ref action }) => {
            match action {
                SecretAction::Set {
                    name,
                    value,
                    recipe,
                    stdin,
                } => secret_cli::set(name, value.as_deref(), recipe.as_deref(), *stdin)?,
                SecretAction::List => secret_cli::list()?,
                SecretAction::Delete { name } => secret_cli::delete(name)?,
            }
            Ok(())
        }
        Some(Commands::Interactive) => run_interactive_command(&cli).await,
        Some(Commands::Serve {
            control_port,
            strict_port,
            ref agent,
            ref web_trusted_proxy_identity,
            require_web_proxy_identity,
            ref tls,
        }) => {
            run_embedded_command(
                control_port,
                strict_port,
                &cli.model,
                agent.as_deref(),
                web_trusted_proxy_identity.as_deref(),
                require_web_proxy_identity,
                tls.clone().into_config()?,
                cli.dangerously_bypass_permissions,
            )
            .await
        }
        Some(Commands::Embedded {
            control_port,
            strict_port,
            ref web_trusted_proxy_identity,
            require_web_proxy_identity,
            ref tls,
        }) => {
            run_embedded_command(
                control_port,
                strict_port,
                &cli.model,
                None,
                web_trusted_proxy_identity.as_deref(),
                require_web_proxy_identity,
                tls.clone().into_config()?,
                cli.dangerously_bypass_permissions,
            )
            .await
        }
        Some(Commands::Eval {
            agent,
            suite,
            model_override,
        }) => {
            if let Some(model) = &model_override {
                tracing::info!(model = %model, "eval: model override active — testing model portability");
            }
            let suite_path = std::path::PathBuf::from(suite);
            let card = eval::run_suite(&agent, &suite_path, model_override.as_deref()).await?;
            println!("{}", card.summary());
            let stored_path = eval::store::store(&card)?;
            println!("Score card stored at {}", stored_path.display());
            Ok(())
        }
        Some(Commands::Migrate { ref source }) => {
            let cwd = std::fs::canonicalize(&cli.cwd)?;
            let report = migrate::run(source, &cwd);
            println!("{}", report.summary());
            Ok(())
        }
        Some(Commands::Auth { ref action }) => run_auth_command(action).await,
        Some(Commands::Tdd { ref action }) => run_tdd_command(action).await,
        Some(Commands::ProjectRules { ref action }) => run_project_rules_command(&cli, action),
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
        Some(Commands::Acp {
            ref agent,
            ref listen,
            ref tls,
        }) => {
            if let Some(addr) = listen {
                acp::run_server(
                    addr,
                    &cli.model,
                    agent.as_deref(),
                    &cli.cwd,
                    tls.clone().into_config()?,
                    cli.dangerously_bypass_permissions,
                )
                .await
            } else {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(acp::run(
                        &cli.model,
                        agent.as_deref(),
                        &cli.cwd,
                        cli.dangerously_bypass_permissions,
                    ))
                    .await
            }
        }
        Some(Commands::Run {
            ref task_spec,
            ref prompt,
            ref prompt_file,
            ref output,
            max_turns,
            timeout,
            token_budget,
            ref manifest,
        }) => {
            // Load task spec from file, then overlay CLI flags
            let spec = task_spec
                .as_ref()
                .map(|path| load_task_spec(path))
                .transpose()?;

            let effective_prompt = prompt
                .as_deref()
                .or_else(|| spec.as_ref().and_then(|s| s.task.prompt.as_deref()));
            let effective_prompt_file = prompt_file.as_deref().or_else(|| {
                spec.as_ref()
                    .and_then(|s| s.task.prompt_file.as_deref())
                    .map(Path::new)
            });
            let effective_output = output.as_deref().or_else(|| {
                spec.as_ref()
                    .and_then(|s| s.output.as_ref())
                    .and_then(|o| o.path.as_deref())
                    .map(Path::new)
            });
            let effective_max_turns = max_turns
                .or_else(|| {
                    spec.as_ref()
                        .and_then(|s| s.bounds.as_ref())
                        .map(|b| b.max_turns)
                })
                .unwrap_or(30);
            let effective_timeout = timeout
                .or_else(|| {
                    spec.as_ref()
                        .and_then(|s| s.bounds.as_ref())
                        .map(|b| b.timeout_secs)
                })
                .unwrap_or(600);
            let effective_token_budget = token_budget.or_else(|| {
                spec.as_ref()
                    .and_then(|s| s.bounds.as_ref())
                    .and_then(|b| b.token_budget)
            });
            let effective_cwd = spec
                .as_ref()
                .and_then(|s| s.task.cwd.as_deref())
                .map(Path::new)
                .unwrap_or(&cli.cwd);

            // Apply agent settings from spec — model override
            let effective_model = spec
                .as_ref()
                .and_then(|s| s.agent.as_ref())
                .and_then(|a| a.model.clone())
                .unwrap_or_else(|| cli.model.clone());

            run_bounded_task(
                effective_prompt,
                effective_prompt_file,
                effective_output,
                effective_max_turns,
                effective_timeout,
                effective_token_budget,
                manifest.as_deref(),
                effective_cwd,
                &effective_model,
                &cli,
            )
            .await
        }
        Some(Commands::Ollama { ref action }) => run_ollama_command(action).await,
        Some(Commands::Embedding { ref action }) => run_embedding_command(action).await,
        Some(Commands::Sentry {
            ref config,
            control_port,
            strict_port,
        }) => run_sentry_command(config, control_port, strict_port, &cli).await,
        Some(Commands::Doctor) => run_doctor_command(&cli).await,
        Some(Commands::Skills { ref action }) => {
            let previous_cwd = std::env::current_dir()?;
            std::env::set_current_dir(&cli.cwd)?;
            let result = match action {
                SkillsAction::List => skills::cmd_list(),
                SkillsAction::Doctor => skills::cmd_doctor(),
                SkillsAction::Install { name } => {
                    if let Some(name) = name.as_deref() {
                        armory::cmd_install(name, armory::ArmoryInstallKind::Skill, &cli.cwd).await
                    } else {
                        skills::cmd_install()
                    }
                }
                SkillsAction::Get { name } => match skills::get_skill_details(name) {
                    Ok(details) => {
                        let manifest = &details.manifest;
                        println!("Skill: {}", manifest.name);
                        if !manifest.description.is_empty() {
                            println!("Description: {}", manifest.description);
                        }
                        if let Some(ref v) = manifest.version {
                            println!("Version: {v}");
                        }
                        if let Some(ref entry) = details.entry {
                            println!("Source: {}", entry.source);
                            println!("Editable: {}", entry.editable);
                            println!("Reloadable: {}", entry.reloadable);
                            if !entry.shadows.is_empty() {
                                println!("Shadows: {}", entry.shadows.join(", "));
                            }
                            if !entry.conflicts.is_empty() {
                                println!("Conflicts: {}", entry.conflicts.join(", "));
                                println!(
                                    "Recommended resolution: merge into a project-local skill so one activation slot injects one merged directive."
                                );
                            }
                        }
                        if !manifest.tags.is_empty() {
                            println!("Tags: {}", manifest.tags.join(", "));
                        }
                        if !manifest.triggers.is_empty() {
                            println!("Triggers: {}", manifest.triggers.join(", "));
                        }
                        if let Some(ref p) = manifest.posture {
                            println!("Posture: {p}");
                        }
                        println!("Path: {}", details.path.display());
                        println!("\n{}", details.body);
                        Ok(())
                    }
                    Err(e) => Err(e),
                },
                SkillsAction::Import {
                    path,
                    project,
                    force,
                } => skills::cmd_import(path, *project, *force),
                SkillsAction::Create {
                    name,
                    content,
                    project_local,
                } => {
                    let slug: String = name
                        .to_lowercase()
                        .replace(' ', "-")
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .collect();
                    if slug.is_empty() {
                        anyhow::bail!("invalid skill name");
                    }
                    let body = std::fs::read_to_string(content)?;
                    let skill_dir = if *project_local {
                        std::env::current_dir()?.join(".omegon/skills").join(&slug)
                    } else {
                        paths::omegon_home()?.join("skills").join(&slug)
                    };
                    std::fs::create_dir_all(&skill_dir)?;
                    std::fs::write(skill_dir.join("SKILL.md"), &body)?;
                    println!("Created skill '{slug}' at {}", skill_dir.display());
                    Ok(())
                }
                SkillsAction::Delete { name } => {
                    if name.contains('/')
                        || name.contains('\\')
                        || name.contains("..")
                        || name.contains('\0')
                    {
                        anyhow::bail!("invalid skill name: path traversal rejected");
                    }
                    let cwd = std::env::current_dir()?;
                    let project_dir = cwd.join(".omegon/skills").join(name);
                    let user_dir = paths::omegon_home()?.join("skills").join(name);
                    if project_dir.exists() {
                        std::fs::remove_dir_all(&project_dir)?;
                        println!("Deleted project-local skill '{name}'");
                    } else if user_dir.exists() {
                        std::fs::remove_dir_all(&user_dir)?;
                        println!("Deleted skill '{name}'");
                    } else {
                        anyhow::bail!("skill '{name}' not found");
                    }
                    Ok(())
                }
            };
            let restore_result = std::env::set_current_dir(previous_cwd);
            if let Err(err) = restore_result {
                return Err(err.into());
            }
            result
        }
        Some(Commands::Catalog { ref action }) => match action {
            CatalogAction::List => catalog::cmd_list(),
            CatalogAction::Install { offline } => catalog::cmd_install(*offline).await,
            CatalogAction::Remove { id } => {
                if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
                    anyhow::bail!("invalid agent ID: path traversal rejected");
                }
                let home = paths::omegon_home()?;
                let catalog_dir = home.join("catalog");
                let entries = catalog::list(&home);
                let entry = entries
                    .iter()
                    .find(|e| e.id == *id)
                    .ok_or_else(|| anyhow::anyhow!("catalog agent '{id}' not found"))?;
                if !entry.bundle_dir.starts_with(&catalog_dir) {
                    anyhow::bail!("refusing to remove agent outside catalog directory");
                }
                std::fs::remove_dir_all(&entry.bundle_dir)?;
                println!("Removed catalog agent '{id}'");
                Ok(())
            }
        },
        Some(Commands::Persona { ref action }) => match action {
            PersonaAction::List => {
                let (personas, tones) = plugins::persona_loader::scan_available();
                if personas.is_empty() && tones.is_empty() {
                    println!("No personas or tones installed.");
                    return Ok(());
                }
                if !personas.is_empty() {
                    println!("Personas ({}):\n", personas.len());
                    for p in &personas {
                        println!("  {:<32} {}", p.id, p.description);
                    }
                }
                if !tones.is_empty() {
                    println!("\nTones ({}):\n", tones.len());
                    for t in &tones {
                        println!("  {:<32} {}", t.id, t.description);
                    }
                }
                Ok(())
            }
            PersonaAction::Create {
                name,
                directive,
                description,
                badge,
            } => {
                let directive_content = std::fs::read_to_string(directive)?;
                let slug: String = name
                    .to_lowercase()
                    .replace(' ', "-")
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
                if slug.is_empty() {
                    anyhow::bail!("invalid persona name");
                }
                let home = paths::omegon_home()?;
                let persona_dir = home.join("armory/personas").join(&slug);
                std::fs::create_dir_all(&persona_dir)?;

                let id = format!("user.{slug}");
                let mut plugin = toml::Table::new();
                let mut plugin_section = toml::Table::new();
                plugin_section.insert("type".into(), "persona".into());
                plugin_section.insert("id".into(), id.clone().into());
                plugin_section.insert("name".into(), name.as_str().into());
                plugin_section.insert("version".into(), "1.0.0".into());
                plugin_section.insert("description".into(), description.as_str().into());
                plugin.insert("plugin".into(), toml::Value::Table(plugin_section));

                let mut persona = toml::Table::new();
                let mut identity = toml::Table::new();
                identity.insert("directive".into(), "PERSONA.md".into());
                persona.insert("identity".into(), toml::Value::Table(identity));
                if let Some(b) = badge {
                    let mut style = toml::Table::new();
                    style.insert("badge".into(), b.as_str().into());
                    persona.insert("style".into(), toml::Value::Table(style));
                }
                plugin.insert("persona".into(), toml::Value::Table(persona));

                std::fs::write(
                    persona_dir.join("plugin.toml"),
                    toml::to_string_pretty(&plugin)?,
                )?;
                std::fs::write(persona_dir.join("PERSONA.md"), &directive_content)?;
                println!("Created persona '{id}' at {}", persona_dir.display());
                Ok(())
            }
            PersonaAction::Delete { id } => {
                let (personas, _) = plugins::persona_loader::scan_available();
                match personas.iter().find(|p| p.id == *id) {
                    Some(p) => {
                        if p.path.exists() {
                            std::fs::remove_dir_all(&p.path)?;
                        }
                        println!("Deleted persona '{id}'");
                        Ok(())
                    }
                    None => anyhow::bail!("persona '{id}' not found"),
                }
            }
        },
        Some(Commands::Task { ref action }) => {
            let cwd = std::fs::canonicalize(&cli.cwd)?;
            match action {
                TaskAction::List => task_tree::cmd_list(&cwd),
                TaskAction::Get { id } => {
                    let task = task_tree::get_task(&cwd, id)?;
                    println!(
                        "{} {} [{}] — {}",
                        task.meta.status.icon(),
                        task.meta.id,
                        task.meta.status.as_str(),
                        task.meta.title
                    );
                    if !task.meta.depends_on.is_empty() {
                        println!("  depends: {}", task.meta.depends_on.join(", "));
                    }
                    if let Some(ref node) = task.meta.design_node_id {
                        println!("  design node: {node}");
                    }
                    if let Some(ref change) = task.meta.openspec_change {
                        println!("  openspec: {change}");
                    }
                    if !task.body.is_empty() {
                        println!("\n{}", task.body);
                    }
                    Ok(())
                }
                TaskAction::Create {
                    title,
                    body,
                    priority,
                    design_node,
                    openspec,
                    tags,
                    depends_on,
                } => {
                    let body_text = body.as_deref().unwrap_or("");
                    let mut task = task_tree::create_task(&cwd, title, body_text)?;

                    let mut modified = false;
                    if let Some(p) = task_tree::Priority::parse(priority)
                        && p != task_tree::Priority::Medium
                    {
                        task.meta.priority = p;
                        modified = true;
                    }
                    if let Some(node) = design_node {
                        task.meta.design_node_id = Some(node.clone());
                        modified = true;
                    }
                    if let Some(change) = openspec {
                        task.meta.openspec_change = Some(change.clone());
                        modified = true;
                    }
                    if let Some(tag_str) = tags {
                        task.meta.tags = tag_str.split(',').map(|s| s.trim().to_string()).collect();
                        modified = true;
                    }
                    if let Some(dep_str) = depends_on {
                        task.meta.depends_on =
                            dep_str.split(',').map(|s| s.trim().to_string()).collect();
                        modified = true;
                    }
                    if modified {
                        task_tree::save_task(&cwd, &task)?;
                    }

                    println!(
                        "Created task '{}' at {}",
                        task.meta.id,
                        task.file_path.display()
                    );
                    Ok(())
                }
                TaskAction::Status { id, status } => {
                    let s = task_tree::TaskStatus::parse(status)
                        .ok_or_else(|| anyhow::anyhow!("invalid status: {status}"))?;
                    let task = task_tree::update_status(&cwd, id, s)?;
                    println!(
                        "{} {} — {}",
                        task.meta.status.icon(),
                        task.meta.id,
                        task.meta.title
                    );
                    Ok(())
                }
                TaskAction::Delete { id } => {
                    task_tree::delete_task(&cwd, id)?;
                    println!("Deleted task '{id}'");
                    Ok(())
                }
            }
        }
        Some(Commands::Nex { ref action }) => {
            nex_cli(action);
            std::process::exit(0);
        }
        Some(Commands::Bench { ref action }) => match action {
            BenchAction::RunTask {
                prompt,
                usage_json,
                slim,
            } => {
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
                    slim: cli.slim || *slim,
                    full: cli.full,
                    no_session: cli.no_session,
                    no_splash: cli.no_splash,
                    tutorial: cli.tutorial,
                    smoke: false,
                    smoke_cleave: false,
                    initial_prompt: None,
                    initial_prompt_file: None,
                    context_class: cli.context_class.clone(),
                    persona: cli.persona.clone(),
                    log_level: cli.log_level.clone(),
                    log_file: cli.log_file.clone(),
                    ollama_integration: cli.ollama_integration,
                    ollama_model: cli.ollama_model.clone(),
                    yes: cli.yes,
                    posture: cli.posture.clone(),
                    architect: false,
                    fabricator: false,
                    explorator: false,
                    devastator: false,
                    dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
                    sandboxed: false,
                    oci_image: cli.oci_image.clone(),
                    oci_runtime: cli.oci_runtime.clone(),
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

/// Mutable state for the default (full-featured) daemon session.
/// Pre-existing agent state from `AgentSetup` — has memory, lifecycle,
/// delegate, and all features. Web API prompts and vox events route here.
///
/// Wrapped in `Option` so spawned turns can `.take()` ownership for the
/// duration of `r#loop::run()`, releasing the Mutex while the turn executes.
struct DefaultSession {
    bus: bus::EventBus,
    context_manager: context::ContextManager,
    conversation: conversation::ConversationState,
}

/// Type alias for the shared session state. `None` means a turn is in progress.
type SharedSession = Arc<tokio::sync::Mutex<Option<DefaultSession>>>;

/// Run a daemon turn with the take/replace pattern. Acquires the session
/// briefly to extract state, runs the turn without holding the lock, then
/// puts state back. Returns `Err` if the session is busy (turn in progress).
async fn run_daemon_turn(
    session: &SharedSession,
    shared_settings: &settings::SharedSettings,
    fallback_model: &str,
    events_tx: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    config: r#loop::LoopConfig,
    setup_fn: impl FnOnce(&mut DefaultSession),
) -> anyhow::Result<()> {
    // Read the current model from shared_settings so SIGHUP reloads and
    // /set_model changes are picked up. Falls back to the startup model if
    // the lock is poisoned.
    let model = shared_settings
        .lock()
        .map(|s| s.model.clone())
        .unwrap_or_else(|_| fallback_model.to_string());

    // Resolve bridge per-turn so credential changes in auth.json are picked
    // up without restarting the daemon.
    let bridge: Box<dyn LlmBridge> = match providers::auto_detect_bridge(&model).await {
        Some(b) => b,
        None => {
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: format!("No LLM provider available for model {model} — check auth"),
            });
            anyhow::bail!(
                "No provider available for model {model}.\n\
                 Run `omegon auth login` to set up authentication."
            );
        }
    };

    // Take ownership — Mutex held only for the .take() call.
    let mut state = {
        let mut guard = session.lock().await;
        guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("session busy — turn already in progress"))?
    };

    setup_fn(&mut state);

    let turn_cancel = CancellationToken::new();
    let result = r#loop::run(
        bridge.as_ref(),
        &mut state.bus,
        &mut state.context_manager,
        &mut state.conversation,
        events_tx,
        turn_cancel,
        &config,
    )
    .await;

    // Always return state, even on error.
    {
        let mut guard = session.lock().await;
        *guard = Some(state);
    }

    result
}

/// Pre-setup agent manifest resolution. Runs BEFORE AgentSetup::new() so
/// that persona, settings, triggers, and workflows are materialized into
/// the filesystem and environment where setup will discover them.
pub(crate) fn apply_agent_manifest_pre_setup(
    agent_id: &str,
    cwd: &std::path::Path,
    shared_settings: &settings::SharedSettings,
) -> anyhow::Result<agent_manifest::ResolvedManifest> {
    let omegon_home = paths::omegon_home().unwrap_or_else(|_| cwd.join(".omegon"));
    let resolved = catalog::resolve(&omegon_home, agent_id)?;

    tracing::info!(
        agent = %resolved.manifest.agent.id,
        domain = %resolved.manifest.agent.domain,
        "loaded agent manifest"
    );

    // ── Verify bundle safety ─────────────────────────────────────────
    let verification = bundle_verify::verify_bundle(&resolved);
    for w in verification.warnings() {
        tracing::warn!(category = w.category, location = %w.location, "bundle warning: {}", w.message);
    }
    if !verification.passed() {
        for e in verification.errors() {
            tracing::error!(category = e.category, location = %e.location, "bundle verification failed: {}", e.message);
        }
        anyhow::bail!(
            "agent bundle '{}' failed verification with {} error(s)",
            resolved.manifest.agent.id,
            verification.errors().len()
        );
    }
    tracing::info!("agent bundle verified");

    // ── Apply settings ───────────────────────────────────────────────
    if let Some(ref s) = resolved.manifest.settings
        && let Ok(mut settings) = shared_settings.lock()
    {
        if let Some(ref m) = s.model {
            settings.model = m.clone();
        }
        if let Some(ref tl) = s.thinking_level
            && let Some(level) = settings::ThinkingLevel::parse(tl)
        {
            settings.thinking = level;
        }
        if let Some(mt) = s.max_turns {
            settings.max_turns = mt;
        }
    }

    // ── Materialize persona as plugin for setup discovery ────────────
    if resolved.persona_directive.is_some() {
        let persona_slug = resolved.manifest.agent.id.replace('.', "-");
        let plugin_dir = cwd.join(".omegon").join("plugins").join(&persona_slug);
        std::fs::create_dir_all(&plugin_dir).ok();

        if let Some(ref directive) = resolved.persona_directive {
            std::fs::write(plugin_dir.join("PERSONA.md"), directive).ok();
        }
        if let Some(ref facts) = resolved.mind_facts_content {
            let mind_dir = plugin_dir.join("mind");
            std::fs::create_dir_all(&mind_dir).ok();
            std::fs::write(mind_dir.join("facts.jsonl"), facts).ok();
        }

        let persona_cfg = resolved.manifest.persona.as_ref();
        let badge = persona_cfg
            .and_then(|p| p.badge.as_deref())
            .map(|b| format!("\n[persona.style]\nbadge = \"{b}\""))
            .unwrap_or_default();
        let skills = persona_cfg
            .and_then(|p| p.activated_skills.as_ref())
            .filter(|s| !s.is_empty())
            .map(|s| {
                format!(
                    "\n[persona.skills]\nactivate = [{}]",
                    s.iter()
                        .map(|sk| format!("\"{sk}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .unwrap_or_default();
        let tools = persona_cfg
            .and_then(|p| p.disabled_tools.as_ref())
            .filter(|t| !t.is_empty())
            .map(|t| {
                format!(
                    "\n[persona.tools]\ndisable = [{}]",
                    t.iter()
                        .map(|tk| format!("\"{tk}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .unwrap_or_default();
        let mind = if resolved.mind_facts_content.is_some() {
            "\n[persona.mind]\nseed_facts = \"mind/facts.jsonl\""
        } else {
            ""
        };

        let plugin_toml = format!(
            "[plugin]\ntype = \"persona\"\nid = \"agent.{persona_slug}\"\n\
             name = \"{name}\"\nversion = \"{ver}\"\n\
             description = \"Bundle-materialized persona\"\n\n\
             [persona.identity]\ndirective = \"PERSONA.md\"\n{mind}{badge}{skills}{tools}\n",
            name = resolved.manifest.agent.name,
            ver = resolved.manifest.agent.version,
        );
        std::fs::write(plugin_dir.join("plugin.toml"), plugin_toml).ok();
        // SAFETY: single-threaded init phase, before any tokio tasks spawn.
        unsafe { std::env::set_var("OMEGON_CHILD_PERSONA", &resolved.manifest.agent.name) };
        tracing::info!(persona = %resolved.manifest.agent.name, "materialized bundle persona");
    }

    // ── Install trigger configs ──────────────────────────────────────
    if let Some(ref trigs) = resolved.manifest.triggers {
        let trigger_dir = cwd.join(".omegon").join("triggers");
        std::fs::create_dir_all(&trigger_dir).ok();
        for t in trigs {
            let config = triggers::TriggerConfig {
                trigger: triggers::TriggerMeta {
                    name: t.name.clone(),
                    enabled: true,
                    schedule: t.schedule.clone(),
                    interval: t.interval.clone(),
                    cron: None,
                    file_watch: None,
                    debounce: None,
                    git_events: None,
                    git_poll_interval: None,
                },
                filter: None,
                prompt: triggers::PromptTemplate {
                    template: t.template.clone(),
                },
                session: None,
            };
            let toml_str = toml::to_string_pretty(&config).unwrap_or_default();
            std::fs::write(trigger_dir.join(format!("{}.toml", t.name)), &toml_str).ok();
        }
    }

    // ── Install workflow template ────────────────────────────────────
    if let Some(ref wf) = resolved.manifest.workflow {
        let wf_dir = cwd.join(".omegon").join("workflows");
        std::fs::create_dir_all(&wf_dir).ok();
        let map_phase = |p: &std::collections::HashMap<String, agent_manifest::PhaseConfig>,
                         name: &str|
         -> Option<workflow::PhaseConfig> {
            p.get(name).map(|pc| workflow::PhaseConfig {
                persona: pc.persona.clone(),
                model: pc.model.clone(),
                max_turns: pc.max_turns,
                context_class: pc.context_class.clone(),
                thinking_level: pc.thinking_level.clone(),
            })
        };
        let phases = wf.phases.as_ref();
        let template = workflow::WorkflowTemplate {
            workflow: workflow::WorkflowMeta {
                name: wf.name.clone(),
                description: String::new(),
            },
            phases: workflow::WorkflowPhases {
                exploring: phases.and_then(|p| map_phase(p, "exploring")),
                specifying: phases.and_then(|p| map_phase(p, "specifying")),
                decomposing: phases.and_then(|p| map_phase(p, "decomposing")),
                implementing: phases.and_then(|p| map_phase(p, "implementing")),
                verifying: phases.and_then(|p| map_phase(p, "verifying")),
            },
        };
        let toml_str = toml::to_string_pretty(&template).unwrap_or_default();
        std::fs::write(wf_dir.join(format!("{}.toml", wf.name)), &toml_str).ok();
    }

    // ── Secret pre-flight ────────────────────────────────────────────
    if let Some(ref secrets) = resolved.manifest.secrets
        && let Some(ref required) = secrets.required
    {
        for s in required {
            if std::env::var(s).is_err() {
                tracing::warn!(secret = %s, "required secret not found in environment");
            }
        }
    }

    Ok(resolved)
}

fn load_web_authority_config(
    trusted_proxy_identity_path: Option<&Path>,
    require_proxy_identity: bool,
) -> anyhow::Result<web::WebAuthorityConfig> {
    let trusted_proxy = match trusted_proxy_identity_path {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("reading trusted proxy identity {}", path.display()))?;
            let identity: web::WebTrustedProxyIdentity = serde_json::from_str(&content)
                .with_context(|| format!("parsing trusted proxy identity {}", path.display()))?;
            if identity.subject.trim().is_empty() {
                anyhow::bail!("trusted proxy identity subject must not be empty");
            }
            if identity.fingerprint.trim().is_empty() {
                anyhow::bail!("trusted proxy identity fingerprint must not be empty");
            }
            Some(identity)
        }
        None => None,
    };

    if require_proxy_identity && trusted_proxy.is_none() {
        anyhow::bail!("--require-web-proxy-identity requires --web-trusted-proxy-identity <PATH>");
    }

    Ok(web::WebAuthorityConfig {
        trusted_proxy,
        require_proxy_identity,
    })
}

async fn run_embedded_command(
    control_port: u16,
    strict_port: bool,
    model: &str,
    agent_id: Option<&str>,
    web_trusted_proxy_identity: Option<&Path>,
    require_web_proxy_identity: bool,
    tls: Option<control_tls::ControlTlsConfig>,
    dangerously_bypass_permissions: bool,
) -> anyhow::Result<()> {
    let cwd = std::fs::canonicalize(".")?;

    let shared_settings = bootstrap::initialize_shared_settings(&bootstrap::SettingsInit {
        model,
        cwd: &cwd,
        cli_posture: None,
        slim: false,
        full: false,
        max_turns: 50,
        apply_profile_posture: false,
    });

    // Settings, persona, triggers, and workflows must be materialized
    // before setup so persona registry and tool surface are correct.
    let agent_manifest_resolved = if let Some(agent_id) = agent_id {
        Some(apply_agent_manifest_pre_setup(
            agent_id,
            &cwd,
            &shared_settings,
        )?)
    } else {
        None
    };

    let mut agent = setup::AgentSetup::new_with_safety(
        &cwd,
        None,
        Some(shared_settings.clone()),
        dangerously_bypass_permissions,
    )
    .await?;
    agent.instance_id = paths::instance_id("embedded");
    bootstrap::apply_runtime_posture(
        &mut agent,
        omegon_traits::OmegonRuntimeProfile::LongRunningDaemon,
        omegon_traits::OmegonAutonomyMode::GuardedAutonomous,
    );

    if let Some(ref resolved) = agent_manifest_resolved
        && let Some(ref exts) = resolved.manifest.extensions
    {
        let omegon_home = paths::omegon_home().unwrap_or_else(|_| cwd.join(".omegon"));
        let ext_dir = omegon_home.join("extensions");
        for ext in exts {
            let installed = ext_dir.join(&ext.name).join("manifest.toml").exists();
            if installed {
                tracing::info!(extension = %ext.name, version = %ext.version, "extension installed");
            } else {
                tracing::error!(
                    extension = %ext.name,
                    version = %ext.version,
                    expected_path = %ext_dir.join(&ext.name).display(),
                    "required extension not installed. Run: omegon extension install <source>"
                );
            }
        }
    }

    let mut model = shared_settings
        .lock()
        .map(|s| s.model.clone())
        .unwrap_or_else(|_| "anthropic:claude-sonnet-4-6".into());
    // If the configured model's provider isn't available, try auto-detection.
    if providers::auto_detect_bridge(&model).await.is_none()
        && let Some(safe) = providers::automation_safe_model()
    {
        tracing::info!(
            configured = %model, resolved = %safe,
            "configured model unavailable — switching to detected provider"
        );
        model = safe.clone();
        if let Ok(mut s) = shared_settings.lock() {
            s.set_model(&safe);
        }
    }
    // Validate provider availability at startup (fail-fast).
    if providers::auto_detect_bridge(&model).await.is_none() {
        tracing::warn!(
            model = %model,
            "no LLM provider available at startup — bridge will be re-resolved per turn"
        );
        eprintln!(
            "⚠ No LLM provider available for model {model}.\n  \
             The daemon will start but tasks will fail until a provider is configured.\n  \
             Run `omegon auth login` to set up authentication."
        );
    }

    let (events_tx, _) = bootstrap::wire_event_channel(&agent, 256);

    let web_role = shared_settings
        .lock()
        .ok()
        .and_then(|settings| crate::permissions::styrene_role_from_settings(&settings))
        .unwrap_or(styrene_rbac::Role::Admin);
    let web_authority =
        load_web_authority_config(web_trusted_proxy_identity, require_web_proxy_identity)?;
    let state = web::WebState::new(agent.dashboard_handles.clone(), events_tx.clone())
        .with_web_role(web_role)
        .with_web_authority(web_authority);
    let vox_daemon_events = state.daemon_events.clone();
    let global_cancel = CancellationToken::new();

    let acp_state = web::acp_ws::AcpWebState {
        web_auth: state.web_auth.clone(),
        web_authority: state.web_authority.clone(),
        model: model.to_string(),
        cwd: cwd.clone(),
        agent_id: agent_id.map(String::from),
        dangerously_bypass_permissions,
        active_connections: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        shutdown: global_cancel.clone(),
    };
    let (startup, mut cmd_rx) =
        web::start_server_with_options(state, control_port, strict_port, acp_state, tls).await?;

    let event = EmbeddedStartupEvent {
        event_type: "omegon.startup",
        schema_version: startup.schema_version,
        pid: std::process::id(),
        http_base: startup.http_base,
        startup_url: startup.startup_url,
        health_url: startup.health_url,
        ready_url: startup.ready_url,
        ws_url: startup.ws_url,
        auth_mode: startup.auth_mode,
        auth_source: startup.auth_source,
    };
    println!("{}", serde_json::to_string(&event)?);

    let cancel_ctrl_c = global_cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        cancel_ctrl_c.cancel();
    });
    #[cfg(unix)]
    {
        let cancel_sigterm = global_cancel.clone();
        tokio::spawn(async move {
            let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
            sig.recv().await;
            cancel_sigterm.cancel();
        });
    }

    let ipc_cancel = tokio_util::sync::CancellationToken::new();
    let (ipc_cmd_tx, mut ipc_cmd_rx) = tokio::sync::mpsc::channel::<tui::TuiCommand>(32);
    {
        let ipc_cfg =
            ipc::IpcServerConfig::from_cwd(&cwd, env!("CARGO_PKG_VERSION"), &agent.session_id);
        let shared_cancel: tui::SharedCancel =
            Arc::new(std::sync::Mutex::new(Some(global_cancel.clone())));
        ipc::start_ipc_server(
            ipc_cfg,
            agent.dashboard_handles.clone(),
            events_tx.clone(),
            ipc_cmd_tx,
            shared_settings.clone(),
            shared_cancel,
            ipc_cancel.clone(),
        );
    }

    let _mqtt_bridge = maybe_start_mqtt_bridge(&cwd, agent.session_id.clone(), events_tx.clone());

    if !agent.vox_polling_handles.is_empty() {
        for handle in agent.vox_polling_handles {
            crate::extensions::vox_bridge::start_vox_bridge(
                handle,
                vox_daemon_events.clone(),
                crate::extensions::vox_bridge::VoxBridgeConfig::default(),
                global_cancel.clone(),
            );
        }
    }

    let voice_status =
        std::sync::Arc::new(std::sync::Mutex::new(agent.initial_harness_status.clone()));
    for rx in agent.voice_notification_receivers {
        crate::extensions::voice_bridge::start_voice_bridge_with_status(
            rx,
            vox_daemon_events.clone(),
            Some(crate::extensions::voice_bridge::VoiceStatusSink::new(
                voice_status.clone(),
                events_tx.clone(),
            )),
            global_cancel.clone(),
        );
    }

    let _daemon_checkpoint_task = checkpoint::spawn_checkpoint_subscriber(
        &events_tx,
        agent.session_id.clone(),
        agent.context_metrics.clone(),
    );

    let daemon_workflow: Option<Arc<workflow::WorkflowTemplate>> =
        workflow::discover_workflow(&cwd).map(Arc::new);
    if let Some(ref wf) = daemon_workflow {
        tracing::info!(workflow = %wf.workflow.name, "daemon loaded workflow template");
    }

    let router = Arc::new(session_router::SessionRouter::new());
    let agent_cwd = agent.cwd.clone();
    let agent_session_id = agent.session_id.clone();
    let agent_secrets = agent.secrets.clone();
    let agent_handles = agent.dashboard_handles.clone();

    // Wrap the pre-existing agent state as the default session. This
    // preserves single-session backward compatibility — events without
    // identity metadata route here (web API, anonymous vox messages).
    let default_session: SharedSession = Arc::new(tokio::sync::Mutex::new(Some(DefaultSession {
        bus: agent.bus,
        context_manager: agent.context_manager,
        conversation: agent.conversation,
    })));

    let trigger_configs = triggers::load_trigger_configs(&cwd);
    let trigger_events = triggers::EventTriggers::from_configs(&trigger_configs);
    let (mut trigger_runtime, _trigger_tx) =
        triggers::TriggerRuntimeBuilder::new(trigger_configs, cwd.clone())
            .build(global_cancel.clone());

    // Track in-flight triggers to prevent re-entrant execution.
    let triggers_in_flight: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        Default::default();

    let idle_poll_interval = tokio::time::Duration::from_secs(30);
    let mut idle_tick = tokio::time::interval(idle_poll_interval);
    idle_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Skip the first immediate tick
    idle_tick.tick().await;

    // Vox event bridge poll — drain inbound messages from extensions
    let mut vox_poll = tokio::time::interval(tokio::time::Duration::from_millis(250));
    vox_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        .expect("failed to register SIGHUP handler");

    tracing::info!("daemon dispatch loop started");
    loop {
        tokio::select! {
            _ = global_cancel.cancelled() => {
                tracing::info!("daemon shutting down (signal)");
                break;
            }
            _ = sighup.recv() => {
                tracing::info!("SIGHUP received — reloading configuration");
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: "Reloading configuration...".into(),
                });
                let profile = settings::Profile::load(&agent_cwd);
                if let Ok(mut s) = shared_settings.lock() {
                    profile.apply_to(&mut s);
                    tracing::info!(model = %s.model, "profile reloaded");
                }
            }
            _ = vox_poll.tick() => {
                // Drain up to 32 events per tick to bound memory pressure.
                // Events beyond the cap stay in the queue for the next tick.
                const MAX_EVENTS_PER_TICK: usize = 32;
                let events: Vec<omegon_traits::DaemonEventEnvelope> = {
                    match vox_daemon_events.lock() {
                        Ok(mut queue) => {
                            let n = queue.len().min(MAX_EVENTS_PER_TICK);
                            queue.drain(..n).collect()
                        }
                        Err(_) => continue,
                    }
                };
                for envelope in events {
                    let key = session_router::CallerKey::from_envelope(&envelope);
                    let trust_level = envelope
                        .payload
                        .get("trust_level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("user")
                        .to_string();
                    tracing::info!(
                        source = %envelope.source,
                        event_id = %envelope.event_id,
                        trust = %trust_level,
                        caller = %key,
                        "daemon: processing vox event"
                    );
                    // Check if an event trigger template matches this envelope.
                    // If so, use the rendered template prompt; otherwise fall
                    // back to the raw payload text.
                    let text = if let Some(matched) = trigger_events.match_envelope(
                        &envelope.source,
                        &envelope.trigger_kind,
                        &envelope.payload,
                    ) {
                        tracing::info!(
                            trigger = %matched.name,
                            source = %envelope.source,
                            "daemon: event trigger matched"
                        );
                        matched.prompt
                    } else {
                        envelope
                            .payload
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    };

                    if text.is_empty() {
                        continue;
                    }

                    // Route all vox events through the default session.
                    // TODO: per-caller session creation via router.get_or_create()
                    // once session factory is wired up.
                    let session = default_session.clone();

                    let events_tx = events_tx.clone();
                    let shared_settings = shared_settings.clone();
                    let model = model.clone();
                    let cwd = agent_cwd.clone();
                    let secrets = agent_secrets.clone();
                    let semaphore = router.semaphore().clone();
                    let _daemon_workflow = daemon_workflow.clone();

                    task_spawn::spawn_best_effort_result(
                        "daemon-turn-vox",
                        async move {
                            let _permit = semaphore.acquire().await
                                .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                            let loop_config = bootstrap::build_loop_config(
                                &shared_settings, &cwd, &model,
                                bootstrap::LoopConfigOverrides {
                                    secrets: Some(secrets),
                                    ..Default::default()
                                },
                            );

                            if let Err(e) = run_daemon_turn(
                                &session, &shared_settings, &model, &events_tx, loop_config,
                                |state| { state.conversation.push_user(text); },
                            ).await {
                                tracing::error!(error = %e, "daemon vox event loop error");
                            }
                            Ok(())
                        },
                    );
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(web::WebCommand::UserPrompt { text, image_paths }) => {
                        if !image_paths.is_empty() {
                            tracing::warn!(
                                count = image_paths.len(),
                                "daemon: prompt attachments are not yet supported in headless serve mode; ignoring"
                            );
                        }
                        tracing::info!(prompt_len = text.len(), "daemon: received user prompt");

                        // Clone handles for the spawned task.
                        let session = default_session.clone();

                        let events_tx = events_tx.clone();
                        let shared_settings = shared_settings.clone();
                        let model = model.clone();
                        let cwd = agent_cwd.clone();
                        let secrets = agent_secrets.clone();
                        let semaphore = router.semaphore().clone();
                        let _daemon_workflow = daemon_workflow.clone();

                        task_spawn::spawn_best_effort_result(
                            "daemon-turn-prompt",
                            async move {
                                let _permit = semaphore.acquire().await
                                    .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                                let loop_config = bootstrap::build_loop_config(
                                    &shared_settings, &cwd, &model,
                                    bootstrap::LoopConfigOverrides {
                                        secrets: Some(secrets),
                                        allow_commit_nudge: true,
                                        ..Default::default()
                                    },
                                );

                                if let Err(e) = run_daemon_turn(
                                    &session, &shared_settings, &model, &events_tx, loop_config,
                                    |state| { state.conversation.push_user(text); },
                                ).await {
                                    tracing::error!(error = %e, "daemon agent loop error");
                                }
                                Ok(())
                            },
                        );
                    }
                    Some(web::WebCommand::ExecuteControl { request, respond_to }) => {
                        tracing::info!(request = ?request, "daemon: control request");
                        let response = control_runtime::execute_daemon_control(
                            request,
                            &shared_settings,
                            &agent_secrets,
                            &agent_cwd,
                            &agent_handles,
                            &events_tx,
                        )
                        .await;
                        if let Some(tx) = respond_to {
                            let _ = tx.send(response);
                        }
                    }
                    Some(web::WebCommand::SlashCommand { name, args, respond_to }) => {
                        // Non-canonical slash commands that didn't map to a
                        // ControlRequest. Route as a user prompt so the agent
                        // can interpret them.
                        tracing::info!(command = %name, "daemon: slash command → prompt");
                        let prompt = if args.is_empty() {
                            format!("/{name}")
                        } else {
                            format!("/{name} {args}")
                        };
                        let session = default_session.clone();

                        let events_tx = events_tx.clone();
                        let shared_settings = shared_settings.clone();
                        let model = model.clone();
                        let cwd = agent_cwd.clone();
                        let secrets = agent_secrets.clone();
                        let semaphore = router.semaphore().clone();

                        task_spawn::spawn_best_effort_result(
                            "daemon-turn-slash",
                            async move {
                                let _permit = semaphore.acquire().await
                                    .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                                let loop_config = bootstrap::build_loop_config(
                                    &shared_settings, &cwd, &model,
                                    bootstrap::LoopConfigOverrides {
                                        secrets: Some(secrets),
                                        ..Default::default()
                                    },
                                );

                                if let Err(e) = run_daemon_turn(
                                    &session, &shared_settings, &model, &events_tx, loop_config,
                                    |state| { state.conversation.push_user(prompt); },
                                ).await {
                                    tracing::error!(error = %e, "daemon slash command loop error");
                                }

                                // If there's a respond_to channel, acknowledge.
                                if let Some(tx) = respond_to {
                                    let _ = tx.send(omegon_traits::SlashCommandResponse {
                                        accepted: true,
                                        output: None,
                                    });
                                }
                                Ok(())
                            },
                        );
                    }
                    Some(web::WebCommand::Cancel) => {
                        tracing::info!("daemon: cancel requested (no active loop)");
                    }
                    Some(web::WebCommand::Shutdown) => {
                        tracing::info!("daemon: shutdown requested");
                        break;
                    }
                    Some(_) => {
                        tracing::debug!("daemon: unhandled command variant");
                    }
                    None => {
                        tracing::info!("daemon: command channel closed");
                        break;
                    }
                }
            }
            ipc_cmd = ipc_cmd_rx.recv() => {
                match ipc_cmd {
                    Some(tui::TuiCommand::SubmitPrompt(submission)) => {
                        tracing::info!(prompt_len = submission.text.len(), "daemon: IPC prompt");
                        let session = default_session.clone();
                        let events_tx = events_tx.clone();
                        let shared_settings = shared_settings.clone();
                        let model = model.clone();
                        let cwd = agent_cwd.clone();
                        let secrets = agent_secrets.clone();
                        let semaphore = router.semaphore().clone();

                        let text = submission.text;
                        task_spawn::spawn_best_effort_result(
                            "daemon-turn-ipc",
                            async move {
                                let _permit = semaphore.acquire().await
                                    .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                                let loop_config = bootstrap::build_loop_config(
                                    &shared_settings, &cwd, &model,
                                    bootstrap::LoopConfigOverrides {
                                        secrets: Some(secrets),
                                        allow_commit_nudge: true,
                                        ..Default::default()
                                    },
                                );

                                if let Err(e) = run_daemon_turn(
                                    &session, &shared_settings, &model, &events_tx, loop_config,
                                    |state| { state.conversation.push_user(text); },
                                ).await {
                                    tracing::error!(error = %e, "daemon IPC agent loop error");
                                }
                                Ok(())
                            },
                        );
                    }
                    Some(tui::TuiCommand::ExecuteControl { request, respond_to }) => {
                        tracing::info!(request = ?request, "daemon: IPC control request");
                        let response = control_runtime::execute_daemon_control(
                            request,
                            &shared_settings,
                            &agent_secrets,
                            &agent_cwd,
                            &agent_handles,
                            &events_tx,
                        )
                        .await;
                        if let Some(tx) = respond_to {
                            let _ = tx.send(response);
                        }
                    }
                    Some(tui::TuiCommand::UpdatePlan { respond_to, .. }) => {
                        let response = omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "Plan mode is not available in daemon IPC sessions yet.".into(),
                            ),
                        };
                        if let Some(tx) = respond_to {
                            let _ = tx.send(response);
                        }
                    }
                    Some(tui::TuiCommand::RunSlashCommand { name, args, respond_to }) => {
                        tracing::info!(command = %name, "daemon: IPC slash command → prompt");
                        let prompt = if args.is_empty() {
                            format!("/{name}")
                        } else {
                            format!("/{name} {args}")
                        };
                        let session = default_session.clone();
                        let events_tx = events_tx.clone();
                        let shared_settings = shared_settings.clone();
                        let model = model.clone();
                        let cwd = agent_cwd.clone();
                        let secrets = agent_secrets.clone();
                        let semaphore = router.semaphore().clone();

                        task_spawn::spawn_best_effort_result(
                            "daemon-turn-ipc-slash",
                            async move {
                                let _permit = semaphore.acquire().await
                                    .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                                let loop_config = bootstrap::build_loop_config(
                                    &shared_settings, &cwd, &model,
                                    bootstrap::LoopConfigOverrides {
                                        secrets: Some(secrets),
                                        ..Default::default()
                                    },
                                );

                                if let Err(e) = run_daemon_turn(
                                    &session, &shared_settings, &model, &events_tx, loop_config,
                                    |state| { state.conversation.push_user(prompt); },
                                ).await {
                                    tracing::error!(error = %e, "daemon IPC slash command loop error");
                                }

                                if let Some(tx) = respond_to {
                                    let _ = tx.send(omegon_traits::SlashCommandResponse {
                                        accepted: true,
                                        output: None,
                                    });
                                }
                                Ok(())
                            },
                        );
                    }
                    Some(tui::TuiCommand::Quit) => {
                        tracing::info!("daemon: IPC shutdown requested");
                        break;
                    }
                    Some(_) => {
                        tracing::debug!("daemon: unhandled IPC command variant");
                    }
                    None => {
                        tracing::debug!("daemon: IPC command channel closed");
                    }
                }
            }
            Some(trigger_event) = trigger_runtime.event_rx.recv() => {
                let trigger_name = match &trigger_event {
                    triggers::TriggerEvent::Scheduled(c) => c.trigger.name.clone(),
                    triggers::TriggerEvent::FileChanged { trigger_name, .. } => trigger_name.clone(),
                    triggers::TriggerEvent::GitChanged { trigger_name, .. } => trigger_name.clone(),
                    triggers::TriggerEvent::Webhook { name, .. } => name.clone(),
                    triggers::TriggerEvent::ForceRun { task_id } => task_id.clone(),
                };

                let prompt = match &trigger_event {
                    triggers::TriggerEvent::Scheduled(c) => c.prompt.template.clone(),
                    triggers::TriggerEvent::FileChanged { trigger_name, paths } => {
                        format!("File change detected in trigger '{trigger_name}': {paths:?}")
                    }
                    triggers::TriggerEvent::GitChanged { kind, detail, .. } => {
                        format!("Git event: {kind} — {detail}")
                    }
                    triggers::TriggerEvent::Webhook { name, payload } => {
                        format!("Webhook '{name}' fired with payload: {payload}")
                    }
                    triggers::TriggerEvent::ForceRun { task_id } => {
                        format!("Force-run task: {task_id}")
                    }
                };

                if prompt.is_empty() {
                    continue;
                }

                {
                    let in_flight = triggers_in_flight.lock().unwrap_or_else(|e| e.into_inner());
                    if in_flight.contains(&trigger_name) {
                        tracing::debug!(trigger = %trigger_name, "trigger still in-flight, skipping");
                        continue;
                    }
                }
                {
                    let mut in_flight = triggers_in_flight.lock().unwrap_or_else(|e| e.into_inner());
                    in_flight.insert(trigger_name.clone());
                }

                tracing::info!(trigger = %trigger_name, "daemon: firing trigger");

                let session = default_session.clone();
                let events_tx = events_tx.clone();
                let shared_settings = shared_settings.clone();
                let model = model.clone();
                let cwd = agent_cwd.clone();
                let secrets = agent_secrets.clone();
                let semaphore = router.semaphore().clone();
                let in_flight = triggers_in_flight.clone();
                let trigger_name_for_cleanup = trigger_name.clone();

                task_spawn::spawn_best_effort_result(
                    "daemon-turn-trigger",
                    async move {
                        let _permit = semaphore.acquire().await
                            .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                        let loop_config = bootstrap::build_loop_config(
                            &shared_settings, &cwd, &model,
                            bootstrap::LoopConfigOverrides {
                                secrets: Some(secrets),
                                ..Default::default()
                            },
                        );

                        if let Err(e) = run_daemon_turn(
                            &session, &shared_settings, &model, &events_tx, loop_config,
                            |state| { state.conversation.push_user(prompt); },
                        ).await {
                            tracing::error!(
                                trigger = %trigger_name,
                                error = %e,
                                "daemon: trigger loop error"
                            );
                        }
                        if let Ok(mut set) = in_flight.lock() {
                            set.remove(&trigger_name_for_cleanup);
                        }
                        Ok(())
                    },
                );
            }
            _ = idle_tick.tick() => {
                let parked = router.park_idle_sessions().await;
                for key in &parked {
                    tracing::info!(caller = %key, "daemon: parked idle session");
                }

                let ready = workflow::query_ready_nodes(&cwd);
                if ready.is_empty() {
                    continue;
                }

                // Dispatch highest-priority ready node
                let mut ready = ready;
                ready.sort_by(|a, b| {
                    a.priority.unwrap_or(u8::MAX).cmp(&b.priority.unwrap_or(u8::MAX))
                });
                let node = &ready[0];
                tracing::info!(
                    node_id = %node.id,
                    title = %node.title,
                    priority = ?node.priority,
                    "daemon: auto-dispatching ready design node"
                );

                let prompt = workflow::build_dispatch_prompt(node);
                let node_id = node.id.clone();

                // Clone handles for the spawned task.
                let session = default_session.clone();
                let events_tx = events_tx.clone();
                let shared_settings = shared_settings.clone();
                let model = model.clone();
                let cwd = agent_cwd.clone();
                let secrets = agent_secrets.clone();
                let semaphore = router.semaphore().clone();
                let _daemon_workflow = daemon_workflow.clone();

                task_spawn::spawn_best_effort_result(
                    "daemon-turn-auto-dispatch",
                    async move {
                        let _permit = semaphore.acquire().await
                            .map_err(|_| anyhow::anyhow!("session semaphore closed"))?;

                        let loop_config = bootstrap::build_loop_config(
                            &shared_settings, &cwd, &model,
                            bootstrap::LoopConfigOverrides {
                                secrets: Some(secrets),
                                allow_commit_nudge: true,
                                enforce_first_turn_execution_bias: true,
                                ..Default::default()
                            },
                        );

                        if let Err(e) = run_daemon_turn(
                            &session, &shared_settings, &model, &events_tx, loop_config,
                            |state| { state.conversation.push_user(prompt); },
                        ).await {
                            tracing::error!(
                                node_id = %node_id,
                                error = %e,
                                "daemon: auto-dispatch agent loop error"
                            );
                        }
                        Ok(())
                    },
                );
            }
        }
    }

    ipc_cancel.cancel();

    // Give in-flight turns a grace period to complete before saving state.
    tracing::info!("daemon: draining in-flight turns (5s grace period)");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // Save the default session (if not currently in a turn).
    {
        let guard = default_session.lock().await;
        if let Some(ref sess) = *guard {
            if let Err(e) =
                session::save_session(&sess.conversation, &agent_cwd, Some(&agent_session_id))
            {
                tracing::debug!("Daemon session save failed (non-fatal): {e}");
            }
        } else {
            tracing::warn!(
                "session still in-flight at shutdown — state will be saved by completing turn"
            );
        }
    }
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
        cleave::orchestrator::MergeOutcome::Success => {
            if let Some(child) = child {
                if child.error.as_deref() == Some("merged after salvaging work from a failed child")
                {
                    format!("  ↺ {label} salvaged and merged after failure")
                } else {
                    format!("  ✓ {label} merged")
                }
            } else {
                format!("  ✓ {label} merged")
            }
        }
        cleave::orchestrator::MergeOutcome::NoChanges => {
            if let Some(child) = child {
                match child.status {
                    cleave::state::ChildStatus::UpstreamExhausted => {
                        format!("  ↯ {label} upstream exhausted (no repo changes to merge)")
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
    let mut agent_setup = setup::AgentSetup::new_with_safety(
        &repo_path,
        None,
        None,
        cli.dangerously_bypass_permissions,
    )
    .await?;
    agent_setup.instance_id = paths::instance_id("cleave");

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
        workflow: workflow::discover_workflow(&cli.cwd),
        sandbox: false, // CLI cleave — no settings context, sandbox opt-in via TUI only
        dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
    };

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::warn!("Interrupted — cancelling cleave");
        cancel_clone.cancel();
    });

    let result = cleave::run_cleave(
        &plan, directive, &repo_path, workspace, &config, cancel, None,
    )
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
            cleave::state::ChildStatus::UpstreamExhausted => "↯",
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

async fn run_ollama_command(action: &OllamaAction) -> anyhow::Result<()> {
    match action {
        OllamaAction::Register => ollama_register().await,
        OllamaAction::Unregister => ollama_unregister(),
        OllamaAction::Status => ollama_status().await,
    }
}

fn ollama_integrations_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ollama")
        .join("integrations")
}

fn ollama_manifest_path() -> PathBuf {
    ollama_integrations_dir().join("omegon.json")
}

fn ollama_symlink_path() -> PathBuf {
    ollama_integrations_dir().join("omegon")
}

async fn ollama_register() -> anyhow::Result<()> {
    let binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("omegon"));
    let binary_str = binary.display().to_string();

    let integrations_dir = ollama_integrations_dir();
    std::fs::create_dir_all(&integrations_dir)?;

    // Write manifest
    let manifest = serde_json::json!({
        "name": "omegon",
        "display_name": "Omegon Agent",
        "description": "Terminal AI agent harness for systems engineering",
        "binary": binary_str,
        "args": ["--ollama-integration"],
        "model_flag": "--ollama-model",
        "version": env!("CARGO_PKG_VERSION")
    });
    let manifest_path = ollama_manifest_path();
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    println!("✓ Wrote manifest: {}", manifest_path.display());

    // Create symlink for PATH-based discovery fallback
    let symlink_path = ollama_symlink_path();
    if symlink_path.exists() || symlink_path.is_symlink() {
        let _ = std::fs::remove_file(&symlink_path);
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&binary, &symlink_path)?;
        println!("✓ Symlinked: {} → {}", symlink_path.display(), binary_str);
    }

    println!("\n`ollama launch omegon` is now available.");
    Ok(())
}

fn ollama_unregister() -> anyhow::Result<()> {
    let manifest_path = ollama_manifest_path();
    let symlink_path = ollama_symlink_path();

    let mut removed = false;
    if manifest_path.exists() {
        std::fs::remove_file(&manifest_path)?;
        println!("✓ Removed manifest: {}", manifest_path.display());
        removed = true;
    }
    if symlink_path.exists() || symlink_path.is_symlink() {
        std::fs::remove_file(&symlink_path)?;
        println!("✓ Removed symlink: {}", symlink_path.display());
        removed = true;
    }

    if !removed {
        println!("Nothing to remove — omegon was not registered.");
    }
    Ok(())
}

async fn ollama_status() -> anyhow::Result<()> {
    let mgr = ollama::OllamaManager::new();

    // Registration
    let manifest_path = ollama_manifest_path();
    if manifest_path.exists() {
        let contents = std::fs::read_to_string(&manifest_path)?;
        let manifest: serde_json::Value = serde_json::from_str(&contents)?;
        let registered_binary = manifest["binary"].as_str().unwrap_or("unknown");
        let binary_exists = Path::new(registered_binary).exists();
        println!(
            "Registration:  ✓ registered (binary: {}{})  ",
            registered_binary,
            if binary_exists {
                ""
            } else {
                " ⚠ binary not found"
            }
        );
    } else {
        println!("Registration:  ✗ not registered (run `omegon ollama register`)");
    }

    // Reachability
    if mgr.is_reachable().await {
        println!("Reachability:  ✓ Ollama is running");

        // Models
        match mgr.list_models().await {
            Ok(models) => {
                println!("Models:        {} available", models.len());
                for m in &models {
                    let size_gb = m.size as f64 / 1_000_000_000.0;
                    println!("               - {} ({:.1} GB)", m.name, size_gb);
                }
            }
            Err(e) => println!("Models:        ✗ failed to list ({e})"),
        }

        // Running
        match mgr.list_running().await {
            Ok(running) if !running.is_empty() => {
                println!("Loaded:        {} in VRAM", running.len());
                for r in &running {
                    let vram_gb = r.size_vram as f64 / 1_000_000_000.0;
                    println!("               - {} ({:.1} GB VRAM)", r.name, vram_gb);
                }
            }
            Ok(_) => println!("Loaded:        none (models will cold-start)"),
            Err(e) => println!("Loaded:        ✗ failed to query ({e})"),
        }
    } else {
        println!("Reachability:  ✗ Ollama is not running or not reachable");
    }

    // Hardware
    let hw = ollama::OllamaManager::hardware_profile();
    let total_gb = hw.total_memory_bytes as f64 / 1_073_741_824.0;
    let vram_gb = hw.estimated_vram_bytes as f64 / 1_073_741_824.0;
    println!(
        "Hardware:      {:.0} GB total, {:.0} GB estimated VRAM, recommended max: {}",
        total_gb, vram_gb, hw.recommended_max_params
    );

    Ok(())
}

async fn run_embedding_command(action: &EmbeddingAction) -> anyhow::Result<()> {
    match action {
        EmbeddingAction::Download { model } => {
            let short_name = model.rsplit('/').next().unwrap_or(model);
            let model_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("omegon")
                .join("models")
                .join(short_name);
            std::fs::create_dir_all(&model_dir)?;

            let base_url = format!("https://huggingface.co/{model}/resolve/main");

            for filename in &["model.onnx", "tokenizer.json"] {
                let target = model_dir.join(filename);
                if target.exists() {
                    println!("  {filename} — already exists, skipping");
                    continue;
                }
                let url = format!("{base_url}/{filename}");
                println!("  Downloading {filename} from {url}...");

                let resp = reqwest::get(&url).await?;
                if !resp.status().is_success() {
                    anyhow::bail!("failed to download {filename}: HTTP {}", resp.status());
                }
                let bytes = resp.bytes().await?;
                std::fs::write(&target, &bytes)?;
                println!("  {filename} — {} bytes written", bytes.len());
            }

            println!("\nModel saved to {}", model_dir.display());
            println!("Omegon will use it automatically when Ollama is unavailable.");
            println!("To verify: omegon embedding status");
        }
        EmbeddingAction::Status => {
            let default_model = std::env::var("OMEGON_EMBED_LOCAL_MODEL")
                .unwrap_or_else(|_| "all-MiniLM-L6-v2".into());
            let model_dir = if let Ok(dir) = std::env::var("OMEGON_EMBED_MODEL_DIR") {
                std::path::PathBuf::from(dir)
            } else {
                dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("omegon")
                    .join("models")
                    .join(&default_model)
            };

            let model_exists = model_dir.join("model.onnx").exists();
            let tokenizer_exists = model_dir.join("tokenizer.json").exists();

            println!("Embedding model: {default_model}");
            println!("Model directory: {}", model_dir.display());
            println!(
                "model.onnx:     {}",
                if model_exists { "present" } else { "MISSING" }
            );
            println!(
                "tokenizer.json: {}",
                if tokenizer_exists {
                    "present"
                } else {
                    "MISSING"
                }
            );

            if model_exists && tokenizer_exists {
                println!("\nStatus: ready");
                #[cfg(feature = "local-embeddings")]
                {
                    match crate::local_embedding::LocalEmbeddingService::load(
                        &model_dir,
                        &default_model,
                    ) {
                        Ok(svc) => println!("Model loads successfully ({})", svc.model_name()),
                        Err(e) => println!("Model failed to load: {e}"),
                    }
                }
                #[cfg(not(feature = "local-embeddings"))]
                println!("Note: build with --features local-embeddings to enable local inference");
            } else {
                println!("\nStatus: not installed");
                println!("Run: omegon embedding download");
            }
        }
        EmbeddingAction::Backfill => {
            let cwd = std::fs::canonicalize(std::env::current_dir()?)?;
            let project_root = setup::find_project_root(&cwd);

            let memory_dir = {
                let ai = project_root.join("ai").join("memory");
                let omegon = project_root.join(".omegon").join("memory");
                if omegon.exists() && !ai.exists() {
                    omegon
                } else {
                    ai
                }
            };
            let db_path = memory_dir.join("facts.db");
            if !db_path.exists() {
                println!("No memory database found at {}", db_path.display());
                return Ok(());
            }

            let backend: std::sync::Arc<dyn omegon_memory::MemoryBackend> =
                std::sync::Arc::new(omegon_memory::SqliteBackend::open(&db_path)?);

            let embed_svc: Box<dyn omegon_memory::EmbeddingService> = {
                let profile = crate::settings::Profile::load(&cwd);
                let ollama = crate::embedding::OllamaEmbeddingService::from_config(
                    profile.embed_url.as_deref(),
                    profile.embed_model.as_deref(),
                );
                if ollama.probe().await {
                    Box::new(ollama)
                } else {
                    #[cfg(feature = "local-embeddings")]
                    {
                        match crate::local_embedding::LocalEmbeddingService::from_default_dir() {
                            Ok(svc) => Box::new(svc),
                            Err(e) => {
                                println!("No embedding service available: {e}");
                                println!("Run: omegon embedding download");
                                return Ok(());
                            }
                        }
                    }
                    #[cfg(not(feature = "local-embeddings"))]
                    {
                        println!(
                            "No embedding service available (Ollama not reachable, local-embeddings feature not enabled)"
                        );
                        return Ok(());
                    }
                }
            };

            let mind = "default";
            let facts = backend
                .list_facts(mind, omegon_memory::FactFilter::default())
                .await?;
            let _meta = backend.embedding_metadata(mind).await?;

            let mut backfilled = 0u32;
            let total = facts.len();
            println!(
                "Backfilling embeddings for {total} facts using {}...",
                embed_svc.model_name()
            );

            for (i, fact) in facts.iter().enumerate() {
                match embed_svc.embed(&fact.content).await {
                    Ok(embedding) => {
                        if let Err(e) = backend
                            .store_embedding(&fact.id, embed_svc.model_name(), &embedding)
                            .await
                        {
                            tracing::warn!(fact_id = %fact.id, error = %e, "failed to store embedding");
                        } else {
                            backfilled += 1;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(fact_id = %fact.id, error = %e, "failed to generate embedding");
                    }
                }

                if (i + 1) % 50 == 0 || i + 1 == total {
                    println!("  [{}/{}] {backfilled} embedded", i + 1, total);
                }
            }

            println!(
                "\nBackfill complete: {backfilled}/{total} facts embedded with {}",
                embed_svc.model_name()
            );
        }
    }
    Ok(())
}

async fn run_doctor_command(cli: &Cli) -> anyhow::Result<()> {
    let cwd = std::fs::canonicalize(&cli.cwd)?;
    let repo_root = setup::find_project_root(&cwd);
    let findings = lifecycle::doctor::audit_repo(&repo_root);

    println!("═══ Lifecycle Audit ═══");
    if findings.is_empty() {
        println!("✓ No suspicious lifecycle drift found.");
    } else {
        println!("{} finding(s)\n", findings.len());
        for f in findings {
            println!("- {} [{}]", f.node_id, f.kind.as_str());
            println!("  {}", f.title);
            println!("  {}", f.detail);
        }
    }

    println!("\n═══ Ollama Diagnostics ═══");
    let mgr = ollama::OllamaManager::new();

    // Registration check
    let manifest_path = ollama_manifest_path();
    if manifest_path.exists() {
        let binary_ok = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| v["binary"].as_str().map(|s| Path::new(s).exists()))
            .unwrap_or(false);
        println!(
            "Registration:  {}",
            if binary_ok {
                "✓ registered, binary valid"
            } else {
                "⚠ registered but binary path invalid — re-run `omegon ollama register`"
            }
        );
    } else {
        println!("Registration:  ✗ not registered (run `omegon ollama register`)");
    }

    // Reachability
    if mgr.is_reachable().await {
        println!("Server:        ✓ reachable");

        match mgr.list_models().await {
            Ok(models) => {
                println!("Models:        {} available", models.len());
                // Check for embedding model
                let has_embed = models.iter().any(|m| m.name.contains("nomic-embed"));
                if has_embed {
                    println!("Embeddings:    ✓ nomic-embed-text present");
                } else {
                    println!(
                        "Embeddings:    ⚠ nomic-embed-text not found — run `ollama pull nomic-embed-text` for hybrid search"
                    );
                }
            }
            Err(e) => println!("Models:        ✗ failed to list ({e})"),
        }

        match mgr.list_running().await {
            Ok(running) if !running.is_empty() => {
                println!("Loaded:        {} model(s) warm", running.len());
            }
            Ok(_) => {
                println!("Loaded:        none — first inference will cold-start");
            }
            Err(_) => {}
        }
    } else {
        println!("Server:        ✗ not reachable (is Ollama running?)");
    }

    // Hardware
    let hw = ollama::OllamaManager::hardware_profile();
    let vram_gb = hw.estimated_vram_bytes as f64 / 1_073_741_824.0;
    println!(
        "Hardware:      {:.0} GB VRAM, recommended max: {} params",
        vram_gb, hw.recommended_max_params
    );

    println!("\n═══ Toolchain ═══");
    // PKL (required for custom postures/agent configs)
    match std::process::Command::new("pkl").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let ver = String::from_utf8_lossy(&output.stdout);
            println!("PKL:           ✓ {}", ver.trim());
        }
        _ => {
            println!("PKL:           ✗ not installed (needed for custom .pkl configs)");
            println!("               brew install pkl  or  https://pkl-lang.org");
        }
    }
    // Git
    match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let ver = String::from_utf8_lossy(&output.stdout);
            println!("Git:           ✓ {}", ver.trim());
        }
        _ => println!("Git:           ✗ not found"),
    }
    // jj (Jujutsu — preferred VCS layer)
    match std::process::Command::new("jj").arg("version").output() {
        Ok(output) if output.status.success() => {
            let ver = String::from_utf8_lossy(&output.stdout);
            println!("jj:            ✓ {}", ver.trim());
            // Check if current project is jj-colocated
            let cwd = std::fs::canonicalize(&cli.cwd).unwrap_or_default();
            let project_root = setup::find_project_root(&cwd);
            if project_root.join(".jj").exists() {
                println!("               co-located repo detected");
                // Show current change ID
                if let Ok(out) = std::process::Command::new("jj")
                    .args(["log", "--no-graph", "-r", "@", "-T", "change_id.short()"])
                    .current_dir(&project_root)
                    .output()
                    && out.status.success()
                {
                    let id = String::from_utf8_lossy(&out.stdout);
                    println!("               working copy: {}", id.trim());
                }
            }
        }
        _ => {
            println!("jj:            - not installed (optional, recommended)");
            println!("               https://martinvonz.github.io/jj/");
        }
    }

    Ok(())
}

fn parse_posture(s: &str) -> Result<String, String> {
    // Accept any string — resolution happens later when we have cwd context.
    // Built-in names and custom posture names are both valid here.
    Ok(s.to_lowercase())
}

/// Resolve posture from CLI flags. Shorthand flags take priority over --posture.
fn resolve_cli_posture(cli: &Cli) -> Option<String> {
    if cli.explorator {
        Some("explorator".to_string())
    } else if cli.fabricator {
        Some("fabricator".to_string())
    } else if cli.architect {
        Some("architect".to_string())
    } else if cli.devastator {
        Some("devastator".to_string())
    } else {
        cli.posture.clone()
    }
}

/// Apply a posture name to settings, resolving custom postures from the filesystem.
fn apply_posture_to_settings(name: &str, settings: &mut settings::Settings, cwd: &std::path::Path) {
    match settings::resolve_posture_by_name(name, cwd) {
        Ok(settings::ResolvedPosture::BuiltIn(preset)) => {
            settings.set_posture(preset);
            tracing::info!(posture = name, "posture set from CLI flag");
        }
        Ok(settings::ResolvedPosture::Custom(custom)) => {
            custom.apply_to(settings);
            tracing::info!(
                posture = name,
                base = custom.def.posture.base,
                "custom posture loaded"
            );
        }
        Err(e) => {
            tracing::warn!("posture resolution failed: {e}");
            eprintln!("warning: {e}");
        }
    }
}

fn cli_prefers_slim_mode(cli: &Cli) -> bool {
    cli.slim
}

async fn run_interactive_command(cli: &Cli) -> anyhow::Result<()> {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
    tracing::info!(model = %cli.model, "omegon interactive starting");

    // Check .omegon-version — show in bootstrap panel (before TUI takes over stderr)
    if let Some(warning) = switch::check_version_file_warning(&cli.cwd) {
        eprintln!("{warning}");
    }

    let shared_settings = bootstrap::initialize_shared_settings(&bootstrap::SettingsInit {
        model: &cli.model,
        cwd: &cli.cwd,
        cli_posture: resolve_cli_posture(cli).as_deref(),
        slim: cli_prefers_slim_mode(cli),
        full: cli.full,
        max_turns: cli.max_turns,
        apply_profile_posture: true,
    });

    // Walk the system temp dir for `omegon-clipboard-*` files older
    // than `Settings.clipboard_retention_hours` (default 24h, 0 to
    // disable) and delete them. Without this sweep clipboard image
    // pastes accumulate indefinitely — operators were seeing
    // multi-month backlogs in `/tmp`. The sweep runs once per
    // interactive launch, before the rest of setup so any failures
    // here don't block startup.
    let clipboard_retention_hours = shared_settings
        .lock()
        .ok()
        .map(|s| s.clipboard_retention_hours)
        .unwrap_or(24);
    let clipboard_retention =
        std::time::Duration::from_secs(clipboard_retention_hours.saturating_mul(3600));
    match clipboard::prune_old_pastes(clipboard_retention) {
        Ok(stats) if stats.deleted > 0 || stats.errors > 0 => {
            tracing::info!("{}", stats.summary());
        }
        Ok(_) => {
            // Nothing to clean — stay quiet on the common path so we
            // don't spam the log on every launch.
        }
        Err(e) => tracing::warn!(error = %e, "clipboard prune failed"),
    }

    // On first launch (no profile.json), sweep the system for existing tools
    // and let the operator choose a starting posture before the TUI appears.
    if first_run::should_run(&cli.cwd) {
        first_run::run_interactive(&cli.cwd, &shared_settings);
    }

    // Fresh by default. --resume opts into session restore; --resume with no value
    // means "most recent" and --fresh forces a clean start.
    let resume = interactive_resume_mode(cli);
    let mut agent = setup::AgentSetup::new_with_safety(&cli.cwd, resume, Some(shared_settings.clone()), cli.dangerously_bypass_permissions).await?;
    agent.instance_id = paths::instance_id("tui");
    bootstrap::apply_runtime_posture(
        &mut agent,
        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
        omegon_traits::OmegonAutonomyMode::OperatorDriven,
    );

    // LLM provider ──────────────────────────────────────────────────────
    // Native Rust clients by default. --bridge flag forces the Node.js subprocess.
    let requested_start_model = shared_settings
        .lock()
        .ok()
        .map(|s| s.model.clone())
        .unwrap_or_else(|| cli.model.clone());
    let resolved_cli_model = providers::resolve_execution_model_spec(&requested_start_model)
        .await
        .unwrap_or_else(|| requested_start_model.clone());
    let requested_provider = providers::infer_provider_id(&requested_start_model);
    let resolved_provider = providers::infer_provider_id(&resolved_cli_model);
    if resolved_cli_model != requested_start_model {
        tracing::info!(
            requested = %requested_start_model,
            requested_provider = %requested_provider,
            resolved = %resolved_cli_model,
            resolved_provider = %resolved_provider,
            "resolved startup model to executable provider without changing selected model"
        );
    } else {
        tracing::info!(
            selected = %requested_start_model,
            selected_provider = %requested_provider,
            "startup model provider route selected"
        );
    }

    let imported_credentials = auth::import_discovered_provider_credentials();
    if imported_credentials > 0 {
        tracing::info!(
            imported = imported_credentials,
            "imported discovered provider credentials before interactive bridge resolution"
        );
    }

    let mut startup_auth_warnings = Vec::new();
    let selected_provider_status = auth::provider_by_id(&requested_provider)
        .map(auth::provider_session_status);
    if selected_provider_status == Some(auth::ProviderSessionStatus::Expired) {
        startup_auth_warnings.push(format!(
            "Credentials for {} are expired. Run /login {} to refresh them before continuing with that profile model.",
            requested_start_model, requested_provider
        ));
    }

    let fallback_providers = shared_settings
        .lock()
        .ok()
        .map(|s| s.fallback_providers.clone())
        .unwrap_or_default();
    let route_ledger = route::CredentialLedger;
    let mut startup_route = route::RouteController::resolve_startup(
        resolved_cli_model.clone(),
        &fallback_providers,
        &route_ledger,
    )
    .await;
    let refreshable_startup_provider = match &startup_route {
        route::ProviderRoute::Disconnected {
            reason:
                route::DisconnectedReason::MissingCredentials {
                    provider,
                    ..
                }
                | route::DisconnectedReason::ExpiredCredentials {
                    provider,
                    ..
                },
            ..
        } => Some(provider.as_str()),
        _ => None,
    };
    if let Some(provider) = refreshable_startup_provider {
        // Startup can begin before a just-completed browser login has been
        // flushed through every auth surface. Do one refresh/adoption pass
        // before emitting the operator-facing credential warning so a valid or
        // refreshable auth.json entry is not reported as absent/stale on relaunch.
        if crate::auth::resolve_with_refresh(provider).await.is_some() {
            startup_route = route::RouteController::resolve_startup(
                resolved_cli_model.clone(),
                &fallback_providers,
                &route_ledger,
            )
            .await;
        }
    }
    let resolved_bridge = match &startup_route {
        route::ProviderRoute::Serving { model }
        | route::ProviderRoute::Fallback { serving: model, .. } => {
            providers::auto_detect_bridge(model).await
        }
        route::ProviderRoute::Disconnected { .. } | route::ProviderRoute::LoginPending { .. } => {
            None
        }
    };
    let mut startup_decision = decide_interactive_startup_model(
        &requested_start_model,
        &resolved_cli_model,
        resolved_bridge.is_some(),
    );
    let (effective_model, bridge): (String, Box<dyn LlmBridge>) = match (&startup_route, resolved_bridge) {
        (route::ProviderRoute::Serving { model }, Some(native)) => {
            tracing::info!(model = %model, "using native LLM provider");
            startup_decision.bridge_model = model.clone();
            startup_decision.provider_connected = true;
            startup_decision.use_null_bridge = false;
            (model.clone(), native)
        }
        (route::ProviderRoute::Fallback { selected, serving, reason }, Some(fallback_bridge)) => {
            tracing::warn!(
                selected = %selected,
                serving = %serving,
                reason = ?reason,
                "selected interactive model unavailable; using explicitly configured fallback provider"
            );
            startup_auth_warnings.push(format!(
                "Selected profile model {selected} is unavailable for this session; explicitly configured fallback {serving} is serving. Remove `fallbackProviders` or refresh credentials to stop fallback."
            ));
            startup_decision.bridge_model = serving.clone();
            startup_decision.provider_connected = true;
            startup_decision.use_null_bridge = false;
            (serving.clone(), fallback_bridge)
        }
        (route::ProviderRoute::Fallback { selected, serving, .. }, None) => {
            tracing::warn!(selected = %selected, serving = %serving, "configured fallback provider resolved but bridge detection failed");
            startup_auth_warnings.push(format!(
                "Configured fallback {serving} for {selected} could not start. Run /login {} or update fallbackProviders.",
                providers::infer_provider_id(serving)
            ));
            startup_decision.provider_connected = false;
            startup_decision.use_null_bridge = true;
            (serving.clone(), Box::new(bridge::NullBridge) as Box<dyn LlmBridge>)
        }
        (route::ProviderRoute::Serving { model }, None) => {
            tracing::warn!(model = %model, "startup credential probe passed but bridge detection failed");
            startup_auth_warnings.push(format!(
                "LLM provider credentials were detected for {model}, but the provider bridge could not start. Run /login {} or check provider configuration.",
                providers::infer_provider_id(model)
            ));
            startup_decision.provider_connected = false;
            startup_decision.use_null_bridge = true;
            (model.clone(), Box::new(bridge::NullBridge) as Box<dyn LlmBridge>)
        }
        (route::ProviderRoute::Disconnected { selected, reason }, _) => {
            tracing::warn!(selected = %selected, reason = ?reason, "no LLM provider available for selected interactive model and no explicit fallback engaged");
            startup_auth_warnings.push(reason.operator_message(selected));
            if fallback_providers.is_empty()
                && let Some(legacy_fallback) = providers::automation_safe_model()
                && providers::infer_provider_id(&legacy_fallback)
                    != providers::infer_provider_id(selected)
            {
                startup_auth_warnings.push(format!(
                    "Omegon no longer silently falls back from {selected} to {legacy_fallback}. To opt into that route, add `fallbackProviders = [\"{}\"]` to the profile, or run `/login {}` to use the selected provider.",
                    providers::infer_provider_id(&legacy_fallback),
                    providers::infer_provider_id(selected)
                ));
            }
            startup_decision.provider_connected = false;
            startup_decision.use_null_bridge = true;
            (
                startup_decision.bridge_model.clone(),
                Box::new(bridge::NullBridge) as Box<dyn LlmBridge>,
            )
        }
        (route::ProviderRoute::LoginPending { .. }, _) => unreachable!("startup route cannot be login-pending"),
    };
    // Update settings with selected-model provider status before TUI reads it.
    if let Ok(mut s) = shared_settings.lock() {
        s.provider_connected = startup_decision.provider_connected;
    }
    let (events_tx, events_rx) = bootstrap::wire_event_channel(&agent, 256);
    let startup_model_intent = settings::Profile::load(&agent.cwd)
        .model_intent
        .and_then(|intent| intent.to_route_intent())
        .unwrap_or_else(|| route::intent_from_route(&startup_route));
    let route_controller = Arc::new(route::RouteController::with_initial_intent(
        startup_route.clone(),
        bridge,
        Some(events_tx.clone()),
        startup_model_intent,
    ));
    let bridge: Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>> = route_controller.bridge();
    if shared_settings.lock().is_ok() {
        agent.bus.replace_feature(Box::new(
            features::model_budget::ModelBudget::with_route_controller(
                shared_settings.clone(),
                route_controller.clone(),
            ),
        ));
        agent.bus.finalize();
    }
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
    for message in startup_auth_warnings {
        let _ = events_tx.send(AgentEvent::SystemNotification { message });
    }
    match &agent.workspace_state.admission {
        crate::workspace::types::AdmissionOutcome::GrantedMutable
        | crate::workspace::types::AdmissionOutcome::GrantedReadOnly => {}
        crate::workspace::types::AdmissionOutcome::ConflictReadOnlySuggested {
            owner_session_id,
        } => {
            let owner = owner_session_id.as_deref().unwrap_or("(unknown)");
            let message = format!(
                "Workspace is already owned by mutable session {owner}. Current startup remained local-only for now; use /workspace to inspect details. Recommended next step: open a sibling workspace or attach read-only until stale-lease adoption is implemented."
            );
            let _ = events_tx.send(AgentEvent::SystemNotification { message });
        }
        crate::workspace::types::AdmissionOutcome::ConflictCreateWorkspaceSuggested {
            owner_session_id,
        } => {
            let owner = owner_session_id.as_deref().unwrap_or("(unknown)");
            let message = format!(
                "Workspace is already occupied by session {owner}. This workflow should use a separate mutable workspace. Use /workspace to inspect current ownership before starting parallel mutable work."
            );
            let _ = events_tx.send(AgentEvent::SystemNotification { message });
        }
        crate::workspace::types::AdmissionOutcome::ConflictStaleLeaseAdoptable {
            owner_session_id,
        } => {
            let owner = owner_session_id.as_deref().unwrap_or("(unknown)");
            let message = format!(
                "Workspace lease from session {owner} appears stale. Use /workspace to inspect the current lease before adopting it or creating a sibling workspace."
            );
            let _ = events_tx.send(AgentEvent::SystemNotification { message });
        }
        crate::workspace::types::AdmissionOutcome::DeniedByAuthorityPolicy { reason } => {
            let message = format!(
                "Workspace authority warning: {reason}. Use /workspace to inspect the current role and occupancy state."
            );
            let _ = events_tx.send(AgentEvent::SystemNotification { message });
        }
    }

    let shared_cancel: tui::SharedCancel = std::sync::Arc::new(std::sync::Mutex::new(None));

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
        if provider == "anthropic"
            && let Some(limits) = auth::probe_anthropic_model_limits(model_id).await
                && let Ok(mut s) = shared_settings.lock() {
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

    let is_oauth = shared_settings
        .lock()
        .ok()
        .map(|s| crate::providers::infer_provider_id(&s.model))
        .and_then(|provider| providers::resolve_api_key_sync(&provider))
        .is_some_and(|(_, oauth)| oauth);

    if let Some(ref class_str) = cli.context_class
        && let Ok(mut s) = shared_settings.lock() {
            match class_str.to_lowercase().as_str() {
                "compact" | "squad" => s.set_requested_context_class(settings::ContextClass::Compact),
                "standard" | "maniple" => {
                    s.set_requested_context_class(settings::ContextClass::Standard)
                }
                "extended" | "clan" => s.set_requested_context_class(settings::ContextClass::Extended),
                "massive" | "legion" => s.set_requested_context_class(settings::ContextClass::Massive),
                _ => tracing::warn!("Unknown context class: {class_str}"),
            }
            tracing::info!(class = %class_str, "requested context class policy applied");
        }

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
    let runtime_inventory = setup::RuntimeSubstrateInventory::from_agent_setup(&agent);
    let startup_skill_activation_events = agent.startup_skill_activation_events.clone();
    let runtime_generation = 1;
    let extension_widgets = std::mem::take(&mut agent.extension_widgets);
    let widget_receivers = std::mem::take(&mut agent.widget_receivers);
fn build_tui_secret_readiness_snapshot(
    agent: &setup::AgentSetup,
) -> Option<crate::capabilities::secrets::SecretReadinessSnapshot> {
    let home = crate::paths::omegon_home().ok()?;
    let armory_home = home.join("armory");
    let project_armory = agent.cwd.join("../omegon-armory");
    let armory_root =
        if !armory_home.join("profiles").exists() && project_armory.join("profiles").exists() {
            project_armory.as_path()
        } else {
            armory_home.as_path()
        };
    let roots = crate::capabilities::inventory::CapabilityInventoryRoots {
        extensions_dir: &home.join("extensions"),
        armory_root,
        catalog_dir: &home.join("catalog"),
    };
    let secret_inputs = crate::capabilities::secrets::SecretReadinessInputs {
        session_diagnostics: agent
            .secrets
            .session_diagnostics()
            .into_iter()
            .map(|diag| crate::capabilities::secrets::SecretSessionDiagnostic {
                name: diag.name,
                warmed: diag.warmed,
            })
            .collect(),
        recipe_descriptors: agent
            .secrets
            .list_recipe_descriptors()
            .into_iter()
            .map(|descriptor| crate::capabilities::secrets::SecretRecipeDescriptorSummary {
                name: descriptor.name,
                kind: descriptor.kind,
            })
            .collect(),
    };
    crate::capabilities::inventory::build_capability_inventory_snapshot_with_secrets(
        roots,
        secret_inputs,
    )
    .map(|snapshot| snapshot.secret_readiness)
    .map_err(|error| {
        tracing::warn!(?error, "failed to build TUI secret readiness snapshot");
        error
    })
    .ok()
}

    let voice_notification_receivers = std::mem::take(&mut agent.voice_notification_receivers);
    let voice_polling_handles = std::mem::take(&mut agent.voice_polling_handles);
    // Show splash only on first launch; skip on subsequent runs unless
    // the operator explicitly replays via /splash.
    let is_first_run = first_run::should_run(&cli.cwd);
    let tui_config = tui::TuiConfig {
        cwd: agent.cwd.to_string_lossy().to_string(),
        is_oauth,
        initial,
        no_splash: cli.no_splash || !is_first_run,
        bus_commands,
        runtime_generation,
        runtime_inventory,
        secret_readiness: build_tui_secret_readiness_snapshot(&agent),
        startup_skill_activation_events,
        dashboard_handles: agent.dashboard_handles.clone(),
        initial_prompt,
        start_tutorial: cli.tutorial,
        resume_info: agent.resume_info.clone(),
        login_prompt_tx: login_prompt_tx.clone(),
        extension_widgets,
        widget_receivers,
        voice_notification_receivers,
        voice_polling_handles,
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
            shared_settings.clone(),
            shared_cancel.clone(),
            ipc_cancel.clone(),
        );
    }

    let _mqtt_bridge =
        maybe_start_mqtt_bridge(&agent.cwd, agent.session_id.clone(), events_tx.clone());

    let (mut agent, mut runtime_state) = split_interactive_agent(agent);

    let runtime_resources = InteractiveRuntimeResources {
        cwd: agent.cwd.clone(),
        secrets: agent.secrets.clone(),
        context_metrics: agent.context_metrics.clone(),
        bridge_model: std::sync::Arc::new(std::sync::Mutex::new(Some(effective_model.clone()))),
        route_controller: route_controller.clone(),
    };

    runtime_state.bus.emit(&omegon_traits::BusEvent::SessionStart {
        cwd: agent.cwd.clone(),
        session_id: agent.session_id.clone(),
    });
    // Drain any requests from session_start handlers
    for request in runtime_state.bus.drain_requests() {
        match request {
            omegon_traits::BusRequest::Notify { message, .. } => {
                let _ = events_tx.send(AgentEvent::SystemNotification { message });
            }
            omegon_traits::BusRequest::AutoStoreFact { .. } => {} // no-op: memory not ready yet
            _ => {}
        }
    }

    let mut runtime = InteractiveRuntimeSupervisor::default();
    let mut deferred_commands = VecDeque::new();
    'interactive: loop {
        let cmd = if let Some(cmd) = deferred_commands.pop_front() {
            cmd
        } else {
            match command_rx.recv().await {
                Some(cmd) => cmd,
                None => break,
            }
        };

        let cmd = match cmd {
            tui::TuiCommand::VoicePrompt { text, metadata } => tui::TuiCommand::SubmitPrompt(tui::PromptSubmission {
                text: format!("🎙 {}", text.trim()),
                image_paths: Vec::new(),
                submitted_by: "voice".to_string(),
                via: "voice",
                queue_mode: tui::PromptQueueMode::UntilReady,
                metadata: tui::PromptMetadata { voice: Some(metadata) },
            }),
            other => other,
        };

        match cmd {
            tui::TuiCommand::Quit => break,

            tui::TuiCommand::ExecuteControl { request, respond_to } => {
                let mut ctx = control_runtime::ControlContext {
                    runtime_state: &mut runtime_state,
                    agent: &mut agent,
                    shared_settings: &shared_settings,
                    bridge: &bridge,
                    login_prompt_tx: &login_prompt_tx,
                    events_tx: &events_tx,
                    cli: &CliRuntimeView {
                        no_session: cli.no_session,
                        model: &cli.model,
                        dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
                    },
                };
                let response = control_runtime::execute_control(&mut ctx, request).await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::UpdatePlan {
                command,
                respond_to,
            } => {
                let response = execute_plan_slash_command(&mut runtime_state, command);
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let repo_root = setup::find_project_root(&cwd);
                let projection = runtime_state
                    .conversation
                    .intent
                    .plan_surface_projection_for_repo(&repo_root);
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                let _ = events_tx.send(AgentEvent::PlanUpdated { projection });
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::RunShellCommand { command, respond_to } => {
                // Spawn so the command loop stays unblocked — operator can
                // submit new prompts / commands while this is in-flight.
                let cwd = agent.cwd.clone();
                let events = events_tx.clone();
                let cancel = tokio_util::sync::CancellationToken::new();

                // Unique ID for this command's tool card.  nanos-since-epoch
                // is fine — shell commands are human-paced, not concurrent.
                let id = format!(
                    "shell-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0),
                );

                // Open a streaming bash card in the conversation immediately
                // so the operator sees the command before output arrives.
                let _ = events.send(AgentEvent::ToolStart {
                    id: id.clone(),
                    name: "bash".to_string(),
                    args: serde_json::json!({ "command": command }),
                });

                tokio::spawn(async move {
                    // Wire execute_streaming → ToolUpdate so the live-partial
                    // region of the card refreshes every 150 ms of new output
                    // (and emits heartbeats every 5 s while silent).
                    let events_sink = events.clone();
                    let id_sink = id.clone();
                    let sink = omegon_traits::ToolProgressSink::from_fn(move |partial| {
                        let _ = events_sink.send(AgentEvent::ToolUpdate {
                            id: id_sink.clone(),
                            partial,
                        });
                    });

                    let result = crate::tools::bash::execute_streaming(
                        &command,
                        &cwd,
                        Some(300),
                        cancel,
                        sink,
                        None,
                    )
                    .await;

                    let (tool_result, is_error) = match result {
                        Ok(r) => {
                            let exited_nonzero = r
                                .details
                                .get("exitCode")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0)
                                != 0;
                            (r, exited_nonzero)
                        }
                        Err(e) => (
                            omegon_traits::ToolResult {
                                content: vec![omegon_traits::ContentBlock::Text {
                                    text: format!("Shell command failed: {e}"),
                                }],
                                details: serde_json::json!({ "exitCode": -1 }),
                            },
                            true,
                        ),
                    };

                    // Close the card with the final result.
                    let _ = events.send(AgentEvent::ToolEnd {
                        id: id.clone(),
                        name: "bash".to_string(),
                        result: tool_result.clone(),
                        is_error,
                    });

                    // Honour control-API callers that pass a respond_to channel.
                    if let Some(tx) = respond_to {
                        let output = tool_result
                            .content
                            .iter()
                            .filter_map(|b| match b {
                                omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let _ = tx.send(omegon_traits::ControlOutputResponse {
                            accepted: !is_error,
                            output: Some(output),
                        });
                    }
                });
            }

            tui::TuiCommand::ShellHandoff { keyboard_enhancement } => {
                if runtime.is_busy() {
                    let _ = events_tx.send(AgentEvent::SystemNotification {
                        message: "Shell handoff refused while a turn is active. Cancel first.".into(),
                    });
                    continue;
                }

                #[cfg(unix)]
                {
                    use crossterm::event::DisableBracketedPaste;
                    use crossterm::terminal::LeaveAlternateScreen;

                    if keyboard_enhancement {
                        let _ = io::stdout()
                            .execute(crossterm::event::PopKeyboardEnhancementFlags);
                    }
                    let _ = disable_raw_mode();
                    let _ = io::stdout().execute(DisableMouseCapture);
                    let _ = io::stdout().execute(DisableBracketedPaste);
                    let _ = io::stdout().execute(LeaveAlternateScreen);
                    let _ = io::stdout().flush();

                    let suspend_result = unsafe { libc::raise(libc::SIGTSTP) };
                    let handoff_error = if suspend_result != 0 {
                        Some(std::io::Error::last_os_error().to_string())
                    } else {
                        None
                    };

                    let _ = enable_raw_mode();
                    if keyboard_enhancement {
                        let _ = io::stdout().execute(
                            crossterm::event::PushKeyboardEnhancementFlags(
                                crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
                            ),
                        );
                    }
                    let _ = io::stdout().execute(EnterAlternateScreen);
                    let _ = io::stdout().execute(crossterm::event::EnableBracketedPaste);
                    let _ = io::stdout().execute(EnableMouseCapture);
                    let _ = io::stdout().flush();

                    if let Some(err) = handoff_error {
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: format!("Shell handoff failed: {err}"),
                        });
                    }
                }

                #[cfg(not(unix))]
                {
                    let _ = events_tx.send(AgentEvent::SystemNotification {
                        message: "Shell handoff is not implemented on this platform yet.".into(),
                    });
                }
            }

            tui::TuiCommand::ModelView { respond_to } => {
                let response = control_runtime::model_view_response(&shared_settings).await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::ModelList { respond_to } => {
                let response = control_runtime::model_list_response().await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::SetModel { model, respond_to } => {
                let response = control_runtime::set_model_response(
                    &mut agent,
                    &shared_settings,
                    &bridge,
                    Some(route_controller.clone()),
                    &model,
                )
                .await;
                if let Some(output) = response.output.clone() {
                    for line in output.split('\n') {
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: line.to_string(),
                        });
                    }
                }
                if response.accepted {
                    if let Ok(mut bridge_model) = runtime_resources.bridge_model.lock() {
                        *bridge_model = None;
                    }
                    let snapshot = route_controller.snapshot().await;
                    if let Err(err) = persist_model_intent(&agent.cwd, &snapshot.intent) {
                        let _ = events_tx.send(AgentEvent::SystemNotification { message: format!("Failed to persist model intent: {err}") });
                    }
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::SetModelGrade { grade, respond_to } => {
                let response = if let Some(parsed) = crate::route::ModelGrade::parse(&grade) {
                    let snapshot = route_controller
                        .set_model_intent(crate::route::ModelIntent::with_grade(parsed))
                        .await;
                    if let Err(err) = persist_model_intent(&agent.cwd, &snapshot.intent) {
                        let _ = events_tx.send(AgentEvent::SystemNotification { message: format!("Failed to persist model intent: {err}") });
                    }
                    let resolved = resolve_current_model_intent_route(&route_controller).await;
                    let active = resolved
                        .as_ref()
                        .and_then(|snapshot| snapshot.serving_model())
                        .or_else(|| snapshot.serving_model())
                        .unwrap_or("disconnected");
                    omegon_traits::SlashCommandResponse {
                        accepted: true,
                        output: Some(format!(
                            "Model intent updated — {}. Active route: {}",
                            snapshot.intent.summary(),
                            active
                        )),
                    }
                } else {
                    omegon_traits::SlashCommandResponse {
                        accepted: false,
                        output: Some(format!(
                            "Invalid model grade: {grade}. Use F, D, C, B, A, or S. Use /model provider local for local endpoints."
                        )),
                    }
                };
                if let Some(output) = response.output.clone() {
                    for line in output.split('\n') {
                        let _ = events_tx.send(AgentEvent::SystemNotification {
                            message: line.to_string(),
                        });
                    }
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::SetModelProvider { provider, respond_to } => {
                let response = if let Some(selection) = crate::route::ProviderSelection::parse(&provider) {
                    let snapshot = route_controller.set_provider_selection(selection).await;
                    if let Err(err) = persist_model_intent(&agent.cwd, &snapshot.intent) {
                        let _ = events_tx.send(AgentEvent::SystemNotification { message: format!("Failed to persist model intent: {err}") });
                    }
                    let resolved = resolve_current_model_intent_route(&route_controller).await;
                    let active = resolved
                        .as_ref()
                        .and_then(|snapshot| snapshot.serving_model())
                        .or_else(|| snapshot.serving_model())
                        .unwrap_or("disconnected");
                    omegon_traits::SlashCommandResponse {
                        accepted: true,
                        output: Some(format!(
                            "Model provider intent updated — {}. Active route: {}",
                            snapshot.intent.summary(),
                            active
                        )),
                    }
                } else {
                    omegon_traits::SlashCommandResponse {
                        accepted: false,
                        output: Some("Invalid model provider selector. Use auto, local, upstream, or an endpoint id.".into()),
                    }
                };
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::SetModelPolicy { policy, respond_to } => {
                let response = if let Some(parsed) = crate::route::GradePolicy::parse(&policy) {
                    let snapshot = route_controller.set_grade_policy(parsed).await;
                    if let Err(err) = persist_model_intent(&agent.cwd, &snapshot.intent) {
                        let _ = events_tx.send(AgentEvent::SystemNotification { message: format!("Failed to persist model intent: {err}") });
                    }
                    let resolved = resolve_current_model_intent_route(&route_controller).await;
                    let active = resolved
                        .as_ref()
                        .and_then(|snapshot| snapshot.serving_model())
                        .or_else(|| snapshot.serving_model())
                        .unwrap_or("disconnected");
                    omegon_traits::SlashCommandResponse {
                        accepted: true,
                        output: Some(format!(
                            "Model policy intent updated — {}. Active route: {}",
                            snapshot.intent.summary(),
                            active
                        )),
                    }
                } else {
                    omegon_traits::SlashCommandResponse {
                        accepted: false,
                        output: Some("Invalid model policy. Use exact, minimum, or nearest.".into()),
                    }
                };
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::ModelUnpin { respond_to } => {
                let snapshot = route_controller.clear_exact_model_override().await;
                if let Err(err) = persist_model_intent(&agent.cwd, &snapshot.intent) {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: format!("Failed to persist model intent: {err}") });
                }
                let output = format!(
                    "Model exact override cleared — {}. Active route unchanged: {}",
                    snapshot.intent.summary(),
                    snapshot.serving_model().unwrap_or("disconnected")
                );
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: output.clone(),
                });
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some(output),
                    });
                }
            }

            tui::TuiCommand::SetThinking { level, respond_to } => {
                let response =
                    control_runtime::set_thinking_response(&shared_settings, &agent.cwd, level).await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::Compact => {
                tracing::info!("manual compaction requested");

                let bridge_guard = bridge.read().await;
                let stream_options = {
                    let s = shared_settings.lock().unwrap();
                    let model = runtime_resources
                        .bridge_model
                        .lock()
                        .ok()
                        .and_then(|guard| guard.clone())
                        .unwrap_or_else(|| s.model.clone());
                    crate::bridge::StreamOptions {
                        model: Some(model),
                        reasoning: Some(s.thinking.as_str().to_string()),
                        extended_context: false,
                        ..Default::default()
                    }
                };
                if let Some((payload, _evict_count)) = runtime_state.conversation.build_compaction_payload()
                {
                    match r#loop::compact_via_llm(bridge_guard.as_ref(), &payload, &stream_options)
                        .await
                    {
                        Ok(summary) => {
                            runtime_state.conversation.apply_compaction(summary);
                            let est = runtime_state.conversation.estimate_tokens();
                            if let Ok(s) = shared_settings.lock() {
                                let ctx_window = s.context_window;

                                // Update metrics
                                if let Ok(mut metrics) = agent.context_metrics.lock() {
                                    metrics.update(
                                        est,
                                        ctx_window,
                                        s.effective_requested_class().label(),
                                        s.thinking.as_str(),
                                    );
                                }

                                if ctx_window > 0 {
                                    let system_prompt = runtime_state.context_manager.build_system_prompt(
                                        runtime_state.conversation.last_user_prompt(),
                                        &runtime_state.conversation,
                                    );
                                    let llm_messages = runtime_state.conversation.build_llm_view();
                                    let prompt_telemetry = runtime_state.context_manager.last_prompt_telemetry();
                                    let context_composition = crate::r#loop::compute_context_composition(
                                        &system_prompt,
                                        &llm_messages,
                                        &runtime_state.bus.tool_definitions(),
                                        ctx_window,
                                        Some(&prompt_telemetry),
                                    );
                                    let _ = events_tx.send(AgentEvent::TurnEnd(Box::new(omegon_traits::AgentEventTurnEnd {
                                        turn: runtime_state.conversation.intent.stats.turns,
                                        turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
                                        model: None,
                                        provider: None,
                                        estimated_tokens: est,
                                        context_window: ctx_window,
                                        context_composition,
                                        actual_input_tokens: 0,
                                        actual_output_tokens: 0,
                                        cache_read_tokens: 0,
                                        cache_creation_tokens: 0,
                                        provider_telemetry: None,
                                        dominant_phase: None,
                                        drift_kind: None,
                                        progress_nudge_reason: None,
                                        intent_task: runtime_state.conversation.intent.current_task.clone(),
                                        intent_phase: Some(format!("{:?}", runtime_state.conversation.intent.lifecycle_phase)),
                                        files_read_count: runtime_state.conversation.intent.files_read.len(),
                                        files_modified_count: runtime_state.conversation.intent.files_modified.len(),
                                        stats_tool_calls: runtime_state.conversation.intent.stats.tool_calls,
                                        // Compaction-completion synthetic emit — no controller in scope.
                                        // Streak counters are an in-loop signal; out-of-loop emitters
                                        // surface zeros and let consumers tell the difference between
                                        // "no streaks" and "no controller" via the loop's own emissions.
                                        streaks: omegon_traits::ControllerStreaks::default(),
                                    })));
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

            tui::TuiCommand::ContextStatus { respond_to } => {
                let response = control_runtime::context_status_response(&runtime_state, &shared_settings).await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::ContextCompact { respond_to } => {
                let mut ctx = control_runtime::ControlContext {
                    runtime_state: &mut runtime_state,
                    agent: &mut agent,
                    shared_settings: &shared_settings,
                    bridge: &bridge,
                    login_prompt_tx: &login_prompt_tx,
                    events_tx: &events_tx,
                    cli: &CliRuntimeView {
                        no_session: cli.no_session,
                        model: &cli.model,
                        dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
                    },
                };
                let response = control_runtime::execute_control(
                    &mut ctx,
                    control_runtime::ControlRequest::ContextCompact,
                )
                .await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::ContextClear { respond_to } => {
                let mut ctx = control_runtime::ControlContext {
                    runtime_state: &mut runtime_state,
                    agent: &mut agent,
                    shared_settings: &shared_settings,
                    bridge: &bridge,
                    login_prompt_tx: &login_prompt_tx,
                    events_tx: &events_tx,
                    cli: &CliRuntimeView {
                        no_session: cli.no_session,
                        model: &cli.model,
                        dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
                    },
                };
                let response = control_runtime::execute_control(
                    &mut ctx,
                    control_runtime::ControlRequest::ContextClear,
                )
                .await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::ListSessions { respond_to } => {
                let text = list_sessions_message(&agent.cwd);
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: text.clone(),
                });
                let _ = events_tx.send(AgentEvent::AgentEnd);
                tracing::info!("{text}");
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some(text),
                    });
                }
            }

            tui::TuiCommand::NewSession { respond_to } => {
                let response = control_runtime::new_session_response(
                    &mut runtime_state,
                    &mut agent,
                    &CliRuntimeView {
                        no_session: cli.no_session,
                        model: &cli.model,
                        dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
                    },
                    &events_tx,
                )
                .await;
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::AuthStatus { respond_to } => {
                let response = control_runtime::auth_status_response().await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::AuthLogin { provider, respond_to } => {
                let response = control_runtime::auth_login_response(
                    &shared_settings,
                    &bridge,
                    &login_prompt_tx,
                    &events_tx,
                    &CliRuntimeView {
                        no_session: cli.no_session,
                        model: &cli.model,
                        dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
                    },
                    &agent.cwd,
                    &provider,
                )
                .await;
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::AuthLogout { provider, respond_to } => {
                let response = control_runtime::auth_logout_response(&provider).await;
                if response.accepted {
                    // Evict provider credentials from the secrets session cache
                    // so hydrate_process_env() cannot re-inject stale values.
                    let env_vars = crate::auth::provider_env_vars(&provider);
                    let evict_names: Vec<&str> = env_vars.to_vec();
                    agent.secrets.evict_secrets(&evict_names);

                    if let Ok(mut s) = shared_settings.lock() {
                        let active_provider = s
                            .model
                            .split_once(':')
                            .map(|(provider, _)| provider)
                            .unwrap_or(s.model.as_str())
                            .to_string();
                        if crate::auth::canonical_provider_id(&active_provider)
                            == crate::auth::canonical_provider_id(&provider)
                        {
                            s.provider_connected = false;
                        }
                    }
                    let mut status = crate::status::HarnessStatus::assemble();
                    status.update_runtime_posture(
                        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
                        omegon_traits::OmegonAutonomyMode::OperatorDriven,
                    );
                    let auth_status = auth::probe_all_providers().await;
                    status.providers = crate::auth::auth_status_to_provider_statuses(&auth_status);
                    status.annotate_provider_runtime_health();
                    status.update_from_bus(&runtime_state.bus);
                    if let Ok(json) = serde_json::to_value(&status) {
                        let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json: json });
                    }
                }
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::AuthUnlock { respond_to } => {
                let response = control_runtime::auth_unlock_response().await;
                if let Some(output) = response.output.clone() {
                    let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
                }
                if let Some(respond_to) = respond_to {
                    let _ = respond_to.send(omegon_traits::ControlOutputResponse {
                        accepted: response.accepted,
                        output: response.output,
                    });
                }
            }

            tui::TuiCommand::StartWebDashboard => {
                let web_state = web::WebState::with_auth_state_and_secrets(
                    agent.dashboard_handles.clone(),
                    events_tx.clone(),
                    agent.web_auth_state.clone(),
                    Some(agent.secrets.clone()),
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
                        tokio::spawn(async move {
                            let mut rx = web_cmd_rx;
                            while let Some(web_cmd) = rx.recv().await {
                                let tui_cmd = match web_cmd {
                                    web::WebCommand::UserPrompt { text, image_paths } => {
                                        tui::TuiCommand::SubmitPrompt(crate::tui::PromptSubmission {
                                            text,
                                            image_paths: image_paths
                                                .into_iter()
                                                .map(std::path::PathBuf::from)
                                                .collect(),
                                            submitted_by: "web-dashboard".to_string(),
                                            via: "websocket",
                                            queue_mode: crate::tui::PromptQueueMode::InterruptAfterTurn,
                                            metadata: crate::tui::PromptMetadata::default(),
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
                                    web::WebCommand::Cancel => tui::TuiCommand::CancelActiveTurn {
                                        submitted_by: "web-dashboard".to_string(),
                                        via: "websocket",
                                    },
                                    web::WebCommand::ExecuteControl { request, respond_to } => {
                                        if cmd_tx_clone.send(tui::TuiCommand::ExecuteControl { request, respond_to }).await.is_err() {
                                            break;
                                        }
                                        continue;
                                    }
                                    web::WebCommand::Shutdown => {
                                        if cmd_tx_clone.send(tui::TuiCommand::Quit).await.is_err() {
                                            break;
                                        }
                                        continue;
                                    }
                                    web::WebCommand::CancelCleaveChild { label, respond_to } => {
                                        let (control_tx, control_rx) = tokio::sync::oneshot::channel();
                                        if cmd_tx_clone.send(tui::TuiCommand::ExecuteControl {
                                            request: crate::control_runtime::ControlRequest::CleaveCancelChild {
                                                label,
                                            },
                                            respond_to: Some(control_tx),
                                        }).await.is_err() {
                                            break;
                                        }
                                        if let Some(respond_to) = respond_to {
                                            tokio::spawn(async move {
                                                let response = match control_rx.await {
                                                    Ok(output) => omegon_traits::SlashCommandResponse {
                                                        accepted: output.accepted,
                                                        output: output.output,
                                                    },
                                                    Err(_) => omegon_traits::SlashCommandResponse {
                                                        accepted: false,
                                                        output: Some(
                                                            "cleave child cancel executor dropped response before completion"
                                                                .to_string(),
                                                        ),
                                                    },
                                                };
                                                let _ = respond_to.send(response);
                                            });
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

            tui::TuiCommand::RunSlashCommand {
                name,
                args,
                respond_to,
            } => {
                let response = execute_remote_slash_command(
                    &mut runtime_state,
                    &mut agent,
                    &events_tx,
                    &shared_settings,
                    &bridge,
                    &login_prompt_tx,
                    cli,
                    &name,
                    &args,
                )
                .await;
                if let Some(reply) = respond_to {
                    let _ = reply.send(response);
                }
            }

            tui::TuiCommand::BusCommand { name, args } => {
                if name == "context_request" {
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

                    let message = match runtime_state
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
                            let route_snapshot = route_controller.snapshot().await;
                            let message = format!(
                                "{}\n\n{}",
                                control_runtime::format_auth_status(&status),
                                route_snapshot.operator_status()
                            );
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
                            let route_controller_for_login = route_controller.clone();
                            let model_for_redetect = shared_settings
                                .lock()
                                .ok()
                                .map(|s| s.model.clone())
                                .unwrap_or_else(|| cli.model.clone());
                            let cwd_for_profile = agent.cwd.clone();
                            let settings_for_login = shared_settings.clone();
                            let bridge_model_for_login = runtime_resources.bridge_model.clone();
                            crate::task_spawn::spawn_operator_task(
                                "interactive-auth-login",
                                events_tx_clone.clone(),
                                crate::task_spawn::OperatorTaskOptions {
                                    panic_notification_prefix: "⚠ Background login task crashed — authentication did not complete safely".to_string(),
                                },
                                async move {
                                    route_controller_for_login
                                        .begin_login(provider_clone.clone())
                                        .await;
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
                                        "google-antigravity" | "antigravity" => {
                                            auth::login_antigravity_with_callbacks(progress, prompt).await
                                        }
                                        "google" | "gemini" => Err(anyhow::anyhow!(auth::operator_api_key_login_guidance(
                                            "google",
                                            "GOOGLE_API_KEY",
                                            "Google AI Studio"
                                        ))),
                                        "openai" => Err(anyhow::anyhow!(auth::operator_api_key_login_guidance(
                                            "openai",
                                            "OPENAI_API_KEY",
                                            "OpenAI API"
                                        ))),
                                        _ => Err(anyhow::anyhow!(auth::operator_auth_unknown_provider_message(
                                            &provider_clone
                                        ))),
                                    };
                                    let provider_label = crate::auth::provider_by_id(&provider_clone)
                                        .map(|p| p.display_name)
                                        .unwrap_or(provider_clone.as_str())
                                        .to_string();
                                    let message = match &result {
                                        Ok(_) => {
                                            format!("✓ Successfully logged in to {provider_label}")
                                        }
                                        Err(e) => format!("✗ Login failed: {}", e),
                                    };
                                    let _ = events_tx_clone
                                        .send(AgentEvent::SystemNotification { message });

                                    if result.is_ok() {
                                        let login_provider_model =
                                            providers::default_model_for_provider(&provider_clone)
                                                .unwrap_or(model_for_redetect.clone());
                                        let effective_model =
                                            providers::resolve_execution_model_spec(
                                                &login_provider_model,
                                            )
                                            .await
                                            .unwrap_or(login_provider_model);
                                        if let Some(new_bridge) =
                                            providers::auto_detect_bridge(&effective_model).await
                                        {
                                            let _ = route_controller_for_login
                                                .complete_login(
                                                    route::LoginOutcome::Succeeded {
                                                        model: effective_model.clone(),
                                                    },
                                                    Some(new_bridge),
                                                )
                                                .await;
                                            if let Ok(mut s) = settings_for_login.lock() {
                                                s.set_model(&effective_model);
                                                s.provider_connected = auth::provider_connected_for_model(&effective_model);
                                                // set_model aligned operator intent with the new bridge.
                                                let mut profile = settings::Profile::load(&cwd_for_profile);
                                                profile.capture_from(&s);
                                                let _ = profile.save(&cwd_for_profile);
                                            }
                                            if let Ok(mut bridge_model) = bridge_model_for_login.lock() {
                                                *bridge_model = Some(effective_model.clone());
                                            }
                                            tracing::info!("bridge hot-swapped after successful login");
                                            let _ =
                                                events_tx_clone.send(AgentEvent::SystemNotification {
                                                    message: auth::operator_provider_connected_message(&effective_model),
                                                });
                                        } else {
                                            let _ = route_controller_for_login
                                                .complete_login(
                                                    route::LoginOutcome::Failed {
                                                        reason: route::LoginFailureReason::Refused(
                                                            format!(
                                                                "provider bridge could not start for {effective_model}"
                                                            ),
                                                        ),
                                                    },
                                                    None,
                                                )
                                                .await;
                                        }
                                    } else if let Err(e) = &result {
                                        let text = e.to_string();
                                        let reason = if text.contains("timed out") {
                                            route::LoginFailureReason::Timeout
                                        } else if text.contains("stale") || text.contains("state") {
                                            route::LoginFailureReason::StaleStateOnly
                                        } else {
                                            route::LoginFailureReason::Refused(text)
                                        };
                                        let _ = route_controller_for_login
                                            .complete_login(
                                                route::LoginOutcome::Failed { reason },
                                                None,
                                            )
                                            .await;
                                    }

                                    Ok(())
                                },
                            );
                        }
                        "auth_logout" => {
                            let provider = args.trim();
                            if provider.is_empty() {
                                let _ = events_tx.send(AgentEvent::SystemNotification {
                                    message: "Error: Provider required for logout".to_string(),
                                });
                            } else {
                                let message = if provider.is_empty() {
                                    "Error: Provider required for logout".to_string()
                                } else {
                                    match auth::logout_provider(provider) {
                                        Ok(()) => {
                                            auth::clear_provider_auth_env(provider);
                                            auth::operator_logout_success_message(
                                                provider,
                                                !auth::provider_env_vars(provider).is_empty(),
                                            )
                                        }
                                        Err(e) => format!("✗ Logout failed: {}", e),
                                    }
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
                            let result = runtime_state.bus.dispatch_command(&name, &args);
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
                    let result = runtime_state.bus.dispatch_command(&name, &args);
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
                let cmd_requests = runtime_state.bus.drain_requests();
                for request in cmd_requests {
                    match request {
                        omegon_traits::BusRequest::Notify { message, .. } => {
                            let _ = events_tx.send(AgentEvent::SystemNotification { message });
                        }
                        omegon_traits::BusRequest::InjectSystemMessage { content } => {
                            runtime_state.conversation.push_user(format!("[System: {content}]"));
                        }
                        omegon_traits::BusRequest::RequestCompaction
                        | omegon_traits::BusRequest::RequestAggressiveDecay => {
                            tracing::info!("Bus: compaction requested");
                        }
                        omegon_traits::BusRequest::RefreshHarnessStatus => {
                            // Re-assemble and broadcast
                            let mut status = crate::status::HarnessStatus::assemble();
                            status.update_runtime_posture(
                                omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
                                omegon_traits::OmegonAutonomyMode::OperatorDriven,
                            );
                            let auth_status = auth::probe_all_providers().await;
                            status.providers =
                                crate::auth::auth_status_to_provider_statuses(&auth_status);
                            status.annotate_provider_runtime_health();
                            status.update_from_bus(&runtime_state.bus);
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
                            if let Err(e) = runtime_state
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
                        omegon_traits::BusRequest::EmitAgentEvent { event } => {
                            let _ = events_tx.send(*event);
                        }
                    }
                }
            }

            tui::TuiCommand::SubmitPrompt(prompt) => {
                let actor = RuntimeActor {
                    kind: runtime_actor_kind_from_via(prompt.via),
                    label: prompt.submitted_by.clone(),
                };
                let via = control_surface_from_via(prompt.via);

                let prompt_id = runtime.enqueue_prompt(prompt.text, prompt.image_paths, actor, via, prompt.metadata, Some(match prompt.queue_mode {
                        crate::tui::PromptQueueMode::InterruptAfterTurn => QueueMode::InterruptAfterTurn,
                        crate::tui::PromptQueueMode::UntilReady => QueueMode::UntilReady,
                        crate::tui::PromptQueueMode::Immediate => QueueMode::Immediate,
                    }));

                if runtime.is_busy() {
                    emit_runtime_queue_notification(&runtime, &events_tx, prompt_id);
                    continue;
                }

                while let Some(active) = runtime.maybe_start_next_turn() {
                    emit_runtime_queue_snapshot(&runtime, &events_tx);
                    let _ = events_tx.send(AgentEvent::RuntimePromptStarted {
                        text: active.prompt.text.clone(),
                        image_paths: active.prompt.image_paths.clone(),
                    });
                    stop_voice_session_if_requested(&active.prompt, &runtime_state.bus, &events_tx)
                        .await;
                    mark_interactive_session_busy(&agent.dashboard_handles, true);

                    let mut quit_after_turn = false;
                    let state_for_turn = runtime_state;
                    let mut turn_task = tokio::task::spawn_local(run_interactive_active_turn(
                        state_for_turn,
                        runtime_resources.clone(),
                        bridge.clone(),
                        shared_settings.clone(),
                        shared_cancel.clone(),
                        pending_compact.clone(),
                        events_tx.clone(),
                        active,
                    ));

                    loop {
                        tokio::select! {
                            turn_result = &mut turn_task => {
                                runtime_state = match turn_result {
                                    Ok(runtime_state) => runtime_state,
                                    Err(join_err) => {
                                        let message = format_interactive_turn_task_failure(&join_err);
                                        tracing::error!("interactive turn task failed: {join_err}");
                                        mark_interactive_session_busy(&agent.dashboard_handles, false);
                                        let _ = events_tx.send(AgentEvent::SystemNotification {
                                            message: message.clone(),
                                        });
                                        let _ = events_tx.send(AgentEvent::AgentEnd);
                                        return Err(anyhow::anyhow!(message));
                                    }
                                };
                                break;
                            }
                            maybe_cmd = command_rx.recv() => {
                                let Some(cmd) = maybe_cmd else {
                                    quit_after_turn = true;
                                    if let Ok(guard) = shared_cancel.lock()
                                        && let Some(ref cancel) = *guard
                                    {
                                        cancel.cancel();
                                    }
                                    continue;
                                };

                                let cmd = match cmd {
                                    tui::TuiCommand::VoicePrompt { text, metadata } => tui::TuiCommand::SubmitPrompt(tui::PromptSubmission {
                                        text: format!("🎙 {}", text.trim()),
                                        image_paths: Vec::new(),
                                        submitted_by: "voice".to_string(),
                                        via: "voice",
                                        queue_mode: tui::PromptQueueMode::UntilReady,
                                        metadata: tui::PromptMetadata { voice: Some(metadata) },
                                    }),
                                    other => other,
                                };

                                match cmd {
                                    tui::TuiCommand::SubmitPrompt(prompt) => {
                                        let actor = RuntimeActor {
                                            kind: runtime_actor_kind_from_via(prompt.via),
                                            label: prompt.submitted_by.clone(),
                                        };
                                        let via = control_surface_from_via(prompt.via);
                                        let prompt_id = runtime.enqueue_prompt(prompt.text, prompt.image_paths, actor, via, prompt.metadata, Some(match prompt.queue_mode {
                                            crate::tui::PromptQueueMode::InterruptAfterTurn => QueueMode::InterruptAfterTurn,
                                            crate::tui::PromptQueueMode::UntilReady => QueueMode::UntilReady,
                                            crate::tui::PromptQueueMode::Immediate => QueueMode::Immediate,
                                        }));
                                        emit_runtime_queue_notification(&runtime, &events_tx, prompt_id);
                                        if let Some(queued_prompt) = runtime.queue.iter().find(|queued| queued.id == prompt_id)
                                            && queued_prompt.requests_voice_close()
                                        {
                                            let _ = events_tx.send(AgentEvent::SystemNotification {
                                                message: "Voice requested shutdown after this prompt; it will be stopped when the active turn completes.".to_string(),
                                            });
                                        }
                                    }
                                    tui::TuiCommand::CancelActiveTurn { submitted_by, via } => {
                                        handle_runtime_cancel_command(
                                            &mut runtime,
                                            &shared_cancel,
                                            &events_tx,
                                            submitted_by,
                                            via,
                                        );
                                    }
                                    tui::TuiCommand::Quit => {
                                        quit_after_turn = true;
                                        if let Ok(guard) = shared_cancel.lock()
                                            && let Some(ref cancel) = *guard
                                        {
                                            cancel.cancel();
                                        }
                                    }
                                    other => deferred_commands.push_back(other),
                                }
                            }
                        }
                    }

                    runtime.complete_active_turn();
                    emit_runtime_queue_snapshot(&runtime, &events_tx);
                    mark_interactive_session_busy(&agent.dashboard_handles, runtime.is_busy());

                    if quit_after_turn {
                        break 'interactive;
                    }
                }
            }
            tui::TuiCommand::CancelActiveTurn { submitted_by, via } => {
                handle_runtime_cancel_command(
                    &mut runtime,
                    &shared_cancel,
                    &events_tx,
                    submitted_by,
                    via,
                );
            }
            tui::TuiCommand::VoicePrompt { .. } => unreachable!("VoicePrompt is normalized above"),
        }
    }

    // Save session + profile
    if !cli.no_session {
        match session::save_session(
            &runtime_state.conversation,
            &agent.cwd,
            Some(agent.session_id.as_str()),
        ) {
            Ok(path) => {
                eprintln!(
                    "Session saved: {}\nResume this session with: omegon --resume {}\nOr from inside Omegon: /resume {}",
                    agent.session_id,
                    agent.session_id,
                    agent.session_id
                );
                tracing::debug!(path = %path.display(), session_id = %agent.session_id, "interactive session saved on exit");
            }
            Err(e) => {
                tracing::debug!("Session save failed: {e}");
            }
        }
    }
    // Always persist profile on exit (captures thinking level changes, etc.)
    if let Ok(s) = shared_settings.lock() {
        let mut profile = settings::Profile::load(&agent.cwd);
        profile.capture_from(&s);
        let _ = profile.save(&agent.cwd);
    }

    // Auto-update: if enabled and a cached update check found a newer
    // version during this session, download and replace the binary before
    // exit. The next launch uses the new version automatically.
    if shared_settings.lock().ok().is_some_and(|s| s.auto_update) {
        let channel = shared_settings
            .lock()
            .ok()
            .map(|s| s.update_channel.clone())
            .unwrap_or_else(|| "stable".to_string());
        let channel = crate::update::UpdateChannel::parse(&channel)
            .unwrap_or(crate::update::UpdateChannel::Stable);
        if let Some(info) = crate::update::read_cache(channel) {
            eprintln!(
                "Auto-updating: v{} → v{} ...",
                info.current, info.latest
            );
            match crate::update::download_and_replace(&info).await {
                Ok(path) => {
                    eprintln!("✓ Updated to v{}. Next launch uses the new version.", info.latest);
                    tracing::info!(path = %path.display(), "auto-update installed");
                }
                Err(e) => {
                    eprintln!("Auto-update failed (non-fatal): {e}");
                    tracing::warn!("auto-update failed: {e}");
                }
            }
        }
    }

    bridge.read().await.shutdown().await;
    tui_handle.abort();
    Ok(())
        })
        .await
}

fn mark_interactive_session_busy(handles: &crate::tui::dashboard::DashboardHandles, busy: bool) {
    if let Ok(mut ss) = handles.session.lock() {
        ss.busy = busy;
    }
}

fn format_interactive_turn_task_failure(join_err: &tokio::task::JoinError) -> String {
    format!("⚠ Interactive turn worker crashed — ending session safely: {join_err}")
}

/// Format an agent loop error into a concise user-facing message.
/// Extracts the meaningful part from API error JSON blobs.
fn format_agent_error(
    e: &anyhow::Error,
    recent_telemetry: Option<&omegon_traits::ProviderTelemetrySnapshot>,
) -> String {
    let raw = format!("{e}");
    let provider = provider_label_from_error(&raw);
    let provider_id = provider.as_deref().unwrap_or("upstream");
    let upstream_class =
        crate::upstream_errors::classify_upstream_error_for_provider(provider_id, &raw);
    let who = provider_display_name(provider_id);

    if provider_id == "anthropic" {
        let headroom = crate::usage::derive_headroom_state(recent_telemetry);
        if matches!(
            upstream_class,
            crate::upstream_errors::UpstreamErrorClass::StalledStream
        ) && matches!(
            headroom,
            crate::usage::UsageHeadroomState::Constrained
                | crate::usage::UsageHeadroomState::Exhausted
        ) {
            let rationale = crate::usage::derive_rationale(recent_telemetry, &headroom);
            return format!(
                "⚠ Provider pressure (Anthropic/Claude) — the stream stopped after recent quota telemetry showed {state} headroom. This is likely usage-window backpressure or exhaustion, not a generic transport stall. Wait for reset or switch provider with /model. ({rationale})",
                state = headroom.as_str(),
            );
        }
        if matches!(
            upstream_class,
            crate::upstream_errors::UpstreamErrorClass::QuotaExceeded
        ) {
            let rationale = crate::usage::derive_rationale(recent_telemetry, &headroom);
            return format!(
                "⚠ Usage limit reached (Anthropic/Claude) — recent upstream telemetry indicates {state} headroom. Wait for the Anthropic usage window to reset or switch provider with /model. ({rationale})",
                state = headroom.as_str(),
            );
        }
        if matches!(
            upstream_class,
            crate::upstream_errors::UpstreamErrorClass::RateLimited
        ) && matches!(
            headroom,
            crate::usage::UsageHeadroomState::Constrained
                | crate::usage::UsageHeadroomState::Exhausted
        ) {
            let rationale = crate::usage::derive_rationale(recent_telemetry, &headroom);
            return format!(
                "⚠ Rate limit / usage pressure (Anthropic/Claude) — recent upstream telemetry indicates {state} headroom. Anthropic is likely throttling or exhausting this session's usage window. Wait for reset or switch provider with /model. ({rationale})",
                state = headroom.as_str(),
            );
        }
    }

    match upstream_class {
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
        | crate::upstream_errors::UpstreamErrorClass::ResponseCancelled => {
            let status_hint = provider_status_hint(provider_id);
            return format!(
                "⚠ Upstream error ({who}) — provider-side failure. Retry later or check {status_hint}."
            );
        }
        crate::upstream_errors::UpstreamErrorClass::SessionExpired => {
            return format!(
                "⚠ Authentication error ({who}) — your session appears expired or no longer valid. Re-authenticate and retry."
            );
        }
        crate::upstream_errors::UpstreamErrorClass::AuthInvalid => {
            // Always log the raw error for diagnostics
            tracing::warn!(provider = %who, raw = %raw, "AuthInvalid — raw upstream response");

            // Codex scope errors: provide targeted guidance
            if raw.contains("api.responses.write") || raw.contains("insufficient permissions") {
                return format!(
                    "⚠ Authentication error ({who}) — your session may have expired or \
                     lacks required permissions. Re-authenticate with /login and retry."
                );
            }

            if let Some(start) = raw.find("\"message\":\"") {
                let rest = &raw[start + 11..];
                if let Some(end) = rest.find('"') {
                    return format!("⚠ Authentication error ({who}) — {}", &rest[..end]);
                }
            }
            // Include truncated raw error so the user can report it
            let truncated = crate::util::truncate_str(&raw, 200);
            return format!(
                "⚠ Authentication error ({who}) — credentials were rejected.\n\
                 Raw: {truncated}\n\
                 Re-authenticate with /login or check your API key."
            );
        }
        _ => {}
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
            return format!("⚠ {}", crate::util::truncate_str(&rest[..end], 40));
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
    fn display_label(&self) -> &str {
        if self.label.is_empty() {
            match self.kind {
                RuntimeActorKind::Tui => "tui",
                RuntimeActorKind::Auspex => "auspex",
                RuntimeActorKind::IpcClient => "ipc-client",
                RuntimeActorKind::WebClient => "web-client",
                RuntimeActorKind::DaemonEvent => "daemon-event",
                RuntimeActorKind::System => "system",
            }
        } else {
            &self.label
        }
    }

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

impl ControlSurface {
    fn label(&self) -> &'static str {
        match self {
            ControlSurface::Tui => "tui",
            ControlSurface::Ipc => "ipc",
            ControlSurface::WebSocket => "websocket",
            ControlSurface::HttpEventIngress => "http-event-ingress",
            ControlSurface::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum QueueMode {
    InterruptAfterTurn,
    #[default]
    UntilReady,
    Immediate,
}

#[derive(Debug, Clone, PartialEq)]
struct PromptEnvelope {
    id: u64,
    text: String,
    image_paths: Vec<PathBuf>,
    submitted_by: RuntimeActor,
    via: ControlSurface,
    metadata: tui::PromptMetadata,
    queue_mode: QueueMode,
}

impl PromptEnvelope {
    fn requests_voice_close(&self) -> bool {
        self.metadata.voice.as_ref().is_some_and(|voice| {
            voice.close_session_requested == Some(true)
                && voice.radio_cue.as_deref() == Some("over_and_out")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveTurnPhase {
    Running,
    Cancelling {
        requested_by: RuntimeActor,
        via: ControlSurface,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveTurnMeta {
    runtime_turn_id: u64,
    prompt: PromptEnvelope,
    phase: ActiveTurnPhase,
}

fn runtime_actor_kind_from_via(via: &str) -> RuntimeActorKind {
    match via {
        "tui" => RuntimeActorKind::Tui,
        "ipc" => RuntimeActorKind::IpcClient,
        "websocket" => RuntimeActorKind::WebClient,
        _ => RuntimeActorKind::System,
    }
}

fn control_surface_from_via(via: &str) -> ControlSurface {
    match via {
        "tui" => ControlSurface::Tui,
        "ipc" => ControlSurface::Ipc,
        "websocket" => ControlSurface::WebSocket,
        _ => ControlSurface::Internal,
    }
}

fn handle_runtime_cancel_command(
    runtime: &mut InteractiveRuntimeSupervisor,
    shared_cancel: &tui::SharedCancel,
    events_tx: &broadcast::Sender<AgentEvent>,
    submitted_by: String,
    via: &'static str,
) {
    let actor = RuntimeActor {
        kind: runtime_actor_kind_from_via(via),
        label: submitted_by,
    };
    let surface = control_surface_from_via(via);
    let active = runtime.request_cancel(actor, surface);
    if active.is_none() {
        let _ = events_tx.send(AgentEvent::SystemNotification {
            message: "Cancel requested, but no active turn is running.".to_string(),
        });
    }
    if let Ok(guard) = shared_cancel.lock()
        && let Some(ref cancel) = *guard
    {
        cancel.cancel();
    }
}

fn emit_runtime_queue_notification(
    runtime: &InteractiveRuntimeSupervisor,
    events_tx: &broadcast::Sender<AgentEvent>,
    prompt_id: u64,
) {
    if let Some(prompt) = runtime.queue.iter().find(|prompt| prompt.id == prompt_id) {
        emit_runtime_queue_snapshot(runtime, events_tx);
        let _ = events_tx.send(AgentEvent::SystemNotification {
            message: format!(
                "Queued prompt #{} from {} via {}; queue depth {}.",
                prompt.id,
                prompt.submitted_by.display_label(),
                prompt.via.label(),
                runtime.queue_depth()
            ),
        });
    }
}

fn emit_runtime_queue_snapshot(
    runtime: &InteractiveRuntimeSupervisor,
    events_tx: &broadcast::Sender<AgentEvent>,
) {
    let snapshot_json = runtime.queue_snapshot_json();
    let _ = events_tx.send(AgentEvent::RuntimeQueueUpdated { snapshot_json });
}

pub(crate) struct InteractiveAgentState {
    pub(crate) bus: crate::bus::EventBus,
    pub(crate) context_manager: crate::context::ContextManager,
    pub(crate) conversation: crate::conversation::ConversationState,
}

pub(crate) struct InteractiveAgentHost {
    pub(crate) session_id: String,
    pub(crate) instance_id: String,
    pub(crate) context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    pub(crate) cwd: PathBuf,
    pub(crate) secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    pub(crate) web_auth_state: crate::web::WebAuthState,
    pub(crate) dashboard_handles: crate::tui::dashboard::DashboardHandles,
    pub(crate) resume_info: Option<setup::ResumeInfo>,
    pub(crate) workspace_state: setup::WorkspaceStartupState,
    pub(crate) runtime_generation: u64,
}

pub(crate) struct CliRuntimeView<'a> {
    pub(crate) no_session: bool,
    pub(crate) model: &'a str,
    pub(crate) dangerously_bypass_permissions: bool,
}

fn interactive_resume_mode(cli: &Cli) -> Option<Option<&str>> {
    if cli.fresh {
        None
    } else {
        cli.resume.as_ref().map(|r| r.as_deref())
    }
}

fn split_interactive_agent(
    agent: setup::AgentSetup,
) -> (InteractiveAgentHost, InteractiveAgentState) {
    let host = InteractiveAgentHost {
        session_id: agent.session_id,
        instance_id: agent.instance_id,
        context_metrics: agent.context_metrics,
        cwd: agent.cwd,
        secrets: agent.secrets,
        web_auth_state: agent.web_auth_state,
        dashboard_handles: agent.dashboard_handles,
        resume_info: agent.resume_info,
        workspace_state: agent.workspace_state,
        runtime_generation: 1,
    };
    let state = InteractiveAgentState {
        bus: agent.bus,
        context_manager: agent.context_manager,
        conversation: agent.conversation,
    };
    (host, state)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InteractiveStartupModelDecision {
    selected_model: String,
    bridge_model: String,
    provider_connected: bool,
    use_null_bridge: bool,
}

fn decide_interactive_startup_model(
    selected_model: &str,
    resolved_model: &str,
    resolved_available: bool,
) -> InteractiveStartupModelDecision {
    InteractiveStartupModelDecision {
        selected_model: selected_model.to_string(),
        bridge_model: resolved_model.to_string(),
        provider_connected: resolved_available,
        use_null_bridge: !resolved_available,
    }
}

#[derive(Clone)]
struct InteractiveRuntimeResources {
    cwd: PathBuf,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    bridge_model: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    route_controller: Arc<route::RouteController>,
}

fn build_interactive_loop_config(
    runtime: &InteractiveRuntimeResources,
    shared_settings: &Arc<std::sync::Mutex<settings::Settings>>,
    pending_compact: &Arc<std::sync::atomic::AtomicBool>,
) -> r#loop::LoopConfig {
    let model = shared_settings
        .lock()
        .map(|s| s.model.clone())
        .unwrap_or_default();

    let ollama_manager = if providers::infer_provider_id(&model) == "ollama" {
        Some(ollama::OllamaManager::new())
    } else {
        None
    };

    bootstrap::build_loop_config(
        shared_settings,
        &runtime.cwd,
        &model,
        bootstrap::LoopConfigOverrides {
            secrets: Some(runtime.secrets.clone()),
            force_compact: Some(pending_compact.clone()),
            allow_commit_nudge: true,
            ollama_manager,
            bridge_model: runtime
                .bridge_model
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            route_controller: Some(runtime.route_controller.clone()),
            ..Default::default()
        },
    )
}

#[allow(clippy::too_many_arguments)]
async fn run_interactive_active_turn(
    mut runtime_state: InteractiveAgentState,
    runtime: InteractiveRuntimeResources,
    bridge: Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    shared_settings: Arc<std::sync::Mutex<settings::Settings>>,
    shared_cancel: tui::SharedCancel,
    pending_compact: Arc<std::sync::atomic::AtomicBool>,
    events_tx: broadcast::Sender<AgentEvent>,
    active: ActiveTurnMeta,
) -> InteractiveAgentState {
    let cancel_keeps_prompt = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut loop_config =
        build_interactive_loop_config(&runtime, &shared_settings, &pending_compact);
    loop_config.cancel_keeps_prompt = Some(cancel_keeps_prompt.clone());

    if active.prompt.image_paths.is_empty() {
        runtime_state
            .conversation
            .push_user(active.prompt.text.clone());
    } else {
        let mut images = Vec::new();
        for path in &active.prompt.image_paths {
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
        runtime_state
            .conversation
            .push_user_with_images(active.prompt.text.clone(), images);
    }

    let cancel = CancellationToken::new();
    if let Ok(mut guard) = shared_cancel.lock() {
        *guard = Some(cancel.clone());
    }

    let run_result = {
        let bridge_guard = bridge.read().await;
        let mut run = std::pin::pin!(r#loop::run(
            bridge_guard.as_ref(),
            &mut runtime_state.bus,
            &mut runtime_state.context_manager,
            &mut runtime_state.conversation,
            &events_tx,
            cancel.clone(),
            &loop_config,
        ));

        tokio::select! {
            result = &mut run => Some(result),
            _ = cancel.cancelled() => {
                let keep_prompt = cancel_keeps_prompt.load(std::sync::atomic::Ordering::Relaxed);
                let disposition = if keep_prompt { "interrupted · kept" } else { "aborted · forgotten" };
                tracing::warn!(
                    runtime_turn_id = active.runtime_turn_id,
                    "operator cancellation requested; abandoning active turn to recover operator surface"
                );
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: format!("Interrupt requested — recovered the operator surface ({disposition}). The abandoned provider/tool request may finish in the background."),
                });
                let _ = events_tx.send(AgentEvent::AgentEnd);
                None
            }
        }
    };

    if (matches!(run_result, Some(Ok(_))) || run_result.is_none()) && cancel.is_cancelled() {
        let keep_prompt = cancel_keeps_prompt.load(std::sync::atomic::Ordering::Relaxed);
        if !keep_prompt {
            runtime_state
                .conversation
                .rollback_last_user_if_text(&active.prompt.text);
        }
        let disposition = if keep_prompt {
            "interrupted · kept"
        } else {
            "aborted · forgotten"
        };
        let _ = events_tx.send(AgentEvent::MessageAbort {
            reason: Some(disposition.to_string()),
        });
    }

    if let Some(Err(e)) = run_result {
        let recent_telemetry = runtime_state.conversation.last_provider_telemetry(None);
        let user_msg = format_agent_error(&e, recent_telemetry.as_ref());
        tracing::error!(
            runtime_turn_id = active.runtime_turn_id,
            "Agent loop error: {e}"
        );
        runtime_state
            .conversation
            .rollback_last_user_if_text(&active.prompt.text);
        let _ = events_tx.send(AgentEvent::SystemNotification { message: user_msg });
        let _ = events_tx.send(AgentEvent::AgentEnd);
    }

    if let Ok(mut guard) = shared_cancel.lock() {
        guard.take();
    }

    let est = runtime_state.conversation.estimate_tokens();
    let settings = shared_settings.lock().unwrap();
    if let Ok(mut metrics) = runtime.context_metrics.lock() {
        metrics.update(
            est,
            settings.context_window,
            settings.effective_requested_class().label(),
            settings.thinking.as_str(),
        );
    }
    let _ = events_tx.send(AgentEvent::ContextUpdated {
        tokens: est as u64,
        context_window: settings.context_window as u64,
        context_class: settings.effective_requested_class().label().to_string(),
        thinking_level: settings.thinking.as_str().to_string(),
    });

    runtime_state
}

async fn stop_voice_session_if_requested(
    prompt: &PromptEnvelope,
    bus: &crate::bus::EventBus,
    events_tx: &tokio::sync::broadcast::Sender<AgentEvent>,
) {
    if !prompt.requests_voice_close() {
        return;
    }

    if !bus.has_tool("voice_session_stop") {
        let _ = events_tx.send(AgentEvent::SystemNotification {
            message: "Voice requested shutdown after this prompt, but no voice_session_stop tool is available.".to_string(),
        });
        return;
    }

    match bus
        .execute_tool(
            "voice_session_stop",
            "voice-over-and-out-stop",
            serde_json::json!({}),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
    {
        Ok(_) => {
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: "Voice session stop requested after over and out.".to_string(),
            });
        }
        Err(err) => {
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: format!("Voice requested shutdown, but voice_session_stop failed: {err}"),
            });
        }
    }
}

#[derive(Debug, Default)]
struct InteractiveRuntimeSupervisor {
    queue: VecDeque<PromptEnvelope>,
    active_turn: Option<ActiveTurnMeta>,
    next_prompt_id: u64,
    next_runtime_turn_id: u64,
    default_queue_mode: QueueMode,
}

impl InteractiveRuntimeSupervisor {
    fn enqueue_prompt(
        &mut self,
        text: String,
        image_paths: Vec<PathBuf>,
        actor: RuntimeActor,
        via: ControlSurface,
        metadata: tui::PromptMetadata,
        queue_mode: Option<QueueMode>,
    ) -> u64 {
        self.next_prompt_id += 1;
        let prompt_id = self.next_prompt_id;
        self.queue.push_back(PromptEnvelope {
            id: prompt_id,
            text,
            image_paths,
            submitted_by: actor,
            via,
            metadata,
            queue_mode: queue_mode.unwrap_or(self.default_queue_mode),
        });
        prompt_id
    }

    fn queue_depth(&self) -> usize {
        self.queue.len()
    }

    fn queue_preview(&self) -> Vec<String> {
        self.queue
            .iter()
            .map(|prompt| {
                let attachment_summary = if prompt.image_paths.is_empty() {
                    String::new()
                } else {
                    let names = prompt
                        .image_paths
                        .iter()
                        .take(3)
                        .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
                        .collect::<Vec<_>>();
                    let suffix = if prompt.image_paths.len() > names.len() {
                        format!(" +{} more", prompt.image_paths.len() - names.len())
                    } else {
                        String::new()
                    };
                    format!(" [{}{}]", names.join(", "), suffix)
                };
                let preview = prompt.text.chars().take(48).collect::<String>();
                let mode = match prompt.queue_mode {
                    QueueMode::InterruptAfterTurn => "after-turn",
                    QueueMode::UntilReady => "ready",
                    QueueMode::Immediate => "now",
                };
                format!("#{} {mode}: {}{}", prompt.id, preview, attachment_summary)
            })
            .collect()
    }

    fn queue_snapshot_json(&self) -> serde_json::Value {
        serde_json::json!({
            "depth": self.queue_depth(),
            "active": self.active_turn.as_ref().map(|active| serde_json::json!({
                "turn_id": active.runtime_turn_id,
                "prompt_id": active.prompt.id,
                "submitted_by": active.prompt.submitted_by.display_label(),
                "via": active.prompt.via.label(),
                "phase": match &active.phase {
                    ActiveTurnPhase::Running => "running",
                    ActiveTurnPhase::Cancelling { .. } => "cancelling",
                },
            })),
            "items": self.queue.iter().map(|prompt| serde_json::json!({
                "id": prompt.id,
                "submitted_by": prompt.submitted_by.display_label(),
                "via": prompt.via.label(),
                "queue_mode": match prompt.queue_mode {
                    QueueMode::InterruptAfterTurn => "interrupt_after_turn",
                    QueueMode::UntilReady => "until_ready",
                    QueueMode::Immediate => "immediate",
                },
                "preview": prompt.text.chars().take(80).collect::<String>(),
                "attachments": prompt.image_paths.len(),
                "voice": prompt.metadata.voice.is_some(),
            })).collect::<Vec<_>>(),
            "previews": self.queue_preview(),
        })
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

    fn pop_front_prompt(&mut self) -> Option<PromptEnvelope> {
        self.queue.pop_front()
    }

    fn push_front_prompt(&mut self, prompt: PromptEnvelope) {
        self.queue.push_front(prompt);
    }

    fn clear_queue(&mut self) {
        self.queue.clear();
    }
}

async fn run_smoke_command(cli: &Cli) -> anyhow::Result<()> {
    eprintln!("omegon {} — smoke test mode", env!("CARGO_PKG_VERSION"));

    let bridge = bootstrap::resolve_bridge_or_bail(&cli.model).await?;
    let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(bridge));

    let exit_code = smoke::run(bridge).await;
    std::process::exit(exit_code);
}

/// Per-turn snapshot for detailed efficiency analysis.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
struct PerTurnSnapshot {
    turn: u32,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    estimated_tokens: usize,
    context_composition: omegon_traits::ContextComposition,
    turn_end_reason: String,
    dominant_phase: Option<String>,
    drift_kind: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
struct BenchmarkUsageSummary {
    requested_model: Option<String>,
    requested_provider: Option<String>,
    resolved_provider: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    dominant_phases: std::collections::BTreeMap<String, u32>,
    drift_kinds: std::collections::BTreeMap<String, u32>,
    progress_nudge_reasons: std::collections::BTreeMap<String, u32>,
    turn_count: u32,
    turn_end_reasons: std::collections::BTreeMap<String, u32>,
    input_tokens: u64,
    output_tokens: u64,
    cache_tokens: u64,
    cache_write_tokens: u64,
    estimated_tokens: usize,
    context_window: usize,
    context_composition: omegon_traits::ContextComposition,
    provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    /// Per-turn token and context snapshots for efficiency analysis.
    turns: Vec<PerTurnSnapshot>,
}

impl BenchmarkUsageSummary {
    fn avg_u64(total: u64, turns: u32) -> u64 {
        if turns == 0 { 0 } else { total / turns as u64 }
    }

    fn avg_usize(total: usize, turns: u32) -> usize {
        if turns == 0 {
            0
        } else {
            total / turns as usize
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn observe_turn(
        &mut self,
        model: Option<String>,
        provider: Option<String>,
        turn_end_reason: omegon_traits::TurnEndReason,
        estimated_tokens: usize,
        dominant_phase: Option<omegon_traits::OodaPhase>,
        drift_kind: Option<omegon_traits::DriftKind>,
        progress_nudge_reason: Option<omegon_traits::ProgressNudgeReason>,
        context_window: usize,
        context_composition: omegon_traits::ContextComposition,
        actual_input_tokens: u64,
        actual_output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
        provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    ) {
        self.model = model;
        self.provider = provider;
        self.turn_count = self.turn_count.saturating_add(1);
        let reason_key = serde_json::to_value(turn_end_reason)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("{:?}", turn_end_reason));
        *self.turn_end_reasons.entry(reason_key.clone()).or_insert(0) += 1;
        if let Some(phase) = dominant_phase {
            let key = serde_json::to_value(phase)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{:?}", phase));
            *self.dominant_phases.entry(key).or_insert(0) += 1;
        }
        if let Some(drift) = drift_kind {
            let key = serde_json::to_value(drift)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{:?}", drift));
            *self.drift_kinds.entry(key).or_insert(0) += 1;
        }
        if let Some(reason) = progress_nudge_reason {
            let key = serde_json::to_value(reason)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{:?}", reason));
            *self.progress_nudge_reasons.entry(key).or_insert(0) += 1;
        }
        self.input_tokens = self.input_tokens.saturating_add(actual_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(actual_output_tokens);
        self.cache_tokens = self.cache_tokens.saturating_add(cache_read_tokens);
        self.cache_write_tokens = self.cache_write_tokens.saturating_add(cache_write_tokens);
        self.estimated_tokens = self.estimated_tokens.saturating_add(estimated_tokens);
        self.context_window = context_window;
        if has_nonempty_context_snapshot(&context_composition) {
            self.context_composition = context_composition.clone();
        }
        self.provider_telemetry = provider_telemetry;

        // Capture per-turn snapshot for efficiency analysis
        self.turns.push(PerTurnSnapshot {
            turn: self.turn_count,
            input_tokens: actual_input_tokens,
            output_tokens: actual_output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            estimated_tokens,
            context_composition,
            turn_end_reason: reason_key,
            dominant_phase: dominant_phase.and_then(|p| {
                serde_json::to_value(p)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
            }),
            drift_kind: drift_kind.and_then(|d| {
                serde_json::to_value(d)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
            }),
        });
    }
}

fn has_nonempty_context_snapshot(context: &omegon_traits::ContextComposition) -> bool {
    context.system_tokens > 0
        || context.tool_schema_tokens > 0
        || context.conversation_tokens > 0
        || context.memory_tokens > 0
        || context.tool_history_tokens > 0
        || context.thinking_tokens > 0
}

fn write_benchmark_usage_json(
    path: &Path,
    summary: &BenchmarkUsageSummary,
    status: &str,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let payload = serde_json::json!({
        "status": status,
        "last_completed_turn": summary.turn_count,
        "requested_model": summary.requested_model,
        "requested_provider": summary.requested_provider,
        "resolved_provider": summary.resolved_provider,
        "model": summary.model,
        "provider": summary.provider,
        "turn_count": summary.turn_count,
        "turn_end_reasons": summary.turn_end_reasons,
        "dominant_phases": summary.dominant_phases,
        "drift_kinds": summary.drift_kinds,
        "progress_nudge_reasons": summary.progress_nudge_reasons,
        "input_tokens": summary.input_tokens,
        "output_tokens": summary.output_tokens,
        "cache_tokens": summary.cache_tokens,
        "cache_write_tokens": summary.cache_write_tokens,
        "estimated_tokens": summary.estimated_tokens,
        "context_window": summary.context_window,
        "context_composition": summary.context_composition,
        "provider_telemetry": summary.provider_telemetry,
        "per_turn": {
            "avg_input_tokens": BenchmarkUsageSummary::avg_u64(summary.input_tokens, summary.turn_count),
            "avg_output_tokens": BenchmarkUsageSummary::avg_u64(summary.output_tokens, summary.turn_count),
            "avg_cache_tokens": BenchmarkUsageSummary::avg_u64(summary.cache_tokens, summary.turn_count),
            "avg_cache_write_tokens": BenchmarkUsageSummary::avg_u64(summary.cache_write_tokens, summary.turn_count),
            "avg_estimated_tokens": BenchmarkUsageSummary::avg_usize(summary.estimated_tokens, summary.turn_count),
        },
        "turns": summary.turns
    });
    std::fs::write(path, serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

async fn run_agent_command(cli: &Cli, usage_json: Option<PathBuf>) -> anyhow::Result<()> {
    tracing::info!(model = %cli.model, "omegon starting");

    let requested_model = cli.model.clone();
    let requested_provider = providers::infer_provider_id(&requested_model);

    if maybe_run_injected_cleave_smoke_child(&cli.cwd, &cli.model).await? {
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
            eprintln!("Usage: omegon --prompt \"<task>\" [--cwd <path>]");
            eprintln!("       omegon --prompt-file <path> [--cwd <path>]");
            eprintln!(
                "       omegon cleave --plan <plan.json> --directive \"<task>\" --workspace <dir>"
            );
            eprintln!();
            eprintln!("Headless coding agent — executes a task and exits.");
            std::process::exit(1);
        }
    };

    let shared_settings = bootstrap::initialize_shared_settings(&bootstrap::SettingsInit {
        model: &cli.model,
        cwd: &cli.cwd,
        cli_posture: resolve_cli_posture(cli).as_deref(),
        slim: cli_prefers_slim_mode(cli),
        full: cli.full,
        max_turns: cli.max_turns,
        apply_profile_posture: true,
    });
    // Headless: force CLI model even if profile tried to override it.
    if let Ok(mut s) = shared_settings.lock() {
        s.set_model(&cli.model);
    }

    if let Some(ref persona) = cli.persona {
        // SAFETY: called before spawning any threads that read this var.
        unsafe { std::env::set_var("OMEGON_CHILD_PERSONA", persona) };
    }

    let resume = cli.resume.as_ref().map(|r| r.as_deref());
    let mut agent = setup::AgentSetup::new_with_safety(
        &cli.cwd,
        resume,
        Some(shared_settings.clone()),
        cli.dangerously_bypass_permissions,
    )
    .await?;
    agent.instance_id = paths::instance_id("run");
    bootstrap::apply_runtime_posture(
        &mut agent,
        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
        omegon_traits::OmegonAutonomyMode::OperatorDriven,
    );
    agent.conversation.push_user(prompt_text.clone());

    let loop_config = bootstrap::build_loop_config(
        &shared_settings,
        &agent.cwd,
        &cli.model,
        bootstrap::LoopConfigOverrides {
            max_retries: cli.max_retries,
            secrets: Some(agent.secrets.clone()),
            enforce_first_turn_execution_bias: true,
            ..Default::default()
        },
    );

    let resolved_provider = providers::resolve_execution_provider(&cli.model).await;
    tracing::info!(
        requested_model = %requested_model,
        requested_provider = %requested_provider,
        resolved_provider = resolved_provider.as_deref().unwrap_or("none"),
        "bench/headless provider resolution"
    );
    if requested_provider == "anthropic"
        && resolved_provider
            .as_deref()
            .is_some_and(|provider| provider != "anthropic")
    {
        anyhow::bail!(
            "provider resolution invariant violated: requested anthropic model '{requested_model}' resolved to '{}'; this should never fall through to another provider",
            resolved_provider.as_deref().unwrap_or("none")
        );
    }
    let bridge = bootstrap::resolve_bridge_or_bail(&cli.model).await?;

    let (events_tx, mut events_rx) = bootstrap::wire_event_channel(&agent, 256);

    let benchmark_summary = std::sync::Arc::new(std::sync::Mutex::new(BenchmarkUsageSummary {
        requested_model: Some(requested_model.clone()),
        requested_provider: Some(requested_provider.clone()),
        resolved_provider: resolved_provider.clone(),
        ..BenchmarkUsageSummary::default()
    }));
    let benchmark_summary_task = std::sync::Arc::clone(&benchmark_summary);
    let usage_json_task = usage_json.clone();

    let event_task = tokio::spawn(async move {
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
                                    crate::util::truncate(text, 200)
                                } else {
                                    text.clone()
                                }
                            }
                            omegon_traits::ContentBlock::Image { .. } => "[image]".into(),
                        })
                        .unwrap_or_default();
                    tracing::info!("  {status} {text}");
                }
                AgentEvent::TurnEnd(te) => {
                    if let Ok(mut summary) = benchmark_summary_task.lock() {
                        summary.observe_turn(
                            te.model,
                            te.provider,
                            te.turn_end_reason,
                            te.estimated_tokens,
                            te.dominant_phase,
                            te.drift_kind,
                            te.progress_nudge_reason,
                            te.context_window,
                            te.context_composition,
                            te.actual_input_tokens,
                            te.actual_output_tokens,
                            te.cache_read_tokens,
                            te.cache_creation_tokens,
                            te.provider_telemetry,
                        );
                        if let Some(path) = usage_json_task.as_ref()
                            && let Err(err) =
                                write_benchmark_usage_json(path, &summary, "in_progress")
                        {
                            tracing::warn!(path = %path.display(), error = %err, "failed to checkpoint benchmark usage json at turn boundary");
                        }
                    }
                    if te.actual_input_tokens > 0 || te.actual_output_tokens > 0 {
                        tracing::info!(
                            "── Turn {} complete — in:{} out:{} ──",
                            te.turn,
                            te.actual_input_tokens,
                            te.actual_output_tokens
                        );
                    } else {
                        tracing::info!("── Turn {} complete ──", te.turn);
                    }
                    // Emit explicit task-done marker so the parent can
                    // track per-task progress without relying solely on
                    // the turn-based heuristic.
                    if te.turn > 0 {
                        tracing::info!("TASK_DONE: {}", te.turn);
                    }
                }
                AgentEvent::AgentEnd => {
                    tracing::info!("Agent complete");
                }
                _ => {}
            }
        }
    });

    let _checkpoint_task = checkpoint::spawn_checkpoint_subscriber(
        &events_tx,
        agent.session_id.clone(),
        agent.context_metrics.clone(),
    );

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
                Ok(path) => {
                    let session_id = agent
                        .resume_info
                        .as_ref()
                        .map(|r| r.session_id.as_str())
                        .unwrap_or_else(|| {
                            path.file_stem()
                                .and_then(|stem| stem.to_str())
                                .unwrap_or("latest")
                        });
                    eprintln!(
                        "Session saved: {session_id}\nResume this session with: omegon --resume {session_id}\nOr from inside Omegon: /resume {session_id}"
                    );
                    tracing::info!(path = %path.display(), session_id, "Session saved");
                }
                Err(e) => tracing::debug!("Session save failed (non-fatal): {e}"),
            }
        }
    }

    // Graceful bridge shutdown.
    //
    // In headless benchmark mode the agent task may finish while other
    // components still hold cloned AgentEvent senders. Waiting forever for the
    // stderr event-printer task to observe channel closure can therefore hang
    // process exit even though the task already succeeded. That in turn blocks
    // usage JSON emission and breaks benchmark artifact finalization.
    bridge.shutdown().await;

    if let Some(path) = usage_json.as_ref() {
        let summary = benchmark_summary
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        write_benchmark_usage_json(path, &summary, "completed")?;
    }

    drop(events_tx);
    match tokio::time::timeout(std::time::Duration::from_millis(250), event_task).await {
        Ok(_) => {}
        Err(_) => {
            tracing::warn!(
                "headless benchmark event-printer task did not drain before shutdown; aborting it"
            );
        }
    }

    match &result {
        Ok(()) => {
            if let Some(last_text) = agent.conversation.last_assistant_text() {
                println!("{last_text}");
            }
        }
        Err(e) => {
            if r#loop::is_upstream_exhausted(e) {
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

async fn maybe_run_injected_cleave_smoke_child(
    cwd: &Path,
    cli_model: &str,
) -> anyhow::Result<bool> {
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
            let shared_settings = settings::shared(cli_model);
            // Apply child runtime profile overrides from env (set by orchestrator)
            if let Ok(mut s) = shared_settings.lock() {
                if let Some(thinking) = std::env::var("OMEGON_CHILD_THINKING_LEVEL")
                    .ok()
                    .and_then(|v| settings::ThinkingLevel::parse(&v))
                {
                    s.thinking = thinking;
                }
                if let Some(class) = std::env::var("OMEGON_CHILD_CONTEXT_CLASS")
                    .ok()
                    .and_then(|v| settings::ContextClass::parse(&v))
                {
                    s.set_requested_context_class(class);
                }
            }
            let mut agent = setup::AgentSetup::new_with_safety(
                cwd,
                None,
                Some(shared_settings.clone()),
                std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok(),
            )
            .await?;
            agent.instance_id = paths::instance_id("cleave");
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
                .map(|s| s.model_short())
                .unwrap_or_else(|| "unknown".into());
            let selected_provider = crate::providers::infer_provider_id(
                &settings_guard
                    .as_ref()
                    .map(|s| s.model.clone())
                    .unwrap_or_default(),
            );
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
                "context_class": settings_guard.as_ref()
                    .map(|s| s.effective_requested_class().short().to_string())
                    .unwrap_or_else(|| status.context_class.clone()),
                "thinking_level": settings_guard.as_ref()
                    .map(|s| s.thinking.as_str().to_string())
                    .unwrap_or_else(|| status.thinking_level.clone()),
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

async fn call_tdd_savepoint_extension(
    tool_name: &str,
    args: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let ext_dir = crate::extension_cli::extensions_dir()?.join("omegon-tdd-savepoint");
    if !ext_dir.join("manifest.toml").is_file() {
        anyhow::bail!("omegon-tdd-savepoint extension is not installed");
    }
    let spawned = crate::extensions::spawn_from_manifest(&ext_dir, &[]).await?;
    let result = spawned
        .feature
        .execute(
            tool_name,
            "cli-tdd",
            args,
            tokio_util::sync::CancellationToken::new(),
        )
        .await?;
    if result
        .details
        .get("is_error")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        anyhow::bail!("extension tool {tool_name} failed: {:?}", result.content);
    }
    Ok(result.details)
}

fn run_project_rules_command(cli: &Cli, action: &ProjectRulesAction) -> anyhow::Result<()> {
    match action {
        ProjectRulesAction::Check { context, json } => {
            let cwd = std::fs::canonicalize(&cli.cwd)?;
            let report = project_rules::check(&cwd, context);
            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "project rules: context={} mode={:?} passed={} findings={}",
                    report.context,
                    report.mode,
                    report.passed,
                    report.findings.len()
                );
                for finding in &report.findings {
                    println!(
                        "- {:?} {} enforced={} subject={} — {}",
                        finding.severity,
                        finding.rule_id,
                        finding.enforced,
                        finding.subject,
                        finding.message
                    );
                }
            }
            if report.passed {
                Ok(())
            } else {
                anyhow::bail!("project rules failed")
            }
        }
    }
}

async fn run_tdd_command(action: &TddAction) -> anyhow::Result<()> {
    match action {
        TddAction::Watch {
            filetype,
            watch_paths,
            change,
            scenario,
            task,
            once,
            emit_baseline,
            persist_failures,
            timeout_secs,
            command,
        } => {
            let command = tdd::TddCommand::new(command.clone())?;
            tdd::watch(tdd::WatchOptions {
                cwd: std::env::current_dir()?,
                filetype: filetype.clone(),
                watch_paths: watch_paths.clone(),
                command,
                change: change.clone(),
                scenario: scenario.clone(),
                task: task.clone(),
                once: *once,
                emit_baseline: *emit_baseline,
                persist_failures: *persist_failures,
                timeout: timeout_secs.map(std::time::Duration::from_secs),
            })
        }
        TddAction::Evidence {
            command_hash,
            change,
            scenario,
            task,
            current_diff_hash,
            current,
            scopes,
            json,
        } => {
            let cwd = std::env::current_dir()?;
            let ext_args = serde_json::json!({
                "cwd": cwd,
                "command_hash": command_hash,
                "change": change,
                "scenario": scenario,
                "task": task,
                "current_diff_hash": current_diff_hash,
                "current": current,
                "scopes": scopes,
            });
            match call_tdd_savepoint_extension("tdd_savepoint_evidence", ext_args).await {
                Ok(details) => {
                    if *json {
                        println!("{}", details);
                    } else {
                        let status = details
                            .get("status_label")
                            .and_then(serde_json::Value::as_str)
                            .or_else(|| details.get("status").and_then(serde_json::Value::as_str))
                            .unwrap_or("unknown");
                        let event_count = details
                            .get("events")
                            .and_then(serde_json::Value::as_array)
                            .map(Vec::len)
                            .unwrap_or(0);
                        println!("status: {status}");
                        println!("events: {event_count}");
                    }
                    Ok(())
                }
                Err(err) => {
                    eprintln!(
                        "omegon tdd evidence is moving to the omegon-tdd-savepoint extension; falling back to legacy core evidence reader ({err})"
                    );
                    let current_diff_hash = if *current {
                        Some(tdd::current_diff_hash(&cwd, scopes))
                    } else {
                        current_diff_hash.clone()
                    };
                    let query = tdd::EvidenceQuery {
                        command_hash: command_hash.clone(),
                        change: change.clone(),
                        scenario: scenario.clone(),
                        task: task.clone(),
                        current_diff_hash,
                    };
                    let events = tdd::read_events(&cwd, &query)?;
                    let status = tdd::classify_evidence(&events, &query);
                    if *json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "status": status,
                                "events": events,
                            })
                        );
                    } else {
                        println!("status: {:?}", status);
                        println!("events: {}", events.len());
                        for event in events.iter().rev().take(5) {
                            println!(
                                "- {} {} command={} change={:?} scenario={:?} task={:?}",
                                event.event_id,
                                event.transition,
                                event.command_hash,
                                event.change,
                                event.scenario,
                                event.task
                            );
                        }
                    }
                    Ok(())
                }
            }
        }
    }
}

async fn run_auth_command(action: &AuthAction) -> anyhow::Result<()> {
    match action {
        AuthAction::Status => {
            let status = auth::probe_all_providers().await;
            println!("{}", control_runtime::format_auth_status(&status));
            Ok(())
        }
        AuthAction::Login { provider } => run_auth_login(provider).await,
        AuthAction::Logout { provider } => match auth::logout_provider(provider) {
            Ok(()) => {
                auth::clear_provider_auth_env(provider);
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
                    s.meta.session_id,
                    s.meta.turns,
                    s.meta.tool_calls,
                    session::session_display_description(&s.meta)
                )
            })
            .collect();
        format!("Recent sessions:\n{}", lines.join("\n"))
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_remote_slash_command(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    events_tx: &broadcast::Sender<AgentEvent>,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    login_prompt_tx: &std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
    cli: &Cli,
    name: &str,
    args: &str,
) -> omegon_traits::SlashCommandResponse {
    use crate::tui::canonical_slash_command;
    use omegon_traits::SlashCommandResponse;

    let Some(command) = canonical_slash_command(name, args) else {
        if let Some(response) = execute_registered_remote_command(runtime_state, cli, name, args) {
            return response;
        }
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} is interactive-only or unavailable via remote slash execution."
            )),
        };
    };

    if let Some(response) = reject_remote_builtin_command(name, &command, cli) {
        return response;
    }

    if let Some(control_request) = control_runtime::control_request_from_slash(&command) {
        let mut ctx = control_runtime::ControlContext {
            runtime_state,
            agent,
            shared_settings,
            bridge,
            login_prompt_tx,
            events_tx,
            cli: &CliRuntimeView {
                no_session: cli.no_session,
                model: &cli.model,
                dangerously_bypass_permissions: cli.dangerously_bypass_permissions,
            },
        };
        return control_runtime::execute_control(&mut ctx, control_request).await;
    }

    if matches!(
        command,
        crate::tui::CanonicalSlashCommand::PlanView
            | crate::tui::CanonicalSlashCommand::PlanSet(_)
            | crate::tui::CanonicalSlashCommand::PlanList
            | crate::tui::CanonicalSlashCommand::PlanShow(_)
            | crate::tui::CanonicalSlashCommand::PlanSwitch(_)
            | crate::tui::CanonicalSlashCommand::PlanResume(_)
            | crate::tui::CanonicalSlashCommand::PlanBackground(_)
            | crate::tui::CanonicalSlashCommand::PlanDetach(_)
            | crate::tui::CanonicalSlashCommand::PlanPromote(_)
            | crate::tui::CanonicalSlashCommand::PlanBind(_)
            | crate::tui::CanonicalSlashCommand::PlanLedger(_)
            | crate::tui::CanonicalSlashCommand::PlanApprove
            | crate::tui::CanonicalSlashCommand::PlanExecute
            | crate::tui::CanonicalSlashCommand::PlanAdvance
            | crate::tui::CanonicalSlashCommand::PlanSkip
            | crate::tui::CanonicalSlashCommand::PlanClear
    ) {
        return execute_plan_slash_command(runtime_state, command);
    }

    if let Some(response) = execute_registered_remote_command(runtime_state, cli, name, args) {
        return response;
    }

    SlashCommandResponse {
        accepted: false,
        output: Some(format!(
            "Command /{name} is interactive-only or unavailable via remote slash execution."
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteBuiltinPolicy {
    Allow,
    RequiresBypass,
    Deny,
}

fn reject_remote_builtin_command(
    name: &str,
    command: &crate::tui::CanonicalSlashCommand,
    cli: &Cli,
) -> Option<omegon_traits::SlashCommandResponse> {
    use omegon_traits::SlashCommandResponse;

    match remote_builtin_policy(name, command) {
        RemoteBuiltinPolicy::Allow => None,
        RemoteBuiltinPolicy::RequiresBypass if cli.dangerously_bypass_permissions => None,
        RemoteBuiltinPolicy::RequiresBypass => Some(SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} requires interactive confirmation and is unavailable via remote slash execution."
            )),
        }),
        RemoteBuiltinPolicy::Deny => Some(SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} is interactive-only or unavailable via remote slash execution."
            )),
        }),
    }
}

fn remote_builtin_policy(
    name: &str,
    command: &crate::tui::CanonicalSlashCommand,
) -> RemoteBuiltinPolicy {
    let registry_name = remote_registry_name_for_command(name, command);
    let Some(definition) = crate::command_registry::builtin_command_definitions()
        .into_iter()
        .find(|definition| definition.name == registry_name)
    else {
        return RemoteBuiltinPolicy::Deny;
    };

    if registry_name == "skills" {
        return match command {
            crate::tui::CanonicalSlashCommand::SkillsView
            | crate::tui::CanonicalSlashCommand::SkillsHelp
            | crate::tui::CanonicalSlashCommand::SkillGet(_) => RemoteBuiltinPolicy::Allow,
            crate::tui::CanonicalSlashCommand::SkillsInstall(_)
            | crate::tui::CanonicalSlashCommand::SkillDelete(_) => {
                RemoteBuiltinPolicy::RequiresBypass
            }
            crate::tui::CanonicalSlashCommand::SkillCreate(_)
            | crate::tui::CanonicalSlashCommand::SkillImport { .. } => RemoteBuiltinPolicy::Deny,
            _ => RemoteBuiltinPolicy::Deny,
        };
    }

    if !definition.availability.cli {
        return RemoteBuiltinPolicy::Deny;
    }

    match command {
        crate::tui::CanonicalSlashCommand::AuthStatus
        | crate::tui::CanonicalSlashCommand::AuthLogout(_) => RemoteBuiltinPolicy::Allow,
        _ if definition.safety.requires_confirmation => RemoteBuiltinPolicy::RequiresBypass,
        _ => RemoteBuiltinPolicy::Allow,
    }
}

fn remote_registry_name_for_command<'a>(
    name: &'a str,
    command: &crate::tui::CanonicalSlashCommand,
) -> &'a str {
    match command {
        crate::tui::CanonicalSlashCommand::AutomationView
        | crate::tui::CanonicalSlashCommand::AutomationSet(_) => "automation",
        crate::tui::CanonicalSlashCommand::AuthLogin { .. }
        | crate::tui::CanonicalSlashCommand::AuthLogout { .. } => "auth",
        crate::tui::CanonicalSlashCommand::SkillsView
        | crate::tui::CanonicalSlashCommand::SkillsHelp
        | crate::tui::CanonicalSlashCommand::SkillsReload
        | crate::tui::CanonicalSlashCommand::SkillsInstall(_)
        | crate::tui::CanonicalSlashCommand::SkillCreate(_)
        | crate::tui::CanonicalSlashCommand::SkillImport { .. }
        | crate::tui::CanonicalSlashCommand::SkillGet(_)
        | crate::tui::CanonicalSlashCommand::SkillDelete(_) => "skills",
        crate::tui::CanonicalSlashCommand::NoteAdd { .. }
        | crate::tui::CanonicalSlashCommand::NotesView
        | crate::tui::CanonicalSlashCommand::NotesClear
        | crate::tui::CanonicalSlashCommand::CheckinView => "notes",
        crate::tui::CanonicalSlashCommand::WorkspaceStatusView
        | crate::tui::CanonicalSlashCommand::WorkspaceListView
        | crate::tui::CanonicalSlashCommand::WorkspaceNew(_)
        | crate::tui::CanonicalSlashCommand::WorkspaceDestroy(_)
        | crate::tui::CanonicalSlashCommand::WorkspaceAdopt
        | crate::tui::CanonicalSlashCommand::WorkspaceRelease
        | crate::tui::CanonicalSlashCommand::WorkspaceArchive
        | crate::tui::CanonicalSlashCommand::WorkspacePrune
        | crate::tui::CanonicalSlashCommand::WorkspaceBindMilestone(_)
        | crate::tui::CanonicalSlashCommand::WorkspaceBindNode(_)
        | crate::tui::CanonicalSlashCommand::WorkspaceBindClear
        | crate::tui::CanonicalSlashCommand::WorkspaceRoleView
        | crate::tui::CanonicalSlashCommand::WorkspaceRoleSet(_)
        | crate::tui::CanonicalSlashCommand::WorkspaceRoleClear
        | crate::tui::CanonicalSlashCommand::WorkspaceKindView
        | crate::tui::CanonicalSlashCommand::WorkspaceKindSet(_)
        | crate::tui::CanonicalSlashCommand::WorkspaceKindClear => "status",
        _ => name,
    }
}

fn execute_registered_remote_command(
    runtime_state: &mut InteractiveAgentState,
    cli: &Cli,
    name: &str,
    args: &str,
) -> Option<omegon_traits::SlashCommandResponse> {
    use omegon_traits::{CommandResult, SlashCommandResponse};

    let definition = runtime_state
        .bus
        .command_definitions()
        .iter()
        .map(|(_, definition)| definition)
        .find(|definition| definition.name == name)?;

    if !definition.availability.cli {
        return Some(SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} is not available via CLI/remote slash execution."
            )),
        });
    }
    if definition.safety.requires_confirmation && !cli.dangerously_bypass_permissions {
        return Some(SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} requires interactive confirmation and is unavailable via remote slash execution."
            )),
        });
    }

    match runtime_state.bus.dispatch_command(name, args) {
        CommandResult::Display(text) => Some(SlashCommandResponse {
            accepted: true,
            output: Some(text),
        }),
        CommandResult::Handled => Some(SlashCommandResponse {
            accepted: true,
            output: None,
        }),
        CommandResult::NotHandled => Some(SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Command /{name} was registered but did not handle the request."
            )),
        }),
    }
}

fn execute_plan_slash_command(
    runtime_state: &mut InteractiveAgentState,
    command: crate::tui::CanonicalSlashCommand,
) -> omegon_traits::SlashCommandResponse {
    use crate::tui::CanonicalSlashCommand;
    use omegon_traits::SlashCommandResponse;

    let intent = &mut runtime_state.conversation.intent;
    match &command {
        CanonicalSlashCommand::PlanList => {
            intent.apply_plan_action(PlanAction::View);
            return SlashCommandResponse {
                accepted: true,
                output: Some(render_plan_list(runtime_state)),
            };
        }
        CanonicalSlashCommand::PlanShow(id) => {
            return SlashCommandResponse {
                accepted: true,
                output: Some(render_plan_show(runtime_state, id)),
            };
        }
        CanonicalSlashCommand::PlanLedger(id) => {
            return SlashCommandResponse {
                accepted: true,
                output: Some(render_plan_ledger(runtime_state, id.as_deref())),
            };
        }
        CanonicalSlashCommand::PlanBackground(id) => {
            let output = runtime_state.conversation.intent.mark_plan_view_status(
                id.as_deref(),
                crate::conversation::PlanStatus::Backgrounded,
                "Plan backgrounded; foreground unchanged.",
            );
            return SlashCommandResponse {
                accepted: true,
                output: Some(output),
            };
        }
        CanonicalSlashCommand::PlanDetach(id) => {
            let output = runtime_state.conversation.intent.mark_plan_view_status(
                id.as_deref(),
                crate::conversation::PlanStatus::Detached,
                "Plan detached; durable artifacts unchanged.",
            );
            return SlashCommandResponse {
                accepted: true,
                output: Some(output),
            };
        }
        CanonicalSlashCommand::PlanSwitch(id) | CanonicalSlashCommand::PlanResume(id) => {
            let output = runtime_state.conversation.intent.switch_visible_plan(id);
            return SlashCommandResponse {
                accepted: true,
                output: Some(output),
            };
        }
        _ => {}
    }

    let clears_completed_plan = matches!(
        command,
        CanonicalSlashCommand::PlanAdvance
            | CanonicalSlashCommand::PlanSkip
            | CanonicalSlashCommand::PlanClear
    );
    match command {
        CanonicalSlashCommand::PlanView => intent.apply_plan_action(PlanAction::View),
        CanonicalSlashCommand::PlanSet(ref items) => intent.apply_plan_action(PlanAction::Set {
            items: items.clone(),
        }),
        CanonicalSlashCommand::PlanApprove => intent.apply_plan_action(PlanAction::Approve),
        CanonicalSlashCommand::PlanExecute => intent.apply_plan_action(PlanAction::Execute),
        CanonicalSlashCommand::PlanAdvance => intent.apply_plan_action(PlanAction::Advance),
        CanonicalSlashCommand::PlanSkip => intent.apply_plan_action(PlanAction::Skip),
        CanonicalSlashCommand::PlanClear => intent.apply_plan_action(PlanAction::Clear),
        _ => {
            return SlashCommandResponse {
                accepted: false,
                output: Some("Not a plan command.".into()),
            };
        }
    }

    let output = match command {
        CanonicalSlashCommand::PlanView if intent.work_plan.is_empty() => intent
            .render_last_completed_work_plan()
            .unwrap_or_else(|| intent.render_work_plan()),
        _ if clears_completed_plan && intent.work_plan.is_empty() => {
            let mut output = format!(
                "Plan cleared
{}",
                intent.render_work_plan()
            );
            if let Some(completed) = intent.render_last_completed_work_plan() {
                output.push_str(
                    "

Last completed plan
",
                );
                output.push_str(&completed);
            }
            output
        }
        _ => intent.render_work_plan(),
    };

    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

fn work_plan_snapshot_with_lifecycle(
    intent: &crate::conversation::IntentDocument,
) -> serde_json::Value {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo_root = setup::find_project_root(&cwd);
    intent.work_plan_snapshot_json_for_repo(&repo_root)
}

fn render_plan_show(runtime_state: &InteractiveAgentState, plan_id: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo_root = setup::find_project_root(&cwd);
    crate::plan::render_plan_show_text(&runtime_state.conversation.intent, &repo_root, plan_id)
}

fn render_plan_ledger(runtime_state: &InteractiveAgentState, plan_id: Option<&str>) -> String {
    let intent = &runtime_state.conversation.intent;
    let mut lines = vec!["Plan ledger".to_string()];
    for entry in &intent.completion_ledger {
        if plan_id.is_none_or(|id| id == entry.plan_id) {
            lines.push(format!(
                "- {} · {} · {} item(s) · {}",
                entry.plan_id,
                entry.source.label(),
                entry.item_count,
                entry.summary
            ));
        }
    }
    if lines.len() == 1 {
        lines.push("- none".to_string());
    }
    lines.join(
        "
",
    )
}

fn render_plan_list(runtime_state: &InteractiveAgentState) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo_root = setup::find_project_root(&cwd);
    crate::plan::render_plan_list_text(&runtime_state.conversation.intent, &repo_root)
}

async fn run_auth_login(provider: &str) -> anyhow::Result<()> {
    let provider = auth::canonical_provider_id(provider);
    let result = match provider {
        "anthropic" => auth::login_anthropic().await,
        "openai-codex" => auth::login_openai().await,
        "google-antigravity" => auth::login_antigravity().await,
        "google" => {
            login_api_key(
                "google",
                "GOOGLE_API_KEY",
                "https://aistudio.google.com/apikey",
            )
            .await
        }
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
        "opencode-go" => {
            login_api_key(
                "opencode-go",
                "OPENCODE_GO_API_KEY",
                "https://opencode.ai/go",
            )
            .await
        }
        _ => {
            eprintln!(
                "Unknown provider: {provider}. Use: anthropic, openai, openai-codex, google, google-antigravity, openrouter, opencode-go, ollama-cloud"
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

/// Task specification — loaded from a TOML file for `omegon run`.
#[derive(Debug, Clone, serde::Deserialize)]
struct TaskSpec {
    task: TaskSpecTask,
    #[serde(default)]
    bounds: Option<TaskSpecBounds>,
    #[serde(default)]
    agent: Option<TaskSpecAgent>,
    #[serde(default)]
    output: Option<TaskSpecOutput>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct TaskSpecTask {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    prompt_file: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct TaskSpecBounds {
    #[serde(default = "default_max_turns")]
    max_turns: u32,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    #[serde(default)]
    token_budget: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct TaskSpecAgent {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    posture: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    persona: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct TaskSpecOutput {
    #[serde(default)]
    path: Option<String>,
}

fn default_max_turns() -> u32 {
    30
}
fn default_timeout() -> u64 {
    600
}

async fn run_sentry_command(
    config_path: &Path,
    control_port: u16,
    strict_port: bool,
    _cli: &Cli,
) -> anyhow::Result<()> {
    let cwd = std::fs::canonicalize(std::env::current_dir()?)?;
    let state_dir = cwd.join(".omegon").join("sentry");
    std::fs::create_dir_all(&state_dir)?;
    let state_db = std::sync::Arc::new(sentry::state_db::StateDb::open(
        &state_dir.join("state.db"),
    )?);

    let instance_id = paths::instance_id("sentry");

    // Auto-detect board:
    //   1. .omegon/tasks/ (task tree) takes precedence
    //   2. sentry.toml (file board)
    //   3. flynt vault marker (.flynt/config.toml or default sqlite)
    //      — selected when omegon is launched into a flynt directory,
    //      including the ACP-from-Zed path. Explicit override via the
    //      FLYNT_VAULT env var, which points at any vault root.
    let task_tree_dir = task_tree::tasks_dir(&cwd);
    let has_task_tree = task_tree_dir.exists()
        && std::fs::read_dir(&task_tree_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    let config_path = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        cwd.join(config_path)
    };
    let has_config = config_path.exists();

    let flynt_vault_root: Option<std::path::PathBuf> = std::env::var("FLYNT_VAULT")
        .ok()
        .map(|raw| {
            // Canonicalize so a relative FLYNT_VAULT under whatever
            // cwd this process inherited (Zed's project root, a shell,
            // a test) resolves to a single concrete path. Falls back
            // to the literal value if the path doesn't exist yet —
            // FlyntTaskBoard::open() will surface the missing-db
            // error with the same string the operator set.
            let p = std::path::PathBuf::from(&raw);
            std::fs::canonicalize(&p).unwrap_or(p)
        })
        // Walk up from cwd to find a vault marker. Critical for the
        // ACP-from-Zed flow: cwd is a git repo nested INSIDE the
        // vault, not the vault root itself.
        .or_else(|| sentry::flynt_board::find_vault_root(&cwd));

    // Optional explicit project scope. When set, FlyntTaskBoard only
    // surfaces tasks on boards owned by this flynt project — prevents
    // cross-project bleed when one omegon process serves a vault that
    // hosts multiple project boards.
    let flynt_project: Option<uuid::Uuid> = std::env::var("FLYNT_PROJECT")
        .ok()
        .and_then(|s| uuid::Uuid::parse_str(&s).ok());

    let (board, config): (std::sync::Arc<dyn sentry::TaskBoard>, sentry::SentryConfig) =
        if has_task_tree {
            if has_config {
                tracing::warn!(
                    "both .omegon/tasks/ and sentry.toml exist — using task tree, sentry.toml tasks ignored"
                );
            }
            tracing::info!(path = %task_tree_dir.display(), "using task tree board");
            let board = std::sync::Arc::new(sentry::tree_board::TaskTreeBoard::new(
                cwd.clone(),
                state_db.clone(),
                instance_id.clone(),
            ));
            let config = if has_config {
                sentry::load_config(&config_path)?
            } else {
                sentry::SentryConfig {
                    sentry: sentry::SentryGlobal {
                        max_concurrent: 1,
                        log_retention_days: 30,
                        routing: None,
                    },
                    tasks: Vec::new(),
                }
            };
            (board, config)
        } else if has_config {
            let config = sentry::load_config(&config_path)?;
            let config_dir = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();
            tracing::info!(
                tasks = config.tasks.len(),
                "using file board from {}",
                config_path.display()
            );
            let board = std::sync::Arc::new(sentry::FileTaskBoard::new(
                config.clone(),
                state_db.clone(),
                instance_id.clone(),
                config_dir,
            ));
            (board, config)
        } else if let Some(vault_root) = flynt_vault_root {
            tracing::info!(
                vault = %vault_root.display(),
                project = ?flynt_project,
                "using flynt vault board"
            );
            let mut board =
                sentry::FlyntTaskBoard::open(vault_root, state_db.clone(), instance_id.clone())?;
            if let Some(pid) = flynt_project {
                // Probe before applying so a typo in FLYNT_PROJECT
                // surfaces at startup rather than as silent empty
                // list_actionable() results. Use the agent's
                // `engagement_list` / `project_list` tools to discover
                // valid UUIDs for this vault.
                match board.project_exists(&pid) {
                    Ok(true) => {}
                    Ok(false) => tracing::warn!(
                        project = %pid,
                        "FLYNT_PROJECT does not match any board in this vault — sentry will see no tasks"
                    ),
                    Err(e) => tracing::warn!(error = %e, "could not probe FLYNT_PROJECT validity"),
                }
                board = board.with_project(pid);
            }
            let board = std::sync::Arc::new(board);
            // FlyntTaskBoard sources tasks from the vault, not a TOML
            // file; the SentryConfig is empty so trigger discovery
            // (below) sees no fileboard tasks. Per-task triggers come
            // from each Task's external_refs (cron:/webhook:) and are
            // surfaced via FlyntTaskBoard::list_actionable.
            let config = sentry::SentryConfig {
                sentry: sentry::SentryGlobal {
                    max_concurrent: 1,
                    log_retention_days: 30,
                    routing: None,
                },
                tasks: Vec::new(),
            };
            (board, config)
        } else {
            anyhow::bail!(
                "no task source found — create .omegon/tasks/, sentry.toml, or open a flynt vault"
            );
        };

    // Build trigger runtime from .omegon/triggers/ configs + sentry.toml schedules
    let mut trigger_configs = triggers::load_trigger_configs(&cwd);

    // Convert sentry.toml task triggers into TriggerConfigs so they flow through
    // the unified trigger system
    for task_cfg in &config.tasks {
        if let Some(ref trig) = task_cfg.trigger {
            let mut meta = triggers::TriggerMeta {
                name: format!("sentry:{}", task_cfg.name),
                enabled: true,
                ..Default::default()
            };
            if let Some(ref cron) = trig.cron {
                meta.cron = Some(cron.schedule.clone());
            }
            if let Some(ref wh) = trig.webhook {
                meta.name = wh.name.clone();
            }
            trigger_configs.push(triggers::TriggerConfig {
                trigger: meta,
                filter: None,
                prompt: triggers::PromptTemplate {
                    template: String::new(),
                },
                session: None,
            });
        }
    }

    let global_cancel = CancellationToken::new();

    let (trigger_runtime, event_tx) =
        triggers::TriggerRuntimeBuilder::new(trigger_configs, cwd.clone())
            .build(global_cancel.clone());

    let sentry_state = sentry::routes::SentryState {
        board: board.clone(),
        state_db: state_db.clone(),
        event_tx: event_tx.clone(),
    };
    let sentry_routes = sentry::routes::sentry_router(sentry_state);

    let global_cancel = CancellationToken::new();

    // SIGTERM + Ctrl-C
    let cancel_sig = global_cancel.clone();
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => { tracing::info!("Ctrl-C received — shutting down sentry"); }
                _ = sigterm.recv() => { tracing::info!("SIGTERM received — shutting down sentry"); }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
            tracing::info!("Ctrl-C received — shutting down sentry");
        }
        cancel_sig.cancel();
    });

    // Prune old run records on startup
    if let Ok(pruned) = state_db.prune_old_runs(config.sentry.log_retention_days)
        && pruned > 0
    {
        tracing::info!(pruned, "pruned old sentry run records");
    }

    let model = _cli.model.clone();

    // Start control plane
    let bind_addr = format!("127.0.0.1:{control_port}");
    let health_router = axum::Router::new()
        .route("/api/healthz", axum::routing::get(|| async { "ok" }))
        .route("/api/readyz", axum::routing::get(|| async { "ok" }));
    let app = health_router.merge(sentry_routes);

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) if !strict_port => {
            tracing::warn!(port = control_port, error = %e, "port unavailable — trying ephemeral");
            tokio::net::TcpListener::bind("127.0.0.1:0").await?
        }
        Err(e) => return Err(e.into()),
    };
    let bound = listener.local_addr()?;

    let startup = serde_json::json!({
        "mode": "sentry",
        "control_url": format!("http://{bound}"),
        "health_url": format!("http://{bound}/api/healthz"),
        "ready_url": format!("http://{bound}/api/readyz"),
        "tasks_url": format!("http://{bound}/api/sentry/tasks"),
        "tasks": config.tasks.iter().map(|t| &t.name).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string(&startup)?);

    let server_cancel = global_cancel.clone();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(server_cancel.cancelled_owned())
            .await
        {
            tracing::error!(error = %e, "sentry control plane error");
        }
    });

    let budget_limits = std::sync::Arc::new(sentry::executor::BudgetLimits::from_config(&config));

    // Run the sentry loop — consumes TriggerEvent from the unified runtime
    let routing = config.sentry.routing.map(std::sync::Arc::new);
    sentry::executor::run_sentry_loop(
        board,
        state_db.clone(),
        budget_limits,
        trigger_runtime.event_rx,
        global_cancel.clone(),
        model,
        cwd,
        config.sentry.max_concurrent,
        routing,
    )
    .await;

    // Release any claimed tasks on shutdown
    if let Ok(released) = state_db.release_all(&instance_id)
        && !released.is_empty()
    {
        tracing::info!(count = released.len(), "released claimed tasks on shutdown");
    }

    tracing::info!("sentry shutdown complete");
    Ok(())
}

fn load_task_spec(path: &Path) -> anyhow::Result<TaskSpec> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read task spec {}: {e}", path.display()))?;
    let spec: TaskSpec = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("invalid task spec {}: {e}", path.display()))?;
    Ok(spec)
}

/// Structured output from a bounded task run.
#[derive(Debug, Clone, serde::Serialize)]
struct RunResult {
    status: String, // "completed", "error", "exhausted", "timeout"
    turns: u32,
    total_input_tokens: u64,
    total_output_tokens: u64,
    files_read: Vec<String>,
    files_modified: Vec<String>,
    duration_secs: f64,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[allow(clippy::too_many_arguments)]
async fn run_bounded_task(
    prompt: Option<&str>,
    prompt_file: Option<&Path>,
    output_path: Option<&Path>,
    max_turns: u32,
    timeout_secs: u64,
    token_budget: Option<u64>,
    _manifest: Option<&str>, // reserved for Phase 2
    cwd: &Path,
    model: &str,
    cli: &Cli,
) -> anyhow::Result<()> {
    let start = std::time::Instant::now();

    // Resolve prompt
    let prompt_text = match (prompt, prompt_file) {
        (Some(p), _) => p.to_string(),
        (None, Some(path)) => {
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            std::fs::read_to_string(&resolved).map_err(|e| {
                anyhow::anyhow!("Failed to read prompt file {}: {e}", resolved.display())
            })?
        }
        (None, None) => {
            eprintln!("omegon run: --prompt or --prompt-file required");
            std::process::exit(1);
        }
    };

    // Setup
    let shared_settings = bootstrap::initialize_shared_settings(&bootstrap::SettingsInit {
        model,
        cwd,
        cli_posture: resolve_cli_posture(cli).as_deref(),
        slim: true,
        full: cli.full,
        max_turns,
        apply_profile_posture: false, // run mode uses explicit config, not profile
    });
    if let Ok(mut s) = shared_settings.lock() {
        s.set_model(model);
    }

    let mut agent = setup::AgentSetup::new_with_safety(
        cwd,
        None,
        Some(shared_settings.clone()),
        cli.dangerously_bypass_permissions,
    )
    .await?;
    agent.instance_id = paths::instance_id("run");
    bootstrap::apply_runtime_posture(
        &mut agent,
        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
        omegon_traits::OmegonAutonomyMode::OperatorDriven,
    );
    agent.conversation.push_user(prompt_text);

    let loop_config = bootstrap::build_loop_config(
        &shared_settings,
        &agent.cwd,
        model,
        bootstrap::LoopConfigOverrides {
            max_retries: cli.max_retries,
            secrets: Some(agent.secrets.clone()),
            enforce_first_turn_execution_bias: true,
            ..Default::default()
        },
    );

    let bridge = bootstrap::resolve_bridge_or_bail(model).await?;
    let (events_tx, mut events_rx) = bootstrap::wire_event_channel(&agent, 256);

    // Token tracking
    let total_in = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total_out = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let turn_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let total_in_t = total_in.clone();
    let total_out_t = total_out.clone();
    let turn_count_t = turn_count.clone();

    // Event consumer — track tokens and turns, log to stderr
    let event_task = tokio::spawn(async move {
        while let Ok(event) = events_rx.recv().await {
            match event {
                AgentEvent::TurnStart { turn } => {
                    turn_count_t.store(turn, std::sync::atomic::Ordering::Relaxed);
                    tracing::info!("── Turn {turn} ──");
                }
                AgentEvent::ToolStart { name, .. } => {
                    tracing::info!("→ {name}");
                }
                AgentEvent::TurnEnd(te) => {
                    total_in_t
                        .fetch_add(te.actual_input_tokens, std::sync::atomic::Ordering::Relaxed);
                    total_out_t.fetch_add(
                        te.actual_output_tokens,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                AgentEvent::AgentEnd => break,
                _ => {}
            }
        }
    });

    // Run with wall-clock timeout
    let cancel = CancellationToken::new();
    let cancel_timeout = cancel.clone();
    let timeout_handle = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(timeout_secs)).await;
        tracing::warn!("Wall-clock timeout ({timeout_secs}s) — cancelling");
        cancel_timeout.cancel();
    });

    let loop_result = r#loop::run(
        bridge.as_ref(),
        &mut agent.bus,
        &mut agent.context_manager,
        &mut agent.conversation,
        &events_tx,
        cancel.clone(),
        &loop_config,
    )
    .await;

    timeout_handle.abort();
    bridge.shutdown().await;
    drop(events_tx);
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), event_task).await;

    let elapsed = start.elapsed().as_secs_f64();
    let turns = turn_count.load(std::sync::atomic::Ordering::Relaxed);
    let in_tokens = total_in.load(std::sync::atomic::Ordering::Relaxed);
    let out_tokens = total_out.load(std::sync::atomic::Ordering::Relaxed);

    // Check token budget
    if let Some(budget) = token_budget
        && in_tokens + out_tokens > budget
    {
        tracing::warn!(
            "Token budget exceeded: {}+{} = {} > {budget}",
            in_tokens,
            out_tokens,
            in_tokens + out_tokens
        );
    }

    let summary = agent
        .conversation
        .last_assistant_text()
        .unwrap_or_default()
        .to_string();

    let (status, error, exit_code) = match &loop_result {
        Ok(()) => {
            if cancel.is_cancelled() {
                (
                    "timeout".to_string(),
                    Some("wall-clock timeout".to_string()),
                    3,
                )
            } else {
                ("completed".to_string(), None, 0)
            }
        }
        Err(e) => {
            if r#loop::is_upstream_exhausted(e) {
                ("exhausted".to_string(), Some(e.to_string()), 2)
            } else {
                ("error".to_string(), Some(e.to_string()), 1)
            }
        }
    };

    let result = RunResult {
        status,
        turns,
        total_input_tokens: in_tokens,
        total_output_tokens: out_tokens,
        files_read: agent
            .conversation
            .intent
            .files_read
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        files_modified: agent
            .conversation
            .intent
            .files_modified
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        duration_secs: elapsed,
        summary,
        error,
    };

    let json = serde_json::to_string_pretty(&result)?;

    match output_path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &json)?;
            tracing::info!(path = %path.display(), "result written");
        }
        None => {
            println!("{json}");
        }
    }

    std::process::exit(exit_code);
}

fn nex_cli(action: &NexAction) {
    let cwd = std::env::current_dir().unwrap_or_default();

    match action {
        NexAction::Init => {
            let nex_dir = cwd.join(".omegon").join("nex");
            let profile_path = nex_dir.join("project.toml");
            if profile_path.exists() {
                eprintln!("  .omegon/nex/project.toml already exists");
                std::process::exit(1);
            }
            let _ = std::fs::create_dir_all(&nex_dir);
            let project_name = cwd
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project");
            let template = format!(
                r#"# Nex sandbox profile for {project_name}
# Docs: https://omegon.styrene.io/docs/sandbox

[profile]
name = "{project_name}"
base = "coding"
# image = "ghcr.io/styrene-lab/omegon:latest"  # explicit image override

# [overlays.custom]
# packages = ["python312Packages.requests"]

[resources]
memory_mb = 2048
# cpu_shares = 1024
# pids_limit = 256
readonly_rootfs = true

# Network isolation policy: isolated | egress | bridge | host
[network]
policy = "isolated"

# Filtered egress — uncomment to allow specific API endpoints only:
# [network]
# policy = "egress"
# [network.egress]
# allow_hosts = ["api.anthropic.com", "api.openai.com"]
# allow_ports = [443]
# deny_private = true
# deny_metadata = true

# Bridge with port mappings — uncomment for dev servers:
# [network]
# policy = "bridge"
# [[network.ports]]
# host = 3000
# container = 3000

[capabilities]
mount_cwd = true
filesystem_write = true
# env_passthrough = ["DATABASE_URL"]
# allowed_tools = ["bash", "read_file", "write_file", "edit_file"]
# denied_tools = ["web_search"]
"#
            );
            if let Err(e) = std::fs::write(&profile_path, template) {
                eprintln!("  Failed to write {}: {e}", profile_path.display());
                std::process::exit(1);
            }
            eprintln!("  Created .omegon/nex/project.toml");
            eprintln!("  Enable with: /sandbox on (in TUI) or edit the profile to customize");
        }
        NexAction::List => {
            let home = dirs::home_dir().unwrap_or_default().join(".omegon");
            let registry = nex::NexRegistry::load(&home, Some(&cwd)).unwrap_or_else(|e| {
                eprintln!("  Failed to load profiles: {e}");
                std::process::exit(1);
            });
            let profiles = registry.list();
            if profiles.is_empty() {
                eprintln!("  No profiles found.");
                return;
            }
            eprintln!("  {} profile(s):\n", profiles.len());
            for p in &profiles {
                let hash_short = if p.profile_hash.len() > 12 {
                    &p.profile_hash[..12]
                } else {
                    &p.profile_hash
                };
                let image = p.image_ref.as_deref().unwrap_or("(needs build)");
                eprintln!("  {:<20} {:<14} {}", p.name, hash_short, image);
            }
        }
        NexAction::Inspect { name } => {
            let home = dirs::home_dir().unwrap_or_default().join(".omegon");
            let registry = nex::NexRegistry::load(&home, Some(&cwd)).unwrap_or_else(|e| {
                eprintln!("  Failed to load profiles: {e}");
                std::process::exit(1);
            });
            match registry.resolve(name) {
                Some(p) => {
                    eprintln!("  Profile: {}", p.name);
                    eprintln!("  Hash:    {}", p.profile_hash);
                    eprintln!("  Domain:  {}", p.base_domain);
                    if let Some(ref img) = p.image_ref {
                        eprintln!("  Image:   {img}");
                    }
                    eprintln!("\n  Resources:");
                    if let Some(mem) = p.resource_limits.memory_mb {
                        eprintln!("    memory:   {mem} MB");
                    }
                    eprintln!("    readonly: {}", p.resource_limits.readonly_rootfs);
                    eprintln!("\n  Network:");
                    eprintln!("    policy:  {}", p.capabilities.network.display_label());
                    if let nex::NexNetworkPolicy::Egress {
                        filter: Some(ref f),
                    } = p.capabilities.network
                    {
                        if !f.allow_hosts.is_empty() {
                            eprintln!("    hosts:   {}", f.allow_hosts.join(", "));
                        }
                        if !f.allow_cidrs.is_empty() {
                            eprintln!("    cidrs:   {}", f.allow_cidrs.join(", "));
                        }
                        if !f.allow_ports.is_empty() {
                            let ports: Vec<String> =
                                f.allow_ports.iter().map(|p| p.to_string()).collect();
                            eprintln!("    ports:   {}", ports.join(", "));
                        }
                        eprintln!("    deny_private:  {}", f.deny_private);
                        eprintln!("    deny_metadata: {}", f.deny_metadata);
                    }
                    if let nex::NexNetworkPolicy::Bridge { ref ports } = p.capabilities.network {
                        for pm in ports {
                            eprintln!("    publish: {}:{}", pm.host, pm.container);
                        }
                    }
                    eprintln!("\n  Capabilities:");
                    eprintln!("    fs_write:  {}", p.capabilities.filesystem_write);
                    eprintln!("    mount_cwd: {}", p.capabilities.mount_cwd);
                    if !p.capabilities.allowed_tools.is_empty() {
                        eprintln!("    allowed:   {}", p.capabilities.allowed_tools.join(", "));
                    }
                    if !p.capabilities.denied_tools.is_empty() {
                        eprintln!("    denied:    {}", p.capabilities.denied_tools.join(", "));
                    }
                }
                None => {
                    eprintln!("  Profile '{}' not found.", name);
                    eprintln!("  Run 'omegon nex list' to see available profiles.");
                    std::process::exit(1);
                }
            }
        }
        NexAction::Compose { name, service } => {
            let home = dirs::home_dir().unwrap_or_default().join(".omegon");
            let registry = nex::NexRegistry::load(&home, Some(&cwd)).unwrap_or_else(|e| {
                eprintln!("  Failed to load profiles: {e}");
                std::process::exit(1);
            });
            match registry.resolve(name) {
                Some(p) => {
                    let output = nex::compose::to_compose_file(p, service.as_deref());
                    // Write to stdout (not stderr) so it can be piped/redirected
                    print!("{output}");
                }
                None => {
                    eprintln!("  Profile '{}' not found.", name);
                    eprintln!("  Run 'omegon nex list' to see available profiles.");
                    std::process::exit(1);
                }
            }
        }
        NexAction::NetworkPolicy { source } => {
            let hosts = if source == "sandboxed" {
                // Default --sandboxed allowlist
                vec![
                    "api.anthropic.com",
                    "api.openai.com",
                    "openrouter.ai",
                    "api.groq.com",
                    "api.x.ai",
                    "api.mistral.ai",
                    "api.cerebras.ai",
                    "api.perplexity.ai",
                    "generativelanguage.googleapis.com",
                    "cloudcode-pa.googleapis.com",
                    "router.huggingface.co",
                    "ollama.com",
                    "opencode.ai",
                    "github.com",
                    "api.github.com",
                    "ghcr.io",
                ]
            } else {
                eprintln!("  Custom profile egress → NetworkPolicy not yet implemented.");
                eprintln!("  Use 'sandboxed' for the default allowlist.");
                std::process::exit(1);
            };

            // Emit both Kubernetes NetworkPolicy and CiliumNetworkPolicy
            let k8s_rules: Vec<String> = hosts
                .iter()
                .map(|h| format!(
                    "    - to:\n        - ipBlock:\n            cidr: 0.0.0.0/0  # resolved from {h}\n      ports:\n        - protocol: TCP\n          port: 443"
                ))
                .collect();

            let cilium_rules: Vec<String> = hosts
                .iter()
                .map(|h| format!("      - matchPattern: \"{h}\""))
                .collect();

            print!(
                r#"# Generated by omegon nex networkpolicy
# Apply with: kubectl apply -f <this-file>
#
# For clusters using Cilium CNI, use the CiliumNetworkPolicy below.
# For vanilla Kubernetes, use the NetworkPolicy (requires DNS-based
# egress support or manual IP resolution).
#
# Set OMEGON_EGRESS_MODE=external in the container env to skip
# iptables and rely on this policy instead.

---
# Kubernetes NetworkPolicy (vanilla)
# NOTE: K8s NetworkPolicy doesn't support FQDN-based egress natively.
# You'll need to resolve these hostnames to IPs or use a CNI that
# supports FQDN policies (Cilium, Calico Enterprise).
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: omegon-egress
  labels:
    app.kubernetes.io/name: omegon
    sh.styrene.omegon.policy: egress-filter
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: omegon
  policyTypes:
    - Egress
  egress:
    # Allow DNS
    - to: []
      ports:
        - protocol: UDP
          port: 53
        - protocol: TCP
          port: 53
    # Allow LLM API endpoints (port 443)
{rules}

---
# CiliumNetworkPolicy (recommended for Cilium CNI)
# Supports FQDN-based egress natively — no IP resolution needed.
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: omegon-egress
  labels:
    app.kubernetes.io/name: omegon
    sh.styrene.omegon.policy: egress-filter
spec:
  endpointSelector:
    matchLabels:
      app.kubernetes.io/name: omegon
  egress:
    - toEndpoints:
        - matchLabels:
            k8s:io.kubernetes.pod.namespace: kube-system
            k8s-app: kube-dns
      toPorts:
        - ports:
            - port: "53"
              protocol: UDP
            - port: "53"
              protocol: TCP
    - toFQDNs:
{cilium_fqdns}
      toPorts:
        - ports:
            - port: "443"
              protocol: TCP
"#,
                rules = k8s_rules.join("\n"),
                cilium_fqdns = cilium_rules.join("\n"),
            );
        }
        NexAction::Status => {
            let runtime = nex::spawn::detect_container_runtime_public();
            match runtime {
                Some(rt) => {
                    eprintln!("  Container runtime: {rt}");
                    eprintln!("  Sandbox ready.");
                    eprintln!("\n  Enable with: /sandbox on (in TUI)");
                }
                None => {
                    eprintln!("  No container runtime found.");
                    eprintln!("\n  Install podman (recommended) or docker:");
                    eprintln!("    macOS:  brew install podman");
                    eprintln!("    Linux:  apt install podman");
                    eprintln!("    NixOS:  nix-env -i podman");
                }
            }
        }
    }
}

// ── Sandboxed re-exec ────────────────────────────────────────────────────
//
// When `--sandboxed` is passed, re-exec the entire omegon session inside
// an OCI container. The current directory is mounted at /work, env vars
// are forwarded, and all remaining CLI args are passed through.
// The user gets the same TUI/headless experience but with kernel-enforced
// filesystem isolation.

async fn run_sandboxed(cli: &Cli) -> anyhow::Result<()> {
    let runtime = cli
        .oci_runtime
        .clone()
        .or_else(|| std::env::var("OMEGON_OCI_RUNTIME").ok())
        .or_else(nex::spawn::detect_container_runtime_public)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "--oci/--sandboxed requires a container runtime.\n\
             Install podman (recommended) or docker:\n  \
             macOS:  brew install podman\n  \
             Linux:  apt install podman\n  \
             NixOS:  nix-env -i podman"
            )
        })?;

    let version = env!("CARGO_PKG_VERSION");
    // Allow CLI/env image override for local/custom builds. The OCI alias uses
    // the full substrate by default because that is the validated image family.
    let image = cli
        .oci_image
        .clone()
        .or_else(|| std::env::var("OMEGON_OCI_IMAGE").ok())
        .or_else(|| std::env::var("OMEGON_SANDBOX_IMAGE").ok())
        .unwrap_or_else(|| format!("ghcr.io/styrene-lab/omegon-full:{version}"));
    let cwd = std::fs::canonicalize(&cli.cwd)?;

    eprintln!("🔒 Running in sandboxed mode ({runtime} → {image})");
    eprintln!("   Workspace: {} → /work", cwd.display());

    // Check if image exists locally; if not, try to pull it
    let image_exists = std::process::Command::new(&runtime)
        .args(["image", "exists", &image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if !image_exists {
        eprintln!("   Image not found locally — pulling...");
        let pull_status = std::process::Command::new(&runtime)
            .args(["pull", &image])
            .status();

        match pull_status {
            Ok(s) if s.success() => {
                eprintln!("   Image pulled successfully.");
            }
            _ => {
                eprintln!(
                    "\n   Failed to pull '{image}'.\n\n   \
                     The sandboxed mode requires an OCI container image.\n   \
                     Options:\n     \
                     1. Build locally: nix build .#oci-coding && podman load < result\n     \
                     2. Use a custom image: OMEGON_SANDBOX_IMAGE=<image> omegon --sandboxed\n     \
                     3. Run without sandbox: omegon (without --sandboxed)\n"
                );
                std::process::exit(1);
            }
        }
    }

    let mut cmd = std::process::Command::new(&runtime);
    cmd.arg("run");
    cmd.arg("--rm");
    cmd.arg("-it"); // interactive + TTY for TUI

    // Mount cwd at /work
    cmd.arg(format!("-v={}:/work", cwd.display()));
    cmd.arg("--workdir=/work");

    // Read-only rootfs — prevent writes outside /work and /tmp
    cmd.arg("--read-only");
    cmd.arg("--tmpfs=/tmp:rw,nosuid,size=512m");

    // Drop all capabilities, then add back only NET_ADMIN for iptables egress filtering
    cmd.arg("--cap-drop=ALL");
    cmd.arg("--cap-add=NET_ADMIN");

    // Resource limits — prevent fork bombs and memory exhaustion
    cmd.arg("--pids-limit=512");
    cmd.arg("--memory=4g");

    // Prevent privilege escalation via setuid/setgid binaries
    cmd.arg("--security-opt=no-new-privileges");

    // NOTE: no --user flag. The entrypoint needs root (within the
    // container namespace) to apply iptables egress filter rules.
    // With rootless podman, container root maps to the host user —
    // no privilege escalation. With docker, --cap-drop=ALL limits
    // the effective capabilities to NET_ADMIN only.

    // Network — bridge with filtered egress. Only known LLM API
    // endpoints are reachable. Blocks exfiltration to arbitrary hosts.
    // The entrypoint applies iptables rules from OMEGON_EGRESS_FILTER.
    cmd.arg("--network=bridge");

    let egress_filter = serde_json::json!({
        "allow_hosts": [
            "api.anthropic.com",
            "api.openai.com",
            "openrouter.ai",
            "api.groq.com",
            "api.x.ai",
            "api.mistral.ai",
            "api.cerebras.ai",
            "api.perplexity.ai",
            "generativelanguage.googleapis.com",
            "cloudcode-pa.googleapis.com",
            "router.huggingface.co",
            "ollama.com",
            "opencode.ai",
            // GitHub (for git operations, release checks)
            "github.com",
            "api.github.com",
            // Container image registry (for pulling images)
            "ghcr.io",
        ],
        "allow_ports": [443],
        "deny_private": true,
        "deny_metadata": true,
    });
    cmd.arg("-e");
    cmd.arg(format!("OMEGON_EGRESS_FILTER={}", egress_filter));
    // Standalone mode — use iptables directly (not cluster CNI)
    cmd.arg("-e");
    cmd.arg("OMEGON_EGRESS_MODE=iptables");
    eprintln!("   Network:   filtered egress (LLM APIs + GitHub only)");

    // The host's ~/.omegon/ contains the encrypted secrets vault.
    // Mount it read-only at /data/omegon/ (the container's OMEGON_HOME).
    // omegon-secrets decrypts in memory at runtime — no plaintext on
    // disk, no API keys in env vars, no secrets in `podman inspect`.
    let omegon_home = crate::paths::omegon_home().ok();
    let vault_path = omegon_home.as_ref().map(|h| h.join("secrets.json"));
    let vault_exists = vault_path.as_ref().is_some_and(|p| p.exists());

    if vault_exists {
        let vault = vault_path.as_ref().unwrap();
        // Mount only the secrets vault file — not the entire ~/.omegon/
        // directory (which contains session history, skills, plugins, etc.)
        // Use a tmpfs overlay for /data/omegon so the entrypoint can mkdir/write,
        // then bind-mount the vault file on top.
        cmd.arg("--tmpfs=/data/omegon:rw,size=1m");
        cmd.arg(format!(
            "-v={}:/data/omegon/secrets.json:ro",
            vault.display()
        ));
        eprintln!(
            "   Secrets:   {} → /data/omegon/secrets.json (vault, read-only)",
            vault.display()
        );
    } else {
        // No vault — fall back to env var forwarding with a warning.
        // This is less secure (secrets visible in env/podman inspect)
        // but necessary for users who haven't set up the vault yet.
        // tmpfs for /data/omegon so the entrypoint can bootstrap secrets.json
        cmd.arg("--tmpfs=/data/omegon:rw,size=1m");
        eprintln!(
            "   Secrets:   no vault found — falling back to env var forwarding\n   \
             Run `omegon auth login` to set up the encrypted secrets vault."
        );
        for key in &[
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "OPENROUTER_API_KEY",
            "GROQ_API_KEY",
            "XAI_API_KEY",
            "MISTRAL_API_KEY",
            "CEREBRAS_API_KEY",
            "GOOGLE_API_KEY",
            "PERPLEXITY_API_KEY",
            "GITHUB_TOKEN",
        ] {
            if let Ok(val) = std::env::var(key) {
                cmd.arg("-e");
                cmd.arg(format!("{key}={val}"));
            }
        }
    }

    // No keyring inside the container — use derived key for vault decryption
    cmd.arg("-e");
    cmd.arg("OMEGON_NO_KEYRING=1");

    // Mark that we're inside the sandbox (prevents infinite re-exec)
    cmd.arg("-e");
    cmd.arg("OMEGON_INSIDE_SANDBOX=1");
    cmd.arg("-e");
    cmd.arg("OMEGON_INSIDE_OCI=1");
    cmd.arg("-e");
    cmd.arg("OMEGON_RUNTIME_CONTEXT=host-shim-oci");
    cmd.arg("-e");
    cmd.arg("OMEGON_OCI_LAUNCHER=omegon");

    // Labels
    cmd.arg("--label=sh.styrene.omegon.sandboxed=true");

    // Image
    cmd.arg(&image);

    // Rebuild CLI args for the containerized omegon, stripping --sandboxed
    // and --cwd (cwd is always /work inside the container)
    let mut inner_args: Vec<String> = Vec::new();
    if let Some(ref prompt) = cli.prompt {
        inner_args.extend(["--prompt".into(), prompt.clone()]);
    }
    if let Some(ref pf) = cli.prompt_file {
        // Prompt file must be inside cwd to be accessible in the container
        let pf_rel = pf.strip_prefix(&cwd).unwrap_or(pf);
        inner_args.extend([
            "--prompt-file".into(),
            format!("/work/{}", pf_rel.display()),
        ]);
    }
    inner_args.extend(["--model".into(), cli.model.clone()]);
    inner_args.extend(["--max-turns".into(), cli.max_turns.to_string()]);
    if cli.slim {
        inner_args.push("--slim".into());
    }
    if cli.fresh {
        inner_args.push("--fresh".into());
    }
    if let Some(ref posture) = cli.posture {
        inner_args.extend(["--posture".into(), posture.clone()]);
    }
    if let Some(ref persona) = cli.persona {
        inner_args.extend(["--persona".into(), persona.clone()]);
    }
    if let Some(ref ctx) = cli.context_class {
        inner_args.extend(["--context-class".into(), ctx.clone()]);
    }

    for arg in &inner_args {
        cmd.arg(arg);
    }

    // Exec — replace this process with the container
    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::ConversationState;
    use clap::CommandFactory;
    use tempfile::tempdir;

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

    fn test_workspace_lease(cwd: &Path) -> crate::workspace::types::WorkspaceLease {
        use crate::workspace::types::{
            Mutability, WorkspaceBackendKind, WorkspaceBindings, WorkspaceKind, WorkspaceLease,
            WorkspaceRole,
        };

        WorkspaceLease {
            project_id: "test-project".into(),
            workspace_id: "test-workspace".into(),
            label: "test".into(),
            path: cwd.display().to_string(),
            backend_kind: WorkspaceBackendKind::LocalDir,
            vcs_ref: None,
            bindings: WorkspaceBindings::default(),
            branch: "main".into(),
            role: WorkspaceRole::Primary,
            workspace_kind: WorkspaceKind::Code,
            mutability: Mutability::Mutable,
            owner_session_id: Some("test-session".into()),
            owner_agent_id: Some("test-agent".into()),
            created_at: "2026-05-14T00:00:00Z".into(),
            last_heartbeat: "2026-05-14T00:00:00Z".into(),
            archived: false,
            archived_at: None,
            archive_reason: None,
            parent_workspace_id: None,
            source: "test".into(),
        }
    }

    fn test_agent_setup() -> setup::AgentSetup {
        let cwd = std::env::current_dir().expect("current dir");
        let secrets_dir =
            std::env::temp_dir().join(format!("omegon-test-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&secrets_dir).expect("test secrets dir");
        let secrets = std::sync::Arc::new(
            omegon_secrets::SecretsManager::new(&secrets_dir).expect("secrets manager"),
        );
        let workspace_lease = test_workspace_lease(&cwd);

        setup::AgentSetup {
            bus: crate::bus::EventBus::new(),
            session_id: "test-session".into(),
            instance_id: "test-instance".into(),
            startup_skill_activation_events: Vec::new(),
            context_metrics: crate::features::context::SharedContextMetrics::new(),
            command_tx: crate::features::context::new_shared_command_tx(),
            context_manager: crate::context::ContextManager::new("test prompt".into(), vec![]),
            conversation: crate::conversation::ConversationState::new(),
            cwd,
            secrets,
            web_auth_state: crate::web::WebAuthState::ephemeral_generated("test-token".into()),
            session_secret_env: vec![],
            startup_snapshot: setup::StartupSnapshot {
                total_facts: 0,
                lifecycle: setup::LifecycleSnapshot {
                    focused_node: None,
                    active_changes: vec![],
                },
            },
            skill_phases: vec![],
            dashboard_handles: crate::tui::dashboard::DashboardHandles::default(),
            initial_harness_status: crate::status::HarnessStatus::default(),
            resume_info: None,
            workspace_state: setup::WorkspaceStartupState {
                lease: workspace_lease,
                admission: crate::workspace::types::AdmissionOutcome::GrantedMutable,
            },
            extension_widgets: vec![],
            extension_metadata: Default::default(),
            extension_rpc_handles: Default::default(),
            widget_receivers: vec![],
            cleave_event_slot: std::sync::Arc::new(std::sync::Mutex::new(None)),
            delegate_event_slot: std::sync::Arc::new(std::sync::Mutex::new(None)),
            vox_polling_handles: vec![],
            voice_notification_receivers: vec![],
            voice_polling_handles: vec![],
        }
    }

    #[test]
    fn format_agent_error_extracts_message() {
        let raw = r#"Anthropic 400 Bad Request: {"type":"error","error":{"type":"invalid_request_error","message":"Input should be a valid dictionary"}}"#;
        let e = anyhow::anyhow!("{raw}");
        let result = format_agent_error(&e, None);
        assert!(
            result.contains("Input should be a valid dictionary"),
            "got: {result}"
        );
    }

    #[test]
    fn format_agent_error_truncates_long() {
        let long = "x".repeat(500);
        let e = anyhow::anyhow!("{long}");
        let result = format_agent_error(&e, None);
        assert!(
            result.len() < 600,
            "should truncate, got len {}",
            result.len()
        );
    }

    #[test]
    fn format_agent_error_extracts_status() {
        let e = anyhow::anyhow!("status=429 Too Many Requests blah blah");
        let result = format_agent_error(&e, None);
        assert!(result.contains("status=429"), "got: {result}");
    }

    #[test]
    fn format_agent_error_collapses_openai_provider_side_failures() {
        let e = anyhow::anyhow!("LLM error: Codex 520: error code: 520");
        let result = format_agent_error(&e, None);
        assert!(
            result.contains("Upstream error (OpenAI/Codex)"),
            "got: {result}"
        );
        assert!(result.contains("status.openai.com"), "got: {result}");
        assert!(!result.contains("error code: 520"), "got: {result}");
    }

    #[test]
    fn format_agent_error_codex_401_does_not_parrot_scope_message() {
        let e = anyhow::anyhow!(
            "LLM error: Codex 401: You have insufficient permissions for this operation. Missing scopes: api.responses.write"
        );
        let result = format_agent_error(&e, None);
        assert!(
            result.contains("Authentication error (OpenAI/Codex)"),
            "got: {result}"
        );
        assert!(
            result.contains("Re-authenticate with /login"),
            "got: {result}"
        );
        // Should NOT expose internal scope names to the user
        assert!(!result.contains("api.responses.write"), "got: {result}");
        assert!(
            !result.contains("insufficient permissions"),
            "got: {result}"
        );
    }

    #[test]
    fn format_agent_error_codex_expired_session_prefers_reauth() {
        let e = anyhow::anyhow!(
            "LLM error: Codex 401 Unauthorized: session expired, please log in again"
        );
        let result = format_agent_error(&e, None);
        assert!(
            result.contains("Authentication error (OpenAI/Codex)"),
            "got: {result}"
        );
        assert!(result.contains("session appears expired"), "got: {result}");
        assert!(!result.contains("please log in again"), "got: {result}");
    }

    #[test]
    fn format_agent_error_anthropic_stall_prefers_quota_pressure_when_recent_telemetry_is_tight() {
        let e = anyhow::anyhow!("Anthropic LLM stream idle for 90s — connection may be stalled");
        let telemetry = omegon_traits::ProviderTelemetrySnapshot {
            provider: "anthropic".into(),
            source: "response_headers".into(),
            unified_5h_utilization_pct: Some(97.0),
            unified_7d_utilization_pct: Some(83.0),
            ..Default::default()
        };
        let result = format_agent_error(&e, Some(&telemetry));
        assert!(
            result.contains("Provider pressure (Anthropic/Claude)"),
            "got: {result}"
        );
        assert!(
            result.contains("usage-window backpressure or exhaustion"),
            "got: {result}"
        );
        assert!(result.contains("5h 97%"), "got: {result}");
        assert!(!result.contains("provider-side failure"), "got: {result}");
    }

    #[test]
    fn format_agent_error_anthropic_quota_prefers_usage_limit_wording() {
        let e = anyhow::anyhow!("Anthropic quota exceeded for this workspace");
        let telemetry = omegon_traits::ProviderTelemetrySnapshot {
            provider: "anthropic".into(),
            source: "response_headers".into(),
            unified_5h_utilization_pct: Some(99.0),
            unified_7d_utilization_pct: Some(91.0),
            ..Default::default()
        };
        let result = format_agent_error(&e, Some(&telemetry));
        assert!(
            result.contains("Usage limit reached (Anthropic/Claude)"),
            "got: {result}"
        );
        assert!(
            result.contains("Anthropic usage window to reset"),
            "got: {result}"
        );
        assert!(!result.contains("provider-side failure"), "got: {result}");
    }

    #[test]
    fn interactive_resume_mode_defaults_to_fresh_session() {
        let cli = Cli::parse_from(["omegon"]);
        assert!(interactive_resume_mode(&cli).is_none());
    }

    #[test]
    fn interactive_resume_mode_resumes_most_recent_when_requested() {
        let cli = Cli::parse_from(["omegon", "--resume"]);
        assert_eq!(interactive_resume_mode(&cli), Some(None));
    }

    #[test]
    fn interactive_resume_mode_resumes_specific_session_when_requested() {
        let cli = Cli::parse_from(["omegon", "--resume", "abc123"]);
        assert_eq!(interactive_resume_mode(&cli), Some(Some("abc123")));
    }

    #[test]
    fn om_invocation_does_not_imply_slim_mode() {
        let cli = Cli::parse_from(["om"]);
        assert!(!cli_prefers_slim_mode(&cli));
    }

    #[test]
    fn explicit_slim_flag_enables_slim_mode() {
        let cli = Cli::parse_from(["om", "--slim"]);
        assert!(cli_prefers_slim_mode(&cli));
    }

    #[test]
    fn full_flag_does_not_erase_explicit_slim_flag_at_parse_time() {
        let cli = Cli::parse_from(["om", "--slim", "--full"]);
        assert!(cli_prefers_slim_mode(&cli));
        assert!(cli.full);
    }

    #[test]
    fn interactive_resume_mode_fresh_overrides_resume_flag() {
        let cli = Cli::parse_from(["omegon", "--fresh", "--resume", "abc123"]);
        assert!(interactive_resume_mode(&cli).is_none());
    }

    #[test]
    fn prompt_envelope_requires_over_and_out_for_voice_close_request() {
        let prompt = PromptEnvelope {
            id: 1,
            text: "🎙 stop listening".to_string(),
            image_paths: Vec::new(),
            submitted_by: RuntimeActor::tui(),
            via: ControlSurface::Tui,
            metadata: tui::PromptMetadata {
                voice: Some(tui::VoicePromptMetadata {
                    event_id: "u-close".to_string(),
                    duration_s: None,
                    radio_cue: Some("over_and_out".to_string()),
                    end_of_turn: Some(true),
                    close_session_requested: Some(true),
                }),
            },
            queue_mode: QueueMode::UntilReady,
        };
        assert!(prompt.requests_voice_close());

        let malformed_close = PromptEnvelope {
            metadata: tui::PromptMetadata {
                voice: Some(tui::VoicePromptMetadata {
                    event_id: "u-close".to_string(),
                    duration_s: None,
                    radio_cue: Some("over".to_string()),
                    end_of_turn: Some(true),
                    close_session_requested: Some(true),
                }),
            },
            ..prompt
        };
        assert!(!malformed_close.requests_voice_close());
    }

    #[test]
    fn interactive_runtime_supervisor_preserves_prompt_metadata() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        let metadata = tui::PromptMetadata {
            voice: Some(tui::VoicePromptMetadata {
                event_id: "u-close".to_string(),
                duration_s: Some(2.1),
                radio_cue: Some("over_and_out".to_string()),
                end_of_turn: Some(true),
                close_session_requested: Some(true),
            }),
        };
        supervisor.enqueue_prompt(
            "🎙 stop listening".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
            metadata.clone(),
            None,
        );

        let active = supervisor.maybe_start_next_turn().expect("active turn");
        assert_eq!(active.prompt.metadata, metadata);
    }

    #[test]
    fn interactive_runtime_supervisor_starts_first_prompt_fifo() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
            tui::PromptMetadata::default(),
            None,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            Vec::new(),
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
            tui::PromptMetadata::default(),
            None,
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
            tui::PromptMetadata::default(),
            None,
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
            tui::PromptMetadata::default(),
            None,
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
            tui::PromptMetadata::default(),
            None,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            vec![PathBuf::from("/tmp/paste.png")],
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
            tui::PromptMetadata::default(),
            None,
        );

        supervisor
            .maybe_start_next_turn()
            .expect("first active turn");
        supervisor.complete_active_turn().expect("first completion");
        let active = supervisor
            .maybe_start_next_turn()
            .expect("second queued prompt should start");

        assert_eq!(active.runtime_turn_id, 2);
        assert_eq!(active.prompt.text, "second");
        assert_eq!(
            active.prompt.image_paths,
            vec![PathBuf::from("/tmp/paste.png")]
        );
        assert_eq!(active.prompt.submitted_by.kind, RuntimeActorKind::Auspex);
        assert_eq!(supervisor.queue_depth(), 0);
    }

    #[test]
    fn conversation_rolls_back_last_user_after_provider_error() {
        let mut conversation = ConversationState::new();
        conversation.push_user("bad replay".to_string());

        assert!(conversation.rollback_last_user_if_text("bad replay"));

        assert_eq!(conversation.last_user_prompt(), "");
    }

    #[test]
    fn conversation_does_not_roll_back_non_matching_user_after_provider_error() {
        let mut conversation = ConversationState::new();
        conversation.push_user("keep me".to_string());

        assert!(!conversation.rollback_last_user_if_text("different prompt"));

        assert_eq!(conversation.last_user_prompt(), "keep me");
    }

    #[test]
    fn interactive_runtime_supervisor_cancel_then_complete_starts_next_queued_turn() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
            tui::PromptMetadata::default(),
            None,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            Vec::new(),
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
            tui::PromptMetadata::default(),
            None,
        );

        supervisor
            .maybe_start_next_turn()
            .expect("first active turn");
        supervisor.request_cancel(RuntimeActor::tui(), ControlSurface::Tui);
        let completed = supervisor.complete_active_turn().expect("completed turn");
        assert_eq!(completed.prompt.text, "first");

        let next = supervisor
            .maybe_start_next_turn()
            .expect("queued prompt should start after cancelled turn completes");
        assert_eq!(next.runtime_turn_id, 2);
        assert_eq!(next.prompt.text, "second");
    }

    #[test]
    fn interactive_runtime_supervisor_queue_notification_reports_authoritative_depth() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        let (events_tx, mut events_rx) = broadcast::channel(4);
        let prompt_id = supervisor.enqueue_prompt(
            "follow up".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
            tui::PromptMetadata::default(),
            None,
        );

        emit_runtime_queue_notification(&supervisor, &events_tx, prompt_id);

        match events_rx.try_recv().expect("queue snapshot") {
            AgentEvent::RuntimeQueueUpdated { snapshot_json } => {
                assert_eq!(snapshot_json["depth"], 1);
                assert_eq!(snapshot_json["items"][0]["id"], 1);
                assert_eq!(snapshot_json["items"][0]["submitted_by"], "local-tui");
                assert_eq!(snapshot_json["items"][0]["via"], "tui");
            }
            other => panic!("expected runtime queue update, got {other:?}"),
        }

        match events_rx.try_recv().expect("queue notification") {
            AgentEvent::SystemNotification { message } => {
                assert!(message.contains("Queued prompt #1"), "{message}");
                assert!(message.contains("local-tui"), "{message}");
                assert!(message.contains("tui"), "{message}");
                assert!(message.contains("queue depth 1"), "{message}");
            }
            other => panic!("expected system notification, got {other:?}"),
        }
    }

    #[test]
    fn interactive_runtime_supervisor_quit_semantics_map_to_cancel_then_stop() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
            tui::PromptMetadata::default(),
            None,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            Vec::new(),
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
            tui::PromptMetadata::default(),
            None,
        );
        supervisor
            .maybe_start_next_turn()
            .expect("first active turn");

        let active = supervisor
            .request_cancel(RuntimeActor::tui(), ControlSurface::Tui)
            .expect("quit should target active turn");
        assert!(matches!(active.phase, ActiveTurnPhase::Cancelling { .. }));
        assert_eq!(
            supervisor.queue_depth(),
            1,
            "quit should not drop queued prompts implicitly"
        );
        assert!(
            supervisor.is_busy(),
            "quit requests cancellation but active turn remains busy until completion"
        );
    }

    #[test]
    fn runtime_queue_snapshot_updates_on_start_and_completion() {
        let mut supervisor = InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "first".to_string(),
            Vec::new(),
            RuntimeActor::tui(),
            ControlSurface::Tui,
            tui::PromptMetadata::default(),
            None,
        );
        supervisor.enqueue_prompt(
            "second".to_string(),
            Vec::new(),
            RuntimeActor::auspex(),
            ControlSurface::Ipc,
            tui::PromptMetadata::default(),
            None,
        );

        let active = supervisor.maybe_start_next_turn().expect("active turn");
        assert_eq!(active.prompt.text, "first");
        let snapshot = supervisor.queue_snapshot_json();
        assert_eq!(snapshot["depth"], 1);
        assert_eq!(snapshot["active"]["phase"], "running");
        assert_eq!(snapshot["items"][0]["preview"], "second");

        supervisor.complete_active_turn().expect("completed turn");
        let snapshot = supervisor.queue_snapshot_json();
        assert_eq!(snapshot["depth"], 1);
        assert!(snapshot["active"].is_null());

        let active = supervisor.maybe_start_next_turn().expect("second turn");
        assert_eq!(active.prompt.text, "second");
        let snapshot = supervisor.queue_snapshot_json();
        assert_eq!(snapshot["depth"], 0);
        assert_eq!(snapshot["active"]["prompt_id"], 2);
    }

    #[tokio::test]
    async fn split_interactive_agent_moves_runtime_state_and_preserves_host_metadata() {
        let agent = test_agent_setup();
        let expected_session_id = agent.session_id.clone();
        let expected_cwd = agent.cwd.clone();
        let expected_resume = agent.resume_info.as_ref().map(|r| r.session_id.clone());
        let expected_message_count = agent.conversation.message_count();
        let expected_tool_count = agent.bus.tool_definitions().len();

        let (host, runtime_state) = split_interactive_agent(agent);

        assert_eq!(host.session_id, expected_session_id);
        assert_eq!(host.cwd, expected_cwd);
        assert_eq!(
            host.resume_info.as_ref().map(|r| r.session_id.clone()),
            expected_resume
        );
        assert_eq!(
            runtime_state.conversation.message_count(),
            expected_message_count
        );
        assert_eq!(
            runtime_state.bus.tool_definitions().len(),
            expected_tool_count
        );
    }

    #[tokio::test]
    async fn split_interactive_agent_keeps_runtime_state_mutable_after_split() {
        let agent = test_agent_setup();
        let expected_cwd = agent.cwd.clone();
        let (host, mut runtime_state) = split_interactive_agent(agent);

        runtime_state
            .conversation
            .push_user("hello from runtime state".to_string());
        let system_prompt = runtime_state.context_manager.build_system_prompt(
            runtime_state.conversation.last_user_prompt(),
            &runtime_state.conversation,
        );

        assert_eq!(host.cwd, expected_cwd);
        assert!(
            runtime_state.conversation.message_count() >= 1,
            "conversation should remain writable after split"
        );
        assert!(
            !system_prompt.is_empty(),
            "context manager should still build prompts after split"
        );
    }

    #[test]
    fn mark_interactive_session_busy_updates_dashboard_flag() {
        let handles = crate::tui::dashboard::DashboardHandles::default();
        mark_interactive_session_busy(&handles, true);
        assert!(handles.session.lock().expect("session lock").busy);
        mark_interactive_session_busy(&handles, false);
        assert!(!handles.session.lock().expect("session lock").busy);
    }

    #[test]
    fn format_interactive_turn_task_failure_reports_safe_shutdown() {
        struct FakeJoinError(String);
        impl std::fmt::Display for FakeJoinError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        let text = FakeJoinError("boom".to_string()).to_string();
        let message = format!(
            "⚠ Interactive turn worker crashed — ending session safely: {}",
            text
        );
        assert!(
            message.contains("Interactive turn worker crashed"),
            "got: {message}"
        );
        assert!(message.contains("ending session safely"), "got: {message}");
        assert!(message.contains("boom"), "got: {message}");
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
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
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
        let output = response.output.unwrap();
        assert!(
            output.contains("requires interactive confirmation"),
            "got: {output}"
        );
    }

    #[test]
    fn remote_slash_focus_is_blocked_by_registry_availability() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "focus",
            "on",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("interactive-only or unavailable")
        );
    }

    #[test]
    fn remote_slash_stats_is_allowed_by_registry_availability() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "stats",
            "",
        ));

        assert!(response.accepted);
        assert!(response.output.unwrap().contains("Session Overview"));
    }

    #[test]
    fn remote_slash_skills_help_is_allowed_without_bypass() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "skills",
            "--help",
        ));

        assert!(response.accepted);
        let output = response.output.unwrap();
        assert!(output.contains("Usage: /skills"), "got: {output}");
        assert!(
            output.contains("/skills opens the active skills inventory menu"),
            "got: {output}"
        );
    }

    #[test]
    fn remote_slash_skills_install_requires_bypass() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "skills",
            "install code-act",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("requires interactive confirmation")
        );
    }

    #[test]
    fn remote_slash_skills_create_is_interactive_only() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "skills",
            "create --project",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("interactive-only or unavailable")
        );
    }

    #[test]
    fn remote_slash_auth_login_requires_bypass_by_builtin_policy() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "auth",
            "login anthropic",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("requires interactive confirmation")
        );
    }

    #[test]
    fn remote_slash_logout_requires_provider() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "logout",
            "",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("interactive-only or unavailable")
        );
    }

    struct EnvVarGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let original = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.original {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn remote_slash_logout_accepts_openai_codex_provider() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        let _auth_env = EnvVarGuard::set_path("OMEGON_AUTH_JSON_PATH", &auth_path);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "logout",
            "openai-codex",
        ));

        assert!(response.accepted);
        let output = response.output.unwrap();
        assert!(output.contains("Logged out"), "got: {output}");
        assert!(
            output.contains("OpenAI/Codex") || output.contains("openai-codex"),
            "got: {output}"
        );
        assert!(
            output.contains("cleared this session's cached auth env"),
            "got: {output}"
        );
    }

    #[test]
    fn remote_slash_logout_rejects_unknown_provider_with_supported_list() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = Cli::try_parse_from(vec!["omegon"]).unwrap();

        let (mut agent, mut runtime_state) = split_interactive_agent(agent);

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "logout",
            "not-a-provider",
        ));

        assert!(!response.accepted);
        let output = response.output.unwrap();
        assert!(output.contains("Unknown provider"), "got: {output}");
        assert!(output.contains("anthropic"), "got: {output}");
        assert!(output.contains("openai-codex"), "got: {output}");
        assert!(!output.contains("ollama,"), "got: {output}");
    }

    struct TestRemoteCommandFeature {
        definition: omegon_traits::CommandDefinition,
        handled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl omegon_traits::Feature for TestRemoteCommandFeature {
        fn name(&self) -> &str {
            "test-remote-command"
        }

        fn commands(&self) -> Vec<omegon_traits::CommandDefinition> {
            vec![self.definition.clone()]
        }

        fn handle_command(&mut self, name: &str, args: &str) -> omegon_traits::CommandResult {
            if name == self.definition.name {
                self.handled
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                omegon_traits::CommandResult::Display(format!("handled {args}"))
            } else {
                omegon_traits::CommandResult::NotHandled
            }
        }
    }

    fn remote_command_definition(
        availability: omegon_traits::CommandAvailability,
        requires_confirmation: bool,
    ) -> omegon_traits::CommandDefinition {
        omegon_traits::CommandDefinition {
            name: "unsafe_test".into(),
            description: "test command".into(),
            subcommands: vec![],
            availability,
            safety: omegon_traits::CommandSafety {
                class: omegon_traits::CommandSafetyClass::StateChanging,
                requires_confirmation,
                prompt_injection_sensitive: false,
            },
        }
    }

    fn register_remote_test_command(
        runtime_state: &mut InteractiveAgentState,
        availability: omegon_traits::CommandAvailability,
        requires_confirmation: bool,
    ) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let handled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        runtime_state
            .bus
            .register(Box::new(TestRemoteCommandFeature {
                definition: remote_command_definition(availability, requires_confirmation),
                handled: handled.clone(),
            }));
        runtime_state.bus.finalize();
        handled
    }

    #[allow(clippy::type_complexity)]
    fn remote_command_test_context(
        bypass: bool,
    ) -> (
        tokio::runtime::Runtime,
        InteractiveAgentHost,
        InteractiveAgentState,
        broadcast::Sender<AgentEvent>,
        settings::SharedSettings,
        std::sync::Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
        std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
        Cli,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let agent = test_agent_setup();
        let (events_tx, _) = broadcast::channel(16);
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "anthropic:claude-sonnet-4-6",
        )));
        let bridge = std::sync::Arc::new(tokio::sync::RwLock::new(Box::new(
            crate::bridge::NullBridge,
        ) as Box<dyn LlmBridge>));
        let login_prompt_tx = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let cli = if bypass {
            Cli::try_parse_from(vec!["omegon", "--dangerously-bypass-permissions"]).unwrap()
        } else {
            Cli::try_parse_from(vec!["omegon"]).unwrap()
        };
        let (agent, runtime_state) = split_interactive_agent(agent);
        (
            rt,
            agent,
            runtime_state,
            events_tx,
            shared_settings,
            bridge,
            login_prompt_tx,
            cli,
        )
    }

    #[test]
    fn remote_registered_command_requires_confirmation_without_bypass() {
        let (
            rt,
            mut agent,
            mut runtime_state,
            events_tx,
            shared_settings,
            bridge,
            login_prompt_tx,
            cli,
        ) = remote_command_test_context(false);
        let handled = register_remote_test_command(
            &mut runtime_state,
            omegon_traits::CommandAvailability::ALL,
            true,
        );

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "unsafe_test",
            "args",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("requires interactive confirmation")
        );
        assert!(!handled.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn remote_registered_command_bypass_allows_confirmation_required_command() {
        let (
            rt,
            mut agent,
            mut runtime_state,
            events_tx,
            shared_settings,
            bridge,
            login_prompt_tx,
            cli,
        ) = remote_command_test_context(true);
        let handled = register_remote_test_command(
            &mut runtime_state,
            omegon_traits::CommandAvailability::ALL,
            true,
        );

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "unsafe_test",
            "args",
        ));

        assert!(response.accepted);
        assert_eq!(response.output.as_deref(), Some("handled args"));
        assert!(handled.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn remote_registered_command_availability_is_not_bypassed() {
        let (
            rt,
            mut agent,
            mut runtime_state,
            events_tx,
            shared_settings,
            bridge,
            login_prompt_tx,
            cli,
        ) = remote_command_test_context(true);
        let handled = register_remote_test_command(
            &mut runtime_state,
            omegon_traits::CommandAvailability {
                tui: true,
                cli: false,
                acp: true,
            },
            true,
        );

        let response = rt.block_on(execute_remote_slash_command(
            &mut runtime_state,
            &mut agent,
            &events_tx,
            &shared_settings,
            &bridge,
            &login_prompt_tx,
            &cli,
            "unsafe_test",
            "args",
        ));

        assert!(!response.accepted);
        assert!(
            response
                .output
                .unwrap()
                .contains("not available via CLI/remote slash execution")
        );
        assert!(!handled.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn provider_connected_helper_rejects_unknown_model_provider() {
        assert!(!auth::provider_connected_for_model(
            "nonexistent-provider:test-model"
        ));
    }

    #[test]
    fn interactive_startup_uses_selected_model_when_available() {
        let decision =
            decide_interactive_startup_model("openai-codex:gpt-5.5", "openai-codex:gpt-5.5", true);

        assert_eq!(decision.selected_model, "openai-codex:gpt-5.5");
        assert_eq!(decision.bridge_model, "openai-codex:gpt-5.5");
        assert!(decision.provider_connected);
        assert!(!decision.use_null_bridge);
    }

    #[test]
    fn interactive_startup_preserves_selected_model_when_route_resolves_differently() {
        let decision =
            decide_interactive_startup_model("openai:gpt-5.5", "openai-codex:gpt-5.5", true);

        assert_eq!(decision.selected_model, "openai:gpt-5.5");
        assert_eq!(decision.bridge_model, "openai-codex:gpt-5.5");
        assert!(decision.provider_connected);
        assert!(!decision.use_null_bridge);
    }

    #[test]
    fn interactive_startup_does_not_replace_unavailable_profile_model_with_safe_fallback() {
        let decision =
            decide_interactive_startup_model("openai-codex:gpt-5.5", "openai-codex:gpt-5.5", false);

        assert_eq!(decision.selected_model, "openai-codex:gpt-5.5");
        assert_eq!(decision.bridge_model, "openai-codex:gpt-5.5");
        assert!(!decision.provider_connected);
        assert!(decision.use_null_bridge);
    }

    #[test]
    fn interactive_loop_config_uses_selected_model_after_startup_resolution() {
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "openai-codex:gpt-5.5",
        )));
        {
            let mut s = shared_settings.lock().unwrap();
            let decision = decide_interactive_startup_model(
                "openai-codex:gpt-5.5",
                "openai-codex:gpt-5.5",
                false,
            );
            s.provider_connected = decision.provider_connected;
        }
        let secrets_dir = tempfile::tempdir().unwrap();
        let runtime = InteractiveRuntimeResources {
            cwd: PathBuf::from("."),
            secrets: std::sync::Arc::new(
                omegon_secrets::SecretsManager::new(secrets_dir.path()).unwrap(),
            ),
            context_metrics: crate::features::context::SharedContextMetrics::new(),
            bridge_model: std::sync::Arc::new(std::sync::Mutex::new(None)),
            route_controller: Arc::new(route::RouteController::new(
                route::ProviderRoute::Disconnected {
                    selected: "openai-codex:gpt-5.5".into(),
                    reason: route::DisconnectedReason::ProviderUnavailable {
                        provider: "openai-codex".into(),
                        detail: "test".into(),
                    },
                },
                Box::new(bridge::NullBridge),
                None,
            )),
        };
        let pending_compact = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let loop_config =
            build_interactive_loop_config(&runtime, &shared_settings, &pending_compact);

        assert_eq!(loop_config.model, "openai-codex:gpt-5.5");
        assert!(!shared_settings.lock().unwrap().provider_connected);
    }

    #[test]
    fn interactive_loop_config_preserves_bridge_fallback_model_separately() {
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::new(
            "openai-codex:gpt-5.5",
        )));
        let secrets_dir = tempfile::tempdir().unwrap();
        let runtime = InteractiveRuntimeResources {
            cwd: PathBuf::from("."),
            secrets: std::sync::Arc::new(
                omegon_secrets::SecretsManager::new(secrets_dir.path()).unwrap(),
            ),
            context_metrics: crate::features::context::SharedContextMetrics::new(),
            bridge_model: std::sync::Arc::new(std::sync::Mutex::new(Some(
                "anthropic:claude-fable-5".to_string(),
            ))),
            route_controller: Arc::new(route::RouteController::new(
                route::ProviderRoute::Fallback {
                    selected: "openai-codex:gpt-5.5".into(),
                    serving: "anthropic:claude-fable-5".into(),
                    reason: route::FallbackReason::MissingCredentials {
                        provider: "openai-codex".into(),
                    },
                },
                Box::new(bridge::NullBridge),
                None,
            )),
        };
        let pending_compact = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let loop_config =
            build_interactive_loop_config(&runtime, &shared_settings, &pending_compact);

        assert_eq!(loop_config.model, "openai-codex:gpt-5.5");
        assert_eq!(
            loop_config.bridge_model.as_deref(),
            Some("anthropic:claude-fable-5")
        );
        assert_eq!(
            shared_settings.lock().unwrap().model,
            "openai-codex:gpt-5.5"
        );
    }

    #[test]
    fn plan_view_returns_last_completed_plan_when_no_active_plan_exists() {
        let mut runtime_state = InteractiveAgentState {
            bus: crate::bus::EventBus::new(),
            context_manager: crate::context::ContextManager::new(String::new(), Vec::new()),
            conversation: crate::conversation::ConversationState::new(),
        };
        runtime_state
            .conversation
            .intent
            .set_work_plan(vec!["recover completed plan".into()]);
        runtime_state.conversation.intent.advance_work_plan();
        runtime_state.conversation.intent.clear_work_plan();

        let response = execute_plan_slash_command(
            &mut runtime_state,
            crate::tui::CanonicalSlashCommand::PlanView,
        );

        assert!(response.accepted);
        let output = response.output.unwrap();
        assert!(output.contains("Plan mode: complete"), "{output}");
        assert!(output.contains("recover completed plan"), "{output}");
    }

    #[test]
    fn plan_list_renders_visible_completed_and_openspec_sections() {
        let cwd = tempfile::tempdir().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(cwd.path()).unwrap();
        std::fs::create_dir_all("openspec/changes/example/specs/lifecycle").unwrap();
        std::fs::write("openspec/changes/example/proposal.md", "# Example\n").unwrap();
        std::fs::write(
            "openspec/changes/example/tasks.md",
            "# Tasks\n\n## 1. Runtime\n<!-- specs: lifecycle/example -->\n\n- [x] 1.1 Done\n- [ ] 1.2 Pending\n",
        )
        .unwrap();

        let mut runtime_state = InteractiveAgentState {
            bus: crate::bus::EventBus::new(),
            context_manager: crate::context::ContextManager::new(String::new(), Vec::new()),
            conversation: crate::conversation::ConversationState::new(),
        };
        runtime_state
            .conversation
            .intent
            .set_work_plan(vec!["visible work".into()]);

        let output = render_plan_list(&runtime_state);
        std::env::set_current_dir(previous_cwd).unwrap();

        assert!(output.contains("Visible"), "{output}");
        assert!(output.contains("visible work"), "{output}");
        assert!(output.contains("OpenSpec"), "{output}");
        assert!(output.contains("example"), "{output}");
        assert!(output.contains("1/2"), "{output}");
        assert!(output.contains("Done"), "{output}");
        assert!(output.contains("Pending"), "{output}");
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
                tls,
                ..
            } => {
                assert_eq!(control_port, 7842);
                assert!(strict_port);
                assert!(tls.cert.is_none());
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
                agent: _,
                tls,
                ..
            } => {
                assert_eq!(control_port, 7842);
                assert!(strict_port);
                assert!(tls.key.is_none());
            }
            _ => panic!("Expected Serve command"),
        }
    }

    #[test]
    fn control_tls_flags_parse_for_serve_and_acp() {
        let cli = Cli::try_parse_from(vec![
            "omegon",
            "serve",
            "--rpc-tls-cert",
            "server.pem",
            "--rpc-tls-key",
            "server-key.pem",
            "--rpc-tls-client-ca",
            "ca.pem",
        ])
        .expect("should parse serve TLS flags");

        match cli.command.unwrap() {
            Commands::Serve { tls, .. } => {
                let config = tls.into_config().expect("valid TLS config").unwrap();
                assert_eq!(config.cert_chain_path, PathBuf::from("server.pem"));
                assert_eq!(config.private_key_path, PathBuf::from("server-key.pem"));
                assert_eq!(config.client_ca_path, Some(PathBuf::from("ca.pem")));
            }
            _ => panic!("Expected Serve command"),
        }

        let cli = Cli::try_parse_from(vec![
            "omegon",
            "acp",
            "--listen",
            "127.0.0.1:0",
            "--control-tls-cert",
            "server.pem",
            "--control-tls-key",
            "server-key.pem",
        ])
        .expect("should parse ACP TLS aliases");

        match cli.command.unwrap() {
            Commands::Acp { tls, .. } => {
                let config = tls.into_config().expect("valid TLS config").unwrap();
                assert_eq!(config.cert_chain_path, PathBuf::from("server.pem"));
                assert_eq!(config.private_key_path, PathBuf::from("server-key.pem"));
                assert_eq!(config.client_ca_path, None);
            }
            _ => panic!("Expected ACP command"),
        }
    }

    #[test]
    fn control_tls_requires_cert_and_key_pair() {
        let cli = Cli::try_parse_from(vec!["omegon", "serve", "--rpc-tls-cert", "server.pem"])
            .expect("clap accepts partial TLS args for semantic validation");

        match cli.command.unwrap() {
            Commands::Serve { tls, .. } => {
                let err = tls.into_config().unwrap_err();
                assert!(err.to_string().contains("must be provided together"));
            }
            _ => panic!("Expected Serve command"),
        }
    }

    #[test]
    fn oci_alias_sets_sandboxed_and_accepts_overrides() {
        let cli = Cli::try_parse_from(vec![
            "omegon",
            "--oci",
            "--oci-image",
            "ghcr.io/styrene-lab/omegon-full:0.27.0-local",
            "--oci-runtime",
            "podman",
            "--prompt",
            "hello",
        ])
        .expect("--oci alias and override flags should parse");

        assert!(cli.sandboxed);
        assert_eq!(
            cli.oci_image.as_deref(),
            Some("ghcr.io/styrene-lab/omegon-full:0.27.0-local")
        );
        assert_eq!(cli.oci_runtime.as_deref(), Some("podman"));
    }

    #[test]
    fn oci_conflicts_with_dangerous_host_bypass() {
        let err = match Cli::try_parse_from(vec![
            "omegon",
            "--oci",
            "--dangerously-bypass-permissions",
        ]) {
            Ok(_) => panic!("OCI boundary must conflict with host permission bypass"),
            Err(err) => err,
        };

        let message = err.to_string();
        assert!(
            message.contains("dangerously-bypass-permissions") || message.contains("oci"),
            "unexpected clap error: {message}"
        );
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
    fn cleave_merge_result_reports_salvaged_merge_honestly() {
        let child = cleave::state::ChildState {
            child_id: 0,
            label: "noop-docs".to_string(),
            description: String::new(),
            scope: vec![],
            depends_on: vec![],
            status: cleave::state::ChildStatus::Completed,
            error: Some("merged after salvaging work from a failed child".to_string()),
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
            &cleave::orchestrator::MergeOutcome::Success,
        );
        assert!(
            line.contains("salvaged and merged after failure"),
            "unexpected line: {line}"
        );
        assert!(
            !line.contains("✓ noop-docs merged"),
            "line should not flatten salvaged merge into plain success: {line}"
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
            "--slim",
        ])
        .expect("bench run-task should parse");

        match cli.command.unwrap() {
            Commands::Bench {
                action:
                    BenchAction::RunTask {
                        prompt,
                        usage_json,
                        slim,
                    },
            } => {
                assert_eq!(prompt, "benchmark prompt");
                assert_eq!(usage_json, PathBuf::from("usage.json"));
                assert!(slim);
            }
            _ => panic!("wrong command parsed"),
        }
    }

    #[test]
    fn headless_benchmark_settings_enable_slim_mode_from_cli() {
        let cli = Cli::try_parse_from([
            "omegon",
            "--slim",
            "bench",
            "run-task",
            "--prompt",
            "benchmark prompt",
            "--usage-json",
            "usage.json",
        ])
        .expect("bench run-task should parse with --slim");

        let shared_settings = settings::shared(&cli.model);
        let profile = settings::Profile::load(&cli.cwd);
        {
            let mut s = shared_settings.lock().unwrap();
            profile.apply_to(&mut s);
            s.set_model(&cli.model);
            if cli.slim {
                s.set_posture(settings::PosturePreset::Explorator);
            }
        }
        let s = shared_settings.lock().unwrap();
        assert!(s.is_slim());
        assert_eq!(s.thinking, crate::settings::ThinkingLevel::Minimal);
        assert_eq!(
            s.requested_context_class,
            Some(crate::settings::ContextClass::Compact)
        );
    }

    #[test]
    fn headless_benchmark_settings_preserve_explicit_cli_model_over_profile() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".omegon")).unwrap();
        std::fs::write(
            dir.path().join(".omegon/profile.json"),
            r#"{
  "lastUsedModel": {
    "provider": "openai",
    "modelId": "gpt-4.1"
  },
  "thinkingLevel": "high",
  "maxTurns": 17
}"#,
        )
        .unwrap();

        let cli = Cli::try_parse_from([
            "omegon",
            "bench",
            "run-task",
            "--cwd",
            dir.path().to_str().unwrap(),
            "--model",
            "anthropic:claude-sonnet-4-6",
            "--prompt",
            "benchmark prompt",
            "--usage-json",
            "usage.json",
        ])
        .expect("bench run-task should parse");

        let shared_settings = settings::shared(&cli.model);
        let profile = settings::Profile::load(&cli.cwd);
        {
            let mut s = shared_settings.lock().unwrap();
            profile.apply_to(&mut s);
            s.set_model(&cli.model);
            if cli.max_turns != 50 {
                s.max_turns = cli.max_turns;
            }
        }

        let s = shared_settings.lock().unwrap();
        assert_eq!(s.model, "anthropic:claude-sonnet-4-6");
        assert_eq!(s.thinking, crate::settings::ThinkingLevel::High);
        assert_eq!(s.max_turns, 17);
    }

    #[test]
    fn benchmark_usage_summary_accumulates_run_totals() {
        let mut summary = BenchmarkUsageSummary {
            requested_model: Some("anthropic:claude-sonnet-4-6".into()),
            requested_provider: Some("anthropic".into()),
            resolved_provider: Some("anthropic".into()),
            ..BenchmarkUsageSummary::default()
        };
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            omegon_traits::TurnEndReason::AssistantCompleted,
            321,
            Some(omegon_traits::OodaPhase::Act),
            None,
            None,
            200_000,
            omegon_traits::ContextComposition {
                system_tokens: 100,
                tool_schema_tokens: 50,
                conversation_tokens: 75,
                memory_tokens: 10,
                tool_history_tokens: 20,
                thinking_tokens: 30,
                free_tokens: 199_715,
                ..Default::default()
            },
            123,
            45,
            6,
            2,
            None,
        );
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            omegon_traits::TurnEndReason::ToolContinuation,
            111,
            Some(omegon_traits::OodaPhase::Observe),
            Some(omegon_traits::DriftKind::OrientationChurn),
            Some(omegon_traits::ProgressNudgeReason::AntiOrientation),
            200_000,
            omegon_traits::ContextComposition {
                system_tokens: 120,
                tool_schema_tokens: 55,
                conversation_tokens: 90,
                memory_tokens: 12,
                tool_history_tokens: 24,
                thinking_tokens: 36,
                free_tokens: 199_663,
                ..Default::default()
            },
            77,
            9,
            4,
            3,
            None,
        );

        assert_eq!(summary.turn_count, 2);
        assert_eq!(
            summary.requested_model.as_deref(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(summary.requested_provider.as_deref(), Some("anthropic"));
        assert_eq!(summary.resolved_provider.as_deref(), Some("anthropic"));
        assert_eq!(summary.input_tokens, 200);
        assert_eq!(summary.output_tokens, 54);
        assert_eq!(summary.cache_tokens, 10);
        assert_eq!(summary.cache_write_tokens, 5);
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
                ..Default::default()
            }
        );
    }

    #[test]
    fn benchmark_usage_summary_ignores_empty_context_snapshots() {
        let mut summary = BenchmarkUsageSummary::default();
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            omegon_traits::TurnEndReason::AssistantCompleted,
            100,
            Some(omegon_traits::OodaPhase::Observe),
            None,
            None,
            1_000_000,
            omegon_traits::ContextComposition {
                system_tokens: 101,
                tool_schema_tokens: 202,
                conversation_tokens: 303,
                memory_tokens: 404,
                tool_history_tokens: 505,
                thinking_tokens: 606,
                free_tokens: 997_879,
                ..Default::default()
            },
            1,
            2,
            3,
            4,
            None,
        );
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            omegon_traits::TurnEndReason::ToolContinuation,
            200,
            Some(omegon_traits::OodaPhase::Observe),
            Some(omegon_traits::DriftKind::OrientationChurn),
            Some(omegon_traits::ProgressNudgeReason::AntiOrientation),
            1_000_000,
            omegon_traits::ContextComposition {
                free_tokens: 1_000_000,
                ..Default::default()
            },
            4,
            5,
            6,
            7,
            None,
        );

        assert_eq!(
            summary.context_composition,
            omegon_traits::ContextComposition {
                system_tokens: 101,
                tool_schema_tokens: 202,
                conversation_tokens: 303,
                memory_tokens: 404,
                tool_history_tokens: 505,
                thinking_tokens: 606,
                free_tokens: 997_879,
                ..Default::default()
            }
        );
    }

    #[test]
    fn benchmark_usage_summary_keeps_latest_context_snapshot() {
        let mut summary = BenchmarkUsageSummary::default();
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            omegon_traits::TurnEndReason::AssistantCompleted,
            100,
            Some(omegon_traits::OodaPhase::Observe),
            None,
            None,
            1_000_000,
            omegon_traits::ContextComposition {
                system_tokens: 10,
                tool_schema_tokens: 20,
                conversation_tokens: 30,
                memory_tokens: 40,
                tool_history_tokens: 50,
                thinking_tokens: 60,
                free_tokens: 999_790,
                ..Default::default()
            },
            1,
            2,
            3,
            4,
            None,
        );
        summary.observe_turn(
            Some("anthropic:claude-sonnet-4-6".into()),
            Some("anthropic".into()),
            omegon_traits::TurnEndReason::ProgressNudge,
            200,
            None,
            Some(omegon_traits::DriftKind::ClosureStall),
            Some(omegon_traits::ProgressNudgeReason::CommitHygiene),
            1_000_000,
            omegon_traits::ContextComposition {
                system_tokens: 101,
                tool_schema_tokens: 202,
                conversation_tokens: 303,
                memory_tokens: 404,
                tool_history_tokens: 505,
                thinking_tokens: 606,
                free_tokens: 997_879,
                ..Default::default()
            },
            4,
            5,
            6,
            7,
            None,
        );

        assert_eq!(
            summary.context_composition,
            omegon_traits::ContextComposition {
                system_tokens: 101,
                tool_schema_tokens: 202,
                conversation_tokens: 303,
                memory_tokens: 404,
                tool_history_tokens: 505,
                thinking_tokens: 606,
                free_tokens: 997_879,
                ..Default::default()
            }
        );
    }

    #[test]
    fn benchmark_usage_json_writer_persists_summary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bench").join("usage.json");
        let summary = BenchmarkUsageSummary {
            requested_model: Some("anthropic:claude-sonnet-4-6".into()),
            requested_provider: Some("anthropic".into()),
            resolved_provider: Some("anthropic".into()),
            model: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            dominant_phases: std::collections::BTreeMap::new(),
            drift_kinds: std::collections::BTreeMap::new(),
            progress_nudge_reasons: std::collections::BTreeMap::new(),
            turn_count: 3,
            turn_end_reasons: std::collections::BTreeMap::from([
                ("assistant_completed".to_string(), 2),
                ("tool_continuation".to_string(), 1),
            ]),
            input_tokens: 123,
            output_tokens: 45,
            cache_tokens: 6,
            cache_write_tokens: 2,
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
                ..Default::default()
            },
            provider_telemetry: None,
            turns: Vec::new(),
        };

        write_benchmark_usage_json(&path, &summary, "completed").unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["status"], "completed");
        assert_eq!(written["last_completed_turn"], 3);
        assert_eq!(written["requested_model"], "anthropic:claude-sonnet-4-6");
        assert_eq!(written["requested_provider"], "anthropic");
        assert_eq!(written["resolved_provider"], "anthropic");
        assert_eq!(written["turn_count"], 3);
        assert_eq!(written["input_tokens"], 123);
        assert_eq!(written["cache_write_tokens"], 2);
        assert_eq!(written["per_turn"]["avg_input_tokens"], 41);
        assert_eq!(written["per_turn"]["avg_cache_tokens"], 2);
        assert_eq!(written["per_turn"]["avg_cache_write_tokens"], 0);
        assert_eq!(written["per_turn"]["avg_estimated_tokens"], 107);
    }

    #[tokio::test]
    async fn benchmark_event_printer_timeout_does_not_block_usage_finalization() {
        let (_events_tx, mut events_rx) = tokio::sync::broadcast::channel::<AgentEvent>(4);
        let blocker = tokio::spawn(async move {
            let _held_sender = _events_tx.clone();
            let _ = events_rx.recv().await;
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        let wait = tokio::time::timeout(std::time::Duration::from_millis(50), blocker).await;
        assert!(
            wait.is_err(),
            "event task should still be blocked before forced shutdown handling"
        );
    }

    #[test]
    fn benchmark_usage_json_writer_supports_in_progress_checkpoints() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bench").join("usage.json");
        let summary = BenchmarkUsageSummary {
            requested_model: Some("anthropic:claude-sonnet-4-6".into()),
            requested_provider: Some("anthropic".into()),
            resolved_provider: Some("anthropic".into()),
            model: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            dominant_phases: std::collections::BTreeMap::new(),
            drift_kinds: std::collections::BTreeMap::new(),
            progress_nudge_reasons: std::collections::BTreeMap::new(),
            turn_count: 2,
            turn_end_reasons: std::collections::BTreeMap::new(),
            input_tokens: 88,
            output_tokens: 21,
            cache_tokens: 5,
            cache_write_tokens: 3,
            estimated_tokens: 144,
            context_window: 200_000,
            context_composition: omegon_traits::ContextComposition::default(),
            provider_telemetry: None,
            turns: Vec::new(),
        };

        write_benchmark_usage_json(&path, &summary, "in_progress").unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["status"], "in_progress");
        assert_eq!(written["last_completed_turn"], 2);
        assert_eq!(written["input_tokens"], 88);
        assert_eq!(written["output_tokens"], 21);
    }

    #[test]
    fn logout_clears_all_provider_auth_env_vars() {
        with_auth_env_lock(|| {
            let original_oauth = std::env::var("ANTHROPIC_OAUTH_TOKEN").ok();
            let original_api = std::env::var("ANTHROPIC_API_KEY").ok();
            unsafe {
                std::env::set_var("ANTHROPIC_OAUTH_TOKEN", "token-1");
                std::env::set_var("ANTHROPIC_API_KEY", "key-1");
            }

            auth::clear_provider_auth_env("anthropic");

            assert!(std::env::var("ANTHROPIC_OAUTH_TOKEN").is_err());
            assert!(std::env::var("ANTHROPIC_API_KEY").is_err());

            unsafe {
                match original_oauth {
                    Some(value) => std::env::set_var("ANTHROPIC_OAUTH_TOKEN", value),
                    None => std::env::remove_var("ANTHROPIC_OAUTH_TOKEN"),
                }
                match original_api {
                    Some(value) => std::env::set_var("ANTHROPIC_API_KEY", value),
                    None => std::env::remove_var("ANTHROPIC_API_KEY"),
                }
            }
        });
    }

    #[test]
    fn logout_clears_openai_codex_session_env_var() {
        with_auth_env_lock(|| {
            let original = std::env::var("CHATGPT_OAUTH_TOKEN").ok();
            unsafe {
                std::env::set_var("CHATGPT_OAUTH_TOKEN", "token-1");
            }

            auth::clear_provider_auth_env("openai-codex");

            assert!(std::env::var("CHATGPT_OAUTH_TOKEN").is_err());

            unsafe {
                match original {
                    Some(value) => std::env::set_var("CHATGPT_OAUTH_TOKEN", value),
                    None => std::env::remove_var("CHATGPT_OAUTH_TOKEN"),
                }
            }
        });
    }

    #[test]
    fn anthropic_subscription_automation_warning_only_for_headless_anthropic_oauth() {
        with_auth_env_lock(|| {
            let original_api = std::env::var("ANTHROPIC_API_KEY").ok();
            let original_oauth = std::env::var("ANTHROPIC_OAUTH_TOKEN").ok();
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
                match original_api {
                    Some(value) => std::env::set_var("ANTHROPIC_API_KEY", value),
                    None => std::env::remove_var("ANTHROPIC_API_KEY"),
                }
                match original_oauth {
                    Some(value) => std::env::set_var("ANTHROPIC_OAUTH_TOKEN", value),
                    None => std::env::remove_var("ANTHROPIC_OAUTH_TOKEN"),
                }
            }
        });
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

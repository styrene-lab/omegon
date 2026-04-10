//! HarnessStatus — unified observable state for TUI, web dashboard, and bootstrap.
//!
//! One struct captures everything the operator needs to see:
//! active persona/tone, MCP servers, secrets, inference backends,
//! container runtime, context routing, memory stats.
//!
//! Three consumers:
//! - Bootstrap: rendered once at startup as a structured TUI panel
//! - TUI footer: continuous, re-rendered on BusEvent::HarnessStatusChanged
//! - Web dashboard: broadcast over WebSocket on the existing event bus

use chrono::{DateTime, Duration, Utc};
use rusqlite::{self, Connection};
use serde::{Deserialize, Serialize};

/// Complete observable state of the harness.
/// Clone + Serialize — crosses thread boundaries and goes over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessStatus {
    // ── Repo state ───────────────────────────────────────────
    pub git_branch: Option<String>,
    pub git_detached: bool,

    // ── Persona system ───────────────────────────────────────
    pub active_persona: Option<PersonaSummary>,
    pub active_tone: Option<ToneSummary>,
    pub installed_plugins: Vec<PluginSummary>,

    // ── MCP servers ──────────────────────────────────────────
    pub mcp_servers: Vec<McpServerStatus>,

    // ── Secrets ──────────────────────────────────────────────
    pub secret_backend: Option<SecretBackendStatus>,
    pub web_auth_mode: Option<String>,
    pub web_auth_source: Option<String>,

    // ── Inference backends ───────────────────────────────────
    pub inference_backends: Vec<InferenceBackendStatus>,

    // ── Container runtime ────────────────────────────────────
    pub container_runtime: Option<ContainerRuntimeStatus>,

    // ── Context routing (three-axis model) ───────────────────
    pub context_class: String,   // "Squad" / "Maniple" / "Clan" / "Legion"
    pub thinking_level: String,  // "Off" / "Minimal" / "Low" / "Medium" / "High"
    pub capability_tier: String, // "retribution" / "victory" / "gloriana"
    pub runtime_profile: omegon_traits::OmegonRuntimeProfile,
    pub autonomy_mode: omegon_traits::OmegonAutonomyMode,
    pub dispatcher: DispatcherStatus,

    // ── Memory ───────────────────────────────────────────────
    pub memory: MemoryStatus,

    // ── Cloud providers ──────────────────────────────────────
    pub providers: Vec<ProviderStatus>,

    // ── Feature availability ───────────────────────────────
    pub memory_available: bool,
    pub cleave_available: bool,
    pub memory_warning: Option<String>,

    // ── Active delegates ─────────────────────────────────────
    /// Currently running delegate processes (cleave children).
    pub active_delegates: Vec<DelegateSummary>,
}

/// Summary of an active delegate process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatcherStatus {
    pub available_options: Vec<String>,
    pub switch_state: String,
    pub request_id: Option<String>,
    pub expected_profile: Option<String>,
    pub expected_model: Option<String>,
    pub active_profile: Option<String>,
    pub active_model: Option<String>,
    pub failure_code: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateSummary {
    pub task_id: String,
    pub agent_name: String,
    pub status: String, // "running" / "completed" / "failed"
    pub elapsed_ms: u64,
}

// ── Sub-types ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonaSummary {
    pub id: String,
    pub name: String,
    pub badge: String,
    pub mind_facts_count: usize,
    pub activated_skills: Vec<String>,
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToneSummary {
    pub id: String,
    pub name: String,
    pub intensity_mode: String, // "full" / "muted" based on current context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub plugin_type: String, // "persona" / "tone" / "skill" / "extension"
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub transport_mode: McpTransportMode,
    pub tool_count: usize,
    pub connected: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpTransportMode {
    LocalProcess,
    OciContainer,
    DockerGateway,
    StyreneMesh,
}

impl std::fmt::Display for McpTransportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalProcess => write!(f, "local"),
            Self::OciContainer => write!(f, "oci"),
            Self::DockerGateway => write!(f, "docker-mcp"),
            Self::StyreneMesh => write!(f, "styrene"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretBackendStatus {
    pub backend: String, // "keyring" / "passphrase" / "styrene-identity"
    pub stored_count: usize,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceBackendStatus {
    pub name: String, // "Candle" / "Ollama" / "Burn-LM"
    pub kind: InferenceKind,
    pub available: bool,
    pub models: Vec<InferenceModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InferenceKind {
    /// Embedded in the binary (Candle, future Burn-LM)
    Native,
    /// External process (Ollama)
    External,
}

impl std::fmt::Display for InferenceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::External => write!(f, "external"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceModelInfo {
    pub name: String,
    pub params: Option<String>, // "30B", "0.6B"
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerRuntimeStatus {
    pub runtime: String, // "podman" / "docker" / "nerdctl"
    pub version: Option<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub total_facts: usize,
    pub active_facts: usize,
    pub project_facts: usize,
    pub persona_facts: usize,
    pub working_facts: usize,
    pub episodes: usize,
    pub edges: usize,
    pub active_persona_mind: Option<String>, // persona name if persona layer has facts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRuntimeStatus {
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String, // "Anthropic" / "OpenAI" / "Copilot"
    pub authenticated: bool,
    pub auth_method: Option<String>, // "oauth" / "api-key" / "copilot"
    pub model: Option<String>,       // active model name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<ProviderRuntimeStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_failure_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecentUpstreamFailure {
    provider: String,
    failure_kind: String,
    timestamp: String,
}

const PROVIDER_RUNTIME_DEGRADED_THRESHOLD: usize = 3;

impl HarnessStatus {
    /// One-line footer summary for TUI.
    /// Example: "⚙ SysEng │ ♪ Concise │ 🔓 3 secrets │ MCP:2 │ Squad │ Medium"
    pub fn footer_summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref p) = self.active_persona {
            let name = truncate_name(&p.name, 15);
            parts.push(format!("{} {}", p.badge, name));
        }
        if let Some(ref t) = self.active_tone {
            let name = truncate_name(&t.name, 12);
            parts.push(format!("♪ {}", name));
        }
        if let Some(ref s) = self.secret_backend {
            let lock = if s.locked { "🔒" } else { "🔓" };
            parts.push(format!("{} {}", lock, s.stored_count));
        }

        let mcp_connected = self.mcp_servers.iter().filter(|s| s.connected).count();
        if mcp_connected > 0 {
            let total_tools: usize = self
                .mcp_servers
                .iter()
                .filter(|s| s.connected)
                .map(|s| s.tool_count)
                .sum();
            parts.push(format!("MCP:{mcp_connected}({total_tools}t)"));
        }

        parts.push(self.context_class.clone());
        parts.push(self.thinking_level.clone());

        parts.join(" │ ")
    }

    /// Apply recent runtime health signals derived from upstream failure logs.
    /// This is intentionally separate from authentication state: a provider can
    /// be authenticated and still be temporarily degraded.
    pub fn annotate_provider_runtime_health(&mut self) {
        let recent = recent_upstream_failures();
        for provider in &mut self.providers {
            let key = provider.name.to_lowercase();
            let matches: Vec<&RecentUpstreamFailure> = recent
                .iter()
                .filter(|entry| entry.provider == key)
                .collect();
            apply_provider_runtime_health(provider, &matches);
        }
    }

    /// Check if any MCP servers failed to connect.
    pub fn mcp_errors(&self) -> Vec<&McpServerStatus> {
        self.mcp_servers
            .iter()
            .filter(|s| s.error.is_some())
            .collect()
    }

    /// Total MCP tools available.
    pub fn mcp_tool_count(&self) -> usize {
        self.mcp_servers
            .iter()
            .filter(|s| s.connected)
            .map(|s| s.tool_count)
            .sum()
    }
}

impl HarnessStatus {
    fn project_root_from_cwd(cwd: &std::path::Path) -> std::path::PathBuf {
        let output = std::process::Command::new("git")
            .current_dir(cwd)
            .args(["rev-parse", "--show-toplevel"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();
        if let Ok(output) = output
            && output.status.success()
        {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                return std::path::PathBuf::from(root);
            }
        }
        cwd.to_path_buf()
    }

    fn probe_memory_status(cwd: &std::path::Path) -> Option<MemoryStatus> {
        let project_root = Self::project_root_from_cwd(cwd);
        let ai = project_root.join("ai").join("memory");
        let omegon = project_root.join(".omegon").join("memory");
        let memory_dir = if omegon.exists() && !ai.exists() {
            omegon
        } else {
            ai
        };
        let db_path = memory_dir.join("facts.db");
        if !db_path.exists() {
            return None;
        }

        let conn = Connection::open(db_path).ok()?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).ok()?;

        let total_facts: usize = conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .ok()?;
        let active_facts: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE status = 'active'",
                [],
                |r| r.get(0),
            )
            .ok()?;
        let project_facts: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE status = 'active' AND layer = 'project'",
                [],
                |r| r.get(0),
            )
            .ok()?;
        let persona_facts: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE status = 'active' AND layer = 'persona'",
                [],
                |r| r.get(0),
            )
            .ok()?;
        let working_facts: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE status = 'active' AND layer = 'working'",
                [],
                |r| r.get(0),
            )
            .ok()?;
        let episodes: usize = conn
            .query_row("SELECT COUNT(*) FROM episodes", [], |r| r.get(0))
            .ok()?;
        let edges: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE status = 'active'",
                [],
                |r| r.get(0),
            )
            .ok()?;
        let active_persona_mind: Option<String> = conn
            .query_row(
                "SELECT mind FROM facts WHERE status = 'active' AND layer = 'persona' GROUP BY mind ORDER BY COUNT(*) DESC, mind ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();

        Some(MemoryStatus {
            total_facts,
            active_facts,
            project_facts,
            persona_facts,
            working_facts,
            episodes,
            edges,
            active_persona_mind,
        })
    }

    /// Probe the system and assemble the initial HarnessStatus at startup.
    /// This is the bootstrap probe — runs once before the event loop.
    pub fn assemble() -> Self {
        let mut status = Self::default();

        if let Ok(output) = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            && output.status.success()
        {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if branch.is_empty() {
                status.git_detached = true;
            } else {
                status.git_branch = Some(branch);
            }
        }

        // Probe container runtime (lazy — only if podman/docker likely available)
        status.container_runtime = probe_container_runtime();

        // Probe secret store
        status.secret_backend = probe_secret_store();

        // Probe Ollama (common local inference backend)
        status.inference_backends = probe_inference_backends();

        if let Ok(cwd) = std::env::current_dir()
            && let Some(memory) = Self::probe_memory_status(&cwd)
        {
            status.update_memory(memory);
        }

        status
    }

    /// Update from the EventBus after plugin discovery completes.
    /// Called in setup.rs after discover_plugins() to populate MCP server
    /// and plugin info that assemble() can't know about.
    pub fn update_from_bus(&mut self, bus: &crate::bus::EventBus) {
        // Populate installed plugins from the bus's registered features
        // (Feature trait doesn't expose identity, so we use tool counts as signal)
        let tool_defs = bus.tool_definitions();
        self.memory_available = tool_defs
            .iter()
            .any(|t| t.name == crate::tool_registry::memory::MEMORY_QUERY);
        self.cleave_available = tool_defs
            .iter()
            .any(|t| t.name == crate::tool_registry::cleave::CLEAVE_ASSESS)
            && tool_defs
                .iter()
                .any(|t| t.name == crate::tool_registry::cleave::CLEAVE_RUN);
        let mcp_tools: Vec<_> = tool_defs
            .iter()
            .filter(|t| t.label.starts_with("mcp:"))
            .collect();

        if !mcp_tools.is_empty() {
            // Group by server name (label is "mcp:servername")
            let mut servers: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for t in &mcp_tools {
                let server = t.label.strip_prefix("mcp:").unwrap_or(&t.label);
                *servers.entry(server.to_string()).or_default() += 1;
            }
            self.mcp_servers = servers
                .into_iter()
                .map(|(name, count)| {
                    McpServerStatus {
                        name,
                        transport_mode: McpTransportMode::LocalProcess, // best guess
                        tool_count: count,
                        connected: true,
                        error: None,
                    }
                })
                .collect();
        }
    }

    /// Update routing state from the settings/profile.
    pub fn update_routing(
        &mut self,
        context_class: &str,
        thinking_level: &str,
        capability_tier: &str,
    ) {
        self.context_class = context_class.into();
        self.thinking_level = thinking_level.into();
        self.capability_tier = capability_tier.into();
    }

    /// Update deployment/autonomy posture exported to IPC/Web/Auspex surfaces.
    pub fn update_runtime_posture(
        &mut self,
        runtime_profile: omegon_traits::OmegonRuntimeProfile,
        autonomy_mode: omegon_traits::OmegonAutonomyMode,
    ) {
        self.runtime_profile = runtime_profile;
        self.autonomy_mode = autonomy_mode;
    }

    pub fn update_dispatcher_state(
        &mut self,
        request_id: Option<String>,
        profile: Option<String>,
        model: Option<String>,
        switch_state: &str,
        failure_code: Option<String>,
        note: Option<String>,
    ) {
        self.dispatcher.request_id = request_id;
        self.dispatcher.expected_profile = profile;
        self.dispatcher.expected_model = model;
        self.dispatcher.switch_state = switch_state.to_string();
        self.dispatcher.failure_code = failure_code;
        self.dispatcher.note = note;
    }

    /// Update memory stats.
    pub fn update_memory(&mut self, stats: MemoryStatus) {
        self.memory = stats;
        self.memory_available = true;
    }
}

/// Truncate a name to fit in the footer, adding "…" if needed.
fn truncate_name(name: &str, max: usize) -> String {
    if name.len() <= max {
        name.to_string()
    } else {
        format!("{}…", &name[..max - 1])
    }
}

fn recent_upstream_failures() -> Vec<RecentUpstreamFailure> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let path = home.join(".omegon").join("upstream-failures.jsonl");
    let Ok(content) = std::fs::read_to_string(path) else {
        return vec![];
    };

    let cutoff = Utc::now() - Duration::minutes(5);
    let mut recent = Vec::new();
    for line in content.lines().rev().take(200) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(provider) = value.get("provider").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(failure_kind) = value.get("failure_kind").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(timestamp) = value.get("timestamp").and_then(|v| v.as_str()) else {
            continue;
        };
        let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
            continue;
        };
        if parsed.with_timezone(&Utc) < cutoff {
            continue;
        }
        recent.push(RecentUpstreamFailure {
            provider: provider.to_string(),
            failure_kind: failure_kind.to_string(),
            timestamp: timestamp.to_string(),
        });
    }
    recent
}

fn apply_provider_runtime_health(
    provider: &mut ProviderStatus,
    matches: &[&RecentUpstreamFailure],
) {
    provider.recent_failure_count = Some(matches.len() as u32);
    if let Some(last) = matches.first() {
        provider.last_failure_kind = Some(last.failure_kind.clone());
        provider.last_failure_at = Some(last.timestamp.clone());
    } else {
        provider.last_failure_kind = None;
        provider.last_failure_at = None;
    }

    provider.runtime_status = if matches.len() >= PROVIDER_RUNTIME_DEGRADED_THRESHOLD {
        Some(ProviderRuntimeStatus::Degraded)
    } else {
        Some(ProviderRuntimeStatus::Healthy)
    };
}

/// Detect container runtime (podman/docker).
fn probe_container_runtime() -> Option<ContainerRuntimeStatus> {
    for runtime in &["podman", "docker", "nerdctl"] {
        if let Ok(output) = std::process::Command::new(runtime)
            .arg("--version")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            && output.status.success()
        {
            let version_str = String::from_utf8_lossy(&output.stdout);
            // Extract version number — typically "podman version 5.3.1" or "Docker version 27.x"
            let version = version_str
                .split_whitespace()
                .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
                .map(|v| v.trim_end_matches(',').to_string());

            return Some(ContainerRuntimeStatus {
                runtime: runtime.to_string(),
                version,
                available: true,
            });
        }
    }
    None
}

/// Probe local inference backends (Ollama, etc.).
fn probe_inference_backends() -> Vec<InferenceBackendStatus> {
    let mut backends = Vec::new();

    // Probe Ollama via HTTP — the standard local inference server
    if let Ok(resp) = std::process::Command::new("curl")
        .args(["-sf", "--max-time", "2", "http://localhost:11434/api/tags"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        && resp.status.success()
    {
        let body = String::from_utf8_lossy(&resp.stdout);
        let models: Vec<InferenceModelInfo> = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["models"].as_array().cloned())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        Some(InferenceModelInfo {
                            name: m["name"].as_str()?.to_string(),
                            params: m["details"]["parameter_size"]
                                .as_str()
                                .map(|s| s.to_string()),
                            context_window: None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        backends.push(InferenceBackendStatus {
            name: "Ollama".into(),
            kind: InferenceKind::External,
            available: true,
            models,
        });
    }

    backends
}

/// Check if secrets.db exists and probe its backend type from the meta table.
fn probe_secret_store() -> Option<SecretBackendStatus> {
    let path = omegon_secrets::SecretStore::default_path();
    if !omegon_secrets::SecretStore::exists(&path) {
        return None;
    }

    // Read the backend type from the SQLite meta table — doesn't require the key.
    let backend = match rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(db) => db
            .query_row("SELECT value FROM meta WHERE key = 'backend'", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap_or_else(|_| "encrypted".into()),
        Err(_) => "encrypted".into(),
    };

    Some(SecretBackendStatus {
        backend,
        stored_count: 0, // unknown until unlocked
        locked: true,
    })
}

impl Default for HarnessStatus {
    fn default() -> Self {
        Self {
            git_branch: None,
            git_detached: false,
            active_persona: None,
            active_tone: None,
            installed_plugins: vec![],
            mcp_servers: vec![],
            secret_backend: None,
            web_auth_mode: None,
            web_auth_source: None,
            inference_backends: vec![],
            container_runtime: None,
            context_class: "Squad".into(),
            thinking_level: "Medium".into(),
            capability_tier: "victory".into(),
            runtime_profile: omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
            autonomy_mode: omegon_traits::OmegonAutonomyMode::OperatorDriven,
            dispatcher: DispatcherStatus {
                available_options: vec!["retribution".into(), "victory".into(), "gloriana".into()],
                switch_state: "idle".into(),
                request_id: None,
                expected_profile: None,
                expected_model: None,
                active_profile: Some("victory".into()),
                active_model: None,
                failure_code: None,
                note: None,
            },
            memory: MemoryStatus {
                total_facts: 0,
                active_facts: 0,
                project_facts: 0,
                persona_facts: 0,
                working_facts: 0,
                episodes: 0,
                edges: 0,
                active_persona_mind: None,
            },
            providers: vec![],
            memory_available: false,
            cleave_available: false,
            memory_warning: None,
            active_delegates: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_is_minimal() {
        let status = HarnessStatus::default();
        assert!(status.active_persona.is_none());
        assert!(status.git_branch.is_none());
        assert!(!status.git_detached);
        assert!(status.mcp_servers.is_empty());
        assert!(status.web_auth_mode.is_none());
        assert!(status.web_auth_source.is_none());
        assert_eq!(status.context_class, "Squad");
        assert!(!status.memory_available);
        assert!(!status.cleave_available);
        assert!(status.memory_warning.is_none());
    }

    #[test]
    fn footer_summary_includes_memory_and_cleave_availability_when_set() {
        let mut status = HarnessStatus::default();
        status.memory_available = true;
        status.cleave_available = true;
        let summary = status.footer_summary();
        assert!(summary.contains("Squad"));
        // footer_summary stays terse; availability lives in slash-stats and harness JSON
        assert!(!summary.contains("Memory"));
        assert!(!summary.contains("Cleave"));
    }

    #[test]
    fn footer_summary_minimal() {
        let status = HarnessStatus::default();
        let footer = status.footer_summary();
        assert!(footer.contains("Squad"));
        assert!(footer.contains("Medium"));
    }

    #[test]
    fn footer_summary_full() {
        let mut status = HarnessStatus::default();
        status.active_persona = Some(PersonaSummary {
            id: "test".into(),
            name: "Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 10,
            activated_skills: vec![],
            disabled_tools: vec![],
        });
        status.active_tone = Some(ToneSummary {
            id: "test".into(),
            name: "Concise".into(),
            intensity_mode: "full".into(),
        });
        status.secret_backend = Some(SecretBackendStatus {
            backend: "passphrase".into(),
            stored_count: 3,
            locked: false,
        });
        status.mcp_servers.push(McpServerStatus {
            name: "filesystem".into(),
            transport_mode: McpTransportMode::LocalProcess,
            tool_count: 5,
            connected: true,
            error: None,
        });

        let footer = status.footer_summary();
        assert!(footer.contains("⚙ Engineer"), "footer: {footer}");
        assert!(footer.contains("♪ Concise"), "footer: {footer}");
        assert!(footer.contains("🔓 3"), "footer: {footer}");
        assert!(footer.contains("MCP:1(5t)"), "footer: {footer}");
    }

    #[test]
    fn mcp_errors_filtered() {
        let mut status = HarnessStatus::default();
        status.mcp_servers.push(McpServerStatus {
            name: "ok".into(),
            transport_mode: McpTransportMode::LocalProcess,
            tool_count: 3,
            connected: true,
            error: None,
        });
        status.mcp_servers.push(McpServerStatus {
            name: "broken".into(),
            transport_mode: McpTransportMode::OciContainer,
            tool_count: 0,
            connected: false,
            error: Some("connection refused".into()),
        });

        assert_eq!(status.mcp_errors().len(), 1);
        assert_eq!(status.mcp_errors()[0].name, "broken");
        assert_eq!(status.mcp_tool_count(), 3);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut status = HarnessStatus::default();
        status.active_persona = Some(PersonaSummary {
            id: "test.persona".into(),
            name: "Test".into(),
            badge: "🧪".into(),
            mind_facts_count: 5,
            activated_skills: vec!["rust".into()],
            disabled_tools: vec!["bash".into()],
        });

        let json = serde_json::to_string(&status).unwrap();
        let parsed: HarnessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.active_persona.unwrap().name, "Test");
    }

    #[test]
    fn assemble_runs_without_panic() {
        let status = HarnessStatus::assemble();
        assert_eq!(status.context_class, "Squad");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn assemble_runs_inside_tokio_runtime() {
        let status = HarnessStatus::assemble();
        assert_eq!(status.context_class, "Squad");
    }

    #[test]
    fn transport_mode_display() {
        assert_eq!(McpTransportMode::LocalProcess.to_string(), "local");
        assert_eq!(McpTransportMode::OciContainer.to_string(), "oci");
        assert_eq!(McpTransportMode::DockerGateway.to_string(), "docker-mcp");
        assert_eq!(McpTransportMode::StyreneMesh.to_string(), "styrene");
    }

    #[test]
    fn inference_kind_display() {
        assert_eq!(InferenceKind::Native.to_string(), "native");
        assert_eq!(InferenceKind::External.to_string(), "external");
    }

    #[test]
    fn provider_runtime_health_requires_threshold_to_mark_degraded() {
        let mut provider = ProviderStatus {
            name: "openai".into(),
            authenticated: true,
            auth_method: Some("oauth".into()),
            model: Some("gpt-5".into()),
            runtime_status: None,
            recent_failure_count: None,
            last_failure_kind: None,
            last_failure_at: None,
        };
        let recent = vec![
            RecentUpstreamFailure {
                provider: "openai".into(),
                failure_kind: "timeout".into(),
                timestamp: "2026-04-03T12:00:00Z".into(),
            },
            RecentUpstreamFailure {
                provider: "openai".into(),
                failure_kind: "timeout".into(),
                timestamp: "2026-04-03T12:00:01Z".into(),
            },
        ];
        let refs: Vec<&RecentUpstreamFailure> = recent.iter().collect();
        apply_provider_runtime_health(&mut provider, &refs);
        assert_eq!(
            provider.runtime_status,
            Some(ProviderRuntimeStatus::Healthy)
        );
        assert_eq!(provider.recent_failure_count, Some(2));
        assert_eq!(provider.last_failure_kind.as_deref(), Some("timeout"));
    }

    #[test]
    fn provider_runtime_health_marks_degraded_at_threshold() {
        let mut provider = ProviderStatus {
            name: "openai".into(),
            authenticated: true,
            auth_method: Some("oauth".into()),
            model: Some("gpt-5".into()),
            runtime_status: None,
            recent_failure_count: None,
            last_failure_kind: None,
            last_failure_at: None,
        };
        let recent = vec![
            RecentUpstreamFailure {
                provider: "openai".into(),
                failure_kind: "timeout".into(),
                timestamp: "2026-04-03T12:00:02Z".into(),
            },
            RecentUpstreamFailure {
                provider: "openai".into(),
                failure_kind: "timeout".into(),
                timestamp: "2026-04-03T12:00:01Z".into(),
            },
            RecentUpstreamFailure {
                provider: "openai".into(),
                failure_kind: "timeout".into(),
                timestamp: "2026-04-03T12:00:00Z".into(),
            },
        ];
        let refs: Vec<&RecentUpstreamFailure> = recent.iter().collect();
        apply_provider_runtime_health(&mut provider, &refs);
        assert_eq!(
            provider.runtime_status,
            Some(ProviderRuntimeStatus::Degraded)
        );
        assert_eq!(provider.recent_failure_count, Some(3));
    }
}

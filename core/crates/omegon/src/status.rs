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

use serde::{Deserialize, Serialize};

/// Complete observable state of the harness.
/// Clone + Serialize — crosses thread boundaries and goes over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessStatus {
    // ── Persona system ───────────────────────────────────────
    pub active_persona: Option<PersonaSummary>,
    pub active_tone: Option<ToneSummary>,
    pub installed_plugins: Vec<PluginSummary>,

    // ── MCP servers ──────────────────────────────────────────
    pub mcp_servers: Vec<McpServerStatus>,

    // ── Secrets ──────────────────────────────────────────────
    pub secret_backend: Option<SecretBackendStatus>,

    // ── Inference backends ───────────────────────────────────
    pub inference_backends: Vec<InferenceBackendStatus>,

    // ── Container runtime ────────────────────────────────────
    pub container_runtime: Option<ContainerRuntimeStatus>,

    // ── Context routing (three-axis model) ───────────────────
    pub context_class: String,      // "Squad" / "Maniple" / "Clan" / "Legion"
    pub thinking_level: String,     // "Off" / "Minimal" / "Low" / "Medium" / "High"
    pub capability_tier: String,    // "retribution" / "victory" / "gloriana"

    // ── Memory ───────────────────────────────────────────────
    pub memory: MemoryStatus,

    // ── Cloud providers ──────────────────────────────────────
    pub providers: Vec<ProviderStatus>,
}

// ── Sub-types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaSummary {
    pub id: String,
    pub name: String,
    pub badge: String,
    pub mind_facts_count: usize,
    pub activated_skills: Vec<String>,
    pub disabled_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneSummary {
    pub id: String,
    pub name: String,
    pub intensity_mode: String, // "full" / "muted" based on current context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub plugin_type: String,    // "persona" / "tone" / "skill" / "extension"
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
    pub backend: String,        // "keyring" / "passphrase" / "styrene-identity"
    pub stored_count: usize,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceBackendStatus {
    pub name: String,           // "Candle" / "Ollama" / "Burn-LM"
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
    pub params: Option<String>,     // "30B", "0.6B"
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerRuntimeStatus {
    pub runtime: String,        // "podman" / "docker" / "nerdctl"
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String,           // "Anthropic" / "OpenAI" / "Copilot"
    pub authenticated: bool,
    pub auth_method: Option<String>, // "oauth" / "api-key" / "copilot"
    pub model: Option<String>,  // active model name
}

// ── Display for bootstrap rendering ──────────────────────────

impl HarnessStatus {
    /// One-line footer summary for TUI.
    /// Example: "⚙ SysEng │ ♪ Concise │ 🔓 3 secrets │ MCP:2 │ Squad │ Medium"
    pub fn footer_summary(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref p) = self.active_persona {
            parts.push(format!("{} {}", p.badge, p.name));
        }
        if let Some(ref t) = self.active_tone {
            parts.push(format!("♪ {}", t.name));
        }
        if let Some(ref s) = self.secret_backend {
            let lock = if s.locked { "🔒" } else { "🔓" };
            parts.push(format!("{} {}", lock, s.stored_count));
        }

        let mcp_connected = self.mcp_servers.iter().filter(|s| s.connected).count();
        if mcp_connected > 0 {
            let total_tools: usize = self.mcp_servers.iter()
                .filter(|s| s.connected)
                .map(|s| s.tool_count)
                .sum();
            parts.push(format!("MCP:{mcp_connected}({total_tools}t)"));
        }

        parts.push(self.context_class.clone());
        parts.push(self.thinking_level.clone());

        parts.join(" │ ")
    }

    /// Check if any MCP servers failed to connect.
    pub fn mcp_errors(&self) -> Vec<&McpServerStatus> {
        self.mcp_servers.iter().filter(|s| s.error.is_some()).collect()
    }

    /// Total MCP tools available.
    pub fn mcp_tool_count(&self) -> usize {
        self.mcp_servers.iter()
            .filter(|s| s.connected)
            .map(|s| s.tool_count)
            .sum()
    }
}

impl Default for HarnessStatus {
    fn default() -> Self {
        Self {
            active_persona: None,
            active_tone: None,
            installed_plugins: vec![],
            mcp_servers: vec![],
            secret_backend: None,
            inference_backends: vec![],
            container_runtime: None,
            context_class: "Squad".into(),
            thinking_level: "Medium".into(),
            capability_tier: "victory".into(),
            memory: MemoryStatus {
                total_facts: 0, active_facts: 0,
                project_facts: 0, persona_facts: 0, working_facts: 0,
                episodes: 0, edges: 0,
                active_persona_mind: None,
            },
            providers: vec![],
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
        assert!(status.mcp_servers.is_empty());
        assert_eq!(status.context_class, "Squad");
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
            id: "test".into(), name: "Engineer".into(), badge: "⚙".into(),
            mind_facts_count: 10, activated_skills: vec![], disabled_tools: vec![],
        });
        status.active_tone = Some(ToneSummary {
            id: "test".into(), name: "Concise".into(), intensity_mode: "full".into(),
        });
        status.secret_backend = Some(SecretBackendStatus {
            backend: "passphrase".into(), stored_count: 3, locked: false,
        });
        status.mcp_servers.push(McpServerStatus {
            name: "filesystem".into(), transport_mode: McpTransportMode::LocalProcess,
            tool_count: 5, connected: true, error: None,
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
            name: "ok".into(), transport_mode: McpTransportMode::LocalProcess,
            tool_count: 3, connected: true, error: None,
        });
        status.mcp_servers.push(McpServerStatus {
            name: "broken".into(), transport_mode: McpTransportMode::OciContainer,
            tool_count: 0, connected: false, error: Some("connection refused".into()),
        });

        assert_eq!(status.mcp_errors().len(), 1);
        assert_eq!(status.mcp_errors()[0].name, "broken");
        assert_eq!(status.mcp_tool_count(), 3);
    }

    #[test]
    fn serialization_roundtrip() {
        let mut status = HarnessStatus::default();
        status.active_persona = Some(PersonaSummary {
            id: "test.persona".into(), name: "Test".into(), badge: "🧪".into(),
            mind_facts_count: 5, activated_skills: vec!["rust".into()],
            disabled_tools: vec!["bash".into()],
        });

        let json = serde_json::to_string(&status).unwrap();
        let parsed: HarnessStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.active_persona.unwrap().name, "Test");
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
}

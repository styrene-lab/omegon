//! Bootstrap panel — structured TUI output at startup.
//!
//! Renders the HarnessStatus as a branded panel showing the operator
//! everything that's available: providers, inference, MCP servers,
//! secrets, plugins, and context routing.
//!
//! Output goes to stderr (via `eprint!` in setup.rs) so it doesn't
//! pollute piped stdout. Degrades to plain text (no ANSI codes) when
//! stderr is not a terminal or `NO_COLOR` env var is set.

use crate::status::*;

/// Render the bootstrap panel to a String.
/// Uses ANSI colors when `color` is true.
pub fn render_bootstrap(status: &HarnessStatus, color: bool) -> String {
    let mut out = String::with_capacity(2048);

    let (bold, dim, cyan, green, yellow, red, reset) = if color {
        ("\x1b[1m", "\x1b[2m", "\x1b[36m", "\x1b[32m", "\x1b[33m", "\x1b[31m", "\x1b[0m")
    } else {
        ("", "", "", "", "", "", "")
    };

    // Banner
    out.push_str(&format!("\n{bold}{cyan}  Ω  Omegon{reset}\n"));
    out.push_str(&format!("{dim}  Systems engineering harness{reset}\n\n"));

    // Providers
    out.push_str(&format!("  {bold}Cloud Providers{reset}\n"));
    if status.providers.is_empty() {
        out.push_str(&format!("    {dim}none configured{reset}\n"));
    } else {
        for p in &status.providers {
            let icon = if p.authenticated { format!("{green}✓{reset}") } else { format!("{yellow}⚠{reset}") };
            let model = p.model.as_deref().unwrap_or("-");
            let auth = p.auth_method.as_deref().unwrap_or("none");
            out.push_str(&format!("    {icon} {:<14} {:<24} {dim}({auth}){reset}\n", p.name, model));
        }
    }
    out.push('\n');

    // Inference backends
    out.push_str(&format!("  {bold}Local Inference{reset}\n"));
    if status.inference_backends.is_empty() {
        out.push_str(&format!("    {dim}none available{reset}\n"));
    } else {
        for b in &status.inference_backends {
            let icon = if b.available { format!("{green}✓{reset}") } else { format!("{dim}○{reset}") };
            let kind = &b.kind;
            let models_str = if b.models.is_empty() {
                "no models".into()
            } else {
                format!("{} model{}", b.models.len(), if b.models.len() == 1 { "" } else { "s" })
            };
            out.push_str(&format!("    {icon} {:<14} {dim}({kind}){reset}  {models_str}\n", b.name));
        }
    }
    out.push('\n');

    // MCP servers
    if !status.mcp_servers.is_empty() {
        out.push_str(&format!("  {bold}MCP Servers{reset}\n"));
        for s in &status.mcp_servers {
            let icon = if s.connected { format!("{green}✓{reset}") } else { format!("{red}✗{reset}") };
            let mode = &s.transport_mode;
            let tools = format!("{} tool{}", s.tool_count, if s.tool_count == 1 { "" } else { "s" });
            out.push_str(&format!("    {icon} {:<14} {dim}({mode}){reset}  {tools}\n", s.name));
            if let Some(ref err) = s.error {
                out.push_str(&format!("      {red}{err}{reset}\n"));
            }
        }
        out.push('\n');
    }

    // Secrets
    if let Some(ref sec) = status.secret_backend {
        out.push_str(&format!("  {bold}Secrets{reset}\n"));
        let lock = if sec.locked { format!("{yellow}🔒{reset}") } else { format!("{green}🔓{reset}") };
        out.push_str(&format!("    {lock} {:<14} {dim}{} stored{reset}\n", sec.backend, sec.stored_count));
        out.push('\n');
    }

    // Container runtime
    if let Some(ref cr) = status.container_runtime {
        let icon = if cr.available { format!("{green}✓{reset}") } else { format!("{dim}○{reset}") };
        let ver = cr.version.as_deref().unwrap_or("unknown");
        out.push_str(&format!("  {bold}Container{reset}\n"));
        out.push_str(&format!("    {icon} {:<14} {dim}{ver}{reset}\n\n", cr.runtime));
    }

    // Active persona/tone
    if status.active_persona.is_some() || status.active_tone.is_some() {
        out.push_str(&format!("  {bold}Identity{reset}\n"));
        if let Some(ref p) = status.active_persona {
            out.push_str(&format!("    {cyan}{} {}{reset}  {dim}{} mind facts, {} skills{reset}\n",
                p.badge, p.name, p.mind_facts_count, p.activated_skills.len()));
        }
        if let Some(ref t) = status.active_tone {
            out.push_str(&format!("    {cyan}♪ {}{reset}  {dim}intensity: {}{reset}\n", t.name, t.intensity_mode));
        }
        out.push('\n');
    }

    // Routing
    out.push_str(&format!("  {bold}Routing{reset}\n"));
    out.push_str(&format!("    Context:  {cyan}{}{reset}\n", status.context_class));
    out.push_str(&format!("    Thinking: {cyan}{}{reset}\n", status.thinking_level));
    out.push_str(&format!("    Tier:     {cyan}{}{reset}\n", status.capability_tier));
    out.push('\n');

    // Memory
    out.push_str(&format!("  {bold}Memory{reset}\n"));
    out.push_str(&format!("    {dim}{} facts ({} active, {} project, {} persona, {} working){reset}\n",
        status.memory.total_facts, status.memory.active_facts,
        status.memory.project_facts, status.memory.persona_facts, status.memory.working_facts));
    out.push_str(&format!("    {dim}{} episodes, {} edges{reset}\n",
        status.memory.episodes, status.memory.edges));
    out.push('\n');

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_renders_without_panic() {
        let status = HarnessStatus::default();
        let output = render_bootstrap(&status, false);
        assert!(output.contains("Omegon"));
        assert!(output.contains("Routing"));
        assert!(output.contains("Memory"));
    }

    #[test]
    fn bootstrap_with_color() {
        let status = HarnessStatus::default();
        let output = render_bootstrap(&status, true);
        assert!(output.contains("\x1b[1m")); // bold
        assert!(output.contains("\x1b[36m")); // cyan
    }

    #[test]
    fn bootstrap_full_status() {
        let mut status = HarnessStatus::default();
        status.providers.push(ProviderStatus {
            name: "Anthropic".into(), authenticated: true,
            auth_method: Some("oauth".into()), model: Some("Claude 4 Sonnet".into()),
        });
        status.providers.push(ProviderStatus {
            name: "OpenAI".into(), authenticated: false,
            auth_method: None, model: None,
        });
        status.inference_backends.push(InferenceBackendStatus {
            name: "Ollama".into(), kind: InferenceKind::External, available: true,
            models: vec![
                InferenceModelInfo { name: "qwen3:30b".into(), params: Some("30B".into()), context_window: Some(262144) },
            ],
        });
        status.mcp_servers.push(McpServerStatus {
            name: "filesystem".into(), transport_mode: McpTransportMode::LocalProcess,
            tool_count: 5, connected: true, error: None,
        });
        status.mcp_servers.push(McpServerStatus {
            name: "postgres".into(), transport_mode: McpTransportMode::OciContainer,
            tool_count: 0, connected: false, error: Some("image not found".into()),
        });
        status.secret_backend = Some(SecretBackendStatus {
            backend: "passphrase".into(), stored_count: 3, locked: false,
        });
        status.container_runtime = Some(ContainerRuntimeStatus {
            runtime: "podman".into(), version: Some("5.3.1".into()), available: true,
        });
        status.active_persona = Some(PersonaSummary {
            id: "test".into(), name: "Systems Engineer".into(), badge: "⚙".into(),
            mind_facts_count: 10, activated_skills: vec!["rust".into(), "typescript".into()],
            disabled_tools: vec![],
        });
        status.active_tone = Some(ToneSummary {
            id: "test".into(), name: "Concise".into(), intensity_mode: "full".into(),
        });
        status.memory.total_facts = 2440;
        status.memory.active_facts = 1800;
        status.memory.project_facts = 1790;
        status.memory.persona_facts = 10;

        let output = render_bootstrap(&status, false);

        assert!(output.contains("Anthropic"), "should show Anthropic: {output}");
        assert!(output.contains("Claude 4 Sonnet"), "should show model");
        assert!(output.contains("Ollama"), "should show Ollama");
        assert!(output.contains("filesystem"), "should show MCP server");
        assert!(output.contains("image not found"), "should show MCP error");
        assert!(output.contains("passphrase"), "should show secret backend");
        assert!(output.contains("podman"), "should show container runtime");
        assert!(output.contains("Systems Engineer"), "should show persona");
        assert!(output.contains("Concise"), "should show tone");
        assert!(output.contains("2440"), "should show fact count");
    }

    #[test]
    fn bootstrap_no_color_has_no_escape_codes() {
        let status = HarnessStatus::default();
        let output = render_bootstrap(&status, false);
        assert!(!output.contains("\x1b["), "no-color mode should have no ANSI codes");
    }
}

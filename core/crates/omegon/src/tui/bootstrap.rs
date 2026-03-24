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

    let (bold, dim, cyan, green, yellow, _red, reset) = if color {
        ("\x1b[1m", "\x1b[2m", "\x1b[36m", "\x1b[32m", "\x1b[33m", "\x1b[31m", "\x1b[0m")
    } else {
        ("", "", "", "", "", "", "")
    };

    // Compact banner
    out.push_str(&format!("\n{bold}{cyan}  Ω  Omegon{reset}\n\n"));

    // Providers — deduplicated by name (case-insensitive), one line each
    let mut has_providers = false;
    let mut seen_providers = std::collections::HashSet::new();
    for p in &status.providers {
        let key = p.name.to_lowercase();
        if !seen_providers.insert(key) { continue; } // skip duplicates
        let icon = if p.authenticated { format!("{green}✓{reset}") } else { format!("{yellow}⚠{reset}") };
        let auth = p.auth_method.as_deref().unwrap_or("none");
        // Display name from canonical provider map
        let display_name = crate::auth::provider_by_id(&p.name.to_lowercase())
            .map(|pc| pc.display_name)
            .unwrap_or(&p.name);
        out.push_str(&format!("  {icon} {:<12} {dim}({auth}){reset}\n", display_name));
        has_providers = true;
    }
    if !has_providers {
        out.push_str(&format!("  {dim}no providers — /login to configure{reset}\n"));
    }

    // Local inference — single line
    for b in &status.inference_backends {
        if b.available {
            let n = b.models.len();
            out.push_str(&format!("  {green}✓{reset} {:<12} {dim}{n} model{}{reset}\n",
                b.name, if n == 1 { "" } else { "s" }));
        }
    }

    // Container — single line, only if present
    if let Some(ref cr) = status.container_runtime {
        if cr.available {
            let ver = cr.version.as_deref().unwrap_or("");
            out.push_str(&format!("  {green}✓{reset} {:<12} {dim}{ver}{reset}\n", cr.runtime));
        }
    }

    // MCP — single line summary
    if !status.mcp_servers.is_empty() {
        let connected = status.mcp_servers.iter().filter(|s| s.connected).count();
        let total = status.mcp_servers.len();
        let tools: usize = status.mcp_servers.iter().map(|s| s.tool_count).sum();
        out.push_str(&format!("  {green}✓{reset} mcp          {dim}{connected}/{total} connected, {tools} tools{reset}\n"));
    }

    out.push('\n');

    // Routing — single line
    out.push_str(&format!("  {dim}Context:{reset} {cyan}{}{reset}  {dim}Thinking:{reset} {cyan}{}{reset}  {dim}Tier:{reset} {cyan}{}{reset}\n",
        status.context_class, status.thinking_level, status.capability_tier));

    // Memory — single line
    let mem = &status.memory;
    if mem.total_facts > 0 {
        out.push_str(&format!("  {dim}{} facts, {} episodes, {} edges{reset}\n",
            mem.total_facts, mem.episodes, mem.edges));
    }

    // Active persona — single line
    if let Some(ref p) = status.active_persona {
        out.push_str(&format!("  {cyan}{} {}{reset}\n", p.badge, p.name));
    }

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
        assert!(output.contains("Context:"));
        assert!(output.contains("Tier:"));
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
        assert!(output.contains("Ollama"), "should show Ollama");
        assert!(output.contains("mcp"), "should show MCP summary");
        assert!(output.contains("podman"), "should show container runtime");
        assert!(output.contains("Systems Engineer"), "should show persona");
        assert!(output.contains("2440"), "should show fact count");
    }

    #[test]
    fn bootstrap_no_color_has_no_escape_codes() {
        let status = HarnessStatus::default();
        let output = render_bootstrap(&status, false);
        assert!(!output.contains("\x1b["), "no-color mode should have no ANSI codes");
    }

    /// /status slash command use case: re-render with live data, no ANSI.
    #[test]
    fn status_command_rerender_no_color() {
        let mut status = HarnessStatus::default();
        // Simulate mid-session state changes
        status.active_persona = Some(PersonaSummary {
            id: "eng".into(), name: "Systems Engineer".into(), badge: "⚙".into(),
            mind_facts_count: 42, activated_skills: vec!["rust".into()],
            disabled_tools: vec![],
        });
        status.context_class = "Maniple".into();
        status.thinking_level = "High".into();
        status.memory.total_facts = 1200;
        status.memory.active_facts = 900;

        // /status renders without ANSI (SlashResult::Display goes through ratatui)
        let output = render_bootstrap(&status, false);
        assert!(!output.contains("\x1b["), "slash command output must not have ANSI codes");
        assert!(output.contains("Systems Engineer"), "should show current persona");
        assert!(output.contains("Maniple"), "should show current context class");
        assert!(output.contains("High"), "should show current thinking level");
        assert!(output.contains("1200"), "should show current fact count");
    }
}

//! TUI snapshot tests — render widgets to TestBackend, capture as insta snapshots.
//!
//! These catch visual regressions: layout changes, text truncation, missing sections.
//! Run `cargo insta review` to inspect and approve snapshot changes.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::dashboard::*;
use super::footer::FooterData;
use super::instruments::InstrumentPanel;
use super::theme::Alpharius;
use crate::lifecycle::types::*;
use crate::settings::ContextClass;
#[allow(unused_imports)]
use crate::settings::Settings;
use crate::status::*;

/// Render a terminal buffer to a multi-line string suitable for insta snapshots.
/// Each line is one row of the terminal, trailing spaces trimmed.
///
/// Version strings of the form `vX.Y.Z → vX.Y.Z` are normalized to
/// `v<current> → v<next>` so snapshots don't break on every release bump.
fn render_to_string(terminal: &Terminal<TestBackend>) -> String {
    let buf = terminal.backend().buffer();
    let area = buf.area;
    let mut lines = Vec::new();
    for y in 0..area.height {
        let line: String = (0..area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string();
        lines.push(line);
    }
    // Trim trailing empty lines
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let raw = lines.join("\n");
    // Normalize compiled-in version strings so snapshots survive release bumps.
    // Pattern: "vMAJOR.MINOR.PATCH → vMAJOR.MINOR.PATCH"
    let normalized = regex_replace_version(&raw);
    normalized
}

fn regex_replace_version(s: &str) -> String {
    // Hand-rolled replace — avoids a regex dep in tests.
    // Matches "v<digits>.<digits>.<digits>[-rc.<digits>] → v<digits>.<digits>.<digits>[-rc.<digits>]"
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(" v") {
        let after_v = &rest[pos + 2..];
        // Check if this looks like a version transition "vX.Y.Z → vX.Y.Z"
        if let Some(end) = version_transition_end(after_v) {
            result.push_str(&rest[..pos]);
            result.push_str(" v<current> → v<next>");
            rest = &rest[pos + 2 + end..];
        } else {
            result.push_str(&rest[..pos + 2]);
            rest = &rest[pos + 2..];
        }
    }
    result.push_str(rest);
    result
}

/// Returns the byte length of a version transition starting just after the leading "v",
/// e.g. for "0.15.7 → v0.15.8   rest" returns Some(len_through_second_version).
fn version_transition_end(s: &str) -> Option<usize> {
    let mut i = 0;
    // consume first version digits: digits.digits.digits[-rc.digits]
    i += version_digits_len(&s[i..])?;
    // consume " → v"
    let arrow = " \u{2192} v";
    if s[i..].starts_with(arrow) {
        i += arrow.len();
    } else {
        return None;
    }
    // consume second version
    i += version_digits_len(&s[i..])?;
    Some(i)
}

fn version_digits_len(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    // MAJOR
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    if i == 0 || i >= bytes.len() || bytes[i] != b'.' { return None; }
    i += 1;
    // MINOR
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    if i == start { return None; }
    if i >= bytes.len() || bytes[i] != b'.' { return None; }
    i += 1;
    // PATCH
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    if i == start { return None; }
    // optional -rc.N
    if s[i..].starts_with("-rc.") {
        i += 4;
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
        if i == start { return None; }
    }
    Some(i)
}

// ═══════════════════════════════════════════════════════════════════
// Dashboard snapshots
// ═══════════════════════════════════════════════════════════════════

#[test]
fn snapshot_dashboard_empty() {
    let mut state = DashboardState::default();
    let backend = TestBackend::new(36, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| state.render_themed(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_dashboard_with_focused_node() {
    let mut state = DashboardState::default();
    state.focused_node = Some(FocusedNodeSummary {
        id: "auth-surface".into(),
        title: "Unified auth surface".into(),
        status: NodeStatus::Implementing,
        open_questions: 2,
        assumptions: 1,
        decisions: 3,
        readiness: 0.5,
        openspec_change: None,
    });
    let backend = TestBackend::new(36, 25);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| state.render_themed(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_dashboard_with_openspec_change() {
    let mut state = DashboardState::default();
    state.active_changes = vec![ChangeSummary {
        name: "tui-surface-pass".into(),
        stage: ChangeStage::Implementing,
        done_tasks: 7,
        total_tasks: 10,
    }];
    let backend = TestBackend::new(36, 25);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| state.render_themed(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_dashboard_with_harness_status() {
    let mut state = DashboardState::default();
    state.harness = Some(HarnessStatus {
        active_persona: Some(PersonaSummary {
            id: "eng".into(),
            name: "Systems Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 42,
            activated_skills: vec!["rust".into(), "typescript".into()],
            disabled_tools: vec![],
        }),
        active_tone: Some(ToneSummary {
            id: "concise".into(),
            name: "Concise".into(),
            intensity_mode: "full".into(),
        }),
        mcp_servers: vec![McpServerStatus {
            name: "filesystem".into(),
            transport_mode: McpTransportMode::LocalProcess,
            tool_count: 5,
            connected: true,
            error: None,
        }],
        secret_backend: Some(SecretBackendStatus {
            backend: "keyring".into(),
            stored_count: 3,
            locked: false,
        }),
        context_class: "Maniple".into(),
        thinking_level: "High".into(),
        ..Default::default()
    });
    let backend = TestBackend::new(36, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| state.render_themed(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

// ═══════════════════════════════════════════════════════════════════
// Footer snapshots
// ═══════════════════════════════════════════════════════════════════

#[test]
fn snapshot_footer_default() {
    let footer = FooterData::default();
    let backend = TestBackend::new(120, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| footer.render(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_footer_with_model_and_context() {
    let footer = FooterData {
        model_id: "claude-sonnet-4-6".into(),
        model_provider: "Anthropic".into(),
        context_percent: 45.0,
        context_window: 200_000,
        context_class: ContextClass::Maniple,
        total_facts: 2400,
        injected_facts: 120,
        working_memory: 8,
        tool_calls: 47,
        turn: 15,
        estimated_tokens: 90_000,
        ..Default::default()
    };
    let backend = TestBackend::new(120, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| footer.render(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_footer_with_persona_and_mcp() {
    let mut footer = FooterData {
        model_id: "claude-sonnet-4-6".into(),
        model_provider: "Anthropic".into(),
        context_percent: 72.0,
        context_window: 272_000,
        context_class: ContextClass::Maniple,
        total_facts: 1800,
        injected_facts: 95,
        working_memory: 5,
        tool_calls: 23,
        turn: 8,
        estimated_tokens: 195_000,
        ..Default::default()
    };
    footer.harness = HarnessStatus {
        active_persona: Some(PersonaSummary {
            id: "eng".into(),
            name: "Systems Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 42,
            activated_skills: vec!["rust".into()],
            disabled_tools: vec![],
        }),
        active_tone: Some(ToneSummary {
            id: "concise".into(),
            name: "Concise".into(),
            intensity_mode: "full".into(),
        }),
        mcp_servers: vec![
            McpServerStatus {
                name: "fs".into(),
                transport_mode: McpTransportMode::LocalProcess,
                tool_count: 5,
                connected: true,
                error: None,
            },
            McpServerStatus {
                name: "db".into(),
                transport_mode: McpTransportMode::OciContainer,
                tool_count: 3,
                connected: true,
                error: None,
            },
        ],
        secret_backend: Some(SecretBackendStatus {
            backend: "keyring".into(),
            stored_count: 7,
            locked: false,
        }),
        ..Default::default()
    };
    let backend = TestBackend::new(120, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| footer.render(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_unified_footer_console() {
    let mut footer = FooterData {
        model_id: "ollama:qwen3".into(),
        model_provider: "ollama".into(),
        context_percent: 68.0,
        context_window: 262_144,
        context_class: ContextClass::Maniple,
        total_facts: 2440,
        injected_facts: 144,
        working_memory: 8,
        tool_calls: 23,
        turn: 8,
        compactions: 2,
        cwd: "/home/user/workspace/black-meridian/omegon".into(),
        thinking_level: "high".into(),
        model_tier: "victory".into(),
        provider_connected: true,
        is_oauth: false,
        ..Default::default()
    };
    footer.harness = HarnessStatus {
        active_persona: Some(PersonaSummary {
            id: "eng".into(),
            name: "Systems Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 42,
            activated_skills: vec!["rust".into()],
            disabled_tools: vec![],
        }),
        active_tone: Some(ToneSummary {
            id: "concise".into(),
            name: "Concise".into(),
            intensity_mode: "full".into(),
        }),
        capability_tier: "victory".into(),
        memory: MemoryStatus {
            total_facts: 2440,
            active_facts: 1800,
            project_facts: 1790,
            persona_facts: 10,
            working_facts: 8,
            episodes: 45,
            edges: 120,
            active_persona_mind: Some("Systems Engineer".into()),
        },
        mcp_servers: vec![McpServerStatus {
            name: "filesystem".into(),
            transport_mode: McpTransportMode::LocalProcess,
            tool_count: 5,
            connected: true,
            error: None,
        }],
        secret_backend: Some(SecretBackendStatus {
            backend: "keyring".into(),
            stored_count: 3,
            locked: false,
        }),
        ..Default::default()
    };

    let mut panel = InstrumentPanel::default();
    panel.update_mind_facts(2440, 8, 45, 0.11);
    panel.update_telemetry(68.0, 200_000, None, false, "high", Some((0, super::instruments::WaveDirection::Right)), true, 0.2);
    panel.tool_started("bash");
    panel.update_telemetry(68.0, 200_000, None, false, "high", None, true, 1.2);
    panel.tool_finished("bash", false);
    panel.tool_started("web_search");
    panel.update_telemetry(68.0, 200_000, None, false, "high", None, true, 8.1);
    panel.tool_finished("web_search", true);
    panel.tool_started("memory_recall");
    panel.update_telemetry(68.0, 200_000, None, false, "high", Some((0, super::instruments::WaveDirection::Left)), true, 0.22);
    panel.tool_finished("memory_recall", false);

    let backend = TestBackend::new(120, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let cols = ratatui::layout::Layout::horizontal([
                ratatui::layout::Constraint::Percentage(32),
                ratatui::layout::Constraint::Percentage(36),
                ratatui::layout::Constraint::Percentage(32),
            ])
            .split(f.area());
            footer.render_left_panel(cols[0], f, &Alpharius);
            panel.render_inference_panel(cols[1], f, &Alpharius);
            panel.render_tools_panel(cols[2], f, &Alpharius);
        })
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

#[test]
fn snapshot_tools_panel_with_runtime_and_error() {
    let mut panel = InstrumentPanel::default();
    panel.tool_started("bash");
    panel.update_telemetry(40.0, 200_000, None, false, "off", None, false, 41.0);
    panel.tool_finished("bash", false);
    panel.tool_started("web_search");
    panel.update_telemetry(40.0, 200_000, None, false, "off", None, false, 8.1);
    panel.tool_finished("web_search", true);
    panel.tool_started("memory_recall");
    panel.update_telemetry(40.0, 200_000, None, false, "off", None, false, 0.22);
    panel.tool_finished("memory_recall", false);
    panel.tool_started("codebase_search");
    panel.update_telemetry(40.0, 200_000, None, false, "off", None, false, 0.35);
    panel.tool_finished("codebase_search", false);

    let backend = TestBackend::new(42, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| panel.render_tools_panel(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

// ═══════════════════════════════════════════════════════════════════
// Bootstrap panel snapshots
// ═══════════════════════════════════════════════════════════════════

#[test]
fn snapshot_bootstrap_default() {
    let status = HarnessStatus::default();
    let output = super::bootstrap::render_bootstrap(&status, false);
    insta::assert_snapshot!(output);
}

#[test]
fn snapshot_bootstrap_full() {
    let status = HarnessStatus {
        providers: vec![
            ProviderStatus {
                name: "Anthropic".into(),
                authenticated: true,
                auth_method: Some("oauth".into()),
                model: Some("Claude Sonnet 4.6".into()),
            },
            ProviderStatus {
                name: "OpenAI".into(),
                authenticated: false,
                auth_method: None,
                model: None,
            },
        ],
        inference_backends: vec![InferenceBackendStatus {
            name: "Ollama".into(),
            kind: InferenceKind::External,
            available: true,
            models: vec![InferenceModelInfo {
                name: "qwen3:30b".into(),
                params: Some("30B".into()),
                context_window: Some(262144),
            }],
        }],
        mcp_servers: vec![McpServerStatus {
            name: "filesystem".into(),
            transport_mode: McpTransportMode::LocalProcess,
            tool_count: 5,
            connected: true,
            error: None,
        }],
        secret_backend: Some(SecretBackendStatus {
            backend: "keyring".into(),
            stored_count: 3,
            locked: false,
        }),
        container_runtime: Some(ContainerRuntimeStatus {
            runtime: "podman".into(),
            version: Some("5.3.1".into()),
            available: true,
        }),
        active_persona: Some(PersonaSummary {
            id: "eng".into(),
            name: "Systems Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 42,
            activated_skills: vec!["rust".into(), "typescript".into()],
            disabled_tools: vec![],
        }),
        active_tone: Some(ToneSummary {
            id: "concise".into(),
            name: "Concise".into(),
            intensity_mode: "full".into(),
        }),
        context_class: "Maniple".into(),
        thinking_level: "High".into(),
        capability_tier: "victory".into(),
        memory: MemoryStatus {
            total_facts: 2440,
            active_facts: 1800,
            project_facts: 1790,
            persona_facts: 10,
            working_facts: 8,
            episodes: 45,
            edges: 120,
            active_persona_mind: Some("Systems Engineer".into()),
        },
        ..Default::default()
    };
    let output = super::bootstrap::render_bootstrap(&status, false);
    insta::assert_snapshot!(output);
}

// ═══════════════════════════════════════════════════════════════════
// Selector snapshots
// ═══════════════════════════════════════════════════════════════════

#[test]
fn snapshot_context_selector() {
    use super::selector::{SelectOption, Selector};
    let selector = Selector::new(
        "Context Class",
        vec![
            SelectOption {
                label: "Squad (128k)".into(),
                value: "Squad".into(),
                description: "Standard sessions".into(),
                active: true,
            },
            SelectOption {
                label: "Maniple (272k)".into(),
                value: "Maniple".into(),
                description: "Extended analysis".into(),
                active: false,
            },
            SelectOption {
                label: "Clan (400k)".into(),
                value: "Clan".into(),
                description: "Large codebase".into(),
                active: false,
            },
            SelectOption {
                label: "Legion (1M+)".into(),
                value: "Legion".into(),
                description: "Massive context".into(),
                active: false,
            },
        ],
    );
    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| selector.render(f.area(), f, &Alpharius))
        .unwrap();
    insta::assert_snapshot!(render_to_string(&terminal));
}

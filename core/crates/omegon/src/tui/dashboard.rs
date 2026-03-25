//! Dashboard panel — design-tree + openspec state display.
//!
//! Rendered as a right-side panel when terminal width >= 100 columns.
//! Shows: focused design node, active openspec changes, session stats.
//! Uses shared widget primitives from `widgets.rs`.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::lifecycle::types::*;
use super::theme::Theme;
use super::widgets;

use std::sync::{Arc, Mutex};

use crate::features::cleave::CleaveProgress;
use crate::lifecycle::context::LifecycleContextProvider;
use crate::lifecycle::design;
use crate::status::HarnessStatus;

/// Shared session stats — written by the TUI, read by the web API.
#[derive(Default)]
pub struct SharedSessionStats {
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
}

/// Shared handles to feature state, for live dashboard updates.
#[derive(Clone, Default)]
pub struct DashboardHandles {
    pub lifecycle: Option<Arc<Mutex<LifecycleContextProvider>>>,
    pub cleave: Option<Arc<Mutex<CleaveProgress>>>,
    pub session: Arc<Mutex<SharedSessionStats>>,
    pub harness: Option<Arc<Mutex<HarnessStatus>>>,
}

impl DashboardHandles {
    /// Refresh dashboard state from the shared feature handles.
    pub fn refresh_into(&self, state: &mut DashboardState) {
        // Lifecycle
        if let Some(ref lp_lock) = self.lifecycle
            && let Ok(lp) = lp_lock.lock() {
                state.focused_node = lp.focused_node_id().and_then(|id| {
                    lp.get_node(id).map(|n| {
                        let sections = design::read_node_sections(n);
                        let assumptions = n.assumption_count();
                        let decisions_count = sections.as_ref().map(|s| s.decisions.iter().filter(|d| d.status == "decided").count()).unwrap_or(0);
                        let readiness = sections.as_ref().map(|s| s.readiness_score()).unwrap_or(0.0);
                        FocusedNodeSummary {
                            id: n.id.clone(),
                            title: n.title.clone(),
                            status: n.status,
                            open_questions: n.open_questions.len() - assumptions,
                            assumptions,
                            decisions: decisions_count,
                            readiness,
                        }
                    })
                });
                state.active_changes = lp.changes().iter()
                    .filter(|c| !matches!(c.stage, ChangeStage::Archived))
                    .map(|c| ChangeSummary {
                        name: c.name.clone(),
                        stage: c.stage,
                        done_tasks: c.done_tasks,
                        total_tasks: c.total_tasks,
                    })
                    .collect();

                // Status counts + node lists
                let nodes = lp.all_nodes();
                let mut counts = StatusCounts { total: nodes.len(), ..Default::default() };
                state.implementing_nodes.clear();
                state.actionable_nodes.clear();
                state.all_nodes.clear();

                for node in nodes.values() {
                    match node.status {
                        NodeStatus::Implementing => { counts.implementing += 1; },
                        NodeStatus::Decided => { counts.decided += 1; },
                        NodeStatus::Exploring => { counts.exploring += 1; },
                        NodeStatus::Implemented => { counts.implemented += 1; },
                        NodeStatus::Blocked => { counts.blocked += 1; },
                        _ => {},
                    }
                    counts.open_questions += node.open_questions.len();

                    let summary = NodeSummary {
                        id: node.id.clone(),
                        title: node.title.clone(),
                        status: node.status,
                        open_questions: node.open_questions.len(),
                        parent: node.parent.clone(),
                    };

                    // Collect active nodes for tree view
                    if !matches!(node.status, NodeStatus::Implemented) {
                        state.all_nodes.push(summary.clone());
                    }
                    if matches!(node.status, NodeStatus::Implementing) {
                        state.implementing_nodes.push(summary.clone());
                    }
                    if matches!(node.status, NodeStatus::Decided) {
                        state.actionable_nodes.push(summary);
                    }
                }
                state.status_counts = counts;
        }

        // Cleave
        if let Some(ref cp_lock) = self.cleave
            && let Ok(cp) = cp_lock.lock() {
                state.cleave = Some(cp.clone());
        }

        // Harness
        if let Some(ref harness_lock) = self.harness
            && let Ok(harness) = harness_lock.lock() {
                state.harness = Some(harness.clone());
        }
    }
}

/// Dashboard state — updated from lifecycle scanning.
#[derive(Default)]
pub struct DashboardState {
    pub focused_node: Option<FocusedNodeSummary>,
    pub active_changes: Vec<ChangeSummary>,
    pub cleave: Option<CleaveProgress>,
    pub harness: Option<HarnessStatus>,
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
    // Enriched: status counts + node lists
    pub status_counts: StatusCounts,
    pub implementing_nodes: Vec<NodeSummary>,
    pub actionable_nodes: Vec<NodeSummary>,
    /// All nodes for tree rendering (active statuses only).
    pub all_nodes: Vec<NodeSummary>,
    /// Tree widget selection state.
    pub tree_state: tui_tree_widget::TreeState<String>,
    // Context gauge
    pub context_used_pct: f32,
    pub context_window_k: usize,

}

#[derive(Default, Clone)]
pub struct StatusCounts {
    pub total: usize,
    pub implementing: usize,
    pub decided: usize,
    pub exploring: usize,
    pub implemented: usize,
    pub blocked: usize,
    pub open_questions: usize,
}

#[derive(Clone)]
pub struct NodeSummary {
    pub id: String,
    pub title: String,
    pub status: NodeStatus,
    pub open_questions: usize,
    pub parent: Option<String>,
}

#[derive(Clone)]
pub struct FocusedNodeSummary {
    pub id: String,
    pub title: String,
    pub status: NodeStatus,
    pub open_questions: usize,
    pub assumptions: usize,
    pub decisions: usize,
    pub readiness: f32,
}

#[derive(Clone)]
pub struct ChangeSummary {
    pub name: String,
    pub stage: ChangeStage,
    pub done_tasks: usize,
    pub total_tasks: usize,
}

impl DashboardState {
    pub fn render(&mut self, area: Rect, frame: &mut Frame) {
        self.render_themed(area, frame, &super::theme::Alpharius);
    }

    pub fn render_themed(&mut self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        // Clear the dashboard area first — ratatui uses diff-based rendering,
        // so stale conversation text from a previous frame (before the dashboard
        // appeared) would bleed through any cells the dashboard doesn't overwrite.
        frame.render_widget(ratatui::widgets::Clear, area);

        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(t.border_dim()))
            .style(Style::default().bg(t.bg()));

        // Render the block border, then work inside its inner area
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Dashboard content area (fractal removed)
        let text_area = inner;
        let inner_w = text_area.width.saturating_sub(1) as usize;
        let mut lines: Vec<Line> = Vec::new();

        // Dashboard title
        lines.push(Line::from(Span::styled(
            " Ω Dashboard ",
            Style::default().fg(t.accent()).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // ─── Status Counts (pipeline) ───────────────────────────
        if self.status_counts.total > 0 {
            let c = &self.status_counts;
            lines.push(Line::from(vec![
                Span::styled(format!("{}", c.total), Style::default().fg(t.fg()).add_modifier(Modifier::BOLD)),
                Span::styled(" nodes", Style::default().fg(t.dim())),
            ]));
            let mut badge_parts: Vec<Span<'static>> = Vec::new();
            if c.implementing > 0 {
                badge_parts.extend(widgets::badge("⚙", &c.implementing.to_string(), t.warning()));
                badge_parts.push(Span::styled(" ", Style::default()));
            }
            if c.decided > 0 {
                badge_parts.extend(widgets::badge("●", &c.decided.to_string(), t.success()));
                badge_parts.push(Span::styled(" ", Style::default()));
            }
            if c.exploring > 0 {
                badge_parts.extend(widgets::badge("◐", &c.exploring.to_string(), t.accent()));
                badge_parts.push(Span::styled(" ", Style::default()));
            }
            if c.implemented > 0 {
                badge_parts.extend(widgets::badge("✓", &c.implemented.to_string(), t.dim()));
            }
            if !badge_parts.is_empty() {
                lines.push(Line::from(badge_parts));
            }
            if c.open_questions > 0 || c.blocked > 0 {
                let mut parts: Vec<Span<'static>> = Vec::new();
                if c.blocked > 0 {
                    parts.extend(widgets::badge("✕", &c.blocked.to_string(), t.error()));
                    parts.push(Span::styled(" ", Style::default()));
                }
                if c.open_questions > 0 {
                    parts.extend(widgets::badge("?", &c.open_questions.to_string(), t.warning()));
                }
                lines.push(Line::from(parts));
            }
            // Pipeline funnel: exploring → decided → implementing → done
            let funnel_w = inner_w.saturating_sub(2);
            if funnel_w >= 16 && c.total > 0 {
                let total = c.total as f32;
                let seg = |count: usize, ch: &str, color: Color| -> Span<'static> {
                    let w = ((count as f32 / total) * funnel_w as f32).round().max(if count > 0 { 1.0 } else { 0.0 }) as usize;
                    Span::styled(ch.repeat(w), Style::default().fg(color))
                };
                lines.push(Line::from(vec![
                    Span::styled(" ", Style::default()),
                    seg(c.exploring, "░", t.accent()),
                    seg(c.decided, "▒", t.success()),
                    seg(c.implementing, "▓", t.warning()),
                    seg(c.implemented, "█", t.dim()),
                ]));
            }
            lines.push(Line::from(""));
        }

        // ─── Focused Node ───────────────────────────────────────
        if let Some(ref node) = self.focused_node {
            lines.push(widgets::section_divider("focus", inner_w, t));
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", node.status.icon()),
                    Style::default().fg(status_color(node.status, t)),
                ),
                Span::styled(node.id.clone(), t.style_heading()),
            ]));
            let title = widgets::truncate_str(&node.title, inner_w.saturating_sub(4), "…");
            lines.push(Line::from(Span::styled(format!("    {title}"), t.style_muted())));
            if node.decisions > 0 || node.open_questions > 0 || node.assumptions > 0 {
                let mut parts: Vec<Span<'static>> = vec![Span::styled("    ", Style::default())];
                if node.decisions > 0 {
                    parts.extend(widgets::badge("✓", &node.decisions.to_string(), t.success()));
                    parts.push(Span::styled(" ", Style::default()));
                }
                if node.open_questions > 0 {
                    parts.extend(widgets::badge("?", &node.open_questions.to_string(), t.warning()));
                    parts.push(Span::styled(" ", Style::default()));
                }
                if node.assumptions > 0 {
                    parts.extend(widgets::badge("⚠", &node.assumptions.to_string(), t.caution()));
                    parts.push(Span::styled(" ", Style::default()));
                }
                // Readiness gauge
                let pct = (node.readiness * 100.0) as u8;
                let readiness_color = if pct >= 80 { t.success() } else if pct >= 50 { t.warning() } else { t.error() };
                parts.push(Span::styled(format!("{pct}%"), Style::default().fg(readiness_color)));
                lines.push(Line::from(parts));
            }
            lines.push(Line::from(""));
        }

        // ─── Active Nodes (tree view) ────────────────────────────
        if !self.all_nodes.is_empty() {
            lines.push(widgets::section_divider("nodes", inner_w, t));
            // Build a flat tree view with indentation based on parent-child
            let roots: Vec<&NodeSummary> = self.all_nodes.iter()
                .filter(|n| {
                    // Root if no parent, or parent is not in our active set
                    n.parent.is_none() || !self.all_nodes.iter().any(|p| Some(&p.id) == n.parent.as_ref())
                })
                .collect();

            fn render_tree_node<'a>(
                node: &NodeSummary, depth: usize, all: &[NodeSummary],
                lines: &mut Vec<Line<'a>>, inner_w: usize, t: &dyn Theme, limit: &mut usize,
            ) {
                if *limit == 0 { return; }
                *limit -= 1;
                let indent = "  ".repeat(depth + 1);
                let (icon, color) = match node.status {
                    NodeStatus::Implementing => ("⚙", t.warning()),
                    NodeStatus::Decided => ("●", t.success()),
                    NodeStatus::Exploring => ("◐", t.accent()),
                    NodeStatus::Blocked => ("✕", t.error()),
                    _ => ("○", t.dim()),
                };
                let max_id = inner_w.saturating_sub(indent.len() + 3);
                let label = widgets::truncate_str(&node.id, max_id, "…");
                lines.push(Line::from(vec![
                    Span::styled(indent, Style::default()),
                    Span::styled(format!("{icon} "), Style::default().fg(color)),
                    Span::styled(label.to_string(), Style::default().fg(t.fg())),
                ]));
                // Render children
                let children: Vec<&NodeSummary> = all.iter()
                    .filter(|n| n.parent.as_deref() == Some(&node.id))
                    .collect();
                for child in children {
                    render_tree_node(child, depth + 1, all, lines, inner_w, t, limit);
                }
            }

            let mut limit = 20_usize; // cap total displayed
            for root in &roots {
                render_tree_node(root, 0, &self.all_nodes, &mut lines, inner_w, t, &mut limit);
            }
            if limit == 0 && self.all_nodes.len() > 20 {
                lines.push(Line::from(Span::styled(
                    format!("  … +{} more", self.all_nodes.len().saturating_sub(20)),
                    Style::default().fg(t.dim()),
                )));
            }
            lines.push(Line::from(""));
        }

        // ─── Active Changes ─────────────────────────────────────
        if !self.active_changes.is_empty() {
            lines.push(widgets::section_divider("openspec", inner_w, t));
            for change in &self.active_changes {
                let (icon, color) = stage_badge(change.stage, t);
                let progress = if change.total_tasks > 0 {
                    format!(" {}/{}", change.done_tasks, change.total_tasks)
                } else {
                    String::new()
                };
                let mut spans: Vec<Span<'static>> = vec![Span::styled("  ", Style::default())];
                spans.extend(widgets::badge(icon, &change.name, color));
                if !progress.is_empty() {
                    spans.push(Span::styled(progress, Style::default().fg(t.dim())));
                }
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
        }

        // ─── Cleave Progress (only if active) ───────────────────
        if let Some(ref cleave) = self.cleave
            && (cleave.active || cleave.total_children > 0) {
                lines.push(widgets::section_divider("cleave", inner_w, t));
                if cleave.active {
                    let done = cleave.completed + cleave.failed;
                    lines.push(Line::from(Span::styled(
                        format!("  ⟳ {}/{} children", done, cleave.total_children),
                        Style::default().fg(t.warning()),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        format!("  ✓ {} ok, {} failed", cleave.completed, cleave.failed),
                        Style::default().fg(if cleave.failed > 0 { t.error() } else { t.success() }),
                    )));
                }
                for child in &cleave.children {
                    let (icon, color) = match child.status.as_str() {
                        "completed" => ("✓", t.success()),
                        "failed" => ("✗", t.error()),
                        "running" => ("⟳", t.warning()),
                        _ => ("○", t.dim()),
                    };
                    let dur = child.duration_secs.map(|d| format!(" {:.0}s", d)).unwrap_or_default();
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {icon} "), Style::default().fg(color)),
                        Span::styled(
                            widgets::truncate_str(&child.label, inner_w.saturating_sub(8), "…").to_string(),
                            Style::default().fg(t.muted()),
                        ),
                        Span::styled(dur, Style::default().fg(t.dim())),
                    ]));
                }
                lines.push(Line::from(""));
        }

        // ─── Harness Status ─────────────────────────────────────
        if let Some(ref harness) = self.harness {
            lines.push(widgets::section_divider("harness", inner_w, t));
            
            // Active persona
            if let Some(ref persona) = harness.active_persona {
                lines.push(Line::from(vec![
                    Span::styled("  ".to_string(), Style::default()),
                    Span::styled(format!("{} ", persona.badge), Style::default().fg(t.accent())),
                    Span::styled(persona.name.clone(), Style::default().fg(t.fg())),
                    Span::styled(format!(" ({})", persona.activated_skills.len()), Style::default().fg(t.dim())),
                ]));
            }
            
            // Active tone
            if let Some(ref tone) = harness.active_tone {
                lines.push(Line::from(vec![
                    Span::styled("  ♪ ".to_string(), Style::default().fg(t.dim())),
                    Span::styled(tone.name.clone(), Style::default().fg(t.muted())),
                    Span::styled(format!(" ({})", tone.intensity_mode), Style::default().fg(t.dim())),
                ]));
            }
            
            // Provider auth status
            if !harness.providers.is_empty() {
                let mut auth_parts = vec![Span::styled("  ".to_string(), Style::default())];
                for (i, provider) in harness.providers.iter().enumerate() {
                    if i > 0 { auth_parts.push(Span::styled(" ".to_string(), Style::default())); }
                    let icon = if provider.authenticated { "●" } else { "○" };
                    let color = if provider.authenticated { t.success() } else { t.error() };
                    auth_parts.push(Span::styled(format!("{} ", icon), Style::default().fg(color)));
                    auth_parts.push(Span::styled(provider.name.clone(), Style::default().fg(t.muted())));
                }
                lines.push(Line::from(auth_parts));
            }
            
            // MCP servers
            let connected_servers = harness.mcp_servers.iter().filter(|s| s.connected).count();
            let total_servers = harness.mcp_servers.len();
            let tool_count = harness.mcp_tool_count();
            let error_count = harness.mcp_errors().len();
            if total_servers > 0 {
                let mut mcp_parts = vec![
                    Span::styled("  MCP ".to_string(), Style::default().fg(t.dim())),
                    Span::styled(format!("{}/{}", connected_servers, total_servers), 
                               Style::default().fg(if connected_servers == total_servers { t.success() } else { t.warning() })),
                ];
                if tool_count > 0 {
                    mcp_parts.push(Span::styled(format!(" ({} tools)", tool_count), Style::default().fg(t.dim())));
                }
                if error_count > 0 {
                    mcp_parts.push(Span::styled(format!(" {} errors", error_count), Style::default().fg(t.error())));
                }
                lines.push(Line::from(mcp_parts));
            }
            
            // Secrets store
            if let Some(ref secrets) = harness.secret_backend {
                let lock_icon = if secrets.locked { "🔒" } else { "🔓" };
                let lock_color = if secrets.locked { t.warning() } else { t.success() };
                lines.push(Line::from(vec![
                    Span::styled("  ".to_string(), Style::default()),
                    Span::styled(format!("{} ", lock_icon), Style::default().fg(lock_color)),
                    Span::styled(secrets.backend.clone(), Style::default().fg(t.muted())),
                    Span::styled(format!(" ({} secrets)", secrets.stored_count), Style::default().fg(t.dim())),
                ]));
            }
            
            // Inference backends
            let available_backends = harness.inference_backends.iter().filter(|b| b.available).count();
            if available_backends > 0 {
                for backend in &harness.inference_backends {
                    if backend.available {
                        let status_icon = "⚡";
                        lines.push(Line::from(vec![
                            Span::styled("  ".to_string(), Style::default()),
                            Span::styled(format!("{} ", status_icon), Style::default().fg(t.success())),
                            Span::styled(backend.name.clone(), Style::default().fg(t.muted())),
                            Span::styled(format!(" ({} models)", backend.models.len()), Style::default().fg(t.dim())),
                        ]));
                    }
                }
            }
            
            // Container runtime
            if let Some(ref runtime) = harness.container_runtime {
                if runtime.available {
                    let version = runtime.version.as_deref().unwrap_or("unknown");
                    lines.push(Line::from(vec![
                        Span::styled("  🐳 ".to_string(), Style::default().fg(t.accent())),
                        Span::styled(runtime.runtime.clone(), Style::default().fg(t.muted())),
                        Span::styled(format!(" v{}", version), Style::default().fg(t.dim())),
                    ]));
                }
            }
            
            lines.push(Line::from(""));
        }

        // ─── Session Stats ──────────────────────────────────────
        lines.push(widgets::section_divider("session", inner_w, t));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format!("{}", self.turns), Style::default().fg(t.fg())),
            Span::styled(" turns · ", Style::default().fg(t.dim())),
            Span::styled(format!("{}", self.tool_calls), Style::default().fg(t.fg())),
            Span::styled(" tool calls", Style::default().fg(t.dim())),
        ]));
        if self.compactions > 0 {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(format!("{}", self.compactions), Style::default().fg(t.fg())),
                Span::styled(" compactions", Style::default().fg(t.dim())),
            ]));
        }

        let widget = Paragraph::new(lines)
            .wrap(Wrap { trim: true });
        frame.render_widget(widget, text_area);
    }
}

fn format_k(tokens: usize) -> String {
    if tokens >= 1_000_000 { format!("{}M", tokens / 1_000_000) }
    else { format!("{}k", tokens / 1000) }
}

fn status_color(status: NodeStatus, t: &dyn Theme) -> Color {
    match status {
        NodeStatus::Seed => t.dim(),
        NodeStatus::Exploring => t.accent(),
        NodeStatus::Resolved | NodeStatus::Decided | NodeStatus::Implemented => t.success(),
        NodeStatus::Implementing => t.warning(),
        NodeStatus::Blocked => t.error(),
        NodeStatus::Deferred => t.caution(),
    }
}

fn stage_badge(stage: ChangeStage, t: &dyn Theme) -> (&'static str, Color) {
    match stage {
        ChangeStage::Proposed => ("◌", t.dim()),
        ChangeStage::Specified => ("◐", t.dim()),
        ChangeStage::Planned => ("▸", t.muted()),
        ChangeStage::Implementing => ("⟳", t.warning()),
        ChangeStage::Verifying => ("◉", t.success()),
        ChangeStage::Archived => ("✓", t.success()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::cleave::ChildProgress;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn buf_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let area = buf.area;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| buf[(x, y)].symbol().to_string()))
            .collect()
    }

    #[test]
    fn empty_dashboard_renders() {
        let mut state = DashboardState::default();
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
    }

    #[test]
    fn dashboard_with_focused_node() {
        let mut state = DashboardState::default();
        state.focused_node = Some(FocusedNodeSummary {
            id: "test-node".into(),
            title: "Test Node".into(),
            status: NodeStatus::Exploring,
            open_questions: 3,
            assumptions: 1,
            decisions: 2,
            readiness: 0.33,
        });
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(text.contains("test-node"), "should render node id: {text}");
    }

    #[test]
    fn dashboard_with_changes() {
        let mut state = DashboardState::default();
        state.active_changes = vec![ChangeSummary {
            name: "my-change".into(),
            stage: ChangeStage::Implementing,
            done_tasks: 3,
            total_tasks: 8,
        }];
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(text.contains("my-change"), "should render change name: {text}");
    }

    #[test]
    fn dashboard_with_cleave_progress() {
        let mut state = DashboardState::default();
        state.cleave = Some(CleaveProgress {
            active: true,
            run_id: "clv-test".into(),
            total_children: 3,
            completed: 1,
            failed: 0,
            children: vec![
                ChildProgress { label: "task-a".into(), status: "completed".into(), duration_secs: Some(12.0) },
                ChildProgress { label: "task-b".into(), status: "running".into(), duration_secs: None },
                ChildProgress { label: "task-c".into(), status: "pending".into(), duration_secs: None },
            ],
        });
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(text.contains("1/3"), "should show progress: {text}");
    }

    #[test]
    fn dashboard_handles_refresh_empty() {
        let handles = DashboardHandles::default();
        let mut state = DashboardState::default();
        handles.refresh_into(&mut state);
        assert!(state.focused_node.is_none());
        assert!(state.active_changes.is_empty());
    }

    #[test]
    fn session_stats_render() {
        let mut state = DashboardState::default();
        state.turns = 15;
        state.tool_calls = 42;
        state.compactions = 2;
        let backend = TestBackend::new(36, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(text.contains("15"), "should show turns: {text}");
        assert!(text.contains("42"), "should show tool calls: {text}");
    }

    #[test]
    fn status_color_mapping() {
        let t = super::super::theme::Alpharius;
        assert_eq!(status_color(NodeStatus::Seed, &t), t.dim());
        assert_eq!(status_color(NodeStatus::Exploring, &t), t.accent());
        assert_eq!(status_color(NodeStatus::Implemented, &t), t.success());
        assert_eq!(status_color(NodeStatus::Blocked, &t), t.error());
    }

    #[test]
    fn stage_badge_mapping() {
        let t = super::super::theme::Alpharius;
        let (icon, _) = stage_badge(ChangeStage::Implementing, &t);
        assert_eq!(icon, "⟳");
        let (icon, _) = stage_badge(ChangeStage::Archived, &t);
        assert_eq!(icon, "✓");
    }

    #[test]
    fn dashboard_with_status_counts() {
        let mut state = DashboardState::default();
        state.status_counts = StatusCounts {
            total: 140,
            implementing: 7,
            decided: 5,
            exploring: 5,
            implemented: 100,
            blocked: 0,
            open_questions: 24,
        };
        let backend = TestBackend::new(36, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();

        let text = buf_text(&terminal);
        assert!(text.contains("140"), "should show total: {text}");
    }

    #[test]
    fn dashboard_with_implementing_nodes() {
        let mut state = DashboardState::default();
        state.status_counts.total = 10;
        let nodes = vec![
            NodeSummary { id: "rust-tui".into(), title: "Rust TUI".into(), status: NodeStatus::Implementing, open_questions: 2, parent: None },
            NodeSummary { id: "web-dash".into(), title: "Web Dashboard".into(), status: NodeStatus::Implementing, open_questions: 0, parent: Some("rust-tui".into()) },
        ];
        state.implementing_nodes = nodes.clone();
        state.all_nodes = nodes;
        let backend = TestBackend::new(36, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();

        let text = buf_text(&terminal);
        assert!(text.contains("rust-tui"), "should show implementing node: {text}");
    }

    #[test]
    fn format_k_values() {
        assert_eq!(format_k(200_000), "200k");
        assert_eq!(format_k(1_000_000), "1M");
    }

    #[test]
    fn dashboard_with_harness_status() {
        let mut state = DashboardState::default();
        state.harness = Some(crate::status::HarnessStatus {
            active_persona: Some(crate::status::PersonaSummary {
                id: "syseng".into(),
                name: "System Engineer".into(),
                badge: "⚙".into(),
                mind_facts_count: 42,
                activated_skills: vec!["rust".into(), "debugging".into()],
                disabled_tools: vec![],
            }),
            active_tone: Some(crate::status::ToneSummary {
                id: "concise".into(),
                name: "Concise".into(),
                intensity_mode: "full".into(),
            }),
            providers: vec![
                crate::status::ProviderStatus {
                    name: "Anthropic".into(),
                    authenticated: true,
                    auth_method: Some("oauth".into()),
                    model: Some("claude-sonnet-4-6".into()),
                },
                crate::status::ProviderStatus {
                    name: "OpenAI".into(),
                    authenticated: false,
                    auth_method: None,
                    model: None,
                },
            ],
            mcp_servers: vec![
                crate::status::McpServerStatus {
                    name: "filesystem".into(),
                    transport_mode: crate::status::McpTransportMode::LocalProcess,
                    tool_count: 8,
                    connected: true,
                    error: None,
                },
            ],
            secret_backend: Some(crate::status::SecretBackendStatus {
                backend: "keyring".into(),
                stored_count: 5,
                locked: false,
            }),
            inference_backends: vec![
                crate::status::InferenceBackendStatus {
                    name: "Ollama".into(),
                    kind: crate::status::InferenceKind::External,
                    available: true,
                    models: vec![
                        crate::status::InferenceModelInfo {
                            name: "llama3.2:3b".into(),
                            params: Some("3B".into()),
                            context_window: Some(131072),
                        },
                    ],
                },
            ],
            container_runtime: Some(crate::status::ContainerRuntimeStatus {
                runtime: "podman".into(),
                version: Some("5.3.1".into()),
                available: true,
            }),
            ..Default::default()
        });
        
        let backend = TestBackend::new(50, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(text.contains("System Engineer"), "should show persona name: {text}");
        assert!(text.contains("Concise"), "should show tone: {text}");
        assert!(text.contains("Anthropic"), "should show provider: {text}");
        assert!(text.contains("MCP"), "should show MCP servers: {text}");
        assert!(text.contains("keyring"), "should show secrets backend: {text}");
        assert!(text.contains("Ollama"), "should show inference backend: {text}");
        assert!(text.contains("podman"), "should show container runtime: {text}");
    }

    #[test]
    fn dashboard_handles_refresh_with_harness() {
        let harness_status = Arc::new(Mutex::new(crate::status::HarnessStatus {
            active_persona: Some(crate::status::PersonaSummary {
                id: "test".into(),
                name: "Test Persona".into(),
                badge: "🧪".into(),
                mind_facts_count: 10,
                activated_skills: vec!["rust".into()],
                disabled_tools: vec![],
            }),
            ..Default::default()
        }));
        
        let handles = DashboardHandles {
            harness: Some(harness_status),
            ..Default::default()
        };
        let mut state = DashboardState::default();
        handles.refresh_into(&mut state);
        
        assert!(state.harness.is_some());
        assert_eq!(
            state.harness.unwrap().active_persona.unwrap().name,
            "Test Persona"
        );
    }

    #[test] 
    fn cleave_section_hidden_when_inactive() {
        let mut state = DashboardState::default();
        // No cleave data - should not render cleave section
        
        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(!text.contains("cleave"), "should not show cleave section when inactive: {text}");
    }

    #[test]
    fn cleave_section_shown_when_active() {
        let mut state = DashboardState::default();
        state.cleave = Some(CleaveProgress {
            active: true,
            run_id: "test-run".into(),
            total_children: 2,
            completed: 1,
            failed: 0,
            children: vec![
                ChildProgress {
                    label: "task-1".into(),
                    status: "completed".into(),
                    duration_secs: Some(5.0),
                },
                ChildProgress {
                    label: "task-2".into(),
                    status: "running".into(),
                    duration_secs: None,
                },
            ],
        });
        
        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            state.render_themed(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text = buf_text(&terminal);
        assert!(text.contains("cleave"), "should show cleave section when active: {text}");
        assert!(text.contains("1/2"), "should show progress: {text}");
    }
}

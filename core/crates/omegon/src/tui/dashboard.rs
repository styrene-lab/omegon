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
                        FocusedNodeSummary {
                            id: n.id.clone(),
                            title: n.title.clone(),
                            status: n.status,
                            open_questions: n.open_questions.len(),
                            decisions: sections.map(|s| s.decisions.len()).unwrap_or(0),
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
    }
}

/// Dashboard state — updated from lifecycle scanning.
#[derive(Default)]
pub struct DashboardState {
    pub focused_node: Option<FocusedNodeSummary>,
    pub active_changes: Vec<ChangeSummary>,
    pub cleave: Option<CleaveProgress>,
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
    pub decisions: usize,
}

#[derive(Clone)]
pub struct ChangeSummary {
    pub name: String,
    pub stage: ChangeStage,
    pub done_tasks: usize,
    pub total_tasks: usize,
}

impl DashboardState {
    pub fn render(&self, area: Rect, frame: &mut Frame) {
        self.render_themed(area, frame, &super::theme::Alpharius);
    }

    pub fn render_themed(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        // Clear the dashboard area first — ratatui uses diff-based rendering,
        // so stale conversation text from a previous frame (before the dashboard
        // appeared) would bleed through any cells the dashboard doesn't overwrite.
        frame.render_widget(ratatui::widgets::Clear, area);

        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(t.border_dim()))
            .title(Span::styled(" Ω Dashboard ", Style::default().fg(t.accent()).add_modifier(Modifier::BOLD)))
            .style(Style::default().bg(t.bg()));

        let inner_w = area.width.saturating_sub(3) as usize; // left border + padding
        let mut lines: Vec<Line<'static>> = Vec::new();

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
            if node.decisions > 0 || node.open_questions > 0 {
                let mut parts: Vec<Span<'static>> = vec![Span::styled("    ", Style::default())];
                if node.decisions > 0 {
                    parts.extend(widgets::badge("●", &node.decisions.to_string(), t.success()));
                    parts.push(Span::styled(" ", Style::default()));
                }
                if node.open_questions > 0 {
                    parts.extend(widgets::badge("?", &node.open_questions.to_string(), t.warning()));
                }
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

        // ─── Cleave Progress ─────────────────────────────────────
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
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(widget, area);
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
        let state = DashboardState::default();
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
            decisions: 2,
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
}

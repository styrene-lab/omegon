//! Shared live action feedback. Runtime/event owners remain authoritative;
//! rendering only reads their latest phase and bounded transient tool list.
use super::{ActivityToolState, App, SlimTurnState};
use crate::surfaces::activity::ActivityToolStatus;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::Instant;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const MAX_ACTION_INPUT_BYTES: usize = 512;
const CANCEL_HINT: &str = "Ctrl+C cancel";

struct LiveAction {
    phase: &'static str,
    tool: Option<String>,
}

impl App {
    fn live_action(&self, now: Instant) -> Option<LiveAction> {
        if !self.agent_active || !self.ui_surfaces.activity {
            return None;
        }
        // Operator-shell episodes have separate lifecycle/inspection ownership;
        // they are not evidence of work by this agent turn.
        let episode = format!("turn:{}", self.turn);
        let current = |tool: &&ActivityToolState| {
            tool.episode_id == episode && tool.expires_at.is_none_or(|deadline| now < deadline)
        };
        let running = self
            .activity_tools
            .iter()
            .filter(current)
            .filter(|tool| tool.status == ActivityToolStatus::Running)
            .collect::<Vec<_>>();
        let tool = running.first().copied().or_else(|| {
            self.activity_tools
                .iter()
                .filter(current)
                .find(|tool| tool.status == ActivityToolStatus::Error)
        });
        let phase = match &self.slim_turn_state {
            _ if self.interrupt_pending => "Canceling",
            SlimTurnState::Interrupting => "Canceling",
            SlimTurnState::UpstreamRetrying(_) => "Retrying provider",
            SlimTurnState::StreamIdle(_) => "Waiting for provider",
            SlimTurnState::Lifecycle(detail)
                if detail.contains("awaiting") || detail.contains("waiting") =>
            {
                "Waiting"
            }
            SlimTurnState::Finished("waiting" | "blocked") => "Waiting",
            _ if !running.is_empty() => "Working",
            SlimTurnState::Thinking => "Thinking",
            SlimTurnState::Responding => "Responding",
            _ => "Working",
        };
        let tool = tool.map(|tool| {
            let mut detail = if tool.status == ActivityToolStatus::Error {
                "Failed ".to_owned()
            } else {
                String::new()
            };
            let name = action_text(&tool.name);
            detail.push_str(if name.is_empty() { "tool" } else { &name });
            if running.len() > 1 {
                detail.push_str(&format!(" · {} running", running.len()));
            }
            if let Some(args) = tool.args_summary.as_deref() {
                let args = action_text(args);
                if !args.is_empty() {
                    detail.push_str(" · ");
                    detail.push_str(&args);
                }
            }
            detail
        });
        Some(LiveAction { phase, tool })
    }

    pub(super) fn live_action_height(&self) -> u16 {
        self.live_action(Instant::now())
            .map_or(0, |action| 1 + u16::from(action.tool.is_some()))
    }

    pub(super) fn render_live_action(&self, area: Rect, frame: &mut Frame) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let Some(action) = self.live_action(Instant::now()) else {
            return;
        };
        let width = usize::from(area.width);
        let phase_width = action.phase.width();
        let mut lines = Vec::new();
        if area.height == 1
            && let Some(tool) = action.tool.as_deref()
        {
            let show_cancel = width >= phase_width + CANCEL_HINT.width() + 12;
            let hint_width = if show_cancel {
                CANCEL_HINT.width() + 2
            } else {
                0
            };
            let mut spans = vec![Span::styled(
                fit_action(action.phase, width),
                self.theme.style_ui_title(),
            )];
            if width > phase_width + 3 + hint_width {
                spans.push(Span::styled(" · ", self.theme.style_ui_secondary()));
                spans.push(Span::styled(
                    fit_action(tool, width - phase_width - 3 - hint_width),
                    self.theme.style_ui_secondary(),
                ));
            }
            if show_cancel {
                append_cancel(&mut spans, width, self.theme.style_ui_hint());
            }
            lines.push(Line::from(spans));
        } else {
            let mut spans = vec![Span::styled(
                fit_action(action.phase, width),
                self.theme.style_ui_title(),
            )];
            append_cancel(&mut spans, width, self.theme.style_ui_hint());
            lines.push(Line::from(spans));
            if area.height > 1
                && let Some(tool) = action.tool
            {
                lines.push(Line::from(Span::styled(
                    fit_action(&tool, width),
                    self.theme.style_ui_secondary(),
                )));
            }
        }
        frame.render_widget(Paragraph::new(lines).style(self.theme.style_panel()), area);
    }
}

fn append_cancel(spans: &mut Vec<Span<'static>>, width: usize, style: ratatui::style::Style) {
    let used = spans.iter().map(|span| span.content.width()).sum::<usize>();
    let remaining = width.saturating_sub(used);
    if remaining >= CANCEL_HINT.width() + 2 {
        spans.push(Span::styled(
            format!(
                "{}{}",
                " ".repeat(remaining - CANCEL_HINT.width()),
                CANCEL_HINT
            ),
            style,
        ));
    }
}

/// Clamp before allocation/parsing. Preserve word separation when controls or
/// newlines appear in an upstream argument summary; terminal commands are inert.
fn action_text(input: &str) -> String {
    let end = input.floor_char_boundary(input.len().min(MAX_ACTION_INPUT_BYTES));
    let bounded = input[..end].replace(['\n', '\r', '\t'], " ");
    super::segments::strip_terminal_control(&bounded)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fit_action(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut cells = 0;
    let mut end = 0;
    let mut word = None;
    for (index, grapheme) in text.grapheme_indices(true) {
        if cells + grapheme.width() > width - 1 {
            break;
        }
        cells += grapheme.width();
        end = index + grapheme.len();
        if grapheme.chars().all(char::is_whitespace) {
            word = Some(index);
        }
    }
    if let Some(boundary) = word
        && boundary > 0
    {
        end = boundary;
    }
    format!("{}…", text[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{AgentEvent, theme::TerminalTheme};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Color};

    fn app() -> App {
        let mut app = App::new(crate::settings::shared("test"));
        app.theme = Box::new(TerminalTheme);
        app.ui_surfaces.activity = true;
        app
    }
    fn render(app: &App, area: Rect) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(area.right().max(2), area.bottom().max(2))).unwrap();
        terminal
            .draw(|frame| app.render_live_action(area, frame))
            .unwrap();
        terminal.backend().buffer().clone()
    }
    fn text(buffer: &Buffer) -> String {
        buffer
            .content
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn publish_stream(app: &mut App) {
        if let Some((generation, revision)) = app.publish_stream_presentation() {
            let _ = render(app, Rect::new(0, 0, 80, 2));
            app.acknowledge_stream_presentation_draw(generation, revision);
        }
    }

    fn tool_start(app: &mut App, id: &str, command: &str) {
        app.handle_drawn_agent_event(AgentEvent::ToolStart {
            id: id.into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            execution_origin: omegon_traits::ToolExecutionOrigin::Agent,
            args: serde_json::json!({"command": command}),
        });
    }
    fn tool_end(app: &mut App, id: &str, is_error: bool) {
        app.handle_drawn_agent_event(AgentEvent::ToolEnd {
            id: id.into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            execution_origin: omegon_traits::ToolExecutionOrigin::Agent,
            is_error,
            result: omegon_traits::ToolResult {
                content: Vec::new(),
                details: serde_json::Value::Null,
            },
        });
    }

    #[test]
    fn action_area_projects_request_thinking_response_and_live_tools() {
        let mut app = app();
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
        assert_eq!(app.live_action_height(), 1);
        assert!(text(&render(&app, Rect::new(0, 0, 80, 1))).contains("Working"));
        app.handle_drawn_agent_event(AgentEvent::ThinkingChunk {
            text: "private reasoning never displayed in action area".into(),
        });
        publish_stream(&mut app);
        let thinking = text(&render(&app, Rect::new(0, 0, 80, 1)));
        assert!(thinking.contains("Thinking"));
        assert!(!thinking.contains("private reasoning"));
        app.handle_drawn_agent_event(AgentEvent::MessageChunk {
            text: "answer text".into(),
        });
        publish_stream(&mut app);
        assert!(text(&render(&app, Rect::new(0, 0, 80, 1))).contains("Responding"));
        tool_start(&mut app, "one", "cargo test");
        app.handle_drawn_agent_event(AgentEvent::ThinkingChunk {
            text: "late reasoning".into(),
        });
        publish_stream(&mut app);
        let running = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(
            running.contains("Working")
                && running.contains("bash")
                && running.contains("cargo test"),
            "{running}"
        );
        assert!(!running.contains("Thinking"));
        assert_eq!(app.live_action_height(), 2);
    }

    #[test]
    fn action_area_selects_newest_running_tool_and_reports_concurrency() {
        let mut app = app();
        app.agent_active = true;
        tool_start(&mut app, "one", "first command");
        tool_start(&mut app, "two", "latest command");
        app.slim_turn_state = SlimTurnState::Responding;
        let output = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(
            output.contains("latest command") && output.contains("2 running"),
            "{output}"
        );
        assert!(!output.contains("first command") && !output.contains("Responding"));
        tool_end(&mut app, "two", false);
        let output = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(output.contains("first command") && !output.contains("latest command"));
        assert!(!output.contains("2 running"));
    }

    #[test]
    fn action_area_does_not_present_success_or_expired_failure_as_running() {
        let mut app = app();
        app.agent_active = true;
        tool_start(&mut app, "one", "completed command");
        tool_end(&mut app, "one", false);
        assert_eq!(app.live_action_height(), 1);
        assert!(!text(&render(&app, Rect::new(0, 0, 80, 2))).contains("completed command"));
        tool_start(&mut app, "two", "failing command");
        tool_end(&mut app, "two", true);
        assert_eq!(app.live_action_height(), 2);
        assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("Failed bash"));
        app.activity_tools.front_mut().unwrap().expires_at =
            Some(Instant::now() - std::time::Duration::from_secs(1));
        assert_eq!(app.live_action_height(), 1);
        assert!(!text(&render(&app, Rect::new(0, 0, 80, 2))).contains("Failed"));
    }

    #[test]
    fn action_area_prioritizes_cancel_and_identifies_retry_and_wait() {
        let mut app = app();
        app.agent_active = true;
        tool_start(&mut app, "one", "cargo test");
        app.slim_turn_state = SlimTurnState::Interrupting;
        assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("Canceling"));
        app.slim_turn_state = SlimTurnState::UpstreamRetrying("opaque details".into());
        let output = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(output.contains("Retrying provider") && !output.contains("opaque details"));
        app.slim_turn_state = SlimTurnState::StreamIdle("900s ambiguous reasoning".into());
        assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("Waiting for provider"));
        app.slim_turn_state = SlimTurnState::Lifecycle("turn awaiting operator".into());
        assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("Waiting"));
    }

    #[test]
    fn action_area_clears_on_authoritative_completion_and_idle_queue_then_reappears() {
        for supervisor in [true, false] {
            let mut app = app();
            app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
            tool_start(&mut app, "one", "cargo test");
            if supervisor {
                app.handle_drawn_agent_event(AgentEvent::RuntimeTurnLifecycleUpdated {
                    snapshot_json: serde_json::json!({"phase": "supervisor_completed"}),
                });
            } else {
                app.handle_drawn_agent_event(AgentEvent::RuntimeQueueUpdated {
                    snapshot_json: serde_json::json!({"depth": 0, "active": null, "items": []}),
                });
            }
            assert_eq!(app.live_action_height(), 0);
            assert!(
                text(&render(&app, Rect::new(0, 0, 80, 2)))
                    .trim()
                    .is_empty()
            );
            app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 2 });
            assert_eq!(app.live_action_height(), 1);
        }
    }

    #[test]
    fn action_area_new_turn_does_not_replay_lingering_failure() {
        let mut app = app();
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
        tool_start(&mut app, "failed", "previous command");
        tool_end(&mut app, "failed", true);
        assert_eq!(app.live_action_height(), 2);
        app.handle_drawn_agent_event(AgentEvent::RuntimeTurnLifecycleUpdated {
            snapshot_json: serde_json::json!({"phase": "supervisor_completed"}),
        });
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 2 });
        assert!(
            app.activity_tools
                .iter()
                .any(|tool| tool.status == ActivityToolStatus::Error),
            "inspection evidence must remain owned by the existing event projection"
        );
        assert_eq!(app.live_action_height(), 1);
        let output = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(!output.contains("Failed") && !output.contains("previous command"));
        tool_start(&mut app, "current", "current command");
        assert_eq!(app.live_action_height(), 2);
        assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("current command"));
    }

    #[test]
    fn action_area_new_runtime_prompt_clears_failure_when_loop_turn_one_repeats() {
        let mut app = app();
        app.handle_drawn_agent_event(AgentEvent::RuntimePromptStarted {
            runtime_turn_id: 41,
            text: "first prompt".into(),
            image_paths: Vec::new(),
        });
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
        tool_start(&mut app, "failed", "previous command");
        tool_end(&mut app, "failed", true);
        assert_eq!(app.live_action_height(), 2);
        app.handle_drawn_agent_event(AgentEvent::RuntimeTurnLifecycleUpdated {
            snapshot_json: serde_json::json!({"phase": "supervisor_completed", "turn_id": 41}),
        });
        assert_eq!(app.live_action_height(), 0);
        // Operator-shell episodes have their own lifecycle and must survive
        // agent-prompt cleanup; their durable and transient owners are separate.
        let mut operator_shell = app.activity_tools.front().unwrap().clone();
        operator_shell.episode_id = "operator-shell:owned-shell".into();
        app.activity_tools.push_back(operator_shell.clone());
        app.handle_drawn_agent_event(AgentEvent::RuntimePromptStarted {
            runtime_turn_id: 42,
            text: "second prompt".into(),
            image_paths: Vec::new(),
        });
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
        assert_eq!(app.live_action_height(), 1);
        let output = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(!output.contains("Failed") && !output.contains("previous command"));
        assert_eq!(
            app.activity_tools.iter().collect::<Vec<_>>(),
            [&operator_shell]
        );
        assert!(
            app.conversation.segments().iter().any(|segment| matches!(
                &segment.content,
                crate::tui::segments::SegmentContent::ToolCard { is_error: true, .. }
            )),
            "prompt cleanup must preserve the failure in the conversation"
        );
        tool_start(&mut app, "new", "current command");
        assert_eq!(app.live_action_height(), 2);
        assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("current command"));
    }

    #[test]
    fn action_area_pending_cancel_survives_late_stream_and_tool_events() {
        let mut app = app();
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
        app.interrupt_pending = true;
        app.slim_turn_state = SlimTurnState::Interrupting;
        for event in [
            AgentEvent::MessageChunk {
                text: "late answer".into(),
            },
            AgentEvent::ThinkingChunk {
                text: "late reasoning".into(),
            },
        ] {
            app.handle_drawn_agent_event(event);
            publish_stream(&mut app);
            assert!(text(&render(&app, Rect::new(0, 0, 80, 2))).contains("Canceling"));
        }
        tool_start(&mut app, "late", "late command");
        let output = text(&render(&app, Rect::new(0, 0, 80, 2)));
        assert!(output.contains("Canceling") && output.contains("late command"));
        app.handle_drawn_agent_event(AgentEvent::RuntimeTurnLifecycleUpdated {
            snapshot_json: serde_json::json!({"phase": "supervisor_completed"}),
        });
        assert_eq!(app.live_action_height(), 0);
    }

    #[test]
    fn action_area_does_not_attribute_operator_shell_to_agent_turn() {
        let mut app = app();
        app.handle_drawn_agent_event(AgentEvent::TurnStart { turn: 1 });
        tool_start(&mut app, "shell", "operator command");
        app.activity_tools.front_mut().unwrap().episode_id = "operator-shell:shell".into();
        assert_eq!(app.live_action_height(), 1);
        assert!(!text(&render(&app, Rect::new(0, 0, 80, 2))).contains("operator command"));
    }

    #[test]
    fn action_area_hidden_and_zero_geometry_leave_canvas_untouched() {
        let mut app = app();
        app.agent_active = true;
        app.ui_surfaces.activity = false;
        assert_eq!(app.live_action_height(), 0);
        let hidden = render(&app, Rect::new(0, 0, 80, 2));
        assert!(
            hidden
                .content
                .iter()
                .all(|cell| cell.symbol() == " " && cell.bg == Color::Reset)
        );
        app.ui_surfaces.activity = true;
        for area in [Rect::new(0, 0, 0, 2), Rect::new(0, 0, 2, 0)] {
            let empty = render(&app, area);
            assert!(
                empty
                    .content
                    .iter()
                    .all(|cell| cell.symbol() == " " && cell.bg == Color::Reset)
            );
        }
    }

    #[test]
    fn action_area_combines_one_row_and_uses_panel_palette_only_inside_area() {
        let mut app = app();
        app.agent_active = true;
        tool_start(&mut app, "one", "cargo test");
        let buffer = render(&app, Rect::new(1, 1, 80, 1));
        let output = text(&buffer);
        assert!(
            output.contains("Working · bash")
                && output.contains("cargo test")
                && output.contains("Ctrl+C cancel"),
            "{output}"
        );
        assert_eq!(buffer[(1, 1)].fg, Color::Indexed(255));
        assert_eq!(buffer[(1, 1)].bg, Color::Indexed(235));
        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
        for width in 1..24 {
            let output = text(&render(&app, Rect::new(0, 0, width, 1)));
            assert!(!output.contains('\u{1b}'));
        }
    }

    #[test]
    fn action_area_sanitizes_and_bounds_external_summaries_before_truncation() {
        let input =
            "alpha\x1b[2J beta\n\t gamma\x1b]52;c;hidden\nclipboard\x07 delta\x1bPprivate\x1b\\";
        assert_eq!(action_text(input), "alpha beta gamma delta");
        assert!(action_text(&"界".repeat(1000)).len() <= MAX_ACTION_INPUT_BYTES);
        for width in 0..30 {
            let result = fit_action("界é 👩\u{200d}💻 ordinary readable words", width);
            assert!(result.width() <= width, "{width}: {result}");
            if result.contains('👩') {
                assert!(result.contains("👩\u{200d}💻"));
            }
            if result.contains('e') && width < 15 {
                assert!(result.contains("é"));
            }
        }
        assert_eq!(
            fit_action("ordinary readable words", 20),
            "ordinary readable…"
        );
    }
}

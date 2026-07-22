//! Read-only modal projection for managed execution sessions.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::theme::Theme;

#[derive(Debug, Clone)]
pub(crate) struct ProcessViewerState {
    pub session_id: String,
    pub scroll: u16,
    pub follow: bool,
    pub confirm_stop: bool,
}

impl ProcessViewerState {
    pub(crate) fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            scroll: 0,
            follow: true,
            confirm_stop: false,
        }
    }

    pub(crate) fn scroll_up(&mut self) {
        self.follow = false;
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub(crate) fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub(crate) fn toggle_follow(&mut self) {
        self.confirm_stop = false;
        self.follow = !self.follow;
        if self.follow {
            self.scroll = 0;
        }
    }

    pub(crate) fn switch_session(&mut self, delta: isize) {
        let sessions = crate::tools::terminal::execution_session_snapshots();
        if sessions.is_empty() {
            return;
        }
        let current = sessions
            .iter()
            .position(|snapshot| snapshot.id == self.session_id)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(sessions.len() as isize) as usize;
        self.session_id = sessions[next].id.clone();
        self.scroll = 0;
        self.follow = true;
        self.confirm_stop = false;
    }

    pub(crate) fn request_stop(&mut self) -> bool {
        if self.confirm_stop {
            self.confirm_stop = false;
            true
        } else {
            self.confirm_stop = true;
            false
        }
    }

    pub(crate) fn cancel_confirmation(&mut self) -> bool {
        std::mem::take(&mut self.confirm_stop)
    }
}

pub(crate) fn render_process_viewer(
    frame: &mut Frame,
    area: Rect,
    theme: &dyn Theme,
    state: &ProcessViewerState,
) {
    let popup = super::command_surfaces::command_modal_area(area);
    frame.render_widget(Clear, popup);

    let snapshot = crate::tools::terminal::execution_session_snapshot_by_id(&state.session_id);
    let (title, body, footer) = match snapshot {
        Some(snapshot) => {
            let status = match snapshot.state {
                crate::tools::terminal::ExecutionSessionState::Running => "running",
                crate::tools::terminal::ExecutionSessionState::Exited => "exited",
                crate::tools::terminal::ExecutionSessionState::Failed => "failed",
            };
            let output = if snapshot.output.is_empty() {
                "(no output yet)".to_string()
            } else {
                snapshot.output
            };
            (
                format!(" Process · {} · {status} ", snapshot.name),
                format!(
                    "$ {}\n# cwd: {}\n# pid: {} · elapsed: {}s\n# transcript: {}{}\n\n{}",
                    snapshot.command,
                    snapshot.cwd.display(),
                    snapshot.pid,
                    snapshot.elapsed_secs,
                    snapshot.transcript_path.display(),
                    if snapshot.transcript_truncated {
                        " · truncated"
                    } else {
                        ""
                    },
                    output,
                ),
                if state.confirm_stop {
                    "Press x again to stop this process · Esc cancel"
                } else if snapshot.capabilities.stop {
                    "←/→ switch · ↑/↓ scroll · f follow · x stop · Esc close · read-only"
                } else {
                    "←/→ switch · ↑/↓ scroll · Esc close · completed"
                },
            )
        }
        None => (
            " Process · unavailable ".to_string(),
            format!("Session '{}' is no longer retained.", state.session_id),
            "Esc close",
        ),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(theme.style_border())
        .title(title)
        .title_bottom(Line::from(footer).style(theme.style_dim()));
    let inner_height = popup.height.saturating_sub(2);
    let body_lines = body.lines().count() as u16;
    let max_scroll = body_lines.saturating_sub(inner_height);
    let scroll = if state.follow {
        max_scroll
    } else {
        state.scroll.min(max_scroll)
    };
    frame.render_widget(
        Paragraph::new(body)
            .block(block)
            .style(theme.style_muted())
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_stop_requires_confirmation_and_escape_cancels() {
        let mut state = ProcessViewerState::new("session");
        assert!(!state.request_stop());
        assert!(state.confirm_stop);
        assert!(state.cancel_confirmation());
        assert!(!state.confirm_stop);
        assert!(!state.request_stop());
        assert!(state.request_stop());
        assert!(!state.confirm_stop);
    }

    #[test]
    fn viewer_state_disables_follow_when_scrolling_up() {
        let mut state = ProcessViewerState::new("session");
        state.scroll = 3;
        state.scroll_up();
        assert!(!state.follow);
        assert_eq!(state.scroll, 2);
        state.toggle_follow();
        assert!(state.follow);
        assert_eq!(state.scroll, 0);
    }
}

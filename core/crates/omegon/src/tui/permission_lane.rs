//! Permission prompt lane rendering and key mapping.

use crossterm::event::{KeyCode, KeyModifiers};
use omegon_traits::{PermissionPersistence, PermissionRequestKind};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::theme;

pub fn permission_persist_scope_label(
    tool_name: &str,
    kind: PermissionRequestKind,
    persistence: PermissionPersistence,
) -> &'static str {
    match persistence {
        PermissionPersistence::ProjectDirectory => "always for this directory",
        PermissionPersistence::SessionDirectory => "always for this directory this session",
        PermissionPersistence::None => match kind {
            PermissionRequestKind::Policy => "allow this operation",
            PermissionRequestKind::PathBoundary => match tool_name {
                "bash" | "terminal" => "always for this command",
                "read" | "view" => "always for this file",
                "edit" | "write" | "change" => "always for this path",
                _ => "always for this operation",
            },
        },
    }
}

pub fn permission_response_for_key(
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<omegon_traits::PermissionResponse> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(omegon_traits::PermissionResponse::Allow),
        KeyCode::Char('a') if !modifiers.contains(KeyModifiers::SHIFT) => {
            Some(omegon_traits::PermissionResponse::AllowSession)
        }
        KeyCode::Char('A') => Some(omegon_traits::PermissionResponse::AlwaysAllow),
        KeyCode::Char('a') if modifiers.contains(KeyModifiers::SHIFT) => {
            Some(omegon_traits::PermissionResponse::AlwaysAllow)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            Some(omegon_traits::PermissionResponse::Deny)
        }
        _ => None,
    }
}

pub fn render_permission_lane(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    tool_name: &str,
    target: &str,
    kind: PermissionRequestKind,
    persistence: PermissionPersistence,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bg = t.surface_bg();
    let text_budget = area.width.saturating_sub(2) as usize;
    let scope = permission_persist_scope_label(tool_name, kind, persistence);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "permission required · ",
            Style::default()
                .fg(t.warning())
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(tool_name, Style::default().fg(t.accent()).bg(bg)),
        Span::styled(" · ", Style::default().fg(t.dim()).bg(bg)),
        Span::styled(
            crate::util::truncate(target, text_budget.saturating_sub(tool_name.len() + 24)),
            Style::default().fg(t.fg()).bg(bg),
        ),
    ])];
    if area.height > 1 {
        lines.push(Line::from(vec![
            Span::styled(
                "y",
                Style::default()
                    .fg(t.warning())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" once · ", Style::default().fg(t.dim()).bg(bg)),
            Span::styled(
                "n",
                Style::default()
                    .fg(t.warning())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" deny · ", Style::default().fg(t.dim()).bg(bg)),
            Span::styled(
                "Shift+A",
                Style::default()
                    .fg(t.warning())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {scope}"), Style::default().fg(t.dim()).bg(bg)),
        ]));
    }
    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

pub fn format_permission_prompt(
    tool_name: &str,
    path: &str,
    _kind: PermissionRequestKind,
    _persistence: PermissionPersistence,
    grant_path: Option<&str>,
) -> String {
    let grant = grant_path
        .map(|path| format!("Grant: {path}\n"))
        .unwrap_or_default();
    format!(
        "Tool: {tool_name}\n\
         Target: {path}\n\
         {grant}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_response_maps_expected_keys() {
        assert_eq!(
            permission_response_for_key(KeyCode::Char('y'), KeyModifiers::NONE),
            Some(omegon_traits::PermissionResponse::Allow)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('n'), KeyModifiers::NONE),
            Some(omegon_traits::PermissionResponse::Deny)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Esc, KeyModifiers::NONE),
            Some(omegon_traits::PermissionResponse::Deny)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('a'), KeyModifiers::NONE),
            Some(omegon_traits::PermissionResponse::AllowSession)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('A'), KeyModifiers::NONE),
            Some(omegon_traits::PermissionResponse::AlwaysAllow)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('a'), KeyModifiers::SHIFT),
            Some(omegon_traits::PermissionResponse::AlwaysAllow)
        );
    }

    #[test]
    fn permission_scope_labels_are_tool_specific() {
        assert_eq!(
            permission_persist_scope_label(
                "bash",
                PermissionRequestKind::PathBoundary,
                PermissionPersistence::None
            ),
            "always for this command"
        );
        assert_eq!(
            permission_persist_scope_label(
                "read",
                PermissionRequestKind::PathBoundary,
                PermissionPersistence::None
            ),
            "always for this file"
        );
        assert_eq!(
            permission_persist_scope_label(
                "edit",
                PermissionRequestKind::PathBoundary,
                PermissionPersistence::None
            ),
            "always for this path"
        );
        assert_eq!(
            permission_persist_scope_label(
                "web_search",
                PermissionRequestKind::Policy,
                PermissionPersistence::None
            ),
            "allow this operation"
        );
    }
}

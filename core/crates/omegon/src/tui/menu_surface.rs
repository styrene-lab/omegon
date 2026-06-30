//! Generic TUI state and rendering helpers for renderer-neutral menu projections.
//!
//! The menu surface is for slash-command inventories and action lists: richer
//! than a prose command panel, broader than a one-dimensional selector, and
//! still small enough to remain keyboard-first.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::surfaces::menu::{
    MenuBadgeTone, MenuProjection, MenuRowKind, MenuRowProjection,
};
use crate::tui::{command_surfaces, theme::Theme};

#[cfg(test)]
use crate::tui::theme::Alpharius;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuState {
    pub active_tab: String,
    pub selected_row: usize,
    pub filter: String,
    pub mode: MenuMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveMenu {
    pub projection: MenuProjection,
    pub state: MenuState,
}

impl ActiveMenu {
    pub(crate) fn new(projection: MenuProjection) -> Self {
        let state = MenuState::new(&projection);
        Self { projection, state }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuMode {
    Browse,
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisibleMenuRow<'a> {
    pub tab_id: &'a str,
    pub group_id: &'a str,
    pub group_label: &'a str,
    pub row: &'a MenuRowProjection,
}

pub(crate) fn menu_visible_window(
    cursor: usize,
    len: usize,
    capacity: usize,
) -> std::ops::Range<usize> {
    if len == 0 || capacity == 0 {
        return 0..0;
    }
    let capacity = capacity.min(len);
    let cursor = cursor.min(len - 1);
    let start = cursor.saturating_add(1).saturating_sub(capacity);
    let end = (start + capacity).min(len);
    start..end
}

pub(crate) fn render_menu_surface(
    frame: &mut Frame,
    area: Rect,
    theme: &dyn Theme,
    projection: &MenuProjection,
    state: &MenuState,
) {
    let popup = command_surfaces::command_modal_area(area);
    frame.render_widget(Clear, popup);
    frame.render_widget(menu_paragraph(theme, projection, state, popup.height), popup);
}

fn menu_paragraph<'a>(
    theme: &dyn Theme,
    projection: &'a MenuProjection,
    state: &MenuState,
    height: u16,
) -> Paragraph<'a> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.style_border())
        .title(format!(" {} ", projection.title));

    let lines = menu_lines(theme, projection, state, height.saturating_sub(2));
    Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true })
}

fn menu_lines<'a>(
    theme: &dyn Theme,
    projection: &'a MenuProjection,
    state: &MenuState,
    inner_height: u16,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        projection.title.clone(),
        Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD),
    )));
    if let Some(summary) = projection.summary.as_deref().filter(|summary| !summary.is_empty()) {
        for line in summary.lines() {
            lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(theme.muted()))));
        }
    }

    if projection.tabs.len() > 1 {
        lines.push(Line::from(
            projection
                .tabs
                .iter()
                .flat_map(|tab| {
                    let style = if tab.id == state.active_tab {
                        Style::default().fg(theme.accent_bright()).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.muted())
                    };
                    [
                        Span::styled(
                            if tab.id == state.active_tab {
                                format!("[{}]", tab.label)
                            } else {
                                format!(" {} ", tab.label)
                            },
                            style,
                        ),
                        Span::raw("  "),
                    ]
                })
                .collect::<Vec<_>>(),
        ));
    }

    let filter_label = match state.mode {
        MenuMode::Search => format!("filter: {}▌", state.filter),
        MenuMode::Browse if state.filter.is_empty() => "filter: / to search".into(),
        MenuMode::Browse => format!("filter: {}", state.filter),
    };
    lines.push(Line::from(Span::styled(
        filter_label,
        Style::default().fg(if matches!(state.mode, MenuMode::Search) {
            theme.accent_bright()
        } else {
            theme.dim()
        }),
    )));
    lines.push(Line::from(""));

    let rows = state.visible_rows(projection);
    let reserved = 6usize;
    let capacity = usize::from(inner_height).saturating_sub(reserved).max(1);
    let window = menu_visible_window(state.selected_row, rows.len(), capacity);
    let mut previous_group: Option<&str> = None;

    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching rows",
            Style::default().fg(theme.muted()),
        )));
    } else {
        if window.start > 0 {
            lines.push(Line::from(Span::styled(
                format!("  ↑ {} more", window.start),
                Style::default().fg(theme.dim()),
            )));
        }
        for idx in window.clone() {
            let visible = &rows[idx];
            if previous_group != Some(visible.group_id) {
                previous_group = Some(visible.group_id);
                lines.push(Line::from(Span::styled(
                    visible.group_label.to_string(),
                    Style::default().fg(theme.muted()).add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(menu_row_line(theme, visible.row, idx == state.selected_row));
            if !visible.row.description.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    {}", visible.row.description),
                    Style::default().fg(theme.muted()),
                )));
            }
        }
        if window.end < rows.len() {
            lines.push(Line::from(Span::styled(
                format!("  ↓ {} more", rows.len() - window.end),
                Style::default().fg(theme.dim()),
            )));
        }
    }

    lines.push(Line::from(""));
    let footer = projection.footer.as_deref().unwrap_or(match state.mode {
        MenuMode::Search => "type to filter · Backspace edit · Esc browse · Enter run",
        MenuMode::Browse => "↑/↓ navigate · Tab category · / search · Enter run · Esc close",
    });
    lines.push(Line::from(Span::styled(footer.to_string(), Style::default().fg(theme.dim()))));
    lines
}

fn menu_row_line<'a>(theme: &dyn Theme, row: &'a MenuRowProjection, selected: bool) -> Line<'a> {
    let marker = if selected { "›" } else { " " };
    let label_style = match row.kind {
        MenuRowKind::Action => Style::default().fg(theme.fg()).add_modifier(Modifier::BOLD),
        MenuRowKind::Object => Style::default().fg(theme.fg()),
        MenuRowKind::Heading => Style::default().fg(theme.accent_bright()).add_modifier(Modifier::BOLD),
    };
    let mut spans = vec![
        Span::styled(format!("{marker} "), Style::default().fg(theme.accent())),
        Span::styled(row.label.clone(), label_style),
    ];
    if let Some(value) = row.value.as_deref().filter(|value| !value.is_empty()) {
        spans.push(Span::styled(format!("  {value}"), Style::default().fg(theme.accent_bright())));
    }
    for badge in &row.badges {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("[{}]", badge.label),
            Style::default().fg(match badge.tone {
                MenuBadgeTone::Neutral => theme.muted(),
                MenuBadgeTone::Success => theme.success(),
                MenuBadgeTone::Warning => theme.warning(),
                MenuBadgeTone::Danger => theme.error(),
                MenuBadgeTone::Info => theme.accent_bright(),
            }),
        ));
    }
    if row.primary_action.is_some() {
        spans.push(Span::styled("  ↵", Style::default().fg(theme.dim())));
    }
    Line::from(spans)
}

impl MenuState {
    pub(crate) fn new(projection: &MenuProjection) -> Self {
        Self {
            active_tab: projection
                .tabs
                .first()
                .map(|tab| tab.id.clone())
                .unwrap_or_else(|| "main".into()),
            selected_row: 0,
            filter: String::new(),
            mode: MenuMode::Browse,
        }
    }

    pub(crate) fn visible_rows<'a>(
        &self,
        projection: &'a MenuProjection,
    ) -> Vec<VisibleMenuRow<'a>> {
        let filter = self.filter.trim().to_lowercase();
        let Some(tab) = projection.tabs.iter().find(|tab| tab.id == self.active_tab) else {
            return Vec::new();
        };

        let mut rows = Vec::new();
        for group in &tab.groups {
            for row in &group.rows {
                if filter.is_empty() || row_matches_filter(row, &filter) {
                    rows.push(VisibleMenuRow {
                        tab_id: &tab.id,
                        group_id: &group.id,
                        group_label: &group.label,
                        row,
                    });
                }
            }
        }
        rows
    }

    pub(crate) fn selected_row<'a>(
        &self,
        projection: &'a MenuProjection,
    ) -> Option<VisibleMenuRow<'a>> {
        self.visible_rows(projection).get(self.selected_row).cloned()
    }

    pub(crate) fn selected_primary_action(
        &self,
        projection: &MenuProjection,
    ) -> Option<crate::surfaces::menu::MenuActionProjection> {
        self.selected_row(projection)
            .and_then(|row| row.row.primary_action.clone())
    }

    pub(crate) fn selected_command(&self, projection: &MenuProjection) -> Option<String> {
        self.selected_primary_action(projection)
            .and_then(|action| action.command)
    }

    pub(crate) fn selected_action_for_key(
        &self,
        projection: &MenuProjection,
        key: char,
    ) -> Option<crate::surfaces::menu::MenuActionProjection> {
        self.action_for_key(projection, key).cloned()
    }

    pub(crate) fn selected_action_command_for_key(
        &self,
        projection: &MenuProjection,
        key: char,
    ) -> Option<String> {
        self.selected_action_for_key(projection, key).and_then(|action| action.command)
    }

    pub(crate) fn row_target_for_action_key(
        &self,
        projection: &MenuProjection,
        key: char,
    ) -> Option<String> {
        self.selected_action_for_key(projection, key)
            .and_then(|action| action.target_row_id)
    }

    fn action_for_key<'a>(
        &self,
        projection: &'a MenuProjection,
        key: char,
    ) -> Option<&'a crate::surfaces::menu::MenuActionProjection> {
        let key = key.to_ascii_lowercase().to_string();
        self.selected_row(projection)
            .and_then(|row| {
                row.row.actions.iter().find(|action| {
                    action
                        .key
                        .as_deref()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&key))
                })
            })
            .or_else(|| {
                projection.actions.iter().find(|action| {
                    action
                        .key
                        .as_deref()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&key))
                })
            })
    }

    pub(crate) fn select_row_by_id(&mut self, projection: &MenuProjection, row_id: &str) -> bool {
        let rows = self.visible_rows(projection);
        if let Some(index) = rows.iter().position(|row| row.row.id == row_id) {
            self.selected_row = index;
            true
        } else {
            false
        }
    }

    pub(crate) fn move_up(&mut self) {
        self.selected_row = self.selected_row.saturating_sub(1);
    }

    pub(crate) fn move_down(&mut self, projection: &MenuProjection) {
        let len = self.visible_rows(projection).len();
        if len > 0 {
            self.selected_row = (self.selected_row + 1).min(len - 1);
        }
    }

    pub(crate) fn next_tab(&mut self, projection: &MenuProjection) {
        self.switch_tab(projection, 1);
    }

    pub(crate) fn previous_tab(&mut self, projection: &MenuProjection) {
        self.switch_tab(projection, -1);
    }

    pub(crate) fn enter_search(&mut self) {
        self.mode = MenuMode::Search;
        self.selected_row = 0;
    }

    pub(crate) fn push_filter_char(&mut self, projection: &MenuProjection, ch: char) {
        self.filter.push(ch);
        self.clamp_selection(projection);
    }

    pub(crate) fn pop_filter_char(&mut self, projection: &MenuProjection) {
        self.filter.pop();
        self.clamp_selection(projection);
    }

    pub(crate) fn exit_search(&mut self) -> bool {
        match self.mode {
            MenuMode::Search => {
                self.mode = MenuMode::Browse;
                true
            }
            MenuMode::Browse if !self.filter.is_empty() => {
                self.filter.clear();
                self.selected_row = 0;
                true
            }
            MenuMode::Browse => false,
        }
    }

    fn switch_tab(&mut self, projection: &MenuProjection, direction: isize) {
        if projection.tabs.is_empty() {
            self.active_tab = "main".into();
            self.selected_row = 0;
            return;
        }
        let current = projection
            .tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .unwrap_or(0);
        let len = projection.tabs.len() as isize;
        let next = (current as isize + direction).rem_euclid(len) as usize;
        self.active_tab = projection.tabs[next].id.clone();
        self.selected_row = 0;
        self.filter.clear();
    }

    fn clamp_selection(&mut self, projection: &MenuProjection) {
        let len = self.visible_rows(projection).len();
        if len == 0 {
            self.selected_row = 0;
        } else {
            self.selected_row = self.selected_row.min(len - 1);
        }
    }
}

fn row_matches_filter(row: &MenuRowProjection, filter: &str) -> bool {
    row.id.to_lowercase().contains(filter)
        || row.label.to_lowercase().contains(filter)
        || row.description.to_lowercase().contains(filter)
        || row.value.as_deref().unwrap_or_default().to_lowercase().contains(filter)
        || row.metadata.iter().any(|item| item.to_lowercase().contains(filter))
        || row.badges.iter().any(|badge| badge.label.to_lowercase().contains(filter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::menu::{
        MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
        MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
    };

    fn row(id: &str, label: &str, command: &str) -> MenuRowProjection {
        MenuRowProjection {
            id: id.into(),
            label: label.into(),
            description: format!("description for {label}"),
            value: None,
            kind: MenuRowKind::Action,
            badges: Vec::new(),
            metadata: vec![format!("meta-{id}")],
            primary_action: Some(MenuActionProjection::command(id, label, command)),
            actions: vec![{
                let mut action = MenuActionProjection::command(
                    format!("install-{id}"),
                    "Install",
                    format!("/skills install {label}"),
                );
                action.key = Some("i".into());
                action
            }],
            safety: None,
            availability: None,
        }
    }

    fn projection() -> MenuProjection {
        MenuProjection {
            id: "test".into(),
            title: "Test".into(),
            summary: None,
            tabs: vec![
                MenuTabProjection {
                    id: "first".into(),
                    label: "First".into(),
                    groups: vec![MenuGroupProjection {
                        id: "alpha".into(),
                        label: "Alpha".into(),
                        description: None,
                        rows: vec![
                            row("rust", "Rust", "/skills get rust"),
                            row("python", "Python", "/skills get python"),
                        ],
                    }],
                },
                MenuTabProjection {
                    id: "second".into(),
                    label: "Second".into(),
                    groups: vec![MenuGroupProjection {
                        id: "beta".into(),
                        label: "Beta".into(),
                        description: None,
                        rows: vec![row("loop", "Loop", "/loop status")],
                    }],
                },
            ],
            actions: Vec::new(),
            footer: None,
        }
    }

    #[test]
    fn defaults_to_first_tab_and_flattens_group_rows() {
        let projection = projection();
        let state = MenuState::new(&projection);
        let rows = state.visible_rows(&projection);

        assert_eq!(state.active_tab, "first");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].group_label, "Alpha");
        assert_eq!(rows[0].row.label, "Rust");
    }

    #[test]
    fn filtering_matches_metadata_and_clamps_selection() {
        let projection = projection();
        let mut state = MenuState::new(&projection);
        state.selected_row = 1;

        state.push_filter_char(&projection, 'r');
        state.push_filter_char(&projection, 'u');
        state.push_filter_char(&projection, 's');
        state.push_filter_char(&projection, 't');

        let rows = state.visible_rows(&projection);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row.id, "rust");
        assert_eq!(state.selected_row, 0);
    }

    #[test]
    fn tab_switch_resets_selection_and_filter() {
        let projection = projection();
        let mut state = MenuState::new(&projection);
        state.selected_row = 1;
        state.filter = "rust".into();

        state.next_tab(&projection);

        assert_eq!(state.active_tab, "second");
        assert_eq!(state.selected_row, 0);
        assert!(state.filter.is_empty());
        assert_eq!(state.visible_rows(&projection)[0].row.id, "loop");
    }

    #[test]
    fn selected_command_returns_primary_action_command() {
        let projection = projection();
        let mut state = MenuState::new(&projection);
        state.move_down(&projection);

        assert_eq!(state.selected_command(&projection).as_deref(), Some("/skills get python"));
    }

    #[test]
    fn selected_action_for_key_returns_full_action() {
        let projection = projection();
        let mut state = MenuState::new(&projection);
        state.move_down(&projection);

        let action = state
            .selected_action_for_key(&projection, 'I')
            .expect("keyed action");

        assert_eq!(action.label, "Install");
        assert_eq!(action.command.as_deref(), Some("/skills install Python"));
        assert_eq!(action.disposition, crate::surfaces::menu::MenuActionDisposition::RunCommand);
    }

    #[test]
    fn selected_action_command_returns_matching_keyed_action() {
        let projection = projection();
        let mut state = MenuState::new(&projection);
        state.move_down(&projection);

        assert_eq!(
            state.selected_action_command_for_key(&projection, 'I').as_deref(),
            Some("/skills install Python")
        );
        assert_eq!(state.selected_action_command_for_key(&projection, 'x'), None);
    }

    #[test]
    fn visible_window_keeps_cursor_in_view() {
        assert_eq!(menu_visible_window(0, 10, 3), 0..3);
        assert_eq!(menu_visible_window(4, 10, 3), 2..5);
        assert_eq!(menu_visible_window(9, 10, 3), 7..10);
        assert_eq!(menu_visible_window(0, 0, 3), 0..0);
    }

    #[test]
    fn rendered_lines_include_ux_chrome_badges_and_more_indicators() {
        let mut projection = projection();
        projection.summary = Some("summary".into());
        projection.tabs[0].groups[0].rows[0].badges.push(MenuBadgeProjection {
            label: "project".into(),
            tone: MenuBadgeTone::Info,
        });
        let mut state = MenuState::new(&projection);
        state.selected_row = 1;

        let theme = Alpharius;
        let text = menu_lines(&theme, &projection, &state, 8)
            .into_iter()
            .map(|line| line.spans.into_iter().map(|span| span.content.into_owned()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Test"), "{text}");
        assert!(text.contains("summary"), "{text}");
        assert!(text.contains("[First]"), "{text}");
        assert!(text.contains("[project]"), "{text}");
        assert!(text.contains("↓"), "{text}");
        assert!(text.contains("Enter run"), "{text}");
    }
}

//! Generic TUI state and rendering helpers for renderer-neutral menu projections.
//!
//! The menu surface is for slash-command inventories and action lists: richer
//! than a prose command panel, broader than a one-dimensional selector, and
//! still small enough to remain keyboard-first.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::surfaces::menu::{MenuBadgeTone, MenuProjection, MenuRowKind, MenuRowProjection};
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
    frame.render_widget(
        menu_paragraph(theme, projection, state, popup.width, popup.height),
        popup,
    );
}

fn menu_paragraph<'a>(
    theme: &dyn Theme,
    projection: &'a MenuProjection,
    state: &MenuState,
    width: u16,
    height: u16,
) -> Paragraph<'a> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.style_border())
        .title(format!(" {} ", projection.title));

    let inner_width = width.saturating_sub(2);
    let lines = menu_lines(
        theme,
        projection,
        state,
        inner_width,
        height.saturating_sub(2),
    );
    Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left)
}

fn menu_lines<'a>(
    theme: &dyn Theme,
    projection: &'a MenuProjection,
    state: &MenuState,
    inner_width: u16,
    inner_height: u16,
) -> Vec<Line<'a>> {
    let width = usize::from(inner_width);
    let mut lines = Vec::new();
    lines.push(clipped_line(
        projection.title.clone(),
        width,
        Style::default()
            .fg(theme.accent())
            .add_modifier(Modifier::BOLD),
    ));
    if let Some(summary) = projection
        .summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
    {
        for line in summary.lines() {
            lines.extend(wrap_display(line, width).into_iter().map(|segment| {
                Line::from(Span::styled(segment, Style::default().fg(theme.muted())))
            }));
        }
    }

    if projection.tabs.len() > 1 {
        lines.push(Line::from(
            projection
                .tabs
                .iter()
                .flat_map(|tab| {
                    let style = if tab.id == state.active_tab {
                        Style::default()
                            .fg(theme.accent_bright())
                            .add_modifier(Modifier::BOLD)
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
    lines.push(clipped_line(
        filter_label,
        width,
        Style::default().fg(if matches!(state.mode, MenuMode::Search) {
            theme.accent_bright()
        } else {
            theme.dim()
        }),
    ));
    lines.push(Line::from(""));

    let rows = state.visible_rows(projection);
    let footer = projection.footer.as_deref().unwrap_or(match state.mode {
        MenuMode::Search => "type to filter · Backspace edit · Esc browse · Enter run",
        MenuMode::Browse => "↑/↓ navigate · Tab category · / search · Enter run · Esc close",
    });
    let footer_segments = wrap_display(footer, width);
    let body_budget = usize::from(inner_height)
        .saturating_sub(lines.len())
        .saturating_sub(1 + footer_segments.len())
        .max(1);
    let mut previous_group: Option<&str> = None;

    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching rows",
            Style::default().fg(theme.muted()),
        )));
    } else {
        let selected = state.selected_row.min(rows.len().saturating_sub(1));
        let columns = MenuRowColumns::for_rows(&rows);
        let selected_height = menu_row_render_height(&rows, selected, true, width);
        let mut start = selected;
        let mut used = selected_height;
        while start > 0 {
            let needed = menu_row_render_height(&rows, start - 1, start == selected, width);
            if used + needed > body_budget.saturating_sub(1).max(1) {
                break;
            }
            used += needed;
            start -= 1;
        }

        let has_above = start > 0;
        let mut used = usize::from(has_above);
        if has_above {
            lines.push(Line::from(Span::styled(
                format!("  ↑ {} more", start),
                Style::default().fg(theme.dim()),
            )));
        }

        let mut end = start;
        while end < rows.len() {
            let needed = menu_row_render_height(&rows, end, end == selected, width);
            let needs_more_indicator = end + 1 < rows.len();
            let reserved_for_more = usize::from(needs_more_indicator);
            if used + needed + reserved_for_more > body_budget && end != selected {
                break;
            }
            let visible = &rows[end];
            if previous_group != Some(visible.group_id) {
                previous_group = Some(visible.group_id);
                lines.push(Line::from(Span::styled(
                    visible.group_label.to_string(),
                    Style::default()
                        .fg(theme.muted())
                        .add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(menu_row_line(
                theme,
                visible.row,
                end == selected,
                width,
                columns,
            ));
            if end == selected && !visible.row.description.is_empty() {
                lines.extend(menu_description_lines(
                    theme,
                    &visible.row.description,
                    width,
                ));
            }
            used += needed;
            end += 1;
        }
        if end < rows.len() {
            lines.push(Line::from(Span::styled(
                format!("  ↓ {} more", rows.len() - end),
                Style::default().fg(theme.dim()),
            )));
        }
    }

    lines.push(Line::from(""));
    for segment in footer_segments {
        lines.push(Line::from(Span::styled(
            segment,
            Style::default().fg(theme.dim()),
        )));
    }
    lines
}

fn clipped_line<'a>(text: impl Into<String>, width: usize, style: Style) -> Line<'a> {
    Line::from(Span::styled(truncate_menu_text(&text.into(), width), style))
}

fn menu_row_render_height(
    rows: &[VisibleMenuRow<'_>],
    idx: usize,
    selected: bool,
    width: usize,
) -> usize {
    let mut height = 1usize;
    if idx == 0 || rows[idx - 1].group_id != rows[idx].group_id {
        height += 1;
    }
    if selected && !rows[idx].row.description.is_empty() {
        height += menu_description_visual_lines(&rows[idx].row.description, width).max(1);
    }
    height
}

fn menu_description_visual_lines(description: &str, width: usize) -> usize {
    let indent = UnicodeWidthStr::width("    ");
    let budget = width.saturating_sub(indent).max(1);
    wrap_display(description, budget).len()
}

fn menu_description_lines<'a>(
    theme: &dyn Theme,
    description: &'a str,
    width: usize,
) -> Vec<Line<'a>> {
    let indent = "    ";
    let budget = width.saturating_sub(UnicodeWidthStr::width(indent)).max(1);
    wrap_display(description, budget)
        .into_iter()
        .map(|segment| {
            Line::from(Span::styled(
                format!("{indent}{segment}"),
                Style::default().fg(theme.muted()),
            ))
        })
        .collect()
}

fn wrap_display(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    if width == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in value.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current.is_empty() {
            if word_width <= width {
                current.push_str(word);
                current_width = word_width;
            } else {
                lines.extend(split_long_word(word, width));
                current_width = UnicodeWidthStr::width(current.as_str());
            }
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut current));
            if word_width <= width {
                current.push_str(word);
                current_width = word_width;
            } else {
                lines.extend(split_long_word(word, width));
                current_width = UnicodeWidthStr::width(current.as_str());
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn split_long_word(word: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in word.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if !current.is_empty() && current_width + ch_width > width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn truncate_menu_text(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= UnicodeWidthStr::width("…") {
        return "…".to_string();
    }
    let ellipsis_width = UnicodeWidthStr::width("…");
    let mut out = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width + ellipsis_width > max_width {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MenuRowColumns {
    label: usize,
    value: usize,
}

impl MenuRowColumns {
    fn for_rows(rows: &[VisibleMenuRow<'_>]) -> Self {
        Self {
            label: rows
                .iter()
                .map(|visible| UnicodeWidthStr::width(visible.row.label.as_str()))
                .max()
                .unwrap_or(0),
            value: rows
                .iter()
                .filter_map(|visible| visible.row.value.as_deref())
                .map(UnicodeWidthStr::width)
                .max()
                .unwrap_or(0),
        }
    }
}

fn padded_menu_field(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(UnicodeWidthStr::width(value));
    format!("{value}{}", " ".repeat(padding))
}

fn menu_row_line<'a>(
    theme: &dyn Theme,
    row: &'a MenuRowProjection,
    selected: bool,
    width: usize,
    columns: MenuRowColumns,
) -> Line<'a> {
    let marker = if selected { "›" } else { " " };
    let label_style = match row.kind {
        MenuRowKind::Action => Style::default().fg(theme.fg()).add_modifier(Modifier::BOLD),
        MenuRowKind::Object => Style::default().fg(theme.fg()),
        MenuRowKind::Heading => Style::default()
            .fg(theme.accent_bright())
            .add_modifier(Modifier::BOLD),
    };
    let mut spans = vec![
        Span::styled(format!("{marker} "), Style::default().fg(theme.accent())),
        Span::styled(padded_menu_field(&row.label, columns.label), label_style),
    ];
    if columns.value > 0 {
        let value = row.value.as_deref().unwrap_or_default();
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            padded_menu_field(value, columns.value),
            Style::default().fg(theme.accent_bright()),
        ));
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
    let mut line = Line::from(spans);
    if width > 0 && line.width() > width {
        let marker_width = UnicodeWidthStr::width(marker) + 1;
        let right_text = row
            .badges
            .iter()
            .map(|badge| format!("[{}]", badge.label))
            .chain(row.primary_action.is_some().then(|| "↵".to_string()))
            .collect::<Vec<_>>()
            .join("  ");
        let suffix_width = if right_text.is_empty() {
            0
        } else {
            UnicodeWidthStr::width(right_text.as_str()) + 2
        };
        let label_budget = width.saturating_sub(marker_width + suffix_width).max(1);
        let mut compact_spans = vec![
            Span::styled(format!("{marker} "), Style::default().fg(theme.accent())),
            Span::styled(truncate_menu_text(&row.label, label_budget), label_style),
        ];
        if !right_text.is_empty() && marker_width + suffix_width < width {
            compact_spans.push(Span::raw("  "));
            compact_spans.push(Span::styled(right_text, Style::default().fg(theme.muted())));
        }
        line = Line::from(compact_spans);
    }
    line
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
        self.visible_rows(projection)
            .get(self.selected_row)
            .cloned()
    }

    pub(crate) fn selected_primary_action(
        &self,
        projection: &MenuProjection,
    ) -> Option<crate::surfaces::menu::MenuActionProjection> {
        self.selected_row(projection)
            .and_then(|row| row.row.primary_action.clone())
    }

    pub(crate) fn selected_action(
        &self,
        projection: &MenuProjection,
    ) -> Option<crate::surfaces::menu::MenuActionProjection> {
        self.selected_primary_action(projection)
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
        self.selected_action_for_key(projection, key)
            .and_then(|action| action.command)
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
        || row
            .value
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains(filter)
        || row
            .metadata
            .iter()
            .any(|item| item.to_lowercase().contains(filter))
        || row
            .badges
            .iter()
            .any(|badge| badge.label.to_lowercase().contains(filter))
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

        assert_eq!(
            state.selected_command(&projection).as_deref(),
            Some("/skills get python")
        );
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
        assert_eq!(
            action.disposition,
            crate::surfaces::menu::MenuActionDisposition::RunCommand
        );
    }

    #[test]
    fn selected_action_command_returns_matching_keyed_action() {
        let projection = projection();
        let mut state = MenuState::new(&projection);
        state.move_down(&projection);

        assert_eq!(
            state
                .selected_action_command_for_key(&projection, 'I')
                .as_deref(),
            Some("/skills install Python")
        );
        assert_eq!(
            state.selected_action_command_for_key(&projection, 'x'),
            None
        );
    }

    #[test]
    fn visible_window_keeps_cursor_in_view() {
        assert_eq!(menu_visible_window(0, 10, 3), 0..3);
        assert_eq!(menu_visible_window(4, 10, 3), 2..5);
        assert_eq!(menu_visible_window(9, 10, 3), 7..10);
        assert_eq!(menu_visible_window(0, 0, 3), 0..0);
    }

    #[test]
    fn rendered_rows_align_values_and_badges_as_columns() {
        let mut projection = projection();
        projection.tabs[0].groups[0].rows[0].label = "SHORT".into();
        projection.tabs[0].groups[0].rows[0].value = Some("ready".into());
        projection.tabs[0].groups[0].rows[0].badges = vec![MenuBadgeProjection {
            label: "optional".into(),
            tone: MenuBadgeTone::Neutral,
        }];
        projection.tabs[0].groups[0].rows[1].label = "MUCH_LONGER_NAME".into();
        projection.tabs[0].groups[0].rows[1].value = Some("missing".into());
        projection.tabs[0].groups[0].rows[1].badges = vec![MenuBadgeProjection {
            label: "required".into(),
            tone: MenuBadgeTone::Danger,
        }];
        let state = MenuState::new(&projection);
        let rows = state.visible_rows(&projection);
        let columns = MenuRowColumns::for_rows(&rows);
        let theme = Alpharius;

        let rendered = rows
            .iter()
            .map(|visible| {
                menu_row_line(&theme, visible.row, false, 80, columns)
                    .spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered[0].find("ready"), rendered[1].find("missing"));
        assert_eq!(rendered[0].find("[optional]"), rendered[1].find("[required]"));
    }

    #[test]
    fn rendered_lines_include_ux_chrome_badges_and_more_indicators() {
        let mut projection = projection();
        projection.summary = Some("summary".into());
        projection.tabs[0].groups[0].rows[0]
            .badges
            .push(MenuBadgeProjection {
                label: "project".into(),
                tone: MenuBadgeTone::Info,
            });
        let state = MenuState::new(&projection);

        let theme = Alpharius;
        let text = menu_lines(&theme, &projection, &state, 80, 8)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Test"), "{text}");
        assert!(text.contains("summary"), "{text}");
        assert!(text.contains("[First]"), "{text}");
        assert!(text.contains("[project]"), "{text}");
        assert!(text.contains("↓"), "{text}");
        assert!(text.contains("Enter run"), "{text}");
    }

    #[test]
    fn rendered_lines_keep_selected_tail_row_visible_with_descriptions() {
        let mut projection = projection();
        projection.tabs[0].groups[0].rows = (0..10)
            .map(|idx| {
                let id = format!("row-{idx}");
                let label = format!("Row {idx}");
                let command = format!("/test {idx}");
                let mut row = row(&id, &label, &command);
                row.description = format!("description {idx}");
                row
            })
            .collect();
        let mut state = MenuState::new(&projection);
        state.selected_row = 9;

        let theme = Alpharius;
        let text = menu_lines(&theme, &projection, &state, 80, 16)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Row 9"), "{text}");
        assert!(text.contains("description 9"), "{text}");
        assert!(!text.contains("Row 0"), "{text}");
        assert!(text.contains("↑"), "{text}");
    }

    #[test]
    fn rendered_lines_scroll_vertical_row_overflow_with_more_indicators() {
        let mut projection = projection();
        projection.tabs[0].groups[0].rows = (0..30)
            .map(|idx| {
                let id = format!("row-{idx}");
                let label = format!("Row {idx}");
                let command = format!("/test {idx}");
                row(&id, &label, &command)
            })
            .collect();
        let mut state = MenuState::new(&projection);
        state.selected_row = 20;

        let theme = Alpharius;
        let text = menu_lines(&theme, &projection, &state, 80, 14)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("› Row 20"), "{text}");
        assert!(!text.contains("Row 0"), "{text}");
        assert!(!text.contains("Row 29"), "{text}");
        assert!(text.contains("↑"), "{text}");
        assert!(text.contains("↓"), "{text}");
    }

    #[test]
    fn rendered_lines_wrap_summary_and_footer_without_truncation() {
        let mut projection = projection();
        projection.summary = Some(
            "Persisted profile controls. profile: user · file: /Users/operator/.omegon/profiles/default.json"
                .into(),
        );
        projection.footer = Some(
            "↑/↓ navigate · / filter · Enter use/view · s save · explicit /profile apply to apply · Esc close"
                .into(),
        );
        let state = MenuState::new(&projection);

        let theme = Alpharius;
        let lines = menu_lines(&theme, &projection, &state, 40, 20);
        let text_lines = lines
            .into_iter()
            .map(|line| {
                let text = line
                    .spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>();
                assert!(
                    UnicodeWidthStr::width(text.as_str()) <= 40,
                    "line exceeds menu width: {text:?}"
                );
                text
            })
            .collect::<Vec<_>>();
        let text = text_lines.join("\n");

        assert!(
            text.contains("/Users/operator/.omegon/profiles/default"),
            "{text}"
        );
        assert!(text.contains(".json"), "{text}");
        assert!(text.contains("explicit /profile apply to"), "{text}");
        assert!(text.contains("apply · Esc close"), "{text}");
        assert!(!text.contains('…'), "{text}");
    }

    #[test]
    fn rendered_lines_wrap_long_selected_description_to_menu_width() {
        let mut projection = projection();
        projection.summary = Some(
            "Agent harness initialization defaults. Pending plan:\nHarness substrate is already present; repair/import actions are still available."
                .into(),
        );
        projection.tabs[0].groups[0].rows[0].description =
            "Detected Cargo.toml. Source: bundled; activation: project_detected; inspect before changing".into();
        let state = MenuState::new(&projection);

        let theme = Alpharius;
        let lines = menu_lines(&theme, &projection, &state, 32, 16);
        let text_lines = lines
            .into_iter()
            .map(|line| {
                let text = line
                    .spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>();
                assert!(
                    UnicodeWidthStr::width(text.as_str()) <= 32,
                    "line exceeds menu width: {text:?}"
                );
                text
            })
            .collect::<Vec<_>>();
        let text = text_lines.join("\n");

        assert!(text.contains("    Detected Cargo.toml. Source:"), "{text}");
        assert!(text.contains("    bundled; activation:"), "{text}");
        assert!(text.contains("    project_detected; inspect"), "{text}");
        assert!(text.contains("    before changing"), "{text}");
    }
}

//! Pure horizontal line grammar for TUI chrome.
//!
//! This module owns visual grammar only: title/metric layout, rule fill,
//! width handling, and semantic emphasis styles. Surface modules retain their
//! own state projection and pass small visual specs here.

use std::borrow::Cow;

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{glyphs, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEmphasis {
    Muted,
    Normal,
    Accent,
    Strong,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulePlacement {
    None,
    Leading,
    Trailing,
    Surround,
    Full,
}

#[derive(Debug, Clone)]
pub struct LineMetric<'a> {
    pub label: Cow<'a, str>,
    pub value: Cow<'a, str>,
    pub emphasis: LineEmphasis,
}

impl<'a> LineMetric<'a> {
    pub fn new(label: impl Into<Cow<'a, str>>, value: impl Into<Cow<'a, str>>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            emphasis: LineEmphasis::Muted,
        }
    }

    pub fn with_emphasis(mut self, emphasis: LineEmphasis) -> Self {
        self.emphasis = emphasis;
        self
    }
}

#[derive(Debug, Clone)]
pub struct HorizontalLineSpec<'a> {
    pub title: Option<Cow<'a, str>>,
    pub title_emphasis: LineEmphasis,
    pub metrics: Vec<LineMetric<'a>>,
    pub rule: RulePlacement,
}

impl<'a> HorizontalLineSpec<'a> {
    pub fn title(title: impl Into<Cow<'a, str>>) -> Self {
        Self {
            title: Some(title.into()),
            title_emphasis: LineEmphasis::Accent,
            metrics: Vec::new(),
            rule: RulePlacement::Trailing,
        }
    }

    pub fn rule(rule: RulePlacement) -> Self {
        Self {
            title: None,
            title_emphasis: LineEmphasis::Muted,
            metrics: Vec::new(),
            rule,
        }
    }

    pub fn with_title_emphasis(mut self, emphasis: LineEmphasis) -> Self {
        self.title_emphasis = emphasis;
        self
    }

    pub fn with_metric(mut self, metric: LineMetric<'a>) -> Self {
        self.metrics.push(metric);
        self
    }
}

pub fn horizontal_line<'a>(
    spec: HorizontalLineSpec<'a>,
    width: u16,
    t: &dyn theme::Theme,
    bg: Color,
) -> Line<'a> {
    if width == 0 {
        return Line::default();
    }

    let mut spans: Vec<Span<'a>> = Vec::new();
    let rule_glyph = glyphs::glyphs().rule(glyphs::RuleGlyphRole::Horizontal);
    let rule_style = Style::default().fg(t.border_dim()).bg(bg);

    if spec.rule == RulePlacement::Full {
        return Line::from(Span::styled(
            repeat_to_width(rule_glyph, width as usize),
            rule_style,
        ));
    }

    if matches!(spec.rule, RulePlacement::Leading | RulePlacement::Surround) {
        spans.push(Span::styled(format!("{rule_glyph} "), rule_style));
    }

    if let Some(title) = spec.title {
        spans.push(Span::styled(
            title.into_owned(),
            emphasis_style(spec.title_emphasis, t, bg),
        ));
    }

    for metric in spec.metrics {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", Style::default().fg(t.muted()).bg(bg)));
        }
        let text = if metric.label.is_empty() {
            metric.value.into_owned()
        } else {
            format!("{} {}", metric.label, metric.value)
        };
        spans.push(Span::styled(text, emphasis_style(metric.emphasis, t, bg)));
    }

    let content_width: usize = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if content_width > width as usize {
        return truncate_spans(spans, width as usize, t, bg);
    }

    if matches!(spec.rule, RulePlacement::Trailing | RulePlacement::Surround) {
        let remaining = width as usize - content_width;
        if remaining > 0 {
            let rule_text = if content_width == 0 {
                repeat_to_width(rule_glyph, remaining)
            } else {
                format!(
                    " {}",
                    repeat_to_width(rule_glyph, remaining.saturating_sub(1))
                )
            };
            spans.push(Span::styled(rule_text, rule_style));
        }
    }

    Line::from(spans)
}

fn emphasis_style(emphasis: LineEmphasis, t: &dyn theme::Theme, bg: Color) -> Style {
    let fg = match emphasis {
        LineEmphasis::Muted => t.muted(),
        LineEmphasis::Normal => t.fg(),
        LineEmphasis::Accent | LineEmphasis::Strong => t.accent_muted(),
        LineEmphasis::Success => t.success(),
        LineEmphasis::Warning => t.warning(),
        LineEmphasis::Error => t.error(),
    };
    let style = Style::default().fg(fg).bg(bg);
    if matches!(emphasis, LineEmphasis::Strong) {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn repeat_to_width(glyph: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    glyph.repeat(width / UnicodeWidthStr::width(glyph).max(1))
}

fn truncate_spans<'a>(
    spans: Vec<Span<'a>>,
    width: usize,
    t: &dyn theme::Theme,
    bg: Color,
) -> Line<'a> {
    let text = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("");
    Line::from(Span::styled(
        truncate_display_width(&text, width),
        Style::default().fg(t.accent_muted()).bg(bg),
    ))
}

fn truncate_display_width(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let ellipsis = "…";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis);
    if width <= ellipsis_width {
        return ellipsis.to_string();
    }
    let budget = width - ellipsis_width;
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > budget {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push_str(ellipsis);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Alpharius, Theme};

    #[test]
    fn full_rule_fills_requested_width() {
        let line = horizontal_line(
            HorizontalLineSpec::rule(RulePlacement::Full),
            5,
            &Alpharius,
            Alpharius.surface_bg(),
        );
        assert_eq!(line.spans[0].content.as_ref(), "─────");
    }

    #[test]
    fn title_metrics_and_trailing_rule_fit_width() {
        let line = horizontal_line(
            HorizontalLineSpec::title("active tool")
                .with_title_emphasis(LineEmphasis::Muted)
                .with_metric(LineMetric::new("name", "bash").with_emphasis(LineEmphasis::Strong))
                .with_metric(LineMetric::new("elapsed", "2s")),
            40,
            &Alpharius,
            Alpharius.surface_bg(),
        );
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.contains("active tool"), "{rendered}");
        assert!(rendered.contains("name bash"), "{rendered}");
        assert!(rendered.ends_with('─'), "{rendered}");
        assert!(UnicodeWidthStr::width(rendered.as_str()) <= 40);
    }

    #[test]
    fn narrow_line_truncates_without_overflow() {
        let line = horizontal_line(
            HorizontalLineSpec::title("very long title")
                .with_metric(LineMetric::new("value", "abcdef")),
            8,
            &Alpharius,
            Alpharius.surface_bg(),
        );
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(UnicodeWidthStr::width(rendered.as_str()) <= 8);
    }
}

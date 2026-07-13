//! System notification segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::SegmentSurfacePolicy;

use super::super::conversation_render_projection::{SegmentRenderContext, terminal_segment_paint};
use super::super::segments::{SegmentRenderMode, apply_rendered_links};

pub struct SystemRenderProps<'a> {
    pub text: &'a str,
    pub surface: SegmentSurfacePolicy,
    pub mode: SegmentRenderMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemRenderPlan {
    pub chrome: SystemChrome,
    pub first_line: SystemLineKind,
    pub body: SystemBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemChrome {
    pub bordered: bool,
    pub title: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemLineKind {
    Info,
    Brand,
    Success,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemBody {
    Notice,
}

pub fn plan(props: &SystemRenderProps<'_>) -> SystemRenderPlan {
    let first = props.text.lines().next().unwrap_or_default();
    let first_line = if first.starts_with('Ω') {
        SystemLineKind::Brand
    } else if first.starts_with('✓') {
        SystemLineKind::Success
    } else if first.starts_with('⚠') || first.starts_with('⟳') || first.starts_with('✗') {
        SystemLineKind::Warning
    } else {
        SystemLineKind::Info
    };
    let slim = matches!(props.mode, SegmentRenderMode::Slim);
    SystemRenderPlan {
        chrome: SystemChrome {
            bordered: !slim,
            title: !slim,
        },
        first_line,
        body: SystemBody::Notice,
    }
}

fn system_block<'a>(render_plan: SystemRenderPlan, bg: Color, border_color: Color) -> Block<'a> {
    if !render_plan.chrome.bordered {
        Block::default()
            .padding(Padding::horizontal(0))
            .style(Style::default().bg(bg))
    } else {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(bg))
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(bg));
        if render_plan.chrome.title {
            block.title_top(Line::from(Span::styled(
                " Ω ",
                Style::default()
                    .fg(border_color)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            )))
        } else {
            block
        }
    }
}

fn system_line_style(
    line: &str,
    index: usize,
    render_plan: SystemRenderPlan,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
) -> Style {
    if index == 0 {
        match render_plan.first_line {
            SystemLineKind::Brand => {
                return Style::default()
                    .fg(theme.accent())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD);
            }
            SystemLineKind::Success => return Style::default().fg(theme.muted()).bg(bg),
            SystemLineKind::Warning => return Style::default().fg(theme.warning()).bg(bg),
            SystemLineKind::Info => {}
        }
    }
    if line.starts_with("  ▸") || line.starts_with("  /") || line.starts_with("  Ctrl") {
        Style::default().fg(theme.muted()).bg(bg)
    } else {
        Style::default().fg(theme.accent_muted()).bg(bg)
    }
}

pub fn render(
    props: SystemRenderProps<'_>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &SegmentRenderContext<'_>,
) {
    let theme = ctx.theme;
    if area.width < 3 || area.height == 0 {
        return;
    }

    let render_plan = plan(&props);
    let paint = terminal_segment_paint(props.surface, ctx);
    let bg = paint.text_bg.unwrap_or(paint.clear_bg);
    let block_bg = paint.surface_bg.unwrap_or(paint.clear_bg);
    let border_color = theme.accent_muted();
    let block = system_block(render_plan, block_bg, border_color);
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = match render_plan.body {
        SystemBody::Notice => props
            .text
            .lines()
            .enumerate()
            .map(|(i, line)| {
                let style = system_line_style(line, i, render_plan, theme, bg);
                Line::from(Span::styled(line.to_string(), style))
            })
            .collect::<Vec<_>>(),
    };

    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
    apply_rendered_links(
        inner,
        &lines,
        buf,
        Style::default()
            .fg(theme.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        inner.height,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{Alpharius, Theme};

    #[test]
    fn system_props_preserve_render_inputs() {
        let props = SystemRenderProps {
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
            text: "notice",
            mode: SegmentRenderMode::Full,
        };
        assert_eq!(props.text, "notice");
        assert_eq!(props.mode, SegmentRenderMode::Full);
    }

    #[test]
    fn system_plan_slim_omits_chrome() {
        let props = SystemRenderProps {
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
            text: "notice",
            mode: SegmentRenderMode::Slim,
        };

        let plan = super::plan(&props);
        assert!(!plan.chrome.bordered);
        assert!(!plan.chrome.title);
        assert_eq!(plan.first_line, SystemLineKind::Info);
        assert_eq!(plan.body, SystemBody::Notice);
    }

    #[test]
    fn system_plan_full_includes_chrome() {
        let props = SystemRenderProps {
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
            text: "notice",
            mode: SegmentRenderMode::Full,
        };

        let plan = super::plan(&props);
        assert!(plan.chrome.bordered);
        assert!(plan.chrome.title);
        assert_eq!(plan.first_line, SystemLineKind::Info);
    }

    #[test]
    fn system_plan_classifies_brand_and_warning_first_lines() {
        let brand = super::plan(&SystemRenderProps {
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
            text: "Ω status",
            mode: SegmentRenderMode::Full,
        });
        assert_eq!(brand.first_line, SystemLineKind::Brand);

        for text in ["⚠ warning", "⟳ retry", "✗ failed"] {
            let warning = super::plan(&SystemRenderProps {
                surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                    surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                    copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                    selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
                },
                text,
                mode: SegmentRenderMode::Full,
            });
            assert_eq!(warning.first_line, SystemLineKind::Warning, "{text}");
        }

        let success = super::plan(&SystemRenderProps {
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
            text: "✓ complete",
            mode: SegmentRenderMode::Full,
        });
        assert_eq!(success.first_line, SystemLineKind::Success);
    }

    #[test]
    fn successful_system_outcome_renders_neutral_not_warning() {
        let area = Rect::new(0, 0, 48, 1);
        let mut buf = Buffer::empty(area);
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Slim);
        render(
            SystemRenderProps {
                surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                    surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                    copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                    selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
                },
                text: "✓ read · file contents · 1 operation",
                mode: SegmentRenderMode::Slim,
            },
            area,
            &mut buf,
            &ctx,
        );

        let visible = (area.left()..area.right())
            .filter_map(|x| buf.cell((x, area.y)))
            .filter(|cell| cell.symbol() != " ")
            .collect::<Vec<_>>();
        assert!(!visible.is_empty());
        assert!(visible.iter().all(|cell| cell.fg == Alpharius.muted()));
        assert!(visible.iter().all(|cell| cell.fg != Alpharius.warning()));
    }

    #[test]
    fn system_renderer_includes_notice_text() {
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Full);
        render(
            SystemRenderProps {
                surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                    surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                    copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                    selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
                },
                text: "notice",
                mode: SegmentRenderMode::Full,
            },
            area,
            &mut buf,
            &ctx,
        );
        let mut rendered = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(
            rendered.contains("notice"),
            "notice should render: {rendered:?}"
        );
    }
}

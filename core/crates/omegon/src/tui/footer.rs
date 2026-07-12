//! Footer bar — 4-card telemetry strip at bottom of TUI.
//!
//! Each card is a bordered Block with a title bar. Cards share `card_bg`
//! background for visual cohesion.

use chrono::{DateTime, Utc};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme::Theme;
use super::widgets::{self, GaugeConfig};
use crate::surfaces::footer::ProjectFooterSurface;

use crate::settings::ContextClass;
use crate::status::HarnessStatus;
use crate::usage::format_provider_telemetry_compact;

#[derive(Clone, Debug)]
pub struct OperatorEventLine {
    pub icon: &'static str,
    pub message: String,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionUsageSlice {
    pub model_id: String,
    pub provider: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

struct EngineFlexRowStyle {
    label_color: Color,
    value_color: Color,
    value_bold: bool,
}

impl EngineFlexRowStyle {
    fn new(label_color: Color, value_color: Color, value_bold: bool) -> Self {
        Self {
            label_color,
            value_color,
            value_bold,
        }
    }
}

/// Footer data — updated by the TUI on every event and rendered each frame.
#[derive(Default)]
pub struct FooterData {
    pub model_id: String,
    pub model_provider: String,
    pub context_percent: f32,
    pub context_window: usize,
    pub context_class: ContextClass,
    pub actual_context_class: ContextClass,
    pub total_facts: usize,
    pub injected_facts: usize,
    pub working_memory: usize,
    pub memory_tokens_est: usize,
    /// Estimated total context tokens (rough heuristic from turn + tool counts).
    pub estimated_tokens: usize,
    /// Cumulative input tokens for the entire session.
    pub session_input_tokens: u64,
    /// Cumulative output tokens for the entire session.
    pub session_output_tokens: u64,
    /// Input tokens for the most recent turn (for per-turn usage display).
    pub last_turn_input_tokens: u64,
    /// Output tokens for the most recent turn.
    pub last_turn_output_tokens: u64,
    /// Per-turn usage attributed to the model/provider active for that turn.
    pub session_usage_slices: Vec<SessionUsageSlice>,
    pub tool_calls: u32,
    pub turn: u32,
    pub compactions: u32,
    pub cwd: String,
    pub is_oauth: bool,
    /// Persistent provider-route warning surfaced by RouteChanged events
    /// (fallback active, disconnected, or login failure). Cleared when route
    /// reaches a clean Serving state.
    pub route_warning: Option<String>,
    /// HarnessStatus — persona, MCP, secrets, inference state.
    /// Updated via BusEvent::HarnessStatusChanged.
    pub harness: HarnessStatus,
    /// Compaction flash counter — set to 3 when compaction occurs, decrements each frame.
    /// When > 0, system card renders with accent border.
    pub compaction_flash_ticks: u8,
    /// Current thinking level name (for engine panel display).
    pub thinking_level: String,
    /// Current posture name (for engine panel display).
    pub posture: String,
    /// Short runtime brand shown in engine chrome ("OM" in slim, "Omegon" in full).
    pub runtime_brand: String,
    /// Current runtime principal (descriptive identity only).
    pub principal_id: String,
    /// Current authorization summary (descriptive only).
    pub authorization: String,
    /// Current model grade name (for engine panel display).
    pub model_tier: String,
    /// Whether a live LLM provider is connected. False when NullBridge is active.
    pub provider_connected: bool,
    /// Available update version (if any).
    pub update_available: Option<String>,
    /// Sandbox isolation enabled — delegates/cleave run in containers.
    pub sandbox: bool,
    /// Inline operator-facing transient events shown under engine version info.
    pub operator_events: Vec<OperatorEventLine>,
    /// Current provider quota/headroom telemetry, if exposed by the upstream.
    pub provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    /// Web-search provider readiness for the liveness gauge. Empty until the
    /// secret readiness snapshot is available. Zero configured providers is
    /// rendered as a degradation (DDG scrape floor only), never as neutral.
    pub web_search_providers: Vec<crate::capabilities::secrets::WebSearchProviderReadiness>,
}

impl FooterData {
    pub fn projection(&self) -> crate::surfaces::footer::FooterProjection {
        self.project_footer_surface()
    }

    /// Update the harness status snapshot from a BusEvent::HarnessStatusChanged.
    pub fn update_harness(&mut self, status: HarnessStatus) {
        self.total_facts = status.memory.total_facts;
        self.working_memory = status.memory.working_facts;
        self.harness = status;
    }

    /// Set compaction flash — triggers accent border on system card for 3 ticks.
    pub fn trigger_compaction_flash(&mut self) {
        self.compaction_flash_ticks = 3;
    }

    /// Decrement compaction flash counter each frame.
    pub fn tick_compaction_flash(&mut self) {
        if self.compaction_flash_ticks > 0 {
            self.compaction_flash_ticks = self.compaction_flash_ticks.saturating_sub(1);
        }
    }

    pub fn render(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let width = area.width as usize;

        // Clear first so stale glyphs from prior surfaces cannot survive under
        // footer chrome. A styled Block updates style but does not blank any
        // pre-existing symbols in the area.
        frame.render_widget(Clear, area);

        // Fill the entire footer zone with footer-specific background.
        // Footer is permanent chrome — darker than conversation card_bg.
        let bg_block = Block::default().style(t.style_footer_bg());
        frame.render_widget(bg_block, area);

        if width < 60 {
            self.render_narrow(area, frame, t);
            return;
        }

        // 4 cards filling the width
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 4),
                Constraint::Ratio(1, 4),
                Constraint::Ratio(1, 4),
                Constraint::Min(10),
            ])
            .split(area);

        self.render_context_card(cols[0], frame, t);
        self.render_model_card(cols[1], frame, t);
        self.render_memory_card(cols[2], frame, t);
        self.render_system_card(cols[3], frame, t);
    }

    /// Render the compact engine fallback panel used when instrument panels are hidden.
    /// In slim mode with instruments visible, engine telemetry lives in the status sidecar row.
    pub fn render_engine_fallback_panel(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let bg = t.footer_bg();
        frame.render_widget(Clear, area);
        let bg_block = Block::default().style(Style::default().bg(bg));
        frame.render_widget(bg_block, area);

        if area.height < 4 || area.width < 20 {
            // Ultra-narrow fallback
            let model_short = short_model(&self.model_id);
            let line = Line::from(vec![
                Span::styled(
                    format!(" Ω {model_short} "),
                    Style::default().fg(t.accent()).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}% ", self.context_percent as u32),
                    Style::default().fg(t.muted()),
                ),
                Span::styled(
                    format!("⌗{}", self.total_facts),
                    Style::default().fg(t.dim()),
                ),
            ]);
            frame.render_widget(Paragraph::new(line).style(Style::default().bg(bg)), area);
            return;
        }

        // Engine fallback only; memory is visualized in the inference panel when instruments are visible.
        self.render_engine_section(area, frame, t);
    }

    fn render_engine_section(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let bg = t.footer_bg();
        let inner = Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: area.height,
        };

        let mut lines: Vec<Line<'static>> = Vec::new();
        let label_width = 7usize;
        let value_width = inner
            .width
            .saturating_sub((label_width as u16).saturating_add(2))
            as usize;

        let push_row = |lines: &mut Vec<Line<'static>>,
                        label: &str,
                        value: String,
                        value_max_width: usize,
                        label_color: Color,
                        value_color: Color,
                        value_bold: bool| {
            let mut value_style = Style::default().fg(value_color);
            if value_bold {
                value_style = value_style.add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<width$} ", label, width = label_width),
                    Style::default().fg(label_color),
                ),
                Span::styled(truncate_for_width(&value, value_max_width), value_style),
            ]));
        };

        lines.push(Line::from(Span::styled(
            " engine",
            Style::default()
                .fg(t.accent_muted())
                .add_modifier(Modifier::BOLD),
        )));

        let provider_runtime = self
            .harness
            .providers
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(&self.model_provider));

        if !self.provider_connected {
            let provider_label = crate::auth::provider_by_id(&self.model_provider)
                .map(|p| p.display_name)
                .unwrap_or(self.model_provider.as_str());
            let status_text = if self.model_provider.trim().is_empty() {
                "⚠ provider login required".to_string()
            } else {
                format!("⚠ {provider_label} login required")
            };
            let action_text = if self.model_provider.trim().is_empty() {
                "/login <provider>".to_string()
            } else {
                format!("/login {}", self.model_provider)
            };
            push_row(
                &mut lines,
                "status",
                status_text,
                value_width,
                t.border_dim(),
                t.warning(),
                true,
            );
            push_row(
                &mut lines,
                "action",
                action_text,
                value_width,
                t.border_dim(),
                t.muted(),
                false,
            );
        } else if let Some(provider) = provider_runtime
            && matches!(
                provider.runtime_status,
                Some(crate::status::ProviderRuntimeStatus::Degraded)
            )
        {
            let failures = provider.recent_failure_count.unwrap_or(0);
            let kind = provider
                .last_failure_kind
                .as_deref()
                .unwrap_or("transient upstream failures");
            let status_suffix = provider
                .last_failure_at
                .as_deref()
                .and_then(format_failure_age)
                .unwrap_or_else(|| "last recently".to_string());
            push_row(
                &mut lines,
                "status",
                format!("≈ degraded · {failures}× {kind} · {status_suffix}"),
                value_width,
                t.border_dim(),
                t.warning(),
                false,
            );
        }

        if let Some(quota_line) =
            format_provider_telemetry_compact(self.provider_telemetry.as_ref())
        {
            push_row(
                &mut lines,
                "limit",
                quota_line,
                value_width,
                t.border_dim(),
                t.accent_muted(),
                false,
            );
        }

        // Web-search liveness gauge. Keyless (DDG scrape floor only) renders
        // as a persistent degradation — operators should not sit in that state.
        if !self.web_search_providers.is_empty() {
            let configured = self
                .web_search_providers
                .iter()
                .filter(|provider| provider.configured)
                .count();
            if configured == 0 {
                push_row(
                    &mut lines,
                    "web",
                    "▲ keyless · ddg floor only · /secrets".to_string(),
                    value_width,
                    t.border_dim(),
                    t.warning(),
                    true,
                );
            } else {
                let mut spans = vec![Span::styled(
                    format!(" {:<width$} ", "web", width = label_width),
                    Style::default().fg(t.border_dim()),
                )];
                for provider in &self.web_search_providers {
                    let (tick, color) = if provider.configured {
                        ("●", t.success())
                    } else {
                        ("○", t.dim())
                    };
                    spans.push(Span::styled(tick, Style::default().fg(color)));
                }
                spans.push(Span::styled(
                    format!(
                        " {configured}/{} providers",
                        self.web_search_providers.len()
                    ),
                    Style::default().fg(t.muted()),
                ));
                lines.push(Line::from(spans));
            }
        }

        if self.update_available.is_some() {
            lines.push(Line::from({
                let mut spans = vec![Span::styled(
                    format!(" {:<width$} ", "version", width = label_width),
                    Style::default().fg(t.border_dim()),
                )];
                spans.extend(version_spans(self.update_available.as_deref(), t));
                spans
            }));
        }

        for event in self.operator_events.iter().take(2) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<width$} ", event.icon, width = label_width),
                    Style::default().fg(event.color),
                ),
                Span::styled(
                    truncate_for_width(&event.message, value_width),
                    Style::default().fg(event.color),
                ),
            ]));
        }

        if lines.len() == 1 {
            lines.push(Line::from(Span::styled(
                " nominal",
                Style::default().fg(t.dim()),
            )));
        }

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(bg));
        frame.render_widget(widget, inner);
    }

    fn engine_flex_row(
        label: &str,
        value: String,
        row_width: usize,
        label_width: usize,
        value_max_width: usize,
        style: EngineFlexRowStyle,
    ) -> Line<'static> {
        let label_text = format!(" {:<width$} ", label, width = label_width);
        let label_display_width = UnicodeWidthStr::width(label_text.as_str());
        let value_budget = value_max_width.min(row_width.saturating_sub(label_display_width + 1));
        let value_text = truncate_for_width(&value, value_budget);
        let value_display_width = UnicodeWidthStr::width(value_text.as_str());
        let spacer_width = row_width.saturating_sub(label_display_width + value_display_width);

        let mut value_style = Style::default().fg(style.value_color);
        if style.value_bold {
            value_style = value_style.add_modifier(Modifier::BOLD);
        }

        Line::from(vec![
            Span::styled(label_text, Style::default().fg(style.label_color)),
            Span::raw(" ".repeat(spacer_width)),
            Span::styled(value_text, value_style),
        ])
    }

    fn render_memory_section(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let bg = t.footer_bg();
        let inner = Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: area.height,
        };

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Header with dim divider
        lines.push(Line::from(Span::styled(
            " memory",
            Style::default()
                .fg(t.accent_muted())
                .add_modifier(Modifier::BOLD),
        )));

        // Mind rows — always show all, even at zero
        let sep = Span::styled(" · ", Style::default().fg(t.border_dim()));

        // Project memory (always active)
        let mut proj: Vec<Span<'static>> = vec![
            Span::styled(" ⬡ ", Style::default().fg(t.accent())),
            Span::styled("project", Style::default().fg(t.fg())),
            Span::styled(
                format!("  ⌗ {}", self.total_facts),
                Style::default().fg(t.muted()),
            ),
        ];
        if self.injected_facts > 0 {
            proj.push(sep.clone());
            proj.push(Span::styled(
                format!("inj {}", self.injected_facts),
                Style::default().fg(t.accent_muted()),
            ));
        }
        lines.push(Line::from(proj));

        // Working memory
        let wm_color = if self.working_memory > 0 {
            t.accent()
        } else {
            t.dim()
        };
        let wm: Vec<Span<'static>> = vec![
            Span::styled(" ⬡ ", Style::default().fg(wm_color)),
            Span::styled(
                "working",
                Style::default().fg(if self.working_memory > 0 {
                    t.fg()
                } else {
                    t.dim()
                }),
            ),
            Span::styled(
                format!("  ⌗ {}", self.working_memory),
                Style::default().fg(t.muted()),
            ),
        ];
        lines.push(Line::from(wm));

        // Token estimate
        if self.memory_tokens_est > 0 {
            lines.push(Line::from(vec![Span::styled(
                format!(
                    " ~{} tokens injected",
                    widgets::format_tokens(self.memory_tokens_est)
                ),
                Style::default().fg(t.dim()),
            )]));
        } else {
            lines.push(Line::from(Span::styled(
                " ~0 tokens injected",
                Style::default().fg(t.dim()),
            )));
        }

        // Compactions
        if self.compactions > 0 {
            lines.push(Line::from(vec![Span::styled(
                format!(" ↻ {} compactions", self.compactions),
                Style::default().fg(t.dim()),
            )]));
        }

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(bg));
        frame.render_widget(widget, inner);
    }

    /// Card block: bordered, titled, card_bg background.
    fn card_block<'a>(title: &str, t: &dyn Theme) -> Block<'a> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_dim()).bg(t.footer_bg()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(
                format!(" {title} "),
                Style::default().fg(t.muted()).bg(t.footer_bg()),
            ))
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(t.footer_bg()))
    }

    fn render_narrow(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let model_short = short_model(&self.model_id);
        let pct = self.context_percent as u32;
        let line = Line::from(vec![
            Span::styled(" Ω ", t.style_accent_bold()),
            Span::styled(format!("{model_short} "), Style::default().fg(t.muted())),
            Span::styled("│ ", Style::default().fg(t.dim())),
            Span::styled(
                format!("{pct}% "),
                Style::default().fg(widgets::percent_color(self.context_percent, t)),
            ),
            Span::styled("│ ", Style::default().fg(t.dim())),
            Span::styled(format!("T·{} ", self.turn), Style::default().fg(t.muted())),
        ]);
        frame.render_widget(Clear, area);
        let widget = Paragraph::new(line).style(Style::default().bg(t.footer_bg()));
        frame.render_widget(widget, area);
    }

    fn render_context_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("context", t);
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Gauge bar
        let bar_w = (inner.width as usize).saturating_sub(12).min(20);
        let pct = self.context_percent.min(100.0);
        let memory_blocks = if self.memory_tokens_est > 0 && self.context_window > 0 {
            let mem_pct = self.memory_tokens_est as f32 / self.context_window as f32 * 100.0;
            ((mem_pct / 100.0) * bar_w as f32) as usize
        } else {
            0
        };

        let mut bar_spans: Vec<Span<'static>> = Vec::new();
        bar_spans.extend(widgets::gauge_bar(
            &GaugeConfig {
                percent: pct,
                bar_width: bar_w,
                memory_blocks,
            },
            t,
        ));

        let pct_str = format!(" {}%", pct as u32);
        bar_spans.push(Span::styled(
            pct_str,
            Style::default().fg(widgets::percent_color(pct, t)),
        ));

        if self.context_window > 0 {
            bar_spans.push(Span::styled(
                format!(" / {}", widgets::format_tokens(self.context_window)),
                Style::default().fg(t.dim()),
            ));
        }
        if self.turn > 0 {
            bar_spans.push(Span::styled(
                format!("  T·{}", self.turn),
                Style::default().fg(t.dim()),
            ));
        }
        lines.push(Line::from(bar_spans));

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(t.footer_bg()));
        frame.render_widget(widget, inner);
    }

    fn render_model_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("model", t);
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        let model_short = short_model(&self.model_id);
        let provider_label = crate::auth::provider_by_id(&self.model_provider)
            .map(|p| p.display_name)
            .unwrap_or(self.model_provider.as_str());
        let source_icon = if self.model_provider == "ollama" {
            "↯"
        } else {
            "☁"
        };
        let source_color = if self.model_provider == "ollama" {
            t.accent()
        } else {
            t.dim()
        };
        let auth_icon = if self.is_oauth { "●" } else { "○" };
        let auth_color = if self.is_oauth {
            t.success()
        } else {
            t.muted()
        };

        let ctx_class_color = match self.actual_context_class {
            ContextClass::Massive => t.accent(),
            ContextClass::Extended => t.fg(),
            _ => t.dim(),
        };
        let context_badge = self.actual_context_class.short().to_string();

        lines.push(Line::from(vec![
            Span::styled(format!("{source_icon} "), Style::default().fg(source_color)),
            Span::styled(
                provider_label.to_string(),
                Style::default().fg(t.fg()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(t.border_dim())),
            Span::styled(context_badge, Style::default().fg(ctx_class_color)),
            Span::styled(
                format!(" {:.0}%", self.context_percent.min(100.0)),
                Style::default().fg(widgets::percent_color(self.context_percent, t)),
            ),
            Span::styled(
                if self.context_window > 0 {
                    format!("/{}", widgets::format_tokens(self.context_window))
                } else {
                    String::new()
                },
                Style::default().fg(t.border_dim()),
            ),
        ]));

        // Second line: model + auth + persona badge
        let mut auth_parts: Vec<Span<'static>> = vec![
            Span::styled(
                model_short.to_string(),
                Style::default().fg(t.muted()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(t.border_dim())),
            Span::styled(format!("{auth_icon} "), Style::default().fg(auth_color)),
            Span::styled(
                if self.is_oauth {
                    "subscription · interactive only"
                } else {
                    "api key"
                },
                Style::default().fg(t.muted()),
            ),
        ];

        // Persona badge
        if let Some(ref p) = self.harness.active_persona {
            auth_parts.push(Span::styled(" · ", Style::default().fg(t.border_dim())));
            auth_parts.push(Span::styled(
                format!("{} {}", p.badge, p.name),
                Style::default().fg(t.accent()),
            ));
        }
        // Tone badge
        if let Some(ref tone) = self.harness.active_tone {
            auth_parts.push(Span::styled(" · ", Style::default().fg(t.border_dim())));
            auth_parts.push(Span::styled(
                format!("♪ {}", tone.name),
                Style::default().fg(t.dim()),
            ));
        }

        lines.push(Line::from(auth_parts));
        if let Some(ref warning) = self.route_warning {
            lines.push(Line::from(Span::styled(
                crate::util::truncate(warning, inner.width.saturating_sub(1) as usize),
                Style::default().fg(t.warning()),
            )));
        }

        if let Some(line) = format_provider_telemetry_compact(self.provider_telemetry.as_ref()) {
            lines.push(Line::from(vec![
                Span::styled("quota ", Style::default().fg(t.dim())),
                Span::styled(line, Style::default().fg(t.accent_muted())),
            ]));
        }

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(t.footer_bg()));
        frame.render_widget(widget, inner);
    }

    fn render_memory_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("memory", t);
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        let sep = Span::styled(" · ", Style::default().fg(t.dim()));
        let mut parts: Vec<Span<'static>> = vec![
            Span::styled("⌗ ", Style::default().fg(t.accent())),
            Span::styled(
                format!("{}", self.total_facts),
                Style::default().fg(t.muted()),
            ),
        ];

        if self.injected_facts > 0 {
            parts.push(sep.clone());
            parts.push(Span::styled("inj ", Style::default().fg(t.dim())));
            parts.push(Span::styled(
                format!("{}", self.injected_facts),
                Style::default().fg(t.muted()),
            ));
        }

        if self.working_memory > 0 {
            parts.push(sep.clone());
            parts.push(Span::styled("wm ", Style::default().fg(t.dim())));
            parts.push(Span::styled(
                format!("{}", self.working_memory),
                Style::default().fg(t.muted()),
            ));
        }

        if self.memory_tokens_est > 0 {
            parts.push(sep);
            parts.push(Span::styled(
                format!("~{}", widgets::format_tokens(self.memory_tokens_est)),
                Style::default().fg(t.dim()),
            ));
        }

        lines.push(Line::from(parts));

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(t.footer_bg()));
        frame.render_widget(widget, inner);
    }

    fn render_system_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let border_color = if self.compaction_flash_ticks > 0 {
            t.accent()
        } else {
            t.border_dim()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color).bg(t.footer_bg()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(
                " system ",
                Style::default().fg(t.muted()).bg(t.footer_bg()),
            ))
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(t.footer_bg()));

        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // cwd — shorten home dir
        let home = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default();
        let display_cwd = if !home.is_empty() && self.cwd.starts_with(&home) {
            format!("~{}", &self.cwd[home.len()..])
        } else {
            self.cwd.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{} ",
                    crate::tui::glyphs::glyphs()
                        .workspace(crate::tui::glyphs::WorkspaceGlyphRole::Directory)
                ),
                Style::default().fg(t.dim()),
            ),
            Span::styled(display_cwd, Style::default().fg(t.muted())),
        ]));

        // Second line: MCP + secrets + tool calls + compactions
        {
            let mut parts: Vec<Span<'static>> = Vec::new();

            // MCP servers
            let mcp_connected = self
                .harness
                .mcp_servers
                .iter()
                .filter(|s| s.connected)
                .count();
            if mcp_connected > 0 {
                let tool_count = self.harness.mcp_tool_count();
                parts.push(Span::styled("MCP ", Style::default().fg(t.dim())));
                parts.push(Span::styled(
                    format!("{}({}t)", mcp_connected, tool_count),
                    Style::default().fg(t.accent()),
                ));
            }

            // Secrets
            if let Some(ref sec) = self.harness.secret_backend {
                if !parts.is_empty() {
                    parts.push(Span::styled(" · ", Style::default().fg(t.dim())));
                }
                let icon = if sec.locked { "🔒" } else { "🔓" };
                parts.push(Span::styled(
                    format!("{icon} {}", sec.stored_count),
                    Style::default().fg(t.muted()),
                ));
            }

            // Tool calls
            if self.tool_calls > 0 {
                if !parts.is_empty() {
                    parts.push(Span::styled(" · ", Style::default().fg(t.dim())));
                }
                parts.push(Span::styled("⚙ ", Style::default().fg(t.dim())));
                parts.push(Span::styled(
                    format!("{}", self.tool_calls),
                    Style::default().fg(t.muted()),
                ));
            }

            // Compactions - show ⟳ icon when flashing
            if self.compactions > 0 {
                if !parts.is_empty() {
                    parts.push(Span::styled(" · ", Style::default().fg(t.dim())));
                }
                let icon = if self.compaction_flash_ticks > 0 {
                    "⟳"
                } else {
                    "↻"
                };
                let color = if self.compaction_flash_ticks > 0 {
                    t.accent()
                } else {
                    t.dim()
                };
                parts.push(Span::styled(format!("{icon} "), Style::default().fg(color)));
                parts.push(Span::styled(
                    format!("{}", self.compactions),
                    Style::default().fg(t.muted()),
                ));
            }

            if !parts.is_empty() {
                lines.push(Line::from(parts));
            }
        }

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(t.footer_bg()));
        frame.render_widget(widget, inner);
    }
}

/// Extract short model name from full ID.
/// Capitalize first letter of a string.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

fn truncate_for_width(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let display_width: usize = value
        .chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum();
    if display_width <= max_width {
        return value.to_string();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > max_width - 1 {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

fn shorten_cwd(cwd: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let home_compacted = std::env::var("HOME")
        .ok()
        .filter(|home| cwd == home || cwd.starts_with(&format!("{home}/")))
        .map(|home| cwd.replacen(&home, "~", 1))
        .unwrap_or_else(|| cwd.to_string());

    if home_compacted.chars().count() <= max_chars {
        return home_compacted;
    }

    let path = std::path::Path::new(&home_compacted);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(home_compacted.as_str());

    if file_name.chars().count() + 2 >= max_chars {
        let tail: String = file_name
            .chars()
            .rev()
            .take(max_chars.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return format!("…{tail}");
    }

    let keep = max_chars.saturating_sub(file_name.chars().count() + 2);
    let prefix: String = home_compacted.chars().take(keep).collect();
    format!("{prefix}…/{file_name}")
}

fn format_context_text(
    actual_class: ContextClass,
    context_percent: f32,
    context_window: usize,
) -> String {
    let badge = actual_class.short().to_string();
    if context_window > 0 {
        format!(
            "{} {:.0}% / ¤{}",
            badge,
            context_percent,
            widgets::format_tokens(context_window)
        )
    } else {
        format!("{} {:.0}%", badge, context_percent)
    }
}

fn is_local_provider(provider: &str) -> bool {
    matches!(provider, "ollama" | "local")
}

fn format_version_text(update_available: Option<&str>) -> String {
    match update_available {
        Some(_) => format!("v{}* - /update", env!("CARGO_PKG_VERSION")),
        None => format!("v{}", env!("CARGO_PKG_VERSION")),
    }
}

fn version_spans(update_available: Option<&str>, t: &dyn Theme) -> Vec<Span<'static>> {
    let current = env!("CARGO_PKG_VERSION");
    let (base, suffix) = current.split_once("-").unwrap_or((current, ""));
    let mut spans = vec![Span::styled(
        format!("v{base}"),
        Style::default()
            .fg(t.accent_bright())
            .add_modifier(Modifier::BOLD),
    )];
    if !suffix.is_empty() {
        spans.push(Span::styled(
            format!("-{suffix}"),
            Style::default().fg(t.dim()),
        ));
    }
    if update_available.is_some() {
        spans.push(Span::styled(
            "*",
            Style::default()
                .fg(t.warning())
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" - /update", Style::default().fg(t.dim())));
    }
    spans
}

fn short_model(model_id: &str) -> String {
    crate::settings::humanize_model_id(model_id)
}

pub(crate) fn format_session_text(
    turn: u32,
    session_input_tokens: u64,
    session_output_tokens: u64,
    last_turn_input_tokens: u64,
    _last_turn_output_tokens: u64,
    _session_usage_slices: &[SessionUsageSlice],
) -> String {
    let mut parts = vec![format!("T{turn}")];

    if session_input_tokens > 0 || session_output_tokens > 0 {
        parts.push(format!(
            "¤{}/¤{}",
            widgets::format_tokens_compact(session_input_tokens as usize),
            widgets::format_tokens_compact(session_output_tokens as usize)
        ));
    }

    // Per-turn token indicator
    if last_turn_input_tokens > 0 {
        parts.push(format!(
            "(turn ¤{})",
            widgets::format_tokens_compact(last_turn_input_tokens as usize),
        ));
    }

    parts.join(" ")
}

fn format_failure_age(timestamp: &str) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(timestamp).ok()?;
    let age = Utc::now().signed_duration_since(parsed.with_timezone(&Utc));
    if age.num_seconds() < 0 {
        return Some("last just now".to_string());
    }
    let secs = age.num_seconds();
    if secs < 60 {
        return Some("last just now".to_string());
    }
    let mins = age.num_minutes();
    if mins < 60 {
        return Some(format!("last {mins}m ago"));
    }
    let hours = age.num_hours();
    if hours < 24 {
        return Some(format!("last {hours}h ago"));
    }
    Some(format!("last {}d ago", age.num_days()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::format_duration_compact;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn footer_projects_semantic_status_surface() {
        let mut data = FooterData {
            model_id: "anthropic:claude-sonnet-4-6".into(),
            model_provider: "anthropic".into(),
            context_percent: 37.5,
            context_window: 200_000,
            estimated_tokens: 75_000,
            total_facts: 10,
            injected_facts: 3,
            working_memory: 4,
            memory_tokens_est: 1200,
            session_input_tokens: 100,
            session_output_tokens: 200,
            last_turn_input_tokens: 10,
            last_turn_output_tokens: 20,
            tool_calls: 5,
            turn: 2,
            compactions: 1,
            cwd: "/tmp/omegon".into(),
            is_oauth: true,
            model_tier: "frontier".into(),
            thinking_level: "medium".into(),
            posture: "engineer".into(),
            runtime_brand: "OM".into(),
            principal_id: "operator".into(),
            authorization: "trusted".into(),
            provider_connected: true,
            web_search_providers: vec![
                crate::capabilities::secrets::WebSearchProviderReadiness {
                    provider: "brave",
                    secret_name: "BRAVE_API_KEY",
                    configured: true,
                },
                crate::capabilities::secrets::WebSearchProviderReadiness {
                    provider: "tavily",
                    secret_name: "TAVILY_API_KEY",
                    configured: false,
                },
            ],
            update_available: Some("0.27.1".into()),
            sandbox: true,
            ..Default::default()
        };
        data.harness.git_branch = Some("release/0.27".into());

        let projection = data.projection();
        assert_eq!(projection.engine.model_id, "anthropic:claude-sonnet-4-6");
        assert_eq!(projection.engine.model_provider, "anthropic");
        assert_eq!(
            projection.engine.web_search_providers,
            vec![("brave".into(), true), ("tavily".into(), false)]
        );
        assert!(projection.engine.model_short.contains("sonnet"));
        assert_eq!(projection.context.percent, 37.5);
        assert_eq!(projection.context.window, 200_000);
        assert_eq!(projection.memory.total_facts, 10);
        assert_eq!(projection.memory.injected_facts, 3);
        assert_eq!(projection.session.turn, 2);
        assert_eq!(projection.session.tool_calls, 5);
        assert_eq!(projection.workspace.cwd_basename, "omegon");
        assert_eq!(
            projection.workspace.git_branch.as_deref(),
            Some("release/0.27")
        );
        assert!(projection.workspace.is_oauth);
    }

    #[test]
    fn footer_renders_without_panic() {
        let data = FooterData {
            model_id: "claude-sonnet-4-6".into(),
            model_provider: "anthropic".into(),
            context_percent: 45.0,
            context_window: 200_000,
            total_facts: 150,
            turn: 5,
            tool_calls: 12,
            ..Default::default()
        };
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                data.render(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();
    }

    #[test]
    fn engine_flex_row_right_aligns_value() {
        let line = FooterData::engine_flex_row(
            "model",
            "gpt-5.5".to_string(),
            32,
            7,
            22,
            EngineFlexRowStyle::new(Color::Blue, Color::White, true),
        );
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(UnicodeWidthStr::width(text.as_str()), 32);
        assert!(text.starts_with(" model   "), "{text:?}");
        assert!(text.ends_with("gpt-5.5"), "{text:?}");
        assert!(text.contains("  gpt-5.5"), "{text:?}");
    }

    #[test]
    fn engine_flex_row_truncates_value_before_right_edge() {
        let line = FooterData::engine_flex_row(
            "limit",
            "codex 100% left · 7d 40% left · credits metered · ok".to_string(),
            28,
            7,
            18,
            EngineFlexRowStyle::new(Color::Blue, Color::White, false),
        );
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(UnicodeWidthStr::width(text.as_str()), 28);
        assert!(text.starts_with(" limit   "), "{text:?}");
        assert!(text.ends_with('…'), "{text:?}");
    }

    #[test]
    fn footer_narrow_terminal() {
        let data = FooterData::default();
        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                data.render(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();
    }

    #[test]
    fn footer_shows_model() {
        let data = FooterData {
            model_id: "claude-opus-4-6".into(),
            model_provider: "anthropic".into(),
            ..Default::default()
        };
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                data.render(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text: String = {
            let buf = terminal.backend().buffer();
            let a = buf.area;
            (0..a.height)
                .flat_map(|y| (0..a.width).map(move |x| buf[(x, y)].symbol().to_string()))
                .collect()
        };
        assert!(text.contains("opus"), "should show model: {text}");
    }

    #[test]
    fn footer_shows_context_percent() {
        let data = FooterData {
            context_percent: 75.0,
            context_window: 200_000,
            ..Default::default()
        };
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                data.render(frame.area(), frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let text: String = {
            let buf = terminal.backend().buffer();
            let a = buf.area;
            (0..a.height)
                .flat_map(|y| (0..a.width).map(move |x| buf[(x, y)].symbol().to_string()))
                .collect()
        };
        assert!(
            text.contains("75") || text.contains("200k"),
            "should show context info: {text}"
        );
    }

    #[test]
    fn model_card_shows_persistent_route_warning() {
        let data = FooterData {
            model_id: "claude-fable-5".into(),
            model_provider: "anthropic".into(),
            route_warning: Some("Login timed out waiting for browser callback".into()),
            context_window: 200_000,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 180, 8);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| data.render(area, frame, &super::super::theme::Alpharius))
            .unwrap();

        let text = render_to_string(&terminal);
        assert!(text.contains("Login timed out"), "{text}");
    }

    #[test]
    fn footer_render_clears_dirty_cells_in_owned_area() {
        let data = FooterData {
            model_id: "claude-sonnet-4-6".into(),
            model_provider: "anthropic".into(),
            context_percent: 45.0,
            context_window: 200_000,
            total_facts: 150,
            turn: 5,
            tool_calls: 12,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 120, 5);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char('X');
                            cell.set_fg(Color::Red);
                            cell.set_bg(Color::Red);
                        }
                    }
                }
                data.render(area, frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let residual = (area.top()..area.bottom())
            .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].symbol() == "X")
            .collect::<Vec<_>>();
        assert!(
            residual.is_empty(),
            "footer should clear dirty cells it owns, residual: {residual:?}"
        );
    }

    #[test]
    fn footer_left_panel_clears_dirty_cells_in_owned_area() {
        let data = FooterData {
            model_id: "ollama:qwen3".into(),
            model_provider: "ollama".into(),
            context_percent: 68.0,
            context_window: 262_144,
            thinking_level: "high".into(),
            model_tier: "B".into(),
            provider_connected: true,
            ..Default::default()
        };
        let area = Rect::new(0, 0, 40, 8);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char('Ω');
                            cell.set_fg(Color::White);
                            cell.set_bg(Color::Black);
                        }
                    }
                }
                data.render_engine_fallback_panel(area, frame, &super::super::theme::Alpharius);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let residual = (area.top()..area.bottom())
            .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
            .filter(|(x, y)| buf[(*x, *y)].symbol() == "Ω")
            .collect::<Vec<_>>();
        assert!(
            residual.is_empty(),
            "left footer panel should clear dirty cells it owns, residual: {residual:?}"
        );
    }

    #[test]
    fn cwd_default_is_empty() {
        let data = FooterData::default();
        assert!(data.model_id.is_empty());
        assert_eq!(data.context_percent, 0.0);
    }

    #[test]
    fn shorten_cwd_replaces_home_with_tilde() {
        let home = std::env::var("HOME").unwrap();
        let path = format!("{home}/workspace/black-meridian/omegon");
        let shortened = shorten_cwd(&path, 128);
        assert!(
            shortened.starts_with("~/"),
            "expected ~ prefix: {shortened}"
        );
    }

    #[test]
    fn session_text_is_compact_and_token_only() {
        let text = format_session_text(
            1,
            12_000,
            3_000,
            0,
            0,
            &[SessionUsageSlice {
                model_id: "anthropic:claude-sonnet-4-6".into(),
                provider: "anthropic".into(),
                input_tokens: 12_000,
                output_tokens: 3_000,
            }],
        );
        assert!(text.starts_with("T1 "), "got {text}");
        assert!(text.contains("¤12k/¤3k"), "got {text}");
        assert!(!text.contains("~$"), "got {text}");
        assert!(!text.contains('⚙'), "got {text}");
        assert!(!text.contains('↻'), "got {text}");
        assert!(!text.contains('·'), "got {text}");
    }

    #[test]
    fn provider_telemetry_line_formats_unified_usage() {
        let text =
            format_provider_telemetry_compact(Some(&omegon_traits::ProviderTelemetrySnapshot {
                provider: "anthropic".into(),
                source: "response_headers".into(),
                unified_5h_utilization_pct: Some(42.0),
                unified_7d_utilization_pct: Some(64.0),
                retry_after_secs: Some(17),
                ..Default::default()
            }))
            .expect("telemetry line");
        assert!(text.contains("5h 42%"), "got {text}");
        assert!(text.contains("7d 64%"), "got {text}");
        assert!(text.contains("retry 17s"), "got {text}");
        assert!(text.ends_with("ok"), "got {text}");
    }

    #[test]
    fn provider_telemetry_line_formats_codex_headers() {
        let text =
            format_provider_telemetry_compact(Some(&omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_active_limit: Some("codex".into()),
                codex_primary_used_pct: Some(0.0),
                codex_secondary_used_pct: Some(60.0),
                codex_primary_reset_secs: Some(13648),
                codex_secondary_reset_secs: Some(348644),
                codex_credits_unlimited: Some(false),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                ..Default::default()
            }))
            .expect("telemetry line");
        assert!(!text.contains("GPT-5.3-Codex-Spark"), "got {text}");
        assert!(text.contains("codex 100% left"), "got {text}");
        assert!(!text.contains("resets 3h47m"), "got {text}");
        assert!(text.contains("7d 40% left"), "got {text}");
        assert!(text.contains("credits metered"), "got {text}");
        assert!(!text.contains("primary"), "got {text}");
        assert!(!text.contains('↻'), "got {text}");
        assert!(text.ends_with("ok"), "got {text}");
    }

    #[test]
    fn truncate_for_width_adds_ellipsis() {
        assert_eq!(truncate_for_width("weekly 4d0h", 6), "weekl…");
        assert_eq!(truncate_for_width("ok", 6), "ok");
        assert_eq!(truncate_for_width("ok", 0), "");
    }

    #[test]
    fn truncate_for_width_uses_display_cell_width_not_char_count() {
        // `⤴` is rendered as a full terminal cell glyph; truncation must use
        // display width math so mixed ASCII + symbol rows do not drift into the
        // neighboring panel when the footer is narrow.
        let value = "⤴ OpenAI/Codex · ↻ sub";
        let truncated = truncate_for_width(value, 10);
        let rendered_width: usize = truncated
            .chars()
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum();
        assert!(
            rendered_width <= 10,
            "got {truncated:?} width {rendered_width}"
        );
        assert!(truncated.ends_with('…'), "got {truncated:?}");
    }

    #[test]
    fn format_duration_compact_covers_ranges() {
        assert_eq!(format_duration_compact(0), "0s");
        assert_eq!(format_duration_compact(59), "59s");
        assert_eq!(format_duration_compact(60), "1m");
        assert_eq!(format_duration_compact(3599), "59m");
        assert_eq!(format_duration_compact(3600), "1h");
        assert_eq!(format_duration_compact(13648), "3h47m");
        assert_eq!(format_duration_compact(86400), "1d");
        assert_eq!(format_duration_compact(348644), "4d");
    }

    #[test]
    fn session_text_stays_token_only_for_unknown_models() {
        let text = format_session_text(
            2,
            12_000,
            3_000,
            0,
            0,
            &[SessionUsageSlice {
                model_id: "unknown:custom-model".into(),
                provider: "unknown".into(),
                input_tokens: 12_000,
                output_tokens: 3_000,
            }],
        );
        assert_eq!(text, "T2 ¤12k/¤3k");
    }

    #[test]
    fn version_text_only_shows_update_hint_when_update_exists() {
        let stable = format_version_text(None);
        assert_eq!(stable, format!("v{}", env!("CARGO_PKG_VERSION")));

        let upgrade = format_version_text(Some("9.9.9"));
        assert_eq!(
            upgrade,
            format!("v{}* - /update", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn context_text_compacts_class_percent_and_window() {
        assert_eq!(
            format_context_text(ContextClass::Standard, 68.0, 272_000),
            "Standard 68% / ¤272k"
        );
        assert_eq!(
            format_context_text(ContextClass::Extended, 42.0, 0),
            "Extended 42%"
        );
        assert_eq!(
            format_context_text(ContextClass::Compact, 68.0, 131_072),
            "Compact 68% / ¤131k"
        );
    }

    fn render_engine_fallback_panel_text(data: &FooterData, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                data.render_engine_fallback_panel(
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                );
            })
            .unwrap();

        render_to_string(&terminal)
    }

    fn render_to_string(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let a = buf.area;
        (0..a.height)
            .flat_map(|y| (0..a.width).map(move |x| buf[(x, y)].symbol().to_string()))
            .collect()
    }

    #[test]
    fn model_card_shows_policy_to_actual_context_mismatch() {
        let data = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            context_percent: 68.0,
            context_window: 131_072,
            context_class: ContextClass::Massive,
            actual_context_class: ContextClass::Compact,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            turn: 7,
            thinking_level: "high".into(),
            model_tier: "B".into(),
            provider_connected: true,
            is_oauth: true,
            ..Default::default()
        };
        let text = render_engine_fallback_panel_text(&data, 64, 10);
        assert!(!text.contains("Massive→Compact"), "got {text}");
    }

    #[test]
    fn left_panel_marks_degraded_status_with_recency() {
        let recent = (Utc::now() - chrono::Duration::minutes(2)).to_rfc3339();
        let data = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            provider_connected: true,
            harness: crate::status::HarnessStatus {
                providers: vec![crate::status::ProviderStatus {
                    name: "openai".into(),
                    authenticated: true,
                    auth_method: Some("oauth".into()),
                    auth_state: Some(crate::status::ProviderAuthState::Configured),
                    model: Some("gpt-5.4".into()),
                    runtime_status: Some(crate::status::ProviderRuntimeStatus::Degraded),
                    recent_failure_count: Some(6),
                    last_failure_kind: Some("stalled stream".into()),
                    last_failure_at: Some(recent),
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let text = render_engine_fallback_panel_text(&data, 72, 8);

        assert!(
            text.contains("6× stalled stream · last 2m ago"),
            "got {text}"
        );
    }

    #[test]
    fn left_panel_renders_provider_telemetry_when_present() {
        let data = FooterData {
            model_id: "openai-codex:gpt-5.4".into(),
            model_provider: "openai-codex".into(),
            provider_connected: true,
            is_oauth: true,
            thinking_level: "high".into(),
            model_tier: "B".into(),
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                codex_active_limit: Some("codex".into()),
                codex_primary_used_pct: Some(0.0),
                codex_secondary_used_pct: Some(60.0),
                codex_primary_reset_secs: Some(13_648),
                codex_secondary_reset_secs: Some(348_644),
                codex_credits_unlimited: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = render_engine_fallback_panel_text(&data, 96, 10);

        assert!(text.contains("limit"), "got {text}");
        assert!(!text.contains("bucket GPT-5.3-Codex-Spark"), "got {text}");
        assert!(!text.contains("GPT-5.3-Codex-Spark"), "got {text}");
        assert!(text.contains("codex 100% left"), "got {text}");
        assert!(!text.contains("resets 3h47m"), "got {text}");
    }

    #[test]
    fn left_panel_renders_session_stats_when_present() {
        let data = FooterData {
            model_id: "openai-codex:gpt-5.4".into(),
            model_provider: "openai-codex".into(),
            provider_connected: true,
            is_oauth: true,
            thinking_level: "medium".into(),
            model_tier: "B".into(),
            turn: 9,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            ..Default::default()
        };
        let text = render_engine_fallback_panel_text(&data, 72, 10);

        assert!(!text.contains("version"), "got {text}");
        assert!(!text.contains("session"), "got {text}");
        assert!(!text.contains("T9"), "got {text}");
        assert!(!text.contains("¤12k/¤3k"), "got {text}");
    }

    #[test]
    fn left_panel_truncates_codex_limit_row_aggressively() {
        let data = FooterData {
            model_id: "openai-codex:gpt-5.4".into(),
            model_provider: "openai-codex".into(),
            provider_connected: true,
            is_oauth: true,
            thinking_level: "high".into(),
            model_tier: "B".into(),
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                codex_active_limit: Some("codex".into()),
                codex_primary_used_pct: Some(0.0),
                codex_secondary_used_pct: Some(60.0),
                codex_primary_reset_secs: Some(13_648),
                codex_secondary_reset_secs: Some(348_644),
                codex_credits_unlimited: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = render_engine_fallback_panel_text(&data, 44, 10);

        assert!(text.contains("limit"), "got {text}");
        assert!(text.contains('…'), "got {text}");
        assert!(!text.contains("weekly 4d"), "got {text}");
        assert!(!text.contains("credits metered"), "got {text}");
    }

    #[test]
    fn left_panel_clears_stale_rows_when_content_shrinks() {
        let backend = TestBackend::new(72, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        let verbose = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            provider_connected: true,
            thinking_level: "high".into(),
            model_tier: "B".into(),
            turn: 9,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai".into(),
                source: "response_headers".into(),
                codex_active_limit: Some("codex".into()),
                codex_primary_used_pct: Some(0.0),
                codex_primary_reset_secs: Some(13_648),
                ..Default::default()
            }),
            operator_events: vec![OperatorEventLine {
                icon: "!",
                message: "stale row sentinel".into(),
                color: Color::Yellow,
            }],
            ..Default::default()
        };

        let compact = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            provider_connected: false,
            ..Default::default()
        };

        terminal
            .draw(|frame| {
                verbose.render_engine_fallback_panel(
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                )
            })
            .unwrap();
        terminal
            .draw(|frame| {
                compact.render_engine_fallback_panel(
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                )
            })
            .unwrap();

        let text = render_to_string(&terminal);
        assert!(text.contains("OpenAI API login required"), "got {text}");
        assert!(text.contains("/login openai"), "got {text}");
        assert!(!text.contains("stale row sentinel"), "got {text}");
        assert!(!text.contains("codex 0%"), "got {text}");
        assert!(!text.contains("T9"), "got {text}");
    }

    #[test]
    fn left_panel_disconnected_provider_names_exact_login_command() {
        let data = FooterData {
            model_id: "openai-codex:gpt-5.5".into(),
            model_provider: "openai-codex".into(),
            provider_connected: false,
            ..Default::default()
        };
        let text = render_engine_fallback_panel_text(&data, 72, 6);

        assert!(text.contains("OpenAI/Codex login required"), "got {text}");
        assert!(text.contains("/login openai-codex"), "got {text}");
        assert!(!text.contains("/login to connect"), "got {text}");
    }

    #[test]
    fn footer_cards_clear_stale_body_rows_when_reused() {
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        let verbose = FooterData {
            model_id: "claude-sonnet-4-6".into(),
            model_provider: "Anthropic".into(),
            context_percent: 72.0,
            context_window: 272_000,
            context_class: ContextClass::Standard,
            total_facts: 1800,
            injected_facts: 95,
            working_memory: 5,
            tool_calls: 23,
            turn: 8,
            estimated_tokens: 195_000,
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "anthropic".into(),
                source: "headers".into(),
                codex_active_limit: Some("burst".into()),
                codex_primary_used_pct: Some(12.0),
                codex_primary_reset_secs: Some(60),
                ..Default::default()
            }),
            harness: HarnessStatus {
                active_persona: Some(crate::status::PersonaSummary {
                    id: "eng".into(),
                    name: "Systems Engineer".into(),
                    badge: "⚙".into(),
                    mind_facts_count: 42,
                    activated_skills: vec!["rust".into()],
                    disabled_tools: vec![],
                }),
                active_tone: Some(crate::status::ToneSummary {
                    id: "concise".into(),
                    name: "Concise".into(),
                    intensity_mode: "full".into(),
                }),
                mcp_servers: vec![crate::status::McpServerStatus {
                    name: "filesystem".into(),
                    transport_mode: crate::status::McpTransportMode::LocalProcess,
                    tool_count: 5,
                    resource_count: 0,
                    prompt_count: 0,
                    connected: true,
                    error: None,
                }],
                secret_backend: Some(crate::status::SecretBackendStatus {
                    backend: "keyring".into(),
                    stored_count: 3,
                    locked: false,
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let compact = FooterData::default();

        terminal
            .draw(|frame| verbose.render(frame.area(), frame, &super::super::theme::Alpharius))
            .unwrap();
        terminal
            .draw(|frame| compact.render(frame.area(), frame, &super::super::theme::Alpharius))
            .unwrap();

        let text = render_to_string(&terminal);
        assert!(!text.contains("Concise"), "got {text}");
        assert!(!text.contains("Systems Engineer"), "got {text}");
        assert!(!text.contains("burst 88% left"), "got {text}");
    }

    #[test]
    fn format_failure_age_handles_recent_and_hourly_values() {
        let just_now = (Utc::now() - chrono::Duration::seconds(30)).to_rfc3339();
        let hourly = (Utc::now() - chrono::Duration::hours(3)).to_rfc3339();

        assert_eq!(
            format_failure_age(&just_now).as_deref(),
            Some("last just now")
        );
        assert_eq!(format_failure_age(&hourly).as_deref(), Some("last 3h ago"));
        assert_eq!(format_failure_age("not-a-time"), None);
    }
}

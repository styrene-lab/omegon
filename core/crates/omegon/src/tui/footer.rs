//! Footer bar — 4-card telemetry strip at bottom of TUI.
//!
//! Each card is a bordered Block with a title bar. Cards share `card_bg`
//! background for visual cohesion.

use chrono::{DateTime, Utc};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph};

use super::model_catalog::ModelCatalog;
use super::theme::Theme;
use super::widgets::{self, GaugeConfig};

use crate::settings::{ContextClass, ContextMode};
use crate::status::HarnessStatus;
use crate::usage::format_provider_telemetry_compact;

#[derive(Clone, Debug)]
pub struct OperatorEventLine {
    pub icon: &'static str,
    pub message: String,
    pub color: Color,
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
    pub context_mode: ContextMode,
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
    pub tool_calls: u32,
    pub turn: u32,
    pub compactions: u32,
    pub cwd: String,
    pub is_oauth: bool,
    /// HarnessStatus — persona, MCP, secrets, inference state.
    /// Updated via BusEvent::HarnessStatusChanged.
    pub harness: HarnessStatus,
    /// Compaction flash counter — set to 3 when compaction occurs, decrements each frame.
    /// When > 0, system card renders with accent border.
    pub compaction_flash_ticks: u8,
    /// Current thinking level name (for engine panel display).
    pub thinking_level: String,
    /// Current model tier name (for engine panel display).
    pub model_tier: String,
    /// Whether a live LLM provider is connected. False when NullBridge is active.
    pub provider_connected: bool,
    /// Available update version (if any).
    pub update_available: Option<String>,
    /// Inline operator-facing transient events shown under engine version info.
    pub operator_events: Vec<OperatorEventLine>,
    /// Current provider quota/headroom telemetry, if exposed by the upstream.
    pub provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
}

impl FooterData {
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

    /// Render the left panel for the split-panel layout (engine + memory).
    /// This replaces the 4-card layout when instruments are visible on the right.
    pub fn render_left_panel(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
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

        // Engine only — memory is visualized in the inference panel
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

        let push_row = |lines: &mut Vec<Line<'static>>,
                        label: &str,
                        value: String,
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
                Span::styled(value, value_style),
            ]));
        };

        lines.push(Line::from(Span::styled(
            " engine",
            Style::default()
                .fg(t.accent_muted())
                .add_modifier(Modifier::BOLD),
        )));

        if !self.provider_connected {
            push_row(
                &mut lines,
                "status",
                "⚠ no provider".to_string(),
                t.border_dim(),
                t.warning(),
                true,
            );
            push_row(
                &mut lines,
                "action",
                "/login to connect".to_string(),
                t.border_dim(),
                t.muted(),
                false,
            );
            push_row(
                &mut lines,
                "version",
                format_version_text(self.update_available.as_deref()),
                t.border_dim(),
                t.dim(),
                false,
            );
        } else {
            let model_short = short_model(&self.model_id);
            let provider_label = crate::auth::provider_by_id(&self.model_provider)
                .map(|p| p.display_name)
                .unwrap_or(self.model_provider.as_str());
            let provider_runtime = self
                .harness
                .providers
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&self.model_provider));
            let provider_icon = if is_local_provider(&self.model_provider) {
                "⤵"
            } else {
                "⤴"
            };
            let auth_text = if is_local_provider(&self.model_provider) {
                "● local"
            } else if self.is_oauth {
                "↻ sub"
            } else {
                "○ api"
            };
            let provider_text = format!("{provider_icon} {provider_label} · {auth_text}");
            let context_text = format_context_text(
                self.context_class,
                self.actual_context_class,
                self.context_percent.min(100.0),
                self.context_window,
            );
            // Tier + thinking level on a separate row so the model name never overflows.
            let tier_line = if self.thinking_level.is_empty() || self.thinking_level == "off" {
                capitalize(&self.model_tier)
            } else {
                format!(
                    "{} · {}",
                    capitalize(&self.model_tier),
                    capitalize(&self.thinking_level)
                )
            };
            let state_line = context_text;
            let session_line = format_session_text(
                &self.model_id,
                self.turn,
                self.session_input_tokens,
                self.session_output_tokens,
            );

            push_row(
                &mut lines,
                "provider",
                provider_text,
                t.border_dim(),
                t.fg(),
                true,
            );
            if let Some(provider) = provider_runtime
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
                    t.border_dim(),
                    t.warning(),
                    false,
                );
            }
            push_row(
                &mut lines,
                "model",
                model_short.to_string(),
                t.border_dim(),
                t.muted(),
                true,
            );
            push_row(
                &mut lines,
                "version",
                format_version_text(self.update_available.as_deref()),
                t.border_dim(),
                t.dim(),
                false,
            );
            if !self.model_tier.is_empty() {
                push_row(
                    &mut lines,
                    "tier",
                    tier_line,
                    t.border_dim(),
                    t.dim(),
                    false,
                );
            }
            push_row(
                &mut lines,
                "state",
                state_line,
                t.border_dim(),
                widgets::percent_color(self.context_percent.min(100.0), t),
                false,
            );
            if self.turn > 0 || self.session_input_tokens > 0 || self.session_output_tokens > 0 {
                push_row(
                    &mut lines,
                    "session",
                    session_line,
                    t.border_dim(),
                    t.muted(),
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
                    t.border_dim(),
                    t.accent_muted(),
                    false,
                );
            }

            for event in self.operator_events.iter().take(2) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<width$} ", event.icon, width = label_width),
                        Style::default().fg(event.color),
                    ),
                    Span::styled(event.message.clone(), Style::default().fg(event.color)),
                ]));
            }
        }

        frame.render_widget(Clear, inner);
        let widget = Paragraph::new(lines).style(Style::default().bg(bg));
        frame.render_widget(widget, inner);
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
            "⚡"
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
            ContextClass::Legion => t.accent(),
            ContextClass::Clan => t.fg(),
            _ => t.dim(),
        };
        let context_badge = if self.context_class != self.actual_context_class {
            format!("{}→{}", self.context_class.short(), self.actual_context_class.short())
        } else {
            self.actual_context_class.short().to_string()
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{source_icon} "), Style::default().fg(source_color)),
            Span::styled(
                provider_label.to_string(),
                Style::default().fg(t.fg()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(t.border_dim())),
            Span::styled(
                context_badge,
                Style::default().fg(ctx_class_color),
            ),
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
            Span::styled("⌂ ", Style::default().fg(t.dim())),
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
    requested_class: ContextClass,
    actual_class: ContextClass,
    context_percent: f32,
    context_window: usize,
) -> String {
    let badge = if requested_class != actual_class {
        format!("{}→{}", requested_class.short(), actual_class.short())
    } else {
        actual_class.short().to_string()
    };
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
        Some(latest) => format!("v{} → v{latest}", env!("CARGO_PKG_VERSION")),
        None => format!("v{}", env!("CARGO_PKG_VERSION")),
    }
}

fn short_model(model_id: &str) -> String {
    crate::settings::humanize_model_id(model_id)
}

fn format_session_text(
    model_id: &str,
    turn: u32,
    session_input_tokens: u64,
    session_output_tokens: u64,
) -> String {
    let mut parts = vec![format!("T{turn}")];

    if session_input_tokens > 0 || session_output_tokens > 0 {
        parts.push(format!(
            "¤{}/¤{}",
            widgets::format_tokens_compact(session_input_tokens as usize),
            widgets::format_tokens_compact(session_output_tokens as usize)
        ));
    }

    if let Some(cost) =
        estimate_session_cost_usd(model_id, session_input_tokens, session_output_tokens)
    {
        parts.push(format_cost_usd(cost));
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

fn estimate_session_cost_usd(
    model_id: &str,
    session_input_tokens: u64,
    session_output_tokens: u64,
) -> Option<f64> {
    let pricing = ModelCatalog::pricing_for_model(model_id)?;
    Some(pricing.estimate_cost_usd(session_input_tokens, session_output_tokens))
}

fn format_cost_usd(cost_usd: f64) -> String {
    if cost_usd <= 0.0 {
        return "~$0".to_string();
    }
    if cost_usd < 0.01 {
        format!("~${cost_usd:.3}")
    } else if cost_usd < 1.0 {
        format!("~${cost_usd:.2}")
    } else {
        format!("~${cost_usd:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::format_duration_compact;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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
            model_tier: "victory".into(),
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
                data.render_left_panel(area, frame, &super::super::theme::Alpharius);
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
    fn session_text_is_compact_and_includes_cost_when_priced() {
        let text = format_session_text("anthropic:claude-sonnet-4-6", 1, 12_000, 3_000);
        assert!(text.starts_with("T1 "), "got {text}");
        assert!(text.contains("¤12k/¤3k"), "got {text}");
        assert!(text.contains("~$"), "got {text}");
        assert!(!text.contains('⚙'), "got {text}");
        assert!(!text.contains('↻'), "got {text}");
        assert!(!text.contains('·'), "got {text}");
    }

    #[test]
    fn provider_telemetry_line_formats_unified_usage() {
        let text = format_provider_telemetry_compact(Some(
            &omegon_traits::ProviderTelemetrySnapshot {
                provider: "anthropic".into(),
                source: "response_headers".into(),
                unified_5h_utilization_pct: Some(42.0),
                unified_7d_utilization_pct: Some(64.0),
                retry_after_secs: Some(17),
                ..Default::default()
            },
        ))
        .expect("telemetry line");
        assert!(text.contains("5h 42%"), "got {text}");
        assert!(text.contains("7d 64%"), "got {text}");
        assert!(text.contains("retry 17s"), "got {text}");
        assert!(text.ends_with("ok"), "got {text}");
    }

    #[test]
    fn provider_telemetry_line_formats_codex_headers() {
        let text = format_provider_telemetry_compact(Some(
            &omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_active_limit: Some("codex".into()),
                codex_primary_pct: Some(0),
                codex_primary_reset_secs: Some(13648),
                codex_secondary_reset_secs: Some(348644),
                codex_credits_unlimited: Some(false),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                ..Default::default()
            },
        ))
        .expect("telemetry line");
        assert!(!text.contains("GPT-5.3-Codex-Spark"), "got {text}");
        assert!(text.contains("codex 0%"), "got {text}");
        assert!(text.contains("resets 3h47m"), "got {text}");
        assert!(text.contains("weekly 4d"), "got {text}");
        assert!(text.contains("credits metered"), "got {text}");
        assert!(!text.contains("primary"), "got {text}");
        assert!(!text.contains('↻'), "got {text}");
        assert!(text.ends_with("ok"), "got {text}");
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
    fn session_text_falls_back_to_tokens_when_pricing_unknown() {
        let text = format_session_text("unknown:custom-model", 2, 12_000, 3_000);
        assert_eq!(text, "T2 ¤12k/¤3k");
    }

    #[test]
    fn session_text_shows_cost_for_priced_models_even_without_catalog_availability() {
        let text = format_session_text("openai:gpt-5.4", 2, 12_000, 3_000);
        assert!(text.contains("~$"), "got {text}");
    }

    #[test]
    fn version_text_only_shows_transition_when_update_exists() {
        let stable = format_version_text(None);
        assert_eq!(stable, format!("v{}", env!("CARGO_PKG_VERSION")));

        let upgrade = format_version_text(Some("9.9.9"));
        assert_eq!(upgrade, format!("v{} → v9.9.9", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn context_text_compacts_class_percent_and_window() {
        assert_eq!(
            format_context_text(ContextClass::Maniple, ContextClass::Maniple, 68.0, 272_000),
            "Maniple 68% / ¤272k"
        );
        assert_eq!(
            format_context_text(ContextClass::Clan, ContextClass::Clan, 42.0, 0),
            "Clan 42%"
        );
        assert_eq!(
            format_context_text(ContextClass::Legion, ContextClass::Squad, 68.0, 131_072),
            "Legion→Squad 68% / ¤131k"
        );
    }

    fn render_left_panel_text(data: &FooterData, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                data.render_left_panel(frame.area(), frame, &super::super::theme::Alpharius);
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
    fn left_panel_keeps_version_visible_without_showing_path_noise() {
        let data = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            context_percent: 68.0,
            context_window: 272_000,
            context_class: ContextClass::Maniple,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            turn: 7,
            cwd: "/Users/test/workspace/black-meridian/omegon/core/crates/omegon".into(),
            thinking_level: "high".into(),
            model_tier: "victory".into(),
            provider_connected: true,
            update_available: Some("9.9.9".into()),
            ..Default::default()
        };
        let text = render_left_panel_text(&data, 52, 10);

        assert!(text.contains("gpt-5.4"), "got {text}");
        assert!(
            text.contains("Victory") || text.contains("victory"),
            "got {text}"
        );
        assert!(text.contains("High") || text.contains("high"), "got {text}");
        assert!(text.contains("T7"), "got {text}");
        assert!(text.contains("version"), "got {text}");
        assert!(text.contains("v"), "got {text}");
        assert!(text.contains("9.9.9"), "got {text}");
        assert!(!text.contains("/Users/test/workspace"), "got {text}");
    }

    #[test]
    fn model_card_shows_policy_to_actual_context_mismatch() {
        let data = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            context_percent: 68.0,
            context_window: 131_072,
            context_class: ContextClass::Legion,
            actual_context_class: ContextClass::Squad,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            turn: 7,
            thinking_level: "high".into(),
            model_tier: "victory".into(),
            provider_connected: true,
            is_oauth: true,
            ..Default::default()
        };
        let text = render_left_panel_text(&data, 64, 10);
        assert!(text.contains("Legion→Squad"), "got {text}");
    }

    #[test]
    fn left_panel_uses_new_engine_provider_and_token_symbols() {
        let data = FooterData {
            model_id: "openai:gpt-5.4".into(),
            model_provider: "openai".into(),
            context_percent: 68.0,
            context_window: 272_000,
            context_class: ContextClass::Maniple,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            turn: 7,
            thinking_level: "high".into(),
            model_tier: "victory".into(),
            provider_connected: true,
            is_oauth: true,
            ..Default::default()
        };
        let text = render_left_panel_text(&data, 64, 10);

        assert!(
            text.contains("⤴ OpenAI") || text.contains("⤴ openai"),
            "got {text}"
        );
        assert!(text.contains("↻ sub"), "got {text}");
        // model name is on its own row; tier + thinking on the next
        assert!(text.contains("gpt-5.4"), "got {text}");
        assert!(text.contains("version"), "got {text}");
        assert!(
            text.contains("Victory · High") || text.contains("victory · high"),
            "got {text}"
        );
        assert!(text.contains("Maniple→Squad 68% / ¤272k"), "got {text}");
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
        let text = render_left_panel_text(&data, 72, 8);

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
            model_tier: "victory".into(),
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                codex_active_limit: Some("codex".into()),
                codex_primary_pct: Some(0),
                codex_primary_reset_secs: Some(13_648),
                codex_secondary_reset_secs: Some(348_644),
                codex_credits_unlimited: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = render_left_panel_text(&data, 96, 10);

        assert!(text.contains("limit"), "got {text}");
        assert!(!text.contains("bucket GPT-5.3-Codex-Spark"), "got {text}");
        assert!(!text.contains("GPT-5.3-Codex-Spark"), "got {text}");
        assert!(text.contains("codex 0%"), "got {text}");
        assert!(text.contains("resets 3h47m"), "got {text}");
    }

    #[test]
    fn left_panel_renders_session_stats_when_present() {
        let data = FooterData {
            model_id: "openai-codex:gpt-5.4".into(),
            model_provider: "openai-codex".into(),
            provider_connected: true,
            is_oauth: true,
            thinking_level: "medium".into(),
            model_tier: "victory".into(),
            turn: 9,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            ..Default::default()
        };
        let text = render_left_panel_text(&data, 72, 10);

        assert!(text.contains("version"), "got {text}");
        assert!(text.contains("session"), "got {text}");
        assert!(text.contains("T9"), "got {text}");
        assert!(text.contains("¤12k/¤3k"), "got {text}");
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
            model_tier: "victory".into(),
            turn: 9,
            session_input_tokens: 12_000,
            session_output_tokens: 3_000,
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai".into(),
                source: "response_headers".into(),
                codex_active_limit: Some("codex".into()),
                codex_primary_pct: Some(0),
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
                verbose.render_left_panel(frame.area(), frame, &super::super::theme::Alpharius)
            })
            .unwrap();
        terminal
            .draw(|frame| {
                compact.render_left_panel(frame.area(), frame, &super::super::theme::Alpharius)
            })
            .unwrap();

        let text = render_to_string(&terminal);
        assert!(text.contains("no provider"), "got {text}");
        assert!(!text.contains("stale row sentinel"), "got {text}");
        assert!(!text.contains("codex 0%"), "got {text}");
        assert!(!text.contains("T9"), "got {text}");
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
            context_class: ContextClass::Maniple,
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
                codex_primary_pct: Some(12),
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
        assert!(!text.contains("burst 12%"), "got {text}");
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

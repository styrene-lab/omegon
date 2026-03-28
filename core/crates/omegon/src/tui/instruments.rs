//! Unified instrument panel — two-panel layout.
//!
//! Ported from the instrument_lab R&D prototype.
//!
//! LEFT: Inference state
//!   - Context bar (gradient fill, caps at 70%)
//!   - Thinking glitch overlay (CRT noise chars on bar surface)
//!   - Tree connector (│├└ linking context to memory)
//!   - Memory sine strings (one per mind, plucked on store/recall)
//!
//! RIGHT: Tool activity
//!   - Bubble-sort list sorted by recency
//!   - Tool names, recency bars, time since last call
//!
//! All use unified navy→teal→amber CIE L* perceptual color ramp.

use super::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

fn panel_bg(t: &dyn Theme) -> Color {
    t.footer_bg()
}

/// Scale an RGB color's brightness.
fn dim_color(c: Color, factor: f64) -> Color {
    if let Color::Rgb(r, g, b) = c {
        Color::Rgb(
            (r as f64 * factor) as u8,
            (g as f64 * factor) as u8,
            (b as f64 * factor) as u8,
        )
    } else {
        c
    }
}

// ─── Color ramp (CIE L* perceptual) ────────────────────────────────────

fn intensity_color(intensity: f64) -> Color {
    if intensity < 0.10 {
        return Color::Rgb(24, 56, 72);
    }
    if intensity < 0.60 {
        return Color::Rgb(42, 180, 200);
    }
    if intensity < 0.85 {
        return Color::Rgb(220, 170, 70);
    }
    Color::Rgb(240, 110, 90)
}

/// Compact glyph+label for the instrument panel. Keeps tool rows readable
/// even in narrow terminals. Format: "⌘ label" — 2-char glyph prefix + short name.
fn tool_short_name(name: &str) -> String {
    let (glyph, label) = match name {
        // ── Core file ops ──
        "bash" => ("⌘", "sh"),
        "read" | "Read" => ("◇", "read"),
        "write" | "Write" => ("◆", "write"),
        "edit" | "Edit" => ("✎", "edit"),
        "view" => ("◈", "view"),
        // ── Git / speculate ──
        "commit" => ("⊕", "commit"),
        "speculate_start" => ("⊘", "spec∘"),
        "speculate_check" => ("⊘", "spec?"),
        "speculate_commit" => ("⊘", "spec✓"),
        "speculate_rollback" => ("⊘", "spec✗"),
        // ── Memory ──
        "memory_store" => ("▪", "mem+"),
        "memory_recall" => ("▫", "recall"),
        "memory_query" => ("▫", "memq"),
        "memory_archive" => ("▪", "mem⌫"),
        "memory_supersede" => ("▪", "mem↻"),
        "memory_connect" => ("▪", "mem⊷"),
        "memory_focus" => ("▪", "focus"),
        "memory_release" => ("▪", "unfoc"),
        "memory_episodes" => ("▫", "epis"),
        "memory_compact" => ("▪", "compct"),
        "memory_search_archive" => ("▫", "marcv"),
        "memory_ingest_lifecycle" => ("▪", "mingt"),
        // ── Design + lifecycle ──
        "design_tree" => ("△", "d.tree"),
        "design_tree_update" => ("▲", "d.tree↑"),
        "openspec_manage" => ("◎", "opsx"),
        // ── Cleave / decomposition ──
        "cleave_assess" => ("⟁", "assess"),
        "cleave_run" => ("⟁", "cleave"),
        "delegate" => ("⇉", "deleg"),
        "delegate_result" => ("⇉", "d.res"),
        "delegate_status" => ("⇉", "d.stat"),
        // ── Web / render ──
        "web_search" => ("⊕", "search"),
        "render_diagram" => ("⬡", "diag"),
        "generate_image_local" => ("⬡", "img"),
        // ── Local inference ──
        "ask_local_model" => ("⊛", "local"),
        "list_local_models" => ("⊛", "l.list"),
        "manage_ollama" => ("⊛", "ollama"),
        // ── Settings / meta ──
        "set_model_tier" => ("⚙", "tier"),
        "set_thinking_level" => ("⚙", "think"),
        "switch_to_offline_driver" => ("⚙", "offln"),
        "manage_tools" => ("⚙", "tools"),
        "whoami" => ("⚙", "whoami"),
        "chronos" => ("⚙", "chrono"),
        "change" => ("⚙", "change"),
        // ── Auth / persona ──
        "auth_status" => ("⚿", "auth"),
        "harness_settings" => ("⚿", "hrnss"),
        "switch_persona" => ("⚿", "persna"),
        "switch_tone" => ("⚿", "tone"),
        "list_personas" => ("⚿", "pers?"),
        // ── Fallback: truncate ──
        other => return other.to_string(),
    };
    format!("{glyph} {label}")
}

const NOISE_CHARS: &[char] = &[
    '▏', '▎', '▍', '░', '▌', '▐', '▒', '┤', '├', '│', '─', '▊', '▋', '▓', '╱', '╲', '┼', '╪', '╫',
    '█', '╬', '■', '◆',
];

// ─── Wave direction ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum WaveDirection {
    Left,   // recall: wave travels ← (mind → inference)
    Right,  // store: wave travels → (inference → mind)
    Center, // supersede: center-out symmetric twang
}

// ─── Mind state (sine string) ───────────────────────────────────────────

struct MindState {
    name: String,
    active: bool,
    fact_count: usize,
    wave: Vec<f64>,
    velocity: Vec<f64>,
    damping: f64,
}

impl MindState {
    fn new(name: &str, active: bool) -> Self {
        let w = 80;
        Self {
            name: name.into(),
            active,
            fact_count: 0,
            wave: vec![0.0; w],
            velocity: vec![0.0; w],
            damping: 0.92,
        }
    }

    fn pluck(&mut self, direction: WaveDirection) {
        let w = self.wave.len();
        match direction {
            WaveDirection::Right => {
                // Store: pulse at LEFT end, travels right →
                for i in 0..w {
                    let dx = i as f64 / 4.0;
                    self.velocity[i] += (-dx * dx / 2.0).exp() * 2.5;
                }
            }
            WaveDirection::Left => {
                // Recall: pulse at RIGHT end, travels left ←
                for i in 0..w {
                    let dx = (w - 1 - i) as f64 / 4.0;
                    self.velocity[i] -= (-dx * dx / 2.0).exp() * 2.5;
                }
            }
            WaveDirection::Center => {
                // Supersede: center-out symmetric twang ↔
                let center = w / 2;
                for i in 0..w {
                    let dx = (i as f64 - center as f64) / 3.0;
                    let pulse = (-dx * dx / 2.0).exp() * 3.0;
                    self.velocity[i] += if i < center { pulse } else { -pulse };
                }
            }
        }
    }

    fn update(&mut self) {
        let w = self.wave.len();
        if w < 3 {
            return;
        }
        let c2 = 0.3;
        let mut accel = vec![0.0; w];
        for i in 1..w - 1 {
            accel[i] = c2 * (self.wave[i - 1] + self.wave[i + 1] - 2.0 * self.wave[i]);
        }
        for i in 0..w {
            self.velocity[i] = (self.velocity[i] + accel[i]) * self.damping;
            self.wave[i] = (self.wave[i] + self.velocity[i]) * 0.999; // slight position damping too
        }
        self.wave[0] = 0.0;
        self.wave[w - 1] = 0.0;
        self.velocity[0] = 0.0;
        self.velocity[w - 1] = 0.0;
    }

    fn max_amplitude(&self) -> f64 {
        self.wave.iter().map(|v| v.abs()).fold(0.0_f64, f64::max)
    }
}

// ─── Tool entry ─────────────────────────────────────────────────────────

struct ToolEntry {
    name: String,
    last_called: f64,
    is_error: bool,
    error_ttl: f64,
}

// ─── Panel ──────────────────────────────────────────────────────────────

pub struct InstrumentPanel {
    time: f64,
    context_fill: f64,
    /// Fraction of context window used by injected memory facts.
    memory_fill: f64,
    /// Static thinking-level fill (0–1) from the setting — not animated.
    thinking_level_pct: f64,
    thinking_active: bool,
    thinking_intensity: f64,
    external_wait: f64,
    minds: Vec<MindState>,
    tools: Vec<ToolEntry>,
    pub focus_mode: bool,
    /// True after the first tool call — panel borders brighten on first fire.
    has_ever_fired: bool,
}

impl Default for InstrumentPanel {
    fn default() -> Self {
        Self {
            time: 0.0,
            context_fill: 0.0,
            memory_fill: 0.0,
            thinking_level_pct: 0.0,
            thinking_active: false,
            thinking_intensity: 0.0,
            external_wait: 0.0,
            minds: vec![
                MindState::new("project", true),
                MindState::new("working", false),
                MindState::new("episodes", false),
                MindState::new("archive", false),
            ],
            tools: Vec::new(),
            focus_mode: false,
            has_ever_fired: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivityMode {
    Idle,
    Thinking,
    ToolChurn,
    Waiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextBand {
    Conversation,
    System,
    Memory,
    Tools,
    Thinking,
    Free,
}

impl InstrumentPanel {
    pub fn preferred_height(&self) -> u16 {
        let active_minds = self.minds.iter().filter(|m| m.active).count().max(1) as u16;
        let tool_rows = self.tools.len().clamp(1, 6) as u16;
        let inference_height = 4u16 + active_minds.min(3);
        let tools_height = 3u16 + tool_rows;
        inference_height.max(tools_height).clamp(10, 16)
    }

    pub fn render_inference_panel(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        if area.width < 20 || area.height < 4 {
            return;
        }
        let (border, label) = if self.has_ever_fired {
            (t.border_dim(), t.dim())
        } else {
            (dim_color(t.border_dim(), 0.5), dim_color(t.dim(), 0.55))
        };
        self.render_inference(area, frame, border, label, t);
    }

    pub fn render_tools_panel(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        if area.width < 20 || area.height < 4 {
            return;
        }
        let (border, label) = if self.has_ever_fired {
            (t.border_dim(), t.dim())
        } else {
            (dim_color(t.border_dim(), 0.5), dim_color(t.dim(), 0.55))
        };
        self.render_tools(area, frame, border, label, t);
    }

    pub fn render(&mut self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        if area.width < 20 || area.height < 4 {
            return;
        }

        let panels = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);

        self.render_inference_panel(panels[0], frame, t);
        self.render_tools_panel(panels[1], frame, t);
    }

    fn context_breakdown(&self) -> [(ContextBand, f64); 6] {
        let total_used = self.context_fill.clamp(0.0, 1.0);
        let system = 0.08_f64.min(total_used);
        let memory = self.memory_fill.min((total_used - system).max(0.0));
        let thinking = (self.thinking_level_pct * 0.12).min((total_used - system - memory).max(0.0));
        let recent_tool_load = ((self.active_tool_load() * 0.10).min(0.10))
            .min((total_used - system - memory - thinking).max(0.0));
        let conversation =
            (total_used - system - memory - thinking - recent_tool_load).max(0.0);
        let free = (1.0 - total_used).max(0.0);
        [
            (ContextBand::Conversation, conversation),
            (ContextBand::System, system),
            (ContextBand::Memory, memory),
            (ContextBand::Tools, recent_tool_load),
            (ContextBand::Thinking, thinking),
            (ContextBand::Free, free),
        ]
    }

    fn active_tool_load(&self) -> f64 {
        self.tools
            .iter()
            .map(|tool| (1.0 - ((self.time - tool.last_called).max(0.0) / 4.0)).clamp(0.0, 1.0))
            .fold(0.0_f64, f64::max)
    }

    fn activity_mode(&self) -> ActivityMode {
        let tool_load = self.active_tool_load();
        if self.external_wait > 0.05 {
            ActivityMode::Waiting
        } else if self.thinking_active && self.thinking_level_pct > 0.0 {
            ActivityMode::Thinking
        } else if tool_load > 0.05 {
            ActivityMode::ToolChurn
        } else {
            ActivityMode::Idle
        }
    }

    fn band_color(band: ContextBand) -> Color {
        match band {
            ContextBand::Conversation => Color::Rgb(70, 126, 160),
            ContextBand::System => Color::Rgb(104, 96, 148),
            ContextBand::Memory => Color::Rgb(58, 176, 156),
            ContextBand::Tools => Color::Rgb(214, 156, 74),
            ContextBand::Thinking => Color::Rgb(132, 110, 212),
            ContextBand::Free => Color::Rgb(16, 24, 34),
        }
    }

    /// Update mind fact counts and memory context fraction.
    pub fn update_mind_facts(
        &mut self,
        total_facts: usize,
        working_memory: usize,
        episodes: usize,
        memory_fill: f64,
    ) {
        if !self.minds.is_empty() {
            self.minds[0].fact_count = total_facts;
            self.minds[0].active = true;
        }
        if self.minds.len() > 1 {
            self.minds[1].fact_count = working_memory;
            self.minds[1].active = working_memory > 0;
        }
        if self.minds.len() > 2 {
            self.minds[2].fact_count = episodes;
            self.minds[2].active = episodes > 0;
        }
        self.memory_fill = memory_fill.clamp(0.0, 0.12);
    }

    /// Update telemetry from harness state.
    pub fn update_telemetry(
        &mut self,
        context_pct: f32,
        tool_name: Option<&str>,
        tool_error: bool,
        thinking_level: &str,
        memory_op: Option<(usize, WaveDirection)>,
        agent_active: bool,
        dt: f64,
    ) {
        self.time += dt;

        // Context: true 0–100% fill, clamped.
        self.context_fill = (context_pct as f64 / 100.0).clamp(0.0, 1.0);

        // Thinking static fill — reflects the setting level, not animated intensity
        self.thinking_level_pct = match thinking_level {
            "high" => 1.0,
            "medium" => 0.60,
            "low" => 0.35,
            "minimal" => 0.15,
            _ => 0.0,
        };

        // Thinking: active only during inference when a thinking budget is configured.
        self.thinking_active = agent_active && self.thinking_level_pct > 0.0;
        let target = if self.thinking_active {
            match thinking_level {
                "high" => 0.85,
                "medium" => 0.6,
                "low" => 0.35,
                "minimal" => 0.15,
                _ => 0.1,
            }
        } else {
            0.0
        };
        self.thinking_intensity += (target - self.thinking_intensity) * dt * 3.0;

        self.external_wait = if agent_active && !self.thinking_active {
            (self.external_wait + dt * 1.8).clamp(0.0, 1.0)
        } else {
            (self.external_wait - dt * 1.2).clamp(0.0, 1.0)
        };

        // Tool: register call
        if tool_name.is_some() {
            self.has_ever_fired = true;
        }
        if let Some(name) = tool_name {
            if let Some(entry) = self.tools.iter_mut().find(|t| t.name == name) {
                entry.last_called = self.time;
                if tool_error {
                    entry.is_error = true;
                    entry.error_ttl = 5.0;
                }
            } else {
                self.tools.push(ToolEntry {
                    name: name.to_string(),
                    last_called: self.time,
                    is_error: tool_error,
                    error_ttl: if tool_error { 5.0 } else { 0.0 },
                });
            }
        }
        // Decay tool error TTLs
        for tool in &mut self.tools {
            if tool.is_error {
                tool.error_ttl -= dt;
                if tool.error_ttl <= 0.0 {
                    tool.is_error = false;
                }
            }
        }

        // Memory: pluck the string
        if let Some((mind_idx, direction)) = memory_op {
            if mind_idx < self.minds.len() {
                if !self.minds[mind_idx].active {
                    self.minds[mind_idx].active = true;
                    self.minds[mind_idx].wave = vec![0.0; 80];
                    self.minds[mind_idx].velocity = vec![0.0; 80];
                }
                self.minds[mind_idx].pluck(direction);
            }
        }

        // Update wave physics
        for mind in &mut self.minds {
            if mind.active {
                mind.update();
            }
        }
    }

    pub fn set_tool_error(&mut self, name: &str) {
        if let Some(entry) = self.tools.iter_mut().find(|t| t.name == name) {
            entry.is_error = true;
            entry.error_ttl = 5.0;
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus_mode = !self.focus_mode;
    }

    fn render_inference(&self, area: Rect, frame: &mut Frame, border: Color, label: Color, t: &dyn Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border).bg(t.footer_bg()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(
                " inference ",
                Style::default().fg(label).bg(t.footer_bg()),
            ))
            .style(Style::default().bg(t.footer_bg()));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 10 || inner.height < 3 {
            return;
        }

        let buf = frame.buffer_mut();
        let active_minds: Vec<usize> = self
            .minds
            .iter()
            .enumerate()
            .filter(|(_, m)| m.active)
            .map(|(i, _)| i)
            .collect();

        // Context bar: top 2 rows
        let bar_h = 2u16.min(inner.height);
        let bar_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: bar_h,
        };
        self.render_context_bar(bar_area, buf, t);

        // Tree + memory strings: break through the left border
        if inner.height > bar_h && !active_minds.is_empty() {
            // Start at the panel's left BORDER (area.x, not inner.x)
            // so the tree trunk overlays the border character
            let tree_area = Rect {
                x: area.x,
                y: inner.y + bar_h,
                width: inner.width + 1, // include border column
                height: inner.height - bar_h,
            };
            self.render_memory_strings(&active_minds, tree_area, buf, t);
        }
    }

    fn render_context_bar(&self, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
        let w = area.width as usize;
        if w == 0 {
            return;
        }

        let breakdown = self.context_breakdown();
        let activity = self.activity_mode();
        let time = self.time;

        let mut spans: Vec<(ContextBand, usize, usize)> = Vec::new();
        let mut cursor = 0usize;
        for (idx, (band, frac)) in breakdown.iter().enumerate() {
            let mut width = if idx == breakdown.len() - 1 {
                w.saturating_sub(cursor)
            } else {
                ((*frac * w as f64).round() as usize).min(w.saturating_sub(cursor))
            };
            if *frac > 0.0 && width == 0 && cursor < w {
                width = 1;
            }
            let end = (cursor + width).min(w);
            spans.push((*band, cursor, end));
            cursor = end;
        }
        if let Some((_, _, end)) = spans.last_mut() {
            *end = w;
        }

        for x in 0..w {
            let (band, start, end) = spans
                .iter()
                .copied()
                .find(|(_, start, end)| x >= *start && x < *end)
                .unwrap_or((ContextBand::Free, x, x + 1));
            let color = Self::band_color(band);
            let rel = if end > start {
                (x - start) as f64 / (end - start).max(1) as f64
            } else {
                0.0
            };
            let center_rel = ((rel - 0.5).abs() * 2.0).min(1.0);

            let (mut top_ch, mut bottom_ch, mut fg) = match band {
                ContextBand::Conversation => (' ', '█', color),
                ContextBand::System => (' ', '■', color),
                ContextBand::Memory => (' ', '▓', color),
                ContextBand::Tools => (' ', '▆', color),
                ContextBand::Thinking => (' ', '▒', color),
                ContextBand::Free => ('·', '·', color),
            };

            match activity {
                ActivityMode::Thinking if band == ContextBand::Thinking => {
                    let phase = (time * 3.0) + (1.0 - center_rel) * 1.8;
                    let pulse = ((phase.sin() + 1.0) * 0.5 * self.thinking_intensity.max(0.15))
                        .clamp(0.0, 1.0);
                    let glyphs = ['░', '▒', '▓', '█'];
                    let idx = (pulse * (glyphs.len() as f64 - 1.0)).round() as usize;
                    bottom_ch = glyphs[idx.min(glyphs.len() - 1)];
                    top_ch = if pulse > 0.72 && center_rel < 0.72 { '▄' } else { ' ' };
                    fg = if pulse > 0.72 {
                        Color::Rgb(198, 178, 255)
                    } else {
                        color
                    };
                }
                ActivityMode::ToolChurn if band == ContextBand::Tools => {
                    let pulse = (((time * 10.0) + x as f64 * 0.9).sin() + 1.0) * 0.5;
                    bottom_ch = if pulse > 0.75 { '█' } else if pulse > 0.4 { '▆' } else { '▄' };
                    top_ch = if pulse > 0.8 { '▂' } else { ' ' };
                    fg = if pulse > 0.75 { Color::Rgb(255, 196, 96) } else { color };
                }
                ActivityMode::Waiting if band == ContextBand::Tools => {
                    let pulse = (((time * 2.2) + x as f64 * 0.1).sin() + 1.0) * 0.5;
                    bottom_ch = if pulse > 0.6 { '▅' } else { '▃' };
                    fg = if pulse > 0.6 { Color::Rgb(232, 186, 104) } else { color };
                }
                ActivityMode::Idle if band == ContextBand::Free => {
                    top_ch = '·';
                    bottom_ch = '·';
                }
                _ => {}
            }

            let divider = spans.iter().any(|(_, _, end)| *end == x && x < w.saturating_sub(1));
            for row in 0..area.height.min(2) {
                let (mut ch, mut row_fg) = if row == 0 { (top_ch, fg) } else { (bottom_ch, fg) };
                if divider {
                    ch = if row == 0 { '╷' } else { '│' };
                    row_fg = Color::Rgb(34, 54, 72);
                }
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + row)) {
                    cell.set_char(ch);
                    cell.set_fg(row_fg);
                    cell.set_bg(panel_bg(t));
                }
            }
        }
    }

    fn render_memory_strings(&self, active_minds: &[usize], area: Rect, buf: &mut Buffer, t: &dyn Theme) {
        let w = area.width as usize;
        let n = active_minds.len();

        for (row_idx, &mind_idx) in active_minds.iter().enumerate() {
            let y = area.y + row_idx as u16;
            if y >= area.bottom() {
                break;
            }
            let mind = &self.minds[mind_idx];
            let is_last = row_idx == n - 1;

            // Tree connector
            let connector = if is_last { "└─" } else { "├─" };
            for (i, ch) in connector.chars().enumerate() {
                if let Some(cell) = buf.cell_mut(Position::new(area.x + i as u16, y)) {
                    cell.set_char(ch);
                    cell.set_fg(Color::Rgb(32, 72, 96));
                    cell.set_bg(panel_bg(t));
                }
            }
            // Vertical trunk on earlier rows
            for prev in 0..row_idx {
                let py = area.y + prev as u16;
                if let Some(cell) = buf.cell_mut(Position::new(area.x, py)) {
                    if cell.symbol() != "├" && cell.symbol() != "└" {
                        cell.set_char('│');
                        cell.set_fg(Color::Rgb(32, 72, 96));
                    }
                }
            }

            // Mind name + fact count
            let name_start = 3usize;
            let name_color = if mind.max_amplitude() > 0.1 {
                Color::Rgb(42, 180, 200)
            } else {
                Color::Rgb(64, 88, 112)
            };
            let label = if mind.fact_count > 0 {
                format!("{} ⌗{}", mind.name, mind.fact_count)
            } else {
                mind.name.clone()
            };
            for (i, ch) in label.chars().enumerate() {
                let x = name_start + i;
                if x >= w {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, y)) {
                    cell.set_char(ch);
                    cell.set_fg(name_color);
                    cell.set_bg(panel_bg(t));
                }
            }

            // Sine wave — braille dots for sub-character resolution
            // Each braille cell: 2 dots wide × 4 dots tall
            // Wave displacement maps to vertical dot position
            let wave_start = (name_start + label.len() + 1).min(w / 3);
            let wave_w = w.saturating_sub(wave_start);
            let wave_len = mind.wave.len();
            let idle_phase = self.time;
            for wx in 0..wave_w {
                let x = wave_start + wx;
                if x >= w {
                    break;
                }

                // Sample two adjacent wave points (one per braille column)
                let pos0 = (wx as f64 * 2.0 / (wave_w as f64 * 2.0)) * wave_len as f64;
                let pos1 = ((wx as f64 * 2.0 + 1.0) / (wave_w as f64 * 2.0)) * wave_len as f64;
                let d0 = mind.wave[(pos0 as usize).min(wave_len.saturating_sub(1))];
                let d1 = mind.wave[(pos1 as usize).min(wave_len.saturating_sub(1))];

                // Map displacement to braille row (0=top, 3=bottom)
                let row0 = (1.5 - d0 * 0.8).clamp(0.0, 3.0) as u8;
                let row1 = (1.5 - d1 * 0.8).clamp(0.0, 3.0) as u8;

                // Braille dot bits: col0=[0x01,0x02,0x04,0x40] col1=[0x08,0x10,0x20,0x80]
                let bit0 = match row0 {
                    0 => 0x01,
                    1 => 0x02,
                    2 => 0x04,
                    _ => 0x40,
                };
                let bit1 = match row1 {
                    0 => 0x08,
                    1 => 0x10,
                    2 => 0x20,
                    _ => 0x80,
                };

                let amp = d0.abs().max(d1.abs());
                let dots = if amp < 0.02 {
                    0x04 | 0x20 // flat middle line when idle
                } else {
                    bit0 | bit1
                };

                let ch = char::from_u32(0x2800 + dots as u32).unwrap_or('·');
                let intensity = (amp * 0.5).min(1.0);
                let color = if intensity > 0.01 {
                    intensity_color(intensity)
                } else {
                    let phase = ((idle_phase * 2.0) + wx as f64 * 0.08).sin() * 0.5 + 0.5;
                    let base = (20.0 + phase * 10.0) as u8;
                    Color::Rgb(base, base.saturating_add(18), base.saturating_add(28))
                };
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, y)) {
                    cell.set_char(ch);
                    cell.set_fg(color);
                    cell.set_bg(panel_bg(t));
                }
            }
        }
    }

    fn render_tools(&self, area: Rect, frame: &mut Frame, border: Color, label: Color, t: &dyn Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border).bg(t.footer_bg()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(
                " tools ",
                Style::default().fg(label).bg(t.footer_bg()),
            ))
            .style(Style::default().bg(t.footer_bg()));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 15 || inner.height < 2 {
            return;
        }

        let buf = frame.buffer_mut();
        let w = inner.width as usize;
        let name_w = 15.min(w / 2);
        let bar_w = w.saturating_sub(name_w + 6).max(2);

        // Sort by recency
        let mut sorted: Vec<&ToolEntry> = self.tools.iter().collect();
        sorted.sort_by(|a, b| {
            b.last_called
                .partial_cmp(&a.last_called)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for (row, tool) in sorted.iter().enumerate() {
            let y = inner.y + row as u16;
            if y >= inner.bottom().saturating_sub(1) {
                break;
            } // leave room for footer

            let age = (self.time - tool.last_called).max(0.0);
            let recency = if age > 120.0 {
                0.0
            } else {
                (1.0 - age / 120.0).max(0.0)
            };

            let indicator = if age < 2.0 { "▸ " } else { "  " };
            let ind_color = if tool.is_error {
                Color::Rgb(224, 72, 72)
            } else if age < 2.0 {
                Color::Rgb(42, 180, 200)
            } else {
                Color::Rgb(20, 40, 55)
            };
            // Tool colors: dim teal → bright teal/cyan (alpharius palette)
            let tool_color = |r: f64| -> Color {
                if r < 0.01 {
                    return Color::Rgb(12, 24, 32);
                }
                let r = r.clamp(0.0, 1.0);
                // Dark teal at low recency, bright alpharius teal at high
                // Matches primary (#2ab4c8) at full intensity
                Color::Rgb(
                    (12.0 + r * 30.0) as u8,  // 12 → 42
                    (24.0 + r * 156.0) as u8, // 24 → 180
                    (32.0 + r * 168.0) as u8, // 32 → 200
                )
            };
            let name_color = if tool.is_error {
                Color::Rgb(224, 72, 72)
            } else if recency > 0.1 {
                tool_color(recency)
            } else {
                Color::Rgb(48, 64, 80)
            };
            let bar_filled = (recency * bar_w as f64) as usize;
            let bar_color = if tool.is_error {
                Color::Rgb(224, 72, 72)
            } else {
                tool_color(recency)
            };

            let time_str = if age > 999.0 {
                "   ·".to_string()
            } else if age > 60.0 {
                format!("{:>3.0}m", age / 60.0)
            } else {
                format!("{:>3.0}s", age)
            };

            let mut x = inner.x;
            for ch in indicator.chars() {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch);
                    cell.set_fg(ind_color);
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }
            let short = tool_short_name(&tool.name);
            let display_name = if short.len() > name_w - 2 {
                &short[..name_w - 2]
            } else {
                short.as_str()
            };
            for ch in display_name.chars() {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch);
                    cell.set_fg(name_color);
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }
            while x < inner.x + 2 + name_w as u16 {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(' ');
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }
            // Bar character degrades with recency — three visual channels:
            // fill length (how much bar), color (teal intensity), character (signal density)
            let bar_char = if recency > 0.7 {
                '≋'
            }
            // strong — just fired
            else if recency > 0.3 {
                '≈'
            }
            // recent — decaying
            else if recency > 0.05 {
                '∿'
            }
            // fading echo
            else {
                '·'
            }; // nearly silent
            for i in 0..bar_w {
                if x >= inner.right() {
                    break;
                }
                let ch = if i < bar_filled { bar_char } else { '·' };
                let c = if i < bar_filled {
                    bar_color
                } else {
                    Color::Rgb(16, 28, 36)
                };
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch);
                    cell.set_fg(c);
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }
            for ch in time_str.chars() {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch);
                    cell.set_fg(Color::Rgb(48, 64, 80));
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }
        }

        // Footer
        let footer_y = inner.bottom().saturating_sub(1);
        if footer_y > inner.y + sorted.len() as u16 {
            let active = self
                .tools
                .iter()
                .filter(|t| self.time - t.last_called < 120.0)
                .count();
            let total = self.tools.len();
            let footer = format!("  {active}/{total} active");
            for (i, ch) in footer.chars().enumerate() {
                let x = inner.x + i as u16;
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, footer_y)) {
                    cell.set_char(ch);
                    cell.set_fg(Color::Rgb(48, 64, 80));
                    cell.set_bg(panel_bg(t));
                }
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intensity_color_floor_is_dim_teal() {
        assert!(matches!(intensity_color(0.0), Color::Rgb(24, 56, 72)));
    }

    #[test]
    fn context_fill_uses_full_percent_range() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(50.0, None, false, "off", None, false, 0.016);
        assert!(
            (panel.context_fill - 0.5).abs() < 0.001,
            "context fill should track 50%"
        );
        panel.update_telemetry(100.0, None, false, "off", None, false, 0.016);
        assert!(
            (panel.context_fill - 1.0).abs() < 0.001,
            "context fill should track 100%"
        );
    }

    #[test]
    fn memory_fill_is_visually_capped() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(10_000, 0, 0, 0.9);
        assert!(
            panel.memory_fill <= 0.12,
            "memory fill should be capped conservatively"
        );
    }

    #[test]
    fn panel_renders_without_panic() {
        let mut panel = InstrumentPanel::default();
        let area = Rect::new(0, 0, 96, 12);
        let backend = ratatui::backend::TestBackend::new(96, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;
        terminal.draw(|f| panel.render(area, f, &t)).unwrap();
    }

    #[test]
    fn preferred_height_grows_with_active_minds_and_tools() {
        let mut panel = InstrumentPanel::default();
        let base = panel.preferred_height();
        panel.update_mind_facts(18, 3, 2, 0.08);
        panel.update_telemetry(62.0, Some("read"), false, "medium", None, true, 0.016);
        panel.update_telemetry(62.0, Some("bash"), false, "medium", None, true, 0.016);
        let grown = panel.preferred_height();
        assert!(grown >= base, "footer height should not shrink after activity");
        assert!(grown >= 10 && grown <= 16, "preferred height stays bounded: {grown}");
    }

    #[test]
    fn wave_physics_dampens() {
        let mut mind = MindState::new("test", true);
        mind.pluck(WaveDirection::Right);
        // Let wave build up from velocity
        for _ in 0..20 {
            mind.update();
        }
        let peak = mind.max_amplitude();
        assert!(
            peak > 0.01,
            "wave should have amplitude after pluck: {peak:.3}"
        );
        // Let it dampen
        for _ in 0..500 {
            mind.update();
        }
        let final_amp = mind.max_amplitude();
        assert!(
            final_amp < peak * 0.5,
            "wave should dampen: peak={peak:.3} final={final_amp:.3}"
        );
    }

    #[test]
    fn tool_registration() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(0.0, Some("bash"), false, "off", None, false, 0.016);
        assert_eq!(panel.tools.len(), 1);
        assert_eq!(panel.tools[0].name, "bash");
    }

    #[test]
    fn update_mind_facts_populates_project_working_and_episodes() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(18, 3, 2, 0.25);
        assert_eq!(panel.minds[0].fact_count, 18);
        assert_eq!(panel.minds[1].fact_count, 3);
        assert_eq!(panel.minds[2].fact_count, 2);
        assert!(panel.minds[2].active, "episodes mind should activate when populated");
        assert!(panel.memory_fill <= 0.12, "memory fill stays conservatively capped");
    }

    #[test]
    fn context_breakdown_stays_normalized_and_ordered() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(18, 3, 2, 0.08);
        panel.update_telemetry(62.0, Some("read"), false, "medium", None, true, 0.016);
        let breakdown = panel.context_breakdown();
        let total: f64 = breakdown.iter().map(|(_, frac)| frac).sum();
        assert!((total - 1.0).abs() < 0.0001, "breakdown should sum to 1.0, got {total}");
        assert_eq!(breakdown[0].0, ContextBand::Conversation);
        assert_eq!(breakdown[1].0, ContextBand::System);
        assert_eq!(breakdown[2].0, ContextBand::Memory);
        assert_eq!(breakdown[3].0, ContextBand::Tools);
        assert_eq!(breakdown[4].0, ContextBand::Thinking);
        assert_eq!(breakdown[5].0, ContextBand::Free);
    }

    #[test]
    fn thinking_activity_mode_beats_tool_churn() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(40.0, Some("bash"), false, "high", None, true, 0.016);
        assert_eq!(panel.activity_mode(), ActivityMode::Thinking);
    }

    #[test]
    fn waiting_activity_mode_appears_without_thinking_budget() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(40.0, None, false, "off", None, true, 0.5);
        assert_eq!(panel.activity_mode(), ActivityMode::Waiting);
    }
}

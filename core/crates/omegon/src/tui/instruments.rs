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
use super::widgets::{truncate_str, visible_width};
use crate::features::cleave::CleaveProgress;
use omegon_traits::ContextComposition;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};
use unicode_width::UnicodeWidthChar;

fn panel_bg(t: &dyn Theme) -> Color {
    t.footer_bg()
}

/// Write `text` left-to-right starting at `(x, y)`, clipped to `max_x`.
/// Each character advances by its CJK-aware cell width (wide chars consume 2 cells;
/// the second cell is blanked so subsequent characters land in the right column).
/// Returns the x position after the last written character.
fn render_str_colored<F>(
    text: &str,
    x: u16,
    y: u16,
    max_x: u16,
    bg: Color,
    buf: &mut Buffer,
    color_for: F,
) -> u16
where
    F: Fn(char) -> Color,
{
    let mut cur_x = x;
    for ch in text.chars() {
        if cur_x >= max_x {
            break;
        }
        let w = UnicodeWidthChar::width_cjk(ch).unwrap_or(1) as u16;
        if let Some(cell) = buf.cell_mut(Position::new(cur_x, y)) {
            cell.set_char(ch);
            cell.set_fg(color_for(ch));
            cell.set_bg(bg);
        }
        // Blank the overflow cell for wide characters so we don't draw into it.
        if w == 2 {
            if let Some(cell) = buf.cell_mut(Position::new(cur_x + 1, y)) {
                cell.set_char(' ');
                cell.set_fg(bg);
                cell.set_bg(bg);
            }
        }
        cur_x = cur_x.saturating_add(w);
    }
    cur_x
}

fn clear_area(area: Rect, buf: &mut Buffer, bg: Color) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char(' ');
                cell.set_fg(bg);
                cell.set_bg(bg);
            }
        }
    }
}

fn clear_row(y: u16, x0: u16, x1: u16, buf: &mut Buffer, bg: Color) {
    for x in x0..x1 {
        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
            cell.set_char(' ');
            cell.set_fg(bg);
            cell.set_bg(bg);
        }
    }
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
pub(crate) fn tool_short_name(name: &str) -> String {
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
        "codebase_search" => ("⌕", "cbase"),
        "codebase_index" => ("⌕", "cidx"),
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
    running: bool,
    started_at: Option<f64>,
    last_duration_ms: Option<u64>,
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
    // ── Per-turn token stats (from TurnEnd) ──
    last_input_tokens: u32,
    last_output_tokens: u32,
    last_cache_read_tokens: u32,
    // ── Cumulative session memory op counters ──
    session_stores: u32,
    session_recalls: u32,
    /// Actual context window size in tokens — used to compute realistic fractions.
    context_window: usize,
    /// Authoritative composition snapshot from the turn runner's accounting model.
    context_composition: ContextComposition,
    /// Live cleave progress snapshot — if active, tools panel becomes cleave panel.
    cleave_progress: Option<CleaveProgress>,
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
            last_input_tokens: 0,
            last_output_tokens: 0,
            last_cache_read_tokens: 0,
            session_stores: 0,
            session_recalls: 0,
            context_window: 200_000,
            context_composition: ContextComposition::default(),
            cleave_progress: None,
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

    fn context_legend_entries() -> [(&'static str, &'static str, Color); 5] {
        [
            ("=", "conv", Self::band_color(ContextBand::Conversation)),
            ("+", "sys", Self::band_color(ContextBand::System)),
            ("*", "mem", Self::band_color(ContextBand::Memory)),
            ("#", "tool", Self::band_color(ContextBand::Tools)),
            ("^", "think", Self::band_color(ContextBand::Thinking)),
        ]
    }

    fn band_glyph(band: ContextBand) -> char {
        match band {
            ContextBand::Conversation => '=',
            ContextBand::System => '+',
            ContextBand::Memory => '*',
            ContextBand::Tools => '#',
            ContextBand::Thinking => '^',
            ContextBand::Free => '·',
        }
    }

    fn render_context_legend_row(&self, y: u16, inner: Rect, buf: &mut Buffer, t: &dyn Theme) {
        clear_row(y, inner.x, inner.right(), buf, panel_bg(t));

        let separator = " ";
        let separator_width = visible_width(separator) as u16;
        let mut x = inner.x;

        for (idx, (icon, label_text, color)) in
            Self::context_legend_entries().into_iter().enumerate()
        {
            let entry = format!("{icon} {label_text}");
            let entry_width = visible_width(&entry) as u16;
            let gap = if idx == 0 { 0 } else { separator_width };

            if x.saturating_add(gap).saturating_add(entry_width) > inner.right() {
                break;
            }
            if gap > 0 {
                x = render_str_colored(separator, x, y, inner.right(), panel_bg(t), buf, |_| {
                    t.dim()
                });
            }

            let icon_chars: Vec<char> = icon.chars().collect();
            x = render_str_colored(&entry, x, y, inner.right(), panel_bg(t), buf, |ch| {
                if icon_chars.contains(&ch) {
                    color
                } else {
                    t.dim()
                }
            });
        }
    }

    fn activity_color(mode: ActivityMode, intensity: f64) -> Color {
        let intensity = intensity.clamp(0.0, 1.0);
        match mode {
            ActivityMode::Idle => Color::Rgb(52, 72, 88),
            ActivityMode::ToolChurn => Color::Rgb(
                (214.0 + 24.0 * intensity) as u8,
                (156.0 + 40.0 * intensity) as u8,
                (74.0 + 22.0 * intensity) as u8,
            ),
            ActivityMode::Waiting => Color::Rgb(
                (184.0 + 48.0 * intensity) as u8,
                (140.0 + 46.0 * intensity) as u8,
                (78.0 + 26.0 * intensity) as u8,
            ),
            ActivityMode::Thinking => Self::thinking_pulse_color(intensity.max(0.25)),
        }
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
        // When a cleave run is active, swap the tools panel to show the child grid.
        // Guard on active only — total_children persists after the run ends and
        // would keep the cleave panel showing forever.
        if let Some(ref cp) = self.cleave_progress {
            if cp.active {
                let border = t.border_dim();
                let label = t.dim();
                self.render_cleave_panel(area, frame, border, label, t, cp);
                return;
            }
        }
        let (border, label) = if self.has_ever_fired {
            (t.border_dim(), t.dim())
        } else {
            (dim_color(t.border_dim(), 0.5), dim_color(t.dim(), 0.55))
        };
        self.render_tools(area, frame, border, label, t);
    }

    /// Push an updated cleave progress snapshot from the orchestrator thread.
    pub fn set_cleave_progress(&mut self, cp: Option<CleaveProgress>) {
        self.cleave_progress = cp;
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
        let composition = &self.context_composition;
        let used_target = self.context_fill.clamp(0.0, 1.0);
        let free_target = (1.0 - used_target).clamp(0.0, 1.0);

        let conversation = composition.conversation_tokens as f64;
        let system = composition.system_tokens as f64;
        let memory = composition.memory_tokens as f64;
        let tools = composition.tool_tokens as f64;
        let thinking = composition.thinking_tokens as f64;
        let used_reported = conversation + system + memory + tools + thinking;
        let scale = if used_reported > 0.0 {
            used_target / used_reported
        } else {
            0.0
        };

        [
            (ContextBand::Conversation, conversation * scale),
            (ContextBand::System, system * scale),
            (ContextBand::Memory, memory * scale),
            (ContextBand::Tools, tools * scale),
            (ContextBand::Thinking, thinking * scale),
            (ContextBand::Free, free_target),
        ]
    }

    fn active_tool_load(&self) -> f64 {
        self.tools
            .iter()
            .map(|tool| {
                let recency =
                    (1.0 - ((self.time - tool.last_called).max(0.0) / 4.0)).clamp(0.0, 1.0);
                if tool.running { 1.0 } else { recency }
            })
            .fold(0.0_f64, f64::max)
    }

    fn format_duration_ms(duration_ms: u64) -> String {
        if duration_ms < 1_000 {
            format!("{:>4}ms", duration_ms)
        } else if duration_ms < 60_000 {
            format!("{:>4.1}s", duration_ms as f64 / 1_000.0)
        } else if duration_ms < 3_600_000 {
            let total_secs = duration_ms / 1_000;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            format!("{:>2}:{secs:02}", mins)
        } else {
            let total_mins = duration_ms / 60_000;
            let hours = total_mins / 60;
            let mins = total_mins % 60;
            format!("{:>2}h{mins:02}", hours)
        }
    }

    pub fn tool_started(&mut self, name: &str) {
        self.has_ever_fired = true;
        if let Some(entry) = self.tools.iter_mut().find(|t| t.name == name) {
            entry.last_called = self.time;
            entry.running = true;
            entry.started_at = Some(self.time);
            entry.is_error = false;
            entry.error_ttl = 0.0;
        } else {
            self.tools.push(ToolEntry {
                name: name.to_string(),
                last_called: self.time,
                is_error: false,
                error_ttl: 0.0,
                running: true,
                started_at: Some(self.time),
                last_duration_ms: None,
            });
        }
    }

    pub fn tool_finished(&mut self, name: &str, is_error: bool) {
        self.has_ever_fired = true;
        if let Some(entry) = self.tools.iter_mut().find(|t| t.name == name) {
            let started_at = entry.started_at.unwrap_or(entry.last_called);
            let duration_ms = ((self.time - started_at).max(0.0) * 1_000.0).round() as u64;
            entry.last_called = self.time;
            entry.running = false;
            entry.started_at = None;
            entry.last_duration_ms = Some(duration_ms);
            entry.is_error = is_error;
            entry.error_ttl = if is_error { 5.0 } else { 0.0 };
        } else {
            self.tools.push(ToolEntry {
                name: name.to_string(),
                last_called: self.time,
                is_error,
                error_ttl: if is_error { 5.0 } else { 0.0 },
                running: false,
                started_at: None,
                last_duration_ms: Some(0),
            });
        }
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
            ContextBand::Conversation => Color::Rgb(232, 236, 242),
            ContextBand::System => Color::Rgb(88, 182, 116),
            ContextBand::Memory => Color::Rgb(148, 108, 212),
            ContextBand::Tools => Color::Rgb(214, 156, 74),
            ContextBand::Thinking => Color::Rgb(70, 126, 214),
            ContextBand::Free => Color::Rgb(16, 24, 34),
        }
    }

    fn thinking_pulse_color(pulse: f64) -> Color {
        let pulse = pulse.clamp(0.0, 1.0);
        let base = (70.0, 126.0, 214.0);
        let peak = (148.0, 196.0, 255.0);
        Color::Rgb(
            (base.0 + (peak.0 - base.0) * pulse) as u8,
            (base.1 + (peak.1 - base.1) * pulse) as u8,
            (base.2 + (peak.2 - base.2) * pulse) as u8,
        )
    }

    /// Update mind fact counts and memory context fraction.
    /// Record token counts and composition from the provider/runtime TurnEnd event.
    pub fn update_turn_tokens(
        &mut self,
        input: u32,
        output: u32,
        cache_read: u32,
        composition: ContextComposition,
        context_window: usize,
    ) {
        self.last_input_tokens = input;
        self.last_output_tokens = output;
        self.last_cache_read_tokens = cache_read;
        self.context_composition = composition;
        self.context_window = context_window.max(1);
    }

    /// Increment cumulative memory operation counters.
    pub fn bump_memory_store(&mut self) {
        self.session_stores += 1;
    }

    pub fn bump_memory_recall(&mut self) {
        self.session_recalls += 1;
    }

    pub fn update_mind_facts(
        &mut self,
        project_facts: usize,
        working_memory: usize,
        episodes: usize,
        memory_fill: f64,
    ) {
        self.update_mind_slot(0, project_facts, true);
        self.update_mind_slot(1, working_memory, working_memory > 0);
        self.update_mind_slot(2, episodes, episodes > 0);
        self.memory_fill = memory_fill.clamp(0.0, 0.12);
    }

    fn update_mind_slot(&mut self, idx: usize, fact_count: usize, active: bool) {
        if idx >= self.minds.len() {
            return;
        }
        let mind = &mut self.minds[idx];
        let previous = mind.fact_count;
        mind.fact_count = fact_count;
        mind.active = active;
        if !mind.active {
            return;
        }
        if previous == 0 && fact_count > 0 {
            mind.wave = vec![0.0; 80];
            mind.velocity = vec![0.0; 80];
            mind.pluck(WaveDirection::Right);
        } else if fact_count > previous {
            mind.pluck(WaveDirection::Right);
        } else if fact_count < previous {
            mind.pluck(WaveDirection::Left);
        }
    }

    /// Update telemetry from harness state.
    pub fn update_telemetry(
        &mut self,
        context_pct: f32,
        context_window: usize,
        _tool_name: Option<&str>,
        _tool_error: bool,
        thinking_level: &str,
        memory_op: Option<(usize, WaveDirection)>,
        agent_active: bool,
        dt: f64,
    ) {
        self.time += dt;

        if context_window > 0 {
            self.context_window = context_window;
        }

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

        // Tool recency/error decay is time-based even without a new tool event.
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
            entry.running = false;
            if let Some(started_at) = entry.started_at.take() {
                entry.last_duration_ms =
                    Some(((self.time - started_at).max(0.0) * 1_000.0).round() as u64);
            }
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus_mode = !self.focus_mode;
    }

    fn render_inference(
        &self,
        area: Rect,
        frame: &mut Frame,
        border: Color,
        label: Color,
        t: &dyn Theme,
    ) {
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
        clear_area(inner, buf, panel_bg(t));
        let active_minds: Vec<usize> = self
            .minds
            .iter()
            .enumerate()
            .filter(|(_, m)| m.active)
            .map(|(i, _)| i)
            .collect();

        // Context composition row + activity row
        let bar_h = 2u16.min(inner.height);
        let bar_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: bar_h,
        };
        self.render_context_bar(bar_area, buf, t);

        if inner.height > bar_h {
            self.render_context_legend_row(inner.y + bar_h, inner, buf, t);
        }

        // Stats row: token counts + memory op tallies
        let stats_row_y = inner.y + bar_h + 1;
        if inner.height > bar_h + 1 {
            clear_row(stats_row_y, inner.x, inner.right(), buf, panel_bg(t));

            fn fmt_k(n: u32) -> String {
                if n >= 1000 {
                    format!("{:.1}k", n as f64 / 1000.0)
                } else {
                    format!("{n}")
                }
            }

            let cache_pct = if self.last_input_tokens > 0 {
                (self.last_cache_read_tokens as f64 / self.last_input_tokens as f64 * 100.0).round()
                    as u32
            } else {
                0
            };
            let token_str = if self.last_input_tokens > 0 {
                format!(
                    "↑ {}  ↓ {}  ⊙ {}%",
                    fmt_k(self.last_input_tokens),
                    fmt_k(self.last_output_tokens),
                    cache_pct,
                )
            } else {
                "↑ —  ↓ —  ⊙ —".to_string()
            };
            let mem_str = format!(
                "   ✦ {}  ◎ {} recalled",
                self.session_stores, self.session_recalls
            );
            let full = format!("{token_str}{mem_str}");

            let dim = t.dim();
            let accent = Color::Rgb(42, 180, 200);
            render_str_colored(
                &full,
                inner.x,
                stats_row_y,
                inner.right(),
                panel_bg(t),
                buf,
                |ch| match ch {
                    '↑' | '↓' | '⊙' | '✦' | '◎' => accent,
                    _ => dim,
                },
            );
        }

        // Tree + memory strings: break through the left border only.
        // Do not paint through the right boundary — in the split layout that would
        // bleed into the adjacent tools panel.
        if inner.height > bar_h + 2 && !active_minds.is_empty() {
            let tree_area = Rect {
                x: area.x,
                y: inner.y + bar_h + 2,
                width: inner.width,
                height: inner.height.saturating_sub(bar_h + 2),
            };
            self.render_memory_strings(&active_minds, tree_area, buf, t);
        }
    }

    fn render_context_bar(&self, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
        let w = area.width as usize;
        if w == 0 || area.height == 0 {
            return;
        }

        // ── Braille left-fill levels (left column top→bottom, then right column) ──
        const FILL: [char; 9] = [
            '\u{2800}', // ⠀ 0/8 empty
            '\u{2840}', // ⡀ 1/8
            '\u{2844}', // ⡄ 2/8
            '\u{2846}', // ⡆ 3/8
            '\u{2847}', // ⡇ 4/8 — left col full
            '\u{28C7}', // ⣇ 5/8
            '\u{28E7}', // ⣏ 6/8
            '\u{28F7}', // ⣟ 7/8
            '\u{28FF}', // ⣿ 8/8 full
        ];

        let breakdown = self.context_breakdown();
        let activity = self.activity_mode();
        let time = self.time;

        let mut boundaries: Vec<(ContextBand, f64, f64)> = Vec::new();
        let mut cursor = 0.0_f64;
        for &(band, frac) in &breakdown {
            let end = (cursor + frac * w as f64).min(w as f64);
            boundaries.push((band, cursor, end));
            cursor = end;
        }

        let composition_y = area.y;
        let activity_y = (area.y + 1).min(area.bottom().saturating_sub(1));

        for x in 0..w {
            let col_start = x as f64;
            let col_end = col_start + 1.0;

            let mut dominant = ContextBand::Free;
            let mut best_coverage = 0.0_f64;
            let mut fill_frac = 0.0_f64;

            for &(band, band_start, band_end) in &boundaries {
                let lo = col_start.max(band_start);
                let hi = col_end.min(band_end);
                if hi > lo {
                    let coverage = hi - lo;
                    if coverage > best_coverage {
                        best_coverage = coverage;
                        dominant = band;
                        fill_frac = coverage;
                    }
                }
            }

            let composition_ch = if dominant == ContextBand::Free || fill_frac <= 0.0 {
                Self::band_glyph(ContextBand::Free)
            } else {
                Self::band_glyph(dominant)
            };
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, composition_y)) {
                cell.set_char(composition_ch);
                cell.set_fg(Self::band_color(dominant));
                cell.set_bg(panel_bg(t));
            }

            if area.height < 2 {
                continue;
            }

            let activity_phase = match activity {
                ActivityMode::Idle => (((time * 1.4) + x as f64 * 0.11).sin() + 1.0) * 0.5,
                ActivityMode::ToolChurn => (((time * 9.0) + x as f64 * 0.7).sin() + 1.0) * 0.5,
                ActivityMode::Waiting => (((time * 2.2) + x as f64 * 0.18).sin() + 1.0) * 0.5,
                ActivityMode::Thinking => {
                    (((time * 3.0) + x as f64 * 0.35).sin() + 1.0)
                        * 0.5
                        * self.thinking_intensity.max(0.15)
                }
            }
            .clamp(0.0, 1.0);

            let (activity_ch, activity_fg) = match activity {
                ActivityMode::Idle => {
                    let c = if activity_phase > 0.55 { '·' } else { ' ' };
                    (c, Self::activity_color(ActivityMode::Idle, activity_phase))
                }
                ActivityMode::ToolChurn => {
                    let c = if activity_phase > 0.72 {
                        '•'
                    } else {
                        '·'
                    };
                    (
                        c,
                        Self::activity_color(ActivityMode::ToolChurn, activity_phase),
                    )
                }
                ActivityMode::Waiting => {
                    let c = if activity_phase > 0.66 { '•' } else { '·' };
                    (
                        c,
                        Self::activity_color(ActivityMode::Waiting, activity_phase),
                    )
                }
                ActivityMode::Thinking => {
                    let c = if activity_phase > 0.72 {
                        '•'
                    } else {
                        '·'
                    };
                    (
                        c,
                        Self::activity_color(ActivityMode::Thinking, activity_phase),
                    )
                }
            };

            if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, activity_y)) {
                cell.set_char(activity_ch);
                cell.set_fg(activity_fg);
                cell.set_bg(panel_bg(t));
            }
        }
    }

    fn render_memory_strings(
        &self,
        active_minds: &[usize],
        area: Rect,
        buf: &mut Buffer,
        t: &dyn Theme,
    ) {
        let w = area.width as usize;
        let n = active_minds.len();

        for (row_idx, &mind_idx) in active_minds.iter().enumerate() {
            let y = area.y + row_idx as u16;
            if y >= area.bottom() {
                break;
            }
            clear_row(y, area.x, area.right(), buf, panel_bg(t));
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
            let label = format!("{} ⌗{}", mind.name, mind.fact_count);
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
            let min_gap = 2usize;
            let min_wave_width = 6usize;
            let protected_label_end = name_start + label.chars().count() + min_gap;
            let max_wave_start = w.saturating_sub(min_wave_width);
            let wave_start = protected_label_end.min(max_wave_start);
            let wave_w = w.saturating_sub(wave_start);
            if wave_w == 0 {
                continue;
            }
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

    /// Cleave panel — displayed in place of the tools panel while a cleave run
    /// is active. Shows one row per child with: status icon, label, current
    /// tool/turn, and elapsed wall-clock time.
    fn render_cleave_panel(
        &self,
        area: Rect,
        frame: &mut Frame,
        border: Color,
        _label_color: Color,
        t: &dyn Theme,
        cp: &CleaveProgress,
    ) {
        // ── Title: " ⟁ cleave N/M " ─────────────────────────────────────
        let done = cp.completed + cp.failed;
        let title_text = if cp.active {
            format!(" ⟁ cleave {done}/{} ", cp.total_children)
        } else {
            format!(" ⟁ cleave {} done ", cp.total_children)
        };
        let title_color = if cp.failed > 0 {
            Color::Rgb(224, 72, 72)
        } else if cp.active {
            Color::Rgb(232, 186, 104) // amber — in-flight
        } else {
            Color::Rgb(42, 180, 200) // teal — complete
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border).bg(t.footer_bg()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(
                title_text,
                Style::default().fg(title_color).bg(t.footer_bg()),
            ))
            .style(Style::default().bg(t.footer_bg()));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 18 || inner.height < 2 {
            return;
        }

        let buf = frame.buffer_mut();
        clear_area(inner, buf, panel_bg(t));

        let w = inner.width as usize;
        // Layout: [ind 2] [label 11] [activity fill] [elapsed 5]
        let elapsed_w: usize = 5;
        let label_w: usize = 11.min(w.saturating_sub(elapsed_w + 4));
        let activity_w: usize = w.saturating_sub(2 + label_w + 1 + elapsed_w);

        // Leave the last row for an aggregate summary line.
        let child_rows = (inner.height as usize).saturating_sub(1);

        for (row, child) in cp.children.iter().enumerate() {
            if row >= child_rows {
                break;
            }
            let y = inner.y + row as u16;
            clear_row(y, inner.x, inner.right(), buf, panel_bg(t));

            // ── Status indicator ──
            let (ind_ch, ind_color) = match child.status.as_str() {
                "running" => ("▶ ", Color::Rgb(232, 186, 104)),
                "completed" => ("✓ ", Color::Rgb(42, 180, 200)),
                "failed" => ("✗ ", Color::Rgb(224, 72, 72)),
                "upstream_exhausted" => ("⚡ ", Color::Rgb(214, 170, 40)),
                _ => ("○ ", Color::Rgb(40, 56, 72)), // pending / unknown
            };
            let mut x = inner.x;
            x = render_str_colored(ind_ch, x, y, inner.right(), panel_bg(t), buf, |_| ind_color);

            // ── Label (padded to label_w) ──
            let label_color = match child.status.as_str() {
                "running" => Color::Rgb(232, 186, 104),
                "completed" => Color::Rgb(42, 180, 200),
                "failed" | "upstream_exhausted" => Color::Rgb(224, 72, 72),
                _ => Color::Rgb(48, 68, 84),
            };
            let display_label: String = child.label.chars().take(label_w).collect();
            x = render_str_colored(
                &display_label,
                x,
                y,
                inner.right(),
                panel_bg(t),
                buf,
                |_| label_color,
            );
            // Pad to label_w
            while x < inner.x + 2 + label_w as u16 {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(' ');
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }

            // ── Activity: tool or turn (dim) ──
            let activity = if let Some(ref tool) = child.last_tool {
                format!("→{tool}")
            } else if let Some(turn) = child.last_turn {
                format!("T{turn}")
            } else if child.status == "running" {
                "…".to_string()
            } else {
                String::new()
            };
            let act_color = Color::Rgb(36, 80, 96);
            let act_display: String = activity.chars().take(activity_w).collect();
            x = render_str_colored(&act_display, x, y, inner.right(), panel_bg(t), buf, |_| {
                act_color
            });
            // Pad to activity_w
            let act_end_x = inner.x + 2 + label_w as u16 + activity_w as u16;
            while x < act_end_x {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(' ');
                    cell.set_bg(panel_bg(t));
                }
                x += 1;
            }

            // ── Elapsed time ──
            let elapsed_secs = if let Some(s) = child.started_at {
                s.elapsed().as_secs_f64()
            } else {
                child.duration_secs.unwrap_or(0.0)
            };
            let elapsed_str = Self::format_elapsed(elapsed_secs);
            let elapsed_color = Color::Rgb(36, 60, 76);
            let _ = render_str_colored(&elapsed_str, x, y, inner.right(), panel_bg(t), buf, |_| {
                elapsed_color
            });
        }

        // ── Summary row ──────────────────────────────────────────────────
        let summary_y = inner.y + child_rows as u16;
        if summary_y < inner.bottom() {
            clear_row(summary_y, inner.x, inner.right(), buf, panel_bg(t));
            let summary = if cp.total_tokens_in > 0 || cp.total_tokens_out > 0 {
                format!(
                    "{}↓ {}↑",
                    crate::tui::widgets::format_tokens_compact(cp.total_tokens_in as usize),
                    crate::tui::widgets::format_tokens_compact(cp.total_tokens_out as usize),
                )
            } else {
                format!("{}/{} done", done, cp.total_children)
            };
            let summary_color = Color::Rgb(36, 60, 76);
            let _ = render_str_colored(
                &summary,
                inner.x,
                summary_y,
                inner.right(),
                panel_bg(t),
                buf,
                |_| summary_color,
            );
        }
    }

    fn format_elapsed(secs: f64) -> String {
        if secs < 60.0 {
            format!("{:4.0}s", secs)
        } else {
            let m = (secs / 60.0) as u64;
            let s = secs as u64 % 60;
            format!("{m}:{s:02}m")
        }
    }

    fn render_tools(
        &self,
        area: Rect,
        frame: &mut Frame,
        border: Color,
        label: Color,
        t: &dyn Theme,
    ) {
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
        clear_area(inner, buf, panel_bg(t));
        let w = inner.width as usize;
        let duration_w = 6usize.min(w.saturating_sub(8)).max(4);
        let name_w = 14.min(w.saturating_sub(duration_w + 6)).max(7);
        let bar_w = w.saturating_sub(name_w + duration_w + 2).max(0);

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

            clear_row(y, inner.x, inner.right(), buf, panel_bg(t));

            let age = (self.time - tool.last_called).max(0.0);
            let recency = if age > 120.0 {
                0.0
            } else {
                (1.0 - age / 120.0).max(0.0)
            };

            let indicator = if tool.is_error {
                "✗ "
            } else if tool.running {
                "▶ "
            } else if age < 2.0 {
                "▸ "
            } else {
                "  "
            };
            let ind_color = if tool.is_error {
                Color::Rgb(224, 72, 72)
            } else if tool.running {
                Color::Rgb(232, 186, 104)
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
            } else if tool.running {
                Color::Rgb(232, 186, 104)
            } else if recency > 0.1 {
                tool_color(recency)
            } else {
                Color::Rgb(48, 64, 80)
            };
            let bar_filled = (recency * bar_w as f64) as usize;
            let bar_color = if tool.is_error {
                Color::Rgb(224, 72, 72)
            } else if tool.running {
                Color::Rgb(232, 186, 104)
            } else {
                tool_color(recency)
            };

            let duration_ms = if tool.running {
                tool.started_at
                    .map(|started_at| ((self.time - started_at).max(0.0) * 1_000.0).round() as u64)
                    .unwrap_or(0)
            } else {
                tool.last_duration_ms.unwrap_or(0)
            };
            let time_str = Self::format_duration_ms(duration_ms);
            let time_color = if tool.is_error {
                Color::Rgb(224, 72, 72)
            } else if tool.running {
                Color::Rgb(232, 186, 104)
            } else {
                Color::Rgb(48, 64, 80)
            };

            let mut x = inner.x;
            x = render_str_colored(indicator, x, y, inner.right(), panel_bg(t), buf, |_| {
                ind_color
            });
            let short = tool_short_name(&tool.name);
            let display_name = truncate_str(&short, name_w, "…");
            x = render_str_colored(&display_name, x, y, inner.right(), panel_bg(t), buf, |_| {
                name_color
            });
            while x < inner.x + 2 + visible_width(&display_name) as u16 {
                if x >= inner.right() {
                    break;
                }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(' ');
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
            x = render_str_colored(&time_str, x, y, inner.right(), panel_bg(t), buf, |_| {
                time_color
            });
            while x < inner.x + 2 + name_w as u16 + duration_w as u16 {
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
        panel.update_telemetry(50.0, 200_000, None, false, "off", None, false, 0.016);
        assert!(
            (panel.context_fill - 0.5).abs() < 0.001,
            "context fill should track 50%"
        );
        panel.update_telemetry(100.0, 200_000, None, false, "off", None, false, 0.016);
        assert!(
            (panel.context_fill - 1.0).abs() < 0.001,
            "context fill should track 100%"
        );
    }

    #[test]
    fn set_cleave_progress_replaces_snapshot() {
        let mut panel = InstrumentPanel::default();
        panel.set_cleave_progress(Some(CleaveProgress {
            active: true,
            run_id: "r1".into(),
            total_children: 2,
            completed: 0,
            failed: 0,
            children: vec![],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));
        assert_eq!(
            panel.cleave_progress.as_ref().map(|cp| cp.run_id.as_str()),
            Some("r1")
        );

        panel.set_cleave_progress(Some(CleaveProgress {
            active: true,
            run_id: "r2".into(),
            total_children: 2,
            completed: 0,
            failed: 0,
            children: vec![],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));
        assert_eq!(
            panel.cleave_progress.as_ref().map(|cp| cp.run_id.as_str()),
            Some("r2")
        );
    }

    #[test]
    fn cleave_panel_reverts_to_tools_when_run_ends() {
        // active=false should mean tools panel is shown, even if total_children > 0.
        // Regression: old guard was `active || total_children > 0` which kept the
        // cleave grid visible forever after a run completed.
        let mut panel = InstrumentPanel::default();
        panel.set_cleave_progress(Some(CleaveProgress {
            active: false, // run finished
            run_id: "done-run".into(),
            total_children: 3, // still populated — old bug would key on this
            completed: 3,
            failed: 0,
            children: vec![],
            total_tokens_in: 100,
            total_tokens_out: 50,
        }));
        // The guard `cp.active` is what's tested — we verify by calling
        // render_tools_panel on a small area and confirming it doesn't crash.
        // The real assertion is code-level: render_cleave_panel is NOT entered.
        let area = ratatui::layout::Rect::new(0, 0, 60, 12);
        let backend = ratatui::backend::TestBackend::new(60, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;
        terminal
            .draw(|f| panel.render_tools_panel(area, f, &t))
            .unwrap();
        // If we reach here without entering the cleave branch, tools panel rendered.
        // Verify the cleave panel title "⟁ cleave" is NOT in the output.
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(
            !text.contains("⟁ cleave"),
            "cleave panel should not render when active=false: {text}"
        );
    }

    #[test]
    fn render_cleave_panel_clears_dirty_border_adjacent_cells() {
        let mut panel = InstrumentPanel::default();
        panel.set_cleave_progress(Some(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            total_children: 3,
            completed: 1,
            failed: 0,
            total_tokens_in: 0,
            total_tokens_out: 0,
            children: vec![
                crate::features::cleave::ChildProgress {
                    label: "alpha".into(),
                    status: "running".into(),
                    duration_secs: Some(3.2),
                    last_tool: Some("memory_recall".into()),
                    last_turn: Some(4),
                    started_at: None,
                    tokens_in: 0,
                    tokens_out: 0,
                },
                crate::features::cleave::ChildProgress {
                    label: "beta".into(),
                    status: "completed".into(),
                    duration_secs: Some(12.4),
                    last_tool: Some("commit".into()),
                    last_turn: Some(8),
                    started_at: None,
                    tokens_in: 0,
                    tokens_out: 0,
                },
            ],
        }));

        let area = Rect::new(0, 0, 48, 8);
        let backend = ratatui::backend::TestBackend::new(48, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char('Ω');
                            cell.set_fg(Color::White);
                            cell.set_bg(Color::Black);
                        }
                    }
                }
                panel.render_tools_panel(area, f, &t);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        for y in area.top()..area.bottom() {
            let left_border = &buf[(area.x, y)];
            let right_border = &buf[(area.right() - 1, y)];
            assert_ne!(left_border.symbol(), "Ω", "left border leaked at row {y}");
            assert_ne!(right_border.symbol(), "Ω", "right border leaked at row {y}");
        }
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
    fn render_clears_dirty_inference_and_tool_rows() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(18, 3, 2, 0.08);
        panel.tool_started("read");
        panel.tool_finished("read", false);
        let area = Rect::new(0, 0, 96, 12);
        let backend = ratatui::backend::TestBackend::new(96, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char('X');
                            cell.set_fg(Color::Red);
                            cell.set_bg(Color::Red);
                        }
                    }
                }
                panel.render(area, f, &t);
            })
            .unwrap();

        let buf = terminal.backend().buffer().clone();
        let footer_bg = panel_bg(&t);
        let residual = (0..buf.area.height)
            .flat_map(|dy| (0..buf.area.width).map(move |dx| (dx, dy)))
            .filter_map(|(dx, dy)| {
                let x = buf.area.x + dx;
                let y = buf.area.y + dy;
                let cell = buf.cell(Position::new(x, y))?;
                (cell.symbol() == "X" && cell.bg == footer_bg).then_some((x, y))
            })
            .collect::<Vec<_>>();

        assert!(
            residual.is_empty(),
            "instrument panel should clear dirty cells it owns, residual: {residual:?}"
        );
    }

    #[test]
    fn preferred_height_grows_with_active_minds_and_tools() {
        let mut panel = InstrumentPanel::default();
        let base = panel.preferred_height();
        panel.update_mind_facts(18, 3, 2, 0.08);
        panel.update_telemetry(
            62.0,
            200_000,
            Some("read"),
            false,
            "medium",
            None,
            true,
            0.016,
        );
        panel.update_telemetry(
            62.0,
            200_000,
            Some("bash"),
            false,
            "medium",
            None,
            true,
            0.016,
        );
        let grown = panel.preferred_height();
        assert!(
            grown >= base,
            "footer height should not shrink after activity"
        );
        assert!(
            grown >= 10 && grown <= 16,
            "preferred height stays bounded: {grown}"
        );
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
        panel.tool_started("bash");
        assert_eq!(panel.tools.len(), 1);
        assert_eq!(panel.tools[0].name, "bash");
        assert!(panel.tools[0].running);
    }

    #[test]
    fn tool_short_name_compacts_codebase_search() {
        assert_eq!(tool_short_name("codebase_search"), "⌕ cbase");
        assert_eq!(tool_short_name("codebase_index"), "⌕ cidx");
    }

    #[test]
    fn render_tools_clears_dirty_border_adjacent_cells() {
        let mut panel = InstrumentPanel::default();
        panel.tool_started("memory_recall");
        panel.tool_finished("memory_recall", false);
        panel.tool_started("commit");
        panel.tool_finished("commit", false);
        panel.tool_started("cleave_assess");
        panel.tool_finished("cleave_assess", false);

        let area = Rect::new(0, 0, 48, 8);
        let backend = ratatui::backend::TestBackend::new(48, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                for y in area.top()..area.bottom() {
                    for x in area.left()..area.right() {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char('Ω');
                            cell.set_fg(Color::White);
                            cell.set_bg(Color::Black);
                        }
                    }
                }
                panel.render_tools_panel(area, f, &t);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        for y in area.top()..area.bottom() {
            let left_border = &buf[(area.x, y)];
            let right_border = &buf[(area.right() - 1, y)];
            assert_ne!(left_border.symbol(), "Ω", "left border leaked at row {y}");
            assert_ne!(right_border.symbol(), "Ω", "right border leaked at row {y}");
        }
    }

    #[test]
    fn render_tools_does_not_paint_past_panel_boundary() {
        let mut panel = InstrumentPanel::default();
        panel.tool_started("Read");
        panel.update_telemetry(0.0, 200_000, None, false, "off", None, false, 0.0);
        panel.tool_finished("Read", false);

        let tools_area = Rect::new(20, 0, 28, 8);
        let backend = ratatui::backend::TestBackend::new(60, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                for y in 0..8 {
                    for x in 0..60 {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char(if x < tools_area.x { 'L' } else { 'R' });
                            cell.set_fg(Color::White);
                            cell.set_bg(Color::Black);
                        }
                    }
                }
                panel.render_tools_panel(tools_area, f, &t);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        for y in 0..8 {
            for x in 0..tools_area.x {
                let cell = &buf[(x, y)];
                assert_eq!(
                    cell.symbol(),
                    "L",
                    "tools panel painted left of its boundary at ({x},{y})"
                );
            }
            for x in tools_area.right()..60 {
                let cell = &buf[(x, y)];
                assert_eq!(
                    cell.symbol(),
                    "R",
                    "tools panel painted right of its boundary at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn tool_runtime_finishes_with_duration() {
        let mut panel = InstrumentPanel::default();
        panel.tool_started("bash");
        panel.update_telemetry(0.0, 200_000, None, false, "off", None, false, 1.25);
        panel.tool_finished("bash", false);
        let tool = panel.tools.iter().find(|t| t.name == "bash").unwrap();
        assert!(!tool.running);
        assert_eq!(tool.last_duration_ms, Some(1250));
    }

    #[test]
    fn duration_formatting_covers_ms_seconds_and_minutes() {
        assert_eq!(InstrumentPanel::format_duration_ms(220), " 220ms");
        assert_eq!(InstrumentPanel::format_duration_ms(8_100), " 8.1s");
        assert_eq!(InstrumentPanel::format_duration_ms(125_000), " 2:05");
    }

    #[test]
    fn zero_fact_minds_keep_explicit_counts() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(0, 0, 0, 0.0);
        assert_eq!(panel.debug_mind_fact_count(0), Some(0));
        assert_eq!(panel.debug_mind_fact_count(1), Some(0));
        assert_eq!(panel.debug_mind_fact_count(2), Some(0));
    }

    #[test]
    fn narrow_memory_rows_preserve_count_before_wave() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(0, 0, 0, 0.0);
        // Need at least 8 rows: border(1) + bar_h(2) + legend(1) + stats(1) + memory(1+) + border(1)
        let area = Rect::new(0, 0, 24, 8);
        let backend = ratatui::backend::TestBackend::new(24, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;

        terminal
            .draw(|f| panel.render_inference_panel(area, f, &t))
            .unwrap();

        let buf = terminal.backend().buffer();
        // Memory strings start at row 5 (bar_h=2, legend=row3, stats=row4, memories=row5+)
        let line: String = (0..buf.area.width)
            .map(|x| buf[(x, 5)].symbol().to_string())
            .collect::<String>();
        assert!(
            line.contains("project ⌗0")
                || line.contains("working ⌗0")
                || line.contains("episodes ⌗0"),
            "narrow memory rows should preserve explicit counts before the wave: {line:?}"
        );
    }

    #[test]
    fn update_mind_facts_populates_project_working_and_episodes() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(18, 3, 2, 0.25);
        assert_eq!(panel.minds[0].fact_count, 18);
        assert_eq!(panel.minds[1].fact_count, 3);
        assert_eq!(panel.minds[2].fact_count, 2);
        assert!(
            panel.minds[2].active,
            "episodes mind should activate when populated"
        );
        assert!(
            panel.memory_fill <= 0.12,
            "memory fill stays conservatively capped"
        );
    }

    #[test]
    fn context_breakdown_stays_normalized_and_ordered() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(18, 3, 2, 0.08);
        panel.update_turn_tokens(
            800,
            120,
            0,
            ContextComposition {
                conversation_tokens: 68_000,
                system_tokens: 10_000,
                memory_tokens: 12_000,
                tool_tokens: 6_000,
                thinking_tokens: 4_000,
                free_tokens: 100_000,
            },
            200_000,
        );
        panel.update_telemetry(
            62.0,
            200_000,
            Some("read"),
            false,
            "medium",
            None,
            true,
            0.016,
        );
        let breakdown = panel.context_breakdown();
        let total: f64 = breakdown.iter().map(|(_, frac)| frac).sum();
        assert!(
            (total - 1.0).abs() < 0.0001,
            "breakdown should sum to 1.0, got {total}"
        );
        assert_eq!(breakdown[0].0, ContextBand::Conversation);
        assert_eq!(breakdown[1].0, ContextBand::System);
        assert_eq!(breakdown[2].0, ContextBand::Memory);
        assert_eq!(breakdown[3].0, ContextBand::Tools);
        assert_eq!(breakdown[4].0, ContextBand::Thinking);
        assert_eq!(breakdown[5].0, ContextBand::Free);
        let used: f64 = breakdown[..5].iter().map(|(_, frac)| frac).sum();
        assert!(
            (used - 0.62).abs() < 0.0001,
            "used bands should match footer percent, got {used}"
        );
        assert!(
            (breakdown[5].1 - 0.38).abs() < 0.0001,
            "free band should be the complement of footer percent, got {}",
            breakdown[5].1
        );
    }

    #[test]
    fn thinking_pulse_stays_in_blue_family() {
        let low = InstrumentPanel::thinking_pulse_color(0.0);
        let high = InstrumentPanel::thinking_pulse_color(1.0);
        assert_eq!(low, InstrumentPanel::band_color(ContextBand::Thinking));
        match high {
            Color::Rgb(r, g, b) => {
                assert!(
                    b >= g && g >= r,
                    "thinking highlight should remain blue-dominant: {r},{g},{b}"
                );
                assert!(
                    b >= 214,
                    "thinking highlight should brighten the blue channel: {r},{g},{b}"
                );
            }
            other => panic!("unexpected thinking highlight color: {other:?}"),
        }
    }

    #[test]
    fn thinking_activity_mode_beats_tool_churn() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(
            40.0,
            200_000,
            Some("bash"),
            false,
            "high",
            None,
            true,
            0.016,
        );
        assert_eq!(panel.activity_mode(), ActivityMode::Thinking);
    }

    #[test]
    fn waiting_activity_mode_appears_without_thinking_budget() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(40.0, 200_000, None, false, "off", None, true, 0.5);
        assert_eq!(panel.activity_mode(), ActivityMode::Waiting);
    }

    #[test]
    fn fact_count_changes_pluck_project_wave() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(10, 0, 0, 0.02);
        for _ in 0..4 {
            panel.update_telemetry(0.0, 200_000, None, false, "off", None, false, 0.016);
        }
        let baseline = panel.minds[0].max_amplitude();
        panel.update_mind_facts(11, 0, 0, 0.02);
        for _ in 0..4 {
            panel.update_telemetry(0.0, 200_000, None, false, "off", None, false, 0.016);
        }
        let after = panel.minds[0].max_amplitude();
        assert!(
            after > baseline,
            "fact count increase should excite the project wave"
        );
    }

    #[test]
    fn update_mind_facts_uses_project_bucket_for_project_row() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(180, 12, 6, 0.04);
        assert_eq!(panel.minds[0].fact_count, 180);
        assert_eq!(panel.minds[1].fact_count, 12);
        assert_eq!(panel.minds[2].fact_count, 6);
    }

    #[test]
    fn inference_context_bar_renders_bucket_monikers() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(180, 12, 6, 0.08);
        panel.tool_started("read");
        panel.update_turn_tokens(
            800,
            120,
            0,
            ContextComposition {
                conversation_tokens: 68_000,
                system_tokens: 10_000,
                memory_tokens: 12_000,
                tool_tokens: 6_000,
                thinking_tokens: 4_000,
                free_tokens: 100_000,
            },
            200_000,
        );
        panel.update_telemetry(
            68.0,
            200_000,
            Some("read"),
            false,
            "high",
            None,
            true,
            0.016,
        );

        let area = Rect::new(0, 0, 64, 10);
        let backend = ratatui::backend::TestBackend::new(64, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;
        terminal
            .draw(|f| panel.render_inference_panel(area, f, &t))
            .unwrap();

        let buf = terminal.backend().buffer();
        let legend_row: String = (0..area.width)
            .map(|x| buf[(x, 3)].symbol().to_string())
            .collect();

        assert!(
            legend_row.contains("= conv"),
            "conversation bucket legend should be visible: {legend_row}"
        );
        assert!(
            legend_row.contains("+ sys"),
            "system bucket legend should be visible: {legend_row}"
        );
        assert!(
            legend_row.contains("* mem"),
            "memory bucket legend should be visible: {legend_row}"
        );
        assert!(
            legend_row.contains("# tool"),
            "tool surface legend should be visible as prompt composition: {legend_row}"
        );
        assert!(
            legend_row.contains("^ think"),
            "thinking bucket legend should be visible: {legend_row}"
        );
        assert!(
            !legend_row.contains("wait") && !legend_row.contains("idle"),
            "composition legend should not include non-context activity-state labels: {legend_row}"
        );
    }

    #[test]
    fn inference_context_bar_separates_composition_from_activity_rows() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(180, 12, 6, 0.08);
        panel.update_turn_tokens(
            800,
            120,
            0,
            ContextComposition {
                conversation_tokens: 68_000,
                system_tokens: 10_000,
                memory_tokens: 12_000,
                tool_tokens: 6_000,
                thinking_tokens: 4_000,
                free_tokens: 100_000,
            },
            200_000,
        );
        panel.update_telemetry(
            68.0,
            200_000,
            Some("read"),
            false,
            "high",
            None,
            true,
            0.016,
        );

        let area = Rect::new(0, 0, 64, 10);
        let backend = ratatui::backend::TestBackend::new(64, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;
        terminal
            .draw(|f| panel.render_inference_panel(area, f, &t))
            .unwrap();

        let buf = terminal.backend().buffer();
        let composition_row: String = (1..area.width - 1)
            .map(|x| buf[(x, 1)].symbol().to_string())
            .collect();
        let activity_row: String = (1..area.width - 1)
            .map(|x| buf[(x, 2)].symbol().to_string())
            .collect();

        assert!(
            composition_row
                .chars()
                .any(|ch| matches!(ch, '=' | '+' | '*' | '#' | '^' | '·')),
            "composition row should use simple band glyphs: {composition_row}"
        );
        assert!(
            activity_row
                .chars()
                .any(|ch| matches!(ch, ' ' | '·' | '•')),
            "activity row should use restrained runtime-state dot glyphs: {activity_row}"
        );
        assert_ne!(
            composition_row, activity_row,
            "composition and activity rows should not be duplicated overlays"
        );
    }

    #[test]
    fn inference_legend_avoids_partial_entries_in_narrow_widths() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(180, 12, 6, 0.08);
        panel.update_turn_tokens(
            800,
            120,
            0,
            ContextComposition {
                conversation_tokens: 68_000,
                system_tokens: 10_000,
                memory_tokens: 12_000,
                tool_tokens: 6_000,
                thinking_tokens: 4_000,
                free_tokens: 100_000,
            },
            200_000,
        );
        panel.update_telemetry(
            68.0,
            200_000,
            Some("read"),
            false,
            "high",
            None,
            true,
            0.016,
        );

        let area = Rect::new(0, 0, 28, 10);
        let backend = ratatui::backend::TestBackend::new(28, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;
        terminal
            .draw(|f| panel.render_inference_panel(area, f, &t))
            .unwrap();

        let buf = terminal.backend().buffer();
        let legend_row: String = (1..area.width - 1)
            .map(|x| buf[(x, 3)].symbol().to_string())
            .collect();
        let trimmed = legend_row.trim_end();

        assert!(
            trimmed.contains("conv") && trimmed.contains("sys"),
            "narrow legend should still show complete leading entries: {trimmed:?}"
        );
        assert!(
            !trimmed.contains("thi")
                && !trimmed.contains("^ t")
                && !trimmed.contains("wait")
                && !trimmed.contains("idle")
                && !trimmed.ends_with('…'),
            "narrow legend should omit entries it cannot fit instead of clipping them: {trimmed:?}"
        );
        assert!(
            !trimmed.contains("conv  ")
                && !trimmed.contains("sys  ")
                && !trimmed.contains("mem  ")
                && !trimmed.contains("think  "),
            "narrow legend should not insert multi-space gaps between entries: {trimmed:?}"
        );
    }

    #[test]
    fn composition_legend_keeps_free_space_implicit() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(180, 12, 6, 0.08);
        panel.update_turn_tokens(
            800,
            120,
            0,
            ContextComposition {
                conversation_tokens: 68_000,
                system_tokens: 10_000,
                memory_tokens: 12_000,
                tool_tokens: 6_000,
                thinking_tokens: 4_000,
                free_tokens: 100_000,
            },
            200_000,
        );
        panel.update_telemetry(68.0, 200_000, None, false, "high", None, true, 0.016);

        let area = Rect::new(0, 0, 64, 10);
        let backend = ratatui::backend::TestBackend::new(64, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;
        terminal
            .draw(|f| panel.render_inference_panel(area, f, &t))
            .unwrap();

        let buf = terminal.backend().buffer();
        let legend_row: String = (1..area.width - 1)
            .map(|x| buf[(x, 3)].symbol().to_string())
            .collect();

        assert!(
            !legend_row.contains("free") && !legend_row.contains('~'),
            "free capacity should remain implicit in the grey simple-glyph tail, not the legend: {legend_row}"
        );
    }

    #[test]
    fn inference_panel_does_not_paint_into_adjacent_tools_panel() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(704, 0, 624, 0.08);
        panel.update_turn_tokens(
            105_100,
            538,
            0,
            ContextComposition {
                conversation_tokens: 105_100,
                system_tokens: 538,
                memory_tokens: 0,
                tool_tokens: 0,
                thinking_tokens: 0,
                free_tokens: 166_362,
            },
            272_000,
        );
        panel.update_telemetry(39.0, 272_000, None, false, "medium", None, true, 0.016);

        let inference_area = Rect::new(0, 0, 36, 10);
        let tools_area = Rect::new(36, 0, 28, 10);
        let backend = ratatui::backend::TestBackend::new(64, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let t = crate::tui::theme::Alpharius;

        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                for y in 0..10 {
                    for x in 0..64 {
                        if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                            cell.set_char(if x < tools_area.x { 'L' } else { 'R' });
                            cell.set_fg(Color::White);
                            cell.set_bg(Color::Black);
                        }
                    }
                }
                panel.render_inference_panel(inference_area, f, &t);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        for y in 0..10 {
            for x in tools_area.x..64 {
                let cell = &buf[(x, y)];
                assert_eq!(
                    cell.symbol(),
                    "R",
                    "inference panel painted into adjacent tools area at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn tool_surface_claims_a_composition_band_when_tools_are_active() {
        let mut panel = InstrumentPanel::default();
        panel.update_mind_facts(180, 12, 6, 0.08);
        panel.tool_started("read");
        panel.update_turn_tokens(
            800,
            120,
            0,
            ContextComposition {
                conversation_tokens: 68_000,
                system_tokens: 10_000,
                memory_tokens: 12_000,
                tool_tokens: 6_000,
                thinking_tokens: 4_000,
                free_tokens: 100_000,
            },
            200_000,
        );
        panel.update_telemetry(68.0, 200_000, Some("read"), false, "high", None, true, 0.016);

        let breakdown = panel.context_breakdown();
        let tools = breakdown
            .iter()
            .find_map(|(band, frac)| (*band == ContextBand::Tools).then_some(*frac))
            .unwrap_or(0.0);
        let conversation = breakdown
            .iter()
            .find_map(|(band, frac)| (*band == ContextBand::Conversation).then_some(*frac))
            .unwrap_or(0.0);

        assert!(tools > 0.0, "active tools should reserve some context surface");
        assert!(conversation > 0.0, "conversation should still retain its own band");
    }
}

impl InstrumentPanel {
    #[cfg(test)]
    pub fn debug_mind_fact_count(&self, idx: usize) -> Option<usize> {
        self.minds.get(idx).map(|mind| mind.fact_count)
    }
}

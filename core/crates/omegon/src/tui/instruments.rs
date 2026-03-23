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

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

// ─── Color ramp (CIE L* perceptual) ────────────────────────────────────

fn intensity_color(intensity: f64) -> Color {
    if intensity < 0.005 { return Color::Rgb(0, 1, 3); }
    let linear = intensity.clamp(0.0, 1.0);
    let i = if linear > 0.008856 { linear.cbrt() } else { linear * 7.787 + 16.0 / 116.0 };
    let i = ((i - 0.138) / (1.0 - 0.138)).clamp(0.0, 1.0);
    if i < 0.3 {
        let t = i / 0.3;
        Color::Rgb((1.0 + t * 3.0) as u8, (4.0 + t * 34.0) as u8, (6.0 + t * 30.0) as u8)
    } else if i < 0.5 {
        let t = (i - 0.3) / 0.2;
        Color::Rgb((4.0 + t * 4.0) as u8, (38.0 + t * 10.0) as u8, (36.0 + t * 6.0) as u8)
    } else {
        let t = (i - 0.5) / 0.5;
        Color::Rgb((8.0 + t * 82.0) as u8, (48.0 - t * 2.0) as u8, (42.0 - t * 34.0) as u8)
    }
}

fn bg_color() -> Color { Color::Rgb(0, 1, 3) }

const NOISE_CHARS: &[char] = &[
    '▏', '▎', '▍', '░', '▌', '▐', '▒', '┤', '├', '│', '─',
    '▊', '▋', '▓', '╱', '╲', '┼', '╪', '╫', '█', '╬', '■', '◆',
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
        Self { name: name.into(), active, fact_count: 0, wave: vec![0.0; w], velocity: vec![0.0; w], damping: 0.92 }
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
        if w < 3 { return; }
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
    thinking_active: bool,
    thinking_intensity: f64,
    minds: Vec<MindState>,
    tools: Vec<ToolEntry>,
    pub focus_mode: bool,
}

impl Default for InstrumentPanel {
    fn default() -> Self {
        Self {
            time: 0.0,
            context_fill: 0.0,
            thinking_active: false,
            thinking_intensity: 0.0,
            minds: vec![
                MindState::new("project", true),
                MindState::new("working", false),
                MindState::new("episodes", false),
                MindState::new("archive", false),
            ],
            tools: Vec::new(),
            focus_mode: false,
        }
    }
}

impl InstrumentPanel {
    /// Update mind fact counts from footer data.
    pub fn update_mind_facts(&mut self, total_facts: usize, working_memory: usize) {
        if !self.minds.is_empty() { self.minds[0].fact_count = total_facts; }
        if self.minds.len() > 1 { self.minds[1].fact_count = working_memory; }
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

        // Context: cap at 70%
        self.context_fill = (context_pct as f64 / 70.0).min(1.0);

        // Thinking: only active during inference
        self.thinking_active = agent_active;
        let target = if agent_active {
            match thinking_level {
                "high" => 0.85, "medium" => 0.6, "low" => 0.35, "minimal" => 0.15, _ => 0.1,
            }
        } else { 0.0 };
        self.thinking_intensity += (target - self.thinking_intensity) * dt * 3.0;

        // Tool: register call
        if let Some(name) = tool_name {
            if let Some(entry) = self.tools.iter_mut().find(|t| t.name == name) {
                entry.last_called = self.time;
                if tool_error { entry.is_error = true; entry.error_ttl = 5.0; }
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
                if tool.error_ttl <= 0.0 { tool.is_error = false; }
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
            if mind.active { mind.update(); }
        }
    }

    pub fn set_tool_error(&mut self, name: &str) {
        if let Some(entry) = self.tools.iter_mut().find(|t| t.name == name) {
            entry.is_error = true;
            entry.error_ttl = 5.0;
        }
    }

    pub fn toggle_focus(&mut self) { self.focus_mode = !self.focus_mode; }

    /// Render the instrument panel, with optional tutorial highlight.
    pub fn render_with_highlight(&mut self, area: Rect, frame: &mut Frame, highlight: bool, theme: &dyn super::theme::Theme) {
        if area.width < 20 || area.height < 4 { return; }

        let panels = Layout::horizontal([
            Constraint::Percentage(55),
            Constraint::Percentage(45),
        ]).split(area);

        let border_color = if highlight { theme.accent_bright() } else { Color::Rgb(20, 40, 55) };
        self.render_inference_with_border(panels[0], frame, border_color);
        self.render_tools_with_border(panels[1], frame, border_color);
    }

    fn render_inference_with_border(&self, area: Rect, frame: &mut Frame, border_color: Color) {
        let title_color = if border_color == Color::Rgb(20, 40, 55) {
            Color::Rgb(64, 88, 112) // default muted title
        } else {
            border_color // highlight: title matches border
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" inference ", Style::default().fg(title_color)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 10 || inner.height < 3 { return; }

        let buf = frame.buffer_mut();
        let active_minds: Vec<usize> = self.minds.iter().enumerate()
            .filter(|(_, m)| m.active).map(|(i, _)| i).collect();

        // Idle state — show ready indicator when context is empty and no minds active
        if self.context_fill < 0.001 && active_minds.is_empty() && !self.thinking_active {
            let hints = [
                ("", Color::Rgb(36, 52, 68)),
                ("  ready", Color::Rgb(48, 80, 100)),
                ("", Color::Rgb(36, 52, 68)),
                ("  context and memory", Color::Rgb(36, 52, 68)),
                ("  activity shown here", Color::Rgb(36, 52, 68)),
            ];
            for (row, (text, color)) in hints.iter().enumerate() {
                let y = inner.y + row as u16;
                if y >= inner.bottom() { break; }
                for (i, ch) in text.chars().enumerate() {
                    let x = inner.x + i as u16;
                    if x >= inner.right() { break; }
                    if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                        cell.set_char(ch); cell.set_fg(*color); cell.set_bg(bg_color());
                    }
                }
            }
            return;
        }

        // Context bar: top 2 rows
        let bar_h = 2u16.min(inner.height);
        let bar_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: bar_h };
        self.render_context_bar(bar_area, buf);

        // Tree + memory strings: break through the left border
        if inner.height > bar_h && !active_minds.is_empty() {
            // Start at the panel's left BORDER (area.x, not inner.x)
            // so the tree trunk overlays the border character
            let tree_area = Rect {
                x: area.x, y: inner.y + bar_h,
                width: inner.width + 1, // include border column
                height: inner.height - bar_h,
            };
            self.render_memory_strings(&active_minds, tree_area, buf);
        }
    }

    fn render_context_bar(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let fill_cols = (self.context_fill * w as f64) as usize;

        for x in 0..w {
            let intensity = if x < fill_cols {
                (x as f64 / fill_cols.max(1) as f64) * self.context_fill
            } else { 0.0 };

            let is_glitch = self.thinking_intensity > 0.05 && {
                let hash = ((x * 17 + (self.time * 8.0) as usize) * 31) % 100;
                (hash as f64) < self.thinking_intensity * 60.0
            };

            if is_glitch {
                let idx = ((x * 7 + (self.time * 12.0) as usize) * 13) % NOISE_CHARS.len();
                let color = intensity_color((intensity + self.thinking_intensity * 0.3).min(1.0));
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y)) {
                    cell.set_char(NOISE_CHARS[idx]);
                    cell.set_fg(color);
                    cell.set_bg(bg_color());
                }
            } else {
                let color = intensity_color(intensity);
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y)) {
                    cell.set_char(if intensity > 0.01 { '█' } else { ' ' });
                    cell.set_fg(color);
                    cell.set_bg(bg_color());
                }
            }
        }

        // Row 2: percentage
        if area.height > 1 {
            let pct = (self.context_fill * 70.0) as u32; // show actual %, not normalized
            let label = format!(" {}%", pct);
            let color = intensity_color(self.context_fill);
            for (i, ch) in label.chars().enumerate() {
                if i >= w { break; }
                if let Some(cell) = buf.cell_mut(Position::new(area.x + i as u16, area.y + 1)) {
                    cell.set_char(ch);
                    cell.set_fg(color);
                    cell.set_bg(bg_color());
                }
            }
        }
    }

    fn render_memory_strings(&self, active_minds: &[usize], area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let n = active_minds.len();

        for (row_idx, &mind_idx) in active_minds.iter().enumerate() {
            let y = area.y + row_idx as u16;
            if y >= area.bottom() { break; }
            let mind = &self.minds[mind_idx];
            let is_last = row_idx == n - 1;

            // Tree connector
            let connector = if is_last { "└─" } else { "├─" };
            for (i, ch) in connector.chars().enumerate() {
                if let Some(cell) = buf.cell_mut(Position::new(area.x + i as u16, y)) {
                    cell.set_char(ch);
                    cell.set_fg(Color::Rgb(32, 72, 96));
                    cell.set_bg(bg_color());
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
                if x >= w { break; }
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, y)) {
                    cell.set_char(ch);
                    cell.set_fg(name_color);
                    cell.set_bg(bg_color());
                }
            }

            // Sine wave — braille dots for sub-character resolution
            // Each braille cell: 2 dots wide × 4 dots tall
            // Wave displacement maps to vertical dot position
            let wave_start = (name_start + label.len() + 1).min(w / 3);
            let wave_w = w.saturating_sub(wave_start);
            let wave_len = mind.wave.len();
            for wx in 0..wave_w {
                let x = wave_start + wx;
                if x >= w { break; }

                // Sample two adjacent wave points (one per braille column)
                let pos0 = (wx as f64 * 2.0 / (wave_w as f64 * 2.0)) * wave_len as f64;
                let pos1 = ((wx as f64 * 2.0 + 1.0) / (wave_w as f64 * 2.0)) * wave_len as f64;
                let d0 = mind.wave[(pos0 as usize).min(wave_len.saturating_sub(1))];
                let d1 = mind.wave[(pos1 as usize).min(wave_len.saturating_sub(1))];

                // Map displacement to braille row (0=top, 3=bottom)
                let row0 = (1.5 - d0 * 0.8).clamp(0.0, 3.0) as u8;
                let row1 = (1.5 - d1 * 0.8).clamp(0.0, 3.0) as u8;

                // Braille dot bits: col0=[0x01,0x02,0x04,0x40] col1=[0x08,0x10,0x20,0x80]
                let bit0 = match row0 { 0 => 0x01, 1 => 0x02, 2 => 0x04, _ => 0x40 };
                let bit1 = match row1 { 0 => 0x08, 1 => 0x10, 2 => 0x20, _ => 0x80 };

                let amp = d0.abs().max(d1.abs());
                let dots = if amp < 0.02 {
                    0x04 | 0x20 // flat middle line when idle
                } else {
                    bit0 | bit1
                };

                let ch = char::from_u32(0x2800 + dots as u32).unwrap_or('·');
                let intensity = (amp * 0.5).min(1.0);
                let color = if intensity > 0.01 { intensity_color(intensity) } else { Color::Rgb(20, 40, 55) };
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, y)) {
                    cell.set_char(ch);
                    cell.set_fg(color);
                    cell.set_bg(bg_color());
                }
            }
        }
    }

    fn render_tools_with_border(&self, area: Rect, frame: &mut Frame, border_color: Color) {
        let title_color = if border_color == Color::Rgb(20, 40, 55) {
            Color::Rgb(64, 88, 112)
        } else {
            border_color
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" tools ", Style::default().fg(title_color)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width < 15 || inner.height < 2 { return; }

        let buf = frame.buffer_mut();
        let w = inner.width as usize;
        let name_w = 15.min(w / 2);
        let bar_w = w.saturating_sub(name_w + 6).max(2);

        // Idle state — show hints when no tools have been called yet
        if self.tools.is_empty() {
            let hints = [
                ("", Color::Rgb(48, 64, 80)),
                ("  tools appear here as", Color::Rgb(48, 64, 80)),
                ("  the agent calls them", Color::Rgb(48, 64, 80)),
                ("", Color::Rgb(48, 64, 80)),
                ("  each shows a recency", Color::Rgb(36, 52, 68)),
                ("  bar and time elapsed", Color::Rgb(36, 52, 68)),
            ];
            for (row, (text, color)) in hints.iter().enumerate() {
                let y = inner.y + row as u16;
                if y >= inner.bottom() { break; }
                for (i, ch) in text.chars().enumerate() {
                    let x = inner.x + i as u16;
                    if x >= inner.right() { break; }
                    if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                        cell.set_char(ch); cell.set_fg(*color); cell.set_bg(bg_color());
                    }
                }
            }
            return;
        }

        // Sort by recency
        let mut sorted: Vec<&ToolEntry> = self.tools.iter().collect();
        sorted.sort_by(|a, b| b.last_called.partial_cmp(&a.last_called).unwrap_or(std::cmp::Ordering::Equal));

        for (row, tool) in sorted.iter().enumerate() {
            let y = inner.y + row as u16;
            if y >= inner.bottom().saturating_sub(1) { break; } // leave room for footer

            let age = (self.time - tool.last_called).max(0.0);
            let recency = if age > 120.0 { 0.0 } else { (1.0 - age / 120.0).max(0.0) };

            let indicator = if age < 2.0 { "▸ " } else { "  " };
            let ind_color = if tool.is_error { Color::Rgb(224, 72, 72) }
                else if age < 2.0 { Color::Rgb(42, 180, 200) }
                else { Color::Rgb(20, 40, 55) };
            // Tool colors: clean teal→amber gradient (not the CIE L* ramp which
            // produces olive/muddy teal at mid-range that looks wrong for bars)
            let tool_color = |r: f64| -> Color {
                if r < 0.01 { return Color::Rgb(20, 30, 40); }
                let r = r.clamp(0.0, 1.0);
                // Dim teal at low recency, bright amber at high
                Color::Rgb(
                    (10.0 + r * 70.0) as u8,   // 10 → 80
                    (30.0 + r * 20.0) as u8,    // 30 → 50
                    (40.0 - r * 30.0) as u8,    // 40 → 10
                )
            };
            let name_color = if tool.is_error { Color::Rgb(224, 72, 72) }
                else if recency > 0.1 { tool_color(recency) }
                else { Color::Rgb(48, 64, 80) };
            let bar_filled = (recency * bar_w as f64) as usize;
            let bar_color = if tool.is_error { Color::Rgb(224, 72, 72) } else { tool_color(recency) };

            let time_str = if age > 999.0 { "   ·".to_string() }
                else if age > 60.0 { format!("{:>3.0}m", age / 60.0) }
                else { format!("{:>3.0}s", age) };

            let mut x = inner.x;
            for ch in indicator.chars() {
                if x >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch); cell.set_fg(ind_color); cell.set_bg(bg_color());
                }
                x += 1;
            }
            let display_name = if tool.name.len() > name_w - 2 { &tool.name[..name_w - 2] } else { &tool.name };
            for ch in display_name.chars() {
                if x >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch); cell.set_fg(name_color); cell.set_bg(bg_color());
                }
                x += 1;
            }
            while x < inner.x + 2 + name_w as u16 {
                if x >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) { cell.set_char(' '); cell.set_bg(bg_color()); }
                x += 1;
            }
            for i in 0..bar_w {
                if x >= inner.right() { break; }
                let ch = if i < bar_filled { '█' } else { '░' };
                let c = if i < bar_filled { bar_color } else { Color::Rgb(10, 16, 24) };
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch); cell.set_fg(c); cell.set_bg(bg_color());
                }
                x += 1;
            }
            for ch in time_str.chars() {
                if x >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                    cell.set_char(ch); cell.set_fg(Color::Rgb(48, 64, 80)); cell.set_bg(bg_color());
                }
                x += 1;
            }
        }

        // Footer
        let footer_y = inner.bottom().saturating_sub(1);
        if footer_y > inner.y + sorted.len() as u16 {
            let active = self.tools.iter().filter(|t| self.time - t.last_called < 120.0).count();
            let total = self.tools.len();
            let footer = format!("  {active}/{total} active");
            for (i, ch) in footer.chars().enumerate() {
                let x = inner.x + i as u16;
                if x >= inner.right() { break; }
                if let Some(cell) = buf.cell_mut(Position::new(x, footer_y)) {
                    cell.set_char(ch); cell.set_fg(Color::Rgb(48, 64, 80)); cell.set_bg(bg_color());
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
    fn intensity_color_floor_is_bg() {
        assert!(matches!(intensity_color(0.0), Color::Rgb(0, 1, 3)));
    }

    #[test]
    fn panel_renders_without_panic() {
        let mut panel = InstrumentPanel::default();
        let area = Rect::new(0, 0, 96, 12);
        let backend = ratatui::backend::TestBackend::new(96, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| panel.render_with_highlight(area, f, false, &crate::tui::theme::Alpharius)).unwrap();
    }

    #[test]
    fn wave_physics_dampens() {
        let mut mind = MindState::new("test", true);
        mind.pluck(WaveDirection::Right);
        // Let wave build up from velocity
        for _ in 0..20 { mind.update(); }
        let peak = mind.max_amplitude();
        assert!(peak > 0.01, "wave should have amplitude after pluck: {peak:.3}");
        // Let it dampen
        for _ in 0..500 { mind.update(); }
        let final_amp = mind.max_amplitude();
        assert!(final_amp < peak * 0.5, "wave should dampen: peak={peak:.3} final={final_amp:.3}");
    }

    #[test]
    fn tool_registration() {
        let mut panel = InstrumentPanel::default();
        panel.update_telemetry(0.0, Some("bash"), false, "off", None, false, 0.016);
        assert_eq!(panel.tools.len(), 1);
        assert_eq!(panel.tools[0].name, "bash");
    }
}

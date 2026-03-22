//! Instrument Lab — R&D prototype for the unified instrument panel.
//! Run: cargo run -p omegon --example instrument_lab
//!
//! Two-panel layout:
//!   LEFT:  Inference state (context bar + thinking glitch + memory sine strings)
//!   RIGHT: Tool activity (bubble-sort list with recency bars)
//!
//! Controls:
//!   1-4    Context: 10% / 30% / 50% / 70%
//!   5      Tool: bash
//!   6      Tool: write (burst)
//!   7      Tool: read
//!   8      Tool: memory_store
//!   9      Thinking: start
//!   0      Thinking: stop
//!   -      Memory: store to project (→ wave)
//!   =      Memory: recall from project (← wave)
//!   [      Memory: link working mind
//!   ]      Memory: unlink working mind
//!   r      Reset all
//!   q      Quit

use std::io;
use std::time::{Duration, Instant};
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let start = Instant::now();
    let mut state = LabState::default();

    loop {
        let t = start.elapsed().as_secs_f64();
        let dt = 0.033;

        state.update(dt, t);

        terminal.draw(|f| {
            let area = f.area();
            let bg = Color::Rgb(0, 1, 3);
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    let cell = &mut f.buffer_mut()[(x, y)];
                    cell.set_bg(bg);
                    cell.set_fg(Color::Rgb(196, 216, 228));
                }
            }

            // Title
            let title = format!(
                " Instrument Lab · ctx {}% · think {} · {} minds · t={:.0}s",
                (state.context_fill * 100.0) as u32,
                if state.thinking_active { "ON" } else { "off" },
                state.minds.iter().filter(|m| m.active).count(),
                t,
            );
            f.render_widget(
                Paragraph::new(title).style(Style::default().fg(Color::Rgb(42, 180, 200)).add_modifier(Modifier::BOLD)),
                Rect { x: area.x, y: area.y, width: area.width, height: 1 },
            );

            // Two-panel split
            let body = Rect { x: area.x, y: area.y + 1, width: area.width, height: area.height.saturating_sub(3) };
            let panels = Layout::horizontal([
                Constraint::Percentage(55),
                Constraint::Percentage(45),
            ]).split(body);

            // LEFT: Inference state
            render_inference_panel(&state, t, panels[0], f.buffer_mut());

            // RIGHT: Tool activity
            render_tool_panel(&state, panels[1], f.buffer_mut());

            // Controls
            let ctrl_area = Rect { x: area.x, y: area.bottom() - 2, width: area.width, height: 2 };
            let controls = vec![
                Line::from(Span::styled(
                    " 1-4=ctx  5-8=tools  9/0=think  -/==mem store/recall  [/]=minds  r=reset  q=quit",
                    Style::default().fg(Color::Rgb(64, 88, 112)),
                )),
            ];
            f.render_widget(Paragraph::new(controls), ctrl_area);
        })?;

        if event::poll(Duration::from_millis(33))? {
            if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('1') => state.context_fill = 0.10,
                    KeyCode::Char('2') => state.context_fill = 0.30,
                    KeyCode::Char('3') => state.context_fill = 0.50,
                    KeyCode::Char('4') => state.context_fill = 0.70,
                    KeyCode::Char('5') => state.fire_tool("bash"),
                    KeyCode::Char('6') => { state.fire_tool("write"); state.fire_tool("write"); state.fire_tool("write"); }
                    KeyCode::Char('7') => state.fire_tool("read"),
                    KeyCode::Char('8') => state.fire_tool("memory_store"),
                    KeyCode::Char('9') => state.thinking_active = true,
                    KeyCode::Char('0') => state.thinking_active = false,
                    KeyCode::Char('-') => state.pluck_mind(0, WaveDirection::Right), // store
                    KeyCode::Char('=') => state.pluck_mind(0, WaveDirection::Left),  // recall
                    KeyCode::Char('[') => state.toggle_mind(1),
                    KeyCode::Char(']') => { if state.minds[1].active { state.toggle_mind(1); } else { state.toggle_mind(2); } }
                    KeyCode::Char('r') => state = LabState::default(),
                    _ => {}
                }
            }
        }
    }

    terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

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

// ─── CRT noise glyphs ──────────────────────────────────────────────────

const NOISE_CHARS: &[char] = &[
    '▏', '▎', '▍', '░',
    '▌', '▐', '▒', '┤', '├', '│', '─',
    '▊', '▋', '▓', '╱', '╲', '┼', '╪', '╫',
    '█', '╬', '■', '◆',
];

// ─── State ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum WaveDirection { Left, Right }

struct MindState {
    name: &'static str,
    active: bool,
    /// Wave samples across the width — displacement from center
    wave: Vec<f64>,
    /// Wave velocity at each sample point
    velocity: Vec<f64>,
    /// Damping factor
    damping: f64,
}

impl MindState {
    fn new(name: &'static str, active: bool) -> Self {
        let w = 60;
        Self { name, active, wave: vec![0.0; w], velocity: vec![0.0; w], damping: 0.96 }
    }

    /// Pluck the string — inject a wave packet traveling in a direction
    fn pluck(&mut self, direction: WaveDirection) {
        let w = self.wave.len();
        let center = w / 2;
        let sign = match direction {
            WaveDirection::Right => 1.0,
            WaveDirection::Left => -1.0,
        };
        // Gaussian pulse in the center
        for i in 0..w {
            let dx = (i as f64 - center as f64) / 4.0;
            let pulse = (-dx * dx / 2.0).exp() * 2.0;
            self.velocity[i] += pulse * sign * 0.5;
        }
    }

    /// Update wave physics — simple 1D wave equation with damping
    fn update(&mut self, _dt: f64) {
        let w = self.wave.len();
        if w < 3 { return; }

        // Wave equation: acceleration = c² * (left + right - 2*center)
        let c2 = 0.3; // wave speed squared
        let mut accel = vec![0.0; w];
        for i in 1..w - 1 {
            accel[i] = c2 * (self.wave[i - 1] + self.wave[i + 1] - 2.0 * self.wave[i]);
        }

        for i in 0..w {
            self.velocity[i] = (self.velocity[i] + accel[i]) * self.damping;
            self.wave[i] += self.velocity[i];
        }

        // Fixed boundaries
        self.wave[0] = 0.0;
        self.wave[w - 1] = 0.0;
        self.velocity[0] = 0.0;
        self.velocity[w - 1] = 0.0;
    }

    fn max_amplitude(&self) -> f64 {
        self.wave.iter().map(|v| v.abs()).fold(0.0_f64, f64::max)
    }
}

struct ToolEntry {
    name: String,
    last_called: f64, // time since epoch
    call_count: u32,
    #[allow(dead_code)] is_error: bool,
}

struct LabState {
    context_fill: f64,       // 0-1
    thinking_active: bool,
    thinking_intensity: f64, // smoothed 0-1
    minds: Vec<MindState>,
    tools: Vec<ToolEntry>,
    time: f64,
    /// RNG for glitch characters
    #[allow(dead_code)] rng: u64,
}

impl Default for LabState {
    fn default() -> Self {
        Self {
            context_fill: 0.0,
            thinking_active: false,
            thinking_intensity: 0.0,
            minds: vec![
                MindState::new("project", true),
                MindState::new("working", false),
                MindState::new("episodes", false),
                MindState::new("archive", false),
            ],
            tools: vec![
                ToolEntry { name: "bash".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "read".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "write".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "edit".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "memory_store".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "memory_recall".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "design_tree".into(), last_called: -999.0, call_count: 0, is_error: false },
                ToolEntry { name: "web_search".into(), last_called: -999.0, call_count: 0, is_error: false },
            ],
            time: 0.0,
            rng: 0xdeadbeef,
        }
    }
}

impl LabState {
    fn update(&mut self, dt: f64, time: f64) {
        self.time = time;

        // Thinking intensity smoothing
        let target = if self.thinking_active { 0.8 } else { 0.0 };
        self.thinking_intensity += (target - self.thinking_intensity) * dt * 3.0;

        // Update mind wave physics
        for mind in &mut self.minds {
            if mind.active {
                mind.update(dt);
            }
        }
    }

    fn fire_tool(&mut self, name: &str) {
        if let Some(tool) = self.tools.iter_mut().find(|t| t.name == name) {
            tool.last_called = self.time;
            tool.call_count += 1;
        }
    }

    fn pluck_mind(&mut self, idx: usize, dir: WaveDirection) {
        if idx < self.minds.len() && self.minds[idx].active {
            self.minds[idx].pluck(dir);
        }
    }

    fn toggle_mind(&mut self, idx: usize) {
        if idx < self.minds.len() {
            self.minds[idx].active = !self.minds[idx].active;
            if self.minds[idx].active {
                let w = 60;
                self.minds[idx].wave = vec![0.0; w];
                self.minds[idx].velocity = vec![0.0; w];
            }
        }
    }

    #[allow(dead_code)]
    fn next_rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
}

// ─── LEFT PANEL: Inference State ────────────────────────────────────────

fn render_inference_panel(state: &LabState, time: f64, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(20, 40, 55)))
        .title(Span::styled(" inference ", Style::default().fg(Color::Rgb(64, 88, 112))));
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width < 10 || inner.height < 4 { return; }

    // Layout: context bar (2 rows) + tree connector + mind waves
    let active_minds: Vec<usize> = state.minds.iter().enumerate()
        .filter(|(_, m)| m.active).map(|(i, _)| i).collect();
    

    // Context bar: top 2 rows
    let bar_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: 2.min(inner.height) };
    render_context_bar(state, time, bar_area, buf);

    // Tree connector + mind waves: remaining rows
    if inner.height > 3 && !active_minds.is_empty() {
        let tree_y = inner.y + 2;
        let tree_h = inner.height.saturating_sub(2);
        let tree_area = Rect { x: inner.x, y: tree_y, width: inner.width, height: tree_h };
        render_memory_strings(state, &active_minds, tree_area, buf);
    }
}

fn render_context_bar(state: &LabState, _time: f64, area: Rect, buf: &mut Buffer) {
    let w = area.width as usize;

    // Row 1: gradient fill bar
    let fill_cols = ((state.context_fill / 0.7).min(1.0) * w as f64) as usize;
    for x in 0..w {
        let intensity = if x < fill_cols {
            (x as f64 / fill_cols.max(1) as f64) * (state.context_fill / 0.7).min(1.0)
        } else {
            0.0
        };

        // Thinking glitch overlay — replace some cells with noise chars
        let is_glitch = state.thinking_intensity > 0.05 && {
            let hash = ((x * 17 + (state.time * 8.0) as usize) * 31) % 100;
            (hash as f64) < state.thinking_intensity * 60.0
        };

        if is_glitch {
            let char_idx = ((x * 7 + (state.time * 12.0) as usize) * 13) % NOISE_CHARS.len();
            let ch = NOISE_CHARS[char_idx];
            let color = intensity_color((intensity + state.thinking_intensity * 0.3).min(1.0));
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y)) {
                cell.set_char(ch);
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

    // Row 2: percentage label
    if area.height > 1 {
        let pct = (state.context_fill * 100.0) as u32;
        let label = format!(" {}% / 200k", pct);
        let label_color = intensity_color((state.context_fill / 0.7).min(1.0));
        for (i, ch) in label.chars().enumerate() {
            if i >= w { break; }
            if let Some(cell) = buf.cell_mut(Position::new(area.x + i as u16, area.y + 1)) {
                cell.set_char(ch);
                cell.set_fg(label_color);
                cell.set_bg(bg_color());
            }
        }
    }
}

fn render_memory_strings(state: &LabState, active_minds: &[usize], area: Rect, buf: &mut Buffer) {
    let w = area.width as usize;
    let n = active_minds.len();

    for (row_idx, &mind_idx) in active_minds.iter().enumerate() {
        let y = area.y + row_idx as u16;
        if y >= area.bottom() { break; }

        let mind = &state.minds[mind_idx];
        let is_last = row_idx == n - 1;

        // Tree connector character (column 0-2)
        let connector = if is_last { "└─" } else { "├─" };
        for (i, ch) in connector.chars().enumerate() {
            if let Some(cell) = buf.cell_mut(Position::new(area.x + i as u16, y)) {
                cell.set_char(ch);
                cell.set_fg(Color::Rgb(32, 72, 96));
                cell.set_bg(bg_color());
            }
        }

        // Vertical connector line for non-last rows
        if !is_last && row_idx > 0 {
            // Draw │ above this row from previous connectors
        }
        // Draw │ on all rows above this for the tree trunk
        for prev_row in 0..row_idx {
            let prev_y = area.y + prev_row as u16;
            if prev_y < area.bottom() {
                if let Some(cell) = buf.cell_mut(Position::new(area.x, prev_y)) {
                    if cell.symbol() != "├" && cell.symbol() != "└" {
                        cell.set_char('│');
                        cell.set_fg(Color::Rgb(32, 72, 96));
                    }
                }
            }
        }

        // Mind name (columns 3-12)
        let name_start = 3usize;
        let name = mind.name;
        for (i, ch) in name.chars().enumerate() {
            let x = name_start + i;
            if x >= w { break; }
            if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, y)) {
                cell.set_char(ch);
                cell.set_fg(if mind.max_amplitude() > 0.1 { Color::Rgb(42, 180, 200) } else { Color::Rgb(64, 88, 112) });
                cell.set_bg(bg_color());
            }
        }

        // Sine wave (columns 13+)
        let wave_start = 13usize;
        let wave_w = w.saturating_sub(wave_start);
        if wave_w == 0 { continue; }

        // Map wave samples to the available width
        let wave_len = mind.wave.len();
        for wx in 0..wave_w {
            let x = wave_start + wx;
            if x >= w { break; }

            // Sample the wave at this position
            let sample_pos = (wx as f64 / wave_w as f64) * wave_len as f64;
            let idx = (sample_pos as usize).min(wave_len - 1);
            let displacement = mind.wave[idx]; // -2 to +2 range typically

            // Map displacement to a character
            // The wave occupies 1 row — we use characters to show vertical displacement
            let amp = displacement.abs();
            let intensity = (amp * 0.5).min(1.0);

            let ch = if amp < 0.05 {
                '─'  // flat string
            } else if amp < 0.3 {
                if displacement > 0.0 { '∿' } else { '∿' }
            } else if amp < 0.8 {
                if displacement > 0.0 { '╱' } else { '╲' }
            } else {
                if displacement > 0.0 { '▀' } else { '▄' }
            };

            let color = if intensity > 0.01 {
                intensity_color(intensity)
            } else {
                Color::Rgb(20, 40, 55) // dim flat string
            };

            if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, y)) {
                cell.set_char(ch);
                cell.set_fg(color);
                cell.set_bg(bg_color());
            }
        }
    }
}

// ─── RIGHT PANEL: Tool Activity ─────────────────────────────────────────

fn render_tool_panel(state: &LabState, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(20, 40, 55)))
        .title(Span::styled(" tools ", Style::default().fg(Color::Rgb(64, 88, 112))));
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width < 15 || inner.height < 3 { return; }

    // Sort tools by recency (most recent first)
    let mut sorted: Vec<(usize, &ToolEntry)> = state.tools.iter().enumerate().collect();
    sorted.sort_by(|a, b| b.1.last_called.partial_cmp(&a.1.last_called).unwrap_or(std::cmp::Ordering::Equal));

    let w = inner.width as usize;
    let name_w = 15.min(w / 2);
    let bar_w = (w - name_w - 6).max(2);

    for (row, (_, tool)) in sorted.iter().enumerate() {
        let y = inner.y + row as u16;
        if y >= inner.bottom() { break; }

        let age = (state.time - tool.last_called).max(0.0);
        let recency = if age > 120.0 { 0.0 } else { (1.0 - age / 120.0).max(0.0) };

        // Indicator
        let indicator = if age < 2.0 { "▸ " } else { "  " };
        let indicator_color = if age < 2.0 { Color::Rgb(42, 180, 200) } else { Color::Rgb(20, 40, 55) };

        // Name
        let name_color = if recency > 0.3 {
            intensity_color(recency)
        } else {
            Color::Rgb(48, 64, 80)
        };

        // Recency bar
        let bar_filled = (recency * bar_w as f64) as usize;
        let bar_color = intensity_color(recency);

        // Time since last call
        let time_str = if age > 999.0 { "   ·".to_string() }
            else if age > 60.0 { format!("{:>3.0}m", age / 60.0) }
            else { format!("{:>3.0}s", age) };

        // Render the row
        let mut x = inner.x;

        // Indicator
        for ch in indicator.chars() {
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char(ch);
                cell.set_fg(indicator_color);
                cell.set_bg(bg_color());
            }
            x += 1;
        }

        // Name (padded)
        let display_name = if tool.name.len() > name_w - 2 {
            &tool.name[..name_w - 2]
        } else {
            &tool.name
        };
        for ch in display_name.chars() {
            if x >= inner.right() { break; }
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char(ch);
                cell.set_fg(name_color);
                cell.set_bg(bg_color());
            }
            x += 1;
        }
        // Pad name
        while x < inner.x + 2 + name_w as u16 {
            if x >= inner.right() { break; }
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char(' ');
                cell.set_bg(bg_color());
            }
            x += 1;
        }

        // Bar
        for i in 0..bar_w {
            if x >= inner.right() { break; }
            let ch = if i < bar_filled { '█' } else { '░' };
            let c = if i < bar_filled { bar_color } else { Color::Rgb(10, 16, 24) };
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char(ch);
                cell.set_fg(c);
                cell.set_bg(bg_color());
            }
            x += 1;
        }

        // Time
        for ch in time_str.chars() {
            if x >= inner.right() { break; }
            if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                cell.set_char(ch);
                cell.set_fg(Color::Rgb(48, 64, 80));
                cell.set_bg(bg_color());
            }
            x += 1;
        }
    }

    // Footer: active/total count
    let footer_y = inner.bottom() - 1;
    if footer_y > inner.y + sorted.len() as u16 {
        let active = state.tools.iter().filter(|t| state.time - t.last_called < 120.0).count();
        let total = state.tools.len();
        let footer = format!("  {active}/{total} active");
        for (i, ch) in footer.chars().enumerate() {
            let x = inner.x + i as u16;
            if x >= inner.right() { break; }
            if let Some(cell) = buf.cell_mut(Position::new(x, footer_y)) {
                cell.set_char(ch);
                cell.set_fg(Color::Rgb(48, 64, 80));
                cell.set_bg(bg_color());
            }
        }
    }
}

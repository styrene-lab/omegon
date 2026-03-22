//! Fractal CIC instrument demo — four simultaneous displays with telemetry simulation.
//! Run: cargo run -p omegon --example fractal_demo
//!
//! Shows all four algorithms in a 2×2 grid at their real target size.
//! Each instrument has simulation buttons that drive realistic telemetry
//! scenarios so you can see how the fractals respond.
//!
//! Controls:
//!   Tab / BackTab  — select instrument (cycles through 4)
//!   ↑/↓            — select parameter within instrument
//!   ←/→            — adjust value (Shift=fine 1%)
//!   Space          — pause/resume all simulation
//!   r              — reset all telemetry to idle
//!   q              — quit
//!
//! Simulation triggers (number keys):
//!   1  — Context: fill 10%
//!   2  — Context: fill 50%
//!   3  — Context: fill 90% (near capacity)
//!   4  — Context: compaction (drops to 30%)
//!
//!   5  — Tools: single tool call
//!   6  — Tools: rapid burst (5 calls)
//!   7  — Tools: cleave (parallel execution)
//!   8  — Tools: tool error
//!
//!   9  — Thinking: extended thinking starts
//!   0  — Thinking: thinking completes
//!
//!   -  — Memory: large recall (inject many facts)
//!   =  — Memory: large write (store facts)
//!   [  — Memory: multi-mind activation
//!   ]  — Memory: compaction/cleanup

use std::io;
use std::time::{Duration, Instant};
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let start = Instant::now();
    let mut state = DemoState::default();

    loop {
        let t = start.elapsed().as_secs_f64();
        let dt = 0.033; // ~30fps

        // Update telemetry simulation
        if !state.paused {
            state.sim.update(dt);
        }

        terminal.draw(|f| {
            let area = f.area();
            let bg = Color::Rgb(0, 1, 3);
            let fg = Color::Rgb(196, 216, 228);
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    let cell = &mut f.buffer_mut()[(x, y)];
                    cell.set_bg(bg);
                    cell.set_fg(fg);
                }
            }

            let chunks = Layout::vertical([
                Constraint::Length(1),  // title
                Constraint::Min(16),   // 2x2 grid + sim panel
                Constraint::Length(3), // controls
            ]).split(area);

            // Title
            let title = format!(
                " CIC Instruments · {} · t={:.0}s",
                if state.paused { "PAUSED" } else { "LIVE" }, t,
            );
            f.render_widget(
                Paragraph::new(title).style(Style::default().fg(Color::Rgb(42, 180, 200)).add_modifier(Modifier::BOLD)),
                chunks[0],
            );

            // Main area: grid left, params+sim right
            let cols = Layout::horizontal([
                Constraint::Length(50), // instruments grid (2 × 24 + gap)
                Constraint::Min(35),   // params + simulation
            ]).split(chunks[1]);

            // 2x2 grid at real instrument size
            let grid_w = 24u16;
            let grid_h = 7u16;

            let instruments: [(&str, &str); 4] = [
                ("sonar", "context"),
                ("radar", "tools"),
                ("thermal", "thinking"),
                ("signal", "memory"),
            ];

            for (idx, (name, telemetry)) in instruments.iter().enumerate() {
                let gx = (idx % 2) as u16;
                let gy = (idx / 2) as u16;
                let inst_area = Rect {
                    x: cols[0].x + gx * grid_w + gx, // +gx for 1px gap
                    y: cols[0].y + gy * grid_h + gy,
                    width: grid_w,
                    height: grid_h,
                };
                if inst_area.right() > cols[0].right() || inst_area.bottom() > cols[0].bottom() {
                    continue;
                }

                let selected = idx == state.selected_instrument;
                let intensity = state.sim.intensity(idx);
                let border_color = if selected {
                    Color::Rgb(42, 180, 200)
                } else {
                    Color::Rgb(20, 40, 55)
                };

                // Intensity indicator in title
                let pct = (intensity * 100.0) as u32;
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        format!(" {} ({}) {}% ", name, telemetry, pct),
                        Style::default().fg(if selected { Color::Rgb(42, 180, 200) } else { Color::Rgb(64, 88, 112) }),
                    ));
                let inner = block.inner(inst_area);
                f.render_widget(block, inst_area);

                if inner.width >= 4 && inner.height >= 2 {
                    render_instrument(idx, t, intensity, inner, f.buffer_mut(), &state);
                }
            }

            // Right panel: params + simulation status + controls
            let right_chunks = Layout::vertical([
                Constraint::Length(10), // params
                Constraint::Length(1),  // separator
                Constraint::Length(6),  // simulation status
                Constraint::Length(1),  // separator
                Constraint::Min(5),    // simulation controls legend
            ]).split(cols[1]);

            // Parameter sliders
            let params = state.params_for(state.selected_instrument);
            let mut lines: Vec<Line<'_>> = vec![
                Line::from(Span::styled(
                    format!(" {} parameters", instruments[state.selected_instrument].0),
                    Style::default().fg(Color::Rgb(42, 180, 200)).add_modifier(Modifier::BOLD),
                )),
            ];
            for (i, (name, val, min, max)) in params.iter().enumerate() {
                let selected = i == state.selected_param;
                let pct = (val - min) / (max - min);
                let bar_w = 12;
                let filled = (pct * bar_w as f64) as usize;
                let bar: String = "█".repeat(filled) + &"░".repeat(bar_w - filled);
                let cursor = if selected { "▸ " } else { "  " };
                let ns = if selected {
                    Style::default().fg(Color::Rgb(42, 180, 200)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(96, 120, 136))
                };
                let bs = if selected {
                    Style::default().fg(Color::Rgb(42, 180, 200))
                } else {
                    Style::default().fg(Color::Rgb(32, 56, 72))
                };
                lines.push(Line::from(vec![
                    Span::styled(cursor, ns),
                    Span::styled(format!("{:<11}", name), ns),
                    Span::styled(format!("{:>6.2} ", val), Style::default().fg(Color::Rgb(196, 216, 228))),
                    Span::styled(bar, bs),
                ]));
            }
            f.render_widget(Paragraph::new(lines), right_chunks[0]);

            // Separator
            f.render_widget(
                Paragraph::new("─".repeat(right_chunks[1].width as usize))
                    .style(Style::default().fg(Color::Rgb(20, 40, 55))),
                right_chunks[1],
            );

            // Simulation status
            let s = &state.sim;
            let status_lines = vec![
                Line::from(Span::styled(" telemetry status", Style::default().fg(Color::Rgb(42, 180, 200)).add_modifier(Modifier::BOLD))),
                Line::from(vec![
                    Span::styled("  context  ", Style::default().fg(Color::Rgb(96, 120, 136))),
                    Span::styled(format!("{:>3}%", (s.context_fill * 100.0) as u32), Style::default().fg(
                        if s.context_fill > 0.8 { Color::Rgb(200, 100, 24) }
                        else if s.context_fill > 0.5 { Color::Rgb(42, 180, 200) }
                        else { Color::Rgb(96, 120, 136) }
                    )),
                    Span::styled(format!("  speed {:.2}", s.context_speed_mult), Style::default().fg(Color::Rgb(64, 88, 112))),
                ]),
                Line::from(vec![
                    Span::styled("  tools    ", Style::default().fg(Color::Rgb(96, 120, 136))),
                    Span::styled(format!("{}", s.tool_state_label()), Style::default().fg(
                        if s.tool_activity > 0.5 { Color::Rgb(42, 180, 200) } else { Color::Rgb(96, 120, 136) }
                    )),
                    Span::styled(format!("  burst {:.2}", s.tool_activity), Style::default().fg(Color::Rgb(64, 88, 112))),
                ]),
                Line::from(vec![
                    Span::styled("  thinking ", Style::default().fg(Color::Rgb(96, 120, 136))),
                    Span::styled(format!("{}", s.thinking_label()), Style::default().fg(
                        if s.thinking_level > 0.3 { Color::Rgb(42, 180, 200) } else { Color::Rgb(96, 120, 136) }
                    )),
                    Span::styled(format!("  depth {:.2}", s.thinking_level), Style::default().fg(Color::Rgb(64, 88, 112))),
                ]),
                Line::from(vec![
                    Span::styled("  memory   ", Style::default().fg(Color::Rgb(96, 120, 136))),
                    Span::styled(format!("{}", s.memory_label()), Style::default().fg(
                        if s.memory_activity > 0.3 { Color::Rgb(42, 180, 200) } else { Color::Rgb(96, 120, 136) }
                    )),
                    Span::styled(format!("  load {:.2}", s.memory_activity), Style::default().fg(Color::Rgb(64, 88, 112))),
                ]),
            ];
            f.render_widget(Paragraph::new(status_lines), right_chunks[2]);

            // Separator
            f.render_widget(
                Paragraph::new("─".repeat(right_chunks[3].width as usize))
                    .style(Style::default().fg(Color::Rgb(20, 40, 55))),
                right_chunks[3],
            );

            // Simulation controls legend
            let legend = vec![
                Line::from(Span::styled(" simulation triggers", Style::default().fg(Color::Rgb(42, 180, 200)).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled("  1 ctx 10%  2 ctx 50%  3 ctx 90%  4 compact", Style::default().fg(Color::Rgb(64, 88, 112)))),
                Line::from(Span::styled("  5 tool     6 burst    7 cleave   8 error", Style::default().fg(Color::Rgb(64, 88, 112)))),
                Line::from(Span::styled("  9 think    0 done     - recall   = write", Style::default().fg(Color::Rgb(64, 88, 112)))),
                Line::from(Span::styled("  [ multi-mind          ] cleanup", Style::default().fg(Color::Rgb(64, 88, 112)))),
            ];
            f.render_widget(Paragraph::new(legend), right_chunks[4]);

            // Bottom controls
            let controls = vec![
                Line::from(Span::styled(
                    " Tab=instrument  ↑↓=param  ←→=adjust (Shift=fine)  Space=pause  r=reset  q=quit",
                    Style::default().fg(Color::Rgb(64, 88, 112)),
                )),
                Line::from(Span::styled(
                    " Color ramp: ", Style::default().fg(Color::Rgb(64, 88, 112)),
                )),
            ];
            // Color ramp inline
            let ramp_y = chunks[2].y + 1;
            if ramp_y < area.bottom() {
                let ramp_area = Rect { x: chunks[2].x + 13, y: ramp_y, width: 40.min(area.width - 14), height: 1 };
                for i in 0..ramp_area.width as usize {
                    let t = i as f64 / ramp_area.width as f64;
                    let c = intensity_color(t);
                    if let Some(cell) = f.buffer_mut().cell_mut(Position::new(ramp_area.x + i as u16, ramp_area.y)) {
                        cell.set_char('█');
                        cell.set_fg(c);
                        cell.set_bg(bg);
                    }
                }
            }
            f.render_widget(Paragraph::new(controls), chunks[2]);
        })?;

        if event::poll(Duration::from_millis(33))? {
            if let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? {
                let fine = modifiers.contains(KeyModifiers::SHIFT);
                match code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab => { state.selected_instrument = (state.selected_instrument + 1) % 4; state.selected_param = 0; }
                    KeyCode::BackTab => { state.selected_instrument = (state.selected_instrument + 3) % 4; state.selected_param = 0; }
                    KeyCode::Up => { let n = state.params_for(state.selected_instrument).len(); state.selected_param = (state.selected_param + n - 1) % n; }
                    KeyCode::Down => { let n = state.params_for(state.selected_instrument).len(); state.selected_param = (state.selected_param + 1) % n; }
                    KeyCode::Left => state.adjust(-1.0, fine),
                    KeyCode::Right => state.adjust(1.0, fine),
                    KeyCode::Char(' ') => state.paused = !state.paused,
                    KeyCode::Char('r') => state.sim = TelemetrySim::default(),
                    // Context scenarios
                    KeyCode::Char('1') => state.sim.set_context(0.10),
                    KeyCode::Char('2') => state.sim.set_context(0.50),
                    KeyCode::Char('3') => state.sim.set_context(0.90),
                    KeyCode::Char('4') => state.sim.compaction(),
                    // Tool scenarios
                    KeyCode::Char('5') => state.sim.tool_call(),
                    KeyCode::Char('6') => state.sim.tool_burst(),
                    KeyCode::Char('7') => state.sim.tool_cleave(),
                    KeyCode::Char('8') => state.sim.tool_error(),
                    // Thinking scenarios
                    KeyCode::Char('9') => state.sim.thinking_start(),
                    KeyCode::Char('0') => state.sim.thinking_stop(),
                    // Memory scenarios
                    KeyCode::Char('-') => state.sim.memory_recall(),
                    KeyCode::Char('=') => state.sim.memory_write(),
                    KeyCode::Char('[') => state.sim.memory_multi_mind(),
                    KeyCode::Char(']') => state.sim.memory_cleanup(),
                    _ => {}
                }
            }
        }
    }

    terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

// ─── Telemetry simulation ───────────────────────────────────────────────

struct TelemetrySim {
    // Context: 0.0 = empty, 1.0 = full
    context_fill: f64,
    context_target: f64,
    context_speed_mult: f64,
    // Tools: 0.0 = idle, 1.0 = max activity. Decays over time.
    tool_activity: f64,
    tool_state: ToolState,
    // Thinking: 0.0 = idle, 1.0 = deep extended thinking
    thinking_level: f64,
    thinking_target: f64,
    // Memory: 0.0 = idle, 1.0 = heavy activity. Decays.
    memory_activity: f64,
    memory_state: MemoryState,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ToolState { #[default] Idle, Single, Burst, Cleave, Error }

#[derive(Default, Clone, Copy, PartialEq)]
enum MemoryState { #[default] Idle, Recall, Write, MultiMind, Cleanup }

impl Default for TelemetrySim {
    fn default() -> Self {
        Self {
            context_fill: 0.0, context_target: 0.0, context_speed_mult: 1.0,
            tool_activity: 0.0, tool_state: ToolState::Idle,
            thinking_level: 0.0, thinking_target: 0.0,
            memory_activity: 0.0, memory_state: MemoryState::Idle,
        }
    }
}

impl TelemetrySim {
    fn update(&mut self, dt: f64) {
        // Context: smooth approach to target
        self.context_fill += (self.context_target - self.context_fill) * dt * 2.0;
        self.context_speed_mult = 0.3 + self.context_fill * 2.5; // faster when fuller

        // Tools: decay toward 0
        self.tool_activity = (self.tool_activity - dt * 0.8).max(0.0);
        if self.tool_activity < 0.05 { self.tool_state = ToolState::Idle; }

        // Thinking: smooth approach to target
        self.thinking_level += (self.thinking_target - self.thinking_level) * dt * 3.0;

        // Memory: decay toward 0
        self.memory_activity = (self.memory_activity - dt * 0.5).max(0.0);
        if self.memory_activity < 0.05 { self.memory_state = MemoryState::Idle; }
    }

    fn intensity(&self, instrument: usize) -> f64 {
        match instrument {
            0 => self.context_fill,          // sonar = context fill level
            1 => self.tool_activity,          // radar = tool activity
            2 => self.thinking_level,         // thermal = thinking depth
            3 => self.memory_activity,        // signal = memory activity
            _ => 0.0,
        }
    }

    fn speed_mult(&self, instrument: usize) -> f64 {
        match instrument {
            0 => self.context_speed_mult,
            1 => 0.3 + self.tool_activity * 3.0,
            2 => 0.2 + self.thinking_level * 2.0,
            3 => 0.3 + self.memory_activity * 2.0,
            _ => 1.0,
        }
    }

    // Context scenarios
    fn set_context(&mut self, fill: f64) { self.context_target = fill.clamp(0.0, 1.0); }
    fn compaction(&mut self) { self.context_target = 0.30; self.context_fill = self.context_fill.max(0.5); }

    // Tool scenarios
    fn tool_call(&mut self)  { self.tool_activity = 0.4; self.tool_state = ToolState::Single; }
    fn tool_burst(&mut self) { self.tool_activity = 0.75; self.tool_state = ToolState::Burst; }
    fn tool_cleave(&mut self){ self.tool_activity = 1.0; self.tool_state = ToolState::Cleave; }
    fn tool_error(&mut self) { self.tool_activity = 0.6; self.tool_state = ToolState::Error; }

    // Thinking scenarios
    fn thinking_start(&mut self) { self.thinking_target = 0.85; }
    fn thinking_stop(&mut self)  { self.thinking_target = 0.0; }

    // Memory scenarios
    fn memory_recall(&mut self)     { self.memory_activity = 0.7; self.memory_state = MemoryState::Recall; }
    fn memory_write(&mut self)      { self.memory_activity = 0.6; self.memory_state = MemoryState::Write; }
    fn memory_multi_mind(&mut self) { self.memory_activity = 0.9; self.memory_state = MemoryState::MultiMind; }
    fn memory_cleanup(&mut self)    { self.memory_activity = 0.5; self.memory_state = MemoryState::Cleanup; }

    fn tool_state_label(&self) -> &str {
        match self.tool_state {
            ToolState::Idle => "idle",
            ToolState::Single => "single call",
            ToolState::Burst => "rapid burst",
            ToolState::Cleave => "CLEAVE",
            ToolState::Error => "ERROR",
        }
    }
    fn thinking_label(&self) -> &str {
        if self.thinking_level > 0.6 { "extended" }
        else if self.thinking_level > 0.2 { "active" }
        else { "idle" }
    }
    fn memory_label(&self) -> &str {
        match self.memory_state {
            MemoryState::Idle => "idle",
            MemoryState::Recall => "recalling",
            MemoryState::Write => "writing",
            MemoryState::MultiMind => "multi-mind",
            MemoryState::Cleanup => "cleanup",
        }
    }
}

// ─── Color ramp: navy → teal → amber ───────────────────────────────────

fn intensity_color(intensity: f64) -> Color {
    let i = intensity.clamp(0.0, 1.0);
    if i < 0.5 {
        let t = i / 0.5;
        let r = (t * 2.0) as u8;
        let g = (4.0 + t * 36.0) as u8;
        let b = (8.0 + t * 32.0) as u8;
        Color::Rgb(r, g, b)
    } else {
        let t = (i - 0.5) / 0.5;
        let r = (2.0 + t * 68.0) as u8;
        let g = (40.0 + t * 20.0) as u8;
        let b = (40.0 - t * 28.0) as u8;
        Color::Rgb(r, g, b)
    }
}

fn bg_color() -> Color { Color::Rgb(0, 1, 3) }

fn pixel_color(value: f64, intensity: f64) -> Color {
    let v = value.clamp(0.0, 1.0);
    if v < 0.01 { return bg_color(); }
    intensity_color(v * intensity)
}

fn pixel_color_floor(value: f64, intensity: f64, floor: f64) -> Color {
    let v = value.clamp(0.0, 1.0);
    if v < 0.01 { return bg_color(); }
    let effective = (v * intensity).max(v * floor);
    intensity_color(effective)
}

// ─── Instrument rendering ──────────────────────────────────────────────

fn render_instrument(idx: usize, time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState) {
    let speed = s.sim.speed_mult(idx);
    match idx {
        0 => render_perlin(time * speed, intensity, area, buf, s),
        1 => render_lissajous(time * speed, intensity, area, buf, s),
        2 => render_plasma(time * speed, intensity, area, buf, s),
        3 => render_attractor(time * speed, intensity, area, buf, s),
        _ => {}
    }
}

fn set_halfblock(buf: &mut Buffer, area: Rect, px: usize, row: usize, top: Color, bot: Color) {
    if let Some(cell) = buf.cell_mut(Position::new(area.x + px as u16, area.y + row as u16)) {
        cell.set_char('▀');
        cell.set_fg(top);
        cell.set_bg(bot);
    }
}

// ─── Perlin (sonar — context health) ────────────────────────────────────

fn render_perlin(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            let top = noise_octaves(px as f64 / s.perlin_scale, py as f64 / s.perlin_scale,
                                     time, s.perlin_octaves as usize, s.perlin_lacunarity);
            let bot = noise_octaves(px as f64 / s.perlin_scale, (py+1) as f64 / s.perlin_scale,
                                     time, s.perlin_octaves as usize, s.perlin_lacunarity);
            let tc = pixel_color((top * 0.5 + 0.5) * s.perlin_amplitude, intensity);
            let bc = pixel_color((bot * 0.5 + 0.5) * s.perlin_amplitude, intensity);
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

fn noise_octaves(x: f64, y: f64, z: f64, octaves: usize, lacunarity: f64) -> f64 {
    let mut val = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut total_amp = 0.0;
    for _ in 0..octaves.max(1) {
        val += noise_sample(x * freq, y * freq, z) * amp;
        total_amp += amp;
        amp *= 0.5;
        freq *= lacunarity;
    }
    val / total_amp
}

fn noise_sample(x: f64, y: f64, z: f64) -> f64 {
    let v1 = (x * 1.3 + z).sin() * (y * 0.7 + z * 0.5).cos();
    let v2 = ((x + y) * 0.8 - z * 0.3).sin();
    let v3 = (x * 2.1 - z * 0.7).cos() * (y * 1.5 + z * 0.4).sin();
    (v1 + v2 + v3) / 3.0
}

// ─── Plasma (thermal — thinking state) ──────────────────────────────────

fn render_plasma(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            let top = plasma_sample(px as f64, py as f64, time, s);
            let bot = plasma_sample(px as f64, (py+1) as f64, time, s);
            let tc = pixel_color((top * 0.5 + 0.5) * s.plasma_amplitude, intensity);
            let bc = pixel_color((bot * 0.5 + 0.5) * s.plasma_amplitude, intensity);
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

fn plasma_sample(x: f64, y: f64, t: f64, s: &DemoState) -> f64 {
    let c = s.plasma_complexity;
    let sp = t;
    let v1 = (x / (6.0 / c) + sp).sin();
    let v2 = ((y / (4.0 / c) + sp * 0.7).sin() + (x / (8.0 / c)).cos()).sin();
    let v3 = ((x * x + y * y).sqrt() * s.plasma_distortion / (6.0 / c) - sp * 1.3).sin();
    let v4 = (x / (3.0 / c) - sp * 0.5).cos() * (y / (5.0 / c) + sp * 0.9).sin();
    (v1 + v2 + v3 + v4) / 4.0
}

// ─── Lissajous (radar — tool activity) ──────────────────────────────────

fn render_lissajous(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    let mut grid = vec![0u32; w * h];
    let nc = s.liss_num_curves as usize;
    let pts = s.liss_points as usize;

    for curve in 0..nc {
        let fx = s.liss_freq_base + curve as f64 * s.liss_freq_spread / nc.max(1) as f64;
        let fy = s.liss_freq_base + 1.0 + curve as f64 * (s.liss_freq_spread * 0.8) / nc.max(1) as f64;
        let phase = time * (1.0 + curve as f64 * 0.03);
        for i in 0..pts {
            let t = i as f64 / pts as f64 * std::f64::consts::TAU;
            let x = (fx * t + phase).sin();
            let y = (fy * t + phase * 0.3).cos();
            let gx = ((x * s.liss_amplitude + 0.5) * w as f64) as usize;
            let gy = ((y * s.liss_amplitude + 0.5) * h as f64) as usize;
            if gx < w && gy < h { grid[gy * w + gx] += 1; }
        }
    }

    let max_hits = (*grid.iter().max().unwrap_or(&1)).max(1) as f64;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            let top_v = (grid[py * w + px] as f64 / max_hits).min(1.0);
            let bot_v = if py+1 < h { (grid[(py+1) * w + px] as f64 / max_hits).min(1.0) } else { 0.0 };
            let tc = pixel_color_floor(top_v, intensity, 0.25);
            let bc = pixel_color_floor(bot_v, intensity, 0.25);
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

// ─── Clifford attractor (signal — memory activity) ──────────────────────

fn render_attractor(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    let mut grid = vec![0u32; w * h];

    let phase = (time * s.attr_evolve_speed).sin() * 0.5 + 0.5;
    let a = s.attr_a + phase * 0.2;
    let b = s.attr_b + (1.0 - phase) * 0.15;
    let c = 1.0 + phase * 0.3;
    let d = 0.7 + (1.0 - phase) * 0.2;

    let iters = s.attr_iterations as usize;
    let spread = s.attr_spread;
    let mut x = 0.1_f64;
    let mut y = 0.1_f64;
    for _ in 0..iters {
        let nx = (a * y).sin() + c * (a * x).cos();
        let ny = (b * x).sin() + d * (b * y).cos();
        x = nx; y = ny;
        let gx = ((x + spread / 2.0) / spread * w as f64) as usize;
        let gy = ((y + spread / 2.0) / spread * h as f64) as usize;
        if gx < w && gy < h { grid[gy * w + gx] += 1; }
    }

    let max_hits = (*grid.iter().max().unwrap_or(&1)).max(1) as f64;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            let top_v = (grid[py * w + px] as f64 / max_hits).powf(s.attr_gamma);
            let bot_v = if py+1 < h { (grid[(py+1) * w + px] as f64 / max_hits).powf(s.attr_gamma) } else { 0.0 };
            let tc = pixel_color_floor(top_v, intensity, 0.2);
            let bc = pixel_color_floor(bot_v, intensity, 0.2);
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

// ─── State ──────────────────────────────────────────────────────────────

struct DemoState {
    selected_instrument: usize,
    selected_param: usize,
    paused: bool,
    sim: TelemetrySim,
    // Perlin (sonar)
    perlin_scale: f64,
    perlin_octaves: f64,
    perlin_lacunarity: f64,
    perlin_amplitude: f64,
    // Plasma (thermal)
    plasma_complexity: f64,
    plasma_distortion: f64,
    plasma_amplitude: f64,
    // Lissajous (radar)
    liss_num_curves: f64,
    liss_freq_base: f64,
    liss_freq_spread: f64,
    liss_amplitude: f64,
    liss_points: f64,
    // Clifford (signal)
    attr_iterations: f64,
    attr_evolve_speed: f64,
    attr_a: f64,
    attr_b: f64,
    attr_spread: f64,
    attr_gamma: f64,
}

impl Default for DemoState {
    fn default() -> Self {
        Self {
            selected_instrument: 0, selected_param: 0, paused: false,
            sim: TelemetrySim::default(),
            perlin_scale: 18.0, perlin_octaves: 2.0,
            perlin_lacunarity: 2.3, perlin_amplitude: 0.5,
            plasma_complexity: 1.65, plasma_distortion: 0.8, plasma_amplitude: 0.88,
            liss_num_curves: 8.0, liss_freq_base: 1.9,
            liss_freq_spread: 1.86, liss_amplitude: 0.50, liss_points: 5375.0,
            attr_iterations: 12000.0, attr_evolve_speed: 0.03, attr_a: -1.4,
            attr_b: 1.6, attr_spread: 5.0, attr_gamma: 0.45,
        }
    }
}

impl DemoState {
    fn params_for(&self, instrument: usize) -> Vec<(&str, f64, f64, f64)> {
        match instrument {
            0 => vec![
                ("scale", self.perlin_scale, 4.0, 30.0),
                ("octaves", self.perlin_octaves, 1.0, 4.0),
                ("lacunarity", self.perlin_lacunarity, 1.0, 4.0),
                ("amplitude", self.perlin_amplitude, 0.1, 1.0),
            ],
            1 => vec![
                ("curves", self.liss_num_curves, 1.0, 12.0),
                ("freq_base", self.liss_freq_base, 1.0, 7.0),
                ("freq_spread", self.liss_freq_spread, 0.1, 3.0),
                ("amplitude", self.liss_amplitude, 0.15, 0.5),
                ("points", self.liss_points, 500.0, 8000.0),
            ],
            2 => vec![
                ("complexity", self.plasma_complexity, 0.3, 3.0),
                ("distortion", self.plasma_distortion, 0.0, 1.5),
                ("amplitude", self.plasma_amplitude, 0.1, 1.0),
            ],
            3 => vec![
                ("iterations", self.attr_iterations, 2000.0, 32000.0),
                ("evolve", self.attr_evolve_speed, 0.005, 0.1),
                ("a", self.attr_a, -2.0, -0.5),
                ("b", self.attr_b, 1.0, 2.0),
                ("spread", self.attr_spread, 3.0, 8.0),
                ("gamma", self.attr_gamma, 0.2, 1.0),
            ],
            _ => vec![],
        }
    }

    fn adjust(&mut self, dir: f64, fine: bool) {
        let params = self.params_for(self.selected_instrument);
        if self.selected_param >= params.len() { return; }
        let (_, val, min, max) = params[self.selected_param];
        let range = max - min;
        let step = if fine { range * 0.01 } else { range * 0.05 };
        let new_val = (val + dir * step).clamp(min, max);

        match self.selected_instrument {
            0 => match self.selected_param {
                0 => self.perlin_scale = new_val,
                1 => self.perlin_octaves = new_val,
                2 => self.perlin_lacunarity = new_val,
                3 => self.perlin_amplitude = new_val,
                _ => {}
            },
            1 => match self.selected_param {
                0 => self.liss_num_curves = new_val,
                1 => self.liss_freq_base = new_val,
                2 => self.liss_freq_spread = new_val,
                3 => self.liss_amplitude = new_val,
                4 => self.liss_points = new_val,
                _ => {}
            },
            2 => match self.selected_param {
                0 => self.plasma_complexity = new_val,
                1 => self.plasma_distortion = new_val,
                2 => self.plasma_amplitude = new_val,
                _ => {}
            },
            3 => match self.selected_param {
                0 => self.attr_iterations = new_val,
                1 => self.attr_evolve_speed = new_val,
                2 => self.attr_a = new_val,
                3 => self.attr_b = new_val,
                4 => self.attr_spread = new_val,
                5 => self.attr_gamma = new_val,
                _ => {}
            },
            _ => {}
        }
    }
}

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

            // Tick waterfall — memory state selects CA rule, activity drives density + speed
            let mem_intensity = state.sim.memory_activity;
            let density = state.ca_density + mem_intensity * 0.25; // aggressive density boost
            let scroll = state.ca_scroll_rate * (0.5 + mem_intensity * 1.5);
            let rule = state.sim.memory_rule(); // Rule 204 idle, 30/110/90/150 per state
            let fade = state.ca_fade;
            state.waterfall.ensure_size(22, 5);
            state.waterfall.tick(dt, scroll, density, rule, fade);
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
                ("waterfall", "memory"),
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
                // Error state on the radar instrument → red border
                let is_error = idx == 1 && state.sim.tool_state == ToolState::Error;
                let border_color = if is_error {
                    Color::Rgb(224, 72, 72)  // theme error red
                } else if selected {
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
    /// Hue override for tool states: 0.0 = use normal ramp, >0 = shift toward amber/red
    tool_hue_override: Option<[u8; 3]>, // Some((r,g,b)) for error red, None for normal ramp
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
            tool_activity: 0.0, tool_state: ToolState::Idle, tool_hue_override: None,
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

        // Tools: decay rate varies by state
        let tool_decay = match self.tool_state {
            ToolState::Idle => 1.0,
            ToolState::Single => 0.6,   // quick decay
            ToolState::Burst => 0.4,    // medium sustain
            ToolState::Cleave => 0.2,   // long sustain — parallel work
            ToolState::Error => 0.3,    // medium sustain for visibility
        };
        self.tool_activity = (self.tool_activity - dt * tool_decay).max(0.0);
        if self.tool_activity < 0.05 {
            self.tool_state = ToolState::Idle;
            self.tool_hue_override = None;
        }

        // Thinking: smooth approach to target
        self.thinking_level += (self.thinking_target - self.thinking_level) * dt * 3.0;

        // Memory: decay toward 0
        self.memory_activity = (self.memory_activity - dt * 0.5).max(0.0);
        if self.memory_activity < 0.05 { self.memory_state = MemoryState::Idle; }
    }

    fn intensity(&self, instrument: usize) -> f64 {
        match instrument {
            // Context caps at 0.7 — auto-compaction fires at ~70%,
            // so that's our visual maximum. Full amber = about to compact.
            0 => (self.context_fill / 0.7).min(1.0),
            1 => self.tool_activity,          // radar = tool activity
            2 => self.thinking_level,         // thermal = thinking depth
            3 => self.memory_activity,        // signal = memory activity
            _ => 0.0,
        }
    }

    fn speed_mult(&self, instrument: usize) -> f64 {
        match instrument {
            0 => self.context_speed_mult,
            1 => match self.tool_state {
                ToolState::Idle => 0.3,
                ToolState::Single => 0.5,
                ToolState::Burst => 0.8,
                ToolState::Cleave => 1.2,
                ToolState::Error => 0.15,    // SLOW — ominous
            },
            2 => {
                // Thinking: color leads, speed follows.
                // Color (intensity) ramps linearly. Speed uses a squared
                // curve — stays slow during ignition, then accelerates
                // once the color is already established.
                let level = self.thinking_level;
                0.2 + level * level * 2.0   // quadratic: slow start, fast finish
            },
            3 => 1.0, // waterfall scroll is handled separately
            _ => 1.0,
        }
    }

    // Context scenarios
    fn set_context(&mut self, fill: f64) { self.context_target = fill.clamp(0.0, 1.0); }
    fn compaction(&mut self) { self.context_target = 0.30; self.context_fill = self.context_fill.max(0.5); }

    // Tool scenarios — each has distinct intensity + color behavior
    fn tool_call(&mut self) {
        self.tool_activity = 0.45;
        self.tool_state = ToolState::Single;
        self.tool_hue_override = None; // normal teal ramp
    }
    fn tool_burst(&mut self) {
        self.tool_activity = 0.75;
        self.tool_state = ToolState::Burst;
        self.tool_hue_override = None; // pushed toward amber by intensity alone
    }
    fn tool_cleave(&mut self) {
        self.tool_activity = 1.0;
        self.tool_state = ToolState::Cleave;
        self.tool_hue_override = None; // full amber via max intensity + slow decay
    }
    fn tool_error(&mut self) {
        self.tool_activity = 0.85;
        self.tool_state = ToolState::Error;
        // Error uses normal amber color (high intensity) — the RED comes
        // from the border, not the fractal body. Red body was invisible
        // in the center against the dark background.
        self.tool_hue_override = None;
    }

    // Thinking scenarios
    fn thinking_start(&mut self) { self.thinking_target = 0.85; }
    fn thinking_stop(&mut self)  { self.thinking_target = 0.0; }

    // Memory scenarios — each state selects a different CA rule.
    // Pushed hard into the amber zone so they pop against the dim idle.
    fn memory_recall(&mut self)     { self.memory_activity = 0.95; self.memory_state = MemoryState::Recall; }
    fn memory_write(&mut self)      { self.memory_activity = 0.85; self.memory_state = MemoryState::Write; }
    fn memory_multi_mind(&mut self) { self.memory_activity = 1.0;  self.memory_state = MemoryState::MultiMind; }
    fn memory_cleanup(&mut self)    { self.memory_activity = 0.7;  self.memory_state = MemoryState::Cleanup; }

    /// Get the CA rule for the current memory state.
    fn memory_rule(&self) -> u8 {
        match self.memory_state {
            MemoryState::Idle    => 204, // identity — bars stay still
            MemoryState::Recall  => 30,  // chaotic cascade — information flowing
            MemoryState::Write   => 110, // complex structured growth
            MemoryState::MultiMind => 90, // Sierpinski branching — systems linking
            MemoryState::Cleanup => 150, // structured chaos → order
        }
    }

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

/// Perceptually uniform intensity → color mapping.
///
/// Human vision follows a power law (Stevens/CIE L*): we're far more
/// sensitive to changes in dark values than bright ones. A linear ramp
/// looks "stuck in navy" at the bottom and "jumps to amber" at the top.
///
/// Fix: apply CIE L* perceptual linearization (cube root) before mapping
/// to the color gradient. This makes equal numeric steps FEEL like equal
/// visual steps across the full range.
fn intensity_color(intensity: f64) -> Color {
    let linear = intensity.clamp(0.0, 1.0);

    // CIE L* perceptual linearization (cube root for values > threshold,
    // linear segment near zero to avoid infinite slope)
    let i = if linear > 0.008856 {
        linear.cbrt()
    } else {
        linear * 7.787 + 16.0 / 116.0
    };
    // Rescale to 0..1
    let i = ((i - 0.138) / (1.0 - 0.138)).clamp(0.0, 1.0);

    // Three-stop gradient — amber gets much more range:
    //   0.0 → 0.3:  dark navy → dim teal (idle zone)
    //   0.3 → 0.5:  teal (brand center, narrow — this is "normal")
    //   0.5 → 1.0:  teal → amber → hot amber (HALF the range is the hot zone)
    if i < 0.3 {
        let t = i / 0.3;
        let r = (1.0 + t * 3.0) as u8;            // 1 → 4
        let g = (4.0 + t * 34.0) as u8;           // 4 → 38
        let b = (6.0 + t * 30.0) as u8;           // 6 → 36
        Color::Rgb(r, g, b)
    } else if i < 0.5 {
        let t = (i - 0.3) / 0.2;
        let r = (4.0 + t * 4.0) as u8;            // 4 → 8
        let g = (38.0 + t * 10.0) as u8;          // 38 → 48
        let b = (36.0 + t * 6.0) as u8;           // 36 → 42
        Color::Rgb(r, g, b)
    } else {
        // HALF the perceptual range is teal→amber. This gives amber
        // enough room to actually register as amber, not a sliver.
        let t = (i - 0.5) / 0.5;
        let r = (8.0 + t * 82.0) as u8;           // 8 → 90
        let g = (48.0 - t * 2.0) as u8;           // 48 → 46
        let b = (42.0 - t * 34.0) as u8;          // 42 → 8
        Color::Rgb(r, g, b)
    }
}

fn bg_color() -> Color { Color::Rgb(0, 1, 3) }

fn pixel_color(value: f64, intensity: f64) -> Color {
    let v = value.clamp(0.0, 1.0);
    if v < 0.01 { return bg_color(); }
    intensity_color(v * intensity)
}

fn pixel_color_floor_hue(value: f64, intensity: f64, floor: f64, hue_override: Option<[u8; 3]>) -> Color {
    let v = value.clamp(0.0, 1.0);
    if v < 0.01 { return bg_color(); }
    let effective = (v * intensity).max(v * floor);
    match hue_override {
        Some([r, g, b]) => {
            // Blend: scale the override color by effective intensity
            let e = effective.clamp(0.0, 1.0);
            Color::Rgb(
                (r as f64 * e * 0.4) as u8,  // subdued — not full blast
                (g as f64 * e * 0.3) as u8,
                (b as f64 * e * 0.3) as u8,
            )
        }
        None => intensity_color(effective),
    }
}

fn pixel_color_hue(value: f64, intensity: f64, hue_override: Option<[u8; 3]>) -> Color {
    let v = value.clamp(0.0, 1.0);
    if v < 0.01 { return bg_color(); }
    match hue_override {
        Some([r, g, b]) => {
            let e = (v * intensity).clamp(0.0, 1.0);
            Color::Rgb(
                (r as f64 * e * 0.4) as u8,
                (g as f64 * e * 0.3) as u8,
                (b as f64 * e * 0.3) as u8,
            )
        }
        None => intensity_color(v * intensity),
    }
}

// ─── Instrument rendering ──────────────────────────────────────────────

fn render_instrument(idx: usize, time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState) {
    let speed = s.sim.speed_mult(idx);
    let hue_override = if idx == 1 { s.sim.tool_hue_override } else { None };
    match idx {
        0 => render_perlin(time * speed, intensity, area, buf, s, None),
        1 => render_lissajous(time * speed, intensity, area, buf, s, hue_override),
        2 => render_plasma(time * speed, intensity, area, buf, s, None),
        3 => render_waterfall(intensity, area, buf, &s.waterfall),
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

fn render_perlin(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState, hue_override: Option<[u8; 3]>) {
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
            let tc = pixel_color_hue((top * 0.5 + 0.5) * s.perlin_amplitude, intensity, hue_override);
            let bc = pixel_color_hue((bot * 0.5 + 0.5) * s.perlin_amplitude, intensity, hue_override);
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

fn render_plasma(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState, _hue_override: Option<[u8; 3]>) {
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

fn render_lissajous(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, s: &DemoState, hue_override: Option<[u8; 3]>) {
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
            let tc = pixel_color_floor_hue(top_v, intensity, 0.25, hue_override);
            let bc = pixel_color_floor_hue(bot_v, intensity, 0.25, hue_override);
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

// ─── CA waterfall (signal — memory activity) ────────────────────────────
//
// A 1D cellular automaton runs across the width. Each tick, rows scroll
// up and a new row is computed from the bottom row using a Wolfram-style
// rule. The rule number and birth density are driven by memory telemetry.
//
// Idle:   sparse random births, simple rule → dim scattered dots
// Active: dense births, complex rule → structured patterns flowing upward
// The waterfall has HISTORY — you see recent activity trailing up.

/// Persistent waterfall state — lives across frames.
struct WaterfallState {
    /// 2D grid of cell values (0.0 = dead, 1.0 = alive, with fade)
    grid: Vec<f64>,
    width: usize,
    height: usize, // in half-block pixels
    /// Accumulator for scroll timing
    scroll_accum: f64,
    /// Simple RNG state
    rng: u64,
}

impl WaterfallState {
    fn new(w: usize, h: usize) -> Self {
        Self {
            grid: vec![0.0; w * h],
            width: w,
            height: h,
            scroll_accum: 0.0,
            rng: 0xdeadbeef_u64,
        }
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    /// Advance the waterfall: scroll up, compute new bottom row.
    /// `scroll_rate`: rows per second. `density`: 0-1 birth probability.
    /// `rule`: Wolfram rule number (0-255). `fade`: per-scroll decay.
    fn tick(&mut self, dt: f64, scroll_rate: f64, density: f64, rule: u8, fade: f64) {
        self.scroll_accum += dt * scroll_rate;

        while self.scroll_accum >= 1.0 {
            self.scroll_accum -= 1.0;
            let w = self.width;
            let h = self.height;

            // Scroll up: shift all rows up by one
            for y in 0..(h - 1) {
                for x in 0..w {
                    self.grid[y * w + x] = self.grid[(y + 1) * w + x] * fade;
                }
            }

            // Compute new bottom row from CA rule applied to previous bottom
            let prev_row = h - 2;
            let new_row = h - 1;
            for x in 0..w {
                let left = if x > 0 { (self.grid[prev_row * w + x - 1] > 0.3) as u8 } else { 0 };
                let center = (self.grid[prev_row * w + x] > 0.3) as u8;
                let right = if x + 1 < w { (self.grid[prev_row * w + x + 1] > 0.3) as u8 } else { 0 };
                let neighborhood = (left << 2) | (center << 1) | right;
                let alive = (rule >> neighborhood) & 1 == 1;

                // Random births based on density
                let random_birth = (self.next_rand() % 1000) < (density * 1000.0) as u64;

                self.grid[new_row * w + x] = if alive || random_birth { 1.0 } else { 0.0 };
            }
        }
    }

    /// Resize if needed (when the instrument area changes).
    fn ensure_size(&mut self, w: usize, h: usize) {
        if self.width != w || self.height != h {
            self.grid = vec![0.0; w * h];
            self.width = w;
            self.height = h;
        }
    }
}

/// CRT noise glyphs — same character set as the splash screen glitch effect.
/// Ordered roughly by visual density: sparse → dense.
const NOISE_CHARS: &[char] = &[
    // Light — idle/sparse
    '▏', '▎', '▍', '░',
    // Medium — active
    '▌', '▐', '▒', '┤', '├', '│', '─',
    // Heavy — intense
    '▊', '▋', '▓', '╱', '╲', '┼', '╪', '╫',
    // Full — maximum
    '█', '╬', '■', '◆',
];

/// Render waterfall using CRT noise glyphs — the splash screen's glitch
/// character set applied as a scrolling signal display. Each cell picks
/// a glyph based on its value (density) and a pseudo-random hash (variety).
/// Color carries the intensity ramp as usual.
fn render_waterfall(intensity: f64, area: Rect, buf: &mut Buffer, wf: &WaterfallState) {
    for cy in 0..area.height as usize {
        for cx in 0..area.width as usize {
            let val = if cx < wf.width && cy < wf.height {
                wf.grid[cy * wf.width + cx]
            } else {
                0.0
            };

            if val < 0.05 {
                // Dead cell — background
                if let Some(cell) = buf.cell_mut(Position::new(area.x + cx as u16, area.y + cy as u16)) {
                    cell.set_char(' ');
                    cell.set_fg(bg_color());
                    cell.set_bg(bg_color());
                }
                continue;
            }

            // Pick glyph: value determines density tier, position adds variety
            let hash = ((cx * 7 + cy * 13 + (val * 100.0) as usize) * 31) % NOISE_CHARS.len();
            let tier = ((val * (NOISE_CHARS.len() - 1) as f64) as usize).min(NOISE_CHARS.len() - 1);
            // Blend tier and hash for variety within the density band
            let idx = (tier / 2 + hash / 2).min(NOISE_CHARS.len() - 1);
            let ch = NOISE_CHARS[idx];

            // Floor is very low — idle waterfall is nearly invisible,
            // bursts pop with color contrast
            let effective = (val * intensity).max(val * 0.08);
            let color = intensity_color(effective);

            if let Some(cell) = buf.cell_mut(Position::new(area.x + cx as u16, area.y + cy as u16)) {
                cell.set_char(ch);
                cell.set_fg(color);
                cell.set_bg(bg_color());
            }
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
    // CA Waterfall (signal)
    waterfall: WaterfallState,
    ca_scroll_rate: f64,  // rows per second
    ca_density: f64,      // birth probability 0-1
    ca_rule: f64,         // Wolfram rule number (cast to u8)
    ca_fade: f64,         // per-scroll brightness decay
}

impl Default for DemoState {
    fn default() -> Self {
        Self {
            selected_instrument: 0, selected_param: 0, paused: false,
            sim: TelemetrySim::default(),
            // Sonar — operator tuned
            perlin_scale: 7.9, perlin_octaves: 2.5,
            perlin_lacunarity: 4.0, perlin_amplitude: 1.0,
            // Thermal — operator tuned
            plasma_complexity: 2.46, plasma_distortion: 0.68, plasma_amplitude: 1.0,
            // Radar — operator tuned
            liss_num_curves: 3.6, liss_freq_base: 1.9,
            liss_freq_spread: 3.0, liss_amplitude: 0.50, liss_points: 500.0,
            // CA Waterfall — signal/memory
            waterfall: WaterfallState::new(22, 5),
            ca_scroll_rate: 6.0,  // rows/sec
            ca_density: 0.008,    // very sparse at idle — near invisible
            ca_rule: 30.0,        // Rule 30 — chaotic, interesting patterns
            ca_fade: 0.85,        // gentle trail fade
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
                ("scroll_rate", self.ca_scroll_rate, 2.0, 30.0),
                ("density", self.ca_density, 0.0, 0.3),
                ("rule", self.ca_rule, 0.0, 255.0),
                ("fade", self.ca_fade, 0.5, 1.0),
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
                0 => self.ca_scroll_rate = new_val,
                1 => self.ca_density = new_val,
                2 => self.ca_rule = new_val,
                3 => self.ca_fade = new_val,
                _ => {}
            },
            _ => {}
        }
    }
}

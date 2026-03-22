//! CIC instrument panel — four simultaneous displays.
//!
//! Ported directly from the fractal_demo example (the operator-tuned reference).
//!
//! ## Four Instruments
//!
//! 1. **Context** (Perlin flow) — context utilization, scale=7.9
//! 2. **Tools** (Lissajous curves) — tool execution activity, curves=3.6
//! 3. **Thinking** (Plasma sine) — inference/thinking state, complexity=2.46
//! 4. **Memory** (CA waterfall) — memory activity with per-mind columns
//!
//! All use unified navy→teal→amber CIE L* perceptual color ramp.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

// ─── Panel ──────────────────────────────────────────────────────────────

/// Instrument panel — four simultaneous displays in a 2×2 grid.
pub struct InstrumentPanel {
    time: f64,
    /// Per-instrument intensity (0.0 = idle, 1.0 = max)
    context_intensity: f64,
    tool_intensity: f64,
    thinking_intensity: f64,
    memory_intensity: f64,
    /// Tool error state → red border. Decays on a timer.
    tool_error: bool,
    tool_error_ttl: f64,
    /// Persistent Lissajous grid — avoids per-frame allocation
    liss_grid: Vec<u32>,
    /// Waterfall persistent state (one per mind)
    waterfalls: [WaterfallState; 4],
    minds_active: [bool; 4],
    /// Focus mode toggle
    pub focus_mode: bool,
}

impl Default for InstrumentPanel {
    fn default() -> Self {
        Self {
            time: 0.0,
            context_intensity: 0.0,
            tool_intensity: 0.0,
            thinking_intensity: 0.0,
            memory_intensity: 0.0,
            tool_error: false,
            tool_error_ttl: 0.0,
            liss_grid: Vec::new(),
            waterfalls: [
                WaterfallState::new(22, 5, 0xdeadbeef),
                WaterfallState::new(22, 5, 0xcafebabe),
                WaterfallState::new(22, 5, 0x8badf00d),
                WaterfallState::new(22, 5, 0xfeedface),
            ],
            minds_active: [true, false, false, false],
            focus_mode: false,
        }
    }
}

impl InstrumentPanel {
    /// Update instrument telemetry from harness state.
    ///
    /// - `context_pct`: 0-100 context utilization
    /// - `tool_call_delta`: tool calls THIS frame (0 if none)
    /// - `thinking_level`: "off"/"minimal"/"low"/"medium"/"high"
    /// - `injected_facts`: facts injected THIS turn (0 if none)
    /// - `agent_active`: whether the agent is currently processing
    /// - `dt`: real frame delta in seconds
    pub fn update_telemetry(
        &mut self,
        context_pct: f32,
        tool_call_delta: u32,
        thinking_level: &str,
        injected_facts: usize,
        agent_active: bool,
        dt: f64,
    ) {
        self.time += dt;

        // Context: cap at 70% (auto-compaction threshold)
        self.context_intensity = (context_pct as f64 / 70.0).min(1.0);

        // Tools: spike on NEW calls, sustain during active agent, slow decay when idle
        if tool_call_delta > 0 {
            self.tool_intensity = (self.tool_intensity + tool_call_delta as f64 * 0.35).min(1.0);
        } else if agent_active {
            // Agent is working — very slow decay so tools sustain between calls
            self.tool_intensity = (self.tool_intensity - dt * 0.08).max(0.0);
        } else {
            // Agent idle — moderate decay back to zero
            self.tool_intensity = (self.tool_intensity - dt * 0.25).max(0.0);
        }

        // Tool error: timer-based decay (5 seconds of red border)
        if self.tool_error {
            self.tool_error_ttl -= dt;
            if self.tool_error_ttl <= 0.0 {
                self.tool_error = false;
            }
        }

        // Thinking: only active during inference, intensity from configured level
        let thinking_target = if agent_active {
            match thinking_level {
                "high" => 0.85, "medium" => 0.6, "low" => 0.35, "minimal" => 0.15, _ => 0.1,
            }
        } else {
            0.0 // idle when agent isn't generating
        };
        // Smooth ramp toward target (not instant jump)
        self.thinking_intensity += (thinking_target - self.thinking_intensity) * dt * 3.0;

        // Memory: spike on memory operations (store, recall, archive, etc)
        if injected_facts > 0 {
            // Each memory op spikes hard — visible burst
            self.memory_intensity = (self.memory_intensity + injected_facts as f64 * 0.4).min(1.0);
        } else if agent_active {
            self.memory_intensity = (self.memory_intensity - dt * 0.15).max(0.0);
        } else {
            self.memory_intensity = (self.memory_intensity - dt * 0.3).max(0.0);
        }

        // Tick waterfalls — per-mind with state-driven CA rules
        // Width is determined at render time; tick uses current grid size
        for i in 0..4 {
            if !self.minds_active[i] { continue; }
            if self.waterfalls[i].width == 0 { continue; } // not yet rendered
            let density = 0.008 + self.memory_intensity * 0.25;
            let scroll = 6.0 * (0.5 + self.memory_intensity * 1.5);
            let rule = if self.memory_intensity > 0.1 {
                [30u8, 110, 90, 150][i]
            } else {
                204
            };
            self.waterfalls[i].tick(dt, scroll, density, rule, 0.85);
        }
    }

    /// Set tool error state — triggers red border on tools instrument.
    pub fn set_tool_error(&mut self) {
        self.tool_error = true;
        self.tool_error_ttl = 5.0; // 5 seconds of red border
        self.tool_intensity = 0.85;
    }

    pub fn toggle_focus(&mut self) {
        self.focus_mode = !self.focus_mode;
    }

    pub fn render(&mut self, area: Rect, frame: &mut Frame) {
        if area.width < 8 || area.height < 4 { return; }

        let rows = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ]).split(area);
        let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);
        let bot = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);

        let instruments: [(Rect, &str, f64, bool); 4] = [
            (top[0], "context",  self.context_intensity, false),
            (top[1], "tools",    self.tool_intensity, self.tool_error),
            (bot[0], "thinking", self.thinking_intensity, false),
            (bot[1], "memory",   self.memory_intensity, false),
        ];

        // Use theme-consistent colors (border_dim, dim fg)
        let border_dim = Color::Rgb(20, 40, 55); // matches theme border_dim range
        let label_fg = Color::Rgb(64, 88, 112);  // matches theme dim
        let error_border = Color::Rgb(224, 72, 72); // matches theme error

        for (idx, (area, label, intensity, is_error)) in instruments.iter().enumerate() {
            let pct = (*intensity * 100.0) as u32;
            let border_color = if *is_error { error_border } else { border_dim };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(
                    format!(" {} {}% ", label, pct),
                    Style::default().fg(label_fg),
                ));
            let inner = block.inner(*area);
            frame.render_widget(block, *area);

            if inner.width < 2 || inner.height < 1 { continue; }

            match idx {
                0 => render_perlin(self.time, *intensity, inner, frame.buffer_mut()),
                1 => {
                    render_lissajous(self.time, *intensity, inner, frame.buffer_mut(), &mut self.liss_grid);
                },
                2 => render_plasma(self.time, *intensity, inner, frame.buffer_mut()),
                3 => render_waterfall_multi(*intensity, inner, frame.buffer_mut(),
                        &mut self.waterfalls, &self.minds_active),
                _ => {}
            }
        }
    }
}

// ─── Color ramp (CIE L* perceptual, ported from demo) ──────────────────

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

fn set_halfblock(buf: &mut Buffer, area: Rect, px: usize, row: usize, top: Color, bot: Color) {
    if let Some(cell) = buf.cell_mut(Position::new(area.x + px as u16, area.y + row as u16)) {
        cell.set_char('▀');
        cell.set_fg(top);
        cell.set_bg(bot);
    }
}

// ─── Perlin flow (context) — ported from demo ──────────────────────────

fn render_perlin(time: f64, intensity: f64, area: Rect, buf: &mut Buffer) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    // Speed increases with intensity (flame effect)
    let speed = 0.3 + intensity * 2.5;
    let t = time * speed;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            let top = noise_octaves(px as f64 / 7.9, py as f64 / 7.9, t, 2, 4.0);
            let bot = noise_octaves(px as f64 / 7.9, (py + 1) as f64 / 7.9, t, 2, 4.0);
            let tc = pixel_color((top * 0.5 + 0.5) * 1.0, intensity);
            let bc = pixel_color((bot * 0.5 + 0.5) * 1.0, intensity);
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

// ─── Plasma sine (thinking) — ported from demo ─────────────────────────

fn render_plasma(time: f64, intensity: f64, area: Rect, buf: &mut Buffer) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    // Quadratic speed: slow ignition, then accelerates
    let speed = 0.2 + intensity * intensity * 2.0;
    let t = time * speed;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            let top = plasma_sample(px as f64, py as f64, t);
            let bot = plasma_sample(px as f64, (py + 1) as f64, t);
            let tc = pixel_color((top * 0.5 + 0.5) * 1.0, intensity);
            let bc = pixel_color((bot * 0.5 + 0.5) * 1.0, intensity);
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

fn plasma_sample(x: f64, y: f64, t: f64) -> f64 {
    let c = 2.46;
    let d = 0.68;
    let v1 = (x / (6.0 / c) + t).sin();
    let v2 = ((y / (4.0 / c) + t * 0.7).sin() + (x / (8.0 / c)).cos()).sin();
    let v3 = ((x * x + y * y).sqrt() * d / (6.0 / c) - t * 1.3).sin();
    let v4 = (x / (3.0 / c) - t * 0.5).cos() * (y / (5.0 / c) + t * 0.9).sin();
    (v1 + v2 + v3 + v4) / 4.0
}

// ─── Lissajous curves (tools) — ported from demo ───────────────────────

fn render_lissajous(time: f64, intensity: f64, area: Rect, buf: &mut Buffer, grid: &mut Vec<u32>) {
    let w = area.width as usize;
    let h = area.height as usize * 2;
    let needed = w * h;
    grid.resize(needed, 0);
    grid.fill(0);

    // More curves and points at higher intensity — the display gets DENSER
    let nc = (3.0 + intensity * 5.0) as usize; // 3 idle → 8 at max
    let pts = (500.0 + intensity * 2000.0) as usize; // 500 idle → 2500 at max
    let speed = 0.3 + intensity * 0.9;
    let t = time * speed;

    for curve in 0..nc {
        let fx = 1.9 + curve as f64 * 3.0 / nc.max(1) as f64;
        let fy = 1.9 + 1.0 + curve as f64 * (3.0 * 0.8) / nc.max(1) as f64;
        let phase = t * (1.0 + curve as f64 * 0.05);
        for i in 0..pts {
            let tt = i as f64 / pts as f64 * std::f64::consts::TAU;
            let x = (fx * tt + phase).sin();
            let y = (fy * tt + phase * 0.3).cos();
            let gx = ((x * 0.5 + 0.5) * w as f64) as usize;
            let gy = ((y * 0.5 + 0.5) * h as f64) as usize;
            if gx < w && gy < h { grid[gy * w + gx] += 1; }
        }
    }

    let max_hits = (*grid.iter().max().unwrap_or(&1)).max(1) as f64;
    for py in (0..h).step_by(2) {
        let row = py / 2;
        if row >= area.height as usize { break; }
        for px in 0..w {
            if px >= area.width as usize { break; }
            // Gamma correction: sqrt boosts mid-range values so curves aren't just dim dots
            let top_v = (grid[py * w + px] as f64 / max_hits).min(1.0).sqrt();
            let bot_v = if py + 1 < h { (grid[(py + 1) * w + px] as f64 / max_hits).min(1.0).sqrt() } else { 0.0 };
            // Use intensity directly — at high intensity, even dim curves are visible
            let tc = pixel_color(top_v, intensity.max(0.15));
            let bc = pixel_color(bot_v, intensity.max(0.15));
            set_halfblock(buf, area, px, row, tc, bc);
        }
    }
}

// ─── CA waterfall (memory) — ported from demo ──────────────────────────

const NOISE_CHARS: &[char] = &[
    '▏', '▎', '▍', '░',
    '▌', '▐', '▒', '┤', '├', '│', '─',
    '▊', '▋', '▓', '╱', '╲', '┼', '╪', '╫',
    '█', '╬', '■', '◆',
];

/// Persistent waterfall state per mind.
pub struct WaterfallState {
    grid: Vec<f64>,
    width: usize,
    height: usize,
    scroll_accum: f64,
    rng: u64,
}

impl WaterfallState {
    fn new(w: usize, h: usize, seed: u64) -> Self {
        Self { grid: vec![0.0; w * h], width: w, height: h, scroll_accum: 0.0, rng: seed }
    }

    fn next_rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn tick(&mut self, dt: f64, scroll_rate: f64, density: f64, rule: u8, fade: f64) {
        self.scroll_accum += dt * scroll_rate;
        while self.scroll_accum >= 1.0 {
            self.scroll_accum -= 1.0;
            let w = self.width;
            let h = self.height;
            for y in 0..(h - 1) {
                for x in 0..w {
                    self.grid[y * w + x] = self.grid[(y + 1) * w + x] * fade;
                }
            }
            let prev_row = h - 2;
            let new_row = h - 1;
            for x in 0..w {
                let left = if x > 0 { (self.grid[prev_row * w + x - 1] > 0.3) as u8 } else { 0 };
                let center = (self.grid[prev_row * w + x] > 0.3) as u8;
                let right = if x + 1 < w { (self.grid[prev_row * w + x + 1] > 0.3) as u8 } else { 0 };
                let neighborhood = (left << 2) | (center << 1) | right;
                let alive = (rule >> neighborhood) & 1 == 1;
                let random_birth = (self.next_rand() % 1000) < (density * 1000.0) as u64;
                self.grid[new_row * w + x] = if alive || random_birth { 1.0 } else { 0.0 };
            }
        }
    }

    fn ensure_size(&mut self, w: usize, h: usize) {
        if self.width != w || self.height != h {
            self.grid = vec![0.0; w * h];
            self.width = w;
            self.height = h;
        }
    }
}

fn render_waterfall_multi(
    intensity: f64, area: Rect, buf: &mut Buffer,
    waterfalls: &mut [WaterfallState; 4], minds_active: &[bool; 4],
) {
    let active_indices: Vec<usize> = minds_active.iter().enumerate()
        .filter(|(_, a)| **a).map(|(i, _)| i).collect();
    let n = active_indices.len();
    if n == 0 { return; }

    let total_w = area.width as usize;
    let gap = if n > 1 { 1 } else { 0 };
    let usable = total_w.saturating_sub(if n > 1 { n - 1 } else { 0 });
    let col_w = usable / n;

    for (seg_idx, &mind_idx) in active_indices.iter().enumerate() {
        let x_offset = seg_idx * (col_w + gap);
        let seg_w = col_w.min(total_w.saturating_sub(x_offset));
        let seg_area = Rect {
            x: area.x + x_offset as u16,
            y: area.y,
            width: seg_w as u16,
            height: area.height,
        };
        // Resize waterfall to match actual render area
        waterfalls[mind_idx].ensure_size(seg_w, area.height as usize);
        render_waterfall(intensity, seg_area, buf, &waterfalls[mind_idx]);

        if seg_idx < n - 1 && gap > 0 {
            let sep_x = area.x + (x_offset + col_w) as u16;
            for y in area.y..area.bottom() {
                if let Some(cell) = buf.cell_mut(Position::new(sep_x, y)) {
                    cell.set_char('│');
                    cell.set_fg(Color::Rgb(20, 40, 55));
                    cell.set_bg(bg_color());
                }
            }
        }
    }
}

fn render_waterfall(intensity: f64, area: Rect, buf: &mut Buffer, wf: &WaterfallState) {
    for cy in 0..area.height as usize {
        for cx in 0..area.width as usize {
            let val = if cx < wf.width && cy < wf.height {
                wf.grid[cy * wf.width + cx]
            } else { 0.0 };

            if val < 0.05 {
                if let Some(cell) = buf.cell_mut(Position::new(area.x + cx as u16, area.y + cy as u16)) {
                    cell.set_char(' ');
                    cell.set_fg(bg_color());
                    cell.set_bg(bg_color());
                }
                continue;
            }

            let hash = ((cx * 7 + cy * 13 + (val * 100.0) as usize) * 31) % NOISE_CHARS.len();
            let tier = ((val * (NOISE_CHARS.len() - 1) as f64) as usize).min(NOISE_CHARS.len() - 1);
            let idx = (tier / 2 + hash / 2).min(NOISE_CHARS.len() - 1);
            let ch = NOISE_CHARS[idx];

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

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intensity_color_floor_is_bg() {
        assert!(matches!(intensity_color(0.0), Color::Rgb(0, 1, 3)));
    }

    #[test]
    fn intensity_color_ramp_progresses() {
        if let Color::Rgb(r, g, b) = intensity_color(0.1) {
            assert!(r < 20 && g < 50 && b < 50, "0.1 dark: ({r},{g},{b})");
        }
        if let Color::Rgb(r, g, b) = intensity_color(1.0) {
            assert!(r > 40 && b < r, "1.0 amber: ({r},{g},{b})");
        }
    }

    #[test]
    fn panel_renders_without_panic() {
        let mut panel = InstrumentPanel::default();
        let area = Rect::new(0, 0, 96, 12);
        let backend = ratatui::backend::TestBackend::new(96, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| panel.render(area, f)).unwrap();
    }

    #[test]
    fn waterfall_scrolls() {
        let mut wf = WaterfallState::new(10, 5, 0xdeadbeef);
        wf.tick(0.5, 10.0, 0.1, 30, 0.85);
        let has_content = wf.grid.iter().any(|&v| v > 0.0);
        assert!(has_content, "waterfall should have content after tick");
    }
}

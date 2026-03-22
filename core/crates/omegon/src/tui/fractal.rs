//! Fractal status surface — living visualization of agent state.
//!
//! Renders one of four algorithms in the dashboard header, switching
//! based on agent activity:
//!
//! - **Idle** → Perlin noise flow (smooth breathing)
//! - **Thinking** → Plasma sine interference (rippling fabric)
//! - **Working** → Lissajous curves (smooth looping trails)
//! - **Cleave** → Lissajous (intensified — more curves, faster)
//!
//! Color uses continuous hue rotation within the Alpharius palette band
//! (~150-220° in HSV). The hue drifts slowly per frame, staying on-brand
//! while always moving.
//!
//! Uses half-block characters (▀) for 2 color channels per cell.

use ratatui::prelude::*;
use ratatui::buffer::Buffer;

/// Agent activity state — drives which algorithm renders.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum AgentMode {
    #[default]
    Idle,
    Thinking,
    Working,
    Cleave,
}

/// The fractal state surface widget.
pub struct FractalWidget {
    /// Current agent mode (drives algorithm selection).
    pub mode: AgentMode,
    /// Animation time (accumulated from dt per frame).
    pub time: f64,
    /// Hue center in degrees — oscillates within the Alpharius band.
    hue_center: f64,
    /// Hue oscillation phase.
    hue_phase: f64,
}

impl Default for FractalWidget {
    fn default() -> Self {
        Self {
            mode: AgentMode::Idle,
            time: 0.0,
            hue_center: 190.0,
            hue_phase: 0.0,
        }
    }
}

impl FractalWidget {
    /// Update from harness telemetry. Call once per frame (~60fps).
    pub fn update_from_status(
        &mut self,
        _context_pct: f32,
        _thinking_level: &str,
        is_agent_active: bool,
        _persona_id: Option<&str>,
        is_cleave_active: bool,
        dt: f64,
    ) {
        self.time += dt;

        // Determine mode
        self.mode = if is_cleave_active {
            AgentMode::Cleave
        } else if is_agent_active {
            AgentMode::Working
        } else {
            AgentMode::Idle
        };

        // Hue target and oscillation range per mode
        // Locked to teal band (165-195°). No blue/purple drift.
        let (target_hue, hue_range, hue_speed) = match self.mode {
            AgentMode::Idle =>     (182.0,  6.0, 0.12),  // 176-188° slow teal
            AgentMode::Thinking => (186.0,  8.0, 0.20),  // 178-194° slightly deeper
            AgentMode::Working =>  (178.0,  8.0, 0.25),  // 170-186° greener teal
            AgentMode::Cleave =>   (180.0, 12.0, 0.35),  // 168-192° wider but still teal
        };

        // Smooth hue center toward target
        self.hue_center += (target_hue - self.hue_center) * dt * 2.0;
        self.hue_phase += dt * hue_speed;

        // Oscillating hue
        let _ = hue_range; // used in current_hue()
    }

    /// Current hue in degrees, oscillating within the mode's band.
    fn current_hue(&self) -> f64 {
        let (_, hue_range, _) = match self.mode {
            AgentMode::Idle =>     (182.0,  6.0, 0.12),
            AgentMode::Thinking => (186.0,  8.0, 0.20),
            AgentMode::Working =>  (178.0,  8.0, 0.25),
            AgentMode::Cleave =>   (180.0, 12.0, 0.35),
        };
        self.hue_center + hue_range * self.hue_phase.sin()
    }

    /// Convert intensity (0-1) to an RGB color at the current hue.
    fn hue_color(&self, intensity: f64) -> Color {
        if intensity < 0.005 {
            return Color::Rgb(0, 1, 3); // surface_bg
        }
        let hue = self.current_hue();
        let saturation = 0.82;
        let value = intensity.sqrt().clamp(0.0, 1.0) * 0.32; // max brightness ~80/255
        hsv_to_rgb(hue, saturation, value)
    }

    /// Render into a ratatui Buffer area using half-block characters.
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 2 {
            return;
        }
        match self.mode {
            AgentMode::Idle => self.render_perlin(area, buf),
            AgentMode::Thinking => self.render_plasma(area, buf),
            AgentMode::Working => self.render_lissajous(area, buf, false),
            AgentMode::Cleave => self.render_lissajous(area, buf, true),
        }
    }

    // ── Perlin flow (idle) ──────────────────────────────────────────────

    fn render_perlin(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let h = area.height as usize * 2;
        let scale = 18.0;
        let speed = 1.8;

        for py in (0..h).step_by(2) {
            let row = py / 2;
            if row >= area.height as usize { break; }
            for px in 0..w {
                if px >= area.width as usize { break; }
                let top = noise_octaves(px as f64 / scale, py as f64 / scale, self.time * speed, 2, 2.3);
                let bot = noise_octaves(px as f64 / scale, (py + 1) as f64 / scale, self.time * speed, 2, 2.3);
                let tc = self.hue_color((top * 0.5 + 0.5).clamp(0.0, 1.0) * 0.5);
                let bc = self.hue_color((bot * 0.5 + 0.5).clamp(0.0, 1.0) * 0.5);
                set_halfblock(buf, area, px, row, tc, bc);
            }
        }
    }

    // ── Plasma sine (thinking) ──────────────────────────────────────────

    fn render_plasma(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let h = area.height as usize * 2;
        let complexity = 1.65;
        let speed = 1.46;
        let distortion = 0.8;

        for py in (0..h).step_by(2) {
            let row = py / 2;
            if row >= area.height as usize { break; }
            for px in 0..w {
                if px >= area.width as usize { break; }
                let top = plasma_sample(px as f64, py as f64, self.time, complexity, speed, distortion);
                let bot = plasma_sample(px as f64, (py + 1) as f64, self.time, complexity, speed, distortion);
                let tc = self.hue_color((top * 0.5 + 0.5).clamp(0.0, 1.0) * 0.88);
                let bc = self.hue_color((bot * 0.5 + 0.5).clamp(0.0, 1.0) * 0.88);
                set_halfblock(buf, area, px, row, tc, bc);
            }
        }
    }

    // ── Lissajous curves (working / cleave) ─────────────────────────────

    fn render_lissajous(&self, area: Rect, buf: &mut Buffer, intense: bool) {
        let w = area.width as usize;
        let h = area.height as usize * 2;
        let mut grid = vec![0u32; w * h];

        let num_curves = if intense { 12 } else { 8 };
        let speed = if intense { 0.85 } else { 0.68 };
        let points = 5375usize;
        let freq_base = 1.9;
        let freq_spread = 1.86;
        let amplitude = 0.50;

        for curve in 0..num_curves {
            let fx = freq_base + curve as f64 * freq_spread / num_curves as f64;
            let fy = freq_base + 1.0 + curve as f64 * (freq_spread * 0.8) / num_curves as f64;
            let phase = self.time * (speed + curve as f64 * 0.03);
            for i in 0..points {
                let t = i as f64 / points as f64 * std::f64::consts::TAU;
                let x = (fx * t + phase).sin();
                let y = (fy * t + phase * 0.3).cos();
                let gx = ((x * amplitude + 0.5) * w as f64) as usize;
                let gy = ((y * amplitude + 0.5) * h as f64) as usize;
                if gx < w && gy < h {
                    grid[gy * w + gx] += 1;
                }
            }
        }

        let max_hits = (*grid.iter().max().unwrap_or(&1)).max(1) as f64;
        for py in (0..h).step_by(2) {
            let row = py / 2;
            if row >= area.height as usize { break; }
            for px in 0..w {
                if px >= area.width as usize { break; }
                let top_v = (grid[py * w + px] as f64 / max_hits).min(1.0);
                let bot_v = if py + 1 < h { (grid[(py + 1) * w + px] as f64 / max_hits).min(1.0) } else { 0.0 };
                let tc = self.hue_color(top_v);
                let bc = self.hue_color(bot_v);
                set_halfblock(buf, area, px, row, tc, bc);
            }
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn set_halfblock(buf: &mut Buffer, area: Rect, px: usize, row: usize, top: Color, bot: Color) {
    if let Some(cell) = buf.cell_mut(Position::new(area.x + px as u16, area.y + row as u16)) {
        cell.set_char('▀');
        cell.set_fg(top);
        cell.set_bg(bot);
    }
}

/// HSV to RGB conversion. Hue in degrees (0-360), S and V in 0-1.
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> Color {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::Rgb(
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

/// Layered noise with octaves.
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

/// Fast smooth noise (sine interference).
fn noise_sample(x: f64, y: f64, z: f64) -> f64 {
    let v1 = (x * 1.3 + z).sin() * (y * 0.7 + z * 0.5).cos();
    let v2 = ((x + y) * 0.8 - z * 0.3).sin();
    let v3 = (x * 2.1 - z * 0.7).cos() * (y * 1.5 + z * 0.4).sin();
    (v1 + v2 + v3) / 3.0
}

/// Plasma sine interference sample.
fn plasma_sample(x: f64, y: f64, t: f64, complexity: f64, speed: f64, distortion: f64) -> f64 {
    let c = complexity;
    let s = t * speed;
    let v1 = (x / (6.0 / c) + s).sin();
    let v2 = ((y / (4.0 / c) + s * 0.7).sin() + (x / (8.0 / c)).cos()).sin();
    let v3 = ((x * x + y * y).sqrt() * distortion / (6.0 / c) - s * 1.3).sin();
    let v4 = (x / (3.0 / c) - s * 0.5).cos() * (y / (5.0 / c) + s * 0.9).sin();
    (v1 + v2 + v3 + v4) / 4.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_to_buffer() {
        let widget = FractalWidget::default();
        let area = Rect::new(0, 0, 36, 8);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let mut non_space = 0;
        for y in 0..area.height {
            for x in 0..area.width {
                if buf.cell(Position::new(x, y)).unwrap().symbol() == "▀" {
                    non_space += 1;
                }
            }
        }
        assert!(non_space > 0, "should render half-block characters");
    }

    #[test]
    fn render_tiny_area_does_not_panic() {
        let widget = FractalWidget::default();
        let area = Rect::new(0, 0, 2, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn hsv_conversion_basic() {
        // Red
        assert!(matches!(hsv_to_rgb(0.0, 1.0, 1.0), Color::Rgb(255, 0, 0)));
        // Green
        assert!(matches!(hsv_to_rgb(120.0, 1.0, 1.0), Color::Rgb(0, 255, 0)));
        // Blue
        assert!(matches!(hsv_to_rgb(240.0, 1.0, 1.0), Color::Rgb(0, 0, 255)));
        // Black
        assert!(matches!(hsv_to_rgb(0.0, 0.0, 0.0), Color::Rgb(0, 0, 0)));
    }

    #[test]
    fn hue_color_below_threshold_returns_bg() {
        let w = FractalWidget::default();
        assert!(matches!(w.hue_color(0.0), Color::Rgb(0, 1, 3)));
        assert!(matches!(w.hue_color(0.004), Color::Rgb(0, 1, 3)));
    }

    #[test]
    fn mode_transitions() {
        let mut w = FractalWidget::default();
        assert_eq!(w.mode, AgentMode::Idle);

        w.update_from_status(0.0, "medium", true, None, false, 0.016);
        assert_eq!(w.mode, AgentMode::Working);

        w.update_from_status(0.0, "medium", true, None, true, 0.016);
        assert_eq!(w.mode, AgentMode::Cleave);

        w.update_from_status(0.0, "medium", false, None, false, 0.016);
        assert_eq!(w.mode, AgentMode::Idle);
    }

    #[test]
    fn all_modes_render_without_panic() {
        let area = Rect::new(0, 0, 36, 8);
        for mode in [AgentMode::Idle, AgentMode::Thinking, AgentMode::Working, AgentMode::Cleave] {
            let mut w = FractalWidget::default();
            w.mode = mode;
            w.time = 5.0;
            let mut buf = Buffer::empty(area);
            w.render(area, &mut buf);
        }
    }
}

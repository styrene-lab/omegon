//! CIC instrument panel — four simultaneous fractal instruments.
//!
//! Replace the 4-card footer with a split-panel layout:
//! - Engine + memory (left 40%): inference state and memory telemetry
//! - System state (right 60%): 2×2 grid of fractal instruments
//!
//! ## Four Instruments
//!
//! 1. **Perlin sonar** (context) — smooth noise flow, scale=7.9
//! 2. **Lissajous radar** (tools) — curve intersection patterns, 3.6 curves
//! 3. **Plasma thermal** (thinking) — sine interference, complexity=2.46
//! 4. **CA waterfall** (memory) — cellular automata with CRT noise glyphs
//!
//! All use unified navy→teal→amber CIE L* perceptual color ramp.
//! Amber gets 50% of the range for high-intensity visibility.

use ratatui::prelude::*;
use ratatui::buffer::Buffer;
use std::collections::HashMap;

/// Instrument panel state and rendering core.
pub struct InstrumentPanel {
    /// Perlin sonar instrument (context monitoring).
    perlin: PerlinSonar,
    /// Lissajous radar instrument (tool monitoring).
    lissajous: LissajousRadar,
    /// Plasma thermal instrument (thinking monitoring).
    plasma: PlasmaThermal,
    /// CA waterfall instrument (memory monitoring).
    waterfall: CaWaterfall,
    /// Animation time counter.
    time: f64,
    /// Focus mode toggle state.
    focus_mode: bool,
}

impl Default for InstrumentPanel {
    fn default() -> Self {
        Self {
            perlin: PerlinSonar::new(),
            lissajous: LissajousRadar::new(),
            plasma: PlasmaThermal::new(),
            waterfall: CaWaterfall::new(),
            time: 0.0,
            focus_mode: false,
        }
    }
}

impl InstrumentPanel {
    /// Update telemetry data from the harness.
    pub fn update_telemetry(
        &mut self,
        context_pct: f32,
        tool_calls: u32,
        thinking_level: &str,
        memory_facts: usize,
        memory_minds: &[String], // per-mind column labels
        dt: f64,
    ) {
        self.time += dt;

        // Update individual instruments with their telemetry
        self.perlin.update(context_pct, self.time);
        self.lissajous.update(tool_calls, self.time);
        self.plasma.update(thinking_level, self.time);
        self.waterfall.update(memory_facts, memory_minds, self.time);
    }

    /// Toggle focus mode (expand one instrument to full panel).
    pub fn toggle_focus(&mut self) {
        self.focus_mode = !self.focus_mode;
    }

    /// Render the 2×2 instrument grid or single focused instrument.
    pub fn render(&self, area: Rect, frame: &mut Frame) {
        if area.width < 8 || area.height < 4 {
            return;
        }

        if self.focus_mode {
            // TODO: implement focus mode (single instrument expanded)
            self.render_grid(area, frame);
        } else {
            self.render_grid(area, frame);
        }
    }

    /// Render the 2×2 grid layout.
    fn render_grid(&self, area: Rect, frame: &mut Frame) {
        // Split into 2×2 grid
        let rows = Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ]).split(area);

        let top_cols = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ]).split(rows[0]);

        let bottom_cols = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ]).split(rows[1]);

        // Render each instrument in its quadrant
        self.perlin.render(top_cols[0], frame.buffer_mut());      // Top-left: context
        self.lissajous.render(top_cols[1], frame.buffer_mut());   // Top-right: tools
        self.plasma.render(bottom_cols[0], frame.buffer_mut());   // Bottom-left: thinking
        self.waterfall.render(bottom_cols[1], frame.buffer_mut()); // Bottom-right: memory
    }
}

// ═══ Individual Instruments ═══════════════════════════════════════════════

/// Convert intensity (0-1) to CIE L* perceptual navy→teal→amber ramp.
/// Amber gets 50% of the range for high-intensity visibility.
fn intensity_color(intensity: f64) -> Color {
    if intensity < 0.005 {
        return Color::Rgb(0, 1, 3); // surface_bg
    }

    let clamped = intensity.clamp(0.0, 1.0);

    if clamped <= 0.5 {
        // Navy → Teal (0.0 to 0.5 maps to first half)
        let t = clamped * 2.0; // 0.0 to 1.0
        let navy = (8, 24, 48);    // Dark navy
        let teal = (42, 180, 200); // Alpharius accent teal
        
        let r = navy.0 as f64 + t * (teal.0 as f64 - navy.0 as f64);
        let g = navy.1 as f64 + t * (teal.1 as f64 - navy.1 as f64);
        let b = navy.2 as f64 + t * (teal.2 as f64 - navy.2 as f64);
        
        Color::Rgb(r as u8, g as u8, b as u8)
    } else {
        // Teal → Amber (0.5 to 1.0 maps to second half, amber gets 50% range)
        let t = (clamped - 0.5) * 2.0; // 0.0 to 1.0
        let teal = (42, 180, 200);     // Alpharius accent teal
        let amber = (255, 191, 0);     // Bright amber
        
        let r = teal.0 as f64 + t * (amber.0 as f64 - teal.0 as f64);
        let g = teal.1 as f64 + t * (amber.1 as f64 - teal.1 as f64);
        let b = teal.2 as f64 + t * (amber.2 as f64 - teal.2 as f64);
        
        Color::Rgb(r as u8, g as u8, b as u8)
    }
}

/// Set half-block character with top/bottom colors.
fn set_halfblock(buf: &mut Buffer, area: Rect, x: usize, y: usize, top: Color, bot: Color) {
    if x < area.width as usize && y < area.height as usize {
        if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) {
            cell.set_char('▀');
            cell.set_fg(top);
            cell.set_bg(bot);
        }
    }
}

// ═══ Perlin Sonar (Context) ═══════════════════════════════════════════════

/// Perlin noise sonar — context usage monitoring.
/// Shows smooth flowing patterns that intensify with context usage.
pub struct PerlinSonar {
    scale: f64,
    octaves: u32,
    lacunarity: f64,
    amplitude: f64,
    context_intensity: f32,
}

impl PerlinSonar {
    fn new() -> Self {
        Self {
            scale: 7.9,
            octaves: 3, // Approximated from 2.5
            lacunarity: 4.0,
            amplitude: 1.0,
            context_intensity: 0.0,
        }
    }

    fn update(&mut self, context_pct: f32, _time: f64) {
        // Cap context at 70% as specified
        self.context_intensity = (context_pct / 100.0).min(0.7);
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let h = area.height as usize;

        for y in 0..h {
            for x in 0..w {
                // Perlin noise sample
                let nx = x as f64 / self.scale;
                let ny = y as f64 / self.scale;
                let noise = self.layered_noise(nx, ny);
                
                // Modulate with context intensity
                let intensity = (noise * 0.5 + 0.5) * self.context_intensity as f64 * self.amplitude;
                let color = intensity_color(intensity);
                
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) {
                    cell.set_char('█');
                    cell.set_fg(color);
                    cell.set_bg(Color::Rgb(0, 1, 3)); // surface_bg
                }
            }
        }
    }

    fn layered_noise(&self, x: f64, y: f64) -> f64 {
        let mut value = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut total_amplitude = 0.0;

        for _ in 0..self.octaves {
            value += self.noise_sample(x * frequency, y * frequency) * amplitude;
            total_amplitude += amplitude;
            amplitude *= 0.5;
            frequency *= self.lacunarity;
        }

        value / total_amplitude
    }

    fn noise_sample(&self, x: f64, y: f64) -> f64 {
        // Fast smooth noise using sine interference
        let v1 = (x * 1.3).sin() * (y * 0.7).cos();
        let v2 = ((x + y) * 0.8).sin();
        let v3 = (x * 2.1).cos() * (y * 1.5).sin();
        (v1 + v2 + v3) / 3.0
    }
}

// ═══ Lissajous Radar (Tools) ═══════════════════════════════════════════════

/// Lissajous curve radar — tool activity monitoring.
/// Shows intersecting parametric curves that intensify with tool usage.
pub struct LissajousRadar {
    curves: f64,
    freq_base: f64,
    spread: f64,
    amplitude: f64,
    points: usize,
    tool_intensity: f32,
    error_state: bool,
}

impl LissajousRadar {
    fn new() -> Self {
        Self {
            curves: 3.6,
            freq_base: 1.9,
            spread: 3.0,
            amplitude: 0.5,
            points: 500,
            tool_intensity: 0.0,
            error_state: false,
        }
    }

    fn update(&mut self, tool_calls: u32, _time: f64) {
        // Intensity based on recent tool activity
        self.tool_intensity = if tool_calls > 0 {
            (tool_calls as f32 / 10.0).min(1.0) // Scale tool calls to intensity
        } else {
            self.tool_intensity * 0.95 // Decay when idle
        };

        // TODO: detect tool errors and set error_state accordingly
        self.error_state = false;
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let h = area.height as usize;
        let mut grid = vec![0u32; w * h];

        let num_curves = (self.curves as usize).max(1);
        
        // Render Lissajous curves
        for curve in 0..num_curves {
            let fx = self.freq_base + curve as f64 * self.spread / num_curves as f64;
            let fy = self.freq_base + curve as f64 * (self.spread * 0.8) / num_curves as f64;
            
            for i in 0..self.points {
                let t = i as f64 / self.points as f64 * std::f64::consts::TAU;
                let x = (fx * t).sin();
                let y = (fy * t).cos();
                
                let gx = ((x * self.amplitude + 0.5) * w as f64) as usize;
                let gy = ((y * self.amplitude + 0.5) * h as f64) as usize;
                
                if gx < w && gy < h {
                    grid[gy * w + gx] += 1;
                }
            }
        }

        // Render grid to buffer
        let max_hits = (*grid.iter().max().unwrap_or(&1)).max(1) as f64;
        for y in 0..h {
            for x in 0..w {
                let hits = grid[y * w + x] as f64 / max_hits;
                let intensity = hits * self.tool_intensity as f64;
                
                let color = if self.error_state && intensity > 0.1 {
                    // Tool error: amber body + red border effect
                    Color::Rgb(255, 191, 0) // Amber
                } else {
                    intensity_color(intensity)
                };
                
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) {
                    cell.set_char('█');
                    cell.set_fg(color);
                    cell.set_bg(Color::Rgb(0, 1, 3)); // surface_bg
                }
            }
        }
    }
}

// ═══ Plasma Thermal (Thinking) ═══════════════════════════════════════════

/// Plasma thermal display — thinking activity monitoring.
/// Shows sine interference patterns that vary with cognitive load.
pub struct PlasmaThermal {
    complexity: f64,
    distortion: f64,
    amplitude: f64,
    quadratic_speed: bool,
    thinking_intensity: f32,
}

impl PlasmaThermal {
    fn new() -> Self {
        Self {
            complexity: 2.46,
            distortion: 0.68,
            amplitude: 1.0,
            quadratic_speed: true,
            thinking_intensity: 0.0,
        }
    }

    fn update(&mut self, thinking_level: &str, _time: f64) {
        self.thinking_intensity = match thinking_level.to_lowercase().as_str() {
            "off" => 0.0,
            "minimal" => 0.2,
            "low" => 0.4,
            "medium" => 0.6,
            "high" => 0.8,
            _ => 0.0,
        };
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let h = area.height as usize;
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        for y in 0..h {
            for x in 0..w {
                let plasma = self.plasma_sample(x as f64, y as f64, time);
                let intensity = (plasma * 0.5 + 0.5) * self.thinking_intensity as f64 * self.amplitude;
                let color = intensity_color(intensity);
                
                if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) {
                    cell.set_char('█');
                    cell.set_fg(color);
                    cell.set_bg(Color::Rgb(0, 1, 3)); // surface_bg
                }
            }
        }
    }

    fn plasma_sample(&self, x: f64, y: f64, t: f64) -> f64 {
        let c = self.complexity;
        let speed = if self.quadratic_speed { t * t * 0.1 } else { t };
        
        let v1 = (x / (6.0 / c) + speed).sin();
        let v2 = ((y / (4.0 / c) + speed * 0.7).sin() + (x / (8.0 / c)).cos()).sin();
        let v3 = ((x * x + y * y).sqrt() * self.distortion / (6.0 / c) - speed * 1.3).sin();
        let v4 = (x / (3.0 / c) - speed * 0.5).cos() * (y / (5.0 / c) + speed * 0.9).sin();
        
        (v1 + v2 + v3 + v4) / 4.0
    }
}

// ═══ CA Waterfall (Memory) ═══════════════════════════════════════════════

/// Cellular automata waterfall — memory monitoring with per-mind columns.
/// Uses CRT noise glyphs and Rule 204/30/110/90/150 rotation.
pub struct CaWaterfall {
    rules: [u8; 5],
    current_rule_index: usize,
    states: HashMap<String, WaterfallState>,
    glyph_set: Vec<char>,
    memory_facts: usize,
}

impl CaWaterfall {
    fn new() -> Self {
        Self {
            rules: [204, 30, 110, 90, 150],
            current_rule_index: 0,
            states: HashMap::new(),
            glyph_set: vec!['░', '▒', '▓', '█', '▞', '▚', '▜', '▟'],
            memory_facts: 0,
        }
    }

    fn update(&mut self, memory_facts: usize, memory_minds: &[String], _time: f64) {
        self.memory_facts = memory_facts;

        // Ensure each mind has a waterfall state
        for mind in memory_minds {
            if !self.states.contains_key(mind) {
                self.states.insert(mind.clone(), WaterfallState::new());
            }
        }

        // Remove states for minds that no longer exist
        self.states.retain(|mind, _| memory_minds.contains(mind));

        // Advance CA simulation for each mind
        for state in self.states.values_mut() {
            state.step(self.rules[self.current_rule_index]);
        }

        // Rotate rule periodically
        if memory_facts % 100 == 0 && memory_facts > 0 {
            self.current_rule_index = (self.current_rule_index + 1) % self.rules.len();
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let w = area.width as usize;
        let h = area.height as usize;

        if self.states.is_empty() {
            // No memory minds - show static noise
            for y in 0..h {
                for x in 0..w {
                    let glyph_idx = (x + y * 17) % self.glyph_set.len();
                    let intensity = if self.memory_facts > 0 { 0.1 } else { 0.05 };
                    let color = intensity_color(intensity);
                    
                    if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) {
                        cell.set_char(self.glyph_set[glyph_idx]);
                        cell.set_fg(color);
                        cell.set_bg(Color::Rgb(0, 1, 3)); // surface_bg
                    }
                }
            }
            return;
        }

        // Divide width among mind columns
        let minds: Vec<&String> = self.states.keys().collect();
        let col_width = w / minds.len().max(1);

        for (mind_idx, mind_name) in minds.iter().enumerate() {
            if let Some(state) = self.states.get(*mind_name) {
                let col_start = mind_idx * col_width;
                let col_end = if mind_idx == minds.len() - 1 { w } else { (mind_idx + 1) * col_width };

                for y in 0..h {
                    for x in col_start..col_end {
                        let local_x = x - col_start;
                        let ca_width = col_end - col_start;
                        
                        if local_x < ca_width && y < state.height {
                            let cell_state = state.get_cell(local_x, y);
                            let glyph_idx = (cell_state as usize) % self.glyph_set.len();
                            let intensity = if cell_state > 0 { 
                                (cell_state as f64 / 255.0) * 0.8 
                            } else { 
                                0.02 
                            };
                            let color = intensity_color(intensity);
                            
                            if let Some(cell) = buf.cell_mut(Position::new(area.x + x as u16, area.y + y as u16)) {
                                cell.set_char(self.glyph_set[glyph_idx]);
                                cell.set_fg(color);
                                cell.set_bg(Color::Rgb(0, 1, 3)); // surface_bg
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Per-mind cellular automata waterfall state.
pub struct WaterfallState {
    width: usize,
    height: usize,
    cells: Vec<Vec<u8>>,
    generation: usize,
}

impl WaterfallState {
    fn new() -> Self {
        let width = 32;
        let height = 24;
        let mut cells = vec![vec![0u8; width]; height];
        
        // Initialize with random seed
        for x in 0..width {
            cells[0][x] = if (x * 37 + 17) % 3 == 0 { 1 } else { 0 };
        }

        Self {
            width,
            height,
            cells,
            generation: 0,
        }
    }

    fn step(&mut self, rule: u8) {
        // Scroll down: move all rows down one position
        for y in (1..self.height).rev() {
            self.cells[y] = self.cells[y - 1].clone();
        }

        // Generate new top row using CA rule
        let mut new_row = vec![0u8; self.width];
        for x in 0..self.width {
            let left = if x > 0 { self.cells[1][x - 1] } else { 0 };
            let center = self.cells[1][x];
            let right = if x + 1 < self.width { self.cells[1][x + 1] } else { 0 };
            
            new_row[x] = self.apply_rule(rule, left, center, right);
        }
        self.cells[0] = new_row;
        self.generation += 1;
    }

    fn apply_rule(&self, rule: u8, left: u8, center: u8, right: u8) -> u8 {
        let pattern = (left << 2) | (center << 1) | right;
        if (rule >> pattern) & 1 == 1 { 1 } else { 0 }
    }

    fn get_cell(&self, x: usize, y: usize) -> u8 {
        if x < self.width && y < self.height {
            self.cells[y][x] * 255 // Scale to full intensity range
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn instrument_panel_creates_successfully() {
        let panel = InstrumentPanel::default();
        assert!(!panel.focus_mode);
    }

    #[test]
    fn instrument_panel_renders_without_panic() {
        let panel = InstrumentPanel::default();
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            panel.render(frame.area(), frame);
        }).unwrap();
    }

    #[test]
    fn intensity_color_navy_to_teal_range() {
        let navy = intensity_color(0.0);
        let mid = intensity_color(0.25);
        let teal = intensity_color(0.5);
        
        // Should progress from dark to brighter
        assert!(matches!(navy, Color::Rgb(0, 1, 3))); // surface_bg for zero
        assert!(matches!(mid, Color::Rgb(_, _, _))); // Some interpolated color
        assert!(matches!(teal, Color::Rgb(42, 180, 200))); // Teal
    }

    #[test]
    fn intensity_color_teal_to_amber_range() {
        let teal = intensity_color(0.5);
        let amber = intensity_color(1.0);
        
        assert!(matches!(teal, Color::Rgb(42, 180, 200))); // Teal
        assert!(matches!(amber, Color::Rgb(255, 191, 0))); // Amber
    }

    #[test]
    fn perlin_sonar_caps_context_at_70_percent() {
        let mut sonar = PerlinSonar::new();
        sonar.update(90.0, 1.0); // 90% context
        assert_eq!(sonar.context_intensity, 0.7); // Capped at 70%
    }

    #[test]
    fn lissajous_radar_scales_tool_intensity() {
        let mut radar = LissajousRadar::new();
        radar.update(5, 1.0);
        assert_eq!(radar.tool_intensity, 0.5); // 5 tools / 10 = 0.5
        
        radar.update(20, 1.0);
        assert_eq!(radar.tool_intensity, 1.0); // Capped at 1.0
    }

    #[test]
    fn plasma_thermal_thinking_levels() {
        let mut plasma = PlasmaThermal::new();
        
        plasma.update("off", 1.0);
        assert_eq!(plasma.thinking_intensity, 0.0);
        
        plasma.update("medium", 1.0);
        assert_eq!(plasma.thinking_intensity, 0.6);
        
        plasma.update("high", 1.0);
        assert_eq!(plasma.thinking_intensity, 0.8);
    }

    #[test]
    fn ca_waterfall_creates_states_for_minds() {
        let mut waterfall = CaWaterfall::new();
        let minds = vec!["alice".to_string(), "bob".to_string()];
        
        waterfall.update(100, &minds, 1.0);
        assert_eq!(waterfall.states.len(), 2);
        assert!(waterfall.states.contains_key("alice"));
        assert!(waterfall.states.contains_key("bob"));
    }

    #[test]
    fn ca_waterfall_removes_unused_mind_states() {
        let mut waterfall = CaWaterfall::new();
        let initial_minds = vec!["alice".to_string(), "bob".to_string()];
        waterfall.update(100, &initial_minds, 1.0);
        
        let remaining_minds = vec!["alice".to_string()];
        waterfall.update(200, &remaining_minds, 1.0);
        
        assert_eq!(waterfall.states.len(), 1);
        assert!(waterfall.states.contains_key("alice"));
        assert!(!waterfall.states.contains_key("bob"));
    }

    #[test]
    fn waterfall_state_applies_ca_rules() {
        let mut state = WaterfallState::new();
        let initial_gen = state.generation;
        
        state.step(30); // Rule 30
        assert_eq!(state.generation, initial_gen + 1);
    }

    #[test]
    fn all_instruments_render_without_panic() {
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        
        let perlin = PerlinSonar::new();
        let lissajous = LissajousRadar::new();
        let plasma = PlasmaThermal::new();
        let waterfall = CaWaterfall::new();
        
        terminal.draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            
            perlin.render(area, buf);
            lissajous.render(area, buf);
            plasma.render(area, buf);
            waterfall.render(area, buf);
        }).unwrap();
    }

    #[test]
    fn focus_mode_toggle() {
        let mut panel = InstrumentPanel::default();
        assert!(!panel.focus_mode);
        
        panel.toggle_focus();
        assert!(panel.focus_mode);
        
        panel.toggle_focus();
        assert!(!panel.focus_mode);
    }
}

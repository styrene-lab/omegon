//! Omegon splash screen — glitch-convergence ASCII logo animation.
//!
//! Each character has a randomized unlock frame weighted center-outward.
//! Before unlock it shows a CRT noise glyph; after unlock the final character.
//! Inspired by CRT phosphor aesthetics.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use super::theme::Theme;

// ─── Animation parameters ───────────────────────────────────────────────────

const FRAME_INTERVAL_MS: u64 = 45; // ~22 fps
/// Total convergence frames (~1.7s at 45ms per frame).
pub const TOTAL_FRAMES: u32 = 38;
/// Hold frames after convergence before accepting dismissal.
pub const HOLD_FRAMES: u32 = 8;

/// CRT noise glyphs.
const NOISE_CHARS: &[char] = &[
    '▓', '▒', '░', '█', '▄', '▀', '▌', '▐', '▊', '▋', '▍', '▎', '▏', '◆', '■', '□', '▪',
    '◇', '┼', '╬', '╪', '╫', '┤', '├', '┬', '┴', '╱', '╲', '│', '─',
];

// ─── Seeded RNG — deterministic noise per frame ─────────────────────────────

struct SimpleRng {
    s: u32,
}

impl SimpleRng {
    fn new(seed: u32) -> Self {
        Self { s: seed }
    }

    fn next(&mut self) -> f64 {
        self.s = self.s.wrapping_mul(1664525).wrapping_add(1013904223) & 0x7fffffff;
        self.s as f64 / 0x7fffffff as f64
    }

    fn choice_char(&mut self, chars: &[char]) -> char {
        let idx = (self.next() * chars.len() as f64) as usize;
        chars[idx.min(chars.len() - 1)]
    }
}

// ─── Logo art — sigil (31 rows) + spacer (2) + wordmark (7 rows) ───────────

const MARK_ROWS: usize = 31;

const LOGO_LINES: &[&str] = &[
    "                                                             ..                 ",
    "                 .@.                               .@@ .    .@                  ",
    "         .. ..*@@@:.                         ...+@@@*.#@@@...@@.                ",
    "      .=@@..@@@@=@@@@@@@.                  .@@@@@@@@@@@@@@@@.@@@.               ",
    " .@@@@@@@@@@@@@@@@@@@@@@..              ....@@@@@@@@@@@@@@@@@@@@@@..            ",
    " ... .@@@@@@@@@..@@@@@@@@@               %@@@@@@@@@@@@@@@@@@@@@@@@@@.           ",
    " .@@=.     ...     .@@@@@@..           .@@@@@@@@@@@@:. ..#@@@@@@@@@.@@@         ",
    "  +.                #@@@@@@            @.@@@@@@@@@          @@@@@@@@..@@.       ",
    "                    @@@@@@@.           .@@@@@@@@@%          .@@@@@@@@@@@*.      ",
    "                   =@@@@@.@.         ...@@@@@@@@@@           .@@@@@@@@@@@@..    ",
    "                 ..@@@@@@#       .@@@@@@@@@@@@@@@%:             ..:@@@.@@@@@    ",
    "                  @@@@@@.  ..%@@@@@@@@@@@@@@@@@@@@@@@@.              @@.  @@    ",
    "                 .@@@@@@..*@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@..          ..@@@.     ",
    "                  @@@@@%+@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@.           .@%      ",
    "                   @@@ @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@=                   ",
    "                   .@.@@@@@@@@@@@@@@@@@@:::@@@@@@@@@@@@@@@@@@@.                 ",
    "                    .@@@@@@@@@@@@@.  .@.@@@@@@@@.=@@@@@@@@@@@@%.                ",
    "                    @@@@@@@@@@@*@@@.    .@@@@@@@@@@ #@@@@@@@@@@.   ... ...      ",
    "                    @@@@@@@@@.@@@@@@@    .@@@@@@@@@@@.@@@@@@@@@@.=@@@@@.@.      ",
    "                    @@@@@@@@.@@@@@@@.@.   ..@@@@@@@@@@.@@@@@@@@@@@@@@@@@@@.     ",
    "                   .@@@@@@@*. @@@@@@@..     ..@@.@@@@@@.@@@@@@@=@@@@..#@@@@@    ",
    "                    @@@@@@@. :@@@@@@@@         ..=. ..@.@@@@@@@%@@@=.  @@@@#.   ",
    "                .   .@@@@@@@@@@@@@@@@+.                .@@@@@@#.@@@@@  ..@@@@ . ",
    "                .+   .@@@@@@.@@@@@@.@                 .@@@@@@@. .@@@@@.   @=%@. ",
    "          ....@@@.  ...@@@@@@#@@@@..                 .@@@@@@=    ..@@@@.. .#.   ",
    "         =+.@@@@@@@@@   =@@@@@@...                 .@@@@@@@.     #.*@@@   ..    ",
    "     .@@@@@@@@@:@@@@@.:. .=@@@@@@@..           ..@@@@@@@@    ..@@@@@@@@         ",
    "     ...@@*  .. ..+.@@@@@@@@@@@@@@@@@@@.    @@@@@@@@@@@@@@@@@@@@@@@#..          ",
    "      @..           @@@@@@@@@@@@@@@@@@@.    @@@@@@@@@@@@@@@@@@@@@@              ",
    "                  @ @@@@@@@@@@@@@@@@@@@.    @@@@@@@@@@@@@@@@@@@@#               ",
    "               .@@@ @@@@@@@@@@@@@@@@@@@.    @@@@@@@@@@@@@@@@@@@@                ",
    // spacer
    "                                                                                ",
    "                                                                                ",
    // wordmark (7 rows)
    "      ...     .  .. .    ... .         . . .      .         ...        .        ",
    "      @@@@@@@@@@  @@@..  .@@@  .@@@@@@@@ .@@@@@@@@@  @@@@@@@@@. =@@@=  @@@      ",
    "      @@@    @@@  @@@@. .@@@@  .@@...... .@@         @@=    @@  =@@@@%.@@@      ",
    "      @@@    @@@  @@.@@.@@.@@  .@@@@@@@. .@@  @@@@@  @@=    @@  =@@.@@@@@@      ",
    "      @@@    @@@  @@ =@@@=.@@  .@@       .@@    .@@  @@=    @@  =@@  @@@@@      ",
    "      *@@@@@@@@:  @@ .@@%..@@  .@@@@@@@@ .@@@@@@@@@ .@@@@@@@@@  =@@  .#@@@      ",
    "       ..     .  .. .     .. . ..      .  ..   .. .   .     .   . ..            ",
];

const COMPACT_MARK_ROWS: usize = 23;

const COMPACT_LOGO_LINES: &[&str] = &[
    "            *                      ```     #`          ",
    "     ` ```##`                   ``````##` .#`          ",
    "````##`#########             `############`##`         ",
    "*`*##############           `##################`       ",
    "##:````*`   `####`         `#########` *#######:##     ",
    "`            #####        ``#######       ######`#`    ",
    "            `#####         #######.        #########`  ",
    "            ####``   ``*@@@@@@@@@@`*          `## #### ",
    "           #####  `@@@@@@@@@@@@@@@@@@@          `#@ :` ",
    "           #####`@@@@@@@@@@@@@@@@@@@@@@@@`        `#`  ",
    "            ##*@@@@@@@@@@@@@@@@@@@@@@@@@@@`            ",
    "             :@@@@@@@@@@@``##```@@@@@@@@@@@``          ",
    "             @@@@@@@@*#:`  `#######`@@@@@@@@`  `   `   ",
    "             @@@@@@@#####`  `########`@@@@@@@`####`#`  ",
    "             @@@@@@ ######    `#`#####`@@@@@@########` ",
    "             @@@@@  ######      `::``#*@@@@@`##`  #### ",
    "             `@@@@@#######            `@@@@@`###` `*## ",
    "        ``#`  .@@@@`#####            `@@@@@` ``###` `**",
    "    ``:######```@@@@@#`            `.@@@@.   `#.##`    ",
    "   ######`####`* `@@@@@@        ``@@@@@`  ``#####`     ",
    "   #*       .@@@@@@@@@@@@@@   :@@@@@@@@@@@@@@##        ",
    "   `        .@@@@@@@@@@@@@@   :@@@@@@@@@@@@@@`         ",
    "         .@  `                              `          ",
    // spacer
    "                                                       ",
    // wordmark (4 rows)
    "   @@@@@@@ @@@` `@@@ @@@@@@``@@@@@@ `@@@@@@@`@@@` @@  ",
    "   @@   @@ @@@@`@@@@ @@```` `@@`    `@@   @@ @@@@ @@  ",
    "   @@   @@ @@ @*@`@@ @@@@`  `@@`@@@ `@@   @@ @@ *@@@  ",
    "   @@@@@@@ @@ `@``@@ @@@@@@``@@@@@@ `@@@@@@@`@@  `@@  ",
];

// ─── Unlock frame assignment ────────────────────────────────────────────────

/// Per-character timing: (appear_frame, unlock_frame).
type FrameMap = Vec<Vec<(u32, u32)>>;

fn assign_unlock_frames(lines: &[&str], total: u32, seed: u32) -> FrameMap {
    let mut rng = SimpleRng::new(seed);
    let height = lines.len();
    let cascade_end = (total as f64 * 0.55) as u32;
    let max_glitch = (total as f64 * 0.40) as u32;

    lines
        .iter()
        .enumerate()
        .map(|(y, line)| {
            let base_appear =
                ((y as f64 / (height.saturating_sub(1).max(1)) as f64) * cascade_end as f64) as u32;
            let cx = line.len() as f64 / 2.0;

            line.chars()
                .enumerate()
                .map(|(x, ch)| {
                    if ch == ' ' {
                        return (0, 0);
                    }
                    let appear = base_appear + (rng.next() * 3.0) as u32;
                    let dist_from_cx = (x as f64 - cx).abs() / cx.max(1.0);
                    let hi =
                        4u32.max((max_glitch as f64 * (0.35 + 0.65 * (1.0 - dist_from_cx))) as u32);
                    let lo = 3u32.max((hi as f64 * 0.25) as u32);
                    let unlock = (appear + lo + (rng.next() * (hi - lo + 1) as f64) as u32)
                        .min(total.saturating_sub(2));
                    (appear, unlock)
                })
                .collect()
        })
        .collect()
}

// ─── Render a single animation frame ────────────────────────────────────────

fn render_frame_lines<'a>(
    frame: u32,
    lines: &[&str],
    frame_map: &FrameMap,
    noise_seed: u32,
    mark_rows: usize,
    t: &dyn Theme,
) -> Vec<Line<'a>> {
    let mut rng = SimpleRng::new(noise_seed.wrapping_add(frame.wrapping_mul(997)));
    let mut output = Vec::with_capacity(lines.len());

    for (y, line_str) in lines.iter().enumerate() {
        let row = &frame_map[y];
        let mut spans: Vec<Span<'a>> = Vec::new();
        let mut current_text = String::new();
        let mut current_style: Option<Style> = None;

        for (x, ch) in line_str.chars().enumerate() {
            let (appear, unlock) = row[x];

            let (display_ch, style) = if ch == ' ' {
                (' ', Style::default())
            } else if frame < appear {
                // Not yet visible
                (' ', Style::default())
            } else if frame >= unlock {
                // Resolved — final glyph
                let color = if y > mark_rows {
                    Style::default()
                        .fg(t.accent_bright())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(t.accent())
                };
                (ch, color)
            } else {
                // Glitching — CRT noise
                let noise = rng.choice_char(NOISE_CHARS);
                let progress =
                    (frame - appear) as f64 / (unlock.saturating_sub(appear).max(1)) as f64;
                let color = if frame == appear {
                    Style::default().fg(t.accent_bright()) // arrival flash
                } else if progress > 0.65 {
                    Style::default().fg(t.dim()) // dimming as it converges
                } else {
                    Style::default().fg(t.accent_muted())
                };
                (noise, color)
            };

            // Batch spans with the same style
            if Some(style) == current_style {
                current_text.push(display_ch);
            } else {
                if !current_text.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current_text),
                        current_style.unwrap_or_default(),
                    ));
                }
                current_text.push(display_ch);
                current_style = Some(style);
            }
        }

        if !current_text.is_empty() {
            spans.push(Span::styled(current_text, current_style.unwrap_or_default()));
        }

        output.push(Line::from(spans));
    }

    output
}

// ─── Loading checklist ──────────────────────────────────────────────────────

/// Loading subsystem status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Pending,
    Active,
    Done,
    Failed,
}

/// A loading checklist item.
#[derive(Debug, Clone)]
pub struct LoadItem {
    pub label: &'static str,
    pub state: LoadState,
    pub summary: Option<String>,
}

const SCAN_GLYPHS: &[&str] = &["░ ", "▒ ", "▓ ", "▒ ", "░ ", "▸ ", "▸ ", "▸ "];

/// Render the checklist as a multi-row grid (3 columns).
fn render_grid<'a>(items: &[LoadItem], scan_frame: usize, col_width: usize, t: &dyn Theme) -> Vec<Line<'a>> {
    let cols = 3usize;
    let rows = (items.len() + cols - 1) / cols;
    let mut output = Vec::with_capacity(rows);

    for row in 0..rows {
        let mut spans: Vec<Span<'a>> = Vec::new();
        for col in 0..cols {
            let idx = row * cols + col; // row-major: items fill left-to-right, top-to-bottom
            if idx >= items.len() {
                break;
            }
            let item = &items[idx];

            let (indicator, ind_style) = match item.state {
                LoadState::Pending => ("· ", Style::default().fg(t.dim())),
                LoadState::Active => {
                    let glyph = SCAN_GLYPHS[scan_frame % SCAN_GLYPHS.len()];
                    (glyph, Style::default().fg(t.accent()))
                }
                LoadState::Done => ("✓ ", Style::default().fg(t.success())),
                LoadState::Failed => ("✗ ", Style::default().fg(t.error())),
            };

            let label_style = match item.state {
                LoadState::Pending => Style::default().fg(t.dim()),
                LoadState::Active => Style::default().fg(t.accent()),
                LoadState::Done => Style::default().fg(t.muted()),
                LoadState::Failed => Style::default().fg(t.error()),
            };

            // Build cell text: "label (summary)" or just "label"
            let cell_text = if let Some(ref summary) = item.summary {
                if summary == "none" || summary == "not found" || summary == "empty" {
                    item.label.to_string()
                } else {
                    format!("{} ({})", item.label, summary)
                }
            } else {
                item.label.to_string()
            };

            // Pad to column width
            let padded = format!("{:<width$}", cell_text, width = col_width);

            spans.push(Span::styled(indicator.to_string(), ind_style));
            spans.push(Span::styled(padded, label_style));
        }
        output.push(Line::from(spans));
    }

    output
}

// ─── Splash state machine ───────────────────────────────────────────────────

/// Tier of logo art to use based on terminal size.
#[derive(Debug, Clone, Copy)]
enum LogoTier {
    Full,    // sigil + wordmark (84+ cols, 46+ rows)
    Compact, // smaller sigil + wordmark (58+ cols, 34+ rows)
    None,    // terminal too small — skip splash
}

fn select_tier(cols: u16, rows: u16) -> LogoTier {
    let full_width = LOGO_LINES.iter().map(|l| l.len()).max().unwrap_or(80) as u16 + 4;
    let full_height = LOGO_LINES.len() as u16 + 6;
    let compact_width = COMPACT_LOGO_LINES.iter().map(|l| l.len()).max().unwrap_or(54) as u16 + 4;
    let compact_height = COMPACT_LOGO_LINES.len() as u16 + 6;

    if cols >= full_width && rows >= full_height {
        LogoTier::Full
    } else if cols >= compact_width && rows >= compact_height {
        LogoTier::Compact
    } else {
        LogoTier::None
    }
}

/// Splash screen state. Drives the animation from `run_tui`.
pub struct SplashScreen {
    pub frame: u32,
    scan_frame: usize,
    frame_map: FrameMap,
    noise_seed: u32,
    lines: &'static [&'static str],
    mark_rows: usize,
    logo_width: usize,
    pub hold_count: u32,
    anim_done: bool,
    pub dismissed: bool,
    items: Vec<LoadItem>,
    prompt_blink: bool,
}

impl SplashScreen {
    /// Create a splash screen, or None if the terminal is too small.
    pub fn new(cols: u16, rows: u16) -> Option<Self> {
        let tier = select_tier(cols, rows);

        let (lines, mark_rows): (&'static [&'static str], usize) = match tier {
            LogoTier::Full => (LOGO_LINES, MARK_ROWS),
            LogoTier::Compact => (COMPACT_LOGO_LINES, COMPACT_MARK_ROWS),
            LogoTier::None => return None,
        };

        let logo_width = lines.iter().map(|l| l.len()).max().unwrap_or(80);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_millis() & 0xffff) as u32)
            .unwrap_or(42);

        let frame_map = assign_unlock_frames(lines, TOTAL_FRAMES, seed);
        let noise_seed = seed.wrapping_mul(7) & 0x7fffffff;

        Some(Self {
            frame: 0,
            scan_frame: 0,
            frame_map,
            noise_seed,
            lines,
            mark_rows,
            logo_width,
            hold_count: 0,
            anim_done: false,
            dismissed: false,
            items: vec![
                LoadItem { label: "cloud", state: LoadState::Pending, summary: None },
                LoadItem { label: "local", state: LoadState::Pending, summary: None },
                LoadItem { label: "hardware", state: LoadState::Pending, summary: None },
                LoadItem { label: "memory", state: LoadState::Pending, summary: None },
                LoadItem { label: "tools", state: LoadState::Pending, summary: None },
                LoadItem { label: "design", state: LoadState::Pending, summary: None },
                LoadItem { label: "secrets", state: LoadState::Pending, summary: None },
                LoadItem { label: "container", state: LoadState::Pending, summary: None },
                LoadItem { label: "mcp", state: LoadState::Pending, summary: None },
            ],
            prompt_blink: false,
        })
    }

    /// Advance one animation frame. Call at ~22fps (45ms intervals).
    pub fn tick(&mut self) {
        if self.dismissed {
            return;
        }

        self.frame += 1;
        self.scan_frame = (self.scan_frame + 1) % SCAN_GLYPHS.len();

        if self.frame >= TOTAL_FRAMES && !self.anim_done {
            self.anim_done = true;
        }

        if self.anim_done {
            self.hold_count += 1;
            if self.hold_count.is_multiple_of(10) {
                self.prompt_blink = !self.prompt_blink;
            }
        }
    }

    /// True when animation is done and loading complete — ready for keypress dismissal.
    pub fn ready_to_dismiss(&self) -> bool {
        self.anim_done
            && self.hold_count >= HOLD_FRAMES
            && self.items.iter().all(|i| matches!(i.state, LoadState::Done | LoadState::Failed))
    }

    /// Dismiss the splash (on keypress or auto).
    pub fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// Update a loading item's state.
    pub fn set_load_state(&mut self, label: &str, state: LoadState) {
        if let Some(item) = self.items.iter_mut().find(|i| i.label == label) {
            item.state = state;
        }
    }

    /// Receive a probe result from the startup systems check.
    pub fn receive_probe(&mut self, result: crate::startup::ProbeResult) {
        if let Some(item) = self.items.iter_mut().find(|i| i.label == result.label) {
            item.state = match result.state {
                crate::startup::ProbeState::Done => LoadState::Done,
                crate::startup::ProbeState::Failed => LoadState::Failed,
            };
            item.summary = Some(result.summary);
        }
    }

    /// Mark all items as done (safety timeout).
    pub fn force_done(&mut self) {
        for item in &mut self.items {
            if matches!(item.state, LoadState::Pending | LoadState::Active) {
                item.state = LoadState::Done;
            }
        }
    }

    /// The frame interval for the animation timer.
    pub fn frame_interval() -> std::time::Duration {
        std::time::Duration::from_millis(FRAME_INTERVAL_MS)
    }

    /// Render the splash screen into a frame.
    pub fn draw(&self, frame: &mut ratatui::Frame, t: &dyn Theme) {
        let area = frame.area();

        // Fill background
        let bg_block = ratatui::widgets::Block::default()
            .style(Style::default().bg(t.bg()));
        frame.render_widget(bg_block, area);

        let mut lines: Vec<Line<'_>> = Vec::new();

        // Render logo frame
        let logo_frame = render_frame_lines(
            self.frame.min(TOTAL_FRAMES),
            self.lines,
            &self.frame_map,
            self.noise_seed,
            self.mark_rows,
            t,
        );

        // Vertically center
        let content_height = logo_frame.len() + 4; // logo + checklist + prompt + spacers
        let top_pad = (area.height as usize).saturating_sub(content_height) / 2;
        for _ in 0..top_pad {
            lines.push(Line::from(""));
        }

        // Horizontally center — add padding to each logo line
        let h_pad = (area.width as usize).saturating_sub(self.logo_width) / 2;
        let pad_str: String = " ".repeat(h_pad);

        for logo_line in &logo_frame {
            let mut padded_spans = vec![Span::raw(pad_str.clone())];
            padded_spans.extend(logo_line.spans.iter().cloned());
            lines.push(Line::from(padded_spans));
        }

        // Checklist grid
        if !self.dismissed {
            lines.push(Line::from(""));

            // Calculate column width from terminal width
            let cols = 3usize;
            let indicator_width = 2; // "✓ "
            let total_indicator = indicator_width * cols;
            let available = (area.width as usize).saturating_sub(total_indicator + 6); // 6 for padding
            let col_width = available / cols;
            let grid_width = (col_width + indicator_width) * cols;
            let cl_pad = (area.width as usize).saturating_sub(grid_width) / 2;

            let grid_lines = render_grid(&self.items, self.scan_frame, col_width, t);
            for grid_line in &grid_lines {
                let mut padded = vec![Span::raw(" ".repeat(cl_pad))];
                padded.extend(grid_line.spans.iter().cloned());
                lines.push(Line::from(padded));
            }

            // "press any key" prompt
            if self.ready_to_dismiss() {
                lines.push(Line::from(""));
                let prompt = "press any key to continue";
                let p_pad = (area.width as usize).saturating_sub(prompt.len()) / 2;
                let color = if self.prompt_blink { t.dim() } else { t.accent() };
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(p_pad)),
                    Span::styled(prompt, Style::default().fg(color)),
                ]));
            }
        }

        let widget = Paragraph::new(lines);
        frame.render_widget(widget, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic() {
        let mut a = SimpleRng::new(42);
        let mut b = SimpleRng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next().to_bits(), b.next().to_bits());
        }
    }

    #[test]
    fn unlock_frames_within_bounds() {
        let lines = &["hello world", "  test  "];
        let map = assign_unlock_frames(lines, 38, 42);
        for row in &map {
            for &(appear, unlock) in row {
                assert!(unlock <= 38, "unlock frame exceeds total");
                assert!(appear <= unlock || (appear == 0 && unlock == 0));
            }
        }
    }

    #[test]
    fn splash_none_for_tiny_terminal() {
        assert!(SplashScreen::new(40, 10).is_none());
    }

    #[test]
    fn splash_some_for_large_terminal() {
        assert!(SplashScreen::new(120, 50).is_some());
    }

    #[test]
    fn splash_lifecycle() {
        let mut s = SplashScreen::new(120, 50).unwrap();
        assert!(!s.ready_to_dismiss());

        // Advance past animation
        for _ in 0..50 {
            s.tick();
        }
        // Still not ready — loading items pending
        assert!(!s.ready_to_dismiss());

        s.force_done();
        // Now ready
        assert!(s.ready_to_dismiss());

        s.dismiss();
        assert!(s.dismissed);
    }

    #[test]
    fn render_frame_produces_correct_line_count() {
        let lines = LOGO_LINES;
        let map = assign_unlock_frames(lines, TOTAL_FRAMES, 42);
        let t = crate::tui::theme::Alpharius;
        let rendered = render_frame_lines(0, lines, &map, 42, MARK_ROWS, &t);
        assert_eq!(rendered.len(), lines.len());
    }

    #[test]
    fn compact_logo_renders() {
        let lines = COMPACT_LOGO_LINES;
        let map = assign_unlock_frames(lines, TOTAL_FRAMES, 42);
        let t = crate::tui::theme::Alpharius;
        let rendered = render_frame_lines(TOTAL_FRAMES, lines, &map, 42, COMPACT_MARK_ROWS, &t);
        assert_eq!(rendered.len(), lines.len());
    }

    #[test]
    fn set_load_state_works() {
        let mut s = SplashScreen::new(120, 50).unwrap();
        s.set_load_state("memory", LoadState::Active);
        assert_eq!(
            s.items.iter().find(|i| i.label == "memory").unwrap().state,
            LoadState::Active,
        );
        s.set_load_state("memory", LoadState::Done);
        assert_eq!(
            s.items.iter().find(|i| i.label == "memory").unwrap().state,
            LoadState::Done,
        );
    }

    #[test]
    fn nine_items_initialized() {
        let s = SplashScreen::new(120, 50).unwrap();
        assert_eq!(s.items.len(), 9);
        let labels: Vec<&str> = s.items.iter().map(|i| i.label).collect();
        assert!(labels.contains(&"cloud"));
        assert!(labels.contains(&"local"));
        assert!(labels.contains(&"hardware"));
        assert!(labels.contains(&"memory"));
        assert!(labels.contains(&"tools"));
        assert!(labels.contains(&"design"));
        assert!(labels.contains(&"secrets"));
        assert!(labels.contains(&"container"));
        assert!(labels.contains(&"mcp"));
    }

    #[test]
    fn receive_probe_updates_item() {
        let mut s = SplashScreen::new(120, 50).unwrap();
        s.receive_probe(crate::startup::ProbeResult {
            label: "cloud",
            state: crate::startup::ProbeState::Done,
            summary: "anthropic, openai".into(),
        });
        let item = s.items.iter().find(|i| i.label == "cloud").unwrap();
        assert_eq!(item.state, LoadState::Done);
        assert_eq!(item.summary.as_deref(), Some("anthropic, openai"));
    }

    #[test]
    fn receive_probe_failed_maps_correctly() {
        let mut s = SplashScreen::new(120, 50).unwrap();
        s.receive_probe(crate::startup::ProbeResult {
            label: "container",
            state: crate::startup::ProbeState::Failed,
            summary: "not found".into(),
        });
        let item = s.items.iter().find(|i| i.label == "container").unwrap();
        assert_eq!(item.state, LoadState::Failed);
    }

    #[test]
    fn grid_renders_without_panic() {
        let t = crate::tui::theme::Alpharius;
        let items = vec![
            LoadItem { label: "cloud", state: LoadState::Done, summary: Some("anthropic".into()) },
            LoadItem { label: "local", state: LoadState::Active, summary: None },
            LoadItem { label: "hardware", state: LoadState::Done, summary: Some("M2, 32GB".into()) },
            LoadItem { label: "memory", state: LoadState::Failed, summary: Some("not found".into()) },
            LoadItem { label: "tools", state: LoadState::Done, summary: Some("48 registered".into()) },
            LoadItem { label: "design", state: LoadState::Pending, summary: None },
        ];
        let lines = render_grid(&items, 0, 24, &t);
        assert_eq!(lines.len(), 2, "6 items / 3 cols = 2 rows");
        // Each line should have spans
        for line in &lines {
            assert!(!line.spans.is_empty());
        }
    }

    #[test]
    fn grid_single_item() {
        let t = crate::tui::theme::Alpharius;
        let items = vec![
            LoadItem { label: "test", state: LoadState::Done, summary: Some("ok".into()) },
        ];
        let lines = render_grid(&items, 0, 20, &t);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn grid_empty() {
        let t = crate::tui::theme::Alpharius;
        let lines = render_grid(&[], 0, 20, &t);
        assert!(lines.is_empty());
    }
}

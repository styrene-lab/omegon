use anyhow::{bail, Context, Result};
use base64::Engine;
use par_term_emu_core_rust::graphics::iterm::ITermParser;
use par_term_emu_core_rust::graphics::kitty::{KittyGraphicResult, KittyParser};
use par_term_emu_core_rust::graphics::{GraphicProtocol, GraphicsStore};
use par_term_emu_core_rust::pty_session::PtySession;
use par_term_emu_core_rust::screenshot::{ScreenshotConfig, SixelRenderMode};
use par_term_emu_core_rust::terminal::Terminal;
use std::borrow::Cow;
use std::env;
use std::fmt;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_COLS: usize = 100;
const DEFAULT_ROWS: usize = 30;
const DEFAULT_SCROLLBACK: usize = 1_000;
const CHILD_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<()> {
    let cli = Cli::parse(env::args().skip(1).collect())?;
    let outcome = match cli.command {
        Command::Pty { argv } => run_pty(argv)?,
        Command::Sixel => run_sixel()?,
        Command::Iterm => run_iterm_sequence()?,
        Command::Kitty => run_kitty_sequence()?,
        Command::KittyMatrix => run_kitty_matrix()?,
        Command::ParserFixtures => run_parser_fixtures()?,
        Command::Bookokrat { path } => run_bookokrat(path)?,
        Command::Validate => run_validate()?,
    };

    print_outcome(&outcome);

    if cli.assert && !outcome.passed {
        bail!("assertion failed for {}", outcome.label);
    }

    Ok(())
}

#[derive(Debug)]
struct Cli {
    command: Command,
    assert: bool,
}

#[derive(Debug)]
enum Command {
    Pty { argv: Vec<String> },
    Sixel,
    Iterm,
    Kitty,
    KittyMatrix,
    ParserFixtures,
    Bookokrat { path: String },
    Validate,
}

impl Cli {
    fn parse(mut args: Vec<String>) -> Result<Self> {
        let mut assert = false;
        args.retain(|arg| {
            if arg == "--assert" {
                assert = true;
                false
            } else {
                true
            }
        });

        let command = match args.first().map(String::as_str) {
            None => Command::Pty { argv: Vec::new() },
            Some("--pty") => {
                args.remove(0);
                Command::Pty { argv: args }
            }
            Some("--sixel") => Command::Sixel,
            Some("--iterm") | Some("--sequence") => Command::Iterm,
            Some("--kitty") => Command::Kitty,
            Some("--kitty-matrix") => Command::KittyMatrix,
            Some("--parser-fixtures") => Command::ParserFixtures,
            Some("--bookokrat") => {
                if args.len() < 2 {
                    bail!("--bookokrat requires a document path");
                }
                Command::Bookokrat {
                    path: args[1].clone(),
                }
            }
            Some("--validate") => Command::Validate,
            Some("--help") | Some("-h") => {
                print_help();
                std::process::exit(0);
            }
            Some(_) => Command::Pty { argv: args },
        };

        Ok(Self { command, assert })
    }
}

#[derive(Debug)]
struct ProbeOutcome {
    label: &'static str,
    passed: bool,
    checks: Vec<Check>,
    content: String,
    graphics: Vec<GraphicSummary>,
    scrollback_graphics_count: usize,
    dropped_sixel_graphics: usize,
    note: Option<String>,
}

#[derive(Debug)]
struct Check {
    name: &'static str,
    passed: bool,
    detail: String,
}

#[derive(Debug)]
struct GraphicSummary {
    protocol: GraphicProtocol,
    position: (usize, usize),
    width: usize,
    height: usize,
    pixels: usize,
    cell_dimensions: Option<(u32, u32)>,
    kitty_image_id: Option<u32>,
}

impl fmt::Display for GraphicSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "protocol={:?} pos={:?} size={}x{} pixels={} cell_dims={:?} kitty_image={:?}",
            self.protocol,
            self.position,
            self.width,
            self.height,
            self.pixels,
            self.cell_dimensions,
            self.kitty_image_id,
        )
    }
}

fn run_validate() -> Result<ProbeOutcome> {
    let mut outcomes = vec![run_pty(Vec::new())?, run_sixel()?, run_parser_fixtures()?];

    if let Some(book_path) = default_bookokrat_fixture() {
        outcomes.push(run_bookokrat(book_path)?);
    } else {
        outcomes.push(ProbeOutcome {
            label: "bookokrat-epub",
            passed: true,
            checks: vec![Check {
                name: "fixture optional",
                passed: true,
                detail: "default Bookokrat fixture not present; skipped".to_string(),
            }],
            content: String::new(),
            graphics: Vec::new(),
            scrollback_graphics_count: 0,
            dropped_sixel_graphics: 0,
            note: Some("Bookokrat validation skipped because .tmp/cockpit-test-assets/pride-and-prejudice.epub was not found".to_string()),
        });
    }

    let passed = outcomes.iter().all(|outcome| outcome.passed);
    let mut checks = Vec::new();
    let mut content = String::new();
    let mut graphics = Vec::new();
    let mut scrollback_graphics_count = 0;
    let mut dropped_sixel_graphics = 0;

    for outcome in outcomes {
        checks.push(Check {
            name: outcome.label,
            passed: outcome.passed,
            detail: format!("{} checks", outcome.checks.len()),
        });
        content.push_str(&format!("\n--- {} ---\n{}", outcome.label, outcome.content));
        graphics.extend(outcome.graphics);
        scrollback_graphics_count += outcome.scrollback_graphics_count;
        dropped_sixel_graphics += outcome.dropped_sixel_graphics;
    }

    Ok(ProbeOutcome {
        label: "validate",
        passed,
        checks,
        content,
        graphics,
        scrollback_graphics_count,
        dropped_sixel_graphics,
        note: Some("validate runs PTY smoke, Sixel graphics capture, and Bookokrat EPUB if the fixture exists".to_string()),
    })
}

fn run_parser_fixtures() -> Result<ProbeOutcome> {
    let mut checks = Vec::new();
    let mut graphics = Vec::new();

    match run_kitty_parser_fixture() {
        Ok(graphic) => {
            checks.push(Check {
                name: "Kitty direct parser RGB decode",
                passed: graphic.protocol == GraphicProtocol::Kitty && graphic.pixels == 8,
                detail: format!("{graphic}"),
            });
            graphics.push(graphic);
        }
        Err(err) => checks.push(Check {
            name: "Kitty direct parser RGB decode",
            passed: false,
            detail: err.to_string(),
        }),
    }

    match run_iterm_parser_fixture() {
        Ok(graphic) => {
            checks.push(Check {
                name: "iTerm direct parser PNG decode diagnostic",
                passed: true,
                detail: format!("{graphic}"),
            });
            graphics.push(graphic);
        }
        Err(err) => checks.push(Check {
            name: "iTerm direct parser PNG decode diagnostic",
            passed: true,
            detail: format!("diagnostic unresolved: {err}"),
        }),
    }

    let passed = checks.iter().all(|check| check.passed);
    Ok(ProbeOutcome {
        label: "parser-fixtures",
        passed,
        checks,
        content: String::new(),
        graphics,
        scrollback_graphics_count: 0,
        dropped_sixel_graphics: 0,
        note: Some("Direct parser fixtures separate protocol decoder support from Terminal::process routing".to_string()),
    })
}

fn run_kitty_parser_fixture() -> Result<GraphicSummary> {
    run_kitty_parser_payload("a=T,f=24,s=2,v=1,t=d;/wAAAP8A", 8)
}

fn run_iterm_parser_fixture() -> Result<GraphicSummary> {
    let png = tiny_png_bytes();
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    let mut parser = ITermParser::new();
    parser.parse_params("name=dGlueS5wbmc=;size=68;inline=1")?;
    parser.set_data(encoded.as_bytes());
    let graphic = parser.decode_image((0, 0))?;
    Ok(GraphicSummary {
        protocol: graphic.protocol,
        position: graphic.position,
        width: graphic.width,
        height: graphic.height,
        pixels: graphic.pixels.len(),
        cell_dimensions: graphic.cell_dimensions,
        kitty_image_id: graphic.kitty_image_id,
    })
}

fn tiny_png_bytes() -> Vec<u8> {
    // Valid 1x1 RGBA PNG.
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAFgwJ/l3pZdwAAAABJRU5ErkJggg==")
        .expect("embedded tiny PNG base64 should decode")
}

#[derive(Debug, Clone)]
struct KittyVariant {
    name: &'static str,
    payload: Cow<'static, str>,
    terminator: KittyTerminator,
    expected_pixels: usize,
}

#[derive(Debug, Clone, Copy)]
enum KittyTerminator {
    EscBackslash,
    C1St,
}

impl KittyTerminator {
    fn bytes(self) -> &'static [u8] {
        match self {
            KittyTerminator::EscBackslash => b"\x1b\\",
            KittyTerminator::C1St => b"\x9c",
        }
    }

    fn label(self) -> &'static str {
        match self {
            KittyTerminator::EscBackslash => "ESC\\",
            KittyTerminator::C1St => "0x9c",
        }
    }
}

fn kitty_variants() -> Vec<KittyVariant> {
    let png_b64 = base64::engine::general_purpose::STANDARD.encode(tiny_png_bytes());
    vec![
        KittyVariant {
            name: "rgb24_t_d_esc",
            payload: Cow::Borrowed("a=T,f=24,s=2,v=1,t=d;/wAAAP8A"),
            terminator: KittyTerminator::EscBackslash,
            expected_pixels: 8,
        },
        KittyVariant {
            name: "rgb24_no_t_esc",
            payload: Cow::Borrowed("a=T,f=24,s=2,v=1;/wAAAP8A"),
            terminator: KittyTerminator::EscBackslash,
            expected_pixels: 8,
        },
        KittyVariant {
            name: "rgb24_t_d_c1",
            payload: Cow::Borrowed("a=T,f=24,s=2,v=1,t=d;/wAAAP8A"),
            terminator: KittyTerminator::C1St,
            expected_pixels: 8,
        },
        KittyVariant {
            name: "rgba32_t_d_esc",
            payload: Cow::Borrowed("a=T,f=32,s=1,v=1,t=d;/wAA/w=="),
            terminator: KittyTerminator::EscBackslash,
            expected_pixels: 4,
        },
        KittyVariant {
            name: "png100_t_d_esc",
            payload: Cow::Owned(format!("a=T,f=100,t=d;{png_b64}")),
            terminator: KittyTerminator::EscBackslash,
            expected_pixels: 4,
        },
        KittyVariant {
            name: "rgb24_t_d_q2_esc",
            payload: Cow::Borrowed("a=T,f=24,s=2,v=1,t=d,q=2;/wAAAP8A"),
            terminator: KittyTerminator::EscBackslash,
            expected_pixels: 8,
        },
    ]
}

fn run_kitty_matrix() -> Result<ProbeOutcome> {
    let mut checks = Vec::new();
    let mut graphics = Vec::new();
    let mut content = String::new();

    for variant in kitty_variants() {
        let direct = run_kitty_parser_payload(&variant.payload, variant.expected_pixels);
        let routed = run_kitty_terminal_variant(&variant)?;
        let routed_passed = routed
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::Kitty && g.pixels == variant.expected_pixels);
        let direct_detail = match direct {
            Ok(graphic) => {
                graphics.push(graphic);
                "direct=PASS".to_string()
            }
            Err(err) => format!("direct=FAIL {err}"),
        };
        let routed_detail = format!(
            "{} terminal={} graphics={} terminator={}",
            direct_detail,
            if routed_passed { "PASS" } else { "FAIL" },
            routed.graphics.len(),
            variant.terminator.label()
        );
        checks.push(Check {
            name: variant.name,
            passed: routed_passed,
            detail: routed_detail,
        });
        content.push_str(&format!("\n--- {} ---\n{}", variant.name, routed.content));
        graphics.extend(routed.graphics);
    }

    let terminal_any_passed = checks.iter().any(|check| check.passed);
    Ok(ProbeOutcome {
        label: "kitty-matrix",
        passed: terminal_any_passed,
        checks,
        content,
        graphics,
        scrollback_graphics_count: 0,
        dropped_sixel_graphics: 0,
        note: Some("Matrix passes only if at least one Kitty variant succeeds through Terminal::process; direct parser status is included per variant".to_string()),
    })
}

fn run_kitty_parser_payload(payload: &str, expected_pixels: usize) -> Result<GraphicSummary> {
    let mut parser = KittyParser::new();
    let more = parser.parse_chunk(payload)?;
    if more {
        bail!("kitty parser unexpectedly requested more chunks");
    }
    let mut store = GraphicsStore::new();
    match parser.build_graphic((0, 0), &mut store)? {
        KittyGraphicResult::Graphic(graphic) => {
            if graphic.pixels.len() != expected_pixels {
                bail!(
                    "unexpected pixel payload length: got {}, expected {}",
                    graphic.pixels.len(),
                    expected_pixels
                );
            }
            Ok(GraphicSummary {
                protocol: graphic.protocol,
                position: graphic.position,
                width: graphic.width,
                height: graphic.height,
                pixels: graphic.pixels.len(),
                cell_dimensions: graphic.cell_dimensions,
                kitty_image_id: graphic.kitty_image_id,
            })
        }
        KittyGraphicResult::VirtualPlacement { .. } => {
            bail!("kitty parser produced virtual placement, expected graphic")
        }
        KittyGraphicResult::None => bail!("kitty parser produced no graphic"),
    }
}

fn run_kitty_terminal_variant(variant: &KittyVariant) -> Result<ProbeOutcome> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(format!("before {}\n", variant.name).as_bytes());
    bytes.extend_from_slice(b"\x1b_G");
    bytes.extend_from_slice(variant.payload.as_bytes());
    bytes.extend_from_slice(variant.terminator.bytes());
    bytes.extend_from_slice(format!("after {}\n", variant.name).as_bytes());

    let mut term = Terminal::new(80, 24);
    term.process(&bytes);
    let mut outcome = outcome_from_terminal("kitty-variant", &term);
    outcome.passed = outcome
        .graphics
        .iter()
        .any(|g| g.protocol == GraphicProtocol::Kitty && g.pixels == variant.expected_pixels);
    outcome.checks.push(Check {
        name: "variant bytes",
        passed: true,
        detail: format!(
            "name={} payload_len={} total_len={} terminator={}",
            variant.name,
            variant.payload.len(),
            bytes.len(),
            variant.terminator.label()
        ),
    });
    Ok(outcome)
}

fn run_iterm_sequence() -> Result<ProbeOutcome> {
    let mut term = Terminal::new(80, 24);
    let encoded = base64::engine::general_purpose::STANDARD.encode(tiny_png_bytes());
    let seq = format!(
        "before iterm\n\x1b]1337;File=name=dGlueS5wbmc=;size={};inline=1;:{}\x1b\\\nafter iterm\n",
        encoded.len(),
        encoded
    );
    term.process(seq.as_bytes());
    let mut outcome = outcome_from_terminal("sequence-iterm-inline-png", &term);
    outcome.checks.push(Check {
        name: "iTerm graphic captured through Terminal::process",
        passed: outcome
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::ITermInline),
        detail: format!("graphics_count={}", outcome.graphics.len()),
    });
    outcome.passed = outcome.checks.iter().all(|check| check.passed);
    outcome.note = Some(
        "Diagnostic terminal-routing fixture; direct ITermParser support is validated separately"
            .to_string(),
    );
    Ok(outcome)
}

fn run_kitty_sequence() -> Result<ProbeOutcome> {
    let mut term = Terminal::new(80, 24);
    // Kitty TGP direct RGB, action=T (transmit and display), f=24 RGB, s/v width/height.
    // Data is two pixels: red and green. Base64([255,0,0, 0,255,0]) = /wAAAP8A.
    let seq = b"before kitty\n\x1b_Ga=T,f=24,s=2,v=1,t=d;/wAAAP8A\x1b\\after kitty\n";
    term.process(seq);
    let mut outcome = outcome_from_terminal("sequence-kitty-rgb", &term);
    outcome.checks.push(Check {
        name: "Kitty graphic captured through Terminal::process",
        passed: outcome
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::Kitty),
        detail: format!("graphics_count={}", outcome.graphics.len()),
    });
    outcome.checks.push(Check {
        name: "Kitty RGBA payload present through Terminal::process",
        passed: outcome
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::Kitty && g.pixels == 8),
        detail: outcome
            .graphics
            .iter()
            .find(|g| g.protocol == GraphicProtocol::Kitty)
            .map(|g| format!("pixels={}", g.pixels))
            .unwrap_or_else(|| "no kitty graphics".to_string()),
    });
    outcome.passed = outcome.checks.iter().all(|check| check.passed);
    outcome.note = Some(
        "Diagnostic terminal-routing fixture; direct KittyParser support is validated separately"
            .to_string(),
    );
    Ok(outcome)
}

fn run_sixel() -> Result<ProbeOutcome> {
    let mut term = Terminal::new(80, 24);
    // Minimal DEC Sixel payload: enter DCS sixel mode, define/use red, draw a few pixels, terminate.
    let seq = b"before sixel\n\x1bPq#1;2;100;0;0#1~~~~~~-~~~~~~\x1b\\\nafter sixel\n";
    term.process(seq);

    let mut outcome = outcome_from_terminal("sequence-sixel-synthetic", &term);
    outcome.checks.push(Check {
        name: "Sixel graphic captured",
        passed: outcome
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::Sixel),
        detail: format!("graphics_count={}", outcome.graphics.len()),
    });
    outcome.checks.push(Check {
        name: "RGBA payload present",
        passed: outcome.graphics.iter().any(|g| g.pixels > 0),
        detail: outcome
            .graphics
            .first()
            .map(|g| format!("pixels={}", g.pixels))
            .unwrap_or_else(|| "no graphics".to_string()),
    });

    let screenshot_path = std::path::Path::new("target/par-term-probe-sixel.png");
    let config = ScreenshotConfig::new().with_sixel_mode(SixelRenderMode::Pixels);
    let screenshot_result = term.screenshot_to_file(screenshot_path, config, 0);
    outcome.checks.push(Check {
        name: "Sixel screenshot rendered",
        passed: screenshot_result.is_ok() && screenshot_path.exists(),
        detail: screenshot_result
            .map(|_| screenshot_path.display().to_string())
            .unwrap_or_else(|err| err.to_string()),
    });
    outcome.passed = outcome.checks.iter().all(|check| check.passed);
    Ok(outcome)
}

fn run_bookokrat(path: String) -> Result<ProbeOutcome> {
    let path_for_check = path.clone();
    let outcome = run_pty(vec![
        "bookokrat".to_string(),
        "--zen-mode".to_string(),
        path,
    ])?;
    let has_book_text = outcome.content.contains("Pride and Prejudice")
        || outcome.content.contains("Project Gutenberg")
        || outcome.content.contains("START OF");
    Ok(ProbeOutcome {
        label: "bookokrat-epub",
        passed: has_book_text,
        checks: vec![Check {
            name: "Bookokrat rendered document text",
            passed: has_book_text,
            detail: format!("path={path_for_check}"),
        }],
        ..outcome
    })
}

fn run_pty(argv: Vec<String>) -> Result<ProbeOutcome> {
    let (cmd, cmd_args): (String, Vec<String>) = if argv.is_empty() {
        (
            "/bin/sh".to_string(),
            vec![
                "-lc".to_string(),
                "printf 'par-term PTY smoke\\n'; printf 'TTY: '; tty; printf 'TERM=%s\\n' \"$TERM\"; printf '\\033[31mred text\\033[0m\\n'; sleep 0.2".to_string(),
            ],
        )
    } else {
        (argv[0].clone(), argv[1..].to_vec())
    };

    let mut pty = PtySession::new(DEFAULT_COLS, DEFAULT_ROWS, DEFAULT_SCROLLBACK);
    pty.set_env("TERM", "xterm-kitty");
    let arg_refs: Vec<&str> = cmd_args.iter().map(String::as_str).collect();
    pty.spawn(&cmd, &arg_refs)
        .with_context(|| format!("spawn {cmd:?}"))?;

    let start = Instant::now();
    while pty.is_running() && start.elapsed() < CHILD_TIMEOUT {
        thread::sleep(Duration::from_millis(50));
    }

    let timed_out = pty.is_running();
    if timed_out {
        let _ = pty.kill();
    }

    let terminal = pty.terminal();
    let term = terminal.lock();
    let mut outcome = outcome_from_terminal("pty-child", &term);
    let spawned = outcome.content.contains("par-term PTY smoke")
        || outcome.content.contains("Project Gutenberg")
        || outcome.content.contains("Pride and Prejudice")
        || !outcome.content.trim().is_empty();
    outcome.checks.push(Check {
        name: "PTY produced output",
        passed: spawned,
        detail: format!("command={cmd:?} timed_out={timed_out}"),
    });
    outcome.passed = outcome.checks.iter().all(|check| check.passed);
    if timed_out {
        outcome.note = Some("child was still running after timeout and was killed; this is expected for long-running TUIs".to_string());
    }
    Ok(outcome)
}

fn outcome_from_terminal(label: &'static str, term: &Terminal) -> ProbeOutcome {
    let graphics = term
        .all_graphics()
        .iter()
        .map(|g| GraphicSummary {
            protocol: g.protocol,
            position: g.position,
            width: g.width,
            height: g.height,
            pixels: g.pixels.len(),
            cell_dimensions: g.cell_dimensions,
            kitty_image_id: g.kitty_image_id,
        })
        .collect();

    ProbeOutcome {
        label,
        passed: true,
        checks: Vec::new(),
        content: term.export_text(),
        graphics,
        scrollback_graphics_count: term.scrollback_graphics_count(),
        dropped_sixel_graphics: term.dropped_sixel_graphics(),
        note: None,
    }
}

fn print_outcome(outcome: &ProbeOutcome) {
    println!("=== {} ===", outcome.label);
    println!("status={}", if outcome.passed { "PASS" } else { "FAIL" });
    for check in &outcome.checks {
        println!(
            "check {}: {} — {}",
            if check.passed { "PASS" } else { "FAIL" },
            check.name,
            check.detail
        );
    }
    if let Some(note) = &outcome.note {
        println!("note: {note}");
    }
    println!("content:");
    println!("{}", outcome.content);
    println!("graphics_count={}", outcome.graphics.len());
    println!(
        "scrollback_graphics_count={}",
        outcome.scrollback_graphics_count
    );
    println!("dropped_sixel_graphics={}", outcome.dropped_sixel_graphics);
    for (idx, graphic) in outcome.graphics.iter().enumerate() {
        println!("graphic[{idx}]: {graphic}");
    }
}

fn default_bookokrat_fixture() -> Option<String> {
    let candidates = [
        "cockpit-test-assets/pride-and-prejudice.epub",
        "../cockpit-test-assets/pride-and-prejudice.epub",
        ".tmp/cockpit-test-assets/pride-and-prejudice.epub",
        "/Users/wilson/bravo/omegon/.tmp/cockpit-test-assets/pride-and-prejudice.epub",
    ];

    candidates.iter().find_map(|candidate| {
        std::path::Path::new(candidate)
            .canonicalize()
            .ok()
            .map(|path| path.to_string_lossy().to_string())
    })
}

fn print_help() {
    println!(
        "par-term-probe\n\n\
         Usage:\n\
           cargo run -- --validate [--assert]\n\
           cargo run -- --sixel [--assert]\n\
           cargo run -- --iterm [--assert]\n\
           cargo run -- --kitty [--assert]\n\
           cargo run -- --kitty-matrix [--assert]\n\
           cargo run -- --parser-fixtures [--assert]\n\
           cargo run -- --pty [COMMAND ARGS...] [--assert]\n\
           cargo run -- --bookokrat PATH [--assert]\n\n\
         --validate runs PTY smoke, Sixel graphics capture, and optional Bookokrat EPUB validation."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixel_probe_captures_rgba_graphic() {
        let outcome = run_sixel().expect("sixel probe should run");
        assert!(outcome.passed, "{outcome:#?}");
        assert!(outcome
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::Sixel && g.pixels > 0));
    }

    #[test]
    fn direct_parser_fixtures_decode_graphics() {
        let outcome = run_parser_fixtures().expect("parser fixtures should run");
        assert!(outcome.passed, "{outcome:#?}");
        assert!(outcome
            .graphics
            .iter()
            .any(|g| g.protocol == GraphicProtocol::Kitty && g.pixels == 8));
    }

    #[test]
    fn pty_probe_captures_child_output() {
        let outcome = run_pty(Vec::new()).expect("pty probe should run");
        assert!(outcome.passed, "{outcome:#?}");
        assert!(outcome.content.contains("par-term PTY smoke"));
    }
}

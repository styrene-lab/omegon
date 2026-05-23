use anyhow::{Context, Result};
use par_term_emu_core_rust::pty_session::PtySession;
use par_term_emu_core_rust::terminal::Terminal;
use std::env;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("--sequence") => { args.remove(0); "sequence" }
        Some("--sixel") => { args.remove(0); "sixel" }
        _ => "pty",
    };

    match mode {
        "sequence" => run_sequence(),
        "sixel" => run_sixel(),
        _ => run_pty(args),
    }
}

fn run_sequence() -> Result<()> {
    let mut term = Terminal::new(80, 24);
    let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";
    let seq = format!("before\n\x1b]1337;File=name=dG90LnBuZw==;size=68;inline=1;:{}\x1b\\\nafter\n", png_b64);
    term.process(seq.as_bytes());
    print_terminal_summary(&term, "sequence-iterm-inline-png");
    Ok(())
}

fn run_sixel() -> Result<()> {
    let mut term = Terminal::new(80, 24);
    // Minimal DEC Sixel-ish payload: enter DCS sixel mode, define/use color, draw a few pixels, terminate.
    // This is deliberately synthetic so we can test parser capture without external tools.
    let seq = b"before sixel\n\x1bPq#1;2;100;0;0#1~~~~~~-~~~~~~\x1b\\\nafter sixel\n";
    term.process(seq);
    print_terminal_summary(&term, "sequence-sixel-synthetic");
    Ok(())
}

fn run_pty(args: Vec<String>) -> Result<()> {
    let (cmd, cmd_args): (String, Vec<String>) = if args.is_empty() {
        (
            "/bin/sh".to_string(),
            vec!["-lc".to_string(), "printf 'par-term PTY smoke\\n'; printf 'TTY: '; tty; printf 'TERM=%s\\n' \"$TERM\"; printf '\\033[31mred text\\033[0m\\n'; sleep 0.2".to_string()],
        )
    } else {
        (args[0].clone(), args[1..].to_vec())
    };

    let mut pty = PtySession::new(100, 30, 1000);
    pty.set_env("TERM", "xterm-kitty");
    let arg_refs: Vec<&str> = cmd_args.iter().map(String::as_str).collect();
    pty.spawn(&cmd, &arg_refs).with_context(|| format!("spawn {cmd:?}"))?;

    let start = Instant::now();
    while pty.is_running() && start.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(50));
    }
    if pty.is_running() {
        let _ = pty.kill();
        eprintln!("killed child after timeout");
    }

    let terminal = pty.terminal();
    let term = terminal.lock();
    print_terminal_summary(&term, "pty-child");
    Ok(())
}

fn print_terminal_summary(term: &Terminal, label: &str) {
    println!("=== {label} ===");
    println!("content:");
    println!("{}", term.export_text());
    println!("graphics_count={}", term.graphics_count());
    println!("scrollback_graphics_count={}", term.scrollback_graphics_count());
    println!("dropped_sixel_graphics={}", term.dropped_sixel_graphics());
    for (idx, g) in term.all_graphics().iter().enumerate() {
        println!(
            "graphic[{idx}]: protocol={:?} pos={:?} size={}x{} original={}x{} pixels={} cell_dims={:?} kitty_image={:?} placement={:?}",
            g.protocol,
            g.position,
            g.width,
            g.height,
            g.original_width,
            g.original_height,
            g.pixels.len(),
            g.cell_dimensions,
            g.kitty_image_id,
            g.placement,
        );
    }
}

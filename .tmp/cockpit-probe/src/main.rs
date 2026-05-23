use std::env;
use std::io::{self, stdout};
use std::time::{Duration, Instant};

use cockpit::{PaneManager, PaneWidget, SpawnConfig};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, args).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("probe error: {e}");
    }
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    args: Vec<String>,
) -> cockpit::Result<()> {
    let mut manager = PaneManager::new();
    let term_size = terminal.size()?;
    let initial_child_area = manager_area_for_child(child_area_for(Rect::new(0, 0, term_size.width, term_size.height), false));
    manager.set_terminal_size(initial_child_area);

    let (cmd, cmd_args) = if args.is_empty() {
        (
            "/bin/sh".to_string(),
            vec![
                "-lc".to_string(),
                "printf 'Cockpit embedded reader pane\\n'; printf 'TTY: '; tty; printf 'TERM=%s\\n' \"$TERM\"; sleep 1; exec sh".to_string(),
            ],
        )
    } else {
        (args[0].clone(), args[1..].to_vec())
    };
    let child = manager.spawn(SpawnConfig::new_command(cmd.clone()).args(cmd_args.clone()))?;
    let child_id = child.id();
    // Cockpit 0.2.2 always assigns the first pane to the first quarter of its
    // internal 4-slot layout and reserves 30% height for sub-panes. We only
    // render one PaneWidget, so compensate: expand pane 0 vertically and give
    // the manager a virtual width 4x the actual reader area so its first
    // quarter matches the visible right pane.
    manager.toggle_pane_expansion(0);

    let mut reader_focused = false;
    let mut reader_wide = false;
    let mut last_tick = Instant::now();
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = layout_chunks(area, reader_wide);

            let left = chunks[0];
            let right = chunks[1];

            let mode = if reader_focused { "READER" } else { "OMEGON" };
            let left_border = if reader_focused { Color::DarkGray } else { Color::Cyan };
            let right_border = if reader_focused { Color::Cyan } else { Color::DarkGray };
            let help = vec![
                Line::from(vec![
                    Span::styled("Omegon shell / conversation mock", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(format!("Mode: {mode}")),
                Line::from(format!("Child command: {} {}", cmd, cmd_args.join(" "))),
                Line::from(format!("Child PID: {:?}", child.pid())),
                Line::from(format!("Child state: {:?}", child.state())),
                Line::from(format!("Reader wide: {}", reader_wide)),
                Line::from(""),
                Line::from("Controls:"),
                Line::from("  Ctrl+N  toggle focus between Omegon mock and reader PTY"),
                Line::from("  Ctrl+Q  quit probe"),
                Line::from("  Ctrl+F  toggle wide reader layout"),
                Line::from(""),
                Line::from("When focus is READER, normal keys route to the child PTY."),
                Line::from("Try: ihello<Esc>:wq in vi, or run a shell command."),
                Line::from(""),
                Line::from("This prototype intentionally bypasses Cockpit's stock 4-pane UI and renders one PaneWidget inside an Omegon-shaped two-column layout."),
            ];
            let left_widget = Paragraph::new(help)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Omegon mock ")
                        .border_style(Style::default().fg(left_border)),
                )
                .wrap(Wrap { trim: false });
            frame.render_widget(left_widget, left);

            if let Some(handle) = manager.get_pane(child_id) {
                let reader = PaneWidget::new(handle)
                    .focused(reader_focused)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Embedded reader PTY ")
                            .border_style(Style::default().fg(right_border)),
                    );
                frame.render_widget(reader, right);
            }
        })?;

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        break;
                    }
                    if key.code == KeyCode::Char('n') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        reader_focused = !reader_focused;
                        if reader_focused {
                            manager.set_focus(child_id);
                        }
                        continue;
                    }
                    if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        reader_wide = !reader_wide;
                        let size = terminal.size()?;
                        manager.set_terminal_size(manager_area_for_child(child_area_for(Rect::new(0, 0, size.width, size.height), reader_wide)));
                        continue;
                    }
                    if reader_focused {
                        manager.route_key(key).await?;
                    }
                }
                Event::Resize(width, height) => {
                    manager.set_terminal_size(manager_area_for_child(child_area_for(Rect::new(0, 0, width, height), reader_wide)));
                }
                _ => {}
            }
        }

        if last_tick.elapsed() > Duration::from_millis(250) {
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn layout_chunks(area: Rect, reader_wide: bool) -> std::rc::Rc<[Rect]> {
    let constraints = if reader_wide {
        [Constraint::Length(36), Constraint::Min(20)]
    } else {
        [Constraint::Percentage(42), Constraint::Percentage(58)]
    };
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area)
}

fn child_area_for(area: Rect, reader_wide: bool) -> Rect {
    layout_chunks(area, reader_wide)[1]
}

fn manager_area_for_child(child: Rect) -> Rect {
    Rect::new(0, 0, child.width.saturating_mul(4), child.height)
}

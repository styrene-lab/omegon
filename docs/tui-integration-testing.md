---
id: tui-integration-testing
title: TUI integration testing — snapshot tests, PTY harness, and interactive verification
status: decided
parent: tui-visual-system
tags: [testing, tui, ratatui, snapshot, pty, insta, ci, quality]
open_questions: []
issue_type: feature
priority: 2
---

# TUI integration testing — snapshot tests, PTY harness, and interactive verification

## Overview

The TUI is 7,926 LoC across 14 files with 124 tests, but most tests are logic-only (conversation state, scroll math, data structures). Only dashboard.rs uses TestBackend for actual render verification, and those tests check for text presence, not visual layout. No snapshot testing. No interactive testing. No PTY-based integration tests.\n\nWith the TUI surface pass adding dashboard harness section, context class selector, toast notifications, and compaction indicators, we need a testing strategy that catches visual regressions — not just logic bugs.

## Research

### Three-tier testing strategy for ratatui TUIs

**Tier 1: Widget snapshot tests (insta + TestBackend)**

The official ratatui-recommended approach. Each widget is rendered to a `TestBackend`, and the buffer content is captured as an `insta` snapshot file. On subsequent runs, any change to the rendered output causes a test failure with a visual diff.

**How it works:**
```rust
#[test]
fn dashboard_harness_section_renders() {
    let backend = TestBackend::new(36, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    
    let mut state = DashboardState::default();
    state.harness = some_test_harness_status();
    
    terminal.draw(|f| state.render_themed(f.area(), f, &Alpharius)).unwrap();
    
    // insta captures the terminal buffer as a snapshot
    insta::assert_snapshot!(terminal_to_string(&terminal));
}
```

**What it catches:** Layout regressions, text truncation, missing sections, wrong colors (as ANSI names), alignment drift. Runs in CI without a terminal.

**Dependencies:** `insta` crate (1.46.3, 30M downloads, well-maintained) + `cargo-insta` CLI for snapshot review. Zero new binary dependencies — insta is dev-only.

**What we already have:** 7 dashboard tests using TestBackend + `buf_text()` string extraction. These check for text containment (`assert!(text.contains("foo"))`). Converting to insta snapshots gives us full-frame regression detection instead of spot checks.

**Effort:** Low. Add `insta` as dev-dependency, convert existing `buf_text` assertions to `assert_snapshot!`, add new snapshot tests for: footer cards, harness section, selector overlays, bootstrap panel.

---

**Tier 2: Interactive scenario tests (TestBackend + synthetic events)**

Simulate user interactions by feeding crossterm events into the TUI event handler and checking the resulting state. Not PTY-based — uses the same TestBackend but drives the App struct through its event loop.

**How it works:**
```rust
#[test]
fn slash_persona_opens_list() {
    let mut app = App::new(test_settings());
    let (tx, _rx) = mpsc::channel(16);
    
    // Simulate typing "/persona\n"
    let result = app.handle_slash_command("/persona", &tx);
    
    match result {
        SlashResult::Display(text) => assert!(text.contains("Available personas")),
        _ => panic!("expected Display result"),
    }
}

#[test]
fn context_selector_changes_settings() {
    let mut app = App::new(test_settings());
    
    // Open context selector
    app.open_context_selector();
    assert!(app.selector.is_some());
    
    // Simulate arrow down + Enter (select Maniple)
    app.selector_move_down();
    let result = app.confirm_selector();
    
    let s = app.settings().lock().unwrap();
    assert_eq!(s.context_class, ContextClass::Maniple);
}
```

**What it catches:** Command routing, selector state machines, event-to-state transitions, setting mutations. The App struct is tested as a state machine without any terminal rendering.

**Dependencies:** None new — just needs the App struct to be testable without a real terminal. May need to extract some methods or add a test constructor.

**Effort:** Medium. Need to make App constructable in tests (currently requires SharedSettings + potentially other setup). Each scenario is ~10-20 lines.

---

**Tier 3: PTY integration tests (ratatui-testlib)**

Full end-to-end: spawn the omegon binary in a pseudo-terminal, send keystrokes, capture rendered output including ANSI escape sequences, assert on visual state.

**How it works:**
```rust
#[test]
fn full_startup_renders_splash_then_editor() {
    let mut harness = TuiTestHarness::new("omegon")
        .args(["--no-splash", "--model", "test"])
        .size(120, 40);
    
    harness.wait_for_text("Ω", Duration::from_secs(5));
    harness.assert_contains("Omegon");
    
    // Check footer is rendered
    harness.assert_row_contains(39, "Squad");
    
    // Type a command
    harness.send_keys("/status\n");
    harness.wait_for_text("Cloud Providers", Duration::from_secs(2));
}
```

**What it catches:** Real terminal rendering (ANSI sequences, color output, cursor positioning), startup sequence, cross-component interaction, the full event loop working end-to-end.

**Dependencies:** `ratatui-testlib` (0.1.0, new crate, 2k downloads) OR `portable-pty` + `vt100` for a custom harness. ratatui-testlib is still early (0.1.0) and has Bevy/Sixel focus we don't need. A lightweight custom PTY harness using `portable-pty` (mature, ~1M downloads) + `vt100` (terminal emulator for parsing output) may be more appropriate.

**Effort:** High. PTY tests are slow (~1-5s each), brittle (timing-dependent), and require the full binary to be built. Best reserved for critical paths: startup, slash commands, basic interaction flow. Not for widget-level regression.

**Recommendation:** Skip ratatui-testlib for now — it's 0.1.0 with Bevy/Sixel focus we don't need. Build a minimal PTY harness (~100 lines) with portable-pty + vt100 if we want Tier 3. But Tier 3 is the last priority.

---

**Rollout order:**
1. **Tier 1 first** — add insta, convert existing dashboard tests to snapshots, add snapshot tests for footer/harness/bootstrap/selector. ~2 hours, immediate value, catches layout regressions in CI.
2. **Tier 2 second** — add scenario tests for slash commands and selector state machines. ~4 hours, catches interaction bugs.
3. **Tier 3 later** — PTY integration tests for startup and critical flows. Defer until TUI is more stable.

### Current test coverage gaps

**What's tested today (124 TUI tests):**
- conversation.rs: 10 tests (streaming, scroll, finalize, expand/collapse, user messages)
- conv_widget.rs: 5 tests (scroll state, height cache, render, force scroll)
- dashboard.rs: ~12 tests (empty/populated renders, tree hierarchy, openspec stages, cleave progress, status counts, harness section)
- bootstrap.rs: 5 tests (render, color/no-color, full status, /status rerender)
- splash.rs: ~2 tests (logo renders, compact logo)
- spinner.rs: ~1 test

**What's NOT tested:**
- **Footer rendering** — no tests. 4 cards with gauge widgets, persona badges, MCP counts, secrets lock, compaction flash. All untested visually.
- **Selector overlays** — no tests. Model selector, thinking selector, new context class selector. State machine untested.
- **Slash command routing** — no tests in mod.rs. 20 commands, none tested for correct dispatch/result type.
- **Event handling** — no tests. AgentEvent → App state transitions (HarnessStatusChanged, tool call rendering, streaming). Critical integration path untested.
- **Toast notifications** — no tests. New feature from TUI surface pass, fires on state transitions.
- **Theme rendering** — no tests. Alpharius color values applied correctly to widgets.
- **Editor** — no tests. Input handling, history navigation, clipboard paste.
- **Effects** — no tests. tachyonfx integration, splash animation.

**Highest-value gaps to close first:**
1. Footer card snapshots (4 cards × ~3 states each = ~12 tests)
2. Slash command dispatch (20 commands × basic routing = ~20 tests)
3. Selector state machine (3 selectors × open/navigate/confirm/cancel = ~12 tests)
4. HarnessStatusChanged event handling (persona switch, MCP change = ~5 tests)

## Decisions

### Decision: Start with T2 (scenario tests for commands + selectors), then T1 (insta snapshots) — behavior before appearance

**Status:** decided
**Rationale:** T2 covers more of the new functionality (slash commands, selectors, event handling, toasts) without adding a new dependency. T1 (insta) is straightforward to add later and mostly guards against visual regression in existing widgets. The new code we just shipped (TUI surface pass, /auth, /persona, /tone, context selector, harness settings) is all behavior — testing it first is higher value.

## Open Questions

*No open questions.*

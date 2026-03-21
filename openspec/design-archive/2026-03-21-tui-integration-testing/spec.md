# TUI integration testing — snapshot tests, PTY harness, and interactive verification — Design Spec (extracted)

> Auto-extracted from docs/tui-integration-testing.md at decide-time.

## Decisions

### Start with T2 (scenario tests for commands + selectors), then T1 (insta snapshots) — behavior before appearance (decided)

T2 covers more of the new functionality (slash commands, selectors, event handling, toasts) without adding a new dependency. T1 (insta) is straightforward to add later and mostly guards against visual regression in existing widgets. The new code we just shipped (TUI surface pass, /auth, /persona, /tone, context selector, harness settings) is all behavior — testing it first is higher value.

## Research Summary

### Three-tier testing strategy for ratatui TUIs

**Tier 1: Widget snapshot tests (insta + TestBackend)**

The official ratatui-recommended approach. Each widget is rendered to a `TestBackend`, and the buffer content is captured as an `insta` snapshot file. On subsequent runs, any change to the rendered output causes a test failure with a visual diff.

**How it works:**
```rust
#[test]
fn dashboard_harness_section_renders() {
    let backend = TestBackend::new(36, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    
    let mut sta…

### Current test coverage gaps

**What's tested today (124 TUI tests):**
- conversation.rs: 10 tests (streaming, scroll, finalize, expand/collapse, user messages)
- conv_widget.rs: 5 tests (scroll state, height cache, render, force scroll)
- dashboard.rs: ~12 tests (empty/populated renders, tree hierarchy, openspec stages, cleave progress, status counts, harness section)
- bootstrap.rs: 5 tests (render, color/no-color, full status, /status rerender)
- splash.rs: ~2 tests (logo renders, compact logo)
- spinner.rs: ~1 test

**Wh…

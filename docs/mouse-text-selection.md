+++
id = "654438b1-a39a-4fa8-8d92-0dd6d1f3ee7d"
kind = "document"
title = "Mouse text selection — EnableMouseCapture blocks native terminal selection"
status = "implemented"
tags = ["tui", "ux", "mouse", "clipboard", "accessibility"]
aliases = ["mouse-text-selection"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "bug"
open_questions = []
priority = "1"
+++

# Mouse text selection — EnableMouseCapture blocks native terminal selection

## Overview

EnableMouseCapture grabs all mouse events for scroll-wheel handling, which prevents the terminal emulator from doing native text selection (click-drag-copy). This is a fundamental tradeoff in crossterm/ratatui apps. Need to find an approach that preserves scroll support while restoring text selection.

## Research

### Approaches used by other TUI apps

1. **Shift+click bypass** — Most terminals (iTerm2, Kitty, Alacritty, WezTerm) let users hold Shift while clicking to bypass app mouse capture and use native selection. This is a terminal feature, not app-controlled. Problem: users don't know about it.

2. **Don't capture mouse** — Remove `EnableMouseCapture` entirely. Lose scroll-wheel support but regain native selection. Many TUI apps (htop, lazygit) don't capture mouse and still work fine. Scroll is handled by Page Up/Down or arrow keys instead.

3. **Toggle mouse capture** — Use a keybinding (e.g., Ctrl+M) to toggle mouse capture on/off. When off, native selection works. When on, scroll works. Zellij uses this approach.

4. **Use only scroll events** — crossterm has `EnableMouseCapture` (all events) vs more granular options. Unfortunately crossterm's mouse capture is all-or-nothing for standard terminals. Some terminals support SGR-Pixels or other extended protocols that could theoretically allow selective capture.

5. **Implement in-app text selection** — Handle mouse click+drag events ourselves, maintain a selection buffer, and copy to clipboard on release. This is what VS Code's integrated terminal does. Most complex but most complete.

Given the junior engineer persona: they don't know about Shift+click. They try to select text, it doesn't work, they think the app is broken. The simplest fix that preserves the most functionality is approach 3 (toggle) with a clear indicator in the footer, or approach 2 (just drop mouse capture — scroll via keyboard is fine).

### rc.16 finding: native selection wraps across full terminal width

With mouse capture removed, native terminal selection works but selects the entire terminal row including the dashboard sidebar. This is inherent to how terminal text selection works — it doesn't know about ratatui's column layout. The text wraps across the conversation + sidebar boundary, making copied text garbled.

This is the same behavior users see in any TUI with a sidebar (htop, btop, lazygit) when selecting text. The standard workaround is: hold Option (macOS) / Alt (Linux) for rectangular/column selection in iTerm2/Kitty, or use the terminal's "select output" feature.

The correct long-term fix is to re-enable mouse capture and implement in-app text selection with OSC 52 clipboard write. This is what VS Code's integrated terminal does. For now, mouse capture needs to come back for scroll-wheel, and we accept the selection tradeoff until we build proper in-app copy.

## Decisions

### Decision: Keep EnableMouseCapture for scroll-wheel; accept modifier-key selection until in-app copy is built

**Status:** decided
**Rationale:** Tested both directions in rc.16. Without mouse capture: scroll-wheel dies, and native selection wraps across the full terminal width including the sidebar, producing garbled text. With mouse capture: scroll-wheel works, and users can still select text using Option+click (macOS iTerm2/Kitty rectangular selection) or Shift+click (most terminals). The scroll-wheel is more important day-to-day than frictionless selection, and cross-panel garbled selection isn't actually useful anyway. The real fix is in-app text selection with OSC 52 clipboard write — that's the approach VS Code's integrated terminal uses. This is not a cost-saving deferral; it's the architecturally correct solution that just hasn't been built yet.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/mod.rs` (modified) — EnableMouseCapture enabled at startup. ScrollUp/ScrollDown handled in Event::Mouse arm. DisableMouseCapture in cleanup/panic paths.

### Constraints

- Scroll-wheel must work — it's the primary navigation for reading conversation history
- Text selection must not require undiscoverable modifier keys for basic use cases
- OSC 52 clipboard write has security implications — some terminals prompt, some block it entirely

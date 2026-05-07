+++
id = "2aaec9d0-162a-4a0a-b7e7-666fd2e8d568"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Mouse text selection — EnableMouseCapture blocks native terminal selection — Design Spec (extracted)

> Auto-extracted from docs/mouse-text-selection.md at decide-time.

## Decisions

### Keep EnableMouseCapture for scroll-wheel; accept modifier-key selection until in-app copy is built (decided)

Tested both directions in rc.16. Without mouse capture: scroll-wheel dies, and native selection wraps across the full terminal width including the sidebar, producing garbled text. With mouse capture: scroll-wheel works, and users can still select text using Option+click (macOS iTerm2/Kitty rectangular selection) or Shift+click (most terminals). The scroll-wheel is more important day-to-day than frictionless selection, and cross-panel garbled selection isn't actually useful anyway. The real fix is in-app text selection with OSC 52 clipboard write — that's the approach VS Code's integrated terminal uses. This is not a cost-saving deferral; it's the architecturally correct solution that just hasn't been built yet.

## Research Summary

### Approaches used by other TUI apps

1. **Shift+click bypass** — Most terminals (iTerm2, Kitty, Alacritty, WezTerm) let users hold Shift while clicking to bypass app mouse capture and use native selection. This is a terminal feature, not app-controlled. Problem: users don't know about it.

2. **Don't capture mouse** — Remove `EnableMouseCapture` entirely. Lose scroll-wheel support but regain native selection. Many TUI apps (htop, lazygit) don't capture mouse and still work fine. Scroll is handled by Page Up/Down or arrow keys inste…

### rc.16 finding: native selection wraps across full terminal width

With mouse capture removed, native terminal selection works but selects the entire terminal row including the dashboard sidebar. This is inherent to how terminal text selection works — it doesn't know about ratatui's column layout. The text wraps across the conversation + sidebar boundary, making copied text garbled.

This is the same behavior users see in any TUI with a sidebar (htop, btop, lazygit) when selecting text. The standard workaround is: hold Option (macOS) / Alt (Linux) for rectangul…

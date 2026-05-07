+++
id = "3fe3d754-c37d-4776-a636-1a07226d9a43"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Non-Capturing Dashboard Overlay

## Overview

Evaluate pi 0.57.0's non-capturing overlay API (OverlayOptions.nonCapturing, OverlayHandle.focus/unfocus/isFocused) for refactoring the dashboard Layer 2 from a modal overlay to a persistent side panel that doesn't steal input.

## Research

### Upstream API Surface (pi 0.57.0)

**OverlayOptions.nonCapturing: boolean** — When true, overlay renders visually but doesn't steal keyboard focus from the editor/main input. This is exactly what we need for a persistent dashboard panel.

**OverlayHandle** methods:
- `focus()` — Takes keyboard focus and brings overlay to front
- `unfocus()` — Releases focus back to previous target
- `isFocused()` — Query focus state
- `setHidden(bool)` / `isHidden()` — Toggle visibility without destroying
- `hide()` — Permanent removal

**ctx.ui.custom() options.onHandle** — Callback receives the OverlayHandle after overlay is shown. This is how we'd get the handle for later focus/unfocus toggling.

**Key insight**: `ctx.ui.custom()` returns `Promise<T>` that resolves when `done()` is called. For a non-capturing overlay that persists indefinitely, we'd need to NOT await the promise — fire-and-forget, storing the handle via `onHandle` for later control. This is a fundamentally different lifecycle than the current modal overlay which `await`s until Esc.

### Current Implementation Analysis

**Current overlay (overlay.ts)**: `showDashboardOverlay()` calls `ctx.ui.custom<void>()` with `overlay: true` and blocks until user presses Esc. Standard capturing overlay — steals all keyboard input.

**Current lifecycle**:
1. User hits Ctrl+Shift+B twice (compact → raised → overlay) or `/dashboard open`
2. `showDashboardOverlay()` awaited — blocks the command handler
3. DashboardOverlay renders as right-anchored panel (40% width, right-center)
4. All keyboard input goes to overlay (tab switching, navigation, expand/collapse)
5. Esc calls `done()`, promise resolves, control returns

**What changes for non-capturing**:
- Overlay created once at session_start (or first toggle), not re-created each time
- `nonCapturing: true` lets editor keep receiving input
- Handle stored in module state for toggle via shortcut
- Keyboard input only reaches overlay when explicitly `focus()`'d
- The overlay can live-render dashboard state while user types prompts
- Esc from focused state → `unfocus()` (not `hide()`), overlay stays visible

**File scope**: Only `overlay.ts` and `index.ts` need changes. The `DashboardOverlay` class (rendering, data) stays the same. The `overlay-data.ts` helper is unchanged.

### Keybind Failure Root Cause

**Root cause found**: `ctrl+shift+b` has NO legacy terminal fallback. The `matchesKey` function for `ctrl+shift+<letter>` only checks `matchesKittySequence()` — there is no raw byte sequence for ctrl+shift+letter in traditional terminals.

**How it works**: pi-tui probes for Kitty keyboard protocol support on startup by sending `\x1b[?u` and checking for a `\x1b[?<flags>u` response. If the terminal responds, it enables the protocol with flags 7 (disambiguate + event types + alternate keys) via `\x1b[>7u`. Only then do `ctrl+shift+<letter>` combos produce distinguishable input sequences.

**Terminal support**:
- ✅ Kitty, WezTerm, Ghostty, foot — full Kitty protocol support
- ❌ macOS Terminal.app — no Kitty protocol
- ⚠️ iTerm2 — partial CSI u support (check version)
- ❌ tmux — needs `extended-keys` option enabled

**Impact**: This is NOT a bug in Omegon. It's a terminal capability issue. `ctrl+shift+b` is the correct registration, but it will **silently fail on terminals without Kitty protocol support**. The `/dashboard` command works everywhere because it's a text command, not a keybind.

**Mitigation options**:
1. Keep `ctrl+shift+b` for capable terminals, document the requirement
2. Add a fallback keybind that works in legacy terminals (e.g., `alt+b` or a function key like `f5`)
3. Both — register two keybinds for the same action

**What terminal is the user running?** This determines if it's a configuration issue or a fundamental limitation.

### Super/Meta Key Support — Not Available

**pi-tui's `parseKeyId` only recognizes three modifiers**: `ctrl`, `shift`, `alt`. There is no `super`, `meta`, or `hyper` in the parser. The `MODIFIERS` bitmask is `{shift: 1, alt: 2, ctrl: 4}` — no super bit (which would be 8 in the Kitty protocol spec).

Even though the Kitty keyboard protocol *does* define super (bit 4, value 8) in its modifier encoding, pi-tui doesn't parse it. Registering `"super+d"` would be parsed as `{key: "d", ctrl: false, shift: false, alt: false}` — the "super" part is silently ignored, reducing it to just pressing `d`.

**Conclusion**: Super/Meta keybinds are not a viable option without upstream pi-tui changes.

### Complete Option Space for Dashboard Keybind

| Option | Pros | Cons |
|--------|------|------|
| `ctrl+shift+<letter>` | Clean, modern, familiar from VS Code/IDEs | Requires Kitty protocol; many combos taken by Kitty terminal defaults; free letters: a,d,i,j,m,p,r,x,y; `d` blocked by pi-tui debug |
| `alt+<letter>` | Works on all terminals (legacy ESC+letter fallback) | `b,d,f,y` taken by editor (word nav, kill ring); could collide with shell conventions; some macOS terminals need Option-as-Meta configured |
| `F<n>` function keys | Works universally, no modifier conflicts | Only F1-F12 handled, no modifiers supported in pi-tui; feels dated; F1-F4 often intercepted by OS/terminal |
| `super+<letter>` | ❌ Not supported by pi-tui | Would require upstream change |
| `ctrl+<letter>` | Legacy-safe | Almost all taken by Emacs bindings (ctrl+a/b/c/d/e/f/k/l/n/p/u/w/y/z) |
| Slash command only | Works everywhere | No quick-toggle UX, must type |

**Best candidates**:
1. **`ctrl+shift+p`** — free in Kitty defaults, mnemonic (panel), but only works with Kitty protocol terminals
2. **`ctrl+shift+d`** — free in Kitty defaults, mnemonic (dashboard), but blocked by pi-tui's hardcoded onDebug handler
3. **`alt+j`** — free everywhere, universal legacy support, but not mnemonic
4. **`F5`** or **`F8`** — universal, no conflicts, but no mnemonics and feels retro
5. **Dual registration**: `ctrl+shift+p` (modern terminals) + `alt+j` or `F5` (legacy fallback)

### Overlay Positioning: Viewport-Pinned

Overlays are **viewport-pinned**, not content-following. The TUI composites overlay lines on top of the rendered output at fixed viewport positions (row/col calculated from anchor + margins). The conversation content scrolls behind the overlay.

This means a non-capturing dashboard panel will be a persistent floating panel pinned to the right side of the terminal, with the conversation visible underneath/beside it. It does NOT flow with the conversation.

**Implication**: The right-anchored 40% width panel will occlude ~40% of the conversation text on the right side. This is fine when the user wants it visible, but they need a way to toggle visibility quickly. `/dashboard` command handles this. The footer hint makes it discoverable.

### Content Reflow — Not Supported by pi-tui

**The TUI cannot reflow conversation content around an overlay.** The render pipeline is:

1. `doRender()` gets `width = this.terminal.columns`
2. `this.render(width)` renders all children (messages, editor, footer) at full terminal width
3. `compositeOverlays()` paints overlay lines ON TOP of the rendered content at fixed positions

There is no mechanism for an overlay to reduce the available width for main content. The width flows from `terminal.columns` → `Container.render(width)` → every child component. An overlay is purely a visual layer painted after content rendering.

**To get content reflow, we would need one of**:
1. **Upstream change to pi-tui**: A concept of "reserved regions" that subtract from available width before content rendering. This is a significant architectural change (panel/split-pane model vs overlay model).
2. **Hack via terminal columns**: Override `terminal.columns` when overlay is visible to trick the renderer into using less width. Fragile — would affect the overlay's own width calculation too.
3. **Extension-level width constraint**: If pi exposes a way for extensions to set a "content width" that the message renderer respects. Currently doesn't exist.

**Conclusion**: Content reflow around a persistent panel requires an upstream pi-tui feature (split-pane or reserved-region support). The current overlay API is overlay-only — it paints on top, never beside.

### Overlay width limitation — conversation reflow not feasible

**Request**: when the side panel is open, restrict conversation output width so text wraps as if the terminal ended at the panel boundary (left edge of overlay).

**Assessment**: Not feasible without pi-tui internals changes.

pi-tui overlays are composited on top of the terminal buffer — the overlay is painted over the right portion of an already-rendered full-width frame. The chat renderer writes at full terminal width, then the overlay is layered on top. There is no "reserved columns" API that would constrain the chat renderer's output width before it writes.

**What would be required upstream**:
- A `setReservedColumns(right: number)` API on the TUI/chat-renderer that the overlay system calls when a non-capturing overlay is shown or hidden.
- The chat output renderer would then use `termWidth - reservedColumns` as its effective wrap width.
- This is a pi-tui concern, not something extensions can implement today.

**Workaround**: None. Panel-mode users see conversation text flowing under the overlay. The overlay is positioned and sized so its left edge falls within the text area, which is the current behavior.

## Decisions

### Decision: ctrl+shift+b intercepted by Kitty — must change keybind

**Status:** decided
**Rationale:** Kitty's default keymap maps ctrl+shift+b to move_window_backward, consuming it before pi receives any input. This is terminal-level interception, not fixable in Omegon. Free ctrl+shift letters in Kitty defaults: a, d, i, j, m, p, r, x, y. Of those, ctrl+shift+d is consumed by pi-tui's hardcoded debug handler. Best candidate: ctrl+shift+d with `alt+d` as a universal legacy fallback that works on all terminals. Also register a second bind so at least one always works regardless of terminal.

### Decision: Use ctrl+shift+p as primary keybind, document Kitty collision issue

**Status:** exploring
**Rationale:** Free ctrl+shift letters in Kitty defaults: a, d, i, j, m, p, r, x, y. Of these, `p` is mnemonic for "panel/project". `ctrl+shift+d` is free in Kitty but blocked by pi-tui's hardcoded debug handler. Since extension shortcuts fire before editor keybindings, we could also register a legacy-compatible fallback. But we need operator input — the keybind is a UX choice, not a technical one. Candidate: `ctrl+shift+p` primary + `/dashboard` as universal fallback command.

### Decision: Drop keybind, use /dashboard command with footer hint

**Status:** decided
**Rationale:** No single keybind works universally, is mnemonic, and is free across terminal emulators. ctrl+shift combos require Kitty protocol and collide with Kitty's default keymap. Super/meta not supported by pi-tui. alt+letter collides with editor Emacs bindings. /dashboard is universal, discoverable, and a footer hint like "/dashboard" makes it obvious. Remove the broken ctrl+shift+b registration entirely.

### Decision: Keep both modal and non-capturing overlay modes

**Status:** decided
**Rationale:** Non-capturing for passive display (glancing while working), focus() to enter interactive mode for navigation/expand. Keeps option C from the cycle discussion — all states available, prune later if unused.

### Decision: /dashboard command cycle: compact → raised → non-capturing → focused

**Status:** decided
**Rationale:** Option C — keep all 4 states. `/dashboard` cycles through them. `/dashboard open` jumps straight to non-capturing panel. `/dashboard focus` enters interactive mode. Footer shows current state and hint to expand. Can prune unused states later based on actual usage.

### Decision: Use ctrl+` as dashboard keybind

**Status:** decided
**Rationale:** ctrl+backtick: free in Kitty terminal defaults, free in pi-tui editor keybindings, supported via SYMBOL_KEYS in matchesKey, familiar pattern (VS Code uses ctrl+` for terminal toggle). Requires Kitty protocol (same as any ctrl+shift combo), but unlike ctrl+shift+b it won't be intercepted by Kitty itself. /dashboard remains the universal fallback for non-Kitty terminals. Supersedes the earlier "drop keybind" decision — we can have both.

## Open Questions

*No open questions.*

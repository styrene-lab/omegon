+++
id = "e967b260-87f1-46e8-bdeb-04802ef00443"
kind = "document"
title = "Clipboard image paste into chat/messages"
status = "implemented"
tags = ["ux", "clipboard", "images", "attachments", "chat"]
aliases = ["clipboard-image-paste"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "bug"
open_questions = []
parent = "conversation-rendering-engine"
priority = "2"
+++

# Clipboard image paste into chat/messages

## Overview

Investigate and fix the failure path where a user cannot paste an image into the chat/message composer for inspection. Design should cover attachment intake, user feedback on paste success/failure, and compatibility with the existing read/view/render image flows.

## Research

### Current failure path diagnosis

Investigated vendor/pi-mono clipboard image paste path. In interactive mode, Ctrl+V invokes handleClipboardImagePaste() in vendor/pi-mono/packages/coding-agent/src/modes/interactive/interactive-mode.ts, which calls readClipboardImage() and silently returns on any failure. On this macOS environment, the native clipboard bridge is unavailable because vendor/pi-mono/packages/coding-agent/src/utils/clipboard-native.ts requires '@cwilson613/clipboard', but the vendored workspace currently has '@mariozechner/clipboard' installed in vendor/pi-mono/node_modules and no '@cwilson613/clipboard' module. Result: clipboard import resolves to null, readClipboardImage() returns null, and paste fails silently with no operator feedback.

### Rust TUI diagnosis (March 2026)

The TS-era fix is obsolete — Omegon is now a Rust TUI. The Rust implementation in `tui/mod.rs` has `clipboard_image_to_temp()` which correctly extracts images from the macOS clipboard via AppleScript. Tested: works standalone.

The bug is in event routing:
1. `EnableBracketedPaste` is active, which causes the terminal to intercept Ctrl+V
2. When clipboard has text, terminal sends an `Event::Paste(text)` — this works for text paste
3. When clipboard has an image (no text), the terminal sends... nothing. No Key event, no Paste event.
4. The `Event::Key(Char('v'), CONTROL)` handler that calls `clipboard_image_to_temp()` never fires

This is a fundamental issue with bracketed paste mode — the terminal owns Ctrl+V and doesn't forward it as a key event.

Possible fixes:
- **Poll clipboard on a timer** — Check clipboard content periodically and show a paste indicator. Too invasive.
- **Use a different keybinding** — e.g., Ctrl+Shift+V or a slash command `/paste`. Discoverable?
- **Disable bracketed paste** — Then Ctrl+V arrives as a Key event, but multi-line paste breaks (each line becomes a separate event).
- **Add a /paste command** — Most reliable. `/paste` checks clipboard for images, attaches if found. Works regardless of terminal paste mode.
- **Use OSC 52 clipboard protocol** — Some terminals support reading clipboard via escape sequences, but this is not universal and has security implications.

### rc.16 resolution: format matching was the root cause

The Rust TUI diagnosis above was wrong about the event routing. Ctrl+V DOES arrive as a `KeyEvent(Char('v'), CONTROL)` in crossterm — bracketed paste doesn't suppress it. The terminal sends raw byte 0x16 for Ctrl+V regardless of paste mode.

The actual root cause: `clipboard_image_to_temp()` matched clipboard formats against UTI strings (`public.png`, `public.jpeg`, `com.compuserve.gif`) that **never appear** in `osascript -e 'clipboard info'` output. The actual output uses class markers: `«class PNGf»`, `JPEG picture`, `TIFF picture`, `GIF picture`, `«class BMP »`. Every `.find()` returned `None`. Every paste silently returned `None`. Since the function was first written.

Fix (rc.16): match on actual markers (`PNGf`, `JPEG picture`, `TIFF picture`, `GIF picture`, `BMP`). Added `try_paste_clipboard_image()` method that shows system message + toast on success. Also handle empty `Event::Paste` for terminals that send those when clipboard has non-text content.

Verified: screenshot in clipboard → Ctrl+V → image renders inline in conversation with `📎 filename` card title. Working end-to-end on macOS with Kitty terminal.

## Decisions

### Decision: Clipboard image paste should tolerate both clipboard package scopes and surface operator-visible failure feedback

**Status:** decided
**Rationale:** The immediate regression is a missing native clipboard module due to package-scope mismatch in the vendored pi workspace. The paste path should try both known package scopes to remain compatible across rename/fork states, and the interactive handler should stop failing silently so operators can tell whether no image was present versus clipboard access/setup failed.

### Decision: Clipboard image paste fixed in Rust TUI — osascript format matching corrected, visible feedback added

**Status:** decided
**Rationale:** The root cause was never event routing or bracketed paste — it was a string matching bug in clipboard_image_to_temp() that checked for UTI strings that macOS osascript never outputs. Fixed by matching the actual class markers. Added system message and toast so the user always sees confirmation. The TS-era diagnosis and vendor/pi-mono file scope are obsolete — Omegon is Rust-native now.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `vendor/pi-mono/packages/coding-agent/src/utils/clipboard-native.ts` (modified) — Load native clipboard bridge from either known package scope to survive vendored rename/fork mismatch.
- `vendor/pi-mono/packages/coding-agent/src/modes/interactive/interactive-mode.ts` (modified) — Stop failing silently on image paste; emit status/warning feedback and preserve temp-file insertion behavior.
- `core/crates/omegon/src/tui/mod.rs` (modified) — clipboard_image_to_temp(): fixed format markers. try_paste_clipboard_image(): system message + toast feedback. Empty Paste event triggers image check.
- `core/crates/omegon/src/tui/segments.rs` (modified) — Image placeholder card: show 📎 filename in border title, removed full temp path and inner text.

### Constraints

- Preserve existing Ctrl+V paste-image workflow and temp-file handoff to view/read.
- Do not silently broaden behavior beyond image paste; only improve module loading compatibility and operator feedback.
- Keep non-image clipboard cases non-fatal, but visible enough to debug.
- Ctrl+V is the keybinding, matching pi behavior
- No slash command for paste — it must work like a native app
- Silent failure on no-image clipboard (user might be pressing Ctrl+V for normal text paste)

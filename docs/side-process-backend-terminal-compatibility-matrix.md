+++
id = "side-process-backend-terminal-compatibility-matrix"
kind = "document"
title = "Side-Process Backend Terminal Compatibility Matrix"
status = "seed"
tags = ["terminal", "compatibility", "substrate", "zellij", "cockpit", "kitty", "extension"]
aliases = ["side-process-backend-terminal-compatibility-matrix", "terminal-requirements-matrix"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "extension-side-process-substrate-api"
dependencies = [
  "extension-side-process-substrate-api",
  "extension-side-process-backend-registry",
  "extension-side-process-capability-negotiation",
  "side-process-backend-macos-linux-compatibility"
]
open_questions = [
  "Which terminal graphics protocols are actually required by Bookokrat PDF/image modes?",
  "Does Zellij preserve Kitty/Sixel/iTerm2 graphics payloads well enough for the reader workflow?",
  "Does Cockpit/par-term preserve, decode, approximate, or drop terminal image protocols?",
  "Can Kitty remote control be assumed enabled, or must Omegon provide setup instructions?",
  "What fallback behavior is acceptable over SSH or in terminals without enhanced graphics/input protocols?",
  "Which requirements are hard gates versus degraded-mode warnings for EPUB/text mode?"
]
related = [
  "extension-side-process-substrate-api",
  "extension-side-process-backend-registry",
  "reader-extension-side-pane-contract",
  "managed-reader-workspace",
  "cockpit-par-term-substrate-analysis",
  "par-term-emu-core-rust-reader-pane-analysis",
  "side-process-backend-macos-linux-compatibility"
]
+++

# Side-Process Backend Terminal Compatibility Matrix

## Overview

Capture the terminal requirements and prerequisites for each side-process substrate backend.

This matrix is intentionally split between:

1. **hard prerequisites** — backend cannot work without them;
2. **capability prerequisites** — backend works, but specific features degrade;
3. **research gates** — unknowns that must be tested before deciding.

The goal is to let extension requests negotiate capabilities without exposing backend details to the extension.

## Capability legend

| Capability | Meaning |
|---|---|
| `host_process` | Can launch an argv-based child process. |
| `adjacent_pane` | Can display the child beside Omegon. |
| `embedded_pane` | Pane lives inside Omegon's TUI process. |
| `external_workspace` | Pane lives in an external mux/workspace. |
| `replace_named_pane` | Can replace an existing logical pane. |
| `close_pane` | Can close a pane programmatically. |
| `focus_pane` | Can move operator focus to/from pane. |
| `resize_propagation` | Child process receives accurate PTY resize. |
| `mouse_passthrough` | Child TUI mouse mode works reliably. |
| `graphics_passthrough` | Actual terminal graphics protocols survive the substrate. |
| `text_tui` | Full-screen text TUI behavior works: alt screen, colors, keyboard. |
| `degraded_preview` | Can display approximated imagery, e.g. half-block fallback, but not real raster imagery. |

## Backend summary

| Backend | Best-fit role | Terminal prerequisite | External binary prerequisite | Current posture |
|---|---|---|---|---|
| Zellij | v1 external managed workspace | Any terminal that can run Zellij; Kitty/Ghostty preferred for graphics tests | `zellij`; child binary such as `bookokrat` | Primary v1 candidate, pending pane-control and graphics validation |
| Cockpit/par-term embedded | Native embedded process pane | Omegon's current Ratatui/crossterm terminal; Kitty/Ghostty preferred for graphics tests | child binary such as `bookokrat`; no external mux | Promising experimental backend; text TUI smoke passed; graphics/mouse unresolved |
| Kitty remote control | Terminal-native side window/pane | Kitty specifically, with remote control enabled/configured | child binary such as `bookokrat`; no mux | Optional spike; powerful but terminal-specific |
| Ghostty-native control | Terminal-native side process if supported | Ghostty specifically; control/split API TBD | child binary such as `bookokrat` | Research-only until a stable control API is identified |
| Pure Ratatui artifact pane | In-process artifact preview, not a child process pane | Any terminal supported by Omegon; Kitty/iTerm2/Sixel improves images via ratatui-image | no child process required | Complementary fallback for static artifacts, not interactive Bookokrat |
| Fallback/setup backend | Structured unavailable result | None | None | Always available; gives setup instructions |

## Detailed compatibility matrix

| Requirement / Feature | Zellij backend | Cockpit/par-term backend | Kitty backend | Pure Ratatui artifact pane | Fallback backend |
|---|---|---|---|---|---|
| Launch argv child process | Expected yes | Proven in smoke prototype | Expected yes via Kitty launch/remote control | No | No |
| Avoid shell interpolation | Required by design; verify command forms | Required by design; prototype used argv command shape | Required; verify remote-control invocation can pass argv safely | N/A | N/A |
| Adjacent side pane | Expected yes | Yes inside Omegon-owned layout | Expected yes in Kitty windows/splits | Only if Omegon allocates layout region | No |
| Embedded in Omegon TUI | No | Yes | No | Yes | No |
| External workspace isolation | Yes | No | Yes, terminal-managed | No | N/A |
| Full-screen text TUI | Expected yes | Proven with `vi`; Bookokrat EPUB text smoke passed | Expected yes | No child TUI | N/A |
| Alternate screen | Expected yes | Proven enough for smoke; needs production validation | Expected yes | N/A | N/A |
| Resize propagation | Expected yes | Prototype reflow worked; production validation needed | Expected yes | Omegon-owned layout only | N/A |
| Mouse passthrough | Unknown; test Bookokrat | Unknown; explicit gate | Expected if Kitty routes correctly; verify | Omegon handles its own mouse only | N/A |
| Truecolor | Expected yes | Expected via terminal emulation; verify | Expected yes | Existing Omegon behavior | N/A |
| Kitty graphics protocol | Unknown through Zellij; hard research gate | Unknown through par-term; hard research gate | Native yes, if child writes to Kitty-compatible terminal | Existing image path may support Kitty via ratatui-image, not child protocol | N/A |
| Sixel | Unknown through Zellij | Unknown through par-term | Kitty support depends on Kitty features/config; verify | ratatui-image may negotiate if terminal supports it | N/A |
| iTerm2 inline images | Unknown through Zellij | Unknown through par-term | No, Kitty backend targets Kitty protocol | ratatui-image may negotiate in iTerm2 outside Kitty backend | N/A |
| Real PDF/image rendering in Bookokrat | Unknown; likely best near-term chance if graphics passes | Unknown; likely depends on par-term behavior | Potentially strong for Kitty-only path | Not via Bookokrat; only static artifact preview | No |
| Half-block/degraded image fallback | Depends on Bookokrat/substrate behavior | Possible if par-term degrades imagery; not equivalent to real imagery | Possible but unnecessary if native graphics works | Yes as degraded artifact rendering | No |
| Replace named pane | Unknown; verify Zellij pane identity/control | Core can own logical handle if embedded | Unknown; depends on Kitty control API | Core-owned | No |
| Close pane programmatically | Expected possible; verify | Expected core-owned; verify cleanup | Expected possible; verify | Core-owned | No |
| Pane persists after Omegon exit | Possible/likely depending Zellij session | No by default; child dies with Omegon unless detached intentionally | Possible terminal-owned | No | N/A |
| Works in Ghostty | Expected if Zellij works in Ghostty; graphics TBD | Expected for text; graphics TBD | No | Yes for normal TUI; graphics per terminal support | Yes |
| Works in Kitty | Expected; graphics TBD | Expected for text; graphics TBD | Yes | Yes | Yes |
| Works over SSH | Possible if Zellij available remotely; graphics depends on terminal/protocol path | Possible but embedded child runs remote-side; graphics/input TBD | Only if Kitty remote control usable in that context; unlikely as default | Yes for text; images depend on terminal path | Yes |
| Requires user setup | Install/configure Zellij unless managed by Omegon | Enable experimental backend/build dependency | Enable Kitty remote control/config | None beyond current Omegon | None |

## Backend prerequisite notes

### Zellij backend

Hard prerequisites:

- `zellij` binary available or managed-installed.
- Omegon running inside a Zellij session, or able to bootstrap/re-exec into one.
- Child command available, e.g. `bookokrat`.
- Backend commands can pass argv without shell interpolation.

Capability prerequisites:

- Stable session/pane targeting for replace/close/focus.
- Graphics passthrough must be tested for Bookokrat image/PDF modes.
- Operator must tolerate mux workspace UX.

Research gates:

- Exact commands for open pane, name pane, target pane, close pane, replace pane.
- Behavior inside Kitty and Ghostty.
- Kitty/Sixel/iTerm2 image behavior through Zellij.

### Cockpit/par-term embedded backend

Hard prerequisites:

- Cockpit/par-term dependencies compile into Omegon or an experimental feature branch.
- Omegon TUI event loop can route events to an embedded pane without destabilizing editor/conversation input.
- Child command available, e.g. `bookokrat`.

Capability prerequisites:

- Core owns layout and lifecycle.
- Resize propagation to child PTY must be reliable.
- Focus indicators and escape hatches must be designed.

Research gates:

- par-term graphics behavior: preserve/decode/drop/approximate Kitty/Sixel/iTerm2 payloads.
- Mouse routing with Bookokrat.
- Crash cleanup and child process termination.
- API/license/maintenance stability.

### Kitty remote-control backend

Hard prerequisites:

- Outer terminal is Kitty.
- Kitty remote control is enabled and reachable.
- Security model is acceptable to the operator.
- Child command available.

Capability prerequisites:

- Remote-control commands must support safe argv-style launch.
- Omegon can identify and manage launched windows/panes.
- Setup instructions must be precise when remote control is disabled.

Research gates:

- Can launch a side pane/window adjacent to Omegon reliably.
- Can close/replace/focus a launched pane.
- Behavior with Kitty graphics is expected strong but must be tested with Bookokrat.
- Whether supporting this creates unacceptable terminal-specific branching.

### Pure Ratatui artifact pane

Hard prerequisites:

- None beyond current Omegon TUI.

Capability prerequisites:

- This is not a process-hosting backend.
- It can only satisfy requests that can be represented as artifacts or previews.

Research gates:

- Whether reader extension can offer useful static previews when process-pane backends are unavailable.
- Whether artifact preview should use existing `view`/display artifact paths instead of side-process substrate.

## Reader-mode requirement mapping

| Reader mode | Required capabilities | Preferred capabilities | Acceptable fallback |
|---|---|---|---|
| EPUB/text | `host_process`, `adjacent_pane`, `text_tui`, `resize_propagation` | `replace_named_pane`, `mouse_passthrough`, `graphics_passthrough` | Open degraded without graphics if text renders well |
| Image | `host_process`, `adjacent_pane`, `graphics_passthrough`, `resize_propagation` | `mouse_passthrough`, `replace_named_pane` | Static artifact preview if process pane unavailable |
| PDF | `host_process`, `adjacent_pane`, `graphics_passthrough`, `resize_propagation` | `mouse_passthrough`, `replace_named_pane` | Static page/image preview only if explicitly accepted |
| Auto | Depends on detected file type | Backend with strongest matching capability set | Explain missing hard requirements |

## Recommended initial policy

For v1 reader work:

1. Treat EPUB/text as the first acceptance target.
2. Treat PDF/image as blocked on `graphics_passthrough` evidence.
3. Prefer Zellij for external side-process reliability.
4. Keep Cockpit/par-term as the native embedded experiment.
5. Do not expose Kitty-specific backend behavior to extensions; keep it behind the backend registry if tested.
6. Always return structured setup/degraded messages instead of silently falling back to a broken pane.

## Validation checklist

For each backend, record:

- terminal emulator and version;
- substrate version;
- command used;
- whether argv was shell-free;
- child process launched;
- resize behavior;
- keyboard behavior;
- mouse behavior;
- graphics behavior for EPUB/image/PDF;
- close/replace/focus behavior;
- failure cleanup behavior.

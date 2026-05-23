+++
id = "side-process-backend-macos-linux-compatibility"
kind = "document"
title = "Side-Process Backend macOS/Linux Compatibility Assessment"
status = "exploring"
tags = ["terminal", "compatibility", "macos", "linux", "substrate", "zellij", "cockpit", "kitty", "ghostty"]
aliases = ["side-process-backend-macos-linux-compatibility", "macos-linux-side-process-compatibility"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "side-process-backend-terminal-compatibility-matrix"
dependencies = [
  "side-process-backend-terminal-compatibility-matrix",
  "extension-side-process-backend-registry",
  "reader-extension-side-pane-contract"
]
open_questions = [
  "Does Zellij's Sixel-only/buggy image path make it unsuitable for Bookokrat PDF/image mode despite being cross-platform?",
  "Can par-term-emu-core-rust's claimed Kitty/Sixel/iTerm2 graphics support be embedded through Cockpit in a way Omegon can render?",
  "Does Cockpit's Unix PTY path behave identically enough on macOS and Linux for production use?",
  "Is Kitty remote control acceptable as an optional macOS/Linux backend despite requiring explicit Kitty configuration?",
  "What Linux display/session environments affect Kitty/Ghostty/par-term image or window behavior?"
]
related = [
  "side-process-backend-terminal-compatibility-matrix",
  "cockpit-par-term-substrate-analysis",
  "par-term-emu-core-rust-reader-pane-analysis",
  "managed-reader-workspace",
  "reader-extension-side-pane-contract"
]
+++

# Side-Process Backend macOS/Linux Compatibility Assessment

## Overview

Assess cross-platform viability for side-process backends on macOS and Linux. Windows is out of scope; WSL is the only plausible Windows path and should be treated like Linux from the terminal application's perspective.

This document updates the terminal compatibility matrix with platform posture and current external evidence.

## Evidence snapshot

- Zellij is a Rust terminal multiplexer and is installable on macOS/Linux, but available public evidence indicates Zellij currently supports Sixel for terminal images and does not yet provide robust Kitty graphics passthrough. One reported Zellij/Yazi discussion says Zellij's Sixel implementation is buggy/performance-sensitive and that users should consider running image-heavy tools outside Zellij if image preview is a strong need.
- Kitty remote control supports launching new windows/tabs and sending input to matched windows. It is explicitly powerful and therefore needs configuration/security review.
- Ghostty documents support for modern terminal developer features including Kitty graphics protocol and Kitty keyboard protocol, plus native tabs/splits. A stable external remote-control/split API for Omegon should not be assumed from that feature list.
- Cockpit is documented as a terminal multiplexer library for Ratatui applications. It provides PTY-backed panes, crash isolation, pane widgets, snapshots, and process state.
- `par-term-emu-core-rust` is published as a Rust terminal emulator library with PTY and Sixel/iTerm2/Kitty graphics support, and `par-term` is described as running on Linux, macOS, and Windows. This improves the plausibility of a macOS/Linux embedded graphics path, but it does not prove Cockpit exposes that path in a way Omegon can use.

## Platform posture table

| Backend | macOS posture | Linux posture | Cross-platform confidence | Main blocker |
|---|---|---|---|---|
| Zellij external workspace | Strong for text/process panes | Strong for text/process panes | High for text TUI; medium-low for graphics-heavy reader | Image/graphics passthrough, especially Kitty protocol, is uncertain/weak |
| Cockpit embedded pane | Plausible; Unix PTY model and Ratatui integration align | Plausible; Unix PTY model and Ratatui integration align | Medium for text TUI; unknown for graphics | par-term/Cockpit graphics integration and mouse routing |
| par-term direct embedded backend | Plausible; crate claims macOS support via par-term ecosystem | Plausible; crate claims Linux support | Medium as research target; unproven in Omegon | Need direct API map and prototype |
| Kitty remote-control backend | Strong if operator uses Kitty and enables remote control | Strong if operator uses Kitty and enables remote control | High within Kitty-only scope; low as general backend | Terminal-specific and requires config/security acceptance |
| Ghostty-native backend | Ghostty available and feature-rich; control API not established here | Ghostty available and feature-rich; control API not established here | Low until stable control API verified | No confirmed Omegon-usable remote/split control interface |
| Pure Ratatui artifact pane | Strong; current Omegon model | Strong; current Omegon model | High for static/text artifacts | Not a process-hosting backend |
| Fallback/setup backend | Strong | Strong | High | Does not satisfy side-pane request |

## Backend-by-backend assessment

### Zellij external workspace

#### macOS

Zellij should be viable for macOS text/process side panes:

- Runs as a terminal multiplexer inside Kitty/Ghostty/other terminals.
- Does not require GUI integration.
- Can host Bookokrat as an external process if Bookokrat is installed.

macOS risks:

- If Bookokrat relies on Kitty graphics protocol for PDF/image rendering, Zellij may not preserve that path.
- Sixel support depends on both Zellij and the outer terminal. Kitty historically centers on Kitty graphics protocol rather than Sixel; Ghostty supports Kitty graphics, but Sixel posture must be tested.
- macOS users may be sensitive to being dropped into a mux workflow unless Omegon bootstraps it cleanly.

#### Linux

Zellij should also be viable for Linux text/process side panes:

- Linux is a natural environment for mux workflows.
- SSH/server use can run Zellij remotely.
- Bookokrat side process can run in the same remote environment as Omegon.

Linux risks:

- Same image/graphics issue as macOS.
- Linux terminal diversity increases degradation cases: Alacritty/foot/WezTerm/Konsole/etc. may differ on Sixel/Kitty graphics.
- Over SSH, the outer terminal on the client still controls graphics support, while the mux and process run remotely.

#### Assessment

Use Zellij as the strongest cross-platform text/process-pane backend. Do not assume it satisfies PDF/image reader mode until graphics tests pass.

Recommended capability classification:

```text
macOS/Linux:
  host_process: yes
  adjacent_pane: yes
  external_workspace: yes
  text_tui: yes
  resize_propagation: likely yes
  graphics_passthrough: unknown/weak
  mouse_passthrough: unknown, test
```

### Cockpit embedded backend

#### macOS

Cockpit is plausible on macOS because Omegon already uses Ratatui/crossterm and Cockpit is a Ratatui multiplexer library. The scratch prototype evidence already exists locally for macOS unless otherwise noted by the operator environment:

- Cockpit pane creation worked.
- PTY child process worked.
- `vi` worked.
- Bookokrat EPUB text worked in a two-column layout.

macOS risks:

- Unix PTY behavior is close enough for text TUIs, but production cleanup/signals/resizes still need validation.
- Terminal graphics in an embedded path depends on Cockpit/par-term rendering behavior, not just the outer terminal.
- If par-term decodes images but Ratatui/crossterm cannot render them as actual graphics in the same cell model, the result may still be degraded.

#### Linux

Cockpit should be plausible on Linux for the same architectural reasons:

- PTYs are native.
- Ratatui/crossterm are cross-platform.
- Most full-screen text TUI behavior should map well.

Linux risks:

- Must test separately; macOS PTY success is evidence, not proof.
- Mouse mode and terminal resize behavior may differ across terminal emulators and SSH.
- If Linux users run Omegon under tmux/Zellij already, nested PTY/mux interactions may complicate input and graphics.

#### Assessment

Cockpit is credible for a macOS/Linux embedded text-TUI backend. It is not yet credible for graphics-heavy Bookokrat modes until par-term graphics behavior is proven end-to-end.

Recommended capability classification:

```text
macOS/Linux:
  host_process: yes, prototype evidence
  adjacent_pane: yes, inside Omegon layout
  embedded_pane: yes
  text_tui: yes, smoke evidence
  resize_propagation: promising
  graphics_passthrough: unknown
  mouse_passthrough: unknown
```

### par-term direct embedded backend

This is separate from Cockpit. Cockpit may be the pane manager; par-term may be the terminal emulator/rendering core.

#### macOS

Published crate metadata and par-term docs claim macOS support. The key relevance is that `par-term-emu-core-rust` claims PTY plus Sixel/iTerm2/Kitty graphics support.

macOS upside:

- iTerm2 inline images are macOS-relevant.
- Kitty and Ghostty on macOS support modern graphics paths.
- If par-term exposes graphics payloads cleanly, it may support real embedded reader imagery.

macOS risks:

- Claimed support does not prove embeddability into Omegon's Ratatui frame.
- GPU/frontend assumptions in `par-term` may not exist in `par-term-emu-core-rust` alone.
- We need to know whether graphics are rendered by par-term's own frontend, exposed as data, or merely parsed.

#### Linux

Published crate metadata and par-term docs claim Linux support.

Linux upside:

- Kitty graphics and Sixel are relevant across Linux terminals.
- Linux has a broader ecosystem for terminal image workflows.

Linux risks:

- Display server differences are mostly hidden at terminal-emulator level, but any external image overlay approach would be sensitive to X11/Wayland. We should avoid depending on overlays for core behavior.
- Need to verify whether par-term's graphics support is frontend-specific.

#### Assessment

par-term is the most important cross-platform research target for embedded graphics. It is not yet a product backend until API and rendering integration are understood.

Recommended capability classification:

```text
macOS/Linux:
  host_process: likely at library level, verify
  text_tui: likely, verify
  graphics_passthrough or graphics_decode: claimed, must verify
  embedded_pane: unknown until Omegon integration prototype
```

### Kitty remote-control backend

#### macOS

Kitty is available on macOS and remote control can launch windows/tabs and send text. This can likely implement side-process panes/windows if configured.

macOS risks:

- Kitty macOS window/tab behavior can interact with native OS tabs/windows in ways that differ from Linux.
- Remote control must be explicitly enabled/listened on.
- Security posture matters because remote control can send arbitrary text and control windows.

#### Linux

Kitty remote control is also strong on Linux.

Linux risks:

- Desktop environment/window manager behavior may affect OS windows, but Kitty tabs/splits should be internal to Kitty.
- Socket paths/listen configuration must be robust.

#### Assessment

Kitty backend is cross-platform across macOS/Linux only for Kitty users. It is likely the strongest graphics path for Kitty specifically, but it is not a general backend. It should remain optional and backend-registry-hidden.

Recommended capability classification:

```text
macOS/Linux with Kitty remote control enabled:
  host_process: likely yes
  adjacent_pane: likely yes
  external_workspace: yes, terminal-managed
  graphics_passthrough: likely strong for Kitty protocol
  text_tui: yes
  replace/close/focus: likely possible, verify
```

### Ghostty-native backend

#### macOS

Ghostty supports modern terminal features including Kitty graphics protocol, Kitty keyboard protocol, and native tabs/splits. That makes it an excellent outer terminal for Zellij or embedded Cockpit/par-term testing.

However, terminal feature support is not the same as an automation/control API. Do not assume Omegon can ask Ghostty to open/manage panes until a documented stable control interface is verified.

#### Linux

Same posture as macOS: strong outer terminal, unproven backend substrate.

#### Assessment

Treat Ghostty as a target outer terminal, not a side-process backend, until control-plane evidence exists.

Recommended capability classification:

```text
macOS/Linux:
  outer_terminal_graphics: strong
  outer_terminal_keyboard: strong
  backend_control_api: unknown
```

### Pure Ratatui artifact pane

#### macOS/Linux

This is the most portable option because it is just Omegon's existing TUI model. It should work anywhere Omegon works.

But it is not a side-process backend. It can only preview artifacts that Omegon renders itself.

Graphics support depends on Omegon's existing terminal image rendering stack and outer terminal capabilities. It may be useful as a degraded fallback for PDFs/images converted to static pages, but not for interactive Bookokrat.

#### Assessment

High cross-platform confidence for static previews. Not a substitute for process panes.

## macOS/Linux compatibility conclusions

### Text/process side panes

Best cross-platform options:

1. Zellij external workspace.
2. Cockpit embedded backend.
3. Kitty backend for Kitty-only users.

Zellij is more operationally mature as a process workspace. Cockpit is more product-integrated but needs deeper event-loop/input hardening.

### Graphics-heavy reader panes

No backend is proven yet.

Likely order of promise:

1. Kitty remote-control backend, but only for Kitty users.
2. par-term direct or Cockpit/par-term embedded path, if graphics support is exposed usefully.
3. Zellij only if Sixel/graphics passthrough proves acceptable.
4. Pure Ratatui artifact preview as fallback, not interactive reader.

### Ghostty

Ghostty should be treated as a first-class outer terminal for testing because it supports modern Kitty graphics and keyboard protocols. It should not yet be treated as a backend until a stable side-pane control API is found.

### SSH/Linux remote use

For SSH, Zellij is the cleanest process-pane story because the mux and child process run remotely. Graphics still depend on the protocol reaching the local terminal. Embedded Cockpit also runs remotely inside Omegon and may work for text; graphics remain unknown.

Kitty remote control is less attractive over SSH because the control plane is local-terminal-specific while the process may be remote.

## Recommended backend policy by platform

### macOS

Default order for `text` reader mode:

1. Zellij, if managed workspace active/available.
2. Cockpit embedded, if experimental backend enabled.
3. Kitty backend, if Kitty remote control explicitly configured.
4. Pure Ratatui artifact fallback or setup instructions.

Default order for `pdf`/`image` reader mode:

1. Kitty backend if explicitly configured and tested.
2. par-term/Cockpit embedded only after graphics proof.
3. Zellij only after graphics proof.
4. Static artifact preview or unavailable.

### Linux

Default order for `text` reader mode:

1. Zellij, especially over SSH/remote workflows.
2. Cockpit embedded, if experimental backend enabled.
3. Kitty backend, if Kitty remote control explicitly configured.
4. Pure Ratatui artifact fallback or setup instructions.

Default order for `pdf`/`image` reader mode:

1. Kitty backend where Kitty is the outer terminal and remote control is configured.
2. par-term/Cockpit embedded after graphics proof.
3. Zellij after Sixel/graphics proof.
4. Static artifact preview or unavailable.

## Research tests required

### Zellij tests

Run on macOS and Linux:

```text
outer terminal: Kitty
outer terminal: Ghostty
substrate: Zellij
child: bookokrat --zen-mode sample.epub
child: bookokrat sample.pdf
child: image-capable terminal tool if Bookokrat image fixture is unavailable
```

Record:

- pane open command;
- pane replacement/close behavior;
- keyboard navigation;
- mouse behavior;
- resize behavior;
- Kitty graphics behavior;
- Sixel behavior;
- failure cleanup.

### Cockpit/par-term tests

Run on macOS and Linux:

```text
outer terminal: Kitty
outer terminal: Ghostty
substrate: embedded Cockpit/par-term prototype
child: bookokrat --zen-mode sample.epub
child: bookokrat sample.pdf
```

Record:

- whether graphics appear as real images, half-blocks, or missing output;
- whether par-term exposes graphics payloads;
- mouse routing;
- resize propagation;
- CPU/memory behavior;
- crash cleanup.

### Kitty backend tests

Run on macOS and Linux with Kitty:

```text
kitty remote control enabled
launch side pane/window with bookokrat --zen-mode sample.epub
launch side pane/window with bookokrat sample.pdf
replace/close/focus launched pane
```

Record:

- exact remote-control setup;
- whether argv can be passed without shell;
- security implications;
- side pane/window placement reliability;
- graphics behavior.

## Decision implication

For macOS/Linux compatibility, the substrate-swappable API is justified. No single backend cleanly covers all cases:

- Zellij covers cross-platform text/process work best.
- Cockpit/par-term is the native embedded path but needs graphics/input proof.
- Kitty is the likely best graphics path but is terminal-specific.
- Ghostty is an excellent outer terminal target but not yet a backend.
- Pure Ratatui is portable but not process-hosting.

The extension API should therefore negotiate capabilities and return degraded/unavailable responses rather than promise one universal side-panel behavior.

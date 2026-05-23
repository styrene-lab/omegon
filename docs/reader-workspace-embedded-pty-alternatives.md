+++
id = "reader-workspace-embedded-pty-alternatives"
kind = "document"
title = "Reader Workspace Embedded PTY Alternatives"
status = "exploring"
tags = ["terminal", "reader", "pty", "rust", "research"]
aliases = ["reader-workspace-embedded-pty-alternatives"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["managed-reader-workspace", "reader-workspace-zellij-spike"]
open_questions = [
  "Does any candidate crate provide production-ready PTY-backed child TUI panes beyond the Cockpit smoke test?",
  "Can Cockpit handle Bookokrat PDF/image rendering, or does the vt100/Ratatui render path lose required graphics protocols?",
  "Can Cockpit mouse routing survive real Bookokrat usage?",
  "Would embedding terminal hosting conflict with Omegon's existing TUI architecture?"
]
parent = "managed-reader-workspace"
related = ["omegon-native-terminal-pane-crate-analysis", "reader-workspace-zellij-spike"]
+++

# Reader Workspace Embedded PTY Alternatives

## Overview

Timebox research into whether Omegon should embed terminal pane hosting instead of using an external workspace substrate.

The default posture is skeptical: hosting arbitrary full-screen TUI subprocesses correctly requires real PTYs, alternate-screen handling, keyboard/mouse routing, resize propagation, rendering integration, and cleanup semantics. Those responsibilities should not move into Omegon unless an existing crate makes them unexpectedly cheap and reliable.

## Candidates

- Cockpit crate.
- r3bl_tui.
- maestro-tui.
- RMUX if it exposes a reusable Rust SDK or daemon API.
- WezTerm only as a comparison point, not a likely fit while Ghostty/Kitty remain target outer terminals.

## Cockpit smoke prototype — 2026-05-22

A scratch Cockpit prototype was created at `.tmp/cockpit-probe` and built successfully with `cargo check`. It depends on `cockpit = "0.2.2"`, `ratatui`, `crossterm`, and `tokio`.

Prototype shape:

- Ratatui alternate-screen app.
- `PaneManager::new()` with terminal-size updates on resize.
- One shell pane.
- One child command pane created with `SpawnConfig::new_command(cmd).args(args)`.
- `Ctrl+N` focus switching.
- `Ctrl+Q` prototype exit.
- Key routing through `manager.route_key(key).await`.

Validated behavior:

- Cockpit created real child PTY panes.
- A full-screen `/usr/bin/vi` child launched successfully.
- Focus switching from the shell pane to the `vi` pane worked.
- Keyboard input routed into `vi`.
- The operator inserted text, wrote the file, and exited `vi` successfully.
- The prototype then exited cleanly.

Operator validation summary:

> all works fine, I'd say success

Evidence files/commands:

```text
.tmp/cockpit-probe
cargo run -- /usr/bin/vi /tmp/cockpit-probe-manual.txt
cargo run -- /usr/bin/vi ./cockpit-probe-noninteractive.log
```

The smoke test resolves the broad assumption that embedded PTY hosting is too complex to validate quickly. Cockpit is credible enough for a dedicated embedded-reader branch. It does not yet prove Bookokrat compatibility, image/PDF behavior, mouse behavior, or a clean Omegon-shaped two-region layout.

## Cockpit two-column Bookokrat EPUB validation — 2026-05-22

The prototype was updated to bypass Cockpit's stock 4-pane/8-subpane UI and render a single `PaneWidget` inside an Omegon-owned two-column Ratatui layout:

```text
left: Omegon shell / conversation mock
right: embedded reader PTY
```

Validation results:

- The two-column layout rendered cleanly.
- Resize reflowed the Omegon mock and embedded reader pane coherently.
- Bookokrat launched successfully inside the embedded Cockpit PTY pane.
- The Pride and Prejudice EPUB rendered readable text, colors, borders, progress, and navigation/footer UI inside the embedded pane.
- Focus mode correctly reported `READER` while routing input to the child PTY.
- Initial non-zen Bookokrat mode was too narrow because Bookokrat's own library/TOC sidebar consumed width inside the embedded pane.
- Upstream docs identify `--zen-mode` as the content-only mode that hides the sidebar. Running Bookokrat with `--zen-mode` makes the EPUB reader use the full embedded PTY width.
- The Cockpit prototype also needed to avoid Cockpit's stock internal 4-slot sizing. The scratch probe currently uses the local Cockpit clone and a virtual-width workaround so the first internal pane receives the visible right-pane width.

Validated command shapes:

```bash
cd .tmp/cockpit-probe
cargo run -- bookokrat $(pwd)/../cockpit-test-assets/pride-and-prejudice.epub
cargo run -- bookokrat --zen-mode $(pwd)/../cockpit-test-assets/pride-and-prejudice.epub
```

This resolves the EPUB/text compatibility and right-pane width gates for the Cockpit branch at smoke-test level. The next unresolved gates are PDF/image behavior, Bookokrat navigation ergonomics, mouse routing, and production integration with Omegon's TUI event/layout model.

## Research gates

For each candidate, verify:

- Repository URL.
- License.
- Maintenance status.
- Whether it is a library, application, framework, or daemon.
- Whether it allocates real PTYs for arbitrary child processes.
- Whether it handles alternate-screen TUIs.
- Whether it propagates resize events.
- Whether it supports keyboard and mouse routing.
- Whether it can be embedded into an existing Ratatui/TUI architecture.
- Whether a small prototype can run Bookokrat or an equivalent full-screen TUI in a child pane.

## Timebox

Do not let embedded terminal research block the Zellij v1 path. If Zellij satisfies the core workflow, embedded alternatives should be evaluated only enough to justify rejection or future exploration.

Suggested spike limit: 0.5–1 engineering day for the most promising embedded candidate.

## Decisions

### Decision: Embedded PTY alternatives are non-blocking for Zellij v1

**Status:** proposed

**Rationale:** The fastest way to validate the product is to test Zellij. Embedded PTY hosting is a larger architectural bet and should not become the critical path unless Zellij fails the core workflow.

### Decision: Cockpit deserves a first-class embedded-reader branch

**Status:** proposed

**Rationale:** The 2026-05-22 smoke prototype proved that Cockpit can host a real full-screen TUI child (`vi`) in a PTY pane, route keyboard input after focus switching, and let the child write and exit successfully. A follow-on two-column prototype showed that Cockpit's stock multiplexer UI is not mandatory: Omegon can own the outer layout and render one `PaneWidget` as an embedded reader pane. Bookokrat successfully launched and rendered the Pride and Prejudice EPUB in that embedded pane, and `--zen-mode` makes the EPUB content use the full reader width. The remaining gates are PDF/image behavior, mouse behavior, Bookokrat navigation ergonomics, and production integration with Omegon's TUI event/layout model.

---
title: Omegon-Native Terminal Pane Crate Analysis
status: seed
tags: [research, terminal, rust, tui, pty, multiplexer, reader]
---

# Omegon-Native Terminal Pane Crate Analysis

## Purpose

Assess whether Omegon can provide a native terminal-pane capability for workflows like `omegon-reader` without requiring operators to manually install a separate multiplexer.

Design tracking for this work now lives under `docs/managed-reader-workspace.md`, with child nodes for the substrate adapter, Zellij spike, UX contract, security/licensing, and embedded PTY alternatives. This research file remains the handoff/source research brief; the design nodes own decisions and implementation readiness.

The immediate product need is:

> Open a file from an Omegon CLI session into a live adjacent terminal pane running Bookokrat, while keeping Omegon usable in the original pane.

The preferred v1 product constraint is intentionally narrow:

```text
Ghostty or Kitty
  └── managed workspace substrate
        ├── Omegon CLI
        └── Bookokrat reader pane
```

This research determines whether that substrate should be:

1. a managed external binary, e.g. Zellij;
2. an embedded Rust crate/library inside Omegon;
3. a new Omegon-native daemon/service;
4. deferred entirely in favor of requiring Zellij.

## Non-goals

- Do not integrate Bookokrat source code. Bookokrat is AGPL-3.0-or-later and should remain an external executable.
- Do not design a generic terminal emulator unless evidence shows an existing crate makes this practical.
- Do not support a broad tmux/Zellij/Kitty/Ghostty backend matrix for v1.
- Do not treat Kitty and Ghostty as pane orchestration peers; they are terminal emulators/rendering environments.

## Baseline Architecture Assumptions

- `omegon-reader` is an MIT-licensed Omegon extension using the MIT Omegon SDK.
- Bookokrat is invoked as an external process: `bookokrat <path>`.
- Managed side-pane mode should avoid tmux/Kitty/Ghostty-specific branching if possible.
- Zellij is currently the leading candidate for a single enforced workspace substrate.
- A truly embedded substrate would need to host arbitrary TUI subprocesses correctly, not just render static text.

## Research Questions

### Product Fit

1. Can the candidate open a live pane running an arbitrary subprocess such as Bookokrat?
2. Can Omegon keep running interactively in a sibling pane?
3. Can panes be named, targeted, closed, replaced, and resized programmatically?
4. Can a stable pane/session handle be returned to the Omegon extension?
5. Can the substrate be started automatically by `omegon reader session`?
6. Is the operator experience simpler than requiring a separate Zellij install?

### Technical Fit

1. Does the candidate allocate real PTYs for child processes?
2. Does it correctly handle alternate-screen TUIs?
3. Does it propagate terminal resize events to child PTYs?
4. Does it route keyboard and mouse input correctly?
5. Does it support truecolor?
6. Does it preserve or pass through terminal graphics protocols relevant to Bookokrat?
   - Kitty graphics protocol
   - Sixel
   - iTerm2 images
7. Does it work inside both Ghostty and Kitty as the outer emulator?
8. Does it expose a Rust API, CLI API, daemon protocol, or plugin API suitable for Omegon?
9. Is it cross-platform enough for Omegon's target audience?
   - macOS first
   - Linux desirable
   - Windows optional unless Omegon requires it

### Operational Fit

1. Is the project maintained?
2. Is the license compatible with Omegon?
3. Can we redistribute or manage-install the binary/library?
4. Is the dependency footprint acceptable?
5. Is the API stable enough to build on?
6. Are there security risks in its control protocol?
7. Does it require user shell configuration?
8. Can it be pinned/versioned by Omegon?

## Candidate List

Start with these candidates. Add more as discovered.

### 1. Zellij

Type: external Rust terminal multiplexer/workspace.

Initial hypothesis:

- Best near-term substrate.
- Prefer as managed external binary rather than embedded library.
- Need to verify programmatic pane lifecycle control.

Research tasks:

- Verify license.
- Verify CLI commands for:
  - start named session;
  - open pane with command;
  - name pane;
  - target existing pane;
  - close pane;
  - replace/respawn pane;
  - resize/split direction.
- Determine whether a plugin is required for stable reader-pane control.
- Test Bookokrat in Zellij inside Ghostty and Kitty.
- Test EPUB text rendering and image/PDF rendering behavior.

### 2. Cockpit crate

Type: possible embeddable terminal multiplexer library for Ratatui apps.

Initial hypothesis:

- Interesting but likely risky for v1.
- Could move too much terminal-host complexity into Omegon.

Research tasks:

- Verify crate URL, repository, license, maintenance status.
- Determine whether it hosts arbitrary subprocesses in PTYs.
- Determine whether it can be embedded in an existing Omegon CLI/TUI architecture.
- Check examples for split panes and subprocess lifecycle.
- Test whether it can run a full-screen TUI child reliably.

### 3. r3bl_tui

Type: Rust TUI framework with in-memory terminal emulation components.

Initial hypothesis:

- More likely useful as inspiration than as a direct pane substrate.

Research tasks:

- Verify license and maintenance.
- Determine whether it provides PTY-backed child process panes.
- Determine whether it is framework-invasive.
- Assess effort to embed Bookokrat as a child TUI process.

### 4. maestro-tui

Type: possible app/framework with PTY pane patterns.

Initial hypothesis:

- Research candidate; probably not a stable substrate.

Research tasks:

- Verify crate/repo/license.
- Identify whether it is a library or app.
- Assess reusable PTY/pane modules.
- Check compatibility with macOS.

### 5. RMUX

Type: Rust terminal multiplexer/daemon with programmable SDK, if available/mature.

Initial hypothesis:

- Potentially ideal long-term if real, licensed compatibly, and mature.
- Too new/risky for v1 until verified.

Research tasks:

- Verify repository, license, release status, docs.
- Validate claims:
  - tmux-compatible CLI;
  - typed async Rust SDK;
  - stable pane IDs;
  - macOS/Linux/Windows PTY support.
- Assess daemon control protocol security.
- Compare with Zellij for Omegon agent control.

### 6. WezTerm

Type: Rust terminal emulator with multiplexing.

Initial hypothesis:

- Not a fit if Ghostty/Kitty remain target outer terminals.
- Useful comparison point only.

Research tasks:

- Confirm whether it can be used as a library/substrate independently of replacing the user's terminal emulator.
- If not, mark rejected for v1.

## Evaluation Matrix

Use this table for each candidate.

| Criterion | Weight | Zellij | Cockpit | r3bl_tui | maestro-tui | RMUX | WezTerm |
|---|---:|---:|---:|---:|---:|---:|---:|
| License compatible | 5 | TBD | TBD | TBD | TBD | TBD | TBD |
| Maintained | 5 | TBD | TBD | TBD | TBD | TBD | TBD |
| Real PTY panes | 5 | TBD | TBD | TBD | TBD | TBD | TBD |
| Programmatic pane handles | 5 | TBD | TBD | TBD | TBD | TBD | TBD |
| Can run Bookokrat | 5 | TBD | TBD | TBD | TBD | TBD | TBD |
| Ghostty outer terminal | 4 | TBD | TBD | TBD | TBD | TBD | TBD |
| Kitty outer terminal | 4 | TBD | TBD | TBD | TBD | TBD | TBD |
| Graphics protocol behavior | 4 | TBD | TBD | TBD | TBD | TBD | TBD |
| Easy managed install | 4 | TBD | TBD | TBD | TBD | TBD | TBD |
| Embeddable API | 3 | TBD | TBD | TBD | TBD | TBD | TBD |
| Operational simplicity | 5 | TBD | TBD | TBD | TBD | TBD | TBD |
| Security model | 5 | TBD | TBD | TBD | TBD | TBD | TBD |

Scoring:

- `0`: no / unsuitable
- `1`: weak / major gaps
- `2`: possible but risky
- `3`: acceptable
- `4`: strong
- `5`: excellent

## Spike Plan

### Spike A: Zellij managed external substrate

Goal: prove the simplest enforced-substrate model.

Steps:

1. Install or locate Zellij.
2. Start a named session for Omegon Reader.
3. From inside the session, open a pane running a harmless command.
4. Open a pane running `bookokrat <sample.epub>`.
5. Capture any pane/session identifiers available.
6. Test close/replace behavior.
7. Record behavior in Ghostty and Kitty.

Acceptance:

- Omegon can reliably open Bookokrat in a side pane.
- Operator can continue using Omegon in the sibling pane.
- The extension can close or replace the reader pane, or the limitation is explicitly documented.

### Spike B: Embedded PTY pane feasibility

Goal: determine whether embedding a crate meaningfully simplifies product UX.

Steps:

1. Pick the most promising embeddable crate after docs/license review.
2. Build a tiny Rust prototype:

```text
left pane: simple Omegon placeholder input loop
right pane: PTY child running `bookokrat <sample.epub>` or another full-screen TUI
```

3. Test resize, alt-screen, keyboard routing, quit behavior, and crash cleanup.

Acceptance:

- If the prototype takes more than a small spike to behave correctly, reject embedded mode for v1.
- If graphics/mouse/alt-screen behavior is broken, reject embedded mode for v1.

### Spike C: RMUX validation

Goal: determine if RMUX deserves a future design branch.

Steps:

1. Verify project existence, license, maturity, docs.
2. Run a local demo if possible.
3. Compare pane-control API against Zellij.

Acceptance:

- Promote to future candidate only if it is real, licensed compatibly, and easier to control programmatically than Zellij.

## Expected Decision Output

At the end of this research, produce a short ADR:

```text
Decision: Use <candidate> as the Omegon Reader managed pane substrate.

Status: proposed/decided/rejected

Rationale:
- ...

Consequences:
- ...

Rejected alternatives:
- ...
```

## Current Leaning

The current leaning is:

```text
Use Zellij as a single enforced managed external substrate for v1.
Do not build internal terminal multiplexing into Omegon unless research shows an embeddable crate is unexpectedly mature and low-risk.
```

Reasoning:

- Zellij avoids the tmux/Kitty/Ghostty compatibility matrix.
- It keeps pane orchestration outside Omegon core.
- It is Rust-native and aligned with the desired ecosystem.
- Embedding terminal multiplexing would make Omegon responsible for PTY hosting and terminal emulation edge cases.

## Notes for Agents

- Ground claims with repository links, licenses, docs, and local command output.
- Do not rely on marketing copy alone.
- Prefer small runnable spikes over abstract comparison.
- Keep Bookokrat as an external process in all prototypes.
- Record exact versions tested.
- If a candidate requires shell interpolation to launch subprocesses, flag that as a security concern.

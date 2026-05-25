+++
id = "reader-workspace-security-licensing"
kind = "document"
title = "Reader Workspace Security and Licensing"
status = "seed"
tags = ["terminal", "reader", "security", "licensing", "process"]
aliases = ["reader-workspace-security-licensing"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["managed-reader-workspace"]
open_questions = [
  "[assumption] Bookokrat can remain a process-only dependency without creating licensing obligations for Omegon beyond user-facing disclosure.",
  "What Zellij license/distribution obligations apply if Omegon offers managed install or bundling?",
  "Can all substrate commands pass `bookokrat` and file paths as argv without shell interpolation?",
  "What information should error messages include without leaking sensitive local paths unnecessarily?",
  "Does the substrate expose a control socket or command channel that needs authentication/authorization review?"
]
parent = "managed-reader-workspace"
related = ["reader-workspace-substrate-adapter", "reader-workspace-zellij-spike"]
+++

# Reader Workspace Security and Licensing

## Overview

Define the security and licensing constraints for managed reader panes.

The core risk is that a reader-pane feature can become an arbitrary process execution API if the abstraction is too broad. The v1 surface should remain capability-specific and path-safe.

## Security requirements

- Treat file paths as path arguments, not shell fragments.
- Avoid `sh -c` and shell interpolation in pane launch commands.
- Validate that the requested file exists before launching the reader.
- Preserve spaces and unicode in file names.
- Report spawn failures clearly.
- Do not expose a generic extension API for arbitrary pane command execution in v1.
- If a substrate daemon/control socket is used, document who can issue commands and how sessions are scoped.

## Licensing boundaries

### Bookokrat

Bookokrat is AGPL-3.0-or-later and should remain an external executable. Omegon should invoke `bookokrat <path>` as a subprocess and must not link or vendor Bookokrat source for this feature.

### Zellij

Zellij license and distribution terms must be verified before choosing managed install or bundling. User-installed Zellij has fewer redistribution obligations than Omegon-managed binary distribution.

### Embedded Rust crates

Embedding a PTY/multiplexer crate creates a stronger license and maintenance relationship than spawning an external substrate. License compatibility must be verified before any embedded prototype becomes implementation.

## Research checklist

- Record Zellij license with repository link.
- Record Bookokrat license with repository link.
- Record licenses for Cockpit, r3bl_tui, maestro-tui, RMUX, and WezTerm if they remain candidates.
- Determine whether managed install is allowed and operationally acceptable.
- Determine whether command construction can be argv-only.
- Review control-channel security for any daemon-like substrate.

## Decisions

### Decision: No generic arbitrary pane execution API in v1

**Status:** proposed

**Rationale:** The product need is reader-specific. Exposing arbitrary pane execution would increase security risk and support burden without being necessary for the first implementation.

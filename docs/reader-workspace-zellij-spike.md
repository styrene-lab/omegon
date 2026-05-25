+++
id = "reader-workspace-zellij-spike"
kind = "document"
title = "Reader Workspace Zellij Spike"
status = "seed"
tags = ["terminal", "reader", "zellij", "spike", "pty"]
aliases = ["reader-workspace-zellij-spike"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["managed-reader-workspace"]
open_questions = [
  "[assumption] Zellij is installed or can be installed on the operator's machine for spike validation.",
  "[assumption] A representative Bookokrat sample document is available for EPUB/text testing.",
  "Does Zellij expose stable enough pane identifiers for replacement?",
  "Does Bookokrat image/PDF behavior require terminal graphics passthrough that Zellij does not support?"
]
parent = "managed-reader-workspace"
related = ["reader-workspace-substrate-adapter", "reader-workspace-ux-contract", "reader-workspace-security-licensing"]
+++

# Reader Workspace Zellij Spike

## Overview

Collect concrete evidence for using Zellij as the v1 managed workspace substrate for Omegon Reader.

This node should record exact commands, versions, terminals, results, and failures. Marketing claims are not enough.

## Spike protocol

### Environment facts

Record:

- macOS version.
- Outer terminal: Ghostty version.
- Outer terminal: Kitty version.
- Zellij version and install method.
- Bookokrat version and install method.
- Omegon build/version used for manual testing.
- Sample file names and formats tested.

### Required tests

1. Start or attach a named Zellij session for Omegon Reader.
2. Launch Omegon in one pane.
3. From the Omegon pane or equivalent command context, open a side pane running a harmless command.
4. Open a side pane running `bookokrat <sample.epub>`.
5. Repeat with a path containing spaces.
6. Verify the original Omegon pane remains interactive.
7. Resize the terminal and verify Bookokrat receives the resize.
8. Close Bookokrat and verify Omegon survives.
9. Manually close the reader pane and observe whether Omegon can recover.
10. Attempt close/replace of the reader pane.
11. Repeat relevant tests in Ghostty and Kitty.

### Graphics/protocol tests

If Bookokrat supports non-text rendering paths, test representative files for:

- EPUB text.
- Images if applicable.
- PDF if applicable.
- Kitty graphics protocol behavior.
- Sixel behavior if applicable.
- Fallback behavior when graphics are unsupported.

## Acceptance gates

Zellij is acceptable for v1 if:

- It can open a Bookokrat side pane without killing or blocking Omegon.
- The original Omegon pane remains interactive.
- It works in at least the primary target outer terminal and has a clear result for the secondary target.
- Command construction can avoid shell interpolation for file paths, or the escaping requirement is precisely documented.
- Missing/failed reader process behavior is understandable and recoverable.

Pane replacement is preferred. If replacement is not reliable, v1 may proceed only if open-only behavior is explicitly accepted by the product node.

## Evidence log

_To be filled during spike execution._

## Decisions

### Decision: Zellij spike is the first implementation gate

**Status:** proposed

**Rationale:** Zellij is the leading candidate and can validate or falsify the central architecture assumption faster than embedded PTY crate research.

+++
id = "reader-extension-side-pane-contract"
kind = "document"
title = "Reader Extension Side-Pane Contract"
status = "seed"
tags = ["reader", "extension", "bookokrat", "side-pane", "api"]
aliases = ["reader-extension-side-pane-contract"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "extension-side-process-substrate-api"
dependencies = ["extension-side-process-substrate-api", "managed-reader-workspace"]
open_questions = [
  "Should `omegon-reader` call a reader-specific core capability or a general side-process capability?",
  "Which Bookokrat modes map to required versus preferred substrate capabilities?",
  "Should Bookokrat be user-installed, managed-installed, or extension-owned?",
  "What is the degraded behavior when graphics passthrough is unavailable?",
  "Should reader panes be replaced by logical document identity or opened as separate panes?"
]
related = ["managed-reader-workspace", "reader-workspace-ux-contract", "reader-workspace-security-licensing", "side-process-backend-terminal-compatibility-matrix"]
+++

# Reader Extension Side-Pane Contract

## Overview

Define the first concrete consumer of the side-process substrate API: the reader extension opening Bookokrat in a side pane.

This node keeps the generic substrate API honest by grounding it in one product workflow.

## Candidate command flow

```text
operator: /reader open path/to/book.epub
  ↓
omegon-reader extension validates user intent
  ↓
extension requests ReaderPane capability from core
  ↓
core validates file path and policy
  ↓
substrate backend opens Bookokrat side pane
  ↓
conversation reports opened/degraded/unavailable status
```

## Reader modes

Potential modes:

- `text` — EPUB/text-first reading; graphics preferred but not required.
- `image` — image/document viewing; graphics required.
- `pdf` — PDF rendering; graphics required unless Bookokrat has acceptable text fallback.
- `auto` — choose requirements based on file type.

## Bookokrat invocation policy

Initial command shape:

```text
bookokrat --zen-mode <path>
```

`--zen-mode` is preferred for side panes because prior Cockpit validation showed the normal Bookokrat sidebar consumes too much width inside embedded panes.

Core should pass this as argv, not shell text.

## Placement and reuse

Initial defaults:

- placement: right;
- reuse: replace named `reader` pane;
- title: basename of document or `Reader`;
- lifecycle: close with workspace/session unless backend naturally persists and operator opts in.

Open question: if stable pane replacement is unavailable, v1 may be open-only with duplicate panes and a clear status message.

## Response UX

Operator-facing messages should distinguish:

- opened normally;
- opened with degraded capabilities, e.g. no graphics passthrough;
- unavailable because substrate missing;
- denied because extension lacks permission;
- failed because Bookokrat missing or exited.

## Research tasks

1. Define exact reader request struct.
2. Map file type to required/preferred capabilities.
3. Define Bookokrat discovery/version check.
4. Define setup instructions for missing Bookokrat and missing substrate.
5. Validate EPUB, PDF, and image behavior across chosen backends.

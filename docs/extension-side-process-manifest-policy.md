+++
id = "extension-side-process-manifest-policy"
kind = "document"
title = "Extension Side-Process Manifest Policy"
status = "seed"
tags = ["extension", "manifest", "security", "process", "policy"]
aliases = ["extension-side-process-manifest-policy"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "extension-side-process-substrate-api"
dependencies = ["extension-side-process-substrate-api"]
open_questions = [
  "How should extension manifests declare allowed side-process commands without becoming too verbose?",
  "Should commands be declared as fixed argv templates, binary allowlists, or named core capabilities?",
  "Should operator consent be required the first time an extension launches a side process?",
  "How much environment inheritance is allowed?",
  "How should policy distinguish bundled tools, user-installed tools, and core-managed dependencies?"
]
related = ["reader-workspace-security-licensing", "extension-side-process-substrate-api"]
+++

# Extension Side-Process Manifest Policy

## Overview

Define how an extension declares permission to request side-process panes.

The core problem: side-process panes are useful, but an unstructured API would let extensions become arbitrary process launchers. The manifest must make process capability explicit, reviewable, and enforceable.

## Candidate manifest shape

```toml
[side_process.reader]
label = "Reader pane"
intent = "Open documents in a side pane using Bookokrat"
command = "bookokrat"
args_template = ["--zen-mode", "{path}"]
path_args = ["path"]
placements = ["right", "bottom"]
reuse = ["replace_named", "open_new"]
requires = ["host_process", "adjacent_pane", "resize_propagation"]
prefers = ["graphics_passthrough", "mouse_passthrough"]
```

This is illustrative, not final. The important property is that command intent and argument positions are declarative.

## Policy dimensions

### Command authority

Options:

1. Core-named capabilities only, e.g. `reader.bookokrat`.
2. Fixed binary allowlist from manifest.
3. Fixed argv template from manifest.
4. Arbitrary argv requested by extension at runtime.

Initial recommendation: use (1) or (3), not (4).

### Path handling

- Path arguments must be typed as paths.
- Core validates existence before launch when required.
- Core preserves spaces and unicode by passing argv, not shell strings.
- Core rejects path traversal only when a capability is scoped to a root; absolute local document paths may be valid for reader workflows.

### Environment

- Default: minimal inherited environment.
- Allow explicit environment variables only when declared.
- Never pass secrets by default.
- If a side process needs secrets, require a separate explicit capability.

### Operator consent

Potential consent levels:

- no prompt for core-owned built-in capability;
- first-use prompt for extension-declared command;
- always prompt for untrusted extension or unsigned package;
- deny by default in headless/automation mode unless configured.

## Research tasks

1. Inspect current extension manifest schema and install trust model.
2. Decide whether side-process policy belongs in the extension manifest, core config, or both.
3. Draft exact TOML schema.
4. Define validation errors and operator-facing messages.
5. Add tests for forbidden shell interpolation, undeclared commands, invalid path args, and unavailable substrate capability.

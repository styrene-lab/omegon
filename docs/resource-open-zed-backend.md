---
id: resource-open-zed-backend
title: "Implement Zed backend for resource.open@1"
status: seed
tags: [host-actions, extensions, resource-open, zed]
open_questions:
  - "[assumption] Zed can be invoked through a policy-gated host opener or CLI path that does not weaken existing process-spawn controls."
dependencies:
  - resource-open-real-backends-125
related:
  - docs/resource-open-real-backends-125.md
---

# Implement Zed backend for resource.open@1

## Overview

Follow-up to the limited #125 implementation. `resource.open@1` now validates policy, parses `file://` URIs with `url::Url`, reports preferred/selected backend diagnostics, and routes ebook/pdf resources through the terminal/Bookokrat backend. Code, text, and directory resources currently route to an explicit Zed unavailable diagnostic.

This node tracks the remaining work to attach a real Zed/editor resource-open backend for editor-oriented resources.

## Target resources

- `code`
- `text`
- `directory`

## Initial constraints

- Preserve the existing HostAction policy order: manifest and workspace-root validation must happen before any editor handoff.
- Use argument-vector process spawning or a host opener abstraction; do not introduce shell-string execution.
- The backend should return a `ResourceOpenResult` with backend `zed`, actual placement, and an opaque handle where available.
- If Zed is unavailable, retain explicit unavailable diagnostics rather than falling back silently.
- Workspace-root decisions must come from the executor context, not ambient `std::env::current_dir()`.

## Open implementation questions

1. Is the intended adapter Zed CLI, macOS app opener, or an ACP/editor-host surface?
2. What runtime policy should allow or deny launching an external editor?
3. Should directories open as projects/workspaces while code/text files open as editor tabs?

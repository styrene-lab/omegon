---
id: resource-open-real-backends-125
title: "Implement real resource.open@1 backends for Flynt, Zed, and terminal reader"
status: implemented
tags: [0.27.0, host-actions, extensions, resource-open]
open_questions: []
dependencies:
  - workstream/0.27-sdk-contract
related:
  - docs/workstream-handoff-resource-open-real-backends-125.md
  - docs/resource-open-flynt-backend.md
  - docs/resource-open-zed-backend.md
---

# Implement real resource.open@1 backends for Flynt, Zed, and terminal reader

## Overview

Follow-up to issue #83 / commit d392f23d. The resource.open@1 HostAction substrate now exists with SDK contract support, manifest policy validation, secure workspace-root enforcement, backend registry scaffolding, deterministic unavailable fallback, and fake-backend routing tests. This node tracks the limited #125 completion: terminal/Bookokrat is wired as the real operator-visible reader backend, while Flynt and Zed are explicitly deferred to follow-up design nodes with unavailable diagnostics preserved.

## Current substrate evidence

Observed in `core/crates/omegon/src/extensions/host_actions.rs`:

- `preferred_resource_backend_kind` routes markdown/diagram/image to Flynt, code/text/directory to Zed, ebook/pdf to Terminal, and unknown/missing kind to Fallback.
- `ResourceOpenBackend` owns the backend interface: `name`, `kind`, `supports`, `unavailable_reason`, and `open`.
- `ResourceBackendRegistry::select` prefers the routed backend kind, then fallback, then any supporting backend.
- `execute_resource_open` validates params, manifest allow flag, scheme, intent, kind, and file roots before selecting any backend.
- `execute_resource_open` reports preferred and selected backend diagnostics when a backend is unavailable or fails after policy validation.
- `file_uri_path` uses `url::Url` for `file://` parsing, including encoded path decoding and non-local host rejection before workspace-root checks.
- `path_allowed_by_roots` uses the executor `workspace_cwd` for `${workspace}` resolution and normalizes/canonicalizes absolute file paths.
- The real terminal executor registry wires ebook/pdf resources to the terminal/Bookokrat backend, keeps Flynt/Zed as explicit unavailable diagnostics, and keeps unknown resources on explicit fallback unavailable behavior.

## Implemented decisions

### Backend ownership boundaries

- Flynt owns rendered/document resources: markdown, diagrams, and images. Real Flynt opening is deferred to [[resource-open-flynt-backend]]; current behavior reports explicit Flynt unavailable diagnostics.
- Zed owns editor-oriented resources: code, text, and directories. Real Zed opening is deferred to [[resource-open-zed-backend]]; current behavior reports explicit Zed unavailable diagnostics.
- Terminal owns reader-oriented resources: ebooks and PDFs. The implemented backend translates eligible `resource.open@1` requests into the existing `terminal.create@1`/Bookokrat execution path so process execution remains covered by terminal HostAction policy.
- Fallback remains an explicit diagnostic backend, not a hidden catch-all success path.

### Runtime availability model

Backends distinguish selection from availability. Outcomes say which backend was preferred, which backend was selected, and why execution could not proceed when unavailable.

### URI parsing

`file://` parsing uses `url::Url`. This covers encoded paths and rejects non-local file URI hosts before workspace-root checks.

### Workspace root source

#125 uses the executor-provided `workspace_cwd` as the policy root. No backend added by this work derives workspace root from ambient `std::env::current_dir()`.

## Completed scope

- Added backend availability/diagnostic surface to `ResourceOpenBackend`.
- Implemented terminal reader backend via existing `terminal.create@1`/Bookokrat plumbing.
- Added tests for terminal ebook/pdf routing, Flynt/Zed unavailable diagnostics, fallback unavailable diagnostics, real registry wiring, and file URI parsing.
- Upgraded `file://` parsing to `url::Url`.
- Updated `CHANGELOG.md` under `[Unreleased]`.

## Validation

Passed during implementation:

```bash
cargo test -p omegon resource_open -- --nocapture
cargo test -p omegon terminal_create -- --nocapture
cargo check -p omegon
git diff --check
```

## Follow-up nodes

- [[resource-open-flynt-backend]] — implement real Flynt opening for markdown, diagram, and image resources.
- [[resource-open-zed-backend]] — implement real Zed/editor opening for code, text, and directory resources.

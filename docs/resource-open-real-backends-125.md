---
id: resource-open-real-backends-125
title: "Implement real resource.open@1 backends for Flynt, Zed, and terminal reader"
status: exploring
tags: [0.27.0, host-actions, extensions, resource-open]
open_questions:
  - "[assumption] Flynt can expose a host-side resource-open surface or adapter without requiring extensions to know Flynt internals."
  - "[assumption] Zed invocation can be implemented as a policy-gated host opener without weakening existing HostAction process-spawn controls."
  - "[assumption] Bookokrat/reader opening can be reached through the existing terminal.create@1 backend registry rather than a separate process-spawn path."
dependencies:
  - workstream/0.27-sdk-contract
related:
  - docs/workstream-handoff-resource-open-real-backends-125.md
---

# Implement real resource.open@1 backends for Flynt, Zed, and terminal reader

## Overview

Follow-up to issue #83 / commit d392f23d. The resource.open@1 HostAction substrate now exists with SDK contract support, manifest policy validation, secure workspace-root enforcement, backend registry scaffolding, deterministic unavailable fallback, and fake-backend routing tests. This node tracks the remaining operator-visible backend work from GitHub issue #125: route validated resource.open@1 requests to real Flynt, Zed, and terminal/Bookokrat backends while preserving explicit fallback outcomes and auditability.

## Current substrate evidence

Observed in `core/crates/omegon/src/extensions/host_actions.rs`:

- `preferred_resource_backend_kind` routes markdown/diagram/image to Flynt, code/text/directory to Zed, ebook/pdf to Terminal, and unknown/missing kind to Fallback.
- `ResourceOpenBackend` owns the backend interface: `name`, `kind`, `supports`, and `open`.
- `ResourceBackendRegistry::select` prefers the routed backend kind, then fallback, then any supporting backend.
- `execute_resource_open` validates params, manifest allow flag, scheme, intent, kind, and file roots before selecting any backend.
- `file_uri_path` is currently simple `file://` prefix stripping.
- `path_allowed_by_roots` uses the executor `workspace_cwd` for `${workspace}` resolution and normalizes/canonicalizes absolute file paths.

## Decisions to carry into implementation

### Backend ownership boundaries

- Flynt owns rendered/document resources: markdown, diagrams, and images. If the current host runtime cannot address Flynt directly, the Flynt backend must report explicit unavailability rather than silently pretending success.
- Zed owns editor-oriented resources: code, text, and directories. It should be implemented as a host opener/CLI integration only after command/path policy is explicit.
- Terminal owns reader-oriented resources: ebooks and PDFs. The first real backend should translate eligible `resource.open@1` requests into the existing `terminal.create@1`/Bookokrat execution path so process execution remains covered by terminal HostAction policy.
- Fallback remains an explicit diagnostic backend, not a hidden catch-all success path.

### Runtime availability model

Backends should distinguish selection from availability. The registry may preserve deterministic preferred-kind routing, but outcomes must say which backend was selected and why execution could not proceed when unavailable. Fallback warnings are part of the operator-visible contract for #125.

### URI parsing

Introduce `url::Url` for `file://` parsing before broadening real backend support. The current prefix-stripping is acceptable for the #83 substrate tests but is too weak for real backend UX because encoded paths, hosts, and platform-specific path forms need explicit handling.

### Workspace root source

Keep `workspace_cwd` as the policy input for #125 only if it is threaded from the executor setup path explicitly. Do not add a backend that derives workspace root from ambient `std::env::current_dir()`; that creates confusing policy behavior when the host process cwd differs from the operator workspace.

### Closure criteria

#125 is complete when:

- terminal/Bookokrat has a real operator-visible backend path or a documented runtime-gated unavailable outcome with tests;
- Flynt and Zed have either real host integrations or explicit unavailable diagnostics with preferred-route tests;
- fallback outcomes include backend selection/availability information;
- file URI handling is upgraded or the remaining limitations are documented in the tests and changelog;
- `CHANGELOG.md` is updated under `[Unreleased]`;
- validation covers `cargo test -p omegon resource_open`, relevant terminal-create tests, `cargo check -p omegon`, and `git diff --check`.

## Implementation sequence

1. Add backend availability/diagnostic surface to `ResourceOpenBackend` or `ResourceBackendRegistry` without changing policy validation order.
2. Implement the terminal reader backend first by reusing `terminal.create@1`/Bookokrat plumbing where policy permits.
3. Add tests for ebook/pdf routing through Terminal, markdown preferred Flynt unavailable diagnostics, code preferred Zed unavailable diagnostics, and fallback warning shape.
4. Upgrade `file://` parsing to `url::Url` or add narrowly scoped tests documenting the accepted subset before real backend execution.
5. Implement Flynt/Zed real adapters only after their host-side invocation path is identified; otherwise land explicit unavailable backends with clear diagnostics.

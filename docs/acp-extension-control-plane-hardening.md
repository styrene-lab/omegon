---
id: acp-extension-control-plane-hardening
title: "ACP extension control-plane hardening"
status: deferred
tags: [acp, extensions, control-plane, policy, issue-132-followup]
open_questions:
  - "[assumption] 0.26.9 generic extension call should be constrained to loaded/enabled extensions, while later hardening can add richer policy and lifecycle control."
  - "[assumption] Extension manifest metadata can eventually declare RPC method policy without breaking current extensions."
dependencies:
  - acp-132-0-26-9-completion
related:
  - docs/acp-132-runtime-observability-extension-control.md
---

# ACP extension control-plane hardening

## Overview

0.26.9 should add the minimal `_extensions/call` needed to close issue #132 P0. This follow-up owns the broader extension control-plane that becomes obvious once generic invocation exists.

## Scope

- Extension RPC method policy declarations.
- Extension lifecycle operations beyond invocation:
  - reload
  - restart
  - enable/disable status refresh
  - load diagnostics refresh
- Extension method catalog/introspection if the SDK supports it.
- Per-method safety classification:
  - read-only
  - mutating
  - host-action mediated
  - policy-denied
- Better structured error taxonomy shared by extension tool calls and ACP extension RPC calls.

## Non-goals

- No Flynt-only endpoint.
- No bypass of host-action approval or extension manifest policy.
- No automatic extension respawn in the 0.26.9 minimal call path unless explicitly designed here later.

## Design direction

Generic extension RPC is a control-plane, not just an escape hatch. After 0.26.9, extension method invocation should become policy-aware, introspectable, and diagnosable. The control plane should explain not only whether an extension is loaded, but whether a particular operation is allowed and why.

## Acceptance criteria

- Extension manifests or runtime metadata can describe callable method policy.
- ACP clients can determine whether an extension method is read-only, mutating, or host-action mediated before invoking it.
- Extension lifecycle state can be refreshed or restarted through structured operations without relying on chat commands.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.

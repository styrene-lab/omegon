---
id: browser-integration-system-fallback-approval
title: "Browser Integration Pattern for Manual System Fallback Approval"
status: exploring
tags: [host-actions, resource-open, browser, approval, system-default]
open_questions:
  - "[assumption] ACP/Flynt approval cards can render degraded fallback metadata clearly enough for operators to distinguish host-owned backends from system-default/browser fallback."
  - "Which URI schemes, if any, should be eligible for browser/system-default fallback beyond file:// in v1: https only, http+https, or none?"
dependencies: []
related: []
---

# Browser Integration Pattern for Manual System Fallback Approval

## Overview

Design future browser/resource-opening integrations so host-owned backends are preferred and OS/browser/system-default fallback is always presented as a manual operator approval step. This prevents extensions from silently escaping into external desktop/browser state while preserving the operator's configured default programs as an explicit fallback path.

## Research

### HostAction domain implications

Current HostAction taxonomy separates operator intent from concrete backend. terminal.create@1 owns process/session lifecycle. resource.open@1 owns semantic resource presentation. OS default opener/browser integration should be modeled as a ResourceOpenBackend with backend=system_default or browser_default and actual_placement=external. When selected only as fallback, it must produce a needs_approval/manual review outcome rather than executing automatically.

## Decisions

### System-default and browser fallback is manual-approval only

**Status:** decided

**Rationale:** OS/browser associations can launch arbitrary external applications, escape host observability, and differ per machine. Treating them as automatic fallbacks would make resource.open@1 behavior nondeterministic and less auditable. Manual approval preserves operator agency while still using configured defaults when the operator wants them.

### Browser integrations start as resource.open@1 backends

**Status:** decided

**Rationale:** The immediate operator intent is usually 'view this resource' rather than 'control a browser'. Browser/system-default support should therefore begin as a backend selected by the host resource router. A separate browser HostAction domain should only be introduced if future use cases require browser-specific policy axes such as tab automation, profile selection, cookies, or DOM/navigation control.

## Open Questions

- [assumption] ACP/Flynt approval cards can render degraded fallback metadata clearly enough for operators to distinguish host-owned backends from system-default/browser fallback.
- Which URI schemes, if any, should be eligible for browser/system-default fallback beyond file:// in v1: https only, http+https, or none?

## Implementation Notes

### File Scope

- `core/crates/omegon/src/extensions/host_actions.rs` — 
- `core/crates/omegon/src/extensions/approval.rs` — 
- `/Users/wilson/workspace/styrene-labs/omegon-extensions/omegon-extension-rs/src/actions/resource.rs` — 
- `docs/browser-integration-system-fallback-approval.md` —

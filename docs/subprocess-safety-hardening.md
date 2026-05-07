+++
id = "11fde41e-21f6-44c0-84d4-2f171078bb30"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Subprocess safety hardening

## Overview

Narrow the repo-consolidation-hardening effort to a first concrete slice that removes risky shell-string execution and broad process-management patterns in browser/server/process helpers, replacing them with safer process spawning and explicit argument handling.

## Research

### Why this is the right first consolidation slice

The parent repo-consolidation-hardening topic is still proposal-only and spans architecture, lifecycle, security, and UX concerns. A subprocess-safety slice is concrete, testable, and cross-cutting enough to deliver immediate hardening without trying to refactor multiple large extensions at once. It aligns with the earlier repo assessment finding to replace broad pkill patterns and shell-string execution with explicit process spawning and argument handling.

## Decisions

### Decision: Start repo consolidation with subprocess/process-management hardening

**Status:** decided
**Rationale:** This slice is small enough to specify and verify cleanly, improves security immediately, and avoids stalling on a repo-wide architecture rewrite before any concrete risk is reduced.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/web-ui/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/web-ui/index.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/local-inference/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/local-inference/index.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/bootstrap/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/bootstrap/index.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/subprocess-safety-hardening/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/subprocess-safety-hardening.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Prefer explicit executable plus argv subprocess dispatch over shell-string command construction in helper flows.
- Do not terminate unrelated Ollama processes when no managed child is tracked by Omegon.

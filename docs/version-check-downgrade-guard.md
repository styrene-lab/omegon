---
id: version-check-downgrade-guard
title: Version check downgrade guard — suppress false update prompts from older registry versions
status: implemented
parent: pi-fork-update-flow
tags: [pi-mono, version-check, bugfix, ux]
open_questions: []
branches: ["feature/version-check-downgrade-guard"]
openspec_change: version-check-downgrade-guard
issue_type: bug
priority: 2
---

# Version check downgrade guard — suppress false update prompts from older registry versions

## Overview

Fix false update notifications caused by treating any registry version mismatch as an available update, even when the registry reports an older version than the running build.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/version-check.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/version-check-downgrade-guard/design.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/version-check-downgrade-guard/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `vendor/pi-mono/packages/coding-agent/src/modes/interactive/interactive-mode.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `vendor/pi-mono/packages/coding-agent/test/interactive-mode-status.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `tests/version-check.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Handle suffix-bearing versions like 0.58.1-cwilson613.2 as newer than 0.58.1-cwilson613.1 while still suppressing downgrade prompts.
- Suffix-bearing versions must compare by numeric segments so older suffix builds do not trigger downgrade prompts while newer suffix builds still notify.

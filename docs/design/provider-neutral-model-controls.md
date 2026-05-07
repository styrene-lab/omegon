+++
id = "0f78651f-7739-46ca-a8d9-9051b45ff31f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider-neutral model controls and driver persistence

## Overview

Track the implementation that makes model controls provider-neutral in operator-facing UX and persists the last-used concrete driver model across sessions.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/model-budget.ts` (modified) — Provider-aware tier descriptions and concrete provider/model notifications
- `extensions/effort/index.ts` (modified) — Restore persisted driver model on startup and report resolved provider/model
- `extensions/lib/model-preferences.ts` (new) — Persist and load last-used concrete driver model from `.omegon/profile.json`
- `extensions/dashboard/footer.ts` (modified) — Compact footer cleanup to a single dashboard-first line with inline model visibility
- `extensions/model-budget.test.ts` (new) — Coverage for provider-aware model control copy
- `extensions/lib/model-preferences.test.ts` (new) — Coverage for last-used model persistence helpers
- `extensions/dashboard/footer-compact.test.ts` (new) — Coverage for compact footer single-line rendering and inline model display

### Constraints

- Persist only successful explicit model switches; failed switch attempts must not overwrite a working saved model.
- On session_start, restore the persisted concrete model before falling back to effort-tier default routing.
- Compact dashboard footer should remain single-line and dashboard-first while still exposing active model/provider at a glance on wide terminals.

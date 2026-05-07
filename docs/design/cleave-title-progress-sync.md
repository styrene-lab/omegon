+++
id = "0089b71c-c5ea-4d46-a08a-fa91c0375708"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Keep terminal title cleave progress in sync

## Overview

Ensure terminal tab titles update as cleave child progress changes so counts like 0/3, 1/3, and 3/3 reflect live dispatcher state instead of only lifecycle phase boundaries.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/dispatcher.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/cleave/dispatcher.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Terminal-title refresh remains event-driven: live cleave counts update when dispatcher child-progress mutations emit dashboard:update, rather than relying on polling or phase-only refreshes.

+++
id = "fccda730-f984-4ac2-b533-bc87df623e7b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave-process-tree — Design

## Spec-Derived Architecture

### dispatch

- **spawnChild uses RPC mode for structured bidirectional communication** (added) — 3 scenarios
- **Task file contract is preserved** (added) — 1 scenarios
- **Review subprocess stays on pipe mode in Phase 1** (added) — 1 scenarios
- **Graceful degradation when RPC pipe breaks** (added) — 2 scenarios
- **Dashboard progress uses structured events** (added) — 2 scenarios

## File Changes

<!-- Add file changes as you design the implementation -->

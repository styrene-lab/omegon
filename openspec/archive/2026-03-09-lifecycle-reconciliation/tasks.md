+++
id = "df004405-4394-4f2e-aded-437541e5fba3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle Reconciliation — Ambient Sync from Implementation Reality back to Design Tree + OpenSpec — Tasks

Dependencies:
- Group 1 establishes shared reconciliation helpers used by Groups 2 and 3.
- Group 2 depends on Group 1.
- Group 3 depends on Group 1.

## 1. Shared lifecycle reconciliation helpers
<!-- specs: lifecycle/reconciliation -->
<!-- skills: typescript -->
- [x] 1.1 Add an OpenSpec reconciliation module that can inspect change/task state and evaluate stale lifecycle conditions
- [x] 1.2 Represent reconciliation warnings with structured reasons and suggested actions so cleave and archive can share the same logic
- [x] 1.3 Add tests covering bound-node detection, incomplete-task stale state, and no-binding stale state

## 2. Post-cleave automatic reconciliation
<!-- specs: lifecycle/reconciliation -->
<!-- skills: typescript -->
- [x] 2.1 Replace direct task checkbox write-back with reconciliation-aware post-merge handling in cleave
- [x] 2.2 Surface a lifecycle reconciliation warning in the cleave report when completed work cannot be mapped back into tasks.md
- [x] 2.3 Ensure OpenSpec progress/status reflects reconciled checkbox counts after successful cleave completion
- [x] 2.4 Add tests for successful task reconciliation and stale mapping warnings

## 3. Archive guard and skill guidance
<!-- specs: lifecycle/reconciliation -->
<!-- skills: typescript -->
- [x] 3.1 Update OpenSpec archive flows (tool + command) to refuse obviously stale lifecycle state instead of silently archiving
- [x] 3.2 Require a bound design-tree node before archive succeeds so node ↔ change lifecycle stays traceable
- [x] 3.3 Update OpenSpec and cleave skill/tool guidance to describe reconciliation checkpoints as a required lifecycle phase
- [x] 3.4 Add tests for archive refusal paths and refreshed lifecycle guidance

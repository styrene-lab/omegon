# Post-Assess Reconciliation — Update OpenSpec and Design Tree after Review/Fix Cycles — Tasks

Dependencies:
- Group 1 establishes shared post-assess reconciliation helpers used by Groups 2 and 3.
- Group 2 depends on Group 1.
- Group 3 depends on Group 1.
- Group 4 depends on Groups 2 and 3.

## 1. Shared post-assess reconciliation model
<!-- specs: lifecycle/post-assess -->
<!-- skills: typescript -->
- [x] 1.1 Add a shared post-assess reconciliation module under `extensions/openspec/` that evaluates assessment outcomes and determines whether lifecycle state should reopen
- [x] 1.2 Represent reconciliation outcomes explicitly: preserve verifying, reopen implementing, append implementation-note deltas, and emit ambiguity warnings
- [x] 1.3 Add unit tests for pass/fail/partial assessment outcomes and ambiguous-review handling

## 2. OpenSpec lifecycle reopening after assessment
<!-- specs: lifecycle/post-assess -->
<!-- skills: typescript -->
- [x] 2.1 Wire `/assess spec` and `/assess cleave` flows to invoke post-assess reconciliation for OpenSpec-backed work
- [x] 2.2 Implement conservative task-state reopening so failed or partially-resolved assessment demotes lifecycle state from verifying back to implementing without semantic task rewriting
- [x] 2.3 Refresh OpenSpec dashboard state immediately after post-assess reconciliation
- [x] 2.4 Add tests proving assessment can reopen a verifying change and that passing assessment preserves verifying

## 3. Design-tree implementation-note deltas
<!-- specs: lifecycle/post-assess -->
<!-- skills: typescript -->
- [x] 3.1 Detect follow-up fix files that are outside the bound design-tree node’s existing file scope
- [x] 3.2 Append reconciliation-driven file scope deltas to implementation notes without deleting existing entries
- [x] 3.3 Append newly discovered constraints from post-assess reconciliation without removing prior constraints
- [x] 3.4 Add tests for implementation-note delta appends

## 4. Skill guidance and operator-facing warnings
<!-- specs: lifecycle/post-assess -->
<!-- skills: typescript -->
- [x] 4.1 Update `skills/openspec/SKILL.md` to document post-assess reconciliation as a required lifecycle checkpoint
- [x] 4.2 Update `skills/cleave/SKILL.md` to document that assessment can reopen implementation state
- [x] 4.3 Ensure ambiguous assessment results produce explicit warnings rather than silent no-ops or semantic task rewriting
- [x] 4.4 Add tests or assertions covering warning/report text where practical

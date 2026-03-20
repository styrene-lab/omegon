---
id: task-file-testing-section
title: Task file Testing Requirements section — structure and injection
status: implemented
parent: testing-directives-pipeline
open_questions: []
---

# Task file Testing Requirements section — structure and injection

## Overview

> Parent: [Testing directives pipeline — falsifiable testing paths from design through implementation](testing-directives-pipeline.md)
> Spawned from: "How should the task file Testing Requirements section be structured — per-function, per-requirement, or as a flat list?"

*To be explored.*

## Research

### Proposed task file Testing Requirements section

The section sits between the existing Contract and Finalization sections. It has three tiers:\n\n```markdown\n## Testing Requirements\n\n### Spec Scenarios (must pass)\nThese scenarios from the spec MUST have corresponding passing tests:\n- Resolve a secret from Vault → test_resolve_vault_secret\n- Vault unreachable returns None → test_vault_unreachable_returns_none\n- Vault sealed returns None → test_vault_sealed_returns_none\n\n### Edge Cases (must have tests)\nEach of these must have at least one test:\n- Empty path string → error, not panic\n- Path with trailing slash → normalized or rejected consistently\n- Response body is not valid JSON → error with context\n- Network timeout mid-response → clean error, no partial state\n- KV v2 response missing data.data field → descriptive error\n\n### Test Convention\n```rust\n// From guards.rs — follow this pattern\n#[test]\nfn block_vault_json() {\n    let guard = PathGuard::new();\n    let decision = guard.check(\"read\", &json!({\"path\": \"/home/user/.omegon/vault.json\"}));\n    assert!(decision.is_some());\n    assert!(decision.unwrap().is_block());\n}\n```\n```\n\nThe first tier comes from the OpenSpec spec scenarios (mapped to the child's scope). The second tier comes from spec edge cases + design falsifiability conditions + fast_forward generated edge cases. The third tier is the existing test example from context.rs.\n\nFor children that don't have an associated OpenSpec change (ad-hoc cleave), tiers 1 and 2 are empty and only the convention example appears — same as today. The improvement is only when the spec pipeline is active."

## Decisions

### Decision: Three-tier structure: spec scenarios (must pass) + edge cases (must have tests) + convention example

**Status:** decided
**Rationale:** Per-requirement grouping is too noisy — a child working on vault.rs doesn't need to see edge cases for the TUI command. Per-function is too granular and can't be determined before implementation. A flat list per child, filtered to scope, is the right level. Three tiers make the contract clear: tier 1 is non-negotiable acceptance criteria, tier 2 is required test coverage, tier 3 is how to write them. The child can see at a glance what's expected and has an example to follow.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — In build_task_file: replace one-liner test_convention with a full Testing Requirements section. When OpenSpec task file content includes testing directives (from enriched tasks.md), extract and format them. When no OpenSpec, fall back to convention example only.
- `core/crates/omegon/src/cleave/context.rs` (modified) — Add extract_testing_directives(task_content: &str) that parses Edge Cases and Spec Scenarios from enriched task file content. Returns a TestingDirectives struct with scenarios, edge_cases, convention_example.
- `extensions/openspec/index.ts` (modified) — In the task file enrichment (called from cleave_run with openspec_change_path), inject spec scenarios and edge cases into each child's task file as a structured Testing Requirements block, filtered to the child's scope.

### Constraints

- Testing Requirements section must stay under 1K tokens to avoid bloating the task file
- Spec scenarios are filtered to the child's scope by matching requirement titles against scope file paths
- Edge cases that don't match the child's scope are excluded
- When no OpenSpec change is active, the section degrades to convention example only — no empty tiers

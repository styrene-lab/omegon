# Task file Testing Requirements section — structure and injection — Design Spec (extracted)

> Auto-extracted from docs/task-file-testing-section.md at decide-time.

## Decisions

### Three-tier structure: spec scenarios (must pass) + edge cases (must have tests) + convention example (decided)

Per-requirement grouping is too noisy — a child working on vault.rs doesn't need to see edge cases for the TUI command. Per-function is too granular and can't be determined before implementation. A flat list per child, filtered to scope, is the right level. Three tiers make the contract clear: tier 1 is non-negotiable acceptance criteria, tier 2 is required test coverage, tier 3 is how to write them. The child can see at a glance what's expected and has an example to follow.

## Research Summary

### Proposed task file Testing Requirements section

The section sits between the existing Contract and Finalization sections. It has three tiers:\n\n```markdown\n## Testing Requirements\n\n### Spec Scenarios (must pass)\nThese scenarios from the spec MUST have corresponding passing tests:\n- Resolve a secret from Vault → test_resolve_vault_secret\n- Vault unreachable returns None → test_vault_unreachable_returns_none\n- Vault sealed returns None → test_vault_sealed_returns_none\n\n### Edge Cases (must have tests)\nEach of these must have at least…

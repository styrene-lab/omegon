---
id: spec-edge-case-format
title: Spec edge case format and authoring conventions
status: implemented
parent: testing-directives-pipeline
open_questions: []
---

# Spec edge case format and authoring conventions

## Overview

> Parent: [Testing directives pipeline — falsifiable testing paths from design through implementation](testing-directives-pipeline.md)
> Spawned from: "What format should edge cases take in spec files — full Given/When/Then scenarios, one-liner test descriptions, or a hybrid?"

*To be explored.*

## Decisions

### Decision: Hybrid: one-liner edge case descriptions under an #### Edge Cases heading, with optional Given/When/Then expansion for complex ones

**Status:** decided
**Rationale:** Full Given/When/Then for every edge case is too expensive during spec authoring — it discourages writing them. One-liners are fast to write and sufficient for the implementing agent to turn into tests. The format is: `- <input condition> → <expected behavior>`. For complex cases that need setup context, expand to abbreviated G/W/T. This keeps the spec readable while ensuring edge cases are captured. The implementing agent is expected to expand each one-liner into a full test function. Example: `- Empty path string → error, not panic` becomes `#[test] fn read_empty_path_returns_error() { ... }`.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `skills/openspec/SKILL.md` (modified) — Add Edge Cases section convention to spec format docs. Show hybrid format: one-liners for simple cases, abbreviated G/W/T for complex. Add example showing both.
- `extensions/openspec/spec.ts` (modified) — Parse #### Edge Cases sections in spec files. Add edgeCases: string[] to RequirementSpec type. Include in spec summary counts.

### Constraints

- Edge cases are associated with their parent requirement, not the spec as a whole
- One-liner format: '- <condition> → <expected behavior>'
- Parser must handle both one-liner and expanded G/W/T edge cases
- Edge case count should appear in /opsx:status output alongside scenario count

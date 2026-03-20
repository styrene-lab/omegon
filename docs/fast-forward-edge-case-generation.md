---
id: fast-forward-edge-case-generation
title: Fast-forward edge case generation and task enrichment
status: implemented
parent: testing-directives-pipeline
open_questions: []
---

# Fast-forward edge case generation and task enrichment

## Overview

> Parent: [Testing directives pipeline — falsifiable testing paths from design through implementation](testing-directives-pipeline.md)
> Spawned from: "How should the fast_forward LLM pass generate edge cases — from scenario analysis, function signature analysis, or both?"

*To be explored.*

## Decisions

### Decision: Scenario-driven edge case generation during fast_forward — analyze each requirement's scenarios to derive untested paths

**Status:** decided
**Rationale:** The scenarios already describe the system's behavior surface. Each scenario implies edge cases by inversion: if 'read returns data when path is allowed', then edge cases include 'read with empty path', 'read with path containing special characters', 'read when response is malformed'. This is a structured derivation, not open-ended generation. The LLM prompt during fast_forward receives the scenarios and asks: 'For each scenario, list 2-3 edge cases that are NOT covered by existing scenarios. Focus on: empty/null inputs, error responses, timeout/network failures, concurrency, and boundary values.' The results are appended to tasks.md as testing directives per task group.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/index.ts` (modified) — In the fast_forward case (line ~554): after generating task items from scenarios, add an LLM call to derive edge cases per requirement. Append '### Testing: Edge cases' subsection to each task group with generated one-liners. Also pull any existing #### Edge Cases from specs into the task group.
- `extensions/openspec/index.ts` (modified) — In the fast_forward task generation: replace the generic '- [ ] Write tests for <requirement>' with a structured testing block that includes spec scenarios as acceptance tests AND edge cases as required additional tests.

### Constraints

- LLM edge case generation is optional — if the LLM call fails, fall back to spec-authored edge cases only
- Edge case generation prompt should be focused: 2-3 per scenario, not open-ended
- Generated edge cases should be appended to tasks.md, not to the spec files (specs are operator-authored truth)
- The testing block in tasks.md should clearly distinguish spec scenarios (must pass) from edge cases (must have tests)

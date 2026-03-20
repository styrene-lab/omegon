# Fast-forward edge case generation and task enrichment — Design Spec (extracted)

> Auto-extracted from docs/fast-forward-edge-case-generation.md at decide-time.

## Decisions

### Scenario-driven edge case generation during fast_forward — analyze each requirement's scenarios to derive untested paths (decided)

The scenarios already describe the system's behavior surface. Each scenario implies edge cases by inversion: if 'read returns data when path is allowed', then edge cases include 'read with empty path', 'read with path containing special characters', 'read when response is malformed'. This is a structured derivation, not open-ended generation. The LLM prompt during fast_forward receives the scenarios and asks: 'For each scenario, list 2-3 edge cases that are NOT covered by existing scenarios. Focus on: empty/null inputs, error responses, timeout/network failures, concurrency, and boundary values.' The results are appended to tasks.md as testing directives per task group.

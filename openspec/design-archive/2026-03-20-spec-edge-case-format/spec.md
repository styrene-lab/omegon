# Spec edge case format and authoring conventions — Design Spec (extracted)

> Auto-extracted from docs/spec-edge-case-format.md at decide-time.

## Decisions

### Hybrid: one-liner edge case descriptions under an #### Edge Cases heading, with optional Given/When/Then expansion for complex ones (decided)

Full Given/When/Then for every edge case is too expensive during spec authoring — it discourages writing them. One-liners are fast to write and sufficient for the implementing agent to turn into tests. The format is: `- <input condition> → <expected behavior>`. For complex cases that need setup context, expand to abbreviated G/W/T. This keeps the spec readable while ensuring edge cases are captured. The implementing agent is expected to expand each one-liner into a full test function. Example: `- Empty path string → error, not panic` becomes `#[test] fn read_empty_path_returns_error() { ... }`.

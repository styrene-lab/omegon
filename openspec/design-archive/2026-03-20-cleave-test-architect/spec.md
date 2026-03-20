# Cleave test-architect phase and post-implementation coverage review — Design Spec (extracted)

> Auto-extracted from docs/cleave-test-architect.md at decide-time.

## Decisions

### Auto-inject test-architect as wave 0 when OpenSpec specs are present; coverage reviewer as deterministic post-merge check (decided)

The test-architect is only useful when there are specs to analyze — without specs there's nothing to derive test plans from. When openspec_change_path is set (which already enables spec-enriched task files), the orchestrator auto-injects a test-architect child as a synthetic wave 0 entry. All implementation children depend on it. The coverage reviewer is deterministic (no LLM) and runs as part of the merge phase reporting — it's a function call, not a child process. Together these add ~30 seconds of latency for a qualitative leap in test coverage guarantees.

## Research Summary

### The problem: self-grading

When the same agent both designs its test suite and writes the implementation, it's grading its own homework. Three failure modes:\n\n1. **Confirmation bias**: The agent writes code first, then writes tests that validate what it built — not what the spec requires. The vault-client child wrote 29 tests but missed `list()` entirely because it focused on what it had just implemented.\n\n2. **Scope reduction**: The agent reduces the testing scope to match its implementation budget. If it's running l…

### Proposed cleave wave restructuring

Current wave model:\n```\nWave 0: [independent children]\nWave 1: [dependent children]\nMerge\n(Optional: review loop per child, currently TS-side)\n```\n\nProposed model with test-architect and coverage gate:\n```\nWave 0: [test-architect]     — reads specs + design, produces test plans\nWave 1: [implementation children] — receive test plans as task context\nMerge\nPost-merge: [coverage reviewer] — checks tests against test plans\n```\n\n### Test-architect child\n- **Input**: OpenSpec specs (sc…

### Cost and latency analysis

**Test-architect child**: Reads specs + design (maybe 3K tokens), generates test plans for N children. Output is ~200-400 tokens per child. Total: one agent turn, maybe 2-3 tool calls (read spec, read design, write plans). At victory tier, this is ~5K input + 2K output = pennies. Wall time: 15-30 seconds.\n\n**Impact on cleave latency**: The test-architect runs as wave 0. If the plan already has an independent foundation child (e.g., vault-client), the test-architect runs in parallel with it. If…

---
id: cleave-test-architect
title: Cleave test-architect phase and post-implementation coverage review
status: implemented
parent: testing-directives-pipeline
dependencies: [spec-edge-case-format, task-file-testing-section]
open_questions: []
priority: 2
---

# Cleave test-architect phase and post-implementation coverage review

## Overview

> Parent: [Testing directives pipeline — falsifiable testing paths from design through implementation](testing-directives-pipeline.md)
> Spawned from: "Should the cleave wave model support a test-architect pre-phase that produces test plans before implementation children run, and a light adversarial review post-phase that checks coverage?"

*To be explored.*

## Research

### The problem: self-grading

When the same agent both designs its test suite and writes the implementation, it's grading its own homework. Three failure modes:\n\n1. **Confirmation bias**: The agent writes code first, then writes tests that validate what it built — not what the spec requires. The vault-client child wrote 29 tests but missed `list()` entirely because it focused on what it had just implemented.\n\n2. **Scope reduction**: The agent reduces the testing scope to match its implementation budget. If it's running low on turns, it writes fewer tests — not simpler code.\n\n3. **Blind spots**: The agent can't adversarially test its own blind spots. If it misunderstood the mockito API, its tests will use the same wrong API and pass (until integration).\n\nSeparation of concerns fixes all three: a different agent (or the same agent in a different role) designs the test plan before implementation starts. The implementing agent follows the plan, not its own judgment about what to test.

### Proposed cleave wave restructuring

Current wave model:\n```\nWave 0: [independent children]\nWave 1: [dependent children]\nMerge\n(Optional: review loop per child, currently TS-side)\n```\n\nProposed model with test-architect and coverage gate:\n```\nWave 0: [test-architect]     — reads specs + design, produces test plans\nWave 1: [implementation children] — receive test plans as task context\nMerge\nPost-merge: [coverage reviewer] — checks tests against test plans\n```\n\n### Test-architect child\n- **Input**: OpenSpec specs (scenarios + edge cases), design.md (constraints, decisions), scope breakdown from the plan\n- **Output**: One `<child-label>-tests.md` file per implementation child, containing:\n  - Required test functions (name, description, key assertions)\n  - Edge case test descriptions\n  - Mock/fixture setup notes (e.g., 'use mockito Server::new_async(), version 1.x')\n  - Expected test count per scope area\n- **Role**: Read-only analysis — no code written. Cheaper model tier (victory, not gloriana). Runs fast because it's pure text generation, no tool calls.\n- **Implementation**: A synthetic child injected by the orchestrator when `openspec_change_path` is set. Not in the user's plan_json — auto-inserted as wave 0.\n\n### How this flows to implementation children\nThe orchestrator reads `<child-label>-tests.md` from the workspace and injects it into the child's task file as the Testing Requirements section. The child sees concrete test function names and descriptions, not 'write tests'. It implements both the feature and the prescribed tests.\n\n### Coverage reviewer (post-merge)\nAfter merge, a lightweight pass (can be local model) reads:\n1. The test-architect's plans\n2. The merged code's actual test functions (parsed from source)\n3. Reports: which planned tests exist, which are missing, which are new (unplanned)\n\nThis is cheaper than the full adversarial review. It's a checklist comparison, not a code review. Output: 'Test plan coverage: 23/27 planned tests implemented. Missing: test_empty_path_error, test_timeout_mid_response, test_concurrent_reads, test_malformed_json_response.'\n\nThe operator decides whether to accept or request additional tests."

### Cost and latency analysis

**Test-architect child**: Reads specs + design (maybe 3K tokens), generates test plans for N children. Output is ~200-400 tokens per child. Total: one agent turn, maybe 2-3 tool calls (read spec, read design, write plans). At victory tier, this is ~5K input + 2K output = pennies. Wall time: 15-30 seconds.\n\n**Impact on cleave latency**: The test-architect runs as wave 0. If the plan already has an independent foundation child (e.g., vault-client), the test-architect runs in parallel with it. If not, it adds one short wave before implementation. Net cost: 15-30 seconds of latency, significant improvement in test coverage.\n\n**Coverage reviewer**: Post-merge, reads test plans + source files. No LLM needed — this is a text matching exercise. Parse test plans for function names, grep merged source for those function names, report delta. Pure Rust, runs in <1 second.\n\n**Comparison to current review**: The existing adversarial review (review.ts) spawns a gloriana-tier agent per child, adds 2-5 minutes per child, and focuses on code quality broadly. The coverage reviewer is orthogonal — it checks specifically whether the test plan was followed. It's faster, cheaper, and answers a precise question.

## Decisions

### Decision: Auto-inject test-architect as wave 0 when OpenSpec specs are present; coverage reviewer as deterministic post-merge check

**Status:** decided
**Rationale:** The test-architect is only useful when there are specs to analyze — without specs there's nothing to derive test plans from. When openspec_change_path is set (which already enables spec-enriched task files), the orchestrator auto-injects a test-architect child as a synthetic wave 0 entry. All implementation children depend on it. The coverage reviewer is deterministic (no LLM) and runs as part of the merge phase reporting — it's a function call, not a child process. Together these add ~30 seconds of latency for a qualitative leap in test coverage guarantees.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — In run_cleave: when openspec_change_path is present, inject a synthetic test-architect ChildPlan at the front of the plan. It depends on nothing; all other children depend on it. Its description includes the spec content and instructs it to write test plan files. After it completes, read its output files and inject them into subsequent children's task files.
- `core/crates/omegon/src/cleave/test_architect.rs` (new) — Test architect module. Functions: build_test_architect_plan(specs, design, children) -> ChildPlan. build_test_architect_prompt(specs, design, child_scopes) -> String. parse_test_plans(workspace_path) -> HashMap<String, TestPlan>. TestPlan struct: required_tests: Vec<TestDescription>, edge_cases: Vec<String>.
- `core/crates/omegon/src/cleave/coverage.rs` (new) — Post-merge coverage check. Functions: check_test_coverage(repo_path, test_plans) -> CoverageReport. Parses Rust/TS/Python source for test function names, matches against test plan entries, reports coverage percentage and missing tests.
- `core/crates/omegon/src/cleave/mod.rs` (modified) — Add test_architect and coverage modules
- `core/crates/omegon/src/cleave/progress.rs` (modified) — Add TestArchitectComplete and CoverageReport progress events

### Constraints

- Test-architect child only injected when openspec_change_path is set — ad-hoc cleaves skip it
- Test-architect runs at victory tier, not gloriana — it's analysis, not deep reasoning
- Test plan files written to workspace as <child-label>-tests.md, one per implementation child
- Coverage reviewer is deterministic (no LLM) — text parsing and matching only
- Coverage report appears in the cleave report alongside merge results
- Test-architect's max_turns should be low (5-10) since it only reads and writes
- Implementation children must not be able to modify the test plan files — they're read-only input

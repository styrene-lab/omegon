---
id: testing-directives-pipeline
title: Testing directives pipeline — falsifiable testing paths from design through implementation
status: implemented
tags: [testing, cleave, openspec, dx]
open_questions: []
issue_type: epic
priority: 2
---

# Testing directives pipeline — falsifiable testing paths from design through implementation

## Overview

The current design-to-implementation pipeline produces insufficient test coverage. Tests are an afterthought — a one-line 'write tests' in task files. The fix is a structured testing layer that flows from design nodes through OpenSpec specs into cleave task files, carrying falsifiable edge-case paths and pre-implementation research notes that give implementing agents a higher success rate.

## Research

### Current pipeline audit — where testing context drops off

**Stage 1: Design tree nodes** — `acceptanceCriteria` has three sub-fields: `scenarios`, `falsifiability`, `constraints`. These exist in the schema but are almost always empty. Of 166 nodes, the vast majority have no acceptance criteria at all. The `/assess design` command (design-assess-command node, decided) is designed to evaluate these but the sections need content first.\n\n**Stage 2: OpenSpec specs** — Given/When/Then scenarios exist. The vault-client spec has 15 scenarios. But they're overwhelmingly happy-path. The vault spec has 3 error scenarios (unreachable, sealed, disallowed path) vs 12 happy paths. No boundary conditions. No concurrency tests. No malformed input tests.\n\n**Stage 3: fast_forward (index.ts line 554-568)** — This is the critical drop-off. The auto-generated tasks.md creates one task per scenario PLUS a generic `Write tests for <requirement>` task appended to each group. That's it. No test specifications. No edge cases. No error paths beyond what the scenarios already cover. The 'Write tests' task gives the implementing agent zero guidance about WHAT to test.\n\n**Stage 4: Cleave task file (orchestrator.rs line 694-699)** — A one-liner: 'Write tests as #[test] functions in the same file or a tests submodule'. No specifics. No edge cases. No coverage requirements.\n\n**Stage 5: Cleave context.rs** — Samples one existing test from the crate. Better than nothing, but it's a format example, not a testing directive.\n\n**Net result**: The implementing agent gets told WHAT to build (from scenarios) and HOW tests look (from convention sampling), but never WHAT TO TEST BEYOND THE HAPPY PATH. Edge cases, error paths, boundary conditions, and regression scenarios are entirely left to the model's training data."

### Intervention points — where to inject testing directives

Three intervention points, each reinforcing the next:\n\n### 1. Design node: `acceptanceCriteria.falsifiability` (already exists, needs population)\nThe design tree schema already has `falsifiability` as an array of strings. These are conditions that SHOULD fail — edge cases, error paths, boundary conditions. Example for the vault client:\n- 'Vault returns HTTP 429 (rate limited) on read — client handles gracefully'\n- 'Allowed path glob with special chars (brackets, question marks) doesn't cause regex injection'\n- 'Token with 0 TTL remaining is treated as expired, not valid'\n- 'KV v2 response with missing data.data field doesn't panic'\n\nThese are falsifiable: you can write a test that either passes or fails. They flow naturally from design research.\n\n### 2. OpenSpec spec: new `#### Edge Cases` section per requirement\nAfter the happy-path scenarios, add an `#### Edge Cases` section with terse scenario outlines. Not full Given/When/Then — just one-liners that the implementing agent must turn into tests:\n```\n#### Edge Cases\n- Empty path string → error, not panic\n- Path with trailing slash normalized\n- Response body is not valid JSON → error with context\n- Network timeout mid-response → clean error, no partial state\n- Concurrent reads to same path → no data races\n```\n\nThese are cheaper to write than full scenarios but provide the critical 'what else to test' signal.\n\n### 3. Cleave task file: explicit `## Testing Requirements` section\nThe task file should have a dedicated section between Contract and Result that lists:\n- The spec scenarios this child must satisfy (extracted from specs)\n- The edge cases this child must cover (from spec edge cases + design falsifiability)\n- The test convention example (from context.rs, already exists)\n- A minimum coverage heuristic: 'At least N tests per public function, including 1+ error path'\n\nThis replaces the anemic 'Write tests as #[test]' one-liner with an actual testing contract."

### Pre-implementation research notes — the other half of the problem

The user's second insight: the task file should also carry 'research/discovery/notes for the implementation before it ever starts'. This is distinct from the context.rs dependency versions and test examples — it's domain-specific knowledge that increases first-attempt success:\n\n1. **API behavior notes**: 'Vault returns 503 when sealed, not 200 with sealed=true in body — check status code first'. 'mockito 1.x Server is async, create with Server::new_async().await'.\n\n2. **Known gotchas**: 'This crate already has a resolve_secret function — wire into it, don't create a parallel path'. 'The TUI mod.rs match arms fall through to bus commands — add your match before the _ arm'.\n\n3. **Architecture constraints**: 'SecretsManager holds VaultClient behind tokio::sync::Mutex — use async access'. 'The on_event handler must return Vec<BusRequest> synchronously — no .await allowed'.\n\nThese notes live in the design node's `implementationNotes` (which already has `constraints`) and should flow into the task file alongside the testing directives. The design node already captures this; the gap is that `build_task_file` doesn't read the OpenSpec design.md and inject relevant sections.\n\nThe cleave tool already supports `openspec_change_path` — when set, it writes enriched task files. The enrichment currently includes design.md context. The improvement is to also extract and format the testing directives from specs."

## Decisions

### Decision: Layered enrichment at each stage — each stage adds what it knows best, downstream stages inherit

**Status:** decided
**Rationale:** The design node knows architectural edge cases (race conditions, state machine boundaries). The spec knows behavioral edge cases (malformed input, missing fields). The task file knows scope-specific edge cases (which functions need error-path tests). Each layer adds what it can see. The downstream consumer inherits everything above it. This avoids a single monolithic 'generate all edge cases' pass and keeps each artifact's concerns local. Implementation: design nodes populate falsifiability, specs add edge case sections, fast_forward extracts both into task descriptions, build_task_file formats them into a Testing Requirements section.

### Decision: Edge cases authored during spec writing with LLM assist during fast_forward — not deferred to implementation

**Status:** decided
**Rationale:** If edge cases are deferred to the implementing agent, they get forgotten — that's the current failure mode. Edge cases must exist BEFORE implementation starts. The spec author (operator or agent in the parent session) writes the initial edge cases alongside scenarios during /opsx:spec. Then fast_forward can augment with LLM-generated edge cases based on the spec content (e.g., 'you have a read() function — what happens on timeout? empty response? 403?'). This means edge cases are a first-class part of the spec, not an afterthought.

### Decision: Testing directives apply to both cleave and direct — same spec artifacts, different delivery mechanism

**Status:** decided
**Rationale:** The edge cases and testing requirements live in the spec and design artifacts, which are read by both cleave task file generation and the in-session agent. For cleave, they're injected into the task file. For direct implementation, they're injected via OpenSpec context injection (which already exists — the focused design node and active openspec changes appear in context). The data is the same; only the delivery channel differs.

## Open Questions

*No open questions.*

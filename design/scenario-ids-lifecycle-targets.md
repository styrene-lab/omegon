+++
kind = "design_node"

[data]
title = "Scenario IDs as Lifecycle Join Keys"
status = "exploring"
issue_type = "architecture"
priority = 1
dependencies = ["8d214819-082b-4742-8b4b-bcca1c528a9c"]
open_questions = [
  "[assumption] OpenSpec scenarios should become addressable lifecycle objects rather than prose-only acceptance criteria.",
  "Should scenario IDs be globally unique across the repository or scoped by OpenSpec domain/change?",
  "Which scenario metadata fields belong in core versus extension-owned evidence providers?",
  "What compatibility policy should apply when a scenario title changes but no explicit id exists?",
  "Should archive gates enforce scenario evidence by default, per-change opt-in, or per-scenario metadata?"
]
+++

## Overview

Scenario IDs should become Omegon's stable join key across lifecycle artifacts once the TDD savepoint kernel is extracted into an extension. The TDD extension proves the value of IDs by attaching deterministic red→green evidence, but the ID system itself belongs in core because many non-TDD providers can attach evidence to the same scenario.

A scenario ID turns OpenSpec prose into an addressable lifecycle object:

```markdown
#### Scenario: Expired token rejected
<!-- id: auth/token-expired -->
Given a user has an expired token
When they request a protected resource
Then the response is 401
```

The ID `auth/token-expired` can then be referenced by tasks, evidence, cleave children, commits, archive gates, waivers, risk metadata, generated docs, and regression history.

## Research

The TDD savepoint prototype added explicit scenario ID parsing plus derived fallback IDs. Derived IDs are useful for compatibility, but explicit IDs are the durable model because titles and requirement headings are mutable.

TDD savepoints are only the first evidence provider. Other likely providers include test runner pass/fail, coverage, contract tests, security review, manual QA, documentation evidence, external issue/PR linkage, and runtime incident/regression linkage.

## Decisions

### Decision: Keep scenario identity in core

**Status:** accepted

Scenario IDs are not TDD-specific. Core OpenSpec parsing should own scenario identity, explicit metadata comment parsing, derived fallback IDs, and generic scenario evidence attachment points. TDD savepoints should become an extension-owned evidence provider.

### Decision: Prefer explicit IDs over derived IDs

**Status:** accepted

Derived IDs from domain/requirement/scenario title are compatibility fallback only. Durable scenarios should use explicit metadata comments like `<!-- id: auth/token-expired -->`.

### Decision: Treat evidence as provider-owned but scenario-addressed

**Status:** proposed

Core should eventually represent scenario evidence generically, e.g. provider/kind/status/event_id/created_at, while extensions own raw evidence and provider-specific interpretation.

## Possible Targets

### 1. Formal scenario metadata parser

Support structured HTML comments near scenario headings:

```markdown
<!-- id: auth/token-expired -->
<!-- risk: high -->
<!-- depends: auth/valid-token, auth/token-parser -->
<!-- tags: security, auth -->
<!-- issue: GH-123 -->
```

Target core type shape:

```rust
Scenario {
    id: String,
    metadata: ScenarioMetadata,
}
```

### 2. Generic scenario evidence registry

Define a provider-neutral evidence summary:

```json
{
  "scenario": "auth/token-expired",
  "provider": "tdd-savepoint",
  "kind": "red-green",
  "status": "tdd-pass",
  "event_id": "redgreen-...",
  "created_at": "..."
}
```

Core reads projected summaries; providers keep raw logs.

### 3. Scenario-addressable commands

Potential CLI/API surface:

```sh
omegon opsx scenario auth/token-expired status
omegon opsx scenario auth/token-expired evidence
omegon opsx scenario auth/token-expired waive
omegon opsx scenario auth/token-expired assign
```

### 4. Scenario-level task planning

Allow tasks to reference scenario IDs directly:

```markdown
## 2. Token rejection behavior
<!-- scenarios: auth/token-expired, auth/token-missing -->

- [ ] 2.1 Add failing tests
- [ ] 2.2 Implement validation
- [ ] 2.3 Capture red→green evidence
```

### 5. Cleave decomposition by scenario set

Cleave children can receive precise acceptance criteria:

```json
{
  "label": "token-errors",
  "scenarios": ["auth/token-expired", "auth/token-missing"]
}
```

Harvest can require evidence for assigned scenario IDs before marking a child complete.

### 6. Scenario coverage and evidence reports

Report lifecycle status by scenario:

```text
Scenario                         Test      TDD       Coverage   Status
auth/token-expired               pass      tdd-pass  covered    ready
auth/token-missing               pass      no-red    covered    review
auth/malformed-token             fail      red       covered    blocked
```

### 7. Scenario-level archive gates

Archive policy can be scenario-specific:

```yaml
archive_policy:
  required_evidence:
    auth/token-expired:
      - tdd-savepoint:tdd-pass
      - test-runner:pass
    auth/rbac-denied:
      - security-review:approved
      - test-runner:pass
```

Initial policy should be report-only; enforcement should be opt-in.

### 8. Waivers with accountability

Waivers should target scenario/provider pairs, not entire changes:

```json
{
  "scenario": "billing/stripe-webhook-replay",
  "provider": "tdd-savepoint",
  "reason": "requires live Stripe replay fixture not available locally",
  "approved_by": "operator",
  "expires": "2026-06-30"
}
```

### 9. Commit trailers and git history

Commits can reference scenario IDs:

```text
OpenSpec-Scenario: auth/token-expired
TDD-Event: redgreen-abc123
```

This makes behavior history queryable through git.

### 10. Regression and release history

Scenario IDs make it possible to track behavior across releases:

```text
auth/token-expired
  introduced: 0.26.0
  failed in: 0.28.1
  fixed in: 0.28.2
  latest evidence: tdd-pass
```

### 11. Documentation generation

Docs can cite scenarios by ID and detect drift when scenario content changes.

### 12. Runtime incident linkage

Runtime incidents can reference impacted scenario IDs, making regression triage lifecycle-aware.


## Hard Seam: What Is a Scenario?

A scenario is the smallest durable behavioral contract in OpenSpec. It is not a test, task, implementation slice, or evidence record. It is the lifecycle object that says: given this precondition, when this action occurs, then this observable outcome must hold.

The scenario seam is:

```text
Scenario = stable identity + behavioral contract + metadata + evidence attachment point
```

### Scenario owns

- stable ID (`<!-- id: ... -->` preferred)
- human-readable title
- Given/When/Then behavioral text
- scenario-local metadata such as risk, tags, dependencies, and external references
- links to provider-neutral evidence summaries

### Scenario does not own

- test runner implementation
- TDD savepoint raw logs
- command execution policy
- task decomposition state
- implementation file ownership
- archive policy itself

Those belong to adjacent systems that reference the scenario by ID.

### Boundary examples

A scenario should be phrased as an observable behavior:

```markdown
#### Scenario: Expired token rejected
<!-- id: auth/token-expired -->
Given a user has an expired token
When they request a protected resource
Then the response is 401
And the body contains `token_expired`
```

The following are not scenarios:

- `Add JwtValidator struct` — implementation task
- `Write pytest for expired tokens` — test task
- `cargo test auth_expired` — command/evidence provider detail
- `JWT auth feature` — requirement/change/feature scope
- `Fix auth bug` — task/change scope

### Scenario identity rule

Scenario IDs should be stable across wording edits. If behavior remains the same, keep the ID. If behavior materially changes, either update the scenario body and mark evidence stale, or create a new scenario ID when the old behavior no longer represents the same contract.

### Scenario evidence rule

Evidence attaches to scenarios; it does not define them. A TDD red→green event, coverage report, manual QA approval, or security review proves or qualifies a scenario, but the scenario remains the behavioral contract.

### Scenario task rule

Tasks implement, verify, or document scenarios. A task can own one or more scenarios; a scenario can require several tasks. Task completion is not scenario verification unless required evidence exists.

## Implementation Notes

### Core-owned scope

- explicit scenario ID parsing
- derived fallback IDs
- scenario metadata comments
- generic evidence summary read model
- scenario evidence/status reporting
- optional archive/report policy hooks

### Extension-owned scope

- TDD savepoint watcher
- command hashing and red→green detection
- raw provider logs
- provider-specific summaries
- provider-specific query/status commands

### Migration path from current prototype

1. Keep `Scenario.id` in core.
2. Replace TDD-specific scenario fields with generic evidence summaries before merge.
3. Move `tdd.rs` watcher/evidence provider into an extension.
4. Have the extension project evidence into an OpenSpec evidence subdirectory under the change.
5. Have core read generic evidence summaries from each change's evidence JSONL files.

## Open Questions

- Should explicit IDs be required for archiveable scenarios, or only recommended?
- Should IDs be globally unique, domain-scoped, or change-scoped?
- Should ID collision be a hard parse error or lifecycle doctor warning?
- How should scenario renames be represented when an explicit ID remains stable?
- What is the minimum generic evidence schema for cross-extension compatibility?

+++
kind = "design_node"

[data]
title = "Scenario Projection Implementation Plan"
status = "decided"
issue_type = "feature"
priority = 1
dependencies = ["scenario-projection-external-tracking", "scenario-ids-lifecycle-targets"]
open_questions = [
  "Should Phase 1 land before or after the TDD savepoint extension extraction branch is split?",
  "Should generic evidence summaries replace tdd_evidence immediately, or support both through a compatibility window?",
  "Should projection registry validation live in lifecycle_doctor or a new opsx scenario command?"
]
+++

## Overview

Implementation plan for turning scenario IDs into a formal projection/evidence layer that can support external project tracking, test-condition modeling, and provider-neutral lifecycle status.

This plan intentionally separates data-model work from provider integrations. First build a file-backed projection/evidence model. Then connect providers such as TDD savepoint, GitHub issues, TestRail-like test cases, coverage, and security review.

## Phase 0 — Cleanup current prototype boundary

### Goals

- Stop growing TDD-specific core code.
- Preserve useful scenario ID parsing.
- Prepare for extension extraction.

### Tasks

- Keep `Scenario.id` in core.
- Keep explicit `<!-- id: ... -->` parsing.
- Keep derived fallback IDs.
- Replace or supplement `Scenario.tdd_evidence` with generic evidence summaries.
- Move TDD-specific evidence classifications toward extension/provider ownership.

### Acceptance

- Core can represent scenario IDs without importing TDD-specific concepts.
- Existing TDD prototype tests still pass or are migrated to extension tests.

## Phase 1 — Core scenario metadata

### Goals

Formalize low-churn scenario metadata in OpenSpec.

### Metadata comments

```markdown
<!-- id: auth/token-expired -->
<!-- risk: high -->
<!-- tags: auth, security -->
<!-- depends: auth/valid-token, auth/token-parser -->
<!-- issue: GH-123 -->
```

### Types

```rust
struct ScenarioMetadata {
    id: String,
    risk: Option<String>,
    tags: Vec<String>,
    depends: Vec<String>,
    external_refs: Vec<ExternalRef>,
}
```

### Tasks

- Extend scenario parser to capture metadata comments before Given/When/Then.
- Add duplicate ID detection within a change.
- Add lifecycle doctor warning for missing explicit IDs on archiveable scenarios.
- Add tests for explicit ID, tags, risk, depends, and malformed metadata.

### Acceptance

- Scenario metadata survives parsing and appears in read model.
- Derived IDs remain fallback only.
- Duplicate IDs are visible as validation warnings.

## Phase 2 — Generic evidence summaries

### Goals

Core reads provider-neutral evidence, not TDD-specific status.

### Schema

```json
{
  "schema": "scenario-evidence/v1",
  "provider": "tdd-savepoint",
  "kind": "red-green",
  "status": "tdd-pass",
  "scenario": "auth/token-expired",
  "change": "jwt-auth",
  "task": "2.1",
  "event_id": "redgreen-...",
  "created_at": "2026-05-30T00:00:00Z"
}
```

### Types

```rust
struct ScenarioEvidenceSummary {
    schema: String,
    provider: String,
    kind: String,
    status: String,
    scenario: String,
    change: Option<String>,
    task: Option<String>,
    event_id: Option<String>,
    created_at: Option<String>,
    extra: serde_json::Value,
}
```

### Tasks

- Add evidence summary parser for `openspec/changes/<change>/evidence/*.jsonl`.
- Attach summaries to scenarios by ID.
- Ignore malformed lines with warnings, not hard failure.
- Preserve unknown fields in `extra`.
- Update lifecycle context to summarize provider/status counts.

### Acceptance

- Multiple evidence providers can attach to one scenario.
- TDD savepoint evidence is represented generically.
- No core enum is required for provider-specific statuses.

## Phase 3 — Scenario projection registry

### Goals

Represent external tracker/test-case mappings and required conditions.

### File

```text
openspec/changes/<change>/scenario-projections.yaml
```

### Schema

```yaml
schema: omegon-scenario-projections/v1

scenarios:
  auth/token-expired:
    projections:
      - id: github-issue
        system: github
        kind: issue
        role: coordination
        external_id: styrene/omegon#123
        sync: pull
    conditions:
      - provider: tdd-savepoint
        kind: red-green
        required_status: tdd-pass
        required: true
```

### Tasks

- Add YAML parser.
- Validate scenario IDs exist.
- Validate projection IDs are unique per scenario.
- Validate sync mode is one of pull/push/bidirectional, but warn that bidirectional is unsupported in v1.
- Validate condition provider/kind/status fields are non-empty.
- Add scenario projection summary to read model.

### Acceptance

- A change can define external issue/test-case projections without mutating external systems.
- Lifecycle status can report projection drift and missing conditions.

## Phase 4 — Scenario readiness aggregation

### Goals

Compute scenario readiness from conditions, evidence, and waivers.

### Status model

```text
ready
missing-evidence
stale-evidence
waived
blocked
unknown
```

### Tasks

- Match conditions against evidence summaries.
- Apply non-expired waivers.
- Report missing required evidence by scenario.
- Add lifecycle context summary.
- Keep archive gates report-only by default.

### Acceptance

- Operator can see which scenarios are ready and why.
- Missing evidence is explained in terms of provider/kind/status.
- Waivers are scoped to scenario/provider/kind.

## Phase 5 — TDD savepoint extension projection

### Goals

Move TDD evidence provider out of core.

### Tasks

- Create `extensions/omegon-tdd-savepoint`.
- Implement `tdd_savepoint_plan`, `run`, `evidence`, `presets`, `status`.
- Write raw logs under extension-owned path.
- Project `scenario-evidence/v1` summaries under change evidence directory.
- Remove core `omegon tdd` CLI or hide it behind prototype/deprecated flag.

### Acceptance

- Core reads TDD evidence without importing extension code.
- Extension can be installed/disabled independently.
- Scenario evidence still appears in lifecycle context.

## Phase 6 — Forge issue projection provider

### Goals

Create external issue projection flow.

### Tasks

- Use existing engagement/forge infrastructure where possible.
- Implement dry-run projection planning.
- Generate issue title/body with hidden scenario markers.
- Store created external IDs in projection registry.
- Start with push-only or pull-only, not bidirectional.

### Acceptance

- Scenario can produce a planned GitHub/forge issue representation.
- Created issue can be linked back to scenario ID.
- Closing issue does not automatically verify scenario.

## Phase 7 — Test management projection provider

### Goals

Map scenarios to test-case systems or file-export equivalents.

### Tasks

- Define generic provider contract.
- Start with JSON/CSV export if real provider credentials are absent.
- Map Given/When/Then to test steps.
- Map risk/tags to priority/labels.

### Acceptance

- Scenarios can export as test cases without making test case canonical.

## Phase 8 — Archive policy integration

### Goals

Use scenario readiness for optional gates.

### Tasks

- Add per-change archive policy metadata.
- Support required providers/statuses by scenario.
- Support waivers with expiration.
- Default remains report-only.
- Strict mode blocks archive on unmet required conditions.

### Acceptance

- Archive reports scenario readiness.
- Strict mode only applies when explicitly opted in.
- Waivers are visible and scoped.

## Sequencing Recommendation

1. Phase 1 + 2: generic metadata and evidence read model.
2. Phase 5: extract TDD savepoint extension onto generic evidence.
3. Phase 3 + 4: projection registry and readiness aggregation.
4. Phase 6/7: external providers.
5. Phase 8: opt-in archive gates.

Do not implement external sync before generic evidence/readiness is stable.

## Risks

- Projection registry could become another source of truth if it stores behavior text. Mitigation: it stores references and external IDs only.
- Bidirectional sync could create hidden lifecycle mutation. Mitigation: defer and require field ownership.
- Conditions could become too strict for exploratory work. Mitigation: report-only default and opt-in gates.
- Evidence provider schemas could drift. Mitigation: provider-neutral `scenario-evidence/v1` with unknown-field preservation.

## Acceptance for the Full Initiative

- A scenario can be identified, projected, evidenced, and assessed without conflating it with tasks/issues/tests.
- Core can report scenario readiness from generic evidence.
- Extensions can provide evidence independently.
- External trackers can represent scenarios without becoming canonical.

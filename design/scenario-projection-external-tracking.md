+++
kind = "design_node"

[data]
title = "Scenario Projection and External Tracking Model"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = ["scenario-ids-lifecycle-targets", "8d214819-082b-4742-8b4b-bcca1c528a9c"]
open_questions = [
  "Which external tracker provider should be implemented first: GitHub/Git forge issues, TestRail-style test cases, or an internal file-only projection registry?",
  "Should bidirectional sync be disallowed in v1 until field ownership and conflict semantics are proven?",
  "Should scenario projection registries live under openspec/changes/<change>/ or a repo-level openspec/scenarios registry after archive?",
  "What waiver approval identity is sufficient for local-only workflows versus team workflows?"
]
+++

## Overview

Scenario IDs make OpenSpec scenarios addressable lifecycle objects. The next seam is externalization: a scenario may need to appear in project trackers, test management systems, CI evidence, release gates, and issue/incident workflows. The correct model is projection, not replacement.

A scenario remains the canonical behavioral contract. External project-tracking objects, test cases, checks, waivers, and evidence records are projections or attachments that reference the scenario by ID.

```text
Scenario       = what must be true
Projection     = where/how that contract is represented externally
Evidence       = what proves or qualifies the scenario
Condition      = what evidence/status is required for readiness
External item  = tracker-native object created/read/synced from projection
```

## Hard Distinctions

### Scenario

A durable behavioral contract:

```text
Given precondition
When action
Then observable outcome
```

### Project-tracking item

A coordination object:

```text
owner, priority, discussion, labels, milestone, status, assignment
```

Examples: GitHub Issue, Linear issue, Jira ticket, Flynt task.

### Test condition

A verification requirement attached to a scenario:

```text
provider X must report status Y before scenario is ready
```

Examples: `tdd-savepoint:tdd-pass`, `test-runner:pass`, `security-review:approved`.

### Evidence

A provider-produced fact that supports or qualifies a scenario:

```text
redgreen-abc123 proves tdd-savepoint:tdd-pass for auth/token-expired
```

## Decisions

### Decision: Do not collapse scenarios into issues

**Status:** decided

Scenarios can project into issues, but an issue is not the scenario. Issue lifecycle state does not equal behavioral verification state.

### Decision: Do not collapse scenarios into test cases

**Status:** decided

Scenarios can project into test cases or test conditions, but a scenario is broader than any one verification mechanism. Manual QA, security review, contract tests, TDD savepoints, and coverage can all attach to the same scenario.

### Decision: Add a scenario projection layer

**Status:** decided

The formal externalization layer is a projection registry that maps scenario IDs to external tracker/test/evidence objects and required readiness conditions.

### Decision: Start report-only, not bidirectional sync

**Status:** decided

V1 should support file-backed projections and pull/push declarations, but real bidirectional sync should be deferred until field ownership and conflict handling are explicit.

## Projection Roles

| Role | Meaning | Examples |
|---|---|---|
| `coordination` | Human work tracking | GitHub Issue, Linear issue, Jira ticket, Flynt task |
| `verification` | Test-case or QA representation | TestRail case, Xray test, Zephyr test |
| `evidence` | Provider-produced proof | CI check, TDD event, coverage report |
| `gate` | Required condition for readiness/archive | tdd-pass required, security review approved |
| `documentation` | Generated/reference docs | scenario docs page, API behavior docs |
| `incident` | Runtime/regression linkage | incident ticket, regression report |

## Proposed Projection Schema

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
      - id: testrail-case
        system: testrail
        kind: test_case
        role: verification
        external_id: C456
        sync: push

    conditions:
      - provider: tdd-savepoint
        kind: red-green
        required_status: tdd-pass
        required: true
      - provider: test-runner
        kind: automated-test
        required_status: pass
        required: true
      - provider: security-review
        kind: review
        required_status: approved
        required: false

    waivers:
      - provider: tdd-savepoint
        kind: red-green
        reason: requires external identity provider fixture
        approved_by: operator
        expires: 2026-06-30
```

## Storage Model

### Scenario spec files

Store low-churn identity and behavior:

```markdown
#### Scenario: Expired token rejected
<!-- id: auth/token-expired -->
<!-- risk: high -->
<!-- tags: auth, security -->
Given ...
```

### Change-local projection registry

```text
openspec/changes/<change>/scenario-projections.yaml
```

Use for projections and conditions during active work.

### Projected evidence files

```text
openspec/changes/<change>/evidence/*.jsonl
```

Provider-owned summaries using `scenario-evidence/v1`.

### Repo-level index/cache

```text
.omegon/scenarios/index.json
```

Generated, not authoritative. Used for fast lookup, dashboards, and external sync scans.

## External Tracker Mapping

### GitHub Issues / Forge issues

Projection role: `coordination`.

Issue body should include stable hidden markers:

```markdown
<!-- omegon-scenario-id: auth/token-expired -->
<!-- omegon-change: jwt-auth -->

## Scenario

Given ...
When ...
Then ...

## Evidence

- tdd-savepoint: pending
- test-runner: pending
```

Do not treat issue close/open as scenario verified unless a policy explicitly maps it to a condition.

### Test management systems

Projection role: `verification`.

Scenario maps to a test case. Given/When/Then can map to test steps; risk/tags map to priority/labels/custom fields. Test case pass/fail is evidence, not source of scenario truth.

### CI systems

Projection role: `evidence`.

CI check runs can publish `scenario-evidence/v1` summaries by scenario ID.

## Sync Semantics

### Pull

External system is observed; Omegon does not mutate it.

### Push

Omegon creates/updates the external projection.

### Bidirectional

Deferred for v1. Requires field-level ownership, conflict detection, and explicit operator resolution.

Example future field ownership:

```yaml
fields:
  title: openspec
  labels: omegon
  assignee: external
  status: external
  evidence: omegon
```

## Implementation Plan

### Phase 1 — Core schema and parser

- Add `ScenarioMetadata` for id/risk/tags/depends/external refs.
- Add generic `ScenarioEvidenceSummary` and `ScenarioCondition` core types.
- Keep explicit scenario IDs in spec comments.
- Parse low-churn metadata comments near scenario headings.

### Phase 2 — Projection registry parser

- Add parser for `openspec/changes/<change>/scenario-projections.yaml`.
- Validate referenced scenario IDs exist in the change/baseline read model.
- Warn on unknown scenarios, duplicate projection IDs, unsupported sync modes, and expired waivers.

### Phase 3 — Generic evidence ingestion

- Read `openspec/changes/<change>/evidence/*.jsonl`.
- Parse `scenario-evidence/v1` summaries.
- Attach matching evidence to `Scenario.evidence` by scenario ID.
- Preserve provider-specific unknown fields for forward compatibility.

### Phase 4 — Scenario status aggregation

- Compute scenario readiness from conditions + evidence + waivers.
- Expose statuses in lifecycle context and status dashboards.
- Keep archive enforcement report-only initially.

### Phase 5 — Forge issue projection provider

- Use existing engagement/forge concepts where possible.
- Support dry-run plan: show issue title/body/labels before mutation.
- Start with push-only or pull-only, not bidirectional.
- Store external IDs in `scenario-projections.yaml`.

### Phase 6 — Test management projection provider

- Define provider trait/contract for TestRail/Xray/Zephyr-like systems.
- Start with file-only mock provider or dry-run export.
- Avoid product-specific assumptions in core.

### Phase 7 — Policy and gates

- Add opt-in per-change/per-scenario archive policy.
- Block only when policy explicitly requires it.
- Support scenario/provider waivers with expiration.

## Acceptance Criteria

- A scenario can be projected to an external issue without making the issue canonical.
- A scenario can declare required verification conditions independent of tracker state.
- Evidence from multiple providers can attach to the same scenario ID.
- Lifecycle status can explain missing evidence/conditions by scenario.
- Projection registries validate references and report drift.
- Sync starts report-only or one-way; bidirectional sync is not implicit.

## Open Questions

- Which tracker provider should be first?
- Should projection registries be archived/merged into baseline on OpenSpec archive?
- Should waivers require signatures/commits for team workflows?
- How should external deletion be represented: projection removed, tombstoned, or drift warning?

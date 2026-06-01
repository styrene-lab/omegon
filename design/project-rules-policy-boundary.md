+++
kind = "design_node"

[data]
title = "Project Rules Policy Boundary"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = ["TDD Savepoint Extension Extraction", "Omegon Evidence Map Schemas"]
open_questions = [
  "What is the first minimal .omegon/project-rules.toml schema we should dogfood in this repository?",
  "Which command should evaluate project rules first: an explicit check command, OpenSpec assess, or CI-only invocation?",
  "How should project rules discover external CI/check providers without hardcoding GitHub Actions, GitLab, or Forgejo semantics?"
]
+++

# Project Rules Policy Boundary

## Overview

Project Rules are the policy layer that turns evidence findings into warnings or enforcement decisions for a specific project. OpenSpec and evidence providers should make structure and evidence legible; Project Rules decide whether that evidence is sufficient for a given project/context.

This deliberately avoids the name "repo rules" because Omegon and Flynt operate on projects, not only source-code repositories. A project may be a code repo, a documentation vault, a research corpus, an engagement workspace, a design package, or a mixed system.

## Core Principle

```text
OpenSpec is descriptive.
Evidence is observational.
Project Rules are policy.
```

OpenSpec should help agents and humans reason about work:

- intent
- requirements
- scenarios
- tasks
- lifecycle state
- attached evidence claims
- support/refutation summaries

OpenSpec should not become an endless set of built-in gates. Hard denial is friction unless it is project-specific, intentional, and visible.

## Boundary

| Layer | Owns | Does not own |
|---|---|---|
| OpenSpec | change structure, scenarios, tasks, claim annotations, advisory findings | hard-coded enforcement policy |
| Evidence providers | generated observations, artifacts, claims, edges | global project policy |
| Core EvidenceStore | reading claims/records/edges and summarizing support | deciding whether a project may archive/merge/release |
| Project Rules | thresholds, severities, hard-deny behavior by context | generating provider-specific evidence |
| CI/extensions | execution context and external check evidence | universal semantics for every project |

## Decision: Findings Are Advisory Until Project Rules Consume Them

`evaluate_evidence_gates(change)` may classify findings as pass/warn/block candidates, but core OpenSpec commands should not hard-deny based on those classifications by default.

A "block" finding means:

```text
If this project has an enforcing rule for this condition, it should fail.
```

It does not mean:

```text
The built-in OpenSpec archive command must always fail.
```

## Decision: Default Mode Is Warn, Not Enforce

Initial defaults should favor visibility over friction:

```text
mode = "warn"
```

Projects opt into enforcement explicitly. This allows dogfooding and incremental adoption without turning the lifecycle system into a blocker factory.

## Proposed Config Path

Primary project-local config:

```text
.omegon/project-rules.toml
```

Future Flynt-facing projection may also expose a normalized view under:

```text
.omegon/evidence/project-rules.json
```

The TOML file is authored policy. JSON projections are derived/read-model artifacts.

## Minimal Schema Sketch

```toml
schema_version = 1
mode = "warn" # warn | enforce

[contexts.default]
mode = "warn"

[contexts.ci]
mode = "enforce"

[[rules]]
id = "no-refuted-evidence-claims"
description = "Explicitly attached OpenSpec evidence claims must not be refuted."
selector = "openspec.scenarios[*].evidence_support[*]"
when_status = ["Refuted", "Mixed"]
severity = "block"
contexts = ["default", "ci"]

[[rules]]
id = "public-api-docs"
description = "Public API documentation claim should be supported."
selector = "claims"
claim = "claim:crate:*:public-api-documented"
required_status = "Supported"
severity = "warn"
contexts = ["default", "ci"]

[[rules]]
id = "behavior-scenarios-have-tdd"
description = "Behavior scenarios should have TDD savepoint evidence."
selector = "openspec.scenarios[*]"
when_tag = "behavior"
require_provider = "tdd-savepoint"
severity = "warn"
contexts = ["ci"]
```

## Policy Concepts

### Mode

| Mode | Meaning |
|---|---|
| `warn` | Produce findings but do not fail commands. |
| `enforce` | Findings with blocking severity produce a failing report for explicit project-rules checks. |

Mode may be global or context-specific.

### Context

Contexts describe where policy is evaluated:

```text
default
local
agent
ci
release
archive
pr
```

The same evidence can have different consequences by context. For example, local archive can warn while CI release blocks.

### Severity

| Severity | Meaning |
|---|---|
| `info` | Informational only. |
| `warn` | Visible issue, non-blocking. |
| `block` | Fails when the active mode/context enforces. |

### Selectors

Initial selectors should be simple named domains rather than a full query language:

```text
openspec.scenarios[*].evidence_support[*]
claims
records
providers
artifacts
```

Avoid overbuilding a policy DSL until we have dogfood use cases.

## First Dogfood Rules for This Project

Start with rules that are useful but not overbearing:

1. **No refuted evidence claims**
   - Selector: OpenSpec scenario evidence support
   - Refuted/Mixed -> block candidate
   - Mode: warn locally, enforce in CI later

2. **Public API docs claim visible**
   - Claim: `claim:crate:*:public-api-documented`
   - Missing/support failure -> warn
   - Mode: warn

3. **Evidence map parses**
   - `.omegon/evidence/manifest.json` and JSONL streams parse
   - Failure -> block candidate in CI

4. **Generated indexes are rebuildable**
   - SQLite index may be absent or stale locally
   - CI can rebuild and compare counts
   - Failure -> warn initially

## Command Surface

Initial explicit command should be non-mutating:

```text
omegon project-rules check --context default
omegon project-rules check --context ci
```

Output:

```json
{
  "mode": "warn",
  "context": "default",
  "passed": true,
  "findings": [
    {
      "rule_id": "no-refuted-evidence-claims",
      "severity": "block",
      "enforced": false,
      "subject": "claim:...",
      "message": "Claim is refuted by evidence:..."
    }
  ]
}
```

When context mode is `enforce`, `passed=false` if any enforced `block` finding exists.

## CI and External Checks

CI should be modeled as both:

1. a **context** for evaluating Project Rules; and
2. an **evidence provider** that can emit check-run evidence.

Example future CI evidence:

```json
{
  "schema": "evidence-record/v1",
  "id": "evidence:github-actions:run:123456",
  "provider": "github-actions",
  "kind": "ci-run",
  "status": "pass",
  "subjects": ["commit:abc123", "change:jwt-auth"],
  "artifacts": ["url:https://github.com/org/repo/actions/runs/123456"],
  "claims": ["claim:change:jwt-auth:ci-pass"]
}
```

Project Rules can then require CI claims in release contexts without OpenSpec hardcoding any CI system.

## Implementation Plan

### Phase 1 — Read Model and Advisory Reports

- Add project-rules config parser.
- Add simple rule structs with mode/context/severity.
- Add evaluator that consumes existing OpenSpec evidence findings and EvidenceStore summaries.
- Add explicit `omegon project-rules check` command.
- No existing lifecycle command hard-blocks.

### Phase 2 — Dogfood Rules

- Add `.omegon/project-rules.toml` to this repo in warn mode.
- Include no-refuted-evidence-claims and evidence-map-parses rules.
- Wire CI later only after local reports are stable.

### Phase 3 — CI Context

- Add CI invocation mode.
- Produce machine-readable JSON and human-readable summary.
- Optionally fail process when mode/context enforces.

### Phase 4 — Provider Extensions

- CI/check extensions produce evidence records.
- Code evidence and TDD evidence remain providers.
- Flynt ingests the resulting evidence graph and rule reports.

## Non-goals

- Do not build a general-purpose policy language now.
- Do not hardcode GitHub/GitLab/Forgejo CI semantics in OpenSpec.
- Do not make OpenSpec archive/verify hard-block by default.
- Do not require every project to use evidence claims.

## Success Criteria

- OpenSpec scenarios can carry evidence support metadata without enforcing policy.
- Project Rules can report refuted or missing evidence in a project-specific way.
- Local default mode warns.
- CI/release contexts can later enforce.
- Flynt can display rule findings as project governance evidence rather than hidden command behavior.

+++
kind = "design_node"

[data]
title = "Omegon Evidence Map Schemas"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = ["tdd-savepoint-extension-extraction"]
open_questions = [
  "Should project-wide .omegon/evidence/*.jsonl be committed by default, or opt-in per project policy?",
  "Should manifest updates be append-only journaled or overwritten atomically on each generation pass?",
  "Which Flynt link syntax should become canonical for evidence IDs and surface IDs?"
]
+++

## Overview

Omegon should own the generated evidence substrate for agentic coding workflows. Flynt should discover and index it as one project-scoped evidence source alongside `.flynt`, `.claude`, `.codex`, generated Doxygen sites, Sphinx/ReadTheDocs output, rustdoc, coverage reports, and other specialized artifacts.

The boundary is:

```text
Omegon owns generation + normalization + evidence-map layout.
Flynt owns discovery + indexing + relationship/UI composition.
```

The normalized project-wide substrate lives under:

```text
.omegon/evidence/
```

OpenSpec change-local evidence remains a projection under:

```text
openspec/changes/<change>/evidence/
```

Raw provider logs remain provider-owned, for example:

```text
.omegon/lifecycle/savepoints/
```

This gives agents a deterministic, generated surface/evidence map without forcing Flynt to own every language/framework extractor.

## Decisions

### Decision: `.omegon/evidence/` is the project-wide normalized evidence map

**Status:** decided

Omegon writes project-wide normalized records under `.omegon/evidence/`. This is the primary ingestion point Flynt can discover as `EvidenceSourceKind::OmegonEvidenceMap`.

### Decision: OpenSpec evidence is a projection, not the registry

**Status:** decided

OpenSpec change directories receive relevant evidence projections, but they are not the only evidence registry. A TDD savepoint can write all three layers:

```text
raw provider log:
  .omegon/lifecycle/savepoints/<command_hash>.jsonl

project-wide normalized evidence:
  .omegon/evidence/records.jsonl

change-local projection:
  openspec/changes/<change>/evidence/tdd-savepoints.jsonl
```

### Decision: Surface facts and evidence records are separate streams

**Status:** decided

Surface facts describe current project/API/tool/config/spec surfaces. Evidence records describe temporal claims/proofs/statuses anchored to source state. They are linked by edges.

```text
surfaces.jsonl = what exists
records.jsonl  = what was proven/generated/observed
edges.jsonl    = how records, surfaces, claims, artifacts, and source anchors relate
```

### Decision: Doxygen is an extractor, not the abstraction

**Status:** decided

Doxygen may produce API-reference artifacts and surface records, but the normalized substrate is extractor-neutral. Other extractors include rustdoc JSON, TypeDoc, OpenAPI, AsyncAPI, extension manifests, Clap CLI metadata/help, Pkl schemas, OpenSpec parsers, and coverage/security scanners.

## Directory Layout

Minimal v1:

```text
.omegon/evidence/
├── manifest.json
├── records.jsonl
├── surfaces.jsonl
├── edges.jsonl
├── artifacts.jsonl
└── indexes/
    ├── by_subject.json
    ├── by_provider.json
    ├── by_claim.json
    ├── by_file.json
    └── by_scenario.json
```

`indexes/` is derived and rebuildable. The canonical portable substrate is the manifest and JSONL streams.

## Manifest Schema — `omegon-evidence-manifest/v1`

File:

```text
.omegon/evidence/manifest.json
```

Schema sketch:

```json
{
  "schema": "omegon-evidence-manifest/v1",
  "generator": {
    "name": "omegon",
    "version": "0.25.5",
    "commit": "468fcabf"
  },
  "project": {
    "root": ".",
    "id": "sha256:...",
    "name": "omegon-secundus"
  },
  "created_at_ms": 1780000000000,
  "source_state": {
    "git_head": "468fcabf...",
    "branch": "exploration/rust-savepoint-tdd",
    "worktree_diff_hash": "sha256:...",
    "dirty": true
  },
  "files": {
    "records": "records.jsonl",
    "surfaces": "surfaces.jsonl",
    "edges": "edges.jsonl",
    "artifacts": "artifacts.jsonl"
  },
  "providers": [
    {
      "id": "tdd-savepoint",
      "kind": "behavior-evidence",
      "raw_roots": [".omegon/lifecycle/savepoints"]
    },
    {
      "id": "surface-map",
      "kind": "surface-index",
      "raw_roots": []
    },
    {
      "id": "doxygen",
      "kind": "api-docs",
      "raw_roots": ["docs/doxygen/html"]
    }
  ]
}
```

Required v1 fields:

| Field | Meaning |
|---|---|
| `schema` | Exact schema identifier. |
| `generator.name` | Usually `omegon`. |
| `generator.version` | Omegon version that generated the files. |
| `project.root` | Project-relative root, usually `.`. |
| `created_at_ms` | Generation timestamp. |
| `files` | Relative paths to canonical streams. |
| `providers` | Provider inventory and raw roots. |

Optional but recommended:

| Field | Meaning |
|---|---|
| `generator.commit` | Omegon commit/version source. |
| `project.id` | Stable project fingerprint. |
| `project.name` | Human-readable project name. |
| `source_state` | Source checkout/freshness anchor. |

## Evidence Record Schema — `evidence-record/v1`

File:

```text
.omegon/evidence/records.jsonl
```

One JSON object per line.

Schema sketch:

```json
{
  "schema": "evidence-record/v1",
  "id": "evidence:tdd-savepoint:redgreen-123",
  "provider": "tdd-savepoint",
  "kind": "red-green",
  "status": "tdd-pass",
  "subjects": [
    "scenario:tdd/stale-pass-query",
    "surface:tool:tdd_savepoint_evidence"
  ],
  "claims": [
    "openspec:tdd-savepoint-extension:tdd/stale-pass-query"
  ],
  "artifacts": [
    "artifact:savepoint-log:sha256_abc"
  ],
  "source_state": {
    "git_head": "468fcabf...",
    "worktree_diff_hash": "sha256:..."
  },
  "created_at_ms": 1780000000000
}
```

Required v1 fields:

| Field | Meaning |
|---|---|
| `schema` | Exact schema identifier. |
| `id` | Stable evidence ID within the project. |
| `provider` | Producer/provider ID, e.g. `tdd-savepoint`, `surface-map`, `doxygen`. |
| `kind` | Provider-specific evidence kind, e.g. `red-green`, `api-reference`, `surface-current`. |
| `status` | Provider-specific status string. |
| `created_at_ms` | Event/generation timestamp. |

Optional but recommended:

| Field | Meaning |
|---|---|
| `subjects` | Surface/scenario/task/source IDs this evidence is about. |
| `claims` | Claim IDs this evidence supports or refutes. |
| `artifacts` | Artifact IDs or paths containing raw/rendered evidence. |
| `source_state` | Commit/hash/freshness anchor. |
| `metadata` | Provider-specific object. |

Status strings are provider-owned but should be stable. Examples:

```text
tdd-pass
stale-pass
red
fail
surface-pass
surface-stale
surface-partial
docs-pass
docs-warnings
docs-fail
```

## Surface Record Schema — `surface-record/v1`

File:

```text
.omegon/evidence/surfaces.jsonl
```

One JSON object per line.

Schema sketch:

```json
{
  "schema": "surface-record/v1",
  "id": "surface:tool:tdd_savepoint_run",
  "kind": "extension-tool",
  "name": "tdd_savepoint_run",
  "source_path": "extensions/omegon-tdd-savepoint/src/main.rs",
  "source_span": {
    "start_line": 123,
    "end_line": 210
  },
  "description": "Run a resolved TDD savepoint command once...",
  "signature": null,
  "input_schema": {},
  "output_schema": null,
  "extractor": "omegon-extension-schema",
  "source_hash": "sha256:...",
  "created_at_ms": 1780000000000
}
```

Required v1 fields:

| Field | Meaning |
|---|---|
| `schema` | Exact schema identifier. |
| `id` | Stable surface ID. |
| `kind` | Surface kind. |
| `name` | Human-readable symbol/tool/schema name. |
| `extractor` | Extractor that produced this record. |

Optional but recommended:

| Field | Meaning |
|---|---|
| `source_path` | Project-relative source path. |
| `source_span` | Line/column span for direct editor open. |
| `description` | Generated or extracted documentation summary. |
| `signature` | Function/type/command signature. |
| `input_schema` / `output_schema` | Tool/API/config schemas. |
| `source_hash` | Hash of source declaration/material. |
| `created_at_ms` | Generation timestamp. |

Initial surface kinds:

```text
rust-crate
rust-module
rust-struct
rust-enum
rust-trait
rust-function
rust-method
extension-tool
cli-command
config-schema
http-endpoint
openspec-change
openspec-requirement
openspec-scenario
doxygen-symbol
external-reference
```

## Edge Schema — `evidence-edge/v1`

File:

```text
.omegon/evidence/edges.jsonl
```

One JSON object per line.

Schema sketch:

```json
{
  "schema": "evidence-edge/v1",
  "from": "evidence:tdd-savepoint:redgreen-123",
  "to": "scenario:tdd/stale-pass-query",
  "kind": "supports",
  "created_at_ms": 1780000000000
}
```

Common edge kinds:

```text
supports
refutes
subjects
declared_in
generated_from
generated_to
documents
tested_by
covers
requires_evidence
stale_against
opens_with
```

## Artifact Record Schema — `artifact-record/v1`

File:

```text
.omegon/evidence/artifacts.jsonl
```

One JSON object per line.

Schema sketch:

```json
{
  "schema": "artifact-record/v1",
  "id": "artifact:doxygen:index",
  "kind": "generated-doc-site",
  "provider": "doxygen",
  "path": "docs/doxygen/html/index.html",
  "open_with": "browser",
  "hash": "sha256:...",
  "created_at_ms": 1780000000000
}
```

Common `open_with` values:

```text
browser
editor
terminal
system
flynt-note
bookokrat
```

## Minimal TDD Savepoint Projection

The first implementation target for `omegon-tdd-savepoint` is to append a normalized evidence record whenever it appends a raw savepoint event.

Example:

```json
{
  "schema": "evidence-record/v1",
  "id": "evidence:tdd-savepoint:redgreen-7a1f",
  "provider": "tdd-savepoint",
  "kind": "red-green",
  "status": "tdd-pass",
  "subjects": [
    "scenario:auth/token-expired"
  ],
  "claims": [
    "openspec:jwt-auth:auth/token-expired"
  ],
  "artifacts": [
    "path:.omegon/lifecycle/savepoints/sha256_abc.jsonl"
  ],
  "source_state": {
    "git_head": "468fcabf...",
    "branch": "exploration/rust-savepoint-tdd",
    "worktree_diff_hash": "sha256:..."
  },
  "created_at_ms": 1780000000000
}
```

Status mapping for TDD savepoint provider:

| Savepoint status | Evidence status |
|---|---|
| `TddPass` | `tdd-pass` |
| `StalePass` | `stale-pass` |
| `RedCaptured` | `red` |
| `PassNoRed` | `pass-no-red` |
| `Fail` | `fail` |
| `NoEvidence` | `no-evidence` |

## Flynt Discovery Contract

Flynt can treat `.omegon/evidence/manifest.json` as a known incoming ingestion point.

Discovery rule:

```text
if <project-root>/.omegon/evidence/manifest.json exists:
    register EvidenceSourceKind::OmegonEvidenceMap
    read manifest.files
    index records/surfaces/edges/artifacts as derived registry state
```

Flynt should not be required to generate or validate these records initially. It can ingest them opportunistically, show freshness/source state, and let high-level Flynt documents cite evidence and surface IDs.

## Persistence Policy

Default policy should distinguish canonical generated streams from derived indexes:

```gitignore
.omegon/evidence/indexes/
.omegon/evidence/cache/
.omegon/lifecycle/savepoints/
```

Projects may opt into committing:

```text
.omegon/evidence/manifest.json
.omegon/evidence/records.jsonl
.omegon/evidence/surfaces.jsonl
.omegon/evidence/edges.jsonl
.omegon/evidence/artifacts.jsonl
```

The project policy is intentionally not universal because audit-heavy projects, open-source libraries, monorepos, and private app repos have different generated-artifact tradeoffs.

## Implementation Sequence

1. Add schema constants/types for `evidence-record/v1` and manifest writing in the TDD savepoint extension.
2. Update `append_event()` to also append `.omegon/evidence/records.jsonl` and create/update `.omegon/evidence/manifest.json`.
3. Add a future `surface-map` provider for extension tool schemas, CLI commands, config schemas, and language API extractors.
4. Replace core `Scenario.tdd_evidence` with provider-neutral `Scenario.evidence: Vec<ScenarioEvidenceSummary>`.
5. Coordinate with Flynt so `.omegon/evidence/manifest.json` is recognized as a known evidence source.

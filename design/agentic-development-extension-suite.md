+++
kind = "design_node"

[data]
title = "Agentic Development Extension Suite"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = ["omegon-evidence-map-schemas", "tdd-savepoint-extension-extraction"]
open_questions = [
  "Should the first packaged suite ship in-tree only, or also publish through Armory as a named bundle?",
  "Should suite-level install/enable UX live in `omegon extension` or a higher-level `omegon devkit` command?",
  "Which provider should own language-specific non-Rust surface extractors after Rust proves the substrate?"
]
+++

## Overview

Omegon should treat evidence generation, code-surface discovery, behavioral proof capture, browser automation, and future documentation audit workflows as a coherent set of agentic-development extensions rather than unrelated tools.

The purpose is to let an agent answer and act on instructions such as:

```text
update the documentation with evidence
prove this scenario red→green
map this code surface into Flynt
trace this diagram claim back to source
```

without each capability inventing its own storage, naming, and discovery rules.

## Suite Name

Use the operator-facing bundle name:

```text
Omegon Agentic Development Kit
```

Short name:

```text
omegon-devkit
```

This names the roll-up, not necessarily one extension binary. It communicates intent better than `surface-map`, which is too ambiguous: surface of what, for whom, and at what layer?

## Extension Naming Decisions

### Decision: Avoid `omegon-surface-map`

**Status:** decided

`omegon-surface-map` is not specific enough. It could mean UI surfaces, tool surfaces, API surfaces, Flynt visual surfaces, or terminal surfaces. The provider we are prototyping is specifically about generated code/API/tool/config surfaces as evidence for agentic development.

### Decision: Name the Rust/API surface provider `omegon-code-evidence`

**Status:** decided

The best near-term extension name for the rustdoc/surface/evidence generator is:

```text
omegon-code-evidence
```

Rationale:

- It says what it produces: code-derived evidence.
- It is not limited to rustdoc forever.
- It can own language extractors such as Rust/rustdoc, TypeScript/TypeDoc, Python/Sphinx/pdoc, OpenAPI, CLI schemas, and config schemas.
- It composes naturally with `omegon-tdd-savepoint`, which produces behavioral evidence.

Rejected alternatives:

| Name | Rejection reason |
|---|---|
| `omegon-surface-map` | Ambiguous; sounds like UI/visual surface mapping. |
| `omegon-rustdoc-evidence` | Too narrow; useful as an extractor ID, not extension name. |
| `omegon-api-evidence` | Too API-specific; misses CLI/config/internal code surfaces. |
| `omegon-doc-evidence` | Sounds like prose documentation, not source/API surfaces. |
| `omegon-code-map` | Good but undersells evidence/provenance. |
| `omegon-dev-evidence` | Close, but less precise than code evidence. |

### Decision: Keep behavioral evidence separate as `omegon-tdd-savepoint`

**Status:** decided

`omegon-tdd-savepoint` remains focused on red/green behavioral proof. Do not fold it into `omegon-code-evidence`. They share `.omegon/evidence/` as substrate but own different provider domains.

## Suite Members

Initial suite:

| Extension | Provider domain | Status |
|---|---|---|
| `omegon-tdd-savepoint` | Behavioral red/green evidence | Scaffolded and producing normalized evidence. |
| `omegon-code-evidence` | Code/API/tool/config surface evidence | Prototype exists as `scripts/generate_rust_surface_evidence.py`; promote next. |
| `omegon-browser` | Browser automation / UI observation evidence | Existing extension; can eventually emit evidence records for UI flows. |

Future possible members:

| Extension | Provider domain |
|---|---|
| `omegon-contract-evidence` | OpenAPI/AsyncAPI/protobuf contract conformance. |
| `omegon-coverage-evidence` | Coverage reports and test-to-surface coverage mapping. |
| `omegon-security-evidence` | Static analysis/security audit evidence. |
| `omegon-doc-audit` | Documentation freshness, link validation, doc coverage summaries. |

The future list is not a mandate to create many binaries. Some may become tools inside `omegon-code-evidence` if their extraction dependencies and lifecycle are similar.

## Shared Substrate

All suite members write normalized streams under:

```text
.omegon/evidence/
```

Canonical files:

```text
manifest.json
records.jsonl
surfaces.jsonl
edges.jsonl
artifacts.jsonl
```

Raw provider logs remain provider-owned, for example:

```text
.omegon/lifecycle/savepoints/
extensions/.../target/doc/*.json
coverage/lcov.info
docs/doxygen/html/
```

Change-local projections remain under:

```text
openspec/changes/<change>/evidence/
```

## Provider Boundaries

### `omegon-tdd-savepoint`

Owns:

- test command planning/running
- red/fail/pass/stale classification
- raw savepoint logs
- behavioral evidence records
- OpenSpec scenario projections

Does not own:

- code/API surface extraction
- doc coverage analysis
- API reference generation

### `omegon-code-evidence`

Owns:

- code/API/tool/config surface extraction
- rustdoc/TypeDoc/Sphinx/Doxygen/OpenAPI extractor adapters
- surface records
- source/artifact edges
- doc coverage evidence
- documentation gap summaries

Does not own:

- running tests for red/green evidence
- long-lived browser automation
- Flynt ingestion/UI

### Flynt

Owns:

- discovery of `.omegon/evidence/manifest.json`
- indexing evidence/surface/artifact records
- visual graph/document relationships
- high-level documentation and diagram citation UX

Does not own:

- generating language-specific evidence maps
- understanding every extractor's raw output format

## User-Facing Workflows

### Update documentation with evidence

Target behavior:

```text
omegon-code-evidence plan
omegon-code-evidence generate --extractor rustdoc
omegon-code-evidence summarize-doc-coverage
```

Agent-facing shorthand:

```text
update the documentation with evidence
```

Expected actions:

1. Generate code surface records.
2. Generate doc coverage evidence.
3. Produce/update a doc coverage summary artifact.
4. Patch missing docs or propose a task list.
5. Regenerate evidence to prove improvement.
6. Flynt ingests the evidence map and links docs/diagrams to source surfaces.

### Prove scenario implementation

Target behavior:

```text
omegon-tdd-savepoint plan
omegon-tdd-savepoint run
omegon-tdd-savepoint evidence
```

Expected output:

- `tdd-pass` / `stale-pass` / `red` / `fail`
- evidence record linked to scenario ID
- optional OpenSpec change-local projection

## Promotion Path From Script to Extension

The current dogfood script:

```text
scripts/generate_rust_surface_evidence.py
```

should be treated as a learning prototype for `omegon-code-evidence`.

Promotion phases:

### Phase 1 — Harden script enough for dogfood

Done / in progress:

- rustdoc JSON extraction
- source path/hash resolution
- derive-noise filtering
- basic signature rendering
- surface records
- source/artifact edges
- doc coverage evidence

### Phase 2 — Stabilize the CLI contract

Add script-level options that should become extension tool inputs:

```text
--output-dir
--append-records / --replace-records
--crate-target
--scope
--extractor rustdoc-json
--summary-path
```

### Phase 3 — Create `extensions/omegon-code-evidence`

Native extension tools:

```text
code_evidence_status
code_evidence_extractors
code_evidence_plan
code_evidence_generate
code_evidence_doc_coverage
code_evidence_query
```

Initial extractor:

```text
rustdoc-json
```

### Phase 4 — Add non-Rust extractors

Only after Rust proves the record shape:

```text
typedoc
openapi
doxygen
sphinx/pdoc
clap-cli
pkl-schema
extension-tools
```

### Phase 5 — Armory/devkit roll-up

Create bundle metadata so operators can install/enable the agentic development kit as one conceptual unit:

```text
omegon-devkit:
  - omegon-tdd-savepoint
  - omegon-code-evidence
  - omegon-browser
```

## Best Next Steps

1. Add a doc coverage summary artifact to the current script:

```text
.omegon/evidence/summaries/rust-doc-coverage.md
```

2. Add artifact + edge records for that summary.
3. Add `--output-dir`, `--replace-records`, and `--summary-path` flags.
4. Use the summary to patch missing public docs in `omegon-tdd-savepoint`.
5. Regenerate evidence and verify `rust-doc-coverage` improves.
6. Promote the script into `extensions/omegon-code-evidence` once the workflow is stable.

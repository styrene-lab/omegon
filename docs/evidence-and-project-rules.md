# Evidence Maps and Project Rules

Omegon 0.26 introduces an experimental local evidence substrate for agentic development. It is designed to let deterministic tools, extensions, generated documentation, tests, citations, and human review records describe what they know without requiring an LLM at policy-check time.

## Evidence map layout

Canonical project evidence lives under:

```text
.omegon/evidence/
├── manifest.json
├── claims.jsonl
├── records.jsonl
├── surfaces.jsonl
├── edges.jsonl
├── artifacts.jsonl
├── summaries/
└── indexes/
```

The JSON and JSONL files are the canonical, portable data. Anything under `indexes/` is derived and rebuildable. The dogfood generator currently builds a SQLite/FTS index at:

```text
.omegon/evidence/indexes/evidence.sqlite
```

If the JSONL streams and SQLite disagree, the JSONL streams win.

## Core concepts

- **Claim**: an assertion that can be supported or refuted.
- **Evidence record**: an observation, generated report, test result, citation, review, or other source of support/refutation.
- **Surface**: a code/API/config/docs object with source anchors.
- **Artifact**: a concrete file, report, URL, generated doc, or external citation target.
- **Edge**: a relationship such as `supports`, `refutes`, `generated_from`, `declared_in`, or `belongs_to`.

Example claim:

```text
claim:crate:omegon-tdd-savepoint:public-api-documented
```

Example support edge:

```text
evidence:code-evidence:rust-doc-coverage:<run>
  --supports-->
claim:crate:omegon-tdd-savepoint:public-api-documented
```

## Generating dogfood Rust evidence

The current prototype generator is intentionally a script, not a stable extension yet:

```bash
python3 scripts/generate_rust_surface_evidence.py
```

It uses nightly rustdoc JSON for `extensions/omegon-tdd-savepoint`, writes surface records, emits doc coverage evidence, creates a markdown summary, and rebuilds the derived SQLite index.

The summary is written to:

```text
.omegon/evidence/summaries/rust-doc-coverage.md
```

This is how to inspect current documentation gaps without reading raw JSONL.

## Querying evidence

A lightweight query helper is available for dogfooding:

```bash
python3 scripts/query_evidence.py claims
python3 scripts/query_evidence.py search public-api
python3 scripts/query_evidence.py get claim:crate:omegon-tdd-savepoint:public-api-documented
python3 scripts/query_evidence.py neighbors claim:crate:omegon-tdd-savepoint:public-api-documented
```

The helper queries the derived SQLite index but reports canonical IDs from the evidence streams.

## OpenSpec integration

OpenSpec scenarios can reference evidence claims explicitly:

```markdown
### Requirement: Public API docs

evidence-claim: claim:crate:omegon-tdd-savepoint:public-api-documented

#### Scenario: Docs are present
Given public API surfaces exist
When evidence is evaluated
Then the documentation claim is supported
```

Omegon annotates loaded scenarios with provider-neutral claim support summaries. OpenSpec remains descriptive: it does not hard-deny archive or implementation operations on its own.

## Project Rules

Project Rules are deterministic local checks over project files and evidence streams. They do not call LLM providers, require auth, run agents, or infer missing evidence.

Configuration lives at:

```text
.omegon/project-rules.toml
```

Dogfood configuration includes:

```toml
[contexts.default]
mode = "warn"

[contexts.ci]
mode = "enforce"
```

Run local advisory checks:

```bash
omegon --cwd . project-rules check --context default
```

Run CI/enforced checks:

```bash
omegon --cwd . project-rules check --context ci
```

Use JSON for automation:

```bash
omegon --cwd . project-rules check --context ci --json
```

Current rule kinds include:

- `evidence-map-parses`
- `no-refuted-evidence-claims`
- `claim-supported`

Repository configuration owns enforcement. OpenSpec and evidence providers only surface facts and severity; project rules decide whether a finding fails a context.

## Stability

The 0.26 evidence schema and code-evidence generator are experimental. Schema identifiers such as `claim-record/v1` and `evidence-record/v1` are stream format versions, not long-term compatibility guarantees before 1.0.

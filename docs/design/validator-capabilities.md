# Validator Capabilities and Operator Overrides

`validate` is the stable agent-facing gate for post-mutation checks. Agents name the paths and requested level; the harness owns validator discovery, execution policy, and reporting.

## Goals

- Keep `validate(paths, level)` as the single mechanical validation process.
- Make validators explicit bounded capabilities rather than assistant-invented shell commands.
- Let opinionated operators add, replace, or disable validators without patching Omegon.
- Make validator provenance, safety, and last-run status visible in TUI/Web/Workbench surfaces.
- Prepare the same metadata shape for future extension-declared validators.

## Non-goals

- Do not make the assistant choose ad hoc validation commands.
- Do not turn `validate` into arbitrary shell execution.
- Do not hardcode every structured data format into the core harness.
- Do not auto-enable community validators that require network, mutation, secrets, host package installation, or container execution.

## Validator classes

- **Built-in language validators**: Rust, TypeScript, and Python project validators currently shipped in core.
- **Core artifact validators**: small deterministic parsers/checks such as JSON, TOML, Markdown hygiene, and conflict markers.
- **Domain validators**: Omegon/Flynt/OpenSpec-specific checks such as model registry, skills, OpenAPI contracts, boards, and flows.
- **Operator overrides**: project/user config entries that supplement or replace matching validators.
- **Extension validators**: future first-class extension capabilities with the same declaration shape and stricter trust/provenance metadata.
- **Armory/community validators**: installable validator packages discovered through Armory, then trusted/enabled by the operator or project policy.

## Project override file

Project-local overrides live at `.omegon/validators.toml`:

```toml
version = 1

[[validators]]
id = "project.markdown"
description = "Project markdownlint policy"
include = ["**/*.md", "CHANGELOG.md"]
exclude = ["target/**"]
levels = ["quick", "standard", "full"]
mode = "supplement" # supplement | replace
replaces = ["core.markdown-basic"]
priority = 100

[validators.runner]
kind = "process"
program = "markdownlint"
args = ["--config", ".markdownlint.json"]
path_arg_mode = "append" # append | none

[validators.policy]
read_only = true
network = false
mutates = false
timeout_secs = 30
```

## Execution policy

The override hatch is intentionally easy, but still bounded:

- no shell strings and no `sh -c`;
- runner arguments are fixed arrays;
- changed paths are appended as literal argv entries when requested;
- cwd is the project root;
- stdout/stderr are captured;
- timeouts are mandatory or defaulted;
- network, mutation, and secret use default to false and must be declared;
- validators declaring network, mutation, or non-read-only access are gated until the trust/policy model explicitly permits them;
- runner output is summarized and attached to structured validation details.

## Merge semantics

- `supplement` runs alongside built-ins and extension validators.
- `replace` suppresses explicitly listed validator ids for matching paths.
- Disabled validators are a future extension to this file and must include provenance in reports.
- Replacement requires stable validator ids for built-ins, operator validators, and extension validators.

## Reporting requirements

Validation reports should include per-validator provenance:

- validator id;
- source file or extension id;
- mode and replacement behavior;
- command/runner kind without secret values;
- checked paths;
- status and diagnostics;
- elapsed time;
- whether the validator was skipped by policy.

Operators should eventually be able to run `validate explain <path>` and see exactly which validators will run and which are replaced or disabled.

## UX/menu design

Bare `/validate` should open a structured validation capability manager rather than dumping text. The menu treats validators as capabilities, not commands.

### Tabs

```text
Validate
├── Overview
├── Active
├── Available
├── Overrides
├── Runs
└── Settings
```

### Overview tab

Answers: "what will validate do right now?"

Rows/actions:

- Run validation for changed files.
- Explain validators for a selected/current path.
- Show counts for built-ins, project overrides, installed Armory validators, extension validators, and gated validators.
- Show last validation run status.

Example summary:

```text
Validation overview
  Status: enabled
  Built-ins: 3 active
  Operator overrides: 2 active
  Armory validators: 4 installed, 1 disabled
  Extension validators: 3 active
  Last run: passed · 14s ago · 5 validators
```

### Active tab

Shows validators participating in current validation policy.

Suggested columns:

- status/badges;
- id;
- source: builtin, project, user, armory, extension;
- match summary;
- levels;
- mode: supplement or replace;
- runner summary;
- safety policy.

Example rows:

```text
ON SUP RO  language.rust          builtin   *.rs + Cargo.toml       quick standard full
ON SUP RO  language.typescript    builtin   *.ts *.tsx + tsconfig   quick standard full
ON SUP RO  language.python        builtin   *.py + pyproject.toml   quick standard full
ON REP RO  project.docs           project   **/*.md                 standard full
OFF SUP NET community.linkcheck   armory    docs/**/*.md            full
MISS SUP RO community.openapi     armory    **/*.openapi.*          standard full
```

Row detail should include full provenance:

```text
Validator: project.docs
Source: .omegon/validators.toml
Mode: replace
Replaces: core.markdown-basic
Runner: process markdownlint --config .markdownlint.json {paths}
Policy: read-only, no-network, no-mutation, timeout 30s
Description: Project markdownlint policy
```

### Available tab

Shows Armory/community validators that can be installed.

Suggested columns:

- install state;
- trust/source: bundled, Styrene, community, local;
- domain;
- safety badges;
- runner/dependency type;
- patterns handled.

Example rows:

```text
○ markdownlint             community   Markdown docs       RO      not installed
○ spectral-openapi         community   OpenAPI contracts   RO NEX  not installed
○ pkl-schema               styrene     Pkl schemas         RO      not installed
○ actionlint               community   GitHub Actions      RO      not installed
○ biome-json               community   JSON/JS/TS format   MUT     gated
```

Installation detail must show manifest, dependencies, policy, and trust state before enabling:

```text
Install validator: spectral-openapi

Source: Armory community
Validator id: community.spectral-openapi
Matches:
  - **/*.openapi.yaml
  - **/*.openapi.yml
Runner:
  kind: process
  program: spectral
  args: lint
Dependencies:
  - @stoplight/spectral-cli via Nex/npm
Policy:
  read-only: true
  network: false
  mutates: false
  timeout: 60s
Trust:
  community package; requires explicit enablement
```

Community lifecycle:

```text
available → installed → trusted → enabled
```

Installed validators that are not trusted should remain disabled.

### Overrides tab

Shows and edits `.omegon/validators.toml`.

Rows are project/user-defined validators with include/exclude/mode/runner/policy summaries. Creation should be form-driven and write TOML, not require hand-editing first.

Creation fields:

```text
id
include
exclude
levels
mode: supplement | replace
replaces
runner kind
program
args
path arg mode: append | none
timeout
read-only
network
mutates
```

### Runs tab

Shows validation run history and makes validator tool calls visible.

Example rows:

```text
14:22:03 passed   standard  5 validators  7 paths
14:18:44 failed   quick     1 validator   CHANGELOG.md
14:05:12 skipped  standard  no validators data/model-registry.json
```

Run detail:

```text
Validation run · standard · passed
Paths:
  CHANGELOG.md
  data/model-registry.json

Validators:
  ✓ core.text                       2 paths · 4ms
  ✓ core.json-syntax                1 path  · 1ms
  ✓ project.docs                    1 path  · 280ms
  ⚠ omegon.model-registry           unavailable · tool missing

Output:
  project.docs (`markdownlint --config .markdownlint.json CHANGELOG.md`): ✓ passed
```

Runs tab actions should include inspect, rerun, copy report, and open failed file.

### Settings tab

Controls validation policy and trust defaults:

```text
Default level: standard
Run on commit: warn
Run after edits: manual
Community validators: require approval
Network validators: disabled
Mutating validators: disabled
Secret validators: disabled
Timeout default: 30s
```

## Explain view

`/validate explain <path>` should show matching, replacement, and missing-but-installable validators.

Example:

```text
CHANGELOG.md

Matched validators:
  ✓ core.text
      source: builtin
      mode: supplement

  ✕ core.markdown-basic
      source: builtin
      replaced by: project.docs

  ✓ project.docs
      source: .omegon/validators.toml
      mode: replace
      runner: markdownlint --config .markdownlint.json {paths}
      policy: read-only, no-network, timeout 30s

Available but not installed:
  omegon.changelog
      install: /validate install omegon.changelog
```

## Backend inventory needed for menus

Menus should consume structured DTOs rather than parsing text output.

```rust
struct ValidationInventory {
    validators: Vec<ValidatorInventoryRow>,
}

struct ValidatorInventoryRow {
    id: String,
    source: ValidatorSource,
    enabled: bool,
    mode: ValidatorMode,
    replaces: Vec<String>,
    include: Vec<String>,
    exclude: Vec<String>,
    levels: Vec<ValidationLevel>,
    runner_summary: String,
    policy: ValidatorPolicy,
    status: ValidatorStatus,
    last_run: Option<ValidatorLastRun>,
}
```

Explain DTO:

```rust
struct ValidationExplain {
    path: PathBuf,
    level: ValidationLevel,
    matched: Vec<ValidatorInventoryRow>,
    replaced: Vec<ValidatorReplacement>,
    skipped: Vec<ValidatorSkip>,
    available: Vec<InstallableValidatorSummary>,
}
```

Run-history DTO:

```rust
struct ValidationRunSummary {
    id: String,
    timestamp: String,
    level: ValidationLevel,
    status: ValidationStatus,
    paths: Vec<PathBuf>,
    validators: Vec<ValidatorRunSummary>,
}
```

## Preliminary implementation plan

### Phase 1 — capability inventory and stable ids

- Add stable ids to built-in validators: `language.rust`, `language.typescript`, `language.python`.
- Introduce shared inventory row structs for built-in and operator validators.
- Add backend function to list active validators for a cwd/level/path set.
- Include operator validator source/provenance, mode, replacement declarations, runner summary, and policy.
- Keep current execution behavior, but make reports use ids.

### Phase 2 — replacement semantics

- Enforce `mode = "replace"` by suppressing listed validator ids for matched paths.
- Report replacement decisions in text and structured details.
- Add tests for replacing `language.rust` and future core validators.

### Phase 3 — explain surface

- Add `validate explain` backend function/command path.
- Show matched, replaced, skipped, and installable validators for a path.
- Surface explain data in TUI menu detail and command output.

### Phase 4 — run history and UX surfacing

- Store session-local validation runs with per-validator results.
- Add structured run summaries to Workbench/TUI/Web projections.
- Ensure operator validator process runs appear as validator runs, not opaque shell/tool noise.

### Phase 5 — `/validate` menu MVP

- Route bare `/validate` to a structured menu.
- Implement Overview and Active tabs first.
- Add row inspection and run-current-path/changed-paths actions.
- Show safety badges and source badges.

### Phase 6 — Overrides tab

- List `.omegon/validators.toml` entries.
- Add form-driven create/edit/delete for project overrides.
- Preserve comments where possible, but correctness beats comment preservation initially.

### Phase 7 — Armory Available tab

- Extend Armory manifests so validators are explicit installable capabilities with policy, dependencies, and trust metadata.
- Show Available validators from Armory.
- Implement install → trusted → enabled lifecycle.
- Gate community/network/mutating/secret validators by policy.

### Phase 8 — prebuilt validator expansion

- Add core artifact validators: `core.json-syntax`, `core.toml-syntax`, `core.markdown-basic`, `core.no-conflict-markers`.
- Add Omegon domain validators: `omegon.model-registry`, `omegon.skill-manifest`, `omegon.openapi-contract`, `omegon.changelog`.
- Expose Flynt/OpenSpec validators through extension capability declarations.

+++
kind = "design_node"

[data]
title = "TDD Savepoint Extension Extraction"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = ["8d214819-082b-4742-8b4b-bcca1c528a9c", "scenario-ids-lifecycle-targets"]
open_questions = [
  "Should long-running watch mode be implemented as a background child process in v1 or deferred until after run/evidence/plan tools prove the model?",
  "Should the extension ship as bundled in-tree first, or immediately as a separate Armory repository?",
  "Should project config live only at .omegon/tdd-savepoint.toml, or should OpenSpec-local config also be supported?",
  "What should the initial policy be for shell execution: unsupported, disabled by default, or allowed only from project config?"
]
+++

## Overview

Extract the TDD savepoint prototype from Omegon core into a native extension named `omegon-tdd-savepoint`. Core should retain scenario identity and generic evidence read-model hooks; the extension should own deterministic red→green capture, language presets, command execution, watch orchestration, raw provider logs, and projected scenario evidence summaries.

This keeps Omegon core lifecycle-neutral while making TDD savepoints composable across Rust, Python, TypeScript, Go, Java, Zig, Make/Just, and other ecosystems through parameterized commands and presets.

## Assessment of Current Landscape

### Extension system

Current native extension pattern is represented by `extensions/omegon-browser`:

- Rust binary using `omegon-extension`
- `manifest.toml` with `[extension]`, `[runtime]`, `[startup]`, and `[config.*]`
- JSON-RPC over stdin/stdout
- implements `initialize`, `get_tools` / `tools/list`, `execute_tool` / `tools/call`, `bootstrap_config`
- installed through `omegon extension install <name>`
- discoverable through Armory registry metadata

This is the correct extraction shape for TDD savepoints.

### Armory

Armory discovery treats extensions as installable assets from `registry.toml`, local paths, or git/tarball sources. The TDD extension should be category `lifecycle` or `testing`; prefer `lifecycle` because the value is not just test execution but lifecycle evidence.

### Current prototype

The core prototype proves:

- command identity hashing
- deterministic pass/fail classification from exit code
- timeout handling
- git/worktree diff identity including untracked files
- red→green event capture
- explicit failing-run persistence with dedupe
- raw JSONL event storage
- projected OpenSpec summaries
- scenario IDs and evidence annotation in the Rust OpenSpec read model

The prototype should now stop growing inside core. It becomes the extraction source.

## Decisions

### Decision: Extract as `omegon-tdd-savepoint`

**Status:** decided

Use a native Rust extension named `omegon-tdd-savepoint`. The name is explicit, discoverable, and narrower than `omegon-tdd`.

### Decision: Kernel remains language-agnostic

**Status:** decided

The extension decides pass/fail only from configured process exit semantics. It must not parse pytest/cargo/vitest/go/maven output in v1. Language support is provided through presets and command templates, not bespoke semantic parsers.

### Decision: Configuration uses four-layer precedence

**Status:** decided

Resolution order:

```text
extension defaults
→ built-in preset
→ project config
→ scenario metadata
→ per-call overrides
```

Every resolved run must be inspectable through a plan/dry-run tool that shows final values and their sources.

### Decision: First-class tool surface starts with plan/run/evidence/presets/status

**Status:** decided

`watch` is useful but long-running. V1 should implement non-blocking or bounded tools first:

- `tdd_savepoint_status`
- `tdd_savepoint_presets`
- `tdd_savepoint_plan`
- `tdd_savepoint_run`
- `tdd_savepoint_evidence`

True background `tdd_savepoint_watch` should either return a watcher handle or be deferred until the extension has a clear background process/session story.

### Decision: Core owns generic scenario identity and evidence, not TDD-specific status

**Status:** decided

Core should retain explicit scenario IDs and derived fallback IDs. Long-term, `Scenario.tdd_evidence` should be replaced by generic scenario evidence summaries. The extension writes provider-specific raw logs and projected generic evidence.

### Decision: Projected evidence belongs under an evidence subdirectory

**Status:** decided

Use:

```text
openspec/changes/<change>/evidence/tdd-savepoints.jsonl
```

not:

```text
openspec/changes/<change>/tdd-savepoints.jsonl
```

This composes with other providers such as coverage, security review, manual QA, and contract tests.

## Extension Architecture

```text
extensions/omegon-tdd-savepoint/
├── Cargo.toml
├── manifest.toml
├── README.md
└── src/
    ├── main.rs          # Extension trait + RPC dispatch
    ├── config.rs        # config loading, merging, source tracking
    ├── presets.rs       # built-in language presets and detection
    ├── event.rs         # raw event + projected evidence schemas
    ├── runner.rs        # command execution, timeout, output bounds
    ├── git_state.rs     # branch/head/diff hash/untracked capture
    ├── evidence.rs      # read/query/classify projected/raw evidence
    ├── openspec.rs      # scenario metadata/project evidence helpers
    └── watch.rs         # optional background watcher implementation
```

## Manifest Shape

```toml
[extension]
name = "omegon-tdd-savepoint"
version = "0.1.0"
description = "Deterministic red-green TDD evidence capture for OpenSpec scenarios across languages"

[runtime]
type = "native"
binary = "target/release/omegon-tdd-savepoint"

[startup]
ping_method = "get_tools"
timeout_ms = 5000

[config.default_timeout_secs]
type = "number"
label = "Default command timeout"
description = "Default timeout for commands run by the TDD savepoint extension."
default = "60"

[config.default_debounce_ms]
type = "number"
label = "Default debounce"
description = "Default file watcher debounce interval."
default = "150"

[config.raw_event_dir]
type = "string"
label = "Raw event directory"
description = "Directory for raw TDD savepoint event logs, relative to project root."
default = ".omegon/lifecycle/savepoints/tdd"

[config.project_config_path]
type = "string"
label = "Project config path"
description = "Path to the project-level TDD savepoint config."
default = ".omegon/tdd-savepoint.toml"
```

## Tool Surface

### `tdd_savepoint_status`

Checks extension readiness:

- git available
- project root detected
- OpenSpec directory present
- project config status
- known presets
- active watchers, if supported

### `tdd_savepoint_presets`

Lists built-in and project-defined presets. Can optionally detect likely presets for the current repo.

### `tdd_savepoint_plan`

Resolves configuration without executing a command. This is mandatory for operator trust.

Input:

```json
{
  "change": "jwt-auth",
  "scenario": "auth/token-expired",
  "preset": null,
  "overrides": {}
}
```

Output:

```json
{
  "resolved": {
    "command": ["pytest", "tests/test_auth.py::test_expired_token"],
    "watch_paths": ["src/auth", "tests/auth"],
    "filetypes": ["py"],
    "timeout_secs": 30,
    "emit_baseline": true,
    "persist_failures": true
  },
  "sources": {
    "command": ".omegon/tdd-savepoint.toml:scenarios.auth/token-expired.command",
    "timeout_secs": "scenario metadata:test.timeout"
  },
  "warnings": []
}
```

### `tdd_savepoint_run`

Runs the resolved command once. Can emit baseline/fail evidence and can classify the run.

### `tdd_savepoint_evidence`

Queries raw/projected evidence and returns a status such as `no-evidence`, `red`, `tdd-pass`, `pass-no-red`, `stale-pass`, or `fail`.

### `tdd_savepoint_watch`

Long-running watch mode. V1 should not block a JSON-RPC call indefinitely. Options:

1. defer watch until run/evidence/plan are proven;
2. spawn a child watcher process and return a handle;
3. use host terminal/background-session support when available.

## Configuration Model

### Resolution precedence

```text
extension defaults
→ built-in preset
→ project preset/config
→ scenario metadata
→ per-call overrides
```

### Project config path

Default:

```text
.omegon/tdd-savepoint.toml
```

### Schema sketch

```toml
schema_version = 1

[defaults]
cwd = "."
timeout_secs = 60
debounce_ms = 150
recursive = true
ignore_gitignored = true
emit_baseline = true
persist_failures = false
dedupe_failures = true
max_output_chars = 8192
success_exit_codes = [0]
raw_event_dir = ".omegon/lifecycle/savepoints/tdd"
project_evidence_dir = "openspec/changes/{change}/evidence"

exclude_globs = [
  ".git/**",
  ".omegon/**",
  "target/**",
  "node_modules/**",
  "dist/**",
  "build/**",
  ".venv/**",
  "__pycache__/**"
]

[presets.backend]
extends = "python-pytest"
watch_paths = ["src/backend", "tests/backend"]
command = ["pytest", "tests/backend"]

[presets.frontend]
extends = "typescript-vitest"
watch_paths = ["web/src", "web/tests"]
command = ["pnpm", "vitest", "run"]

[scenarios."auth/token-expired"]
preset = "backend"
command = ["pytest", "tests/backend/test_auth.py::test_expired_token"]
timeout_secs = 30
emit_baseline = true
persist_failures = true
```

### Scenario metadata overrides

Scenario metadata can override or complete config:

```markdown
#### Scenario: Expired token rejected
<!-- id: auth/token-expired -->
<!-- test.preset: backend -->
<!-- test.command: pytest tests/backend/test_auth.py::test_expired_token -->
<!-- test.watch: src/backend, tests/backend -->
<!-- test.timeout: 30 -->
```

Use sparingly. Durable command mappings are usually better in project config; scenario metadata is best for identity, risk, dependencies, tags, and exceptions.

## Parameterization Surface

### Execution

- command argv
- cwd
- env
- timeout_secs
- success_exit_codes
- max_output_chars
- shell mode (defer or disabled by default)

### Watch

- watch_paths
- filetypes
- include_globs
- exclude_globs
- recursive
- follow_symlinks
- ignore_gitignored
- debounce_ms
- coalesce_while_running

### Evidence

- raw_event_dir
- project_evidence_dir
- emit_baseline
- persist_failures
- dedupe_failures
- dedupe_key
- hash_algorithm
- code_state_strategy
- provider name
- evidence kind

### Scenario/OpenSpec

- change
- scenario
- task
- scenario command mapping
- required status
- waiver policy

## Built-in Presets

Built-ins are convenience defaults, not policy.

Examples:

```toml
[presets.python-pytest]
filetypes = ["py"]
watch_paths = ["src", "tests"]
command = ["pytest"]
detect_files = ["pyproject.toml", "pytest.ini", "tox.ini"]
detect_dirs = ["tests"]

[presets.typescript-vitest]
filetypes = ["ts", "tsx", "js", "jsx"]
watch_paths = ["src", "test", "tests"]
command = ["pnpm", "vitest", "run"]
detect_files = ["package.json", "vitest.config.ts", "vitest.config.js"]

[presets.go-test]
filetypes = ["go"]
watch_paths = ["."]
command = ["go", "test", "./..."]
detect_files = ["go.mod"]

[presets.rust-cargo]
filetypes = ["rs"]
watch_paths = ["src", "tests"]
command = ["cargo", "test"]
detect_files = ["Cargo.toml"]

[presets.java-maven]
filetypes = ["java"]
watch_paths = ["src/main", "src/test"]
command = ["mvn", "test"]
detect_files = ["pom.xml"]

[presets.generic-just]
watch_paths = ["."]
command = ["just", "test"]
detect_files = ["justfile", "Justfile"]
```

## Multi-command Scenarios

Polyglot scenarios may need several commands.

Future schema:

```toml
[scenarios."checkout/tax-calculation"]
strategy = "all"

[[scenarios."checkout/tax-calculation".commands]]
id = "backend"
preset = "backend"
command = ["pytest", "tests/test_tax.py::test_checkout_tax"]

[[scenarios."checkout/tax-calculation".commands]]
id = "frontend"
preset = "frontend"
command = ["pnpm", "vitest", "run", "tax-summary"]
```

Strategies:

- `all`: all commands must red→green
- `any`: any command red→green is enough
- `primary`: one command provides scenario status, others are supplementary

V1 may defer this but the config should not preclude it.

## Evidence Schema

### Raw event

```json
{
  "schema": "tdd-savepoint-event/v1",
  "provider": "tdd-savepoint",
  "event_id": "redgreen-...",
  "transition": "failing_to_passing",
  "command_hash": "sha256:...",
  "command": ["pytest", "tests/test_auth.py::test_expired_token"],
  "exit_before": 1,
  "exit_after": 0,
  "worktree_diff_hash_before": "sha256:...",
  "worktree_diff_hash_after": "sha256:..."
}
```

### Projected evidence

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
  "command_hash": "sha256:...",
  "worktree_diff_hash": "sha256:...",
  "created_at": "2026-05-30T00:00:00Z"
}
```

## Safety and Policy

Defaults:

- no shell execution
- no package installation
- timeout always set
- output bounded
- cwd constrained to project root
- watch paths constrained to project root
- commands executed as argv arrays

Optional future policy:

```toml
[policy]
allow_shell = false
allow_commands = ["pytest", "pnpm", "go", "cargo", "mvn", "gradle", "just", "make"]
deny_commands = ["rm", "curl", "wget", "ssh", "scp"]
require_project_config_for_watch = false
```

## Extraction Plan

### Phase 1 — Core cleanup

- Keep scenario ID parsing in core.
- Replace TDD-specific `Scenario.tdd_evidence` with generic scenario evidence.
- Move projected evidence path to `openspec/changes/<change>/evidence/`.

### Phase 2 — Extension skeleton

- Create `extensions/omegon-tdd-savepoint`.
- Add `manifest.toml`, README, Cargo.toml, and extension main.
- Expose status, presets, plan, run, and evidence tools.

### Phase 3 — Move kernel

- Move command hash, runner, git state, evidence, and config resolution from core prototype into extension modules.
- Keep tests with ecosystem-neutral commands.

### Phase 4 — Config resolver

- Implement built-in presets.
- Load `.omegon/tdd-savepoint.toml`.
- Parse scenario metadata overrides.
- Produce resolved config with source map.

### Phase 5 — Evidence projection and core read model

- Extension writes `scenario-evidence/v1` summaries.
- Core reads generic evidence from change evidence files.
- Lifecycle context reports evidence by provider/status.

### Phase 6 — Watch mode

- Implement background watcher handles or defer until host background execution is clearly supported.
- Do not block RPC indefinitely.

### Phase 7 — Armory packaging

- Add Armory registry metadata.
- Mirror `omegon-browser` release packaging once extension stabilizes.

## Risks and Mitigations

### Risk: Configuration opacity

Mitigation: `tdd_savepoint_plan` is mandatory and returns source mapping for every resolved field.

### Risk: Language presets become semantic adapters

Mitigation: presets only provide defaults. Pass/fail remains exit-code based in v1.

### Risk: Arbitrary command execution expands attack surface

Mitigation: argv-only execution, project-root path constraints, timeout, output bounds, optional allow/deny policy, no package installation.

### Risk: Watch mode blocks extension RPC

Mitigation: implement run/evidence/plan first; watch must return a handle or use host background session support.

### Risk: Scenario metadata becomes noisy

Mitigation: prefer project config for command mappings; scenario metadata should focus on IDs, risk, dependencies, tags, and exceptions.

## Acceptance Criteria

- Extension can resolve a scenario to a command using built-in preset, project config, metadata, and call overrides.
- `plan` shows resolved config and source map without mutation.
- `run` records bounded evidence from an argv command without shell execution.
- `evidence` returns status by scenario/change.
- Core can read provider-neutral scenario evidence summaries.
- Built-in presets cover Python, TypeScript/JavaScript, Go, Rust, Java, and generic Make/Just workflows.
- No extension tool blocks indefinitely.

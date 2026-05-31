+++
kind = "design_node"

[data]
title = "TDD Savepoint Extension Extraction"
status = "decided"
issue_type = "architecture"
priority = 1
dependencies = ["8d214819-082b-4742-8b4b-bcca1c528a9c", "scenario-ids-lifecycle-targets"]
open_questions = [
  "Should the extension ship as bundled in-tree first, or immediately as a separate Armory repository?",
  "Should project config live only at .omegon/tdd-savepoint.toml, or should OpenSpec-local config also be supported?"
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

### Decision: Configuration uses five-layer precedence

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

True background `tdd_savepoint_watch` is explicitly deferred until `plan`, `run`, and `evidence` are stable. Watch must not block JSON-RPC indefinitely. When implemented, it should either request a host background terminal/session or spawn an extension-owned worker process and return a watcher handle.

### Decision: Command execution is argv-only in v1

**Status:** decided

The initial extension execution policy is argv-only: no shell strings, no package installation, bounded stdout/stderr tails, explicit timeout, canonicalized `cwd`, and project-root path constraints. Shell mode can be reconsidered later only behind explicit project configuration and policy checks.

### Decision: Core CLI becomes a compatibility wrapper

**Status:** decided

Core must stop owning savepoint execution. The migration path is: try the `omegon-tdd-savepoint` extension first for bounded commands, fall back to the legacy core implementation while emitting a deprecation hint, then delete the core kernel once the extension run/evidence/watch path is stable.

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


## Execution Ownership Design

The next extraction boundary is command execution. Core should no longer grow `core/crates/omegon/src/tdd.rs`; that file is now legacy compatibility code until the extension proves equivalent behavior.

Ownership split:

| Area | Owner | Notes |
|---|---|---|
| OpenSpec parsing | core | Scenario identity remains lifecycle-neutral. |
| Scenario ID derivation | core | Explicit IDs and fallback IDs are not TDD-specific. |
| Generic evidence read model | core | Reads provider-neutral summaries from `evidence/*.jsonl`. |
| Command hashing | extension | Part of execution identity, not lifecycle identity. |
| Process spawning | extension | Bounded argv execution only. |
| Red/green capture | extension | Provider-specific evidence semantics. |
| Raw JSONL logs | extension | Stored under `.omegon/lifecycle/savepoints/` for now. |
| Projected scenario evidence | extension | Written under `openspec/changes/<change>/evidence/`. |
| Watch orchestration | extension | Deferred until bounded run/evidence are stable. |

### `tdd_savepoint_plan`

`plan` is the trust boundary. It resolves what would run without executing or mutating state. Every resolved field must report its source.

Initial v1 input should accept direct per-call values plus an optional built-in preset:

```json
{
  "cwd": ".",
  "change": "jwt-auth",
  "scenario": "auth/token-expired",
  "task": "2.1",
  "preset": "rust-cargo",
  "command": ["cargo", "test", "-p", "omegon", "auth_token_expired"],
  "watch_paths": ["core/crates/omegon/src/auth", "core/crates/omegon/tests"],
  "filetype": "rs",
  "timeout_secs": 60,
  "emit_baseline": true,
  "persist_failures": true,
  "current_diff_hash": true
}
```

Output shape:

```json
{
  "resolved": {
    "project_root": "/abs/project",
    "cwd": "/abs/project",
    "command": ["cargo", "test", "-p", "omegon", "auth_token_expired"],
    "command_hash": "sha256:...",
    "watch_paths": ["core/crates/omegon/src/auth", "core/crates/omegon/tests"],
    "filetype": "rs",
    "timeout_secs": 60,
    "emit_baseline": true,
    "persist_failures": true,
    "max_output_chars": 8192,
    "change": "jwt-auth",
    "scenario": "auth/token-expired",
    "task": "2.1"
  },
  "sources": {
    "command": "per-call",
    "watch_paths": "per-call",
    "filetype": "preset:rust-cargo",
    "timeout_secs": "per-call",
    "emit_baseline": "default",
    "persist_failures": "per-call",
    "max_output_chars": "default"
  },
  "warnings": []
}
```

Plan must fail if no command can be resolved. It should not run a command, append evidence, or start watchers.

### `tdd_savepoint_run`

`run` executes the resolved plan once and optionally records evidence. It is the first real transfer of execution ownership from core to extension.

Modes:

| Mode | Behavior |
|---|---|
| `record=false` | Execute only; return outcome, no evidence mutation. |
| `baseline=true` | Record a baseline event for the current outcome. |
| `persist_failures=true` and current fails | Record/dedupe a `fail` event. |
| current passes after prior red evidence | Record `failing_to_passing`. |

Non-watch red→green detection uses prior evidence rather than in-process watcher state:

```text
current command passes
AND prior matching evidence contains baseline/fail with non-zero exit
→ record failing_to_passing
```

The matching key is:

```text
command_hash + change? + scenario? + task?
```

If scenario is provided, it narrows the match. If not, command hash alone is acceptable for command-level evidence.

### Runner constraints

V1 execution policy:

- command must be an argv array; shell strings are rejected
- timeout is always set; default 60 seconds
- stdout/stderr tails are bounded; default 8192 characters
- explicit `cwd` canonicalizes to an existing directory
- project root is `git rev-parse --show-toplevel` when available
- run `cwd`, watch paths, raw event paths, and projected evidence paths must remain under project root
- no package installation or implicit dependency fetching

When moving runner code, replace byte-index string tailing with character-safe truncation:

```rust
fn tail_string(s: &str, max_chars: usize) -> String {
    let len = s.chars().count();
    if len <= max_chars {
        s.to_string()
    } else {
        s.chars().skip(len - max_chars).collect()
    }
}
```

## Core Compatibility Wrapper

Migration stages:

### Stage A — Legacy core kernel

Current state. `omegon tdd watch` and `omegon tdd evidence` call `crate::tdd` directly.

### Stage B — Extension-first bounded commands

Core CLI tries the extension first for bounded commands such as evidence and run. If the extension is missing or disabled, core falls back to legacy implementation and prints a deprecation hint:

```text
omegon tdd is moving to the omegon-tdd-savepoint extension.
Install/enable it with:
  omegon extension install ./extensions/omegon-tdd-savepoint
```

Migrate `evidence` before `watch` because it is bounded and low risk.

### Stage C — Remove execution from core

Once extension `plan`, `run`, and `evidence` are stable, delete process execution, command hashing, watcher orchestration, and raw event writing from core. Core keeps only CLI argument parsing, extension dispatch, and generic evidence reading.

### Stage D — Pure alias

Eventually, `omegon tdd ...` becomes a user-facing alias for extension tools. No TDD execution kernel remains in core.

## Next Implementation Sequence

1. Add `tdd_savepoint_plan` with defaults, built-in presets, per-call overrides, command hashing, source map, and no mutation.
2. Add `tdd_savepoint_run` with bounded argv execution, baseline/fail/red→green recording, and evidence classification.
3. Dispatch `omegon tdd evidence` through the extension first, falling back to legacy core.
4. Replace `Scenario.tdd_evidence` with provider-neutral scenario evidence summaries.
5. Implement watch handles only after bounded run/evidence prove stable.

Do not implement watch mode next. The next implementation step is `plan`, then `run`.

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

### Phase 3 — Move bounded execution

- Add `tdd_savepoint_plan` with defaults, built-in presets, per-call overrides, command hashing, and source mapping.
- Add `tdd_savepoint_run` with bounded argv execution, timeout, output tails, baseline/fail/red→green recording, and evidence classification.
- Keep tests with ecosystem-neutral commands.

### Phase 4 — Config resolver

- Load `.omegon/tdd-savepoint.toml`.
- Parse scenario metadata overrides.
- Produce resolved config with source map across all five layers.

### Phase 5 — Evidence projection and core read model

- Extension writes `scenario-evidence/v1` summaries.
- Core reads generic evidence from change evidence files.
- Lifecycle context reports evidence by provider/status.

### Phase 6 — Watch mode

- Implement watch only after `plan`, `run`, and `evidence` are stable.
- Prefer host background terminal/session support; otherwise spawn an extension-owned worker and return a watcher handle.
- Add list/stop tools if the extension owns watcher child processes.
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

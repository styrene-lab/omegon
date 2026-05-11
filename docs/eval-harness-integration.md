+++
id = "e8c2f19a-4d6c-4b8f-9e1a-3f7d2c5a8b90"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Evaluation Harness Integration Guide

How to thread new features through the omegon evaluation and testing
infrastructure. This is the reference for anyone adding capabilities
that need to be benchmarked, tested end-to-end, or validated in CI.

## Architecture Overview

Three tiers of testing, each with its own entry point:

```
Tier 1: Unit tests (cargo test -p omegon)
    └─ Inline #[test] and #[tokio::test] in src/**/*.rs
    └─ Run on every push via .github/workflows/test.yml
    └─ ~2200 tests, <10s wall clock

Tier 2: Integration tests (cargo test -p omegon --test <name>)
    └─ core/crates/omegon/tests/*.rs — blackbox process-level tests
    └─ Run in CI as separate steps (some gated by env vars)
    └─ 4 test binaries: daemon_serve, extension_install, live_upstream, sandbox

Tier 3: Benchmark evals (python3 scripts/benchmark_harness.py)
    └─ ai/benchmarks/tasks/*.yaml — task spec files
    └─ Runs omegon headlessly against a clean repo checkout
    └─ Measures tokens, turns, wall clock, acceptance test pass rate
    └─ Results in ai/benchmarks/runs/*.json
```

## Adding Unit Tests for New Features

**Pattern:** Tests go in `#[cfg(test)] mod tests { ... }` at the bottom
of the source file, or in a dedicated `integration_tests.rs` module
under the feature's directory (e.g., `sentry/integration_tests.rs`).

**Cross-module integration tests** (testing how multiple sentry
components interact) go in `src/sentry/integration_tests.rs`. This
module has access to all `pub(crate)` items in the binary crate.

**Key conventions:**
- Use `tempfile::tempdir()` for all filesystem state
- Use `StateDb::in_memory()` for sqlite-backed state
- Never rely on env vars for test isolation — use struct-level flags
  (env vars race when tests run in parallel)
- For flynt board tests, use the `create_flynt_db()` + `insert_flynt_task()`
  helpers in `integration_tests.rs`
- For task tree tests, use `create_task_file()` to write `.omegon/tasks/` files

**Example — testing a new sentry feature:**

```rust
// In src/sentry/integration_tests.rs

#[test]
fn my_new_feature_does_the_thing() {
    let tmp = tempfile::tempdir().unwrap();
    let state_db = Arc::new(StateDb::in_memory().unwrap());
    // ... set up fixtures, call the code, assert results
}
```

## Adding Blackbox Integration Tests

**When to use:** Testing process-level behavior — CLI args, daemon
startup, HTTP API contracts, signal handling.

**Pattern:** Create `core/crates/omegon/tests/my_feature_blackbox.rs`.

**Key conventions:**
- Resolve the omegon binary via `CARGO_BIN_EXE_omegon` env var
  (set automatically by `cargo test`)
- Use `Command::new(binary).env(...)` for process isolation
- Set `RUST_LOG=error` to reduce noise
- Set `OMEGON_HOME` to a tempdir to isolate config/state
- Set `OMEGON_NO_KEYRING=1` to avoid keychain prompts
- Implement `Drop` on wrapper structs to kill child processes
- Gate behind env vars if the test needs network or external services

**Add to CI** in `.github/workflows/test.yml`:
```yaml
- name: My feature blackbox tests
  run: cargo test -p omegon --test my_feature_blackbox -- --test-threads=1
```

## Adding Benchmark Task Specs

**When to use:** Measuring the agent's ability to accomplish a task,
comparing across models or harnesses, tracking token efficiency.

### Task spec format (YAML)

Create `ai/benchmarks/tasks/my-task.yaml`:

```yaml
id: my-task
kind: implementation  # or: diagnostic, research, refactor
repo: .               # relative to omegon root, or absolute
base_ref: main        # git ref for clean checkout
prompt: |
  <The exact prompt given to the agent>

acceptance:
  required:
    - "grep -q 'expected_string' path/to/file"
    - "cargo test -p my_crate 2>&1 | grep 'test result: ok'"
  optional:
    - "wc -l path/to/file | awk '{print ($1 < 200) ? \"ok\" : \"too long\"}'"
  failure_if:
    - "grep -q 'FIXME' path/to/file"

matrix:
  harnesses: [omegon, om]
  models: [anthropic:claude-sonnet-4-6]

budget:
  soft:
    max_turns: 20
    max_total_tokens: 1500000
    max_wall_clock_sec: 1200
  hard:
    max_turns: 40
    max_total_tokens: 3000000
    max_wall_clock_sec: 1800

success_files:
  - path/to/expected/output.rs
```

### Running a benchmark

```bash
# Single task, single harness
python3 scripts/benchmark_harness.py ai/benchmarks/tasks/my-task.yaml \
  --harness omegon \
  --model anthropic:claude-sonnet-4-6

# Slim mode
python3 scripts/benchmark_harness.py ai/benchmarks/tasks/my-task.yaml \
  --harness om

# Result appears in ai/benchmarks/runs/<timestamp>-my-task-omegon-*.json
```

### How the harness calls omegon

The `OmegonAdapter` runs:
```
cargo run -p omegon -- bench run-task \
  --prompt "<prompt>" \
  --usage-json /tmp/benchmark-usage-XXX.json \
  [--model <model>] \
  [--slim]
```

This is a headless single-prompt execution that writes token usage to
the JSON file. The harness then runs acceptance tests (shell commands)
against the working directory.

### Acceptance tests

Each entry in `acceptance.required` is a shell command. Exit 0 = pass,
non-zero = fail. The harness runs them in the clean repo after the
agent finishes. Use `grep`, `test -f`, `cargo test`, `python -c`, etc.

## Threading New Features Through Evals

When you add a capability (e.g., OpenAPI tools, code-act, model
routing), you want to know if it works in practice, not just in unit
tests. Here's the playbook:

### 1. Write a benchmark task that exercises the feature

Example for OpenAPI tools:
```yaml
id: openapi-integration
kind: implementation
repo: .
base_ref: main
prompt: |
  This project has an OpenAPI spec at .omegon/apis/petstore.yaml.
  Use the compiled API tools to list all pets, then create a new
  pet named "benchmark-dog". Verify the pet was created by listing
  again.
acceptance:
  required:
    - "test -f .omegon/apis/petstore.yaml"
```

Example for model routing:
```yaml
id: routing-cost-check
kind: diagnostic
repo: .
base_ref: main
prompt: |
  Check if CI passed on the last commit.
# With routing enabled, this should be classified as SIMPLE
# and routed to the light model.
```

### 2. Add feature-specific env vars to the harness

In `scripts/benchmark_harness.py`, the `benchmark_process_env()`
function controls what environment the agent sees. Add your feature's
env vars there:

```python
def benchmark_process_env(repo_path, clean_path, harness, task_id):
    env = os.environ.copy()
    env["OMEGON_CODE_ACT"] = "1"  # enable code-act for benchmarks
    # ... existing vars
    return env
```

### 3. Validate in CI (optional)

For features that should be continuously validated, add a task spec to
`ai/benchmarks/tasks/` and add a CI step:

```yaml
- name: Benchmark regression check
  run: |
    python3 scripts/benchmark_harness.py \
      ai/benchmarks/tasks/my-feature.yaml \
      --harness omegon \
      --model anthropic:claude-sonnet-4-6
    python3 scripts/validate_benchmarks.py
```

## Quick Reference: Test Commands

```bash
# All unit tests (~7s)
cargo test -p omegon

# Specific module
cargo test -p omegon -- sentry::integration_tests
cargo test -p omegon -- tools::openapi
cargo test -p omegon -- code_act

# With local-embeddings feature
cargo test -p omegon --features local-embeddings -- local_embedding

# Blackbox tests
cargo test -p omegon --test daemon_serve_blackbox
cargo test -p omegon --test extension_install_blackbox -- --test-threads=1

# Live provider smoke (requires API keys)
OMEGON_RUN_LIVE_UPSTREAM_TESTS=1 cargo test -p omegon --test live_upstream_smoke

# Benchmark
python3 scripts/benchmark_harness.py ai/benchmarks/tasks/<task>.yaml --harness omegon

# Lipstyk
lipstyk --threshold 50 --exclude-tests core/crates/

# License audit
cargo license --json > /tmp/lic.json && python3 scripts/license-audit.py -i /tmp/lic.json --summary
```

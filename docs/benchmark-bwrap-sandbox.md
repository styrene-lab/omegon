+++
id = "28d0df9f-09ce-407d-9d30-d77b0452e2ed"
kind = "document"
title = "Opt-in bwrap sandbox for benchmark adapter invocations"
status = "seed"
tags = ["benchmark", "harness", "sandbox", "isolation", "bwrap"]
aliases = ["benchmark-bwrap-sandbox"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = ["Network policy: keep network on (LLM APIs work, sandbox only protects host fs) or unshare-net (sandbox is real but adapters cannot reach providers)?", "Toolchain bind-mount detection: rust toolchain may live under ~/.rustup, /usr/local/cargo, linuxbrew, or system. How does the wrapper discover the right paths without hardcoding?", "Adapter HOME exposure: claude / pi adapters read config from ~/.claude and ~/.pi. Bind read-only? Bind read-write? Skip and force --no-config flags?", "macOS fallback: bwrap is Linux-only. Should sandbox: bwrap on macOS hard-fail, soft-fail (warn and continue unsandboxed), or be a parse-time validation error on the task spec?", "Test surface: how does CI exercise the sandboxed path on hosts without bwrap installed?"]
parent = "benchmark-redesign-task-and-eval-spec"
+++

# Opt-in bwrap sandbox for benchmark adapter invocations

## Status

Seed. Deferred from `feat/benchmark-harness-grading-v1` (the seven-commit
benchmark grading + matrix + regression-detection branch). The other Tier 3
items in that branch — moving the cargo target dir out of the source tree
(commit `8f6b65b2`) and surfacing telemetry availability in `--report` mode
(commit `b6236a76`) — are already merged-ready. This item is the only
roadmap entry from the original analysis that did not land in that branch.

## Overview

Add an opt-in `sandbox: bwrap | none` field to the benchmark task spec.
When set to `bwrap`, wrap the per-cell adapter invocation in `bwrap`
with a minimal profile that bind-mounts only the clean repo, the
benchmark cargo cache, and the bare minimum of system paths the
adapter needs. Default remains `none` so existing tasks are unaffected.

## Why this is opt-in, not default

The roadmap analysis that produced this branch is explicit on this
point: the original AI-evals recommendation was to wrap every run in
Podman or Firecracker microVMs, and the assessment rejected that
because:

1. Containerizing per cell would have broken the deliberate
   `CARGO_TARGET_DIR` cache-sharing optimization. (That cache has now
   been moved out of the source tree in commit `8f6b65b2`, which makes
   the cache concern less acute but does not eliminate it — a per-cell
   container would still need a careful bind-mount strategy.)
2. The realistic threat model is "agent accidentally writes to host
   paths outside the clean repo," not "untrusted RCE." A clean-room
   git clone plus the relocated cargo cache already covers most of
   that threat model.
3. The blast radius is small enough that mandatory containerization
   would be paying for paranoia the workload does not justify.

`bwrap` is the right tool for the residual concern: it is rootless,
daemonless, has near-zero startup cost, and composes cleanly with the
existing per-cell subprocess model. But making it the default would
inherit all the problems containerization had — bind-mount fragility,
toolchain detection, network-vs-isolation tradeoffs — without a task
that actually needs the protection. Opt-in lets a specific task that
asks for sandboxing get it without paying that cost on every benchmark
run.

## Research

### Threat model the sandbox actually addresses

Concrete things bwrap (with network on, HOME bind-mounted read-only,
clean repo and cargo cache bind-mounted read-write) would prevent:

- Agent shells out to `rm -rf ~/some-other-project`.
- Agent edits files outside the clean repo (e.g. modifies the operator's
  global git config, leaks state into a sibling repo).
- Cargo build mutates a target dir other than the configured one.
- Test command pollutes `/tmp` in a way that affects the host.

Things bwrap with network on does not prevent (and that should not be
in scope for v1):

- Agent calls out to LLM APIs and exfiltrates information.
- Agent makes outbound HTTP requests that mutate external state.
- Agent uses cargo install to fetch and run arbitrary crates.

Strict isolation (`--unshare-net`) is incompatible with the adapters
themselves: claude, omegon, and pi all need to call out to providers
to do anything. So v1's bwrap profile keeps network on. The protection
is a real but bounded host-filesystem guard, not a hostile-code jail.

### Adapter-specific bind-mount requirements

Each adapter has different host paths it needs:

- **omegon**: `cargo` (rust toolchain), source crates under the clean
  repo, `CARGO_TARGET_DIR` (already relocated by item 7), the omegon
  binary built from source (`cargo run -p omegon`).
- **claude-code**: `claude` binary in PATH, claude config under
  `~/.claude` (read for credentials).
- **pi**: `pi` binary in PATH, pi config under `~/.pi/agent`
  (`PI_CODING_AGENT_DIR`).

The wrapper needs a per-adapter bind-mount profile, not a single
one-size-fits-all profile. The cleanest factoring is probably an
`adapter.sandbox_paths()` method on `HarnessAdapter` that returns
the additional bind-mounts the adapter needs, and a wrapper that
composes the adapter-specific paths with a base profile.

### Toolchain discovery

The base profile needs to bind-mount the rust toolchain. On the
current dev box that lives at `/home/linuxbrew/.linuxbrew/...`, but
on a CI runner it might be `/usr/local/cargo` or `~/.rustup`. Two
discovery options:

1. **Resolve symlinks of `which("cargo")`** and bind the parent
   directory chain. Robust to install location, fragile to
   cargo wrappers.
2. **Run `rustc --print sysroot`** and bind that. Cleaner but adds
   a subprocess call to every benchmark run.

Both are viable; (2) is probably the right v1 choice because it gives
the right answer regardless of how cargo is installed.

### macOS

bwrap is Linux-only. Three viable behaviors when a task declares
`sandbox: bwrap` and the host is macOS:

- **hard-fail**: parse-time validation error. Cleanest signal but
  blocks operators who happen to be on macOS from running a
  bwrap-tagged task at all.
- **soft-fail**: warn and run unsandboxed. Convenient but quietly
  reduces the protection the task asked for. Risk: the operator
  thinks they got isolation and didn't.
- **adapter-validation error**: succeed at parse time, fail at
  `validate_environment()` like the existing missing-binary errors.
  Symmetric with how missing `claude` is handled today.

The third option is probably the right one — it matches the existing
adapter-validation pattern and surfaces the problem at the moment the
operator actually tries to run the task, not at parse time.

## Proposed task-spec surface

```yaml
# Opt-in. Default is "none" (no sandbox, current behavior).
sandbox: bwrap

# Optional: declare extra bind-mounts the task needs (e.g. fixture
# directories, system libraries the test uses). The base profile and
# adapter-specific profile are composed automatically.
sandbox_extra_binds:
  - /opt/some-fixture
```

## Implementation sketch

1. Add `sandbox` and `sandbox_extra_binds` fields to `TaskSpec` and
   to `load_task_spec`.
2. Add a `wrap_with_bwrap(cmd, *, clean_repo_path, cache_dir, adapter,
   extra_binds)` helper in `scripts/benchmark_harness.py` that returns
   the wrapped argv. If `which("bwrap") is None`, raise `AdapterError`.
3. Add an optional `sandbox_paths(self) -> list[str]` method to
   `HarnessAdapter` that subclasses override to declare adapter-specific
   bind-mounts. Default returns `[]`.
4. Update each adapter's `run()` method to wrap its `cmd` via
   `wrap_with_bwrap` when `spec.sandbox == "bwrap"`.
5. Adapter `validate_environment()` checks for bwrap availability when
   the task spec requests it, and fails with a clear message if absent
   (mirrors the existing `claude-code adapter requires 'claude' in PATH`
   pattern).
6. Tests:
   - Unit tests for `wrap_with_bwrap` argv shape (no actual bwrap
     invocation), gated to skip on hosts without bwrap installed.
   - Integration test that runs a fake adapter under bwrap (or skips on
     hosts without it) and asserts the adapter cannot read a file
     outside the bind-mounted set.
   - Parse-time tests for the new spec fields, including malformed
     values.

## Out of scope for this design node

- Podman or Firecracker microVMs. The original analysis explicitly
  rejected these as the wrong tool for the workload; revisiting that
  decision belongs in a separate design node, not here.
- Network isolation. v1 keeps network on; an `--unshare-net` mode
  would only be useful for tasks whose adapters do not call out to
  LLM providers, which is not the current adapter set.
- Multi-platform sandboxing. macOS/Windows users will get an
  adapter-validation error when a task declares `sandbox: bwrap`, and
  that is the v1 contract.

## Why this is deferred, not abandoned

Doing bwrap right is genuinely 100+ lines of code plus a real
test-harness-shaped problem (bwrap may not be installed on dev boxes,
the toolchain bind-mount logic is OS-and-install-shaped, the
network-vs-isolation tradeoff is real). The value relative to what
already shipped in `feat/benchmark-harness-grading-v1` is meaningful
but not load-bearing for the redesign-doc questions the rest of the
branch was answering.

The right shape for this work is its own branch, with its own focused
review and a chance to discuss the open questions above before the
implementation locks them in. This design node exists so the work is
not lost and so the next operator picking it up has the context the
deferral itself produced.

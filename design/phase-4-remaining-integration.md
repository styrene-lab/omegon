+++
id = "4e896968-cbb3-459f-8827-bf6520db9459"
kind = "design_node"
title = "Phase 4 — remaining integration work from Delfhos competitive analysis"
status = "research"
tags = ["code-act", "sandbox", "proxy", "skills", "integration"]
aliases = ["phase-4-remaining"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = [
    "Should code-act be a skill or a slash command or both?",
    "Is Unix socket the right IPC for the tool proxy, or should it be HTTP localhost?",
    "Should the OCI sandbox use the same container as --sandboxed or a lighter one?",
    "How should code-act interact with the permission system in interactive mode?",
]
parent = "omega"
priority = "3"
related = ["code-act-execution", "openapi-tool-compiler", "semantic-memory-embeddings", "dual-llm-model-routing"]
+++

# Phase 4 — remaining integration work

## Context

Four design nodes were implemented through Phases 1-3 based on the
Delfhos competitive analysis. Three deferred items remain — all are
integration-heavy and require deeper system plumbing than the core
features themselves.

## What's implemented (for reference)

| Feature | Phase 1 | Phase 2 | Phase 3 |
|---|---|---|---|
| OpenAPI Compiler | Compiler, provider, auth, wired | ETag cache, allow/confirm, auto-discovery | — |
| Semantic Memory | Local ONNX embedder, fallback chain | Download CLI, backfill command | Session-end extraction |
| Code-Act | Executor, prompts, permission gate | Sentry mode routing | Retry loop |
| Model Routing | Heuristic classifier, config | LLM prefilter, quick_completion, outcome tracking | Adaptive thresholds |

## Remaining items

### 1. Code-Act interactive trigger

**Problem:** Code-act is available in sentry (via `execution_mode = "code-act"`)
but not in interactive sessions. An operator who wants the agent to generate
a script instead of making tool calls has no way to request it.

**Options to research:**

A. **Slash command `/code-act <prompt>`** — routes through
   CanonicalSlashCommand → ControlRequest → agent loop. The agent loop
   would need a code-act execution path parallel to the normal tool-calling
   loop. Requires changes to: tui/mod.rs (command parsing), control_runtime.rs
   (dispatch), loop.rs (execution path).

B. **Skill file** — a `.omegon/skills/code-act/SKILL.md` that instructs the
   agent to generate code instead of using tools. Minimal code change (skill
   system already exists), but the agent would still use tool calls to execute
   the code (bash tool with the generated script). Less clean than native
   support but works today.

C. **Agent-initiated** — the agent loop detects when code-gen would be more
   efficient (batch operations, data transformations) and automatically
   switches. Requires a classification heuristic in the loop itself. Most
   sophisticated but hardest to get right.

**Recommendation:** Start with B (skill file) as an immediate unblock, then
build A for production use. C is a research problem.

**Estimated effort:** B = 1 hour (skill file + prompt). A = ~200 lines
(slash command + loop integration). C = research.

### 2. Unix socket tool proxy for code-act

**Problem:** Phase 1 code-act only has filesystem-local proxies (bash,
read_file, write_file). The generated script can't call web_search,
web_fetch, extension tools, or any tool that requires the omegon process.

**Design:**

The host omegon process starts a Unix socket listener in the temp directory
before executing the script. The Python prelude gains an `_omegon_rpc()`
function that sends JSON-RPC requests over the socket. The host dispatches
these through the normal tool execution pipeline (permissions, logging,
budget tracking).

```
┌─────────────┐     Unix socket      ┌─────────────┐
│  Python      │ ──── JSON-RPC ────▶ │  omegon      │
│  script      │ ◀── response ────── │  host        │
│  (sandbox)   │                      │  (tool       │
│              │                      │   dispatch)  │
└─────────────┘                      └─────────────┘
```

**Research questions:**
- Should the socket be in the `.omegon/` temp dir or a system temp dir?
- How to handle async tool calls from a sync Python script? (Python
  `socket.recv()` blocks; the host needs to handle concurrent requests
  if the script uses `asyncio.gather`)
- Timeout semantics: per-RPC-call timeout or inherit from the task timeout?
- Error format: should RPC errors be Python exceptions or return values?

**Estimated effort:** ~200 lines Rust (socket server + dispatcher) + ~50
lines Python (RPC client in prelude). Plus tests with a mock tool.

### 3. OCI sandbox for code-act

**Problem:** Code-act scripts run as bare `python3` subprocesses with the
omegon process's full privileges. The `--sandboxed` flag re-execs the
entire omegon binary in a container, but code-act needs a lighter approach:
run just the generated script in a container while the host process stays
native.

**Design:**

When `--sandboxed` is active or `OMEGON_CODE_ACT_SANDBOX=1` is set:
1. Build a minimal container image with Python 3 + the injected prelude
2. Bind-mount the workspace directory (read-write) and the Unix socket
3. Execute the script via `podman run` or `docker run` instead of `python3`
4. Collect stdout/stderr from the container exit

**Research questions:**
- Should the container image be pre-built and pulled, or built on-the-fly
  from a Dockerfile in the omegon distribution?
- How to handle Python package dependencies the script might need?
  (Generated code is constrained to stdlib, but edge cases exist)
- Network isolation: should the container have network access? The socket
  proxy provides tool access, but the script might need `pip install` or
  direct HTTP.
- Performance: container startup adds 0.5-2s per execution. Acceptable
  for sentry tasks but may feel slow for interactive use.

**Estimated effort:** ~80 lines (container exec wrapper, mount config).
Plus container image definition and CI integration for image builds.

## Implementation order

1. **Skill file for code-act** (immediate unblock, zero code)
2. **Unix socket tool proxy** (enables full tool access from scripts)
3. **Slash command /code-act** (production interactive trigger)
4. **OCI sandbox** (security hardening for untrusted scripts)

Items 1 and 2 are independent and can be parallelized. Items 3 and 4
depend on 2 (the proxy must exist before the slash command makes sense,
and the sandbox needs the proxy for tool access).

## Technical debt from adversarial review (Phase 3)

Resolved during Phase 3 hardening passes:

- **Session-end extraction ran in nested block_on** — moved to
  `tokio::spawn` fire-and-forget. No longer risks deadlock on
  single-threaded runtimes or during shutdown.
- **Extraction used session's primary model** — hardcoded to Haiku.
  Extraction is a ~200-token classification, not worth Opus tokens.
- **Fact parsing was untestable** — extracted into `parse_extracted_facts()`
  pure function. Handles numbered lists, bullets, NONE responses,
  short-line filtering. 4 unit tests.
- **Routing stats had no time window** — now 7-day sliding window.
  Old outcomes age out instead of dominating the statistics.
- **Routing was escalation-only** — `routing_adjustment()` now returns
  -1/0/+1. Complex tasks with >95% success rate on light model get
  de-escalated. All classes participate in adaptive adjustment.

### Known remaining debt

- **`quick_completion` creates a new bridge per call.** `auto_detect_bridge`
  resolves credentials and creates HTTP clients each time. In a sentry
  loop with 50 tasks, this means 50+ bridge constructions for the
  prefilter alone. Fix: add a bridge cache keyed on model_spec to
  `providers.rs`, or accept a pre-resolved bridge in `quick_completion`.
  **Impact:** ~100ms overhead per prefilter call. Acceptable for sentry
  (tasks take minutes), but would matter for interactive code-act where
  latency is visible.

- **Backfill command re-embeds all facts.** No per-fact check for
  existing embeddings. Acceptable since backfill is operator-initiated
  and runs once. A proper fix would require a new `MemoryBackend` method
  (`facts_without_embeddings`) or a join query.

- **OpenAPI auto-discovery reads the directory on every startup.**
  No caching of the directory listing. Negligible for typical projects
  (<10 spec files). Would matter if someone drops hundreds of specs
  in `.omegon/apis/`.

## Non-goals for this phase

- **Multi-language code-act** (shell, JavaScript, etc.) — Python-only
  is sufficient. Shell mode is a nice-to-have but not blocking.
- **Agent-initiated code-act switching** — research problem, not
  implementation work.
- **Code-act in ACP** — the ACP protocol doesn't have a code-gen mode.
  If needed, it can be exposed as a tool call that returns the script
  output.
- **Bridge caching** — the `quick_completion` bridge overhead is
  documented above but not in scope for Phase 4. It's a cross-cutting
  concern that should be addressed in the provider layer, not patched
  per-caller.

## Armory positioning

### Current state

- **Code-act skill**: bundled into the binary (`skills/code-act/SKILL.md`).
  Always available without armory installation. Does NOT appear in
  `omegon armory browse` since armory discovers skills from the upstream
  `omegon-armory` GitHub repo, not from bundled skills.

- **OpenAPI tools**: registered dynamically as a ToolProvider. Not visible
  in armory. Project-local by nature (`.omegon/openapi.toml` and
  `.omegon/apis/`), not ecosystem-shared.

- **Local embeddings**: binary feature flag (`--features local-embeddings`).
  Not an installable armory item — it's a compile-time capability.

- **Model routing**: sentry config (`[sentry.routing]`). Not an armory
  concern — it's runtime configuration, not an installable component.

### Actions needed for upstream armory repo

1. **Add `code-act` to `omegon-armory/skills/`** — create a `plugin.toml`
   entry so the skill appears in `omegon armory browse --kind skills`.
   The `InstalledState::skill()` already checks bundled skills, so it
   would show as "installed" automatically.

2. **Consider an `api-integrations` plugin category** — for distributing
   curated OpenAPI spec packages (e.g., "Stripe toolkit" with
   allow/confirm/read_only pre-configured). Not blocking, but would
   make the OpenAPI system discoverable through armory.

3. **No armory action needed for embeddings or routing** — these are
   runtime capabilities, not installable ecosystem items.

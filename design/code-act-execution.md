+++
id = "7e1a96a9-576a-4003-9c2f-0890a5e5ae5e"
kind = "design_node"
title = "Code-act execution mode — LLM generates executable scripts instead of tool calls"
status = "decided"
tags = ["agent-loop", "execution", "code-gen", "sandbox", "tools"]
aliases = ["code-act-execution"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = ["language choice (Python vs shell vs both)", "tool proxy injection mechanism"]
parent = "omega"
priority = "2"
related = ["rust-agent-loop", "autonomous-tasking"]
+++

# Code-act execution mode — LLM generates executable scripts instead of tool calls

## Problem

Sequential tool calling forces a round-trip to the LLM for every decision point. "Read all PRs, filter by label, check CI status for each, summarize failures" requires 4+ turns with intermediate tool results fed back. The LLM already knows the full plan — it just can't express it in one shot because the tool-calling protocol is inherently sequential.

Code generation collapses this into a single LLM call that produces an executable script with loops, conditionals, parallel execution, and error handling. Delfhos proves this works: their entire execution model is code-gen, and it enables `asyncio.gather()` for parallel tool calls, try/except for graceful degradation, and variable binding between steps — all impossible in standard tool-calling.

## Design

### Opt-in execution mode

Code-act is **not** a replacement for tool calling. It's an additional execution mode available per-task or per-skill:

```toml
# .omegon/tasks/batch-pr-review.md
+++
id = "batch-pr-review"
title = "Batch PR review"
status = "todo"

[execution]
mode = "code-act"       # NEW — default is "tool-call"
language = "python"     # or "shell"
model = "anthropic:claude-sonnet-4-6"
max_turns = 5
+++
```

Or via the sentry config:
```toml
[[task]]
name = "data-pipeline"
prompt = "..."
execution_mode = "code-act"
```

### Execution flow

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Build prompt │────▶│ LLM generates│────▶│  Execute in  │
│  with tool    │     │ Python/shell │     │  sandbox     │
│  API docs     │     │ script       │     │  container   │
└──────────────┘     └──────────────┘     └──────────────┘
                                                │
                                           ┌────▼────┐
                                           │ Collect  │
                                           │ output + │
                                           │ artifacts│
                                           └─────────┘
```

1. **Prompt construction** — inject available tool APIs as Python function signatures + docstrings (not JSON schema). Include the task prompt, relevant context, and code-gen rules.

2. **Code generation** — single LLM call produces a complete script. The script uses injected helper functions that proxy back to omegon's actual tool implementations.

3. **Sandbox execution** — run the script inside the existing OCI sandbox (same infrastructure as `--sandboxed` mode). The tool proxies communicate back to the host omegon process via a Unix socket or HTTP bridge.

4. **Output collection** — capture stdout, stderr, exit code, and any files written to the workspace.

5. **Retry loop** — if execution fails, feed the error + partial output back to the LLM for regeneration (up to `max_turns` attempts).

### Tool proxy injection

For Python code-act mode, inject a prelude that provides tool proxies:

```python
# Auto-injected by omegon code-act runtime
import json, os, subprocess

def bash(command: str) -> str:
    """Run a shell command and return stdout."""
    result = subprocess.run(command, shell=True, capture_output=True, text=True)
    return result.stdout

def read_file(path: str) -> str:
    """Read a file and return its contents."""
    with open(path) as f:
        return f.read()

def write_file(path: str, content: str) -> None:
    """Write content to a file."""
    with open(path, 'w') as f:
        f.write(content)

def web_search(query: str) -> str:
    """Search the web and return results."""
    # Proxied back to omegon's web_search tool via Unix socket
    return _omegon_rpc("web_search", {"query": query})

def web_fetch(url: str) -> str:
    """Fetch a URL and return its content."""
    return _omegon_rpc("web_fetch", {"url": url})
```

The `_omegon_rpc()` bridge sends tool calls back to the host process, which executes them through the normal tool dispatch pipeline (permissions, logging, etc.).

For shell code-act mode, inject helper scripts and environment variables that provide equivalent functionality.

### When to use code-act

**Good fit:**
- Batch operations over collections (review all PRs, process all files matching a pattern)
- Data transformation pipelines (read from API, transform, write to another)
- Tasks requiring conditional logic between tool calls
- Tasks where the LLM can express the full plan upfront

**Poor fit:**
- Interactive/conversational tasks
- Tasks requiring human approval at each step
- Tasks where the plan depends on intermediate results that can't be predicted
- Simple single-tool-call tasks (overhead of code-gen not justified)

### Safety

- All code runs inside the OCI sandbox — no host filesystem access, no network beyond what the proxy allows
- Tool proxies enforce the same permission model as direct tool calls
- Generated code is logged for audit
- Budget enforcement applies to both the LLM call and any proxied tool calls
- The sandbox has a hard timeout matching the task's `timeout_secs`

## Scope

### Phase 1: Python code-act with shell tool proxies
- Code-act prompt construction with tool API docs as Python signatures
- Script extraction from LLM response (fenced code block parsing)
- OCI sandbox execution with stdout/stderr capture
- `bash()`, `read_file()`, `write_file()` proxies (filesystem-local, no RPC needed)
- Error → retry loop
- ~400 lines: prompt builder, executor, output collector

### Phase 2: Full tool proxy bridge
- Unix socket RPC bridge between sandbox and host process
- Proxy all registered tools (web_search, web_fetch, extension tools)
- Permission enforcement through the proxy layer
- ~300 lines: bridge server, proxy codegen

### Phase 3: Shell mode + skill integration
- Shell code-act (bash script generation)
- Skill-level opt-in (`code-act` skill that switches mode)
- Sentry integration (tasks can specify `execution_mode`)
- ~200 lines

## Critical files

| File | Purpose |
|---|---|
| `src/code_act/mod.rs` | Module root, CodeActExecutor |
| `src/code_act/prompt.rs` | Tool API → Python signature generation |
| `src/code_act/sandbox.rs` | OCI execution + output collection |
| `src/code_act/proxy.rs` | Tool proxy bridge (Phase 2) |
| `src/code_act/prelude.py` | Injected Python helper functions |

## Dependencies

No new dependencies. Uses existing OCI sandbox infrastructure, existing tool dispatch, existing LLM providers.

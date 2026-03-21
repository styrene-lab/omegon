---
id: functional-plugins
title: Functional plugins — code-backed skills with tools, endpoints, and runtime logic
status: implemented
parent: persona-system
tags: [plugins, architecture, extensions, tools, mcp, code]
open_questions: []
issue_type: feature
priority: 2
---

# Functional plugins — code-backed skills with tools, endpoints, and runtime logic

## Overview

Markdown-only plugins (persona/tone/skill) are passive — they inject context. Functional plugins have executable code: tools backed by HTTP endpoints, WASM modules, or subprocess scripts. The question: how does someone write a plugin that *does* something, not just *says* something? This bridges the existing HTTP plugin system (plugin.toml with tools/endpoints) and the new armory manifest format.

## Research

### The spectrum: passive → functional → autonomous

Plugins exist on a spectrum of capability:

**Passive (what we have)**
- Persona: PERSONA.md + mind/facts.jsonl → context injection only
- Tone: TONE.md + exemplars → prompt modification only
- Skill: SKILL.md → guidance injection only
- Zero code, zero runtime, zero risk.

**Functional (what we need)**
- Tool-bearing: exposes tools the agent can call (HTTP, subprocess, WASM)
- Context-producing: generates dynamic context at runtime (not static markdown)
- Event-reacting: listens for agent events and takes action
- Has code, has runtime, needs sandboxing.

**Autonomous (future)**
- Sub-agent: spawns its own agent loop for delegated work
- Long-running: maintains state across sessions
- This is the Omega coordinator tier — out of scope here.

The existing HTTP plugin system (`manifest.rs` + `http_feature.rs`) already handles functional plugins via HTTP endpoints. The gap is: **there's no easy way to write one.** An operator shouldn't need to run a separate HTTP server just to add a tool that reads a CSV file.

### Three execution models for functional plugins

**1. Script-backed tools (simplest)**

A tool defined in plugin.toml that runs a local script when invoked:

```toml
[plugin]
type = "extension"
id = "dev.example.csv-analyzer"
name = "CSV Analyzer"
version = "1.0.0"
description = "Analyze CSV files with pandas"

[[tools]]
name = "analyze_csv"
description = "Run statistical analysis on a CSV file"
runner = "python"
script = "tools/analyze.py"
parameters = { type = "object", properties = { path = { type = "string" }, query = { type = "string" } }, required = ["path"] }
timeout_secs = 30
```

The harness spawns `python tools/analyze.py` with args as JSON on stdin, reads JSON result from stdout. Simple, language-agnostic, zero infrastructure.

**2. HTTP-backed tools (existing)**

```toml
[[tools]]
name = "scribe_status"
description = "Get engagement status"
endpoint = "http://localhost:3000/api/status"
method = "GET"
```

Requires the operator to run a server. Good for services that are already running (Scribe, Jira, etc). Not good for one-off tools.

**3. WASM-backed tools (future, sandboxed)**

```toml
[[tools]]
name = "diagram_render"
description = "Render a D2 diagram to SVG"
runner = "wasm"
module = "tools/diagram.wasm"
```

Sandboxed execution, portable, no external runtime. Requires WASM toolchain for authors. Best for compute-intensive or security-sensitive operations.

**Recommendation**: Script-backed tools are the 80% solution. They're what Claude Code plugins would be if Claude Code had an open plugin system. The operator writes a Python/Node/Bash script, declares it in plugin.toml, done.

The stdin/stdout JSON contract:
- Input: `{"path": "/data/sales.csv", "query": "mean revenue by quarter"}`  
- Output: `{"result": "...", "error": null}` or `{"result": null, "error": "..."}`
- Exit code 0 = success, non-zero = error
- Timeout enforced by harness (default 30s)

WASM is the long-term play but script-backed gets us 80% now.

### Unified plugin.toml covers all types

The beauty of the unified manifest: a single plugin can be both passive AND functional. A PCB designer persona can bundle markdown guidance AND executable tools:

```toml
[plugin]
type = "persona"
id = "dev.styrene.omegon.pcb-designer"
name = "PCB Designer"
version = "1.0.0"
description = "PCB design persona with KiCad integration"

[persona.identity]
directive = "PERSONA.md"

[persona.mind]
seed_facts = "mind/facts.jsonl"

# Functional: tools the agent can call
[[tools]]
name = "drc_check"
description = "Run KiCad Design Rule Check on the current PCB"
runner = "python"
script = "tools/drc_check.py"
parameters = { type = "object", properties = { pcb_path = { type = "string" } }, required = ["pcb_path"] }
timeout_secs = 60

[[tools]]
name = "bom_export"
description = "Export Bill of Materials from schematic"
runner = "python"
script = "tools/bom_export.py"
parameters = { type = "object", properties = { sch_path = { type = "string" }, format = { type = "string", enum = ["csv", "json"] } } }

# Dynamic context: refresh component library status on session start
[context]
runner = "python"
script = "context/library_status.py"
ttl_turns = 50

[detect]
file_patterns = ["*.kicad_pcb", "*.kicad_sch", "*.kicad_pro"]
```

This is one plugin.toml, one install command, one repo. The operator gets: domain expertise (PERSONA.md), domain knowledge (mind/facts.jsonl), and domain tools (DRC check, BOM export) — all from `omegon plugin install https://github.com/someone/pcb-designer`.

The `runner` field distinguishes script-backed from HTTP-backed:
- `runner = "python"` + `script = "..."` → subprocess with JSON stdin/stdout
- `runner = "node"` + `script = "..."` → same pattern, Node.js
- `runner = "bash"` + `script = "..."` → shell script
- No `runner` + `endpoint = "..."` → HTTP call (existing behavior)
- `runner = "wasm"` + `module = "..."` → future WASM execution

### OCI containers as a tool runner — answering deps and sandboxing

OCI containers solve both open questions:

**Dependencies**: declared in the Containerfile, frozen in the image. No `requirements.txt` parsing, no venv management, no "works on my machine." The image IS the dependency declaration.

**Sandboxing**: containers run isolated by default — no host filesystem access unless explicitly mounted, no network unless explicitly allowed, resource limits via cgroup. The operator chooses the isolation level:
- `--mount=cwd` → mount the current working directory (most tools need this)
- `--network=none` → air-gapped execution (security tools, offline analysis)
- `--network=host` → full network (API calls, web scraping)

**The runner model**:

```toml
[[tools]]
name = "drc_check"
description = "Run KiCad Design Rule Check"
runner = "oci"
image = "ghcr.io/styrene-lab/omegon-tool-kicad-drc:latest"
# Or build from local Containerfile
build = "tools/drc/Containerfile"
mount_cwd = true
network = false
timeout_secs = 120
```

**Contract**: same JSON stdin/stdout as script-backed tools. The harness:
1. Pulls/builds the image if needed (cached)
2. Runs `podman run` (or `docker run`) with the configured mounts/network
3. Pipes tool arguments as JSON to stdin
4. Reads JSON result from stdout
5. Enforces timeout via `--stop-timeout`

**Why podman over docker**: rootless by default, no daemon, OCI-compliant, better security posture. Falls back to docker if podman isn't available.

**Cross-platform payoff**: a Linux-only tool (KiCad CLI, EDA tools, system analyzers) runs on macOS via podman machine. A Windows-only tool runs via WSL2 container. The plugin author builds once, the operator runs anywhere.

**Image distribution**: published to any OCI registry (GHCR, Docker Hub, private). The plugin.toml declares the image URI. `omegon plugin install` pulls the image on first activation. Updates via standard image tag management.

**Build from source**: for development/customization, the plugin can include a Containerfile. `omegon plugin build <id>` builds the image locally. The Containerfile is committed to the plugin repo alongside the tool script.

```
my-pcb-tools/
├── plugin.toml
├── PERSONA.md
├── tools/
│   ├── drc_check/
│   │   ├── Containerfile      ← builds the tool image
│   │   ├── drc_check.py       ← the actual tool code
│   │   └── requirements.txt   ← frozen in the image
│   └── bom_export/
│       ├── Containerfile
│       └── bom_export.py
└── mind/
    └── facts.jsonl
```

## Decisions

### Decision: OCI containers are a first-class tool runner alongside script/HTTP/WASM

**Status:** decided
**Rationale:** Containers solve dependencies (frozen in image), sandboxing (isolated by default), and cross-platform (Linux tools run on macOS via podman machine). Same JSON stdin/stdout contract as script-backed tools. runner='oci' with image URI or local Containerfile build path. Podman preferred (rootless, daemonless), docker fallback. Plugin can declare mount_cwd, network, timeout per tool.

### Decision: Dependencies declared in Containerfile, not managed by omegon

**Status:** decided
**Rationale:** Omegon should not be a package manager. For script-backed tools, the operator manages their own Python/Node environment. For OCI tools, the Containerfile IS the dependency declaration — requirements.txt, apt packages, etc. are frozen in the image layer. This is the clean boundary: omegon manages plugin lifecycle, the image manages runtime dependencies.

### Decision: Sandboxing via container isolation — operator controls mount and network policy per tool

**Status:** decided
**Rationale:** Containers provide natural sandboxing. Script-backed tools trust the operator (they run on the host). OCI tools are isolated by default — no host access unless mount_cwd=true, no network unless network=true. This gives a clean security gradient: passive plugins (zero risk) → script tools (operator trust) → OCI tools (sandboxed) → WASM tools (fully sandboxed, future).

### Decision: Context scripts run once at load time, not per-turn

**Status:** decided
**Rationale:** The Feature::provide_context() method is synchronous — can't spawn subprocesses. Running a script per-turn would be expensive and add latency. Instead, context is generated once at plugin load (async from_manifest) and cached. ttl_turns controls how long it persists in the context budget. If fresh context is needed, the plugin provides a tool that the agent can call explicitly.

### Decision: Local plugins symlinked, not copied — development mode

**Status:** decided
**Rationale:** Symlinks let developers edit plugin source and see changes immediately without reinstalling. The symlink marker (→) in `plugin list` makes the mode visible. On Windows, falls back to directory copy since symlinks require admin privileges.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/plugins/armory_feature.rs` (new) — ArmoryFeature — Feature impl for script-backed (Python/Node/Bash) and OCI container tools. JSON stdin/stdout contract, timeout+cancellation, parse_tool_output(). 12 tests.
- `core/crates/omegon/src/plugins/armory.rs` (modified) — Added Clone derive to ToolEntry (needed by ArmoryFeature::from_manifest filter+collect)
- `core/crates/omegon/src/plugins/mcp.rs` (modified) — Made detect_container_runtime() pub(crate) for reuse by OCI tool runner
- `core/crates/omegon/src/plugins/mod.rs` (modified) — Wired ArmoryFeature into load_armory_plugin() — armory manifests with script/OCI tools now create functional features
- `core/crates/omegon/src/plugins/armory_feature.rs` (modified) — Added generate_context() for [context] section — script/HTTP runner, 15s/10s timeout, cached at load time. provide_context() serves cached output with priority 60, ttl_turns from manifest. Context-only plugins (no tools) now create features.
- `core/crates/omegon/src/plugin_cli.rs` (new) — Plugin lifecycle CLI — install (git clone or local symlink), list (table with type/version/description), remove (symlink or dir), update (git pull --ff-only). 12 tests.
- `core/crates/omegon/src/main.rs` (modified) — Added Plugin subcommand with Install/List/Remove/Update actions, wired to plugin_cli module

### Constraints

- Script tools spawn subprocess in plugin_root directory with JSON on stdin
- OCI tools deny network by default (--network=none unless network=true)
- OCI mount uses :Z suffix for SELinux compatibility
- Container runtime is lazy-detected (OnceLock) — only probed if OCI tools exist
- HTTP-only tools in armory manifests are excluded — handled by HttpPluginFeature

+++
id = "0a76705c-865f-40a6-b149-1f73d7c11d9a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Functional plugins — code-backed skills with tools, endpoints, and runtime logic — Design Spec (extracted)

> Auto-extracted from docs/functional-plugins.md at decide-time.

## Decisions

### OCI containers are a first-class tool runner alongside script/HTTP/WASM (decided)

Containers solve dependencies (frozen in image), sandboxing (isolated by default), and cross-platform (Linux tools run on macOS via podman machine). Same JSON stdin/stdout contract as script-backed tools. runner='oci' with image URI or local Containerfile build path. Podman preferred (rootless, daemonless), docker fallback. Plugin can declare mount_cwd, network, timeout per tool.

### Dependencies declared in Containerfile, not managed by omegon (decided)

Omegon should not be a package manager. For script-backed tools, the operator manages their own Python/Node environment. For OCI tools, the Containerfile IS the dependency declaration — requirements.txt, apt packages, etc. are frozen in the image layer. This is the clean boundary: omegon manages plugin lifecycle, the image manages runtime dependencies.

### Sandboxing via container isolation — operator controls mount and network policy per tool (decided)

Containers provide natural sandboxing. Script-backed tools trust the operator (they run on the host). OCI tools are isolated by default — no host access unless mount_cwd=true, no network unless network=true. This gives a clean security gradient: passive plugins (zero risk) → script tools (operator trust) → OCI tools (sandboxed) → WASM tools (fully sandboxed, future).

## Research Summary

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
- Event-reacting: listens for agent event…

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
parameters = { type = "object", properties = { path = { type = "string" }, query = { type = "string" } }…

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
desc…

### OCI containers as a tool runner — answering deps and sandboxing

OCI containers solve both open questions:

**Dependencies**: declared in the Containerfile, frozen in the image. No `requirements.txt` parsing, no venv management, no "works on my machine." The image IS the dependency declaration.

**Sandboxing**: containers run isolated by default — no host filesystem access unless explicitly mounted, no network unless explicitly allowed, resource limits via cgroup. The operator chooses the isolation level:
- `--mount=cwd` → mount the current working directory …

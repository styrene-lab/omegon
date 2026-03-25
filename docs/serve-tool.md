---
id: serve-tool
title: serve tool — long-lived background process management
status: exploring
tags: [tools, ux, webdev]
open_questions:
  - "Should MCP server discovery be automatic (detect stdio/http MCP protocol on start) or explicit (--mcp flag)?"
  - "TUI visualization: activity light per service in the instrument panel, or a dedicated services row in the footer, or both?"
jj_change_id: zosuwkpvpspkxktroknoupulskymmxvk
---

# serve tool — long-lived background process management

## Overview

A structured tool for running long-lived background processes (dev servers, watchers, build daemons) that survive bash command timeouts. The agent can start, stop, list, and check logs of background services. Uses launchctl on macOS, systemd --user on Linux, or a simple PID-file daemon manager as fallback. Key use case: web development workflows where `npm run dev` / `astro dev` / `vite` need to stay running while the agent does other work.

## Research

### TUI visualization options

Three candidates for surfacing long-running services:\n\n1. **Instrument panel row** — a compact row in the CIC panel showing service names with colored status dots (green=running, red=dead, amber=starting). Fits the existing telemetry pattern. Could pulse/animate when a service has recent log activity.\n\n2. **Footer indicator** — a small segment in the footer bar like `[astro:4321 ●]` next to the model/context indicators. Minimal footprint, always visible.\n\n3. **Both** — footer shows count + activity light (`3 svc ●●●`), instrument panel shows details on focus.\n\nFor MCP servers specifically, the activity light could track tool invocations — dim when idle, bright when the agent is actively using MCP tools from that server. This naturally integrates with the existing signal-density bar chars (≋ ≈ ∿ ·) for recency.

## Decisions

### Decision: Single tool with subcommands: serve(action, command, name, ...)

**Status:** decided
**Rationale:** Matches manage_ollama pattern. Actions: start, stop, list, logs, check. Keeps tool count minimal. Name is optional — auto-generated from command if not provided.

### Decision: PID file + log file daemon manager, no platform-specific service managers

**Status:** decided
**Rationale:** launchctl/systemd are overkill and have PATH/env issues. A simple approach: fork the process, write PID to ~/.config/omegon/serve/{name}.pid, redirect stdout/stderr to {name}.log. Check liveness via kill -0. Works everywhere. The key insight is we just need the process to outlive the bash tool's timeout — we don't need OS-level service management.

### Decision: Services auto-stop on session exit by default, persist with --persist flag

**Status:** decided
**Rationale:** Most dev servers should die when the session ends. But sometimes you want them to survive (e.g. running a preview build while you work in another session). Default is cleanup; opt-in to persistence.

### Decision: Serve as MCP server lifecycle manager — pull, start, discover tools, use, stop

**Status:** exploring
**Rationale:** If serve can manage any long-lived process, it can manage MCP servers. The flow would be: serve start --mcp npx @modelcontextprotocol/server-github, which starts the process, detects it speaks MCP (stdio or streamable-http), runs tools/list to discover capabilities, and registers them as available tools for the session. On session exit, the server is stopped. This replaces the static mcp.json configuration with a dynamic, agent-driven approach. Combined with OCI, the agent could pull an MCP server container and stand it up on demand.

## Open Questions

- Should MCP server discovery be automatic (detect stdio/http MCP protocol on start) or explicit (--mcp flag)?
- TUI visualization: activity light per service in the instrument panel, or a dedicated services row in the footer, or both?

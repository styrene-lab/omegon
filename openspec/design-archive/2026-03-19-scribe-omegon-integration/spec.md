+++
id = "cc6c1fe3-1113-4eec-bc0a-3d6564622e78"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Scribe × Omegon integration — wrapper harness vs bolt-on extension — Design Spec (extracted)

> Auto-extracted from docs/scribe-omegon-integration.md at decide-time.

## Decisions

### Bolt-on plugin + WebSocket consumer (Option B + H), not wrapper (exploring)

The wrapper destroys the TUI, couples release cycles, bloats the binary, and solves a problem (IPC elimination) that doesn't exist (10ms localhost calls vs 5-30s LLM calls). The bolt-on plugin pattern is language-agnostic, composable, and reusable for any company-specific integration. The WebSocket protocol was designed exactly for this use case.

### Bolt-on plugin + WebSocket consumer (Option B + H), not wrapper (decided)

The wrapper destroys the TUI (12k LoC), couples release cycles, bloats the binary 3-4x, and eliminates ~10ms of IPC that's irrelevant against 5-30s LLM calls. The bolt-on pattern is language-agnostic (plugin.toml + HTTP), composable, independently releasable, and reusable for any company-specific integration. The WebSocket protocol was explicitly designed for external UI consumers.

## Research Summary

### What Scribe actually is

Scribe is a Dioxus 0.7 fullstack app (SSR + WASM hydration) for Recro's engagement management. It tracks:

- **Partnerships** (Active/Inactive/Archived) — linked to GitHub org repos
- **Engagements** (Draft/Active/Completed/OnHold/Cancelled) — with start/end dates, git project URLs (GitHub/Azure DevOps/GitLab)
- **Session logs** — timestamped entries for each engagement
- **Team members** — with roles (Lead/Senior/Mid/Junior)
- **Engagement logs** — work logs, status updates, timeline events

Da…

### Option W: Wrapper harness — Scribe embeds Omegon

**Concept**: Scribe becomes the outer binary. It runs Dioxus fullstack as the web UI AND spawns/embeds the Omegon agent loop as a library. The Dioxus web UI provides both Scribe's engagement management AND the agent conversation interface. Scribe's server process handles the LLM streaming, tool execution, and memory — all within the same process.

**What this requires technically**:

1. **Omegon as a library crate** — extract the agent loop, bus, features, providers into `omegon-core` (lib) sepa…

### Option B: Bolt-on — Omegon loads a Scribe plugin

**Concept**: Omegon remains the standalone `oa` binary. Scribe integration is a plugin — a manifest file + optional sidecar — that omegon discovers and loads. When omegon runs in a directory with a `.scribe` marker, it activates Scribe context enrichment and tools.

**Plugin manifest**: `~/.omegon/plugins/scribe/plugin.toml`
```toml
[plugin]
name = "scribe"
version = "0.1.0"
description = "Recro engagement tracking"

[activation]
# Plugin activates when any of these are true
marker_files = [".sc…

### Option H: Hybrid — Scribe's web UI connects to Omegon via WebSocket

**Concept**: Neither wraps the other. Both run independently. Scribe's Dioxus UI adds an "Agent" tab that connects to omegon's WebSocket protocol on localhost:7842. The agent conversation happens inside Scribe's web UI but the agent process is standalone omegon.

This is actually the **existing architecture** — omegon already exposes a WebSocket protocol with 15 event types and 4 command types. The embedded dashboard.html is already a consumer of this protocol. Scribe's Dioxus UI would be a seco…

### Assessment — unbiased

**The wrapper approach (Option W) is wrong for this system.**

Here's why, without hedging:

**1. The value proposition is inverted.** Omegon is a general-purpose agent harness. Scribe is a company-specific engagement tracker. Wrapping the general tool inside the specific tool means the general tool can't exist without the specific tool. Every non-Recro user (including you using omegon on personal projects) would need to run Scribe to get an agent. That's architecturally backwards.

**2. The TUI…

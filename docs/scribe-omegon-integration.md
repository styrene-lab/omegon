+++
id = "33e44f91-fd28-4771-ab47-ce04bcd0d323"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Scribe × Omegon integration — wrapper harness vs bolt-on extension

## Overview

Assess whether Scribe should wrap Omegon as a 'harness harness' (Dioxus app embedding the agent loop) or bolt onto it as an external plugin. The answer determines the extension architecture for all company-specific integrations.

## Research

### What Scribe actually is

Scribe is a Dioxus 0.7 fullstack app (SSR + WASM hydration) for Recro's engagement management. It tracks:

- **Partnerships** (Active/Inactive/Archived) — linked to GitHub org repos
- **Engagements** (Draft/Active/Completed/OnHold/Cancelled) — with start/end dates, git project URLs (GitHub/Azure DevOps/GitLab)
- **Session logs** — timestamped entries for each engagement
- **Team members** — with roles (Lead/Senior/Mid/Junior)
- **Engagement logs** — work logs, status updates, timeline events

Data model: SQLite via sqlx, synced from GitHub repos via octocrab. Each engagement repo has an empty `.scribe` marker file. The Scribe server runs background sync every 15 minutes, pulling repo metadata, commit activity, and PR status into the local DB.

Server functions (Dioxus `#[server]`): `get_partnerships`, `get_engagements`, `get_engagement_summary`, `get_engagement_timeline`, `create_log_entry`, `update_log_entry`, `trigger_sync`.

Build: `dx serve` → produces a single server binary + WASM client assets. Deployed as a container to k8s. Separate from omegon entirely — different repo, different deployment, different runtime.

### Option W: Wrapper harness — Scribe embeds Omegon

**Concept**: Scribe becomes the outer binary. It runs Dioxus fullstack as the web UI AND spawns/embeds the Omegon agent loop as a library. The Dioxus web UI provides both Scribe's engagement management AND the agent conversation interface. Scribe's server process handles the LLM streaming, tool execution, and memory — all within the same process.

**What this requires technically**:

1. **Omegon as a library crate** — extract the agent loop, bus, features, providers into `omegon-core` (lib) separate from the binary's `main.rs` + TUI. The library would expose `AgentLoop::new(config) → run(prompt)` or similar.

2. **Dioxus ↔ Agent bridge** — Scribe's Dioxus server functions would call into the omegon agent loop. The `#[server]` functions would need to drive streaming responses back to the Dioxus WASM client via server-sent events or WebSocket (Dioxus has `use_server_future` but it's request-response, not streaming).

3. **TUI replacement** — The ratatui TUI would be unused. All agent interaction happens through Scribe's web UI. The editor, conversation view, dashboard, footer, splash — all replaced by Dioxus components.

4. **Single binary** — Scribe + Omegon compile into one binary. The dependency graph merges: Dioxus + ratatui + axum (Scribe's) + axum (Omegon's) + SQLite (Scribe's) + SQLite (Omegon memory's) + syntect + tachyonfx + ratatui-image + octocrab + ... Binary size: probably 30-50MB.

**What breaks**:

- **The TUI is dead** — 12,000+ LoC of ratatui work (conversation widget, segments, editor, footer, dashboard, splash, effects, images, spinner) becomes dead code. Omegon-as-TUI ceases to exist. You can only use the agent through the Scribe web UI.

- **Terminal-first workflow destroyed** — The entire value proposition of omegon (`oa interactive` from any terminal) goes away. You'd need a browser open to use the agent. For SSH sessions, pair programming in tmux, quick one-shot prompts from the CLI — all broken.

- **Dioxus streaming is hard** — Dioxus 0.7's server functions are RPC-style (call → return). Streaming agent responses (thinking chunks, tool calls, progressive text) don't map cleanly to Dioxus's reactivity model. You'd need to bolt on a raw WebSocket handler outside of Dioxus — which is what omegon already has in `web/ws.rs`. So you'd end up reimplementing omegon's WebSocket protocol inside Scribe's axum router.

- **Two SQLite databases in one process** — Scribe's engagement DB and omegon-memory's facts DB. Different schemas, different migration stories, different backup/sync patterns. Not a technical blocker, but adds operational complexity.

- **Coupling Scribe's release cycle to Omegon's** — Every omegon update requires rebuilding Scribe. Every Scribe UI fix requires recompiling the agent. Feature velocity on both stalls.

- **Scribe becomes the deployment target for ALL omegon users** — But most omegon users don't have Scribe. The standalone `oa` binary works for any repo. Making Scribe the host means the general-purpose tool becomes company-specific.

**What you gain**:

- Single process. No IPC between Scribe and the agent.
- Scribe's Dioxus UI can render agent conversations with full web richness (tables, images, interactive elements).
- Partnership context is directly available in-process — no HTTP calls needed.

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
marker_files = [".scribe"]
env_vars = ["SCRIBE_URL"]

[context]
# Inject partnership/engagement info into system prompt
endpoint = "{SCRIBE_URL}/api/context"
# Or read from local .scribe file
local_file = ".scribe"
ttl_turns = 20
priority = 40

[[tools]]
name = "scribe_status"
description = "Get current engagement status, team, and recent activity"
endpoint = "{SCRIBE_URL}/api/engagement/{engagement_id}/summary"
parameters = { type = "object", properties = { engagement_id = { type = "integer" } } }

[[tools]]
name = "scribe_log"
description = "Add a work log entry to the current engagement"
endpoint = "{SCRIBE_URL}/api/logs"
method = "POST"
parameters = { type = "object", properties = { content = { type = "string" }, category = { type = "string" } } }

[events]
# Forward agent events to Scribe for session logging
turn_end = "{SCRIBE_URL}/api/sessions/ingest"
```

**What this requires technically**:

1. **Plugin loader in omegon** — reads `~/.omegon/plugins/*/plugin.toml`, creates `ToolAdapter` instances backed by HTTP calls to the declared endpoints. Maybe 200-300 lines of Rust.

2. **Scribe API endpoints** — Scribe already has server functions for everything needed. Add a few REST endpoints that omegon can call (or use the existing Dioxus server functions via their HTTP routes).

3. **Context enrichment** — When the plugin activates (`.scribe` marker detected), omegon calls `{SCRIBE_URL}/api/context?repo=current-repo` and injects the response as a context signal. Partnership name, engagement status, team members, recent activity — all injected into the LLM's system prompt.

4. **Event forwarding** — On TurnEnd, omegon POSTs a summary to `{SCRIBE_URL}/api/sessions/ingest`. Scribe ingests it as a session log entry, linked to the engagement.

**What you keep**:

- **TUI works** — `oa interactive` from any terminal, SSH, tmux. Zero change.
- **Standalone works** — Users without Scribe use omegon normally. No degradation.
- **Scribe's web UI works** — Scribe shows engagement data, agent session logs (forwarded via events), partnership status. The Scribe web UI is for engagement management, not agent interaction.
- **Independent release cycles** — Omegon ships on its own. Scribe ships on its own. The plugin manifest is the contract.
- **WebSocket protocol** — If you want to use the agent through Scribe's web UI, Scribe can connect to omegon's existing `ws://localhost:7842/ws` endpoint. The protocol is already designed for this ("any web UI can connect and drive the agent as a black box").

**What you lose**:

- IPC overhead — HTTP calls to Scribe for context/events instead of in-process access.
- Two processes — omegon + scribe running simultaneously (but Scribe is already a separate deployment anyway).

### Option H: Hybrid — Scribe's web UI connects to Omegon via WebSocket

**Concept**: Neither wraps the other. Both run independently. Scribe's Dioxus UI adds an "Agent" tab that connects to omegon's WebSocket protocol on localhost:7842. The agent conversation happens inside Scribe's web UI but the agent process is standalone omegon.

This is actually the **existing architecture** — omegon already exposes a WebSocket protocol with 15 event types and 4 command types. The embedded dashboard.html is already a consumer of this protocol. Scribe's Dioxus UI would be a second, richer consumer.

**What this requires**: Scribe adds a Dioxus page component that opens a WebSocket to omegon, renders the agent conversation with Dioxus's RSX, and sends user prompts/slash commands back. Maybe 300 lines of Dioxus component code.

**What this gets you**: Full web rendering of agent output (tables, images, interactive elements) inside the engagement management context. Partnership info shown alongside the agent conversation. Session logs automatically linked to the current engagement.

**What this doesn't do**: It doesn't inject partnership context into the agent's system prompt — for that you still need the bolt-on plugin from Option B. But the two are complementary: Option B for agent-side context enrichment + Option H for UI-side rich rendering.

### Assessment — unbiased

**The wrapper approach (Option W) is wrong for this system.**

Here's why, without hedging:

**1. The value proposition is inverted.** Omegon is a general-purpose agent harness. Scribe is a company-specific engagement tracker. Wrapping the general tool inside the specific tool means the general tool can't exist without the specific tool. Every non-Recro user (including you using omegon on personal projects) would need to run Scribe to get an agent. That's architecturally backwards.

**2. The TUI is not optional — it's the primary interface.** You built 12,000+ LoC of terminal UI because terminal-first is the right interface for an agent that runs alongside your editor. The wrapper kills this. Dioxus can do terminal rendering (Dioxus has a TUI renderer via `dioxus-tui`) but it's experimental, doesn't support ratatui widgets, and would require rewriting every component. You'd be throwing away working software to adopt an unproven rendering path.

**3. Dioxus fullstack doesn't solve the streaming problem.** Dioxus 0.7 server functions are request-response (`#[server] async fn → Result<T>`). Agent responses are streaming (thinking chunks arrive over seconds, tool calls interleave with text). Dioxus has `use_server_future` for async data loading but not for server-push streaming. To stream agent events to the Dioxus client, you'd need a raw WebSocket handler bolted onto the Dioxus axum router — which is exactly what omegon already has. The wrapper doesn't give you streaming for free; it makes you reimplement it.

**4. The "single process" benefit is illusory.** The IPC overhead of HTTP calls from omegon→Scribe is negligible (localhost, JSON, <1ms). You're making 1-2 HTTP calls per turn for context enrichment and event forwarding. The LLM call itself takes 5-30 seconds. The context call takes 10ms. The cost of merging two processes to eliminate 10ms of IPC is: coupled release cycles, doubled dependency graph, 3-4x binary size, and loss of the standalone deployment model.

**5. You already designed the right interface.** The WebSocket protocol (`ws://localhost:7842/ws`) was explicitly designed as "the full agent interface — any web UI can connect and drive the agent as a black box." This is the correct abstraction boundary. Scribe's web UI connecting to this WebSocket is Option H — and it works today with zero omegon changes.

**The right architecture is Option B + H:**

- **Option B (bolt-on plugin)** handles the agent-side integration: `.scribe` marker detection → context enrichment → tool registration → event forwarding. This is ~300 lines of Rust in omegon (plugin loader) + a few REST endpoints in Scribe.

- **Option H (WebSocket consumer)** handles the UI-side integration: Scribe's Dioxus UI connects to omegon's WebSocket for rich agent conversation rendering inside the engagement context. This is ~300 lines of Dioxus component code in Scribe.

Both approaches compose cleanly. They don't couple the release cycles. They don't destroy the TUI. They don't bloat the binary. And they establish a reusable pattern: any company-specific system can integrate with omegon the same way — declare tools, inject context, forward events, optionally connect to the WebSocket for UI.

**The plugin manifest (plugin.toml) is the extension API.** It's not the Feature trait (that's internal). It's not the ToolAdapter (that's a Rust implementation detail). The contract between omegon and external systems is: "here are my tools, here's my context endpoint, here are the events I want." TOML manifest + HTTP endpoints. Language-agnostic. No compilation required. Works for Rust, Python, Go, whatever the external system is written in.

## Decisions

### Decision: Bolt-on plugin + WebSocket consumer (Option B + H), not wrapper

**Status:** exploring
**Rationale:** The wrapper destroys the TUI, couples release cycles, bloats the binary, and solves a problem (IPC elimination) that doesn't exist (10ms localhost calls vs 5-30s LLM calls). The bolt-on plugin pattern is language-agnostic, composable, and reusable for any company-specific integration. The WebSocket protocol was designed exactly for this use case.

### Decision: Bolt-on plugin + WebSocket consumer (Option B + H), not wrapper

**Status:** decided
**Rationale:** The wrapper destroys the TUI (12k LoC), couples release cycles, bloats the binary 3-4x, and eliminates ~10ms of IPC that's irrelevant against 5-30s LLM calls. The bolt-on pattern is language-agnostic (plugin.toml + HTTP), composable, independently releasable, and reusable for any company-specific integration. The WebSocket protocol was explicitly designed for external UI consumers.

## Open Questions

*No open questions.*

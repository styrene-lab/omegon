+++
id = "f6c26bb2-a49a-436f-808d-7f77cdbf9f3d"
kind = "document"
title = "A2A Protocol Integration — Agent-to-Agent interoperability for Omegon"
status = "deferred"
tags = ["architecture", "a2a", "interoperability", "multi-agent", "protocol", "strategic"]
aliases = ["a2a-protocol-integration"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = ["Should A2A be adopted incrementally (server-only first, then client) or as a full bidirectional protocol from the start?", "Should A2A replace the cleave child task-file protocol, or run alongside it as an optional transport?", "What is the security model for an A2A endpoint on a coding agent that handles repo secrets — localhost-only? mTLS? OAuth with what identity provider?", "Does A2A subsume the Omega HTTP API design question, or are they separate concerns (A2A for external interop, bespoke RPC for internal Omega↔Omegon)?"]
related = ["omega", "multi-instance-coordination"]
+++

# A2A Protocol Integration — Agent-to-Agent interoperability for Omegon

## Overview

Assess whether and how Omegon should adopt Google's Agent-to-Agent (A2A) protocol for inter-agent communication. A2A is an open protocol (now under Linux Foundation governance) built on HTTP + JSON-RPC 2.0 + SSE that enables agents to discover each other via Agent Cards, delegate tasks, and stream results. It complements MCP (which Omegon already bridges for tool access) by adding the agent-as-peer layer on top of the agent-as-tool layer.

## Research

### A2A Protocol Summary

**What A2A is:** An open protocol (Google, now Linux Foundation) for agent-to-agent communication over HTTP. Built on JSON-RPC 2.0 with SSE streaming. IBM's Agent Communication Protocol merged into A2A in August 2025.

**Core concepts:**
- **Agent Card** (`/.well-known/agent-card.json`): JSON capability advertisement — name, description, skills, supported content types, auth requirements. The discovery mechanism.
- **Task lifecycle**: `submitted → working → input-required → completed/failed/canceled`. Tasks are the unit of work between agents.
- **Message/Part model**: Messages contain Parts (text, file, data). Agents exchange messages within tasks.
- **Streaming**: SSE for real-time task progress updates.
- **Auth**: OAuth 2.0, API keys, or bearer tokens per Agent Card declaration.

**How it complements MCP:**
- MCP = agent-to-tool (structured function calls to external systems)
- A2A = agent-to-agent (peer delegation, collaboration, back-and-forth negotiation)
- An agent might use MCP internally to call APIs, but expose itself to other agents via A2A

**Adoption:** 50+ launch partners (Atlassian, Salesforce, SAP, etc.). SDKs in Python, TypeScript, Go. Production implementations at H2O.ai, various enterprise platforms.

### Omegon's Current Inter-Agent Surface

**What Omegon already has:**
1. **MCP Bridge** (`extensions/mcp-bridge/`): Connects to MCP servers (stdio + Streamable HTTP), registers their tools as native pi tools. Project/user/extension config layers. Already handles the agent-to-tool axis.
2. **Cleave child dispatch** (`extensions/cleave/dispatcher.ts`): Spawns child Omegon processes in git worktrees. Children are full agent instances with their own context — but communication is one-way (parent writes task file → child executes → parent reads result). No back-and-forth negotiation.
3. **Web UI server** (`extensions/web-ui/`): HTTP server on localhost — aspirational, mostly scaffolding. Could host an A2A endpoint.
4. **Omega design node** (`docs/omega.md`): Future Rust execution engine with open questions about HTTP API surface (REST vs JSON-RPC) and real-time progress push (WebSocket vs polling). A2A could answer both.
5. **Slash command bridge** (`extensions/lib/slash-command-bridge.ts`): Internal structured command execution — agent-callable but not externally accessible.

**What's missing:**
- No external discovery mechanism — other agents can't find or invoke Omegon
- No structured capability advertisement
- Cleave children can't negotiate with the parent (no input-required state)
- No standard way for external agents to delegate work to Omegon or vice versa

### Integration Assessment — Where A2A Fits and Where It Doesn't

**Strong fit (high value, tractable):**

1. **Omegon as A2A Server** — Expose Omegon's capabilities (code analysis, refactoring, test generation, design exploration) to external orchestrators or other agents. An Agent Card advertising Omegon's skills would let enterprise workflows delegate coding tasks to it. This aligns with the existing web-ui server skeleton.

2. **Cleave children as A2A tasks** — Replace the current fire-and-forget task-file protocol with A2A task lifecycle. Children could report `working` status with streaming progress (solving the observability problem cleave-child-observability addressed ad-hoc), request `input-required` when they need parent guidance (currently impossible — children just fail or guess), and report structured results. The task lifecycle states map cleanly to CleavePhase.

3. **Omega API surface** — The open question in the Omega design node ("REST vs JSON-RPC?") is directly answered by A2A: JSON-RPC 2.0 over HTTP with SSE streaming. Adopting A2A for the Omega↔Omegon boundary would mean the protocol choice is a settled industry standard rather than a bespoke design.

**Moderate fit (valuable but complex):**

4. **Omegon as A2A Client** — Delegate tasks to external A2A agents (e.g., a specialized security scanner, a documentation generator, a deployment agent). This extends MCP's tool-calling with peer-level collaboration. However, the current session model is single-agent — adding external agent delegation requires workflow orchestration that doesn't exist yet.

5. **Multi-instance coordination** — The decided design node `multi-instance-coordination` envisions parallel Omegon instances on the same repo. A2A's discovery + task model could provide the coordination protocol instead of inventing one.

**Weak fit (low value or premature):**

6. **Agent Card for local-only Omegon** — If Omegon only runs locally on the operator's machine, agent discovery via `/.well-known/` URLs has no audience. A2A's value is in networked, multi-party scenarios. For single-operator use, the protocol overhead buys nothing.

7. **A2A for MCP replacement** — MCP and A2A are complementary, not competing. Omegon should keep MCP for tool access and only add A2A for agent-to-agent communication.

**Key risk:** A2A adds HTTP server infrastructure to what is currently a purely CLI tool. Running an HTTP listener changes the security surface significantly — especially if Omegon handles code with secrets. The Semgrep security analysis flags prompt injection inheritance as the primary A2A threat.

## Decisions

### Decision: A2A is not the right protocol for internal cleave child coordination

**Status:** decided
**Rationale:** A2A solves untrusted federation — discovery, negotiation, and auth between agents from different vendors and trust boundaries. Omegon controls the entire fleet: it spawns the children, knows their capabilities, and trusts them completely. HTTP + JSON-RPC + OAuth between co-located subprocesses is massive overhead for zero trust benefit. The one genuine gap (mid-task negotiation) is a subprocess IPC problem, not an interoperability protocol problem.

### Decision: Defer A2A for external interop — no current demand

**Status:** decided
**Rationale:** A2A remains relevant if Omegon ever needs to expose capabilities to external orchestrators or federate with untrusted agents. That demand doesn't exist today. Revisit when Omega development begins or when external integration requests appear.

## Open Questions

- Should A2A be adopted incrementally (server-only first, then client) or as a full bidirectional protocol from the start?
- Should A2A replace the cleave child task-file protocol, or run alongside it as an optional transport?
- What is the security model for an A2A endpoint on a coding agent that handles repo secrets — localhost-only? mTLS? OAuth with what identity provider?
- Does A2A subsume the Omega HTTP API design question, or are they separate concerns (A2A for external interop, bespoke RPC for internal Omega↔Omegon)?

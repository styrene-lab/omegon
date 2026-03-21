# HarnessStatus contract — unified status surface for TUI, web dashboard, and bootstrap — Design Spec (extracted)

> Auto-extracted from docs/harness-status-contract.md at decide-time.

## Decisions

### Bootstrap is a structured TUI panel, not plain printf (decided)

The bootstrap output is the operator's first impression. A ratatui-rendered panel with colored status indicators (✓/⚠/○), aligned columns, and grouped sections looks professional and matches the TUI aesthetic. Plain printf is for CI/headless (--no-tui flag). The bootstrap panel is the same HarnessStatus struct rendered once — same data, same renderer, just displayed before the event loop starts.

### HarnessStatus is event-driven via BusEvent::HarnessStatusChanged (decided)

State changes are infrequent (persona switch, MCP connect, secret unlock) but time-sensitive — the operator needs to see them immediately. Polling would either be too frequent (waste) or too slow (stale UI). The existing EventBus already delivers typed events to all subscribers. Adding HarnessStatusChanged is one enum variant. The TUI footer re-renders on this event. The web dashboard broadcasts it over WebSocket.

### Web dashboard uses existing WebSocket event bus — HarnessStatusChanged is a new message type (decided)

The web dashboard already connects via WebSocket to receive agent events (tool calls, messages, cleave progress). Adding HarnessStatusChanged as a new message type on the same connection keeps the architecture simple — one WebSocket, all events. No separate /api/status endpoint needed. The dashboard renders the latest HarnessStatus snapshot from the most recent event.

## Research Summary

### Seven UI domains from this session's work

Each domain has distinct data, controls, and display requirements:

**1. Persona System** — Active persona badge (footer), active tone badge (footer), `/persona` and `/tone` commands, persona picker (settings overlay), plugin list (settings + dashboard). Data: PersonaSummary { id, name, badge, mind_facts_count, activated_skills, disabled_tools }.

**2. MCP Transport** — Server connection status (bootstrap + dashboard), tool list with mcp: labels, error notifications. Data: McpServerStatus { name…

### Three consumers, one source of truth

**Consumer 1: Bootstrap (startup)**
Prints once when omegon launches. Shows the full inventory: cloud providers, local inference, MCP servers, secrets backend, container runtime, installed plugins. This is the operator's "what do I have?" view. Already partially exists in the operator-capability-profile bootstrap flow — HarnessStatus extends it.

**Consumer 2: TUI (continuous)**
Footer bar shows: model name + context class badge + thinking level + active persona badge + active tone badge + secre…

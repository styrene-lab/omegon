+++
id = "62cf7bb7-42ca-402b-a150-40df291296bb"
kind = "document"
title = "TUI surface pass — expose new subsystems in dashboard, footer, selectors, and commands"
status = "implemented"
tags = ["tui", "ux", "dashboard", "footer", "commands", "persona", "mcp", "auth", "secrets", "inference"]
aliases = ["tui-surface-pass"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = ["feature/tui-surface-pass"]
issue_type = "epic"
open_questions = []
openspec_change = "tui-surface-pass"
parent = "tui-visual-system"
priority = "1"
+++

# TUI surface pass — expose new subsystems in dashboard, footer, selectors, and commands

## Overview

The Rust core has grown significantly — persona system, MCP transport (5 modes including HTTP), encrypted secrets, auth surface, harness settings, plugin CLI, context class routing, inference backends — but the TUI only partially exposes this. The footer shows persona/tone badges and MCP counts, but the dashboard shows none of it. No selector overlays for persona/tone/context-class. Several slash commands exist but have no visual feedback beyond text dumps.\n\nThis is a single coordinated pass to bring the TUI in line with the backend.

## Research

### Gap analysis — backend capability vs TUI exposure

**TUI files:** 7,926 LoC across 14 files. Key surfaces: mod.rs (1,944 — app struct, slash commands, event loop), dashboard.rs (616 — right panel), footer.rs (404 — 4-card strip), conversation.rs (417), editor.rs (413), selector.rs (201), bootstrap.rs (232).

### Proposed work items — ordered by impact

**Tier 1 — High impact, low effort (selector overlays + dashboard sections):**

1. **Persona selector overlay** — /persona with no args opens an interactive picker (like /model). Shows installed personas with badge, name, mind fact count, active skills. Arrow keys to navigate, Enter to activate, Esc to cancel. Replaces the text list dump.

2. **Tone selector overlay** — Same pattern as persona selector. Shows tones with name, intensity config, exemplar count.

3. **Context class selector overlay** — /context opens picker showing Compact/Standard/Extended/Massive with token counts and descriptions. Currently a text toggle between 200k/1M.

4. **Dashboard: harness status section** — New section below cleave progress showing:
   - Active persona + tone (if any)
   - Provider auth status (✓/✗ per provider)
   - MCP servers (connected count / total, tool count)
   - Secrets store (locked/unlocked)
   - Inference (Ollama status, model count)
   This is essentially the bootstrap panel data rendered as a persistent dashboard section.

**Tier 2 — Medium impact (event-driven feedback):**

5. **Toast notifications on state changes** — When HarnessStatusChanged arrives, compare with previous state and toast on meaningful transitions:
   - Persona activated/deactivated
   - MCP server connected/disconnected
   - Auth token expired
   - Secrets store unlocked
   - Compaction fired

6. **Dashboard refresh on HarnessStatusChanged** — The dashboard currently only refreshes on lifecycle events. Add a refresh trigger on HarnessStatusChanged so the new harness section updates in real time.

7. **Compaction visual indicator** — Brief footer flash or toast when auto-compact fires. Currently invisible to the operator.

**Tier 3 — Polish (formatted command output):**

8. **/stats as a styled card** — Instead of plain text, render session stats in a bordered card with aligned columns and color-coded values.

9. **/auth as a styled overlay** — Provider auth table with colored status indicators instead of plain text.

10. **/memory as a visual breakdown** — Show memory stats with a section distribution bar (Architecture: N, Decisions: M, etc.) instead of raw counts.

**Tier 4 — Fractal status surface (separate node, decided, P3):**

11. Fractal widget at bottom of dashboard sidebar — already designed in fractal-status-surface node.

## Decisions

### Decision: Dashboard sections are independent — harness status always visible, cleave hides when idle

**Status:** decided
**Rationale:** Each dashboard section represents a different subsystem. They render independently in vertical stack order: design tree (always), openspec (always), cleave (visible only when active), harness status (always). When cleave is idle, its section disappears and the remaining sections get more vertical space. The harness section is not a replacement for cleave — it's a peer.

### Decision: Depth-ordered settings surface — deep changes get selectors, shallow changes are inline

**Status:** decided
**Rationale:** Settings are ordered by depth of impact: model (deepest — changes everything) → thinking level → context class → persona → tone (shallowest — cosmetic voice). Deep changes warrant a full selector overlay because the operator needs to see options and understand consequences. Shallow changes can be inline — persona/tone switch via the existing /persona and /tone commands with a quick-pick list, or a lightweight inline selector, not a full overlay. Model stays as a full overlay selector. Thinking level stays as a full overlay. Context class gets a lightweight selector (4 options, one line each). Persona and tone use the existing slash command lists — they're quick enough at typical install counts (2-5 options) that an overlay adds ceremony without value.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/dashboard.rs` (modified) — Add harness status section (persona/tone, providers, MCP, secrets, inference, container). Make cleave section conditional (hide when idle). Vertical stack: design tree → openspec → cleave (if active) → harness status.
- `core/crates/omegon/src/tui/selector.rs` (modified) — Add SelectorKind::ContextClass with 4 options (Compact/Standard/Extended/Massive + token counts + descriptions)
- `core/crates/omegon/src/tui/mod.rs` (modified) — /context opens context class selector overlay. Toast notifications on HarnessStatusChanged state transitions (persona switch, MCP connect/disconnect, auth expiry, compaction). Dashboard refresh on HarnessStatusChanged.
- `core/crates/omegon/src/tui/footer.rs` (modified) — Compaction flash indicator (brief accent color pulse on system card when compaction fires)
- `core/crates/omegon/src/status.rs` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Cleave section hides entirely when no cleave is active — not just empty, invisible
- Harness status section reads from FooterData.harness (same HarnessStatus, no separate data path)
- Context class selector shows nominal token count and one-line description per class
- Toast notifications compare previous HarnessStatus snapshot to detect meaningful transitions — don't toast on every event

## Dashboard (right panel, 616 LoC)

**Currently shows:**
- Design tree: focused node, implementing nodes, actionable nodes, tree hierarchy
- OpenSpec: active changes with stage + task progress
- Cleave: active children with status + elapsed time

**Missing:**
- Persona/tone active status (badge + name)
- MCP server list with health indicators (connected/disconnected/error)
- Provider auth status (authenticated/expired/missing per provider)
- Secrets store status (locked/unlocked, backend type)
- Inference backends (Ollama available, models loaded)
- Container runtime (podman/docker, version)
- Session stats section (turns, tool calls, duration — currently only in /stats)

## Footer (4-card strip, 404 LoC)

**Currently shows:**
- Card 1 (Context): utilization gauge, window size, context class badge
- Card 2 (Model): model ID, provider, persona badge + tone badge (from HarnessStatus)
- Card 3 (Memory): total facts, injected, working memory, tokens estimate
- Card 4 (System): turns, tool calls, MCP count + tool count, secrets lock icon

**Working well** — the footer already pulls from HarnessStatus for persona/tone/MCP/secrets. The main issue is density: the badges are small and can be missed.

## Selector overlays (201 LoC)

**Currently has:**
- Model selector (Tab cycles providers, Enter confirms)
- Thinking level selector

**Missing:**
- Persona selector (/persona shows text list, should have interactive picker)
- Tone selector (/tone shows text list, should have interactive picker)
- Context class selector (/context shows text, should have interactive picker)
- Auth status overlay (visual table of all backends)

## Slash commands

**Commands with good TUI integration:**
- /model → opens selector overlay
- /think → opens selector overlay
- /dash → toggles panel + /dash open starts web
- /splash → replays animation
- /status → renders bootstrap panel inline

**Commands that dump text but could have visual treatment:**
- /persona → text list, should open selector
- /tone → text list, should open selector
- /context → text toggle, should open selector with class descriptions
- /auth → text table, could be a styled overlay
- /stats → text dump, could be a formatted card
- /memory → text dump, could show visual breakdown

## Event handling gaps

**HarnessStatusChanged is handled** (tui/mod.rs:1082) — updates footer_data.harness. But:
- No dashboard refresh on persona/tone switch
- No toast notification on MCP server connect/disconnect
- No visual feedback on auth expiry
- No indicator when compaction fires (auto_compact returns BusRequest::RequestCompaction but no TUI signal)

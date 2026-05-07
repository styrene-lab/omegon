+++
id = "e4d8699a-ea5c-42b5-ac42-688be7b1f6fb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# TUI surface pass — expose new subsystems in dashboard, footer, selectors, and commands — Design Spec (extracted)

> Auto-extracted from docs/tui-surface-pass.md at decide-time.

## Decisions

### Dashboard sections are independent — harness status always visible, cleave hides when idle (decided)

Each dashboard section represents a different subsystem. They render independently in vertical stack order: design tree (always), openspec (always), cleave (visible only when active), harness status (always). When cleave is idle, its section disappears and the remaining sections get more vertical space. The harness section is not a replacement for cleave — it's a peer.

### Depth-ordered settings surface — deep changes get selectors, shallow changes are inline (decided)

Settings are ordered by depth of impact: model (deepest — changes everything) → thinking level → context class → persona → tone (shallowest — cosmetic voice). Deep changes warrant a full selector overlay because the operator needs to see options and understand consequences. Shallow changes can be inline — persona/tone switch via the existing /persona and /tone commands with a quick-pick list, or a lightweight inline selector, not a full overlay. Model stays as a full overlay selector. Thinking level stays as a full overlay. Context class gets a lightweight selector (4 options, one line each). Persona and tone use the existing slash command lists — they're quick enough at typical install counts (2-5 options) that an overlay adds ceremony without value.

## Research Summary

### Gap analysis — backend capability vs TUI exposure

**TUI files:** 7,926 LoC across 14 files. Key surfaces: mod.rs (1,944 — app struct, slash commands, event loop), dashboard.rs (616 — right panel), footer.rs (404 — 4-card strip), conversation.rs (417), editor.rs (413), selector.rs (201), bootstrap.rs (232).

### Proposed work items — ordered by impact

**Tier 1 — High impact, low effort (selector overlays + dashboard sections):**

1. **Persona selector overlay** — /persona with no args opens an interactive picker (like /model). Shows installed personas with badge, name, mind fact count, active skills. Arrow keys to navigate, Enter to activate, Esc to cancel. Replaces the text list dump.

2. **Tone selector overlay** — Same pattern as persona selector. Shows tones with name, intensity config, exemplar count.

3. **Context class selector overlay…

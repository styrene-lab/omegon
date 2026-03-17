---
subsystem: dashboard
design_docs:
  - design/unified-dashboard.md
  - design/dashboard-wide-truncation.md
  - design/non-capturing-dashboard.md
  - design/clickable-dashboard.md
  - design/cleave-title-progress-sync.md
openspec_baselines:
  - dashboard.md
  - dashboard/terminal-title.md
last_updated: 2026-03-10
---

# Dashboard

> Unified live footer showing design tree, OpenSpec, cleave progress, model routing, and recovery state across compact, raised, panel, and focused display modes.

## What It Does

The dashboard extension replaces pi's default footer with a multi-mode persistent display that unifies state from across all Omegon subsystems. It provides four display modes:

- **Compact** (3 lines): Context gauge, token stats, model/thinking indicator, git branch — always-visible summary
- **Raised** (up to 10 lines): Adds design tree focus, OpenSpec changes, live cleave dispatch progress, directive mind indicator with branch-match status
- **Panel**: Full overlay with scrollable detail view
- **Focused**: Single-subsystem deep view

Mode cycling via `/dash` command or `ctrl+\`` keybind. The dashboard consumes state from decentralized emitters — each subsystem (cleave, design-tree, OpenSpec, model-budget) writes to shared state via `emitDashboardUpdate()`, and the dashboard re-renders on change.

Terminal title (`\x1b]0;...\x07`) mirrors compact dashboard state including cleave child progress percentages.

## Key Files

| File | Role |
|------|------|
| `extensions/dashboard/index.ts` | Extension entry — `setFooter()`, mode cycling, slash commands, bridge registration |
| `extensions/dashboard/footer.ts` | Footer component — renders compact/raised modes within 10-line cap |
| `extensions/dashboard/overlay.ts` | Panel overlay — scrollable detail view with keyboard navigation |
| `extensions/dashboard/overlay-data.ts` | Data preparation for overlay sections |
| `extensions/dashboard/types.ts` | `DashboardState`, `DashboardMode`, per-subsystem state interfaces |
| `extensions/dashboard/shared-state.ts` | `DASHBOARD_UPDATE_EVENT`, `emitDashboardUpdate()` |
| `extensions/dashboard/context-gauge.ts` | Context window usage gauge (tokens remaining) |
| `extensions/dashboard/memory-audit.ts` | Memory statistics for dashboard display |
| `extensions/dashboard/uri-helper.ts` | OSC 8 clickable links for dashboard items |
| `extensions/dashboard/file-watch.ts` | File system watchers for design-tree/openspec changes |
| `extensions/terminal-title.ts` | Terminal title sync from dashboard state |

## Design Decisions

- **Decentralized emitters, centralized renderer**: Each subsystem emits state updates via `pi.events.emit(DASHBOARD_UPDATE_EVENT)`. The dashboard subscribes and re-renders. No subsystem imports dashboard code directly.
- **Footer raise/lower + overlay**: Two-tier display — footer modes for persistent view, overlay for detail. No external terminal pane (tmux/zellij) required.
- **`ctrl+\`` keybind**: Chosen after `Ctrl+Shift+D` (shadowed by pi-tui debug handler) and `Ctrl+Shift+B`/`Ctrl+Shift+P` (shadowed by Kitty terminal) were both blocked. `/dash` command is the reliable fallback.
- **Cleave progress in terminal title**: `emitCleaveChildProgress` updates shared state on child stdout lines; terminal title reflects `[2/4 ██░░]` style progress.
- **Directive indicator**: When a directive mind is active, raised mode shows `▸ directive: name ✓` (branch match) or `▸ directive: name ⚠ main` (mismatch). Helps operators notice when they've drifted off the directive branch.
- **Dashboard URI helper for clickable items**: OSC 8 links on design tree nodes, OpenSpec changes, and file paths open in the configured editor.

## Behavioral Contracts

See `openspec/baseline/dashboard.md` and `openspec/baseline/dashboard/terminal-title.md` for Given/When/Then scenarios covering:
- Footer mode transitions (compact ↔ raised ↔ panel)
- Context gauge accuracy
- Cleave progress display
- Terminal title format and update triggers

## Constraints & Known Limitations

- Footer capped at 10 lines to avoid pushing conversation off-screen
- `ctx.ui.custom()` overlays are blocking — the panel steals keyboard focus from the editor
- Only one custom footer allowed at a time (pi TUI limitation)
- Terminal title updates require terminal emulator OSC support (iTerm2, Kitty, WezTerm)

## Related Subsystems

- [Cleave](cleave.md) — emits `CleaveState` for dispatch progress
- [Model Routing](model-routing.md) — emits recovery state and model info
- [Design Tree](design-tree.md) — emits focused node and tree stats
- [OpenSpec](openspec.md) — emits active change status

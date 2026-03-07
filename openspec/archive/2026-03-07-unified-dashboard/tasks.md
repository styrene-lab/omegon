# unified-dashboard — Tasks

## 1. Shared State Types & Emitter Infrastructure

- [x] 1.1 Extend SharedState interface in extensions/shared-state.ts with designTree?, openspec?, cleave? properties and their type definitions
- [x] 1.2 Add DashboardEvent type and "dashboard:update" channel constant

## 2. Design Tree Emitter

- [x] 2.1 Add emitDashboardState() function to design-tree/index.ts that writes sharedState.designTree with node counts, focused node, and open questions
- [x] 2.2 Call emitDashboardState() at every point where updateWidget() was previously called (tool_execution_end, focus/unfocus, status changes)
- [x] 2.3 Remove all setWidget("design-tree", ...) calls from design-tree/index.ts
- [x] 2.4 Remove the /design widget toggle command (widget subcommand) — dashboard subsumes this
- [x] 2.5 Fire pi.events.emit("dashboard:update") after each sharedState write

## 3. OpenSpec Emitter

- [x] 3.1 Add emitDashboardState() function to openspec/index.ts that writes sharedState.openspec with change names, stages, and task progress
- [x] 3.2 Call emitDashboardState() after session_start scan and after any change mutation (propose, add_spec, fast_forward, archive)
- [x] 3.3 Fire pi.events.emit("dashboard:update") after each sharedState write

## 4. Cleave Emitter

- [x] 4.1 Add emitDashboardState() function to cleave/index.ts that writes sharedState.cleave with status, runId, and children array
- [x] 4.2 Emit idle state on session_start
- [x] 4.3 Emit state transitions: assessing → planning → dispatching → merging → done/failed
- [x] 4.4 In dispatcher.ts, update sharedState.cleave.children[n] in spawn start/exit callbacks with status and elapsed time
- [x] 4.5 Fire pi.events.emit("dashboard:update") on each transition and child status change

## 5. Dashboard Footer Component

- [x] 5.1 Create extensions/dashboard/types.ts with DashboardMode, DashboardState interfaces
- [x] 5.2 Create extensions/dashboard/footer.ts with DashboardFooter Component class implementing render(width): string[]
- [x] 5.3 Implement compact mode (Layer 0): single dashboard summary line with ◈ D:x/y ◎ OS:n ⚡ status + context gauge
- [x] 5.4 Implement raised mode (Layer 1): design tree section, openspec section, cleave section (5-8 lines)
- [x] 5.5 Reimplement built-in footer data: pwd (~), git branch, session name, input/output/cache tokens, cost, context%, model name, thinking level, extension statuses
- [x] 5.6 Support invalidate() for theme changes — rebuild all themed strings

## 6. Dashboard Extension Entry Point

- [x] 6.1 Create extensions/dashboard/index.ts with extension registration, setFooter(), and pi.events subscription
- [x] 6.2 Register Ctrl+Shift+D shortcut via pi.registerShortcut to toggle raised/lowered and call tui.requestRender()
- [x] 6.3 Implement /dashboard slash command for toggle and status info
- [x] 6.4 Persist raised/lowered state via pi.appendEntry("dashboard-state") and restore on session_start
- [x] 6.5 Subscribe to "dashboard:update" events and re-render footer
- [x] 6.6 Track turn count via turn_end events (absorbing status-bar logic)
- [x] 6.7 Read sharedState.memoryTokenEstimate for context gauge (absorbing status-bar logic)
- [x] 6.8 Unsubscribe from pi.events on session_shutdown

## 7. Cleanup & Wiring

- [x] 7.1 Delete extensions/status-bar.ts
- [x] 7.2 Update package.json: remove status-bar.ts from pi.extensions, add extensions/dashboard/index.ts after design-tree
- [x] 7.3 Verify extension load order: cleave → openspec → design-tree → dashboard
- [x] 7.4 Smoke test: jiti load + mock render verified compact (3 lines) and raised (9 lines) modes with full data

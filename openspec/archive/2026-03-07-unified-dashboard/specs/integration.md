# Dashboard Integration

Wiring: keyboard shortcut, /dashboard command, session state persistence, extension lifecycle, and the removal of status-bar.ts.

## Requirements

### R1: Ctrl+Shift+D shortcut toggles raise/lower
A registered keyboard shortcut must toggle the footer between compact and raised modes, and call tui.requestRender().

### R2: /dashboard command
A slash command must provide: toggle (raise/lower), and info about the dashboard state.

### R3: Session state persistence
The raised/lowered state must be persisted via pi.appendEntry("dashboard-state", { raised }) and restored on session_start from the last such entry.

### R4: Dashboard subscribes to pi.events
The dashboard extension must subscribe to "dashboard:update" events and re-render the footer when received.

### R5: Extension load order
The dashboard extension must be listed AFTER design-tree, openspec, and cleave in package.json pi.extensions to ensure sharedState is populated.

### R6: status-bar.ts removed
status-bar.ts must be deleted and its entry removed from package.json pi.extensions. The context gauge, turn counter, and memory bar must be rendered by the dashboard footer instead.

### R7: design-tree widget removed
design-tree must no longer call setWidget. Its /design widget toggle command should be removed or adapted to toggle the dashboard raised state.

### R8: pi.events cleanup on shutdown
The dashboard must unsubscribe from pi.events on session_shutdown to avoid leaks.

## Scenarios

### S1: Ctrl+Shift+D toggles footer mode
Given the dashboard is in compact mode
When the user presses Ctrl+Shift+D
Then the footer switches to raised mode
And pressing Ctrl+Shift+D again returns to compact mode

### S2: /dashboard command toggles mode
Given the dashboard is in compact mode
When the user runs /dashboard
Then the footer switches to raised mode

### S3: Dashboard state persists across sessions
Given the user raises the footer via Ctrl+Shift+D
When the session is saved and restored
Then the footer starts in raised mode

### S4: Dashboard re-renders on events
Given the dashboard is rendered in compact mode
When design-tree emits a "dashboard:update" event after a node status change
Then the footer re-renders with updated design tree counts

### S5: status-bar.ts is removed
Given pi-kit is loaded
When the extension list is evaluated
Then status-bar.ts is not in package.json pi.extensions
And no extension calls setStatus("status-bar", ...)

### S6: Turn counter increments in footer
Given status-bar.ts is removed
When a turn completes (turn_end event)
Then the dashboard footer's turn counter increments
And the context gauge updates

### S7: design-tree no longer renders widget
Given design-tree extension is loaded with dashboard active
When design-tree would previously render its widget
Then setWidget("design-tree", ...) is not called
And the dashboard footer shows design tree data instead

### S8: Dashboard loads after emitters
Given package.json lists dashboard after design-tree
When session_start fires
Then sharedState.designTree is already populated before the dashboard reads it

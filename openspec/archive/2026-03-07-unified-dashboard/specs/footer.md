# Custom Footer Component

The dashboard renders a custom footer via setFooter() that operates in two modes: compact (Layer 0) and raised (Layer 1). It must reimplement all built-in footer data.

## Requirements

### R1: Compact footer (Layer 0, default)
The compact footer must show: dashboard status icons + key metrics on one line, plus the original footer data (pwd, git branch, session name, token stats, cost, context%, model, thinking level) on subsequent lines.

### R2: Raised footer (Layer 1)
The raised footer must expand to show section details for design tree, openspec, and cleave, followed by the original footer data. Total height should not exceed 10 lines.

### R3: Built-in footer data reimplemented
The custom footer must display: pwd (with ~ for home), git branch, session name, cumulative token stats (input/output/cacheRead/cacheWrite), cumulative cost, context window percentage, model name with provider (when multi-provider), thinking level indicator (for reasoning models), and extension statuses.

### R4: Context gauge from status-bar absorbed
The context gauge (turn counter + memory/conversation/free bar + percentage) currently in status-bar.ts must be rendered in the compact footer line, replacing the separate setStatus call.

### R5: Design tree section in raised mode
When raised, the footer must show: node status summary, focused node with open questions count, and first open question text.

### R6: OpenSpec section in raised mode
When raised, the footer must show: each change with status icon, task progress (done/total), and stage tags.

### R7: Cleave section in raised mode
When raised and cleave is idle, show "idle". When dispatching, show per-child status (pending/running/done/failed) with labels and elapsed time.

### R8: Theme compliance
All text must use ctx.ui.theme color functions. Status severity colors: decided=success, exploring=accent, blocked=error, deferred=muted. invalidate() must rebuild all themed content.

## Scenarios

### S1: Compact footer renders dashboard summary line
Given sharedState has designTree (4 nodes, 3 decided), openspec (2 changes), cleave (idle)
When the footer renders in compact mode at width 120
Then the first line contains "◈ D:3/4" and "◎ OS:2" and "⚡ idle"
And the second line contains pwd with git branch
And the third line contains token stats and model name

### S2: Compact footer renders context gauge
Given status-bar.ts is deleted
And the dashboard footer renders in compact mode
When context usage is 45%
Then the compact line includes the turn counter and context bar with correct severity color

### S3: Raised footer shows design tree detail
Given sharedState.designTree has focusedNode with 3 open questions
When footer is in raised mode
Then it shows the focused node title and "3 open questions"
And it shows the first open question text

### S4: Raised footer shows OpenSpec changes
Given sharedState.openspec has 2 changes: one with 16/16 tasks, one with 0/31 tasks
When footer is in raised mode
Then it shows "✓ scenario-first 16/16" with success color
And it shows "◦ skill-aware 0/31" with muted color

### S5: Raised footer shows live cleave progress
Given sharedState.cleave has status "dispatching" with 3 children (1 done, 1 running, 1 pending)
When footer is in raised mode
Then it shows "⚡ Cleave dispatching: 1/3 ✓"
And it shows child statuses with labels

### S6: Footer stays under 10 lines when raised
Given all three sections have data
When footer renders in raised mode
Then total line count is 10 or fewer

### S7: Footer rebuilds on theme invalidation
Given the footer has rendered in compact mode
When invalidate() is called (theme change)
Then all themed strings are rebuilt with the new theme
And the next render reflects the updated colors

### S8: Built-in footer data is complete
Given a session with cumulative token usage and cost
When the footer renders (compact or raised)
Then it shows pwd (with ~ substitution), git branch, session name (if set), input/output/cache tokens, cost with $ prefix, context window percentage, model ID, and thinking level (if reasoning model)

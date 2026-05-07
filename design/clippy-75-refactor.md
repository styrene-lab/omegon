+++
id = "524cd4bd-2536-46ee-9a57-77724a2b0725"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Remaining 75 clippy warnings — structured refactoring plan

## RefCell across await (9 warnings) — all in acp.rs

All 9 are the same pattern: `conn.borrow()` held across `.await` in the
Zed ACP protocol handler. Safe in practice (single-threaded LocalSet) but
technically unsound. Warning 1 (line 180, `send_to_worker`) is a real hazard
if concurrent tasks mutate `self.worker` during a send.

**Fix:** Clone the borrowed value or the sender before the await point.
All 9 are low-effort (3-5 lines each). Should be done in one commit.

## Large enum variants (7 warnings)

| Enum | Variant | Size | Action |
|------|---------|------|--------|
| BusRequest::EmitAgentEvent | 544B | **Box<AgentEvent>** — high priority, hot path |
| AgentEvent::TurnEnd | 540B | **Box telemetry fields** — broadcast channel cloning |
| AgentMessage::Assistant | 380B | **Box<AssistantMessage>** — conversation iteration |
| SegmentContent::ToolCard | 347B | **Box<PartialToolResult>** — TUI rendering |
| BusEvent::TurnEnd | 439B | Restructure into TurnTelemetry group |
| LlmEvent::Done | 304B | Leave — consumed immediately, no cloning |
| ResolvedPosture::Custom | 224B | Leave — singleton lifetime |

## Too many function args (12 warnings)

**Fix (5 functions):**
- `spawn_delegate()` 12 args → DelegateConfig + DelegateContext structs
- `update_routing()` 9 args → RoutingConfig struct
- `render_tool_card()` 15 args → ToolCardData struct
- `observe_turn()` 15 args → TurnTokens + TurnSignals structs
- `update_telemetry()` 9 args → remove 2 unused params (`_tool_name`, `_tool_error`)

**Leave (7 functions):** visit(), build_task_file(), on_turn_end(),
render_assistant_text(), render_choice_option(), run_interactive_active_turn(),
execute_remote_slash_command() — justified complexity or rendering functions.

## &PathBuf → &Path (4 warnings)

Change function signatures from `&PathBuf` to `&Path`. No breaking changes
since no external consumers. Locations: extensions/state.rs (2), delegate.rs (2).

## Implementation order

1. Remove unused params in update_telemetry() — 1 minute
2. &PathBuf → &Path — 4 signatures, no behavior change
3. RefCell fixes in acp.rs — 9 clone-before-await patterns
4. Box large enum variants — 4 high-priority boxes
5. Config struct refactors — 4 functions, needs test updates

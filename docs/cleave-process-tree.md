+++
id = "710b84ef-63fd-4679-bdc8-e42afb5664d7"
kind = "document"
title = "Cleave Process Tree — bidirectional parent↔child coordination"
status = "implemented"
tags = ["architecture", "cleave", "subprocess", "ipc", "coordination", "strategic"]
aliases = ["cleave-process-tree"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
issue_type = "feature"
open_questions = []
openspec_change = "cleave-process-tree"
priority = "2"
related = ["multi-instance-coordination", "cleave-child-observability"]
+++

# Cleave Process Tree — bidirectional parent↔child coordination

## Overview

Replace cleave's current fire-and-forget task-file protocol with bidirectional parent↔child communication. Children are trusted subprocesses spawned by Omegon — no discovery, no auth, no HTTP overhead. The goal is enabling mid-task negotiation (child asks parent for input), sibling awareness (children know what others have done), structured progress (richer than stdout line scraping), and coordinated resource access (shared file locks, interface contracts).

## Research

### Current cleave child communication model

**Today's protocol is entirely file-based and unidirectional:**

1. Parent writes `N-task.md` with contract, scope, and directive
2. Parent spawns child `pi -p --no-session` with prompt on stdin
3. Child executes in isolated git worktree
4. Parent scrapes child stdout line-by-line for dashboard status (debounced, heuristic filtering via `isChildStatusLine`)
5. Child writes results back to `N-task.md` (Status, Summary, Artifacts, Decisions, Interfaces)
6. Parent reads task file post-exit to determine `SUCCESS/PARTIAL/FAILED/NEEDS_DECOMPOSITION`
7. Parent merges child's branch back to base

**What works:** Isolation (worktrees), parallelism (wave dispatch), result harvesting (task files), review loop (re-run in same worktree).

**What doesn't work:**
- **No mid-task input:** Child gets stuck → fails. No way to ask parent "which approach should I take?" or "this file conflicts with sibling, what do I do?"
- **No sibling awareness:** Children don't know what other children are doing. Two children modifying the same interface can't coordinate at execution time — conflicts are detected only at merge.
- **Stdout scraping is lossy:** `isChildStatusLine` is a heuristic filter. Structured progress (% complete, files touched, decisions made) requires parsing free-text output.
- **No partial result streaming:** Parent only sees the final task file. A child producing useful intermediate artifacts (e.g., an interface definition that another child needs) can't share them until it's done.
- **Exit code is unreliable:** Child might exit 0 but write FAILED in the task file, or exit non-zero but have completed useful work. The status determination logic in `dispatchSingleChild` has multiple fallback paths to reconcile this.

### IPC mechanism candidates

All candidates must work with `spawn()` detached child processes on macOS and Linux.

**1. Structured stdin/stdout JSON lines (JSONL over stdio)**
- Parent writes JSON messages to child stdin, child writes JSON messages to stdout
- Zero infrastructure — uses existing process pipes
- Requires a framing protocol: each line is a complete JSON object with `type` field
- Challenge: pi's `-p` mode currently reads the full prompt from stdin then closes it. Would need a mode where stdin stays open for ongoing messages.
- Challenge: child stdout is currently consumed for "the LLM response". Structured messages would need to be multiplexed with or replace the free-text output.

**2. Unix domain socket per child**
- Parent creates a socket, passes path to child via env var
- Bidirectional, reliable, well-understood
- Works across worktree boundaries (socket lives in `/tmp` or `~/.pi/cleave/`)
- Slightly more setup than stdio but cleaner separation of control channel from output
- Node.js `net.createServer` / `net.connect` — no dependencies

**3. Named pipe (FIFO)**
- One pipe per direction (parent→child, child→parent)
- Simpler than sockets but less flexible (no multiplexing)
- macOS/Linux compatible

**4. Shared file with inotify/fswatch**
- Children write to a shared coordination file, parent watches
- Fragile, platform-dependent, race-prone
- Not recommended

**5. Localhost HTTP**
- Parent runs HTTP server, children POST to it
- Works but adds HTTP overhead for co-located processes
- This is what A2A does — already rejected for this use case

**Recommendation: Option 2 (Unix domain socket) for the control channel.** Stdio stays for prompt delivery (one-shot) and output capture (for review). The socket is a separate bidirectional channel for structured coordination messages independent of the LLM I/O.

### Message types for parent↔child protocol

A minimal message set that solves the identified gaps:

**Parent → Child:**
- `sibling_update {childId, label, event: "completed"|"published_interface"|"decision", data}` — Inform child about sibling progress. Enables reactive coordination.
- `input_response {requestId, content}` — Reply to a child's input request.
- `abort {reason}` — Tell child to stop (replaces SIGTERM for graceful shutdown).

**Child → Parent:**
- `progress {percent?, phase?, filesModified?, message}` — Structured progress replacing stdout scraping.
- `input_request {requestId, question, context, options?}` — Ask parent for guidance. Parent can auto-resolve, delegate to operator, or escalate.
- `publish {type: "interface"|"decision"|"artifact", name, content}` — Announce an intermediate result that siblings might need.
- `status {status: "working"|"blocked"|"completed"|"failed", summary?}` — Explicit lifecycle state changes (replaces exit-code + task-file reconciliation).

**Key design principle:** Messages are advisory, not blocking. A child that never connects to the socket still works exactly as today — the protocol is an enhancement layer, not a requirement. This preserves backward compatibility with the current fire-and-forget model.

### Critical finding: pi already has RPC mode with bidirectional JSON-RPC over stdio

**Pi's `--mode rpc` (vendor/pi-mono/packages/coding-agent/src/modes/rpc/rpc-mode.ts) provides everything the coordination channel needs — no custom IPC required.**

The RPC mode:
1. Reads JSON commands from stdin (parent → child)
2. Writes JSON events + responses to stdout (child → parent)
3. Stays alive between messages (not one-shot like `-p` mode)
4. Full bidirectional session control:
   - `prompt` — send the initial task
   - `steer` — inject a steering message into the conversation mid-turn (sibling updates!)
   - `follow_up` — add a follow-up message after the current turn (input responses!)
   - `abort` — graceful cancellation
   - `get_state` — query session state (structured progress!)
5. Extension UI proxying: `select`, `confirm`, `input`, `notify` all route through the JSON channel — the parent can answer on behalf of the child
6. All agent events (tool_call, tool_result, assistant_message, etc.) stream as JSON objects on stdout

**Current cleave spawns:** `pi -p --no-session` — writes prompt to stdin, closes stdin, scrapes stdout for human-readable lines.

**Proposed:** `pi --mode rpc --no-session` — writes `{type: "prompt", message: taskContent}` to stdin, reads structured JSON events from stdout, can inject `steer`/`follow_up` messages at any time.

**What this obsoletes from our earlier research:**
- Unix domain sockets — not needed, stdin/stdout already bidirectional in RPC mode
- Named pipes — same
- Custom message types — RPC mode already has prompt/steer/follow_up/abort/get_state
- Custom progress protocol — agent events already stream as structured JSON

**What this answers for our open questions:**
- Q1 (how does the child use the channel): The child doesn't need a special tool. The parent injects messages via `steer`/`follow_up` which appear as user messages in the child's conversation. The LLM sees them naturally.
- Q3 (how does sibling_update change behavior): Parent sends a `steer` message like "Sibling 'api-layer' just published interface X: {...}. Incorporate this if relevant to your scope." The child LLM processes it as a normal context injection.
- Q2 (blocking vs async): `steer` is fire-and-forget from the parent's side — it's injected into the child's context at the next opportunity. No blocking required.

**Remaining risk:**
- `--mode rpc` with `--no-session` compatibility — needs verification
- Extension loading in RPC children — `bindExtensions()` is called in `runRpcMode`, so Omegon extensions should load. But needs testing.
- Stdout is now JSON-only — the current `onLine` heuristic parser must be replaced with JSON event parsing. This is a clean improvement, not a risk.
- Steer message timing: if the child LLM is mid-generation, when does the steer get processed? Pi's RPC mode queues steers and they're picked up between turns. For sibling updates this is fine (advisory, not urgent). For abort, there's an explicit abort command.

### Revised architecture: RPC mode as the coordination channel

**The Unix domain socket proposal is superseded.** Pi's built-in RPC mode already provides the exact bidirectional JSON-RPC channel needed. The change is purely in how cleave spawns and communicates with children — no new IPC infrastructure at all.

**Current spawn:** `omegon -p --no-session` (pipe mode)
- stdin: full prompt text, then close
- stdout: free-text LLM output, heuristically scraped
- lifecycle: fire-and-forget, exit code + task file for result

**Proposed spawn:** `omegon --mode rpc --no-session`
- stdin: JSON-RPC commands, stays open for the session lifetime
- stdout: structured JSON events (AgentSessionEvent) + RPC responses + extension UI requests
- lifecycle: parent sends `prompt` to start work, monitors events, can `steer`/`follow_up`/`abort` at any time

**How this answers each gap:**

| Gap | RPC Solution |
|-----|-------------|
| Mid-task input | Child's extension calls `ui.select()` or `ui.input()` → RPC emits `extension_ui_request` to stdout → parent reads it, decides, sends `extension_ui_response` on stdin |
| Sibling awareness | Parent sends `steer` command with sibling update text → child LLM sees it as injected context on next turn |
| Structured progress | All agent events stream as typed JSON: `message_start`, `tool_call`, `tool_result`, `message_end`, `auto_compaction_start/end` — parent parses these instead of scraping |
| Reliable status | Parent monitors `message_end` events for completion, `tool_result` events for file modifications, `abort` for cancellation. No exit-code reconciliation needed. |
| Graceful shutdown | Parent sends `abort` command → child gracefully stops and exits |

**Critical reintegration safety property:** The task file contract is UNCHANGED. Children still write their results to `N-task.md` with Status/Summary/Artifacts. The merge process is unchanged. RPC mode is an *enhancement to the communication channel*, not a replacement for the coordination contract. A child that loses its RPC connection (parent crash, pipe break) still has its worktree, its task file, and its git branch — it degrades to today's behavior.

**Extension loading:** `runRpcMode` calls `session.bindExtensions()` with a full extension UI context. All Omegon extensions load in RPC children, including the cleave extension itself (though nested cleave would be unusual). Skills, tools, and commands all work.

**Risk: stdout is no longer human-readable.** The review loop in `review.ts` currently reads `result.stdout` to parse review verdicts. In RPC mode, stdout is JSON events. The review prompt/result would need to be extracted from the structured events (specifically the final `message_end` event's content) rather than from raw stdout. This is a tractable change — the `executeWithReview` function's `ReviewExecutor.execute()` interface would return the structured event stream instead of raw text.

### Reintegration safety analysis — can RPC children go off the rails?

**Failure mode analysis for RPC children vs current pipe children:**

**1. Steer injection causes child to abandon its task scope**
Risk: Parent sends `steer` with sibling info → child LLM decides the sibling's work is more interesting and starts modifying files outside its scope.
Mitigation: The contract prompt is the FIRST message. Steers arrive later and have lower priority in the LLM's context. The existing scope enforcement ("Only work on files within your task scope") is already in the contract. Steers should be clearly framed as informational: "FYI: sibling published X. Do NOT change your scope."
Residual risk: LOW. Same risk as today if the task prompt mentioned other components — LLMs occasionally wander. The worktree + branch isolation limits blast radius.

**2. Extension UI request blocks forever — child hangs**
Risk: Child's extension calls `ui.input()` → emits extension_ui_request → parent never responds → child hangs waiting.
Mitigation: RPC mode's `createDialogPromise` already supports `timeout` and `signal` (AbortSignal). If the parent doesn't respond within the timeout, the dialog resolves with the default value. The existing `childTimeoutMs` in the dispatcher enforces an outer deadline.
Residual risk: LOW. Timeouts are already built into the protocol.

**3. RPC connection breaks mid-execution (pipe closed)**
Risk: Parent crashes → stdin closes → child's `attachJsonlLineReader` gets EOF → child can't receive new commands.
Mitigation: The child is a detached process with its own worktree. Losing the RPC channel means it can't receive steers or aborts, but it still has its task prompt in memory and will continue executing. When it finishes, it writes to the task file and exits. The parent (on restart, or the subprocess-tracker cleanup) can still read the task file and merge the branch.
Residual risk: LOW. Degrades to exactly today's behavior — fire-and-forget with task file result.

**4. Steer messages pollute the context window — child runs out of space**
Risk: Parent sends many steer messages (frequent sibling updates) → child's context fills with coordination noise → compaction discards important task context.
Mitigation: Parent should send steers sparingly — only for published interfaces/decisions that are directly relevant to this child's scope (check file scope overlap before sending). Rate-limit to at most 1 steer per sibling completion, and only if the sibling's published artifacts overlap this child's scope.
Residual risk: MEDIUM. This needs a deliberate throttling policy. Unbounded steers would genuinely degrade child performance.

**5. Review loop breaks because stdout is JSON**
Risk: `executeWithReview` currently reads `result.stdout` as the LLM's text output. In RPC mode, stdout is JSON events.
Mitigation: The `ReviewExecutor` interface abstraction already exists. `execute()` and `review()` return `{exitCode, stdout, stderr}`. For RPC children, `stdout` would be reconstructed from `message_end` events — extracting the text content from the structured event stream. The interface doesn't change; the implementation behind it does.
Residual risk: LOW. Well-defined extraction point.

**6. Wave dispatch and dependency ordering still work?**
Yes. RPC mode doesn't change the wave structure. Parent still dispatches waves in order, waits for all children in a wave to finish before starting the next wave. The difference is "finish" is detected from the event stream (final `message_end` → task file write → process exit) rather than just process exit + task file read.

**7. Merge conflicts are detected the same way?**
Yes. Branch isolation and merge are git operations. The child's communication protocol doesn't affect how branches are merged. `conflicts.ts` detects conflicts from task file content (file claims, interfaces, decisions) — those are still written to task files.

**Conclusion:** RPC mode does NOT increase reintegration risk. The task file contract and git worktree isolation are the actual coordination boundaries, and those are unchanged. RPC is a communication channel enhancement that runs alongside the existing contract. The one new risk (steer context pollution) is mitigable with a throttling policy and is strictly opt-in — no steers are sent unless the feature is enabled.

### Open question resolutions

**Q1: How does the child use the coordination channel?**
RESOLVED: No custom tool needed. Pi's RPC mode already handles this:
- Progress: agent events stream automatically (no child action required)
- Input requests: extensions call `ui.input()`/`ui.select()` → RPC proxies to parent → parent responds
- Sibling awareness: parent sends `steer` command → appears as injected user message in child conversation
The child LLM doesn't need to "know" it's being coordinated. It works normally and the RPC channel mediates.

**Q2: Blocking vs async for input requests?**
RESOLVED: Semi-blocking with timeout. RPC mode's `createDialogPromise` already implements this — the extension call blocks the child's tool execution while waiting for the parent's `extension_ui_response`, but with a configurable timeout and AbortSignal support. If the parent doesn't respond, the dialog returns a default value and the child continues.

**Q3: How does sibling_update reach the child?**
RESOLVED: Parent sends `steer` command. The steer message is injected into the child's conversation as a user message at the next turn boundary. The LLM reads it as natural context. Steers should be informational and explicitly scope-preserving: "Sibling 'api-layer' completed and published interface AuthService. If your work depends on this interface, use it. Do not modify files outside your scope."

**Q4: Minimum viable version?**
RESOLVED: Yes — MVP is spawn children in RPC mode + parse JSON events instead of scraping stdout. No steers, no input proxying, no sibling coordination. Just structured observability. This alone eliminates:
- `isChildStatusLine` heuristic filtering
- `stripAnsiForStatus` processing
- Exit code ↔ task file status reconciliation
- The debounced `onChildLine` callback

MVP scope: modify `spawnChild()` to use `--mode rpc`, replace stdout line parsing with JSON event parsing, emit structured progress to the dashboard. Everything else (steers, input proxying, sibling updates) is Phase 2.

## Decisions

### Decision: Use pi's built-in RPC mode as the cleave child coordination channel

**Status:** decided
**Rationale:** Pi's --mode rpc provides bidirectional JSON-RPC over stdin/stdout with prompt/steer/follow_up/abort commands and typed AgentSessionEvent streaming. No custom IPC needed. Task file contract and git worktree isolation are unchanged — RPC is a communication enhancement, not a coordination contract replacement. Reintegration safety analysis shows all failure modes are LOW risk except steer context pollution (MEDIUM, mitigable with throttling, Phase 2 only).

### Decision: Phase 1 (MVP): RPC spawn + structured event parsing, no steers or input proxying

**Status:** decided
**Rationale:** MVP swaps spawnChild() from -p to --mode rpc, replaces stdout line scraping with JSON event parsing, and emits structured progress to the dashboard. No sibling coordination, no input proxying. This alone eliminates isChildStatusLine heuristic, stripAnsiForStatus, exit-code reconciliation, and the debounced onChildLine callback. Low risk, high observability gain.

### Decision: Phase 2: Sibling awareness via steer, input proxying via extension UI, structured abort

**Status:** decided
**Rationale:** Steer injection, extension UI request proxying, and graceful abort require the steer throttling policy and review loop compatibility questions to be resolved first. These are high-value features but depend on Phase 1 being stable.

### Decision: Review loop stays on pipe mode for MVP, migrates to RPC in Phase 2

**Status:** decided
**Rationale:** The review subprocess is a separate spawn from the execution subprocess. It can continue using -p mode while the primary execution child uses RPC. This decouples the review migration from the core dispatch migration and reduces MVP scope.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/dispatcher.ts` (modified) — Replace spawnChild() to use --mode rpc. Replace stdout line parsing with JSON event stream parsing. Replace onLine callback with structured event handler. Remove isChildStatusLine, stripAnsiForStatus. Adapt dispatchSingleChild result extraction from JSON events.
- `extensions/cleave/dispatcher.test.ts` (modified) — Update spawn mock to emit JSON events instead of text lines. Test RPC prompt command construction. Test event stream parsing for progress, tool_call, message_end. Test graceful degradation when RPC pipe breaks.
- `extensions/cleave/rpc-child.ts` (new) — New module: RPC child communication helpers. JSON line framing for stdin commands. Event stream parser for stdout. Event-to-progress mapping (AgentSessionEvent → dashboard progress). Typed wrappers for prompt/abort commands.
- `extensions/cleave/rpc-child.test.ts` (new) — Tests for RPC child communication: JSON framing, event parsing, prompt command construction, abort handling, pipe-break degradation.
- `extensions/cleave/review.ts` (modified) — Keep review subprocess on pipe mode (MVP decision). No changes in Phase 1.
- `extensions/cleave/index.ts` (modified) — Update emitCleaveChildProgress to consume structured events instead of debounced stdout lines. Remove onChildLine debounce timer from dispatchSingleChild.
- `extensions/cleave/types.ts` (modified) — Add RPC-specific types: RpcChildEvent, RpcProgressUpdate, RpcChildState.
- `skills/cleave/SKILL.md` (modified) — Document RPC mode dispatch, structured event streaming, Phase 1 vs Phase 2 capabilities.

### Constraints

- Task file contract (N-task.md with Status/Summary/Artifacts) must not change — RPC is communication enhancement only
- Review subprocess stays on pipe mode in Phase 1 — decouple review migration from core dispatch
- Child must degrade gracefully to fire-and-forget behavior if RPC pipe breaks
- No steer injection in Phase 1 — structured observability only
- spawnChild must support both RPC and pipe mode during transition (feature flag or per-call option)

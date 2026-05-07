+++
id = "bf3de358-efcf-4f02-8738-c80ae2a2a181"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave Process Tree — bidirectional parent↔child coordination — Design Spec (extracted)

> Auto-extracted from docs/cleave-process-tree.md at decide-time.

## Decisions

### Use pi's built-in RPC mode as the cleave child coordination channel (decided)

Pi's --mode rpc provides bidirectional JSON-RPC over stdin/stdout with prompt/steer/follow_up/abort commands and typed AgentSessionEvent streaming. No custom IPC needed. Task file contract and git worktree isolation are unchanged — RPC is a communication enhancement, not a coordination contract replacement. Reintegration safety analysis shows all failure modes are LOW risk except steer context pollution (MEDIUM, mitigable with throttling, Phase 2 only).

### Phase 1 (MVP): RPC spawn + structured event parsing, no steers or input proxying (decided)

MVP swaps spawnChild() from -p to --mode rpc, replaces stdout line scraping with JSON event parsing, and emits structured progress to the dashboard. No sibling coordination, no input proxying. This alone eliminates isChildStatusLine heuristic, stripAnsiForStatus, exit-code reconciliation, and the debounced onChildLine callback. Low risk, high observability gain.

### Phase 2: Sibling awareness via steer, input proxying via extension UI, structured abort (decided)

Steer injection, extension UI request proxying, and graceful abort require the steer throttling policy and review loop compatibility questions to be resolved first. These are high-value features but depend on Phase 1 being stable.

### Review loop stays on pipe mode for MVP, migrates to RPC in Phase 2 (decided)

The review subprocess is a separate spawn from the execution subprocess. It can continue using -p mode while the primary execution child uses RPC. This decouples the review migration from the core dispatch migration and reduces MVP scope.

## Research Summary

### Current cleave child communication model

**Today's protocol is entirely file-based and unidirectional:**

1. Parent writes `N-task.md` with contract, scope, and directive
2. Parent spawns child `pi -p --no-session` with prompt on stdin
3. Child executes in isolated git worktree
4. Parent scrapes child stdout line-by-line for dashboard status (debounced, heuristic filtering via `isChildStatusLine`)
5. Child writes results back to `N-task.md` (Status, Summary, Artifacts, Decisions, Interfaces)
6. Parent reads task file post-exit to deter…

### IPC mechanism candidates

All candidates must work with `spawn()` detached child processes on macOS and Linux.

**1. Structured stdin/stdout JSON lines (JSONL over stdio)**
- Parent writes JSON messages to child stdin, child writes JSON messages to stdout
- Zero infrastructure — uses existing process pipes
- Requires a framing protocol: each line is a complete JSON object with `type` field
- Challenge: pi's `-p` mode currently reads the full prompt from stdin then closes it. Would need a mode where stdin stays open for o…

### Message types for parent↔child protocol

A minimal message set that solves the identified gaps:

**Parent → Child:**
- `sibling_update {childId, label, event: "completed"|"published_interface"|"decision", data}` — Inform child about sibling progress. Enables reactive coordination.
- `input_response {requestId, content}` — Reply to a child's input request.
- `abort {reason}` — Tell child to stop (replaces SIGTERM for graceful shutdown).

**Child → Parent:**
- `progress {percent?, phase?, filesModified?, message}` — Structured progress r…

### Critical finding: pi already has RPC mode with bidirectional JSON-RPC over stdio

**Pi's `--mode rpc` (vendor/pi-mono/packages/coding-agent/src/modes/rpc/rpc-mode.ts) provides everything the coordination channel needs — no custom IPC required.**

The RPC mode:
1. Reads JSON commands from stdin (parent → child)
2. Writes JSON events + responses to stdout (child → parent)
3. Stays alive between messages (not one-shot like `-p` mode)
4. Full bidirectional session control:
   - `prompt` — send the initial task
   - `steer` — inject a steering message into the conversation mid-tur…

### Revised architecture: RPC mode as the coordination channel

**The Unix domain socket proposal is superseded.** Pi's built-in RPC mode already provides the exact bidirectional JSON-RPC channel needed. The change is purely in how cleave spawns and communicates with children — no new IPC infrastructure at all.

**Current spawn:** `omegon -p --no-session` (pipe mode)
- stdin: full prompt text, then close
- stdout: free-text LLM output, heuristically scraped
- lifecycle: fire-and-forget, exit code + task file for result

**Proposed spawn:** `omegon --mode rpc…

### Reintegration safety analysis — can RPC children go off the rails?

**Failure mode analysis for RPC children vs current pipe children:**

**1. Steer injection causes child to abandon its task scope**
Risk: Parent sends `steer` with sibling info → child LLM decides the sibling's work is more interesting and starts modifying files outside its scope.
Mitigation: The contract prompt is the FIRST message. Steers arrive later and have lower priority in the LLM's context. The existing scope enforcement ("Only work on files within your task scope") is already in the con…

### Open question resolutions

**Q1: How does the child use the coordination channel?**
RESOLVED: No custom tool needed. Pi's RPC mode already handles this:
- Progress: agent events stream automatically (no child action required)
- Input requests: extensions call `ui.input()`/`ui.select()` → RPC proxies to parent → parent responds
- Sibling awareness: parent sends `steer` command → appears as injected user message in child conversation
The child LLM doesn't need to "know" it's being coordinated. It works normally and the RPC …

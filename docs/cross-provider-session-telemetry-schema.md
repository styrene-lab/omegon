+++
id = "5dbdeea6-a1c4-48e2-b3eb-de35aa94806c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cross-provider session telemetry schema for replay and inspection

## Overview

Define a provider-agnostic session/event log schema rich enough to support a claude-devtools-class inspector for Omegon across Anthropic, OpenAI-compatible providers, Codex, and local models. The schema should preserve replayability, token/cost/quota attribution, tool execution detail, context composition, model/provider switching, and subagent/cleave trees without binding the format to any single upstream provider's transcript structure.

## Research

### Assessment from cleave smoke test friction

The current Rust cleave path is observability-poor because parent/child coordination is process-scrape based rather than schema-first.

Observed architecture from code:
- `run_cleave_command()` shells into the same `omegon` binary for orchestration.
- `dispatch_child()` spawns full child processes via `current_exe()` with `--prompt-file --cwd --model --max-turns`.
- Parent watches **child stderr only** for liveness/activity and uses regex-like parsing (`parse_child_activity`) to infer turns and tool calls from tracing text.
- Child stdout is only read after exit; during execution it is not part of the progress channel.
- Idle timeout is keyed to absence of stderr lines, not semantic progress heartbeats.

Why this feels brittle in practice:
1. **No structured RPC/event stream between parent and child.** The parent is scraping log lines, not consuming typed events.
2. **Silence looks like failure.** Local / cold-start models can spend a long time loading or thinking without emitting stderr, so the orchestrator interprets quiescence as stuck.
3. **Progress is lossy.** Only patterns like `→ tool` and `Turn N` are surfaced; context projection, retry classification, provider switching, and token/quota data are invisible.
4. **Nested CLI invocation is expensive and opaque.** A full Omegon child process is launched for each task even when the parent already has a harness and event bus.
5. **The transport boundary is stdout/stderr, not a canonical telemetry schema.** That prevents a claude-devtools-class replay/inspector from understanding cleave runs cleanly.

Assessment: the problem is not merely 'cleave is buggy'; it is that child execution currently uses a weak transport. This is exactly the kind of area the cross-provider session telemetry schema should cover: typed child-session lifecycle events, semantic heartbeats, progress snapshots, and structured completion payloads rather than log scraping.

## Open Questions

*No open questions.*

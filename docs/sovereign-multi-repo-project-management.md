---
id: sovereign-multi-repo-project-management
title: Sovereign multi-repo project management on top of omegon-design
status: exploring
parent: git-native-task-management
tags: [forgejo, multi-repo, sovereign, project-management, web]
open_questions: []
jj_change_id: urroornuzoyklopmyzxtuytzwknnxtqp
issue_type: epic
priority: 3
---

# Sovereign multi-repo project management on top of omegon-design

## Overview

Use the extracted omegon-design crate as the domain layer for a separate project-management application that aggregates multiple repos, likely alongside Forgejo. Provide a unified cross-project view while keeping each repo's `.omegon/design/` as the source of truth.

## Research

### Reference project: claude-devtools

claude-devtools is an adjacent reference point, but it solves a different layer of the stack. Based on its site and GitHub README summaries, it is a read-only inspector for Claude Code sessions rather than a project-management system.

What it appears to do:
- Reads raw Claude Code session logs from `~/.claude/`.
- Reconstructs execution traces: file reads, regex searches, edit diffs, bash output, subagent trees, and token usage.
- Rebuilds per-turn context composition from recorded artifacts such as `CLAUDE.md` injections, skill activations, `@`-mentioned files, tool I/O, thinking, team overhead, and user text.
- Presents that reconstruction in a searchable desktop/web UI, including SSH access to inspect remote machines.

How it gains its 'insights':
- Not by instrumenting or wrapping Claude Code in real time.
- Not by proprietary provider-side introspection.
- By parsing durable local session artifacts Claude Code already writes, then replaying / classifying them into higher-level categories.

Implication for Omegon:
- This validates the product value of post-hoc observability over agent sessions.
- But its source of truth is session telemetry, whereas sovereign multi-repo project management should use repo-native state (`.omegon/design/`, OpenSpec changes, milestones, sessions) as the primary model.
- The strongest analogue for us is an inspector/dashboard layer over our own durable artifacts, not a wrapper around live execution.

### What Claude-class session richness Omegon should log

A claude-devtools-compatible experience for Omegon requires durable, structured event logging that is richer than a plain transcript. The key lesson from Claude's ecosystem is not 'copy their file format' but 'persist enough semantic structure that a replay engine can reconstruct execution, context, and cost after the fact.'

Minimum richness Omegon should emit across all providers/models:

1. **Session identity and topology**
   - session_id, parent_session_id, root_session_id
   - agent_id / child-agent label / cleave child label
   - repo root, cwd, branch, worktree, host identity
   - start/end timestamps, status, restart/recovery markers

2. **Turn lifecycle events**
   - user turn opened
   - model request built
   - streaming started / delta received / completed / errored
   - tool phase started/completed
   - compaction/decay/rebuild triggered
   - retry classification (transient/context-overflow/malformed-history)

3. **Canonical message content**
   - provider-agnostic normalized messages actually sent/received
   - canonical tool IDs and provider-specific blobs/IDs
   - assistant text, tool_use, tool_result, thinking summaries/availability markers
   - dropped/redacted blocks logged as such, not silently discarded

4. **Context composition / projection**
   - what entered the request window for this turn
   - system prompt components
   - memory injections
   - focused design node / spec injections
   - tool catalog included for the turn
   - selected historical messages / projected buffer segments
   - compaction summaries or decay decisions

5. **Token / cost / quota accounting**
   - provider-reported input/output/cache tokens when available
   - estimated attribution by category: system prompt, memory, design/spec context, tool schema, conversation history, user text, thinking, tool I/O
   - rate-limit headers, remaining quota, reset hints, retry-after
   - model/provider chosen and switching reason

6. **Tool execution detail**
   - tool name, arguments, redaction policy, duration, exit status
   - rich result metadata: files touched, bytes/lines returned, truncation markers, diff hunks, stderr/stdout lengths
   - security-sensitive access markers (.env, secrets, auth files)

7. **Parallel/subagent structure**
   - child session creation events
   - task delegation prompt summary
   - linkage between parent turn and child execution tree
   - harvested result summary, merge/conflict outcome

8. **Outcome semantics**
   - files changed
   - tests run + pass/fail
   - commits created / branch moved / OpenSpec lifecycle transition
   - design-tree node touched / milestone impacted

If Omegon emits this as a stable canonical schema, a claude-devtools-like inspector can work on Omegon directly, even while provider/projector internals differ underneath.

## Open Questions

*No open questions.*

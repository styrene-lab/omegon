---
id: omegon-session
title: Omegon Session — provider-agnostic session identity and resumption
status: exploring
related: [omega, model-degradation]
tags: [architecture, orchestration, session, core]
open_questions: []
issue_type: feature
priority: 2
---

# Omegon Session — provider-agnostic session identity and resumption

## Overview

Omegon sessions are not model sessions. A single Omegon session may span multiple providers (anthropic for the driver, openai for embeddings, copilot for cleave children, local for image generation), multiple cleave worktrees executing in parallel, and tier switches mid-conversation. Claude Code can resume because it owns the entire conversation with one provider. Omegon cannot — it orchestrates across providers, so the session boundary must be defined at the harness level, not the provider level.\n\nThe exit card already serializes key session state (git branch, design tree shape, openspec changes, memory delta). This is the seed of a session manifest — a serializable snapshot that captures enough context to resume work without replaying the conversation.\n\nThis design explores: what IS an Omegon session, what state defines it, how is it saved/restored, and how does this compose with the future orchestration layer (omega).

## Research

### Existing state systems and their session coverage

Omegon already captures substantial session state across several systems, but none of them own the concept of "a session" holistically:\n\n**Project Memory** (`extensions/project-memory/`):\n- Facts (1400+) persist across sessions — architecture, decisions, constraints, patterns\n- Episodic narratives generated at shutdown: title + narrative summarizing goals/decisions/outcomes\n- Working memory buffer: 25 pinned facts that survive compaction\n- Startup injection: up to 12K chars of relevant facts injected into first turn\n- *Gap:* Memory captures what was learned, not what was being done\n\n**Design Tree** (`extensions/design-tree/`):\n- Node statuses track lifecycle: seed → exploring → resolved → decided → implementing → implemented\n- Focused node injected into conversation context\n- Dashboard state shared via `sharedState.designTree`\n- *Gap:* Knows what's being designed, not what the operator was actively working on this session\n\n**OpenSpec** (`extensions/openspec/`):\n- Active changes with stages: proposed → specced → planned → implementing → verifying\n- Tasks.md tracks individual work items\n- *Gap:* Knows what changes exist, not which ones were touched this session\n\n**Git state:**\n- Branch, dirty files, recent commits\n- Cleave worktrees tracked in `.git/worktrees/`\n- *Gap:* No session binding — git doesn't know about Omegon sessions\n\n**Pi conversation branch:**\n- Full message history with tool calls and results\n- Compaction summaries when context overflows\n- Session files in `sessions/` directory (JSONL per-session per-cwd)\n- *Gap:* Provider-specific — tied to whichever model drove the conversation\n\n**Exit card data** (just implemented):\n- Branch + dirty count, design node counts, openspec active changes, fact count + delta, embedding coverage\n- This is already a mini-manifest — the question is whether to formalize and extend it

### What Claude Code does (and why we can't just copy it)

Claude Code resumes by replaying the conversation history to Claude. This works because:\n1. One provider (Anthropic) owns the entire conversation\n2. The conversation IS the session — all context lives in the message history\n3. Compaction/summarization reduces the history to fit context windows\n\nOmegon can't do this because:\n1. Multiple providers may have been used — the driver may have been anthropic, but cleave children used copilot, embeddings used openai, and image gen used local FLUX\n2. The conversation history belongs to the pi harness, not to any provider\n3. A session may include parallel execution (cleave) where 4 separate conversations happened simultaneously in worktrees\n4. Tier switches mid-session mean different models saw different parts of the conversation\n5. The operator's intent and progress are distributed across multiple state systems (memory, design tree, openspec, git)\n\nThe fundamental difference: Claude Code's session IS a conversation. Omegon's session is an **orchestration** — a coordination of multiple conversations, state changes, and parallel executions toward a goal.

### Session manifest sketch — what gets saved at /exit

A session manifest is a serializable snapshot written at exit (or periodically). It captures enough to answer: "What was the operator doing, what's the state of that work, and what should happen next?"\n\nCandidate structure:\n```\n{\n  id: uuid,\n  timestamp: ISO8601,\n  cwd: string,\n  duration_minutes: number,\n  \n  // Intent — what was the operator trying to accomplish?\n  goals: string[],              // extracted from conversation or explicit\n  nextSteps: string[],          // operator-stated or inferred\n  \n  // Git context\n  git: {\n    branch: string,\n    dirty: number,\n    recentCommits: { hash, subject }[],  // this session's commits\n    worktrees: string[],                  // active cleave worktrees\n  },\n  \n  // Design state\n  designTree: {\n    focusedNode?: string,\n    activeNodes: { id, status, questionsRemaining }[],\n    transitionsThisSession: { id, from, to }[],\n  },\n  \n  // OpenSpec state\n  openspec: {\n    activeChanges: { name, stage }[],\n    changesAdvancedThisSession: string[],\n  },\n  \n  // Memory state\n  memory: {\n    factsCreated: number,\n    factsArchived: number,\n    workingMemoryIds: string[],\n    episodeId?: string,\n  },\n  \n  // Execution context\n  execution: {\n    providersTouched: string[],\n    cleaveRunsCompleted: number,\n    modelTierAtExit: string,\n    thinkingLevelAtExit: string,\n  },\n}\n```\n\nKey insight: most of this data is already computed or available at exit time. The exit card computes git state, design tree counts, openspec changes, and memory stats. The manifest is a superset that adds temporal context (what changed THIS session) and intent (goals + next steps).

### Resumption as context injection (not replay)

The key architectural insight: session resumption in Omegon is NOT conversation replay. It's **context injection into a fresh conversation**.\n\nThis is actually what the memory system already does — injects relevant facts into the system prompt so the model has context without seeing the prior conversation. Session resumption extends this pattern:\n\n1. At exit: write session manifest to `.pi/sessions/latest.json` (or timestamped)\n2. At startup: detect prior session manifest\n3. If resume: inject manifest as a structured system message — the model receives:\n   - \"Previous session summary\" (episodic narrative)\n   - \"Active work items\" (design nodes, openspec changes)\n   - \"Session state\" (branch, uncommitted changes, in-flight tasks)\n   - \"Suggested next steps\" (from prior session's exit)\n4. The new conversation starts with full awareness of prior context WITHOUT needing the same provider or replaying messages\n\nThis is fundamentally more robust than conversation replay because:\n- Provider-agnostic: works even if you switch from anthropic to copilot between sessions\n- Context-efficient: a 500-turn session compresses to ~2K tokens of manifest\n- Composable: the manifest is just another context source alongside memory facts\n- Deterministic: no drift from re-summarizing old conversations through a different model\n\nThe compaction system already does this mid-session (compress old turns into a summary). Session resumption is compaction across the session boundary.

### Session lifecycle and the omega connection

The session concept has a natural evolution path toward omega orchestration:\n\n**Level 0 — Current state (no session identity):**\nEach pi invocation is independent. Memory and design tree provide continuity. Episodic narratives are post-hoc summaries. No explicit resume.\n\n**Level 1 — Local session manifest (this design):**\nSave/restore session state for single-operator, single-machine continuity. The exit card becomes a manifest. Startup offers resume. One session = one operator working in one repo.\n\n**Level 2 — Session as work unit:**\nA session has an explicit goal, tracks progress toward it, and knows when it's done. Multiple consecutive sessions pursuing the same goal link into a \"work stream\". Design tree nodes and openspec changes become session goals.\n\n**Level 3 — Distributed sessions (omega):**\nOmega coordinates multiple agents, each running their own session. The session manifest becomes a message format — one agent's exit manifest is another agent's resume context. Cleave is already a local prototype of this: it spawns child sessions in worktrees, each with their own conversation, and merges the results.\n\n**Level 4 — Session DAG:**\nSessions have parent-child relationships (cleave spawns children), peer relationships (two operators working the same repo), and causal relationships (session B resumed from session A's manifest). The session DAG is the execution history of a project.\n\nThis design targets Level 1, with Level 2 as a stretch, designed to not preclude Levels 3-4.

### Cross-machine resumption — the manifest must be portable

The operator may start a session on a desktop, close it, and resume on a laptop (or a different machine entirely). If the manifest is a local file in `.pi/sessions/`, it doesn't travel. This constrains the storage model.\n\nWhat DOES travel between machines already:\n- **Git** — the repo is pushed/pulled. Anything committed is portable.\n- **Memory facts** — `.pi/memory/facts.jsonl` is git-tracked (merge=union). Facts sync when the repo syncs.\n- **Design tree** — `docs/*.md` files are git-tracked. Design state syncs.\n- **OpenSpec** — `openspec/` directory is git-tracked. Change state syncs.\n\nWhat does NOT travel:\n- `.pi/memory/facts.db` — gitignored (SQLite, not mergeable)\n- `sessions/` JSONL — currently gitignored? Need to check\n- Pi conversation history — local to the pi process\n- Ollama models, embedding vectors — machine-specific\n\nImplication: **the session manifest must live in git** to be portable. This means it should be a committed file, not a database entry or a local temp file.\n\nCandidate locations:\n- `.pi/session.json` — single file, latest session only, git-tracked\n- `.pi/sessions/<id>.json` — history of sessions, git-tracked (but noisy)\n- Embedded in an existing git-tracked artifact (e.g. a fact, or an episodic narrative in facts.jsonl)\n\nThe simplest: `.pi/session.json` — a single file that always represents the last session's exit state. It's committed at exit (auto or manual), and any machine that pulls gets it. Previous sessions don't accumulate — the episodic narratives in memory already serve that archival function.\n\nCross-machine resume flow:\n1. Machine A: /exit writes `.pi/session.json` + commits (or operator commits)\n2. Machine B: git pull → pi starts → detects `.pi/session.json` → offers resume\n3. Resume: inject manifest as context, clear the file (or mark as consumed)\n4. Fresh: ignore/clear the file, start clean\n\nEdge case: machine A exits but doesn't push. Machine B has stale or no manifest. This is fine — resume is best-effort. The durable state (memory, design tree, openspec) is the source of truth. The manifest just accelerates re-orientation.

### Minimal manifest — what actually needs to be in session.json

Given: index not copy, under 1K tokens, git-portable, all references are IDs/names.\n\nThe manifest answers three questions:\n1. **What was being done?** — goals, focused design node, active openspec changes\n2. **What's the state?** — branch, dirty count, design transitions, memory delta\n3. **What's next?** — explicit next steps from the session\n\nMinimal schema:\n```json\n{\n  \"v\": 1,\n  \"id\": \"uuid\",\n  \"ts\": \"2026-03-14T14:30:00Z\",\n  \"cwd\": \"/Users/cwilson/workspace/ai/omegon\",\n  \"branch\": \"main\",\n  \"dirty\": 0,\n  \"episode\": \"Published omegon@0.6.3, implemented model degradation...\",\n  \"focus\": \"omegon-session\",\n  \"active\": [\"dash-raised-layout\", \"nix-deps\"],\n  \"next\": [\"Implement cross-tier degradation\", \"Explore nix-deps\"],\n  \"commits\": [\"a3a6534 feat(design-tree): add resolved status\", \"582fc10 feat(sci-ui): exit card\"],\n  \"facts\": { \"created\": 3, \"archived\": 1, \"total\": 1462 }\n}\n```\n\nThat's ~400 bytes of JSON. The resume injection would be even terser:\n\n```\nPrevious session (2026-03-14, 2h):\nPublished omegon@0.6.3, implemented model degradation.\nFocused on: omegon-session (exploring, 5 open questions)\nActive: dash-raised-layout, nix-deps\nCommits: feat(design-tree): add resolved status, feat(sci-ui): exit card\nNext: Implement cross-tier degradation. Explore nix-deps.\n```\n\nThat's ~80 tokens. Well under budget. The design tree focus injection and memory injection add the substance — this just provides the temporal bridge.

### Git worktrees are local-only — implications for remote execution

Git worktrees (`git worktree add`) create local filesystem directories that share the object store via symlinks into `.git/worktrees/`. They cannot span machines — the symlinks and lock files are local paths.\n\nThis means:\n- Cleave's parallel execution model is strictly local (same machine, same filesystem)\n- Remote/distributed execution (omega Level 3+) cannot use worktrees as the isolation primitive\n- The session manifest is portable (JSON in git), but the execution context it describes may reference worktrees that only exist locally\n\nFor session resumption this is fine: the manifest records that worktrees existed (as signal that cleave was active), but doesn't depend on them being present on the resuming machine. If machine B resumes and the worktrees don't exist, that just means cleave work completed or needs re-running — the merged results are in git history either way.\n\nFor omega: distributed execution will need a different isolation primitive than worktrees. Candidates: full clones on remote machines, container-isolated builds, or bare repos with sparse checkout. This is an omega-level concern, not a session-level concern. Worth flagging on the omega design node.

## Decisions

### Decision: Manifest is an index over existing artifacts, not a copy

**Status:** decided
**Rationale:** The manifest points to state that lives in durable systems (memory facts by ID, design node by ID, openspec change by name, git branch by ref). It never duplicates their content — it says "the operator was focused on node X" not "here is the full node X document". This keeps the manifest small and avoids staleness.

### Decision: Token budget: minimal — plain summary facts, no formatting overhead

**Status:** decided
**Rationale:** The session resume context should be a tight plaintext block — goals, active work pointers, next steps. No markdown headers, no bullet hierarchies, no decorative structure. Target: under 1K tokens for the resume injection. The existing systems (memory, design tree focus, openspec) already inject their own context — the manifest only needs to bridge the gap between "what those systems know" and "what the operator was doing".

### Decision: Manifest lives in git as .pi/session.json — portable by default

**Status:** decided
**Rationale:** All other durable state that travels between machines already lives in git (facts.jsonl, design docs, openspec). The session manifest should follow the same pattern. A single .pi/session.json file committed at exit. No accumulation — episodic narratives handle history. Cross-machine resume works via git pull. Staleness is acceptable because the manifest is an accelerant, not the source of truth.

### Decision: Minimal manifest: ~400 bytes JSON, ~80 token resume injection

**Status:** decided
**Rationale:** The manifest captures: session ID, timestamp, cwd, branch, dirty count, episodic summary (1 line), focused design node ID, active work item IDs, this-session commit subjects, next steps, and fact counts. All references are IDs/names — no content duplication. The resume injection is a plain text block under 100 tokens that bridges temporal context. The heavy lifting is done by memory injection and design tree focus which already have their own budgets.

### Decision: Session is repo-scoped, not branch-scoped; cleave is one session with child executions

**Status:** decided
**Rationale:** A session is one operator sitting down to work in one repo. Switching branches mid-session doesn't start a new session — the operator is still pursuing the same goal. Cleave spawning 4 worktrees is one session with 4 child executions, not 5 sessions. The manifest records the branch at exit and the commits made, but the session identity is the (repo, timestamp, operator) tuple. This aligns with how episodic narratives already work — one narrative per pi invocation per cwd.

### Decision: Manifest supplements pi conversation — operates at a higher layer

**Status:** decided
**Rationale:** Pi's conversation history is the provider-facing message stream. The session manifest operates above it — it's injected into the conversation as context (like memory facts are) but is not part of the conversation itself. A resumed session starts a fresh pi conversation with the manifest injected as a system message. This means resume works even when switching providers between sessions.

### Decision: Automatic detection with opt-out — resume by default, /fresh to skip

**Status:** exploring
**Rationale:** If .pi/session.json exists and is recent (within some staleness window), inject it automatically on startup. The operator sees a brief "Resuming session from..." notification. If they want a clean start, /fresh clears the manifest. This mirrors how memory injection works — automatic, silent, helpful. No prompt asking "resume? y/n" — that adds friction to every startup. Open question: what's the staleness window? A 2-week-old manifest from a different branch is probably noise, not signal.

### Decision: Session manifest is the unit of handoff for omega orchestration

**Status:** exploring
**Rationale:** When omega coordinates multiple agents, one agent's exit manifest becomes another's resume context. The manifest is already provider-agnostic and git-portable — it's a natural message format for inter-agent handoff. Cleave child processes are a local prototype: they receive a prompt (analogous to a manifest) and produce results that get merged. The manifest schema should be designed to work both as a file (.pi/session.json) and as a message payload in an omega coordination protocol. Not blocking Level 1 on this — just ensuring the schema doesn't preclude it.

### Decision: Auto-resume with staleness: same branch = resume, different branch or >7d = skip

**Status:** decided
**Rationale:** If .pi/session.json exists, the current branch matches the manifest's branch, and the timestamp is within 7 days — inject automatically. Different branch suggests the operator moved on. Over 7 days suggests stale context that memory facts already cover better. /fresh command to explicitly skip. No interactive prompt — just a brief notification showing what was resumed.

### Decision: Worktree references in manifest are informational, not required for resume

**Status:** decided
**Rationale:** The manifest may note that cleave worktrees were active, but resume does not depend on them existing. Worktrees are local-only (symlinks into .git/worktrees/). Cross-machine resume gracefully degrades — if worktrees are gone, merged results are in git history. Distributed execution isolation is an omega concern, not a session concern.

### Decision: Manifest schema designed as both file and message — omega-ready without omega dependency

**Status:** decided
**Rationale:** The manifest is plain JSON with no filesystem assumptions (paths are context, not requirements). It works as .pi/session.json for local resume and as a payload in a future coordination protocol. Local-only state (worktrees, embedding vectors) is informational — resume never depends on it. This means Level 1 implementation doesn't need to anticipate omega's protocol, just avoid precluding it.

## Open Questions

*No open questions.*

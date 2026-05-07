+++
id = "4090acab-009a-4614-89f7-aadaec82e024"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory System Overhaul — Reliable Cross-Session Context Continuity

## Overview

The memory system is failing its primary job: when a new session starts and the user asks a follow-up question, the agent should have enough context to answer without interrogating the user. This is a trust and UX failure. The /exit fact dump was supposed to solve this but clearly isn't. Root causes unknown — could be extraction gaps (facts not stored at exit), retrieval gaps (facts stored but not found), injection gaps (facts found but not surfaced at startup), or coverage gaps (not enough context captured per topic).

## Research

### Injection Pipeline Audit

Context injection fires on `before_tool_call`/user message event. Mode selection: if embeddings available AND ≥50% of facts are vectorized AND userText.length > 10 AND factCount > 20 → semantic mode (core sections + top-20 semantic hits + working memory, capped at 30 facts). Otherwise: bulk dump (up to 50 facts). Core sections hard-wired to ["Constraints", "Specs"] — NOT Architecture or Decisions. This means Architecture facts (where most project state lives) are NOT guaranteed injection; they only appear if semantically close to the user's first message. When the user asks "where are my greens", semantic search finds nothing because no fact about greens was ever stored.

### Extraction Gap — Coverage Bias

The extraction prompt explicitly tells the model to focus on "DURABLE technical facts — architecture, decisions, constraints, patterns, bugs" and to NOT store "transient details". Visual/aesthetic work (color additions, theme changes, what colors were added to a palette) likely gets classified as transient and skipped. If you worked on adding greens to a theme/palette in a prior session, extraction would have seen that as a file edit rather than a durable architectural fact, and skipped it. The episodic narrative system should have caught it as a session summary, but episode generation uses Ollama direct (qwen3-embedding or similar small model) which may truncate or miss fine-grained visual details.

### DB State Audit (2026-03-13)

Project DB: 1273 active facts, 1003 vectors (79% coverage), 38 episodes. Section breakdown: Known Issues 309, Architecture 306, Decisions 285, Constraints 196, Specs 92, Patterns 85. Confidence: min/max/avg all 1.000 — decay is computed dynamically but stored confidence never changes, and all facts have been reinforced 12–119x, so their effective half-lives are extended to ~years. Facts are functionally immortal. Global DB: 78 active facts but 1293 vectors — 1215 orphan vectors from archived/superseded facts that were never cleaned up. Global extraction is OFF (DEFAULT_CONFIG.globalExtractionEnabled = false), so global DB is only populated by manual tool calls and lifecycle ingest, never by automatic extraction. Latest episode date: 2026-03-07 — 6 days of sessions with zero episodes generated.

### Heuristic Verdict: System Does Not Meet Its Claims

The system claims to be a "mid-term memory bridge" between files (long-term) and context (short-term). It fails this on every axis:

CLAIM 1 — Cross-session continuity: FAIL. 1273 facts exist but the agent sees at most 30 via semantic search, capped at 50 in bulk fallback. With uniform confidence=1.0, bulk mode renders the first 50 facts in section alphabetical order — always the same 50, regardless of what was recently worked on. Semantic mode retrieves 30 facts relevant to the user's *first message*, but that message is a continuation ("where are my greens") that assumes context the system hasn't loaded yet.

CLAIM 2 — Episodic memory: FAIL. Episode generation has been silent since 2026-03-07 (6 days). The exit handler calls generateEpisodeDirect (Ollama) with a 15-second timeout. If Ollama isn't running at /exit time, the episode is silently dropped. No cloud fallback for episodes.

CLAIM 3 — Semantic retrieval: PARTIAL. Semantic mode is enabled (79% vector coverage > 50% threshold). But core sections are hardwired to ["Constraints", "Specs"] — the two *least* architecturally useful sections. Architecture (306 facts) and Decisions (285 facts) are only retrieved if semantically close to the user's first message. This is backwards: you always want Architecture in context.

CLAIM 4 — Global knowledge: FAIL. Global extraction is disabled (DEFAULT_CONFIG.globalExtractionEnabled = false). The global DB has 1215 orphan vectors — vectors pointing to archived facts that were never pruned. Global injection cap is 15 facts, rendered even when they're irrelevant (security audit facts from completely different projects are being injected into every Omegon session).

CLAIM 5 — Decay/pruning: FAIL. Decay is implemented in code (exponential half-life) but confidence never reaches the minimumConfidence threshold because high reinforcement counts extend half-lives to effectively years. The extraction agent is instructed to "archive facts to stay under 50" but the agent only sees the current session conversation — it can't audit all 1273 facts. Result: monotonic accumulation with no ceiling.

### Root Cause: Memory as Search vs Memory as Context

The fundamental design flaw: the system treats memory as a *search index* (query-on-demand) rather than as *context reconstruction* (proactive continuity). A mid-term memory bridge should answer: "given that I'm starting a new session on this project, what should I know before the user speaks?" The current system answers: "given what the user just said, what facts match that query?" These are different problems. The first requires recency-awareness, session-continuity loading, and proactive injection. The second requires good embeddings and a relevant query — which doesn't exist when the user says something that assumes prior context. Secondary root causes: (1) The extraction agent targets "durable architectural facts" and filters out task-completion work, but task completion is exactly what enables continuity. (2) The fact corpus has grown 25x past the system's effective injection window with no automatic pruning. (3) Episodes — which would solve the proactive continuity problem — have a flaky dependency on Ollama at shutdown and no graceful fallback.

## Decisions

### Decision: Fix 1: Session-start proactive injection — load recency window before first message

**Status:** exploring
**Rationale:** At session_start, inject: (a) the 3 most recent episodes (narrative of what was worked on), (b) the 20 most recently reinforced facts regardless of section, (c) Architecture + Decisions core sections. This gives the agent "what we were doing" before the user even speaks, solving the continuation-question failure mode. Current behavior of waiting for first message and doing semantic search against it is reactive and fails when the question assumes context.

### Decision: Fix 2: Change core sections from [Constraints, Specs] to [Architecture, Decisions]

**Status:** exploring
**Rationale:** Architecture and Decisions contain the project's identity and accumulated choices — they should always be in context. Constraints and Specs are task-specific and are better retrieved semantically when relevant. The current mapping is inverted: it guarantees injection of what's task-specific and leaves out what's structural. Constraints: 196 facts, many of them specific to one feature. Decisions: 285 facts describing why the system is the way it is. Architecture: 306 facts describing what the system is. These two are the foundation the agent needs for every task.

### Decision: Fix 3: Enforce hard fact cap via DB-level age/confidence pruning, not LLM extraction

**Status:** exploring
**Rationale:** 1273 facts with no ceiling is the system eating itself. The LLM extraction agent can't audit all 1273 facts — it only sees recent conversation. Need a structural cap: a nightly/per-session DB job that runs computeConfidence() on every fact and archives those below minimumConfidence. Current issue: reinforcement counts are so high (up to 119) that effective half-lives are years, so decay never fires. Two options: (A) cap maximum effective half-life (e.g. 90 days regardless of reinforcement), (B) reduce DECAY.halfLifeDays dramatically so facts from 9 days ago without reinforcement actually decay. The current halfLifeDays is unknown — need to read it. Also: require architectural review / extraction pass at the per-section level (e.g. when Architecture exceeds 50 facts, run extraction scoped to only Architecture to prune it) rather than holistic extraction that can't see everything.

### Decision: Fix 4: Episode generation must use cloud fallback, not Ollama-only at shutdown

**Status:** exploring
**Rationale:** Episodes have been silent for 6 days because generateEpisodeDirect requires Ollama at shutdown with a 15-second timeout. Ollama isn't always running. The episode is the highest-value memory artifact — it captures narrative continuity. It should fall back to: gpt-5.3-codex-spark (cheap, fast) → haiku → fail gracefully with a minimal template-generated episode. The same fallback chain logic already exists for compaction; episode generation should reuse it.

### Decision: Fix 5: Add task-completion fact class — auto-store what was accomplished, not just architecture

**Status:** exploring
**Rationale:** The extraction agent filters out "transient details" including completed work. But "I added greens to the Alpharius palette in extensions/dashboard/theme.ts" is exactly what needs to survive sessions. Need a lightweight parallel channel: when write_file/edit is called in a session, queue a summary fact "agent wrote X to Y for purpose Z" in a short-lived recency buffer. These don't need to be permanent architectural facts — they're mid-term (survive 1–3 sessions, then decay). This is the actual mid-term memory that's missing. Current system only has long-term (architecture facts, very slow decay) and short-term (context window). No mid-term.

### Decision: Fix 6: Prune global DB orphan vectors and fix global injection to be project-relevant only

**Status:** exploring
**Rationale:** Global DB has 1215 orphan vectors (vectors for archived/superseded facts never cleaned up). This bloats the DB and degrades semantic search performance in global mode. Also, the 15 global facts injected into every session are currently all security-audit facts from unrelated projects — they're noise in Omegon development sessions. Fix: (1) add ON DELETE CASCADE to facts_vec FK or run a cleanup job, (2) add project-relevance scoring to global injection so infrastructure security facts from project X don't inject into project Y sessions.

## Implementation Notes

All five primary fixes shipped as separate child design nodes (all `implemented`):

| Child node | Fix | Status |
|---|---|---|
| `memory-session-continuity` | Proactive startup injection — recency window + Architecture/Decisions core + last 3 episodes before first message | implemented |
| `memory-episode-reliability` | Cloud-first episode generation fallback chain (haiku → codex-spark → template floor) | implemented |
| `memory-task-completion-facts` | Mid-term Recent Work facts triggered by write/edit tool calls, 2-day half-life decay | implemented |
| `memory-pruning-ceiling` | 90-day half-life cap + per-section LLM archival pass at session_start when section > 60 facts | implemented |

Fix 6 (global DB orphan vector cleanup) was identified as lower priority and deferred — the global store is not on the hot path for Omegon sessions.

Key files: `extensions/project-memory/index.ts` (injection pipeline, session_start hooks), `extensions/project-memory/factstore.ts` (computeConfidence ceiling), `extensions/project-memory/extraction-v2.ts` (episode fallback chain, runSectionPruningPass).

## Acceptance Criteria

### Scenarios

- **Given** a new session starts after prior work, **when** the user asks a continuation question before any tool call, **then** the agent has Architecture + Decisions facts and the last 3 episode narratives already in context from the startup injection payload.
- **Given** semantic injection mode is active, **when** it selects core sections, **then** `["Architecture", "Decisions"]` are always included (not `["Constraints", "Specs"]` — the old inverted mapping).
- **Given** a session ends via `/exit`, **when** episode generation runs, **then** at least a minimum viable episode is stored (template floor) even if all model calls fail.
- **Given** the agent writes or edits a file during a session, **when** the session ends, **then** a `Recent Work` fact capturing what was written is stored with a 2-day half-life.
- **Given** a memory section has grown beyond 60 facts, **when** a new session starts, **then** a targeted LLM archival pass runs for that section and archives the least-useful facts to bring it under the ceiling.
- **Given** a fact has been reinforced 100+ times, **when** 120 days elapse without reinforcement, **then** its confidence has decayed significantly (< 0.25) rather than staying near 1.0.

### Falsifiability

- If the agent fails to answer a continuation question that was answered in a prior session episode, the proactive startup injection is not working.
- If `core sections` in semantic mode is anything other than `["Architecture", "Decisions"]`, the Fix 2 change was reverted or bypassed.
- If the total fact count across all sections grows monotonically past ~360 (6 sections × 60 ceiling) across multiple sessions, the pruning ceiling is not firing.
- If no episode is recorded after a session that used `/exit`, the episode fallback chain has a gap (the template floor should always produce an episode).
- If `Recent Work` facts appear in the Architecture or Decisions sections, the section isolation is broken.

### Constraints

- [x] Startup injection payload is built at `session_start` before the user's first message, not reactively on first turn.
- [x] Core sections for semantic injection are `["Architecture", "Decisions"]` — Constraints and Specs retrieved semantically only.
- [x] Episode generation uses a fallback chain with a deterministic template floor as the guaranteed minimum output.
- [x] Task-completion facts live in the `Recent Work` section with `RECENT_WORK_DECAY` (halfLifeDays=2, reinforcementFactor=1.0).
- [x] `computeConfidence()` caps effective half-life at 90 days for all decay profiles.
- [x] Per-section pruning ceiling is 60 facts; `Recent Work` is explicitly excluded.
- [x] All child design nodes are `implemented` status before this umbrella node closes.

## Open Questions

*No open questions.*

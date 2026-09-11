+++
id = "de89557f-6ba2-4597-9140-7a13098c810a"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/memory-lifecycle-integration.md", "design/memory-mind-audit.md", "design/cheap-gpt-memory-models.md", "memory-system-overhaul.md", "memory-session-continuity.md", "memory-episode-reliability.md", "memory-task-completion-facts.md", "memory-pruning-ceiling.md"]
last_updated = "2026-09-08"
last_reviewed = "2026-08-26"
openspec_baselines = ["memory.md", "memory/lifecycle.md", "memory/models.md", "project-memory/compaction.md"]
subsystem = "project-memory"
+++

# Project Memory

> Persistent fact storage, semantic retrieval, episodic session narratives, context injection, and cross-session knowledge accumulation.

## What It Does

Project memory gives agents persistent knowledge across sessions. It operates at multiple levels:

- **Fact store**: Schema-v13 SQLite+WAL database (`ai/memory/facts.db`, or the existing `.omegon/memory/facts.db` legacy root) with atomic facts organized by section (Architecture, Decisions, Constraints, Known Issues, Patterns & Conventions, Specs, Recent Work). Facts are stored, superseded, archived, and connected in a knowledge graph. Inferred lifecycle summaries remain pending until operator confirmation and retain their declared source references afterward. [Applicability rules](memory-applicability.md) constrain current selection without changing lifecycle status.
- **Semantic retrieval**: `memory_recall(query)` uses compatible identified vectors plus keyword retrieval. Model/revision/preprocessing identity and source fingerprints must agree before comparison. Legacy, incompatible, or stale vectors produce explicit degradation. See [retrieval and index repair](memory-retrieval.md).
- **Working memory**: 25-slot buffer of pinned facts that survive context compaction and get priority injection.
- **Episodic memory**: Bounded, source-linked episodes captured from validated semantic replay. Evidence is stored before independently configured extraction; generated candidates remain pending inferences. See [memory formation](memory-formation.md) for configuration, limits, and recovery behavior.
- **Context injection**: Three-layer proactive startup injection (last 3 episodes + recency window + Architecture/Decisions core) fires before the user's first message. Semantic injection on first message adds task-specific facts on top.
- **Task-completion facts**: Write/edit tool calls queue `Recent Work` facts with 2-day half-life, capturing mid-term "what was accomplished" continuity.
- **Structural pruning ceiling**: `computeConfidence()` caps effective half-life at 90 days regardless of reinforcement count. Per-section LLM archival pass fires at session_start when any section exceeds 60 facts.
- **Mind-scoped durability**: Durable facts, vectors, edges, and episodes are isolated by mind label. The host selects the active mind scope; the managed version-1 service does not expose a standalone durable mind-record or parent-mutation API.
- **JSONL sync**: `facts.jsonl` exports active and historical facts for git tracking. Modern records include persisted confidence, reinforcement state, and source-session metadata in an optional `operational` object. Computed retrieval/decay scores remain transient. Legacy updates that omit `operational` preserve the destination's existing state. Lamport versions control replacement; equal or older versions do not overwrite a record. The `merge=union` gitattribute enables multi-branch fact merging.
- **Global knowledge base**: Cross-project facts stored in `~/.config/omegon/global-memory.db`.

## Key Files

| File | Role |
|------|------|
| `core/crates/omegon/src/features/memory.rs` | Agent-facing memory tools and hybrid recall orchestration |
| `core/crates/omegon/src/memory_service.rs` | Managed serial owner for project/global stores, JSONL, and configured Codex-vault effects |
| `core/crates/omegon-memory/` | SQLite storage, JSONL sync, embeddings, search, episodes, and graph types |
| `core/crates/omegon/src/embedding.rs` | Ollama embedding service used for hybrid search when reachable |
| `core/crates/omegon/src/local_embedding.rs` | Optional ONNX embedding service compiled behind the `local-embeddings` feature |
| `core/crates/omegon/src/setup.rs` | Starts and captures the managed memory binding, selects embedding services, and registers tools |

## Design Decisions

- **SQLite+WAL for storage, JSONL for git sync**: Database handles concurrent reads during extraction; JSONL enables cross-branch merging via git union strategy.
- **Payload-bound operation replay**: Durable mutations can carry a stable operation identity. Exact replay returns the original compact effect; reuse with a different payload fails before mutation. Targeted changes use fact-version preconditions instead of one global store revision.
- **Managed JSONL and Codex-vault synchronization**: One boot worker owns the selected project `facts.jsonl` and explicitly configured vault effects. Startup preserves empty-store, non-child JSONL bootstrap behavior. Bounded imports keep Lamport conflict rules and operation replay; deterministic exports and vault projections compare bytes before synced atomic replacement. Vault inputs are snapshotted before mutation, reject static traversal/symlink escape, and use stable note/fact lineage so unchanged, edited, moved, aliased, and repeated synchronization converges.
- **Managed production consumers**: Tools, context, lifecycle ingestion, session-end persistence, status, provider-result writes, and embedding backfill use a boot-captured exact-generation binding or a bounded managed composition. The host retains session-local pins, context rendering, provider selection, extraction, and embedding computation. Tracked provider tasks settle before the memory worker shuts down.
- **Typed optional availability**: Memory tools and status remain declared when the managed service is unavailable and return typed unavailable evidence. Durable memory context is omitted while unrelated context, sessions, frontends, and host-owned compaction continue.
- **Governed schema v13 migration**: Startup migrates schemas v5-v12 before opening the project store. Historical v5/v6 `default` records move to `legacy`; post-v7 `default` records move to `primensus`. Schema v13 adds nullable recorded applicability. Scoped JSONL records use `applicable_fact`, requiring a scope-aware reader. Legacy records retain unknown scope and do not gain fabricated confirmation; legacy vectors remain unverified until repair.
- **Semantic search primary, FTS5 fallback**: Embeddings give better retrieval; FTS5 always works as a fallback. The current selection order is configured Ollama embedding service, optional local ONNX service, then FTS5-only recall.
- **Pointer facts over inline details**: Facts reference files (`"X does Y. See path/to/file.ts"`) instead of inlining implementation details — keeps facts atomic and maintainable.
- **Store conclusions, not investigation steps**: Facts capture final state, not debugging journey.
- **Proactive startup injection over reactive search**: Session_start injects Architecture + Decisions core sections + recency window + last 3 episodes before the user speaks. Reactive semantic search on first message augments this; it does not replace it.
- **Core sections = Architecture + Decisions**: These are the structural anchors always in context. Constraints and Specs are retrieved semantically only when task-relevant.
- **90-day half-life ceiling**: `MAX_HALF_LIFE_DAYS = 90` in `factstore.ts` — reinforcement extends half-life up to 90 days max, then decay has teeth. Facts needing indefinite survival must be pinned via `memory_focus`.
- **60-fact per-section ceiling**: `runSectionPruningPass()` fires at session_start for any section > 60 facts. Sends section facts to extraction model with instructions to identify archival candidates. `Recent Work` excluded (handled by 2-day decay).
- **Recent Work section for task-completion**: Write/edit tool calls queue lightweight facts in `Recent Work` with `RECENT_WORK_DECAY` (halfLifeDays=2, reinforcementFactor=1.0 — reinforcement does NOT extend these). Mid-term bridge between architecture facts and ephemeral context.
- **Evidence before inference**: Extraction uses the configured host model. Provider failure or cancellation preserves captured source evidence and a typed unavailable or pending state. Uncommitted advisory strings do not replace unavailable semantic sources.
- **Cheap/local models for extraction and embeddings**: Background extraction and embedding paths avoid burning expensive frontier calls where possible. For embeddings, the Rust runtime supports Ollama and an optional local ONNX sentence-transformer path.
- **Mind-label scoping**: The selected mind label scopes durable reads and writes without duplicating records. Directive lifecycle policy and any transfer of discoveries between scopes remain host-owned rather than a standalone managed mind hierarchy API.
- **Context pressure auto-compaction**: When context window usage exceeds thresholds, memory triggers compaction. Local (45s) → codex-spark (60s) → haiku (30s) fallback chain.

## Behavioral Contracts

See `openspec/baseline/memory.md`, `openspec/baseline/memory/lifecycle.md`, `openspec/baseline/memory/models.md`, and `openspec/baseline/project-memory/compaction.md` for Given/When/Then scenarios.

## Local ONNX Embeddings

The local embedding service is opt-in at build time:

```sh
cargo build --release --features local-embeddings
```

At runtime, Omegon looks for `model.onnx` and `tokenizer.json` under:

```text
~/.config/omegon/models/all-MiniLM-L6-v2/
```

Override the model name with `OMEGON_EMBED_LOCAL_MODEL` or the exact directory with `OMEGON_EMBED_MODEL_DIR`. The expected default model shape is `all-MiniLM-L6-v2` with 384-dimensional vectors. There is no stable public `omegon embedding download` command yet; place the model files directly or use the Ollama embedding path.

## Constraints & Known Limitations

- Embeddings require a reachable Ollama embedding service or a `local-embeddings` build with local ONNX model files — degrades to FTS5 keyword search without one
- Working memory capped at 25 facts to control context injection size
- Source capture starts at session end. Interruption before capture commits can still omit the episode; interruption during extraction leaves durable evidence with pending status. Automatic restart scheduling remains planned.
- JSONL merge=union can create duplicates if the same fact is modified on two branches
- Global DB injection injects up to 15 facts from `~/.config/omegon/global-memory.db`; global extraction is off by default so the global DB only receives manually stored facts and lifecycle-ingest candidates
- Vault synchronization rejects static traversal and symlink escape, but it is not a sandbox against hostile concurrent replacement of an already validated filesystem path

## Related Subsystems

- [Model Routing](model-routing.md) — controls extraction/compaction model selection
- [Design Tree](design-tree.md) — lifecycle events stored as facts on status transitions
- [OpenSpec](openspec.md) — lifecycle events on archive
- [Dashboard](dashboard.md) — memory statistics displayed in raised mode

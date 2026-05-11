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
last_updated = "2026-03-17"
last_reviewed = "2026-05-11"
openspec_baselines = ["memory.md", "memory/lifecycle.md", "memory/models.md", "project-memory/compaction.md"]
subsystem = "project-memory"
+++

# Project Memory

> Persistent fact storage, semantic retrieval, episodic session narratives, context injection, and cross-session knowledge accumulation.

## What It Does

Project memory gives agents persistent knowledge across sessions. It operates at multiple levels:

- **Fact store**: SQLite+WAL database (`ai/memory/facts.db`) with atomic facts organized by section (Architecture, Decisions, Constraints, Known Issues, Patterns & Conventions, Specs, Recent Work). Facts are stored, superseded, archived, and connected in a knowledge graph.
- **Semantic retrieval**: Facts embedded for `memory_recall(query)` similarity search. Omegon first probes the configured Ollama embedding endpoint, then falls back to a local ONNX embedding service when the binary is built with `local-embeddings` and model files are present. If no embedding backend is available, recall falls back to FTS5 keyword search.
- **Working memory**: 25-slot buffer of pinned facts that survive context compaction and get priority injection.
- **Episodic memory**: Session narratives generated at shutdown via fallback chain (cloud → local → template floor), capturing goals, decisions, sequences, and outcomes.
- **Context injection**: Three-layer proactive startup injection (last 3 episodes + recency window + Architecture/Decisions core) fires before the user's first message. Semantic injection on first message adds task-specific facts on top.
- **Task-completion facts**: Write/edit tool calls queue `Recent Work` facts with 2-day half-life, capturing mid-term "what was accomplished" continuity.
- **Structural pruning ceiling**: `computeConfidence()` caps effective half-life at 90 days regardless of reinforcement count. Per-section LLM archival pass fires at session_start when any section exceeds 60 facts.
- **Directive minds**: `implement` forks a scoped mind from `default`; all fact reads/writes auto-scope to the directive. `archive` ingests discoveries back to `default` and cleans up. Zero-copy fork with parent-chain inheritance — no fact duplication, parent embeddings and edges are reused.
- **JSONL sync**: `facts.jsonl` exported for git tracking; `merge=union` gitattribute enables multi-branch fact merging. Volatile runtime scoring metadata (confidence, reinforcement counts, decay scores) omitted from exports for stable diffs.
- **Global knowledge base**: Cross-project facts stored in `~/.config/omegon/global-memory.db`.

## Key Files

| File | Role |
|------|------|
| `core/crates/omegon/src/features/memory.rs` | Agent-facing memory tools and hybrid recall orchestration |
| `core/crates/omegon-memory/` | SQLite storage, JSONL sync, embeddings, search, episodes, and graph types |
| `core/crates/omegon/src/embedding.rs` | Ollama embedding service used for hybrid search when reachable |
| `core/crates/omegon/src/local_embedding.rs` | Optional ONNX embedding service compiled behind the `local-embeddings` feature |
| `core/crates/omegon/src/setup.rs` | Wires memory storage, embedding service selection, and tool registration |

## Design Decisions

- **SQLite+WAL for storage, JSONL for git sync**: Database handles concurrent reads during extraction; JSONL enables cross-branch merging via git union strategy.
- **Semantic search primary, FTS5 fallback**: Embeddings give better retrieval; FTS5 always works as a fallback. The current selection order is configured Ollama embedding service, optional local ONNX service, then FTS5-only recall.
- **Pointer facts over inline details**: Facts reference files (`"X does Y. See path/to/file.ts"`) instead of inlining implementation details — keeps facts atomic and maintainable.
- **Store conclusions, not investigation steps**: Facts capture final state, not debugging journey.
- **Proactive startup injection over reactive search**: Session_start injects Architecture + Decisions core sections + recency window + last 3 episodes before the user speaks. Reactive semantic search on first message augments this; it does not replace it.
- **Core sections = Architecture + Decisions**: These are the structural anchors always in context. Constraints and Specs are retrieved semantically only when task-relevant.
- **90-day half-life ceiling**: `MAX_HALF_LIFE_DAYS = 90` in `factstore.ts` — reinforcement extends half-life up to 90 days max, then decay has teeth. Facts needing indefinite survival must be pinned via `memory_focus`.
- **60-fact per-section ceiling**: `runSectionPruningPass()` fires at session_start for any section > 60 facts. Sends section facts to extraction model with instructions to identify archival candidates. `Recent Work` excluded (handled by 2-day decay).
- **Recent Work section for task-completion**: Write/edit tool calls queue lightweight facts in `Recent Work` with `RECENT_WORK_DECAY` (halfLifeDays=2, reinforcementFactor=1.0 — reinforcement does NOT extend these). Mid-term bridge between architecture facts and ephemeral context.
- **Episode fallback chain**: Generation tries cloud (haiku → codex-spark) first; Ollama optional. Guaranteed template-floor episode when all models fail — at least date + tool counts + files written.
- **Cheap/local models for extraction and embeddings**: Background extraction and embedding paths avoid burning expensive frontier calls where possible. For embeddings, the Rust runtime supports Ollama and an optional local ONNX sentence-transformer path.
- **Mind-per-directive scoping**: Each directive gets its own mind (memory namespace) forked from `default`. Parent-chain inheritance means child minds see parent facts without duplication. On archive, valuable discoveries are ingested back to `default`. This isolates directive-specific context while preserving project-wide knowledge.
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
- Episode generation runs at session shutdown — abrupt kill (SIGKILL) skips episode; `/exit` uses the full fallback chain
- JSONL merge=union can create duplicates if the same fact is modified on two branches
- Global DB injection injects up to 15 facts from `~/.config/omegon/global-memory.db`; global extraction is off by default so the global DB only receives manually stored facts and lifecycle-ingest candidates

## Related Subsystems

- [Model Routing](model-routing.md) — controls extraction/compaction model selection
- [Design Tree](design-tree.md) — lifecycle events stored as facts on status transitions
- [OpenSpec](openspec.md) — lifecycle events on archive
- [Dashboard](dashboard.md) — memory statistics displayed in raised mode

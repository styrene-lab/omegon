+++
id = "365ecb4b-1868-4381-95d7-fa93e57f9373"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory extraction pipeline — TS reference implementation for Rust port

## Overview

The extraction pipeline converts conversation context into durable memory facts and edges.
It runs in two phases: project extraction (session-scoped) and global extraction (cross-project).
This document captures the full TS implementation as a reference for the Rust port, including
three critical fixes applied in omegon-pi commit `944d2ab`.

## Architecture

```
Conversation → Phase 1 (project extraction) → processExtraction() + processEdges()
                                              ↓
                                        new facts created?
                                              ↓ yes
                                     Phase 2 (global extraction) → global processExtraction() + processEdges()
```

### Phase 1: Project Extraction

**Trigger**: Token delta exceeds threshold OR tool call count since last extraction exceeds limit.
Managed by `triggers.ts` — not every turn triggers extraction.

**Input**: Current active facts (with IDs) + last 30 conversation messages serialized.

**LLM prompt**: Instructs the model to output JSONL with action types:

```jsonl
{"type":"observe","section":"Architecture","content":"The project uses SQLite for storage"}
{"type":"reinforce","id":"abc123"}
{"type":"supersede","id":"abc123","section":"Architecture","content":"Migrated to PostgreSQL"}
{"type":"archive","id":"abc123"}
{"type":"connect","source":"<fact_id>","target":"<fact_id>","relation":"depends_on","description":"module A imports from module B"}
```

**Action routing** (post-fix):
```typescript
const actions = parseExtractionOutput(rawOutput);
const factActions = actions.filter(a => a.type !== "connect");
const edgeActions = actions.filter(a => a.type === "connect");

store.processExtraction(mind, factActions);     // observe/reinforce/supersede/archive
if (edgeActions.length > 0) {
  store.processEdges(edgeActions);              // connect → edges table
}
```

⚠️ **Fix #1**: Before this fix, connect actions were passed to `processExtraction()` which silently
dropped them (its switch statement only handles observe/reinforce/supersede/archive). The fix splits
connect actions and routes them to `processEdges()`.

### Phase 2: Global Extraction

**Trigger**: Phase 1 produced new facts AND `globalExtractionEnabled` is true AND global store exists.

⚠️ **Fix #2**: `globalExtractionEnabled` was defaulted to `false` on March 4th to suppress 429 rate
limit errors. The rate limit handling (silent skip on 429) was the actual fix — the default should
be `true`. Changed back to `true`.

**Input**: New project facts + existing global facts (with IDs) + existing global edges.

**LLM prompt**: Different from Phase 1 — focused on cross-project generalization. Supports
observe (rewritten to be project-agnostic), reinforce, connect (between global fact IDs),
supersede, archive.

**Edge creation**: Global extraction can create edges between global facts. The extracted edges
reference global fact IDs from the "EXISTING GLOBAL FACTS" section. An observe action must
promote a project fact to global FIRST — then a subsequent extraction cycle can connect it.

## Data Types

### ExtractionAction (TypeScript reference)

```typescript
interface ExtractionAction {
  type: "observe" | "reinforce" | "supersede" | "archive" | "connect";
  // fact actions
  id?: string;              // reinforce, supersede, archive
  section?: string;         // observe, supersede
  content?: string;         // observe, supersede
  // connect-specific
  source?: string;          // source fact ID
  target?: string;          // target fact ID
  relation?: string;        // short verb phrase
  description?: string;     // human-readable explanation
}
```

### Rust equivalent

```rust
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ExtractionAction {
    Observe { section: Section, content: String },
    Reinforce { id: String },
    Supersede { id: String, section: Section, content: String },
    Archive { id: String },
    Connect {
        source: String,
        target: String,
        relation: String,
        #[serde(default)]
        description: Option<String>,
    },
}
```

### ParseExtractionOutput

```rust
fn parse_extraction_output(raw: &str) -> Vec<ExtractionAction> {
    raw.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .filter_map(|l| {
            // Try parse, also accept {action: "observe"} as alias for {type: "observe"}
            serde_json::from_str::<ExtractionAction>(l.trim()).ok()
        })
        .collect()
}
```

## processExtraction (fact actions)

Handles observe, reinforce, supersede, archive within a single SQLite transaction:

| Action | Behavior |
|--------|----------|
| **observe** | Hash content → if exists in mind chain, reinforce; else insert new fact |
| **reinforce** | Increment `reinforcement_count`, update `last_reinforced` timestamp |
| **supersede** | Archive old fact (set `superseded_by`), insert replacement |
| **archive** | Set status = 'archived' |

Returns `{ reinforced: usize, added: usize, new_fact_ids: Vec<String> }`.

## processEdges (connect actions)

Handles connect actions within a single SQLite transaction:

```
for each action where type == "connect":
    validate source, target, relation all present
    verify source fact exists (getFact)
    verify target fact exists (getFact)
    call storeEdge(source, target, relation, description)
        → deduplicates by (source, target, relation) triple
        → reinforces on duplicate (increments reinforcement_count)
```

Returns `{ added: usize, reinforced: usize }`.

## Edge Schema

The `edges` table in SQLite:

```sql
CREATE TABLE edges (
    id                  TEXT PRIMARY KEY,
    source_fact_id      TEXT NOT NULL,
    target_fact_id      TEXT NOT NULL,
    relation            TEXT NOT NULL,
    description         TEXT,
    confidence          REAL NOT NULL DEFAULT 1.0,
    decay_rate          REAL NOT NULL DEFAULT 0.023,   -- ~30 day half-life
    reinforcement_count INTEGER NOT NULL DEFAULT 1,
    last_reinforced     TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active', -- active | archived
    source_mind         TEXT,
    target_mind         TEXT,
    session             TEXT
);

CREATE INDEX idx_edges_source   ON edges(source_fact_id);
CREATE INDEX idx_edges_target   ON edges(target_fact_id);
CREATE INDEX idx_edges_relation ON edges(relation);
CREATE INDEX idx_edges_status   ON edges(status);
```

### Edge confidence decay

Edges decay like facts: `confidence = e^(-decay_rate * days_since_reinforced)`.
Reinforcement resets the timer. The default decay rate (0.023) gives ~30 day half-life.
This means unreinforced edges fade naturally — structural relationships that are
no longer observed in conversation lose confidence over time.

## Extraction Prompt Guidelines

### Phase 1 prompt (project extraction)

Key directives for the LLM:
- Output ONLY valid JSONL (one JSON object per line, no commentary)
- Focus on DURABLE technical facts — architecture, decisions, constraints, patterns, bugs
- DO NOT output transient details (debugging steps, file contents, command output)
- Prefer pointers over content (name concept + reference file path, ~40 word limit)
- Valid sections: Architecture, Decisions, Constraints, Known Issues, Patterns & Conventions, Specs
- **connect**: reference existing fact IDs from the active facts list; common relations:
  `depends_on`, `imports`, `enables`, `motivated_by`, `contradicts`, `changes_with`,
  `requires`, `conflicts_with`, `instance_of`

⚠️ **Fix #3**: The Phase 1 prompt originally had no connect action type documented.
Without it in the prompt, the LLM never produces connect actions, so project-level
edges are never created. The connect action type was added with the relations list above.

### Phase 2 prompt (global extraction)

Additional directives beyond Phase 1:
- Rewrite promoted facts to be project-agnostic (remove project-specific paths/names)
- connect actions must reference GLOBAL fact IDs only (not project-scoped IDs)
- To connect a new project fact, first promote it with observe, then connect in the next cycle
- Prefer cross-section connections (Architecture ↔ Decisions) over same-section

## Trigger Logic

```typescript
function shouldExtract(state, totalTokens, config): boolean {
    if (state.isRunning) return false;
    if (state.manualStoresSinceExtract >= config.manualStoreThreshold && state.isInitialized)
        return false;  // suppress during manual store bursts

    if (!state.isInitialized) {
        return totalTokens >= config.minimumTokensToInit;  // first extraction
    }

    const tokenDelta = totalTokens - state.lastExtractedTokens;
    if (tokenDelta < config.minimumTokensBetweenUpdate) return false;

    return state.toolCallsSinceExtract >= config.toolCallsBetweenUpdates;
}
```

Default thresholds:
- `minimumTokensToInit`: 4000 (first extraction after 4K tokens)
- `minimumTokensBetweenUpdate`: 8000 (at least 8K new tokens between extractions)
- `toolCallsBetweenUpdates`: 5 (at least 5 tool calls between extractions)
- `manualStoreThreshold`: 3 (suppress after 3 manual `memory_store` calls)

## Rust Port Checklist

### Must implement

- [ ] `ExtractionAction` enum with serde tag-based deserialization
- [ ] `parse_extraction_output()` — tolerant JSONL parser (skip malformed lines)
- [ ] Phase 1 prompt builder — include connect action type from day one
- [ ] `process_extraction()` — observe/reinforce/supersede/archive in single transaction
- [ ] `process_edges()` — connect actions routed separately, FK validation, dedup
- [ ] Extraction trigger state machine (token delta + tool call counting)
- [ ] Phase 2 global extraction (same LLM call pattern, different prompt + store)
- [ ] `globalExtractionEnabled` config — default `true`
- [ ] Rate limit handling on Phase 2 — silent skip on 429, no noisy warnings

### Must NOT repeat

- [ ] Do not route connect actions through `process_extraction()` — they must go to `process_edges()`
- [ ] Do not default `global_extraction_enabled` to false
- [ ] Do not omit the connect action type from the Phase 1 prompt

### Edge creation from code analysis (/init)

Future: `/init` should populate structural edges by scanning the codebase:
- Static imports → `imports` / `depends_on` edges between module facts
- Git co-change analysis → `changes_with` edges with confidence from frequency
- Submodule boundaries → `inside_submodule` edges

These edges feed the DSM-based work decomposition model (see `work-decomposition-model.md`).

## Test Coverage

### TS tests (reference for Rust port)

From `edges.test.ts`:
- Edge CRUD: store, dedup, different relations, both-direction retrieval, archive
- Edge confidence decay: fresh edge ~1.0
- `processEdges`: creates from extraction output, reinforces duplicates, skips bad IDs, ignores non-connect
- Phase 1 routing: connect actions create edges when routed to processEdges (fix validation)
- Phase 1 routing: processExtraction silently ignores connect actions (regression guard)
- Injection rendering: Connections section appears when edges exist, omitted when empty
- Batch edge retrieval: `getEdgesForFacts` with limit and empty results

### Minimum Rust tests

```rust
#[test] fn parse_extraction_output_handles_connect_actions() { ... }
#[test] fn process_extraction_ignores_connect_actions() { ... }
#[test] fn process_edges_validates_fact_existence() { ... }
#[test] fn process_edges_deduplicates_by_triple() { ... }
#[test] fn extraction_cycle_routes_connect_to_process_edges() { ... }
```

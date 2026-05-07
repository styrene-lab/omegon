+++
id = "dad32203-2408-4c83-967f-e1cbee44b623"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory schema authority — Rust owns the schema, TS adapts non-destructively

## Overview

The Rust omegon-memory crate (types.rs + sqlite.rs) is the authoritative schema for the memory/factstore. The TS factstore.ts must adapt to it, not the other way around. This node documents the schema requirements from the Rust side — specifically the persona mind layer, tags field, and any new fields needed — so the parallel TS consolidation work can absorb them non-destructively via additive migrations.

## Research

### Current schema alignment between Rust and TS

**Aligned (both sides have):**
- facts table: id, mind, content, section, status, confidence, reinforcement_count, decay_rate, decay_profile, last_reinforced, created_at, version, supersedes/superseded_by, source, content_hash, last_accessed, created_session, superseded_at, archived_at, jj_change_id
- episodes table: id, mind, date, title, narrative, created_at, affected_nodes, affected_changes, files_changed, tags, tool_calls_count
- edges table: id, source_id, target_id, relation, description, weight/confidence, created_at
- minds table: name, description, status, origin_type, created_at
- facts_vec table: fact_id, embedding, model_name, dims, created_at
- FTS5 index on facts(content)
- Schema versioning (TS: SCHEMA_VERSION=5, Rust: implicit via init_schema)

**TS has, Rust missing:**
- episodes.jj_change_id column (TS migration 5 adds it, Rust types.rs Episode struct doesn't have it)
- Explicit schema_version table with migration tracking (Rust uses idempotent CREATE IF NOT EXISTS)

**Rust has, TS missing:**
- Nothing critical — Rust was designed to mirror TS

**NEITHER side has (needed for persona system):**
- facts.persona_id — which persona was active when this fact was stored
- facts.layer — which memory layer this fact belongs to ('project', 'persona', 'working')
- facts.tags — searchable tags (persona mind stores use tags for domain classification)
- minds table persona fields — minds can represent persona mind stores, not just projects

### Schema requirements from the persona system (decided design)

The persona system (design node `persona-system`, decided) requires these schema additions:

**1. `facts.persona_id TEXT` (nullable, additive)**
When a persona is active and a fact is stored into the persona mind layer, this field records which persona owns it. NULL = project fact (default, backward-compatible). This is the key discriminator for the layered merge — on persona deactivation, facts with `persona_id = X` are removed from the active query set.

**2. `facts.layer TEXT NOT NULL DEFAULT 'project'` (additive)**
Memory layer classification: 'project' (default), 'persona', 'working'. Controls injection priority ordering and lifecycle (persona facts are portable across projects, working facts are session-scoped). The PluginRegistry already models this in Rust (MemoryLayers struct) — the DB column persists it.

**3. `facts.tags TEXT` (nullable, JSON array, additive)**
Persona mind seed facts carry tags for domain classification (e.g. ["pcb", "trace-width", "thermal"]). Tags enable filtered queries within a persona mind. Stored as JSON array in SQLite.

**4. `minds.origin_type` extended values**
Currently: 'active', 'archived'. Needs: 'persona' — indicates this mind record represents a persona's dedicated mind store, not a project. The field already exists as TEXT — the new value is purely semantic, no schema change needed.

**5. `episodes.jj_change_id TEXT` (nullable, additive)**
The TS side already adds this in migration 5. The Rust Episode struct in types.rs needs the field added to match.

**6. Schema migration table**
The Rust side should adopt explicit schema versioning (like TS's schema_version table) instead of relying solely on CREATE IF NOT EXISTS. This enables incremental migrations without full table recreation.

All additions are nullable with defaults — existing databases read cleanly without migration. New fields appear as NULL until populated. This is the "non-destructive adaptation" contract: the TS factstore can add these columns via its existing migration system, and old data keeps working.

## Decisions

### Decision: Schema v6: persona_id, layer, tags columns — Rust is the authority

**Status:** decided
**Rationale:** The Rust omegon-memory crate defines the canonical schema. Schema v6 adds: facts.persona_id (TEXT, nullable — which persona owns this fact), facts.layer (TEXT, default 'project' — memory layer classification), facts.tags (TEXT, JSON array — domain classification tags). Migration from v5 is additive (ALTER TABLE ADD COLUMN with defaults). Existing data reads cleanly. The TS factstore should add these same columns in its next migration, reading from the schema-contract.json file that Rust generates. Indexes: idx_facts_persona (WHERE persona_id IS NOT NULL), idx_facts_layer (WHERE status = 'active').

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon-memory/src/types.rs` (modified) — Fact gains persona_id (Option<String>), layer (String, default 'project'), tags (Vec<String>). Episode gains jj_change_id.
- `core/crates/omegon-memory/src/sqlite.rs` (modified) — Schema v6 migration: ALTER TABLE adds persona_id/layer/tags columns. schema_version table with explicit version tracking. Partial indexes on persona_id and layer.
- `core/crates/omegon-memory/schema-contract.json` (new) — Canonical schema contract file — generated by Rust, consumed by TS factstore for alignment verification
- `extensions/project-memory/factstore.ts` (modified) — TS factstore v6 migration: addCol for persona_id, layer, tags. idx_facts_persona partial index.

### Constraints

- All v6 columns are nullable with defaults — existing v5 databases read cleanly without migration
- schema-contract.json is the single source of truth for cross-language schema alignment
- Rust is the authority — TS adapts non-destructively via additive ALTER TABLE

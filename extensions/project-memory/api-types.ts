/**
 * project-memory/api-types — Omega /api/memory/* HTTP contract
 *
 * This file is the source of truth for the wire protocol between:
 *   - The TypeScript auspex bridge (TS extension, pi API surface)
 *   - The Rust Omega daemon (/api/memory/* Axum routes)
 *
 * Field names are snake_case throughout to match Rust serde conventions
 * (#[serde(rename_all = "snake_case")] or #[serde(deny_unknown_fields)]).
 *
 * When the Rust implementation is built, each interface here becomes a
 * corresponding Rust struct deriving Serialize, Deserialize, with field
 * names identical to these. Any deviation is a bug in the Rust port.
 *
 * Versioning: the HTTP API is versioned via the Accept header or a /v1/ prefix.
 * Breaking changes increment the major version. Additive fields use Option<T>
 * with serde defaults in Rust and optional typing here.
 */

import type { SectionName } from "./template.ts";
import type { DecayProfileName } from "./core.ts";

// ─── Core record types ────────────────────────────────────────────────────────

/**
 * A memory fact as returned by the API.
 * Mirrors the `facts` SQLite table — all fields except `embedding` (DB-only).
 *
 * Rust: src/memory/types.rs::Fact
 */
export interface FactRecord {
  id: string;
  mind: string;
  content: string;
  section: SectionName;
  status: FactStatus;
  confidence: number;
  reinforcement_count: number;
  decay_rate: number;
  /** Discriminant for the decay profile used at write time. Persisted in DB.
   *  Allows correct confidence computation at read time. Default: "standard". */
  decay_profile: DecayProfileName;
  last_reinforced: string;   // ISO 8601
  created_at: string;        // ISO 8601
  /** Lamport logical timestamp. Incremented on every mutation.
   *  On git-sync conflict: higher version always wins. Default on import: 0. */
  version: number;
  superseded_by?: string;    // fact ID; present only when status === "superseded"
  source?: string;           // origin annotation: "lifecycle:openspec:X" | "extraction" | "manual"
}

export type FactStatus = "active" | "archived" | "superseded";

/**
 * A session episode narrative.
 * Rust: src/memory/types.rs::Episode
 */
export interface EpisodeRecord {
  id: string;
  mind: string;
  date: string;        // ISO 8601 date
  title: string;
  narrative: string;   // free-form prose summary
  created_at: string;  // ISO 8601
  /** Design node IDs touched this session. Optional; populated by extraction subagent. */
  affected_nodes?: string[];
  /** OpenSpec change names touched this session. */
  affected_changes?: string[];
  /** git diff --stat summary of files changed. */
  files_changed?: string[];
  /** Semantic tags: "architecture", "bugfix", "refactor", "investigation", etc. */
  tags?: string[];
  /** Rough activity measure — number of LLM tool calls in this session. */
  tool_calls_count?: number;
}

/**
 * A directional relationship between two facts.
 * Rust: src/memory/types.rs::Edge
 */
export interface EdgeRecord {
  id: string;
  source_id: string;
  target_id: string;
  relation: string;      // "depends_on", "contradicts", "generalizes", etc.
  description?: string;
  weight: number;        // 0.0–1.0; higher = stronger relationship
  created_at: string;    // ISO 8601
}

// ─── POST /api/memory/facts ───────────────────────────────────────────────────

export interface StoreFactRequest {
  mind: string;
  content: string;
  section: SectionName;
  /** Decay profile to use for this fact. Defaults to "standard". */
  decay_profile?: DecayProfileName;
  /** Optional initial confidence override (0.0–1.0). Default: 1.0. */
  confidence?: number;
  /** Origin annotation for provenance tracking. */
  source?: string;
}

export interface StoreFactResponse {
  /** The ID of the stored or matched fact. */
  id: string;
  /** What happened: new fact stored, existing fact reinforced, or exact duplicate found. */
  action: "stored" | "reinforced" | "deduplicated";
  fact: FactRecord;
}

// ─── GET /api/memory/context ──────────────────────────────────────────────────

export interface ContextRequest {
  mind: string;
  /** Raw text query used for semantic fact selection. Omega embeds it.
   *  Required when mode === "semantic". */
  query?: string;
  mode: InjectionMode;
  /** Pinned fact IDs — always included first, before semantic or bulk selection. */
  working_memory?: string[];
  /** Maximum character budget for the rendered context block. Default: 12000.
   *  Omega stops adding facts when the budget would be exceeded. */
  max_chars?: number;
  /** Per-section maximum fact counts. Overrides Omega's default caps. */
  section_caps?: Partial<Record<SectionName, number>>;
  /** Number of recent session episodes to include. Default: 1. 0 to suppress. */
  episodes?: number;
  /** Whether to include global knowledge (cross-project facts).
   *  In semantic mode: included only if relevant (similarity ≥ 0.45).
   *  In bulk mode: excluded unless explicitly true. Default: false. */
  include_global?: boolean;
}

export type InjectionMode = "semantic" | "bulk";

export interface ContextResponse {
  /** Pre-rendered markdown block suitable for direct LLM context injection. */
  markdown: string;
  stats: ContextStats;
}

export interface ContextStats {
  facts_injected: number;
  mode: InjectionMode;
  episodes_injected: number;
  global_facts_injected: number;
  char_count: number;
  /** True if some facts were dropped to stay within max_chars budget. */
  budget_exhausted: boolean;
}

// ─── POST /api/memory/recall ──────────────────────────────────────────────────

export interface RecallRequest {
  mind: string;
  /** Raw text query. Omega embeds it and runs cosine similarity × decay scoring. */
  query: string;
  /** Number of results. Default: 10. */
  k?: number;
  /** Minimum cosine similarity. Default: 0.3. */
  min_similarity?: number;
  /** Optional section filter. */
  section?: SectionName;
}

export interface RecallResponse {
  facts: ScoredFact[];
}

export interface ScoredFact extends FactRecord {
  /** Raw cosine similarity between query vector and fact vector (0.0–1.0). */
  similarity: number;
  /** Combined score: similarity × decay-adjusted confidence. Used for ranking. */
  score: number;
}

// ─── GET /api/memory/facts ────────────────────────────────────────────────────

export interface ListFactsRequest {
  mind: string;
  section?: SectionName;
  status?: FactStatus;
}

export interface ListFactsResponse {
  facts: FactRecord[];
  total: number;
}

// ─── PATCH /api/memory/facts/:id ─────────────────────────────────────────────

export interface UpdateFactRequest {
  action: UpdateFactAction;
  /** For "supersede": the replacement fact content. */
  content?: string;
  /** For "supersede": the section for the replacement fact.
   *  Defaults to the original fact's section. */
  section?: SectionName;
  /** For "supersede": the decay profile for the replacement fact.
   *  Defaults to the original fact's decay profile. */
  decay_profile?: DecayProfileName;
}

export type UpdateFactAction = "reinforce" | "archive" | "supersede";

export interface UpdateFactResponse {
  action: UpdateFactAction;
  fact: FactRecord;
  /** Present when action === "supersede". The new replacement fact. */
  new_fact?: FactRecord;
}

// ─── POST /api/memory/edges ───────────────────────────────────────────────────

export interface CreateEdgeRequest {
  source_id: string;
  target_id: string;
  /** Short verb phrase: "depends_on", "contradicts", "enables", "generalizes", etc. */
  relation: string;
  description?: string;
}

export interface CreateEdgeResponse {
  edge: EdgeRecord;
  /** True if an existing edge was reinforced (weight increased) rather than created. */
  reinforced: boolean;
}

// ─── GET /api/memory/export ───────────────────────────────────────────────────

/**
 * Response: Content-Type: application/x-ndjson
 * Each line is one JSONL record: FactRecord | EpisodeRecord | EdgeRecord | MindRecord
 * Line ordering: minds first, then facts, then edges, then episodes.
 * Deterministic output (sorted by id within each type) for stable git diffs.
 */
export interface ExportOptions {
  mind: string;
  /** Include archived and superseded facts. Default: true (full history). */
  include_archived?: boolean;
}

// ─── POST /api/memory/import ─────────────────────────────────────────────────

export interface ImportRequest {
  /** Raw JSONL content. Each line is a fact, episode, edge, or mind record.
   *  Conflict resolution: higher `version` (Lamport timestamp) always wins.
   *  Facts without `version` field get version=0 on import. */
  jsonl: string;
}

export interface ImportResponse {
  imported: number;    // new records
  reinforced: number;  // existing records with lower version updated
  skipped: number;     // same or lower version, no change
  errors: number;      // lines that failed to parse
}

// ─── POST /api/memory/episodes ───────────────────────────────────────────────

export interface StoreEpisodeRequest {
  mind: string;
  title: string;
  narrative: string;
  /** ISO 8601 date. Default: today. */
  date?: string;
  affected_nodes?: string[];
  affected_changes?: string[];
  files_changed?: string[];
  tags?: string[];
  tool_calls_count?: number;
}

export interface StoreEpisodeResponse {
  episode: EpisodeRecord;
}

// ─── GET /api/memory/episodes ────────────────────────────────────────────────

export interface ListEpisodesRequest {
  mind: string;
  /** Number of most recent episodes to return. Default: 3. */
  k?: number;
  /** Optional semantic query — returns episodes ranked by narrative similarity. */
  query?: string;
}

export interface ListEpisodesResponse {
  episodes: EpisodeRecord[];
}

// ─── POST /api/memory/parse-extraction ───────────────────────────────────────

/**
 * Validate and parse raw extraction subagent output.
 * The TS extraction subagent sends LLM-generated JSONL here for typed validation.
 * Omega parses each line against the ExtractionAction schema and returns
 * validated actions + a list of lines that failed parsing.
 *
 * Rust: src/memory/extraction.rs::parse_extraction_output
 */
export interface ParseExtractionRequest {
  /** Raw JSONL output from the extraction subagent. */
  raw_output: string;
}

export interface ParseExtractionResponse {
  actions: ExtractionAction[];
  /** Lines that failed schema validation — for debugging. */
  errors: string[];
}

export type ExtractionAction =
  | { type: "store"; content: string; section: SectionName; decay_profile?: DecayProfileName }
  | { type: "archive"; id: string }
  | { type: "supersede"; id: string; content: string; section?: SectionName }
  | { type: "connect"; source_id: string; target_id: string; relation: string; description?: string };

// ─── GET /api/memory/stats ────────────────────────────────────────────────────

export interface MemoryStatsResponse {
  mind: string;
  total_facts: number;
  active_facts: number;
  archived_facts: number;
  superseded_facts: number;
  facts_with_vectors: number;
  embedding_model?: string;   // null if no vectors exist
  embedding_dims?: number;
  episodes: number;
  edges: number;
  db_size_bytes: number;
  /** Lamport clock high-water mark — max version seen in this DB. */
  version_hwm: number;
}

// ─── JSONL wire format ────────────────────────────────────────────────────────

/**
 * The canonical JSONL line types for git-sync.
 * Each line is one of these discriminated union members.
 * Rust: src/memory/jsonl.rs — use serde(tag = "type") for tagged deserialization.
 */
export type JsonlRecord =
  | ({ type: "fact" } & FactRecord)
  | ({ type: "episode" } & EpisodeRecord)
  | ({ type: "edge" } & EdgeRecord)
  | ({ type: "mind" } & MindRecord);

export interface MindRecord {
  name: string;
  created_at: string;
  description?: string;
}

// ─── Embedding metadata ───────────────────────────────────────────────────────

/**
 * Embedding model registration.
 * Stored in `embedding_metadata` table — one row per model ever used.
 * Facts in `facts_vec` carry a `model_name` FK to this table.
 *
 * Mismatch between query vector dims and stored model dims is a hard error:
 * EmbeddingDimensionMismatch { expected: u32, got: u32, model: String }
 *
 * Rust: src/memory/vectors.rs::EmbeddingMetadata
 */
export interface EmbeddingMetadata {
  model_name: string;   // e.g., "qwen3-embedding:0.6b"
  dims: number;         // e.g., 384
  inserted_at: string;  // ISO 8601 — when first used
}

export interface EmbeddingDimensionMismatchError {
  error: "EmbeddingDimensionMismatch";
  expected_dims: number;
  got_dims: number;
  stored_model: string;
  message: string;
}

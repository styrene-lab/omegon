/**
 * Project Memory — Fact Store
 *
 * SQLite-backed storage for memory facts with decay-based reinforcement.
 * Replaces the markdown-based MemoryStorage for structured persistence.
 *
 * Schema:
 *   facts — individual knowledge atoms with confidence decay
 *   minds — named memory stores with lifecycle
 *   facts_fts — FTS5 virtual table for full-text search
 *
 * Rendering:
 *   Active facts are rendered to Markdown-KV for LLM context injection.
 *   The LLM never sees the database directly.
 */

import * as path from "node:path";
import * as fs from "node:fs";
import * as crypto from "node:crypto";
import { SECTIONS, type SectionName } from "./template.js";

/**
 * Resolve the SQLite database constructor.
 * Prefers better-sqlite3 (native, battle-tested), falls back to node:sqlite.
 */
function loadDatabase(): any {
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    return require("better-sqlite3");
  } catch {
    // Fallback: wrap node:sqlite DatabaseSync to match better-sqlite3 API subset
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const { DatabaseSync } = require("node:sqlite");
    return class NodeSqliteWrapper {
      private db: any;
      constructor(filepath: string) {
        this.db = new DatabaseSync(filepath);
      }
      pragma(stmt: string) {
        return this.db.prepare(`PRAGMA ${stmt}`).get();
      }
      exec(sql: string) {
        this.db.exec(sql);
      }
      prepare(sql: string) {
        const s = this.db.prepare(sql);
        return {
          run: (...args: any[]) => s.run(...args),
          get: (...args: any[]) => s.get(...args),
          all: (...args: any[]) => s.all(...args),
        };
      }
      close() {
        this.db.close();
      }
      transaction(fn: Function) {
        return (...args: any[]) => {
          this.db.exec("BEGIN");
          try {
            const result = fn(...args);
            this.db.exec("COMMIT");
            return result;
          } catch (e) {
            this.db.exec("ROLLBACK");
            throw e;
          }
        };
      }
    };
  }
}

const Database = loadDatabase();

/** Generate a short unique ID */
function nanoid(size = 12): string {
  const bytes = crypto.randomBytes(size);
  return bytes.toString("base64url").slice(0, size);
}

/** Normalize content for dedup hashing */
function normalizeForHash(content: string): string {
  return content
    .replace(/^-\s*/, "")
    .trim()
    .toLowerCase()
    .replace(/\s+/g, " ");
}

/** Compute content hash for dedup */
function contentHash(content: string): string {
  return crypto.createHash("sha256").update(normalizeForHash(content)).digest("hex").slice(0, 16);
}

// --- Types ---

export interface Fact {
  id: string;
  mind: string;
  section: string;
  content: string;
  status: "active" | "superseded" | "archived";
  created_at: string;
  created_session: string | null;
  supersedes: string | null;
  superseded_at: string | null;
  archived_at: string | null;
  source: "manual" | "extraction" | "ingest" | "migration";
  content_hash: string;
  confidence: number;
  last_reinforced: string;
  reinforcement_count: number;
  decay_rate: number;
}

export interface MindRecord {
  name: string;
  description: string;
  status: "active" | "refined" | "retired";
  origin_type: "local" | "link" | "remote";
  origin_path: string | null;
  origin_url: string | null;
  readonly: number; // 0 or 1
  parent: string | null;
  created_at: string;
  last_sync: string | null;
}

export interface StoreFactOptions {
  mind?: string;
  section: SectionName;
  content: string;
  source?: Fact["source"];
  session?: string | null;
  supersedes?: string | null;
  confidence?: number;
  reinforcement_count?: number;
  decay_rate?: number;
}

export interface ReinforcementResult {
  reinforced: number;
  added: number;
  newFactIds: string[];
}

export interface Edge {
  id: string;
  source_fact_id: string;
  target_fact_id: string;
  relation: string;
  description: string;
  confidence: number;
  last_reinforced: string;
  reinforcement_count: number;
  decay_rate: number;
  status: "active" | "archived";
  created_at: string;
  created_session: string | null;
  source_mind: string | null;
  target_mind: string | null;
}

export interface EdgeResult {
  added: number;
  reinforced: number;
}

// --- Decay math ---

/** Project-level decay parameters */
const DECAY = {
  /** Base rate — how fast a single-reinforcement fact decays */
  baseRate: 0.05,
  /** Each reinforcement multiplies the effective half-life by this factor */
  reinforcementFactor: 1.8,
  /** Minimum confidence before a fact is considered for exclusion */
  minimumConfidence: 0.1,
  /** Days after which an unreinforced fact hits minimumConfidence */
  halfLifeDays: 14,
} as const;

/**
 * Global-level decay parameters.
 * Shorter base half-life (30d) means unpromoted one-offs decay faster than project facts.
 * Higher reinforcement factor (2.5x) means each cross-project confirmation dramatically
 * extends durability. A fact reinforced by 3 projects holds 71% after 90 days;
 * 5 reinforcements gives 95% at 90 days. Rewards genuinely cross-cutting knowledge.
 */
export const GLOBAL_DECAY = {
  baseRate: 0.023, // ln(2)/30 ≈ 0.023
  reinforcementFactor: 2.5,
  minimumConfidence: 0.1,
  halfLifeDays: 30,
} as const;

/**
 * Compute current confidence for a fact based on time since last reinforcement.
 * Uses exponential decay with reinforcement-adjusted half-life.
 *
 * halfLife = DECAY.halfLifeDays * (DECAY.reinforcementFactor ^ (reinforcement_count - 1))
 * confidence = e^(-ln(2) * daysSinceReinforced / halfLife)
 */
export type DecayProfile = typeof DECAY | typeof GLOBAL_DECAY;

export function computeConfidence(
  daysSinceReinforced: number,
  reinforcementCount: number,
  profile: DecayProfile = DECAY,
): number {
  const halfLife = profile.halfLifeDays * Math.pow(profile.reinforcementFactor, reinforcementCount - 1);
  const confidence = Math.exp(-Math.LN2 * daysSinceReinforced / halfLife);
  return Math.max(confidence, 0);
}

// --- FactStore ---

export class FactStore {
  private db: any;
  private dbPath: string;
  private decayProfile: DecayProfile;

  constructor(memoryDir: string, opts?: { decay?: DecayProfile; dbName?: string }) {
    this.decayProfile = opts?.decay ?? DECAY;
    this.dbPath = path.join(memoryDir, opts?.dbName ?? "facts.db");
    fs.mkdirSync(memoryDir, { recursive: true });
    this.db = new Database(this.dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("foreign_keys = ON");
    this.initSchema();
  }

  private initSchema(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS minds (
        name        TEXT PRIMARY KEY,
        description TEXT NOT NULL DEFAULT '',
        status      TEXT NOT NULL DEFAULT 'active',
        origin_type TEXT NOT NULL DEFAULT 'local',
        origin_path TEXT,
        origin_url  TEXT,
        readonly    INTEGER NOT NULL DEFAULT 0,
        parent      TEXT,
        created_at  TEXT NOT NULL,
        last_sync   TEXT
      );

      CREATE TABLE IF NOT EXISTS facts (
        id                  TEXT PRIMARY KEY,
        mind                TEXT NOT NULL DEFAULT 'default',
        section             TEXT NOT NULL,
        content             TEXT NOT NULL,
        status              TEXT NOT NULL DEFAULT 'active',
        created_at          TEXT NOT NULL,
        created_session     TEXT,
        supersedes          TEXT,
        superseded_at       TEXT,
        archived_at         TEXT,
        source              TEXT NOT NULL DEFAULT 'manual',
        content_hash        TEXT NOT NULL,
        confidence          REAL NOT NULL DEFAULT 1.0,
        last_reinforced     TEXT NOT NULL,
        reinforcement_count INTEGER NOT NULL DEFAULT 1,
        decay_rate          REAL NOT NULL DEFAULT ${DECAY.baseRate},
        FOREIGN KEY (mind) REFERENCES minds(name) ON DELETE CASCADE
      );

      CREATE INDEX IF NOT EXISTS idx_facts_active
        ON facts(mind, status) WHERE status = 'active';
      CREATE INDEX IF NOT EXISTS idx_facts_hash
        ON facts(mind, content_hash);
      CREATE INDEX IF NOT EXISTS idx_facts_section
        ON facts(mind, section) WHERE status = 'active';
      CREATE INDEX IF NOT EXISTS idx_facts_supersedes
        ON facts(supersedes);
      CREATE INDEX IF NOT EXISTS idx_facts_temporal
        ON facts(created_at);
      CREATE INDEX IF NOT EXISTS idx_facts_confidence
        ON facts(mind, confidence) WHERE status = 'active';
      CREATE INDEX IF NOT EXISTS idx_facts_session
        ON facts(created_session);

      CREATE TABLE IF NOT EXISTS edges (
        id                  TEXT PRIMARY KEY,
        source_fact_id      TEXT NOT NULL,
        target_fact_id      TEXT NOT NULL,
        relation            TEXT NOT NULL,
        description         TEXT NOT NULL,
        confidence          REAL NOT NULL DEFAULT 1.0,
        last_reinforced     TEXT NOT NULL,
        reinforcement_count INTEGER NOT NULL DEFAULT 1,
        decay_rate          REAL NOT NULL DEFAULT ${DECAY.baseRate},
        status              TEXT NOT NULL DEFAULT 'active',
        created_at          TEXT NOT NULL,
        created_session     TEXT,
        source_mind         TEXT,
        target_mind         TEXT,
        FOREIGN KEY (source_fact_id) REFERENCES facts(id) ON DELETE CASCADE,
        FOREIGN KEY (target_fact_id) REFERENCES facts(id) ON DELETE CASCADE
      );

      CREATE INDEX IF NOT EXISTS idx_edges_source
        ON edges(source_fact_id) WHERE status = 'active';
      CREATE INDEX IF NOT EXISTS idx_edges_target
        ON edges(target_fact_id) WHERE status = 'active';
      CREATE INDEX IF NOT EXISTS idx_edges_relation
        ON edges(relation) WHERE status = 'active';
    `);

    // FTS5 virtual table for full-text search
    this.db.exec(`
      CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
        id UNINDEXED,
        mind UNINDEXED,
        section UNINDEXED,
        content,
        content='facts',
        content_rowid='rowid'
      );
    `);

    // Triggers to keep FTS in sync
    // Check if triggers exist before creating (CREATE TRIGGER IF NOT EXISTS not universally supported)
    const triggerExists = this.db.prepare(
      `SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='facts_fts_insert'`
    ).get();

    if (!triggerExists) {
      this.db.exec(`
        CREATE TRIGGER facts_fts_insert AFTER INSERT ON facts BEGIN
          INSERT INTO facts_fts(rowid, id, mind, section, content)
            VALUES (NEW.rowid, NEW.id, NEW.mind, NEW.section, NEW.content);
        END;

        CREATE TRIGGER facts_fts_delete AFTER DELETE ON facts BEGIN
          INSERT INTO facts_fts(facts_fts, rowid, id, mind, section, content)
            VALUES ('delete', OLD.rowid, OLD.id, OLD.mind, OLD.section, OLD.content);
        END;

        CREATE TRIGGER facts_fts_update AFTER UPDATE ON facts BEGIN
          INSERT INTO facts_fts(facts_fts, rowid, id, mind, section, content)
            VALUES ('delete', OLD.rowid, OLD.id, OLD.mind, OLD.section, OLD.content);
          INSERT INTO facts_fts(rowid, id, mind, section, content)
            VALUES (NEW.rowid, NEW.id, NEW.mind, NEW.section, NEW.content);
        END;
      `);
    }

    // Ensure 'default' mind exists
    const defaultMind = this.db.prepare(`SELECT 1 FROM minds WHERE name = 'default'`).get();
    if (!defaultMind) {
      this.db.prepare(`
        INSERT INTO minds (name, description, status, origin_type, created_at)
        VALUES ('default', 'Project default memory', 'active', 'local', ?)
      `).run(new Date().toISOString());
    }
  }

  // ---------------------------------------------------------------------------
  // Fact CRUD
  // ---------------------------------------------------------------------------

  /**
   * Store a fact. Returns the fact ID if stored, or null if duplicate.
   * Handles dedup via content_hash and optional explicit supersession.
   */
  storeFact(opts: StoreFactOptions): { id: string; duplicate: boolean } {
    const mind = opts.mind ?? "default";
    const now = new Date().toISOString();
    const hash = contentHash(opts.content);
    const source = opts.source ?? "manual";
    const content = opts.content.replace(/^-\s*/, "").trim();

    // Dedup check — same mind, same hash, still active
    const existing = this.db.prepare(
      `SELECT id FROM facts WHERE mind = ? AND content_hash = ? AND status = 'active'`
    ).get(mind, hash);

    if (existing) {
      // Reinforce the existing fact instead of duplicating
      this.reinforceFact(existing.id);
      return { id: existing.id, duplicate: true };
    }

    const id = nanoid();

    // If superseding, mark old fact
    if (opts.supersedes) {
      this.db.prepare(
        `UPDATE facts SET status = 'superseded', superseded_at = ? WHERE id = ?`
      ).run(now, opts.supersedes);
    }

    this.db.prepare(`
      INSERT INTO facts (id, mind, section, content, status, created_at, created_session,
                         supersedes, source, content_hash, confidence, last_reinforced,
                         reinforcement_count, decay_rate)
      VALUES (?, ?, ?, ?, 'active', ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id, mind, opts.section, content, now,
      opts.session ?? null,
      opts.supersedes ?? null,
      source, hash,
      opts.confidence ?? 1.0,
      now,
      opts.reinforcement_count ?? 1,
      opts.decay_rate ?? this.decayProfile.baseRate,
    );

    return { id, duplicate: false };
  }

  /**
   * Reinforce a fact — bump confidence, extend half-life.
   */
  reinforceFact(id: string): void {
    const now = new Date().toISOString();
    this.db.prepare(`
      UPDATE facts
      SET confidence = 1.0,
          last_reinforced = ?,
          reinforcement_count = reinforcement_count + 1
      WHERE id = ?
    `).run(now, id);
  }

  /**
   * Process extraction output: a list of observed facts and directives.
   * Returns counts of what happened.
   */
  processExtraction(
    mind: string,
    actions: ExtractionAction[],
    session?: string,
  ): ReinforcementResult {
    let reinforced = 0;
    let added = 0;
    const newFactIds: string[] = [];

    const tx = this.db.transaction(() => {
      for (const action of actions) {
        switch (action.type) {
          case "observe": {
            // Fact observed in session — reinforce if exists, add if new
            const hash = contentHash(action.content);
            const existing = this.db.prepare(
              `SELECT id FROM facts WHERE mind = ? AND content_hash = ? AND status = 'active'`
            ).get(mind, hash);

            if (existing) {
              this.reinforceFact(existing.id);
              reinforced++;
            } else {
              const result = this.storeFact({
                mind,
                section: action.section,
                content: action.content,
                source: "extraction",
                session,
              });
              if (!result.duplicate) newFactIds.push(result.id);
              added++;
            }
            break;
          }
          case "reinforce": {
            // Explicit reinforcement by ID
            if (action.id) {
              this.reinforceFact(action.id);
              reinforced++;
            }
            break;
          }
          case "supersede": {
            // Explicit replacement
            if (action.id && action.content && action.section) {
              const result = this.storeFact({
                mind,
                section: action.section,
                content: action.content,
                source: "extraction",
                session,
                supersedes: action.id,
              });
              if (!result.duplicate) newFactIds.push(result.id);
              added++;
            }
            break;
          }
          case "archive": {
            // Explicit archival
            if (action.id) {
              this.archiveFact(action.id);
            }
            break;
          }
        }
      }
    });

    tx();
    return { reinforced, added, newFactIds };
  }

  /** Archive a fact */
  archiveFact(id: string): void {
    const now = new Date().toISOString();
    this.db.prepare(
      `UPDATE facts SET status = 'archived', archived_at = ? WHERE id = ?`
    ).run(now, id);
  }

  /** Archive all facts from a specific session */
  archiveSession(session: string): number {
    const now = new Date().toISOString();
    const result = this.db.prepare(
      `UPDATE facts SET status = 'archived', archived_at = ?
       WHERE created_session = ? AND status = 'active'`
    ).run(now, session);
    return result.changes;
  }

  // ---------------------------------------------------------------------------
  // Edge CRUD
  // ---------------------------------------------------------------------------

  /**
   * Store an edge between two facts. Deduplicates by source+target+relation.
   * If the same edge exists, reinforces it instead.
   */
  storeEdge(opts: {
    sourceFact: string;
    targetFact: string;
    relation: string;
    description: string;
    session?: string;
    sourceMind?: string;
    targetMind?: string;
  }): { id: string; duplicate: boolean } {
    const now = new Date().toISOString();

    // Dedup: same source, target, and relation
    const existing = this.db.prepare(
      `SELECT id FROM edges
       WHERE source_fact_id = ? AND target_fact_id = ? AND relation = ? AND status = 'active'`
    ).get(opts.sourceFact, opts.targetFact, opts.relation);

    if (existing) {
      this.reinforceEdge(existing.id);
      return { id: existing.id, duplicate: true };
    }

    const id = nanoid();
    this.db.prepare(`
      INSERT INTO edges (id, source_fact_id, target_fact_id, relation, description,
                         confidence, last_reinforced, reinforcement_count, decay_rate,
                         status, created_at, created_session, source_mind, target_mind)
      VALUES (?, ?, ?, ?, ?, 1.0, ?, 1, ?, 'active', ?, ?, ?, ?)
    `).run(
      id, opts.sourceFact, opts.targetFact, opts.relation, opts.description,
      now, this.decayProfile.baseRate, now, opts.session ?? null,
      opts.sourceMind ?? null, opts.targetMind ?? null,
    );

    return { id, duplicate: false };
  }

  /** Reinforce an edge */
  reinforceEdge(id: string): void {
    const now = new Date().toISOString();
    this.db.prepare(`
      UPDATE edges
      SET confidence = 1.0, last_reinforced = ?, reinforcement_count = reinforcement_count + 1
      WHERE id = ?
    `).run(now, id);
  }

  /** Archive an edge */
  archiveEdge(id: string): void {
    this.db.prepare(
      `UPDATE edges SET status = 'archived' WHERE id = ?`
    ).run(id);
  }

  /** Get active edges for a fact (both directions) */
  getEdgesForFact(factId: string): Edge[] {
    const edges = this.db.prepare(`
      SELECT * FROM edges
      WHERE (source_fact_id = ? OR target_fact_id = ?) AND status = 'active'
    `).all(factId, factId) as Edge[];

    return this.applyEdgeDecay(edges);
  }

  /** Get all active edges, optionally filtered by mind */
  getActiveEdges(mind?: string): Edge[] {
    let edges: Edge[];
    if (mind) {
      edges = this.db.prepare(`
        SELECT * FROM edges
        WHERE (source_mind = ? OR target_mind = ?) AND status = 'active'
      `).all(mind, mind) as Edge[];
    } else {
      edges = this.db.prepare(
        `SELECT * FROM edges WHERE status = 'active'`
      ).all() as Edge[];
    }
    return this.applyEdgeDecay(edges);
  }

  /** Get a single edge by ID */
  getEdge(id: string): Edge | null {
    const edge = this.db.prepare(`SELECT * FROM edges WHERE id = ?`).get(id) as Edge | null;
    if (edge) {
      const [decayed] = this.applyEdgeDecay([edge]);
      return decayed;
    }
    return null;
  }

  /**
   * Get active edges connected to any of the given fact IDs.
   * Returns top N by reinforcement count, filtered by min confidence after decay.
   */
  getEdgesForFacts(factIds: string[], limit: number = 20, minConfidence: number = DECAY.minimumConfidence): Edge[] {
    if (factIds.length === 0) return [];

    const placeholders = factIds.map(() => "?").join(",");
    const edges = this.db.prepare(`
      SELECT * FROM edges
      WHERE status = 'active'
        AND (source_fact_id IN (${placeholders}) OR target_fact_id IN (${placeholders}))
      ORDER BY reinforcement_count DESC
      LIMIT ?
    `).all(...factIds, ...factIds, limit * 2) as Edge[]; // fetch extra to account for decay filtering

    const decayed = this.applyEdgeDecay(edges);
    return decayed
      .filter(e => e.confidence >= minConfidence)
      .slice(0, limit);
  }

  /** Apply confidence decay to edges (same decay profile as this store's facts) */
  private applyEdgeDecay(edges: Edge[]): Edge[] {
    const now = Date.now();
    for (const edge of edges) {
      const lastReinforced = new Date(edge.last_reinforced).getTime();
      const daysSince = (now - lastReinforced) / (1000 * 60 * 60 * 24);
      edge.confidence = computeConfidence(daysSince, edge.reinforcement_count, this.decayProfile);
    }
    return edges;
  }

  /**
   * Process edge actions from global extraction.
   * Handles connect and reinforce_edge action types.
   */
  processEdges(
    actions: ExtractionAction[],
    session?: string,
  ): EdgeResult {
    let added = 0;
    let reinforced = 0;

    const tx = this.db.transaction(() => {
      for (const action of actions) {
        if (action.type !== "connect") continue;
        if (!action.source || !action.target || !action.relation) continue;

        // Verify both facts exist
        const sourceFact = this.getFact(action.source);
        const targetFact = this.getFact(action.target);
        if (!sourceFact || !targetFact) continue;

        const result = this.storeEdge({
          sourceFact: action.source,
          targetFact: action.target,
          relation: action.relation,
          description: action.description ?? `${action.relation}: ${sourceFact.content.slice(0, 50)} → ${targetFact.content.slice(0, 50)}`,
          session,
          sourceMind: sourceFact.mind,
          targetMind: targetFact.mind,
        });

        if (result.duplicate) reinforced++;
        else added++;
      }
    });

    tx();
    return { added, reinforced };
  }

  // ---------------------------------------------------------------------------
  // Queries
  // ---------------------------------------------------------------------------

  /**
   * Get active facts for a mind, with confidence decay applied.
   * Optionally limit to top N by confidence.
   */
  getActiveFacts(mind: string, limit?: number): Fact[] {
    const facts = this.db.prepare(
      `SELECT * FROM facts WHERE mind = ? AND status = 'active'
       ORDER BY section, created_at`
    ).all(mind) as Fact[];

    // Apply time-based confidence decay
    const now = Date.now();
    for (const fact of facts) {
      const lastReinforced = new Date(fact.last_reinforced).getTime();
      const daysSince = (now - lastReinforced) / (1000 * 60 * 60 * 24);
      fact.confidence = computeConfidence(daysSince, fact.reinforcement_count, this.decayProfile);
    }

    // Sort by confidence descending within each section
    facts.sort((a, b) => {
      if (a.section !== b.section) {
        const idxA = SECTIONS.indexOf(a.section as SectionName);
        const idxB = SECTIONS.indexOf(b.section as SectionName);
        return idxA - idxB;
      }
      return b.confidence - a.confidence;
    });

    if (limit) {
      return facts.slice(0, limit);
    }
    return facts;
  }

  /** Count active facts for a mind */
  countActiveFacts(mind: string): number {
    const row = this.db.prepare(
      `SELECT COUNT(*) as count FROM facts WHERE mind = ? AND status = 'active'`
    ).get(mind);
    return row?.count ?? 0;
  }

  /** Full-text search across all facts (all minds, all statuses) */
  searchFacts(query: string, mind?: string): Fact[] {
    // FTS5 match syntax
    const ftsQuery = query.split(/\s+/).filter(t => t.length > 0).join(" AND ");
    if (!ftsQuery) return [];

    if (mind) {
      return this.db.prepare(`
        SELECT f.* FROM facts f
        JOIN facts_fts fts ON f.rowid = fts.rowid
        WHERE facts_fts MATCH ? AND f.mind = ?
        ORDER BY rank
      `).all(ftsQuery, mind) as Fact[];
    }

    return this.db.prepare(`
      SELECT f.* FROM facts f
      JOIN facts_fts fts ON f.rowid = fts.rowid
      WHERE facts_fts MATCH ?
      ORDER BY rank
    `).all(ftsQuery) as Fact[];
  }

  /** Search archived/superseded facts (replaces searchArchive) */
  searchArchive(query: string, mind?: string): Fact[] {
    const ftsQuery = query.split(/\s+/).filter(t => t.length > 0).join(" AND ");
    if (!ftsQuery) return [];

    if (mind) {
      return this.db.prepare(`
        SELECT f.* FROM facts f
        JOIN facts_fts fts ON f.rowid = fts.rowid
        WHERE facts_fts MATCH ? AND f.mind = ? AND f.status IN ('archived', 'superseded')
        ORDER BY f.created_at DESC
      `).all(ftsQuery, mind) as Fact[];
    }

    return this.db.prepare(`
      SELECT f.* FROM facts f
      JOIN facts_fts fts ON f.rowid = fts.rowid
      WHERE facts_fts MATCH ? AND f.status IN ('archived', 'superseded')
      ORDER BY f.created_at DESC
    `).all(ftsQuery) as Fact[];
  }

  /** Get a single fact by ID */
  getFact(id: string): Fact | null {
    return this.db.prepare(`SELECT * FROM facts WHERE id = ?`).get(id) as Fact | null;
  }

  /** Get supersession chain for a fact */
  getSupersessionChain(id: string): Fact[] {
    const chain: Fact[] = [];
    let current = this.getFact(id);
    while (current) {
      chain.push(current);
      if (current.supersedes) {
        current = this.getFact(current.supersedes);
      } else {
        break;
      }
    }
    return chain;
  }

  // ---------------------------------------------------------------------------
  // Rendering — Markdown-KV for LLM injection
  // ---------------------------------------------------------------------------

  /**
   * Render active facts as Markdown-KV for LLM context injection.
   * Filters by confidence threshold and respects a line budget.
   */
  renderForInjection(mind: string, opts?: { maxFacts?: number; minConfidence?: number; maxEdges?: number }): string {
    const maxFacts = opts?.maxFacts ?? 80;
    const maxEdges = opts?.maxEdges ?? 20;
    const minConfidence = opts?.minConfidence ?? this.decayProfile.minimumConfidence;

    let facts = this.getActiveFacts(mind);

    // Filter by confidence
    facts = facts.filter(f => f.confidence >= minConfidence);

    // Limit
    if (facts.length > maxFacts) {
      // Take top N by confidence, but maintain section grouping
      facts.sort((a, b) => b.confidence - a.confidence);
      facts = facts.slice(0, maxFacts);
      // Re-sort by section
      facts.sort((a, b) => {
        const idxA = SECTIONS.indexOf(a.section as SectionName);
        const idxB = SECTIONS.indexOf(b.section as SectionName);
        if (idxA !== idxB) return idxA - idxB;
        return b.confidence - a.confidence;
      });
    }

    const lines: string[] = [
      "<!-- Project Memory — managed by project-memory extension -->",
      "",
    ];

    const sectionDescriptions: Record<string, string> = {
      Architecture: "_System structure, component relationships, key abstractions_",
      Decisions: "_Choices made and their rationale_",
      Constraints: "_Requirements, limitations, environment details_",
      "Known Issues": "_Bugs, flaky tests, workarounds_",
      "Patterns & Conventions": "_Code style, project conventions, common approaches_",
    };

    // Build a set of rendered fact IDs for edge lookup
    const renderedFactIds = new Set<string>();

    for (const section of SECTIONS) {
      const sectionFacts = facts.filter(f => f.section === section);
      lines.push(`## ${section}`);
      lines.push(sectionDescriptions[section] ?? "");
      lines.push("");
      if (sectionFacts.length > 0) {
        for (const f of sectionFacts) {
          const date = f.created_at.split("T")[0];
          lines.push(`- ${f.content} [${date}]`);
          renderedFactIds.add(f.id);
        }
      }
      lines.push("");
    }

    // Render edges between rendered facts (capped)
    const relevantEdges = renderedFactIds.size > 0
      ? this.getEdgesForFacts([...renderedFactIds], maxEdges, minConfidence)
      : [];

    if (relevantEdges.length > 0) {
      lines.push("## Connections");
      lines.push("_Relationships between facts across domains_");
      lines.push("");
      for (const edge of relevantEdges) {
        const sourceFact = this.getFact(edge.source_fact_id);
        const targetFact = this.getFact(edge.target_fact_id);
        if (!sourceFact || !targetFact) continue;
        const srcLabel = sourceFact.content.length > 60
          ? sourceFact.content.slice(0, 57) + "..."
          : sourceFact.content;
        const tgtLabel = targetFact.content.length > 60
          ? targetFact.content.slice(0, 57) + "..."
          : targetFact.content;
        lines.push(`- ${srcLabel} **—${edge.relation}→** ${tgtLabel}`);
      }
      lines.push("");
    }

    return lines.join("\n");
  }

  // ---------------------------------------------------------------------------
  // Mind management
  // ---------------------------------------------------------------------------

  /** Create a mind */
  createMind(name: string, description: string, opts?: { parent?: string; origin_type?: string; origin_path?: string; readonly?: boolean }): void {
    this.db.prepare(`
      INSERT INTO minds (name, description, status, origin_type, origin_path, readonly, parent, created_at)
      VALUES (?, ?, 'active', ?, ?, ?, ?, ?)
    `).run(
      name, description,
      opts?.origin_type ?? "local",
      opts?.origin_path ?? null,
      opts?.readonly ? 1 : 0,
      opts?.parent ?? null,
      new Date().toISOString(),
    );
  }

  /** Get a mind record */
  getMind(name: string): MindRecord | null {
    return this.db.prepare(`SELECT * FROM minds WHERE name = ?`).get(name) as MindRecord | null;
  }

  /** List all minds */
  listMinds(): (MindRecord & { factCount: number })[] {
    return this.db.prepare(`
      SELECT m.*, COALESCE(fc.count, 0) as factCount
      FROM minds m
      LEFT JOIN (
        SELECT mind, COUNT(*) as count FROM facts WHERE status = 'active' GROUP BY mind
      ) fc ON m.name = fc.mind
      ORDER BY CASE m.status WHEN 'active' THEN 0 WHEN 'refined' THEN 1 WHEN 'retired' THEN 2 END
    `).all() as (MindRecord & { factCount: number })[];
  }

  /** Update mind status */
  setMindStatus(name: string, status: MindRecord["status"]): void {
    this.db.prepare(`UPDATE minds SET status = ? WHERE name = ?`).run(status, name);
  }

  /** Delete a mind and all its facts */
  deleteMind(name: string): void {
    if (name === "default") throw new Error("Cannot delete the default mind");
    const tx = this.db.transaction(() => {
      this.db.prepare(`DELETE FROM facts WHERE mind = ?`).run(name);
      this.db.prepare(`DELETE FROM minds WHERE name = ?`).run(name);
    });
    tx();
  }

  /** Check if a mind exists */
  mindExists(name: string): boolean {
    return !!this.db.prepare(`SELECT 1 FROM minds WHERE name = ?`).get(name);
  }

  /** Check if a mind is readonly */
  isMindReadonly(name: string): boolean {
    const mind = this.getMind(name);
    return mind?.readonly === 1;
  }

  /** Fork a mind — copy all active facts to a new mind */
  forkMind(sourceName: string, newName: string, description: string): void {
    const tx = this.db.transaction(() => {
      this.createMind(newName, description, { parent: sourceName });

      const facts = this.getActiveFacts(sourceName);
      const now = new Date().toISOString();

      for (const fact of facts) {
        this.db.prepare(`
          INSERT INTO facts (id, mind, section, content, status, created_at, created_session,
                             source, content_hash, confidence, last_reinforced,
                             reinforcement_count, decay_rate)
          VALUES (?, ?, ?, ?, 'active', ?, NULL, 'ingest', ?, 1.0, ?, ?, ?)
        `).run(
          nanoid(), newName, fact.section, fact.content, now,
          fact.content_hash, now, fact.reinforcement_count, fact.decay_rate,
        );
      }
    });
    tx();
  }

  /** Ingest facts from one mind into another */
  ingestMind(sourceName: string, targetName: string): { factsIngested: number; duplicatesSkipped: number } {
    const sourceFacts = this.getActiveFacts(sourceName);
    let ingested = 0;
    let skipped = 0;

    const tx = this.db.transaction(() => {
      for (const fact of sourceFacts) {
        const result = this.storeFact({
          mind: targetName,
          section: fact.section as SectionName,
          content: fact.content,
          source: "ingest",
          reinforcement_count: fact.reinforcement_count,
        });
        if (result.duplicate) {
          skipped++;
        } else {
          ingested++;
        }
      }

      // Retire source if writable
      if (!this.isMindReadonly(sourceName)) {
        this.setMindStatus(sourceName, "retired");
      }
    });
    tx();

    return { factsIngested: ingested, duplicatesSkipped: skipped };
  }

  // ---------------------------------------------------------------------------
  // Active mind state (persisted in DB via a settings table or pragma)
  // ---------------------------------------------------------------------------

  /** Get/set active mind using a simple key-value in the DB */
  getActiveMind(): string | null {
    // Use a lightweight approach — store in a settings row
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)
    `);
    const row = this.db.prepare(`SELECT value FROM settings WHERE key = 'active_mind'`).get();
    if (!row) return null;
    const name = row.value;
    if (name && this.mindExists(name)) return name;
    return null;
  }

  setActiveMind(name: string | null): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT)
    `);
    this.db.prepare(`
      INSERT OR REPLACE INTO settings (key, value) VALUES ('active_mind', ?)
    `).run(name);
  }

  // ---------------------------------------------------------------------------
  // JSONL Export/Import — portable fact sync across machines
  // ---------------------------------------------------------------------------

  /**
   * Export all facts and edges to JSONL format.
   * Each line is a self-contained JSON object with type prefix.
   * Includes all statuses so the full history is portable.
   */
  exportToJsonl(): string {
    const lines: string[] = [];

    // Export minds (except default which is auto-created)
    const minds = this.listMinds();
    for (const mind of minds) {
      if (mind.name === "default") continue;
      lines.push(JSON.stringify({
        _type: "mind",
        name: mind.name,
        description: mind.description,
        status: mind.status,
        origin_type: mind.origin_type,
        created_at: mind.created_at,
      }));
    }

    // Export all active facts (all minds)
    const allFacts = this.db.prepare(
      `SELECT * FROM facts WHERE status = 'active' ORDER BY mind, section, created_at`
    ).all() as Fact[];

    for (const fact of allFacts) {
      lines.push(JSON.stringify({
        _type: "fact",
        id: fact.id,
        mind: fact.mind,
        section: fact.section,
        content: fact.content,
        status: fact.status,
        created_at: fact.created_at,
        source: fact.source,
        content_hash: fact.content_hash,
        confidence: fact.confidence,
        last_reinforced: fact.last_reinforced,
        reinforcement_count: fact.reinforcement_count,
        decay_rate: fact.decay_rate,
        supersedes: fact.supersedes,
      }));
    }

    // Export active edges
    const allEdges = this.db.prepare(
      `SELECT * FROM edges WHERE status = 'active' ORDER BY created_at`
    ).all() as Edge[];

    for (const edge of allEdges) {
      lines.push(JSON.stringify({
        _type: "edge",
        id: edge.id,
        source_fact_id: edge.source_fact_id,
        target_fact_id: edge.target_fact_id,
        relation: edge.relation,
        description: edge.description,
        confidence: edge.confidence,
        last_reinforced: edge.last_reinforced,
        reinforcement_count: edge.reinforcement_count,
        decay_rate: edge.decay_rate,
        source_mind: edge.source_mind,
        target_mind: edge.target_mind,
      }));
    }

    return lines.join("\n") + "\n";
  }

  /**
   * Import from JSONL, merging with existing data.
   * Uses content_hash dedup for facts — existing facts get reinforced,
   * new facts get inserted. Edges dedup by source+target+relation.
   * Returns counts of what happened.
   */
  importFromJsonl(jsonl: string): { factsAdded: number; factsReinforced: number; edgesAdded: number; edgesReinforced: number; mindsCreated: number } {
    let factsAdded = 0;
    let factsReinforced = 0;
    let edgesAdded = 0;
    let edgesReinforced = 0;
    let mindsCreated = 0;

    // Map from imported fact ID → local fact ID (for edge remapping)
    const factIdMap = new Map<string, string>();

    const tx = this.db.transaction(() => {
      for (const line of jsonl.split("\n")) {
        const trimmed = line.trim();
        if (!trimmed) continue;

        let record: any;
        try {
          record = JSON.parse(trimmed);
        } catch {
          continue;
        }

        switch (record._type) {
          case "mind": {
            if (!this.mindExists(record.name)) {
              this.createMind(record.name, record.description ?? "", {
                origin_type: record.origin_type ?? "local",
              });
              mindsCreated++;
            }
            break;
          }
          case "fact": {
            const mind = record.mind ?? "default";
            if (!this.mindExists(mind)) {
              this.createMind(mind, "", { origin_type: "local" });
              mindsCreated++;
            }

            // Dedup by content hash
            const hash = record.content_hash ?? contentHash(record.content);
            const existing = this.db.prepare(
              `SELECT id FROM facts WHERE mind = ? AND content_hash = ? AND status = 'active'`
            ).get(mind, hash);

            if (existing) {
              // Reinforce, take higher reinforcement count
              const existingFact = this.getFact(existing.id);
              if (existingFact && record.reinforcement_count > existingFact.reinforcement_count) {
                this.db.prepare(`
                  UPDATE facts SET reinforcement_count = ?, last_reinforced = ?, confidence = 1.0
                  WHERE id = ?
                `).run(record.reinforcement_count, record.last_reinforced ?? new Date().toISOString(), existing.id);
              } else {
                this.reinforceFact(existing.id);
              }
              factIdMap.set(record.id, existing.id);
              factsReinforced++;
            } else {
              const id = nanoid();
              const now = new Date().toISOString();
              this.db.prepare(`
                INSERT INTO facts (id, mind, section, content, status, created_at, created_session,
                                   supersedes, source, content_hash, confidence, last_reinforced,
                                   reinforcement_count, decay_rate)
                VALUES (?, ?, ?, ?, 'active', ?, NULL, ?, ?, ?, ?, ?, ?, ?)
              `).run(
                id, mind, record.section, record.content,
                record.created_at ?? now,
                record.supersedes ?? null,
                record.source ?? "ingest",
                hash,
                record.confidence ?? 1.0,
                record.last_reinforced ?? now,
                record.reinforcement_count ?? 1,
                record.decay_rate ?? this.decayProfile.baseRate,
              );
              factIdMap.set(record.id, id);
              factsAdded++;
            }
            break;
          }
          case "edge": {
            // Remap fact IDs
            const sourceId = factIdMap.get(record.source_fact_id) ?? record.source_fact_id;
            const targetId = factIdMap.get(record.target_fact_id) ?? record.target_fact_id;

            // Verify both facts exist locally
            if (!this.getFact(sourceId) || !this.getFact(targetId)) continue;

            const result = this.storeEdge({
              sourceFact: sourceId,
              targetFact: targetId,
              relation: record.relation,
              description: record.description,
              sourceMind: record.source_mind,
              targetMind: record.target_mind,
            });

            if (result.duplicate) {
              edgesReinforced++;
            } else {
              edgesAdded++;
            }
            break;
          }
        }
      }
    });

    tx();
    return { factsAdded, factsReinforced, edgesAdded, edgesReinforced, mindsCreated };
  }

  /**
   * Get the mtime of the database file, or null if it doesn't exist.
   */
  getDbMtime(): Date | null {
    try {
      const stat = fs.statSync(this.dbPath);
      return stat.mtime;
    } catch {
      return null;
    }
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  close(): void {
    this.db.close();
  }

  getDbPath(): string {
    return this.dbPath;
  }
}

// --- Extraction action types ---

export interface ExtractionAction {
  type: "observe" | "reinforce" | "supersede" | "archive" | "connect";
  id?: string;
  section?: SectionName;
  content?: string;
  // connect-specific fields
  source?: string;
  target?: string;
  relation?: string;
  description?: string;
}

/**
 * Parse extraction agent output (JSONL) into actions.
 * Tolerant — skips malformed lines.
 */
export function parseExtractionOutput(output: string): ExtractionAction[] {
  const actions: ExtractionAction[] = [];
  for (const line of output.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("//") || trimmed.startsWith("#")) continue;
    try {
      const parsed = JSON.parse(trimmed);
      if (parsed.type && typeof parsed.type === "string") {
        actions.push(parsed as ExtractionAction);
      } else if (parsed.action) {
        // Accept {action: "observe"} as alias for {type: "observe"}
        actions.push({ ...parsed, type: parsed.action } as ExtractionAction);
      }
    } catch {
      // Skip malformed lines — best effort
      continue;
    }
  }
  return actions;
}

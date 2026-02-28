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
}

// --- Decay math ---

/** Default decay parameters */
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
 * Compute current confidence for a fact based on time since last reinforcement.
 * Uses exponential decay with reinforcement-adjusted half-life.
 *
 * halfLife = DECAY.halfLifeDays * (DECAY.reinforcementFactor ^ (reinforcement_count - 1))
 * confidence = e^(-ln(2) * daysSinceReinforced / halfLife)
 */
export function computeConfidence(
  daysSinceReinforced: number,
  reinforcementCount: number,
): number {
  const halfLife = DECAY.halfLifeDays * Math.pow(DECAY.reinforcementFactor, reinforcementCount - 1);
  const confidence = Math.exp(-Math.LN2 * daysSinceReinforced / halfLife);
  return Math.max(confidence, 0);
}

// --- FactStore ---

export class FactStore {
  private db: any;
  private dbPath: string;

  constructor(memoryDir: string) {
    this.dbPath = path.join(memoryDir, "facts.db");
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
      opts.decay_rate ?? DECAY.baseRate,
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
              this.storeFact({
                mind,
                section: action.section,
                content: action.content,
                source: "extraction",
                session,
              });
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
              this.storeFact({
                mind,
                section: action.section,
                content: action.content,
                source: "extraction",
                session,
                supersedes: action.id,
              });
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
    return { reinforced, added };
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
      fact.confidence = computeConfidence(daysSince, fact.reinforcement_count);
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
  renderForInjection(mind: string, opts?: { maxFacts?: number; minConfidence?: number }): string {
    const maxFacts = opts?.maxFacts ?? 80;
    const minConfidence = opts?.minConfidence ?? DECAY.minimumConfidence;

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

    for (const section of SECTIONS) {
      const sectionFacts = facts.filter(f => f.section === section);
      lines.push(`## ${section}`);
      lines.push(sectionDescriptions[section] ?? "");
      lines.push("");
      if (sectionFacts.length > 0) {
        for (const f of sectionFacts) {
          const date = f.created_at.split("T")[0];
          lines.push(`- ${f.content} [${date}]`);
        }
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
  type: "observe" | "reinforce" | "supersede" | "archive";
  id?: string;
  section?: SectionName;
  content?: string;
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

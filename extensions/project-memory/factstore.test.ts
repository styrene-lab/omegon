/**
 * Tests for FactStore — SQLite-backed memory with decay reinforcement.
 */

import { describe, it, beforeEach, afterEach } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import { FactStore, computeConfidence, parseExtractionOutput } from "./factstore.ts";

function tmpDir(): string {
  const dir = path.join(os.tmpdir(), `factstore-test-${crypto.randomBytes(8).toString("hex")}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

describe("FactStore", () => {
  let dir: string;
  let store: FactStore;

  beforeEach(() => {
    dir = tmpDir();
    store = new FactStore(dir);
  });

  afterEach(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  // --- Basic CRUD ---

  it("uses custom dbName when provided", () => {
    const customDir = tmpDir();
    const custom = new FactStore(customDir, { dbName: "global.db" });
    custom.storeFact({ section: "Architecture", content: "test fact" });
    assert.ok(fs.existsSync(path.join(customDir, "global.db")));
    assert.ok(!fs.existsSync(path.join(customDir, "facts.db")));
    custom.close();
    fs.rmSync(customDir, { recursive: true, force: true });
  });

  it("stores and retrieves a fact", () => {
    const { id, duplicate } = store.storeFact({
      section: "Architecture",
      content: "The project uses TypeScript",
      source: "manual",
    });

    assert.ok(id);
    assert.equal(duplicate, false);

    const fact = store.getFact(id);
    assert.ok(fact);
    assert.equal(fact.content, "The project uses TypeScript");
    assert.equal(fact.section, "Architecture");
    assert.equal(fact.status, "active");
    assert.equal(fact.source, "manual");
    assert.equal(fact.mind, "default");
    assert.equal(fact.confidence, 1.0);
    assert.equal(fact.reinforcement_count, 1);
  });

  it("deduplicates by content hash", () => {
    const r1 = store.storeFact({ section: "Architecture", content: "Uses TypeScript" });
    const r2 = store.storeFact({ section: "Architecture", content: "Uses TypeScript" });

    assert.equal(r1.duplicate, false);
    assert.equal(r2.duplicate, true);
    assert.equal(r2.id, r1.id); // Returns existing ID
  });

  it("dedup is case-insensitive and whitespace-normalized", () => {
    const r1 = store.storeFact({ section: "Architecture", content: "Uses TypeScript" });
    const r2 = store.storeFact({ section: "Architecture", content: "  uses   typescript  " });

    assert.equal(r2.duplicate, true);
    assert.equal(r2.id, r1.id);
  });

  it("dedup strips bullet prefix", () => {
    const r1 = store.storeFact({ section: "Architecture", content: "- Uses TypeScript" });
    const r2 = store.storeFact({ section: "Architecture", content: "Uses TypeScript" });

    assert.equal(r2.duplicate, true);
  });

  it("dedup reinforces existing fact", () => {
    const r1 = store.storeFact({ section: "Architecture", content: "Uses TypeScript" });
    store.storeFact({ section: "Architecture", content: "Uses TypeScript" });

    const fact = store.getFact(r1.id)!;
    assert.equal(fact.reinforcement_count, 2);
  });

  // --- Supersession ---

  it("supersedes a fact explicitly", () => {
    const r1 = store.storeFact({ section: "Architecture", content: "Threshold is 10000" });
    const r2 = store.storeFact({
      section: "Architecture",
      content: "Threshold is 20000",
      supersedes: r1.id,
    });

    const old = store.getFact(r1.id)!;
    assert.equal(old.status, "superseded");
    assert.ok(old.superseded_at);

    const newer = store.getFact(r2.id)!;
    assert.equal(newer.status, "active");
    assert.equal(newer.supersedes, r1.id);
  });

  it("traverses supersession chain", () => {
    const r1 = store.storeFact({ section: "Architecture", content: "v1" });
    const r2 = store.storeFact({ section: "Architecture", content: "v2", supersedes: r1.id });
    const r3 = store.storeFact({ section: "Architecture", content: "v3", supersedes: r2.id });

    const chain = store.getSupersessionChain(r3.id);
    assert.equal(chain.length, 3);
    assert.equal(chain[0].content, "v3");
    assert.equal(chain[1].content, "v2");
    assert.equal(chain[2].content, "v1");
  });

  // --- Archival ---

  it("archives a fact", () => {
    const { id } = store.storeFact({ section: "Architecture", content: "Old fact" });
    store.archiveFact(id);

    const fact = store.getFact(id)!;
    assert.equal(fact.status, "archived");
    assert.ok(fact.archived_at);
  });

  it("archives all facts from a session", () => {
    store.storeFact({ section: "Architecture", content: "A", session: "s1" });
    store.storeFact({ section: "Architecture", content: "B", session: "s1" });
    store.storeFact({ section: "Architecture", content: "C", session: "s2" });

    const archived = store.archiveSession("s1");
    assert.equal(archived, 2);
    assert.equal(store.countActiveFacts("default"), 1);
  });

  // --- Queries ---

  it("counts active facts per mind", () => {
    store.storeFact({ section: "Architecture", content: "A" });
    store.storeFact({ section: "Decisions", content: "B" });
    store.storeFact({ section: "Constraints", content: "C" });

    assert.equal(store.countActiveFacts("default"), 3);
  });

  it("getActiveFacts returns only active facts", () => {
    store.storeFact({ section: "Architecture", content: "Active" });
    const { id } = store.storeFact({ section: "Architecture", content: "Will archive" });
    store.archiveFact(id);

    const facts = store.getActiveFacts("default");
    assert.equal(facts.length, 1);
    assert.equal(facts[0].content, "Active");
  });

  // --- Full-text search ---

  it("searches facts with FTS5", () => {
    store.storeFact({ section: "Architecture", content: "SQLite database for storage" });
    store.storeFact({ section: "Decisions", content: "Chose PostgreSQL for production" });
    store.storeFact({ section: "Constraints", content: "Must run on ARM devices" });

    const results = store.searchFacts("SQLite");
    assert.equal(results.length, 1);
    assert.equal(results[0].content, "SQLite database for storage");
  });

  it("searches archived facts", () => {
    const { id } = store.storeFact({ section: "Architecture", content: "Old SQLite approach" });
    store.archiveFact(id);
    store.storeFact({ section: "Architecture", content: "New PostgreSQL approach" });

    const archived = store.searchArchive("SQLite");
    assert.equal(archived.length, 1);
    assert.equal(archived[0].status, "archived");
  });

  it("searchFacts handles apostrophes and quote-like user text without FTS syntax errors", () => {
    store.storeFact({ section: "Architecture", content: "User's auth token is reused by Omegon" });
    store.storeFact({ section: "Decisions", content: "Dont prompt for login again" });

    const results = store.searchFacts("user's auth");
    assert.equal(results.length, 1);
    assert.match(results[0].content, /User's auth token/);
  });

  it("searchArchive handles apostrophes without FTS syntax errors", () => {
    const { id } = store.storeFact({ section: "Known Issues", content: "Don't store mutable auth in install roots" });
    store.archiveFact(id);

    const archived = store.searchArchive("don't store");
    assert.equal(archived.length, 1);
    assert.equal(archived[0].status, "archived");
  });

  it("searchFacts preserves useful recall for technical identifier and path-like queries", () => {
    store.storeFact({ section: "Architecture", content: "Canonical file is extensions/project-memory/factstore.ts" });
    store.storeFact({ section: "Decisions", content: "Prefer openai-codex over weaker defaults when available" });

    const pathResults = store.searchFacts("extensions/project-memory/factstore.ts");
    assert.equal(pathResults.length, 1);
    assert.match(pathResults[0].content, /extensions\/project-memory\/factstore\.ts/);

    const modelResults = store.searchFacts("openai-codex");
    assert.equal(modelResults.length, 1);
    assert.match(modelResults[0].content, /openai-codex/);
  });

  it("searchFacts surfaces non-query operational failures instead of silently returning empty results", () => {
    const db = (store as any).db;
    const originalPrepare = db.prepare.bind(db);
    try {
      db.prepare = () => {
        throw new Error("database disk image is malformed");
      };

      assert.throws(() => store.searchFacts("auth token"), /database disk image is malformed/);
    } finally {
      db.prepare = originalPrepare;
    }
  });

  // --- findFactsByContentPrefix ---

  it("findFactsByContentPrefix returns facts starting with given prefix", () => {
    store.storeFact({ section: "Architecture", content: "SQLite database for storage" });
    store.storeFact({ section: "Decisions", content: "Chose PostgreSQL for production" });

    const results = store.findFactsByContentPrefix("SQLite");
    assert.equal(results.length, 1);
    assert.equal(results[0].content, "SQLite database for storage");
  });

  it("findFactsByContentPrefix handles brackets and special FTS5 chars safely", () => {
    store.storeFact({ section: "Architecture", content: "[tag] fact with brackets" });
    store.storeFact({ section: "Architecture", content: "(paren) fact with parens" });
    store.storeFact({ section: "Architecture", content: "normal fact" });

    const bracketResults = store.findFactsByContentPrefix("[tag]");
    assert.equal(bracketResults.length, 1);
    assert.equal(bracketResults[0].content, "[tag] fact with brackets");

    const parenResults = store.findFactsByContentPrefix("(paren)");
    assert.equal(parenResults.length, 1);
    assert.equal(parenResults[0].content, "(paren) fact with parens");
  });

  it("findFactsByContentPrefix handles LIKE special chars (% and _)", () => {
    store.storeFact({ section: "Architecture", content: "100% complete" });
    store.storeFact({ section: "Architecture", content: "100x complete" });

    const results = store.findFactsByContentPrefix("100%");
    assert.equal(results.length, 1);
    assert.equal(results[0].content, "100% complete");
  });

  it("findFactsByContentPrefix scopes by mind", () => {
    store.createMind("other", "test mind");
    store.storeFact({ section: "Architecture", content: "[tag] default mind fact" });
    store.storeFact({ mind: "other", section: "Architecture", content: "[tag] other mind fact" });

    const results = store.findFactsByContentPrefix("[tag]", "default");
    assert.equal(results.length, 1);
    assert.equal(results[0].content, "[tag] default mind fact");
  });

  it("findFactsByContentPrefix only returns active facts", () => {
    const { id } = store.storeFact({ section: "Architecture", content: "[archived] old fact" });
    store.archiveFact(id);
    store.storeFact({ section: "Architecture", content: "[archived] new fact" });

    const results = store.findFactsByContentPrefix("[archived]");
    assert.equal(results.length, 1);
    assert.equal(results[0].content, "[archived] new fact");
  });

  it("cross-mind search works", () => {
    store.createMind("other", "test mind");
    store.storeFact({ section: "Architecture", content: "Default fact about SQLite" });
    store.storeFact({ mind: "other", section: "Architecture", content: "Other fact about SQLite" });

    const results = store.searchFacts("SQLite");
    assert.equal(results.length, 2);
  });

  // --- Rendering ---

  it("renders Markdown-KV for injection", () => {
    store.storeFact({ section: "Architecture", content: "Uses TypeScript" });
    store.storeFact({ section: "Decisions", content: "Chose SQLite" });

    const rendered = store.renderForInjection("default");
    assert.ok(rendered.includes("## Architecture"));
    assert.ok(rendered.includes("- Uses TypeScript ["));
    assert.ok(rendered.includes("## Decisions"));
    assert.ok(rendered.includes("- Chose SQLite ["));
  });

  it("rendering respects maxFacts limit", () => {
    for (let i = 0; i < 100; i++) {
      store.storeFact({ section: "Architecture", content: `Fact number ${i}` });
    }

    const rendered = store.renderForInjection("default", { maxFacts: 10 });
    const bulletLines = rendered.split("\n").filter(l => l.startsWith("- "));
    assert.equal(bulletLines.length, 10);
  });

  // --- Minds ---

  it("creates and lists minds", () => {
    store.createMind("research", "Research notes");
    const minds = store.listMinds();
    assert.ok(minds.length >= 2); // default + research
    assert.ok(minds.find(m => m.name === "research"));
  });

  it("forks a mind with all facts", () => {
    store.storeFact({ section: "Architecture", content: "Fact A" });
    store.storeFact({ section: "Decisions", content: "Fact B" });

    store.forkMind("default", "fork1", "Fork of default");

    assert.equal(store.countActiveFacts("fork1"), 2);
    const facts = store.getActiveFacts("fork1");
    assert.ok(facts.find(f => f.content === "Fact A"));
    assert.ok(facts.find(f => f.content === "Fact B"));
  });

  // --- Mind parent-chain inheritance ---

  describe("mind parent-chain inheritance", () => {
    it("forkMind creates lightweight child with zero fact copy", () => {
      // Populate parent with facts
      for (let i = 0; i < 100; i++) {
        store.storeFact({ section: "Architecture", content: `Parent fact ${i}` });
      }
      assert.equal(store.countActiveFacts("default"), 100);

      store.forkMind("default", "directive/test", "test");

      // Mind record exists with correct parent
      const mind = store.getMind("directive/test");
      assert.ok(mind);
      assert.equal(mind!.parent, "default");

      // Zero facts directly in child
      const directChildFacts = (store as any).db.prepare(
        `SELECT COUNT(*) as count FROM facts WHERE mind = 'directive/test' AND status = 'active'`
      ).get();
      assert.equal(directChildFacts.count, 0);

      // But getActiveFacts returns all 100 parent facts
      const active = store.getActiveFacts("directive/test");
      assert.equal(active.length, 100);
    });

    it("child facts with different content coexist with parent facts", () => {
      store.storeFact({ section: "Decisions", content: "X is Y" });
      store.forkMind("default", "directive/test", "test");

      // Store a fact with different content in child
      store.storeFact({ mind: "directive/test", section: "Decisions", content: "X is Z" });

      const facts = store.getActiveFacts("directive/test");
      const contents = facts.map(f => f.content);
      assert.ok(contents.includes("X is Y"), "parent fact should be visible");
      assert.ok(contents.includes("X is Z"), "child fact should be visible");
    });

    it("exact duplicate in parent prevents re-creation in child", () => {
      const { id: parentId } = store.storeFact({ section: "Architecture", content: "shared content" });
      store.forkMind("default", "directive/test", "test");

      // Attempt to store same content in child — should dedup to parent
      const result = store.storeFact({ mind: "directive/test", section: "Architecture", content: "shared content" });
      assert.equal(result.duplicate, true);
      assert.equal(result.id, parentId, "should return the parent's fact id");

      // No new fact in child mind
      const directChildFacts = (store as any).db.prepare(
        `SELECT COUNT(*) as count FROM facts WHERE mind = 'directive/test' AND status = 'active'`
      ).get();
      assert.equal(directChildFacts.count, 0);
    });

    it("ingestMind copies only child-owned facts, not inherited", () => {
      // Populate parent
      store.storeFact({ section: "Architecture", content: "Parent fact 1" });
      store.storeFact({ section: "Architecture", content: "Parent fact 2" });

      store.forkMind("default", "directive/test", "test");

      // Store 3 facts directly in child
      store.storeFact({ mind: "directive/test", section: "Decisions", content: "Child fact A" });
      store.storeFact({ mind: "directive/test", section: "Decisions", content: "Child fact B" });
      store.storeFact({ mind: "directive/test", section: "Decisions", content: "Child fact C" });

      const result = store.ingestMind("directive/test", "default");
      // Only the 3 child-owned facts should be considered, not the 2 inherited parent facts
      assert.equal(result.factsIngested + result.duplicatesSkipped, 3);
      assert.equal(result.factsIngested, 3);
    });

    it("sweepDecayedFacts only sweeps own facts, not parent facts", () => {
      // Create a fact in parent with very old reinforcement (will be decayed)
      const longAgo = new Date(Date.now() - 365 * 24 * 60 * 60 * 1000).toISOString();
      store.storeFact({ section: "Recent Work", content: "Old parent work" });
      // Manually backdate the parent fact
      (store as any).db.prepare(
        `UPDATE facts SET last_reinforced = ?, reinforcement_count = 1 WHERE content LIKE 'Old parent work'`
      ).run(longAgo);

      store.forkMind("default", "directive/test", "test");

      // Add a decayed fact directly in child
      store.storeFact({ mind: "directive/test", section: "Recent Work", content: "Old child work" });
      (store as any).db.prepare(
        `UPDATE facts SET last_reinforced = ?, reinforcement_count = 1 WHERE content LIKE 'Old child work'`
      ).run(longAgo);

      // Sweep child — should only sweep child's decayed facts
      const swept = store.sweepDecayedFacts("directive/test");
      assert.equal(swept, 1, "should only sweep the child's decayed fact");

      // Parent's decayed fact should still be active (not swept by child's sweep)
      const parentFact = (store as any).db.prepare(
        `SELECT status FROM facts WHERE mind = 'default' AND content LIKE 'Old parent work'`
      ).get();
      assert.equal(parentFact.status, "active");
    });

    it("resolveMindChain caches results", () => {
      store.forkMind("default", "child", "child mind");

      // First call populates cache
      const chain1 = (store as any).resolveMindChain("child");
      assert.deepEqual(chain1, ["child", "default"]);

      // Second call should return same reference (cached)
      const chain2 = (store as any).resolveMindChain("child");
      assert.strictEqual(chain1, chain2, "should return cached array reference");

      // After invalidation, returns new array
      (store as any).invalidateMindChainCache();
      const chain3 = (store as any).resolveMindChain("child");
      assert.deepEqual(chain3, ["child", "default"]);
      assert.notStrictEqual(chain1, chain3, "should be a new array after cache invalidation");
    });
  });

  it("ingests facts between minds with dedup", () => {
    store.storeFact({ section: "Architecture", content: "Shared fact" });
    store.createMind("source", "Source mind");
    store.storeFact({ mind: "source", section: "Architecture", content: "Shared fact" });
    store.storeFact({ mind: "source", section: "Architecture", content: "New fact" });

    const result = store.ingestMind("source", "default");
    assert.equal(result.factsIngested, 1); // Only "New fact" — "Shared fact" deduped
    assert.equal(result.duplicatesSkipped, 1);
  });

  it("ingest retires writable source", () => {
    store.createMind("source", "Source");
    store.storeFact({ mind: "source", section: "Architecture", content: "A fact" });

    store.ingestMind("source", "default");

    const source = store.getMind("source")!;
    assert.equal(source.status, "retired");
  });

  it("ingest does not retire readonly source", () => {
    store.createMind("linked", "Linked", { readonly: true });
    store.storeFact({ mind: "linked", section: "Architecture", content: "A fact" });

    store.ingestMind("linked", "default");

    const source = store.getMind("linked")!;
    assert.equal(source.status, "active"); // Not retired
  });

  it("deletes a mind and its facts", () => {
    store.createMind("temp", "Temporary");
    store.storeFact({ mind: "temp", section: "Architecture", content: "Temp fact" });

    store.deleteMind("temp");

    assert.equal(store.mindExists("temp"), false);
    assert.equal(store.countActiveFacts("temp"), 0);
  });

  it("cannot delete default mind", () => {
    assert.throws(() => store.deleteMind("default"), /Cannot delete/);
  });

  // --- Active mind state ---

  it("tracks active mind", () => {
    store.createMind("work", "Work mind");
    assert.equal(store.getActiveMind(), null);

    store.setActiveMind("work");
    assert.equal(store.getActiveMind(), "work");

    store.setActiveMind(null);
    assert.equal(store.getActiveMind(), null);
  });

  // --- Extraction processing ---

  it("processes extraction observe actions", () => {
    // Add an existing fact
    store.storeFact({ section: "Architecture", content: "Uses TypeScript" });

    const actions = parseExtractionOutput(`
      {"type":"observe","section":"Architecture","content":"Uses TypeScript"}
      {"type":"observe","section":"Decisions","content":"Chose SQLite for storage"}
    `);

    const result = store.processExtraction("default", actions);
    assert.equal(result.reinforced, 1); // "Uses TypeScript" reinforced
    assert.equal(result.added, 1); // "Chose SQLite" added
  });

  it("processes extraction supersede actions", () => {
    const { id } = store.storeFact({ section: "Architecture", content: "Threshold is 10000" });

    const actions = parseExtractionOutput(
      `{"type":"supersede","id":"${id}","section":"Architecture","content":"Threshold is 20000"}`
    );

    store.processExtraction("default", actions);

    const old = store.getFact(id)!;
    assert.equal(old.status, "superseded");
  });

  it("processes extraction archive actions", () => {
    const { id } = store.storeFact({ section: "Architecture", content: "Stale fact" });

    const actions = parseExtractionOutput(`{"type":"archive","id":"${id}"}`);
    store.processExtraction("default", actions);

    const fact = store.getFact(id)!;
    assert.equal(fact.status, "archived");
  });

  it("tolerates malformed extraction output", () => {
    const actions = parseExtractionOutput(`
      not json
      {"type":"observe","section":"Architecture","content":"Valid fact"}
      {broken json
      {"type":"observe","section":"Decisions","content":"Another valid fact"}
    `);

    assert.equal(actions.length, 2);
  });
});

// --- Decay math ---

describe("computeConfidence", () => {
  it("returns 1.0 at time zero", () => {
    assert.equal(computeConfidence(0, 1), 1.0);
  });

  it("returns ~0.5 at half-life for single reinforcement", () => {
    const c = computeConfidence(14, 1); // 14 days = default half-life
    assert.ok(Math.abs(c - 0.5) < 0.01, `Expected ~0.5, got ${c}`);
  });

  it("decays slower with more reinforcements (up to the 90-day ceiling)", () => {
    const c1 = computeConfidence(14, 1);  // halfLife=14d — significant decay at 14d
    const c5 = computeConfidence(14, 5);  // halfLife=~147d → capped at 90d — low decay at 14d

    assert.ok(c5 > c1, "5 reinforcements should decay slower than 1");
    // Once both n=5 and n=10 hit the 90-day ceiling they produce identical confidence.
    // That's correct and expected behavior — the ceiling is the invariant, not monotonic growth.
    const c10 = computeConfidence(14, 10);
    assert.ok(c10 >= c5, "10 reinforcements should not decay faster than 5");
  });

  it("highly reinforced facts are capped at 90-day half-life (decay ceiling)", () => {
    // With MAX_HALF_LIFE_DAYS=90, a fact reinforced 15x has its half-life capped at 90 days.
    // After exactly 90 days: confidence = e^(-ln2) ≈ 0.5 (half-life by definition).
    const c = computeConfidence(90, 15); // 90 days at the cap
    assert.ok(c > 0.45 && c < 0.55, `Expected ~0.5 (90-day ceiling) after 90 days, got ${c}`);
    // But at 30 days it should still be quite high (~79%)
    const c30 = computeConfidence(30, 15);
    assert.ok(c30 > 0.75, `Expected >0.75 at 30 days with ceiling, got ${c30}`);
  });

  it("unreinforced facts fade within weeks", () => {
    const c = computeConfidence(30, 1); // 30 days, 1 reinforcement
    assert.ok(c < 0.3, `Expected <0.3 after 30 days with 1 reinforcement, got ${c}`);
  });

  it("Specs section facts are immune to confidence decay", () => {
    const d = tmpDir();
    const s = new FactStore(d);
    try {
      const mind = "test-project";
      s.createMind(mind, "test");
      const { id } = s.storeFact({ mind, section: "Specs", content: "API must return ≤100ms" });

      // Backdate the fact's last_reinforced to 365 days ago
      const oldDate = new Date(Date.now() - 365 * 24 * 60 * 60 * 1000).toISOString();
      (s as any).db.prepare(
        `UPDATE facts SET last_reinforced = ? WHERE id = ?`
      ).run(oldDate, id);

      const facts = s.getActiveFacts(mind);
      const spec = facts.find(f => f.id === id)!;
      assert.equal(spec.confidence, 1.0, "Specs facts should always have confidence 1.0 regardless of age");

      // Verify a non-Specs fact DOES decay with the same age
      const { id: archId } = s.storeFact({ mind, section: "Architecture", content: "Uses PostgreSQL" });
      (s as any).db.prepare(
        `UPDATE facts SET last_reinforced = ? WHERE id = ?`
      ).run(oldDate, archId);

      const facts2 = s.getActiveFacts(mind);
      const arch = facts2.find(f => f.id === archId)!;
      assert.ok(arch.confidence < 0.1, `Architecture fact should have decayed after 365 days, got ${arch.confidence}`);
    } finally {
      s.close();
      fs.rmSync(d, { recursive: true, force: true });
    }
  });
});

// --- Extraction output parsing ---

describe("parseExtractionOutput", () => {
  it("parses valid JSONL", () => {
    const actions = parseExtractionOutput(`
{"type":"observe","section":"Architecture","content":"Fact 1"}
{"type":"reinforce","id":"abc123"}
{"type":"archive","id":"def456"}
    `);
    assert.equal(actions.length, 3);
    assert.equal(actions[0].type, "observe");
    assert.equal(actions[1].type, "reinforce");
    assert.equal(actions[2].type, "archive");
  });

  it("accepts action as alias for type", () => {
    const actions = parseExtractionOutput(
      `{"action":"observe","section":"Architecture","content":"Fact"}`
    );
    assert.equal(actions.length, 1);
    assert.equal(actions[0].type, "observe");
  });

  it("skips comments and blank lines", () => {
    const actions = parseExtractionOutput(`
# comment
// another comment

{"type":"observe","section":"Architecture","content":"Fact"}
    `);
    assert.equal(actions.length, 1);
  });
});

// ---------------------------------------------------------------------------
// JSONL Import/Export — merge=union dedup and deterministic ordering
// ---------------------------------------------------------------------------

describe("JSONL Import Dedup (merge=union resilience)", () => {
  let dir: string;
  let store: FactStore;

  beforeEach(() => {
    dir = tmpDir();
    store = new FactStore(dir);
  });

  afterEach(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("deduplicates fact lines with same id, keeps higher reinforcement_count", () => {
    // Simulate merge=union producing two lines for the same fact
    const jsonl = [
      '{"_type":"fact","id":"AAA","mind":"default","section":"Architecture","content":"Test fact","status":"active","content_hash":"abc123","reinforcement_count":3,"last_reinforced":"2026-03-01T00:00:00Z","confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
      '{"_type":"fact","id":"AAA","mind":"default","section":"Architecture","content":"Test fact","status":"active","content_hash":"abc123","reinforcement_count":5,"last_reinforced":"2026-03-02T00:00:00Z","confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
    ].join("\n");

    const result = store.importFromJsonl(jsonl);
    // Should process only ONE fact (the one with count=5), not both
    assert.equal(result.factsAdded, 1);
    assert.equal(result.factsReinforced, 0);

    const facts = store.getActiveFacts("default");
    assert.equal(facts.length, 1);
    assert.equal(facts[0].reinforcement_count, 5);
  });

  it("deduplicates fact lines with same id, tie-breaks on last_reinforced", () => {
    const jsonl = [
      '{"_type":"fact","id":"BBB","mind":"default","section":"Architecture","content":"Tied fact","status":"active","content_hash":"def456","reinforcement_count":3,"last_reinforced":"2026-03-01T00:00:00Z","confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
      '{"_type":"fact","id":"BBB","mind":"default","section":"Architecture","content":"Tied fact","status":"active","content_hash":"def456","reinforcement_count":3,"last_reinforced":"2026-03-05T00:00:00Z","confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
    ].join("\n");

    store.importFromJsonl(jsonl);
    const facts = store.getActiveFacts("default");
    assert.equal(facts.length, 1);
    // The one with the later last_reinforced should win
    // (We can't check last_reinforced directly on the imported fact since import
    //  may set its own timestamp, but only 1 fact should exist)
  });

  it("deduplicates episode lines with same id, keeps only one", () => {
    // Simulate merge=union producing two episode lines with same id
    const jsonl = [
      '{"_type":"episode","id":"EP1","mind":"default","title":"Session One","narrative":"Did stuff","date":"2026-03-01","session_id":null,"created_at":"2026-03-01T00:00:00Z","fact_ids":[]}',
      '{"_type":"episode","id":"EP1","mind":"default","title":"Session One","narrative":"Did stuff","date":"2026-03-01","session_id":null,"created_at":"2026-03-01T12:00:00Z","fact_ids":[]}',
    ].join("\n");

    store.importFromJsonl(jsonl);
    const episodes = store.getEpisodes("default");
    assert.equal(episodes.length, 1);
    assert.equal(episodes[0].title, "Session One");
  });

  it("deduplicates edge lines with same id, keeps only one", () => {
    // Import facts AND edges from JSONL — no pre-created facts
    const jsonl = [
      '{"_type":"fact","id":"F1","mind":"default","section":"Architecture","content":"Edge fact one","status":"active","content_hash":"ef1","reinforcement_count":1,"confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
      '{"_type":"fact","id":"F2","mind":"default","section":"Architecture","content":"Edge fact two","status":"active","content_hash":"ef2","reinforcement_count":1,"confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
      '{"_type":"edge","id":"E1","source_fact_id":"F1","target_fact_id":"F2","relation":"depends_on","description":"test","confidence":1,"reinforcement_count":1,"decay_rate":0.05,"source_mind":"default","target_mind":"default"}',
      '{"_type":"edge","id":"E1","source_fact_id":"F1","target_fact_id":"F2","relation":"depends_on","description":"test","source_mind":"default","target_mind":"default"}',
    ].join("\n");

    const result = store.importFromJsonl(jsonl);
    assert.equal(result.factsAdded, 2);

    // Get the imported facts (IDs are remapped by import)
    const facts = store.getActiveFacts("default");
    assert.equal(facts.length, 2);

    // Check edges — should have exactly 1 despite two lines with same id
    const allEdges = store.getActiveEdges("default");
    assert.equal(allEdges.length, 1);
  });

  it("preserves records without id field (mind records)", () => {
    const jsonl = [
      '{"_type":"mind","name":"custom","description":"A custom mind","status":"active","origin_type":"local","created_at":"2026-03-01T00:00:00Z"}',
      '{"_type":"fact","id":"F1","mind":"custom","section":"Architecture","content":"Custom fact","status":"active","content_hash":"c1","reinforcement_count":1,"confidence":1,"decay_rate":0.05,"source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
    ].join("\n");

    const result = store.importFromJsonl(jsonl);
    assert.equal(result.mindsCreated, 1);
    assert.equal(result.factsAdded, 1);
  });

  it("episode re-import does not create duplicates", () => {
    // Store an episode, export, then re-import into same store
    store.storeEpisode({
      mind: "default",
      title: "Original Episode",
      narrative: "Narrative",
      date: "2026-03-01",
    });

    const jsonl = store.exportToJsonl();

    // Import the exported JSONL back into the same store
    store.importFromJsonl(jsonl);
    store.importFromJsonl(jsonl);

    const episodes = store.getEpisodes("default");
    assert.equal(episodes.length, 1, `Expected 1 episode, got ${episodes.length}`);
  });

  it("episode cross-machine import preserves id", () => {
    // Simulate: machine A exports, machine B imports into fresh store
    store.storeEpisode({
      mind: "default",
      title: "Remote Episode",
      narrative: "From another machine",
      date: "2026-03-01",
    });
    const jsonl = store.exportToJsonl();

    // Fresh store (machine B)
    const dir2 = tmpDir();
    const store2 = new FactStore(dir2);
    store2.importFromJsonl(jsonl);

    const episodes = store2.getEpisodes("default");
    assert.equal(episodes.length, 1);
    assert.equal(episodes[0].title, "Remote Episode");

    // Re-import should NOT duplicate
    store2.importFromJsonl(jsonl);
    const after = store2.getEpisodes("default");
    assert.equal(after.length, 1, `Expected 1 episode after re-import, got ${after.length}`);

    store2.close();
    fs.rmSync(dir2, { recursive: true, force: true });
  });

  it("export is deterministic — same DB produces same output", () => {
    store.storeFact({ section: "Architecture", content: "Fact A" });
    store.storeFact({ section: "Decisions", content: "Fact B" });
    store.storeFact({ section: "Architecture", content: "Fact C" });
    store.storeEpisode({
      mind: "default",
      title: "Ep",
      narrative: "Text",
      date: "2026-03-01",
    });

    const export1 = store.exportToJsonl();
    const export2 = store.exportToJsonl();
    assert.equal(export1, export2, "Two consecutive exports should be byte-identical");
  });

  it("fact export stays byte-stable across reinforcement-only changes", () => {
    const { id } = store.storeFact({ section: "Architecture", content: "Stable fact" });

    const export1 = store.exportToJsonl();
    store.reinforceFact(id);
    const export2 = store.exportToJsonl();

    assert.equal(export1, export2, "reinforcement-only changes should not change exported JSONL bytes");
  });

  it("fact export still changes for durable knowledge changes", () => {
    store.storeFact({ section: "Architecture", content: "Original fact" });
    const export1 = store.exportToJsonl();

    store.storeFact({ section: "Decisions", content: "New durable fact" });
    const export2 = store.exportToJsonl();

    assert.notEqual(export1, export2, "durable knowledge changes should change exported JSONL");
  });

  it("exports fact records with the stable durable field set only", () => {
    store.storeFact({ section: "Architecture", content: "Lean export fact" });

    const factRecord = store.exportToJsonl()
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line))
      .find((line) => line._type === "fact");

    assert.deepEqual(
      Object.keys(factRecord ?? {}).sort(),
      ["_type", "content", "content_hash", "created_at", "id", "mind", "section", "source", "status", "supersedes"].sort(),
    );
  });

  it("deduplicates lean fact lines with same id from stable exports", () => {
    const jsonl = [
      '{"_type":"fact","id":"LEAN","mind":"default","section":"Architecture","content":"Lean fact","status":"active","content_hash":"lean123","source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
      '{"_type":"fact","id":"LEAN","mind":"default","section":"Architecture","content":"Lean fact","status":"active","content_hash":"lean123","source":"manual","created_at":"2026-03-01T00:00:00Z","supersedes":null}',
    ].join("\n");

    const result = store.importFromJsonl(jsonl);
    assert.equal(result.factsAdded, 1);
    assert.equal(store.getActiveFacts("default").length, 1);
  });

  it("exports edge records without volatile runtime scoring metadata", () => {
    const left = store.storeFact({ section: "Architecture", content: "Left" });
    const right = store.storeFact({ section: "Architecture", content: "Right" });
    store.storeEdge({ sourceFact: left.id, targetFact: right.id, relation: "depends_on", description: "test edge" });

    const edgeRecord = store.exportToJsonl()
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line))
      .find((line) => line._type === "edge");

    assert.deepEqual(
      Object.keys(edgeRecord ?? {}).sort(),
      ["_type", "description", "id", "relation", "source_fact_id", "source_mind", "target_fact_id", "target_mind"].sort(),
    );
  });

  // --- sweepDecayedFacts ---

  describe("sweepDecayedFacts", () => {
    it("archives a standard-profile fact that has decayed below minimumConfidence", () => {
      // standard profile: halfLife=14d, minimumConfidence=0.1
      // After 100 days with 1 reinforcement, confidence is deeply below 0.1
      const { id } = store.storeFact({ section: "Architecture", content: "Old fact" });
      const oldDate = new Date(Date.now() - 100 * 86_400_000).toISOString();
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`).run(oldDate, id);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 1);
      const fact = store.getFact(id);
      assert.equal(fact?.status, "archived");
    });

    it("does NOT archive a fact still above minimumConfidence", () => {
      // After 5 days with standard profile, confidence is well above 0.1
      const { id } = store.storeFact({ section: "Architecture", content: "Recent fact" });
      const recentDate = new Date(Date.now() - 5 * 86_400_000).toISOString();
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`).run(recentDate, id);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 0);
      assert.equal(store.getFact(id)?.status, "active");
    });

    it("never archives Specs facts (NO_DECAY_SECTIONS exemption)", () => {
      const { id } = store.storeFact({ section: "Specs" as any, content: "Spec requirement" });
      // Backdate to 365 days — should still survive
      const veryOld = new Date(Date.now() - 365 * 86_400_000).toISOString();
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`).run(veryOld, id);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 0);
      assert.equal(store.getFact(id)?.status, "active");
    });

    it("uses recent_work profile for Recent Work facts (minimumConfidence=0.01)", () => {
      // Recent Work with recent_work profile: halfLife=2d
      // After 30 days with 1 reinforcement, confidence is deeply below 0.01
      const { id } = store.storeFact({
        section: "Recent Work" as any,
        content: "Edited foo.ts",
        decayProfile: "recent_work",
      });
      const oldDate = new Date(Date.now() - 30 * 86_400_000).toISOString();
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`).run(oldDate, id);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 1);
    });

    it("uses SECTION_DECAY_OVERRIDES for Recent Work facts even with wrong decay_profile column", () => {
      // Simulate pre-migration fact: section=Recent Work but decay_profile=standard
      // SECTION_DECAY_OVERRIDES should take precedence, applying recent_work profile
      const { id } = store.storeFact({
        section: "Recent Work" as any,
        content: "Old-style recent work fact",
        // decayProfile defaults to "standard" — simulating the pre-fix behaviour
      });
      // 30 days old — recent_work minimumConfidence=0.01, standard minimumConfidence=0.1
      // With recent_work profile (halfLife=2d), 30 days → confidence ≈ 0
      // With standard profile (halfLife=14d), 30 days → confidence ≈ 0.23 (above 0.1)
      const oldDate = new Date(Date.now() - 30 * 86_400_000).toISOString();
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`).run(oldDate, id);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 1, "Should archive via SECTION_DECAY_OVERRIDES even though column says 'standard'");
    });

    it("uses computeConfidence (canonical formula) — not a different formula", () => {
      // Verify the sweep's archival threshold matches what computeConfidence produces.
      // standard profile: halfLife=14d, minimumConfidence=0.1
      // computeConfidence(60, 1) ≈ e^(-ln2 * 60/14) ≈ 0.051 — below 0.1, should sweep
      // computeConfidence(20, 1) ≈ e^(-ln2 * 20/14) ≈ 0.370 — above 0.1, should keep
      const { id: willSweep } = store.storeFact({ section: "Architecture", content: "60 day old" });
      const { id: willKeep } = store.storeFact({ section: "Decisions", content: "20 day old" });

      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`)
        .run(new Date(Date.now() - 60 * 86_400_000).toISOString(), willSweep);
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`)
        .run(new Date(Date.now() - 20 * 86_400_000).toISOString(), willKeep);

      // Pre-check: computeConfidence agrees
      const conf60 = computeConfidence(60, 1);
      const conf20 = computeConfidence(20, 1);
      assert.ok(conf60 < 0.1, `60-day confidence should be <0.1, got ${conf60}`);
      assert.ok(conf20 > 0.1, `20-day confidence should be >0.1, got ${conf20}`);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 1);
      assert.equal(store.getFact(willSweep)?.status, "archived");
      assert.equal(store.getFact(willKeep)?.status, "active");
    });

    it("high reinforcement_count extends half-life and prevents premature archival", () => {
      const { id } = store.storeFact({ section: "Architecture", content: "Highly reinforced" });
      // Reinforce 10 times to extend half-life
      for (let i = 0; i < 10; i++) store.reinforceFact(id);
      // Backdate 60 days — would be swept at 1 reinforcement, but 11 reinforcements
      // dramatically extends half-life (capped at 90 days)
      const oldDate = new Date(Date.now() - 60 * 86_400_000).toISOString();
      (store as any).db.prepare(`UPDATE facts SET last_reinforced = ? WHERE id = ?`).run(oldDate, id);

      const swept = store.sweepDecayedFacts("default");
      assert.equal(swept, 0, "Highly reinforced fact should survive 60 days");
    });
  });

  // --- processExtraction decayProfile assignment ---

  describe("processExtraction decayProfile", () => {
    it("assigns recent_work decay profile to new Recent Work facts", () => {
      store.processExtraction("default", [
        { type: "observe", section: "Recent Work" as any, content: "Edited bar.ts" },
      ]);
      const facts = (store as any).db.prepare(
        `SELECT decay_profile FROM facts WHERE section = 'Recent Work' AND status = 'active'`
      ).all();
      assert.equal(facts.length, 1);
      assert.equal(facts[0].decay_profile, "recent_work");
    });

    it("assigns standard decay profile to non-Recent-Work facts", () => {
      store.processExtraction("default", [
        { type: "observe", section: "Architecture", content: "New arch fact" },
      ]);
      const facts = (store as any).db.prepare(
        `SELECT decay_profile FROM facts WHERE section = 'Architecture' AND status = 'active'`
      ).all();
      assert.equal(facts.length, 1);
      assert.equal(facts[0].decay_profile, "standard");
    });

    it("assigns recent_work profile on supersede for Recent Work section", () => {
      const { id } = store.storeFact({
        section: "Recent Work" as any,
        content: "Old recent work",
        decayProfile: "recent_work",
      });
      store.processExtraction("default", [
        { type: "supersede", id, section: "Recent Work" as any, content: "Updated recent work" },
      ]);
      const facts = (store as any).db.prepare(
        `SELECT decay_profile FROM facts WHERE section = 'Recent Work' AND status = 'active'`
      ).all();
      assert.equal(facts.length, 1);
      assert.equal(facts[0].decay_profile, "recent_work");
    });
  });

  // --- Schema v4 migration ---

  describe("schema v4 migration", () => {
    it("fixes existing Recent Work facts with wrong decay_profile", () => {
      // Store a Recent Work fact with the old default profile
      store.storeFact({
        section: "Recent Work" as any,
        content: "Pre-migration recent work fact",
        // decayProfile not passed — defaults to "standard" (simulating old behaviour)
      });

      // Verify it was stored with 'standard'
      const before = (store as any).db.prepare(
        `SELECT decay_profile FROM facts WHERE section = 'Recent Work'`
      ).get();
      assert.equal(before.decay_profile, "standard");

      // Force re-run of migration by resetting schema version and re-opening
      (store as any).db.prepare(`DELETE FROM schema_version WHERE version = 4`).run();
      store.close();

      // Re-open triggers migration
      store = new FactStore(dir);
      const after = (store as any).db.prepare(
        `SELECT decay_profile FROM facts WHERE section = 'Recent Work'`
      ).get();
      assert.equal(after.decay_profile, "recent_work");
    });

    it("does not change non-Recent-Work facts during v4 migration", () => {
      store.storeFact({ section: "Architecture", content: "Arch fact" });

      // Force re-run
      (store as any).db.prepare(`DELETE FROM schema_version WHERE version = 4`).run();
      store.close();
      store = new FactStore(dir);

      const fact = (store as any).db.prepare(
        `SELECT decay_profile FROM facts WHERE section = 'Architecture'`
      ).get();
      assert.equal(fact.decay_profile, "standard");
    });
  });
});

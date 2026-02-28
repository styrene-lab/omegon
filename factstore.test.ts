/**
 * Tests for FactStore — SQLite-backed memory with decay reinforcement.
 */

import { describe, it, beforeEach, afterEach } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import { FactStore, computeConfidence, parseExtractionOutput } from "./factstore.js";

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

  it("decays slower with more reinforcements", () => {
    const c1 = computeConfidence(14, 1);
    const c5 = computeConfidence(14, 5);
    const c10 = computeConfidence(14, 10);

    assert.ok(c5 > c1, "5 reinforcements should decay slower than 1");
    assert.ok(c10 > c5, "10 reinforcements should decay slower than 5");
  });

  it("highly reinforced facts remain confident for months", () => {
    const c = computeConfidence(90, 15); // 90 days, 15 reinforcements
    assert.ok(c > 0.8, `Expected >0.8 after 90 days with 15 reinforcements, got ${c}`);
  });

  it("unreinforced facts fade within weeks", () => {
    const c = computeConfidence(30, 1); // 30 days, 1 reinforcement
    assert.ok(c < 0.3, `Expected <0.3 after 30 days with 1 reinforcement, got ${c}`);
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

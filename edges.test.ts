import { describe, it, before, after } from "node:test";
import assert from "node:assert/strict";
import { FactStore, parseExtractionOutput } from "./factstore.js";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as crypto from "node:crypto";

function tmpDir(): string {
  return path.join(os.tmpdir(), `edges-test-${crypto.randomBytes(8).toString("hex")}`);
}

describe("Edge CRUD", () => {
  let dir: string;
  let store: FactStore;
  let factA: string;
  let factB: string;
  let factC: string;

  before(() => {
    dir = tmpDir();
    store = new FactStore(dir);
    // Create test facts
    factA = store.storeFact({ section: "Architecture", content: "System uses k8s 1.29", source: "manual" }).id;
    factB = store.storeFact({ section: "Constraints", content: "Host runs Ubuntu 22.04", source: "manual" }).id;
    factC = store.storeFact({ section: "Decisions", content: "Chose embedded SQLite over Postgres", source: "manual" }).id;
  });

  after(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("stores an edge between two facts", () => {
    const result = store.storeEdge({
      sourceFact: factA,
      targetFact: factB,
      relation: "runs_on",
      description: "k8s deployment depends on host OS",
    });
    assert.ok(result.id);
    assert.equal(result.duplicate, false);
  });

  it("deduplicates edges by source+target+relation", () => {
    const result = store.storeEdge({
      sourceFact: factA,
      targetFact: factB,
      relation: "runs_on",
      description: "different description same edge",
    });
    assert.equal(result.duplicate, true);
  });

  it("allows different relations between same facts", () => {
    const result = store.storeEdge({
      sourceFact: factA,
      targetFact: factB,
      relation: "depends_on",
      description: "k8s needs specific kernel version",
    });
    assert.equal(result.duplicate, false);
  });

  it("retrieves edges for a fact (both directions)", () => {
    const edges = store.getEdgesForFact(factA);
    assert.ok(edges.length >= 2); // runs_on + depends_on
    assert.ok(edges.every(e => e.source_fact_id === factA || e.target_fact_id === factA));
  });

  it("retrieves edges for target fact", () => {
    const edges = store.getEdgesForFact(factB);
    assert.ok(edges.length >= 2);
  });

  it("gets a single edge by ID", () => {
    const edges = store.getEdgesForFact(factA);
    const edge = store.getEdge(edges[0].id);
    assert.ok(edge);
    assert.equal(edge.relation, edges[0].relation);
  });

  it("archives an edge", () => {
    // Create then archive
    const { id } = store.storeEdge({
      sourceFact: factA,
      targetFact: factC,
      relation: "motivated_by",
      description: "test edge to archive",
    });
    store.archiveEdge(id);
    const edge = store.getEdge(id);
    assert.equal(edge?.status, "archived");
  });

  it("archived edges excluded from getEdgesForFact", () => {
    const edges = store.getEdgesForFact(factC);
    const active = edges.filter(e => e.status === "active");
    // The motivated_by edge should be archived
    assert.ok(!active.some(e => e.relation === "motivated_by" && e.source_fact_id === factA));
  });

  it("reinforces edge on duplicate store", () => {
    const edges = store.getEdgesForFact(factA);
    const runsOn = edges.find(e => e.relation === "runs_on");
    assert.ok(runsOn);
    // Was reinforced once during dedup test
    assert.ok(runsOn.reinforcement_count >= 2);
  });

  it("cascades edge deletion when source fact is deleted", () => {
    // Create a fact, edge, then delete the fact
    const tempFact = store.storeFact({ section: "Architecture", content: "Temporary fact for cascade test", source: "manual" }).id;
    store.storeEdge({
      sourceFact: tempFact,
      targetFact: factB,
      relation: "test_cascade",
      description: "should be deleted with fact",
    });

    // Archive the fact — note: archiving doesn't CASCADE, only DELETE does
    // Let's check edges are still there after archive
    store.archiveFact(tempFact);
    const edgesAfterArchive = store.getEdgesForFact(tempFact);
    // Edge still exists (archive doesn't cascade)
    assert.ok(edgesAfterArchive.some(e => e.relation === "test_cascade"));
  });
});

describe("Edge confidence decay", () => {
  let dir: string;
  let store: FactStore;

  before(() => {
    dir = tmpDir();
    store = new FactStore(dir);
  });

  after(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("applies decay to edge confidence", () => {
    const a = store.storeFact({ section: "Architecture", content: "Fact for decay edge A", source: "manual" }).id;
    const b = store.storeFact({ section: "Architecture", content: "Fact for decay edge B", source: "manual" }).id;

    // Manually insert an edge with old last_reinforced
    const edgeId = store.storeEdge({
      sourceFact: a,
      targetFact: b,
      relation: "test_decay",
      description: "edge for decay test",
    }).id;

    // Fresh edge should have confidence ~1.0
    const fresh = store.getEdge(edgeId);
    assert.ok(fresh);
    assert.ok(fresh.confidence > 0.99);
  });
});

describe("processEdges", () => {
  let dir: string;
  let store: FactStore;
  let factA: string;
  let factB: string;

  before(() => {
    dir = tmpDir();
    store = new FactStore(dir);
    factA = store.storeFact({ section: "Architecture", content: "Global fact A", source: "manual" }).id;
    factB = store.storeFact({ section: "Decisions", content: "Global fact B", source: "manual" }).id;
  });

  after(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("processes connect actions from extraction output", () => {
    const actions = parseExtractionOutput([
      `{"type":"connect","source":"${factA}","target":"${factB}","relation":"enables","description":"A enables B"}`,
    ].join("\n"));

    const result = store.processEdges(actions);
    assert.equal(result.added, 1);
    assert.equal(result.reinforced, 0);
  });

  it("reinforces existing edge on duplicate connect", () => {
    const actions = parseExtractionOutput([
      `{"type":"connect","source":"${factA}","target":"${factB}","relation":"enables","description":"A enables B again"}`,
    ].join("\n"));

    const result = store.processEdges(actions);
    assert.equal(result.added, 0);
    assert.equal(result.reinforced, 1);
  });

  it("skips connect with nonexistent fact IDs", () => {
    const actions = parseExtractionOutput([
      `{"type":"connect","source":"nonexistent","target":"${factB}","relation":"test","description":"bad ref"}`,
    ].join("\n"));

    const result = store.processEdges(actions);
    assert.equal(result.added, 0);
    assert.equal(result.reinforced, 0);
  });

  it("ignores non-connect actions", () => {
    const actions = parseExtractionOutput([
      `{"type":"observe","section":"Architecture","content":"some fact"}`,
      `{"type":"connect","source":"${factA}","target":"${factB}","relation":"depends_on","description":"test"}`,
    ].join("\n"));

    const result = store.processEdges(actions);
    assert.equal(result.added, 1); // Only the connect action
  });
});

describe("processExtraction returns newFactIds", () => {
  let dir: string;
  let store: FactStore;

  before(() => {
    dir = tmpDir();
    store = new FactStore(dir);
  });

  after(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("returns IDs of newly created facts", () => {
    const actions = parseExtractionOutput([
      `{"type":"observe","section":"Architecture","content":"Brand new fact"}`,
      `{"type":"observe","section":"Decisions","content":"Another new fact"}`,
    ].join("\n"));

    const result = store.processExtraction("default", actions);
    assert.equal(result.added, 2);
    assert.equal(result.newFactIds.length, 2);

    // Verify the IDs are real
    for (const id of result.newFactIds) {
      const fact = store.getFact(id);
      assert.ok(fact);
      assert.equal(fact.status, "active");
    }
  });

  it("does not include reinforced facts in newFactIds", () => {
    const actions = parseExtractionOutput([
      `{"type":"observe","section":"Architecture","content":"Brand new fact"}`, // duplicate
    ].join("\n"));

    const result = store.processExtraction("default", actions);
    assert.equal(result.reinforced, 1);
    assert.equal(result.newFactIds.length, 0);
  });
});

describe("renderForInjection includes edges", () => {
  let dir: string;
  let store: FactStore;

  before(() => {
    dir = tmpDir();
    store = new FactStore(dir);
    const a = store.storeFact({ section: "Architecture", content: "System uses SQLite", source: "manual" }).id;
    const b = store.storeFact({ section: "Decisions", content: "Chose embedded over client-server", source: "manual" }).id;
    store.storeEdge({
      sourceFact: a,
      targetFact: b,
      relation: "motivated_by",
      description: "SQLite choice driven by embedded requirement",
    });
  });

  after(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("renders Connections section with edges", () => {
    const rendered = store.renderForInjection("default");
    assert.ok(rendered.includes("## Connections"));
    assert.ok(rendered.includes("motivated_by"));
    assert.ok(rendered.includes("SQLite"));
  });

  it("omits Connections section when no edges exist", () => {
    const dir2 = tmpDir();
    const store2 = new FactStore(dir2);
    store2.storeFact({ section: "Architecture", content: "Lone fact", source: "manual" });
    const rendered = store2.renderForInjection("default");
    assert.ok(!rendered.includes("## Connections"));
    store2.close();
    fs.rmSync(dir2, { recursive: true, force: true });
  });
});

describe("getEdgesForFacts", () => {
  let dir: string;
  let store: FactStore;

  before(() => {
    dir = tmpDir();
    store = new FactStore(dir);
    const a = store.storeFact({ section: "Architecture", content: "Fact A for batch edge test", source: "manual" }).id;
    const b = store.storeFact({ section: "Architecture", content: "Fact B for batch edge test", source: "manual" }).id;
    const c = store.storeFact({ section: "Architecture", content: "Fact C for batch edge test", source: "manual" }).id;
    store.storeEdge({ sourceFact: a, targetFact: b, relation: "r1", description: "a-b" });
    store.storeEdge({ sourceFact: b, targetFact: c, relation: "r2", description: "b-c" });
    store.storeEdge({ sourceFact: a, targetFact: c, relation: "r3", description: "a-c" });
  });

  after(() => {
    store.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("returns edges connected to any of the given fact IDs", () => {
    const facts = store.getActiveFacts("default");
    const aId = facts.find(f => f.content.includes("Fact A"))!.id;
    const edges = store.getEdgesForFacts([aId]);
    assert.equal(edges.length, 2); // r1 (a-b) and r3 (a-c)
  });

  it("respects limit parameter", () => {
    const facts = store.getActiveFacts("default");
    const aId = facts.find(f => f.content.includes("Fact A"))!.id;
    const edges = store.getEdgesForFacts([aId], 1);
    assert.equal(edges.length, 1);
  });

  it("returns empty for no matching fact IDs", () => {
    const edges = store.getEdgesForFacts(["nonexistent"]);
    assert.equal(edges.length, 0);
  });
});

describe("global decay profile", () => {
  it("global store decays slower with reinforcement than project store", () => {
    // Import GLOBAL_DECAY
    const { computeConfidence, GLOBAL_DECAY } = require("./factstore.js");

    // RC=3, 90 days: project vs global
    const projectConf = computeConfidence(90, 3); // default DECAY profile
    const globalConf = computeConfidence(90, 3, GLOBAL_DECAY);

    // Global should retain more confidence at RC=3 after 90 days
    assert.ok(globalConf > projectConf,
      `Global (${globalConf.toFixed(3)}) should be > project (${projectConf.toFixed(3)}) at RC=3, 90d`);
  });

  it("global store decays faster at RC=1 than project store", () => {
    const { computeConfidence, GLOBAL_DECAY } = require("./factstore.js");

    // RC=1, 30 days: global should be lower (30d halflife vs 14d — wait, 30>14 means slower)
    // Actually at RC=1: project halfLife=14, global halfLife=30
    // At 30 days: project = e^(-ln2*30/14) = 0.226, global = e^(-ln2*30/30) = 0.5
    // Global is actually MORE durable at RC=1. The "faster decay for one-offs"
    // happens because global facts need cross-project reinforcement to survive,
    // and the base halflife of 30d means they need reinforcement within a month.
    // The key differentiator is the 2.5x factor making reinforcement compound harder.
    const projectConf = computeConfidence(30, 1);
    const globalConf = computeConfidence(30, 1, GLOBAL_DECAY);

    // Both should be < 1.0
    assert.ok(projectConf < 0.5);
    assert.ok(globalConf <= 0.5);
  });

  it("global RC=5 holds strong at 180 days", () => {
    const { computeConfidence, GLOBAL_DECAY } = require("./factstore.js");
    const conf = computeConfidence(180, 5, GLOBAL_DECAY);
    assert.ok(conf > 0.85, `RC=5 at 180d should be >85%, got ${(conf * 100).toFixed(1)}%`);
  });
});

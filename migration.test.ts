/**
 * Tests for migration from markdown memory to SQLite fact store.
 */

import { describe, it, beforeEach, afterEach } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import { FactStore } from "./factstore.js";
import { migrateToFactStore, needsMigration, markMigrated } from "./migration.js";

function tmpDir(): string {
  const dir = path.join(os.tmpdir(), `migration-test-${crypto.randomBytes(8).toString("hex")}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

const SAMPLE_MEMORY = `<!-- Project Memory — managed by project-memory extension -->
<!-- Do not edit while a pi session is actively running -->

## Architecture
_System structure, component relationships, key abstractions_

- The project uses TypeScript with ESM modules
- Storage layer uses markdown files for persistence
- Background extraction spawns a subprocess

## Decisions
_Choices made and their rationale_

- Chose SQLite over JSONL for structured storage
- Memory file has 80-line content limit

## Constraints
_Requirements, limitations, environment details_

- Node.js v22+ required for node:sqlite

## Known Issues
_Bugs, flaky tests, workarounds_

- Dedup only works on exact matches after normalization

## Patterns & Conventions
_Code style, project conventions, common approaches_

- Validate at domain boundary, not UI layer
`;

describe("Migration", () => {
  let dir: string;

  beforeEach(() => {
    dir = tmpDir();
  });

  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("detects when migration is needed", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");
    assert.equal(needsMigration(dir), true);
  });

  it("detects when migration is not needed (no markdown)", () => {
    assert.equal(needsMigration(dir), false);
  });

  it("detects when migration is not needed (db exists)", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");
    fs.writeFileSync(path.join(dir, "facts.db"), "", "utf8");
    assert.equal(needsMigration(dir), false);
  });

  it("migrates default memory.md", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");

    const store = new FactStore(dir);
    const result = migrateToFactStore(dir, store);

    assert.equal(result.errors.length, 0);
    assert.equal(result.factsImported, 8); // 3 + 2 + 1 + 1 + 1

    const facts = store.getActiveFacts("default");
    assert.equal(facts.length, 8);

    // Check sections are correct
    const archFacts = facts.filter(f => f.section === "Architecture");
    assert.equal(archFacts.length, 3);
    assert.ok(archFacts.find(f => f.content === "The project uses TypeScript with ESM modules"));

    // Check migration source
    assert.ok(facts.every(f => f.source === "migration"));

    // Check reinforcement count is elevated for migrated facts
    assert.ok(facts.every(f => f.reinforcement_count === 5));

    store.close();
  });

  it("migrates archive files", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");

    const archiveDir = path.join(dir, "archive");
    fs.mkdirSync(archiveDir, { recursive: true });
    fs.writeFileSync(path.join(archiveDir, "2026-01.md"), `
<!-- Archived 2026-01-15 -->
[Architecture] Old architecture fact that was removed
[Decisions] Old decision that changed
`, "utf8");

    const store = new FactStore(dir);
    const result = migrateToFactStore(dir, store);

    assert.equal(result.archiveFactsImported, 2);

    // Archive facts should still be searchable
    const searchResults = store.searchFacts("Old architecture");
    assert.equal(searchResults.length, 1);

    store.close();
  });

  it("migrates minds", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");

    // Create a mind
    const mindDir = path.join(dir, "minds", "research");
    fs.mkdirSync(mindDir, { recursive: true });
    fs.writeFileSync(path.join(mindDir, "meta.json"), JSON.stringify({
      name: "research",
      description: "Research notes",
      status: "active",
      origin: { type: "local" },
    }), "utf8");
    fs.writeFileSync(path.join(mindDir, "memory.md"), `
## Architecture

- Research finding about performance

## Decisions

- Decided to benchmark three approaches
`, "utf8");

    const store = new FactStore(dir);
    const result = migrateToFactStore(dir, store);

    assert.equal(result.mindsImported, 1);
    assert.ok(store.mindExists("research"));
    assert.equal(store.countActiveFacts("research"), 2);

    store.close();
  });

  it("is idempotent — running twice doesn't duplicate", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");

    const store = new FactStore(dir);
    const r1 = migrateToFactStore(dir, store);
    const r2 = migrateToFactStore(dir, store);

    assert.equal(r1.factsImported, 8);
    assert.equal(r2.factsImported, 0);
    assert.equal(r2.duplicatesSkipped, 8);

    assert.equal(store.countActiveFacts("default"), 8);
    store.close();
  });

  it("marks migration complete by renaming files", () => {
    fs.writeFileSync(path.join(dir, "memory.md"), SAMPLE_MEMORY, "utf8");

    markMigrated(dir);

    assert.equal(fs.existsSync(path.join(dir, "memory.md")), false);
    assert.equal(fs.existsSync(path.join(dir, "memory.md.migrated")), true);
  });
});

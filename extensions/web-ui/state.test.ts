/**
 * Tests for ControlPlaneState snapshot builder.
 *
 * Tests pure logic in state.ts — no HTTP, no pi-tui, no file-system mutations.
 * Uses sharedState manipulation to verify snapshot fidelity.
 */

import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { sharedState } from "../lib/shared-state.ts";
import { buildControlPlaneState, buildSlice } from "./state.ts";
import { SCHEMA_VERSION } from "./types.ts";

// ── Test repo root (this workspace) ──────────────────────────────────────────
// import.meta.url is always defined in ESM (which this project uses), so the
// fallback branch is unreachable in practice — but we guard anyway for clarity.
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../..");
const STARTED_AT = Date.now() - 5000;

// ── Helpers ───────────────────────────────────────────────────────────────────

function buildSnapshot() {
  return buildControlPlaneState(REPO_ROOT, STARTED_AT);
}

// ── Shape tests ───────────────────────────────────────────────────────────────

describe("buildControlPlaneState — top-level shape", () => {
  it("contains schemaVersion", () => {
    const snap = buildSnapshot();
    assert.equal(snap.schemaVersion, SCHEMA_VERSION);
    assert.equal(snap.schemaVersion, 2);
  });

  it("contains all required top-level sections", () => {
    const snap = buildSnapshot();
    const required: Array<keyof typeof snap> = [
      "session",
      "dashboard",
      "designTree",
      "openspec",
      "cleave",
      "models",
      "memory",
      "health",
    ];
    for (const key of required) {
      assert.ok(key in snap, `missing top-level key: ${key}`);
      assert.notEqual(snap[key], null, `${key} should not be null`);
    }
  });

  it("is fully JSON-serialisable", () => {
    const snap = buildSnapshot();
    assert.doesNotThrow(() => JSON.stringify(snap));
    const roundtripped = JSON.parse(JSON.stringify(snap));
    assert.equal(roundtripped.schemaVersion, SCHEMA_VERSION);
  });
});

// ── Session section ───────────────────────────────────────────────────────────

describe("session section", () => {
  it("capturedAt is an ISO 8601 string", () => {
    const { session } = buildSnapshot();
    assert.ok(typeof session.capturedAt === "string");
    assert.ok(!isNaN(Date.parse(session.capturedAt)));
  });

  it("repoRoot matches the provided path", () => {
    const { session } = buildSnapshot();
    assert.equal(session.repoRoot, REPO_ROOT);
  });

  it("piKitVersion is a non-empty string", () => {
    const { session } = buildSnapshot();
    assert.ok(typeof session.piKitVersion === "string");
    assert.ok(session.piKitVersion.length > 0);
  });

  it("gitBranch is a string or null", () => {
    const { session } = buildSnapshot();
    assert.ok(
      session.gitBranch === null || typeof session.gitBranch === "string"
    );
  });
});

// ── Health section ────────────────────────────────────────────────────────────

describe("health section", () => {
  it("status is ok", () => {
    const { health } = buildSnapshot();
    assert.equal(health.status, "ok");
  });

  it("uptimeMs reflects elapsed time since startedAt", () => {
    const { health } = buildSnapshot();
    assert.ok(health.uptimeMs >= 5000);
    assert.ok(health.uptimeMs < 60_000);
  });

  it("serverAlive is true", () => {
    const { health } = buildSnapshot();
    assert.equal(health.serverAlive, true);
  });
});

// ── Dashboard section ─────────────────────────────────────────────────────────

describe("dashboard section — derived from sharedState", () => {
  beforeEach(() => {
    // Reset relevant fields
    sharedState.memoryTokenEstimate = 0;
    sharedState.recovery = undefined;
    sharedState.effort = undefined;
    sharedState.dashboardMode = undefined;
    sharedState.dashboardTurns = undefined;
  });

  it("mode defaults to 'compact' when sharedState.dashboardMode is absent", () => {
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.mode, "compact");
  });

  it("mode reflects sharedState.dashboardMode when set", () => {
    sharedState.dashboardMode = "raised";
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.mode, "raised");
  });

  it("turns defaults to 0 when sharedState.dashboardTurns is absent", () => {
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.turns, 0);
  });

  it("turns reflects sharedState.dashboardTurns when set", () => {
    sharedState.dashboardTurns = 42;
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.turns, 42);
  });

  it("memoryTokenEstimate matches sharedState", () => {
    sharedState.memoryTokenEstimate = 1234;
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.memoryTokenEstimate, 1234);
  });

  it("recovery is null when sharedState.recovery is absent", () => {
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.recovery, null);
  });

  it("recovery reflects latest sharedState.recovery", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-sonnet",
      classification: "rate_limited",
      summary: "Rate limited",
      action: "cooldown",
      retryCount: 2,
      maxRetries: 3,
      timestamp: 1_000_000,
      escalated: false,
      target: undefined,
      cooldowns: undefined,
      attemptId: undefined,
    };
    const { dashboard } = buildSnapshot();
    assert.equal(dashboard.recovery?.provider, "anthropic");
    assert.equal(dashboard.recovery?.classification, "rate_limited");
    assert.equal(dashboard.recovery?.retryCount, 2);
  });

  it("snapshot is derived from latest live state (not stale)", () => {
    sharedState.memoryTokenEstimate = 100;
    const snap1 = buildSnapshot();
    assert.equal(snap1.dashboard.memoryTokenEstimate, 100);

    // Simulate a state change
    sharedState.memoryTokenEstimate = 9999;
    const snap2 = buildSnapshot();
    assert.equal(snap2.dashboard.memoryTokenEstimate, 9999);
    // Snap1 still has old value — no shared reference
    assert.equal(snap1.dashboard.memoryTokenEstimate, 100);
  });
});

// ── Cleave section ────────────────────────────────────────────────────────────

describe("cleave section", () => {
  beforeEach(() => {
    sharedState.cleave = undefined;
  });

  it("defaults to idle when sharedState.cleave is absent", () => {
    const { cleave } = buildSnapshot();
    assert.equal(cleave.status, "idle");
    assert.equal(cleave.runId, null);
    assert.deepEqual(cleave.children, []);
    assert.equal(cleave.updatedAt, null);
  });

  it("reflects active cleave run", () => {
    sharedState.cleave = {
      status: "dispatching",
      runId: "run-abc",
      children: [
        { label: "api", status: "running", elapsed: 1200 },
        { label: "db", status: "pending" },
      ],
      updatedAt: 1_700_000_000_000,
    };
    const { cleave } = buildSnapshot();
    assert.equal(cleave.status, "dispatching");
    assert.equal(cleave.runId, "run-abc");
    assert.equal(cleave.children.length, 2);
    assert.equal(cleave.children[0].label, "api");
    assert.equal(cleave.children[0].elapsed, 1200);
    assert.equal(cleave.children[1].elapsed, null);
    assert.equal(cleave.updatedAt, 1_700_000_000_000);
  });
});

// ── Memory section ────────────────────────────────────────────────────────────

describe("memory section", () => {
  beforeEach(() => {
    sharedState.memoryTokenEstimate = 0;
    sharedState.lastMemoryInjection = undefined;
  });

  it("tokenEstimate matches sharedState", () => {
    sharedState.memoryTokenEstimate = 512;
    const { memory } = buildSnapshot();
    assert.equal(memory.tokenEstimate, 512);
  });

  it("lastInjection is null when not set", () => {
    const { memory } = buildSnapshot();
    assert.equal(memory.lastInjection, null);
  });

  it("lastInjection aggregates fact counts", () => {
    sharedState.lastMemoryInjection = {
      mode: "semantic" as any,
      projectFactCount: 10,
      edgeCount: 2,
      workingMemoryFactCount: 3,
      semanticHitCount: 7,
      episodeCount: 5,
      globalFactCount: 4,
      payloadChars: 2000,
      estimatedTokens: 800,
    };
    const { memory } = buildSnapshot();
    // factCount = projectFactCount + globalFactCount + workingMemoryFactCount
    assert.equal(memory.lastInjection?.factCount, 17);
    assert.equal(memory.lastInjection?.episodeCount, 5);
    assert.equal(memory.lastInjection?.workingMemoryCount, 3);
    assert.equal(memory.lastInjection?.totalTokens, 800);
  });
});

// ── OpenSpec section ──────────────────────────────────────────────────────────

describe("openspec section", () => {
  it("changes is an array", () => {
    const { openspec } = buildSnapshot();
    assert.ok(Array.isArray(openspec.changes));
  });

  it("each change has required fields", () => {
    const { openspec } = buildSnapshot();
    for (const c of openspec.changes) {
      assert.ok(typeof c.name === "string");
      assert.ok(typeof c.stage === "string");
      assert.ok(typeof c.hasProposal === "boolean");
      assert.ok(typeof c.tasksTotal === "number");
      assert.ok(Array.isArray(c.specDomains));
    }
  });
});

// ── DesignTree section ────────────────────────────────────────────────────────

describe("designTree section", () => {
  it("nodeCount is a non-negative integer", () => {
    const { designTree } = buildSnapshot();
    assert.ok(typeof designTree.nodeCount === "number");
    assert.ok(designTree.nodeCount >= 0);
    assert.ok(Number.isInteger(designTree.nodeCount));
  });

  it("nodes is an array", () => {
    const { designTree } = buildSnapshot();
    assert.ok(Array.isArray(designTree.nodes));
  });

  it("each node has required fields", () => {
    const { designTree } = buildSnapshot();
    for (const n of designTree.nodes) {
      assert.ok(typeof n.id === "string");
      assert.ok(typeof n.title === "string");
      assert.ok(typeof n.status === "string");
      assert.ok(Array.isArray(n.questions));
      assert.ok(Array.isArray(n.tags));
    }
  });

  it("openQuestionCount matches sum of node question counts", () => {
    const { designTree } = buildSnapshot();
    const sum = designTree.nodes.reduce((acc, n) => acc + n.questionCount, 0);
    assert.equal(designTree.openQuestionCount, sum);
  });
});

// ── buildSlice ────────────────────────────────────────────────────────────────

describe("buildSlice", () => {
  const SLICES = [
    "session",
    "dashboard",
    "designTree",
    "openspec",
    "cleave",
    "models",
    "memory",
    "health",
  ] as const;

  for (const slice of SLICES) {
    it(`slice '${slice}' returns a non-null object`, () => {
      const result = buildSlice(slice, REPO_ROOT, STARTED_AT);
      assert.notEqual(result, null);
      assert.equal(typeof result, "object");
    });
  }

  it("slice result is JSON-serialisable", () => {
    for (const slice of SLICES) {
      assert.doesNotThrow(
        () => JSON.stringify(buildSlice(slice, REPO_ROOT, STARTED_AT)),
        `slice '${slice}' is not JSON-serialisable`
      );
    }
  });
});

describe("designPipeline section", () => {
  it("designPipeline is present in ControlPlaneState", () => {
    const snap = buildControlPlaneState(process.cwd(), Date.now());
    assert.ok(snap.designPipeline !== undefined, "designPipeline should be present");
  });

  it("designPipeline.capturedAt is an ISO string", () => {
    const snap = buildControlPlaneState(process.cwd(), Date.now());
    assert.ok(typeof snap.designPipeline.capturedAt === "string");
    assert.ok(!isNaN(Date.parse(snap.designPipeline.capturedAt)));
  });

  it("designPipeline.changes is an array", () => {
    const snap = buildControlPlaneState(process.cwd(), Date.now());
    assert.ok(Array.isArray(snap.designPipeline.changes));
  });

  it("designPipeline.funnelCounts has numeric fields", () => {
    const snap = buildControlPlaneState(process.cwd(), Date.now());
    const { funnelCounts } = snap.designPipeline;
    assert.ok(typeof funnelCounts.total === "number");
    assert.ok(typeof funnelCounts.bound === "number");
    assert.ok(typeof funnelCounts.tasksComplete === "number");
    assert.ok(typeof funnelCounts.assessed === "number");
    assert.ok(typeof funnelCounts.archived === "number");
  });

  it("buildSlice('designPipeline') returns the same shape", () => {
    const slice = buildSlice("designPipeline", process.cwd(), Date.now());
    assert.ok(slice !== null && typeof slice === "object");
    const dp = slice as ReturnType<typeof buildControlPlaneState>["designPipeline"];
    assert.ok(typeof dp.capturedAt === "string");
    assert.ok(Array.isArray(dp.changes));
  });
});

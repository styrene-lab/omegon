/**
 * Tests for dashboard overlay data builders and state management.
 *
 * Tests pure logic in overlay-data.ts — no pi-tui dependency needed.
 */
import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { sharedState } from "../lib/shared-state.ts";
import {
  TABS,
  MAX_CONTENT_LINES,
  statusIcon,
  buildDesignItems,
  buildOpenSpecItems,
  buildCleaveItems,
  rebuildItems,
  clampIndex,
  type ThemeFn,
} from "./overlay-data.ts";

// ── Stub theme function (pass-through, no ANSI) ────────────────

const th: ThemeFn = (_color, text) => text;

// ── Helpers ─────────────────────────────────────────────────────

function renderItem(item: { lines: (th: ThemeFn, w: number) => string[] }): string {
  return item.lines(th, 60).join(" ");
}

// ── Tests ───────────────────────────────────────────────────────

describe("TABS", () => {
  it("has 4 tabs with correct IDs", () => {
    assert.equal(TABS.length, 4);
    assert.deepEqual(TABS.map(t => t.id), ["design", "openspec", "cleave", "system"]);
  });
});

describe("statusIcon", () => {
  it("returns correct icons for all known statuses", () => {
    assert.equal(statusIcon("decided", th), "●");
    assert.equal(statusIcon("exploring", th), "◐");
    assert.equal(statusIcon("seed", th), "◌");
    assert.equal(statusIcon("blocked", th), "✕");
    assert.equal(statusIcon("deferred", th), "◑");
    assert.equal(statusIcon("implementing", th), "⟳");
    assert.equal(statusIcon("implemented", th), "✓");
  });

  it("returns default icon for unknown status", () => {
    assert.equal(statusIcon("nonsense", th), "○");
    assert.equal(statusIcon("", th), "○");
  });
});

describe("clampIndex", () => {
  it("returns 0 for empty list", () => {
    assert.equal(clampIndex(5, 0), 0);
    assert.equal(clampIndex(0, 0), 0);
    assert.equal(clampIndex(-1, 0), 0);
  });

  it("clamps to last item when index too large", () => {
    assert.equal(clampIndex(10, 3), 2);
    assert.equal(clampIndex(3, 3), 2);
  });

  it("preserves valid index", () => {
    assert.equal(clampIndex(1, 5), 1);
    assert.equal(clampIndex(0, 1), 0);
  });
});

describe("buildDesignItems", () => {
  it("returns empty for undefined data", () => {
    assert.deepEqual(buildDesignItems(undefined, new Set()), []);
  });

  it("shows empty hint for zero nodes", () => {
    const items = buildDesignItems({
      nodeCount: 0, decidedCount: 0, exploringCount: 0, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0, focusedNode: null,
    }, new Set());
    // Summary + empty hint
    assert.ok(items.find(i => i.key === "dt-empty"), "should show empty hint");
  });

  it("builds summary item", () => {
    const items = buildDesignItems({
      nodeCount: 5, decidedCount: 2, exploringCount: 1, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 3, focusedNode: null,
    }, new Set());

    assert.ok(items.length >= 1);
    assert.equal(items[0]!.key, "dt-summary");
    const text = renderItem(items[0]!);
    assert.ok(text.includes("2 decided"));
    assert.ok(text.includes("1 exploring"));
    assert.ok(text.includes("3 open questions"));
  });

  it("builds focused node item", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 1, exploringCount: 0, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0,
      focusedNode: { id: "n1", title: "Auth Design", status: "decided", questions: [] },
    }, new Set());

    const focused = items.find(i => i.key === "dt-focused-n1");
    assert.ok(focused);
    assert.equal(focused.openUri, undefined, "focused item without filePath should not have openUri");
    const text = renderItem(focused);
    assert.ok(text.includes("Auth Design"));
    assert.ok(text.includes("(focused)"));
  });

  it("emits openUri for linkable design items", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 1, exploringCount: 0, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0,
      focusedNode: {
        id: "n1",
        title: "Clickable Node",
        status: "decided",
        questions: [],
        filePath: `${process.cwd()}/docs/clickable-dashboard.md`,
      },
    }, new Set());

    const focused = items.find(i => i.key === "dt-focused-n1");
    assert.ok(focused?.openUri);
    assert.match(focused!.openUri!, /file:\/\/|http:\/\/localhost:/);
  });

  it("shows questions when expanded", () => {
    const expanded = new Set(["dt-focused-n1"]);
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 2,
      focusedNode: { id: "n1", title: "Node", status: "exploring", questions: ["Why?", "How?"] },
    }, expanded);

    const questions = items.filter(i => i.key.startsWith("dt-q-"));
    assert.equal(questions.length, 2);
    assert.ok(renderItem(questions[0]!).includes("Why?"));
    assert.ok(renderItem(questions[1]!).includes("How?"));
  });

  it("hides questions when collapsed", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 2,
      focusedNode: { id: "n1", title: "Node", status: "exploring", questions: ["Why?", "How?"] },
    }, new Set()); // not expanded

    const questions = items.filter(i => i.key.startsWith("dt-q-"));
    assert.equal(questions.length, 0);
  });

  it("marks focused node as expandable when it has questions", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 1,
      focusedNode: { id: "n1", title: "Node", status: "exploring", questions: ["Why?"] },
    }, new Set());

    const focused = items.find(i => i.key === "dt-focused-n1");
    assert.ok(focused?.expandable);
  });

  it("shows node list when nodes provided", () => {
    const items = buildDesignItems({
      nodeCount: 3, decidedCount: 1, exploringCount: 2, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0, focusedNode: null,
      nodes: [
        { id: "a", title: "Alpha", status: "decided", questionCount: 0 },
        { id: "b", title: "Beta", status: "exploring", questionCount: 2 },
        { id: "c", title: "Gamma", status: "seed", questionCount: 0 },
      ],
    }, new Set());

    const nodeItems = items.filter(i => i.key.startsWith("dt-node-"));
    assert.equal(nodeItems.length, 3, "should list all nodes");
    assert.ok(renderItem(nodeItems[1]!).includes("Beta"), "should include node title");
  });

  it("skips focused node from node list to avoid duplication", () => {
    const items = buildDesignItems({
      nodeCount: 3, decidedCount: 1, exploringCount: 2, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0,
      focusedNode: { id: "b", title: "Beta", status: "exploring", questions: [] },
      nodes: [
        { id: "a", title: "Alpha", status: "decided", questionCount: 0 },
        { id: "b", title: "Beta", status: "exploring", questionCount: 0 },
        { id: "c", title: "Gamma", status: "seed", questionCount: 0 },
      ],
    }, new Set());

    const nodeItems = items.filter(i => i.key.startsWith("dt-node-"));
    assert.equal(nodeItems.length, 2, "focused node should be excluded from list");
    assert.ok(!nodeItems.find(i => i.key === "dt-node-b"), "node b should not appear in list");
    assert.ok(items.find(i => i.key === "dt-focused-b"), "node b should appear as focused");
  });

  it("renders question count badge on nodes with open questions", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 3, focusedNode: null,
      nodes: [
        { id: "x", title: "Has Questions", status: "exploring", questionCount: 3 },
      ],
    }, new Set());

    const node = items.find(i => i.key === "dt-node-x");
    assert.ok(node, "should have node item");
    const text = renderItem(node);
    assert.ok(text.includes("(3?)"), `expected question badge, got: ${text}`);
  });

  it("omits question badge when questionCount is 0", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 1, exploringCount: 0, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0, focusedNode: null,
      nodes: [
        { id: "y", title: "No Questions", status: "decided", questionCount: 0 },
      ],
    }, new Set());

    const node = items.find(i => i.key === "dt-node-y");
    assert.ok(node);
    const text = renderItem(node);
    assert.ok(!text.includes("?"), `should not have question badge, got: ${text}`);
  });

  it("shows empty hint when no nodes array", () => {
    const items = buildDesignItems({
      nodeCount: 0, decidedCount: 0, exploringCount: 0, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0, focusedNode: null,
    }, new Set());

    const emptyItem = items.find(i => i.key === "dt-empty");
    assert.ok(emptyItem, "should show empty hint");
  });
});

describe("buildOpenSpecItems", () => {
  it("returns empty for undefined data", () => {
    assert.deepEqual(buildOpenSpecItems(undefined, new Set()), []);
  });

  it("returns empty for no changes", () => {
    assert.deepEqual(buildOpenSpecItems({ changes: [] }, new Set()), []);
  });

  it("builds summary and change items", () => {
    const items = buildOpenSpecItems({
      changes: [
        { name: "auth-flow", stage: "spec", tasksDone: 2, tasksTotal: 5 },
        { name: "db-migrate", stage: "impl", tasksDone: 3, tasksTotal: 3 },
      ],
    }, new Set());

    assert.ok(items.length >= 3); // summary + 2 changes
    assert.ok(renderItem(items[0]!).includes("2 active changes"));
    assert.ok(renderItem(items[1]!).includes("auth-flow"));
    assert.ok(renderItem(items[1]!).includes("2/5"));
    assert.ok(renderItem(items[2]!).includes("db-migrate"));
  });

  it("shows done icon for completed changes", () => {
    const items = buildOpenSpecItems({
      changes: [{ name: "done-change", stage: "tasks", tasksDone: 5, tasksTotal: 5 }],
    }, new Set());

    const change = items.find(i => i.key === "os-change-done-change");
    assert.ok(change);
    assert.ok(renderItem(change).includes("✓"));
  });

  it("shows stage, artifact rows, and progress bar when expanded", () => {
    const expanded = new Set(["os-change-auth-flow"]);
    const items = buildOpenSpecItems({
      changes: [{
        name: "auth-flow",
        stage: "spec",
        tasksDone: 2,
        tasksTotal: 5,
        path: "openspec/changes/clickable-dashboard-items",
        artifacts: ["proposal", "design", "tasks"],
      }],
    }, expanded);

    const stageItem = items.find(i => i.key === "os-stage-auth-flow");
    assert.ok(stageItem, "should have stage item");
    assert.ok(renderItem(stageItem).includes("stage: spec"));

    const proposalItem = items.find(i => i.key === "os-artifact-auth-flow-proposal");
    const designItem = items.find(i => i.key === "os-artifact-auth-flow-design");
    const tasksItem = items.find(i => i.key === "os-artifact-auth-flow-tasks");
    assert.ok(proposalItem, "should have proposal artifact row");
    assert.ok(designItem, "should have design artifact row");
    assert.ok(tasksItem, "should have tasks artifact row");
    assert.ok(renderItem(proposalItem).includes("proposal"));
    assert.ok(renderItem(designItem).includes("design"));
    assert.ok(renderItem(tasksItem).includes("tasks"));

    const progressItem = items.find(i => i.key === "os-progress-auth-flow");
    assert.ok(progressItem, "should have progress item");
    const progressText = renderItem(progressItem);
    assert.ok(progressText.includes("█"), "should have filled blocks");
    assert.ok(progressText.includes("40%"), "should show percentage");
  });
});

describe("buildCleaveItems", () => {
  it("returns empty for undefined data", () => {
    assert.deepEqual(buildCleaveItems(undefined, new Set()), []);
  });

  it("builds status header", () => {
    const items = buildCleaveItems({
      status: "dispatching", runId: "run-42", children: [],
    }, new Set());

    assert.ok(items.length >= 1);
    const text = renderItem(items[0]!);
    assert.ok(text.includes("dispatching"));
    assert.ok(text.includes("run-42"));
  });

  it("builds child summary and items", () => {
    const items = buildCleaveItems({
      status: "dispatching", runId: undefined,
      children: [
        { label: "alpha", status: "done", elapsed: 45 },
        { label: "beta", status: "running" },
        { label: "gamma", status: "failed", elapsed: 120 },
      ],
    }, new Set());

    const summary = items.find(i => i.key === "cl-summary");
    assert.ok(summary);
    const summaryText = renderItem(summary);
    assert.ok(summaryText.includes("3 children"));
    assert.ok(summaryText.includes("1 ✓"));
    assert.ok(summaryText.includes("1 ⟳"));
    assert.ok(summaryText.includes("1 ✕"));

    const alpha = items.find(i => i.key === "cl-child-alpha");
    assert.ok(alpha);
    assert.ok(alpha.expandable, "child with elapsed should be expandable");
  });

  it("shows elapsed time when expanded", () => {
    const expanded = new Set(["cl-child-alpha"]);
    const items = buildCleaveItems({
      status: "done", runId: undefined,
      children: [{ label: "alpha", status: "done", elapsed: 125 }],
    }, expanded);

    const elapsed = items.find(i => i.key === "cl-elapsed-alpha");
    assert.ok(elapsed, "should have elapsed item");
    const text = renderItem(elapsed);
    assert.ok(text.includes("2m 5s"), `expected '2m 5s', got '${text}'`);
  });

  it("shows seconds-only for short durations", () => {
    const expanded = new Set(["cl-child-fast"]);
    const items = buildCleaveItems({
      status: "done", runId: undefined,
      children: [{ label: "fast", status: "done", elapsed: 8 }],
    }, expanded);

    const elapsed = items.find(i => i.key === "cl-elapsed-fast");
    assert.ok(elapsed);
    assert.ok(renderItem(elapsed).includes("8s"));
  });
});

describe("rebuildItems", () => {
  beforeEach(() => {
    (sharedState as any).designTree = undefined;
    (sharedState as any).openspec = undefined;
    (sharedState as any).cleave = undefined;
  });

  it("dispatches to correct builder per tab", () => {
    (sharedState as any).designTree = {
      nodeCount: 1, decidedCount: 1, exploringCount: 0, blockedCount: 0, implementingCount: 0, implementedCount: 0, deferredCount: 0,
      openQuestionCount: 0, focusedNode: null,
    };
    const designItems = rebuildItems("design", new Set());
    assert.ok(designItems.length > 0);

    const openspecItems = rebuildItems("openspec", new Set());
    assert.equal(openspecItems.length, 0); // no openspec data
  });
});

describe("MAX_CONTENT_LINES", () => {
  it("is a positive number", () => {
    assert.ok(MAX_CONTENT_LINES > 0);
    assert.equal(MAX_CONTENT_LINES, 30);
  });
});

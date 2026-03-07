/**
 * Tests for dashboard overlay data builders and state management.
 *
 * Tests pure logic in overlay-data.ts — no pi-tui dependency needed.
 */
import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { sharedState } from "../shared-state.ts";
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
  it("has 3 tabs with correct IDs", () => {
    assert.equal(TABS.length, 3);
    assert.deepEqual(TABS.map(t => t.id), ["design", "openspec", "cleave"]);
  });
});

describe("statusIcon", () => {
  it("returns correct icons for all known statuses", () => {
    assert.equal(statusIcon("decided", th), "●");
    assert.equal(statusIcon("exploring", th), "◐");
    assert.equal(statusIcon("seed", th), "◌");
    assert.equal(statusIcon("blocked", th), "✕");
    assert.equal(statusIcon("deferred", th), "◑");
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

  it("returns empty for zero nodes", () => {
    assert.deepEqual(buildDesignItems({
      nodeCount: 0, decidedCount: 0, exploringCount: 0, blockedCount: 0,
      openQuestionCount: 0, focusedNode: null,
    }, new Set()), []);
  });

  it("builds summary item", () => {
    const items = buildDesignItems({
      nodeCount: 5, decidedCount: 2, exploringCount: 1, blockedCount: 0,
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
      nodeCount: 1, decidedCount: 1, exploringCount: 0, blockedCount: 0,
      openQuestionCount: 0,
      focusedNode: { id: "n1", title: "Auth Design", status: "decided", questions: [] },
    }, new Set());

    const focused = items.find(i => i.key === "dt-focused-n1");
    assert.ok(focused);
    const text = renderItem(focused);
    assert.ok(text.includes("Auth Design"));
    assert.ok(text.includes("(focused)"));
  });

  it("shows questions when expanded", () => {
    const expanded = new Set(["dt-focused-n1"]);
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0,
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
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0,
      openQuestionCount: 2,
      focusedNode: { id: "n1", title: "Node", status: "exploring", questions: ["Why?", "How?"] },
    }, new Set()); // not expanded

    const questions = items.filter(i => i.key.startsWith("dt-q-"));
    assert.equal(questions.length, 0);
  });

  it("marks focused node as expandable when it has questions", () => {
    const items = buildDesignItems({
      nodeCount: 1, decidedCount: 0, exploringCount: 1, blockedCount: 0,
      openQuestionCount: 1,
      focusedNode: { id: "n1", title: "Node", status: "exploring", questions: ["Why?"] },
    }, new Set());

    const focused = items.find(i => i.key === "dt-focused-n1");
    assert.ok(focused?.expandable);
  });

  it("shows seed hint when no focused node", () => {
    const items = buildDesignItems({
      nodeCount: 3, decidedCount: 1, exploringCount: 0, blockedCount: 0,
      openQuestionCount: 0, focusedNode: null,
    }, new Set());

    const seedItem = items.find(i => i.key === "dt-seeds");
    assert.ok(seedItem, "should have seed hint");
    assert.ok(renderItem(seedItem).includes("seed"));
  });

  it("omits seed hint when all nodes are decided/exploring/blocked", () => {
    const items = buildDesignItems({
      nodeCount: 3, decidedCount: 2, exploringCount: 1, blockedCount: 0,
      openQuestionCount: 0, focusedNode: null,
    }, new Set());

    const seedItem = items.find(i => i.key === "dt-seeds");
    assert.ok(!seedItem, "should not have seed hint when all classified");
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
      changes: [{ name: "done-change", tasksDone: 5, tasksTotal: 5 }],
    }, new Set());

    const change = items.find(i => i.key === "os-change-done-change");
    assert.ok(change);
    assert.ok(renderItem(change).includes("✓"));
  });

  it("shows stage and progress bar when expanded", () => {
    const expanded = new Set(["os-change-auth-flow"]);
    const items = buildOpenSpecItems({
      changes: [{ name: "auth-flow", stage: "spec", tasksDone: 2, tasksTotal: 5 }],
    }, expanded);

    const stageItem = items.find(i => i.key === "os-stage-auth-flow");
    assert.ok(stageItem, "should have stage item");
    assert.ok(renderItem(stageItem).includes("stage: spec"));

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
      status: "running", runId: "run-42", children: [],
    }, new Set());

    assert.ok(items.length >= 1);
    const text = renderItem(items[0]!);
    assert.ok(text.includes("running"));
    assert.ok(text.includes("run-42"));
  });

  it("builds child summary and items", () => {
    const items = buildCleaveItems({
      status: "running", runId: null,
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
      status: "done", runId: null,
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
      status: "done", runId: null,
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
      nodeCount: 1, decidedCount: 1, exploringCount: 0, blockedCount: 0,
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

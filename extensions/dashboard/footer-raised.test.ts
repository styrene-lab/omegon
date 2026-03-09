import { beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";

import { DashboardFooter } from "./footer.ts";
import { sharedState } from "../shared-state.ts";
import type { DashboardState } from "./types.ts";

function makeTheme() {
  return {
    fg: (_color: string, text: string) => text,
    bold: (text: string) => text,
  };
}

function makeFooterData() {
  return {
    getAvailableProviderCount: () => 2,
    getGitBranch: () => "main",
    getExtensionStatuses: () => new Map<string, string>(),
  };
}

function makeContext() {
  return {
    cwd: "/Users/cwilson/workspace/ai/pi-kit",
    model: {
      provider: "openai-codex",
      id: "gpt-5.4",
      reasoning: true,
    },
    getContextUsage: () => ({ percent: 31, contextWindow: 272000 }),
    sessionManager: {
      getEntries: () => [],
      getSessionName: () => undefined,
    },
  };
}

describe("DashboardFooter raised mode polish", () => {
  beforeEach(() => {
    (sharedState as any).designTree = {
      nodeCount: 4,
      decidedCount: 1,
      exploringCount: 1,
      implementingCount: 1,
      implementedCount: 1,
      blockedCount: 0,
      openQuestionCount: 0,
      focusedNode: null,
      implementingNodes: [{ id: "memory", title: "Memory integration", branch: "feature/memory", filePath: "docs/memory.md" }],
      nodes: [],
    };
    (sharedState as any).openspec = {
      changes: [{ name: "memory-lifecycle-integration", stage: "verifying", tasksDone: 6, tasksTotal: 6 }],
    };
    (sharedState as any).lastMemoryInjection = {
      mode: "semantic",
      projectFactCount: 30,
      edgeCount: 0,
      workingMemoryFactCount: 4,
      semanticHitCount: 12,
      episodeCount: 3,
      globalFactCount: 15,
      payloadChars: 4800,
      estimatedTokens: 1200,
    };
  });

  it("keeps wide raised mode stacked instead of bleeding multiple sections across one row", () => {
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [{ label: "memory-core", status: "running" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.ok(lines.some((line) => line.includes("◈ Design Tree")));
    assert.ok(lines.some((line) => line.includes("◎ OpenSpec")));
    assert.ok(lines.every((line) => !line.includes(" │ ")));
    assert.ok(lines.every((line) => !(line.includes("◈ Design Tree") && line.includes("◎ OpenSpec"))), lines.join("\n"));
  });

  it("hides stale failed cleave state after it ages out", () => {
    (sharedState as any).cleave = {
      status: "failed",
      updatedAt: Date.now() - 60_000,
      children: [{ label: "memory-core", status: "failed" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.ok(lines.every((line) => !line.includes("⚡ Cleave")));
  });

  it("keeps memory audit compact enough for wide raised mode", () => {
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    const memoryLine = lines.find((line) => line.includes("Memory "));
    assert.ok(memoryLine);
    assert.ok(!memoryLine?.includes("chars:"));
    assert.ok(!memoryLine?.includes("hits:"));
  });

  it("truncates raised rows by dropping metadata before the primary label", () => {
    (sharedState as any).designTree = {
      nodeCount: 1,
      decidedCount: 0,
      exploringCount: 0,
      implementingCount: 1,
      implementedCount: 0,
      blockedCount: 0,
      openQuestionCount: 4,
      focusedNode: {
        id: "long-node",
        title: "I2P Integration With An Extremely Verbose Title That Must Stay Recognizable",
        status: "implementing",
        questions: ["one", "two", "three", "four"],
        branch: "feature/i2p-integration-with-a-very-very-long-branch-name",
        filePath: "docs/unified-dashboard.md",
      },
      implementingNodes: [],
      nodes: [],
    };
    (sharedState as any).openspec = {
      changes: [{
        name: "very-long-openspec-change-name-that-should-still-show-before-progress-metadata",
        stage: "implementing",
        tasksDone: 25,
        tasksTotal: 27,
        path: `${process.cwd()}/openspec/changes/dashboard-wide-truncation`,
      }],
    };
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(110);
    const designLine = lines.find((line) => line.includes("I2P Integration"));
    const specLine = lines.find((line) => line.includes("very-long-openspec-change-name"));
    assert.ok(designLine, lines.join("\n"));
    assert.ok(specLine, lines.join("\n"));
    assert.ok(designLine!.includes("⚙"), designLine);
    assert.ok(specLine!.includes("◦"), specLine);
  });
});

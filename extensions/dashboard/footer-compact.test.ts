import { beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";

import { DashboardFooter } from "./footer.ts";
import { sharedState } from "../lib/shared-state.ts";
import type { DashboardState } from "./types.ts";

function makeTheme() {
  return {
    fg: (_color: string, text: string) => text,
    bold: (text: string) => text,
  };
}

function makeFooterData(providerCount = 2) {
  return {
    getAvailableProviderCount: () => providerCount,
    getGitBranch: () => "main",
    getExtensionStatuses: () => new Map<string, string>(),
  };
}

function makeContext() {
  return {
    cwd: "/Users/cwilson/workspace/ai/omegon",
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

describe("DashboardFooter compact mode", () => {
  beforeEach(() => {
    (sharedState as any).designTree = {
      nodeCount: 4,
      decidedCount: 4,
      exploringCount: 0,
      implementingCount: 0,
      implementedCount: 15,
      blockedCount: 0,
      openQuestionCount: 0,
      focusedNode: null,
      implementingNodes: [],
    };
    (sharedState as any).openspec = {
      changes: [{ name: "x", stage: "implementing", tasksDone: 0, tasksTotal: 7 }],
    };
    (sharedState as any).cleave = { status: "idle", children: [] };
  });

  it("renders a single dashboard-only line in compact mode", () => {
    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "compact", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.equal(lines.length, 1);
  });

  it("shows provider-aware model info inline in wide compact mode", () => {
    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "compact", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.match(lines[0], /openai-codex\/gpt-5\.4/);
  });

  it("preserves primary dashboard summaries before truncating low-priority metadata", () => {
    (sharedState as any).designTree = {
      nodeCount: 4,
      decidedCount: 4,
      exploringCount: 0,
      implementingCount: 0,
      implementedCount: 15,
      blockedCount: 0,
      openQuestionCount: 0,
      focusedNode: {
        id: "very-long-node",
        title: "Extremely Long Focused Design Node Title That Should Not Displace Core Summaries",
        status: "decided",
        questions: [],
      },
      implementingNodes: [],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "compact", turns: 0 } satisfies DashboardState,
    );
    footer.setContext({
      ...makeContext(),
      model: {
        provider: "provider-with-a-very-long-name",
        id: "model-with-a-very-long-identifier-that-should-be-truncated-last",
        reasoning: true,
      },
    } as any);

    const [line] = footer.render(95);
    assert.ok(line.includes("◈"), line);
    assert.ok(line.includes("◎"), line);
    assert.ok(line.includes("⚡ idle"), line);
    assert.ok(!line.includes("provider-with-a-very-long-name"), line);
    assert.ok(!line.includes("model-with-a-very-long-identifier"), line);
  });
});

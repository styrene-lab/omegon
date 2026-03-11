import { beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";

import { DashboardFooter } from "./footer.ts";
import { sharedState } from "../shared-state.ts";
import type { DashboardState, RecoveryDashboardState } from "./types.ts";

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

function makeRecoveryState(overrides: Partial<RecoveryDashboardState> = {}): RecoveryDashboardState {
  return {
    provider: "anthropic",
    modelId: "claude-sonnet-4-5",
    classification: "rate_limit",
    summary: "Anthropic rate limited the last assistant turn.",
    action: "switch_offline",
    timestamp: Date.now(),
    retryCount: 0,
    maxRetries: 1,
    target: { provider: "local", modelId: "qwen3:30b", label: "Qwen3 30B" },
    cooldowns: [
      {
        scope: "provider",
        key: "anthropic",
        provider: "anthropic",
        until: Date.now() + 65_000,
        reason: "429",
      },
    ],
    ...overrides,
  };
}

describe("dashboard recovery state", () => {
  beforeEach(() => {
    (sharedState as any).designTree = null;
    (sharedState as any).openspec = null;
    (sharedState as any).cleave = { status: "idle", children: [] };
    (sharedState as any).recovery = undefined;
    delete (sharedState as any).lastMemoryInjection;
  });

  it("accepts recovery state and cooldown summaries on shared state", () => {
    (sharedState as any).recovery = makeRecoveryState();

    assert.equal((sharedState as any).recovery?.action, "switch_offline");
    assert.equal((sharedState as any).recovery?.target?.provider, "local");
    assert.equal((sharedState as any).recovery?.cooldowns?.[0]?.provider, "anthropic");
  });

  it("renders compact recovery summary with cooldown guidance", () => {
    (sharedState as any).recovery = makeRecoveryState();

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "compact", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const [line] = footer.render(160);
    // Compact mode shows only the terse action badge + provider/model, not the full error string.
    assert.match(line, /↺ offline/);
    assert.match(line, /anthropic\/claude-sonnet-4-5/);
    assert.match(line, /anthropic 1m/);
    assert.doesNotMatch(line, /Anthropic rate limited the last assistant turn\./);
  });

  it("renders raised recovery section with action, target, and cooldown", () => {
    (sharedState as any).recovery = makeRecoveryState({
      action: "retry",
      classification: "server_error",
      summary: "Retrying once after upstream server_error.",
      retryCount: 1,
      maxRetries: 1,
      target: { provider: "anthropic", modelId: "claude-sonnet-4-5" },
    });

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    // At width>=120 the wide 2-column layout places recovery in a ~79-char
    // left column — header + summary fit, but the long target annotation
    // ("→ anthropic/claude-sonnet-4-5") gets truncated. Use stacked width
    // (< 120) to assert the full detail line.
    const linesWide = footer.render(160);
    assert.ok(linesWide.some((line) => line.includes("↺ Recovery") && line.includes("retrying") && line.includes("server_error")), linesWide.join("\n"));
    assert.ok(linesWide.some((line) => line.includes("Retrying once after upstream server_error.")), linesWide.join("\n"));

    const linesStacked = footer.render(100);
    assert.ok(linesStacked.some((line) => line.includes("1/1 retries")), linesStacked.join("\n"));
    assert.ok(linesStacked.some((line) => line.includes("→ anthropic/claude-sonnet-4-5")), linesStacked.join("\n"));
  });

  it("omits recovery lines when no recovery state is present", () => {
    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.ok(lines.every((line) => !line.includes("↺ Recovery")), lines.join("\n"));
  });
});

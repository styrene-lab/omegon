import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  computeMemoryBudgetPolicy,
  createMemoryInjectionMetrics,
  estimateTokensFromChars,
  formatMemoryInjectionMetrics,
} from "./injection-metrics.ts";

describe("memory injection metrics", () => {
  it("derives estimated tokens from payload chars", () => {
    assert.equal(estimateTokensFromChars("12345678"), 2);
  });

  it("creates a complete metrics snapshot with defaults", () => {
    const metrics = createMemoryInjectionMetrics({
      mode: "bulk",
      projectFactCount: 12,
      edgeCount: 4,
      payloadChars: 400,
    });

    assert.deepEqual(metrics, {
      mode: "bulk",
      projectFactCount: 12,
      edgeCount: 4,
      workingMemoryFactCount: 0,
      semanticHitCount: 0,
      episodeCount: 0,
      globalFactCount: 0,
      payloadChars: 400,
      estimatedTokens: 100,
      baselineContextTokens: undefined,
      userPromptTokensEstimate: undefined,
      observedInputTokens: undefined,
      inferredAdditionalPromptTokens: undefined,
      estimatedVsObservedDelta: undefined,
    });
  });

  it("formats metrics for memory stats output", () => {
    const lines = formatMemoryInjectionMetrics({
      mode: "semantic",
      projectFactCount: 18,
      edgeCount: 0,
      workingMemoryFactCount: 3,
      semanticHitCount: 7,
      episodeCount: 2,
      globalFactCount: 5,
      payloadChars: 800,
      estimatedTokens: 200,
      baselineContextTokens: 5000,
      userPromptTokensEstimate: 25,
      observedInputTokens: 5300,
      inferredAdditionalPromptTokens: 275,
      estimatedVsObservedDelta: -75,
    });

    assert.deepEqual(lines, [
      "Last injection mode: semantic",
      "Last injection facts: 18",
      "Last injection edges: 0",
      "Last injection working-memory facts: 3",
      "Last injection semantic hits: 7",
      "Last injection episodes: 2",
      "Last injection global facts: 5",
      "Last injection payload: 800 chars",
      "Last injection estimate: 200 tokens",
      "Baseline context before injection: 5000",
      "User prompt estimate: 25 tokens",
      "Observed next input usage: 5300 tokens",
      "Inferred added prompt tokens: 275",
      "Estimate delta vs inferred: -75",
    ]);
  });

  it("formats null metrics for memory stats output", () => {
    const lines = formatMemoryInjectionMetrics(null);
    assert.deepEqual(lines, ["Last injection: none recorded this session"]);
  });

  it("formats undefined metrics for memory stats output", () => {
    const lines = formatMemoryInjectionMetrics(undefined);
    assert.deepEqual(lines, ["Last injection: none recorded this session"]);
  });

  it("computes a tighter low-signal budget policy", () => {
    const policy = computeMemoryBudgetPolicy({
      usedTokens: 12000,
      usedPercent: 20,
      userText: "help",
    });

    assert.deepEqual(policy, {
      maxChars: 8000,
      includeStructuralFill: false,
      includeGlobalFacts: false,
      includeEpisode: false,
    });
  });

  it("enables richer additions only for higher-signal turns", () => {
    const policy = computeMemoryBudgetPolicy({
      usedTokens: 6000,
      usedPercent: 20,
      userText: "Assess the current memory injection behavior against recent changes and compare it with cross-project patterns.",
    });

    assert.equal(policy.includeStructuralFill, true);
    assert.equal(policy.includeGlobalFacts, true);
    assert.equal(policy.includeEpisode, true);
  });
});

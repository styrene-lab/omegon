import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { sharedState } from "../lib/shared-state.ts";

describe("project memory shared state", () => {
  beforeEach(() => {
    (sharedState as any).memoryTokenEstimate = 0;
    delete (sharedState as any).lastMemoryInjection;
  });

  it("stores a structured last-memory-injection snapshot", () => {
    (sharedState as any).memoryTokenEstimate = 123;
    (sharedState as any).lastMemoryInjection = {
      mode: "bulk",
      projectFactCount: 10,
      edgeCount: 4,
      workingMemoryFactCount: 0,
      semanticHitCount: 0,
      episodeCount: 2,
      globalFactCount: 5,
      payloadChars: 492,
      estimatedTokens: 123,
    };

    assert.equal(sharedState.memoryTokenEstimate, 123);
    assert.equal(sharedState.lastMemoryInjection?.mode, "bulk");
    assert.equal(sharedState.lastMemoryInjection?.projectFactCount, 10);
    assert.equal(sharedState.lastMemoryInjection?.estimatedTokens, 123);
  });
});

import { describe, it } from "node:test";
import { strict as assert } from "node:assert";
import { createTriggerState, shouldExtract } from "./triggers.js";
import { DEFAULT_CONFIG } from "./types.js";

const config = { ...DEFAULT_CONFIG };

describe("createTriggerState", () => {
  it("returns expected defaults", () => {
    const s = createTriggerState();
    assert.equal(s.lastExtractedTokens, 0);
    assert.equal(s.toolCallsSinceExtract, 0);
    assert.equal(s.manualStoresSinceExtract, 0);
    assert.equal(s.isInitialized, false);
    assert.equal(s.isRunning, false);
  });
});

describe("shouldExtract", () => {
  it("returns false when isRunning", () => {
    const s = createTriggerState();
    s.isRunning = true;
    assert.equal(shouldExtract(s, 100_000, config), false);
  });

  describe("manual store suppression", () => {
    it("does NOT suppress before initialization (avoids bootstrap deadlock)", () => {
      const s = createTriggerState();
      s.manualStoresSinceExtract = config.manualStoreThreshold;
      // Has enough tokens for first extraction
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config), true);
    });

    it("suppresses after initialization when threshold met", () => {
      const s = createTriggerState();
      s.isInitialized = true;
      s.manualStoresSinceExtract = config.manualStoreThreshold;
      s.lastExtractedTokens = 10_000;
      s.toolCallsSinceExtract = config.toolCallsBetweenUpdates;
      assert.equal(shouldExtract(s, 10_000 + config.minimumTokensBetweenUpdate, config), false);
    });

    it("does not suppress when below threshold", () => {
      const s = createTriggerState();
      s.isInitialized = true;
      s.manualStoresSinceExtract = config.manualStoreThreshold - 1;
      s.lastExtractedTokens = 10_000;
      s.toolCallsSinceExtract = config.toolCallsBetweenUpdates;
      assert.equal(shouldExtract(s, 10_000 + config.minimumTokensBetweenUpdate, config), true);
    });
  });

  describe("first extraction (not initialized)", () => {
    it("returns false below minimumTokensToInit", () => {
      const s = createTriggerState();
      assert.equal(shouldExtract(s, config.minimumTokensToInit - 1, config), false);
    });

    it("returns true at exactly minimumTokensToInit", () => {
      const s = createTriggerState();
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config), true);
    });

    it("returns true above minimumTokensToInit", () => {
      const s = createTriggerState();
      assert.equal(shouldExtract(s, config.minimumTokensToInit + 1, config), true);
    });
  });

  describe("subsequent extractions (initialized)", () => {
    it("returns false when token delta is insufficient", () => {
      const s = createTriggerState();
      s.isInitialized = true;
      s.lastExtractedTokens = 10_000;
      s.toolCallsSinceExtract = config.toolCallsBetweenUpdates;
      assert.equal(shouldExtract(s, 10_000 + config.minimumTokensBetweenUpdate - 1, config), false);
    });

    it("returns false when tool calls are insufficient", () => {
      const s = createTriggerState();
      s.isInitialized = true;
      s.lastExtractedTokens = 10_000;
      s.toolCallsSinceExtract = config.toolCallsBetweenUpdates - 1;
      assert.equal(shouldExtract(s, 10_000 + config.minimumTokensBetweenUpdate, config), false);
    });

    it("returns true when both thresholds are met", () => {
      const s = createTriggerState();
      s.isInitialized = true;
      s.lastExtractedTokens = 10_000;
      s.toolCallsSinceExtract = config.toolCallsBetweenUpdates;
      assert.equal(shouldExtract(s, 10_000 + config.minimumTokensBetweenUpdate, config), true);
    });
  });

  describe("exponential backoff on failures", () => {
    it("no backoff with 0 failures (default)", () => {
      const s = createTriggerState();
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 0), true);
    });

    it("after 1 failure: skips odd-numbered opportunities", () => {
      const s = createTriggerState();
      // 1 failure → backoffSlots = 2 → only fires when toolCalls % 2 === 0
      s.toolCallsSinceExtract = 1;
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 1), false);
      s.toolCallsSinceExtract = 2;
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 1), true);
    });

    it("after 3 failures: skips most opportunities (backoff = 8)", () => {
      const s = createTriggerState();
      // 3 failures → backoffSlots = 8
      s.toolCallsSinceExtract = 3;
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 3), false);
      s.toolCallsSinceExtract = 8;
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 3), true);
    });

    it("caps backoff at 16", () => {
      const s = createTriggerState();
      // 10 failures → would be 1024, but capped at 16
      s.toolCallsSinceExtract = 16;
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 10), true);
      s.toolCallsSinceExtract = 15;
      assert.equal(shouldExtract(s, config.minimumTokensToInit, config, 10), false);
    });
  });
});

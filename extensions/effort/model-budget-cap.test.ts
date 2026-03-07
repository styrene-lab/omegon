/**
 * Tests for effort cap enforcement in model-budget.
 *
 * Validates that checkEffortCap() correctly blocks upgrades past the
 * effort ceiling while allowing downgrades and lateral switches.
 *
 * Spec: effort → model-budget respects effort cap on upgrades
 * Spec: effort → /effort cap locks the ceiling, agent can only downgrade
 */

import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { checkEffortCap, TIER_ORDER } from "../model-budget.ts";
import { sharedState } from "../shared-state.ts";

// ─── Helpers ──────────────────────────────────────────────────

/** Set effort cap state for testing. */
function setEffortCap(driver: string, name: string, level: number) {
  (sharedState as any).effort = {
    capped: true,
    driver,
    name,
    level,
  };
}

/** Clear effort state entirely. */
function clearEffort() {
  (sharedState as any).effort = undefined;
}

/** Set effort state with capped=false. */
function setEffortUncapped(driver: string, name: string, level: number) {
  (sharedState as any).effort = {
    capped: false,
    driver,
    name,
    level,
  };
}

// ─── Tests ────────────────────────────────────────────────────

describe("TIER_ORDER", () => {
  it("defines correct ordering: local < haiku < sonnet < opus", () => {
    assert.ok(TIER_ORDER.local < TIER_ORDER.haiku);
    assert.ok(TIER_ORDER.haiku < TIER_ORDER.sonnet);
    assert.ok(TIER_ORDER.sonnet < TIER_ORDER.opus);
  });
});

describe("checkEffortCap", () => {
  beforeEach(() => {
    clearEffort();
  });

  // Spec: No cap allows any switch
  describe("no cap active", () => {
    it("allows any switch when effort is undefined", () => {
      const result = checkEffortCap("opus");
      assert.equal(result.blocked, false);
      assert.equal(result.message, undefined);
    });

    it("allows any switch when effort exists but is not capped", () => {
      setEffortUncapped("sonnet", "Substantial", 3);
      const result = checkEffortCap("opus");
      assert.equal(result.blocked, false);
    });

    it("allows haiku when no cap", () => {
      const result = checkEffortCap("haiku");
      assert.equal(result.blocked, false);
    });

    it("allows sonnet when no cap", () => {
      const result = checkEffortCap("sonnet");
      assert.equal(result.blocked, false);
    });
  });

  // Spec: Cap blocks upgrade past ceiling
  describe("cap blocks upgrades", () => {
    it("blocks opus when capped at sonnet (Ruthless)", () => {
      setEffortCap("sonnet", "Ruthless", 4);
      const result = checkEffortCap("opus");
      assert.equal(result.blocked, true);
      assert.ok(result.message);
      assert.ok(result.message.includes("Ruthless"));
      assert.ok(result.message.includes("level 4"));
      assert.ok(result.message.includes("sonnet"));
      assert.ok(result.message.includes("opus"));
    });

    it("blocks opus when capped at haiku", () => {
      setEffortCap("local", "Low", 1);
      const result = checkEffortCap("opus");
      assert.equal(result.blocked, true);
      assert.ok(result.message!.includes("Low"));
    });

    it("blocks sonnet when capped at local (Low)", () => {
      setEffortCap("local", "Low", 1);
      const result = checkEffortCap("sonnet");
      assert.equal(result.blocked, true);
    });

    it("blocks haiku when capped at local (Low)", () => {
      setEffortCap("local", "Low", 1);
      const result = checkEffortCap("haiku");
      assert.equal(result.blocked, true);
    });

    it("blocks opus when capped at Substantial (driver=sonnet)", () => {
      setEffortCap("sonnet", "Substantial", 3);
      const result = checkEffortCap("opus");
      assert.equal(result.blocked, true);
      assert.ok(result.message!.includes("Substantial"));
      assert.ok(result.message!.includes("/effort uncap"));
    });
  });

  // Spec: Cap allows downgrade
  describe("cap allows downgrades", () => {
    it("allows haiku when capped at sonnet (Ruthless)", () => {
      setEffortCap("sonnet", "Ruthless", 4);
      const result = checkEffortCap("haiku");
      assert.equal(result.blocked, false);
    });

    it("allows haiku when capped at opus (Omnissiah)", () => {
      setEffortCap("opus", "Omnissiah", 7);
      const result = checkEffortCap("haiku");
      assert.equal(result.blocked, false);
    });

    it("allows sonnet when capped at opus", () => {
      setEffortCap("opus", "Absolute", 6);
      const result = checkEffortCap("sonnet");
      assert.equal(result.blocked, false);
    });
  });

  // Spec: Cap allows lateral switch (same tier)
  describe("cap allows lateral switches", () => {
    it("allows sonnet when capped at sonnet", () => {
      setEffortCap("sonnet", "Ruthless", 4);
      const result = checkEffortCap("sonnet");
      assert.equal(result.blocked, false);
    });

    it("allows opus when capped at opus", () => {
      setEffortCap("opus", "Omnissiah", 7);
      const result = checkEffortCap("opus");
      assert.equal(result.blocked, false);
    });
  });
});

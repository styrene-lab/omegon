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
import { sharedState } from "../lib/shared-state.ts";

// ─── Helpers ──────────────────────────────────────────────────

/** Set effort cap state for testing.
 *  capLevel determines the ceiling; driver reflects the CURRENT tier
 *  (which may differ if the operator switched tiers after capping).
 */
function setEffortCap(driver: string, name: string, level: number, capLevel?: number) {
  (sharedState as any).effort = {
    capped: true,
    driver,
    name,
    level,
    capLevel: capLevel ?? level,
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
  it("defines correct ordering: local < retribution < victory < gloriana", () => {
    assert.ok(TIER_ORDER.local < TIER_ORDER.retribution);
    assert.ok(TIER_ORDER.retribution < TIER_ORDER.victory);
    assert.ok(TIER_ORDER.victory < TIER_ORDER.gloriana);
  });
});

describe("checkEffortCap", () => {
  beforeEach(() => {
    clearEffort();
  });

  // Spec: No cap allows any switch
  describe("no cap active", () => {
    it("allows any switch when effort is undefined", () => {
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, false);
      assert.equal(result.message, undefined);
    });

    it("allows any switch when effort exists but is not capped", () => {
      setEffortUncapped("victory", "Substantial", 3);
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, false);
    });

    it("allows retribution when no cap", () => {
      const result = checkEffortCap("retribution");
      assert.equal(result.blocked, false);
    });

    it("allows victory when no cap", () => {
      const result = checkEffortCap("victory");
      assert.equal(result.blocked, false);
    });
  });

  // Spec: Cap blocks upgrade past ceiling
  describe("cap blocks upgrades", () => {
    it("blocks gloriana when capped at victory (Ruthless)", () => {
      setEffortCap("victory", "Ruthless", 4);
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, true);
      assert.ok(result.message);
      assert.ok(result.message.includes("Ruthless"));
      assert.ok(result.message.includes("level 4"));
      assert.ok(result.message.includes("victory"));
      assert.ok(result.message.includes("gloriana"));
    });

    it("blocks gloriana when capped at retribution", () => {
      setEffortCap("local", "Servitor", 1);
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, true);
      assert.ok(result.message!.includes("Servitor"));
    });

    it("blocks victory when capped at local (Servitor)", () => {
      setEffortCap("local", "Servitor", 1);
      const result = checkEffortCap("victory");
      assert.equal(result.blocked, true);
    });

    it("blocks retribution when capped at local (Servitor)", () => {
      setEffortCap("local", "Servitor", 1);
      const result = checkEffortCap("retribution");
      assert.equal(result.blocked, true);
    });

    it("blocks gloriana when capped at Substantial (driver=victory)", () => {
      setEffortCap("victory", "Substantial", 3);
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, true);
      assert.ok(result.message!.includes("Substantial"));
      assert.ok(result.message!.includes("/effort uncap"));
    });
  });

  // Spec: Cap allows downgrade
  describe("cap allows downgrades", () => {
    it("allows retribution when capped at victory (Ruthless)", () => {
      setEffortCap("victory", "Ruthless", 4);
      const result = checkEffortCap("retribution");
      assert.equal(result.blocked, false);
    });

    it("allows retribution when capped at gloriana (Omnissiah)", () => {
      setEffortCap("gloriana", "Omnissiah", 7);
      const result = checkEffortCap("retribution");
      assert.equal(result.blocked, false);
    });

    it("allows victory when capped at gloriana", () => {
      setEffortCap("gloriana", "Absolute", 6);
      const result = checkEffortCap("victory");
      assert.equal(result.blocked, false);
    });
  });

  // Spec: Cap survives tier switching (C1 regression)
  describe("cap derives ceiling from capLevel, not current driver", () => {
    it("blocks gloriana when capped at Ruthless even after switching to Omnissiah", () => {
      // Operator capped at Ruthless (level 4, driver=victory), then switched to Omnissiah
      // Current driver is now "gloriana", but capLevel is still 4 (victory ceiling)
      setEffortCap("gloriana", "Omnissiah", 7, 4);
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, true);
      assert.ok(result.message!.includes("Ruthless"));
      assert.ok(result.message!.includes("level 4"));
    });

    it("allows victory when capped at Ruthless even after switching to Omnissiah", () => {
      setEffortCap("gloriana", "Omnissiah", 7, 4);
      const result = checkEffortCap("victory");
      assert.equal(result.blocked, false);
    });

    it("blocks victory when capped at Servitor even after switching to Absolute", () => {
      setEffortCap("gloriana", "Absolute", 6, 1);
      const result = checkEffortCap("victory");
      assert.equal(result.blocked, true);
    });
  });

  // Spec: Cap allows lateral switch (same tier)
  describe("cap allows lateral switches", () => {
    it("allows victory when capped at victory", () => {
      setEffortCap("victory", "Ruthless", 4);
      const result = checkEffortCap("victory");
      assert.equal(result.blocked, false);
    });

    it("allows gloriana when capped at gloriana", () => {
      setEffortCap("gloriana", "Omnissiah", 7);
      const result = checkEffortCap("gloriana");
      assert.equal(result.blocked, false);
    });
  });
});

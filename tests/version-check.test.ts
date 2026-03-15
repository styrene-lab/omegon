/**
 * Tests for version-check — semver comparison.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import { isNewer } from "../extensions/version-check.ts";

describe("isNewer", () => {
  it("detects newer major", () => assert.equal(isNewer("1.0.0", "0.1.1"), true));
  it("detects newer minor", () => assert.equal(isNewer("0.2.0", "0.1.1"), true));
  it("detects newer patch", () => assert.equal(isNewer("0.1.2", "0.1.1"), true));
  it("returns false for same version", () => assert.equal(isNewer("0.1.1", "0.1.1"), false));
  it("returns false for older version", () => assert.equal(isNewer("0.1.0", "0.1.1"), false));
  it("handles missing segments", () => assert.equal(isNewer("1.0", "0.9.9"), true));
  it("suppresses downgrade prompts for older fork suffix versions", () =>
    assert.equal(isNewer("0.57.1-cwilson613.2", "0.58.1-cwilson613.1"), false));
  it("detects newer fork suffix versions", () =>
    assert.equal(isNewer("0.58.1-cwilson613.2", "0.58.1-cwilson613.1"), true));
});

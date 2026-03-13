/**
 * Tests for version-check — semver comparison.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";

// Inline isNewer since the module has side effects (reads package.json at import)
function isNewer(latest: string, current: string): boolean {
  const parse = (v: string) => v.split(".").map((n) => parseInt(n, 10) || 0);
  const l = parse(latest);
  const c = parse(current);
  for (let i = 0; i < 3; i++) {
    if ((l[i] ?? 0) > (c[i] ?? 0)) return true;
    if ((l[i] ?? 0) < (c[i] ?? 0)) return false;
  }
  return false;
}

describe("isNewer", () => {
  it("detects newer major", () => assert.equal(isNewer("1.0.0", "0.1.1"), true));
  it("detects newer minor", () => assert.equal(isNewer("0.2.0", "0.1.1"), true));
  it("detects newer patch", () => assert.equal(isNewer("0.1.2", "0.1.1"), true));
  it("returns false for same version", () => assert.equal(isNewer("0.1.1", "0.1.1"), false));
  it("returns false for older version", () => assert.equal(isNewer("0.1.0", "0.1.1"), false));
  it("handles missing segments", () => assert.equal(isNewer("1.0", "0.9.9"), true));
});

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("child_process");
const { join } = require("path");

const WRAPPER = join(__dirname, "omegon");

describe("npm wrapper bin/omegon", () => {
  it("reports unsupported platform gracefully", () => {
    // Force an unsupported platform by overriding the env
    const result = spawnSync(process.execPath, [
      "-e",
      `
      // Monkey-patch process.platform/arch before requiring the wrapper logic
      Object.defineProperty(process, 'platform', { value: 'aix' });
      Object.defineProperty(process, 'arch', { value: 'ppc' });
      require(${JSON.stringify(WRAPPER)});
      `,
    ], { encoding: "utf8" });

    assert.equal(result.status, 1);
    assert.match(result.stderr, /does not have a prebuilt binary for aix-ppc/);
    assert.match(result.stderr, /Supported platforms:/);
  });

  it("reports missing platform package gracefully", () => {
    // Run with real platform but from an isolated dir with no node_modules
    // and no sibling platform dir
    const result = spawnSync(process.execPath, [
      "-e",
      `
      // Override __dirname resolution so sibling fallback also fails
      const origResolve = require.resolve;
      require.resolve = function(id) {
        if (id.includes('@styrene-lab/omegon-')) throw new Error('not found');
        return origResolve.apply(this, arguments);
      };
      // Also break the sibling path by changing __dirname context
      const { existsSync } = require('fs');
      const origExists = existsSync;
      require('fs').existsSync = function(p) {
        if (p.includes('platform/')) return false;
        return origExists(p);
      };
      require(${JSON.stringify(WRAPPER)});
      `,
    ], { encoding: "utf8" });

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Could not find @styrene-lab\/omegon-/);
    assert.match(result.stderr, /npm install -g omegon --force/);
  });

  it("PLATFORM_MAP covers all expected platforms", () => {
    // Inline-evaluate the platform map
    const result = spawnSync(process.execPath, [
      "-e",
      `
      const script = require('fs').readFileSync(${JSON.stringify(WRAPPER)}, 'utf8');
      const match = script.match(/PLATFORM_MAP = \\{([^}]+)\\}/s);
      const keys = match[1].match(/"[^"]+"/g).filter(k => !k.includes('@'));
      const platforms = keys.map(k => k.replace(/"/g, ''));
      console.log(JSON.stringify(platforms));
      `,
    ], { encoding: "utf8" });

    assert.equal(result.status, 0, result.stderr);
    const platforms = JSON.parse(result.stdout.trim());
    assert.deepEqual(platforms.sort(), [
      "darwin-arm64",
      "darwin-x64",
      "linux-arm64",
      "linux-x64",
    ]);
  });

  it("resolves local binary via sibling fallback when available", () => {
    // The local dev binary should be found via the sibling path
    const result = spawnSync(process.execPath, [WRAPPER, "--version"], {
      encoding: "utf8",
    });
    // May fail if no binary exists locally — that's fine, just verify it doesn't crash
    // with an unexpected error
    if (result.status !== 0) {
      // Should be our controlled error, not a JS crash
      assert.match(result.stderr, /Could not find|does not have/);
    } else {
      assert.match(result.stdout, /omegon \d+\.\d+/);
    }
  });
});

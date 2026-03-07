import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  discoverGuardrails,
  parseSkillFrontmatter,
  formatGuardrailResults,
  evaluateCondition,
  runGuardrails,
  type GuardrailCheck,
  type GuardrailSuite,
} from "./guardrails.ts";

// ─── Test Helpers ────────────────────────────────────────────────────────────

function makeTmpDir(): string {
  const dir = join(tmpdir(), `guardrails-test-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

// ─── parseSkillFrontmatter ───────────────────────────────────────────────────

describe("parseSkillFrontmatter", () => {
  it("extracts guardrails array from frontmatter", () => {
    const content = `---
name: typescript
guardrails:
  - name: typecheck
    cmd: npx tsc --noEmit
    timeout: 60
  - name: lint
    cmd: npx eslint .
    condition: file_exists(.eslintrc.json)
---

# TypeScript Skill
`;
    const result = parseSkillFrontmatter(content);
    assert.ok(result.guardrails);
    assert.equal(result.guardrails.length, 2);
    assert.equal(result.guardrails[0]?.name, "typecheck");
    assert.equal(result.guardrails[0]?.cmd, "npx tsc --noEmit");
    assert.equal(result.guardrails[0]?.timeout, 60);
    assert.equal(result.guardrails[1]?.name, "lint");
    assert.equal(result.guardrails[1]?.condition, "file_exists(.eslintrc.json)");
  });

  it("returns empty for SKILL.md without guardrails", () => {
    const content = `---
name: python
description: Python development guidance
---

# Python Skill
`;
    const result = parseSkillFrontmatter(content);
    assert.equal(result.guardrails, undefined);
  });

  it("returns empty when no frontmatter present", () => {
    const content = "# Just a markdown file\n\nNo frontmatter here.";
    const result = parseSkillFrontmatter(content);
    assert.equal(result.guardrails, undefined);
  });

  it("handles frontmatter with unclosed delimiters", () => {
    const content = "---\nname: broken\nguardrails:\n  - name: test\n    cmd: echo hi\n";
    const result = parseSkillFrontmatter(content);
    assert.equal(result.guardrails, undefined);
  });
});

// ─── evaluateCondition ───────────────────────────────────────────────────────

describe("evaluateCondition", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = makeTmpDir();
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("file_exists returns true for existing file", () => {
    writeFileSync(join(tmpDir, "tsconfig.json"), "{}");
    assert.equal(evaluateCondition("file_exists(tsconfig.json)", tmpDir), true);
  });

  it("file_exists returns false for missing file", () => {
    assert.equal(evaluateCondition("file_exists(.eslintrc.json)", tmpDir), false);
  });

  it("unknown condition defaults to true", () => {
    assert.equal(evaluateCondition("something_weird()", tmpDir), true);
  });
});

// ─── discoverGuardrails ──────────────────────────────────────────────────────

describe("discoverGuardrails", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = makeTmpDir();
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("discovers typecheck from package.json scripts", () => {
    writeFileSync(
      join(tmpDir, "package.json"),
      JSON.stringify({ scripts: { typecheck: "tsc --noEmit" } })
    );
    const checks = discoverGuardrails(tmpDir);
    assert.equal(checks.length, 1);
    assert.equal(checks[0]?.name, "typecheck");
    assert.equal(checks[0]?.cmd, "tsc --noEmit");
    assert.equal(checks[0]?.source, "package-script");
  });

  it("discovers lint from package.json scripts", () => {
    writeFileSync(
      join(tmpDir, "package.json"),
      JSON.stringify({ scripts: { lint: "eslint ." } })
    );
    const checks = discoverGuardrails(tmpDir);
    assert.equal(checks.length, 1);
    assert.equal(checks[0]?.name, "lint");
  });

  it("auto-detects typecheck from tsconfig.json when no package-script", () => {
    writeFileSync(join(tmpDir, "tsconfig.json"), "{}");
    const checks = discoverGuardrails(tmpDir);
    assert.equal(checks.length, 1);
    assert.equal(checks[0]?.name, "typecheck");
    assert.equal(checks[0]?.cmd, "npx tsc --noEmit");
    assert.equal(checks[0]?.source, "auto-detect");
  });

  it("package-script typecheck wins over tsconfig auto-detect", () => {
    writeFileSync(
      join(tmpDir, "package.json"),
      JSON.stringify({ scripts: { typecheck: "npm run tsc" } })
    );
    writeFileSync(join(tmpDir, "tsconfig.json"), "{}");
    const checks = discoverGuardrails(tmpDir);
    const tc = checks.filter((c) => c.name === "typecheck");
    assert.equal(tc.length, 1);
    assert.equal(tc[0]?.source, "package-script");
    assert.equal(tc[0]?.cmd, "npm run tsc");
  });

  it("auto-detects mypy from pyproject.toml", () => {
    writeFileSync(join(tmpDir, "pyproject.toml"), "[project]\nname='test'");
    const checks = discoverGuardrails(tmpDir);
    assert.ok(checks.some((c) => c.name === "typecheck-python"));
  });

  it("auto-detects clippy from Cargo.toml", () => {
    writeFileSync(join(tmpDir, "Cargo.toml"), "[package]\nname='test'");
    const checks = discoverGuardrails(tmpDir);
    assert.ok(checks.some((c) => c.name === "clippy"));
  });

  it("reads skill frontmatter guardrails", () => {
    const skillPath = join(tmpDir, "SKILL.md");
    writeFileSync(
      skillPath,
      `---
guardrails:
  - name: custom-check
    cmd: echo ok
    timeout: 10
---
# Skill
`
    );
    const checks = discoverGuardrails(tmpDir, [skillPath]);
    assert.ok(checks.some((c) => c.name === "custom-check" && c.source === "skill-frontmatter"));
  });

  it("skips skill guardrails with failing condition", () => {
    const skillPath = join(tmpDir, "SKILL.md");
    writeFileSync(
      skillPath,
      `---
guardrails:
  - name: conditional-check
    cmd: echo ok
    condition: file_exists(nonexistent.yml)
---
# Skill
`
    );
    const checks = discoverGuardrails(tmpDir, [skillPath]);
    assert.ok(!checks.some((c) => c.name === "conditional-check"));
  });

  it("deduplicates: package-script wins over skill-frontmatter", () => {
    writeFileSync(
      join(tmpDir, "package.json"),
      JSON.stringify({ scripts: { typecheck: "tsc --noEmit" } })
    );
    const skillPath = join(tmpDir, "SKILL.md");
    writeFileSync(
      skillPath,
      `---
guardrails:
  - name: typecheck
    cmd: mypy .
---
# Skill
`
    );
    const checks = discoverGuardrails(tmpDir, [skillPath]);
    const tc = checks.filter((c) => c.name === "typecheck");
    assert.equal(tc.length, 1);
    assert.equal(tc[0]?.source, "package-script");
  });

  it("returns empty for directory with no indicators", () => {
    const checks = discoverGuardrails(tmpDir);
    assert.equal(checks.length, 0);
  });
});

// ─── runGuardrails ───────────────────────────────────────────────────────────

describe("runGuardrails", () => {
  it("runs passing check", () => {
    const checks: GuardrailCheck[] = [
      { name: "echo", cmd: "echo hello", timeout: 5, source: "auto-detect" },
    ];
    const suite = runGuardrails("/tmp", checks);
    assert.equal(suite.allPassed, true);
    assert.equal(suite.results.length, 1);
    assert.equal(suite.results[0]?.passed, true);
    assert.equal(suite.results[0]?.exitCode, 0);
    assert.ok(suite.results[0]?.output.includes("hello"));
  });

  it("runs failing check", () => {
    const checks: GuardrailCheck[] = [
      { name: "fail", cmd: "exit 1", timeout: 5, source: "auto-detect" },
    ];
    const suite = runGuardrails("/tmp", checks);
    assert.equal(suite.allPassed, false);
    assert.equal(suite.results[0]?.passed, false);
    assert.equal(suite.results[0]?.exitCode, 1);
  });

  it("captures mixed results", () => {
    const checks: GuardrailCheck[] = [
      { name: "pass", cmd: "echo ok", timeout: 5, source: "auto-detect" },
      { name: "fail", cmd: "exit 2", timeout: 5, source: "auto-detect" },
    ];
    const suite = runGuardrails("/tmp", checks);
    assert.equal(suite.allPassed, false);
    assert.equal(suite.results[0]?.passed, true);
    assert.equal(suite.results[1]?.passed, false);
    assert.equal(suite.results[1]?.exitCode, 2);
  });
});

// ─── formatGuardrailResults ──────────────────────────────────────────────────

describe("formatGuardrailResults", () => {
  it("formats all-passed suite", () => {
    const suite: GuardrailSuite = {
      allPassed: true,
      durationMs: 100,
      results: [
        {
          check: { name: "typecheck", cmd: "tsc", timeout: 30, source: "auto-detect" },
          passed: true,
          exitCode: 0,
          output: "",
          durationMs: 50,
        },
        {
          check: { name: "lint", cmd: "eslint", timeout: 30, source: "auto-detect" },
          passed: true,
          exitCode: 0,
          output: "",
          durationMs: 50,
        },
      ],
    };
    const result = formatGuardrailResults(suite);
    assert.ok(result.includes("✅"));
    assert.ok(result.includes("typecheck"));
    assert.ok(result.includes("lint"));
  });

  it("formats failure suite with output", () => {
    const suite: GuardrailSuite = {
      allPassed: false,
      durationMs: 200,
      results: [
        {
          check: { name: "typecheck", cmd: "tsc", timeout: 30, source: "auto-detect" },
          passed: false,
          exitCode: 2,
          output: "error TS2345: Argument of type...",
          durationMs: 100,
        },
        {
          check: { name: "lint", cmd: "eslint", timeout: 30, source: "auto-detect" },
          passed: true,
          exitCode: 0,
          output: "",
          durationMs: 100,
        },
      ],
    };
    const result = formatGuardrailResults(suite);
    assert.ok(result.includes("confirmed issues from compiler/linter output"));
    assert.ok(result.includes("❌ typecheck"));
    assert.ok(result.includes("exit code 2"));
    assert.ok(result.includes("error TS2345"));
    assert.ok(result.includes("✅ Passed: lint"));
  });
});

// ─── Output Capping ──────────────────────────────────────────────────────────

describe("output capping", () => {
  it("caps output at 50 lines", () => {
    // Generate 100 lines of output
    const lines = Array.from({ length: 100 }, (_, i) => `line ${i}`).join("\\n");
    const checks: GuardrailCheck[] = [
      { name: "verbose", cmd: `printf '${lines}'`, timeout: 5, source: "auto-detect" },
    ];
    const suite = runGuardrails("/tmp", checks);
    const output = suite.results[0]?.output ?? "";
    const outputLines = output.split("\n");
    // 50 content lines + 1 truncation message
    assert.ok(outputLines.length <= 52, `Expected ≤52 lines, got ${outputLines.length}`);
    assert.ok(output.includes("truncated"));
  });
});

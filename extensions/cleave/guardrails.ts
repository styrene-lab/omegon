/**
 * cleave/guardrails — Deterministic guardrail discovery, execution, and formatting.
 *
 * Discovers checks from package.json scripts, auto-detection of build tools,
 * and skill frontmatter. Runs them and formats results for injection into
 * review loops, post-merge checks, and /assess commands.
 */

import { execSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

// ─── Types ───────────────────────────────────────────────────────────────────

export interface GuardrailCheck {
  name: string;
  cmd: string;
  timeout: number;
  source: "package-script" | "skill-frontmatter" | "auto-detect";
}

export interface GuardrailResult {
  check: GuardrailCheck;
  passed: boolean;
  exitCode: number;
  output: string;
  durationMs: number;
}

export interface GuardrailSuite {
  results: GuardrailResult[];
  allPassed: boolean;
  durationMs: number;
}

export interface SkillGuardrailEntry {
  name: string;
  cmd: string;
  timeout?: number;
  condition?: string;
}

// ─── Frontmatter Parsing ─────────────────────────────────────────────────────

/**
 * Parse YAML frontmatter from a SKILL.md file, extracting the `guardrails:` array.
 * Uses simple line-based parsing — no yaml dependency.
 */
export function parseSkillFrontmatter(content: string): {
  guardrails?: SkillGuardrailEntry[];
} {
  const lines = content.split("\n");
  if (lines[0]?.trim() !== "---") return {};

  let endIdx = -1;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i]?.trim() === "---") {
      endIdx = i;
      break;
    }
  }
  if (endIdx < 0) return {};

  const fmLines = lines.slice(1, endIdx);

  // Find guardrails: key
  let guardrailStart = -1;
  for (let i = 0; i < fmLines.length; i++) {
    if (fmLines[i]?.match(/^guardrails:\s*$/)) {
      guardrailStart = i + 1;
      break;
    }
  }
  if (guardrailStart < 0) return {};

  const entries: SkillGuardrailEntry[] = [];
  let current: Partial<SkillGuardrailEntry> | null = null;

  for (let i = guardrailStart; i < fmLines.length; i++) {
    const line = fmLines[i] ?? "";
    // Stop if we hit a top-level key (no leading whitespace, has colon)
    if (line.match(/^\S/) && line.includes(":")) break;

    const itemMatch = line.match(/^\s*-\s+(\w+):\s*(.+)$/);
    if (itemMatch) {
      // New array item starting with first property
      if (current?.name && current?.cmd) {
        entries.push(current as SkillGuardrailEntry);
      }
      current = {};
      const key = itemMatch[1] as string;
      const val = itemMatch[2] as string;
      assignEntry(current, key, val);
      continue;
    }

    const propMatch = line.match(/^\s+(\w+):\s*(.+)$/);
    if (propMatch && current) {
      const key = propMatch[1] as string;
      const val = propMatch[2] as string;
      assignEntry(current, key, val);
    }
  }

  if (current?.name && current?.cmd) {
    entries.push(current as SkillGuardrailEntry);
  }

  return entries.length > 0 ? { guardrails: entries } : {};
}

function assignEntry(
  entry: Partial<SkillGuardrailEntry>,
  key: string,
  val: string
): void {
  switch (key) {
    case "name":
      entry.name = val.trim();
      break;
    case "cmd":
      entry.cmd = val.trim();
      break;
    case "timeout":
      entry.timeout = parseInt(val.trim(), 10) || undefined;
      break;
    case "condition":
      entry.condition = val.trim();
      break;
  }
}

// ─── Condition Evaluation ────────────────────────────────────────────────────

/**
 * Evaluate a condition string. Currently supports:
 * - `file_exists(path)` — checks if the file exists relative to cwd
 */
export function evaluateCondition(condition: string, cwd: string): boolean {
  const match = condition.match(/^file_exists\((.+)\)$/);
  if (match) {
    const filePath = match[1] as string;
    return existsSync(join(cwd, filePath));
  }
  // Unknown condition — default to true (include the check)
  return true;
}

// ─── Discovery ───────────────────────────────────────────────────────────────

/**
 * Discover guardrail checks from package.json, auto-detection, and skill frontmatter.
 * Priority: package-script > auto-detect > skill-frontmatter (dedup by name).
 */
export function discoverGuardrails(
  cwd: string,
  skillPaths?: string[]
): GuardrailCheck[] {
  const checks: GuardrailCheck[] = [];
  const seen = new Map<string, GuardrailCheck["source"]>();

  function addCheck(check: GuardrailCheck): void {
    const existing = seen.get(check.name);
    if (existing) {
      // Priority: package-script > auto-detect > skill-frontmatter
      const priority: Record<GuardrailCheck["source"], number> = {
        "package-script": 3,
        "auto-detect": 2,
        "skill-frontmatter": 1,
      };
      if (priority[check.source] <= priority[existing]) return;
      // Replace lower-priority entry
      const idx = checks.findIndex((c) => c.name === check.name);
      if (idx >= 0) checks[idx] = check;
      seen.set(check.name, check.source);
      return;
    }
    checks.push(check);
    seen.set(check.name, check.source);
  }

  // 1. package.json scripts
  const pkgPath = join(cwd, "package.json");
  if (existsSync(pkgPath)) {
    try {
      const pkg = JSON.parse(readFileSync(pkgPath, "utf-8")) as {
        scripts?: Record<string, string>;
      };
      const scripts = pkg.scripts ?? {};
      if (scripts.typecheck) {
        addCheck({
          name: "typecheck",
          cmd: scripts.typecheck,
          timeout: 30,
          source: "package-script",
        });
      }
      if (scripts.lint) {
        addCheck({
          name: "lint",
          cmd: scripts.lint,
          timeout: 30,
          source: "package-script",
        });
      }
    } catch {
      // Malformed package.json — skip
    }
  }

  // 2. Auto-detection
  if (!seen.has("typecheck") && existsSync(join(cwd, "tsconfig.json"))) {
    addCheck({
      name: "typecheck",
      cmd: "npx tsc --noEmit",
      timeout: 30,
      source: "auto-detect",
    });
  }
  if (existsSync(join(cwd, "pyproject.toml"))) {
    addCheck({
      name: "typecheck-python",
      cmd: "mypy .",
      timeout: 30,
      source: "auto-detect",
    });
  }
  if (existsSync(join(cwd, "Cargo.toml"))) {
    addCheck({
      name: "clippy",
      cmd: "cargo clippy -- -D warnings",
      timeout: 60,
      source: "auto-detect",
    });
  }

  // 3. Skill frontmatter
  if (skillPaths) {
    for (const sp of skillPaths) {
      if (!existsSync(sp)) continue;
      try {
        const content = readFileSync(sp, "utf-8");
        const fm = parseSkillFrontmatter(content);
        if (fm.guardrails) {
          for (const entry of fm.guardrails) {
            // Evaluate condition
            if (entry.condition && !evaluateCondition(entry.condition, cwd)) {
              continue;
            }
            addCheck({
              name: entry.name,
              cmd: entry.cmd,
              timeout: entry.timeout ?? 30,
              source: "skill-frontmatter",
            });
          }
        }
      } catch {
        // Unreadable skill file — skip
      }
    }
  }

  return checks;
}

// ─── Execution ───────────────────────────────────────────────────────────────

const MAX_OUTPUT_LINES = 50;

function capOutput(output: string): string {
  const lines = output.split("\n");
  if (lines.length <= MAX_OUTPUT_LINES) return output;
  return (
    lines.slice(0, MAX_OUTPUT_LINES).join("\n") +
    `\n... (${lines.length - MAX_OUTPUT_LINES} more lines truncated)`
  );
}

/**
 * Run all guardrail checks sequentially and collect results.
 */
export function runGuardrails(
  cwd: string,
  checks: GuardrailCheck[]
): GuardrailSuite {
  const suiteStart = Date.now();
  const results: GuardrailResult[] = [];

  for (const check of checks) {
    const start = Date.now();
    let passed = false;
    let exitCode = 1;
    let output = "";

    try {
      const result = execSync(check.cmd, {
        cwd,
        timeout: check.timeout * 1000,
        encoding: "utf-8",
        stdio: "pipe",
      });
      output = capOutput(result ?? "");
      passed = true;
      exitCode = 0;
    } catch (err: unknown) {
      const e = err as {
        status?: number;
        stdout?: string;
        stderr?: string;
        killed?: boolean;
        message?: string;
      };
      exitCode = e.status ?? 1;
      const parts: string[] = [];
      if (e.stdout) parts.push(e.stdout);
      if (e.stderr) parts.push(e.stderr);
      output = capOutput(
        parts.length > 0 ? parts.join("\n") : e.message ?? "Unknown error"
      );
      if (e.killed) {
        output = `[TIMEOUT after ${check.timeout}s]\n${output}`;
      }
    }

    results.push({
      check,
      passed,
      exitCode,
      output,
      durationMs: Date.now() - start,
    });
  }

  return {
    results,
    allPassed: results.every((r) => r.passed),
    durationMs: Date.now() - suiteStart,
  };
}

// ─── Formatting ──────────────────────────────────────────────────────────────

/**
 * Format guardrail results as markdown for injection into prompts / reports.
 */
export function formatGuardrailResults(suite: GuardrailSuite): string {
  if (suite.allPassed) {
    const names = suite.results.map((r) => r.check.name).join(", ");
    return `✅ All deterministic checks passed (${names})`;
  }

  const failures = suite.results.filter((r) => !r.passed);
  const lines: string[] = [
    "These are confirmed issues from compiler/linter output — not opinions:",
    "",
  ];

  for (const f of failures) {
    lines.push(`### ❌ ${f.check.name} (exit code ${f.exitCode})`);
    lines.push("");
    lines.push("```");
    lines.push(f.output);
    lines.push("```");
    lines.push("");
  }

  const passed = suite.results.filter((r) => r.passed);
  if (passed.length > 0) {
    lines.push(
      `✅ Passed: ${passed.map((r) => r.check.name).join(", ")}`
    );
  }

  return lines.join("\n");
}

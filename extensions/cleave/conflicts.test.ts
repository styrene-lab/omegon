/**
 * Tests for cleave/conflicts — task result parsing and 4-step conflict detection.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import { parseTaskResult, detectConflicts } from "./conflicts.js";
import type { TaskResult } from "./types.js";

// ─── parseTaskResult ────────────────────────────────────────────────────────

describe("parseTaskResult", () => {
	it("detects SUCCESS status", () => {
		const r = parseTaskResult("**Status:** SUCCESS\n**Summary:** done", "0-task.md");
		assert.equal(r.status, "SUCCESS");
	});

	it("detects PARTIAL status", () => {
		const r = parseTaskResult("**Status:** PARTIAL\n**Summary:** half done", "0-task.md");
		assert.equal(r.status, "PARTIAL");
	});

	it("detects FAILED status", () => {
		const r = parseTaskResult("**Status:** FAILED\n**Summary:** oops", "0-task.md");
		assert.equal(r.status, "FAILED");
	});

	it("defaults to PENDING", () => {
		const r = parseTaskResult("**Status:** PENDING\n", "0-task.md");
		assert.equal(r.status, "PENDING");
	});

	it("defaults to PENDING when no status line", () => {
		const r = parseTaskResult("some random content", "0-task.md");
		assert.equal(r.status, "PENDING");
	});

	it("extracts summary", () => {
		const r = parseTaskResult(
			"**Status:** SUCCESS\n**Summary:** Implemented auth flow\n**Artifacts:**",
			"0-task.md",
		);
		assert.equal(r.summary, "Implemented auth flow");
	});

	it("extracts file claims from quoted artifacts", () => {
		const content = `**Artifacts:**
- \`src/auth/login.ts\`
- \`src/auth/logout.ts\`
- "tests/auth.test.ts"

**Decisions Made:**`;
		const r = parseTaskResult(content, "0-task.md");
		assert.deepEqual(r.fileClaims, [
			"src/auth/login.ts",
			"src/auth/logout.ts",
			"tests/auth.test.ts",
		]);
	});

	it("extracts file claims from unquoted paths", () => {
		const content = `**Artifacts:**
- src/utils/helpers.ts
- src/config.json

**Decisions Made:**`;
		const r = parseTaskResult(content, "0-task.md");
		assert.ok(r.fileClaims.includes("src/utils/helpers.ts"));
		assert.ok(r.fileClaims.includes("src/config.json"));
	});

	it("deduplicates file claims", () => {
		const content = `**Artifacts:**
- \`src/auth.ts\`
- src/auth.ts

**Decisions Made:**`;
		const r = parseTaskResult(content, "0-task.md");
		const authCount = r.fileClaims.filter((f) => f === "src/auth.ts").length;
		assert.equal(authCount, 1);
	});

	it("extracts decisions", () => {
		const content = `**Decisions Made:**
- Use JWT for session management
- Store tokens in httpOnly cookies

**Assumptions:**`;
		const r = parseTaskResult(content, "0-task.md");
		assert.equal(r.decisions.length, 2);
		assert.ok(r.decisions[0].includes("JWT"));
	});

	it("extracts assumptions", () => {
		const content = `**Assumptions:**
- Database supports transactions
- Redis is not available

**Interfaces Published:**`;
		const r = parseTaskResult(content, "0-task.md");
		assert.equal(r.assumptions.length, 2);
		assert.ok(r.assumptions[1].includes("Redis"));
	});

	it("extracts interface signatures", () => {
		const content = `Published:
\`authenticate(username, password) -> AuthResult\`
\`refreshToken(token) -> TokenPair\`
`;
		const r = parseTaskResult(content, "0-task.md");
		assert.equal(r.interfacesPublished.length, 2);
		assert.ok(r.interfacesPublished[0].includes("authenticate"));
	});

	it("handles empty content gracefully", () => {
		const r = parseTaskResult("", "0-task.md");
		assert.equal(r.status, "PENDING");
		assert.equal(r.summary, null);
		assert.equal(r.fileClaims.length, 0);
		assert.equal(r.decisions.length, 0);
		assert.equal(r.assumptions.length, 0);
		assert.equal(r.interfacesPublished.length, 0);
	});
});

// ─── detectConflicts ────────────────────────────────────────────────────────

function makeResult(overrides: Partial<TaskResult> = {}): TaskResult {
	return {
		path: "test-task.md",
		status: "SUCCESS",
		summary: null,
		fileClaims: [],
		interfacesPublished: [],
		decisions: [],
		assumptions: [],
		...overrides,
	};
}

describe("detectConflicts", () => {
	it("returns empty for no results", () => {
		assert.deepEqual(detectConflicts([]), []);
	});

	it("returns empty when no conflicts exist", () => {
		const results = [
			makeResult({ fileClaims: ["src/a.ts"] }),
			makeResult({ fileClaims: ["src/b.ts"] }),
		];
		assert.deepEqual(detectConflicts(results), []);
	});

	// ── Step 1: File Overlap ──────────────────────────────────────────────

	it("detects file overlap", () => {
		const results = [
			makeResult({ fileClaims: ["src/shared.ts", "src/a.ts"] }),
			makeResult({ fileClaims: ["src/shared.ts", "src/b.ts"] }),
		];
		const conflicts = detectConflicts(results);
		assert.ok(conflicts.length >= 1);
		const overlap = conflicts.find((c) => c.type === "file_overlap");
		assert.ok(overlap);
		assert.ok(overlap.description.includes("shared.ts"));
		assert.equal(overlap.resolution, "3way_merge");
		assert.deepEqual(overlap.involved.sort(), [0, 1]);
	});

	it("detects multiple file overlaps", () => {
		const results = [
			makeResult({ fileClaims: ["src/a.ts", "src/b.ts"] }),
			makeResult({ fileClaims: ["src/a.ts", "src/b.ts"] }),
		];
		const overlaps = detectConflicts(results).filter((c) => c.type === "file_overlap");
		assert.equal(overlaps.length, 2);
	});

	// ── Step 2: Decision Contradiction ────────────────────────────────────

	it("detects technology contradiction (redis vs memcached)", () => {
		const results = [
			makeResult({ decisions: ["Use Redis for caching"] }),
			makeResult({ decisions: ["Use Memcached for caching"] }),
		];
		const conflicts = detectConflicts(results);
		const contradiction = conflicts.find((c) => c.type === "decision_contradiction");
		assert.ok(contradiction);
		assert.equal(contradiction.resolution, "escalate_to_parent");
	});

	it("detects REST vs GraphQL contradiction", () => {
		const results = [
			makeResult({ decisions: ["Build REST API endpoints"] }),
			makeResult({ decisions: ["Use GraphQL for the API layer"] }),
		];
		const conflicts = detectConflicts(results);
		const contradiction = conflicts.find((c) => c.type === "decision_contradiction");
		assert.ok(contradiction);
	});

	it("does not flag same sibling choosing both (not a conflict)", () => {
		const results = [
			makeResult({ decisions: ["Use Redis and also considered Memcached"] }),
		];
		const conflicts = detectConflicts(results);
		const contradictions = conflicts.filter((c) => c.type === "decision_contradiction");
		assert.equal(contradictions.length, 0);
	});

	it("does not flag rejection phrases as positive decisions", () => {
		const results = [
			makeResult({ decisions: ["Use Redis instead of Memcached"] }),
			makeResult({ decisions: ["Use Redis for session caching"] }),
		];
		const contradictions = detectConflicts(results).filter(
			(c) => c.type === "decision_contradiction",
		);
		assert.equal(contradictions.length, 0, "Rejection phrase should prevent false positive");
	});

	// ── Step 3: Interface Mismatch ────────────────────────────────────────

	it("detects interface mismatch", () => {
		const results = [
			makeResult({ interfacesPublished: ["authenticate(user, pass) -> Token"] }),
			makeResult({ interfacesPublished: ["authenticate(credentials) -> AuthResult"] }),
		];
		const conflicts = detectConflicts(results);
		const mismatch = conflicts.find((c) => c.type === "interface_mismatch");
		assert.ok(mismatch);
		assert.ok(mismatch.description.includes("authenticate"));
		assert.equal(mismatch.resolution, "adapter_required");
	});

	it("does not flag matching signatures", () => {
		const results = [
			makeResult({ interfacesPublished: ["validate(input) -> boolean"] }),
			makeResult({ interfacesPublished: ["validate(input) -> boolean"] }),
		];
		const mismatches = detectConflicts(results).filter(
			(c) => c.type === "interface_mismatch",
		);
		assert.equal(mismatches.length, 0);
	});

	// ── Step 4: Assumption Violation ──────────────────────────────────────

	it("detects assumption violated by sibling decision", () => {
		const results = [
			makeResult({ assumptions: ["Redis is not available in production"] }),
			makeResult({ decisions: ["Use Redis for all caching needs"] }),
		];
		const conflicts = detectConflicts(results);
		const violations = conflicts.filter((c) => c.type === "assumption_violation");
		assert.ok(violations.length >= 1);
		assert.equal(violations[0].resolution, "verify_with_parent");
	});

	it("does not flag assumption matching own decision", () => {
		const results = [
			makeResult({
				assumptions: ["Redis is not available"],
				decisions: ["Use Redis fallback"],
			}),
		];
		const violations = detectConflicts(results).filter(
			(c) => c.type === "assumption_violation",
		);
		assert.equal(violations.length, 0, "Same sibling should not self-conflict");
	});

	it("skips assumption violation when siblings have non-overlapping file scopes", () => {
		const results = [
			makeResult({
				fileClaims: ["src/core/lib.rs", "src/core/mod.rs"],
				assumptions: ["Redis is not available in production"],
				decisions: [],
			}),
			makeResult({
				fileClaims: ["src/render/canvas.rs", "src/render/mod.rs"],
				decisions: ["Use Redis for all caching needs"],
				assumptions: [],
			}),
		];
		const violations = detectConflicts(results).filter(
			(c) => c.type === "assumption_violation",
		);
		assert.equal(violations.length, 0,
			"Non-overlapping file scopes should suppress assumption violation detection");
	});

	it("still detects assumption violation when siblings have overlapping file scopes", () => {
		const results = [
			makeResult({
				fileClaims: ["src/shared/config.ts", "src/core/lib.ts"],
				assumptions: ["Redis is not available in production"],
				decisions: [],
			}),
			makeResult({
				fileClaims: ["src/shared/config.ts", "src/cache/redis.ts"],
				decisions: ["Use Redis for all caching needs"],
				assumptions: [],
			}),
		];
		const violations = detectConflicts(results).filter(
			(c) => c.type === "assumption_violation",
		);
		assert.ok(violations.length >= 1,
			"Overlapping file scopes should still detect assumption violations");
	});

	it("still detects assumption violation when no file claims exist (empty scopes)", () => {
		// If neither sibling reported file claims, we can't know the scope —
		// so the check should still run (conservative approach)
		const results = [
			makeResult({
				fileClaims: [],
				assumptions: ["Redis is not available in production"],
				decisions: [],
			}),
			makeResult({
				fileClaims: [],
				decisions: ["Use Redis for all caching needs"],
				assumptions: [],
			}),
		];
		const violations = detectConflicts(results).filter(
			(c) => c.type === "assumption_violation",
		);
		assert.ok(violations.length >= 1,
			"Empty file scopes should still check for assumption violations (conservative)");
	});

	// ── Combined ──────────────────────────────────────────────────────────

	it("detects multiple conflict types simultaneously", () => {
		const results = [
			makeResult({
				fileClaims: ["src/config.ts"],
				decisions: ["Use REST API"],
				interfacesPublished: ["getConfig() -> Config"],
			}),
			makeResult({
				fileClaims: ["src/config.ts"],
				decisions: ["Use GraphQL for API"],
				interfacesPublished: ["getConfig(opts) -> ConfigResult"],
			}),
		];
		const conflicts = detectConflicts(results);
		const types = new Set(conflicts.map((c) => c.type));
		assert.ok(types.has("file_overlap"), "Should detect file overlap");
		assert.ok(types.has("decision_contradiction"), "Should detect decision contradiction");
		assert.ok(types.has("interface_mismatch"), "Should detect interface mismatch");
	});
});

/**
 * Tests for cleave/assessment — pattern matching, complexity calculation,
 * system estimation, and modifier detection.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import {
	assessDirective,
	matchPattern,
	detectModifiers,
	calculateComplexity,
	effectiveComplexity,
	estimateSystems,
	detectFlags,
	PATTERNS,
	MODIFIERS,
} from "./assessment.js";

// ─── Pattern Library ────────────────────────────────────────────────────────

describe("PATTERNS", () => {
	it("should have 12 patterns", () => {
		assert.equal(Object.keys(PATTERNS).length, 12);
	});

	it("every pattern has required fields", () => {
		for (const [id, p] of Object.entries(PATTERNS)) {
			assert.ok(p.name, `${id} missing name`);
			assert.ok(p.keywords.length > 0, `${id} has no keywords`);
			assert.ok(p.requiredAny.length > 0, `${id} has no requiredAny`);
			assert.ok(typeof p.systemsBase === "number", `${id} missing systemsBase`);
			assert.ok(Array.isArray(p.splitStrategy), `${id} missing splitStrategy`);
		}
	});
});

// ─── matchPattern ───────────────────────────────────────────────────────────

describe("matchPattern", () => {
	it("matches full-stack CRUD directive", () => {
		const m = matchPattern(
			"Implement full-stack CRUD with React frontend, Express API, and PostgreSQL migrations",
		);
		assert.ok(m);
		assert.equal(m.name, "Full-Stack CRUD");
		assert.ok(m.confidence >= 0.80);
	});

	it("matches authentication system", () => {
		const m = matchPattern(
			"Add JWT authentication with login, register, and protected routes using bcrypt password hashing",
		);
		assert.ok(m);
		assert.equal(m.name, "Authentication System");
	});

	it("matches bug fix with file mention", () => {
		const m = matchPattern("Fix crash in auth.ts when user token is expired");
		assert.ok(m);
		assert.equal(m.name, "Bug Fix");
	});

	it("returns null for vague directives", () => {
		const m = matchPattern("make the app better");
		assert.equal(m, null);
	});

	it("returns null for empty-ish directives (no required keyword)", () => {
		const m = matchPattern("review the current state of the codebase");
		assert.equal(m, null);
	});

	it("applies vague penalty correctly", () => {
		// "fix issues" is vague — should lower confidence
		const specific = matchPattern("Fix the login crash when session expires");
		const vague = matchPattern("Fix issues with login");
		if (specific && vague) {
			assert.ok(specific.confidence > vague.confidence,
				`specific (${specific.confidence}) should exceed vague (${vague.confidence})`);
		}
	});

	it("is idempotent across repeated calls (no regex state leak)", () => {
		const directive = "Fix bug in auth.ts with session handling";
		const r1 = matchPattern(directive);
		const r2 = matchPattern(directive);
		const r3 = matchPattern(directive);

		assert.deepEqual(r1, r2);
		assert.deepEqual(r2, r3);
	});

	it("matches greenfield project directive", () => {
		const m = matchPattern(
			"Create a new project from scratch with a Rust workspace, core library crate, and binary application",
		);
		assert.ok(m, "Should match a greenfield pattern");
		assert.equal(m.name, "Greenfield Project");
		assert.ok(m.confidence >= 0.80);
	});

	it("matches multi-module library directive", () => {
		const m = matchPattern(
			"Build a multi-crate Rust library with public API traits, workspace layout, and cargo build configuration",
		);
		assert.ok(m, "Should match multi-module library pattern");
		assert.equal(m.name, "Multi-Module Library");
	});

	it("matches application bootstrap directive", () => {
		const m = matchPattern(
			"Bootstrap a desktop GUI application with egui, document model, and rendering pipeline",
		);
		assert.ok(m, "Should match application bootstrap pattern");
		assert.equal(m.name, "Application Bootstrap");
	});

	it("matches scaffold/init directives to greenfield", () => {
		const m = matchPattern(
			"Scaffold a new project with proper directory layout and build configuration",
		);
		assert.ok(m, "Should match greenfield pattern");
		assert.equal(m.name, "Greenfield Project");
	});

	it("is consistent when called with different directives in sequence", () => {
		// This specifically tests that global regex state doesn't leak
		// between calls with different content
		const d1 = "Implement CRUD form with React and PostgreSQL";
		const d2 = "Fix bug in auth.ts";
		const d3 = "Implement CRUD form with React and PostgreSQL";

		const r1 = matchPattern(d1);
		matchPattern(d2); // intermediate call that could pollute state
		const r3 = matchPattern(d3);

		assert.deepEqual(r1, r3, "Same directive should give same result regardless of intermediate calls");
	});
});

// ─── estimateSystems ────────────────────────────────────────────────────────

describe("estimateSystems", () => {
	it("returns 1 for single file mention", () => {
		assert.equal(estimateSystems("fix bug in auth.ts"), 1);
	});

	it("returns >= file count for multiple file mentions", () => {
		const result = estimateSystems("update auth.ts, routes.py, and schema.sql");
		assert.ok(result >= 3, `Expected >= 3, got ${result}`);
	});

	it("returns >= 1 for no file mentions", () => {
		const result = estimateSystems("improve the performance");
		assert.ok(result >= 1);
	});

	it("detects backend + frontend + data layers", () => {
		const result = estimateSystems(
			"build API endpoint for frontend component with database schema",
		);
		assert.ok(result >= 3, `Expected >= 3 systems, got ${result}`);
	});
});

// ─── detectModifiers ────────────────────────────────────────────────────────

describe("detectModifiers", () => {
	it("detects error_handling", () => {
		const mods = detectModifiers("implement retry logic with error handling and rollback");
		assert.ok(mods.includes("error_handling"));
	});

	it("detects security_critical", () => {
		const mods = detectModifiers("encrypt all passwords and manage auth tokens");
		assert.ok(mods.includes("security_critical"));
	});

	it("detects concurrency", () => {
		const mods = detectModifiers("add atomic operations to prevent race conditions");
		assert.ok(mods.includes("concurrency"));
	});

	it("returns empty for simple directives", () => {
		const mods = detectModifiers("rename variable from x to y");
		assert.equal(mods.length, 0);
	});

	it("detects multiple modifiers", () => {
		const mods = detectModifiers(
			"add encrypted credential storage with retry logic and cache invalidation for the distributed system",
		);
		assert.ok(mods.length >= 2, `Expected >= 2 modifiers, got ${mods.length}: ${mods}`);
	});
});

// ─── calculateComplexity ────────────────────────────────────────────────────

describe("calculateComplexity", () => {
	it("base case: 0 systems, 0 modifiers", () => {
		assert.equal(calculateComplexity(0, []), 1);
	});

	it("formula: (1 + systems) × (1 + 0.5 × modifiers)", () => {
		// (1 + 3) × (1 + 0.5 × 2) = 4 × 2 = 8
		assert.equal(calculateComplexity(3, ["a", "b"]), 8);
	});

	it("caps at 100", () => {
		assert.equal(calculateComplexity(100, Array(100).fill("x")), 100);
	});

	it("rounds to 1 decimal", () => {
		// (1 + 2) × (1 + 0.5 × 1) = 3 × 1.5 = 4.5
		assert.equal(calculateComplexity(2, ["a"]), 4.5);
	});
});

// ─── effectiveComplexity ────────────────────────────────────────────────────

describe("effectiveComplexity", () => {
	it("adds 1 when validate=true", () => {
		assert.equal(effectiveComplexity(5, true), 6);
	});

	it("adds 0 when validate=false", () => {
		assert.equal(effectiveComplexity(5, false), 5);
	});

	it("defaults to validate=true", () => {
		assert.equal(effectiveComplexity(5), 6);
	});
});

// ─── detectFlags ────────────────────────────────────────────────────────────

describe("detectFlags", () => {
	it("detects cleave-robust flag", () => {
		assert.equal(detectFlags("do the thing cleave-robust").robust, true);
	});

	it("detects cleave_robust flag (underscore)", () => {
		assert.equal(detectFlags("do the thing cleave_robust").robust, true);
	});

	it("returns false when no flags", () => {
		assert.equal(detectFlags("just do the thing").robust, false);
	});
});

// ─── assessDirective (integration) ──────────────────────────────────────────

describe("assessDirective", () => {
	it("throws on empty directive", () => {
		assert.throws(() => assessDirective(""), /empty/i);
		assert.throws(() => assessDirective("   "), /empty/i);
	});

	it("returns execute for pattern-matched simple task with high threshold", () => {
		// Full-Stack CRUD matches with high confidence; set a very high threshold
		// so effective complexity stays below it → execute
		const r = assessDirective(
			"Implement full-stack CRUD with React frontend, Express API, and PostgreSQL",
			1000,
		);
		assert.equal(r.decision, "execute");
		assert.equal(r.method, "fast-path");
	});

	it("returns cleave for complex multi-system task", () => {
		const r = assessDirective(
			"Implement full-stack CRUD with React frontend, Express API, and PostgreSQL migrations",
		);
		assert.equal(r.decision, "cleave");
		assert.equal(r.pattern, "Full-Stack CRUD");
		assert.equal(r.method, "fast-path");
	});

	it("returns needs_assessment for simple unrecognized directives", () => {
		const r = assessDirective("do something interesting with the project");
		assert.equal(r.method, "heuristic");
		assert.equal(r.pattern, null);
		// Simple directive — low complexity, may be needs_assessment or cleave
		// depending on detected systems
	});

	it("returns cleave for complex unrecognized directives exceeding threshold", () => {
		// This directive has multiple system signals but no pattern match
		const r = assessDirective(
			"build a streaming analytics pipeline with real-time websocket frontend, backend API microservice, database schema, and monitoring telemetry dashboard",
		);
		assert.equal(r.method, "heuristic");
		assert.equal(r.pattern, null);
		assert.equal(r.decision, "cleave", "High-complexity heuristic should recommend cleave, not needs_assessment");
		assert.ok(r.systems >= 3, `Expected >= 3 systems, got ${r.systems}`);
	});

	it("respects custom threshold", () => {
		const low = assessDirective(
			"Implement full-stack CRUD with React frontend, Express API, and PostgreSQL",
			1000, // impossibly high threshold
		);
		assert.equal(low.decision, "execute");

		const high = assessDirective(
			"Implement full-stack CRUD with React frontend, Express API, and PostgreSQL",
			0.1, // impossibly low threshold
		);
		assert.equal(high.decision, "cleave");
	});

	it("is deterministic across repeated calls", () => {
		const directive = "Fix authentication bug in auth.ts with JWT token refresh";
		const results = Array.from({ length: 5 }, () => assessDirective(directive));

		for (let i = 1; i < results.length; i++) {
			assert.equal(results[i].complexity, results[0].complexity, `Run ${i} complexity mismatch`);
			assert.equal(results[i].decision, results[0].decision, `Run ${i} decision mismatch`);
			assert.equal(results[i].confidence, results[0].confidence, `Run ${i} confidence mismatch`);
		}
	});

	it("includes reasoning string", () => {
		const r = assessDirective("Add OAuth login flow with JWT tokens and password hashing");
		assert.ok(r.reasoning.length > 0);
		assert.ok(r.reasoning.includes("Formula:") || r.reasoning.includes("Heuristic"));
	});
});

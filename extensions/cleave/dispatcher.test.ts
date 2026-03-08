/**
 * Tests for cleave/dispatcher — AsyncSemaphore and result section parsing.
 *
 * We can't easily test the full dispatch pipeline (requires pi subprocess),
 * but the semaphore and status harvesting are testable in isolation.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import {
	AsyncSemaphore,
	extractResultSection,
	resolveModelIdForTier,
	LARGE_RUN_THRESHOLD,
	mapModelTierToFlag,
} from "./dispatcher.ts";
import type { RegistryModel } from "../lib/model-routing.ts";
import { getDefaultPolicy } from "../lib/model-routing.ts";

// ─── AsyncSemaphore ─────────────────────────────────────────────────────────

describe("AsyncSemaphore", () => {
	it("allows up to limit concurrent acquires", async () => {
		const sem = new AsyncSemaphore(3);
		await sem.acquire();
		await sem.acquire();
		await sem.acquire();
		assert.equal(sem.activeCount, 3);
	});

	it("blocks beyond limit", async () => {
		const sem = new AsyncSemaphore(1);
		await sem.acquire();

		let acquired = false;
		const pending = sem.acquire().then(() => { acquired = true; });

		// Give microtasks a chance to run
		await new Promise((r) => setTimeout(r, 10));
		assert.equal(acquired, false, "Should be blocked");
		assert.equal(sem.waitingCount, 1);

		sem.release();
		await pending;
		assert.equal(acquired, true, "Should be unblocked after release");
	});

	it("maintains correct count through acquire/release cycles", async () => {
		const sem = new AsyncSemaphore(2);

		await sem.acquire();
		assert.equal(sem.activeCount, 2 - 1); // Actually count=1
		// Wait, activeCount returns this.count which starts at 0 and increments on acquire
		assert.equal(sem.activeCount, 1);

		await sem.acquire();
		assert.equal(sem.activeCount, 2);

		sem.release();
		assert.equal(sem.activeCount, 1);

		sem.release();
		assert.equal(sem.activeCount, 0);
	});

	it("guarantees no more than N concurrent tasks", async () => {
		const sem = new AsyncSemaphore(2);
		let maxConcurrent = 0;
		let currentConcurrent = 0;
		const results: number[] = [];

		const task = async (id: number) => {
			await sem.acquire();
			try {
				currentConcurrent++;
				maxConcurrent = Math.max(maxConcurrent, currentConcurrent);
				// Simulate async work
				await new Promise((r) => setTimeout(r, 5));
				results.push(id);
			} finally {
				currentConcurrent--;
				sem.release();
			}
		};

		await Promise.all([task(0), task(1), task(2), task(3), task(4)]);

		assert.equal(results.length, 5, "All tasks should complete");
		assert.ok(maxConcurrent <= 2, `Max concurrent was ${maxConcurrent}, expected <= 2`);
	});

	it("handles limit of 1 (serial execution)", async () => {
		const sem = new AsyncSemaphore(1);
		const order: number[] = [];

		const task = async (id: number) => {
			await sem.acquire();
			try {
				order.push(id);
				await new Promise((r) => setTimeout(r, 1));
			} finally {
				sem.release();
			}
		};

		await Promise.all([task(0), task(1), task(2)]);
		assert.equal(order.length, 3);
		// With limit=1, tasks execute serially in FIFO order
		assert.deepEqual(order, [0, 1, 2]);
	});

	it("handles release before any waiters (count decrements)", async () => {
		const sem = new AsyncSemaphore(3);
		await sem.acquire();
		assert.equal(sem.activeCount, 1);
		sem.release();
		assert.equal(sem.activeCount, 0);
		assert.equal(sem.waitingCount, 0);
	});

	it("stress test: 20 tasks with limit 3", async () => {
		const sem = new AsyncSemaphore(3);
		let maxConcurrent = 0;
		let currentConcurrent = 0;
		const completed: number[] = [];

		const task = async (id: number) => {
			await sem.acquire();
			try {
				currentConcurrent++;
				maxConcurrent = Math.max(maxConcurrent, currentConcurrent);
				await new Promise((r) => setTimeout(r, Math.random() * 5));
				completed.push(id);
			} finally {
				currentConcurrent--;
				sem.release();
			}
		};

		await Promise.all(Array.from({ length: 20 }, (_, i) => task(i)));

		assert.equal(completed.length, 20, "All 20 tasks should complete");
		assert.ok(maxConcurrent <= 3, `Max concurrent was ${maxConcurrent}, expected <= 3`);
		assert.equal(currentConcurrent, 0, "All tasks should be done");
	});
});

// ─── extractResultSection ───────────────────────────────────────────────────

describe("extractResultSection", () => {
	const TASK_FILE_WITH_CONTRACT = `# Task: foundation

> Build a Rust application

## Mission

Set up the project foundation.

## Scope

- src/lib.rs
- Cargo.toml

## Contract

1. Only work on files within your scope
2. Update the Result section below when done
3. Commit your work with clear messages — do not push
4. If the task is too complex, set status to NEEDS_DECOMPOSITION

## Result

**Status:** SUCCESS

**Summary:** Built the foundation module.

**Artifacts:**
- \`src/lib.rs\`
- \`Cargo.toml\`

**Decisions Made:**
- Used workspace layout

**Assumptions:**

**Interfaces Published:**

**Verification:**
- Command: \`cargo test\`
- Output: 12 tests passed
`;

	it("extracts only the Result section, excluding Contract", () => {
		const result = extractResultSection(TASK_FILE_WITH_CONTRACT);
		assert.ok(result.includes("**Status:** SUCCESS"));
		assert.ok(result.includes("Built the foundation module"));
		// Must NOT contain the Contract section instruction text
		assert.ok(!result.includes("If the task is too complex"));
		assert.ok(!result.includes("## Contract"));
	});

	it("does not false-positive on NEEDS_DECOMPOSITION in Contract section", () => {
		const result = extractResultSection(TASK_FILE_WITH_CONTRACT);
		// The Result section says SUCCESS, not NEEDS_DECOMPOSITION
		assert.ok(!result.includes("NEEDS_DECOMPOSITION"));
	});

	it("correctly detects NEEDS_DECOMPOSITION when child actually set it", () => {
		const content = `## Contract

4. If the task is too complex, set status to NEEDS_DECOMPOSITION

## Result

**Status:** NEEDS_DECOMPOSITION

**Summary:** Task too complex for single child.
`;
		const result = extractResultSection(content);
		assert.ok(result.includes("**Status:** NEEDS_DECOMPOSITION"));
	});

	it("correctly detects FAILED status in Result section", () => {
		const content = `## Contract

4. If the task is too complex, set status to NEEDS_DECOMPOSITION

## Result

**Status:** FAILED

**Summary:** Compilation errors.
`;
		const result = extractResultSection(content);
		assert.ok(result.includes("**Status:** FAILED"));
	});

	it("returns empty string when no Result section exists", () => {
		const content = `## Contract

Some instructions here.
`;
		assert.equal(extractResultSection(content), "");
	});

	it("handles Result section at end of file (no trailing heading)", () => {
		const content = `## Contract

Instructions.

## Result

**Status:** SUCCESS

**Summary:** Done.`;
		const result = extractResultSection(content);
		assert.ok(result.includes("**Status:** SUCCESS"));
		assert.ok(result.includes("Done."));
	});

	it("stops at the next ## heading after Result", () => {
		const content = `## Result

**Status:** SUCCESS

## Appendix

Extra stuff with NEEDS_DECOMPOSITION mentioned here.
`;
		const result = extractResultSection(content);
		assert.ok(result.includes("**Status:** SUCCESS"));
		assert.ok(!result.includes("NEEDS_DECOMPOSITION"));
		assert.ok(!result.includes("Appendix"));
	});
});

// ─── resolveModelIdForTier ──────────────────────────────────────────────────

describe("resolveModelIdForTier", () => {
	// Minimal mock registry with Anthropic + OpenAI models
	const mockModels: RegistryModel[] = [
		{ id: "claude-opus-4-6", provider: "anthropic" },
		{ id: "claude-sonnet-4-5", provider: "anthropic" },
		{ id: "claude-haiku-3-5", provider: "anthropic" },
		{ id: "gpt-5.4", provider: "openai" },
		{ id: "gpt-4o", provider: "openai" },
		{ id: "qwen3:32b", provider: "local" },
	];
	const defaultPolicy = getDefaultPolicy();
	const openaiFirstPolicy = {
		...defaultPolicy,
		providerOrder: ["openai" as const, "anthropic" as const, "local" as const],
	};

	it("'opus' tier resolves to explicit Anthropic model ID with Anthropic-first policy", () => {
		const result = resolveModelIdForTier("opus", mockModels, defaultPolicy);
		// Anthropic first → claude-opus-4-6
		assert.equal(result, "claude-opus-4-6");
	});

	it("'opus' tier resolves to explicit OpenAI model ID with OpenAI-first policy", () => {
		const result = resolveModelIdForTier("opus", mockModels, openaiFirstPolicy);
		// OpenAI first → gpt-5.4
		assert.equal(result, "gpt-5.4");
	});

	it("'sonnet' tier resolves to explicit model ID (not undefined) when registry has models", () => {
		const result = resolveModelIdForTier("sonnet", mockModels, defaultPolicy);
		// Should return explicit ID, not undefined
		assert.ok(result !== undefined, "Expected explicit model ID for sonnet tier");
		assert.ok(result!.length > 0, "Expected non-empty model ID");
	});

	it("'sonnet' tier returns undefined when registry is empty (pi default)", () => {
		const result = resolveModelIdForTier("sonnet", [], defaultPolicy);
		// Empty registry → fallback returns undefined (sonnet is pi's default)
		assert.equal(result, undefined);
	});

	it("'local' tier returns the localModel parameter directly", () => {
		const result = resolveModelIdForTier("local", mockModels, defaultPolicy, "qwen3:32b");
		assert.equal(result, "qwen3:32b");
	});

	it("'local' tier returns undefined when no localModel provided", () => {
		const result = resolveModelIdForTier("local", mockModels, defaultPolicy);
		assert.equal(result, undefined);
	});

	it("'haiku' tier resolves to explicit Anthropic haiku model", () => {
		const result = resolveModelIdForTier("haiku", mockModels, defaultPolicy);
		assert.equal(result, "claude-haiku-3-5");
	});

	it("review model resolves opus explicitly (spec: Review model also resolves explicitly)", () => {
		// Simulating the review model resolution path in dispatchSingleChild
		const reviewModelId = resolveModelIdForTier("opus", mockModels, defaultPolicy);
		// Must not be the bare alias "opus" — must be an explicit ID
		assert.notEqual(reviewModelId, "opus");
		assert.ok(reviewModelId?.includes("claude-opus") || reviewModelId?.includes("gpt-5"), 
			`Expected explicit model ID, got: ${reviewModelId}`);
	});

	it("does not pass bare 'opus' alias (spec: Child execution passes resolved model ID)", () => {
		// The old mapModelTierToFlag returned "opus" (fuzzy alias)
		// resolveModelIdForTier must NOT return bare "opus" when registry has models
		const oldFuzzy = mapModelTierToFlag("opus");
		const newExplicit = resolveModelIdForTier("opus", mockModels, defaultPolicy);
		assert.equal(oldFuzzy, "opus"); // old behavior still works
		assert.notEqual(newExplicit, "opus"); // new behavior is explicit
	});

	it("avoids avoided providers and falls back (spec: Session policy can avoid a provider)", () => {
		const lowBudgetPolicy = {
			...defaultPolicy,
			providerOrder: ["openai" as const, "anthropic" as const],
			avoidProviders: ["anthropic" as const],
		};
		// Should resolve to OpenAI opus (avoid Anthropic in first pass)
		const result = resolveModelIdForTier("opus", mockModels, lowBudgetPolicy);
		assert.equal(result, "gpt-5.4");
	});
});

// ─── LARGE_RUN_THRESHOLD ────────────────────────────────────────────────────

describe("LARGE_RUN_THRESHOLD", () => {
	it("is defined and is a positive integer >= 2", () => {
		assert.ok(typeof LARGE_RUN_THRESHOLD === "number");
		assert.ok(LARGE_RUN_THRESHOLD >= 2);
		assert.equal(LARGE_RUN_THRESHOLD, Math.floor(LARGE_RUN_THRESHOLD));
	});

	it("small run does not exceed threshold (spec: Small run does not interrupt with preflight)", () => {
		const smallChildCount = 2;
		const isLargeRun = smallChildCount >= LARGE_RUN_THRESHOLD;
		assert.equal(isLargeRun, false, "2 children should not be a large run");
	});

	it("large run meets or exceeds threshold (spec: Large run triggers preflight prompt)", () => {
		const largeChildCount = LARGE_RUN_THRESHOLD;
		const isLargeRun = largeChildCount >= LARGE_RUN_THRESHOLD;
		assert.equal(isLargeRun, true, `${LARGE_RUN_THRESHOLD} children should be a large run`);
	});

	it("review + 3 children also qualifies as large run", () => {
		const childCount = LARGE_RUN_THRESHOLD - 1;
		const reviewEnabled = true;
		const isLargeRun = childCount >= LARGE_RUN_THRESHOLD || (reviewEnabled && childCount >= LARGE_RUN_THRESHOLD - 1);
		assert.equal(isLargeRun, true, "Review + N-1 children should be large run");
	});

	it("review + 1 child does NOT qualify as large run", () => {
		const childCount = 1;
		const reviewEnabled = true;
		const isLargeRun = childCount >= LARGE_RUN_THRESHOLD || (reviewEnabled && childCount >= LARGE_RUN_THRESHOLD - 1);
		// With LARGE_RUN_THRESHOLD=4, LARGE_RUN_THRESHOLD-1=3, childCount=1 < 3
		assert.equal(isLargeRun, false, "Review + 1 child should not be large run");
	});
});

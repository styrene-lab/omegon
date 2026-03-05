/**
 * Tests for cleave/dispatcher — AsyncSemaphore and result section parsing.
 *
 * We can't easily test the full dispatch pipeline (requires pi subprocess),
 * but the semaphore and status harvesting are testable in isolation.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import { AsyncSemaphore, extractResultSection } from "./dispatcher.js";

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

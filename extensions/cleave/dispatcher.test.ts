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
	emitCleaveChildProgress,
	extractResultSection,
	resolveModelIdForTier,
	LARGE_RUN_THRESHOLD,
	DEFAULT_CHILD_TIMEOUT_MS,
	IDLE_TIMEOUT_MS,
	dispatchChildren,
} from "./dispatcher.ts";
import type { RegistryModel, ProviderRoutingPolicy } from "../lib/model-routing.ts";
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

	it("'gloriana' tier resolves to explicit Anthropic model ID with Anthropic-first policy", () => {
		const result = resolveModelIdForTier("gloriana", mockModels, defaultPolicy);
		// Anthropic first → claude-opus-4-6
		assert.equal(result, "claude-opus-4-6");
	});

	it("'gloriana' tier resolves to explicit OpenAI model ID with OpenAI-first policy", () => {
		const result = resolveModelIdForTier("gloriana", mockModels, openaiFirstPolicy);
		// OpenAI first → gpt-5.4
		assert.equal(result, "gpt-5.4");
	});

	it("'victory' tier resolves to explicit model ID (not undefined) when registry has models", () => {
		const result = resolveModelIdForTier("victory", mockModels, defaultPolicy);
		// Should return explicit ID, not undefined
		assert.ok(result !== undefined, "Expected explicit model ID for victory tier");
		assert.ok(result!.length > 0, "Expected non-empty model ID");
	});

	it("'victory' tier returns undefined when registry is empty (pi default)", () => {
		const result = resolveModelIdForTier("victory", [], defaultPolicy);
		// Empty registry → fallback returns undefined (victory is pi's default)
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

	it("'retribution' tier resolves to explicit Anthropic retribution-class model", () => {
		const result = resolveModelIdForTier("retribution", mockModels, defaultPolicy);
		assert.equal(result, "claude-haiku-3-5");
	});

	it("review model resolves gloriana explicitly (spec: Review model also resolves explicitly)", () => {
		// Simulating the review model resolution path in dispatchSingleChild
		const reviewModelId = resolveModelIdForTier("gloriana", mockModels, defaultPolicy);
		// Must not be the bare alias "gloriana" — must be an explicit ID
		assert.notEqual(reviewModelId, "gloriana");
		assert.ok(reviewModelId?.includes("claude-opus") || reviewModelId?.includes("gpt-5"), 
			`Expected explicit model ID, got: ${reviewModelId}`);
	});

	it("does not pass bare 'gloriana' alias (spec: Child execution passes resolved model ID)", () => {
		// resolveModelIdForTier must NOT return bare tier alias — must be explicit model ID
		const result = resolveModelIdForTier("gloriana", mockModels, defaultPolicy);
		assert.notEqual(result, "gloriana", "Must not return bare alias 'gloriana'");
		assert.notEqual(result, "retribution", "Must not return bare alias 'retribution'");
		assert.ok(result && result.includes("-"), `Expected explicit model ID (with dash), got: ${result}`);
	});

	it("does not pass bare alias even for retribution tier (spec: Prefer explicit IDs)", () => {
		const result = resolveModelIdForTier("retribution", mockModels, defaultPolicy);
		assert.notEqual(result, "retribution", "Must not return bare alias 'retribution'");
		assert.ok(result && result.includes("-"), `Expected explicit model ID, got: ${result}`);
	});

	it("returns undefined (not bare alias) when registry is empty and tier is gloriana", () => {
		// When the registry is empty, we cannot get an explicit ID. Returning the bare
		// alias violates the spec — return undefined so no --model flag is passed.
		const result = resolveModelIdForTier("gloriana", [], defaultPolicy);
		assert.equal(result, undefined, "Empty registry: should return undefined, not bare alias");
	});

	it("returns undefined (not bare alias) when registry is empty and tier is retribution", () => {
		const result = resolveModelIdForTier("retribution", [], defaultPolicy);
		assert.equal(result, undefined, "Empty registry: should return undefined, not bare alias");
	});

	it("avoids avoided providers and falls back (spec: Session policy can avoid a provider)", () => {
		const lowBudgetPolicy = {
			...defaultPolicy,
			providerOrder: ["openai" as const, "anthropic" as const],
			avoidProviders: ["anthropic" as const],
		};
		// Should resolve to OpenAI gloriana (avoid Anthropic in first pass)
		const result = resolveModelIdForTier("gloriana", mockModels, lowBudgetPolicy);
		assert.equal(result, "gpt-5.4");
	});
});

describe("emitCleaveChildProgress", () => {
	it("updates shared cleave child progress and emits a dashboard event", () => {
		const previous = (sharedState as any).cleave;
		try {
			(sharedState as any).cleave = {
				status: "dispatching",
				runId: "test-run",
				updatedAt: 0,
				children: [
					{ label: "child-0", status: "pending" },
					{ label: "child-1", status: "pending" },
				],
			};
			const events: Array<{ channel: string; data: unknown }> = [];
			const pi = {
				events: {
					emit: (channel: string, data: unknown) => {
						events.push({ channel, data });
					},
				},
			} as any;

			emitCleaveChildProgress(pi, 1, { status: "running" });
			assert.equal((sharedState as any).cleave.children[1].status, "running");
			assert.ok((sharedState as any).cleave.updatedAt > 0);
			assert.equal(events.length, 1);
			assert.equal(events[0]?.channel, "dashboard:update");

			emitCleaveChildProgress(pi, 1, { status: "done", elapsed: 42 });
			assert.equal((sharedState as any).cleave.children[1].status, "done");
			assert.equal((sharedState as any).cleave.children[1].elapsed, 42);
			assert.equal(events.length, 2);
		} finally {
			(sharedState as any).cleave = previous;
		}
	});
});

// ─── ProviderRoutingPolicy structure (spec: Session policy stores provider order and flags) ──

describe("ProviderRoutingPolicy structure", () => {
	// Spec scenario: "Session policy stores provider order and flags"
	// Verifies that the correct field names exist and are honoured by getDefaultPolicy().
	// This catches phantom-field bugs (e.g. preferCheapCloud vs cheapCloudPreferredOverLocal).

	it("getDefaultPolicy returns providerOrder array (spec: provider order stored)", () => {
		const policy = getDefaultPolicy();
		assert.ok(Array.isArray(policy.providerOrder), "providerOrder must be an array");
		assert.ok(policy.providerOrder.length > 0, "providerOrder must be non-empty");
	});

	it("getDefaultPolicy returns cheapCloudPreferredOverLocal flag (spec: cheap-cloud-over-local flag stored)", () => {
		const policy = getDefaultPolicy();
		assert.ok(
			Object.prototype.hasOwnProperty.call(policy, "cheapCloudPreferredOverLocal"),
			"Policy must have cheapCloudPreferredOverLocal field (not preferCheapCloud)",
		);
		assert.equal(typeof policy.cheapCloudPreferredOverLocal, "boolean");
	});

	it("getDefaultPolicy returns requirePreflightForLargeRuns flag (spec: large-run preflight flag stored)", () => {
		const policy = getDefaultPolicy();
		assert.ok(
			Object.prototype.hasOwnProperty.call(policy, "requirePreflightForLargeRuns"),
			"Policy must have requirePreflightForLargeRuns field",
		);
		assert.equal(typeof policy.requirePreflightForLargeRuns, "boolean");
	});

	it("policy with cheapCloudPreferredOverLocal=true is structurally valid (no phantom fields)", () => {
		const policy: ProviderRoutingPolicy = {
			providerOrder: ["anthropic", "openai", "local"],
			cheapCloudPreferredOverLocal: true,
			requirePreflightForLargeRuns: false,
			avoidProviders: [],
		};
		// cheapCloudPreferredOverLocal must be the canonical field name — TypeScript
		// compile would fail if the field name were wrong (no 'any' cast here).
		assert.equal(policy.cheapCloudPreferredOverLocal, true);
		assert.deepEqual(policy.providerOrder, ["anthropic", "openai", "local"]);
	});

	it("policy avoidProviders field stores avoided provider list (spec: Session policy can avoid a provider)", () => {
		const policy: ProviderRoutingPolicy = {
			providerOrder: ["openai", "anthropic", "local"],
			cheapCloudPreferredOverLocal: false,
			requirePreflightForLargeRuns: false,
			avoidProviders: ["anthropic"],
		};
		assert.deepEqual(policy.avoidProviders, ["anthropic"]);
		// Future resolution should skip Anthropic — tested via resolveModelIdForTier
		// in the "avoids avoided providers" test above.
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

	it("review + 2 children does NOT qualify as large run (spec: boundary — below LARGE_RUN_THRESHOLD-1)", () => {
		// With LARGE_RUN_THRESHOLD=4, the secondary condition is: review && childCount >= 3.
		// So review + 2 children (childCount=2 < 3) must NOT trigger preflight.
		const childCount = 2;
		const reviewEnabled = true;
		const isLargeRun = childCount >= LARGE_RUN_THRESHOLD || (reviewEnabled && childCount >= LARGE_RUN_THRESHOLD - 1);
		// LARGE_RUN_THRESHOLD - 1 = 3; childCount 2 < 3 → not large
		assert.equal(isLargeRun, false, "Review + 2 children should not be large run (boundary: below LARGE_RUN_THRESHOLD-1)");
	});

	it("review + 1 child does NOT qualify as large run", () => {
		const childCount = 1;
		const reviewEnabled = true;
		const isLargeRun = childCount >= LARGE_RUN_THRESHOLD || (reviewEnabled && childCount >= LARGE_RUN_THRESHOLD - 1);
		// With LARGE_RUN_THRESHOLD=4, LARGE_RUN_THRESHOLD-1=3, childCount=1 < 3
		assert.equal(isLargeRun, false, "Review + 1 child should not be large run");
	});
});

// ─── dispatchChildren preflight integration ─────────────────────────────────
//
// These tests verify that dispatchChildren() *actually calls* pi.ui.input()
// when a large run triggers preflight, and *does not call it* for small runs.
// Pure arithmetic tests (above) only verify the formula — these tests catch
// cases where the preflight block is deleted or bypassed in dispatchChildren().
//
// Strategy: inject a mock pi with a trackable ui.input(), set routingPolicy
// on sharedState so requirePreflightForLargeRuns=true, and supply enough
// pre-failed children so dispatch waves complete immediately without spawning.
// ────────────────────────────────────────────────────────────────────────────

import { sharedState } from "../lib/shared-state.ts";
import type { CleaveState, ChildState, RpcProgressUpdate } from "./types.ts";

/** Build a minimal CleaveState with N pre-failed children for test isolation. */
function makeTestState(childCount: number): CleaveState {
	const children: ChildState[] = Array.from({ length: childCount }, (_, i) => ({
		childId: i,
		label: `child-${i}`,
		status: "failed" as const, // pre-failed: dispatchSingleChild skips immediately
		dependsOn: [],
		branch: `branch-${i}`,
		worktreePath: undefined,
		executeModel: "victory",
	}));
	return {
		runId: "test-run",
		phase: "dispatch",
		directive: "test directive",
		repoPath: "/tmp",
		baseBranch: "main",
		assessment: null,
		plan: null,
		children,
		workspacePath: "/tmp",
		totalDurationSec: 0,
		createdAt: new Date().toISOString(),
	};
}

/** Build a mock pi object that tracks ui.input() calls. */
function makeMockPi() {
	let inputCallCount = 0;
	const pi = {
		ui: {
			input: async (_prompt: string): Promise<string> => {
				inputCallCount++;
				return ""; // operator presses Enter → keep current
			},
		},
		exec: async (_cmd: string, args: string[]) => {
			if (args[0] === "status") return { code: 0, stdout: "", stderr: "" };
			return { code: 0, stdout: "", stderr: "" };
		},
		// Minimal stubs so dispatchSingleChild doesn't crash looking for pi internals
		modelRegistry: { getAll: () => [] },
	} as any;
	return { pi, getInputCallCount: () => inputCallCount };
}

describe("dispatchChildren preflight — integration (spec: Large/Small run triggers)", () => {
	// Save and restore routingPolicy around each test to avoid cross-test pollution
	let savedRoutingPolicy: unknown;
	const setup = () => {
		savedRoutingPolicy = (sharedState as any).routingPolicy;
	};
	const teardown = () => {
		(sharedState as any).routingPolicy = savedRoutingPolicy;
	};

	it("calls pi.ui.input() when large run + requirePreflightForLargeRuns=true (spec: Large run triggers preflight prompt)", async () => {
		setup();
		try {
			(sharedState as any).routingPolicy = {
				providerOrder: ["anthropic", "openai", "local"],
				cheapCloudPreferredOverLocal: false,
				requirePreflightForLargeRuns: true,
				avoidProviders: [],
			};

			const { pi, getInputCallCount } = makeMockPi();
			const state = makeTestState(LARGE_RUN_THRESHOLD); // exactly at threshold

			const messages: string[] = [];
			await dispatchChildren(pi, state, 4, 5000, undefined, undefined, (msg) => messages.push(msg));

			assert.ok(getInputCallCount() > 0, 
				`pi.ui.input() should have been called once for large run (${LARGE_RUN_THRESHOLD} children), but was called ${getInputCallCount()} times`);
			// Preflight progress message should have been emitted
			assert.ok(
				messages.some((m) => m.includes("Preflight") || m.includes("preflight") || m.includes("provider")),
				`Expected a preflight-related progress message, got: ${JSON.stringify(messages)}`,
			);
		} finally {
			teardown();
		}
	});

	it("does NOT call pi.ui.input() for small run (spec: Small run does not interrupt with preflight)", async () => {
		setup();
		try {
			(sharedState as any).routingPolicy = {
				providerOrder: ["anthropic", "openai", "local"],
				cheapCloudPreferredOverLocal: false,
				requirePreflightForLargeRuns: true,
				avoidProviders: [],
			};

			const { pi, getInputCallCount } = makeMockPi();
			const smallChildCount = LARGE_RUN_THRESHOLD - 2; // clearly below threshold
			const state = makeTestState(smallChildCount > 0 ? smallChildCount : 1);

			await dispatchChildren(pi, state, 4, 5000, undefined, undefined, undefined);

			assert.equal(getInputCallCount(), 0,
				`pi.ui.input() should NOT be called for small run (${state.children.length} children < threshold ${LARGE_RUN_THRESHOLD})`);
		} finally {
			teardown();
		}
	});

	it("does NOT call pi.ui.input() when requirePreflightForLargeRuns=false even for large run", async () => {
		setup();
		try {
			(sharedState as any).routingPolicy = {
				providerOrder: ["anthropic", "openai", "local"],
				cheapCloudPreferredOverLocal: false,
				requirePreflightForLargeRuns: false,  // preflight disabled
				avoidProviders: [],
			};

			const { pi, getInputCallCount } = makeMockPi();
			const state = makeTestState(LARGE_RUN_THRESHOLD + 2); // large run but preflight disabled

			await dispatchChildren(pi, state, 4, 5000, undefined, undefined, undefined);

			assert.equal(getInputCallCount(), 0,
				"pi.ui.input() should NOT be called when requirePreflightForLargeRuns=false");
		} finally {
			teardown();
		}
	});

	it("skips preflight gracefully when pi.ui.input is not a function (spec: C2 regression)", async () => {
		setup();
		try {
			(sharedState as any).routingPolicy = {
				providerOrder: ["anthropic"],
				cheapCloudPreferredOverLocal: false,
				requirePreflightForLargeRuns: true,
				avoidProviders: [],
			};

			// pi.ui exists but .input is not a function — the C2 bug scenario
			const mockPi = {
				ui: { /* no input */ },
				exec: async () => ({ code: 0, stdout: "", stderr: "" }),
				modelRegistry: { getAll: () => [] },
			} as any;
			const state = makeTestState(LARGE_RUN_THRESHOLD);
			const messages: string[] = [];

			// Should not throw; should emit a "skipped" progress message
			await assert.doesNotReject(
				dispatchChildren(mockPi, state, 4, 5000, undefined, undefined, (msg) => messages.push(msg)),
			);
			assert.ok(
				messages.some((m) => m.toLowerCase().includes("skipped")),
				`Expected a 'skipped' progress message when ui.input is absent, got: ${JSON.stringify(messages)}`,
			);
		} finally {
			teardown();
		}
	});
});

// ─── RPC mode: emitCleaveChildProgress with structured progress ─────────────

describe("emitCleaveChildProgress — RPC structured progress", () => {
	it("uses rpcProgress.summary as lastLine for dashboard backward compat", () => {
		const previous = (sharedState as any).cleave;
		try {
			(sharedState as any).cleave = {
				status: "dispatching",
				runId: "test-rpc",
				updatedAt: 0,
				children: [
					{ label: "child-0", status: "running" },
				],
			};
			const events: Array<{ channel: string; data: unknown }> = [];
			const pi = {
				events: {
					emit: (channel: string, data: unknown) => {
						events.push({ channel, data });
					},
				},
			} as any;

			const progress: RpcProgressUpdate = {
				kind: "tool",
				summary: "tool: read src/auth.ts",
				toolName: "read",
			};
			emitCleaveChildProgress(pi, 0, { rpcProgress: progress });

			const child = (sharedState as any).cleave.children[0];
			assert.equal(child.lastLine, "tool: read src/auth.ts");
			assert.ok(child.recentLines?.includes("tool: read src/auth.ts"));
			assert.equal(events.length, 1);
		} finally {
			(sharedState as any).cleave = previous;
		}
	});

	it("appends to recentLines ring buffer and caps at 30", () => {
		const previous = (sharedState as any).cleave;
		try {
			(sharedState as any).cleave = {
				status: "dispatching",
				runId: "test-rpc",
				updatedAt: 0,
				children: [
					{ label: "child-0", status: "running", recentLines: Array.from({ length: 29 }, (_, i) => `line-${i}`) },
				],
			};
			const pi = {
				events: { emit: () => {} },
			} as any;

			emitCleaveChildProgress(pi, 0, {
				rpcProgress: { kind: "tool", summary: "tool: bash npm test" },
			});
			const child = (sharedState as any).cleave.children[0];
			assert.equal(child.recentLines.length, 30);

			// Adding one more should trim to 30
			emitCleaveChildProgress(pi, 0, {
				rpcProgress: { kind: "lifecycle", summary: "Agent completed" },
			});
			assert.equal(child.recentLines.length, 30);
			assert.equal(child.recentLines[29], "Agent completed");
		} finally {
			(sharedState as any).cleave = previous;
		}
	});

	it("rpcProgress takes precedence over lastLine when both provided", () => {
		const previous = (sharedState as any).cleave;
		try {
			(sharedState as any).cleave = {
				status: "dispatching",
				runId: "test-rpc",
				updatedAt: 0,
				children: [
					{ label: "child-0", status: "running" },
				],
			};
			const pi = {
				events: { emit: () => {} },
			} as any;

			// rpcProgress should win over lastLine
			emitCleaveChildProgress(pi, 0, {
				rpcProgress: { kind: "tool", summary: "tool: edit file.ts" },
				lastLine: "should be ignored",
			});
			const child = (sharedState as any).cleave.children[0];
			assert.equal(child.lastLine, "tool: edit file.ts");
		} finally {
			(sharedState as any).cleave = previous;
		}
	});
});

// ─── Timeout constants ──────────────────────────────────────────────────────

describe("timeout constants", () => {
	it("DEFAULT_CHILD_TIMEOUT_MS is 15 minutes", () => {
		assert.equal(DEFAULT_CHILD_TIMEOUT_MS, 15 * 60 * 1000);
	});

	it("IDLE_TIMEOUT_MS is 3 minutes", () => {
		assert.equal(IDLE_TIMEOUT_MS, 3 * 60 * 1000);
	});

	it("idle timeout is shorter than wall-clock timeout", () => {
		assert.ok(IDLE_TIMEOUT_MS < DEFAULT_CHILD_TIMEOUT_MS,
			"idle timeout must be shorter than wall-clock timeout");
	});
});

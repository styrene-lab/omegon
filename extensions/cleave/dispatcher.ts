/**
 * cleave/dispatcher — Child process dispatch and monitoring.
 *
 * Spawns `pi` subprocesses for each child task, using the same
 * subagent pattern as pi's example extension. Each child runs in
 * its own git worktree with an isolated context.
 *
 * Supports two backends:
 * - "cloud": spawns a full `pi` process (uses cloud API)
 * - "local": spawns `pi` with --model pointing to a local Ollama model
 *
 * The dispatcher handles:
 * - Dependency-ordered wave execution
 * - Concurrency limiting
 * - Timeout enforcement
 * - Result harvesting from task files
 */

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import { DASHBOARD_UPDATE_EVENT, sharedState } from "../lib/shared-state.ts";
import type { ChildState, CleaveState, ModelTier } from "./types.ts";
import { computeDispatchWaves } from "./planner.ts";
import { executeWithReview, type ReviewConfig, type ReviewExecutor, DEFAULT_REVIEW_CONFIG } from "./review.ts";
import { saveState } from "./workspace.ts";
import { resolveTier, getDefaultPolicy, getViableModels, type ProviderRoutingPolicy, type RegistryModel } from "../lib/model-routing.ts";

// ─── Large-run threshold ────────────────────────────────────────────────────

/**
 * Number of children at or above which a run is considered "large".
 * When session policy requirePreflightForLargeRuns is true and this threshold
 * is exceeded, the operator is asked for their preferred provider before dispatch.
 *
 * Also triggers for runs with review enabled where children >= 3.
 */
export const LARGE_RUN_THRESHOLD = 4;

// ─── Explicit model resolution ──────────────────────────────────────────────

/**
 * Resolve an abstract tier to a concrete model ID string using the session
 * routing policy and the available model registry.
 *
 * This is the replacement for mapModelTierToFlag() — it produces explicit model
 * IDs rather than fuzzy aliases, satisfying the design decision:
 * "Prefer explicit model IDs over fuzzy tier aliases at execution time".
 *
 * @param tier        Abstract tier (local|retribution|victory|gloriana)
 * @param models      Snapshot of the pi model registry
 * @param policy      Session routing policy
 * @param localModel  Local model name (for "local" tier fallback)
 * @returns           Explicit model ID to pass to --model, or undefined
 */
export function resolveModelIdForTier(
	tier: ModelTier,
	models: RegistryModel[],
	policy: ProviderRoutingPolicy,
	localModel?: string,
): string | undefined {
	// "local" tier: use the provided local model name directly
	if (tier === "local") {
		return localModel;
	}

	// Use the shared resolver to get an explicit model ID
	const resolved = resolveTier(tier, models, policy);
	if (resolved) {
		return resolved.modelId;
	}

	// Fallback: if resolver found nothing (empty registry, no API keys),
	// return undefined so no --model flag is passed. Callers must NEVER pass
	// a bare tier alias — that violates the spec decision "Prefer explicit model IDs
	// over fuzzy tier aliases at execution time."
	return undefined;
}

export function emitCleaveChildProgress(
	pi: Pick<ExtensionAPI, "events">,
	childId: number,
	patch: { status?: "pending" | "running" | "done" | "failed"; elapsed?: number; startedAt?: number; lastLine?: string; worktreePath?: string },
): void {
	const cleaveState = (sharedState as any).cleave;
	if (!cleaveState?.children?.[childId]) return;
	if (patch.status !== undefined) {
		cleaveState.children[childId].status = patch.status;
	}
	if (patch.elapsed !== undefined) {
		cleaveState.children[childId].elapsed = patch.elapsed;
	}
	if (patch.startedAt !== undefined) {
		cleaveState.children[childId].startedAt = patch.startedAt;
	}
	if (patch.worktreePath !== undefined) {
		cleaveState.children[childId].worktreePath = patch.worktreePath;
	}
	if (patch.lastLine !== undefined) {
		// Update lastLine for backward compat
		cleaveState.children[childId].lastLine = patch.lastLine;
		// Append to ring buffer (cap at 30)
		const child = cleaveState.children[childId];
		if (!child.recentLines) child.recentLines = [];
		child.recentLines.push(patch.lastLine);
		if (child.recentLines.length > 30) child.recentLines.splice(0, child.recentLines.length - 30);
	}
	cleaveState.updatedAt = Date.now();
	pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "cleave", childId, patch });
}

// ─── Result section parsing ─────────────────────────────────────────────────

/**
 * Extract just the ## Result section from a task file.
 *
 * The Contract section contains instructional text like
 * "set status to NEEDS_DECOMPOSITION" which must NOT be matched
 * as an actual status. By isolating the Result section, we only
 * match status strings the child agent actually wrote.
 *
 * Returns the content from "## Result" to the next "##" heading or EOF.
 * Returns empty string if no Result section found.
 */
export function extractResultSection(content: string): string {
	const resultIdx = content.indexOf("## Result");
	if (resultIdx === -1) return "";
	const afterResult = content.slice(resultIdx);
	// Find the next ## heading after the Result heading itself
	const nextHeading = afterResult.indexOf("\n## ", 1);
	return nextHeading === -1 ? afterResult : afterResult.slice(0, nextHeading);
}

// ─── Model resolution ───────────────────────────────────────────────────────

/**
 * Scope-based autoclassification thresholds.
 *
 * Ground rule: once classified as "local", the child STAYS local.
 * If the local model fails, the task fails — it never silently escalates
 * to cloud. This prevents autoclassification from being a leaky abstraction
 * that degrades to cloud spend under pressure.
 */
const LOCAL_SCOPE_THRESHOLD = 3;      // ≤ this many files → local
const SONNET_SCOPE_THRESHOLD = 8;     // ≤ this many files → victory, > → gloriana

/**
 * Tier ordering for floor comparison. Higher number = higher tier.
 * Used by applyEffortFloor to determine "higher of the two".
 */
const TIER_ORDER: Record<ModelTier, number> = {
	local: 0,
	retribution: 1,
	victory: 2,
	gloriana: 3,
};

/**
 * Classify a child's execution tier based on scope analysis.
 *
 * Returns a tier suggestion or undefined if scope doesn't give a clear signal.
 * The caller decides whether to use this or defer to other resolution steps.
 */
export function classifyByScope(
	scope: string[],
): ModelTier | undefined {
	if (scope.length === 0) return undefined;

	// Count the unique non-test files in scope
	const nonTestFiles = scope.filter((f) => !f.endsWith(".test.ts") && !f.endsWith(".test.js") && !f.endsWith(".spec.ts") && !f.endsWith(".spec.js"));
	const effectiveSize = nonTestFiles.length;

	if (effectiveSize <= LOCAL_SCOPE_THRESHOLD) return "local";
	if (effectiveSize <= SONNET_SCOPE_THRESHOLD) return "victory";
	return "gloriana";
}

/**
 * Apply effort-tier floor to a classified model tier.
 *
 * Reads sharedState.effort (written by the effort extension) and:
 * 1. If effort is undefined → return classified unchanged (backward compat)
 * 2. If effort.cleavePreferLocal is true → force "local" (Low/Average tiers)
 * 3. Otherwise → return the higher of classified vs effort.cleaveFloor
 *
 * This is called at the end of resolveExecuteModel (after scope/skill
 * classification) to enforce the operator's global effort policy.
 * Explicit executeModel annotations bypass this — they are checked
 * before applyEffortFloor is reached.
 */
export function applyEffortFloor(classified: ModelTier): ModelTier {
	const effort = (sharedState as any).effort as
		| { cleavePreferLocal: boolean; cleaveFloor: ModelTier }
		| undefined;

	// (1) No effort state — backward compatible passthrough
	if (!effort) return classified;

	// (2) Effort forces all-local (Low/Average tiers)
	if (effort.cleavePreferLocal && classified !== "local") return "local";

	// (3) Floor enforcement — return the higher of classified vs floor
	const floor = effort.cleaveFloor;
	if (floor && TIER_ORDER[floor] > TIER_ORDER[classified]) return floor;

	return classified;
}

/**
 * Resolve the execution model tier for a child.
 *
 * Resolution order (first non-null wins):
 * 1. Explicit annotation — child.executeModel already set (from plan or task annotation)
 * 2. Scope-based autoclassification — ≤3 files → local, ≤8 → victory, >8 → gloriana
 *    (only when local model is available)
 * 3. Skill tier hint — highest preferredTier from matched skills
 * 4. Default — victory
 *
 * NO-FAIL-PAST RULE: Once a child is assigned "local" tier here, the dispatcher
 * will NOT escalate to cloud on failure. The child either succeeds locally or fails.
 * This is enforced structurally — dispatchSingleChild has no retry/escalation path.
 */
export function resolveExecuteModel(
	child: { scope?: string[]; skills?: string[]; executeModel?: ModelTier },
	preferLocal: boolean,
	localModelAvailable: boolean,
	getPreferredTierFn?: (skills: string[]) => ModelTier | undefined,
): ModelTier {
	// 1. Explicit annotation on the child plan — always respected
	//    Bypasses effort floor — explicit annotations are deliberate overrides.
	if (child.executeModel) return child.executeModel;

	// Effort-based preferLocal override: if the effort tier says cleavePreferLocal,
	// treat this dispatch as prefer-local regardless of the caller's flag.
	const effectivePreferLocal = preferLocal || !!(sharedState as any).effort?.cleavePreferLocal;

	let classified: ModelTier | undefined;

	// 2. Scope-based autoclassification (when local is available)
	if (localModelAvailable && child.scope && child.scope.length > 0) {
		const scopeTier = classifyByScope(child.scope);
		if (scopeTier) {
			// preferLocal mode: cap at local (never auto-classify UP to cloud)
			classified = (effectivePreferLocal && scopeTier !== "local") ? "local" : scopeTier;
		}
	}

	// 2b. Global prefer_local flag (no scope info but local requested)
	if (!classified && effectivePreferLocal && localModelAvailable) {
		classified = "local";
	}

	// 3. Skill-based tier hint
	if (!classified && child.skills && child.skills.length > 0 && getPreferredTierFn) {
		const tier = getPreferredTierFn(child.skills);
		if (tier) classified = tier;
	}

	// 4. Default
	if (!classified) classified = "victory";

	// 5. Apply effort floor (raises tier if below minimum, or forces local)
	return applyEffortFloor(classified);
}

// ─── Child prompt construction ──────────────────────────────────────────────

/**
 * Build the prompt sent to a child pi process.
 *
 * Uses a sandwich pattern: contract first, context middle, contract reminder last.
 * Skill directives (D2) instruct the child to read SKILL.md files for
 * domain-specific guidance rather than inlining them (200+ lines each).
 */
export function buildChildPrompt(
	taskFileContent: string,
	rootDirective: string,
	workspacePath: string,
): string {
	// Detect if the task file has a Specialist Skills section
	const hasSkills = taskFileContent.includes("## Specialist Skills");

	const contractLines = [
		"## Contract",
		"",
		"You are a child agent managed by the Cleave orchestrator. Follow these rules:",
		"",
		"1. **Scope**: Only work on files within your task scope. Do not modify files outside it.",
		"2. **Task file**: Update your task file when done:",
		"   - Set **Status:** to exactly one of: SUCCESS, PARTIAL, FAILED, or NEEDS_DECOMPOSITION",
		"   - Fill in Summary, Artifacts, Decisions Made, Interfaces Published",
		"3. **Commits**: Commit your work with clear messages. Do not push.",
		"4. **No side effects**: Do not install global packages or modify system state.",
		"5. **Verification**: Run tests or checks and report results in the Verification section.",
		`6. **Workspace**: ${workspacePath}`,
	];

	if (hasSkills) {
		contractLines.push(
			"7. **Skills**: Your task includes a Specialist Skills section. Use the `read` tool to load each listed SKILL.md file before starting work. Follow the conventions and patterns described in those skill files.",
		);
	}

	return [
		contractLines.join("\n"),
		"",
		"## Root Directive",
		"",
		`> ${rootDirective}`,
		"",
		"## Your Task",
		"",
		taskFileContent,
		"",
		"## REMINDER",
		"",
		"Update your task file with the correct status when done. Stay within scope.",
	].join("\n");
}

// ─── Process spawning ───────────────────────────────────────────────────────

interface ChildResult {
	exitCode: number;
	stdout: string;
	stderr: string;
}

/**
 * Spawn a `pi` process for a child task.
 *
 * Uses `pi -p --no-session` for non-interactive execution.
 * The prompt is passed via stdin.
 */
/**
 * Decide whether a raw stdout line from a child pi process is meaningful
 * enough to show as a live status update.
 *
 * pi -p --no-session output includes JSON tool-call records, blank separators,
 * and short metadata lines — these are noisy.  We keep only lines that look
 * like human-readable prose or file-action descriptions.
 */
function isChildStatusLine(raw: string): boolean {
	const s = raw.trim();
	if (s.length < 12) return false;
	// JSON objects / arrays — tool call records
	if (s.startsWith("{") || s.startsWith("[")) return false;
	// ANSI / box-drawing heavy lines (progress bars, borders)
	// eslint-disable-next-line no-control-regex
	if (/\x1b\[/.test(s)) return false;
	// Separator / divider lines
	if (/^[-─═━=*#>|]+\s*$/.test(s)) return false;
	// Very long lines are likely encoded / binary data
	if (s.length > 240) return false;
	return true;
}

/** Strip ANSI codes from a line for display in the dashboard. */
function stripAnsiForStatus(s: string): string {
	// eslint-disable-next-line no-control-regex
	return s.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "").trim();
}

async function spawnChild(
	prompt: string,
	cwd: string,
	timeoutMs: number,
	signal?: AbortSignal,
	localModel?: string,
	onLine?: (line: string) => void,
): Promise<ChildResult> {
	const args = ["-p", "--no-session"];
	if (localModel) {
		args.push("--model", localModel);
	}

	return new Promise<ChildResult>((resolve) => {
		let stdout = "";
		let stderr = "";
		let killed = false;

		const proc = spawn("pi", args, {
			cwd,
			stdio: ["pipe", "pipe", "pipe"],
			env: {
				...process.env,
				// Prevent nested detection issues
				PI_CHILD: "1",
				// https://warhammer40k.fandom.com/wiki/Alpha_Legion
				I_AM: "alpharius",
			},
		});

		// Write prompt to stdin
		if (proc.stdin) {
			proc.stdin.write(prompt);
			proc.stdin.end();
		}

		let lineBuf = "";
		proc.stdout?.on("data", (data: Buffer) => {
			const chunk = data.toString();
			stdout += chunk;
			if (onLine) {
				// Parse line by line and forward meaningful lines
				lineBuf += chunk;
				const parts = lineBuf.split("\n");
				lineBuf = parts.pop() ?? "";
				for (const part of parts) {
					const clean = stripAnsiForStatus(part);
					if (isChildStatusLine(clean)) onLine(clean);
				}
			}
		});
		proc.stderr?.on("data", (data) => { stderr += data.toString(); });

		// Timeout enforcement
		const timer = setTimeout(() => {
			killed = true;
			proc.kill("SIGTERM");
			setTimeout(() => {
				if (!proc.killed) proc.kill("SIGKILL");
			}, 5_000);
		}, timeoutMs);

		// Abort signal support
		const onAbort = () => {
			killed = true;
			proc.kill("SIGTERM");
		};
		signal?.addEventListener("abort", onAbort, { once: true });

		proc.on("close", (code) => {
			clearTimeout(timer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: killed ? -1 : (code ?? 1),
				stdout,
				stderr: killed ? `Killed (timeout or abort)\n${stderr}` : stderr,
			});
		});

		proc.on("error", (err) => {
			clearTimeout(timer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: 1,
				stdout: "",
				stderr: `Failed to spawn pi: ${err.message}`,
			});
		});
	});
}

// ─── Concurrency control ────────────────────────────────────────────────────

/**
 * Simple async semaphore. Guarantees that at most `limit` tasks run
 * concurrently. Uses a queue of resolve callbacks — no polling, no races.
 */
export class AsyncSemaphore {
	private count: number;
	private readonly limit: number;
	private readonly waiters: Array<() => void> = [];

	constructor(limit: number) {
		this.limit = limit;
		this.count = 0;
	}

	async acquire(): Promise<void> {
		if (this.count < this.limit) {
			this.count++;
			return;
		}
		return new Promise<void>((resolve) => {
			this.waiters.push(resolve);
		});
	}

	release(): void {
		const next = this.waiters.shift();
		if (next) {
			// Hand the slot directly to the next waiter (count stays the same)
			next();
		} else {
			this.count--;
		}
	}

	/** Current number of acquired slots (for testing/debugging). */
	get activeCount(): number { return this.count; }
	/** Current number of waiters in queue (for testing/debugging). */
	get waitingCount(): number { return this.waiters.length; }
}

// ─── Dispatch orchestration ─────────────────────────────────────────────────

/**
 * Dispatch all children in dependency-ordered waves.
 *
 * Children within a wave run in parallel (up to maxParallel).
 * Waves are executed sequentially.
 */
export async function dispatchChildren(
	pi: ExtensionAPI,
	state: CleaveState,
	maxParallel: number,
	childTimeoutMs: number,
	localModel?: string,
	signal?: AbortSignal,
	onProgress?: (msg: string) => void,
	reviewConfig?: ReviewConfig,
): Promise<void> {
	const statusResult = await pi.exec("git", ["status", "--porcelain"], {
		cwd: state.repoPath,
		timeout: 5_000,
	});
	if (statusResult.stdout.trim()) {
		throw new Error(
			"Dispatch blocked: repository became dirty before child execution. Resolve the dirty-tree preflight before dispatching.\n" +
			statusResult.stdout.trim(),
		);
	}

	// ── Large-run preflight ──────────────────────────────────────────────────
	// Before dispatching, check if this run qualifies as "large" and the session
	// policy requires operator input before committing to a provider.
	const policy: ProviderRoutingPolicy = (sharedState as any).routingPolicy ?? getDefaultPolicy();
	const childCount = state.children.length;
	const reviewEnabled = reviewConfig?.enabled ?? false;
	const isLargeRun =
		childCount >= LARGE_RUN_THRESHOLD ||
		(reviewEnabled && childCount >= LARGE_RUN_THRESHOLD - 1);

	if (isLargeRun && policy.requirePreflightForLargeRuns) {
		onProgress?.(
			`Preflight: ${childCount} children${reviewEnabled ? " + review" : ""} — asking operator for provider preference…`,
		);
		// Guard: pi.ui.input must exist and be a function. Optional chaining on
		// pi.ui?.input silently returns undefined if input is absent — that path
		// would fall through without any log and is indistinguishable from the
		// operator pressing Enter. Explicit typeof check surfaces the skip.
		const uiInput = (pi as any).ui?.input;
		if (typeof uiInput !== "function") {
			onProgress?.("Preflight skipped (input not available in non-interactive mode)");
		} else {
			try {
				const answer = await uiInput.call(
					(pi as any).ui,
					`🗂️  Large Cleave run (${childCount} children${reviewEnabled ? ", review on" : ""}). ` +
					`Which provider should be favored?\n` +
					`  [1] anthropic  [2] openai  [3] local  [Enter] keep current (${policy.providerOrder[0] ?? "anthropic"}): `,
				) as string | undefined;
				const trimmed = (answer ?? "").trim().toLowerCase();
				let chosenProvider: string | undefined;
				if (trimmed === "1" || trimmed === "anthropic") chosenProvider = "anthropic";
				else if (trimmed === "2" || trimmed === "openai") chosenProvider = "openai";
				else if (trimmed === "3" || trimmed === "local") chosenProvider = "local";

				if (chosenProvider) {
					// Update the session-wide routing policy: move chosen provider to front
					const newOrder = [
						chosenProvider as any,
						...policy.providerOrder.filter((p) => p !== chosenProvider),
					];
					policy.providerOrder = newOrder;
					(sharedState as any).routingPolicy = policy;
					onProgress?.(`Provider order updated: ${newOrder.join(" → ")}`);
				} else {
					onProgress?.(`Keeping current provider order: ${policy.providerOrder.join(" → ")}`);
				}
			} catch {
				// pi.input() threw unexpectedly; proceed with defaults
				onProgress?.("Preflight skipped (input threw an unexpected error)");
			}
		}
	}

	const waves = computeDispatchWaves(
		state.children.map((c) => ({ label: c.label, dependsOn: c.dependsOn })),
	);

	const semaphore = new AsyncSemaphore(maxParallel);
	const effectiveReviewConfig = reviewConfig ?? DEFAULT_REVIEW_CONFIG;

	let childrenDispatched = 0;
	const totalChildren = state.children.length;

	for (let waveIdx = 0; waveIdx < waves.length; waveIdx++) {
		const waveLabels = waves[waveIdx];
		const waveChildren = state.children.filter((c) => waveLabels.includes(c.label));
		onProgress?.(
			`dispatching ${waveChildren.map((c) => c.label).join(", ")}`,
		);
		childrenDispatched += waveChildren.length;

		const promises = waveChildren.map(async (child) => {
			await semaphore.acquire();
			try {
				await dispatchSingleChild(pi, state, child, childTimeoutMs, localModel, signal, effectiveReviewConfig);
			} finally {
				semaphore.release();
			}
		});

		await Promise.all(promises);

		// Persist state after each wave
		saveState(state);

		// Check for abort
		if (signal?.aborted) break;
	}
}

/**
 * Dispatch a single child: read task file, spawn pi, harvest result.
 *
 * Per-child model routing: each child's `executeModel` tier determines
 * which model is passed via `--model`. The `localModel` param provides
 * the Ollama model name for children with "local" tier.
 *
 * When review is enabled, the execution is wrapped in executeWithReview
 * which runs an adversarial review loop with severity gating and churn detection.
 */
async function dispatchSingleChild(
	pi: ExtensionAPI,
	state: CleaveState,
	child: ChildState,
	timeoutMs: number,
	localModel?: string,
	signal?: AbortSignal,
	reviewConfig?: ReviewConfig,
): Promise<void> {
	// Skip children that are already settled — idempotent on resume.
	// "completed" covers a successful prior run; "failed" covers worktree
	// creation failures or a previous dispatch that returned non-zero.
	if (child.status === "completed" || child.status === "failed") return;

	child.status = "running";
	child.startedAt = new Date().toISOString();
	const startedAtMs = Date.now();

	// Mirror to sharedState for live dashboard updates (include startedAt for elapsed ticker)
	emitCleaveChildProgress(pi, child.childId, { status: "running", startedAt: startedAtMs, worktreePath: child.worktreePath });

	// Debounced last-line emitter: buffers stdout lines and pushes to shared
	// state at most once per 500ms to avoid flooding the event bus.
	let pendingLine: string | undefined;
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;
	const flushLine = () => {
		if (pendingLine !== undefined) {
			emitCleaveChildProgress(pi, child.childId, { lastLine: pendingLine });
			pendingLine = undefined;
		}
	};
	const onChildLine = (line: string) => {
		pendingLine = line;
		if (!debounceTimer) {
			debounceTimer = setTimeout(() => {
				debounceTimer = undefined;
				flushLine();
			}, 500);
		}
	};
	const stopDebounce = () => {
		clearTimeout(debounceTimer);
		debounceTimer = undefined;
	};

	// Resolve an explicit model ID for this child using the shared resolver.
	// This replaces the old mapModelTierToFlag() fuzzy-alias approach.
	const effectiveTier = (child.executeModel as ModelTier) ?? "victory";
	const activePolicy: ProviderRoutingPolicy = (sharedState as any).routingPolicy ?? getDefaultPolicy();
	let registryModels: RegistryModel[] = [];
	try {
		const registry = (pi as any).modelRegistry;
		if (registry != null) {
			registryModels = getViableModels(registry);
		}
		// If modelRegistry is absent (e.g. test environment), registryModels stays []
		// and resolveTier will use policy-based fallbacks.
	} catch (err) {
		// getViableModels() threw — log and continue with empty registry so resolver can still
		// apply policy-based fallbacks rather than silently passing no --model flag.
		console.warn("[cleave] getViableModels() threw:", err);
	}
	const modelFlag = resolveModelIdForTier(effectiveTier, registryModels, activePolicy, localModel);
	child.backend = child.executeModel === "local" ? "local" : "cloud";

	// Read the task file
	const taskFilePath = join(state.workspacePath, `${child.childId}-task.md`);
	let taskContent: string;
	try {
		taskContent = readFileSync(taskFilePath, "utf-8");
	} catch {
		child.status = "failed";
		child.error = `Task file not found: ${taskFilePath}`;
		return;
	}

	// Build prompt
	const prompt = buildChildPrompt(taskContent, state.directive, state.workspacePath);

	// Determine working directory
	const cwd = child.worktreePath || state.repoPath;

	// Build executor adapter for the review loop
	const executor: ReviewExecutor = {
		execute: async (execPrompt: string, execCwd: string, execModelFlag?: string) => {
			return spawnChild(execPrompt, execCwd, timeoutMs, signal, execModelFlag, onChildLine);
		},
		review: async (reviewPrompt: string, reviewCwd: string) => {
			// Reviews always use gloriana (D4: highest available tier) — resolve to explicit ID
			const reviewModelId = resolveModelIdForTier("gloriana", registryModels, activePolicy, localModel);
			// Review runs don't stream lastLine — they're short and we don't want
			// review commentary to overwrite the last execution status line.
			return spawnChild(reviewPrompt, reviewCwd, timeoutMs, signal, reviewModelId);
		},
		readFile: (path: string) => readFileSync(path, "utf-8"),
	};

	const effectiveReviewConfig = reviewConfig ?? DEFAULT_REVIEW_CONFIG;

	// Execute with optional review loop
	const reviewResult = await executeWithReview(
		executor,
		taskFilePath,
		state.directive,
		cwd,
		effectiveReviewConfig,
		modelFlag,
	);

	// Stop the debounce timer — child process is done
	stopDebounce();

	// Use the initial execution result for status determination
	const result = reviewResult.executeResult;

	child.completedAt = new Date().toISOString();
	if (child.startedAt) {
		child.durationSec = Math.round(
			(new Date(child.completedAt).getTime() - new Date(child.startedAt).getTime()) / 1000,
		);
	}

	// Persist review metadata on the child state
	child.reviewIterations = reviewResult.reviewHistory.length;
	child.reviewDecision = reviewResult.finalDecision;
	child.reviewHistory = reviewResult.reviewHistory.map((r) => ({
		round: r.round,
		status: r.verdict.status,
		issueCount: r.verdict.issues.length,
		reappeared: r.reappeared,
	}));
	if (reviewResult.escalationReason) {
		child.reviewEscalationReason = reviewResult.escalationReason;
	}

	// Determine child status from process exit code
	if (result.exitCode === 0) {
		child.status = "completed";
	} else if (result.exitCode === -1) {
		child.status = "failed";
		child.error = "Timed out or aborted";
	} else {
		child.status = "failed";
		child.error = result.stderr.slice(0, 2000) || `Exit code ${result.exitCode}`;
	}

	// If review escalated, mark the child as failed
	if (reviewResult.finalDecision === "escalated") {
		child.status = "failed";
		child.error = `Review escalated: ${reviewResult.escalationReason}`;
	}

	// Re-read the task file to check if the child updated the status.
	// IMPORTANT: Only parse the ## Result section to avoid false positives
	// from the Contract section boilerplate which mentions NEEDS_DECOMPOSITION
	// as an instruction (not as an actual status).
	try {
		const updatedContent = readFileSync(taskFilePath, "utf-8");
		const resultSection = extractResultSection(updatedContent);
		if (resultSection.includes("**Status:** NEEDS_DECOMPOSITION")) {
			child.status = "needs_decomposition";
		} else if (resultSection.includes("**Status:** FAILED")) {
			child.status = "failed";
			child.error = "Child reported FAILED in task file";
		} else if (resultSection.includes("**Status:** SUCCESS") || resultSection.includes("**Status:** PARTIAL")) {
			// Child explicitly reported success — trust the task file over exit code
			// But only if review didn't escalate
			if (reviewResult.finalDecision !== "escalated") {
				child.status = "completed";
			}
		}
	} catch {
		// Task file not readable — keep whatever status we have
	}

	// Mirror final status to sharedState for live dashboard updates
	emitCleaveChildProgress(pi, child.childId, {
		status: child.status === "completed" ? "done" : child.status === "failed" ? "failed" : "pending",
		elapsed: child.durationSec,
	});
}

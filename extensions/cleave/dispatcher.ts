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
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import type { ChildState, CleaveState } from "./types.js";
import { computeDispatchWaves } from "./planner.js";
import { saveState } from "./workspace.js";

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

// ─── Child prompt construction ──────────────────────────────────────────────

/**
 * Build the prompt sent to a child pi process.
 *
 * Uses a sandwich pattern: contract first, context middle, contract reminder last.
 */
function buildChildPrompt(
	taskFileContent: string,
	rootDirective: string,
	workspacePath: string,
): string {
	const contract = `## Contract

You are a child agent managed by the Cleave orchestrator. Follow these rules:

1. **Scope**: Only work on files within your task scope. Do not modify files outside it.
2. **Task file**: Update your task file when done:
   - Set **Status:** to exactly one of: SUCCESS, PARTIAL, FAILED, or NEEDS_DECOMPOSITION
   - Fill in Summary, Artifacts, Decisions Made, Interfaces Published
3. **Commits**: Commit your work with clear messages. Do not push.
4. **No side effects**: Do not install global packages or modify system state.
5. **Verification**: Run tests or checks and report results in the Verification section.
6. **Workspace**: ${workspacePath}`;

	return [
		contract,
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
async function spawnChild(
	prompt: string,
	cwd: string,
	timeoutMs: number,
	signal?: AbortSignal,
	localModel?: string,
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
			},
		});

		// Write prompt to stdin
		if (proc.stdin) {
			proc.stdin.write(prompt);
			proc.stdin.end();
		}

		proc.stdout?.on("data", (data) => { stdout += data.toString(); });
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
): Promise<void> {
	const waves = computeDispatchWaves(
		state.children.map((c) => ({ label: c.label, dependsOn: c.dependsOn })),
	);

	const semaphore = new AsyncSemaphore(maxParallel);

	for (let waveIdx = 0; waveIdx < waves.length; waveIdx++) {
		const waveLabels = waves[waveIdx];
		const waveChildren = state.children.filter((c) => waveLabels.includes(c.label));

		onProgress?.(
			`Wave ${waveIdx + 1}/${waves.length}: dispatching ${waveChildren.map((c) => c.label).join(", ")}`,
		);

		const promises = waveChildren.map(async (child) => {
			await semaphore.acquire();
			try {
				await dispatchSingleChild(pi, state, child, childTimeoutMs, localModel, signal);
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
 */
async function dispatchSingleChild(
	pi: ExtensionAPI,
	state: CleaveState,
	child: ChildState,
	timeoutMs: number,
	localModel?: string,
	signal?: AbortSignal,
): Promise<void> {
	// Skip children that already failed (e.g., worktree creation failure)
	if (child.status === "failed") return;

	child.status = "running";
	child.startedAt = new Date().toISOString();

	// Determine execution backend
	const useLocal = localModel && child.backend === "local";
	child.backend = useLocal ? "local" : "cloud";

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

	// Spawn
	const result = await spawnChild(
		prompt,
		cwd,
		timeoutMs,
		signal,
		useLocal ? localModel : undefined,
	);

	child.completedAt = new Date().toISOString();
	if (child.startedAt) {
		child.durationSec = Math.round(
			(new Date(child.completedAt).getTime() - new Date(child.startedAt).getTime()) / 1000,
		);
	}

	if (result.exitCode === 0) {
		child.status = "completed";
	} else if (result.exitCode === -1) {
		child.status = "failed";
		child.error = "Timed out or aborted";
	} else {
		child.status = "failed";
		child.error = result.stderr.slice(0, 2000) || `Exit code ${result.exitCode}`;
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
			child.status = "completed";
		}
	} catch {
		// Task file not readable — keep whatever status we have
	}
}

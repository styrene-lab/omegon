/**
 * cleave/dispatcher — Child process dispatch and monitoring.
 *
 * Spawns Omegon-owned subprocesses for each child task, using the same
 * subagent pattern as pi's example extension. Each child runs in
 * its own git worktree with an isolated context.
 *
 * Supports two backends:
 * - "cloud": spawns a full Omegon child process (uses cloud API)
 * - "local": spawns Omegon with --model pointing to a local Ollama model
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
import type { ExtensionAPI } from "@styrene-lab/pi-coding-agent";
import { DASHBOARD_UPDATE_EVENT, sharedState } from "../lib/shared-state.ts";
import type { ChildState, CleaveState, ModelTier, RpcChildEvent, RpcProgressUpdate } from "./types.ts";
import { computeDispatchWaves } from "./planner.ts";
import { sendRpcCommand, buildPromptCommand, parseRpcEventStream, mapEventToProgress } from "./rpc-child.ts";
import { executeWithReview, type ReviewConfig, type ReviewExecutor, DEFAULT_REVIEW_CONFIG } from "./review.ts";
import { saveState } from "./workspace.ts";
import { resolveTier, getDefaultPolicy, getViableModels, type ProviderRoutingPolicy, type RegistryModel } from "../lib/model-routing.ts";
import { resolveOmegonSubprocess, resolveNativeAgent, type NativeAgentSpec } from "../lib/omegon-subprocess.ts";
import { registerCleaveProc, deregisterCleaveProc, killCleaveProc } from "./subprocess-tracker.ts";

// ─── Large-run threshold ────────────────────────────────────────────────────

/**
 * Number of children at or above which a run is considered "large".
 * When session policy requirePreflightForLargeRuns is true and this threshold
 * is exceeded, the operator is asked for their preferred provider before dispatch.
 *
 * Also triggers for runs with review enabled where children >= 3.
 */
export const LARGE_RUN_THRESHOLD = 4;

/**
 * Default per-child wall-clock timeout (15 minutes).
 * Hard backstop — children that exceed this are killed regardless of activity.
 */
export const DEFAULT_CHILD_TIMEOUT_MS = 15 * 60 * 1000;

/**
 * Hardcoded provider:model defaults for native dispatch when the pi model
 * registry is unavailable (e.g. cleave_run tool context where pi.modelRegistry
 * is not populated). The native binary needs an explicit provider:model string.
 */
export const NATIVE_TIER_DEFAULTS: Record<string, string> = {
	gloriana: "anthropic:claude-sonnet-4-20250514",
	victory: "anthropic:claude-sonnet-4-20250514",
	retribution: "anthropic:claude-haiku-3-5-20241022",
};

/**
 * Default RPC idle timeout (3 minutes).
 * If no RPC event arrives within this window, the child is considered stalled
 * and is killed. Resets on every event (tool_start, tool_end, assistant_message,
 * etc.). Only applies to RPC mode — pipe mode children use wall-clock only.
 */
export const IDLE_TIMEOUT_MS = 3 * 60 * 1000;

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
	patch: { status?: "pending" | "running" | "done" | "failed"; elapsed?: number; startedAt?: number; lastLine?: string; worktreePath?: string; rpcProgress?: RpcProgressUpdate },
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
	if (patch.rpcProgress !== undefined) {
		// Structured RPC progress — use summary as lastLine for backward compat
		const summary = patch.rpcProgress.summary;
		cleaveState.children[childId].lastLine = summary;
		const child = cleaveState.children[childId];
		if (!child.recentLines) child.recentLines = [];
		child.recentLines.push(summary);
		if (child.recentLines.length > 30) child.recentLines.splice(0, child.recentLines.length - 30);
	} else if (patch.lastLine !== undefined) {
		// Update lastLine for backward compat (pipe mode)
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
		"5. **Testing**: Write tests for new functions and changed behavior in co-located *.test.ts files. Run them and report results in the Verification section. Untested code is incomplete.",
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

/**
 * Spawn a child in pipe mode (legacy).
 * Uses `pi -p --no-session`, writes prompt to stdin, closes stdin.
 */
async function spawnChildPipe(
	prompt: string,
	cwd: string,
	timeoutMs: number,
	signal?: AbortSignal,
	localModel?: string,
	onLine?: (line: string) => void,
): Promise<ChildResult> {
	const omegon = resolveOmegonSubprocess();
	const args = [...omegon.argvPrefix, "-p", "--no-session"];
	if (localModel) {
		args.push("--model", localModel);
	}

	return new Promise<ChildResult>((resolve) => {
		let stdout = "";
		let stderr = "";
		let killed = false;

		const proc = spawn(omegon.command, args, {
			cwd,
			stdio: ["pipe", "pipe", "pipe"],
			detached: true,
			env: {
				...process.env,
				PI_CHILD: "1",
				I_AM: "alpharius",
			},
		});
		registerCleaveProc(proc);

		// Write prompt to stdin and close (pipe mode)
		if (proc.stdin) {
			proc.stdin.write(prompt);
			proc.stdin.end();
		}

		let lineBuf = "";
		proc.stdout?.on("data", (data: Buffer) => {
			const chunk = data.toString();
			stdout += chunk;
			if (onLine) {
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

		let escalationTimer: ReturnType<typeof setTimeout> | undefined;
		const scheduleEscalation = () => {
			escalationTimer = setTimeout(() => {
				if (!proc.killed) {
					try {
						if (proc.pid) process.kill(-proc.pid, "SIGKILL");
					} catch {
						try { proc.kill("SIGKILL"); } catch { /* already dead */ }
					}
				}
			}, 5_000);
		};

		const timer = setTimeout(() => {
			killed = true;
			killCleaveProc(proc);
			scheduleEscalation();
		}, timeoutMs);

		const onAbort = () => {
			killed = true;
			killCleaveProc(proc);
			scheduleEscalation();
		};
		signal?.addEventListener("abort", onAbort, { once: true });

		let settled = false;
		proc.on("close", (code) => {
			if (settled) return;
			settled = true;
			deregisterCleaveProc(proc);
			clearTimeout(timer);
			clearTimeout(escalationTimer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: killed ? -1 : (code ?? 1),
				stdout,
				stderr: killed ? `Killed (timeout or abort)\n${stderr}` : stderr,
			});
		});

		proc.on("error", (err) => {
			if (settled) return;
			settled = true;
			deregisterCleaveProc(proc);
			clearTimeout(timer);
			clearTimeout(escalationTimer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: 1,
				stdout: "",
				stderr: `Failed to spawn pi: ${err.message}`,
			});
		});
	});
}

/**
 * Spawn a child using the native omegon-agent binary (pipe mode).
 *
 * The Rust binary:
 * - Accepts task via --prompt (reads stdin if not provided, but we pass explicitly)
 * - Writes events to stderr (tracing logs)
 * - Writes final assistant text to stdout (exit code 0 = success)
 * - Has built-in: read, write, edit, bash tools + path traversal protection,
 *   turn limits, retry, stuck detection, and auto-validation
 *
 * This is Ship of Theseus Plank #1 — headless cleave children execute in
 * the Rust loop instead of spawning a full TS Omegon process.
 */
async function spawnChildNative(
	native: NativeAgentSpec,
	prompt: string,
	cwd: string,
	timeoutMs: number,
	model: string,
	signal?: AbortSignal,
	onLine?: (line: string) => void,
	maxTurns: number = 50,
): Promise<ChildResult> {
	const args = [
		"--prompt", prompt,
		"--cwd", cwd,
		"--bridge", native.bridgePath,
		"--model", model,
		"--max-turns", String(maxTurns),
	];

	return new Promise<ChildResult>((resolve) => {
		let stdout = "";
		let stderr = "";
		let killed = false;

		const proc = spawn(native.binaryPath, args, {
			cwd,
			stdio: ["pipe", "pipe", "pipe"],
			detached: true,
			env: {
				...process.env,
				// RUST_LOG controls tracing verbosity for the Rust binary
				RUST_LOG: process.env.RUST_LOG ?? "info",
			},
		});
		registerCleaveProc(proc);

		// stdin not needed — prompt passed via --prompt flag
		proc.stdin?.end();

		let lineBuf = "";
		proc.stdout?.on("data", (data: Buffer) => {
			const chunk = data.toString();
			stdout += chunk;
			if (onLine) {
				lineBuf += chunk;
				const parts = lineBuf.split("\n");
				lineBuf = parts.pop() ?? "";
				for (const part of parts) {
					const clean = stripAnsiForStatus(part);
					if (isChildStatusLine(clean)) onLine(clean);
				}
			}
		});

		// Parse stderr for tool events (tracing lines contain → toolname and ✓/✗)
		proc.stderr?.on("data", (data: Buffer) => {
			const chunk = data.toString();
			stderr += chunk;
			if (onLine) {
				// Extract tool progress from tracing output
				for (const line of chunk.split("\n")) {
					const toolMatch = line.match(/→ (\w+)/);
					if (toolMatch) {
						onLine(`tool: ${toolMatch[1]}`);
					} else if (line.includes("✓") || line.includes("✗")) {
						const clean = stripAnsiForStatus(line);
						if (clean.length > 5) onLine(clean);
					}
				}
			}
		});

		let escalationTimer: ReturnType<typeof setTimeout> | undefined;
		const scheduleEscalation = () => {
			escalationTimer = setTimeout(() => {
				if (!proc.killed) {
					try {
						if (proc.pid) process.kill(-proc.pid, "SIGKILL");
					} catch {
						try { proc.kill("SIGKILL"); } catch { /* already dead */ }
					}
				}
			}, 5_000);
		};

		const timer = setTimeout(() => {
			killed = true;
			killCleaveProc(proc);
			scheduleEscalation();
		}, timeoutMs);

		const onAbort = () => {
			killed = true;
			killCleaveProc(proc);
			scheduleEscalation();
		};
		signal?.addEventListener("abort", onAbort, { once: true });

		let settled = false;
		proc.on("close", (code) => {
			if (settled) return;
			settled = true;
			deregisterCleaveProc(proc);
			clearTimeout(timer);
			clearTimeout(escalationTimer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: killed ? -1 : (code ?? 1),
				stdout,
				stderr: killed ? `Killed (timeout or abort)\n${stderr}` : stderr,
			});
		});

		proc.on("error", (err) => {
			if (settled) return;
			settled = true;
			deregisterCleaveProc(proc);
			clearTimeout(timer);
			clearTimeout(escalationTimer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: 1,
				stdout: "",
				stderr: `Failed to spawn omegon-agent: ${err.message}`,
			});
		});
	});
}

/** Events collected during an RPC child session. */
interface RpcChildResult extends ChildResult {
	events: RpcChildEvent[];
	pipeBroken: boolean;
}

/**
 * Spawn a child in RPC mode.
 * Uses `--mode rpc --no-session`, sends prompt via sendRpcCommand on stdin,
 * parses stdout as a JSON event stream. Stdin stays open for the session lifetime.
 */
async function spawnChildRpc(
	prompt: string,
	cwd: string,
	timeoutMs: number,
	signal?: AbortSignal,
	localModel?: string,
	onEvent?: (event: RpcChildEvent) => void,
	idleTimeoutMs: number = IDLE_TIMEOUT_MS,
): Promise<RpcChildResult> {
	const omegon = resolveOmegonSubprocess();
	const args = [...omegon.argvPrefix, "--mode", "rpc", "--no-session"];
	if (localModel) {
		args.push("--model", localModel);
	}

	return new Promise<RpcChildResult>((resolve) => {
		let stderr = "";
		let killed = false;
		const events: RpcChildEvent[] = [];
		let pipeBroken = false;

		const proc = spawn(omegon.command, args, {
			cwd,
			stdio: ["pipe", "pipe", "pipe"],
			detached: true,
			env: {
				...process.env,
				PI_CHILD: "1",
				I_AM: "alpharius",
			},
		});
		registerCleaveProc(proc);

		// Send prompt via RPC command on stdin — keep stdin open
		if (proc.stdin) {
			const cmd = buildPromptCommand(prompt);
			sendRpcCommand(proc.stdin, cmd);
			// Do NOT close stdin — child may need it for the session lifetime
		}

		// Collect stderr
		proc.stderr?.on("data", (data) => { stderr += data.toString(); });

		// ── Idle timeout ─────────────────────────────────────────────────
		// Reset on every RPC event. If no event arrives within the idle
		// window, the child is stalled — kill it.
		let idleKilled = false;
		let idleTimer: ReturnType<typeof setTimeout> | undefined;
		const resetIdleTimer = () => {
			if (idleTimer) clearTimeout(idleTimer);
			if (idleTimeoutMs > 0) {
				idleTimer = setTimeout(() => {
					if (!killed && !proc.killed) {
						idleKilled = true;
						killed = true;
						killCleaveProc(proc);
						scheduleEscalation();
					}
				}, idleTimeoutMs);
			}
		};

		// Parse stdout exclusively via RPC event stream (no competing data listener)
		let escalationTimer: ReturnType<typeof setTimeout> | undefined;
		const scheduleEscalation = () => {
			escalationTimer = setTimeout(() => {
				if (!proc.killed) {
					try {
						if (proc.pid) process.kill(-proc.pid, "SIGKILL");
					} catch {
						try { proc.kill("SIGKILL"); } catch { /* already dead */ }
					}
				}
			}, 5_000);
		};

		let sawAgentEnd = false;
		let eventsFinished: Promise<void> = Promise.resolve();
		if (proc.stdout) {
			eventsFinished = (async () => {
				try {
					for await (const event of parseRpcEventStream(proc.stdout!)) {
						events.push(event);
						resetIdleTimer(); // activity — push back the idle deadline
						if (event.type === "pipe_closed") {
							pipeBroken = !sawAgentEnd; // only a break if agent didn't finish cleanly
							if (pipeBroken && !killed && !proc.killed) {
								// Stdout died unexpectedly — kill immediately instead of
								// waiting 3min for the idle timeout to notice.
								killed = true;
								killCleaveProc(proc);
								scheduleEscalation();
							}
						}
						// When the agent loop finishes, close stdin and kill after
						// a brief grace period. The RPC mode's process stays alive
						// on `new Promise(() => {})` even after stdin closes — it has
						// no shutdown command. Without the kill, the child sits idle
						// until the 3min idle timeout, which gets misreported as a
						// pipe break.
						if (event.type === "agent_end") {
							sawAgentEnd = true;
							onEvent?.(event);
							// Agent is done — kill immediately. The RPC process has
							// no shutdown command and hangs on `new Promise(() => {})`
							// forever. There is nothing left to flush.
							try { proc.stdin?.end(); } catch { /* already closed */ }
							killed = true;
							killCleaveProc(proc);
							scheduleEscalation();
							break; // Exit the event loop — we're done
						}
						onEvent?.(event);
					}
				} catch {
					// Stream parsing error — treat as pipe break
					pipeBroken = true;
				}
			})();
		}

		// Start the idle timer now — if the child never emits an event, it's
		// caught within the idle window rather than waiting for the full wall clock.
		resetIdleTimer();

		const timer = setTimeout(() => {
			killed = true;
			killCleaveProc(proc);
			scheduleEscalation();
		}, timeoutMs);

		const onAbort = () => {
			killed = true;
			killCleaveProc(proc);
			scheduleEscalation();
		};
		signal?.addEventListener("abort", onAbort, { once: true });

		let settled = false;
		proc.on("close", async (code) => {
			if (settled) return;
			settled = true;
			deregisterCleaveProc(proc);
			clearTimeout(timer);
			clearTimeout(escalationTimer);
			clearTimeout(idleTimer);
			signal?.removeEventListener("abort", onAbort);

			// Close stdin if still open (child has exited)
			try { proc.stdin?.end(); } catch { /* already closed */ }

			// Wait for all RPC events to be consumed before resolving
			await eventsFinished;

			const killReason = sawAgentEnd
				? stderr // Clean completion — agent finished, we killed the leftover process
				: idleKilled
					? `Killed (idle — no RPC events for ${Math.round(idleTimeoutMs / 1000)}s)\n${stderr}`
					: killed
						? `Killed (timeout or abort)\n${stderr}`
						: stderr;

			resolve({
				// sawAgentEnd means the agent completed successfully — treat as exit 0
				// even though we had to kill the process (RPC has no shutdown command)
				exitCode: sawAgentEnd ? 0 : (killed ? -1 : (code ?? 1)),
				stdout: "",
				stderr: killReason,
				events,
				pipeBroken,
			});
		});

		proc.on("error", (err) => {
			if (settled) return;
			settled = true;
			deregisterCleaveProc(proc);
			clearTimeout(timer);
			clearTimeout(escalationTimer);
			clearTimeout(idleTimer);
			signal?.removeEventListener("abort", onAbort);
			resolve({
				exitCode: 1,
				stdout: "",
				stderr: `Failed to spawn pi: ${err.message}`,
				events,
				pipeBroken: true,
			});
		});
	});
}

/**
 * Spawn a child process — dispatches to native, RPC, or pipe mode.
 *
 * Dispatch priority:
 * 1. Native (Rust binary) — when nativeAgent is provided and useNative=true
 * 2. RPC (JSON events) — when useRpc=true and not using native
 * 3. Pipe (legacy) — fallback
 *
 * @param useRpc      When true, uses RPC mode with structured events.
 * @param nativeAgent When provided, attempts native dispatch (Ship of Theseus).
 * @param useNative   When true (and nativeAgent is available), use the native binary.
 * @param model       Model ID string (for native binary --model flag).
 */
async function spawnChild(
	prompt: string,
	cwd: string,
	timeoutMs: number,
	signal?: AbortSignal,
	localModel?: string,
	onLine?: (line: string) => void,
	useRpc?: boolean,
	onEvent?: (event: RpcChildEvent) => void,
	nativeAgent?: NativeAgentSpec | null,
	useNative?: boolean,
	model?: string,
): Promise<ChildResult> {
	// Native dispatch: use the Rust binary when available and requested
	if (useNative && nativeAgent && model) {
		return spawnChildNative(nativeAgent, prompt, cwd, timeoutMs, model, signal, onLine);
	}
	if (useRpc) {
		return spawnChildRpc(prompt, cwd, timeoutMs, signal, localModel, onEvent);
	}
	return spawnChildPipe(prompt, cwd, timeoutMs, signal, localModel, onLine);
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

	// ── Progress callbacks ──────────────────────────────────────────────────
	// RPC mode: direct event forwarding (no debounce)
	// Pipe mode: debounced line emitter (legacy)
	const useRpc = true;

	// RPC event handler — forward structured progress directly
	const onRpcEvent = (event: RpcChildEvent) => {
		const progress = mapEventToProgress(event);
		if (progress) {
			emitCleaveChildProgress(pi, child.childId, { rpcProgress: progress });
		}
	};

	// Pipe mode fallback: debounced last-line emitter
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

	// ── Native agent resolution ─────────────────────────────────────────────
	// Ship of Theseus: prefer the Rust binary for non-local cloud children.
	// The native agent has the 4 primitive tools (read, write, edit, bash)
	// plus turn limits, retry, stuck detection, and auto-validation.
	// Local-tier children still go through TS (Ollama integration is Phase 3).
	const nativeAgent = resolveNativeAgent();

	// Build the provider:model string for the Rust binary.
	// resolveModelIdForTier returns just the model ID (e.g., "claude-sonnet-4-20250514");
	// the Rust binary expects "provider:model" format (e.g., "anthropic:claude-sonnet-4-20250514").
	let nativeModelSpec: string | undefined;
	if (effectiveTier !== "local") {
		if (modelFlag) {
			const resolved = resolveTier(effectiveTier, registryModels, activePolicy);
			nativeModelSpec = resolved
				? `${resolved.provider}:${resolved.modelId}`
				: `anthropic:${modelFlag}`; // fallback: assume anthropic
		} else {
			// Registry unavailable (empty registryModels) — use hardcoded tier defaults.
			// The native binary just needs a provider:model string; pi-ai resolution
			// isn't available in this code path when the registry is empty.
			nativeModelSpec = NATIVE_TIER_DEFAULTS[effectiveTier];
		}
	}

	const useNative = nativeAgent != null
		&& effectiveTier !== "local"
		&& nativeModelSpec != null
		// Opt-out: OMEGON_NATIVE_DISPATCH=0 disables native dispatch
		&& process.env.OMEGON_NATIVE_DISPATCH !== "0";

	// DEBUG: trace native dispatch decision
	console.warn(`[cleave:debug] child=${child.label} effectiveTier=${effectiveTier} nativeAgent=${nativeAgent != null} nativeModelSpec=${nativeModelSpec} OMEGON_NATIVE_DISPATCH=${process.env.OMEGON_NATIVE_DISPATCH} → useNative=${useNative}`);

	if (useNative) {
		child.backend = "native";
	}

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
		execute: async (execPrompt: string, execCwd: string, _execModelFlag?: string) => {
			if (useNative) {
				// Native dispatch: Rust binary with pipe mode (stdout=final text)
				// Uses nativeModelSpec (provider:model) instead of the bare model ID
				return spawnChild(
					execPrompt, execCwd, timeoutMs, signal, undefined,
					onChildLine, false /* not RPC */, undefined,
					nativeAgent, true /* useNative */, nativeModelSpec,
				);
			}
			// TS dispatch: RPC mode for structured events
			return spawnChild(execPrompt, execCwd, timeoutMs, signal, _execModelFlag,
				useRpc ? undefined : onChildLine, useRpc, useRpc ? onRpcEvent : undefined);
		},
		review: async (reviewPrompt: string, reviewCwd: string) => {
			// Reviews always use TS + pipe mode (Phase 1) + gloriana tier.
			// The native binary doesn't have review infrastructure yet.
			const reviewModelId = resolveModelIdForTier("gloriana", registryModels, activePolicy, localModel);
			return spawnChild(reviewPrompt, reviewCwd, timeoutMs, signal, reviewModelId,
				undefined, false /* pipe mode for review */);
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

	// RPC pipe-break handling: if stdout closed unexpectedly, mark failed
	// but preserve worktree and branch for recovery
	if (useRpc && "pipeBroken" in result && (result as RpcChildResult).pipeBroken) {
		// Only override status if it wasn't already set to completed (child may
		// have finished before the pipe break was detected)
		if (child.status !== "completed") {
			child.status = "failed";
			child.error = "RPC pipe break: stdout closed unexpectedly — worktree preserved for recovery";
			// Do NOT clean up worktree — preserve for manual recovery
		}
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

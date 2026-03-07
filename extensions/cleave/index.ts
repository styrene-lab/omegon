/**
 * cleave — Recursive task decomposition extension for pi.
 *
 * Provides:
 *   - `cleave_assess` tool: Assess directive complexity (LLM-callable)
 *   - `/assess` command: Code assessment toolkit (cleave, diff, spec, complexity)
 *   - `/cleave` command: Full decomposition workflow
 *   - Session-start handler: Surfaces active OpenSpec changes with task progress
 *
 * State machine: ASSESS → PLAN → CONFIRM → DISPATCH → HARVEST → REPORT
 *
 * Ported from styrene-lab/cleave (Python) — the pattern library, complexity
 * formula, conflict detection, and worktree management are preserved.
 * The Claude Code SDK calls are replaced with pi's extension API.
 */

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { truncateTail, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, formatSize } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";

import { sharedState, DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import { assessDirective, PATTERNS } from "./assessment.js";
import { detectConflicts, parseTaskResult } from "./conflicts.js";
import { dispatchChildren, resolveExecuteModel } from "./dispatcher.js";
import { DEFAULT_REVIEW_CONFIG, type ReviewConfig } from "./review.js";
import {
	detectOpenSpec,
	findExecutableChanges,
	openspecChangeToSplitPlanWithContext,
	buildOpenSpecContext,
	writeBackTaskCompletion,
	getActiveChangesStatus,
	type OpenSpecContext,
} from "./openspec.js";
import { buildPlannerPrompt, getRepoTree, parsePlanResponse } from "./planner.js";
import {
	matchSkillsToAllChildren,
	resolveSkillPaths,
	getPreferredTier,
} from "./skills.js";
import type { CleaveState, ChildState, SplitPlan } from "./types.js";
import { DEFAULT_CONFIG } from "./types.js";
import { initWorkspace, readTaskFiles, saveState } from "./workspace.js";
import type { SkillDirective } from "./workspace.js";
import {
	cleanupWorktrees,
	createWorktree,
	ensureCleanWorktree,
	getCurrentBranch,
	mergeBranch,
	pruneWorktreeDirs,
} from "./worktree.js";

// ─── Dashboard state emitter ────────────────────────────────────────────────

/** Map internal ChildStatus to the dashboard's simplified status. */
function mapChildStatus(status: string): "pending" | "running" | "done" | "failed" {
	if (status === "completed") return "done";
	if (status === "running" || status === "failed") return status;
	return "pending"; // pending, needs_decomposition → pending
}

/**
 * Emit cleave dashboard state to sharedState.cleave and fire the
 * dashboard update event so the footer re-renders immediately.
 *
 * Called at lifecycle transitions so the unified dashboard can
 * render live progress without polling.
 */
function emitCleaveState(
	pi: ExtensionAPI,
	status: string,
	runId?: string,
	children?: Array<{ label: string; status: string; durationSec?: number }>,
): void {
	(sharedState as any).cleave = {
		status,
		runId,
		children: children?.map((c) => ({
			label: c.label,
			status: mapChildStatus(c.status),
			elapsed: c.durationSec,
		})),
	};
	pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "cleave" });
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function generateRunId(): string {
	return `clv-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

function formatAssessment(a: ReturnType<typeof assessDirective>): string {
	const lines = [
		"**Assessment**",
		"",
		`  Decision: **${a.decision}**`,
		`  Complexity: ${a.complexity}`,
		`  Systems: ${a.systems}`,
		`  Modifiers: ${a.modifiers.length > 0 ? a.modifiers.join(", ") : "none"}`,
		`  Method: ${a.method}`,
	];
	if (a.pattern) {
		lines.push(`  Pattern: ${a.pattern} (${(a.confidence * 100).toFixed(0)}%)`);
	}
	lines.push("", `  ${a.reasoning}`);
	return lines.join("\n");
}

function formatConflicts(conflicts: ReturnType<typeof detectConflicts>): string {
	if (conflicts.length === 0) return "No conflicts detected. ✓";
	return conflicts
		.map(
			(c, i) =>
				`**Conflict ${i + 1}:** ${c.type}\n` +
				`  ${c.description}\n` +
				`  Involved: tasks ${c.involved.join(", ")}\n` +
				`  Resolution: ${c.resolution}`,
		)
		.join("\n\n");
}

function formatSpecVerification(ctx: OpenSpecContext): string {
	const lines = [
		"### Spec Verification",
		"",
		"The following spec scenarios should now be satisfied. **Verify each one:**",
		"",
	];

	for (const ss of ctx.specScenarios) {
		lines.push(`**${ss.domain} → ${ss.requirement}**`);
		for (const scenario of ss.scenarios) {
			// Extract just the scenario name (first line) and the Given/When/Then
			const scenarioLines = scenario.split("\n");
			const name = scenarioLines[0];
			lines.push(`- [ ] ${name}`);
			// Include Given/When/Then as indented detail
			const gwt = scenarioLines.slice(1).filter((l) => l.trim());
			if (gwt.length > 0) {
				for (const l of gwt) {
					lines.push(`      ${l.trim()}`);
				}
			}
		}
		lines.push("");
	}

	if (ctx.apiContract) {
		lines.push(
			"**API Contract Conformance (`api.yaml`)**",
			"- [ ] All contract paths/methods are implemented",
			"- [ ] Request/response schemas match the contract",
			"- [ ] Status codes and error responses match the contract",
			"- [ ] No undocumented endpoints exist outside the contract",
			"",
		);
	}

	lines.push(
		"---",
		"Run tests, inspect the code, or manually verify each scenario above.",
		"If all pass, the change is ready for `/opsx:archive`.",
	);

	return lines.join("\n");
}

// ─── Extension ──────────────────────────────────────────────────────────────

export default function cleaveExtension(pi: ExtensionAPI) {
	// ── Initialize dashboard state ──────────────────────────────────
	emitCleaveState(pi, "idle");

	// ── Agent start: inject OpenSpec status into context ─────────────
	// Uses before_agent_start (not session_start) so the status message
	// enters the agent's conversation context, not just the TUI display.
	let openspecFirstTurn = true;

	pi.on("before_agent_start", (_event, ctx) => {
		if (!openspecFirstTurn) return;
		openspecFirstTurn = false;

		try {
			const status = getActiveChangesStatus(ctx.cwd);
			if (status.length === 0) return;

			const lines = ["**OpenSpec Changes**", ""];
			for (const s of status) {
				const progress = s.totalTasks > 0
					? `${s.doneTasks}/${s.totalTasks} tasks`
					: "no tasks";
				const artifacts: string[] = [];
				if (s.hasProposal) artifacts.push("proposal");
				if (s.hasDesign) artifacts.push("design");
				if (s.hasSpecs) artifacts.push("specs");
				const artStr = artifacts.length > 0 ? ` [${artifacts.join(", ")}]` : "";

				const icon = s.totalTasks > 0 && s.doneTasks >= s.totalTasks ? "✓" : "◦";
				lines.push(`  ${icon} **${s.name}** — ${progress}${artStr}`);
			}

			const incomplete = status.filter((s) => s.totalTasks > 0 && s.doneTasks < s.totalTasks);
			if (incomplete.length > 0) {
				lines.push("", `Use \`/opsx:apply\` to continue or \`/cleave\` to parallelize.`);
			}

			const withTasks = status.filter((s) => s.totalTasks > 0);
			const allDone = withTasks.length > 0 && withTasks.every((s) => s.doneTasks >= s.totalTasks);
			if (allDone) {
				lines.push("", `All tasks complete. Run \`/opsx:verify\` → \`/opsx:archive\` to finalize.`);
			}

			const content = lines.join("\n");
			return {
				message: {
					customType: "openspec-status",
					content,
					display: true,
				},
			};
		} catch {
			// Non-fatal — don't block agent start
		}
	});


	// ── cleave_assess tool ───────────────────────────────────────────────
	pi.registerTool({
		name: "cleave_assess",
		label: "Cleave Assess",
		description:
			"Assess the complexity of a task directive to determine if it should be " +
			"decomposed (cleaved) into subtasks or executed directly. Returns complexity " +
			"score, matched pattern, confidence, and decision (execute/cleave).\n\n" +
			"Use before attempting complex multi-system tasks to decide whether decomposition is warranted.",
		promptSnippet:
			"Assess task complexity for decomposition — returns pattern match, complexity score, and execute/cleave decision",
		promptGuidelines: [
			"Call cleave_assess before starting any multi-system or cross-cutting task to determine if decomposition is needed",
			"If decision is 'execute', proceed directly. If 'cleave', use /cleave to decompose. If 'needs_assessment', proceed directly — it means no pattern matched but the task is likely simple enough for in-session execution.",
			"Complexity formula: (1 + systems) × (1 + 0.5 × modifiers). Threshold default: 2.0.",
			"The /assess command provides code assessment: `/assess cleave` (adversarial review + auto-fix), `/assess diff [ref]` (review only), `/assess spec [change]` (validate against OpenSpec scenarios).",
			"When the repo has openspec/ with active changes, suggest `/assess spec` after implementation and before `/opsx:archive`.",
		],

		parameters: Type.Object({
			directive: Type.String({ description: "The task directive to assess" }),
			threshold: Type.Optional(Type.Number({ description: "Complexity threshold (default: 2.0)" })),
		}),

		async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
			const assessment = assessDirective(params.directive, params.threshold ?? DEFAULT_CONFIG.threshold);
			const text = formatAssessment(assessment);

			return {
				content: [{ type: "text", text }],
				details: {
					...assessment,
					availablePatterns: Object.values(PATTERNS).map((p) => p.name),
				},
			};
		},
	});

	// ── /assess command ──────────────────────────────────────────────────
	const ASSESS_SUBS = [
		{ value: "cleave", label: "cleave", description: "Adversarial review → auto-fix (optional: ref)" },
		{ value: "diff", label: "diff", description: "Assess uncommitted or recent changes for issues" },
		{ value: "spec", label: "spec", description: "Assess implementation against OpenSpec scenarios" },
		{ value: "complexity", label: "complexity", description: "Assess directive complexity (cleave_assess)" },
	];

	pi.registerCommand("assess", {
		description: "Adversarial review + auto-fix (default), or: /assess <diff|spec|complexity> [args]",
		getArgumentCompletions: (prefix: string) => {
			const parts = prefix.split(" ");
			if (parts.length <= 1) {
				// First argument: complete subcommand names
				const partial = parts[0] || "";
				const filtered = ASSESS_SUBS.filter((s) => s.value.startsWith(partial));
				return filtered.length > 0 ? filtered : null;
			}
			// After subcommand, no further completions
			return null;
		},
		handler: async (args, ctx) => {
			const trimmed = (args || "").trim();

			// Bare /assess → adversarial session review (no auto-fix)
			if (!trimmed) {
				pi.sendUserMessage([
					"# Adversarial Assessment",
					"",
					"You are now operating as a hostile reviewer. Your job is to find everything wrong with the work completed in this session. Do not be polite. Do not hedge. If something is broken, say it's broken.",
					"",
					"## Procedure",
					"",
					"1. **Reconstruct scope** — Review the full conversation to identify every change made: files created, files edited, commands run, architectural decisions taken. Build a complete manifest.",
					"",
					"2. **Static analysis** — For every file touched, read the current state and check for:",
					"   - Syntax errors, type mismatches, undefined references",
					"   - Logic errors: off-by-ones, wrong operators, inverted conditions, unreachable branches",
					"   - Unhandled edge cases: nil/null/empty inputs, boundary values, concurrent access",
					"   - Resource leaks: unclosed handles, missing cleanup, unbounded growth",
					"   - Security: injection vectors, hardcoded secrets, insecure defaults, path traversal",
					"   - Dependency issues: missing imports, version conflicts, circular dependencies",
					"",
					"3. **Behavioral analysis** — Trace actual execution paths:",
					"   - Does the happy path work end-to-end?",
					"   - What happens on every error path? Are errors swallowed, misclassified, or leaked?",
					"   - Race conditions, deadlocks, TOCTOU bugs?",
					"   - State consistency across all paths?",
					"",
					"4. **Design critique** — Evaluate structural decisions:",
					"   - Does the solution solve the *actual* problem or a simplified version?",
					"   - Unnecessary abstractions, premature generalizations, gold-plating?",
					"   - Does it violate existing codebase conventions?",
					"   - Will it be maintainable by someone who didn't write it?",
					"",
					"5. **Test coverage** — If tests were written or modified:",
					"   - Do tests assert the right things or just exercise code?",
					"   - Missing negative tests, boundary tests, integration tests?",
					"   - Could tests pass with a broken implementation (tautological)?",
					"   - If no tests were written, should there have been?",
					"",
					"6. **Omission audit** — What was *not* done that should have been:",
					"   - Missing error handling, logging, observability",
					"   - Missing migrations, config changes, documentation",
					"   - Missing cleanup of dead code, stale references",
					"   - Incomplete implementation that was hand-waved",
					"",
					"## Output Format",
					"",
					"### Verdict",
					"One of: `PASS` | `PASS WITH CONCERNS` | `NEEDS REWORK` | `REJECT`",
					"",
					"### Critical Issues",
					"Problems that will cause failures, data loss, or security vulnerabilities. Each with file path, line number, and concrete description.",
					"",
					"### Warnings",
					"Problems that won't immediately break but indicate fragility or future risk.",
					"",
					"### Nitpicks",
					"Style, naming, or structural issues that are suboptimal but functional.",
					"",
					"### Omissions",
					"Things that should exist but don't.",
					"",
					"### What Actually Worked",
					"Brief acknowledgment of what was done correctly.",
					"",
					"---",
					"",
					"Do NOT ask clarifying questions. Do NOT skip files because they're \"probably fine.\" Read everything that was changed. Be thorough. Be specific. Cite line numbers.",
				].join("\n"));
				return;
			}

			const parts = trimmed.split(/\s+/);
			const sub = parts[0] || "";
			const rest = parts.slice(1).join(" ");

			switch (sub) {
				// ── /assess cleave ──────────────────────────────────────
				// Adversarial review of recent work → produce categorized
				// issue list → immediately dispatch cleave to fix everything.
				case "cleave": {
					// Gather context: recent git changes
					// Use user-provided ref or auto-detect a sensible range
					const ref = rest || "";
					let diffStat = "";
					let diffContent = "";
					let recentLog = "";
					let effectiveRef = ref;

					if (!effectiveRef) {
						// Auto-detect: try HEAD~3 first, fall back to lower counts
						// or unstaged diff if the repo is too shallow
						for (const candidate of ["HEAD~3", "HEAD~2", "HEAD~1"]) {
							try {
								const test = await pi.exec("git", ["rev-parse", "--verify", candidate], { cwd: ctx.cwd, timeout: 3_000 });
								if (test.code === 0) { effectiveRef = candidate; break; }
							} catch { /* try next */ }
						}
					}

					if (effectiveRef) {
						try {
							const stat = await pi.exec("git", ["diff", "--stat", effectiveRef], { cwd: ctx.cwd, timeout: 5_000 });
							diffStat = stat.stdout.trim();
							const diff = await pi.exec("git", ["diff", effectiveRef], { cwd: ctx.cwd, timeout: 10_000 });
							diffContent = diff.stdout.slice(0, 30_000); // Cap to avoid blowing context
							const log = await pi.exec("git", ["log", "--oneline", "-10"], { cwd: ctx.cwd, timeout: 5_000 });
							recentLog = log.stdout.trim();
						} catch { /* fall through to unstaged */ }
					}

					// Fall back to unstaged diff
					if (!diffStat && !diffContent) {
						try {
							const stat = await pi.exec("git", ["diff", "--stat"], { cwd: ctx.cwd, timeout: 5_000 });
							diffStat = stat.stdout.trim();
							const diff = await pi.exec("git", ["diff"], { cwd: ctx.cwd, timeout: 10_000 });
							diffContent = diff.stdout.slice(0, 30_000);
							effectiveRef = "unstaged";
						} catch { /* non-git or error — proceed anyway */ }
					}

					if (!diffStat && !diffContent) {
						pi.sendMessage({
							customType: "view",
							content: "No recent changes found. Nothing to assess.",
							display: true,
						});
						return;
					}

					pi.sendMessage({
						customType: "view",
						content: [
							"**Assess → Cleave pipeline starting...**",
							"",
							`Reviewing changes since \`${effectiveRef}\`:`,
							"```",
							diffStat,
							"```",
						].join("\n"),
						display: true,
					});

					pi.sendUserMessage(
						[
							"## Adversarial Review → Auto-Fix Pipeline",
							"",
							"You are doing an adversarial code review of recent changes.",
							"Your job is to find real issues, then fix them automatically.",
							"",
							"### Step 1: Review",
							"",
							"Analyze these recent changes for:",
							"- **Critical bugs**: logic errors, race conditions, missing error handling",
							"- **Warnings**: misleading names, missing edge cases, fragile patterns",
							"- **Nits**: dead code, style inconsistencies (low priority)",
							"",
							"Recent commits:",
							"```",
							recentLog,
							"```",
							"",
							"Diff stat:",
							"```",
							diffStat,
							"```",
							"",
							"Full diff (truncated to 30KB):",
							"```diff",
							diffContent,
							"```",
							"",
							"### Step 2: Categorize",
							"",
							"Present findings as a numbered list grouped by severity:",
							"- **C1, C2...** for critical issues",
							"- **W1, W2...** for warnings",
							"- **N1, N2...** for nits",
							"",
							"### Step 3: Fix",
							"",
							"After presenting the list, **immediately fix all Critical and Warning issues**.",
							"Do NOT wait for confirmation — the user invoked `/assess cleave` which means",
							'"assess and fix in one shot". Work through C and W items systematically.',
							"Nits are optional — fix them if trivial, skip if not.",
							"",
							"After all fixes, run the test suite to verify nothing broke.",
							"Then commit with a conventional commit message summarizing all fixes.",
						].join("\n"),
						{ deliverAs: "followUp" },
					);
					return;
				}

				// ── /assess diff [ref] ─────────────────────────────────
				// Assess a specific diff range for issues (review only, no auto-fix)
				case "diff": {
					const ref = rest || "HEAD~1";
					let diffContent = "";
					let diffStat = "";
					try {
						const stat = await pi.exec("git", ["diff", "--stat", ref], { cwd: ctx.cwd, timeout: 5_000 });
						diffStat = stat.stdout.trim();
						const diff = await pi.exec("git", ["diff", ref], { cwd: ctx.cwd, timeout: 10_000 });
						diffContent = diff.stdout.slice(0, 40_000);
					} catch (e: any) {
						pi.sendMessage({
							customType: "view",
							content: `Failed to get diff for \`${ref}\`: ${e.message}`,
							display: true,
						});
						return;
					}

					if (!diffContent) {
						pi.sendMessage({
							customType: "view",
							content: `No changes found relative to \`${ref}\`.`,
							display: true,
						});
						return;
					}

					pi.sendMessage({
						customType: "view",
						content: [
							`**Assessing diff since \`${ref}\`...**`,
							"```",
							diffStat,
							"```",
						].join("\n"),
						display: true,
					});

					pi.sendUserMessage(
						[
							`## Code Review: diff since \`${ref}\``,
							"",
							"Do an adversarial code review of these changes.",
							"Find bugs, fragile patterns, missing edge cases, and style issues.",
							"",
							"Categorize findings as:",
							"- **C1, C2...** Critical (logic errors, security, data loss)",
							"- **W1, W2...** Warning (fragile, misleading, missing cases)",
							"- **N1, N2...** Nit (style, dead code, minor)",
							"",
							"Diff stat:",
							"```",
							diffStat,
							"```",
							"",
							"```diff",
							diffContent,
							"```",
							"",
							"Present findings only — do NOT fix anything unless I ask.",
						].join("\n"),
						{ deliverAs: "followUp" },
					);
					return;
				}

				// ── /assess spec [change] ──────────────────────────────
				// Review implementation against OpenSpec spec scenarios.
				// Uses Given/When/Then as the assessment criteria.
				case "spec": {
					const repoPath = ctx.cwd;
					const openspecDir = detectOpenSpec(repoPath);
					if (!openspecDir) {
						pi.sendMessage({
							customType: "view",
							content: "No `openspec/` directory found. Nothing to assess against.",
							display: true,
						});
						return;
					}

					const changes = findExecutableChanges(openspecDir);
					if (changes.length === 0) {
						pi.sendMessage({
							customType: "view",
							content: "No OpenSpec changes with tasks.md found.",
							display: true,
						});
						return;
					}

					// If a change name was provided, use it; otherwise pick the
					// one with the most incomplete tasks
					let target = rest
						? changes.find((c) => c.name === rest || c.name.includes(rest))
						: null;

					if (!target) {
						// Auto-select: prefer the most recently modified change
						// (filesystem mtime is a better proxy for "what I'm working on"
						// than task count)
						const status = getActiveChangesStatus(repoPath);
						const withTasks = status.filter((s) => s.totalTasks > 0);

						if (withTasks.length > 0) {
							const byRecency = [...withTasks].sort((a, b) => b.lastModifiedMs - a.lastModifiedMs);
							target = changes.find((c) => c.name === byRecency[0].name) ?? changes[0];
						} else {
							target = changes[0];
						}
					}

					const specCtx = buildOpenSpecContext(target.path);
					if (specCtx.specScenarios.length === 0) {
						pi.sendMessage({
							customType: "view",
							content: `Change \`${target.name}\` has no delta spec scenarios to assess against.\n\nUse \`/assess diff\` for general code review instead.`,
							display: true,
						});
						return;
					}
					const scenarios = specCtx.specScenarios;

					// Build the scenario criteria for the LLM
					const scenarioText = scenarios.map((s) => {
						const scenList = s.scenarios.map((sc) => {
							const lines = sc.split("\n").map((l) => `    ${l}`).join("\n");
							return lines;
						}).join("\n");
						return `**${s.domain} → ${s.requirement}**\n${scenList}`;
					}).join("\n\n");

					// Get recent diff for context
					let diffContent = "";
					try {
						const diff = await pi.exec("git", ["diff", "HEAD~5", "--", "."], { cwd: repoPath, timeout: 10_000 });
						diffContent = diff.stdout.slice(0, 30_000);
					} catch {
						try {
							const diff = await pi.exec("git", ["diff", "--", "."], { cwd: repoPath, timeout: 10_000 });
							diffContent = diff.stdout.slice(0, 30_000);
						} catch { /* proceed without diff */ }
					}

					// Include design decisions if available
					const designContext = specCtx.decisions.length > 0
						? [
							"### Design Decisions",
							"",
							"The implementation should also reflect these decisions from design.md:",
							"",
							...specCtx.decisions.map((d) => `- ${d}`),
							"",
						]
						: [];

					// Include API contract if available
					const apiContractContext = specCtx.apiContract
						? [
							"### API Contract",
							"",
							"The implementation must conform to this OpenAPI/AsyncAPI contract (`api.yaml`).",
							"Verify that:",
							"- All paths/methods defined in the contract are implemented",
							"- Request/response schemas match the contract exactly",
							"- Status codes and error responses match the contract",
							"- Security schemes are applied as specified",
							"- Any endpoint in the code but NOT in the contract is flagged as undocumented",
							"",
							"```yaml",
							specCtx.apiContract.length > 15_000
								? specCtx.apiContract.slice(0, 15_000) + "\n# ... (truncated)"
								: specCtx.apiContract,
							"```",
							"",
						]
						: [];

					pi.sendMessage({
						customType: "view",
						content: [
							`**Spec Assessment: \`${target.name}\`**`,
							"",
							`Evaluating implementation against ${scenarios.length} spec scenarios` +
								(specCtx.decisions.length > 0 ? ` and ${specCtx.decisions.length} design decisions` : "") +
								(specCtx.apiContract ? " and API contract (`api.yaml`)" : "") +
								"...",
						].join("\n"),
						display: true,
					});

					pi.sendUserMessage(
						[
							`## Spec-Driven Assessment: \`${target.name}\``,
							"",
							"Assess whether the current implementation satisfies these OpenSpec scenarios.",
							"For each scenario, determine: **PASS**, **FAIL**, or **UNCLEAR**.",
							...(specCtx.apiContract ? [
								"",
								"Also verify implementation conformance to the API contract.",
							] : []),
							"",
							"### Acceptance Criteria",
							"",
							scenarioText,
							"",
							...designContext,
							...apiContractContext,
							"### Instructions",
							"",
							"1. Read the relevant source files to check each scenario",
							...(specCtx.apiContract ? [
								"   - Also check route definitions, schemas, and status codes against the API contract",
							] : []),
							"2. For each scenario, report:",
							"   - **PASS** — implementation clearly satisfies the Given/When/Then",
							"   - **FAIL** — implementation contradicts or is missing",
							"   - **UNCLEAR** — can't determine without running tests",
							"3. Summarize with a count: N/M scenarios passing",
							"4. For any FAIL items, explain what's wrong and suggest fixes",
							"5. Do NOT auto-fix — this is assessment only",
							...(diffContent ? [
								"",
								"### Recent Changes (for context)",
								"",
								"```diff",
								diffContent,
								"```",
							] : []),
						].join("\n"),
						{ deliverAs: "followUp" },
					);
					return;
				}

				// ── /assess complexity <directive> ─────────────────────
				case "complexity": {
					if (!rest) {
						pi.sendMessage({
							customType: "view",
							content: "Usage: `/assess complexity <directive>`\n\nAssess whether a task should be decomposed or executed directly.",
							display: true,
						});
						return;
					}

					const assessment = assessDirective(rest);
					pi.sendMessage({
						customType: "view",
						content: [
							formatAssessment(assessment),
							"",
							assessment.decision === "cleave"
								? "**→ Decomposition recommended.** Use `/cleave " + rest + "` to proceed."
								: assessment.decision === "execute"
									? "**→ Execute directly.** Task is below complexity threshold."
									: "**→ Manual assessment needed.** No pattern matched.",
						].join("\n"),
						display: true,
					});
					return;
				}

				// ── /assess <freeform> — adversarial review with custom instructions
				default: {
					pi.sendUserMessage([
						"# Adversarial Assessment",
						"",
						"You are now operating as a hostile reviewer. Your job is to find everything wrong with the work completed in this session.",
						"",
						"**User instructions:** " + trimmed,
						"",
						"Follow the user's instructions above for tone and scope, but still perform a thorough review.",
						"Read every file that was changed. Be specific. Cite line numbers.",
						"",
						"## Output Format",
						"",
						"### Verdict",
						"One of: `PASS` | `PASS WITH CONCERNS` | `NEEDS REWORK` | `REJECT`",
						"",
						"### Critical Issues",
						"### Warnings",
						"### Nitpicks",
						"### Omissions",
						"### What Actually Worked",
					].join("\n"));
					return;
				}
			}
		},
	});

	// ── /cleave command ──────────────────────────────────────────────────
	pi.registerCommand("cleave", {
		description: "Recursive task decomposition (usage: /cleave <directive>)",
		handler: async (args, ctx) => {
			const directive = (args || "").trim();

			if (!directive) {
				pi.sendMessage({
					customType: "view",
					content: [
						"**Cleave — Recursive Task Decomposition**",
						"",
						"Usage: `/cleave <directive>`",
						"",
						"Example: `/cleave Implement JWT authentication with refresh tokens`",
						"",
						"The directive will be assessed for complexity. If it exceeds the",
						"threshold, it will be decomposed into 2-4 child tasks executed",
						"in parallel via git worktrees.",
						"",
						"Available patterns: " + Object.values(PATTERNS).map((p) => p.name).join(", "),
					].join("\n"),
					display: true,
				});
				return;
			}

			// Delegate the full workflow to the LLM via a structured prompt.
			// This allows the LLM to handle the interactive confirm gates
			// and adapt to user feedback, while we provide all the mechanical
			// infrastructure via tools.

			const assessment = assessDirective(directive);
			const assessmentText = formatAssessment(assessment);

			if (assessment.decision === "execute" || assessment.decision === "needs_assessment") {
				pi.sendMessage({
					customType: "view",
					content: [
						assessmentText,
						"",
						assessment.decision === "needs_assessment"
							? "**→ Execute directly** — no pattern matched; heuristic suggests in-session execution."
							: "**→ Execute directly** — complexity is below threshold.",
						"Proceeding with the task in-session.",
					].join("\n"),
					display: true,
				});

				// Hand off to the LLM to execute directly
				pi.sendUserMessage(
					`Execute this task directly (cleave assessment says it's simple enough):\n\n${directive}`,
					{ deliverAs: "followUp" },
				);
				return;
			}

			// Task needs cleaving — check for OpenSpec first, then fall back to LLM
			const repoPath = ctx.cwd;

			// ── OpenSpec fast path ─────────────────────────────────────
			const openspecDir = detectOpenSpec(repoPath);
			if (openspecDir) {
				const executableChanges = findExecutableChanges(openspecDir);
				if (executableChanges.length > 0) {
					// Try to find a change whose name matches the directive.
					// Three strategies: exact slug containment, word overlap, partial prefix.
					const directiveSlug = directive.toLowerCase().replace(/[^\w]+/g, "-");
					const directiveWords = new Set(
						directive.toLowerCase().replace(/[^\w\s]/g, "").split(/\s+/).filter((w) => w.length > 2),
					);

					const matched = executableChanges.find((c) => {
						// Strategy 1: slug containment (either direction)
						if (directiveSlug.includes(c.name) || c.name.includes(directiveSlug.slice(0, 20))) return true;
						// Strategy 2: word overlap — change name words appear in directive
						const changeWords = c.name.split("-").filter((w) => w.length > 2);
						const overlap = changeWords.filter((w) => directiveWords.has(w)).length;
						if (changeWords.length > 0 && overlap >= Math.ceil(changeWords.length * 0.5)) return true;
						return false;
					});

					// Only use OpenSpec if we found a matching change — never silently
					// pick an unrelated change
					if (!matched) {
						// No match — mention available changes but fall through to LLM planner
						pi.sendMessage({
							customType: "view",
							content: [
								`OpenSpec changes found but none matched the directive.`,
								`Available: ${executableChanges.map((c) => c.name).join(", ")}`,
								`Falling back to LLM planner.`,
							].join("\n"),
							display: true,
						});
					}
					const change = matched;
					const result = change ? openspecChangeToSplitPlanWithContext(change.path) : null;

					if (result) {
						const { plan, context } = result;
						const planJson = JSON.stringify(plan, null, 2);

						// Report what OpenSpec artifacts we found
						const artifactNotes: string[] = [];
						if (context.designContent) artifactNotes.push(`design.md (${context.decisions.length} decisions, ${context.fileChanges.length} file changes)`);
						if (context.specScenarios.length > 0) artifactNotes.push(`specs (${context.specScenarios.length} scenarios for post-merge verification)`);

						pi.sendMessage({
							customType: "view",
							content: [
								assessmentText,
								"",
								`**→ OpenSpec plan detected** from \`${change.name}/tasks.md\``,
								...(artifactNotes.length > 0 ? [`**Artifacts:** ${artifactNotes.join("; ")}`] : []),
								"",
								`**Rationale:** ${plan.rationale}`,
								`**Children:** ${plan.children.map((c) => c.label).join(", ")}`,
								"",
								"Review the plan and confirm to execute via `cleave_run`.",
							].join("\n"),
							display: true,
						});

						pi.sendUserMessage(
							[
								"## Cleave Decomposition (OpenSpec)",
								"",
								`OpenSpec change \`${change.name}\` provides a pre-built split plan.`,
								"",
								"### Split Plan",
								"",
								"```json",
								planJson,
								"```",
								"",
								"Present this plan to the user for review. After confirmation,",
								`use the \`cleave_run\` tool with this plan_json, the original directive,`,
								`and \`openspec_change_path\` set to \`${change.path}\`.`,
								"",
								"### Original Directive",
								"",
								directive,
							].join("\n"),
							{ deliverAs: "followUp" },
						);
						return;
					}
				}
			}

			// ── LLM planning fallback ──────────────────────────────────
			let repoTree: string;
			try {
				repoTree = await getRepoTree(pi, repoPath);
			} catch {
				repoTree = "(unable to read repo structure)";
			}

			const plannerPrompt = buildPlannerPrompt(directive, repoTree, []);

			pi.sendMessage({
				customType: "view",
				content: [
					assessmentText,
					"",
					"**→ Decomposition needed.** Generating split plan...",
				].join("\n"),
				display: true,
			});

			// Delegate to the LLM to:
			// 1. Generate a split plan (can use ask_local_model or think about it)
			// 2. Present the plan for confirmation
			// 3. Execute via cleave_run tool
			pi.sendUserMessage(
				[
					`## Cleave Decomposition`,
					"",
					`The directive needs decomposition (complexity ${assessment.complexity}, pattern: ${assessment.pattern || "none"}).`,
					"",
					"### Step 1: Generate a split plan",
					"",
					"Use `ask_local_model` with this planning prompt to generate a JSON split plan:",
					"",
					"```",
					plannerPrompt,
					"```",
					"",
					"Parse the JSON response and present the plan to me for review.",
					"",
					"### Step 2: After I confirm",
					"",
					"Use the `cleave_run` tool with the plan to execute the decomposition.",
					"",
					"### Original Directive",
					"",
					directive,
				].join("\n"),
				{ deliverAs: "followUp" },
			);
		},
	});

	// ── cleave_run tool ──────────────────────────────────────────────────
	pi.registerTool({
		name: "cleave_run",
		label: "Cleave Run",
		description:
			"Execute a cleave decomposition plan. Creates git worktrees for each child, " +
			"dispatches child pi processes, harvests results, detects conflicts, and " +
			"merges branches back. Requires a split plan (from cleave_assess + planning).\n\n" +
			"Each child runs in an isolated git worktree on its own branch.",
		promptSnippet:
			"Execute a cleave decomposition plan — parallel child dispatch in git worktrees, conflict detection, merge, and report",
		promptGuidelines: [
			"When an OpenSpec change was used to generate the plan, ALWAYS pass `openspec_change_path` so child tasks get design context and tasks.md is updated on completion.",
			"After cleave_run completes with OpenSpec, follow the Next Steps in the report (typically `/assess spec` → `/opsx:verify` → `/opsx:archive`).",
		],
		parameters: Type.Object({
			directive: Type.String({ description: "The original task directive" }),
			plan_json: Type.String({
				description:
					'JSON string of the split plan: {"children": [{"label": "...", "description": "...", "scope": [...], "depends_on": [...]}], "rationale": "..."}',
			}),
			prefer_local: Type.Optional(
				Type.Boolean({ description: "Use local model for leaf tasks when possible (default: true)" }),
			),
			max_parallel: Type.Optional(
				Type.Number({ description: "Maximum parallel children (default: 4)" }),
			),
			openspec_change_path: Type.Optional(
				Type.String({
					description:
						"Path to an OpenSpec change directory. When provided, child task files are " +
						"enriched with design.md context (architecture decisions, file scope) and " +
						"post-merge verification checks specs against implementation.",
				}),
			),
			review: Type.Optional(
				Type.Boolean({
					description:
						"Enable adversarial review loop after each child completes. " +
						"Runs an opus-tier reviewer that checks for bugs, security issues, " +
						"and spec compliance. Severity-gated fix iterations with churn detection. " +
						"Default: false.",
				}),
			),
			review_max_warning_fixes: Type.Optional(
				Type.Number({
					description: "Maximum fix iterations for warning-level issues (default: 1)",
				}),
			),
			review_max_critical_fixes: Type.Optional(
				Type.Number({
					description: "Maximum fix iterations for critical issues before escalation (default: 2)",
				}),
			),
			review_churn_threshold: Type.Optional(
				Type.Number({
					description: "Fraction of reappearing issues that triggers churn bail (default: 0.5)",
				}),
			),
		}),

		async execute(_toolCallId, params, signal, onUpdate, ctx) {
			// Parse the plan
			emitCleaveState(pi, "assessing");

			let plan: SplitPlan;
			try {
				plan = parsePlanResponse(params.plan_json);
			} catch (e: any) {
				emitCleaveState(pi, "failed");
				throw new Error(`Invalid split plan: ${e.message}`);
			}

			const repoPath = ctx.cwd;
			const maxParallel = params.max_parallel ?? DEFAULT_CONFIG.maxParallel;
			const preferLocal = params.prefer_local ?? DEFAULT_CONFIG.preferLocal;

			emitCleaveState(pi, "planning");

			// ── OPENSPEC CONTEXT ───────────────────────────────────────
			let openspecCtx: OpenSpecContext | null = null;
			if (params.openspec_change_path) {
				try {
					openspecCtx = buildOpenSpecContext(params.openspec_change_path);
				} catch {
					// Non-fatal — proceed without enrichment
				}
			}

			// ── SKILL MATCHING ─────────────────────────────────────────
			// Initialize skills on children (parsePlanResponse may not set them)
			for (const child of plan.children) {
				child.skills = child.skills ?? [];
			}

			// Auto-match skills from scope patterns for children without annotations
			matchSkillsToAllChildren(plan.children);

			// Resolve skill names to absolute SKILL.md paths
			const allSkillNames = new Set(plan.children.flatMap((c) => c.skills));
			const { resolved: resolvedPaths } = resolveSkillPaths([...allSkillNames]);

			// Build per-child skill directive map
			const resolvedSkillMap = new Map<number, SkillDirective[]>();
			for (let i = 0; i < plan.children.length; i++) {
				const child = plan.children[i];
				const directives: SkillDirective[] = [];
				for (const skillName of child.skills) {
					const found = resolvedPaths.find((r) => r.skill === skillName);
					if (found) {
						directives.push({ skill: found.skill, path: found.path });
					}
				}
				resolvedSkillMap.set(i, directives);
			}

			// ── PREFLIGHT ──────────────────────────────────────────────
			await ensureCleanWorktree(pi, repoPath);
			const baseBranch = await getCurrentBranch(pi, repoPath);

			// ── MODEL RESOLUTION ───────────────────────────────────────
			// Determine local model availability (needed for model resolution)
			let localModelAvailable = false;
			let localModel: string | undefined;
			if (preferLocal) {
				try {
					const ollamaResult = await pi.exec("ollama", ["list", "--json"], { timeout: 5_000 });
					if (ollamaResult.code === 0) {
						const models = JSON.parse(ollamaResult.stdout);
						if (Array.isArray(models?.models) && models.models.length > 0) {
							localModel = models.models[0].name;
							localModelAvailable = true;
						}
					}
				} catch {
					// No local model available
				}
			}

			// Resolve execute model for each child
			for (const child of plan.children) {
				child.executeModel = resolveExecuteModel(
					child,
					preferLocal,
					localModelAvailable,
					getPreferredTier,
				);
			}

			// ── INITIALIZE STATE ───────────────────────────────────────
			const state: CleaveState = {
				runId: generateRunId(),
				phase: "dispatch",
				directive: params.directive,
				repoPath,
				baseBranch,
				assessment: assessDirective(params.directive),
				plan,
				children: plan.children.map((c, i) => ({
					childId: i,
					label: c.label,
					dependsOn: c.dependsOn,
					status: "pending" as const,
					branch: `cleave/${i}-${c.label}`,
					backend: c.executeModel === "local" ? "local" as const : "cloud" as const,
					executeModel: c.executeModel,
				})),
				workspacePath: "",
				totalDurationSec: 0,
				createdAt: new Date().toISOString(),
			};

			// Create workspace — pass OpenSpec context and resolved skills to enrich child task files
			const wsPath = initWorkspace(state, plan, repoPath, openspecCtx, resolvedSkillMap);
			state.workspacePath = wsPath;

			// ── CREATE WORKTREES ───────────────────────────────────────
			onUpdate?.({
				content: [{ type: "text", text: "Creating git worktrees..." }],
				details: { phase: "dispatch", children: state.children },
			});

			for (const child of state.children) {
				try {
					const wt = await createWorktree(pi, repoPath, child.label, child.childId, baseBranch);
					child.worktreePath = wt.path;
					child.branch = wt.branch;
				} catch (e: any) {
					child.status = "failed";
					child.error = `Worktree creation failed: ${e.message}`;
				}
			}

			saveState(state);

			// ── DISPATCH ───────────────────────────────────────────────
			// localModel was already resolved in MODEL RESOLUTION section above

			emitCleaveState(pi, "dispatching", state.runId, state.children);

			onUpdate?.({
				content: [{ type: "text", text: `Dispatching ${state.children.length} children...` }],
				details: { phase: "dispatch", children: state.children },
			});

			// ── REVIEW CONFIG ──────────────────────────────────────
			const reviewConfig: ReviewConfig = {
				enabled: params.review ?? DEFAULT_REVIEW_CONFIG.enabled,
				maxWarningFixes: params.review_max_warning_fixes ?? DEFAULT_REVIEW_CONFIG.maxWarningFixes,
				maxCriticalFixes: params.review_max_critical_fixes ?? DEFAULT_REVIEW_CONFIG.maxCriticalFixes,
				churnThreshold: params.review_churn_threshold ?? DEFAULT_REVIEW_CONFIG.churnThreshold,
			};

			await dispatchChildren(
				pi,
				state,
				maxParallel,
				120 * 60 * 1000, // 2 hour timeout per child
				localModel,
				signal ?? undefined,
				(msg) => {
					onUpdate?.({
						content: [{ type: "text", text: msg }],
						details: { phase: "dispatch", children: state.children },
					});
				},
				reviewConfig,
			);

			// ── HARVEST + CONFLICTS ────────────────────────────────────
			emitCleaveState(pi, "merging", state.runId, state.children);

			state.phase = "harvest";
			saveState(state);

			const taskContents = readTaskFiles(wsPath);
			const taskResults = [...taskContents.entries()].map(([id, content]) =>
				parseTaskResult(content, `${id}-task.md`),
			);
			const conflicts = detectConflicts(taskResults);

			// ── MERGE ──────────────────────────────────────────────────
			state.phase = "reunify";
			saveState(state);

			const completedChildren = state.children.filter((c) => c.status === "completed");
			const mergeResults: Array<{ label: string; branch: string; success: boolean; conflicts: string[] }> = [];

			for (const child of completedChildren) {
				const result = await mergeBranch(pi, repoPath, child.branch, baseBranch);
				mergeResults.push({
					label: child.label,
					branch: child.branch,
					success: result.success,
					conflicts: result.conflictFiles,
				});
				// On merge failure, stop merging further children to avoid
				// compounding a partially-merged state
				if (!result.success) break;
			}

			// ── CLEANUP ────────────────────────────────────────────────
			// Only clean up worktrees if all merges succeeded. On merge
			// failure, preserve branches so the user can manually resolve.
			const mergeFailures = mergeResults.filter((m) => !m.success);

			// ── TASK WRITE-BACK ────────────────────────────────────────
			// Mark completed child tasks as [x] done in OpenSpec tasks.md
			let writeBackResult: { updated: number; totalTasks: number; allDone: boolean } | null = null;
			if (params.openspec_change_path && mergeFailures.length === 0) {
				const completedLabels = state.children
					.filter((c) => c.status === "completed")
					.map((c) => c.label);
				try {
					writeBackResult = writeBackTaskCompletion(params.openspec_change_path, completedLabels);
				} catch {
					// Non-fatal — report will note write-back wasn't possible
				}
			}

			// ── SPEC VERIFICATION ──────────────────────────────────────
			// If OpenSpec specs exist, check implementation against scenarios
			let specVerification: string | null = null;
			if (openspecCtx && openspecCtx.specScenarios.length > 0 && mergeFailures.length === 0) {
				specVerification = formatSpecVerification(openspecCtx);
			}
			if (mergeResults.length > 0 && mergeFailures.length === 0) {
				// All merges succeeded — safe to clean up worktrees and branches
				await cleanupWorktrees(pi, repoPath);
			} else if (mergeResults.length === 0) {
				// No merges attempted (e.g., all children misclassified or failed).
				// Preserve branches — they may contain committed work.
				// Only prune worktree directories to reclaim disk space.
				await pruneWorktreeDirs(pi, repoPath);
			} else {
				// Partial merge failure — preserve branches for manual resolution
				await pruneWorktreeDirs(pi, repoPath);
			}

			// ── REPORT ─────────────────────────────────────────────────
			state.phase = "complete";
			state.completedAt = new Date().toISOString();
			state.totalDurationSec = Math.round(
				(new Date(state.completedAt).getTime() - new Date(state.createdAt).getTime()) / 1000,
			);

			const allOk =
				state.children.every((c) => c.status === "completed") &&
				mergeResults.every((m) => m.success) &&
				conflicts.length === 0;

			if (!allOk) state.phase = "failed";
			emitCleaveState(pi, allOk ? "done" : "failed", state.runId, state.children);
			saveState(state);

			// Build report
			const completedCount = state.children.filter((c) => c.status === "completed").length;
			const failedCount = state.children.filter((c) => c.status === "failed").length;

			const reportLines = [
				`## Cleave Report: ${state.runId}`,
				"",
				`**Directive:** ${params.directive}`,
				`**Status:** ${allOk ? "✓ SUCCESS" : "✗ ISSUES DETECTED"}`,
				`**Children:** ${completedCount} completed, ${failedCount} failed of ${state.children.length}`,
				`**Duration:** ${state.totalDurationSec}s`,
				`**Workspace:** \`${wsPath}\``,
				"",
			];

			// Child details
			for (const child of state.children) {
				const icon = child.status === "completed" ? "✓" : child.status === "failed" ? "✗" : "⏳";
				const dur = child.durationSec ? ` (${child.durationSec}s)` : "";
				const reviewNote = child.reviewIterations && child.reviewIterations > 0
					? ` [${child.reviewIterations} review${child.reviewIterations > 1 ? "s" : ""}: ${child.reviewDecision}]`
					: "";
				reportLines.push(`  ${icon} **${child.label}** [${child.backend ?? "cloud"}]${dur}: ${child.status}${reviewNote}`);
				if (child.error) reportLines.push(`    Error: ${child.error}`);
				if (child.reviewEscalationReason) reportLines.push(`    Review: ${child.reviewEscalationReason}`);
			}

			// Conflicts
			if (conflicts.length > 0) {
				reportLines.push("", "### Conflicts", "", formatConflicts(conflicts));
			}

			// Merge results (always show — makes partial merge state explicit)
			if (mergeResults.length > 0) {
				reportLines.push("", "### Merge Results");
				const mergeSuccesses = mergeResults.filter((m) => m.success);
				const notAttempted = completedChildren
					.filter((c) => !mergeResults.some((m) => m.label === c.label))
					.map((c) => c.label);
				for (const m of mergeSuccesses) {
					reportLines.push(`  ✓ ${m.label} merged`);
				}
				for (const m of mergeFailures) {
					reportLines.push(`  ✗ ${m.label}: conflicts in ${m.conflicts.join(", ")}`);
				}
				for (const label of notAttempted) {
					reportLines.push(`  ⏭ ${label}: skipped (earlier merge failed)`);
				}
			}

			// Spec verification (post-merge)
			if (specVerification) {
				reportLines.push("", specVerification);
			}

			// Task write-back status
			if (writeBackResult && writeBackResult.updated > 0) {
				reportLines.push(
					"",
					"### Task Write-Back",
					`  ✓ Marked ${writeBackResult.updated} tasks as done in \`tasks.md\``,
				);
			}

			// Next steps guidance
			if (allOk && params.openspec_change_path) {
				reportLines.push("", "### Next Steps");
				if (writeBackResult?.allDone) {
					reportLines.push(
						"  All tasks complete. Ready to finalize:",
						"  1. Run `/assess spec` to validate implementation against spec scenarios",
						"  2. Run `/opsx:verify` for full verification",
						"  3. Run `/opsx:archive` to merge delta specs and close the change",
					);
				} else {
					reportLines.push(
						"  Some tasks remain. Continue with:",
						"  1. Run `/opsx:apply` to work on remaining tasks",
						"  2. Or run `/cleave` again targeting the unfinished groups",
					);
				}
			} else if (allOk && !params.openspec_change_path) {
				reportLines.push(
					"",
					"### Next Steps",
					"  Run tests and review the merged changes.",
				);
			}

			const rawReport = reportLines.join("\n");
			const truncation = truncateTail(rawReport, {
				maxLines: DEFAULT_MAX_LINES,
				maxBytes: DEFAULT_MAX_BYTES,
			});
			let report = truncation.content;
			if (truncation.truncated) {
				report += `\n\n[Report truncated: ${truncation.outputLines} of ${truncation.totalLines} lines` +
					` (${formatSize(truncation.outputBytes)} of ${formatSize(truncation.totalBytes)})]`;
			}

			return {
				content: [{ type: "text", text: report }],
				details: {
					runId: state.runId,
					success: allOk,
					childrenCompleted: completedCount,
					childrenFailed: failedCount,
					conflictsFound: conflicts.length,
					mergeFailures: mergeFailures.length,
					workspacePath: wsPath,
				},
			};
		},
	});
}

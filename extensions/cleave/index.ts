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

import { StringEnum } from "@mariozechner/pi-ai";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { truncateTail, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, formatSize } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";

import { assessDirective, PATTERNS } from "./assessment.js";
import { detectConflicts, parseTaskResult } from "./conflicts.js";
import { dispatchChildren } from "./dispatcher.js";
import {
	detectOpenSpec,
	findExecutableChanges,
	openspecChangeToSplitPlanWithContext,
	buildOpenSpecContext,
	writeBackTaskCompletion,
	getActiveChangesStatus,
	readSpecScenarios,
	type OpenSpecContext,
} from "./openspec.js";
import { buildPlannerPrompt, getRepoTree, parsePlanResponse } from "./planner.js";
import type { CleaveState, ChildState, SplitPlan } from "./types.js";
import { DEFAULT_CONFIG } from "./types.js";
import { initWorkspace, readTaskFiles, saveState } from "./workspace.js";
import {
	cleanupWorktrees,
	createWorktree,
	ensureCleanWorktree,
	getCurrentBranch,
	mergeBranch,
	pruneWorktreeDirs,
} from "./worktree.js";

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

	lines.push(
		"---",
		"Run tests, inspect the code, or manually verify each scenario above.",
		"If all pass, the change is ready for `/opsx:archive`.",
	);

	return lines.join("\n");
}

// ─── Extension ──────────────────────────────────────────────────────────────

export default function cleaveExtension(pi: ExtensionAPI) {
	// ── Session start: surface active OpenSpec changes ────────────────
	pi.on("session_start", (_event, ctx) => {
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

			pi.sendMessage({
				customType: "view",
				content: lines.join("\n"),
				display: true,
			});
		} catch {
			// Non-fatal — don't block session start
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
			"If decision is 'execute', proceed directly. If 'cleave', use /cleave to decompose.",
			"Complexity formula: (1 + systems) × (1 + 0.5 × modifiers). Threshold default: 2.0.",
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
		{ value: "cleave", label: "cleave", description: "Adversarial review → fix plan → auto-execute" },
		{ value: "diff", label: "diff", description: "Assess uncommitted or recent changes for issues" },
		{ value: "spec", label: "spec", description: "Assess implementation against OpenSpec scenarios" },
		{ value: "complexity", label: "complexity", description: "Assess directive complexity (cleave_assess)" },
	];

	pi.registerCommand("assess", {
		description: "Code assessment toolkit (usage: /assess <cleave|diff|complexity> [args])",
		getArgumentCompletions: (prefix: string) => {
			const filtered = ASSESS_SUBS.filter((s) => s.value.startsWith(prefix));
			return filtered.length > 0 ? filtered : null;
		},
		handler: async (args, ctx) => {
			const parts = (args || "").trim().split(/\s+/);
			const sub = parts[0] || "";
			const rest = parts.slice(1).join(" ");

			switch (sub) {
				// ── /assess cleave ──────────────────────────────────────
				// Adversarial review of recent work → produce categorized
				// issue list → immediately dispatch cleave to fix everything.
				case "cleave": {
					// Gather context: recent git changes
					let diffStat = "";
					let diffContent = "";
					let recentLog = "";
					try {
						const stat = await pi.exec("git", ["diff", "--stat", "HEAD~3"], { cwd: ctx.cwd, timeout: 5_000 });
						diffStat = stat.stdout.trim();
						const diff = await pi.exec("git", ["diff", "HEAD~3"], { cwd: ctx.cwd, timeout: 10_000 });
						diffContent = diff.stdout.slice(0, 30_000); // Cap to avoid blowing context
						const log = await pi.exec("git", ["log", "--oneline", "-10"], { cwd: ctx.cwd, timeout: 5_000 });
						recentLog = log.stdout.trim();
					} catch {
						// Fall back to unstaged diff
						try {
							const stat = await pi.exec("git", ["diff", "--stat"], { cwd: ctx.cwd, timeout: 5_000 });
							diffStat = stat.stdout.trim();
							const diff = await pi.exec("git", ["diff"], { cwd: ctx.cwd, timeout: 10_000 });
							diffContent = diff.stdout.slice(0, 30_000);
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
							`Reviewing changes from last 3 commits:`,
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
						// Auto-select: prefer the change with incomplete tasks
						const status = getActiveChangesStatus(repoPath);
						const incomplete = status
							.filter((s) => s.totalTasks > 0 && s.doneTasks < s.totalTasks)
							.sort((a, b) => (b.totalTasks - b.doneTasks) - (a.totalTasks - a.doneTasks));

						if (incomplete.length > 0) {
							target = changes.find((c) => c.name === incomplete[0].name) ?? changes[0];
						} else {
							target = changes[0];
						}
					}

					const scenarios = readSpecScenarios(target.path);
					if (scenarios.length === 0) {
						pi.sendMessage({
							customType: "view",
							content: `Change \`${target.name}\` has no delta spec scenarios to assess against.\n\nUse \`/assess diff\` for general code review instead.`,
							display: true,
						});
						return;
					}

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

					pi.sendMessage({
						customType: "view",
						content: [
							`**Spec Assessment: \`${target.name}\`**`,
							"",
							`Evaluating implementation against ${scenarios.length} spec scenarios...`,
						].join("\n"),
						display: true,
					});

					pi.sendUserMessage(
						[
							`## Spec-Driven Assessment: \`${target.name}\``,
							"",
							"Assess whether the current implementation satisfies these OpenSpec scenarios.",
							"For each scenario, determine: **PASS**, **FAIL**, or **UNCLEAR**.",
							"",
							"### Acceptance Criteria",
							"",
							scenarioText,
							"",
							"### Instructions",
							"",
							"1. Read the relevant source files to check each scenario",
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

				// ── /assess (no subcommand) ────────────────────────────
				default: {
					// If they typed something that's not a subcommand, treat
					// it as a complexity assessment of the whole string
					if (sub && !ASSESS_SUBS.some((s) => s.value === sub)) {
						const fullDirective = args!.trim();
						const assessment = assessDirective(fullDirective);
						pi.sendMessage({
							customType: "view",
							content: [
								formatAssessment(assessment),
								"",
								assessment.decision === "cleave"
									? "**→ Decomposition recommended.** Use `/cleave " + fullDirective + "` to proceed."
									: assessment.decision === "execute"
										? "**→ Execute directly.** Task is below complexity threshold."
										: "**→ Manual assessment needed.** No pattern matched.",
							].join("\n"),
							display: true,
						});
						return;
					}

					pi.sendMessage({
						customType: "view",
						content: [
							"**Assess — Code Assessment Toolkit**",
							"",
							"| Subcommand | Description |",
							"|---|---|",
							"| `/assess cleave` | Adversarial review of recent work → auto-fix all issues |",
							"| `/assess diff [ref]` | Review changes since ref (default: HEAD~1) — analysis only |",
							"| `/assess spec [change]` | Assess implementation against OpenSpec spec scenarios |",
							"| `/assess complexity <directive>` | Check if a task needs decomposition |",
							"| `/assess <directive>` | Shorthand for `/assess complexity <directive>` |",
							"",
							"**`/assess cleave`** is the power move: reviews the last 3 commits,",
							"finds Critical and Warning issues, then immediately fixes them all",
							"and commits the result.",
							"",
							"**`/assess spec`** validates implementation against OpenSpec Given/When/Then",
							"scenarios from delta specs.",
						].join("\n"),
						display: true,
					});
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
					// Try to find a change whose name matches the directive
					const directiveSlug = directive.toLowerCase().replace(/[^\w]+/g, "-");
					const matched = executableChanges.find((c) =>
						directiveSlug.includes(c.name) || c.name.includes(directiveSlug.slice(0, 20)),
					);

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
		}),

		async execute(_toolCallId, params, signal, onUpdate, ctx) {
			// Parse the plan
			let plan: SplitPlan;
			try {
				plan = parsePlanResponse(params.plan_json);
			} catch (e: any) {
				throw new Error(`Invalid split plan: ${e.message}`);
			}

			const repoPath = ctx.cwd;
			const maxParallel = params.max_parallel ?? DEFAULT_CONFIG.maxParallel;
			const preferLocal = params.prefer_local ?? DEFAULT_CONFIG.preferLocal;

			// ── OPENSPEC CONTEXT ───────────────────────────────────────
			let openspecCtx: OpenSpecContext | null = null;
			if (params.openspec_change_path) {
				try {
					openspecCtx = buildOpenSpecContext(params.openspec_change_path);
				} catch {
					// Non-fatal — proceed without enrichment
				}
			}

			// ── PREFLIGHT ──────────────────────────────────────────────
			await ensureCleanWorktree(pi, repoPath);
			const baseBranch = await getCurrentBranch(pi, repoPath);

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
					backend: preferLocal ? "local" as const : "cloud" as const,
				})),
				workspacePath: "",
				totalDurationSec: 0,
				createdAt: new Date().toISOString(),
			};

			// Create workspace — pass OpenSpec context to enrich child task files
			const wsPath = initWorkspace(state, plan, repoPath, openspecCtx);
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
			// Determine local model if preferLocal
			let localModel: string | undefined;
			if (preferLocal) {
				try {
					// Check for available local models via Ollama
					const ollamaResult = await pi.exec("ollama", ["list", "--json"], { timeout: 5_000 });
					if (ollamaResult.code === 0) {
						// Just use the first available model — the dispatcher will use it
						// for children marked as "local" backend
						const models = JSON.parse(ollamaResult.stdout);
						if (Array.isArray(models?.models) && models.models.length > 0) {
							localModel = models.models[0].name;
						}
					}
				} catch {
					// No local model available — all children go cloud
				}
			}

			onUpdate?.({
				content: [{ type: "text", text: `Dispatching ${state.children.length} children...` }],
				details: { phase: "dispatch", children: state.children },
			});

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
			);

			// ── HARVEST + CONFLICTS ────────────────────────────────────
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
			if (mergeFailures.length === 0) {
				await cleanupWorktrees(pi, repoPath);
			} else {
				// Prune worktree directories (they're copies) but keep the branches
				// for manual conflict resolution
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
				reportLines.push(`  ${icon} **${child.label}** [${child.backend ?? "cloud"}]${dur}: ${child.status}`);
				if (child.error) reportLines.push(`    Error: ${child.error}`);
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

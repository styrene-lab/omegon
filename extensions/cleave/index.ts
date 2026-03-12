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

import type { ExtensionAPI, ExtensionCommandContext, AgentToolUpdateCallback } from "@cwilson613/pi-coding-agent";
import { truncateTail, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, formatSize } from "@cwilson613/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";

import { sharedState, DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import { debug } from "../debug.ts";
import { emitOpenSpecState } from "../openspec/dashboard-state.ts";
import { getSharedBridge, buildSlashCommandResult } from "../lib/slash-command-bridge.ts";
import { buildAssessBridgeResult } from "./bridge.ts";
import {
	assessDirective,
	PATTERNS,
	runDesignStructuralCheck,
	buildDesignAssessmentPrompt,
	parseDesignAssessmentFindings,
	type AssessCompletion,
	type AssessEffect,
	type AssessLifecycleHint,
	type AssessLifecycleOutcome,
	type AssessLifecycleRecord,
	type AssessSpecScenarioResult,
	type AssessSpecSummary,
	type AssessStructuredResult,
	type DesignAssessmentResult,
	type DesignAssessmentFinding,
} from "./assessment.ts";
import { detectConflicts, parseTaskResult } from "./conflicts.ts";
import { emitResolvedBugCandidate } from "./lifecycle-emitter.ts";
import { dispatchChildren, resolveExecuteModel } from "./dispatcher.ts";
import { DEFAULT_REVIEW_CONFIG, type ReviewConfig } from "./review.ts";
import {
	detectOpenSpec,
	findExecutableChanges,
	openspecChangeToSplitPlanWithContext,
	buildOpenSpecContext,
	writeBackTaskCompletion,
	getActiveChangesStatus,
	type OpenSpecContext,
} from "./openspec.ts";
import { buildPlannerPrompt, getRepoTree, parsePlanResponse } from "./planner.ts";
import {
	matchSkillsToAllChildren,
	resolveSkillPaths,
	getPreferredTier,
} from "./skills.ts";
import { discoverGuardrails, runGuardrails, formatGuardrailResults } from "./guardrails.ts";
import type { CleaveState, ChildState, SplitPlan } from "./types.ts";
import { DEFAULT_CONFIG } from "./types.ts";
import {
	buildCheckpointPlan,
	classifyDirtyPaths as classifyPreflightDirtyPaths,
	findIncompleteRuns,
	initWorkspace,
	loadState,
	readTaskFiles,
	saveState,
	type ClassifiedDirtyPath,
	type DirtyTreeClassification as WorkspaceDirtyTreeClassification,
} from "./workspace.ts";
import type { SkillDirective } from "./workspace.ts";
import {
	cleanupWorktrees,
	createWorktree,
	ensureCleanWorktree,
	getCurrentBranch,
	mergeBranch,
	pruneWorktreeDirs,
} from "./worktree.ts";
import { inspectGitState } from "../lib/git-state.ts";

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
		updatedAt: Date.now(),
		children: children?.map((c) => ({
			label: c.label,
			status: mapChildStatus(c.status),
			elapsed: c.durationSec,
		})),
	};
	debug("cleave", "emitState", { status, runId, childCount: children?.length });
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

interface DirtyTreePreflightOptions {
	repoPath: string;
	openspecChangePath?: string;
	onUpdate?: AgentToolUpdateCallback<Record<string, unknown>>;
	ui?: { input?: (prompt: string, initial?: string) => Promise<string | undefined> };
}

const TRANSIENT_CLIPBOARD_ATTACHMENT_PATH =
	/^\/var\/folders\/[A-Za-z0-9_-]+\/[A-Za-z0-9_-]+\/T\/pi-clipboard-[A-Fa-f0-9-]+\.(?:png|jpe?g|gif|webp)$/;

function normalizePreflightInput(response: string | undefined): string | undefined {
	const trimmed = response?.trim();
	if (!trimmed) return undefined;
	if (TRANSIENT_CLIPBOARD_ATTACHMENT_PATH.test(trimmed)) return undefined;
	return trimmed;
}

function formatDirtyTreeSummary(classification: WorkspaceDirtyTreeClassification, suggestedMessage: string | null): string {
	const renderGroup = (title: string, entries: ClassifiedDirtyPath[], empty: string): string[] => [
		title,
		...(entries.length > 0
			? entries.map((entry) => `- [${entry.confidence}] \`${entry.path}\` — ${entry.reason}`)
			: [`- ${empty}`]),
		"",
	];
	const unrelatedOrUnknown = [...classification.unrelated, ...classification.unknown];
	const lines = [
		"### Dirty Tree Preflight",
		"",
		"Cleave requires an explicit preflight decision before worktree creation.",
		"",
		...renderGroup("**Related changes**", classification.related, "none detected"),
		...renderGroup("**Unrelated / unknown changes**", unrelatedOrUnknown, "none detected"),
		...renderGroup("**Volatile artifacts**", classification.volatile, "none detected"),
		"**Actions:** `checkpoint`, `stash-unrelated`, `stash-volatile`, `proceed-without-cleave`, `cancel`",
		...(suggestedMessage ? ["", `Suggested checkpoint commit: \`${suggestedMessage}\``] : []),
	];
	return lines.join("\n");
}

async function stashPaths(pi: ExtensionAPI, repoPath: string, label: string, entries: ClassifiedDirtyPath[]): Promise<void> {
	if (entries.length === 0) return;
	const args = ["stash", "push", "-u", "-m", label, "--", ...entries.map((entry) => entry.path)];
	const result = await pi.exec("git", args, { cwd: repoPath, timeout: 15_000 });
	if (result.code !== 0) throw new Error(result.stderr.trim() || `Failed to stash ${label}`);
}

async function checkpointRelatedChanges(
	pi: ExtensionAPI,
	repoPath: string,
	classification: WorkspaceDirtyTreeClassification,
	checkpointMessage: string | null,
	ui?: { input?: (prompt: string, initial?: string) => Promise<string | undefined> },
): Promise<void> {
	// When the user explicitly chooses "checkpoint", commit all non-volatile dirty
	// files — not just those confidently classified as related. The conservative
	// classification is for automatic decisions; an explicit user choice overrides it.
	const allNonVolatile = [
		...classification.related,
		...classification.unrelated,
		...classification.unknown,
	].map((f) => f.path);

	const filesToCommit = classification.checkpointFiles.length > 0
		? classification.checkpointFiles
		: allNonVolatile;

	if (filesToCommit.length === 0) {
		throw new Error(
			"Checkpoint scope is empty — no dirty files found to commit (only volatile artifacts are dirty). " +
			"Choose a different preflight action.",
		);
	}

	// Patch classification so the rest of the function uses the resolved file list.
	classification = { ...classification, checkpointFiles: filesToCommit };
	if (typeof ui?.input !== "function") {
		throw new Error("Checkpoint requires interactive approval, but input is unavailable.");
	}
	const suggested = checkpointMessage ?? "chore(cleave): checkpoint before cleave";
	const response = normalizePreflightInput(await ui.input(
		[
			`Checkpoint ${classification.checkpointFiles.length} related file(s).`,
			`Press Enter to approve the suggested message, type a custom commit message to approve with edits, or type 'cancel' to decline.`,
			`Suggested message: ${suggested}`,
		].join("\n"),
		suggested,
	));
	if (!response) {
		// Accept the suggested message when the operator confirms with Enter.
	} else if (response.toLowerCase() === "cancel") {
		throw new Error("Checkpoint cancelled before commit approval.");
	}
	const commitMessage = response && response.length > 0 ? response : suggested;
	const addResult = await pi.exec("git", ["add", "--", ...classification.checkpointFiles], { cwd: repoPath, timeout: 15_000 });
	if (addResult.code !== 0) {
		throw new Error(
			`git add failed during checkpoint — ${addResult.stderr.trim() || "unknown error staging checkpoint files"}. ` +
			"The checkpoint was not created. Choose a different preflight action or resolve the staging error first.",
		);
	}
	const commitResult = await pi.exec("git", ["commit", "-m", commitMessage, "--", ...classification.checkpointFiles], {
		cwd: repoPath,
		timeout: 20_000,
	});
	if (commitResult.code !== 0) {
		throw new Error(
			`git commit failed during checkpoint — ${commitResult.stderr.trim() || "unknown error creating checkpoint commit"}. ` +
			"The checkpoint was not created. Resolve the git error and try again.",
		);
	}
}

export async function runDirtyTreePreflight(pi: ExtensionAPI, options: DirtyTreePreflightOptions): Promise<"continue" | "skip_cleave" | "cancelled"> {
	const status = await pi.exec("git", ["status", "--porcelain"], {
		cwd: options.repoPath,
		timeout: 5_000,
	});
	const gitState = inspectGitState(status.stdout);
	if (gitState.entries.length === 0) return "continue";

	const openspecContext = options.openspecChangePath
		? (() => {
			try {
				return buildOpenSpecContext(options.openspecChangePath!);
			} catch {
				return null;
			}
		})()
		: null;
	const changeName = options.openspecChangePath?.replace(/\\/g, "/").split("/").pop() ?? null;
	const classification = classifyPreflightDirtyPaths(gitState.entries.map((entry) => entry.path), {
		changeName,
		openspecContext,
	});
	// Compute initial checkpoint plan for the summary display only.
	// The plan is rebuilt from currentClassification inside the loop on each attempt (C4).
	const initialCheckpointPlan = buildCheckpointPlan(classification, { changeName, openspecContext });
	const summary = formatDirtyTreeSummary(classification, initialCheckpointPlan.message);
	options.onUpdate?.({ content: [{ type: "text", text: summary }], details: { phase: "preflight" } });

	if (gitState.nonVolatile.length === 0) {
		if (classification.volatile.length > 0) {
			await stashPaths(pi, options.repoPath, "cleave-preflight-volatile", classification.volatile);
			options.onUpdate?.({
				content: [{ type: "text", text: "Volatile-only dirty tree detected — stashed volatile artifacts automatically before cleave." }],
				details: { phase: "preflight", autoResolved: "volatile_only_stash" },
			});
		}
		return "continue";
	}

	if (typeof options.ui?.input !== "function") {
		throw new Error(summary + "\n\nInteractive input is unavailable, so cleave cannot resolve the dirty tree automatically.");
	}

	// Mutable classification — refreshed after each checkpoint attempt (C1/W1).
	let currentClassification = classification;

	while (true) {
		const answer = normalizePreflightInput(await options.ui.input(
			"Dirty tree action [checkpoint|stash-unrelated|stash-volatile|proceed-without-cleave|cancel]:",
		))?.toLowerCase();
		try {
			switch (answer) {
				case "checkpoint": {
					// Rebuild the checkpoint plan from the current (possibly refreshed) classification (C4).
					const currentCheckpointPlan = buildCheckpointPlan(currentClassification, { changeName, openspecContext });
					const committedFiles = new Set(currentClassification.checkpointFiles);
					await checkpointRelatedChanges(pi, options.repoPath, currentClassification, currentCheckpointPlan.message, options.ui);
					// Re-verify cleanliness after the checkpoint commit.
					const postCheckpointStatus = await pi.exec("git", ["status", "--porcelain"], {
						cwd: options.repoPath,
						timeout: 5_000,
					});
					const postState = inspectGitState(postCheckpointStatus.stdout);
					if (postState.entries.length === 0) {
						// Tree is clean — checkpoint fully resolved the dirty tree.
						return "continue";
					}

					// Re-derive classification from the post-checkpoint state (C1).
					currentClassification = classifyPreflightDirtyPaths(
						postState.entries.map((e) => e.path),
						{ changeName, openspecContext },
					);

					// C2: If only volatile files remain, auto-stash and continue.
					if (postState.nonVolatile.length === 0 && currentClassification.volatile.length > 0) {
						await stashPaths(pi, options.repoPath, "cleave-preflight-volatile", currentClassification.volatile);
						options.onUpdate?.({
							content: [{ type: "text", text: "Checkpoint succeeded. Remaining volatile artifacts stashed automatically — cleave continuing." }],
							details: { phase: "preflight", autoResolved: "volatile_only_stash" },
						});
						return "continue";
					}

					// Remaining dirty files — emit precise diagnosis (W1: distinguish committed-but-still-dirty vs excluded-from-scope).
					const remainingPaths = postState.entries.map((e) => e.path);
					const diagnosisLines = [
						"Checkpoint committed successfully, but dirty files remain — cleave cannot continue yet:",
						...currentClassification.related.map((f) =>
							committedFiles.has(f.path)
								? `  • ${f.path}  [was committed but remains dirty — file may have been modified after staging or only partially staged]`
								: `  • ${f.path}  [related but excluded from checkpoint scope — confidence too low to commit automatically]`
						),
						...currentClassification.unrelated.map((f) => `  • ${f.path}  [unrelated: ${f.reason}]`),
						...currentClassification.unknown.map((f) => `  • ${f.path}  [unknown — not in change scope, was not checkpointed]`),
						...currentClassification.volatile.map((f) => `  • ${f.path}  [volatile artifact — will be auto-stashed]`),
						"",
						"Choose another preflight action to resolve the remaining files.",
					];
					options.onUpdate?.({
						content: [{ type: "text", text: diagnosisLines.join("\n") }],
						details: { phase: "preflight", postCheckpointDirty: remainingPaths },
					});
					break;
				}
				case "stash-unrelated":
					// C1: Use currentClassification (refreshed after checkpoint) not the stale original.
					await stashPaths(pi, options.repoPath, "cleave-preflight-unrelated", [...currentClassification.unrelated, ...currentClassification.unknown]);
					return "continue";
				case "stash-volatile":
					await stashPaths(pi, options.repoPath, "cleave-preflight-volatile", currentClassification.volatile);
					return "continue";
				case "proceed-without-cleave":
					return "skip_cleave";
				case "cancel":
				case "":
					return "cancelled";
				default:
					options.onUpdate?.({
						content: [{ type: "text", text: "Invalid preflight action. Choose checkpoint, stash-unrelated, stash-volatile, proceed-without-cleave, or cancel." }],
						details: { phase: "preflight" },
					});
			}
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			options.onUpdate?.({
				content: [{ type: "text", text: `Preflight action failed: ${message}` }],
				details: { phase: "preflight" },
			});
		}
	}
}

interface AssessExecutionContext {
	cwd: string;
	bridgeInvocation?: boolean;
	hasUI?: boolean;
	model?: { id?: string };
	waitForIdle?: (() => Promise<void>) | undefined;
}

interface AssessDiffContext {
	ref: string;
	diffStat: string;
	diffContent: string;
	recentLog: string;
}

function makeAssessResult<TData>(input: {
	subcommand: AssessStructuredResult<TData>["subcommand"];
	args: string;
	ok: boolean;
	summary: string;
	humanText: string;
	data: TData;
	effects?: AssessEffect[];
	nextSteps?: string[];
	completion?: AssessCompletion;
	lifecycle?: AssessLifecycleHint;
	lifecycleRecord?: AssessLifecycleRecord;
}): AssessStructuredResult<TData> {
	return {
		command: "assess",
		subcommand: input.subcommand,
		args: input.args,
		ok: input.ok,
		summary: input.summary,
		humanText: input.humanText,
		data: input.data,
		effects: input.effects ?? [],
		nextSteps: input.nextSteps ?? [],
		completion: input.completion,
		lifecycle: input.lifecycle,
		lifecycleRecord: input.lifecycleRecord,
	};
}

async function collectAssessmentSnapshot(pi: ExtensionAPI, cwd: string): Promise<{ gitHead: string | null; fingerprint: string }> {
	let gitHead: string | null = null;
	let status = "";

	try {
		const head = await pi.exec("git", ["rev-parse", "--short", "HEAD"], { cwd, timeout: 5_000 });
		if (head.code === 0) gitHead = head.stdout.trim() || null;
	} catch {
		/* proceed with null gitHead */
	}

	try {
		const diff = await pi.exec("git", ["status", "--short", "--untracked-files=all"], { cwd, timeout: 5_000 });
		if (diff.code === 0) status = diff.stdout.trim();
	} catch {
		/* proceed with empty status */
	}

	const fingerprint = createHash("sha256")
		.update(gitHead ?? "nogit")
		.update("\n")
		.update(status)
		.digest("hex");

	return { gitHead, fingerprint };
}

async function buildLifecycleRecord(
	pi: ExtensionAPI,
	cwd: string,
	options: {
		changeName: string;
		assessmentKind: "spec" | "cleave";
		outcome: AssessLifecycleOutcome;
		recommendedAction: string | null;
		changedFiles?: string[];
		constraints?: string[];
		snapshot?: { gitHead: string | null; fingerprint: string };
	},
): Promise<AssessLifecycleRecord> {
	const snapshot = options.snapshot ?? await collectAssessmentSnapshot(pi, cwd);
	return {
		changeName: options.changeName,
		assessmentKind: options.assessmentKind,
		outcome: options.outcome,
		timestamp: new Date().toISOString(),
		snapshot,
		reconciliation: {
			reopen: options.outcome === "reopen",
			changedFiles: [...new Set((options.changedFiles ?? []).map((file) => file.trim()).filter(Boolean))],
			constraints: [...new Set((options.constraints ?? []).map((constraint) => constraint.trim()).filter(Boolean))],
			recommendedAction: options.recommendedAction,
		},
	};
}

interface AssessSpecAgentResult {
	summary: AssessSpecSummary;
	scenarios: AssessSpecScenarioResult[];
	changedFiles?: string[];
	constraints?: string[];
	overallNotes?: string;
}

interface SpecAssessmentRunnerInput {
	repoPath: string;
	changeName: string;
	scenarioText: string;
	designContext: string[];
	apiContractContext: string[];
	diffContent: string;
	expectedScenarioCount: number;
	modelId?: string;
}

interface SpecAssessmentRunnerOutput {
	assessed: AssessSpecAgentResult;
	snapshot?: { gitHead: string | null; fingerprint: string };
}

interface AssessExecutorOverrides {
	runSpecAssessment?: (input: SpecAssessmentRunnerInput) => Promise<SpecAssessmentRunnerOutput>;
}

function isInteractiveAssessContext(ctx: AssessExecutionContext): ctx is AssessExecutionContext & ExtensionCommandContext {
	return ctx.bridgeInvocation !== true && ctx.hasUI === true && typeof ctx.waitForIdle === "function";
}

function countSpecScenarios(specCtx: OpenSpecContext): number {
	return specCtx.specScenarios.reduce((total, scenarioSet) => total + scenarioSet.scenarios.length, 0);
}

function determineSpecOutcome(summary: AssessSpecSummary): AssessLifecycleOutcome {
	if (summary.fail > 0) return "reopen";
	if (summary.unclear > 0) return "ambiguous";
	return "pass";
}

function normalizeSpecAssessment(payload: AssessSpecAgentResult, expectedTotal: number): AssessSpecAgentResult {
	const scenarios = payload.scenarios.map((scenario) => ({
		...scenario,
		evidence: [...new Set((scenario.evidence ?? []).map((entry) => entry.trim()).filter(Boolean))],
		notes: scenario.notes?.trim() || undefined,
	}));
	const summary: AssessSpecSummary = {
		total: payload.summary.total,
		pass: payload.summary.pass,
		fail: payload.summary.fail,
		unclear: payload.summary.unclear,
	};
	if (summary.total !== expectedTotal || scenarios.length !== expectedTotal) {
		throw new Error(`Assessment returned ${scenarios.length}/${expectedTotal} scenarios.`);
	}
	return {
		summary,
		scenarios,
		changedFiles: [...new Set((payload.changedFiles ?? []).map((entry) => entry.trim()).filter(Boolean))],
		constraints: [...new Set((payload.constraints ?? []).map((entry) => entry.trim()).filter(Boolean))],
		overallNotes: payload.overallNotes?.trim() || undefined,
	};
}

function extractJsonObject(text: string): string | null {
	const fenced = text.match(/```json\s*([\s\S]*?)```/i);
	if (fenced?.[1]) return fenced[1].trim();
	const firstBrace = text.indexOf("{");
	const lastBrace = text.lastIndexOf("}");
	if (firstBrace === -1 || lastBrace === -1 || lastBrace <= firstBrace) return null;
	return text.slice(firstBrace, lastBrace + 1).trim();
}

function extractAssistantText(content: unknown): string {
	if (typeof content === "string") return content.trim();
	if (!Array.isArray(content)) return "";
	return content
		.map((item) => {
			if (typeof item === "string") return item;
			if (!item || typeof item !== "object") return "";
			return typeof (item as { text?: unknown }).text === "string"
				? (item as { text: string }).text
				: "";
		})
		.join("\n")
		.trim();
}

function formatSpecOutcomeLabel(outcome: AssessLifecycleOutcome): string {
	switch (outcome) {
		case "pass":
			return "PASS";
		case "reopen":
			return "REOPEN";
		case "ambiguous":
			return "AMBIGUOUS";
	}
}

function buildSpecAssessmentHumanText(changeName: string, assessed: AssessSpecAgentResult, outcome: AssessLifecycleOutcome): string {
	const lines = [
		`**Spec Assessment Complete: \`${changeName}\`**`,
		"",
		`Outcome: **${formatSpecOutcomeLabel(outcome)}**`,
		`Scenarios: ${assessed.summary.pass}/${assessed.summary.total} pass` +
			(assessed.summary.fail > 0 ? `, ${assessed.summary.fail} fail` : "") +
			(assessed.summary.unclear > 0 ? `, ${assessed.summary.unclear} unclear` : ""),
	];

	for (const scenario of assessed.scenarios) {
		lines.push(
			"",
			`- [${scenario.status}] ${scenario.domain} → ${scenario.requirement}`,
			`  ${scenario.scenario.replace(/\n/g, " ")}`,
			...scenario.evidence.map((entry) => `  Evidence: ${entry}`),
			...(scenario.notes ? [`  Notes: ${scenario.notes}`] : []),
		);
	}

	if (assessed.overallNotes) {
		lines.push("", `Overall notes: ${assessed.overallNotes}`);
	}

	return lines.join("\n");
}

async function runSpecAssessmentSubprocess(
	input: SpecAssessmentRunnerInput,
): Promise<SpecAssessmentRunnerOutput> {
	const prompt = [
		"You are performing a read-only OpenSpec compliance assessment.",
		"Operate in read-only plan mode. Never call edit, write, or any workspace-mutating command.",
		"Inspect the repository and determine whether the implementation satisfies every OpenSpec scenario below.",
		"Return ONLY a JSON object with this exact shape:",
		"{",
		'  "summary": { "total": number, "pass": number, "fail": number, "unclear": number },',
		'  "scenarios": [',
		'    { "domain": string, "requirement": string, "scenario": string, "status": "PASS"|"FAIL"|"UNCLEAR", "evidence": string[], "notes"?: string }',
		"  ],",
		'  "changedFiles": string[],',
		'  "constraints": string[],',
		'  "overallNotes"?: string',
		"}",
		"Rules:",
		`- Emit exactly ${input.expectedScenarioCount} scenario entries.`,
		"- Use FAIL when the code clearly contradicts or omits the scenario.",
		"- Use UNCLEAR only when code inspection cannot safely prove PASS or FAIL.",
		"- Evidence must cite concrete files, symbols, or line references when possible.",
		"- changedFiles should list files that would need modification if the result reopens work.",
		"- constraints should list newly discovered implementation constraints.",
		"- Do not wrap the JSON in explanatory prose.",
		"",
		`Change: ${input.changeName}`,
		"",
		"## Acceptance Criteria",
		"",
		input.scenarioText,
		"",
		...input.designContext,
		...input.apiContractContext,
		...(input.diffContent ? ["### Recent Changes", "", "```diff", input.diffContent, "```", ""] : []),
	].join("\n");

	const args = ["--mode", "json", "--plan", "-p", "--no-session"];
	if (input.modelId) args.push("--model", input.modelId);

	return await new Promise<SpecAssessmentRunnerOutput>((resolve, reject) => {
		const proc = spawn("pi", args, {
			cwd: input.repoPath,
			shell: false,
			stdio: ["pipe", "pipe", "pipe"],
			env: {
				...process.env,
				PI_CHILD: "1",
				TERM: process.env.TERM ?? "dumb",
			},
		});
		let stdout = "";
		let stderr = "";
		let buffer = "";
		let assistantText = "";
		let settled = false;
		const settleReject = (error: Error) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			reject(error);
		};
		const settleResolve = (value: SpecAssessmentRunnerOutput) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			resolve(value);
		};
		const timer = setTimeout(() => {
			proc.kill("SIGTERM");
			setTimeout(() => {
				if (!proc.killed) proc.kill("SIGKILL");
			}, 5_000);
			settleReject(new Error(`Timed out after 120s while assessing ${input.changeName}.`));
		}, 120_000);

		const processLine = (line: string) => {
			if (!line.trim()) return;
			stdout += line + "\n";
			let event: unknown;
			try {
				event = JSON.parse(line);
			} catch {
				return;
			}
			if (!event || typeof event !== "object") return;
			const typed = event as { type?: string; message?: { role?: string; content?: unknown } };
			if (typed.type === "message_end" && typed.message?.role === "assistant") {
				assistantText = extractAssistantText(typed.message.content);
			}
		};

		proc.stdout.on("data", (data) => {
			buffer += data.toString();
			const lines = buffer.split("\n");
			buffer = lines.pop() || "";
			for (const line of lines) processLine(line);
		});
		proc.stderr.on("data", (data) => {
			stderr += data.toString();
		});
		proc.on("error", (error) => {
			settleReject(error);
		});
		proc.on("close", (code) => {
			if (buffer.trim()) processLine(buffer.trim());
			if ((code ?? 1) !== 0) {
				settleReject(new Error(stderr.trim() || `Assessment subprocess exited with code ${code ?? 1}.`));
				return;
			}
			const sourceText = assistantText || stdout;
			const jsonText = extractJsonObject(sourceText);
			if (!jsonText) {
				settleReject(new Error(`Assessment subprocess did not return parseable JSON.\n${stderr || stdout}`));
				return;
			}
			try {
				const parsed = JSON.parse(jsonText) as AssessSpecAgentResult;
				settleResolve({
					assessed: normalizeSpecAssessment(parsed, input.expectedScenarioCount),
				});
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				settleReject(new Error(`Assessment JSON was invalid: ${message}`));
			}
		});

		proc.stdin.write(prompt);
		proc.stdin.end();
	});
}

function applyAssessEffects(pi: ExtensionAPI, result: AssessStructuredResult): void {
	for (const effect of result.effects) {
		if (effect.type === "view") {
			pi.sendMessage({
				customType: "view",
				content: effect.content,
				display: effect.display ?? true,
			});
			continue;
		}
		if (effect.type === "follow_up") {
			pi.sendUserMessage(effect.content, { deliverAs: "followUp" });
		}
	}
}

async function collectAssessDiffContext(
	pi: ExtensionAPI,
	cwd: string,
	ref: string,
	fallbackToUnstaged: boolean,
): Promise<AssessDiffContext | null> {
	let effectiveRef = ref;
	let diffStat = "";
	let diffContent = "";
	let recentLog = "";

	if (!effectiveRef && fallbackToUnstaged) {
		for (const candidate of ["HEAD~3", "HEAD~2", "HEAD~1"]) {
			try {
				const test = await pi.exec("git", ["rev-parse", "--verify", candidate], { cwd, timeout: 3_000 });
				if (test.code === 0) {
					effectiveRef = candidate;
					break;
				}
			} catch {
				/* try next */
			}
		}
	}

	if (effectiveRef) {
		try {
			const stat = await pi.exec("git", ["diff", "--stat", effectiveRef], { cwd, timeout: 5_000 });
			diffStat = stat.stdout.trim();
			const diff = await pi.exec("git", ["diff", effectiveRef], { cwd, timeout: 10_000 });
			diffContent = diff.stdout.slice(0, 40_000);
			const log = await pi.exec("git", ["log", "--oneline", "-10"], { cwd, timeout: 5_000 });
			recentLog = log.stdout.trim();
		} catch {
			/* fall through */
		}
	}

	if (!diffStat && !diffContent && fallbackToUnstaged) {
		try {
			const stat = await pi.exec("git", ["diff", "--stat"], { cwd, timeout: 5_000 });
			diffStat = stat.stdout.trim();
			const diff = await pi.exec("git", ["diff"], { cwd, timeout: 10_000 });
			diffContent = diff.stdout.slice(0, 40_000);
			effectiveRef = "unstaged";
		} catch {
			/* proceed without git diff */
		}
	}

	if (!diffStat && !diffContent) return null;
	return { ref: effectiveRef, diffStat, diffContent, recentLog };
}

function buildGuardrailPreamble(cwd: string): string {
	try {
		const checks = discoverGuardrails(cwd);
		if (checks.length === 0) return "";
		const suite = runGuardrails(cwd, checks);
		return formatGuardrailResults(suite);
	} catch {
		return "";
	}
}

async function executeAssessCleave(
	pi: ExtensionAPI,
	ctx: AssessExecutionContext,
	args: string,
): Promise<AssessStructuredResult> {
	const diffContext = await collectAssessDiffContext(pi, ctx.cwd, args.trim(), true);
	if (!diffContext) {
		return makeAssessResult({
			subcommand: "cleave",
			args,
			ok: false,
			summary: "No recent changes found",
			humanText: "No recent changes found. Nothing to assess.",
			data: { reason: "no_changes" },
			effects: [{ type: "view", content: "No recent changes found. Nothing to assess." }],
		});
	}

	const activeOpenSpec = getActiveChangesStatus(ctx.cwd)
		.filter((status) => status.totalTasks > 0)
		.sort((a, b) => b.lastModifiedMs - a.lastModifiedMs);
	const targetChange = activeOpenSpec[0]?.name;
	const lifecycle = targetChange
		? { changeName: targetChange, assessmentKind: "cleave" as const, outcomes: ["pass", "reopen", "ambiguous"] as const }
		: undefined;
	const postAssessInstruction = targetChange
		? [
			"",
			`After review/fixes/tests, call \`openspec_manage\` with action \`reconcile_after_assess\`, change_name \`${targetChange}\`, assessment_kind \`cleave\`, and outcome:`,
			"- `pass` if all Critical/Warning work is resolved cleanly",
			"- `reopen` if remaining work or follow-up fixes reopen implementation",
			"- `ambiguous` if you cannot safely map reviewer findings back to task state",
			"Include `changed_files` for any follow-up fix files and `constraints` for new implementation constraints discovered during review.",
		]
		: [];
	const guardrailPreamble = buildGuardrailPreamble(ctx.cwd);
	const prompt = [
		"## Adversarial Review → Auto-Fix Pipeline",
		"",
		"You are doing an adversarial code review of recent changes.",
		"Your job is to find real issues, then fix them automatically.",
		"",
		...(guardrailPreamble ? [
			"### Deterministic Analysis",
			"",
			guardrailPreamble,
			"",
			"The above are compiler/linter findings — treat failures as Critical issues.",
			"",
		] : []),
		"### Step 1: Review",
		"",
		"Analyze these recent changes for:",
		"- **Critical bugs**: logic errors, race conditions, missing error handling",
		"- **Warnings**: misleading names, missing edge cases, fragile patterns",
		"- **Nits**: dead code, style inconsistencies (low priority)",
		"",
		"Recent commits:",
		"```",
		diffContext.recentLog,
		"```",
		"",
		"Diff stat:",
		"```",
		diffContext.diffStat,
		"```",
		"",
		"Full diff (truncated to 40KB):",
		"```diff",
		diffContext.diffContent,
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
		...postAssessInstruction,
	].join("\n");
	const lifecycleRecord = targetChange
		? await buildLifecycleRecord(pi, ctx.cwd, {
			changeName: targetChange,
			assessmentKind: "cleave",
			outcome: "ambiguous",
			recommendedAction: `Run openspec_manage reconcile_after_assess ${targetChange} with outcome pass, reopen, or ambiguous after review completes.`,
		})
		: undefined;
	const humanText = [
		"**Assess → Cleave pipeline starting...**",
		"",
		`Reviewing changes since \`${diffContext.ref}\`:`,
		"```",
		diffContext.diffStat,
		"```",
	].join("\n");
	const nextSteps = ["Review findings", "Apply all Critical and Warning fixes", "Run verification and reconcile lifecycle state if needed"];
	return makeAssessResult({
		subcommand: "cleave",
		args,
		ok: true,
		summary: `Prepared adversarial review for ${diffContext.ref}`,
		humanText,
		data: {
			ref: diffContext.ref,
			diffStat: diffContext.diffStat,
			recentLog: diffContext.recentLog,
			hasGuardrails: Boolean(guardrailPreamble),
			reconcileChange: targetChange ?? null,
			snapshot: lifecycleRecord?.snapshot ?? null,
		},
		effects: [
			{ type: "view", content: humanText },
			{ type: "follow_up", content: prompt },
			...(lifecycle ? [{ type: "reconcile_hint" as const, ...lifecycle }] : []),
		],
		nextSteps,
		lifecycle,
		lifecycleRecord,
	});
}

async function executeAssessDiff(
	pi: ExtensionAPI,
	ctx: AssessExecutionContext,
	args: string,
): Promise<AssessStructuredResult> {
	const requestedRef = args.trim() || "HEAD~1";
	const diffContext = await collectAssessDiffContext(pi, ctx.cwd, requestedRef, false);
	if (!diffContext) {
		const humanText = `No changes found relative to \`${requestedRef}\`.`;
		return makeAssessResult({
			subcommand: "diff",
			args,
			ok: false,
			summary: `No diff found for ${requestedRef}`,
			humanText,
			data: { ref: requestedRef, reason: "no_changes" },
			effects: [{ type: "view", content: humanText }],
		});
	}

	const guardrailPreamble = buildGuardrailPreamble(ctx.cwd);
	const humanText = [
		`**Assessing diff since \`${diffContext.ref}\`...**`,
		"```",
		diffContext.diffStat,
		"```",
	].join("\n");
	const prompt = [
		`## Code Review: diff since \`${diffContext.ref}\``,
		"",
		"Do an adversarial code review of these changes.",
		"Find bugs, fragile patterns, missing edge cases, and style issues.",
		"",
		...(guardrailPreamble ? [
			"### Deterministic Analysis",
			"",
			guardrailPreamble,
			"",
			"The above are compiler/linter findings — treat failures as Critical issues.",
			"",
		] : []),
		"Categorize findings as:",
		"- **C1, C2...** Critical (logic errors, security, data loss)",
		"- **W1, W2...** Warning (fragile, misleading, missing cases)",
		"- **N1, N2...** Nit (style, dead code, minor)",
		"",
		"Diff stat:",
		"```",
		diffContext.diffStat,
		"```",
		"",
		"```diff",
		diffContext.diffContent,
		"```",
		"",
		"Present findings only — do NOT fix anything unless I ask.",
	].join("\n");
	return makeAssessResult({
		subcommand: "diff",
		args,
		ok: true,
		summary: `Prepared review for ${diffContext.ref}`,
		humanText,
		data: {
			ref: diffContext.ref,
			diffStat: diffContext.diffStat,
			hasGuardrails: Boolean(guardrailPreamble),
		},
		effects: [
			{ type: "view", content: humanText },
			{ type: "follow_up", content: prompt },
		],
		nextSteps: ["Read the review findings", "Decide whether to fix issues or continue implementation"],
	});
}

async function executeAssessSpec(
	pi: ExtensionAPI,
	ctx: AssessExecutionContext,
	args: string,
	overrides?: AssessExecutorOverrides,
): Promise<AssessStructuredResult> {
	const repoPath = ctx.cwd;
	const openspecDir = detectOpenSpec(repoPath);
	if (!openspecDir) {
		const humanText = "No `openspec/` directory found. Nothing to assess against.";
		return makeAssessResult({
			subcommand: "spec",
			args,
			ok: false,
			summary: "OpenSpec directory not found",
			humanText,
			data: { reason: "openspec_missing" },
			effects: [{ type: "view", content: humanText }],
		});
	}

	const changes = findExecutableChanges(openspecDir);
	if (changes.length === 0) {
		const humanText = "No OpenSpec changes with tasks.md found.";
		return makeAssessResult({
			subcommand: "spec",
			args,
			ok: false,
			summary: "No executable OpenSpec changes found",
			humanText,
			data: { reason: "no_executable_changes" },
			effects: [{ type: "view", content: humanText }],
		});
	}

	const requestedChange = args.trim();
	let target = requestedChange
		? changes.find((change) => change.name === requestedChange || change.name.includes(requestedChange))
		: null;
	if (!target) {
		const status = getActiveChangesStatus(repoPath);
		const withTasks = status.filter((entry) => entry.totalTasks > 0);
		if (withTasks.length > 0) {
			const byRecency = [...withTasks].sort((a, b) => b.lastModifiedMs - a.lastModifiedMs);
			target = changes.find((change) => change.name === byRecency[0].name) ?? changes[0];
		} else {
			target = changes[0];
		}
	}

	const specCtx = buildOpenSpecContext(target.path);
	if (specCtx.specScenarios.length === 0) {
		const humanText = `Change \`${target.name}\` has no delta spec scenarios to assess against.\n\nUse \`/assess diff\` for general code review instead.`;
		return makeAssessResult({
			subcommand: "spec",
			args,
			ok: false,
			summary: `No delta scenarios found for ${target.name}`,
			humanText,
			data: { changeName: target.name, reason: "no_scenarios" },
			effects: [{ type: "view", content: humanText }],
		});
	}

	const scenarioText = specCtx.specScenarios.map((scenarioSet) => {
		const renderedScenarios = scenarioSet.scenarios.map((scenario) => {
			const lines = scenario.split("\n").map((line) => `    ${line}`).join("\n");
			return lines;
		}).join("\n");
		return `**${scenarioSet.domain} → ${scenarioSet.requirement}**\n${renderedScenarios}`;
	}).join("\n\n");

	let diffContent = "";
	try {
		const diff = await pi.exec("git", ["diff", "HEAD~5", "--", "."], { cwd: repoPath, timeout: 10_000 });
		diffContent = diff.stdout.slice(0, 30_000);
	} catch {
		try {
			const diff = await pi.exec("git", ["diff", "--", "."], { cwd: repoPath, timeout: 10_000 });
			diffContent = diff.stdout.slice(0, 30_000);
		} catch {
			/* proceed without diff */
		}
	}

	const designContext = specCtx.decisions.length > 0
		? [
			"### Design Decisions",
			"",
			"The implementation should also reflect these decisions from design.md:",
			"",
			...specCtx.decisions.map((decision) => `- ${decision}`),
			"",
		]
		: [];
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
	const lifecycle: AssessLifecycleHint = {
		changeName: target.name,
		assessmentKind: "spec",
		outcomes: ["pass", "reopen", "ambiguous"],
	};
	const totalScenarioCount = countSpecScenarios(specCtx);
	const introText = [
		`**Spec Assessment: \`${target.name}\`**`,
		"",
		`Evaluating implementation against ${totalScenarioCount} spec scenarios`
			+ (specCtx.decisions.length > 0 ? ` and ${specCtx.decisions.length} design decisions` : "")
			+ (specCtx.apiContract ? " and API contract (`api.yaml`)" : "")
			+ "...",
	].join("\n");
	const prompt = [
		`## Spec-Driven Assessment: \`${target.name}\``,
		"",
		"Assess whether the current implementation satisfies these OpenSpec scenarios.",
		"For each scenario, determine: **PASS**, **FAIL**, or **UNCLEAR**.",
		...(specCtx.apiContract ? ["", "Also verify implementation conformance to the API contract."] : []),
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
		...(specCtx.apiContract ? ["   - Also check route definitions, schemas, and status codes against the API contract"] : []),
		"2. For each scenario, report:",
		"   - **PASS** — implementation clearly satisfies the Given/When/Then",
		"   - **FAIL** — implementation contradicts or is missing",
		"   - **UNCLEAR** — can't determine without running tests",
		"3. Summarize with a count: N/M scenarios passing",
		"4. For any FAIL items, explain what's wrong and suggest fixes",
		"5. Do NOT auto-fix — this is assessment only",
		`6. After the assessment, if the result reopens work or reveals new constraints/file-scope drift, call \`openspec_manage\` with action \`reconcile_after_assess\`, change_name \`${target.name}\`, assessment_kind \`spec\`, and outcome \`reopen\` or \`ambiguous\` as appropriate. If all scenarios pass cleanly, call it with outcome \`pass\` to refresh lifecycle state.`,
		...(diffContent ? ["", "### Recent Changes (for context)", "", "```diff", diffContent, "```"] : []),
	].join("\n");

	if (isInteractiveAssessContext(ctx)) {
		const lifecycleRecord = await buildLifecycleRecord(pi, ctx.cwd, {
			changeName: target.name,
			assessmentKind: "spec",
			outcome: "ambiguous",
			recommendedAction: `Run openspec_manage reconcile_after_assess ${target.name} with outcome pass, reopen, or ambiguous after scenario evaluation completes.`,
		});
		return makeAssessResult({
			subcommand: "spec",
			args,
			ok: true,
			summary: `Prepared spec assessment for ${target.name}`,
			humanText: introText,
			data: {
				changeName: target.name,
				scenarioCount: totalScenarioCount,
				decisionCount: specCtx.decisions.length,
				hasApiContract: Boolean(specCtx.apiContract),
				snapshot: lifecycleRecord.snapshot,
			},
			effects: [
				{ type: "view", content: introText },
				{ type: "follow_up", content: prompt },
				{ type: "reconcile_hint" as const, ...lifecycle },
			],
			nextSteps: ["Assess each scenario", "Reconcile lifecycle state based on the assessment outcome"],
			completion: { completed: false, completedInBand: false, requiresFollowUp: true },
			lifecycle,
			lifecycleRecord,
		});
	}

	const runSpecAssessment = overrides?.runSpecAssessment ?? runSpecAssessmentSubprocess;
	const completed = await runSpecAssessment({
		repoPath,
		changeName: target.name,
		scenarioText,
		designContext,
		apiContractContext,
		diffContent,
		expectedScenarioCount: totalScenarioCount,
		modelId: ctx.model?.id,
	});
	const assessed = normalizeSpecAssessment(completed.assessed, totalScenarioCount);
	const outcome = determineSpecOutcome(assessed.summary);
	const snapshot = completed.snapshot ?? await collectAssessmentSnapshot(pi, ctx.cwd);
	const recommendedAction = `Call openspec_manage reconcile_after_assess for ${target.name} with assessment_kind spec and outcome ${outcome}.`;
	const lifecycleRecord = await buildLifecycleRecord(pi, ctx.cwd, {
		changeName: target.name,
		assessmentKind: "spec",
		outcome,
		recommendedAction,
		changedFiles: assessed.changedFiles,
		constraints: assessed.constraints,
		snapshot,
	});
	const humanText = buildSpecAssessmentHumanText(target.name, assessed, outcome);
	const summary = `Completed spec assessment for ${target.name}: ${assessed.summary.pass}/${assessed.summary.total} pass, ${assessed.summary.fail} fail, ${assessed.summary.unclear} unclear`;
	const nextSteps = [
		`Call openspec_manage reconcile_after_assess for ${target.name} with outcome ${outcome}`,
		...(outcome === "pass" ? [`If archive gates clear, run /opsx:archive ${target.name}`] : ["Address findings before archive"]),
	];
	return makeAssessResult({
		subcommand: "spec",
		args,
		ok: true,
		summary,
		humanText,
		data: {
			changeName: target.name,
			outcome,
			scenarioSummary: assessed.summary,
			scenarios: assessed.scenarios,
			changedFiles: assessed.changedFiles ?? [],
			constraints: assessed.constraints ?? [],
			overallNotes: assessed.overallNotes ?? null,
			snapshot,
			recommendedReconcileOutcome: outcome,
		},
		effects: [{ type: "reconcile_hint" as const, ...lifecycle }],
		nextSteps,
		completion: { completed: true, completedInBand: true, requiresFollowUp: false, outcome },
		lifecycle,
		lifecycleRecord,
	});
}

async function executeAssessComplexity(args: string): Promise<AssessStructuredResult> {
	const directive = args.trim();
	if (!directive) {
		const humanText = "Usage: `/assess complexity <directive>`\n\nAssess whether a task should be decomposed or executed directly.";
		return makeAssessResult({
			subcommand: "complexity",
			args,
			ok: false,
			summary: "Missing directive for complexity assessment",
			humanText,
			data: { reason: "missing_directive" },
			effects: [{ type: "view", content: humanText }],
		});
	}

	const assessment = assessDirective(directive);
	const humanText = [
		formatAssessment(assessment),
		"",
		assessment.decision === "cleave"
			? `**→ Decomposition recommended.** Use \`/cleave ${directive}\` to proceed.`
			: assessment.decision === "execute"
				? "**→ Execute directly.** Task is below complexity threshold."
				: "**→ Manual assessment needed.** No pattern matched.",
	].join("\n");
	return makeAssessResult({
		subcommand: "complexity",
		args,
		ok: true,
		summary: `Complexity decision: ${assessment.decision}`,
		humanText,
		data: assessment,
		effects: [{ type: "view", content: humanText }],
		nextSteps: assessment.decision === "cleave" ? [`Run /cleave ${directive}`] : ["Execute directly"],
	});
}

async function runDesignAssessmentSubprocess(
	repoPath: string,
	nodeId: string,
	modelId?: string,
): Promise<{ findings: DesignAssessmentFinding[]; nodeTitle: string; structuralPass: boolean }> {
	const prompt = [
		"You are performing a read-only design-tree node assessment.",
		"Operate in read-only plan mode. Never call edit, write, or any workspace-mutating command.",
		"",
		`## Task`,
		"",
		`1. Call design_tree with action='node', node_id='${nodeId}' to load the node.`,
		"2. Run the structural pre-check:",
		"   - open_questions must be empty — if not, emit a structural finding for each",
		"   - decisions must have at least one entry — if not, emit a structural finding",
		"   - acceptanceCriteria must have at least one scenario, falsifiability, or constraint — if not, emit a structural finding",
		"3. If structural pre-check fails, output ONLY the JSON result below and stop.",
		"4. Otherwise, evaluate each acceptance criterion against the document body:",
		"   - For each Scenario (Given/When/Then): does the document body address the Then clause?",
		"   - For each Falsifiability condition: is it addressed, ruled out, or acknowledged as a known risk?",
		"   - For each Constraint: is it satisfied by the document content?",
		"",
		"## Output Format",
		"",
		"Output ONLY a single JSON object (no prose, no markdown, no code blocks):",
		"{",
		'  "nodeTitle": "<title from node>",',
		'  "structuralPass": true|false,',
		'  "findings": [',
		'    {"type":"scenario"|"falsifiability"|"constraint"|"structural","index":N,"pass":true|false,"finding":"<reason>"}',
		"  ]",
		"}",
	].join("\n");

	const args = ["--mode", "json", "--plan", "-p", "--no-session"];
	if (modelId) args.push("--model", modelId);

	return await new Promise<{ findings: DesignAssessmentFinding[]; nodeTitle: string; structuralPass: boolean }>(
		(resolve, reject) => {
			const proc = spawn("pi", args, {
				cwd: repoPath,
				shell: false,
				stdio: ["pipe", "pipe", "pipe"],
				env: { ...process.env, PI_CHILD: "1", TERM: process.env.TERM ?? "dumb" },
			});
			let buffer = "";
			let assistantText = "";
			let settled = false;
			const settleReject = (error: Error) => {
				if (settled) return;
				settled = true;
				clearTimeout(timer);
				reject(error);
			};
			const settleResolve = (value: { findings: DesignAssessmentFinding[]; nodeTitle: string; structuralPass: boolean }) => {
				if (settled) return;
				settled = true;
				clearTimeout(timer);
				resolve(value);
			};
			const timer = setTimeout(() => {
				proc.kill("SIGTERM");
				setTimeout(() => { if (!proc.killed) proc.kill("SIGKILL"); }, 5_000);
				settleReject(new Error(`Timed out after 120s while assessing design node ${nodeId}.`));
			}, 120_000);
			const processLine = (line: string) => {
				if (!line.trim()) return;
				let event: unknown;
				try { event = JSON.parse(line); } catch { return; }
				if (!event || typeof event !== "object") return;
				const typed = event as { type?: string; message?: { role?: string; content?: unknown } };
				if (typed.type === "message_end" && typed.message?.role === "assistant") {
					assistantText = extractAssistantText(typed.message.content);
				}
			};
			proc.stdout.on("data", (data) => {
				buffer += (data as Buffer).toString();
				const lines = buffer.split("\n");
				buffer = lines.pop() || "";
				for (const line of lines) processLine(line);
			});
			let stderr = "";
			proc.stderr.on("data", (data) => { stderr += (data as Buffer).toString(); });
			proc.on("error", (error) => settleReject(error));
			proc.on("close", (code) => {
				if (buffer.trim()) processLine(buffer.trim());
				if ((code ?? 1) !== 0) {
					settleReject(new Error(stderr.trim() || `Design assessment subprocess exited with code ${code ?? 1}.`));
					return;
				}
				const jsonText = extractJsonObject(assistantText || buffer);
				if (!jsonText) {
					settleReject(new Error(`Design assessment subprocess did not return parseable JSON.\n${stderr}`));
					return;
				}
				try {
					const parsed = JSON.parse(jsonText) as { nodeTitle?: string; structuralPass?: boolean; findings?: DesignAssessmentFinding[] };
					settleResolve({
						nodeTitle: parsed.nodeTitle ?? nodeId,
						structuralPass: parsed.structuralPass ?? true,
						findings: Array.isArray(parsed.findings) ? parsed.findings : [],
					});
				} catch (err) {
					settleReject(new Error(`Design assessment JSON was invalid: ${String(err)}`));
				}
			});
			proc.stdin.write(prompt + "\n");
			proc.stdin.end();
		},
	);
}

async function executeAssessDesign(
	pi: ExtensionAPI,
	ctx: AssessExecutionContext,
	args: string,
): Promise<AssessStructuredResult> {
	const cwd = ctx.cwd;
	// Resolve: explicit arg → focused node → error
	const nodeId = args.trim() || sharedState.designTree?.focusedNode?.id || null;

	if (!nodeId) {
		const humanText = "Usage: `/assess design <node-id>`\n\nProvide a design-tree node ID, or set a focused node via `design_tree_update` with action 'focus' and run `/assess design` without arguments.";
		return makeAssessResult({
			subcommand: "design",
			args,
			ok: false,
			summary: "Missing node-id for design assessment",
			humanText,
			data: { reason: "missing_node_id" },
			effects: [{ type: "view", content: humanText }],
		});
	}

	// Build the interactive follow-up prompt
	const interactivePrompt = [
		`## Design Assessment: \`${nodeId}\``,
		"",
		"Assess this design-tree node for readiness to be marked as 'decided'.",
		"",
		"### Steps",
		"",
		`1. Call \`design_tree\` with \`action='node'\`, \`node_id='${nodeId}'\` to load the node and its document body.`,
		"2. **Structural pre-check** (fail fast with specific finding per gap):",
		"   - If `open_questions.length > 0`: FAIL — list each unresolved question",
		"   - If `decisions.length === 0`: FAIL — no decisions recorded",
		"   - If `acceptanceCriteria` has no scenarios, falsifiability, or constraints: FAIL — empty acceptance criteria",
		"   - If any structural check fails, stop here and report findings.",
		"3. **Acceptance criteria evaluation** (against the document body):",
		"   - For each **Scenario** (Given/When/Then): does the document body address the Then clause?",
		"   - For each **Falsifiability** condition: is it addressed, ruled out, or noted as a known risk?",
		"   - For each **Constraint**: is it satisfied by the document content?",
		"4. **Write `assessment.json`** to `openspec/design/${nodeId}/assessment.json` with structure:",
		"   ```json",
		`   {"nodeId":"${nodeId}","pass":true|false,"structuralPass":true|false,"findings":[...]}`,
		"   ```",
		"   Each finding: `{\"type\":\"scenario\"|\"falsifiability\"|\"constraint\"|\"structural\",\"index\":N,\"pass\":true|false,\"finding\":\"<reason>\"}`",
		"5. **Report** overall PASS/FAIL with per-finding details.",
		"   - If PASS: suggest `design_tree_update` with `set_status(decided)` for this node.",
		"   - If FAIL: list each failing finding with an actionable fix.",
	].join("\n");

	if (isInteractiveAssessContext(ctx)) {
		const introText = `Running design assessment for node \`${nodeId}\`…`;
		return makeAssessResult({
			subcommand: "design",
			args,
			ok: true,
			summary: `Prepared design assessment for ${nodeId}`,
			humanText: introText,
			data: { nodeId },
			effects: [
				{ type: "view", content: introText },
				{ type: "follow_up", content: interactivePrompt },
			],
			nextSteps: ["Evaluate acceptance criteria", "Write assessment.json", "Set status to decided if pass"],
			completion: { completed: false, completedInBand: false, requiresFollowUp: true },
		});
	}

	// Bridged / subprocess mode
	let subResult: { findings: DesignAssessmentFinding[]; nodeTitle: string; structuralPass: boolean };
	try {
		subResult = await runDesignAssessmentSubprocess(cwd, nodeId, ctx.model?.id);
	} catch (err) {
		const msg = `Design assessment subprocess failed: ${String(err)}`;
		return makeAssessResult({
			subcommand: "design",
			args,
			ok: false,
			summary: msg,
			humanText: msg,
			data: { reason: "subprocess_failed", nodeId },
			effects: [{ type: "view", content: msg }],
		});
	}

	const { findings, nodeTitle, structuralPass } = subResult;
	const overallPass = structuralPass && findings.length > 0 && findings.every((f) => f.pass);

	const result: DesignAssessmentResult = { nodeId, pass: overallPass, structuralPass, findings };

	// Write assessment.json
	await writeDesignAssessment(cwd, nodeId, result);

	// Build human text
	const failFindings = findings.filter((f) => !f.pass);
	const passFindings = findings.filter((f) => f.pass);
	const humanLines: string[] = [
		`## Design Assessment: ${nodeTitle} (${nodeId})`,
		"",
		overallPass
			? `**✅ PASS** — ${passFindings.length}/${findings.length} criteria satisfied. Ready to set status → decided.`
			: structuralPass
				? `**❌ FAIL** — ${failFindings.length}/${findings.length} criteria not satisfied.`
				: "**❌ Structural pre-check failed** — resolve these issues before assessing.",
		"",
	];
	if (failFindings.length > 0) {
		humanLines.push("### Issues to Resolve");
		for (const f of failFindings) humanLines.push(`- [${f.type}#${f.index}] ${f.finding}`);
		humanLines.push("");
	}
	if (passFindings.length > 0) {
		humanLines.push("### Satisfied");
		for (const f of passFindings) humanLines.push(`- ✓ [${f.type}#${f.index}] ${f.finding}`);
	}

	const humanText = humanLines.join("\n");
	const nextSteps = overallPass
		? [`Run design_tree_update with action 'set_status', node_id '${nodeId}', status 'decided'`]
		: failFindings.map((f) => f.finding.split(".")[0] ?? f.finding);

	return makeAssessResult({
		subcommand: "design",
		args,
		ok: overallPass,
		summary: overallPass
			? `Design node '${nodeId}' passed — ready to decide`
			: `Design node '${nodeId}' failed — ${failFindings.length} issue(s) to resolve`,
		humanText,
		data: result,
		effects: [{ type: "view", content: humanText }],
		nextSteps,
	});
}

async function writeDesignAssessment(cwd: string, nodeId: string, result: DesignAssessmentResult): Promise<void> {
	try {
		const { writeFile } = await import("node:fs/promises");
		const { join } = await import("node:path");
		const { existsSync } = await import("node:fs");
		const dir = join(cwd, "openspec", "design", nodeId);
		// Do NOT create the directory — if it doesn't exist the node has no design change
		// scaffolded yet, and creating assessment.json here would trigger the "active not
		// archived" gate on set_status(decided) and implement. Write only if already scaffolded.
		if (!existsSync(dir)) return;
		await writeFile(join(dir, "assessment.json"), JSON.stringify(result, null, 2), "utf8");
	} catch {
		// non-fatal — assessment result still returned to caller
	}
}

export function createAssessStructuredExecutors(pi: ExtensionAPI, overrides?: AssessExecutorOverrides) {
	return {
		cleave: (args: string, ctx: AssessExecutionContext) => executeAssessCleave(pi, ctx, args),
		diff: (args: string, ctx: AssessExecutionContext) => executeAssessDiff(pi, ctx, args),
		spec: (args: string, ctx: AssessExecutionContext) => executeAssessSpec(pi, ctx, args, overrides),
		complexity: (args: string) => executeAssessComplexity(args),
		design: (args: string, ctx: AssessExecutionContext) => executeAssessDesign(pi, ctx, args),
	} as const;
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
			"The /assess command provides code assessment: `/assess cleave` (adversarial review + auto-fix), `/assess diff [ref]` (review only), `/assess spec [change]` (validate against OpenSpec scenarios), `/assess design [node-id]` (evaluate design-tree node readiness before set_status(decided)).",
			"When the repo has openspec/ with active changes, suggest `/assess spec` after implementation and before `/opsx:archive`.",
			"Run `/assess design <node-id>` before calling design_tree_update with set_status(decided) to verify acceptance criteria are satisfied.",
		],

		parameters: Type.Object({
			directive: Type.String({ description: "The task directive to assess" }),
			threshold: Type.Optional(Type.Number({ description: "Complexity threshold (default: 2.0)" })),
		}),

		renderCall(params) {
			const dir = params.directive.length > 60
				? params.directive.slice(0, 57) + "…"
				: params.directive;
			return `◊ assess ${dir}`;
		},

		renderResult(_params, result) {
			if (!result?.details) return undefined;
			const d = result.details as { complexity?: number; decision?: string };
			const decisionIcon =
				d.decision === "execute" ? "✓ execute"
				: d.decision === "cleave" ? "⚡ cleave"
				: "? needs_assessment";
			return `◊ assess complexity=${d.complexity?.toFixed(2) ?? "?"} → ${decisionIcon}`;
		},

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
		{ value: "design", label: "design", description: "Assess design-tree node readiness before set_status(decided)" },
	];
	const assessExecutors = createAssessStructuredExecutors(pi);
	const slashCommandBridge = getSharedBridge();
	const toBridgeAssessResult = (
		bridgedArgs: string[],
		result: AssessStructuredResult,
	): ReturnType<typeof buildSlashCommandResult> => buildAssessBridgeResult(bridgedArgs, result);

	slashCommandBridge.register(pi, {
		name: "assess",
		description: "Adversarial review + auto-fix (default), or: /assess <diff|spec|complexity> [args]",
		bridge: {
			agentCallable: true,
			sideEffectClass: "workspace-write",
			resultContract: "cleave.assess.v1",
			summary: "Lifecycle-safe assessment commands for spec, diff, cleave, and complexity",
		},
		getArgumentCompletions: (prefix: string) => {
			const parts = prefix.split(" ");
			if (parts.length <= 1) {
				const partial = parts[0] || "";
				const filtered = ASSESS_SUBS.filter((s) => s.value.startsWith(partial));
				return filtered.length > 0 ? filtered : null;
			}
			return null;
		},
		structuredExecutor: async (args, ctx) => {
			const trimmed = (args || "").trim();
			if (!trimmed) {
				return buildSlashCommandResult("assess", [], {
					ok: false,
					summary: "/assess requires an explicit bridged subcommand",
					humanText: "Bare /assess remains interactive-only in v1. Use one of: /assess spec, /assess diff, /assess cleave, or /assess complexity.",
					data: { supportedSubcommands: ASSESS_SUBS.map((sub) => sub.value) },
					effects: { sideEffectClass: "workspace-write" },
					nextSteps: ASSESS_SUBS.map((sub) => ({ label: `Run /assess ${sub.value}` })),
				});
			}

			const parts = trimmed.split(/\s+/);
			const sub = parts[0] || "";
			const rest = parts.slice(1).join(" ");
			const assessCtx: AssessExecutionContext = {
				cwd: ctx.cwd,
				bridgeInvocation: (ctx as { bridgeInvocation?: boolean }).bridgeInvocation,
				hasUI: ctx.hasUI,
				model: ctx.model ? { id: ctx.model.id } : undefined,
				waitForIdle: "waitForIdle" in ctx && typeof ctx.waitForIdle === "function"
					? ctx.waitForIdle.bind(ctx)
					: undefined,
			};
			switch (sub) {
				case "cleave":
					return toBridgeAssessResult(parts, await assessExecutors.cleave(rest, assessCtx));
				case "diff":
					return toBridgeAssessResult(parts, await assessExecutors.diff(rest, assessCtx));
				case "spec":
					return toBridgeAssessResult(parts, await assessExecutors.spec(rest, assessCtx));
				case "complexity":
					return toBridgeAssessResult(parts, await assessExecutors.complexity(rest));
				case "design":
					return toBridgeAssessResult(parts, await assessExecutors.design(rest, assessCtx));
				default:
					return buildSlashCommandResult("assess", parts, {
						ok: false,
						summary: `Unsupported bridged /assess target: ${sub}`,
						humanText: `Bridged /assess currently supports only: ${ASSESS_SUBS.map((item) => item.value).join(", ")}. Freeform adversarial review remains interactive-only in v1.`,
						data: { supportedSubcommands: ASSESS_SUBS.map((item) => item.value) },
						effects: { sideEffectClass: "workspace-write" },
						nextSteps: ASSESS_SUBS.map((item) => ({ label: `Run /assess ${item.value}` })),
					});
			}
		},
		interactiveHandler: async (result, args) => {
			const trimmed = (args || "").trim();
			const sub = trimmed.split(/\s+/)[0] || "";
			if (!trimmed || !ASSESS_SUBS.some((item) => item.value === sub)) {
				pi.sendUserMessage([
					"# Adversarial Assessment",
					"",
					trimmed
						? "You are now operating as a hostile reviewer. Your job is to find everything wrong with the work completed in this session."
						: "You are now operating as a hostile reviewer. Your job is to find everything wrong with the work completed in this session. Do not be polite. Do not hedge. If something is broken, say it's broken.",
					...(trimmed ? ["", "**User instructions:** " + trimmed] : []),
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
			const assessResult: AssessStructuredResult = {
				command: "assess",
				subcommand: (result.data as any)?.subcommand ?? "diff",
				args: trimmed,
				ok: result.ok,
				summary: result.summary,
				humanText: result.humanText,
				data: (result.data as any)?.data,
				effects: (result.data as any)?.assessEffects ?? [],
				nextSteps: (result.nextSteps ?? []).map((step) => step.label),
				completion: (result.data as any)?.completion,
				lifecycle: (result.data as any)?.lifecycleHint,
				lifecycleRecord: result.lifecycle as AssessLifecycleRecord | undefined,
			};
			applyAssessEffects(pi, assessResult);
		},
		agentHandler: async (result, args) => {
			const trimmed = (args || "").trim();
			const assessResult: AssessStructuredResult = {
				command: "assess",
				subcommand: (result.data as any)?.subcommand ?? "diff",
				args: trimmed,
				ok: result.ok,
				summary: result.summary,
				humanText: result.humanText,
				data: (result.data as any)?.data,
				effects: (result.data as any)?.assessEffects ?? [],
				nextSteps: (result.nextSteps ?? []).map((step) => step.label),
				completion: (result.data as any)?.completion,
				lifecycle: (result.data as any)?.lifecycleHint,
				lifecycleRecord: result.lifecycle as AssessLifecycleRecord | undefined,
			};
			applyAssessEffects(pi, assessResult);
		},
	});
	pi.registerTool(slashCommandBridge.createToolDefinition());

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
					const change = matched!;
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

	// ── /cleave resume command ────────────────────────────────────────
	/**
	 * Resume an interrupted cleave run.
	 *
	 * When a cleave session is killed before the harvest/merge phase the
	 * workspace state file is left with `phase: "dispatch"` and some children
	 * still `pending`.  This command:
	 *
	 *   1. Finds the most recent interrupted run for the current repo.
	 *   2. Re-dispatches pending children (dispatchChildren skips completed ones).
	 *   3. Runs the harvest/merge phase and cleans up worktrees.
	 *
	 * The full harvest is intentionally the same code path as cleave_run
	 * (minus OpenSpec write-back which requires the original change path).
	 */
	pi.registerCommand("cleave resume", {
		description: "Resume an interrupted cleave run — re-dispatch pending children and complete the harvest/merge phase",
		handler: async (_args, ctx) => {
			const repoPath = ctx.cwd;
			const signal: AbortSignal | undefined = (ctx as any).signal;

			const incomplete = findIncompleteRuns(repoPath);
			if (incomplete.length === 0) {
				pi.sendMessage({
					customType: "view",
					content: "**Cleave Resume** — no interrupted runs found for this repository.",
					display: true,
				});
				return;
			}

			const state = incomplete[0]!;

			const emit = (text: string) => pi.sendMessage({ customType: "view", content: text, display: true });

			const completedBefore = state.children.filter((c) => c.status === "completed").length;
			const pendingChildren = state.children.filter(
				(c) => c.status !== "completed" && c.status !== "failed",
			);

			const header = [
				`**Cleave Resume** — \`${state.runId}\``,
				`Directive: ${state.directive}`,
				`Base branch: \`${state.baseBranch}\``,
				"",
				...state.children.map((c) => {
					const icon = c.status === "completed" ? "✅" : c.status === "failed" ? "❌" : "⏳";
					return `  ${icon} [${c.childId}] \`${c.label}\` — ${c.status}`;
				}),
				"",
				`${completedBefore} already completed, ${pendingChildren.length} to dispatch`,
			].join("\n");
			emit(header);

			// ── Re-dispatch any pending children ──────────────────────
			if (pendingChildren.length > 0) {
				emit(`Resuming dispatch for ${pendingChildren.length} pending child(ren)…`);
				emitCleaveState(pi, "dispatching", state.runId, state.children);
				await dispatchChildren(
					pi,
					state,
					4, // maxParallel
					120 * 60 * 1000,
					undefined,
					signal,
					(msg) => emit(msg),
					DEFAULT_REVIEW_CONFIG,
				);
				saveState(state);
			}

			// ── Harvest + merge ────────────────────────────────────────
			emitCleaveState(pi, "merging", state.runId, state.children);
			state.phase = "harvest";
			saveState(state);

			const taskContents = readTaskFiles(state.workspacePath);
			const taskResults = [...taskContents.entries()].map(([id, content]) =>
				parseTaskResult(content, `${id}-task.md`),
			);
			const conflicts = detectConflicts(taskResults);

			state.phase = "reunify";
			saveState(state);

			const completedChildren2 = state.children.filter((c) => c.status === "completed");
			const mergeResults: Array<{ label: string; branch: string; success: boolean; conflicts: string[] }> = [];

			for (const child of completedChildren2) {
				const result = await mergeBranch(pi, repoPath, child.branch, state.baseBranch);
				mergeResults.push({ label: child.label, branch: child.branch, success: result.success, conflicts: result.conflictFiles });
				if (!result.success) break;
			}

			const mergeFailures = mergeResults.filter((m) => !m.success);

			if (mergeResults.length > 0 && mergeFailures.length === 0) {
				await cleanupWorktrees(pi, repoPath);
			} else {
				await pruneWorktreeDirs(pi, repoPath);
			}

			// ── Finalise state ─────────────────────────────────────────
			const allOk =
				state.children.every((c) => c.status === "completed") &&
				mergeResults.every((m) => m.success) &&
				conflicts.length === 0;

			state.phase = allOk ? "complete" : "failed";
			state.completedAt = new Date().toISOString();
			state.totalDurationSec = Math.round(
				(new Date(state.completedAt).getTime() - new Date(state.createdAt).getTime()) / 1000,
			);
			emitCleaveState(pi, allOk ? "done" : "failed", state.runId, state.children);
			saveState(state);

			const completedCount = state.children.filter((c) => c.status === "completed").length;
			const failedCount = state.children.filter((c) => c.status === "failed").length;

			const report = [
				`## Cleave Resume Report: ${state.runId}`,
				"",
				`**Status:** ${allOk ? "✓ COMPLETE" : "✗ ISSUES"}`,
				`**Children:** ${completedCount} completed, ${failedCount} failed of ${state.children.length}`,
				`**Merges:** ${mergeResults.filter((m) => m.success).length} succeeded, ${mergeFailures.length} failed`,
				conflicts.length > 0 ? `\n${formatConflicts(conflicts)}` : "",
				mergeFailures.length > 0
					? `\n**Merge failures:**\n${mergeFailures.map((m) => `  • \`${m.branch}\`: ${m.conflicts.join(", ") || "unknown error"}`).join("\n")}`
					: "",
			].filter(Boolean).join("\n");
			emit(report);
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
			"When an OpenSpec change was used to generate the plan, ALWAYS pass `openspec_change_path` so child tasks get design context and tasks.md is reconciled on completion.",
			"Treat lifecycle reconciliation as required: after cleave_run, ensure tasks.md, design-tree status, and dashboard-facing progress reflect the merged reality before archive.",
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

		renderCall(params) {
			const plan = (() => {
				try { return JSON.parse(params.plan_json); } catch { return null; }
			})();
			const n = Array.isArray(plan?.children) ? plan.children.length : "?";
			const dir = params.directive.length > 50
				? params.directive.slice(0, 47) + "…"
				: params.directive;
			return `⚡ cleave ${n} children — ${dir}`;
		},

		renderResult(params, result, isPartial) {
			if (isPartial) {
				// Phase-aware child table from details
				const d = result?.details as {
					children?: Array<{ label: string; status: string }>;
					phase?: string;
				} | undefined;
				const children = d?.children ?? [];
				if (children.length === 0) return undefined;
				const done = children.filter((c) => c.status === "completed").length;
				const running = children.filter((c) => c.status === "running").length;
				const failed = children.filter((c) => c.status === "failed").length;
				const total = children.length;
				const rows = children.map((c) => {
					const icon =
						c.status === "completed" ? "✓"
						: c.status === "running" ? "⟳"
						: c.status === "failed" ? "✕"
						: "○";
					return `  ${icon} ${c.label}`;
				});
				const summary = `⚡ cleave ${done + running}/${total} active · ${failed} failed`;
				return [summary, ...rows].join("\n");
			}
			// Final result
			const d = result?.details as {
				success?: boolean;
				childrenCompleted?: number;
				childrenFailed?: number;
			} | undefined;
			const n = (d?.childrenCompleted ?? 0) + (d?.childrenFailed ?? 0);
			const total = n;
			const dir = params.directive.length > 40
				? params.directive.slice(0, 37) + "…"
				: params.directive;
			if (d?.success) {
				return `⚡ cleave ✓ done ${d.childrenCompleted}/${total} merged — ${dir}`;
			}
			return `⚡ cleave ✗ ${d?.childrenFailed ?? "?"} failed of ${total} — ${dir}`;
		},

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
			const toolUi = (ctx as { ui?: { input?: (prompt: string, initial?: string) => Promise<string | undefined> } }).ui;
			const preflightOutcome = await runDirtyTreePreflight(pi, {
				repoPath,
				openspecChangePath: params.openspec_change_path,
				onUpdate,
				ui: typeof toolUi?.input === "function" ? { input: toolUi.input.bind(toolUi) } : undefined,
			});
			if (preflightOutcome === "skip_cleave") {
				const message = "Dirty-tree preflight resolved to proceed without cleave. Worktree creation and dispatch were skipped.";
				emitCleaveState(pi, "idle");
				return {
					content: [{ type: "text", text: message }],
					details: { phase: "preflight", skipped: true, reason: "proceed_without_cleave" },
				};
			}
			if (preflightOutcome === "cancelled") {
				emitCleaveState(pi, "idle");
				return {
					content: [{ type: "text", text: "Cleave cancelled during dirty-tree preflight." }],
					details: { phase: "preflight", cancelled: true },
				};
			}
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
							// Prefer code-optimised models for leaf tasks; fall back in order
							const available = models.models.map((m: { name: string }) => m.name);
							// Code-biased preference from shared registry (extensions/lib/local-models.ts)
							const { PREFERRED_ORDER_CODE: preferredCodeModels } = await import("../lib/local-models.ts");
							localModel =
								preferredCodeModels.find((id) => available.includes(id)) ?? available[0];
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
			const cleaveCandidates = taskResults.flatMap((result) =>
				result.summary
					? emitResolvedBugCandidate(result.summary, result.path)
					: [],
			);

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
			let writeBackResult: { updated: number; totalTasks: number; allDone: boolean; unmatchedLabels: string[] } | null = null;
			if (params.openspec_change_path && mergeFailures.length === 0) {
				const completedLabels = state.children
					.filter((c) => c.status === "completed")
					.map((c) => c.label);
				try {
					writeBackResult = writeBackTaskCompletion(params.openspec_change_path, completedLabels);
					emitOpenSpecState(repoPath, pi);
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
			// ── POST-MERGE GUARDRAILS ──────────────────────────────────
			let guardrailReport: string | null = null;
			if (mergeFailures.length === 0) {
				try {
					const checks = discoverGuardrails(repoPath);
					if (checks.length > 0) {
						const suite = runGuardrails(repoPath, checks);
						if (suite.allPassed) {
							guardrailReport = "### Static Analysis\n\n✅ All deterministic checks passed after merge";
						} else {
							const failures = suite.results.filter((r) => !r.passed);
							const lines = ["### Static Analysis", "", "⚠ **Post-merge regressions detected**", ""];
							for (const f of failures) {
								const capped = f.output.split("\n").slice(0, 20).join("\n");
								lines.push(`**${f.check.name}** (exit ${f.exitCode}, ${f.durationMs}ms):`);
								lines.push("```", capped, "```", "");
							}
							guardrailReport = lines.join("\n");
						}
					}
				} catch { /* non-fatal */ }
			}

			if (cleaveCandidates.length > 0) {
				(sharedState.lifecycleCandidateQueue ??= []).push({
					source: "cleave",
					context: `cleave run ${state.runId} final outcomes`,
					candidates: cleaveCandidates,
				});
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

			// Post-merge guardrail results
			if (guardrailReport) {
				reportLines.push("", guardrailReport);
			}

			// Task write-back status
			if (writeBackResult && writeBackResult.updated > 0) {
				reportLines.push(
					"",
					"### Task Write-Back",
					`  ✓ Marked ${writeBackResult.updated} tasks as done in \`tasks.md\``,
				);
			}
			if (writeBackResult && writeBackResult.unmatchedLabels.length > 0) {
				reportLines.push(
					"",
					"### Lifecycle Reconciliation Warning",
					"  ⚠ Completed cleave work could not be mapped back into `tasks.md` for:",
					...writeBackResult.unmatchedLabels.map((label) => `  - ${label}`),
					"",
					"  tasks.md no longer matches the implementation plan. Reconcile the OpenSpec task groups before archive.",
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

/**
 * OpenSpec Extension
 *
 * The specification layer for spec-and-test-driven development.
 * Manages the OpenSpec lifecycle:
 *
 *   propose → spec → plan → implement → verify → archive
 *
 * Commands:
 *   /opsx:propose <name> <title>  — Create a new change with proposal.md
 *   /opsx:spec <change>           — Generate or edit specs for a change
 *   /opsx:ff <change>             — Fast-forward: scaffold design.md + tasks.md from specs
 *   /opsx:status                  — Show all active changes with lifecycle stage
 *   /opsx:verify <change>         — Check spec verification status
 *   /opsx:archive <change>        — Archive completed change, merge specs to baseline
 *
 * Tools:
 *   openspec_manage               — Agent-callable change lifecycle operations
 */

import type { ExtensionAPI, ExtensionContext } from "@cwilson613/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { StringEnum } from "../lib/typebox-helpers.ts";
import { Text } from "@cwilson613/pi-tui";
import { sciCall, sciLoading, sciOk, sciErr, sciExpanded } from "../lib/sci-ui.ts";
import { sciBanner } from "../lib/sci-ui.ts";
import * as fs from "node:fs";
import * as path from "node:path";
import { getSharedBridge, buildSlashCommandResult, type BridgedSlashCommand, type SlashCommandExecutionContext } from "../lib/slash-command-bridge.ts";
import { shouldRefreshOpenSpecForPath } from "../dashboard/file-watch.ts";

import type { ChangeInfo } from "./types.ts";
import {
	getOpenSpecDir,
	listChanges,
	getChange,
	createChange,
	addSpec,
	archiveChange,
	generateSpecFromProposal,
	parseSpecContent,
	countScenarios,
	summarizeSpecs,
	generateSpecFile,
	computeAssessmentSnapshot,
	readAssessmentRecord,
	writeAssessmentRecord,
	getAssessmentStatus,
	type AssessmentKind,
	type AssessmentOutcome,
	type AssessmentRecord,
	type LifecycleSummary,
} from "./spec.ts";
import { buildLifecycleSummary } from "./lifecycle.ts";
import { transitionDesignNodesOnArchive, resolveBoundDesignNodes } from "./archive-gate.ts";
import { deleteMergedBranches } from "./branch-cleanup.ts";
import { emitOpenSpecState } from "./dashboard-state.ts";
import {
	applyPostAssessReconciliation,
	evaluateLifecycleReconciliation,
	formatReconciliationIssues,
} from "./reconcile.ts";
import { scanDesignDocs } from "../design-tree/tree.ts";
import { emitDesignTreeState } from "../design-tree/dashboard-state.ts";
import { emitArchiveCandidates, emitReconcileCandidates } from "./lifecycle-emitter.ts";
import { sharedState } from "../lib/shared-state.ts";

interface AssessmentState {
	record: AssessmentRecord | null;
	status: "missing" | "current" | "stale";
	reason: string;
}

// ─── Extension ───────────────────────────────────────────────────────────────

export default function openspecExtension(pi: ExtensionAPI): void {
	let openspecWatcher: fs.FSWatcher | null = null;
	let openspecRefreshTimer: NodeJS.Timeout | null = null;

	function scheduleOpenSpecRefresh(cwd: string, filePath?: string): void {
		if (filePath && !shouldRefreshOpenSpecForPath(filePath, cwd)) {
			return;
		}
		if (openspecRefreshTimer) clearTimeout(openspecRefreshTimer);
		openspecRefreshTimer = setTimeout(() => {
			openspecRefreshTimer = null;
			emitOpenSpecState(cwd, pi);
		}, 75);
	}

	function startOpenSpecWatcher(cwd: string): void {
		const dir = path.join(cwd, "openspec");
		if (!fs.existsSync(dir)) return;
		openspecWatcher?.close();
		openspecWatcher = null;
		try {
			openspecWatcher = fs.watch(dir, { recursive: true }, (_eventType, filename) => {
				const filePath = typeof filename === "string" && filename.length > 0
					? path.join(dir, filename)
					: undefined;
				scheduleOpenSpecRefresh(cwd, filePath);
			});
		} catch {
			// Best effort only — unsupported platforms fall back to command/tool-driven emits.
		}
	}

	// ─── Dashboard: emit on session start so dashboard has data immediately ───

	pi.on("session_start", async (_event, ctx) => {
		emitOpenSpecState(ctx.cwd, pi);
		startOpenSpecWatcher(ctx.cwd);
	});

	// ─── Helpers ─────────────────────────────────────────────────────

	function stageIcon(stage: ChangeInfo["stage"]): string {
		switch (stage) {
			case "proposed": return "◌";
			case "specified": return "◐";
			case "planned": return "▸";
			case "implementing": return "⟳";
			case "verifying": return "◉";
			case "archived": return "✓";
		}
	}

	function stageColor(stage: ChangeInfo["stage"]): string {
		switch (stage) {
			case "proposed": return "muted";
			case "specified": return "accent";
			case "planned": return "warning";
			case "implementing": return "accent";
			case "verifying": return "success";
			case "archived": return "dim";
		}
	}

	function formatChangeStatus(c: ChangeInfo): string {
		const progress = c.totalTasks > 0
			? `${c.doneTasks}/${c.totalTasks} tasks`
			: "no tasks";
		const specSummary = c.specs.length > 0
			? ` · ${summarizeSpecs(c.specs)}`
			: "";
		return `${stageIcon(c.stage)} **${c.name}** (${c.stage}) — ${progress}${specSummary}`;
	}

	function nextStepHint(c: ChangeInfo): string {
		switch (c.stage) {
			case "proposed":
				return `Next: \`/opsx:spec ${c.name}\` to add specifications`;
			case "specified":
				return `Next: \`/opsx:ff ${c.name}\` to generate design + tasks, then \`/cleave\``;
			case "planned":
				return `Next: \`/cleave\` to execute tasks in parallel`;
			case "implementing":
				return `Next: Continue implementation or \`/cleave\` remaining tasks`;
			case "verifying":
				return `Next: \`/assess spec ${c.name}\` then \`/opsx:archive ${c.name}\``;
			case "archived":
				return "Complete.";
		}
	}

	function buildReconciliationNextSteps(
		changeName: string,
		assessmentKind: "spec" | "cleave",
		outcome: "pass" | "reopen" | "ambiguous",
	): string[] {
		switch (outcome) {
			case "pass":
				return [
					`Run /opsx:archive ${changeName} if lifecycle artifacts are current`,
					`Optionally run /opsx:verify ${changeName} for an operator-facing verification pass`,
				];
			case "reopen":
				return [
					`Resume implementation for ${changeName} and reconcile any new follow-up task(s) in tasks.md`,
					`Re-run /assess ${assessmentKind} ${changeName} after fixes`,
				];
			case "ambiguous":
				return [
					`Review the assessment summary for ${changeName} and decide whether to reopen work or restate findings structurally`,
					`If changes were made, re-run /assess ${assessmentKind} ${changeName} before archive`,
				];
			default:
				return [];
		}
	}

	async function getAssessmentState(cwd: string, change: ChangeInfo): Promise<AssessmentState> {
		const assessment = getAssessmentStatus(cwd, change.name);
		if (!assessment.record) {
			return {
				record: null,
				status: "missing",
				reason: "No persisted assessment record found for this change.",
			};
		}
		if (!assessment.freshness.current) {
			return {
				record: assessment.record,
				status: "stale",
				reason: "The persisted assessment does not match the current implementation snapshot.",
			};
		}
		return {
			record: assessment.record,
			status: "current",
			reason: "The persisted assessment matches the current implementation snapshot.",
		};
	}

	function formatAssessmentSummary(record: AssessmentRecord): string[] {
		return [
			`Assessment kind: ${record.assessmentKind}`,
			`Outcome: ${record.outcome}`,
			`Timestamp: ${record.timestamp}`,
			`Snapshot: git=${record.snapshot.gitHead ?? "detached"} fingerprint=${record.snapshot.fingerprint ? "present" : "missing"}`,
			`Recommended action: ${record.reconciliation.recommendedAction ?? "none"}`,
			...(record.summary ? [`Summary: ${record.summary}`] : []),
		];
	}

	// getLifecycleSummary is the single shared resolver for all lifecycle surfaces.
	// It is imported from lifecycle.ts so that tests can import and verify the same
	// function is used by both status and get surfaces (not re-implemented locally).
	const getLifecycleSummary = buildLifecycleSummary;

	// ─── Tool: openspec_manage ───────────────────────────────────────

	pi.registerTool({
		name: "openspec_manage",
		label: "Implementation",
		description:
			"Manage Implementation (OpenSpec) changes: create proposals, add specs, generate plans, check status, archive. " +
			"The Implementation layer drives spec-driven development. For tracked changes, use design_tree_update(implement) from a decided node — this tool is for untracked/throwaway changes only.\n\n" +
			"Actions:\n" +
			"- status: List all active changes with lifecycle stage\n" +
			"- get: Get details of a specific change\n" +
			"- propose: Create a new change (name, title, intent required)\n" +
			"- add_spec: Add a spec file to a change (change_name, domain, spec_content required)\n" +
			"- generate_spec: Generate spec from proposal content (change_name, domain required)\n" +
			"- fast_forward: Generate design.md + tasks.md from specs (change_name required)\n" +
			"- archive: Archive a completed change (change_name required)",
		promptSnippet:
			"Manage OpenSpec lifecycle — propose changes, write specs, generate plans, verify, archive",
		promptGuidelines: [
			"⚠️  IMPORTANT: For tracked changes use design_tree_update(implement) from a decided node — /opsx:propose is for untracked/throwaway changes only.",
			"The primary entry point for all tracked work is design_tree_update with action 'implement' on a decided design node, which scaffolds the full change directory automatically.",
			"Before implementing any multi-file change, create an OpenSpec change with a proposal and specs.",
			"Specs define what must be true BEFORE code is written — they are the source of truth for correctness.",
			"Use 'propose' to start an untracked change, 'add_spec' or 'generate_spec' to define requirements with Given/When/Then scenarios.",
			"Use 'fast_forward' to generate design.md and tasks.md from the specs, then `/cleave` to execute.",
			"Treat lifecycle reconciliation as required: after implementation checkpoints, ensure tasks.md and bound design-tree state reflect reality before archive.",
			"After `/assess spec` or `/assess cleave`, call `openspec_manage` with action `reconcile_after_assess` when review reopens work, changes file scope, or uncovers new constraints.",
			"Archive should refuse obviously stale lifecycle state (for example incomplete tasks or no design-tree binding) until reconciliation is done.",
			"After implementation, use `/assess spec` to verify specs are satisfied, then 'archive' to close the change.",
			"The full lifecycle: propose → spec → fast_forward → /cleave → /assess spec → archive",
		],
		parameters: Type.Object({
			action: StringEnum([
				"status", "get", "propose", "add_spec", "generate_spec",
				"fast_forward", "archive", "reconcile_after_assess",
			] as const),
			change_name: Type.Optional(Type.String({ description: "Change name/slug (for get, add_spec, generate_spec, fast_forward, archive, reconcile_after_assess)" })),
			// propose params
			name: Type.Optional(Type.String({ description: "Change name for propose (will be slugified)" })),
			title: Type.Optional(Type.String({ description: "Change title (for propose)" })),
			intent: Type.Optional(Type.String({ description: "Change intent/description (for propose)" })),
			// add_spec params
			domain: Type.Optional(Type.String({ description: "Spec domain name, e.g., 'auth' or 'auth/tokens' (for add_spec, generate_spec)" })),
			spec_content: Type.Optional(Type.String({ description: "Raw spec markdown content (for add_spec)" })),
			// generate_spec context
			decisions: Type.Optional(Type.Array(
				Type.Object({ title: Type.String(), rationale: Type.String() }),
				{ description: "Design decisions to include in generated spec (for generate_spec)" },
			)),
			open_questions: Type.Optional(Type.Array(Type.String(), { description: "Open questions to convert to placeholder requirements (for generate_spec)" })),
			assessment_kind: Type.Optional(StringEnum(["spec", "cleave"] as const)),
			outcome: Type.Optional(StringEnum(["pass", "reopen", "ambiguous"] as const)),
			summary: Type.Optional(Type.String({ description: "Brief operator-facing summary of what assessment found" })),
			changed_files: Type.Optional(Type.Array(Type.String(), { description: "Files touched during follow-up fixes after assessment" })),
			constraints: Type.Optional(Type.Array(Type.String(), { description: "New implementation constraints discovered during assessment" })),
		}),

		async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
			const cwd = ctx.cwd;

			switch (params.action) {
				// ── status ────────────────────────────────────────────
				case "status": {
					const changes = listChanges(cwd);
					if (changes.length === 0) {
						return {
							content: [{
								type: "text",
								text: "No active OpenSpec changes.\n\nUse openspec_manage with action 'propose' to start a new change, " +
									"or `/opsx:propose <name> <title>` interactively.",
							}],
							details: { changes: [] },
						};
					}

					const lines = changes.map((c) => {
						const lifecycle = getLifecycleSummary(cwd, c);
						const verificationLine = lifecycle.verificationSubstate
							? `\n  Verification: ${lifecycle.verificationSubstate}`
							: "";
						const nextLine = lifecycle.nextAction
							? `\n  Next: ${lifecycle.nextAction}`
							: `\n  ${nextStepHint(c)}`;
						return `${formatChangeStatus(c)}${verificationLine}${nextLine}`;
					});

					return {
						content: [{ type: "text", text: lines.join("\n\n") }],
						details: {
							changes: changes.map((c) => {
								const lifecycle = getLifecycleSummary(cwd, c);
								return {
									name: c.name,
									stage: lifecycle.stage,
									verificationStage: lifecycle.stage,
									verificationSubstate: lifecycle.verificationSubstate,
									archiveReady: lifecycle.archiveReady,
									bindingStatus: lifecycle.bindingStatus,
									nextAction: lifecycle.nextAction,
									totalTasks: lifecycle.totalTasks,
									doneTasks: lifecycle.doneTasks,
									specCount: countScenarios(c.specs),
								};
							}),
						},
					};
				}

				// ── get ──────────────────────────────────────────────
				case "get": {
					if (!params.change_name) {
						return { content: [{ type: "text", text: "Error: change_name required" }], details: {}, isError: true };
					}
					const change = getChange(cwd, params.change_name);
					if (!change) {
						return { content: [{ type: "text", text: `Change '${params.change_name}' not found` }], details: {}, isError: true };
					}

					const lines = [
						formatChangeStatus(change),
						"",
						`**Path:** ${change.path}`,
						`**Artifacts:** ${[
							change.hasProposal && "proposal.md",
							change.hasDesign && "design.md",
							change.hasTasks && "tasks.md",
							change.hasSpecs && "specs/",
						].filter(Boolean).join(", ") || "none"}`,
					];

					if (change.specs.length > 0) {
						lines.push("", "**Specs:**");
						for (const spec of change.specs) {
							const reqs = spec.sections.flatMap((s) => s.requirements);
							const scenarios = reqs.flatMap((r) => r.scenarios);
							lines.push(`  - ${spec.domain}: ${reqs.length} requirements, ${scenarios.length} scenarios`);
						}
					}

					const assessmentRecord = readAssessmentRecord(cwd, change.name);
					if (assessmentRecord) {
						lines.push("", "**Assessment:**");
						for (const line of formatAssessmentSummary(assessmentRecord)) {
							lines.push(`  - ${line}`);
						}
					}

					const lifecycle = getLifecycleSummary(cwd, change);
					if (lifecycle.verificationSubstate) {
						lines.push("", `**Verification substate:** ${lifecycle.verificationSubstate}`);
					}

					lines.push("", lifecycle.nextAction ? `Next: ${lifecycle.nextAction}` : nextStepHint(change));

					// Include proposal content if it exists
					if (change.hasProposal) {
						const proposalContent = fs.readFileSync(path.join(change.path, "proposal.md"), "utf-8");
						lines.push("", "--- Proposal ---", "", proposalContent.slice(0, 4000));
					}

					return {
						content: [{ type: "text", text: lines.join("\n") }],
						details: { change: { name: change.name, stage: change.stage, specs: change.specs.length } },
					};
				}

				// ── propose ──────────────────────────────────────────
				case "propose": {
					if (!params.name || !params.title || !params.intent) {
						return { content: [{ type: "text", text: "Error: name, title, and intent required for propose" }], details: {}, isError: true };
					}
					try {
						const result = createChange(cwd, params.name, params.title, params.intent);
						emitOpenSpecState(cwd, pi);
						return {
							content: [{
								type: "text",
								text: `Created OpenSpec change at ${result.changePath}\n\n` +
									`Files: ${result.files.join(", ")}\n\n` +
									`Next: Add specs with \`openspec_manage\` action 'generate_spec' or 'add_spec', ` +
									`or interactively with \`/opsx:spec ${path.basename(result.changePath)}\``,
							}],
							details: { changePath: result.changePath, files: result.files },
						};
					} catch (e) {
						return { content: [{ type: "text", text: `Error: ${(e as Error).message}` }], details: {}, isError: true };
					}
				}

				// ── add_spec ─────────────────────────────────────────
				case "add_spec": {
					if (!params.change_name || !params.domain || !params.spec_content) {
						return { content: [{ type: "text", text: "Error: change_name, domain, and spec_content required" }], details: {}, isError: true };
					}
					const change = getChange(cwd, params.change_name);
					if (!change) {
						return { content: [{ type: "text", text: `Change '${params.change_name}' not found` }], details: {}, isError: true };
					}

					const specPath = addSpec(change.path, params.domain, params.spec_content);
					const sections = parseSpecContent(params.spec_content);
					const scenarioCount = sections.flatMap(
						(s) => s.requirements.flatMap((r) => r.scenarios),
					).length;

					emitOpenSpecState(cwd, pi);
					return {
						content: [{
							type: "text",
							text: `Added spec: ${specPath}\n\n` +
								`Parsed: ${sections.length} section(s), ${scenarioCount} scenario(s)\n\n` +
								`Next: Add more specs or use \`/opsx:ff ${params.change_name}\` to generate tasks.`,
						}],
						details: { specPath, sections: sections.length, scenarios: scenarioCount },
					};
				}

				// ── generate_spec ────────────────────────────────────
				case "generate_spec": {
					if (!params.change_name || !params.domain) {
						return { content: [{ type: "text", text: "Error: change_name and domain required" }], details: {}, isError: true };
					}
					const change = getChange(cwd, params.change_name);
					if (!change) {
						return { content: [{ type: "text", text: `Change '${params.change_name}' not found` }], details: {}, isError: true };
					}

					// Read proposal for context
					let proposalContent = "";
					if (change.hasProposal) {
						proposalContent = fs.readFileSync(path.join(change.path, "proposal.md"), "utf-8");
					}

					const specContent = generateSpecFromProposal({
						domain: params.domain,
						proposalContent,
						decisions: params.decisions,
						openQuestions: params.open_questions,
					});

					const specPath = addSpec(change.path, params.domain, specContent);

					emitOpenSpecState(cwd, pi);
					return {
						content: [{
							type: "text",
							text: `Generated spec: ${specPath}\n\n` +
								`**This is a scaffold — refine the Given/When/Then scenarios before proceeding.**\n\n` +
								`The generated scenarios are placeholders. Edit them to be specific and testable.\n\n` +
								`Next: Review and refine specs, then \`/opsx:ff ${params.change_name}\` to generate tasks.`,
						}],
						details: { specPath, generated: true },
					};
				}

				// ── fast_forward ─────────────────────────────────────
				case "fast_forward": {
					if (!params.change_name) {
						return { content: [{ type: "text", text: "Error: change_name required" }], details: {}, isError: true };
					}
					const change = getChange(cwd, params.change_name);
					if (!change) {
						return { content: [{ type: "text", text: `Change '${params.change_name}' not found` }], details: {}, isError: true };
					}
					if (!change.hasSpecs && !change.hasProposal) {
						return {
							content: [{ type: "text", text: "Change has no specs or proposal. Add specs first with 'add_spec' or 'generate_spec'." }],
							details: {},
							isError: true,
						};
					}

					const files: string[] = [];

					// Generate design.md if not present
					if (!change.hasDesign) {
						const designLines = [`# ${change.name} — Design`, ""];

						if (change.specs.length > 0) {
							designLines.push("## Spec-Derived Architecture", "");
							for (const spec of change.specs) {
								designLines.push(`### ${spec.domain}`, "");
								for (const section of spec.sections) {
									if (section.type === "removed") continue;
									for (const req of section.requirements) {
										designLines.push(`- **${req.title}** (${section.type}) — ${req.scenarios.length} scenarios`);
									}
								}
								designLines.push("");
							}
						}

						// Read proposal for additional context
						if (change.hasProposal) {
							const proposal = fs.readFileSync(path.join(change.path, "proposal.md"), "utf-8");
							const scopeMatch = proposal.match(/##\s+Scope\s*\n([\s\S]*?)(?=\n##\s|$)/i);
							if (scopeMatch) {
								designLines.push("## Scope", "", scopeMatch[1].trim(), "");
							}
						}

						designLines.push("## File Changes", "");
						designLines.push("<!-- Add file changes as you design the implementation -->", "");

						fs.writeFileSync(path.join(change.path, "design.md"), designLines.join("\n"));
						files.push("design.md");
					}

					// Generate tasks.md if not present
					if (!change.hasTasks) {
						const taskLines = [`# ${change.name} — Tasks`, ""];

						if (change.specs.length > 0) {
							// Generate task groups from spec domains/requirements
							let groupNum = 1;
							for (const spec of change.specs) {
								for (const section of spec.sections) {
									if (section.type === "removed") continue;
									for (const req of section.requirements) {
										taskLines.push(`## ${groupNum}. ${req.title}`, "");
										// Each scenario becomes a task
										let taskNum = 1;
										for (const s of req.scenarios) {
											taskLines.push(`- [ ] ${groupNum}.${taskNum} ${s.title}`);
											taskNum++;
										}
										// Add a verification task
										taskLines.push(`- [ ] ${groupNum}.${taskNum} Write tests for ${req.title}`);
										taskLines.push("");
										groupNum++;
									}
								}
							}
						} else {
							taskLines.push("## 1. Implementation", "");
							taskLines.push("- [ ] 1.1 Implement the proposed change", "");
						}

						fs.writeFileSync(path.join(change.path, "tasks.md"), taskLines.join("\n"));
						files.push("tasks.md");
					}

					if (files.length === 0) {
						return {
							content: [{ type: "text", text: `design.md and tasks.md already exist for '${change.name}'. Delete them to regenerate.` }],
							details: {},
						};
					}

					emitOpenSpecState(cwd, pi);
					return {
						content: [{
							type: "text",
							text: `Fast-forwarded '${change.name}': generated ${files.join(", ")}\n\n` +
								`Next: Review the generated files, then \`/cleave\` to execute tasks in parallel.`,
						}],
						details: { files },
					};
				}

				// ── reconcile_after_assess ──────────────────────────
				case "reconcile_after_assess": {
					if (!params.change_name || !params.assessment_kind || !params.outcome) {
						return {
							content: [{ type: "text", text: "Error: change_name, assessment_kind, and outcome required" }],
							details: {},
							isError: true,
						};
					}

					const change = getChange(cwd, params.change_name);
					const result = applyPostAssessReconciliation(cwd, params.change_name, {
						assessmentKind: params.assessment_kind,
						outcome: params.outcome,
						summary: params.summary,
						changedFiles: params.changed_files,
						constraints: params.constraints,
					});
					const snapshot = change ? computeAssessmentSnapshot(cwd, params.change_name) : null;
					const assessmentPath = change && snapshot
						? writeAssessmentRecord(cwd, params.change_name, {
							changeName: params.change_name,
							assessmentKind: params.assessment_kind as AssessmentKind,
							outcome: params.outcome as AssessmentOutcome,
							timestamp: new Date().toISOString(),
							summary: params.summary,
							snapshot,
							reconciliation: {
								reopen: params.outcome === "reopen",
								changedFiles: params.changed_files ?? [],
								constraints: params.constraints ?? [],
								recommendedAction: params.outcome === "pass" ? null : "Run openspec_manage reconcile_after_assess before archive.",
							},
						})
						: null;

					const reconcileCandidates = emitReconcileCandidates(params.change_name, params.summary, params.constraints);
					if (reconcileCandidates.length > 0) {
						(sharedState.lifecycleCandidateQueue ??= []).push({
							source: "openspec",
							context: `reconcile_after_assess for '${params.change_name}'`,
							candidates: reconcileCandidates,
						});
					}

					emitOpenSpecState(cwd, pi);
					const tree = scanDesignDocs(path.join(cwd, "docs"));
					emitDesignTreeState(pi, tree, null);

					const lifecycleStatus = evaluateLifecycleReconciliation(cwd, params.change_name);
					const nextSteps = buildReconciliationNextSteps(params.change_name, params.assessment_kind, params.outcome);
					const lifecycleSignals = {
						assessmentKind: params.assessment_kind,
						outcome: params.outcome,
						reopened: result.reopened,
						archiveReady: params.outcome === "pass" && lifecycleStatus.issues.length === 0,
						requiresOpenSpecReconciliation: result.updatedTaskState || result.outcome !== "pass",
						requiresDesignTreeRefresh: result.updatedNodeIds.length > 0,
						boundNodeIds: lifecycleStatus.boundNodeIds,
						issues: lifecycleStatus.issues,
					};
					const observedEffects = {
						filesChanged: [
							...(result.updatedTaskState ? [`openspec/changes/${params.change_name}/tasks.md`] : []),
							...result.updatedNodeIds.map((nodeId) => `docs/${nodeId}.md`),
						],
						lifecycleTouched: [
							"openspec",
							...(result.updatedNodeIds.length > 0 ? ["design-tree"] : []),
						],
						sideEffectClass: "workspace-write",
					};

					const lines = [
						`Post-assess reconciliation applied to '${params.change_name}'.`,
						"",
						`Assessment kind: ${params.assessment_kind}`,
						`Outcome: ${result.outcome}`,
						`Lifecycle reopened: ${result.reopened ? "yes" : "no"}`,
						`Task state updated: ${result.updatedTaskState ? "yes" : "no"}`,
						`Archive ready: ${lifecycleSignals.archiveReady ? "yes" : "no"}`,
						...(assessmentPath ? [`Assessment record: ${assessmentPath}`] : []),
					];
					if (result.updatedNodeIds.length > 0) {
						lines.push(`Updated design nodes: ${result.updatedNodeIds.join(", ")}`);
					}
					if (result.appendedFileScope.length > 0) {
						lines.push(`Appended file-scope deltas: ${result.appendedFileScope.join(", ")}`);
					}
					if (result.appendedConstraints.length > 0) {
						lines.push(`Appended constraints: ${result.appendedConstraints.join(" | ")}`);
					}
					if (lifecycleStatus.issues.length > 0) {
						lines.push("", "Remaining lifecycle issues:", formatReconciliationIssues(lifecycleStatus.issues));
					}
					if (result.warning) {
						lines.push("", `Warning: ${result.warning}`);
					}
					if (nextSteps.length > 0) {
						lines.push("", "Next steps:", ...nextSteps.map((step) => `- ${step}`));
					}

					return {
						content: [{ type: "text", text: lines.join("\n") }],
						details: {
							...result,
							assessmentPath,
							lifecycleSignals,
							observedEffects,
							nextSteps,
							reconcileCandidatesEmitted: reconcileCandidates.length,
						},
					};
				}

				// ── archive ──────────────────────────────────────────
				case "archive": {
					if (!params.change_name) {
						return { content: [{ type: "text", text: "Error: change_name required" }], details: {}, isError: true };
					}

					const changeInfo = getChange(cwd, params.change_name);
					if (!changeInfo) {
						return {
							content: [{ type: "text", text: `Change '${params.change_name}' not found` }],
							details: {},
							isError: true,
						};
					}
					// Archive gate: use the canonical lifecycle resolver so that the readiness
					// check here is identical to what the status/get surfaces report.
					const lifecycle = getLifecycleSummary(cwd, changeInfo);
					if (!lifecycle.archiveReady) {
						const assessmentState = await getAssessmentState(cwd, changeInfo);
						return {
							content: [{
								type: "text",
								text: [
									`Archive refused for '${params.change_name}': ${lifecycle.reason ?? lifecycle.nextAction ?? "lifecycle not ready for archive."}`,
									...(assessmentState.record ? ["", ...formatAssessmentSummary(assessmentState.record)] : []),
								].join("\n"),
							}],
							details: { lifecycle },
							isError: true,
						};
					}

					const result = archiveChange(cwd, params.change_name);
					if (!result.archived) {
						return {
							content: [{ type: "text", text: result.operations.join("\n") }],
							details: {},
							isError: true,
						};
					}

					if (changeInfo) {
						const archiveCandidates = emitArchiveCandidates({ ...changeInfo, stage: "archived" });
						if (archiveCandidates.length > 0) {
							(sharedState.lifecycleCandidateQueue ??= []).push({
								source: "openspec",
								context: `archive for '${params.change_name}'`,
								candidates: archiveCandidates,
							});
							result.operations.push(`Emitted ${archiveCandidates.length} lifecycle memory candidate(s)`);
						}
					}

					// Archive gate: transition implementing → implemented in design tree
					const transitioned = transitionDesignNodesOnArchive(cwd, params.change_name);
					if (transitioned.length > 0) {
						result.operations.push(
							`Transitioned design node${transitioned.length > 1 ? "s" : ""} to implemented: ${transitioned.join(", ")}`,
						);
					}

					// Auto-delete merged feature branches from transitioned design nodes
					const allBranches = resolveBoundDesignNodes(cwd, params.change_name)
						.flatMap((n) => n.branches ?? []);
					if (allBranches.length > 0) {
						const { deleted, skipped } = await deleteMergedBranches(pi, cwd, allBranches);
						if (deleted.length > 0) {
							result.operations.push(`Deleted merged branches: ${deleted.join(", ")}`);
						}
						if (skipped.length > 0) {
							result.operations.push(`Skipped unmerged/protected branches: ${skipped.join(", ")}`);
						}
					}

					emitOpenSpecState(cwd, pi);
					return {
						content: [{
							type: "text",
							text: `Archived '${params.change_name}':\n\n` +
								result.operations.map((op) => `  - ${op}`).join("\n") +
								"\n\nSpecs have been merged to baseline. Change is complete.",
						}],
						details: { operations: result.operations, transitionedNodes: transitioned },
					};
				}
			}

			return { content: [{ type: "text", text: "Unknown action" }], details: {} };
		},

		renderCall(args, theme) {
			let summary = args.action as string;
			switch (args.action) {
				case "propose":
					summary = args.name ? `propose:${args.name}` : "propose";
					break;
				case "add_spec":
					summary = args.change_name
						? `add_spec:${args.change_name}${args.domain ? `/${args.domain}` : ""}`
						: "add_spec";
					break;
				case "generate_spec":
					summary = args.change_name
						? `generate_spec:${args.change_name}${args.domain ? `/${args.domain}` : ""}`
						: "generate_spec";
					break;
				case "fast_forward":
				case "get":
				case "archive":
				case "reconcile_after_assess":
					summary = args.change_name ? `${args.action}:${args.change_name}` : args.action;
					break;
				case "status":
					summary = "status";
					break;
			}
			return sciCall("openspec_manage", summary, theme);
		},

		renderResult(result, { expanded, isPartial }, theme) {
			if (isPartial) {
				return sciLoading("openspec_manage", theme);
			}

			if ((result as any).isError) {
				const first = result.content?.[0];
				const msg = (first && "text" in first ? first.text : "Error").split("\n")[0];
				return sciErr(msg, theme);
			}

			// Build action-specific summary for both collapsed and expanded
			const details = (result.details || {}) as Record<string, any>;
			let summary = "";
			let expandedLines: string[] = [];

			if (details.changePath) {
				// propose
				const name = typeof details.changePath === "string"
					? details.changePath.split("/").pop() ?? details.changePath
					: "";
				summary = `✓ proposed ${name}`;
				expandedLines = [
					theme.fg("accent", `Change: ${name}`),
					theme.fg("dim", `Path: ${details.changePath}`),
				];
			} else if (details.specPath !== undefined && details.sections !== undefined) {
				// add_spec
				const specName = typeof details.specPath === "string"
					? details.specPath.split("/").slice(-2).join("/")
					: "spec";
				const sections = Array.isArray(details.sections) ? details.sections : [];
				summary = `✓ spec added ${specName}`;
				expandedLines = [
					theme.fg("accent", `Spec: ${specName}`),
					...sections.map((s: any) =>
						`  ${theme.fg("muted", s.title ?? s)} ${s.requirements ? theme.fg("dim", `· ${s.requirements} req${s.requirements !== 1 ? "s" : ""}`) : ""}`,
					),
				];
			} else if (details.specPath !== undefined && details.generated) {
				// generate_spec
				const specName = typeof details.specPath === "string"
					? details.specPath.split("/").slice(-2).join("/")
					: "spec";
				summary = `✓ spec generated ${specName}`;
				expandedLines = [theme.fg("accent", `Generated: ${specName}`)];
			} else if (details.files && !details.operations) {
				// fast_forward
				const files = Array.isArray(details.files) ? details.files : [];
				summary = `✓ fast-forwarded (${files.join(", ")})`;
				expandedLines = files.map((f: string) => `  ${theme.fg("success", "✓")} ${theme.fg("muted", f)}`);
			} else if (details.operations) {
				// archive
				const firstContent = result.content?.[0];
				const name = details.transitionedNodes !== undefined
					? ((firstContent && "text" in firstContent ? firstContent.text : "").match(/Archived '([^']+)'/)?.[1] ?? "change")
					: "change";
				const ops = Array.isArray(details.operations) ? details.operations : [];
				summary = `✓ archived ${name}`;
				expandedLines = ops.map((op: string) => `  ${theme.fg("muted", op)}`);
				if (details.transitionedNodes) {
					expandedLines.push(theme.fg("dim", `  Design nodes transitioned: ${details.transitionedNodes}`));
				}
			} else if (details.changes) {
				// status
				const changes = Array.isArray(details.changes) ? details.changes : [];
				const count = changes.length;
				summary = count === 0 ? "no active changes" : `${count} change${count !== 1 ? "s" : ""}`;
				const STAGE_ICONS: Record<string, string> = {
					proposed: "◌", specced: "◐", planned: "●", ready: "★", complete: "✓",
				};
				expandedLines = changes.map((c: any) => {
					const icon = STAGE_ICONS[c.stage] ?? "·";
					return `  ${theme.fg("accent", icon)} ${theme.fg("muted", c.name)} ${theme.fg("dim", `(${c.stage})`)}`;
				});
			} else if (details.change) {
				// get
				const c = details.change;
				const name = c?.name ?? "";
				const stage = c?.stage ?? "";
				summary = `${name} (${stage})`;
				const STAGE_ICONS: Record<string, string> = {
					proposed: "◌", specced: "◐", planned: "●", ready: "★", complete: "✓",
				};
				const icon = STAGE_ICONS[stage] ?? "·";
				expandedLines = [
					`${theme.fg("accent", icon)} ${theme.fg("muted", name)} ${theme.fg("dim", stage)}`,
				];
				if (c.specs && Array.isArray(c.specs)) {
					expandedLines.push(theme.fg("dim", `  Specs: ${c.specs.length}`));
					for (const s of c.specs.slice(0, 5)) {
						expandedLines.push(`    ${theme.fg("muted", typeof s === "string" ? s : s.domain ?? s.path ?? "")}`);
					}
				}
			} else if (details.reconcileCandidatesEmitted !== undefined) {
				// reconcile_after_assess
				const changeName = details.changeName
					?? ((result.content?.[0] && "text" in result.content[0]
						? result.content[0].text : "").match(/reconciliation applied to '([^']+)'/)?.[1]
					?? "change");
				const outcome = details.lifecycleSignals?.outcome ?? "";
				summary = `✓ reconciled ${changeName}${outcome ? ` (${outcome})` : ""}`;
			} else {
				const first = result.content?.[0];
				summary = (first && "text" in first ? first.text?.split("\n")[0] : null) || "done";
			}

			if (expanded && expandedLines.length > 0) {
				return sciExpanded(expandedLines, summary, theme);
			}

			if (expanded) {
				// Fallback: raw text
				const first = result.content?.[0];
				const full = (first && "text" in first ? first.text : null) || "Done";
				const lines = full.split("\n");
				return sciExpanded(lines, summary, theme);
			}

			return sciOk(summary, theme);
		},
	});

	// ─── Bridged Commands ────────────────────────────────────────────────────

	const bridge = getSharedBridge();

	bridge.register(pi, {
		name: "opsx:propose",
		description: "Create a new untracked OpenSpec change: /opsx:propose <name> <title>. For tracked work, use design_tree_update(implement) from a decided node instead.",
		bridge: {
			agentCallable: true,
			sideEffectClass: "workspace-write",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			const trimmedArgs = (args || "").trim();
			
			if (ctx.bridgeInvocation) {
				// When called via bridge, args are JSON-encoded to preserve boundaries
				let parsedArgs: string[];
				try {
					parsedArgs = JSON.parse(trimmedArgs);
				} catch (e) {
					return buildSlashCommandResult("opsx:propose", [], {
						ok: false,
						summary: "Bridge argument parsing error",
						humanText: "Error: Invalid argument format from bridge",
						effects: { sideEffectClass: "workspace-write" },
					});
				}
				
				const [name, title, intent] = parsedArgs;
				
				if (!name) {
					return buildSlashCommandResult("opsx:propose", parsedArgs, {
						ok: false,
						summary: "Usage: /opsx:propose <name> <title> <intent>",
						humanText: "Error: name required for propose",
						effects: { sideEffectClass: "workspace-write" },
					});
				}
				
				const finalTitle = title || name;
				const finalIntent = intent || "";
				
				try {
					const result = createChange(ctx.cwd, name, finalTitle, finalIntent);
					emitOpenSpecState(ctx.cwd, pi);
					
					return buildSlashCommandResult("opsx:propose", [name, finalTitle, finalIntent], {
						ok: true,
						summary: `Created OpenSpec change: ${path.basename(result.changePath)}`,
						humanText: `Created: ${result.changePath}\n\nNext: Add specs with \`/opsx:spec ${path.basename(result.changePath)}\` ` +
							`or use \`openspec_manage\` with action \`generate_spec\``,
						data: { changePath: result.changePath, files: result.files },
						effects: {
							sideEffectClass: "workspace-write",
							filesChanged: result.files.map(f => path.join(result.changePath, f)),
							lifecycleTouched: ["openspec"],
						},
						nextSteps: [
							{ label: "Add specs", command: `/opsx:spec ${path.basename(result.changePath)}` },
							{ label: "Generate specs", rationale: "Use openspec_manage with action generate_spec" },
						],
					});
				} catch (e) {
					return buildSlashCommandResult("opsx:propose", [name, finalTitle, finalIntent], {
						ok: false,
						summary: `Error: ${(e as Error).message}`,
						humanText: `Error: ${(e as Error).message}`,
						effects: { sideEffectClass: "workspace-write" },
					});
				}
			} else {
				// Interactive path - parse name and title, then prompt for intent if needed
				const parts = trimmedArgs.split(/\s+/);
				const name = parts[0];
				const title = parts.slice(1).join(" ");
				
				if (!name) {
					return buildSlashCommandResult("opsx:propose", [name, title].filter(Boolean), {
						ok: false,
						summary: "Usage: /opsx:propose <name> <title>",
						humanText: "Error: name required for propose",
						effects: { sideEffectClass: "workspace-write" },
					});
				}
				
				const finalTitle = title || name;
				// Interactive prompting for intent will be handled in interactiveHandler
				// For now, use empty string and let handler prompt if needed
				const intent = "";
				
				try {
					const result = createChange(ctx.cwd, name, finalTitle, intent);
					emitOpenSpecState(ctx.cwd, pi);

					pi.sendMessage({
						customType: "openspec-created",
						content: `Created OpenSpec change \`${path.basename(result.changePath)}\`.\n\n` +
							`Next step: Define specs with \`/opsx:spec ${path.basename(result.changePath)}\` ` +
							`or use \`openspec_manage\` with action \`generate_spec\` to scaffold Given/When/Then scenarios.`,
						display: true,
					}, { triggerTurn: false });

					return buildSlashCommandResult("opsx:propose", [name, finalTitle, intent], {
						ok: true,
						summary: `Created OpenSpec change: ${path.basename(result.changePath)}`,
						humanText: `Created: ${result.changePath}\n\nNext: Add specs with \`/opsx:spec ${path.basename(result.changePath)}\` ` +
							`or use \`openspec_manage\` with action \`generate_spec\``,
						data: { changePath: result.changePath, files: result.files },
						effects: {
							sideEffectClass: "workspace-write",
							filesChanged: result.files.map(f => path.join(result.changePath, f)),
							lifecycleTouched: ["openspec"],
						},
						nextSteps: [
							{ label: "Add specs", command: `/opsx:spec ${path.basename(result.changePath)}` },
							{ label: "Generate specs", rationale: "Use openspec_manage with action generate_spec" },
						],
					});
				} catch (e) {
					return buildSlashCommandResult("opsx:propose", [name, finalTitle, intent], {
						ok: false,
						summary: `Error: ${(e as Error).message}`,
						humanText: `Error: ${(e as Error).message}`,
						effects: { sideEffectClass: "workspace-write" },
					});
				}
			}
		},
		interactiveHandler: async (result, args, ctx) => {
			if (!result.ok) {
				ctx.ui.notify(result.humanText, "error");
				return;
			}

			// Check if we need to prompt for intent 
			const trimmedArgs = (args || "").trim();
			const parts = trimmedArgs.split(/\s+/);
			const name = parts[0];
			const title = parts.slice(1).join(" ");
			
			if (name && !title) {
				// Only name provided, prompt for title and intent
				const titleInput = await ctx.ui.input("Enter change title:");
				if (!titleInput) {
					ctx.ui.notify("Change creation cancelled", "warning");
					return;
				}
				
				const intentInput = await ctx.ui.input("Enter change intent (what this change accomplishes):");
				if (!intentInput) {
					ctx.ui.notify("Change creation cancelled", "warning");
					return;
				}

				try {
					const newResult = createChange(ctx.cwd, name, titleInput, intentInput);
					emitOpenSpecState(ctx.cwd, pi);
					ctx.ui.notify(`Created OpenSpec change: ${path.basename(newResult.changePath)}`, "info");
				} catch (e) {
					ctx.ui.notify(`Error: ${(e as Error).message}`, "error");
				}
			} else if (name && title) {
				// Change was already created by structuredExecutor with empty intent.
				// Prompt for intent and patch proposal.md — do NOT call createChange again.
				const intentInput = await ctx.ui.input("Enter change intent (what this change accomplishes):");
				const changeData = result.data as { changePath?: string } | undefined;
				if (intentInput && changeData?.changePath) {
					try {
						const proposalPath = path.join(changeData.changePath, "proposal.md");
						if (fs.existsSync(proposalPath)) {
							const current = fs.readFileSync(proposalPath, "utf-8");
							fs.writeFileSync(proposalPath, current.replace(/^## Intent\n[\s\S]*?(?=\n##|$)/m, `## Intent\n${intentInput}\n`));
						}
						emitOpenSpecState(ctx.cwd, pi);
						ctx.ui.notify(`Created OpenSpec change: ${path.basename(changeData.changePath)}`, "info");
					} catch (e) {
						ctx.ui.notify(`Error updating intent: ${(e as Error).message}`, "warning");
						ctx.ui.notify(result.humanText, "info");
					}
				} else {
					// Use the result we already have (with empty intent)
					ctx.ui.notify(result.humanText, "info");
				}
			} else {
				// All arguments provided or error case
				ctx.ui.notify(result.humanText, result.ok ? "info" : "error");
			}
		},
	} satisfies BridgedSlashCommand);

	bridge.register(pi, {
		name: "opsx:spec",
		description: "Generate or add specs for a change: /opsx:spec <change>",
		bridge: {
			agentCallable: true,
			sideEffectClass: "workspace-write",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			let changeName: string;
			if (ctx.bridgeInvocation) {
				try {
					const parsedArgs = JSON.parse((args || "").trim());
					changeName = parsedArgs[0] || "";
				} catch (e) {
					changeName = (args || "").trim();
				}
			} else {
				changeName = (args || "").trim();
			}
			
			if (!changeName) {
				return buildSlashCommandResult("opsx:spec", [], {
					ok: false,
					summary: "Usage: /opsx:spec <change-name>",
					humanText: "Error: change-name required",
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				return buildSlashCommandResult("opsx:spec", [changeName], {
					ok: false,
					summary: `Change '${changeName}' not found`,
					humanText: `Change '${changeName}' not found`,
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			let proposalContent = "";
			if (change.hasProposal) {
				proposalContent = fs.readFileSync(path.join(change.path, "proposal.md"), "utf-8");
			}

			// Actually generate specs instead of just requesting them
			try {
				if (!change.hasProposal) {
					return buildSlashCommandResult("opsx:spec", [changeName], {
						ok: false,
						summary: "No proposal found",
						humanText: `Change '${changeName}' has no proposal. Run /opsx:propose first.`,
						effects: { sideEffectClass: "workspace-write" },
						nextSteps: [
							{ label: "Create proposal", command: `/opsx:propose ${changeName}` },
						],
					});
				}

				// Generate a default spec from the proposal
				const specContent = generateSpecFromProposal({
					domain: "core",
					proposalContent,
				});

				// Ensure specs directory exists
				const specsDir = path.join(change.path, "specs");
				fs.mkdirSync(specsDir, { recursive: true });

				// Write the generated spec
				const specFilePath = path.join(specsDir, "core.md");
				fs.writeFileSync(specFilePath, specContent);

				emitOpenSpecState(ctx.cwd, pi);

				const content = [
					`Generated spec file: specs/core.md`,
					"",
					change.hasProposal ? `Based on proposal content from proposal.md` : "Generated default spec structure.",
					"",
					"Edit the spec to add more specific Given/When/Then scenarios.",
					"Each scenario should be specific and testable.",
				].join("\n");

				if (!ctx.bridgeInvocation) {
					pi.sendMessage({
						customType: "openspec-spec-generated",
						content: `Generated spec for \`${changeName}\`:\n\n${content}`,
						display: true,
					}, { triggerTurn: false });
				}

				return buildSlashCommandResult("opsx:spec", [changeName], {
					ok: true,
					summary: `Generated spec for '${changeName}'`,
					humanText: content,
					data: { 
						changeName, 
						specFilePath: path.relative(ctx.cwd, specFilePath),
						hasProposal: change.hasProposal, 
						generatedContent: specContent.slice(0, 1000) 
					},
					effects: { 
						sideEffectClass: "workspace-write",
						filesChanged: [path.relative(ctx.cwd, specFilePath)],
						lifecycleTouched: ["openspec"],
					},
					nextSteps: [
						{ label: "Review and edit spec", command: `edit ${path.relative(ctx.cwd, specFilePath)}` },
						{ label: "Generate design and tasks", command: `/opsx:ff ${changeName}` },
					],
				});
			} catch (e) {
				return buildSlashCommandResult("opsx:spec", [changeName], {
					ok: false,
					summary: `Error generating spec: ${(e as Error).message}`,
					humanText: `Error generating spec: ${(e as Error).message}`,
					effects: { sideEffectClass: "workspace-write" },
				});
			}
		},
		// No agentHandler needed - the structuredExecutor does the work
	} satisfies BridgedSlashCommand);

	bridge.register(pi, {
		name: "opsx:ff",
		description: "Fast-forward: generate design + tasks from specs: /opsx:ff <change>",
		bridge: {
			agentCallable: true,
			sideEffectClass: "workspace-write",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			let changeName: string;
			if (ctx.bridgeInvocation) {
				try {
					const parsedArgs = JSON.parse((args || "").trim());
					changeName = parsedArgs[0] || "";
				} catch (e) {
					changeName = (args || "").trim();
				}
			} else {
				changeName = (args || "").trim();
			}
			
			if (!changeName) {
				return buildSlashCommandResult("opsx:ff", [], {
					ok: false,
					summary: "Usage: /opsx:ff <change-name>",
					humanText: "Error: change-name required",
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				return buildSlashCommandResult("opsx:ff", [changeName], {
					ok: false,
					summary: `Change '${changeName}' not found`,
					humanText: `Change '${changeName}' not found`,
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			if (!change.hasSpecs && !change.hasProposal) {
				return buildSlashCommandResult("opsx:ff", [changeName], {
					ok: false,
					summary: "Change has no specs or proposal",
					humanText: "Change has no specs or proposal. Run /opsx:spec first.",
					effects: { sideEffectClass: "workspace-write" },
					nextSteps: [
						{ label: "Add specs", command: `/opsx:spec ${changeName}` },
					],
				});
			}

			const files: string[] = [];

			// Generate design.md if not present
			if (!change.hasDesign) {
				const designLines = [`# ${change.name} — Design`, ""];

				if (change.specs.length > 0) {
					designLines.push("## Spec-Derived Architecture", "");
					for (const spec of change.specs) {
						designLines.push(`### ${spec.domain}`, "");
						for (const section of spec.sections) {
							if (section.type === "removed") continue;
							for (const req of section.requirements) {
								designLines.push(`- **${req.title}** (${section.type}) — ${req.scenarios.length} scenarios`);
							}
						}
						designLines.push("");
					}
				}

				// Read proposal for additional context
				if (change.hasProposal) {
					const proposal = fs.readFileSync(path.join(change.path, "proposal.md"), "utf-8");
					const scopeMatch = proposal.match(/##\s+Scope\s*\n([\s\S]*?)(?=\n##\s|$)/i);
					if (scopeMatch) {
						designLines.push("## Scope", "", scopeMatch[1].trim(), "");
					}
				}

				designLines.push("## File Changes", "");
				designLines.push("<!-- Add file changes as you design the implementation -->", "");

				fs.writeFileSync(path.join(change.path, "design.md"), designLines.join("\n"));
				files.push("design.md");
			}

			// Generate tasks.md if not present
			if (!change.hasTasks) {
				const taskLines = [`# ${change.name} — Tasks`, ""];

				if (change.specs.length > 0) {
					// Generate task groups from spec domains/requirements
					let groupNum = 1;
					for (const spec of change.specs) {
						for (const section of spec.sections) {
							if (section.type === "removed") continue;
							for (const req of section.requirements) {
								taskLines.push(`## ${groupNum}. ${req.title}`, "");
								// Each scenario becomes a task
								let taskNum = 1;
								for (const s of req.scenarios) {
									taskLines.push(`- [ ] ${groupNum}.${taskNum} ${s.title}`);
									taskNum++;
								}
								// Add a verification task
								taskLines.push(`- [ ] ${groupNum}.${taskNum} Write tests for ${req.title}`);
								taskLines.push("");
								groupNum++;
							}
						}
					}
				} else {
					taskLines.push("## 1. Implementation", "");
					taskLines.push("- [ ] 1.1 Implement the proposed change", "");
				}

				fs.writeFileSync(path.join(change.path, "tasks.md"), taskLines.join("\n"));
				files.push("tasks.md");
			}

			if (files.length === 0) {
				return buildSlashCommandResult("opsx:ff", [changeName], {
					ok: false,
					summary: "design.md and tasks.md already exist",
					humanText: `design.md and tasks.md already exist for '${changeName}'. Delete them to regenerate.`,
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			emitOpenSpecState(ctx.cwd, pi);

			// Read the generated content to include in the response
			const generatedContent: { [filename: string]: string } = {};
			for (const filename of files) {
				const filePath = path.join(change.path, filename);
				generatedContent[filename] = fs.readFileSync(filePath, "utf-8");
			}

			const content = [
				`Generated files for '${changeName}':`,
				"",
				...files.map(f => `- ${f}`),
				"",
				"Files are ready for review and implementation.",
				"Next: Review the generated tasks and run `/cleave` to execute them.",
			].join("\n");

			if (!ctx.bridgeInvocation) {
				pi.sendMessage({
					customType: "openspec-ff-complete",
					content: `Generated design and tasks for \`${changeName}\`:\n\n${content}`,
					display: true,
				}, { triggerTurn: false });
			}

			return buildSlashCommandResult("opsx:ff", [changeName], {
				ok: true,
				summary: `Fast-forwarded '${changeName}': generated ${files.join(", ")}`,
				humanText: content,
				data: { files, changeName, generatedContent },
				effects: {
					sideEffectClass: "workspace-write",
					filesChanged: files.map(f => path.join(change.path, f)),
					lifecycleTouched: ["openspec"],
				},
				nextSteps: [
					{ label: "Review files", rationale: "Check generated design.md and tasks.md" },
					{ label: "Execute tasks", command: "/cleave" },
				],
			});
		},
		// No agentHandler needed - the structuredExecutor returns complete information
	} satisfies BridgedSlashCommand);

	bridge.register(pi, {
		name: "opsx:status",
		description: "Show all active OpenSpec changes",
		bridge: {
			agentCallable: true,
			sideEffectClass: "read",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			const changes = listChanges(ctx.cwd);
			if (changes.length === 0) {
				return buildSlashCommandResult("opsx:status", [], {
					ok: true,
					summary: "No active OpenSpec changes",
					humanText: "No active OpenSpec changes. Use /opsx:propose to create one.",
					data: { changes: [] },
					effects: { sideEffectClass: "read" },
					nextSteps: [
						{ label: "Create change", command: "/opsx:propose", rationale: "Start a new OpenSpec change" },
					],
				});
			}

			const lines = changes.map((c) => {
				const lifecycle = getLifecycleSummary(ctx.cwd, c);
				const verificationLine = lifecycle.verificationSubstate ? `\n  Verification: ${lifecycle.verificationSubstate}` : "";
				const nextLine = lifecycle.nextAction ? `\n  → ${lifecycle.nextAction}` : `\n  → ${nextStepHint(c)}`;
				return `${formatChangeStatus(c)}${verificationLine}${nextLine}`;
			});

			return buildSlashCommandResult("opsx:status", [], {
				ok: true,
				summary: "OpenSpec changes status",
				humanText: lines.join("\n\n"),
				data: {
					changes: changes.map((c) => {
						const lifecycle = getLifecycleSummary(ctx.cwd, c);
						return {
							name: c.name,
							stage: lifecycle.stage,
							verificationStage: lifecycle.stage,
							verificationSubstate: lifecycle.verificationSubstate,
							archiveReady: lifecycle.archiveReady,
							bindingStatus: lifecycle.bindingStatus,
							nextAction: lifecycle.nextAction,
							totalTasks: lifecycle.totalTasks,
							doneTasks: lifecycle.doneTasks,
							specCount: countScenarios(c.specs),
						};
					}),
				},
				effects: { sideEffectClass: "read" },
			});
		},
	} satisfies BridgedSlashCommand);

	bridge.register(pi, {
		name: "opsx:verify",
		description: "Check verification status of a change: /opsx:verify <change>",
		bridge: {
			agentCallable: true,
			sideEffectClass: "read",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			let changeName: string;
			if (ctx.bridgeInvocation) {
				try {
					const parsedArgs = JSON.parse((args || "").trim());
					changeName = parsedArgs[0] || "";
				} catch (e) {
					changeName = (args || "").trim();
				}
			} else {
				changeName = (args || "").trim();
			}
			
			if (!changeName) {
				return buildSlashCommandResult("opsx:verify", [], {
					ok: false,
					summary: "Usage: /opsx:verify <change-name>",
					humanText: "Usage: /opsx:verify <change-name>",
					effects: { sideEffectClass: "read" },
				});
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				return buildSlashCommandResult("opsx:verify", [changeName], {
					ok: false,
					summary: `Change '${changeName}' not found`,
					humanText: `Change '${changeName}' not found`,
					effects: { sideEffectClass: "read" },
				});
			}

			if (!change.hasSpecs) {
				return buildSlashCommandResult("opsx:verify", [changeName], {
					ok: false,
					summary: `Change '${changeName}' has no specs to verify against`,
					humanText: `Change '${changeName}' has no specs to verify against`,
					effects: { sideEffectClass: "read" },
					nextSteps: [
						{ label: "Add specs", command: `/opsx:spec ${changeName}` },
					],
				});
			}

			const assessmentState = await getAssessmentState(ctx.cwd, change);
			const lifecycle = getLifecycleSummary(ctx.cwd, change);
			const effectiveSubstate = lifecycle.verificationSubstate
				?? (assessmentState.record?.outcome === "reopen" ? "reopened-work" : null);
			const effectiveReason: string | null = lifecycle.reason
				?? (effectiveSubstate === "reopened-work" ? "The latest persisted assessment reopened work." : null);
			const effectiveNextAction = lifecycle.nextAction
				?? (effectiveSubstate === "reopened-work"
					? `Complete follow-up work for ${changeName}, reconcile lifecycle artifacts, then re-run /assess spec ${changeName}`
					: null);

			if (effectiveSubstate === "archive-ready" && assessmentState.record) {
				const summaryLines = [
					`Verification state for '${changeName}': ${effectiveSubstate}`,
					...(effectiveReason ? [`Why: ${effectiveReason}`] : []),
					...(effectiveNextAction ? [`Next: ${effectiveNextAction}`] : []),
					"",
					...formatAssessmentSummary(assessmentState.record),
				];

				return buildSlashCommandResult("opsx:verify", [changeName], {
					ok: true,
					summary: `Archive ready: ${changeName}`,
					humanText: summaryLines.join("\n"),
					data: {
						changeName,
						substate: effectiveSubstate,
						reason: effectiveReason,
						nextAction: effectiveNextAction,
						assessment: assessmentState.record,
						archiveReady: true,
					},
					effects: { sideEffectClass: "read" },
					nextSteps: effectiveNextAction ? [{ label: effectiveNextAction }] : [],
				});
			}

			if ((effectiveSubstate === "reopened-work" || effectiveSubstate === "missing-binding" || effectiveSubstate === "awaiting-reconciliation") && assessmentState.record) {
				const summaryLines = [
					`Verification state for '${changeName}': ${effectiveSubstate}`,
					...(effectiveReason ? [`Why: ${effectiveReason}`] : []),
					...(effectiveNextAction ? [`Next: ${effectiveNextAction}`] : []),
					"",
					...formatAssessmentSummary(assessmentState.record),
				];

				return buildSlashCommandResult("opsx:verify", [changeName], {
					ok: false,
					summary: `Verification blocked: ${changeName}`,
					humanText: summaryLines.join("\n"),
					data: {
						changeName,
						substate: effectiveSubstate,
						reason: effectiveReason,
						nextAction: effectiveNextAction,
						assessment: assessmentState.record,
						archiveReady: false,
					},
					effects: { sideEffectClass: "read" },
					nextSteps: effectiveNextAction ? [{ label: effectiveNextAction }] : [],
				});
			}

			const refreshReason = assessmentState.status === "missing"
				? "No persisted assessment exists yet."
				: effectiveReason ?? assessmentState.reason;

			const content = [
				`[OpenSpec: Verify \`${changeName}\`]`,
				"",
				`Verification state: ${effectiveSubstate ?? lifecycle.verificationSubstate ?? change.stage}`,
				...(effectiveReason ? [effectiveReason, ""] : []),
				`${refreshReason}`,
				"",
				`Run \`/assess spec ${changeName}\` now and persist the resulting structured lifecycle state by calling \`openspec_manage\` with action \`reconcile_after_assess\`, change_name \`${changeName}\`, assessment_kind \`spec\`, and the appropriate outcome.`,
				"",
				"If the assessment passes cleanly, persist outcome `pass`. If it reopens work, persist `reopen`. If the reviewer cannot determine status safely, persist `ambiguous`.",
				"",
				`After persistence, archive remains gated until the current assessment for \`${changeName}\` explicitly passes.`,
			].join("\n");

			if (!ctx.bridgeInvocation) {
				pi.sendMessage({
					customType: "openspec-verify",
					content,
					display: true,
				}, { triggerTurn: true });
			}

			return buildSlashCommandResult("opsx:verify", [changeName], {
				ok: true,
				summary: `Verification assessment needed for '${changeName}'`,
				humanText: content,
				data: {
					changeName,
					substate: effectiveSubstate ?? lifecycle.verificationSubstate ?? change.stage,
					reason: refreshReason,
					nextAction: `/assess spec ${changeName}`,
					assessment: assessmentState.record,
					archiveReady: false,
				},
				effects: { sideEffectClass: "read" },
				nextSteps: [
					{ label: "Run assessment", command: `/assess spec ${changeName}`, rationale: "Verify specs against implementation" },
				],
			});
		},
		interactiveHandler: async (result, args, ctx) => {
			const data = result.data as any;
			if (data && data.archiveReady && result.ok) {
				ctx.ui.notify(result.humanText, "info");
			} else if (data && !data.archiveReady && data.substate && (data.substate === "reopened-work" || data.substate === "missing-binding" || data.substate === "awaiting-reconciliation")) {
				ctx.ui.notify(result.humanText, "warning");
			} else if (result.ok) {
				// Trigger agent message for assessment requests
				return;
			} else {
				ctx.ui.notify(result.humanText, "warning");
			}
		},
		agentHandler: async (result, _args, _ctx) => {
			const archiveReady = result.data && typeof result.data === 'object' && 
				'archiveReady' in result.data ? (result.data as { archiveReady: boolean }).archiveReady : false;
			if (result.ok && result.humanText && !archiveReady) {
				pi.sendMessage({
					customType: "openspec-verify",
					content: result.humanText,
					display: true,
				}, { triggerTurn: true });
			}
		},
	} satisfies BridgedSlashCommand);

	bridge.register(pi, {
		name: "opsx:archive",
		description: "Archive a completed change: /opsx:archive <change>",
		bridge: {
			agentCallable: true,
			sideEffectClass: "workspace-write",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			let changeName: string;
			if (ctx.bridgeInvocation) {
				try {
					const parsedArgs = JSON.parse((args || "").trim());
					changeName = parsedArgs[0] || "";
				} catch (e) {
					changeName = (args || "").trim();
				}
			} else {
				changeName = (args || "").trim();
			}
			
			if (!changeName) {
				return buildSlashCommandResult("opsx:archive", [], {
					ok: false,
					summary: "Usage: /opsx:archive <change-name>",
					humanText: "Usage: /opsx:archive <change-name>",
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			const changeInfo = getChange(ctx.cwd, changeName);
			if (!changeInfo) {
				return buildSlashCommandResult("opsx:archive", [changeName], {
					ok: false,
					summary: `Change '${changeName}' not found`,
					humanText: `Change '${changeName}' not found`,
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			// Archive gate: use the canonical lifecycle resolver so that readiness
			// reported here is identical to what the status/get surfaces show.
			const lifecycle = getLifecycleSummary(ctx.cwd, changeInfo);
			if (!lifecycle.archiveReady) {
				const assessmentState = await getAssessmentState(ctx.cwd, changeInfo);
				const message = [
					`Archive refused for '${changeName}': ${lifecycle.reason ?? lifecycle.nextAction ?? "lifecycle not ready for archive."}`,
					...(assessmentState.record ? ["", ...formatAssessmentSummary(assessmentState.record)] : []),
				].join("\n");

				return buildSlashCommandResult("opsx:archive", [changeName], {
					ok: false,
					summary: "Archive refused: lifecycle not ready",
					humanText: message,
					data: { lifecycle, gateRefusal: true },
					effects: { sideEffectClass: "workspace-write" },
					nextSteps: [
						{ label: "Run verification", command: `/opsx:verify ${changeName}`, rationale: "Refresh assessment to unblock archive" },
					],
				});
			}

			const result = archiveChange(ctx.cwd, changeName);
			if (!result.archived) {
				return buildSlashCommandResult("opsx:archive", [changeName], {
					ok: false,
					summary: "Archive failed",
					humanText: result.operations.join("\n"),
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			if (changeInfo) {
				const archiveCandidates = emitArchiveCandidates({ ...changeInfo, stage: "archived" });
				if (archiveCandidates.length > 0) {
					(sharedState.lifecycleCandidateQueue ??= []).push({
						source: "openspec",
						context: `archive for '${changeName}'`,
						candidates: archiveCandidates,
					});
					result.operations.push(`Emitted ${archiveCandidates.length} lifecycle memory candidate(s)`);
				}
			}

			// Archive gate: transition implementing → implemented in design tree
			const transitioned = transitionDesignNodesOnArchive(ctx.cwd, changeName);
			if (transitioned.length > 0) {
				result.operations.push(
					`Transitioned design node${transitioned.length > 1 ? "s" : ""} to implemented: ${transitioned.join(", ")}`,
				);
			}

			// Auto-delete merged feature branches from transitioned design nodes
			const allBranches = resolveBoundDesignNodes(ctx.cwd, changeName)
				.flatMap((n) => n.branches ?? []);
			if (allBranches.length > 0) {
				const { deleted, skipped } = await deleteMergedBranches(pi, ctx.cwd, allBranches);
				if (deleted.length > 0) {
					result.operations.push(`Deleted merged branches: ${deleted.join(", ")}`);
				}
				if (skipped.length > 0) {
					result.operations.push(`Skipped unmerged/protected branches: ${skipped.join(", ")}`);
				}
			}

			emitOpenSpecState(ctx.cwd, pi);

			const summaryText = `Archived '${changeName}':\n${result.operations.map((op) => `  - ${op}`).join("\n")}`;

			return buildSlashCommandResult("opsx:archive", [changeName], {
				ok: true,
				summary: `Archived '${changeName}'`,
				humanText: summaryText,
				data: { operations: result.operations, transitionedNodes: transitioned },
				effects: {
					sideEffectClass: "workspace-write",
					filesChanged: [`openspec/archive/${changeName}`],
					lifecycleTouched: ["openspec", ...(transitioned.length > 0 ? ["design-tree"] : [])],
				},
				nextSteps: [
					{ label: "Change complete", rationale: "Specs merged to baseline" },
				],
			});
		},
		interactiveHandler: async (result, args, ctx) => {
			if (result.ok) {
				ctx.ui.notify(result.humanText, "info");
			} else {
				ctx.ui.notify(result.humanText, "warning");
			}
		},
	} satisfies BridgedSlashCommand);

	bridge.register(pi, {
		name: "opsx:apply",
		description: "Continue implementing a change (delegates to /cleave)",
		bridge: {
			agentCallable: true,
			sideEffectClass: "workspace-write",
		},
		structuredExecutor: async (args: string, ctx: SlashCommandExecutionContext) => {
			let changeName: string;
			if (ctx.bridgeInvocation) {
				try {
					const parsedArgs = JSON.parse((args || "").trim());
					changeName = parsedArgs[0] || "";
				} catch (e) {
					changeName = (args || "").trim();
				}
			} else {
				changeName = (args || "").trim();
			}
			
			if (!changeName) {
				return buildSlashCommandResult("opsx:apply", [], {
					ok: false,
					summary: "Usage: /opsx:apply <change-name>",
					humanText: "Error: change-name required",
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				return buildSlashCommandResult("opsx:apply", [changeName], {
					ok: false,
					summary: `Change '${changeName}' not found`,
					humanText: `Change '${changeName}' not found`,
					effects: { sideEffectClass: "workspace-write" },
				});
			}

			if (!change.hasTasks) {
				return buildSlashCommandResult("opsx:apply", [changeName], {
					ok: false,
					summary: `Change '${changeName}' has no tasks`,
					humanText: `Change '${changeName}' has no tasks. Run /opsx:ff first.`,
					effects: { sideEffectClass: "workspace-write" },
					nextSteps: [
						{ label: "Generate tasks", command: `/opsx:ff ${changeName}` },
					],
				});
			}

			const content = [
				`[OpenSpec: Apply \`${changeName}\`]`,
				"",
				`Continue implementing \`${changeName}\` — ${change.doneTasks}/${change.totalTasks} tasks done.`,
				"",
				"Use `/cleave` to parallelize remaining tasks, or work on them directly.",
			].join("\n");

			if (!ctx.bridgeInvocation) {
				pi.sendMessage({
					customType: "openspec-apply",
					content,
					display: true,
				}, { triggerTurn: true });
			}

			return buildSlashCommandResult("opsx:apply", [changeName], {
				ok: true,
				summary: `Apply requested for '${changeName}' (${change.doneTasks}/${change.totalTasks} tasks done)`,
				humanText: content,
				data: { changeName, doneTasks: change.doneTasks, totalTasks: change.totalTasks },
				effects: { sideEffectClass: "workspace-write" },
				nextSteps: [
					{ label: "Parallelize tasks", command: "/cleave", rationale: "Execute remaining tasks in parallel" },
					{ label: "Work directly", rationale: "Continue implementation manually" },
				],
			});
		},
		agentHandler: async (result, _args, _ctx) => {
			if (result.ok && result.humanText) {
				pi.sendMessage({
					customType: "openspec-apply",
					content: result.humanText,
					display: true,
				}, { triggerTurn: true });
			}
		},
	} satisfies BridgedSlashCommand);

	// ─── Message Renderers ───────────────────────────────────────────

	pi.registerMessageRenderer("openspec-created", (message, _options, theme) => {
		const content = (message.content as string) || "";
		return sciBanner("◎", "openspec:created", [content.split("\n")[0]], theme);
	});

	pi.registerMessageRenderer("openspec-status", (message, _options, theme) => {
		const content = (message.content as string) || "";
		const lines = content.split("\n").filter(Boolean).slice(0, 6);
		return sciBanner("◎", "openspec:status", lines, theme);
	});

	for (const type of ["openspec-spec-request", "openspec-ff-request", "openspec-verify", "openspec-apply"]) {
		pi.registerMessageRenderer(type, (message, _options, theme) => {
			const lines = ((message.content as string) || "").split("\n");
			const title = lines[0] || "";
			return sciBanner("◎", type.replace("openspec-", "openspec:"), [title], theme);
		});
	}
}

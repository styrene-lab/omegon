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

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { StringEnum } from "../lib/typebox-helpers.ts";
import { Text } from "@mariozechner/pi-tui";
import * as fs from "node:fs";
import * as path from "node:path";

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
} from "./spec.ts";
import { transitionDesignNodesOnArchive } from "./archive-gate.ts";
import { emitOpenSpecState } from "./dashboard-state.ts";
import {
	applyPostAssessReconciliation,
	evaluateLifecycleReconciliation,
	formatReconciliationIssues,
} from "./reconcile.ts";
import { scanDesignDocs } from "../design-tree/tree.ts";
import { emitDesignTreeState } from "../design-tree/dashboard-state.ts";

// ─── Extension ───────────────────────────────────────────────────────────────

export default function openspecExtension(pi: ExtensionAPI): void {

	// ─── Dashboard: emit on session start so dashboard has data immediately ───

	pi.on("session_start", async (_event, ctx) => {
		emitOpenSpecState(ctx.cwd, pi);
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

	// ─── Tool: openspec_manage ───────────────────────────────────────

	pi.registerTool({
		name: "openspec_manage",
		label: "OpenSpec",
		description:
			"Manage OpenSpec changes: create proposals, add specs, generate plans, check status, archive. " +
			"OpenSpec is the specification layer for spec-driven development.\n\n" +
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
			"Before implementing any multi-file change, create an OpenSpec change with a proposal and specs.",
			"Specs define what must be true BEFORE code is written — they are the source of truth for correctness.",
			"Use 'propose' to start a change, 'add_spec' or 'generate_spec' to define requirements with Given/When/Then scenarios.",
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
						return `${formatChangeStatus(c)}\n  ${nextStepHint(c)}`;
					});

					return {
						content: [{ type: "text", text: lines.join("\n\n") }],
						details: {
							changes: changes.map((c) => ({
								name: c.name,
								stage: c.stage,
								totalTasks: c.totalTasks,
								doneTasks: c.doneTasks,
								specCount: countScenarios(c.specs),
							})),
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

					lines.push("", nextStepHint(change));

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

					const result = applyPostAssessReconciliation(cwd, params.change_name, {
						assessmentKind: params.assessment_kind,
						outcome: params.outcome,
						summary: params.summary,
						changedFiles: params.changed_files,
						constraints: params.constraints,
					});

					emitOpenSpecState(cwd, pi);
					const tree = scanDesignDocs(path.join(cwd, "docs"));
					emitDesignTreeState(pi, tree, null);

					const lines = [
						`Post-assess reconciliation applied to '${params.change_name}'.`,
						"",
						`Outcome: ${result.outcome}`,
						`Lifecycle reopened: ${result.reopened ? "yes" : "no"}`,
						`Task state updated: ${result.updatedTaskState ? "yes" : "no"}`,
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
					if (result.warning) {
						lines.push("", `Warning: ${result.warning}`);
					}

					return {
						content: [{ type: "text", text: lines.join("\n") }],
						details: result,
					};
				}

				// ── archive ──────────────────────────────────────────
				case "archive": {
					if (!params.change_name) {
						return { content: [{ type: "text", text: "Error: change_name required" }], details: {}, isError: true };
					}

					const reconciliation = evaluateLifecycleReconciliation(cwd, params.change_name);
					if (reconciliation.issues.length > 0) {
						return {
							content: [{
								type: "text",
								text: [
									`Archive refused for '${params.change_name}' because lifecycle state is stale:`,
									"",
									formatReconciliationIssues(reconciliation.issues),
								].join("\n"),
							}],
							details: reconciliation,
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

					// Archive gate: transition implementing → implemented in design tree
					const transitioned = transitionDesignNodesOnArchive(cwd, params.change_name);
					if (transitioned.length > 0) {
						result.operations.push(
							`Transitioned design node${transitioned.length > 1 ? "s" : ""} to implemented: ${transitioned.join(", ")}`,
						);
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
			let text = theme.fg("toolTitle", theme.bold("openspec "));
			text += theme.fg("accent", args.action);
			if (args.change_name) text += " " + theme.fg("dim", args.change_name);
			return new Text(text, 0, 0);
		},

		renderResult(result, _opts, theme) {
			if ((result as any).isError) {
				const first = result.content?.[0];
				return new Text(theme.fg("error", (first && 'text' in first ? first.text : "Error")), 0, 0);
			}
			const first = result.content?.[0];
			const text = (first && 'text' in first ? first.text : null) || "Done";
			return new Text(theme.fg("success", text.split("\n")[0]), 0, 0);
		},
	});

	// ─── Commands ────────────────────────────────────────────────────

	pi.registerCommand("opsx:propose", {
		description: "Create a new OpenSpec change: /opsx:propose <name> <title>",
		handler: async (args, ctx) => {
			const parts = (args || "").trim().split(/\s+/);
			const name = parts[0];
			const title = parts.slice(1).join(" ");

			if (!name) {
				ctx.ui.notify("Usage: /opsx:propose <name> <title>", "warning");
				return;
			}

			const finalTitle = title || name;
			const intent = await ctx.ui.input("Intent (what this change accomplishes):");
			if (!intent) return;

			try {
				const result = createChange(ctx.cwd, name, finalTitle, intent);
				ctx.ui.notify(`Created: ${result.changePath}`, "info");

				pi.sendMessage({
					customType: "openspec-created",
					content: `Created OpenSpec change \`${path.basename(result.changePath)}\`.\n\n` +
						`Next step: Define specs with \`/opsx:spec ${path.basename(result.changePath)}\` ` +
						`or use \`openspec_manage\` with action \`generate_spec\` to scaffold Given/When/Then scenarios.`,
					display: true,
				}, { triggerTurn: false });
			} catch (e) {
				ctx.ui.notify(`Error: ${(e as Error).message}`, "error");
			}
		},
	});

	pi.registerCommand("opsx:spec", {
		description: "Generate or add specs for a change: /opsx:spec <change>",
		handler: async (args, ctx) => {
			const changeName = (args || "").trim();
			if (!changeName) {
				ctx.ui.notify("Usage: /opsx:spec <change-name>", "warning");
				return;
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				ctx.ui.notify(`Change '${changeName}' not found`, "error");
				return;
			}

			// Ask the agent to generate specs
			let proposalContent = "";
			if (change.hasProposal) {
				proposalContent = fs.readFileSync(path.join(change.path, "proposal.md"), "utf-8");
			}

			pi.sendMessage({
				customType: "openspec-spec-request",
				content: [
					`[OpenSpec: Generate specs for \`${changeName}\`]`,
					"",
					change.hasProposal ? `Proposal:\n${proposalContent.slice(0, 3000)}` : "No proposal found.",
					"",
					"Generate Given/When/Then specs for this change using `openspec_manage` with action `add_spec`.",
					"Each spec file should have:",
					"  - `## ADDED Requirements` section",
					"  - `### Requirement: <title>` for each requirement",
					"  - `#### Scenario: <title>` with Given/When/Then clauses",
					"",
					"Make scenarios specific and testable — they will drive the implementation and verification.",
				].join("\n"),
				display: true,
			}, { triggerTurn: true });
		},
	});

	pi.registerCommand("opsx:ff", {
		description: "Fast-forward: generate design + tasks from specs: /opsx:ff <change>",
		handler: async (args, ctx) => {
			const changeName = (args || "").trim();
			if (!changeName) {
				ctx.ui.notify("Usage: /opsx:ff <change-name>", "warning");
				return;
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				ctx.ui.notify(`Change '${changeName}' not found`, "error");
				return;
			}

			if (!change.hasSpecs && !change.hasProposal) {
				ctx.ui.notify("Change has no specs or proposal. Run /opsx:spec first.", "warning");
				return;
			}

			pi.sendMessage({
				customType: "openspec-ff-request",
				content: [
					`[OpenSpec: Fast-forward \`${changeName}\`]`,
					"",
					`Use \`openspec_manage\` with action \`fast_forward\` and change_name \`${changeName}\` ` +
					`to generate design.md and tasks.md from the specs.`,
					"",
					"Then present the generated tasks for review before running `/cleave`.",
				].join("\n"),
				display: true,
			}, { triggerTurn: true });
		},
	});

	pi.registerCommand("opsx:status", {
		description: "Show all active OpenSpec changes",
		handler: async (_args, ctx) => {
			const changes = listChanges(ctx.cwd);
			if (changes.length === 0) {
				ctx.ui.notify("No active OpenSpec changes. Use /opsx:propose to create one.", "info");
				return;
			}

			const lines = changes.map((c) => `${formatChangeStatus(c)}\n  → ${nextStepHint(c)}`);
			ctx.ui.notify(lines.join("\n\n"), "info");
		},
	});

	pi.registerCommand("opsx:verify", {
		description: "Check verification status of a change: /opsx:verify <change>",
		handler: async (args, ctx) => {
			const changeName = (args || "").trim();
			if (!changeName) {
				ctx.ui.notify("Usage: /opsx:verify <change-name>", "warning");
				return;
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				ctx.ui.notify(`Change '${changeName}' not found`, "error");
				return;
			}

			if (!change.hasSpecs) {
				ctx.ui.notify(`Change '${changeName}' has no specs to verify against`, "warning");
				return;
			}

			// Delegate to /assess spec which already handles spec verification
			pi.sendMessage({
				customType: "openspec-verify",
				content: [
					`[OpenSpec: Verify \`${changeName}\`]`,
					"",
					`Run \`/assess spec ${changeName}\` to verify the implementation against spec scenarios.`,
					"",
					`If all scenarios pass, the change is ready for \`/opsx:archive ${changeName}\`.`,
				].join("\n"),
				display: true,
			}, { triggerTurn: true });
		},
	});

	pi.registerCommand("opsx:archive", {
		description: "Archive a completed change: /opsx:archive <change>",
		handler: async (args, ctx) => {
			const changeName = (args || "").trim();
			if (!changeName) {
				ctx.ui.notify("Usage: /opsx:archive <change-name>", "warning");
				return;
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				ctx.ui.notify(`Change '${changeName}' not found`, "error");
				return;
			}

			const reconciliation = evaluateLifecycleReconciliation(ctx.cwd, changeName);
			if (reconciliation.issues.length > 0) {
				ctx.ui.notify(
					`Archive refused for '${changeName}' because lifecycle state is stale:\n${formatReconciliationIssues(reconciliation.issues)}`,
					"warning",
				);
				return;
			}

			const result = archiveChange(ctx.cwd, changeName);
			if (result.archived) {
				// Archive gate: transition implementing → implemented
				const transitioned = transitionDesignNodesOnArchive(ctx.cwd, changeName);
				if (transitioned.length > 0) {
					result.operations.push(
						`Transitioned design node${transitioned.length > 1 ? "s" : ""} to implemented: ${transitioned.join(", ")}`,
					);
				}
				ctx.ui.notify(
					`Archived '${changeName}':\n${result.operations.map((op) => `  - ${op}`).join("\n")}`,
					"info",
				);
			} else {
				ctx.ui.notify(result.operations.join("\n"), "error");
			}
		},
	});

	// Convenience: /opsx:apply delegates to /cleave
	pi.registerCommand("opsx:apply", {
		description: "Continue implementing a change (delegates to /cleave)",
		handler: async (args, ctx) => {
			const changeName = (args || "").trim();
			if (!changeName) {
				ctx.ui.notify("Usage: /opsx:apply <change-name>", "warning");
				return;
			}

			const change = getChange(ctx.cwd, changeName);
			if (!change) {
				ctx.ui.notify(`Change '${changeName}' not found`, "error");
				return;
			}

			if (!change.hasTasks) {
				ctx.ui.notify(`Change '${changeName}' has no tasks. Run /opsx:ff first.`, "warning");
				return;
			}

			pi.sendMessage({
				customType: "openspec-apply",
				content: [
					`[OpenSpec: Apply \`${changeName}\`]`,
					"",
					`Continue implementing \`${changeName}\` — ${change.doneTasks}/${change.totalTasks} tasks done.`,
					"",
					"Use `/cleave` to parallelize remaining tasks, or work on them directly.",
				].join("\n"),
				display: true,
			}, { triggerTurn: true });
		},
	});

	// ─── Message Renderers ───────────────────────────────────────────

	pi.registerMessageRenderer("openspec-created", (message, _options, theme) => {
		const text = theme.fg("success", theme.bold("◈ OpenSpec")) + " " +
			theme.fg("muted", (message.content as string).split("\n")[0]);
		return new Text(text, 0, 0);
	});

	pi.registerMessageRenderer("openspec-status", (message, _options, theme) => {
		const text = theme.fg("accent", theme.bold("◈ OpenSpec Status\n")) +
			theme.fg("muted", (message.content as string));
		return new Text(text, 0, 0);
	});

	for (const type of ["openspec-spec-request", "openspec-ff-request", "openspec-verify", "openspec-apply"]) {
		pi.registerMessageRenderer(type, (message, _options, theme) => {
			const lines = ((message.content as string) || "").split("\n");
			const title = lines[0] || "";
			const text = theme.fg("warning", theme.bold(title));
			return new Text(text, 0, 0);
		});
	}
}

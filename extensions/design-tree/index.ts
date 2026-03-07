/**
 * Design Tree Extension
 *
 * Codifies the interactive design paradigm:
 *   EXPLORE → RESEARCH → CRYSTALLIZE → BRANCH → RECURSE
 *
 * Provides two tools for agent autonomy:
 *   - design_tree       (queries: list, node, frontier, dependencies, children)
 *   - design_tree_update (mutations: create, set_status, add_question,
 *                         remove_question, add_research, add_decision,
 *                         add_dependency, branch, focus, unfocus, implement)
 *
 * Commands for interactive use:
 *   /design list|focus|unfocus|decide|explore|block|defer|branch|frontier|new|update|implement
 *
 * Documents use YAML frontmatter + structured body sections:
 *   ## Overview | ## Research | ## Decisions | ## Open Questions | ## Implementation Notes
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { StringEnum } from "../lib/typebox-helpers.js";
import { Text } from "@mariozechner/pi-tui";
import * as fs from "node:fs";
import * as path from "node:path";
import { execFileSync } from "node:child_process";

import { sharedState, DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import type { DesignTreeDashboardState } from "../shared-state.ts";

import type { DesignNode, DesignTree, NodeStatus } from "./types.js";
import { VALID_STATUSES, STATUS_ICONS, STATUS_COLORS } from "./types.js";
import {
	scanDesignDocs,
	getChildren,
	getAllOpenQuestions,
	getDocBody,
	getNodeSections,
	createNode,
	setNodeStatus,
	addOpenQuestion,
	removeOpenQuestion,
	addResearch,
	addDecision,
	addDependency,
	addRelated,
	addImplementationNotes,
	branchFromQuestion,
	toSlug,
	validateNodeId,
	scaffoldOpenSpecChange,
	matchBranchToNode,
	appendBranch,
	readGitBranch,
	sanitizeBranchName,
} from "./tree.js";

// ─── Extension ───────────────────────────────────────────────────────────────

export default function designTreeExtension(pi: ExtensionAPI): void {
	let tree: DesignTree = { nodes: new Map(), docsDir: "" };
	let focusedNode: string | null = null;

	function reload(cwd: string): void {
		const docsDir = path.join(cwd, "docs");
		tree = scanDesignDocs(docsDir);
	}

	function docsDir(cwd: string): string {
		return path.join(cwd, "docs");
	}

	// ─── Dashboard State Emitter ─────────────────────────────────────────

	function emitDesignTreeState(ctx: ExtensionContext, dt: DesignTree, focused: DesignNode | null): void {
		const nodes = Array.from(dt.nodes.values());
		const state: DesignTreeDashboardState = {
			nodeCount: nodes.length,
			decidedCount: nodes.filter((n) => n.status === "decided").length,
			exploringCount: nodes.filter((n) => n.status === "exploring" || n.status === "seed").length,
			implementingCount: nodes.filter((n) => n.status === "implementing").length,
			implementedCount: nodes.filter((n) => n.status === "implemented").length,
			blockedCount: nodes.filter((n) => n.status === "blocked").length,
			openQuestionCount: getAllOpenQuestions(dt).length,
			focusedNode: focused
				? {
						id: focused.id,
						title: focused.title,
						status: focused.status,
						questions: [...focused.open_questions],
						branch: focused.branches?.[0],
						branchCount: focused.branches?.length ?? 0,
					}
				: null,
			implementingNodes: nodes
				.filter((n) => n.status === "implementing")
				.map((n) => ({ id: n.id, title: n.title, branch: n.branches?.[0] })),
		};

		sharedState.designTree = state;

		// Notify dashboard for immediate re-render
		pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "design-tree" });
	}

	// ─── Implement Logic (shared between tool and command) ───────────────

	interface ImplementResult {
		ok: boolean;
		message: string;
		branch?: string;
		changePath?: string;
		files?: string[];
	}

	function executeImplement(cwd: string, node: DesignNode, branchPrefix: string = "feature"): ImplementResult {
		// Scaffold OpenSpec change
		const result = scaffoldOpenSpecChange(cwd, tree, node);

		// C2: Check if scaffold actually created files — bail if already exists
		if (result.files.length === 0) {
			return { ok: false, message: result.message };
		}

		// Determine branch name
		const branchName = node.branches?.length > 0
			? node.branches[0]
			: `${branchPrefix}/${node.id}`;

		// C1: Validate branch name before shell operations
		const safeBranch = sanitizeBranchName(branchName);
		if (!safeBranch) {
			return { ok: false, message: `Invalid branch name: '${branchName}' — contains disallowed characters` };
		}

		// Atomic write: status + openspec_change + branch in one pass
		setNodeStatus(node, "implementing");
		appendBranch({ ...node, status: "implementing" }, safeBranch);

		// Write openspec_change field
		let content = fs.readFileSync(node.filePath, "utf-8");
		if (!content.includes("openspec_change:")) {
			content = content.replace(
				/^(---\n[\s\S]*?)(---\n)/m,
				`$1openspec_change: ${node.id}\n$2`,
			);
			fs.writeFileSync(node.filePath, content);
		}

		// Create git branch — use execFileSync (array args, no shell interpolation)
		try {
			execFileSync("git", ["checkout", "-b", safeBranch], { cwd, stdio: "pipe" });
		} catch {
			try {
				execFileSync("git", ["checkout", safeBranch], { cwd, stdio: "pipe" });
			} catch {
				// Non-fatal — branch operations may fail in worktrees or CI
			}
		}

		return {
			ok: true,
			message: result.message + `\n\nStatus: implementing\nBranch: ${safeBranch}\nOpenSpec change: ${node.id}`,
			branch: safeBranch,
			changePath: result.changePath,
			files: result.files,
		};
	}

	// ─── Tool: design_tree (queries) ─────────────────────────────────────

	pi.registerTool({
		name: "design_tree",
		label: "Design Tree",
		description:
			"Query the design tree: list nodes, get node details with structured sections, " +
			"find open questions (frontier), check dependencies, list children. " +
			"Documents have structured sections: Overview, Research, Decisions, Open Questions, Implementation Notes.",
		promptSnippet: "Query the design exploration tree — nodes, status, open questions, dependencies, structured content",
		promptGuidelines: [
			"Use design_tree to check the state of design documents before creating or modifying them",
			"When the user says 'let's explore X', use design_tree to find the relevant node and its open questions",
			"After a design discussion converges, use design_tree_update with action 'set_status' to mark the node as decided",
			"When discussion reveals new sub-topics, use design_tree_update with action 'branch' to create child nodes",
			"Use action 'node' to read the full structured content (research, decisions, implementation notes)",
		],
		parameters: Type.Object({
			action: StringEnum(["list", "node", "frontier", "dependencies", "children"] as const),
			node_id: Type.Optional(
				Type.String({ description: "Node ID (required for node, dependencies, children)" }),
			),
		}),
		async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
			reload(ctx.cwd);

			switch (params.action) {
				case "list": {
					const nodes = Array.from(tree.nodes.values()).map((n) => ({
						id: n.id,
						title: n.title,
						status: n.status,
						parent: n.parent || null,
						tags: n.tags,
						open_questions: n.open_questions.length,
						dependencies: n.dependencies,
					}));
					return {
						content: [{ type: "text", text: JSON.stringify(nodes, null, 2) }],
						details: { nodes },
					};
				}

				case "node": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const sections = getNodeSections(node);
					const children = getChildren(tree, node.id).map((c) => ({ id: c.id, title: c.title, status: c.status }));

					const result = {
						id: node.id,
						title: node.title,
						status: node.status,
						parent: node.parent || null,
						dependencies: node.dependencies,
						related: node.related,
						tags: node.tags,
						children,
						sections: {
							overview: sections.overview,
							research: sections.research,
							decisions: sections.decisions,
							openQuestions: sections.openQuestions,
							implementationNotes: {
								fileScope: sections.implementationNotes.fileScope,
								constraints: sections.implementationNotes.constraints,
							},
							extraSections: sections.extraSections.map((s) => s.heading),
						},
					};

					// Also include the raw body for the LLM to reference
					const body = getDocBody(node.filePath, 8000);

					return {
						content: [{
							type: "text",
							text: JSON.stringify(result, null, 2) + "\n\n--- Document Content ---\n\n" + body,
						}],
						details: { node: result },
					};
				}

				case "frontier": {
					const questions = getAllOpenQuestions(tree);
					const grouped: Record<string, string[]> = {};
					for (const { node, question } of questions) {
						if (!grouped[node.id]) grouped[node.id] = [];
						grouped[node.id].push(question);
					}
					return {
						content: [{
							type: "text",
							text:
								`${questions.length} open questions across ${Object.keys(grouped).length} nodes:\n\n` +
								Object.entries(grouped)
									.map(
										([id, qs]) =>
											`## ${tree.nodes.get(id)?.title || id}\n${qs.map((q, i) => `  ${i + 1}. ${q}`).join("\n")}`,
									)
									.join("\n\n"),
						}],
						details: { questions: grouped },
					};
				}

				case "dependencies": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const deps = node.dependencies
						.map((id) => tree.nodes.get(id))
						.filter(Boolean)
						.map((n) => ({ id: n!.id, title: n!.title, status: n!.status }));
					const dependents = Array.from(tree.nodes.values())
						.filter((n) => n.dependencies.includes(params.node_id!))
						.map((n) => ({ id: n.id, title: n.title, status: n.status }));

					return {
						content: [{
							type: "text",
							text: `Dependencies of ${node.title}:\n` +
								JSON.stringify({ depends_on: deps, depended_by: dependents }, null, 2),
						}],
						details: { depends_on: deps, depended_by: dependents },
					};
				}

				case "children": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const children = getChildren(tree, params.node_id).map((c) => ({
						id: c.id,
						title: c.title,
						status: c.status,
						open_questions: c.open_questions.length,
					}));
					return {
						content: [{ type: "text", text: `Children of ${params.node_id}:\n${JSON.stringify(children, null, 2)}` }],
						details: { children },
					};
				}
			}

			return { content: [{ type: "text", text: "Unknown action" }], details: {} };
		},

		renderCall(args, theme) {
			let text = theme.fg("toolTitle", theme.bold("design_tree "));
			text += theme.fg("accent", args.action);
			if (args.node_id) text += " " + theme.fg("dim", args.node_id);
			return new Text(text, 0, 0);
		},

		renderResult(result, { expanded }, theme) {
			if (result.isError) {
				return new Text(theme.fg("error", result.content?.[0]?.text || "Error"), 0, 0);
			}
			const details = result.details || {};
			let text = "";

			if (details.nodes) {
				const nodes = details.nodes as Array<{ id: string; status: string; open_questions: number }>;
				text = theme.fg("success", `${nodes.length} nodes`) + "\n";
				if (expanded) {
					for (const n of nodes) {
						const icon = STATUS_ICONS[n.status as NodeStatus] || "?";
						const color = STATUS_COLORS[n.status as NodeStatus] || "muted";
						text += theme.fg(color as Parameters<typeof theme.fg>[0], `  ${icon} ${n.id}`) +
							(n.open_questions > 0 ? theme.fg("dim", ` [${n.open_questions}?]`) : "") + "\n";
					}
				}
			} else if (details.node) {
				const n = details.node as { title: string; status: NodeStatus; sections?: { openQuestions?: string[] } };
				text = theme.fg("accent", `${STATUS_ICONS[n.status]} ${n.title}`) +
					theme.fg("muted", ` (${n.status})`);
				const qCount = n.sections?.openQuestions?.length || 0;
				if (qCount > 0) text += theme.fg("dim", ` — ${qCount} questions`);
			} else if (details.questions) {
				const q = details.questions as Record<string, string[]>;
				const total = Object.values(q).flat().length;
				text = theme.fg("warning", `${total} open questions`);
			} else {
				text = result.content?.[0]?.text?.slice(0, 100) || "Done";
			}

			return new Text(text, 0, 0);
		},
	});

	// ─── Tool: design_tree_update (mutations) ────────────────────────────

	pi.registerTool({
		name: "design_tree_update",
		label: "Design Tree Update",
		description:
			"Mutate the design tree: create nodes, change status, add/remove questions, " +
			"add research findings, record decisions, add dependencies, branch from questions, " +
			"set focus, or bridge to OpenSpec for implementation.\n\n" +
			"Actions:\n" +
			"- create: Create a new design node (id, title required; parent, status, tags, overview optional)\n" +
			"- set_status: Change node status (seed/exploring/decided/blocked/deferred)\n" +
			"- add_question: Add an open question to a node\n" +
			"- remove_question: Remove an open question by text\n" +
			"- add_research: Add a research entry (heading + content)\n" +
			"- add_decision: Record a design decision (title, status, rationale)\n" +
			"- add_dependency: Add a dependency between nodes\n" +
			"- add_related: Add a related node reference\n" +
			"- add_impl_notes: Add implementation notes (file_scope, constraints)\n" +
			"- branch: Create a child node from a parent's open question\n" +
			"- focus: Set the focused design node for context injection\n" +
			"- unfocus: Clear the focused node\n" +
			"- implement: Bridge a decided node to OpenSpec — scaffold a change directory",
		promptSnippet:
			"Mutate the design tree — create nodes, set status, add research/decisions/questions, branch, implement",
		promptGuidelines: [
			"Use 'create' to start a new design exploration. Status defaults to 'seed'.",
			"Use 'set_status' to transition nodes: seed → exploring → decided. Use 'blocked' or 'deferred' as needed.",
			"Use 'add_question' when discussion reveals unknowns. Use 'remove_question' when questions are answered.",
			"Use 'add_research' to record findings with a heading and content.",
			"Use 'add_decision' to crystallize choices with title, status (exploring/decided/rejected), and rationale.",
			"Use 'branch' to spawn a child node from a parent's open question — this removes the question from the parent.",
			"Use 'focus' to set which node's context gets injected into the conversation.",
			"Use 'implement' on a decided node to generate an OpenSpec change directory for cleave execution.",
			"When an OpenSpec change exists for a decided node, suggest `/cleave` to parallelize the implementation.",
		],
		parameters: Type.Object({
			action: StringEnum([
				"create", "set_status", "add_question", "remove_question",
				"add_research", "add_decision", "add_dependency", "add_related",
				"add_impl_notes", "branch", "focus", "unfocus", "implement",
			] as const),
			node_id: Type.Optional(Type.String({ description: "Target node ID (required for most actions)" })),
			// create params
			title: Type.Optional(Type.String({ description: "Node title (for create)" })),
			parent: Type.Optional(Type.String({ description: "Parent node ID (for create)" })),
			status: Type.Optional(Type.String({ description: "Node status (for create, set_status)" })),
			tags: Type.Optional(Type.Array(Type.String(), { description: "Tags (for create)" })),
			overview: Type.Optional(Type.String({ description: "Overview text (for create)" })),
			// question params
			question: Type.Optional(Type.String({ description: "Question text (for add_question, remove_question, branch)" })),
			// research params
			heading: Type.Optional(Type.String({ description: "Research heading (for add_research)" })),
			content: Type.Optional(Type.String({ description: "Content text (for add_research)" })),
			// decision params
			decision_title: Type.Optional(Type.String({ description: "Decision title (for add_decision)" })),
			decision_status: Type.Optional(Type.String({ description: "exploring|decided|rejected (for add_decision)" })),
			rationale: Type.Optional(Type.String({ description: "Decision rationale (for add_decision)" })),
			// dependency / related
			target_id: Type.Optional(Type.String({ description: "Target node ID (for add_dependency, add_related)" })),
			// branch params
			child_id: Type.Optional(Type.String({ description: "Child node ID (for branch)" })),
			child_title: Type.Optional(Type.String({ description: "Child node title (for branch)" })),
			// impl notes
			file_scope: Type.Optional(
				Type.Array(
					Type.Object({
						path: Type.String(),
						description: Type.String(),
						action: Type.Optional(StringEnum(["new", "modified", "deleted"] as const)),
					}),
					{ description: "File scope entries (for add_impl_notes)" },
				),
			),
			constraints: Type.Optional(Type.Array(Type.String(), { description: "Constraints (for add_impl_notes)" })),
		}),

		async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
			reload(ctx.cwd);
			const dd = docsDir(ctx.cwd);

			switch (params.action) {
				// ── create ────────────────────────────────────────────────
				case "create": {
					if (!params.node_id || !params.title) {
						return { content: [{ type: "text", text: "Error: node_id and title required for create" }], details: {}, isError: true };
					}
					const idError = validateNodeId(params.node_id);
					if (idError) {
						return { content: [{ type: "text", text: `Error: ${idError}` }], details: {}, isError: true };
					}
					if (tree.nodes.has(params.node_id)) {
						return { content: [{ type: "text", text: `Error: node '${params.node_id}' already exists` }], details: {}, isError: true };
					}
					const validStatus = params.status && VALID_STATUSES.includes(params.status as NodeStatus)
						? params.status as NodeStatus
						: "seed";

					const node = createNode(dd, {
						id: params.node_id,
						title: params.title,
						parent: params.parent,
						status: validStatus,
						tags: params.tags,
						overview: params.overview,
					});

					reload(ctx.cwd);
					focusedNode = params.node_id;
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);

					return {
						content: [{ type: "text", text: `Created design node '${node.title}' (${node.status}) at ${node.filePath}` }],
						details: { node: { id: node.id, title: node.title, status: node.status, filePath: node.filePath } },
					};
				}

				// ── set_status ────────────────────────────────────────────
				case "set_status": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const newStatus = params.status as NodeStatus;
					if (!newStatus || !VALID_STATUSES.includes(newStatus)) {
						return { content: [{ type: "text", text: `Invalid status '${params.status}'. Valid: ${VALID_STATUSES.join(", ")}` }], details: {}, isError: true };
					}
					const oldStatus = node.status;
					const updated = setNodeStatus(node, newStatus);
					tree.nodes.set(updated.id, updated);
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);

					let text = `${STATUS_ICONS[newStatus]} '${node.title}': ${oldStatus} → ${newStatus}`;

					// If transitioning to decided, check for OpenSpec bridge opportunity
					if (newStatus === "decided") {
						const sections = getNodeSections(node);
						const hasDecisions = sections.decisions.length > 0;
						const hasImplNotes = sections.implementationNotes.fileScope.length > 0 ||
							sections.implementationNotes.constraints.length > 0;

						if (hasDecisions || hasImplNotes) {
							text += "\n\nThis node has decisions and/or implementation notes. " +
								"Use design_tree_update with action 'implement' to scaffold an OpenSpec change, " +
								"then `/cleave` to parallelize the implementation.";
						} else {
							text += "\n\nConsider adding decisions and implementation notes before implementing.";
						}
					}

					return {
						content: [{ type: "text", text }],
						details: { id: node.id, oldStatus, newStatus },
					};
				}

				// ── add_question ──────────────────────────────────────────
				case "add_question": {
					if (!params.node_id || !params.question) {
						return { content: [{ type: "text", text: "Error: node_id and question required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const updated = addOpenQuestion(node, params.question);
					tree.nodes.set(updated.id, updated);
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					return {
						content: [{ type: "text", text: `Added question to '${node.title}': ${params.question}` }],
						details: { id: node.id, question: params.question, totalQuestions: updated.open_questions.length },
					};
				}

				// ── remove_question ───────────────────────────────────────
				case "remove_question": {
					if (!params.node_id || !params.question) {
						return { content: [{ type: "text", text: "Error: node_id and question required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const updated = removeOpenQuestion(node, params.question);
					tree.nodes.set(updated.id, updated);
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					return {
						content: [{ type: "text", text: `Removed question from '${node.title}'` }],
						details: { id: node.id, remainingQuestions: updated.open_questions.length },
					};
				}

				// ── add_research ──────────────────────────────────────────
				case "add_research": {
					if (!params.node_id || !params.heading || !params.content) {
						return { content: [{ type: "text", text: "Error: node_id, heading, and content required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					addResearch(node, params.heading, params.content);
					return {
						content: [{ type: "text", text: `Added research '${params.heading}' to '${node.title}'` }],
						details: { id: node.id, heading: params.heading },
					};
				}

				// ── add_decision ──────────────────────────────────────────
				case "add_decision": {
					if (!params.node_id || !params.decision_title) {
						return { content: [{ type: "text", text: "Error: node_id and decision_title required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const validDecisionStatuses = ["exploring", "decided", "rejected"];
					const rawDStatus = params.decision_status || "exploring";
					if (!validDecisionStatuses.includes(rawDStatus)) {
						return { content: [{ type: "text", text: `Invalid decision_status '${rawDStatus}'. Valid: ${validDecisionStatuses.join(", ")}` }], details: {}, isError: true };
					}
					const dStatus = rawDStatus as "exploring" | "decided" | "rejected";
					addDecision(node, {
						title: params.decision_title,
						status: dStatus,
						rationale: params.rationale || "",
					});
					return {
						content: [{ type: "text", text: `Added decision '${params.decision_title}' (${dStatus}) to '${node.title}'` }],
						details: { id: node.id, decision: params.decision_title, status: dStatus },
					};
				}

				// ── add_dependency ────────────────────────────────────────
				case "add_dependency": {
					if (!params.node_id || !params.target_id) {
						return { content: [{ type: "text", text: "Error: node_id and target_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					if (!tree.nodes.has(params.target_id)) {
						return { content: [{ type: "text", text: `Target node '${params.target_id}' not found` }], details: {}, isError: true };
					}
					const updated = addDependency(node, params.target_id);
					tree.nodes.set(updated.id, updated);
					return {
						content: [{ type: "text", text: `Added dependency: '${node.title}' depends on '${params.target_id}'` }],
						details: { id: node.id, dependency: params.target_id },
					};
				}

				// ── add_related ───────────────────────────────────────────
				case "add_related": {
					if (!params.node_id || !params.target_id) {
						return { content: [{ type: "text", text: "Error: node_id and target_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const targetNode = tree.nodes.get(params.target_id);
					if (!targetNode) {
						return { content: [{ type: "text", text: `Target node '${params.target_id}' not found` }], details: {}, isError: true };
					}
					const updated = addRelated(node, params.target_id, targetNode);
					tree.nodes.set(updated.id, updated);
					return {
						content: [{ type: "text", text: `Added related: '${node.title}' ↔ '${targetNode.title}' (bidirectional)` }],
						details: { id: node.id, related: params.target_id },
					};
				}

				// ── add_impl_notes ────────────────────────────────────────
				case "add_impl_notes": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					if (!params.file_scope && !params.constraints) {
						return { content: [{ type: "text", text: "Error: at least one of file_scope or constraints required" }], details: {}, isError: true };
					}
					addImplementationNotes(node, {
						fileScope: params.file_scope,
						constraints: params.constraints,
					});
					const added: string[] = [];
					if (params.file_scope) added.push(`${params.file_scope.length} file scope entries`);
					if (params.constraints) added.push(`${params.constraints.length} constraints`);
					return {
						content: [{ type: "text", text: `Added implementation notes to '${node.title}': ${added.join(", ")}` }],
						details: { id: node.id },
					};
				}

				// ── branch ───────────────────────────────────────────────
				case "branch": {
					if (!params.node_id || !params.question) {
						return { content: [{ type: "text", text: "Error: node_id and question required for branch" }], details: {}, isError: true };
					}
					const childId = params.child_id || toSlug(params.question);
					const childTitle = params.child_title || params.question.slice(0, 60);

					const child = branchFromQuestion(tree, params.node_id, params.question, childId, childTitle);
					if (!child) {
						return { content: [{ type: "text", text: `Could not branch: node '${params.node_id}' not found or question not present` }], details: {}, isError: true };
					}

					reload(ctx.cwd);
					focusedNode = childId;
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);

					return {
						content: [{
							type: "text",
							text: `Branched '${childTitle}' from '${params.node_id}' — question moved to child node.\n` +
								`File: ${child.filePath}\n` +
								`Focus set to new node. Use design_tree with action 'node' to see its content.`,
						}],
						details: { child: { id: child.id, title: child.title, parent: params.node_id } },
					};
				}

				// ── focus / unfocus ──────────────────────────────────────
				case "focus": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required for focus" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					focusedNode = params.node_id;

					// Auto-transition seed → exploring
					if (node.status === "seed") {
						const updated = setNodeStatus(node, "exploring");
						tree.nodes.set(updated.id, updated);
					}

					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					return {
						content: [{ type: "text", text: `Focused on '${node.title}'. Context will be injected on next turn.` }],
						details: { focusedNode: params.node_id },
					};
				}

				case "unfocus": {
					focusedNode = null;
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					return {
						content: [{ type: "text", text: "Design focus cleared." }],
						details: { focusedNode: null },
					};
				}

				// ── implement ────────────────────────────────────────────
				case "implement": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required for implement" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					if (node.status !== "decided") {
						return {
							content: [{
								type: "text",
								text: `Node '${node.title}' is '${node.status}', not 'decided'. ` +
									`Resolve open questions and set status to 'decided' before implementing.`,
							}],
							details: {},
							isError: true,
						};
					}

					const implResult = executeImplement(ctx.cwd, node);
					reload(ctx.cwd);
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);

					return {
						content: [{ type: "text", text: implResult.message }],
						details: implResult.ok
							? { changePath: implResult.changePath, files: implResult.files, branch: implResult.branch }
							: {},
						isError: !implResult.ok,
					};
				}
			}

			return { content: [{ type: "text", text: "Unknown action" }], details: {} };
		},

		renderCall(args, theme) {
			let text = theme.fg("toolTitle", theme.bold("design_tree_update "));
			text += theme.fg("warning", args.action);
			if (args.node_id) text += " " + theme.fg("dim", args.node_id);
			return new Text(text, 0, 0);
		},

		renderResult(result, _opts, theme) {
			if (result.isError) {
				return new Text(theme.fg("error", result.content?.[0]?.text || "Error"), 0, 0);
			}
			const text = result.content?.[0]?.text || "Done";
			// Show first line only in collapsed view
			const firstLine = text.split("\n")[0];
			return new Text(theme.fg("success", firstLine), 0, 0);
		},
	});

	// ─── Commands (interactive) ──────────────────────────────────────────

	pi.registerCommand("design", {
		description:
			"Design tree: list | focus [id] | unfocus | decide [id] | explore [id] | " +
			"block [id] | defer [id] | branch | frontier | new <id> <title> | " +
			"update [id] | implement [id]",
		getArgumentCompletions: (prefix: string) => {
			const subcommands = [
				"list", "focus", "unfocus", "decide", "explore",
				"block", "defer", "branch", "frontier", "new", "update",
				"implement",
			];
			const parts = prefix.split(" ");
			if (parts.length <= 1) {
				return subcommands
					.filter((s) => s.startsWith(prefix))
					.map((s) => ({ value: s, label: s }));
			}
			const sub = parts[0];
			if (["focus", "decide", "explore", "block", "defer", "update", "implement"].includes(sub) && parts.length === 2) {
				const partial = parts[1] || "";
				return Array.from(tree.nodes.keys())
					.filter((id) => id.startsWith(partial))
					.map((id) => {
						const node = tree.nodes.get(id)!;
						return { value: `${sub} ${id}`, label: `${id} — ${node.title} (${node.status})` };
					});
			}
			return null;
		},
		handler: async (args, ctx) => {
			reload(ctx.cwd);
			const parts = (args || "list").trim().split(/\s+/);
			const subcommand = parts[0];

			switch (subcommand) {
				case "list": {
					if (tree.nodes.size === 0) {
						ctx.ui.notify("No design documents found in docs/. Create one with /design new <id> <title>", "info");
						return;
					}
					const total = tree.nodes.size;
					const decided = Array.from(tree.nodes.values()).filter((n) => n.status === "decided").length;
					const exploring = Array.from(tree.nodes.values()).filter(
						(n) => n.status === "exploring" || n.status === "seed",
					).length;
					const blocked = Array.from(tree.nodes.values()).filter((n) => n.status === "blocked").length;
					const openQ = getAllOpenQuestions(tree).length;

					const lines = [`${decided}/${total} decided, ${exploring} exploring, ${openQ} open questions`];
					if (blocked > 0) lines[0] += `, ${blocked} blocked`;

					const byStatus = new Map<string, DesignNode[]>();
					for (const node of tree.nodes.values()) {
						const list = byStatus.get(node.status) || [];
						list.push(node);
						byStatus.set(node.status, list);
					}
					for (const [status, nodes] of byStatus) {
						const icon = STATUS_ICONS[status as NodeStatus];
						const names = nodes.map((n) => n.title).join(", ");
						lines.push(`${icon} ${status}: ${names}`);
					}

					if (focusedNode) {
						const node = tree.nodes.get(focusedNode);
						if (node) lines.push(`▸ Focused: ${node.title}`);
					}

					ctx.ui.notify(lines.join("\n"), "info");
					break;
				}

				case "focus": {
					const id = parts[1];
					if (!id) {
						const ids = Array.from(tree.nodes.keys());
						if (ids.length === 0) {
							ctx.ui.notify("No design nodes to focus on", "info");
							return;
						}
						const labels = ids.map((nid) => {
							const n = tree.nodes.get(nid)!;
							const icon = STATUS_ICONS[n.status];
							return `${icon} ${nid} — ${n.title} (${n.open_questions.length}?)`;
						});
						const choice = await ctx.ui.select("Focus on which node?", labels);
						if (!choice) return;
						focusedNode = choice.split(" — ")[0].replace(/^[◌◐●✕◑]\s*/, "");
					} else {
						const node = tree.nodes.get(id);
						if (!node) {
							ctx.ui.notify(`Node '${id}' not found`, "error");
							return;
						}
						focusedNode = id;
						if (node.status === "seed") {
							setNodeStatus(node, "exploring");
							ctx.ui.notify(`${node.title}: seed → exploring`, "info");
						}
					}
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);

					const node = tree.nodes.get(focusedNode!)!;
					const openQ = node.open_questions.length > 0
						? `\n\nOpen questions:\n${node.open_questions.map((q, i) => `${i + 1}. ${q}`).join("\n")}`
						: "";

					pi.sendMessage(
						{
							customType: "design-focus",
							content: `[Design Focus: ${node.title} (${node.status})]${openQ}\n\nLet's explore this design space.`,
							display: true,
						},
						{ triggerTurn: false },
					);
					break;
				}

				case "unfocus": {
					focusedNode = null;
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					ctx.ui.notify("Design focus cleared", "info");
					break;
				}

				case "decide":
				case "explore":
				case "block":
				case "defer": {
					const statusMap: Record<string, NodeStatus> = {
						decide: "decided", explore: "exploring", block: "blocked", defer: "deferred",
					};
					const id = parts[1] || focusedNode;
					if (!id) {
						ctx.ui.notify(`Usage: /design ${subcommand} <node-id>`, "warning");
						return;
					}
					const node = tree.nodes.get(id);
					if (!node) {
						ctx.ui.notify(`Node '${id}' not found`, "error");
						return;
					}
					const newStatus = statusMap[subcommand];
					setNodeStatus(node, newStatus);
					if (subcommand === "explore") focusedNode = id;
					reload(ctx.cwd);
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					ctx.ui.notify(`${STATUS_ICONS[newStatus]} '${node.title}' → ${newStatus}`, "success");
					break;
				}

				case "frontier": {
					const questions = getAllOpenQuestions(tree);
					if (questions.length === 0) {
						ctx.ui.notify("No open questions in the design tree", "info");
						return;
					}
					const items = questions.map(({ node, question }) => `[${node.id}] ${question}`);
					const choice = await ctx.ui.select(`Open Questions (${questions.length}):`, items);
					if (choice) {
						const match = choice.match(/^\[([^\]]+)\]/);
						if (match) {
							focusedNode = match[1];
							emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
							const node = tree.nodes.get(match[1])!;
							const question = choice.replace(/^\[[^\]]+\]\s*/, "");
							pi.sendMessage(
								{
									customType: "design-frontier",
									content: `[Exploring open question from ${node.title}]\n\nQuestion: ${question}\n\nLet's dig into this.`,
									display: true,
								},
								{ triggerTurn: true },
							);
						}
					}
					break;
				}

				case "branch": {
					let nodeId = focusedNode;
					if (!nodeId) {
						const ids = Array.from(tree.nodes.keys());
						const labels = ids.map((id) => {
							const n = tree.nodes.get(id)!;
							return `${id} — ${n.title} (${n.open_questions.length} questions)`;
						});
						const choice = await ctx.ui.select("Branch from which node?", labels);
						if (!choice) return;
						nodeId = choice.split(" — ")[0];
					}
					const node = tree.nodes.get(nodeId);
					if (!node) return;
					if (node.open_questions.length === 0) {
						ctx.ui.notify(`${node.title} has no open questions to branch from`, "info");
						return;
					}
					const selected = await ctx.ui.select(
						`Branch from '${node.title}' — select a question:`,
						node.open_questions,
					);
					if (!selected) return;

					const suggestedId = toSlug(selected);
					const newId = await ctx.ui.input("Node ID:", suggestedId);
					if (!newId) return;
					const newTitle = await ctx.ui.input("Title:", selected.slice(0, 60));
					if (!newTitle) return;

					branchFromQuestion(tree, nodeId, selected, newId, newTitle);
					reload(ctx.cwd);
					focusedNode = newId;
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					ctx.ui.notify(`Created ${newId}.md — branched from ${node.title}`, "success");
					break;
				}

				case "new": {
					const id = parts[1];
					const title = parts.slice(2).join(" ");
					if (!id || !title) {
						ctx.ui.notify("Usage: /design new <id> <title>", "warning");
						return;
					}
					const idErr = validateNodeId(id);
					if (idErr) {
						ctx.ui.notify(`Invalid node ID '${id}': ${idErr}`, "error");
						return;
					}
					createNode(docsDir(ctx.cwd), { id, title });
					reload(ctx.cwd);
					focusedNode = id;
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					ctx.ui.notify(`Created ${id}.md`, "success");
					break;
				}

				case "update": {
					const id = parts[1] || focusedNode;
					if (!id) {
						ctx.ui.notify("Usage: /design update <node-id>", "warning");
						return;
					}
					const node = tree.nodes.get(id);
					if (!node) {
						ctx.ui.notify(`Node '${id}' not found`, "error");
						return;
					}

					const action = await ctx.ui.select(`Update '${node.title}':`, [
						"Add open question",
						"Remove open question",
						"Add dependency",
						"Add related node",
					]);
					if (!action) return;

					if (action === "Add open question") {
						const question = await ctx.ui.input("New open question:");
						if (!question) return;
						addOpenQuestion(node, question);
						reload(ctx.cwd);
						emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
						ctx.ui.notify(`Added question to ${node.title}`, "success");
					} else if (action === "Remove open question") {
						if (node.open_questions.length === 0) {
							ctx.ui.notify("No open questions to remove", "info");
							return;
						}
						const toRemove = await ctx.ui.select("Remove which question?", node.open_questions);
						if (!toRemove) return;
						removeOpenQuestion(node, toRemove);
						reload(ctx.cwd);
						emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
						ctx.ui.notify(`Removed question from ${node.title}`, "success");
					} else if (action === "Add dependency") {
						const otherNodes = Array.from(tree.nodes.keys()).filter(
							(nid) => nid !== id && !node.dependencies.includes(nid),
						);
						if (otherNodes.length === 0) {
							ctx.ui.notify("No available nodes to add as dependency", "info");
							return;
						}
						const labels = otherNodes.map((nid) => {
							const n = tree.nodes.get(nid)!;
							return `${nid} — ${n.title}`;
						});
						const choice = await ctx.ui.select("Add dependency:", labels);
						if (!choice) return;
						addDependency(node, choice.split(" — ")[0]);
						reload(ctx.cwd);
						emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
						ctx.ui.notify(`Added dependency: ${choice.split(" — ")[0]}`, "success");
					} else if (action === "Add related node") {
						const otherNodes = Array.from(tree.nodes.keys()).filter(
							(nid) => nid !== id && !node.related.includes(nid),
						);
						if (otherNodes.length === 0) {
							ctx.ui.notify("No available nodes to add as related", "info");
							return;
						}
						const labels = otherNodes.map((nid) => {
							const n = tree.nodes.get(nid)!;
							return `${nid} — ${n.title}`;
						});
						const choice = await ctx.ui.select("Add related:", labels);
						if (!choice) return;
						const relatedId = choice.split(" — ")[0];
						const targetNode = tree.nodes.get(relatedId);
						addRelated(node, relatedId, targetNode);
						reload(ctx.cwd);
						emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
						ctx.ui.notify(`Added related: ${relatedId} (bidirectional)`, "success");
					}
					break;
				}

				case "implement": {
					const id = parts[1] || focusedNode;
					if (!id) {
						ctx.ui.notify("Usage: /design implement <node-id>", "warning");
						return;
					}
					const node = tree.nodes.get(id);
					if (!node) {
						ctx.ui.notify(`Node '${id}' not found`, "error");
						return;
					}
					if (node.status !== "decided") {
						ctx.ui.notify(
							`'${node.title}' is '${node.status}', not 'decided'. Resolve questions first.`,
							"warning",
						);
						return;
					}

					const implResult = executeImplement(ctx.cwd, node);
					reload(ctx.cwd);
					emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
					ctx.ui.notify(implResult.message, implResult.ok ? "success" : "error");
					break;
				}

				default:
					ctx.ui.notify(
						"Subcommands: list, focus, unfocus, decide, explore, block, defer, branch, frontier, new, update, implement",
						"info",
					);
			}
		},
	});

	// ─── Context Injection ───────────────────────────────────────────────

	pi.on("before_agent_start", async (_event, ctx) => {
		reload(ctx.cwd);

		// Auto-associate branch on every turn (catches branch switches)
		tryAssociateBranch(ctx);

		if (tree.nodes.size === 0) return;

		if (focusedNode) {
			const node = tree.nodes.get(focusedNode);
			if (node) {
				const sections = getNodeSections(node);
				const openQ = node.open_questions.length > 0
					? `\n\nOpen questions:\n${node.open_questions.map((q, i) => `${i + 1}. ${q}`).join("\n")}`
					: "";
				const deps = node.dependencies
					.map((id) => {
						const d = tree.nodes.get(id);
						return d ? `- ${d.title} (${d.status})` : null;
					})
					.filter(Boolean)
					.join("\n");
				const depsText = deps ? `\nDependencies:\n${deps}` : "";

				const decisionsSummary = sections.decisions.length > 0
					? `\n\nDecisions:\n${sections.decisions.map((d) => `- ${d.title} (${d.status})`).join("\n")}`
					: "";

				const body = getDocBody(node.filePath, 6000);

				return {
					message: {
						customType: "design-context",
						content:
							`[Design Tree — Focused on: ${node.title} (${node.status})]` +
							depsText + decisionsSummary + openQ +
							`\n\n--- Document Summary ---\n${body}` +
							`\n\nYou can use the design_tree and design_tree_update tools to query and modify the design tree. ` +
							`When this design discussion reaches a conclusion, use design_tree_update to set_status to 'decided'. ` +
							`If new sub-topics emerge, use design_tree_update to branch child nodes.`,
						display: false,
					},
				};
			}
		}

		const decided = Array.from(tree.nodes.values()).filter((n) => n.status === "decided").length;
		const exploring = Array.from(tree.nodes.values()).filter(
			(n) => n.status === "exploring" || n.status === "seed",
		).length;
		const totalQ = getAllOpenQuestions(tree).length;

		return {
			message: {
				customType: "design-context",
				content:
					`[Design Tree: ${tree.nodes.size} nodes — ${decided} decided, ${exploring} exploring, ${totalQ} open questions]\n` +
					`Use the design_tree tool to query the design space and design_tree_update to modify it.`,
				display: false,
			},
		};
	});

	// Filter stale design-context messages
	pi.on("context", async (event) => {
		let foundLatest = false;
		const keep = new Array(event.messages.length).fill(true);
		for (let i = event.messages.length - 1; i >= 0; i--) {
			const msg = event.messages[i] as { customType?: string };
			if (msg.customType === "design-context") {
				if (!foundLatest) {
					foundLatest = true;
				} else {
					keep[i] = false;
				}
			}
		}
		if (foundLatest) {
			const filtered = event.messages.filter((_, i) => keep[i]);
			if (filtered.length !== event.messages.length) {
				return { messages: filtered };
			}
		}
	});

	// ─── Message Renderers ───────────────────────────────────────────────

	pi.registerMessageRenderer("design-focus", (message, _options, theme) => {
		const titleMatch = (message.content as string).match(/\[Design Focus: (.+?)\]/);
		const title = titleMatch ? titleMatch[1] : "Unknown";
		let text = theme.fg("accent", theme.bold(`◈ Focus → ${title}`));

		const questionsMatch = (message.content as string).match(/Open questions:\n([\s\S]*?)(?:\n\n|$)/);
		if (questionsMatch) {
			const lines = questionsMatch[1].split("\n").filter(Boolean);
			for (const line of lines) {
				text += "\n  " + theme.fg("dim", line);
			}
		}
		return new Text(text, 0, 0);
	});

	pi.registerMessageRenderer("design-frontier", (message, _options, theme) => {
		const questionMatch = (message.content as string).match(/Question: (.+)/);
		const question = questionMatch ? questionMatch[1] : "Unknown";
		let text = theme.fg("warning", theme.bold("◈ Frontier")) + " ";
		text += theme.fg("muted", question);
		return new Text(text, 0, 0);
	});

	// ─── Branch Auto-Association ─────────────────────────────────────────
	// Note: pi's onBranchChange callback (ReadonlyFooterDataProvider) is only
	// accessible inside setFooter(), which conflicts with the dashboard extension.
	// We use before_agent_start polling with a dedup guard instead — readGitBranch
	// reads .git/HEAD which is a trivial stat+read, and the lastAssociatedBranch
	// guard ensures we only process actual changes.

	let lastAssociatedBranch: string | null = null;

	function tryAssociateBranch(ctx: ExtensionContext): void {
		const branch = readGitBranch(ctx.cwd);
		if (!branch || branch === lastAssociatedBranch) return;
		lastAssociatedBranch = branch;

		reload(ctx.cwd);
		const matched = matchBranchToNode(tree, branch);
		if (matched && !matched.branches.includes(branch)) {
			const updated = appendBranch(matched, branch);
			tree.nodes.set(updated.id, updated);
			emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
		}
	}

	// ─── Session Lifecycle ───────────────────────────────────────────────

	pi.on("session_start", async (_event, ctx) => {
		reload(ctx.cwd);

		const entries = ctx.sessionManager.getEntries();
		const focusEntry = entries
			.filter(
				(e: { type: string; customType?: string }) =>
					e.type === "custom" && e.customType === "design-tree-focus",
			)
			.pop() as { data?: { focusedNode: string | null } } | undefined;

		if (focusEntry?.data?.focusedNode) {
			focusedNode = focusEntry.data.focusedNode;
		}

		if (tree.nodes.size > 0) {
			emitDesignTreeState(ctx, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
		}

		// Auto-associate current branch on session start
		tryAssociateBranch(ctx);
	});

	pi.on("agent_end", async () => {
		if (tree.nodes.size > 0) {
			pi.appendEntry("design-tree-focus", { focusedNode });
		}
	});
}



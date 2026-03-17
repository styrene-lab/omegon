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

import type { ExtensionAPI, ExtensionContext } from "@styrene-lab/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { StringEnum } from "../lib/typebox-helpers.ts";
import { Text } from "@styrene-lab/pi-tui";
import * as fs from "node:fs";
import * as path from "node:path";
import { execFileSync } from "node:child_process";
import { shouldRefreshDesignTreeForPath } from "../dashboard/file-watch.ts";
import { sharedState } from "../lib/shared-state.ts";

import { emitDesignTreeState } from "./dashboard-state.ts";
import { sciCall, sciLoading, sciOk, sciErr, sciExpanded, sciBanner } from "../lib/sci-ui.ts";
import { SciDesignCard, buildCardDetails } from "./design-card.ts";
import type { DesignCardDetails } from "./design-card.ts";
import { emitConstraintCandidates, emitDecisionCandidates } from "./lifecycle-emitter.ts";
import { resolveNodeOpenSpecBinding, resolveDesignSpecBinding } from "../openspec/archive-gate.ts";
import { resolveLifecycleSummary, getAssessmentStatus, getChange, getOpenSpecDir } from "../openspec/spec.ts";
import { evaluateLifecycleReconciliation } from "../openspec/reconcile.ts";
import type { LifecycleSummary } from "../openspec/spec.ts";

import type { DesignNode, DesignTree, NodeStatus, IssueType, Priority } from "./types.ts";
import { VALID_STATUSES, STATUS_ICONS, STATUS_COLORS, VALID_ISSUE_TYPES, PRIORITY_LABELS } from "./types.ts";
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
	scaffoldDesignOpenSpecChange,
	mirrorOpenQuestionsToDesignSpec,
	matchBranchToNode,
	appendBranch,
	readGitBranch,
	sanitizeBranchName,
	writeNodeDocument,
	parseFrontmatter,
	countAcceptanceCriteria,
} from "./tree.ts";
import { getSharedBridge, buildSlashCommandResult } from "../lib/slash-command-bridge.ts";

// ─── Extension ───────────────────────────────────────────────────────────────

export default function designTreeExtension(pi: ExtensionAPI): void {
	let tree: DesignTree = { nodes: new Map(), docsDir: "" };
	let focusedNode: string | null = null;
	let docsWatcher: fs.FSWatcher | null = null;
	let docsRefreshTimer: NodeJS.Timeout | null = null;

	function reload(cwd: string): void {
		const docsDir = path.join(cwd, "docs");
		tree = scanDesignDocs(docsDir);
	}

	function docsDir(cwd: string): string {
		return path.join(cwd, "docs");
	}

	function emitCurrentState(): void {
		if (tree.nodes.size === 0) return;
		emitDesignTreeState(pi, tree, focusedNode ? tree.nodes.get(focusedNode) ?? null : null);
	}

	function scheduleDocsRefresh(filePath?: string): void {
		if (filePath && !shouldRefreshDesignTreeForPath(filePath, tree.docsDir || docsDir(process.cwd()))) {
			return;
		}
		if (docsRefreshTimer) clearTimeout(docsRefreshTimer);
		docsRefreshTimer = setTimeout(() => {
			docsRefreshTimer = null;
			if (!tree.docsDir) return;
			tree = scanDesignDocs(tree.docsDir);
			emitCurrentState();
		}, 75);
	}

	function startDocsWatcher(cwd: string): void {
		const dir = docsDir(cwd);
		if (!fs.existsSync(dir)) return;
		docsWatcher?.close();
		docsWatcher = null;
		try {
			docsWatcher = fs.watch(dir, { recursive: true }, (_eventType, filename) => {
				const filePath = typeof filename === "string" && filename.length > 0
					? path.join(dir, filename)
					: undefined;
				scheduleDocsRefresh(filePath);
			});
		} catch {
			// Best effort only — unsupported platforms simply fall back to command/tool-driven emits.
		}
	}

	// ─── Canonical lifecycle summary helper ──────────────────────────────

	/**
	 * Compute a normalized LifecycleSummary for a design node when it is bound
	 * to an OpenSpec change. Returns null when the node has no binding.
	 *
	 * Routes through resolveLifecycleSummary so all callers share a single
	 * lifecycle truth rather than deriving stage/binding/readiness independently.
	 */
	function resolveNodeLifecycleSummary(cwd: string, node: DesignNode): LifecycleSummary | null {
		const binding = resolveNodeOpenSpecBinding(cwd, node);
		if (!binding.bound || !binding.changeName) return null;

		try {
			const assessment = getAssessmentStatus(cwd, binding.changeName);
			const reconciliation = evaluateLifecycleReconciliation(cwd, binding.changeName);
			const archiveBlocked = reconciliation.issues.length > 0;
			const archiveBlockedReason = archiveBlocked
				? reconciliation.issues.map((i) => i.message).join("; ")
				: null;
			const archiveBlockedIssueCodes = reconciliation.issues.map((i) => i.code);

			const change = getChange(cwd, binding.changeName);
			if (!change) return null;

			return resolveLifecycleSummary({
				change,
				record: assessment.record,
				freshness: assessment.freshness,
				archiveBlocked,
				archiveBlockedReason,
				archiveBlockedIssueCodes,
				boundNodeIds: [node.id],
			});
		} catch {
			// Non-fatal — return null if OpenSpec data is unavailable
			return null;
		}
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

		// Bail if scaffold failed (e.g. change directory already exists)
		if (result.files.length === 0) {
			return { ok: false, message: result.message };
		}

		// D1: Explicit `branch` frontmatter field overrides derived name.
		// Otherwise derive from prefix + node ID. Never read branches[] as override —
		// that array is a historical record, not an intent.
		const branchName = node.branch ?? `${branchPrefix}/${node.id}`;

		// Validate branch name before any shell or fs operations
		const safeBranch = sanitizeBranchName(branchName);
		if (!safeBranch) {
			return { ok: false, message: `Invalid branch name: '${branchName}' — contains disallowed characters` };
		}

		// Write all frontmatter fields in one pass to avoid partial state on failure.
		// setNodeStatus and appendBranch each do a full file rewrite; we consolidate
		// by writing the final intended state directly.
		const existingBranches = node.branches ?? [];
		const updatedNode: DesignNode = {
			...node,
			status: "implementing",
			branches: existingBranches.includes(safeBranch)
				? existingBranches
				: [...existingBranches, safeBranch],
			openspec_change: node.id,
		};
		// Use writeNodeDocument to emit all fields in one write
		const sections = getNodeSections(node);
		writeNodeDocument(updatedNode, sections);

		// Create git branch — execFileSync with array args, no shell interpolation
		try {
			execFileSync("git", ["checkout", "-b", safeBranch], { cwd, stdio: "pipe" });
		} catch {
			try {
				// Branch already exists — switch to it
				execFileSync("git", ["checkout", safeBranch], { cwd, stdio: "pipe" });
			} catch {
				// Non-fatal: branch ops may fail in worktrees or detached HEAD
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
			"Use action 'ready' to find nodes that are decided and have all dependencies implemented — work queue for sprint planning",
			"Use action 'blocked' to find nodes that are explicitly blocked or have unresolved dependency blockers — shows exactly which dep is blocking each node",
		],
		parameters: Type.Object({
			action: StringEnum(["list", "node", "frontier", "dependencies", "children", "ready", "blocked"] as const),
			node_id: Type.Optional(
				Type.String({ description: "Node ID (required for node, dependencies, children)" }),
			),
		}),
		async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
			reload(ctx.cwd);

			switch (params.action) {
				case "list": {
					const nodes = Array.from(tree.nodes.values()).map((n) => {
						const binding = resolveNodeOpenSpecBinding(ctx.cwd, n);
						const lifecycleSummary = resolveNodeLifecycleSummary(ctx.cwd, n);
						return {
							id: n.id,
							title: n.title,
							status: n.status,
							parent: n.parent || null,
							tags: n.tags,
							open_questions: n.open_questions.length,
							dependencies: n.dependencies,
							branches: n.branches,
							openspec_change: n.openspec_change ?? null,
							priority: n.priority ?? null,
							issue_type: n.issue_type ?? null,
							acceptance_criteria_summary: countAcceptanceCriteria(n),
							lifecycle: {
								// Normalized binding status from canonical resolver when available.
								// The fallback (binding.bound ? "bound" : "unbound") is an explicit safety
								// guard for the error paths where resolveNodeLifecycleSummary returns null
								// (e.g. getChange() fails or throws). For successfully bound nodes,
								// resolveLifecycleSummary(bound:true) now returns "bound" directly.
								boundToOpenSpec: binding.bound,
								bindingStatus: lifecycleSummary?.bindingStatus ?? (binding.bound ? "bound" : "unbound"),
								implementationPhase: n.status === "implementing" || n.status === "implemented",
								archiveReady: lifecycleSummary?.archiveReady ?? null,
								nextAction: lifecycleSummary?.nextAction ?? null,
							},
						};
					});
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

					const binding = resolveNodeOpenSpecBinding(ctx.cwd, node);
					const lifecycleSummary = resolveNodeLifecycleSummary(ctx.cwd, node);
					const result = {
						id: node.id,
						title: node.title,
						status: node.status,
						parent: node.parent || null,
						dependencies: node.dependencies,
						related: node.related,
						tags: node.tags,
						branches: node.branches,
						openspecChange: node.openspec_change ?? null,
						priority: node.priority ?? null,
						issue_type: node.issue_type ?? null,
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
							acceptanceCriteria: sections.acceptanceCriteria,
							extraSections: sections.extraSections.map((s) => s.heading),
						},
						lifecycle: {
							// Backward-compatible boolean for existing callers
							boundToOpenSpec: binding.bound,
							// Normalized binding status from canonical resolver
							bindingStatus: lifecycleSummary?.bindingStatus ?? (binding.bound ? "bound" : "unbound"),
							canImplement: node.status === "decided" || node.status === "resolved",
							isImplementationPhase: node.status === "implementing" || node.status === "implemented",
							reopenSignalTarget: binding.changeName ?? node.openspec_change ?? node.id,
							// Canonical lifecycle fields from resolveLifecycleSummary when available
							archiveReady: lifecycleSummary?.archiveReady ?? null,
							verificationSubstate: lifecycleSummary?.verificationSubstate ?? null,
							nextAction: lifecycleSummary?.nextAction ?? null,
							openspecStage: lifecycleSummary?.stage ?? null,
							implementationNoteCounts: {
								fileScope: sections.implementationNotes.fileScope.length,
								constraints: sections.implementationNotes.constraints.length,
							},
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

				case "ready": {
					// Nodes with status='decided' where every dependency is 'implemented'
					// AND design-phase OpenSpec change is archived.
					const readyNodes = Array.from(tree.nodes.values())
						.filter((n) => {
							if (n.status !== "decided" && n.status !== "resolved") return false;
							// Hard gate: design spec must be archived
							const specBinding = resolveDesignSpecBinding(ctx.cwd, n.id);
							if (!specBinding.archived) return false;
							return n.dependencies.every((depId) => {
								const dep = tree.nodes.get(depId);
								return dep?.status === "implemented";
							});
						})
						.sort((a, b) => {
							// Sort by urgency descending: priority 1 (critical) first, 5 (trivial) last.
							// "priority desc" in the spec means "most-urgent first", which is
							// ascending numeric order because 1 = highest urgency.
							// Nodes without priority sort last (treated as 5).
							const pa = a.priority ?? 5;
							const pb = b.priority ?? 5;
							return pa - pb;
						})
						.map((n) => ({
							id: n.id,
							title: n.title,
							status: n.status,
							priority: n.priority ?? null,
							issue_type: n.issue_type ?? null,
							tags: n.tags,
							openspec_change: n.openspec_change ?? null,
						}));

					return {
						content: [{ type: "text", text: `${readyNodes.length} node(s) ready to implement:\n\n${JSON.stringify(readyNodes, null, 2)}` }],
						details: { ready: readyNodes },
					};
				}

				case "blocked": {
					// Nodes explicitly blocked OR whose dependencies are not yet 'implemented'
					// OR whose design-phase OpenSpec change is missing/not-archived.

					// Pre-compute spec bindings once for all decided nodes to avoid repeated I/O.
					// Scan openspec/design-archive/ once to build a set of archived node IDs,
					// avoiding O(n) readdirSync calls (one per decided node) on the same dir.
					const designArchiveDir = path.join(ctx.cwd, "openspec", "design-archive");
					const archivedDesignIds = new Set<string>();
					if (fs.existsSync(designArchiveDir)) {
						for (const entry of fs.readdirSync(designArchiveDir, { withFileTypes: true })) {
							if (!entry.isDirectory()) continue;
							const m = entry.name.match(/^\d{4}-\d{2}-\d{2}-(.+)$/);
							if (m) archivedDesignIds.add(m[1]);
						}
					}

					const specBindingCache = new Map<string, ReturnType<typeof resolveDesignSpecBinding>>();
					for (const n of tree.nodes.values()) {
						if (n.status === "decided") {
							// Use pre-scanned archivedDesignIds to avoid a second readdirSync per node.
							const designDir = path.join(ctx.cwd, "openspec", "design", n.id);
							const active =
								fs.existsSync(designDir) &&
								fs.statSync(designDir).isDirectory() &&
								fs.readdirSync(designDir).length > 0;
							const archivedInSet = archivedDesignIds.has(n.id);
							specBindingCache.set(n.id, {
								active,
								archived: archivedInSet && !active,
								missing: !active && !archivedInSet,
							});
						}
					}

					const blockedNodes = Array.from(tree.nodes.values())
						.filter((n) => {
							if (n.status === "implemented") return false;
							if (n.status === "blocked") return true;
							// Only surface dep-blocked signal for actively-worked nodes.
							// seed/deferred nodes are intentionally parked — flagging them as
							// blocked would be misleading noise.
							if (n.status === "seed" || n.status === "deferred") return false;
							// decided nodes: also block if design spec is not archived
							if (n.status === "decided") {
								const specBinding = specBindingCache.get(n.id)!;
								if (!specBinding.archived) return true;
							}
							// exploring/deciding nodes with at least one non-implemented dependency
							return n.dependencies.some((depId) => {
								const dep = tree.nodes.get(depId);
								return !dep || dep.status !== "implemented";
							});
						})
						.map((n) => {
							const blockingDeps = n.dependencies
								.filter((depId) => {
									const dep = tree.nodes.get(depId);
									return !dep || dep.status !== "implemented";
								})
								.map((depId) => {
									const dep = tree.nodes.get(depId);
									return {
										id: depId,
										title: dep?.title ?? "(unknown)",
										status: dep?.status ?? "missing",
									};
								});

							// Determine blocking_reason and inject synthetic design-spec dep when needed
							let blockingReason: "design-spec-not-archived" | "dependencies" | "explicit";
							let allBlockingDeps = [...blockingDeps];

							if (n.status === "blocked") {
								blockingReason = "explicit";
							} else if (n.status === "decided") {
								const specBinding = specBindingCache.get(n.id)!;
								if (!specBinding.archived) {
									blockingReason = "design-spec-not-archived";
									allBlockingDeps = [
										{
											id: "design-spec-missing",
											title: "Design spec not archived",
											status: "missing",
										},
										...allBlockingDeps,
									];
								} else {
									blockingReason = "dependencies";
								}
							} else {
								blockingReason = "dependencies";
							}

							return {
								id: n.id,
								title: n.title,
								status: n.status,
								priority: n.priority ?? null,
								issue_type: n.issue_type ?? null,
								tags: n.tags,
								openspec_change: n.openspec_change ?? null,
								blocking_reason: blockingReason,
								blocking_deps: allBlockingDeps,
							};
						});

					return {
						content: [{ type: "text", text: `${blockedNodes.length} node(s) blocked:\n\n${JSON.stringify(blockedNodes, null, 2)}` }],
						details: { blocked: blockedNodes },
					};
				}
			}

			return { content: [{ type: "text", text: "Unknown action" }], details: {} };
		},

		renderCall(args, theme) {
			const summary = args.action + (args.node_id ? ":" + args.node_id : "");
			return sciCall("design_tree", summary, theme);
		},

		renderResult(result, { expanded, isPartial }, theme) {
			if (isPartial) {
				return sciLoading("design_tree", theme);
			}
			if ((result as any).isError) {
				const first = result.content?.[0];
				const errLine = (first && 'text' in first ? first.text : "Error") ?? "Error";
				return sciErr(errLine.split("\n")[0].slice(0, 80), theme);
			}

			const details = (result.details || {}) as Record<string, any>;

			// Rich card for single node results (expanded)
			if (expanded && details.node) {
				const n = details.node as Record<string, any>;
				const cardDetails: DesignCardDetails = {
					id: n.id ?? "",
					title: n.title ?? "",
					status: n.status ?? "seed",
					priority: n.priority ?? undefined,
					issue_type: n.issue_type ?? undefined,
					overview: n.sections?.overview ?? "",
					decisions: n.sections?.decisions?.map((d: any) => ({ title: d.title, status: d.status })) ?? [],
					openQuestions: n.sections?.openQuestions ?? [],
					dependencies: (n.dependencies ?? []).map((id: string) => {
						const dep = tree.nodes.get(id);
						return dep ? { id: dep.id, title: dep.title, status: dep.status } : { id, title: id, status: "seed" as NodeStatus };
					}),
					children: n.children ?? [],
					fileScope: n.sections?.implementationNotes?.fileScope ?? [],
					constraints: n.sections?.implementationNotes?.constraints ?? [],
					openspec_change: n.openspecChange ?? undefined,
					branches: n.branches ?? [],
				};
				return new SciDesignCard(`design_tree:node → ${cardDetails.id}`, cardDetails, theme);
			}

			if (expanded) {
				const first = result.content?.[0];
				const fullText = (first && 'text' in first ? first.text : null) ?? "";
				const lines = fullText.split("\n");
				return sciExpanded(lines, `${lines.length} lines`, theme);
			}

			let summary = "";

			if (details.nodes) {
				const nodes = details.nodes as Array<{ id: string; status: string; open_questions: number }>;
				summary = `${nodes.length} nodes`;
			} else if (details.node) {
				const n = details.node as { title: string; status: NodeStatus; sections?: { openQuestions?: string[] } };
				const qCount = n.sections?.openQuestions?.length || 0;
				summary = `${STATUS_ICONS[n.status]} ${n.title} (${n.status})` + (qCount > 0 ? ` — ${qCount} questions` : "");
			} else if (details.questions) {
				const q = details.questions as Record<string, string[]>;
				const total = Object.values(q).flat().length;
				summary = `${total} open questions`;
			} else {
				const first = result.content?.[0];
				summary = ((first && 'text' in first ? first.text : null) ?? "Done").split("\n")[0].slice(0, 80);
			}

			return sciOk(summary, theme);
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
			"- set_status: Change node status (seed/exploring/resolved/decided/blocked/deferred)\n" +
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
			"- implement: Bridge a decided node to OpenSpec — scaffold a change directory\n" +
			"- set_priority: Set the priority (1-5) on a node\n" +
			"- set_issue_type: Set the issue type (epic/feature/task/bug/chore) on a node",
		promptSnippet:
			"Mutate the design tree — create nodes, set status, add research/decisions/questions, branch, implement",
		promptGuidelines: [
			"Use 'create' to start a new design exploration. Status defaults to 'seed'.",
			"Use 'set_status' to transition nodes: seed → exploring → resolved → decided. Use 'resolved' when design questions are answered but the formal lifecycle gate hasn't cleared. Use 'blocked' or 'deferred' as needed.",
			"Use 'add_question' when discussion reveals unknowns. Use 'remove_question' when questions are answered.",
			"Use 'add_research' to record findings with a heading and content.",
			"Use 'add_decision' to crystallize choices with title, status (exploring/decided/rejected), and rationale.",
			"Use 'branch' to spawn a child node from a parent's open question — this removes the question from the parent.",
			"Use 'focus' to set which node's context gets injected into the conversation.",
			"Use 'implement' on a decided node to generate an OpenSpec change directory for cleave execution.",
			"When an OpenSpec change exists for a decided node, suggest `/cleave` to parallelize the implementation.",
			"Use 'set_priority' to assign a priority 1 (critical) to 5 (trivial) to a node.",
			"Use 'set_issue_type' to classify a node as epic/feature/task/bug/chore.",
		],
		parameters: Type.Object({
			action: StringEnum([
				"create", "set_status", "add_question", "remove_question",
				"add_research", "add_decision", "add_dependency", "add_related",
				"add_impl_notes", "branch", "focus", "unfocus", "implement",
				"set_priority", "set_issue_type",
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
			// set_priority params
			priority: Type.Optional(Type.Number({ description: "Priority 1 (critical) to 5 (trivial) (for set_priority)" })),
			// set_issue_type params
			issue_type: Type.Optional(Type.String({ description: "epic|feature|task|bug|chore (for set_issue_type)" })),
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
					emitCurrentState();

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

					// Hard gate: set_status(decided) requires archived design spec.
					// Exception: bug/chore/task nodes with no open questions can skip
					// the design-phase ceremony — the decision IS the diagnosis.
					if (newStatus === "decided") {
						const lightweightTypes = new Set(["bug", "chore", "task"]);
						const isLightweight = node.issue_type && lightweightTypes.has(node.issue_type);
						const hasOpenQuestions = (node.open_questions?.length ?? 0) > 0;

						// Lightweight nodes: allow decided if no open questions remain,
						// regardless of design-phase spec state.
						if (!(isLightweight && !hasOpenQuestions)) {
							const designSpec = resolveDesignSpecBinding(ctx.cwd, node.id);
							if (designSpec.missing) {
								return {
									content: [{ type: "text", text: `Cannot mark '${node.title}' decided: scaffold design spec first via set_status(exploring).` }],
									details: { id: node.id, blockedBy: "design-openspec-missing" },
									isError: true,
								};
							}
							if (designSpec.active && !designSpec.archived) {
								return {
									content: [{ type: "text", text: `Cannot mark '${node.title}' decided: run \`/assess design ${node.id}\` then archive the design change before marking decided.` }],
									details: { id: node.id, blockedBy: "design-openspec-not-archived" },
									isError: true,
								};
							}
						}
					}

					const updated = setNodeStatus(node, newStatus);
					tree.nodes.set(updated.id, updated);
					emitCurrentState();

					let text = `${STATUS_ICONS[newStatus]} '${node.title}': ${oldStatus} → ${newStatus}`;

					// If transitioning to exploring, scaffold design OpenSpec change (idempotent)
					if (newStatus === "exploring") {
						const scaffoldResult = scaffoldDesignOpenSpecChange(ctx.cwd, updated);
						if (scaffoldResult.created) {
							text += `\n\nScaffolded design spec at openspec/design/${node.id}/\n` +
								`  - proposal.md, spec.md, tasks.md\n\n` +
								`Fill in ## Acceptance Criteria in the node doc (Scenarios / Falsifiability / Constraints) before running /assess design.`;
						}
					}

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
					// Emit memory fact for the open question
					(sharedState.lifecycleCandidateQueue ??= []).push({
						source: "design-tree",
						context: `Open question added to node '${node.id}'`,
						candidates: [{
							sourceKind: "design-decision",
							authority: "explicit",
							section: "Specs",
							content: `OPEN [${node.id}]: ${params.question}`,
							artifactRef: {
								type: "design-node",
								path: node.filePath,
								subRef: node.id,
							},
						}],
					});
					mirrorOpenQuestionsToDesignSpec(ctx.cwd, updated);
					emitCurrentState();
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
					// Schedule archival of the corresponding memory fact by content prefix.
					// Check whether add_question ever emitted a fact for this question so the
					// caller gets explicit feedback when no matching fact exists (e.g. question
					// was added before this extension version was deployed).
					const factContentPrefix = `OPEN [${node.id}]: ${params.question}`;
					const emittedFacts = (sharedState.lifecycleCandidateQueue ?? [])
						.flatMap((m) => m.candidates)
						.filter((c) => c.section === "Specs" && c.content === factContentPrefix);
					const factWasEmitted = emittedFacts.length > 0;
					(sharedState.factArchiveQueue ??= []).push(factContentPrefix);
					mirrorOpenQuestionsToDesignSpec(ctx.cwd, updated);
					emitCurrentState();
					return {
						content: [
							{
								type: "text",
								text: factWasEmitted
									? `Removed question from '${node.title}'`
									: `Removed question from '${node.title}' (note: no corresponding memory fact found — question may have been added before fact-emission was deployed)`,
							},
						],
						details: { id: node.id, remainingQuestions: updated.open_questions.length, factWasEmitted },
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
					emitCurrentState();
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
					const decisionCandidates = emitDecisionCandidates(node, params.decision_title, dStatus);
					if (decisionCandidates.length > 0) {
						(sharedState.lifecycleCandidateQueue ??= []).push({
							source: "design-tree",
							context: `Decided design decision in '${node.id}'`,
							candidates: decisionCandidates,
						});
					}
					emitCurrentState();
					return {
						content: [{ type: "text", text: `Added decision '${params.decision_title}' (${dStatus}) to '${node.title}'` }],
						details: { id: node.id, decision: params.decision_title, status: dStatus, emittedCandidates: decisionCandidates.length },
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
					emitCurrentState();
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
					emitCurrentState();
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
					const constraintCandidates = emitConstraintCandidates(node, params.constraints);
					if (constraintCandidates.length > 0) {
						(sharedState.lifecycleCandidateQueue ??= []).push({
							source: "design-tree",
							context: `Implementation constraints recorded for '${node.id}'`,
							candidates: constraintCandidates,
						});
					}
					emitCurrentState();
					return {
						content: [{ type: "text", text: `Added implementation notes to '${node.title}': ${added.join(", ")}` }],
						details: { id: node.id, emittedCandidates: constraintCandidates.length },
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
					emitCurrentState();

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

					emitCurrentState();
					return {
						content: [{ type: "text", text: `Focused on '${node.title}'. Context will be injected on next turn.` }],
						details: { focusedNode: params.node_id },
					};
				}

				case "unfocus": {
					focusedNode = null;
					emitCurrentState();
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
					if (node.status !== "decided" && node.status !== "resolved") {
						return {
							content: [{
								type: "text",
								text: `Node '${node.title}' is '${node.status}', not 'decided' or 'resolved'. ` +
									`Resolve open questions and set status to 'decided' (or 'resolved') before implementing.`,
							}],
							details: {},
							isError: true,
						};
					}

					// Hard gate: design-phase spec must be archived before implementation.
					// Exception: bug/chore/task nodes that are already decided with no
					// open questions can proceed directly — they don't need a design-phase
					// spec because the decision is the bug diagnosis itself.
					{
						const lightweightTypes = new Set(["bug", "chore", "task"]);
						const isLightweight = node.issue_type && lightweightTypes.has(node.issue_type);
						const hasOpenQuestions = (node.open_questions?.length ?? 0) > 0;
						const skipDesignGate = isLightweight && !hasOpenQuestions && (node.status === "decided" || node.status === "resolved");

						if (!skipDesignGate) {
							const designSpec = resolveDesignSpecBinding(ctx.cwd, node.id);
							if (designSpec.missing) {
								return {
									content: [{ type: "text", text: "Scaffold design spec first via set_status(exploring)" }],
									details: { id: node.id, blockedBy: "design-openspec-missing" },
									isError: true,
								};
							}
							if (designSpec.active && !designSpec.archived) {
								return {
									content: [{ type: "text", text: `Cannot implement '${node.title}': archive the design change first (\`/opsx:archive\` on the design change).` }],
									details: { id: node.id, blockedBy: "design-openspec-not-archived" },
									isError: true,
								};
							}
						}
					}

					const implResult = executeImplement(ctx.cwd, node);
					reload(ctx.cwd);
					emitCurrentState();

					// Fork a directive-scoped memory mind so facts discovered during
					// this work are isolated until archive merges them back.
					if (implResult.ok) {
						const mindName = `directive/${node.id}`;
						(sharedState.mindLifecycleQueue ??= []).push(
							{ action: "fork", mind: mindName, description: `Memory scope for ${implResult.branch ?? node.id}` },
							{ action: "activate", mind: mindName },
						);
					}

					return {
						content: [{ type: "text", text: implResult.message }],
						details: implResult.ok
							? { changePath: implResult.changePath, files: implResult.files, branch: implResult.branch }
							: {},
						isError: !implResult.ok,
					};
				}

				// ── set_priority ──────────────────────────────────────────
				case "set_priority": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const p = params.priority !== undefined ? Math.round(params.priority) : undefined;
					if (p === undefined || p < 1 || p > 5) {
						return { content: [{ type: "text", text: "Error: priority must be an integer 1–5" }], details: {}, isError: true };
					}
					const updatedNode = { ...node, priority: p as Priority };
					const sections = getNodeSections(node);
					writeNodeDocument(updatedNode, sections);
					tree.nodes.set(updatedNode.id, updatedNode);
					reload(ctx.cwd);
					emitCurrentState();
					return {
						content: [{ type: "text", text: `Priority set to ${p} on '${node.title}'` }],
						details: { node_id: node.id, priority: p },
					};
				}

				// ── set_issue_type ────────────────────────────────────────
				case "set_issue_type": {
					if (!params.node_id) {
						return { content: [{ type: "text", text: "Error: node_id required" }], details: {}, isError: true };
					}
					const node = tree.nodes.get(params.node_id);
					if (!node) {
						return { content: [{ type: "text", text: `Node '${params.node_id}' not found` }], details: {}, isError: true };
					}
					const it = params.issue_type as IssueType | undefined;
					if (!it || !VALID_ISSUE_TYPES.includes(it)) {
						return {
							content: [{ type: "text", text: `Error: issue_type must be one of: ${VALID_ISSUE_TYPES.join(", ")}` }],
							details: {},
							isError: true,
						};
					}
					const updatedNode = { ...node, issue_type: it };
					const sections = getNodeSections(node);
					writeNodeDocument(updatedNode, sections);
					tree.nodes.set(updatedNode.id, updatedNode);
					reload(ctx.cwd);
					emitCurrentState();
					return {
						content: [{ type: "text", text: `Issue type set to '${it}' on '${node.title}'` }],
						details: { node_id: node.id, issue_type: it },
					};
				}
			}

			return { content: [{ type: "text", text: "Unknown action" }], details: {} };
		},

		renderCall(args, theme) {
			let summary = args.action;
			if (args.node_id) summary += ":" + args.node_id;

			switch (args.action) {
				case "set_status":
					if (args.status) summary += " → " + args.status;
					break;
				case "add_question":
				case "remove_question":
					if (args.question) summary += " " + `"${String(args.question).slice(0, 50)}"`;
					break;
				case "add_decision":
					if (args.decision_title) summary += " " + `"${String(args.decision_title).slice(0, 45)}"`;
					break;
				case "add_research":
					if (args.heading) summary += " " + `"${String(args.heading).slice(0, 45)}"`;
					break;
				case "create":
					if (args.title) summary += " " + `"${String(args.title).slice(0, 45)}"`;
					break;
			}

			return sciCall("design_tree_update", summary, theme);
		},

		renderResult(result, { expanded, isPartial }, theme) {
			if (isPartial) {
				return sciLoading("design_tree_update", theme);
			}

			const isErr = (result as any).isError;
			const first = result.content?.[0];
			const firstLine = ((first && 'text' in first ? first.text : null) ?? "Done").split("\n")[0];

			if (isErr || firstLine.startsWith("Error") || firstLine.startsWith("Cannot")) {
				return sciErr(firstLine.slice(0, 80), theme);
			}

			if (expanded) {
				const fullText = (first && 'text' in first ? first.text : null) ?? "";
				const lines = fullText.split("\n");
				return sciExpanded(lines, `${lines.length} lines`, theme);
			}

			// Collapsed: action-specific one-liners from result.details
			const details = (result.details ?? {}) as Record<string, unknown>;

			// Determine action from details fields
			if ("newStatus" in details && "id" in details) {
				// set_status result
				const ns = details.newStatus as string;
				return sciOk(`→ ${ns}  ${String(details.id)}`, theme);
			}
			if ("totalQuestions" in details && "question" in details) {
				// add_question
				const q = String(details.question).slice(0, 50);
				const total = String(details.totalQuestions);
				return sciOk(`+ question  "${q}"  (${total} total)`, theme);
			}
			if ("remainingQuestions" in details && "question" in details) {
				// remove_question
				const q = String(details.question).slice(0, 50);
				const rem = String(details.remainingQuestions);
				return sciOk(`− question  "${q}"  (${rem} remaining)`, theme);
			}
			if ("decision" in details && "status" in details) {
				// add_decision
				const d = String(details.decision).slice(0, 45);
				const ds = String(details.status);
				return sciOk(`+ decision  "${d}"  ${ds}`, theme);
			}
			if ("heading" in details) {
				// add_research
				const h = String(details.heading).slice(0, 45);
				return sciOk(`+ research  "${h}"`, theme);
			}
			if ("changePath" in details) {
				// implement
				const cp = String(details.changePath ?? "").replace(/^.*openspec\//, "openspec/");
				return sciOk(`✓ scaffolded  ${cp}`, theme);
			}
			if ("node" in details && typeof details.node === "object" && details.node !== null) {
				// create
				const n = details.node as { id: string; status: string };
				return sciOk(`✓ created  ${n.id}  ${n.status}`, theme);
			}
			if ("focusedNode" in details) {
				const fid = details.focusedNode as string | null;
				return sciOk(fid ? `→ focused  ${fid}` : "focus cleared", theme);
			}

			// Fallback: first line of content text
			return sciOk(firstLine.slice(0, 80), theme);
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
					const decided = Array.from(tree.nodes.values()).filter((n) => n.status === "decided" || n.status === "resolved").length;
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
					emitCurrentState();

					const node = tree.nodes.get(focusedNode!)!;
					const sections = getNodeSections(node);
					const cardDetails = buildCardDetails(node, sections, tree);

					const openQ = node.open_questions.length > 0
						? `\n\nOpen questions:\n${node.open_questions.map((q, i) => `${i + 1}. ${q}`).join("\n")}`
						: "";

					pi.sendMessage(
						{
							customType: "design-focus",
							content: `[Design Focus: ${node.title} (${node.status})]${openQ}\n\nLet's explore this design space.`,
							display: true,
							details: cardDetails,
						},
						{ triggerTurn: false },
					);
					break;
				}

				case "unfocus": {
					focusedNode = null;
					emitCurrentState();
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
					emitCurrentState();
					ctx.ui.notify(`${STATUS_ICONS[newStatus]} '${node.title}' → ${newStatus}`, "info");
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
							emitCurrentState();
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
					emitCurrentState();
					ctx.ui.notify(`Created ${newId}.md — branched from ${node.title}`, "info");
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
					emitCurrentState();
					ctx.ui.notify(`Created ${id}.md`, "info");
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
						emitCurrentState();
						ctx.ui.notify(`Added question to ${node.title}`, "info");
					} else if (action === "Remove open question") {
						if (node.open_questions.length === 0) {
							ctx.ui.notify("No open questions to remove", "info");
							return;
						}
						const toRemove = await ctx.ui.select("Remove which question?", node.open_questions);
						if (!toRemove) return;
						removeOpenQuestion(node, toRemove);
						reload(ctx.cwd);
						emitCurrentState();
						ctx.ui.notify(`Removed question from ${node.title}`, "info");
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
						emitCurrentState();
						ctx.ui.notify(`Added dependency: ${choice.split(" — ")[0]}`, "info");
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
						emitCurrentState();
						ctx.ui.notify(`Added related: ${relatedId} (bidirectional)`, "info");
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
					if (node.status !== "decided" && node.status !== "resolved") {
						ctx.ui.notify(
							`'${node.title}' is '${node.status}', not 'decided'/'resolved'. Resolve questions first.`,
							"warning",
						);
						return;
					}

					const implResult = executeImplement(ctx.cwd, node);
					reload(ctx.cwd);
					emitCurrentState();
					ctx.ui.notify(implResult.message, implResult.ok ? "info" : "error");
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
				const cardDetails = buildCardDetails(node, sections, tree);

				// Build structured content for the model
				const contentParts: string[] = [];
				contentParts.push(`[Design Tree — Focused on: ${node.title} (${node.status})]`);

				if (node.priority != null || node.issue_type) {
					const meta: string[] = [];
					if (node.priority != null) meta.push(`P${node.priority} ${PRIORITY_LABELS[node.priority as Priority] ?? ""}`);
					if (node.issue_type) meta.push(node.issue_type);
					contentParts.push(`Type: ${meta.join(" · ")}`);
				}

				if (sections.overview?.trim()) {
					contentParts.push(`\nOverview: ${sections.overview.trim().slice(0, 300)}`);
				}

				if (node.dependencies.length > 0) {
					const deps = node.dependencies
						.map((id) => { const d = tree.nodes.get(id); return d ? `- ${d.title} (${d.status})` : null; })
						.filter(Boolean);
					if (deps.length > 0) contentParts.push(`\nDependencies:\n${deps.join("\n")}`);
				}

				// Children
				const children = Array.from(tree.nodes.values()).filter((n) => n.parent === node.id);
				if (children.length > 0) {
					contentParts.push(`\nChildren:\n${children.map((c) => `- ${c.title} (${c.status})`).join("\n")}`);
				}

				if (sections.decisions.length > 0) {
					contentParts.push(`\nDecisions:\n${sections.decisions.map((d) => `- ${d.title} (${d.status})`).join("\n")}`);
				}

				if (node.open_questions.length > 0) {
					contentParts.push(`\nOpen questions:\n${node.open_questions.map((q, i) => `${i + 1}. ${q}`).join("\n")}`);
				}

				if (sections.implementationNotes.fileScope.length > 0) {
					const scope = sections.implementationNotes.fileScope
						.map((f) => `- ${f.action ? `[${f.action}] ` : ""}${f.path}`)
						.join("\n");
					contentParts.push(`\nFile scope:\n${scope}`);
				}

				if (sections.implementationNotes.constraints.length > 0) {
					contentParts.push(`\nConstraints:\n${sections.implementationNotes.constraints.map((c) => `- ${c}`).join("\n")}`);
				}

				contentParts.push(
					`\nUse design_tree(action='node', node_id='${node.id}') to read the full document including research sections. ` +
					`Use design_tree_update to modify it. ` +
					`When this discussion reaches a conclusion, use design_tree_update to set_status to 'decided'. ` +
					`If new sub-topics emerge, use design_tree_update to branch child nodes.`,
				);

				return {
					message: {
						customType: "design-context",
						content: contentParts.join("\n"),
						display: false,
						details: cardDetails,
					},
				};
			}
		}

		const implemented = Array.from(tree.nodes.values()).filter((n) => n.status === "implemented").length;
		const implementing = Array.from(tree.nodes.values()).filter((n) => n.status === "implementing").length;
		const decided = Array.from(tree.nodes.values()).filter((n) => n.status === "decided" || n.status === "resolved").length;
		const exploring = Array.from(tree.nodes.values()).filter(
			(n) => n.status === "exploring" || n.status === "seed",
		).length;
		const blocked = Array.from(tree.nodes.values()).filter((n) => n.status === "blocked").length;
		const deferred = Array.from(tree.nodes.values()).filter((n) => n.status === "deferred").length;
		const totalQ = getAllOpenQuestions(tree).length;
		const summaryParts = [
			`${tree.nodes.size} nodes`,
			`${implemented} implemented`,
			`${implementing} implementing`,
			`${decided} decided`,
			`${exploring} exploring`,
			`${totalQ} open questions`,
		];
		if (blocked > 0) summaryParts.push(`${blocked} blocked`);
		if (deferred > 0) summaryParts.push(`${deferred} deferred`);

		return {
			message: {
				customType: "design-context",
				content:
					`[Design Tree: ${summaryParts.join(" — ")}]\n` +
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
		const details = message.details as DesignCardDetails | undefined;

		// Rich card when details are available
		if (details?.id) {
			return new SciDesignCard(`design:focus → ${details.id}`, details, theme);
		}

		// Fallback for legacy messages without details
		const titleMatch = (message.content as string).match(/\[Design Focus: (.+?)\]/);
		const title = titleMatch ? titleMatch[1] : "Unknown";

		const questionsMatch = (message.content as string).match(/Open questions:\n([\s\S]*?)(?:\n\n|$)/);
		const questionLines = questionsMatch
			? questionsMatch[1].split("\n").filter(Boolean)
			: [];

		return sciBanner("◈", "design:focus → " + title, questionLines, theme);
	});

	pi.registerMessageRenderer("design-frontier", (message, _options, theme) => {
		const questionMatch = (message.content as string).match(/Question: (.+)/);
		const question = questionMatch ? questionMatch[1] : "Unknown";
		return sciBanner("◈", "design:frontier", [question], theme);
	});

	pi.registerMessageRenderer("design-context", (message, _options, theme) => {
		const details = message.details as DesignCardDetails | undefined;

		// Rich card when details are available (focused node context)
		if (details?.id) {
			return new SciDesignCard(`design:context → ${details.id}`, details, theme);
		}

		// Summary context (no focused node) — show as a thin banner
		const content = (message.content as string) || "";
		const firstLine = content.split("\n")[0] || "";
		return sciBanner("◈", "design:context", [firstLine], theme);
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
			emitCurrentState();
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

		emitCurrentState();
		startDocsWatcher(ctx.cwd);

		// Check for migrateable design docs and hint
		const { toMigrate } = detectMigratableDesignDocs(ctx.cwd);
		if (toMigrate.length > 0) {
			ctx.ui.notify(
				`📦 ${toMigrate.length} completed design doc(s) can be archived to docs/design/. Run /migrate to clean up.`,
				"info",
			);
		}

		// Auto-associate current branch on session start
		tryAssociateBranch(ctx);
	});

	// ─── /migrate command ────────────────────────────────────────────────

	/**
	 * Detect design docs in docs/ that should be archived to docs/design/
	 * and offer to move them. Returns { migrated, skipped, errors }.
	 */
	function detectMigratableDesignDocs(cwd: string): {
		toMigrate: Array<{ file: string; id: string; status: string }>;
		activeExplorations: Array<{ file: string; id: string; status: string }>;
	} {
		const docsDir = path.join(cwd, "docs");
		if (!fs.existsSync(docsDir)) return { toMigrate: [], activeExplorations: [] };

		const designSubdir = path.join(docsDir, "design");
		const alreadyArchived = new Set<string>();
		if (fs.existsSync(designSubdir)) {
			for (const f of fs.readdirSync(designSubdir)) {
				if (f.endsWith(".md")) alreadyArchived.add(f);
			}
		}

		const toMigrate: Array<{ file: string; id: string; status: string }> = [];
		const activeExplorations: Array<{ file: string; id: string; status: string }> = [];

		for (const file of fs.readdirSync(docsDir)) {
			if (!file.endsWith(".md")) continue;
			if (alreadyArchived.has(file)) continue;

			const filePath = path.join(docsDir, file);
			const stat = fs.statSync(filePath);
			if (!stat.isFile()) continue;

			const content = fs.readFileSync(filePath, "utf-8");
			const fm = parseFrontmatter(content);
			if (!fm || !fm.id || !fm.status) continue; // Not a design doc

			const status = fm.status as string;
			const entry = { file, id: fm.id as string, status };

			if (status === "implemented" || status === "deferred") {
				toMigrate.push(entry);
			} else {
				// seed, exploring, resolved, decided, blocked — leave in docs/
				activeExplorations.push(entry);
			}
		}

		return { toMigrate, activeExplorations };
	}

	function executeDocsMigration(cwd: string, files: Array<{ file: string }>): {
		migrated: string[];
		errors: Array<{ file: string; error: string }>;
	} {
		const docsDir = path.join(cwd, "docs");
		const designDir = path.join(docsDir, "design");
		fs.mkdirSync(designDir, { recursive: true });

		const migrated: string[] = [];
		const errors: Array<{ file: string; error: string }> = [];

		// Detect if we're in a git repo
		let useGitMv = false;
		try {
			execFileSync("git", ["rev-parse", "--git-dir"], { cwd, stdio: "pipe" });
			useGitMv = true;
		} catch {
			// Not a git repo — use plain fs.rename
		}

		for (const { file } of files) {
			const src = path.join(docsDir, file);
			const dst = path.join(designDir, file);

			try {
				if (useGitMv) {
					execFileSync("git", ["mv", src, dst], { cwd, stdio: "pipe" });
				} else {
					fs.renameSync(src, dst);
				}
				migrated.push(file);
			} catch (e: any) {
				errors.push({ file, error: e.message?.slice(0, 200) ?? "unknown error" });
			}
		}

		return { migrated, errors };
	}

	pi.registerCommand("migrate", {
		description: "Migrate design docs: archive implemented/deferred explorations to docs/design/",
		handler: async (_args, ctx) => {
			const cwd = ctx.cwd;
			const { toMigrate, activeExplorations } = detectMigratableDesignDocs(cwd);

			if (toMigrate.length === 0) {
				const msg = activeExplorations.length > 0
					? `No design docs to migrate. ${activeExplorations.length} active exploration(s) remain in docs/ (correct).`
					: "No design docs found in docs/. Nothing to migrate.";
				ctx.ui.notify(msg, "info");
				return;
			}

			// Show what will be migrated
			const lines = [
				`Found ${toMigrate.length} design doc(s) to archive to docs/design/:`,
				"",
				...toMigrate.map((d) => `  ${STATUS_ICONS[d.status as NodeStatus] ?? "○"} ${d.file} (${d.status})`),
			];
			if (activeExplorations.length > 0) {
				lines.push(
					"",
					`${activeExplorations.length} active exploration(s) will stay in docs/:`,
					...activeExplorations.map((d) => `  ${STATUS_ICONS[d.status as NodeStatus] ?? "○"} ${d.file} (${d.status})`),
				);
			}

			const confirmed = await ctx.ui.confirm(
				"Migrate design docs",
				lines.join("\n") + "\n\nProceed with migration?",
			);
			if (!confirmed) {
				ctx.ui.notify("Migration cancelled.", "info");
				return;
			}

			const { migrated, errors } = executeDocsMigration(cwd, toMigrate);

			// Reload the tree to pick up new locations
			reload(cwd);
			emitCurrentState();

			const summary = [
				`✅ Migrated ${migrated.length} design doc(s) to docs/design/`,
			];
			if (errors.length > 0) {
				summary.push(`⚠️  ${errors.length} error(s):`);
				for (const e of errors) summary.push(`  ${e.file}: ${e.error}`);
			}
			if (activeExplorations.length > 0) {
				summary.push(`ℹ️  ${activeExplorations.length} active exploration(s) unchanged in docs/`);
			}

			pi.sendMessage({
				customType: "design-tree-migrate",
				content: summary.join("\n"),
				display: true,
			});
		},
	});

	// Bridge /migrate for agent access
	const bridge = getSharedBridge();
	bridge.register(pi, {
		name: "migrate",
		description: "Migrate design docs: archive implemented/deferred explorations to docs/design/",
		bridge: {
			agentCallable: true,
			sideEffectClass: "git-write",
			requiresConfirmation: true,
			summary: "Archive completed design docs from docs/ to docs/design/",
		},
		structuredExecutor: async (_args, ctx) => {
			const cwd = (ctx as ExtensionContext).cwd;
			const { toMigrate, activeExplorations } = detectMigratableDesignDocs(cwd);

			if (toMigrate.length === 0) {
				return buildSlashCommandResult("migrate", [], {
					ok: true,
					summary: "Nothing to migrate",
					humanText: activeExplorations.length > 0
						? `No completed design docs to migrate. ${activeExplorations.length} active exploration(s) remain in docs/.`
						: "No design docs found in docs/.",
					effects: { sideEffectClass: "read" },
				});
			}

			const { migrated, errors } = executeDocsMigration(cwd, toMigrate);

			reload(cwd);
			emitCurrentState();

			const humanLines = [
				`Migrated ${migrated.length} design doc(s) to docs/design/`,
				...migrated.map((f) => `  ✓ ${f}`),
			];
			if (errors.length > 0) {
				humanLines.push(`${errors.length} error(s):`);
				for (const e of errors) humanLines.push(`  ✗ ${e.file}: ${e.error}`);
			}

			return buildSlashCommandResult("migrate", [], {
				ok: errors.length === 0,
				summary: `Migrated ${migrated.length} doc(s)${errors.length > 0 ? `, ${errors.length} error(s)` : ""}`,
				humanText: humanLines.join("\n"),
				effects: {
					sideEffectClass: "git-write",
					filesChanged: migrated.map((f) => `docs/design/${f}`),
				},
				data: { migrated, errors, activeExplorations: activeExplorations.map((a) => a.file) },
			});
		},
	});

	// ─── Session lifecycle ───────────────────────────────────────────────

	pi.on("agent_end", async () => {
		if (tree.nodes.size > 0) {
			pi.appendEntry("design-tree-focus", { focusedNode });
		}
	});
}



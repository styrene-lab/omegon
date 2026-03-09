import * as fs from "node:fs";
import * as path from "node:path";

import {
	scanDesignDocs,
	getNodeSections,
	writeNodeDocument,
} from "../design-tree/tree.ts";
import type { DesignNode, FileScope } from "../design-tree/types.ts";
import { getChange } from "./spec.ts";

export type ReconciliationIssueCode = "incomplete_tasks" | "missing_design_binding";
export type PostAssessOutcome = "pass" | "reopen" | "ambiguous";
export type AssessmentKind = "spec" | "cleave";

export interface ReconciliationIssue {
	code: ReconciliationIssueCode;
	message: string;
	suggestedAction: string;
}

export interface LifecycleReconciliationStatus {
	changeName: string;
	boundNodeIds: string[];
	issues: ReconciliationIssue[];
}

export interface PostAssessReconciliationInput {
	assessmentKind: AssessmentKind;
	outcome: PostAssessOutcome;
	summary?: string;
	changedFiles?: string[];
	constraints?: string[];
}

export interface PostAssessReconciliationResult {
	changeName: string;
	outcome: PostAssessOutcome;
	reopened: boolean;
	warning?: string;
	updatedTaskState: boolean;
	updatedNodeIds: string[];
	appendedFileScope: string[];
	appendedConstraints: string[];
}

function findBoundNodes(cwd: string, changeName: string): DesignNode[] {
	const docsDir = path.join(cwd, "docs");
	const tree = scanDesignDocs(docsDir);
	return Array.from(tree.nodes.values()).filter((node) =>
		node.openspec_change === changeName || node.id === changeName,
	);
}

export function evaluateLifecycleReconciliation(cwd: string, changeName: string): LifecycleReconciliationStatus {
	const issues: ReconciliationIssue[] = [];
	const change = getChange(cwd, changeName);
	const boundNodes = findBoundNodes(cwd, changeName);

	if (boundNodes.length === 0) {
		issues.push({
			code: "missing_design_binding",
			message: `OpenSpec change '${changeName}' is not bound to any design-tree node via openspec_change or matching node ID.`,
			suggestedAction: "Bind the change to a decided/implementing design node before archive so lifecycle tracking stays traceable.",
		});
	}

	if (change && change.hasTasks && change.totalTasks > 0 && change.doneTasks < change.totalTasks) {
		issues.push({
			code: "incomplete_tasks",
			message: `OpenSpec change '${changeName}' still has ${change.totalTasks - change.doneTasks} incomplete task(s) in tasks.md.`,
			suggestedAction: "Reconcile tasks.md to match implemented work or finish the remaining tasks before archive.",
		});
	}

	return {
		changeName,
		boundNodeIds: boundNodes.map((node) => node.id),
		issues,
	};
}

function appendPostAssessFollowUpTask(changePath: string, input: PostAssessReconciliationInput): boolean {
	const tasksPath = path.join(changePath, "tasks.md");
	if (!fs.existsSync(tasksPath)) return false;

	const content = fs.readFileSync(tasksPath, "utf-8");
	const summary = (input.summary || `Resolve remaining findings from /assess ${input.assessmentKind}`).trim();
	const escapedSummary = summary.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	const existing = new RegExp(`^\\s*-\\s+\\[ \\]\\s+\\d+(?:\\.\\d+)?\\s+${escapedSummary}$`, "m");
	if (existing.test(content)) return false;

	const groupNumbers = [...content.matchAll(/^##\s+(\d+)\.\s+/gm)].map((m) => parseInt(m[1], 10));
	const nextGroup = groupNumbers.length > 0 ? Math.max(...groupNumbers) + 1 : 1;
	const nextTask = `${nextGroup}.1`;
	const block = [
		"",
		`## ${nextGroup}. Post-assess follow-up`,
		"<!-- skills: typescript -->",
		`- [ ] ${nextTask} ${summary}`,
	].join("\n");

	fs.writeFileSync(tasksPath, content.replace(/\s*$/, "") + block + "\n", "utf-8");
	return true;
}

function appendImplementationDeltas(
	node: DesignNode,
	changedFiles: readonly string[],
	constraints: readonly string[],
): { appendedFiles: string[]; appendedConstraints: string[] } {
	const sections = getNodeSections(node);
	const existingFiles = new Set(sections.implementationNotes.fileScope.map((entry) => entry.path));
	const existingConstraints = new Set(sections.implementationNotes.constraints);

	const newFileScope: FileScope[] = [];
	for (const file of changedFiles) {
		if (!existingFiles.has(file)) {
			newFileScope.push({
				path: file,
				description: "Post-assess reconciliation delta — touched during follow-up fixes",
				action: "modified",
			});
		}
	}

	const newConstraints = constraints.filter((constraint) => !existingConstraints.has(constraint));
	if (newFileScope.length === 0 && newConstraints.length === 0) {
		return { appendedFiles: [], appendedConstraints: [] };
	}

	sections.implementationNotes.fileScope.push(...newFileScope);
	sections.implementationNotes.constraints.push(...newConstraints);
	writeNodeDocument(node, sections);

	return {
		appendedFiles: newFileScope.map((entry) => entry.path),
		appendedConstraints: newConstraints,
	};
}

export function applyPostAssessReconciliation(
	cwd: string,
	changeName: string,
	input: PostAssessReconciliationInput,
): PostAssessReconciliationResult {
	const change = getChange(cwd, changeName);
	const boundNodes = findBoundNodes(cwd, changeName);
	const changedFiles = (input.changedFiles ?? []).map((file) => file.trim()).filter(Boolean);
	const constraints = (input.constraints ?? []).map((constraint) => constraint.trim()).filter(Boolean);

	let updatedTaskState = false;
	let warning: string | undefined;
	const updatedNodeIds: string[] = [];
	const appendedFileScope: string[] = [];
	const appendedConstraints: string[] = [];

	if (input.outcome === "ambiguous") {
		warning = "Post-assess reconciliation could not safely map reviewer prose back into OpenSpec tasks. No semantic task rewriting was attempted.";
	} else if (input.outcome === "reopen" && change) {
		updatedTaskState = appendPostAssessFollowUpTask(change.path, input);
	}

	for (const node of boundNodes) {
		const delta = appendImplementationDeltas(node, changedFiles, constraints);
		if (delta.appendedFiles.length > 0 || delta.appendedConstraints.length > 0) {
			updatedNodeIds.push(node.id);
			appendedFileScope.push(...delta.appendedFiles);
			appendedConstraints.push(...delta.appendedConstraints);
		}
	}

	return {
		changeName,
		outcome: input.outcome,
		reopened: input.outcome === "reopen",
		warning,
		updatedTaskState,
		updatedNodeIds,
		appendedFileScope,
		appendedConstraints,
	};
}

export function formatReconciliationIssues(issues: readonly ReconciliationIssue[]): string {
	return issues.map((issue) => `- ${issue.message}\n  → ${issue.suggestedAction}`).join("\n");
}

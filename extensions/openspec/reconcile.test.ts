import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { generateFrontmatter, scanDesignDocs } from "../design-tree/tree.ts";
import type { DesignNode, NodeStatus } from "../design-tree/types.ts";
import {
	applyPostAssessReconciliation,
	evaluateLifecycleReconciliation,
	formatReconciliationIssues,
} from "./reconcile.ts";

function writeDesignDoc(docsDir: string, id: string, status: NodeStatus, openspecChange?: string): void {
	const node: DesignNode = {
		id,
		title: `Test ${id}`,
		status,
		dependencies: [],
		related: [],
		tags: [],
		open_questions: [],
		branches: [],
		openspec_change: openspecChange,
		filePath: path.join(docsDir, `${id}.md`),
		lastModified: Date.now(),
	};
	const fm = generateFrontmatter(node);
	const content = fm + `\n# ${node.title}\n\n## Overview\n\nTest node.\n`;
	fs.writeFileSync(path.join(docsDir, `${id}.md`), content);
}

describe("evaluateLifecycleReconciliation", () => {
	let tmpDir: string;
	let docsDir: string;
	let changeDir: string;

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "openspec-reconcile-"));
		docsDir = path.join(tmpDir, "docs");
		changeDir = path.join(tmpDir, "openspec", "changes", "my-change");
		fs.mkdirSync(docsDir, { recursive: true });
		fs.mkdirSync(changeDir, { recursive: true });
		fs.writeFileSync(path.join(changeDir, "proposal.md"), "# Proposal");
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("passes when tasks are complete and a design node is bound", () => {
		writeDesignDoc(docsDir, "my-change", "implementing", "my-change");
		fs.writeFileSync(path.join(changeDir, "tasks.md"), [
			"## 1. A",
			"- [x] 1.1 Done",
		].join("\n"));

		const result = evaluateLifecycleReconciliation(tmpDir, "my-change");
		assert.deepStrictEqual(result.boundNodeIds, ["my-change"]);
		assert.equal(result.issues.length, 0);
	});

	it("reports incomplete tasks as stale lifecycle state", () => {
		writeDesignDoc(docsDir, "my-change", "implementing", "my-change");
		fs.writeFileSync(path.join(changeDir, "tasks.md"), [
			"## 1. A",
			"- [x] 1.1 Done",
			"- [ ] 1.2 Remaining",
		].join("\n"));

		const result = evaluateLifecycleReconciliation(tmpDir, "my-change");
		assert.equal(result.issues.length, 1);
		assert.equal(result.issues[0].code, "incomplete_tasks");
	});

	it("reports missing design-tree binding", () => {
		fs.writeFileSync(path.join(changeDir, "tasks.md"), [
			"## 1. A",
			"- [x] 1.1 Done",
		].join("\n"));

		const result = evaluateLifecycleReconciliation(tmpDir, "my-change");
		assert.equal(result.issues.length, 1);
		assert.equal(result.issues[0].code, "missing_design_binding");
	});

	it("formats reconciliation issues for operator-facing messages", () => {
		const text = formatReconciliationIssues([
			{
				code: "missing_design_binding",
				message: "Missing design binding",
				suggestedAction: "Bind the change first.",
			},
		]);
		assert.match(text, /Missing design binding/);
		assert.match(text, /Bind the change first/);
	});
});

describe("applyPostAssessReconciliation", () => {
	let tmpDir: string;
	let docsDir: string;
	let changeDir: string;

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "openspec-post-assess-"));
		docsDir = path.join(tmpDir, "docs");
		changeDir = path.join(tmpDir, "openspec", "changes", "my-change");
		fs.mkdirSync(docsDir, { recursive: true });
		fs.mkdirSync(changeDir, { recursive: true });
		fs.writeFileSync(path.join(changeDir, "proposal.md"), "# Proposal");
		fs.writeFileSync(path.join(changeDir, "tasks.md"), [
			"## 1. Initial",
			"- [x] 1.1 Done",
		].join("\n"));
		writeDesignDoc(docsDir, "my-change", "implementing", "my-change");
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("reopens task state by appending a follow-up task", () => {
		const result = applyPostAssessReconciliation(tmpDir, "my-change", {
			assessmentKind: "spec",
			outcome: "reopen",
			summary: "Resolve failing scenario coverage",
		});

		assert.equal(result.reopened, true);
		assert.equal(result.updatedTaskState, true);
		const content = fs.readFileSync(path.join(changeDir, "tasks.md"), "utf-8");
		assert.match(content, /Post-assess follow-up/);
		assert.match(content, /Resolve failing scenario coverage/);
	});

	it("preserves verifying state on pass without changing tasks", () => {
		const before = fs.readFileSync(path.join(changeDir, "tasks.md"), "utf-8");
		const result = applyPostAssessReconciliation(tmpDir, "my-change", {
			assessmentKind: "spec",
			outcome: "pass",
		});
		const after = fs.readFileSync(path.join(changeDir, "tasks.md"), "utf-8");

		assert.equal(result.reopened, false);
		assert.equal(result.updatedTaskState, false);
		assert.equal(after, before);
	});

	it("appends implementation-note file deltas and constraints", () => {
		const result = applyPostAssessReconciliation(tmpDir, "my-change", {
			assessmentKind: "cleave",
			outcome: "reopen",
			changedFiles: ["extensions/foo.ts"],
			constraints: ["Follow-up fixes must preserve stable output ordering"],
		});

		assert.deepStrictEqual(result.appendedFileScope, ["extensions/foo.ts"]);
		assert.deepStrictEqual(result.appendedConstraints, ["Follow-up fixes must preserve stable output ordering"]);

		const tree = scanDesignDocs(docsDir);
		const node = tree.nodes.get("my-change");
		assert.ok(node);
		const content = fs.readFileSync(node!.filePath, "utf-8");
		assert.match(content, /extensions\/foo.ts/);
		assert.match(content, /reconciliation delta/);
		assert.match(content, /stable output ordering/);
	});

	it("emits an explicit warning for ambiguous assessment", () => {
		const result = applyPostAssessReconciliation(tmpDir, "my-change", {
			assessmentKind: "spec",
			outcome: "ambiguous",
		});
		assert.match(result.warning ?? "", /No semantic task rewriting/);
	});
});

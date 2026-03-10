import { afterEach, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import designTreeExtension from "./index.ts";
import { generateFrontmatter } from "./tree.ts";
import type { DesignNode } from "./types.ts";

interface RegisteredTool {
	name: string;
	execute: (...args: unknown[]) => Promise<unknown>;
}

function createFakePi() {
	const tools: RegisteredTool[] = [];
	const commands = new Map<string, unknown>();
	const eventHandlers = new Map<string, unknown[]>();
	return {
		tools,
		commands,
		events: {
			emit() {},
		},
		registerTool(tool: RegisteredTool) {
			tools.push(tool);
		},
		registerCommand(name: string, command: unknown) {
			commands.set(name, command);
		},
		registerMessageRenderer() {},
		on(event: string, handler: unknown) {
			const handlers = eventHandlers.get(event) ?? [];
			handlers.push(handler);
			eventHandlers.set(event, handlers);
		},
		async sendMessage() {},
	};
}

function writeDesignDoc(docsDir: string, id: string): void {
	const node: DesignNode = {
		id,
		title: `Test ${id}`,
		status: "decided",
		dependencies: [],
		related: [],
		tags: [],
		open_questions: [],
		branches: [],
		filePath: path.join(docsDir, `${id}.md`),
		lastModified: Date.now(),
	};
	const content = `${generateFrontmatter(node)}\n# ${node.title}\n\n## Overview\n\nTest node.\n`;
	fs.writeFileSync(node.filePath, content);
}

describe("design-tree lifecycle metadata", () => {
	let tmpDir: string;
	let pi: ReturnType<typeof createFakePi>;

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "design-tree-index-"));
		const docsDir = path.join(tmpDir, "docs");
		const changeDir = path.join(tmpDir, "openspec", "changes", "my-change");
		fs.mkdirSync(docsDir, { recursive: true });
		fs.mkdirSync(changeDir, { recursive: true });
		fs.writeFileSync(path.join(changeDir, "proposal.md"), "# Proposal\n");
		writeDesignDoc(docsDir, "my-change");

		pi = createFakePi();
		designTreeExtension(pi as unknown as ExtensionAPI);
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	interface NodeLifecycle {
		boundToOpenSpec: boolean;
		bindingStatus: "bound" | "unbound" | "unknown";
		implementationPhase?: boolean;
		archiveReady: boolean | null;
		nextAction: string | null;
		reopenSignalTarget?: string;
		openspecStage?: string | null;
		verificationSubstate?: string | null;
	}

	async function runTool(params: Record<string, unknown>) {
		const tool = pi.tools.find((entry) => entry.name === "design_tree");
		assert.ok(tool, "missing design_tree tool");
		const result = await tool.execute("tool-1", params, {} as never, () => {}, { cwd: tmpDir });
		return result as {
			details: {
				nodes: Array<{ lifecycle: NodeLifecycle }>;
				node: { lifecycle: NodeLifecycle };
			};
		};
	}

	it("reports fallback id-based OpenSpec bindings in list and node metadata", async () => {
		const listResult = await runTool({ action: "list" });
		assert.equal(listResult.details.nodes[0].lifecycle.boundToOpenSpec, true);

		const nodeResult = await runTool({ action: "node", node_id: "my-change" });
		assert.equal(nodeResult.details.node.lifecycle.boundToOpenSpec, true);
		assert.equal(nodeResult.details.node.lifecycle.reopenSignalTarget, "my-change");
	});

	it("list action exposes canonical bindingStatus from lifecycle resolver", async () => {
		const listResult = await runTool({ action: "list" });
		const node = listResult.details.nodes[0];
		// boundToOpenSpec should remain true (backward-compat)
		assert.equal(node.lifecycle.boundToOpenSpec, true);
		// bindingStatus should be the normalized string form
		assert.ok(
			["bound", "unbound", "unknown"].includes(node.lifecycle.bindingStatus),
			`bindingStatus must be canonical, got: ${node.lifecycle.bindingStatus}`,
		);
		// archiveReady and nextAction fields must be present (may be null for a proposal-only change)
		assert.ok("archiveReady" in node.lifecycle, "archiveReady must be present in list lifecycle");
		assert.ok("nextAction" in node.lifecycle, "nextAction must be present in list lifecycle");
	});

	it("node action exposes full canonical lifecycle fields", async () => {
		const nodeResult = await runTool({ action: "node", node_id: "my-change" });
		const lc = nodeResult.details.node.lifecycle;

		// Backward-compat fields preserved
		assert.equal(lc.boundToOpenSpec, true);
		assert.equal(lc.reopenSignalTarget, "my-change");

		// Canonical fields from resolveLifecycleSummary
		assert.ok(
			["bound", "unbound", "unknown"].includes(lc.bindingStatus),
			`bindingStatus must be canonical, got: ${lc.bindingStatus}`,
		);
		assert.ok("archiveReady" in lc, "archiveReady must be present");
		assert.ok("verificationSubstate" in lc, "verificationSubstate must be present");
		assert.ok("nextAction" in lc, "nextAction must be present");
		assert.ok("openspecStage" in lc, "openspecStage must be present");
	});

	it("unbound node reports unbound bindingStatus without lifecycle summary", async () => {
		// Create a node that has no matching openspec change directory
		const docsDir = path.join(tmpDir, "docs");
		const node: DesignNode = {
			id: "orphan-node",
			title: "Orphan",
			status: "decided",
			dependencies: [],
			related: [],
			tags: [],
			open_questions: [],
			branches: [],
			filePath: path.join(docsDir, "orphan-node.md"),
			lastModified: Date.now(),
		};
		const { generateFrontmatter } = await import("./tree.ts");
		const content = `${generateFrontmatter(node)}\n# Orphan\n\n## Overview\n\nNo openspec change.\n`;
		fs.writeFileSync(node.filePath, content);

		const nodeResult = await runTool({ action: "node", node_id: "orphan-node" });
		const lc = nodeResult.details.node.lifecycle;
		assert.equal(lc.boundToOpenSpec, false, "orphan should not be bound");
		assert.equal(lc.bindingStatus, "unbound", "orphan bindingStatus should be 'unbound'");
		assert.equal(lc.archiveReady, null, "archiveReady should be null when no lifecycle summary");
		assert.equal(lc.verificationSubstate, null, "verificationSubstate should be null when no lifecycle summary");
		assert.equal(lc.nextAction, null, "nextAction should be null when no lifecycle summary");
	});
});

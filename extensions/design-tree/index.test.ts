import { afterEach, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import designTreeExtension from "./index.ts";
import { generateFrontmatter } from "./tree.ts";
import type { DesignNode } from "./types.ts";
import { sharedState } from "../shared-state.ts";

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
		// bindingStatus must be "bound" for a known-bound node — not merely a valid string
		assert.equal(
			node.lifecycle.bindingStatus,
			"bound",
			`bindingStatus must be "bound" for a bound node, got: ${node.lifecycle.bindingStatus}`,
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

describe("design-tree dashboard refresh helper", () => {
	let tmpDir: string;
	let pi: ReturnType<typeof createFakePi>;
	let emitCalls: Array<{ channel: string; data: unknown }>;

	function createFakePiWithEmitTracking() {
		emitCalls = [];
		const tools: RegisteredTool[] = [];
		const commands = new Map<string, unknown>();
		return {
			tools,
			commands,
			events: {
				emit(channel: string, data: unknown) {
					emitCalls.push({ channel, data });
				},
				on() {
					return () => {};
				},
			},
			registerTool(tool: RegisteredTool) {
				tools.push(tool);
			},
			registerCommand(name: string, command: unknown) {
				commands.set(name, command);
			},
			registerMessageRenderer() {},
			on(_event: string, _handler: unknown) {},
			async sendMessage() {},
		};
	}

	type ToolResult = { content: Array<{ type: string; text?: string }>; details: Record<string, unknown>; isError?: boolean };

	async function runUpdateTool(params: Record<string, unknown>): Promise<ToolResult> {
		const tool = pi.tools.find((entry) => entry.name === "design_tree_update");
		assert.ok(tool, "missing design_tree_update tool");
		return tool.execute("tool-1", params, {} as never, () => {}, { cwd: tmpDir }) as Promise<ToolResult>;
	}

	async function runQueryTool(params: Record<string, unknown>): Promise<ToolResult> {
		const tool = pi.tools.find((entry) => entry.name === "design_tree");
		assert.ok(tool, "missing design_tree tool");
		return tool.execute("tool-1", params, {} as never, () => {}, { cwd: tmpDir }) as Promise<ToolResult>;
	}

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "design-tree-dashboard-refresh-"));
		const docsDir = path.join(tmpDir, "docs");
		fs.mkdirSync(docsDir, { recursive: true });
		// Write a seed node for mutation tests
		writeDesignDoc(docsDir, "alpha-node");

		pi = createFakePiWithEmitTracking() as unknown as ReturnType<typeof createFakePi>;
		designTreeExtension(pi as unknown as ExtensionAPI);
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("emits a dashboard update event when node status changes", async () => {
		const before = emitCalls.length;
		await runUpdateTool({ action: "set_status", node_id: "alpha-node", status: "exploring" });
		const dashboardEmits = emitCalls.slice(before).filter((c) => c.channel === "dashboard:update");
		assert.ok(dashboardEmits.length >= 1, "expected at least one dashboard:update event after set_status");
	});

	it("emits a dashboard update event when focus changes", async () => {
		const before = emitCalls.length;
		await runUpdateTool({ action: "focus", node_id: "alpha-node" });
		const dashboardEmits = emitCalls.slice(before).filter((c) => c.channel === "dashboard:update");
		assert.ok(dashboardEmits.length >= 1, "expected at least one dashboard:update event after focus");
	});

	it("dashboard state reflects focused node after mutation", async () => {
		await runUpdateTool({ action: "focus", node_id: "alpha-node" });
		// The shared state should have the focused node populated
		const dt = sharedState.designTree;
		assert.ok(dt, "designTree state should be populated after focus");
		assert.ok(dt!.focusedNode, "focusedNode should be set after focus action");
		assert.equal(dt!.focusedNode!.id, "alpha-node", "focused node id should match");
	});

	it("dashboard state reflects correct node counts after status mutation", async () => {
		// After init, alpha-node is 'decided'
		const before = sharedState.designTree?.decidedCount ?? 0;
		await runUpdateTool({ action: "set_status", node_id: "alpha-node", status: "exploring" });
		const dt = sharedState.designTree;
		assert.ok(dt, "designTree state should be populated after set_status");
		// decidedCount should decrease, exploringCount should increase
		assert.ok(dt!.decidedCount < before || before === 0, "decidedCount should not exceed pre-mutation value");
		assert.ok(dt!.exploringCount >= 1, "exploringCount should reflect the exploring node");
	});

	it("dashboard state clears focusedNode when unfocus is called", async () => {
		await runUpdateTool({ action: "focus", node_id: "alpha-node" });
		assert.ok(sharedState.designTree?.focusedNode, "node should be focused before unfocus");
		await runUpdateTool({ action: "unfocus" });
		assert.equal(sharedState.designTree?.focusedNode, null, "focusedNode should be null after unfocus");
	});

	it("emits a dashboard update event when add_research is called", async () => {
		const before = emitCalls.length;
		await runUpdateTool({ action: "add_research", node_id: "alpha-node", heading: "Findings", content: "Some research." });
		const dashboardEmits = emitCalls.slice(before).filter((c) => c.channel === "dashboard:update");
		assert.ok(dashboardEmits.length >= 1, "expected at least one dashboard:update event after add_research");
	});

	it("emits a dashboard update event when add_decision is called", async () => {
		const before = emitCalls.length;
		await runUpdateTool({ action: "add_decision", node_id: "alpha-node", decision_title: "Use TypeScript", decision_status: "decided", rationale: "Type safety" });
		const dashboardEmits = emitCalls.slice(before).filter((c) => c.channel === "dashboard:update");
		assert.ok(dashboardEmits.length >= 1, "expected at least one dashboard:update event after add_decision");
	});

	it("emits a dashboard update event when add_impl_notes is called", async () => {
		const before = emitCalls.length;
		await runUpdateTool({ action: "add_impl_notes", node_id: "alpha-node", constraints: ["Must be fast"] });
		const dashboardEmits = emitCalls.slice(before).filter((c) => c.channel === "dashboard:update");
		assert.ok(dashboardEmits.length >= 1, "expected at least one dashboard:update event after add_impl_notes");
	});

	it("emits a dashboard update event when add_question is called", async () => {
		const before = emitCalls.length;
		await runUpdateTool({ action: "add_question", node_id: "alpha-node", question: "What is the best approach?" });
		const dashboardEmits = emitCalls.slice(before).filter((c) => c.channel === "dashboard:update");
		assert.ok(dashboardEmits.length >= 1, "expected at least one dashboard:update event after add_question");
	});

	it("set_priority persists priority to frontmatter and returns success", async () => {
		const result = await runUpdateTool({ action: "set_priority", node_id: "alpha-node", priority: 2 });
		assert.ok(!result.isError, `set_priority should succeed, got: ${result.content[0]?.text}`);
		assert.match(result.content[0]?.text ?? "", /priority.*2|2.*priority/i);

		// Verify frontmatter was written
		const filePath = path.join(tmpDir, "docs", "alpha-node.md");
		const raw = fs.readFileSync(filePath, "utf8");
		assert.match(raw, /priority:\s*2/);
	});

	it("set_priority rejects out-of-range value", async () => {
		const result = await runUpdateTool({ action: "set_priority", node_id: "alpha-node", priority: 6 });
		assert.ok(result.isError, "set_priority should fail for priority 6");
	});

	it("set_priority rejects missing priority", async () => {
		const result = await runUpdateTool({ action: "set_priority", node_id: "alpha-node" });
		assert.ok(result.isError, "set_priority should fail when priority is missing");
	});

	it("set_priority fails for unknown node", async () => {
		const result = await runUpdateTool({ action: "set_priority", node_id: "no-such-node", priority: 1 });
		assert.ok(result.isError, "set_priority should fail for unknown node");
	});

	it("set_issue_type persists issue_type to frontmatter and returns success", async () => {
		const result = await runUpdateTool({ action: "set_issue_type", node_id: "alpha-node", issue_type: "feature" });
		assert.ok(!result.isError, `set_issue_type should succeed, got: ${result.content[0]?.text}`);
		assert.match(result.content[0]?.text ?? "", /feature/i);

		const filePath = path.join(tmpDir, "docs", "alpha-node.md");
		const raw = fs.readFileSync(filePath, "utf8");
		assert.match(raw, /issue_type:\s*feature/);
	});

	it("set_issue_type rejects invalid issue type", async () => {
		const result = await runUpdateTool({ action: "set_issue_type", node_id: "alpha-node", issue_type: "invalid-type" });
		assert.ok(result.isError, "set_issue_type should fail for invalid type");
	});

	it("set_issue_type fails for unknown node", async () => {
		const result = await runUpdateTool({ action: "set_issue_type", node_id: "no-such-node", issue_type: "bug" });
		assert.ok(result.isError, "set_issue_type should fail for unknown node");
	});

	it("list action includes priority and issue_type fields", async () => {
		await runUpdateTool({ action: "set_priority", node_id: "alpha-node", priority: 3 });
		await runUpdateTool({ action: "set_issue_type", node_id: "alpha-node", issue_type: "task" });

		const result = await runQueryTool({ action: "list" });
		const nodes = JSON.parse(result.content[0]?.text ?? "[]");
		const alpha = nodes.find((n: { id: string }) => n.id === "alpha-node");
		assert.ok(alpha, "alpha-node should appear in list");
		assert.equal(alpha.priority, 3, "list should include priority");
		assert.equal(alpha.issue_type, "task", "list should include issue_type");
	});

	it("node action includes priority and issue_type fields", async () => {
		await runUpdateTool({ action: "set_priority", node_id: "alpha-node", priority: 1 });
		await runUpdateTool({ action: "set_issue_type", node_id: "alpha-node", issue_type: "bug" });

		const result = await runQueryTool({ action: "node", node_id: "alpha-node" });
		const raw = result.content[0]?.text ?? "";
		const jsonPart = raw.split("--- Document Content ---")[0].trim();
		const nodeData = JSON.parse(jsonPart);
		assert.equal(nodeData.priority, 1, "node action should include priority");
		assert.equal(nodeData.issue_type, "bug", "node action should include issue_type");
	});
});

describe("design-tree ready and blocked query actions", () => {
	let tmpDir: string;
	let pi: ReturnType<typeof createFakePi>;

	function writeFrontmatterDoc(docsDir: string, node: Partial<DesignNode> & { id: string; title: string }): void {
		const full: DesignNode = {
			status: "seed",
			dependencies: [],
			related: [],
			tags: [],
			open_questions: [],
			branches: [],
			filePath: path.join(docsDir, `${node.id}.md`),
			lastModified: Date.now(),
			...node,
		};
		const content = `${generateFrontmatter(full)}\n# ${full.title}\n\n## Overview\n\nTest.\n`;
		fs.writeFileSync(full.filePath, content);
	}

	async function runQueryTool(params: Record<string, unknown>) {
		const tool = pi.tools.find((e) => e.name === "design_tree");
		assert.ok(tool, "missing design_tree tool");
		return tool.execute("tool-1", params, {} as never, () => {}, { cwd: tmpDir }) as Promise<{
			details: Record<string, unknown>;
			content: Array<{ type: string; text: string }>;
		}>;
	}

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "design-tree-ready-blocked-"));
		const docsDir = path.join(tmpDir, "docs");
		fs.mkdirSync(docsDir, { recursive: true });

		// dep-a: implemented (satisfies dependencies)
		writeFrontmatterDoc(docsDir, { id: "dep-a", title: "Dep A", status: "implemented" });
		// dep-b: decided (NOT implemented — creates a blocker)
		writeFrontmatterDoc(docsDir, { id: "dep-b", title: "Dep B", status: "decided" });
		// ready-node: decided + all deps implemented
		writeFrontmatterDoc(docsDir, { id: "ready-node", title: "Ready Node", status: "decided", dependencies: ["dep-a"], priority: 2, issue_type: "feature" });
		// blocked-by-dep: decided + has unimplemented dep
		writeFrontmatterDoc(docsDir, { id: "blocked-by-dep", title: "Blocked By Dep", status: "decided", dependencies: ["dep-b"] });
		// explicitly-blocked: status=blocked
		writeFrontmatterDoc(docsDir, { id: "explicitly-blocked", title: "Explicitly Blocked", status: "blocked" });
		// no-deps: decided with no deps (should appear in ready)
		writeFrontmatterDoc(docsDir, { id: "no-deps", title: "No Deps", status: "decided", priority: 1 });

		pi = createFakePi();
		designTreeExtension(pi as unknown as ExtensionAPI);
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("ready action returns decided nodes with all deps implemented", async () => {
		const result = await runQueryTool({ action: "ready" });
		const ready = result.details.ready as Array<{ id: string }>;
		const ids = ready.map((n) => n.id);
		assert.ok(ids.includes("ready-node"), "ready-node should appear (decided, dep implemented)");
		assert.ok(ids.includes("no-deps"), "no-deps should appear (decided, no dependencies)");
		assert.ok(!ids.includes("blocked-by-dep"), "blocked-by-dep should NOT appear (dep not implemented)");
		assert.ok(!ids.includes("dep-a"), "dep-a should NOT appear (status=implemented, not decided)");
		assert.ok(!ids.includes("explicitly-blocked"), "explicitly-blocked should NOT appear (status=blocked)");
	});

	it("ready action sorts by priority ascending (lower number = higher priority)", async () => {
		const result = await runQueryTool({ action: "ready" });
		const ready = result.details.ready as Array<{ id: string; priority: number | null }>;
		const priorities = ready.map((n) => n.priority ?? 5);
		for (let i = 1; i < priorities.length; i++) {
			assert.ok(priorities[i - 1] <= priorities[i], `priority order violated at index ${i}: ${priorities[i - 1]} > ${priorities[i]}`);
		}
	});

	it("ready action includes priority, issue_type, tags, and openspec_change fields", async () => {
		const result = await runQueryTool({ action: "ready" });
		const ready = result.details.ready as Array<{ id: string; priority: number | null; issue_type: string | null; tags: string[]; openspec_change: string | null }>;
		const rn = ready.find((n) => n.id === "ready-node");
		assert.ok(rn, "ready-node must be present");
		assert.equal(rn!.priority, 2);
		assert.equal(rn!.issue_type, "feature");
		assert.ok(Array.isArray(rn!.tags), "tags must be an array");
		assert.equal(rn!.openspec_change, null);
	});

	it("blocked action returns explicitly blocked nodes", async () => {
		const result = await runQueryTool({ action: "blocked" });
		const blocked = result.details.blocked as Array<{ id: string; blocking_deps: Array<{ id: string; status: string }> }>;
		const ids = blocked.map((n) => n.id);
		assert.ok(ids.includes("explicitly-blocked"), "explicitly-blocked must appear");
	});

	it("blocked action returns nodes with unimplemented dependencies", async () => {
		const result = await runQueryTool({ action: "blocked" });
		const blocked = result.details.blocked as Array<{ id: string; blocking_deps: Array<{ id: string; title: string; status: string }> }>;
		const bn = blocked.find((n) => n.id === "blocked-by-dep");
		assert.ok(bn, "blocked-by-dep must appear");
		assert.equal(bn!.blocking_deps.length, 1);
		assert.equal(bn!.blocking_deps[0].id, "dep-b");
		assert.equal(bn!.blocking_deps[0].status, "decided");
	});

	it("blocked action does not include implemented nodes", async () => {
		const result = await runQueryTool({ action: "blocked" });
		const blocked = result.details.blocked as Array<{ id: string }>;
		const ids = blocked.map((n) => n.id);
		assert.ok(!ids.includes("dep-a"), "dep-a (implemented) should not appear in blocked");
	});

	it("blocked action includes priority, issue_type, and blocking_deps fields on each entry", async () => {
		const result = await runQueryTool({ action: "blocked" });
		const blocked = result.details.blocked as Array<{
			id: string;
			priority: number | null;
			issue_type: string | null;
			tags: string[];
			openspec_change: string | null;
			blocking_deps: unknown[];
		}>;
		for (const entry of blocked) {
			assert.ok("priority" in entry, `${entry.id}: missing priority`);
			assert.ok("issue_type" in entry, `${entry.id}: missing issue_type`);
			assert.ok("tags" in entry, `${entry.id}: missing tags`);
			assert.ok("openspec_change" in entry, `${entry.id}: missing openspec_change`);
			assert.ok(Array.isArray(entry.blocking_deps), `${entry.id}: blocking_deps must be an array`);
		}
	});

	it("explicitly blocked node has empty blocking_deps array when it has no dependencies", async () => {
		const result = await runQueryTool({ action: "blocked" });
		const blocked = result.details.blocked as Array<{ id: string; blocking_deps: unknown[] }>;
		const eb = blocked.find((n) => n.id === "explicitly-blocked");
		assert.ok(eb, "explicitly-blocked must appear");
		assert.equal(eb!.blocking_deps.length, 0, "no dependencies so blocking_deps should be empty");
	});
});

// ─── Acceptance Criteria — list and node surface ─────────────────────────────

describe("acceptance criteria in list and node responses", () => {
	let tmpDir: string;
	let pi: ReturnType<typeof createFakePi>;

	function writeNodeWithAC(docsDir: string, id: string, acContent: string): void {
		const node: DesignNode = {
			id,
			title: `AC Node ${id}`,
			status: "decided",
			dependencies: [],
			related: [],
			tags: [],
			open_questions: [],
			branches: [],
			filePath: path.join(docsDir, `${id}.md`),
			lastModified: Date.now(),
		};
		const fm = generateFrontmatter(node);
		const content = `${fm}\n# ${node.title}\n\n## Overview\n\nTest.\n\n## Acceptance Criteria\n\n${acContent}\n`;
		fs.writeFileSync(node.filePath, content);
	}

	async function runTool(params: Record<string, unknown>) {
		const tool = pi.tools.find((t) => t.name === "design_tree");
		assert.ok(tool, "missing design_tree tool");
		return tool.execute("tool-1", params, {} as never, () => {}, { cwd: tmpDir }) as Promise<{
			details: Record<string, unknown>;
		}>;
	}

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "design-tree-ac-"));
		const docsDir = path.join(tmpDir, "docs");
		fs.mkdirSync(docsDir, { recursive: true });

		// Node with full acceptance criteria (1 scenario, 1 falsifiability, 1 constraint)
		writeNodeWithAC(
			docsDir,
			"ac-full",
			[
				"### Scenarios",
				"",
				"**Given** a user with access",
				"**When** they call the API",
				"**Then** a 200 response is returned",
				"",
				"### Falsifiability",
				"",
				"- This decision is wrong if: performance degrades by >10%",
				"",
				"### Constraints",
				"",
				"- [x] Must not break existing tests",
			].join("\n"),
		);

		// Node with preamble before first Given (W1 regression case)
		writeNodeWithAC(
			docsDir,
			"ac-preamble",
			[
				"### Scenarios",
				"",
				"This section describes integration scenarios.",
				"",
				"**Given** the system is running",
				"**When** a request arrives",
				"**Then** it is processed",
				"",
				"**Given** the system is idle",
				"**When** no request arrives",
				"**Then** nothing happens",
			].join("\n"),
		);

		// Node with no acceptance criteria
		writeDesignDoc(docsDir, "no-ac");

		pi = createFakePi();
		designTreeExtension(pi as unknown as ExtensionAPI);
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("list response includes acceptance_criteria_summary for nodes that have AC", async () => {
		const result = await runTool({ action: "list" });
		const nodes = result.details.nodes as Array<{
			id: string;
			acceptance_criteria_summary: { scenarios: number; falsifiability: number; constraints: number } | null;
		}>;
		const full = nodes.find((n) => n.id === "ac-full");
		assert.ok(full, "ac-full node missing from list");
		assert.ok(full.acceptance_criteria_summary !== null, "ac-full should have a non-null summary");
		assert.equal(full.acceptance_criteria_summary!.scenarios, 1, "expected 1 scenario");
		assert.equal(full.acceptance_criteria_summary!.falsifiability, 1, "expected 1 falsifiability item");
		assert.equal(full.acceptance_criteria_summary!.constraints, 1, "expected 1 constraint");
	});

	it("list response returns null acceptance_criteria_summary for nodes without AC", async () => {
		const result = await runTool({ action: "list" });
		const nodes = result.details.nodes as Array<{
			id: string;
			acceptance_criteria_summary: unknown;
		}>;
		const plain = nodes.find((n) => n.id === "no-ac");
		assert.ok(plain, "no-ac node missing from list");
		assert.equal(plain.acceptance_criteria_summary, null, "no-ac should return null summary");
	});

	it("node response includes full acceptanceCriteria for a node with AC", async () => {
		const result = await runTool({ action: "node", node_id: "ac-full" });
		const node = (result.details as { node: { sections: { acceptanceCriteria?: {
			scenarios: Array<{ title: string; given: string; when: string; then: string }>;
			falsifiability: Array<{ condition: string }>;
			constraints: Array<{ text: string; checked: boolean }>;
		} } } }).node;
		const ac = node.sections.acceptanceCriteria;
		assert.ok(ac, "acceptanceCriteria missing from node response");
		assert.equal(ac!.scenarios.length, 1);
		assert.equal(ac!.falsifiability.length, 1);
		assert.equal(ac!.constraints.length, 1);
		assert.equal(ac!.constraints[0].checked, true);
	});

	it("node response returns empty acceptanceCriteria arrays for a node without AC", async () => {
		const result = await runTool({ action: "node", node_id: "no-ac" });
		const node = (result.details as { node: { sections: { acceptanceCriteria?: { scenarios: unknown[]; falsifiability: unknown[]; constraints: unknown[] } } } }).node;
		const ac = node.sections.acceptanceCriteria;
		assert.ok(ac, "acceptanceCriteria field should always be present");
		assert.equal(ac!.scenarios.length, 0);
		assert.equal(ac!.falsifiability.length, 0);
		assert.equal(ac!.constraints.length, 0);
	});

	it("W1 regression: preamble before first Given does not cause off-by-one in scenario titles", async () => {
		const result = await runTool({ action: "node", node_id: "ac-preamble" });
		const node = (result.details as { node: { sections: { acceptanceCriteria?: { scenarios: Array<{ title: string }> } } } }).node;
		const ac = node.sections.acceptanceCriteria;
		assert.ok(ac, "acceptanceCriteria missing");
		assert.equal(ac!.scenarios.length, 2, "expected 2 scenarios");
		assert.equal(ac!.scenarios[0].title, "Scenario 1", "first scenario should be Scenario 1");
		assert.equal(ac!.scenarios[1].title, "Scenario 2", "second scenario should be Scenario 2");
	});
});

describe("design-spec-gates: set_status(decided) and implement", () => {
	let tmpDir: string;
	let pi: ReturnType<typeof createFakePi>;

	type ToolResult = { content: Array<{ type: string; text?: string }>; details: Record<string, unknown>; isError?: boolean };

	async function runUpdateTool(params: Record<string, unknown>): Promise<ToolResult> {
		const tool = pi.tools.find((entry) => entry.name === "design_tree_update");
		assert.ok(tool, "missing design_tree_update tool");
		return tool.execute("tool-1", params, {} as never, () => {}, { cwd: tmpDir }) as Promise<ToolResult>;
	}

	function writeExploringNode(docsDir: string, id: string): void {
		const node: DesignNode = {
			id,
			title: `Node ${id}`,
			status: "exploring",
			dependencies: [],
			related: [],
			tags: [],
			open_questions: [],
			branches: [],
			filePath: path.join(docsDir, `${id}.md`),
			lastModified: Date.now(),
		};
		const content = `${generateFrontmatter(node)}\n# ${node.title}\n\n## Overview\n\nTest.\n`;
		fs.writeFileSync(node.filePath, content);
	}

	beforeEach(() => {
		tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "design-tree-spec-gates-"));
		const docsDir = path.join(tmpDir, "docs");
		fs.mkdirSync(docsDir, { recursive: true });
		writeExploringNode(docsDir, "my-node");

		pi = createFakePi();
		designTreeExtension(pi as unknown as ExtensionAPI);
	});

	afterEach(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	// ── set_status(decided) gates ─────────────────────────────────────────

	it("set_status(decided) is blocked when design spec is missing", async () => {
		// No openspec/design/my-node/ and no openspec/design-archive/ entry
		const result = await runUpdateTool({ action: "set_status", node_id: "my-node", status: "decided" });
		assert.ok(result.isError, "should be blocked when design spec is missing");
		assert.match(
			result.content[0]?.text ?? "",
			/scaffold design spec first/i,
			"error message should mention scaffolding a design spec",
		);
	});

	it("set_status(decided) is blocked when design spec is active (not yet archived)", async () => {
		// Create openspec/design/my-node/ to simulate an active design change
		fs.mkdirSync(path.join(tmpDir, "openspec", "design", "my-node"), { recursive: true });
		fs.writeFileSync(path.join(tmpDir, "openspec", "design", "my-node", "proposal.md"), "# Design proposal\n");

		const result = await runUpdateTool({ action: "set_status", node_id: "my-node", status: "decided" });
		assert.ok(result.isError, "should be blocked when design spec is still active");
		assert.match(
			result.content[0]?.text ?? "",
			/archive.*design change/i,
			"error message should mention archiving the design change",
		);
	});

	it("set_status(decided) succeeds when design spec is archived", async () => {
		// Create openspec/design-archive/2026-03-12-my-node/
		fs.mkdirSync(path.join(tmpDir, "openspec", "design-archive", "2026-03-12-my-node"), { recursive: true });

		const result = await runUpdateTool({ action: "set_status", node_id: "my-node", status: "decided" });
		assert.ok(!result.isError, `should succeed when design spec is archived, got: ${result.content[0]?.text}`);
		assert.match(result.content[0]?.text ?? "", /decided/, "success message should mention decided");

		// W3: verify the status was actually written to disk
		const raw = fs.readFileSync(path.join(tmpDir, "docs", "my-node.md"), "utf8");
		assert.match(raw, /status:\s*decided/, "node status should be persisted as 'decided' on disk");
	});

	// ── implement gates ───────────────────────────────────────────────────

	it("implement is blocked when design spec is missing", async () => {
		// Manually set node to decided to bypass set_status gate
		const filePath = path.join(tmpDir, "docs", "my-node.md");
		const raw = fs.readFileSync(filePath, "utf8");
		fs.writeFileSync(filePath, raw.replace("status: exploring", "status: decided"));

		const result = await runUpdateTool({ action: "implement", node_id: "my-node" });
		assert.ok(result.isError, "implement should be blocked when design spec is missing");
		assert.match(
			result.content[0]?.text ?? "",
			/scaffold design spec first/i,
			"error message should mention scaffolding a design spec",
		);
	});

	it("implement is blocked when design spec is active (not yet archived)", async () => {
		// Set node to decided directly in the file
		const filePath = path.join(tmpDir, "docs", "my-node.md");
		const raw = fs.readFileSync(filePath, "utf8");
		fs.writeFileSync(filePath, raw.replace("status: exploring", "status: decided"));

		// Create active design change
		fs.mkdirSync(path.join(tmpDir, "openspec", "design", "my-node"), { recursive: true });
		fs.writeFileSync(path.join(tmpDir, "openspec", "design", "my-node", "proposal.md"), "# Design\n");

		const result = await runUpdateTool({ action: "implement", node_id: "my-node" });
		assert.ok(result.isError, "implement should be blocked when design spec is active");
		assert.match(
			result.content[0]?.text ?? "",
			/archive.*design change/i,
			"error message should mention archiving the design change",
		);
	});

	it("implement succeeds when design spec is archived", async () => {
		// Set node to decided directly in the file
		const filePath = path.join(tmpDir, "docs", "my-node.md");
		const raw = fs.readFileSync(filePath, "utf8");
		fs.writeFileSync(filePath, raw.replace("status: exploring", "status: decided"));

		// Create archived design change
		fs.mkdirSync(path.join(tmpDir, "openspec", "design-archive", "2026-03-12-my-node"), { recursive: true });

		const result = await runUpdateTool({ action: "implement", node_id: "my-node" });
		assert.ok(!result.isError, `implement should succeed when design spec is archived, got: ${result.content[0]?.text}`);

		// W4: verify scaffoldOpenSpecChange side-effect — proposal.md should be created
		const changeDir = path.join(tmpDir, "openspec", "changes", "my-node");
		assert.ok(fs.existsSync(changeDir), "openspec/changes/<node-id>/ directory should be created");
		assert.ok(
			fs.existsSync(path.join(changeDir, "proposal.md")),
			"proposal.md should be scaffolded in the change directory",
		);
	});
});

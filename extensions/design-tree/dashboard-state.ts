import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

import type { DesignNode, DesignTree } from "./types.ts";
import { getAllOpenQuestions } from "./tree.ts";
import { sharedState, DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import type { DesignTreeDashboardState } from "../shared-state.ts";
import { debug } from "../debug.ts";

export function emitDesignTreeState(pi: ExtensionAPI, dt: DesignTree, focused: DesignNode | null): void {
	if (dt.nodes.size === 0) return;
	const allNodes = Array.from(dt.nodes.values());
	// Exclude implemented/deferred nodes from the active dashboard view — they're archived journals
	const nodes = allNodes.filter((n) => n.status !== "implemented" && n.status !== "deferred");
	const state: DesignTreeDashboardState = {
		nodeCount: nodes.length,
		decidedCount: nodes.filter((n) => n.status === "decided").length,
		exploringCount: nodes.filter((n) => n.status === "exploring" || n.status === "seed").length,
		implementingCount: nodes.filter((n) => n.status === "implementing").length,
		implementedCount: allNodes.filter((n) => n.status === "implemented").length,
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
					filePath: focused.filePath,
				}
			: null,
		nodes: nodes.map((n) => ({
			id: n.id,
			title: n.title,
			status: n.status,
			questionCount: n.open_questions.length,
			filePath: n.filePath,
			branches: n.branches ?? [],
		})),
		implementingNodes: nodes
			.filter((n) => n.status === "implementing")
			.map((n) => ({ id: n.id, title: n.title, branch: n.branches?.[0], filePath: n.filePath })),
	};

	sharedState.designTree = state;
	debug("design-tree", "emitState", { nodeCount: nodes.length, decided: state.decidedCount, exploring: state.exploringCount });
	pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "design-tree" });
}

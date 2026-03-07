/**
 * Design tree archive gate — transition implementing nodes to implemented on archive.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { scanDesignDocs, writeNodeDocument, getNodeSections } from "../design-tree/tree.ts";

/**
 * Scan the design tree for nodes whose openspec_change matches the archived
 * change name. Transition any node in "implementing" status to "implemented"
 * using a single writeNodeDocument call (consistent with executeImplement).
 *
 * @param cwd     Project root (parent of the docs/ directory)
 * @param changeName  OpenSpec change name to match against openspec_change field
 * @returns IDs of nodes transitioned to implemented
 */
export function transitionDesignNodesOnArchive(cwd: string, changeName: string): string[] {
	const docsDir = path.join(cwd, "docs");
	if (!fs.existsSync(docsDir)) return [];

	const tree = scanDesignDocs(docsDir);
	const transitioned: string[] = [];

	for (const node of tree.nodes.values()) {
		if (node.openspec_change === changeName && node.status === "implementing") {
			const sections = getNodeSections(node);
			writeNodeDocument({ ...node, status: "implemented" }, sections);
			transitioned.push(node.id);
		}
	}
	return transitioned;
}

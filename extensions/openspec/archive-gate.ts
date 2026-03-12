/**
 * Design tree archive/lifecycle gate helpers.
 *
 * Centralizes OpenSpec ↔ design-tree binding truth so status surfaces,
 * reconciliation, and archive transitions all agree on whether a change is
 * bound to a design node.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { scanDesignDocs, writeNodeDocument, getNodeSections } from "../design-tree/tree.ts";
import type { DesignNode } from "../design-tree/types.ts";

export type OpenSpecBindingMatch = "explicit" | "id-fallback";

// ─── Design-phase spec binding ───────────────────────────────────────────────

/**
 * Result of resolving whether a design node has a design-phase OpenSpec change.
 *
 * Paths checked:
 *  - active:   openspec/design/<nodeId>/      (a live design-phase change)
 *  - archived: openspec/design-archive/YYYY-MM-DD-<nodeId>/  (completed and archived)
 *
 * Exactly one of {active, archived} can be true; missing is true when neither exists.
 */
export interface DesignSpecBinding {
	/** A completed design-phase change exists in openspec/design-archive/. */
	archived: boolean;
	/** A live design-phase change exists in openspec/design/. */
	active: boolean;
	/** No design-phase change found in either location. */
	missing: boolean;
}

/**
 * Scan the design-phase OpenSpec directories for a node's design change.
 *
 * @param cwd    Project root
 * @param nodeId Design node ID to match
 */
export function resolveDesignSpecBinding(cwd: string, nodeId: string): DesignSpecBinding {
	const designDir = path.join(cwd, "openspec", "design", nodeId);
	const designArchiveDir = path.join(cwd, "openspec", "design-archive");

	// W1: require at least one file inside the directory before treating it as active
	const active =
		fs.existsSync(designDir) &&
		fs.statSync(designDir).isDirectory() &&
		fs.readdirSync(designDir).length > 0;

	// W2: scan both branches unconditionally — active takes precedence over archived
	// but we still detect archived so callers can surface manual-recovery conflicts.
	let archived = false;
	if (fs.existsSync(designArchiveDir)) {
		for (const entry of fs.readdirSync(designArchiveDir, { withFileTypes: true })) {
			if (!entry.isDirectory()) continue;
			// Match YYYY-MM-DD-<nodeId> convention
			const match = entry.name.match(/^\d{4}-\d{2}-\d{2}-(.+)$/);
			if (match && match[1] === nodeId) {
				archived = true;
				break;
			}
		}
	}

	// Precedence: active wins over archived (active change is not yet complete)
	return { archived: archived && !active, active, missing: !active && !archived };
}

export interface OpenSpecBindingResolution {
	bound: boolean;
	changeName: string | null;
	match: OpenSpecBindingMatch | null;
}

function listKnownOpenSpecChangeNames(cwd: string): Set<string> {
	const names = new Set<string>();
	const openspecDir = path.join(cwd, "openspec");
	const changesDir = path.join(openspecDir, "changes");
	const archiveDir = path.join(openspecDir, "archive");

	if (fs.existsSync(changesDir)) {
		for (const entry of fs.readdirSync(changesDir, { withFileTypes: true })) {
			if (entry.isDirectory()) names.add(entry.name);
		}
	}

	if (fs.existsSync(archiveDir)) {
		for (const entry of fs.readdirSync(archiveDir, { withFileTypes: true })) {
			if (!entry.isDirectory()) continue;
			const match = entry.name.match(/^\d{4}-\d{2}-\d{2}-(.+)$/);
			names.add(match ? match[1] : entry.name);
		}
	}

	return names;
}

export function resolveNodeOpenSpecBinding(cwd: string, node: DesignNode): OpenSpecBindingResolution {
	const knownChangeNames = listKnownOpenSpecChangeNames(cwd);

	if (node.openspec_change) {
		if (knownChangeNames.has(node.openspec_change)) {
			return {
				bound: true,
				changeName: node.openspec_change,
				match: "explicit",
			};
		}
		return {
			bound: false,
			changeName: node.openspec_change,
			match: null,
		};
	}

	if (knownChangeNames.has(node.id)) {
		return {
			bound: true,
			changeName: node.id,
			match: "id-fallback",
		};
	}

	return {
		bound: false,
		changeName: null,
		match: null,
	};
}

export function resolveBoundDesignNodes(cwd: string, changeName: string): DesignNode[] {
	const docsDir = path.join(cwd, "docs");
	if (!fs.existsSync(docsDir)) return [];

	const tree = scanDesignDocs(docsDir);
	return Array.from(tree.nodes.values()).filter((node) => {
		const binding = resolveNodeOpenSpecBinding(cwd, node);
		return binding.bound && binding.changeName === changeName;
	});
}

/**
 * Scan the design tree for nodes matching the archived OpenSpec change.
 * Matches by explicit `openspec_change` frontmatter field OR by convention
 * (node ID = change name) using the shared binding resolver. Transitions
 * `implementing` or `decided` nodes to `implemented`.
 *
 * @param cwd     Project root (parent of the docs/ directory)
 * @param changeName  OpenSpec change name to match against
 * @returns IDs of nodes transitioned to implemented
 */
export function transitionDesignNodesOnArchive(cwd: string, changeName: string): string[] {
	const transitioned: string[] = [];

	for (const node of resolveBoundDesignNodes(cwd, changeName)) {
		const transitionable = node.status === "implementing" || node.status === "decided";
		if (!transitionable) continue;
		const sections = getNodeSections(node);
		writeNodeDocument({ ...node, status: "implemented" }, sections);
		transitioned.push(node.id);
	}
	return transitioned;
}

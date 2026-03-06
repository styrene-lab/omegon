/**
 * design-tree/types — Shared type definitions for the design tree extension.
 */

// ─── Node Status ─────────────────────────────────────────────────────────────

export type NodeStatus = "seed" | "exploring" | "decided" | "blocked" | "deferred";

export const VALID_STATUSES: NodeStatus[] = ["seed", "exploring", "decided", "blocked", "deferred"];

export const STATUS_ICONS: Record<NodeStatus, string> = {
	seed: "◌",
	exploring: "◐",
	decided: "●",
	blocked: "✕",
	deferred: "◑",
};

export const STATUS_COLORS: Record<NodeStatus, string> = {
	seed: "muted",
	exploring: "accent",
	decided: "success",
	blocked: "error",
	deferred: "warning",
};

// ─── Structured Sections ─────────────────────────────────────────────────────

/** A decision recorded in the ## Decisions section */
export interface DesignDecision {
	title: string;
	status: "exploring" | "decided" | "rejected";
	rationale: string;
}

/** A research entry recorded in the ## Research section */
export interface ResearchEntry {
	heading: string;
	content: string;
}

/** File scope entry from ## Implementation Notes */
export interface FileScope {
	path: string;
	description: string;
	action?: "new" | "modified" | "deleted";
}

/** Parsed structured sections from the document body */
export interface DocumentSections {
	overview: string;
	research: ResearchEntry[];
	decisions: DesignDecision[];
	openQuestions: string[];
	implementationNotes: {
		fileScope: FileScope[];
		constraints: string[];
		rawContent: string;
	};
	/** Any content not in a recognized section */
	extraSections: Array<{ heading: string; content: string }>;
}

// ─── Design Node ─────────────────────────────────────────────────────────────

export interface DesignNode {
	id: string;
	title: string;
	status: NodeStatus;
	parent?: string;
	dependencies: string[];
	related: string[];
	tags: string[];
	/** Open questions — synced from ## Open Questions body section */
	open_questions: string[];
	filePath: string;
	lastModified: number;
}

export interface DesignTree {
	nodes: Map<string, DesignNode>;
	docsDir: string;
}

// ─── Document Template ───────────────────────────────────────────────────────

export const SECTION_HEADINGS = {
	overview: "## Overview",
	research: "## Research",
	decisions: "## Decisions",
	openQuestions: "## Open Questions",
	implementationNotes: "## Implementation Notes",
} as const;

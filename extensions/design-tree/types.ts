/**
 * design-tree/types — Shared type definitions for the design tree extension.
 */

// ─── Node Status ─────────────────────────────────────────────────────────────

export type NodeStatus = "seed" | "exploring" | "resolved" | "decided" | "implementing" | "implemented" | "blocked" | "deferred";

export const VALID_STATUSES: NodeStatus[] = ["seed", "exploring", "resolved", "decided", "implementing", "implemented", "blocked", "deferred"];

export const STATUS_ICONS: Record<NodeStatus, string> = {
	seed: "◌",
	exploring: "◐",
	resolved: "◉",
	decided: "●",
	implementing: "⚙",
	implemented: "✓",
	blocked: "✕",
	deferred: "◑",
};

export const STATUS_COLORS: Record<NodeStatus, string> = {
	seed: "muted",
	exploring: "accent",
	resolved: "success",
	decided: "success",
	implementing: "accent",
	implemented: "success",
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

// ─── Acceptance Criteria ─────────────────────────────────────────────────────

/** A Given/When/Then scenario from ## Acceptance Criteria → ### Scenarios */
export interface AcceptanceCriteriaScenario {
	title: string;
	given: string;
	when: string;
	then: string;
}

/** A falsifiability condition from ## Acceptance Criteria → ### Falsifiability */
export interface AcceptanceCriteriaFalsifiability {
	condition: string;
}

/** A checkbox constraint from ## Acceptance Criteria → ### Constraints */
export interface AcceptanceCriteriaConstraint {
	text: string;
	checked: boolean;
}

/** Parsed ## Acceptance Criteria section */
export interface AcceptanceCriteria {
	scenarios: AcceptanceCriteriaScenario[];
	falsifiability: AcceptanceCriteriaFalsifiability[];
	constraints: AcceptanceCriteriaConstraint[];
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
	acceptanceCriteria: AcceptanceCriteria;
	/** Any content not in a recognized section */
	extraSections: Array<{ heading: string; content: string }>;
}

// ─── Issue Type ──────────────────────────────────────────────────────────────

export type IssueType = "epic" | "feature" | "task" | "bug" | "chore";

export const VALID_ISSUE_TYPES: IssueType[] = ["epic", "feature", "task", "bug", "chore"];

export const ISSUE_TYPE_ICONS: Record<IssueType, string> = {
	epic: "⬡",
	feature: "★",
	task: "◻",
	bug: "✖",
	chore: "⟳",
};

// ─── Priority ────────────────────────────────────────────────────────────────

export type Priority = 1 | 2 | 3 | 4 | 5;

export const PRIORITY_LABELS: Record<Priority, string> = {
	1: "critical",
	2: "high",
	3: "medium",
	4: "low",
	5: "trivial",
};

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
	/** Explicit branch name override (D1). When set, used instead of feature/<node-id> */
	branch?: string;
	/** Git branches associated with this node — history of all branches that carried this work */
	branches: string[];
	/** OpenSpec change name linked to this node */
	openspec_change?: string;
	/** Issue type classification (epic/feature/task/bug/chore) */
	issue_type?: IssueType;
	/** Priority from 1 (critical) to 5 (trivial) */
	priority?: Priority;
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
	acceptanceCriteria: "## Acceptance Criteria",
} as const;

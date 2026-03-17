/**
 * cleave/types — Shared type definitions for the cleave extension.
 */

import { BUILTIN_VOLATILE_ALLOWLIST } from "../lib/git-state.ts";

// ─── Assessment ──────────────────────────────────────────────────────────────

export interface PatternDefinition {
	name: string;
	description: string;
	keywords: string[];
	requiredAny: string[];
	expectedComponents: Record<string, string[]>;
	systemsBase: number;
	modifiersDefault: string[];
	splitStrategy: string[];
}

export interface PatternMatch {
	patternId: string;
	name: string;
	confidence: number;
	keywordsMatched: string[];
	systems: number;
	modifiers: string[];
}

export interface AssessmentResult {
	complexity: number;
	systems: number;
	modifiers: string[];
	method: "fast-path" | "heuristic";
	pattern: string | null;
	confidence: number;
	decision: "execute" | "cleave" | "needs_assessment";
	reasoning: string;
	skipInterrogation: boolean;
}

export interface AssessmentFlags {
	robust: boolean;
}

// ─── Planning ────────────────────────────────────────────────────────────────

/** Model tier for child execution — maps to pi's --model flag */
export type ModelTier = "local" | "retribution" | "victory" | "gloriana";

export interface ChildPlan {
	label: string;
	description: string;
	scope: string[];
	dependsOn: string[];
	/** Spec domains this child owns (from <!-- specs: ... --> annotation). Empty if none. */
	specDomains: string[];
	/** Skill names matched to this child (from <!-- skills: ... --> annotation or auto-matched from scope). Empty if none. */
	skills: string[];
	/** Resolved execution model tier. Set by model resolution before dispatch. */
	executeModel?: ModelTier;
}

export interface SplitPlan {
	children: ChildPlan[];
	rationale: string;
}

// ─── Preflight ───────────────────────────────────────────────────────────────

export const DEFAULT_VOLATILE_ALLOWLIST: readonly string[] = BUILTIN_VOLATILE_ALLOWLIST;

export type PreflightFileClass = "related" | "unrelated" | "unknown" | "volatile";

export type ClassificationConfidence = "high" | "medium" | "low";

export type PreflightAction =
	| "checkpoint"
	| "stash_unrelated"
	| "stash_volatile"
	| "continue_without_cleave"
	| "cancel";

export type PreflightDisposition = "proceed" | "continue_without_cleave" | "cancel";

export interface PreflightClassifiedFile {
	path: string;
	class: PreflightFileClass;
	confidence: ClassificationConfidence;
	reason: string;
	tracked: boolean;
	untracked: boolean;
}

export interface PreparedPreflightCheckpoint {
	message: string;
	paths: string[];
	requiresApproval: true;
}

export interface PreparedPreflightStash {
	label: string;
	paths: string[];
	includeUntracked: boolean;
}

export interface PreflightPlan {
	checkpoint: PreparedPreflightCheckpoint | null;
	stashUnrelated: PreparedPreflightStash | null;
	stashVolatile: PreparedPreflightStash | null;
	safeToProceedAfterVolatileOnly: boolean;
}

export interface CleavePreflightSummary {
	hasOpenSpecContext: boolean;
	isDirty: boolean;
	files: PreflightClassifiedFile[];
	related: PreflightClassifiedFile[];
	unrelated: PreflightClassifiedFile[];
	unknown: PreflightClassifiedFile[];
	volatile: PreflightClassifiedFile[];
	availableActions: PreflightAction[];
	plan: PreflightPlan;
}

export interface CleavePreflightOutcome {
	action: PreflightAction;
	disposition: PreflightDisposition;
	checkpointApproved: boolean;
}

// ─── Execution ───────────────────────────────────────────────────────────────

export type ChildStatus =
	| "pending"
	| "running"
	| "completed"
	| "failed"
	| "needs_decomposition";

export interface ChildState {
	childId: number;
	label: string;
	dependsOn: string[];
	status: ChildStatus;
	branch: string;
	worktreePath?: string;
	startedAt?: string;
	completedAt?: string;
	error?: string;
	/** Duration in seconds */
	durationSec?: number;
	/** "local" | "cloud" — which execution backend was used */
	backend?: "local" | "cloud";
	/** Resolved execution model tier for this child */
	executeModel?: string;
	/** Number of review iterations completed (0 = no review, 1+ = reviewed) */
	reviewIterations?: number;
	/** Review history: verdict + issues per round */
	reviewHistory?: Array<{
		round: number;
		status: string;
		issueCount: number;
		reappeared: string[];
	}>;
	/** Final review decision */
	reviewDecision?: "accepted" | "escalated" | "no_review";
	/** Escalation reason if review loop failed */
	reviewEscalationReason?: string;
}

export type CleavePhase =
	| "assess"
	| "plan"
	| "confirm"
	| "dispatch"
	| "harvest"
	| "reunify"
	| "report"
	| "complete"
	| "failed";

export interface CleaveState {
	runId: string;
	phase: CleavePhase;
	directive: string;
	repoPath: string;
	baseBranch: string;
	assessment: AssessmentResult | null;
	plan: SplitPlan | null;
	children: ChildState[];
	workspacePath: string;
	/** Total wall-clock duration in seconds */
	totalDurationSec: number;
	createdAt: string;
	completedAt?: string;
	error?: string;
}

// ─── Conflicts ───────────────────────────────────────────────────────────────

export type ConflictType =
	| "file_overlap"
	| "decision_contradiction"
	| "interface_mismatch"
	| "assumption_violation";

export type ConflictResolution =
	| "3way_merge"
	| "escalate_to_parent"
	| "adapter_required"
	| "verify_with_parent";

export interface Conflict {
	type: ConflictType;
	description: string;
	involved: number[];
	resolution: ConflictResolution;
}

export interface TaskResult {
	path: string;
	status: "SUCCESS" | "PARTIAL" | "FAILED" | "PENDING" | "NOT_FOUND";
	summary: string | null;
	fileClaims: string[];
	interfacesPublished: string[];
	decisions: string[];
	assumptions: string[];
}

export interface ReunificationResult {
	tasksFound: number;
	rollupStatus: "SUCCESS" | "PARTIAL" | "FAILED" | "PENDING";
	conflicts: Conflict[];
	files: string[];
	interfaces: string[];
	decisions: string[];
	readyToClose: boolean;
}

// ─── RPC Child Communication ─────────────────────────────────────────────────

/**
 * Events received from a child process running in RPC mode.
 *
 * This is a discriminated union covering:
 * - AgentEvent types (agent lifecycle, turn, message, tool execution)
 * - AgentSessionEvent extensions (auto_compaction, auto_retry)
 * - RPC response events (command acknowledgements)
 * - Synthetic pipe_closed event (stdout closed)
 *
 * We define this as our own union rather than importing from pi-mono
 * to avoid pulling in transitive dependencies and to add the synthetic
 * pipe_closed event.
 */
export type RpcChildEvent =
	// Agent lifecycle
	| { type: "agent_start" }
	| { type: "agent_end"; messages: unknown[] }
	// Turn lifecycle
	| { type: "turn_start" }
	| { type: "turn_end"; message: unknown; toolResults: unknown[] }
	// Message lifecycle
	| { type: "message_start"; message?: unknown }
	| { type: "message_update"; message?: unknown; assistantMessageEvent?: unknown }
	| { type: "message_end"; message?: unknown }
	// Tool execution
	| { type: "tool_execution_start"; toolCallId: string; toolName: string; args: unknown }
	| { type: "tool_execution_update"; toolCallId: string; toolName: string; args: unknown; partialResult: unknown }
	| { type: "tool_execution_end"; toolCallId: string; toolName: string; result: unknown; isError: boolean }
	// Session extensions
	| { type: "auto_compaction_start"; reason: "threshold" | "overflow" }
	| { type: "auto_compaction_end"; result?: unknown; aborted: boolean; willRetry: boolean; errorMessage?: string }
	| { type: "auto_retry_start"; attempt: number; maxAttempts: number; delayMs: number; errorMessage: string }
	| { type: "auto_retry_end"; success: boolean; attempt: number; finalError?: string }
	// Extension UI requests (from child extensions calling ui.select/ui.confirm)
	| { type: "extension_ui_request"; requestId: string; extensionId: string; method: string; params: unknown }
	// RPC command response
	| { type: "response"; id?: string; command: string; success: boolean; data?: unknown; error?: string }
	// Synthetic: stdout pipe closed (graceful degradation)
	| { type: "pipe_closed" };

/**
 * Structured progress update derived from an RPC child event.
 * Used by the dashboard to display child status.
 */
export interface RpcProgressUpdate {
	kind: "tool" | "lifecycle" | "error";
	summary: string;
	toolName?: string;
}

// ─── Config ──────────────────────────────────────────────────────────────────

export interface CleaveConfig {
	/** Complexity threshold — above this, the directive gets cleaved */
	threshold: number;
	/** Maximum recursion depth */
	maxDepth: number;
	/** Maximum parallel children */
	maxParallel: number;
	/** Use local model for leaf tasks when possible */
	preferLocal: boolean;
	/** Success criteria for the directive */
	successCriteria: string[];
}

export const DEFAULT_CONFIG: CleaveConfig = {
	threshold: 2.0,
	maxDepth: 3,
	maxParallel: 4,
	preferLocal: true,
	successCriteria: [],
};

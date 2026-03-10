/**
 * openspec/spec — Pure domain logic for OpenSpec operations.
 *
 * No pi dependency — can be tested standalone. Handles:
 * - Spec file parsing and generation
 * - Change directory management
 * - Lifecycle stage computation
 * - Spec scaffolding from proposals and design nodes
 * - Archive operations
 * - Per-change assessment artifact persistence and freshness checks
 */

import { execFileSync } from "node:child_process";
import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as path from "node:path";
import type {
	ChangeInfo,
	ChangeStage,
	Requirement,
	Scenario,
	SpecFile,
	SpecSection,
} from "./types.ts";

// ─── Constants ───────────────────────────────────────────────────────────────

const OPENSPEC_DIR = "openspec";
const CHANGES_DIR = "changes";
const ARCHIVE_DIR = "archive";
const BASELINE_DIR = "baseline";
const ASSESSMENT_FILE = "assessment.json";
const ASSESSMENT_SCHEMA_VERSION = 1;

export type AssessmentKind = "spec" | "cleave" | "diff";
export type AssessmentOutcome = "pass" | "reopen" | "ambiguous";

export interface AssessmentSnapshotFile {
	path: string;
	exists: boolean;
	size: number;
	sha256: string | null;
}

export interface AssessmentSnapshot {
	gitHead: string | null;
	fingerprint: string;
	dirty: boolean;
	scopedPaths: string[];
	files: AssessmentSnapshotFile[];
}

export interface AssessmentReconciliationHints {
	reopen: boolean;
	changedFiles: string[];
	constraints: string[];
	recommendedAction: string | null;
}

export interface AssessmentRecord {
	schemaVersion: 1;
	changeName: string;
	assessmentKind: AssessmentKind;
	outcome: AssessmentOutcome;
	timestamp: string;
	summary?: string;
	snapshot: AssessmentSnapshot;
	reconciliation: AssessmentReconciliationHints;
}

export interface AssessmentFreshness {
	current: boolean;
	reasons: string[];
}

export type VerificationSubstate =
	| "missing-assessment"
	| "stale-assessment"
	| "reopened-work"
	| "missing-binding"
	| "archive-ready"
	| "awaiting-reconciliation";

export interface VerificationStatus {
	coarseStage: ChangeStage;
	substate: VerificationSubstate | null;
	nextAction: string | null;
	reason: string | null;
}

// ─── Validation ──────────────────────────────────────────────────────────────

/** Validate a change name — prevent path traversal */
export function validateChangeName(name: string): string | null {
	if (!name) return "Change name cannot be empty";
	if (name.length > 80) return "Change name too long (max 80 characters)";
	if (name.includes("/") || name.includes("\\")) return "Change name cannot contain path separators";
	if (name.includes("..")) return "Change name cannot contain '..'";
	if (name.startsWith(".")) return "Change name cannot start with '.'";
	if (!/^[a-z0-9][a-z0-9_-]*$/.test(name)) return "Change name must be lowercase alphanumeric with hyphens/underscores";
	return null;
}

/** Validate a spec domain path — allow forward slashes for nesting but prevent traversal */
export function validateDomain(domain: string): string | null {
	if (!domain) return "Domain cannot be empty";
	if (domain.length > 120) return "Domain too long (max 120 characters)";
	if (domain.includes("\\")) return "Domain cannot contain backslashes";
	if (domain.includes("..")) return "Domain cannot contain '..'";
	if (domain.startsWith("/") || domain.startsWith(".")) return "Domain cannot start with '/' or '.'";
	if (!/^[a-z0-9][a-z0-9_/-]*$/.test(domain)) return "Domain must be lowercase alphanumeric with hyphens, underscores, and forward slashes";
	return null;
}

// ─── Change Discovery ────────────────────────────────────────────────────────

/**
 * Get the openspec directory path for a repo, or null if it doesn't exist.
 */
export function getOpenSpecDir(repoPath: string): string | null {
	const dir = path.join(repoPath, OPENSPEC_DIR);
	return fs.existsSync(dir) ? dir : null;
}

/**
 * Ensure the openspec directory structure exists.
 */
export function ensureOpenSpecDir(repoPath: string): string {
	const dir = path.join(repoPath, OPENSPEC_DIR, CHANGES_DIR);
	fs.mkdirSync(dir, { recursive: true });
	return path.join(repoPath, OPENSPEC_DIR);
}

/**
 * List all active (non-archived) changes with full status.
 */
export function listChanges(repoPath: string): ChangeInfo[] {
	const openspecDir = getOpenSpecDir(repoPath);
	if (!openspecDir) return [];

	const changesDir = path.join(openspecDir, CHANGES_DIR);
	if (!fs.existsSync(changesDir)) return [];

	const entries = fs.readdirSync(changesDir, { withFileTypes: true });
	const changes: ChangeInfo[] = [];

	for (const entry of entries) {
		if (!entry.isDirectory() || entry.name === "archive") continue;
		const changePath = path.join(changesDir, entry.name);
		changes.push(getChangeInfo(entry.name, changePath));
	}

	return changes;
}

/**
 * Get a specific change by name.
 */
export function getChange(repoPath: string, name: string): ChangeInfo | null {
	const nameError = validateChangeName(name);
	if (nameError) return null;

	const openspecDir = getOpenSpecDir(repoPath);
	if (!openspecDir) return null;

	const changePath = path.join(openspecDir, CHANGES_DIR, name);
	if (!fs.existsSync(changePath)) return null;

	return getChangeInfo(name, changePath);
}

/**
 * Build full change info including lifecycle stage computation.
 */
function getChangeInfo(name: string, changePath: string): ChangeInfo {
	const hasProposal = fs.existsSync(path.join(changePath, "proposal.md"));
	const hasDesign = fs.existsSync(path.join(changePath, "design.md"));
	const hasTasks = fs.existsSync(path.join(changePath, "tasks.md"));
	const specsDir = path.join(changePath, "specs");
	const hasSpecs = fs.existsSync(specsDir);

	let totalTasks = 0;
	let doneTasks = 0;
	if (hasTasks) {
		const content = fs.readFileSync(path.join(changePath, "tasks.md"), "utf-8");
		const checkboxes = content.match(/^\s*-\s+\[[ xX]\]/gm) || [];
		totalTasks = checkboxes.length;
		doneTasks = (content.match(/^\s*-\s+\[[xX]\]/gm) || []).length;
	}

	const specs = hasSpecs ? parseSpecsDir(specsDir) : [];
	const stage = computeStage(hasProposal, hasSpecs, hasTasks, totalTasks, doneTasks);

	return {
		name,
		path: changePath,
		stage,
		hasProposal,
		hasDesign,
		hasSpecs,
		hasTasks,
		totalTasks,
		doneTasks,
		specs,
	};
}

/**
 * Compute the lifecycle stage from artifact presence and task progress.
 */
export function computeStage(
	hasProposal: boolean,
	hasSpecs: boolean,
	hasTasks: boolean,
	totalTasks: number,
	doneTasks: number,
): ChangeStage {
	if (!hasProposal && !hasTasks && !hasSpecs) return "proposed";
	if (hasTasks && totalTasks > 0 && doneTasks >= totalTasks) return "verifying";
	if (hasTasks && totalTasks > 0 && doneTasks > 0) return "implementing";
	if (hasTasks) return "planned";
	if (hasSpecs) return "specified";
	return "proposed";
}

// ─── Assessment Artifact Helpers ────────────────────────────────────────────

function getChangePath(repoPath: string, changeName: string): string | null {
	const nameError = validateChangeName(changeName);
	if (nameError) return null;

	const openspecDir = getOpenSpecDir(repoPath);
	if (!openspecDir) return null;

	const changePath = path.join(openspecDir, CHANGES_DIR, changeName);
	return fs.existsSync(changePath) ? changePath : null;
}

export function getAssessmentArtifactPath(changePath: string): string {
	return path.join(changePath, ASSESSMENT_FILE);
}

function isAssessmentKind(value: unknown): value is AssessmentKind {
	return value === "spec" || value === "cleave" || value === "diff";
}

function isAssessmentOutcome(value: unknown): value is AssessmentOutcome {
	return value === "pass" || value === "reopen" || value === "ambiguous";
}

function parseStringArray(value: unknown): string[] {
	if (!Array.isArray(value)) return [];
	return value.filter((entry): entry is string => typeof entry === "string").map((entry) => entry.trim()).filter(Boolean);
}

function normalizeSnapshot(input: unknown): AssessmentSnapshot | null {
	if (!input || typeof input !== "object") return null;
	const candidate = input as Record<string, unknown>;
	if (typeof candidate.fingerprint !== "string" || !candidate.fingerprint) return null;

	const filesRaw = Array.isArray(candidate.files) ? candidate.files : [];
	const files: AssessmentSnapshotFile[] = filesRaw.flatMap((entry) => {
		if (!entry || typeof entry !== "object") return [];
		const file = entry as Record<string, unknown>;
		if (typeof file.path !== "string") return [];
		if (typeof file.exists !== "boolean") return [];
		if (typeof file.size !== "number") return [];
		if (file.sha256 !== null && typeof file.sha256 !== "string") return [];
		return [{
			path: file.path,
			exists: file.exists,
			size: file.size,
			sha256: file.sha256,
		}];
	});

	return {
		gitHead: typeof candidate.gitHead === "string" ? candidate.gitHead : null,
		fingerprint: candidate.fingerprint,
		dirty: candidate.dirty === true,
		scopedPaths: parseStringArray(candidate.scopedPaths),
		files,
	};
}

function normalizeReconciliation(input: unknown, outcome: AssessmentOutcome): AssessmentReconciliationHints {
	if (!input || typeof input !== "object") {
		return {
			reopen: outcome === "reopen",
			changedFiles: [],
			constraints: [],
			recommendedAction: outcome === "reopen" ? "Run openspec_manage reconcile_after_assess before archive." : null,
		};
	}

	const candidate = input as Record<string, unknown>;
	const changedFiles = parseStringArray(candidate.changedFiles);
	const constraints = parseStringArray(candidate.constraints);
	const recommendedAction = typeof candidate.recommendedAction === "string" && candidate.recommendedAction.trim()
		? candidate.recommendedAction.trim()
		: outcome === "reopen"
			? "Run openspec_manage reconcile_after_assess before archive."
			: null;

	return {
		reopen: typeof candidate.reopen === "boolean" ? candidate.reopen : outcome === "reopen",
		changedFiles,
		constraints,
		recommendedAction,
	};
}

function normalizeAssessmentRecord(input: unknown): AssessmentRecord | null {
	if (!input || typeof input !== "object") return null;
	const candidate = input as Record<string, unknown>;
	if (candidate.schemaVersion !== ASSESSMENT_SCHEMA_VERSION) return null;
	if (typeof candidate.changeName !== "string" || !candidate.changeName) return null;
	if (!isAssessmentKind(candidate.assessmentKind)) return null;
	if (!isAssessmentOutcome(candidate.outcome)) return null;
	if (typeof candidate.timestamp !== "string" || !candidate.timestamp) return null;

	const snapshot = normalizeSnapshot(candidate.snapshot);
	if (!snapshot) return null;

	return {
		schemaVersion: ASSESSMENT_SCHEMA_VERSION,
		changeName: candidate.changeName,
		assessmentKind: candidate.assessmentKind,
		outcome: candidate.outcome,
		timestamp: candidate.timestamp,
		...(typeof candidate.summary === "string" && candidate.summary.trim() ? { summary: candidate.summary.trim() } : {}),
		snapshot,
		reconciliation: normalizeReconciliation(candidate.reconciliation, candidate.outcome),
	};
}

function safeReadGit(repoPath: string, args: readonly string[]): string | null {
	try {
		return execFileSync("git", args, {
			cwd: repoPath,
			encoding: "utf-8",
			stdio: ["ignore", "pipe", "ignore"],
		}).trim() || null;
	} catch {
		return null;
	}
}

function detectGitDirty(repoPath: string, scopedPaths: readonly string[]): boolean {
	if (scopedPaths.length === 0) return false;
	const output = safeReadGit(repoPath, ["status", "--short", "--", ...scopedPaths]);
	return output !== null && output.length > 0;
}

function parseDesignFileScope(changePath: string): string[] {
	const designPath = path.join(changePath, "design.md");
	if (!fs.existsSync(designPath)) return [];

	const content = fs.readFileSync(designPath, "utf-8");
	const fileChangesSection = content.match(/##\s+File Changes\s*\n([\s\S]*?)(?=\n##\s|$)/i);
	if (!fileChangesSection) return [];

	const scoped = new Set<string>();
	for (const line of fileChangesSection[1].split("\n")) {
		const match = line.match(/-\s+`([^`]+)`/);
		if (match && match[1].trim()) scoped.add(match[1].trim());
	}
	return Array.from(scoped).sort();
}

function listChangeArtifactPaths(changePath: string): string[] {
	const collected: string[] = [];
	const stack = [changePath];

	while (stack.length > 0) {
		const dir = stack.pop();
		if (!dir) continue;
		const entries = fs.readdirSync(dir, { withFileTypes: true });
		for (const entry of entries) {
			const fullPath = path.join(dir, entry.name);
			if (entry.isDirectory()) {
				stack.push(fullPath);
				continue;
			}
			collected.push(fullPath);
		}
	}

	return collected.sort();
}

function buildSnapshotFile(repoPath: string, relativeFilePath: string): AssessmentSnapshotFile {
	const absolutePath = path.join(repoPath, relativeFilePath);
	if (!fs.existsSync(absolutePath) || !fs.statSync(absolutePath).isFile()) {
		return {
			path: relativeFilePath,
			exists: false,
			size: 0,
			sha256: null,
		};
	}

	const content = fs.readFileSync(absolutePath);
	return {
		path: relativeFilePath,
		exists: true,
		size: content.length,
		sha256: crypto.createHash("sha256").update(content).digest("hex"),
	};
}

export function computeAssessmentSnapshot(repoPath: string, changeName: string): AssessmentSnapshot | null {
	const changePath = getChangePath(repoPath, changeName);
	if (!changePath) return null;

	const scopedPaths = parseDesignFileScope(changePath);
	const artifactRelativePaths = listChangeArtifactPaths(changePath)
		.map((filePath) => path.relative(repoPath, filePath))
		.filter((filePath) => filePath !== path.join(OPENSPEC_DIR, CHANGES_DIR, changeName, ASSESSMENT_FILE));

	const snapshotPaths = Array.from(new Set([
		...scopedPaths,
		...artifactRelativePaths,
	])).sort();
	const files = snapshotPaths.map((filePath) => buildSnapshotFile(repoPath, filePath));
	const gitHead = safeReadGit(repoPath, ["rev-parse", "HEAD"]);
	const dirty = detectGitDirty(repoPath, snapshotPaths);

	const fingerprintSeed = JSON.stringify({
		changeName,
		gitHead,
		dirty,
		files,
	});

	return {
		gitHead,
		fingerprint: crypto.createHash("sha256").update(fingerprintSeed).digest("hex"),
		dirty,
		scopedPaths,
		files,
	};
}

export function readAssessmentRecord(repoPath: string, changeName: string): AssessmentRecord | null {
	const changePath = getChangePath(repoPath, changeName);
	if (!changePath) return null;

	const artifactPath = getAssessmentArtifactPath(changePath);
	if (!fs.existsSync(artifactPath)) return null;

	try {
		const parsed = JSON.parse(fs.readFileSync(artifactPath, "utf-8")) as unknown;
		const record = normalizeAssessmentRecord(parsed);
		if (!record || record.changeName !== changeName) return null;
		return record;
	} catch {
		return null;
	}
}

export function writeAssessmentRecord(
	repoPath: string,
	changeName: string,
	record: Omit<AssessmentRecord, "schemaVersion">,
): string {
	const changePath = getChangePath(repoPath, changeName);
	if (!changePath) {
		throw new Error(`OpenSpec change '${changeName}' not found`);
	}
	if (record.changeName !== changeName) {
		throw new Error(`Assessment record change '${record.changeName}' does not match requested change '${changeName}'`);
	}

	const normalized: AssessmentRecord = {
		schemaVersion: ASSESSMENT_SCHEMA_VERSION,
		changeName,
		assessmentKind: record.assessmentKind,
		outcome: record.outcome,
		timestamp: record.timestamp,
		...(record.summary ? { summary: record.summary } : {}),
		snapshot: record.snapshot,
		reconciliation: normalizeReconciliation(record.reconciliation, record.outcome),
	};

	const artifactPath = getAssessmentArtifactPath(changePath);
	fs.writeFileSync(artifactPath, JSON.stringify(normalized, null, 2) + "\n", "utf-8");
	return artifactPath;
}

export function evaluateAssessmentFreshness(
	record: AssessmentRecord | null,
	currentSnapshot: AssessmentSnapshot | null,
): AssessmentFreshness {
	if (!record) {
		return { current: false, reasons: ["Missing assessment record"] };
	}
	if (!currentSnapshot) {
		return { current: false, reasons: ["Current implementation snapshot unavailable"] };
	}

	const reasons: string[] = [];
	if (record.snapshot.fingerprint !== currentSnapshot.fingerprint) {
		reasons.push("Implementation snapshot fingerprint differs from the persisted assessment record");
	}
	if (record.snapshot.gitHead !== currentSnapshot.gitHead) {
		reasons.push("Git HEAD differs from the persisted assessment record");
	}
	if (record.snapshot.dirty !== currentSnapshot.dirty) {
		reasons.push("Working tree cleanliness differs from the persisted assessment record");
	}
	if (record.outcome !== "pass") {
		reasons.push(`Assessment outcome is '${record.outcome}', not 'pass'`);
	}
	if (record.reconciliation.reopen) {
		reasons.push("Assessment record indicates lifecycle reconciliation is still open");
	}

	return {
		current: reasons.length === 0,
		reasons,
	};
}

export function getAssessmentStatus(repoPath: string, changeName: string): {
	record: AssessmentRecord | null;
	snapshot: AssessmentSnapshot | null;
	freshness: AssessmentFreshness;
} {
	const record = readAssessmentRecord(repoPath, changeName);
	const snapshot = computeAssessmentSnapshot(repoPath, changeName);
	return {
		record,
		snapshot,
		freshness: evaluateAssessmentFreshness(record, snapshot),
	};
}

export function resolveVerificationStatus(input: {
	stage: ChangeStage;
	record: AssessmentRecord | null;
	freshness: AssessmentFreshness;
	archiveBlocked?: boolean;
	archiveBlockedReason?: string | null;
	archiveBlockedIssueCodes?: readonly string[];
	changeName: string;
}): VerificationStatus {
	if (input.stage !== "verifying") {
		return {
			coarseStage: input.stage,
			substate: null,
			nextAction: null,
			reason: null,
		};
	}

	if (!input.record) {
		return {
			coarseStage: input.stage,
			substate: "missing-assessment",
			nextAction: `/assess spec ${input.changeName}`,
			reason: "No persisted assessment record exists for this task-complete change.",
		};
	}

	if (input.record.outcome === "reopen" || input.record.reconciliation.reopen) {
		return {
			coarseStage: input.stage,
			substate: "reopened-work",
			nextAction: `Complete follow-up work for ${input.changeName}, reconcile lifecycle artifacts, then re-run /assess spec ${input.changeName}`,
			reason: "The latest persisted assessment reopened work.",
		};
	}

	if (!input.freshness.current || input.record.outcome === "ambiguous") {
		return {
			coarseStage: input.stage,
			substate: "stale-assessment",
			nextAction: `Refresh /assess spec ${input.changeName} for the current implementation snapshot`,
			reason: input.record.outcome === "ambiguous"
				? "The latest persisted assessment is ambiguous and must be refreshed before archive."
				: input.freshness.reasons.join(" "),
		};
	}

	if (input.archiveBlockedIssueCodes?.includes("missing_design_binding")) {
		return {
			coarseStage: input.stage,
			substate: "missing-binding",
			nextAction: input.archiveBlockedReason ?? `Bind ${input.changeName} to a design-tree node before archive`,
			reason: input.archiveBlockedReason ?? "No valid design-tree binding can be established for this change.",
		};
	}

	if (input.archiveBlocked) {
		return {
			coarseStage: input.stage,
			substate: "awaiting-reconciliation",
			nextAction: input.archiveBlockedReason ?? `Reconcile lifecycle artifacts for ${input.changeName} before archive`,
			reason: input.archiveBlockedReason ?? "Lifecycle reconciliation is still blocking archive.",
		};
	}

	return {
		coarseStage: input.stage,
		substate: "archive-ready",
		nextAction: `/opsx:archive ${input.changeName}`,
		reason: "Assessment passed for the current snapshot and archive gates are clear.",
	};
}

// ─── Spec Parsing ────────────────────────────────────────────────────────────

/**
 * Parse all spec files in a specs/ directory.
 */
export function parseSpecsDir(specsDir: string): SpecFile[] {
	if (!fs.existsSync(specsDir)) return [];

	const files = findSpecFiles(specsDir);
	return files.map((filePath) => {
		const content = fs.readFileSync(filePath, "utf-8");
		const domain = filePath
			.replace(specsDir + "/", "")
			.replace(/\/spec\.md$/, "")
			.replace(/\.md$/, "");

		return {
			domain,
			filePath,
			sections: parseSpecContent(content),
		};
	});
}

/**
 * Recursively find spec.md files.
 */
function findSpecFiles(dir: string): string[] {
	const results: string[] = [];
	const entries = fs.readdirSync(dir, { withFileTypes: true });

	for (const entry of entries) {
		const fullPath = path.join(dir, entry.name);
		if (entry.isDirectory()) {
			results.push(...findSpecFiles(fullPath));
		} else if (entry.name.endsWith(".md")) {
			results.push(fullPath);
		}
	}

	return results.sort();
}

/**
 * Parse a spec file's content into sections and requirements.
 */
export function parseSpecContent(content: string): SpecSection[] {
	const sections: SpecSection[] = [];

	// Split on ## ADDED/MODIFIED/REMOVED headings
	const sectionRe = /^##\s+(ADDED|MODIFIED|REMOVED)\s+Requirements?\s*$/gim;
	const parts: Array<{ type: SpecSection["type"]; startIndex: number }> = [];

	let match: RegExpExecArray | null;
	while ((match = sectionRe.exec(content)) !== null) {
		parts.push({
			type: match[1].toLowerCase() as SpecSection["type"],
			startIndex: match.index + match[0].length,
		});
	}

	for (let i = 0; i < parts.length; i++) {
		const start = parts[i].startIndex;
		const end = i + 1 < parts.length ? parts[i + 1].startIndex - parts[i + 1].type.length - 20 : content.length;
		const sectionContent = content.slice(start, end).trim();
		const requirements = parseRequirements(sectionContent);

		sections.push({
			type: parts[i].type,
			requirements,
		});
	}

	return sections;
}

/**
 * Parse requirements from a section's content.
 */
function parseRequirements(content: string): Requirement[] {
	const requirements: Requirement[] = [];
	const reqRe = /^###\s+Requirement:\s*(.+)$/gm;
	const reqPositions: Array<{ title: string; startIndex: number }> = [];

	let match: RegExpExecArray | null;
	while ((match = reqRe.exec(content)) !== null) {
		reqPositions.push({
			title: match[1].trim(),
			startIndex: match.index + match[0].length,
		});
	}

	for (let i = 0; i < reqPositions.length; i++) {
		const start = reqPositions[i].startIndex;
		const end = i + 1 < reqPositions.length
			? content.lastIndexOf("###", reqPositions[i + 1].startIndex)
			: content.length;
		const reqContent = content.slice(start, end).trim();

		// Extract description (text before first #### Scenario)
		const firstScenario = reqContent.indexOf("#### Scenario:");
		const description = firstScenario >= 0
			? reqContent.slice(0, firstScenario).trim()
			: reqContent.trim();

		const scenarios = parseScenarios(reqContent);

		requirements.push({
			title: reqPositions[i].title,
			description,
			scenarios,
		});
	}

	return requirements;
}

/**
 * Parse Given/When/Then scenarios from requirement content.
 */
export function parseScenarios(content: string): Scenario[] {
	const scenarios: Scenario[] = [];
	const scenarioRe = /####\s+Scenario:\s*(.+)/g;
	const positions: Array<{ title: string; startIndex: number }> = [];

	let match: RegExpExecArray | null;
	while ((match = scenarioRe.exec(content)) !== null) {
		positions.push({
			title: match[1].trim(),
			startIndex: match.index + match[0].length,
		});
	}

	for (let i = 0; i < positions.length; i++) {
		const start = positions[i].startIndex;
		const end = i + 1 < positions.length
			? content.lastIndexOf("####", positions[i + 1].startIndex)
			: content.length;
		const block = content.slice(start, end).trim();

		const given = extractClause(block, "Given");
		const when = extractClause(block, "When");
		const then = extractClause(block, "Then");
		const andClauses = extractAndClauses(block);

		if (given || when || then) {
			scenarios.push({
				title: positions[i].title,
				given: given || "",
				when: when || "",
				then: then || "",
				...(andClauses.length > 0 && { and: andClauses }),
			});
		}
	}

	return scenarios;
}

/**
 * Extract a Given/When/Then clause from a scenario block.
 */
function extractClause(block: string, keyword: string): string | null {
	// Match "Given ..." up to next keyword or end
	const re = new RegExp(
		`^${keyword}\\s+(.+?)(?=\\n(?:Given|When|Then|And)\\s|$)`,
		"ms",
	);
	const match = block.match(re);
	return match ? match[1].trim() : null;
}

/**
 * Extract "And ..." clauses from a scenario block.
 */
function extractAndClauses(block: string): string[] {
	const clauses: string[] = [];
	const re = /^And\s+(.+)$/gm;
	let match: RegExpExecArray | null;
	while ((match = re.exec(block)) !== null) {
		clauses.push(match[1].trim());
	}
	return clauses;
}

// ─── Spec Generation ─────────────────────────────────────────────────────────

/**
 * Generate a spec file from a proposal and optional design decisions.
 *
 * This creates the ADDED Requirements section with placeholder scenarios
 * derived from the proposal's intent and any design decisions.
 */
export function generateSpecFromProposal(opts: {
	domain: string;
	proposalContent: string;
	decisions?: Array<{ title: string; rationale: string }>;
	openQuestions?: string[];
}): string {
	const lines: string[] = [
		`# ${opts.domain} — Delta Spec`,
		"",
		"## ADDED Requirements",
		"",
	];

	// Extract intent from proposal
	const intentMatch = opts.proposalContent.match(
		/##\s+Intent\s*\n([\s\S]*?)(?=\n##\s|$)/i,
	);
	const intent = intentMatch ? intentMatch[1].trim() : "Implement the proposed change.";

	// Generate a requirement from intent
	lines.push(`### Requirement: ${opts.domain} core functionality`, "");
	lines.push(intent, "");

	lines.push(`#### Scenario: Happy path`, "");
	lines.push("Given the system is in a default state");
	lines.push(`When the ${opts.domain} feature is exercised`);
	lines.push("Then the expected behavior is observed");
	lines.push("");

	// Generate requirements from decisions
	if (opts.decisions && opts.decisions.length > 0) {
		for (const d of opts.decisions) {
			lines.push(`### Requirement: ${d.title}`, "");
			lines.push(d.rationale, "");

			lines.push(`#### Scenario: ${d.title} — default case`, "");
			lines.push("Given the system uses the decided approach");
			lines.push(`When ${d.title.toLowerCase()} is applied`);
			lines.push("Then the system behaves according to the decision");
			lines.push("");
		}
	}

	// Convert open questions to placeholder requirements
	if (opts.openQuestions && opts.openQuestions.length > 0) {
		lines.push("## MODIFIED Requirements", "");
		lines.push(
			"<!-- Open questions from design exploration — refine these into concrete scenarios -->",
			"",
		);
		for (const q of opts.openQuestions) {
			lines.push(`### Requirement: ${q.replace(/\?$/, "")}`, "");
			lines.push(`<!-- TODO: Refine from open question: "${q}" -->`, "");
			lines.push(`#### Scenario: ${q.replace(/\?$/, "")} — resolved`, "");
			lines.push("Given the question has been resolved");
			lines.push("When the resolution is applied");
			lines.push("Then the system reflects the answer");
			lines.push("");
		}
	}

	return lines.join("\n");
}

/**
 * Generate a scenario block as markdown.
 */
export function formatScenario(s: Scenario): string {
	const lines = [
		`#### Scenario: ${s.title}`,
		`Given ${s.given}`,
		`When ${s.when}`,
		`Then ${s.then}`,
	];
	if (s.and) {
		for (const clause of s.and) {
			lines.push(`And ${clause}`);
		}
	}
	return lines.join("\n");
}

/**
 * Generate a complete spec file from structured data.
 */
export function generateSpecFile(domain: string, sections: SpecSection[]): string {
	const lines = [`# ${domain} — Delta Spec`, ""];

	for (const section of sections) {
		const typeLabel = section.type.charAt(0).toUpperCase() + section.type.slice(1);
		lines.push(`## ${typeLabel.toUpperCase()} Requirements`, "");

		for (const req of section.requirements) {
			lines.push(`### Requirement: ${req.title}`, "");
			if (req.description) {
				lines.push(req.description, "");
			}
			for (const s of req.scenarios) {
				lines.push(formatScenario(s), "");
			}
		}
	}

	return lines.join("\n");
}

// ─── Change Operations ───────────────────────────────────────────────────────

/**
 * Create a new OpenSpec change with a proposal.
 */
export function createChange(
	repoPath: string,
	name: string,
	title: string,
	intent: string,
): { changePath: string; files: string[] } {
	const slug = name
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "")
		.slice(0, 60);

	const openspecDir = ensureOpenSpecDir(repoPath);
	const changePath = path.join(openspecDir, CHANGES_DIR, slug);

	if (fs.existsSync(changePath)) {
		const existing = fs.readdirSync(changePath).filter((f) => f.endsWith(".md"));
		if (existing.length > 0) {
			throw new Error(
				`Change '${slug}' already exists with files: ${existing.join(", ")}. ` +
				`Delete it first: rm -rf ${changePath}`,
			);
		}
	}

	fs.mkdirSync(changePath, { recursive: true });

	const proposalLines = [
		`# ${title}`,
		"",
		"## Intent",
		"",
		intent,
		"",
		"## Scope",
		"",
		"<!-- Define what is in scope and out of scope -->",
		"",
		"## Success Criteria",
		"",
		"<!-- How will we know this change is complete and correct? -->",
		"",
	];

	const proposalPath = path.join(changePath, "proposal.md");
	fs.writeFileSync(proposalPath, proposalLines.join("\n"));

	return { changePath, files: ["proposal.md"] };
}

/**
 * Add specs to an existing change.
 * Creates specs/<domain>.md with the provided content.
 */
export function addSpec(
	changePath: string,
	domain: string,
	content: string,
): string {
	// Validate domain to prevent path traversal
	const domainError = validateDomain(domain);
	if (domainError) throw new Error(domainError);

	const specsDir = path.join(changePath, "specs");
	fs.mkdirSync(specsDir, { recursive: true });

	const specPath = path.join(specsDir, domain + ".md");

	// Defense-in-depth: verify resolved path is within specs directory
	const resolved = path.resolve(specPath);
	const resolvedBase = path.resolve(specsDir);
	if (!resolved.startsWith(resolvedBase + path.sep) && resolved !== resolvedBase) {
		throw new Error(`Path traversal detected: domain '${domain}' resolves outside specs/`);
	}

	// Ensure parent dirs for nested domains
	fs.mkdirSync(path.dirname(specPath), { recursive: true });
	fs.writeFileSync(specPath, content);

	return specPath;
}

/**
 * Archive a completed change.
 *
 * Moves specs to baseline/ and the change directory to archive/.
 * Returns the list of operations performed.
 */
export function archiveChange(
	repoPath: string,
	changeName: string,
): { operations: string[]; archived: boolean } {
	const nameError = validateChangeName(changeName);
	if (nameError) return { operations: [nameError], archived: false };

	const openspecDir = getOpenSpecDir(repoPath);
	if (!openspecDir) return { operations: ["No openspec/ directory found"], archived: false };

	const changePath = path.join(openspecDir, CHANGES_DIR, changeName);
	if (!fs.existsSync(changePath)) {
		return { operations: [`Change '${changeName}' not found`], archived: false };
	}

	const operations: string[] = [];

	// 1. Merge specs to baseline
	const specsDir = path.join(changePath, "specs");
	if (fs.existsSync(specsDir)) {
		const baselineDir = path.join(openspecDir, BASELINE_DIR);
		fs.mkdirSync(baselineDir, { recursive: true });

		const specFiles = findSpecFiles(specsDir);
		for (const specFile of specFiles) {
			const relativePath = specFile.replace(specsDir + "/", "");
			const baselinePath = path.join(baselineDir, relativePath);
			fs.mkdirSync(path.dirname(baselinePath), { recursive: true });

			if (fs.existsSync(baselinePath)) {
				// Merge: append ADDED sections to existing baseline
				const existingContent = fs.readFileSync(baselinePath, "utf-8");
				const deltaContent = fs.readFileSync(specFile, "utf-8");
				const merged = mergeSpecToBaseline(existingContent, deltaContent);
				fs.writeFileSync(baselinePath, merged);
				operations.push(`Merged ${relativePath} into baseline`);
			} else {
				// New baseline file — convert delta format to baseline format
				const deltaContent = fs.readFileSync(specFile, "utf-8");
				const baseline = deltaToBaseline(deltaContent);
				fs.writeFileSync(baselinePath, baseline);
				operations.push(`Created baseline/${relativePath}`);
			}
		}
	}

	// 2. Move change to archive
	const archiveDir = path.join(openspecDir, ARCHIVE_DIR);
	fs.mkdirSync(archiveDir, { recursive: true });

	const timestamp = new Date().toISOString().slice(0, 10);
	const archiveName = `${timestamp}-${changeName}`;
	const archivePath = path.join(archiveDir, archiveName);

	fs.renameSync(changePath, archivePath);
	operations.push(`Archived change to ${ARCHIVE_DIR}/${archiveName}`);

	return { operations, archived: true };
}

/**
 * Merge delta spec ADDED requirements into an existing baseline spec.
 */
function mergeSpecToBaseline(existing: string, delta: string): string {
	// Extract ADDED requirements from delta
	const addedMatch = delta.match(
		/##\s+ADDED\s+Requirements?\s*\n([\s\S]*?)(?=\n##\s+(?:ADDED|MODIFIED|REMOVED)|$)/i,
	);
	if (!addedMatch) return existing;

	// Find the end of the existing content (before any trailing whitespace)
	const trimmed = existing.trimEnd();

	// Append the added requirements as regular requirements (no ADDED label)
	const addedContent = addedMatch[1].trim();
	return trimmed + "\n\n" + addedContent + "\n";
}

/**
 * Convert a delta spec to baseline format.
 * Strips ADDED/MODIFIED/REMOVED section headers, keeping just requirements.
 */
function deltaToBaseline(delta: string): string {
	// Get the title
	const titleMatch = delta.match(/^#\s+(.+)/);
	const title = titleMatch ? titleMatch[1].replace(/\s*—\s*Delta Spec$/, "") : "Spec";

	const lines = [`# ${title}`, ""];

	// Extract all requirements regardless of section
	const reqRe = /###\s+Requirement:\s*(.+)/g;
	let match: RegExpExecArray | null;
	const positions: number[] = [];

	while ((match = reqRe.exec(delta)) !== null) {
		positions.push(match.index);
	}

	for (let i = 0; i < positions.length; i++) {
		const start = positions[i];
		const end = i + 1 < positions.length ? positions[i + 1] : delta.length;
		lines.push(delta.slice(start, end).trim(), "");
	}

	return lines.join("\n");
}

// ─── Canonical Lifecycle Resolver ────────────────────────────────────────────

/**
 * Normalized lifecycle summary for an OpenSpec change.
 *
 * This is the single source of truth for a change's lifecycle state.
 * All consumers (status surfaces, archive gates, dashboard, design-tree)
 * should derive their display from this shape rather than recomputing
 * stage/substate/readiness independently.
 */
export interface LifecycleSummary {
	/** Coarse lifecycle stage. Preserves historical stage contract. */
	stage: ChangeStage;

	/**
	 * Fine-grained verification substate. Only non-null when stage === 'verifying'.
	 * Adds precision without changing the coarse stage contract.
	 */
	verificationSubstate: VerificationSubstate | null;

	/** Whether the change is safe to archive. */
	archiveReady: boolean;

	/** Whether a design-tree binding exists for this change. */
	bindingStatus: "bound" | "unbound" | "unknown";

	/** Total number of tasks (0 if no tasks.md). */
	totalTasks: number;

	/** Number of completed tasks. */
	doneTasks: number;

	/**
	 * Assessment freshness. Null when no assessment record has been written.
	 */
	assessmentFreshness: AssessmentFreshness | null;

	/** Suggested next action for the operator. */
	nextAction: string | null;
}

/**
 * Canonical lifecycle resolver.
 *
 * Accepts the raw artifact state and derived assessment/reconciliation data,
 * and returns one normalized LifecycleSummary through a single implementation
 * path. All callers must route through this function — no separate stage or
 * substate derivations.
 */
export function resolveLifecycleSummary(input: {
	change: Pick<ChangeInfo, "name" | "stage" | "totalTasks" | "doneTasks">;
	record: AssessmentRecord | null;
	freshness: AssessmentFreshness | null;
	archiveBlocked: boolean;
	archiveBlockedReason: string | null;
	archiveBlockedIssueCodes: readonly string[];
}): LifecycleSummary {
	const { change, record, freshness, archiveBlocked, archiveBlockedReason, archiveBlockedIssueCodes } = input;

	// Derive verification status via existing resolveVerificationStatus — preserving
	// the historical substate contract without duplicating its logic.
	const vs = resolveVerificationStatus({
		stage: change.stage,
		record,
		freshness: freshness ?? { current: false, reasons: ["No assessment record"] },
		archiveBlocked,
		archiveBlockedReason,
		archiveBlockedIssueCodes,
		changeName: change.name,
	});

	const archiveReady = vs.substate === "archive-ready";

	const bindingStatus: LifecycleSummary["bindingStatus"] = archiveBlockedIssueCodes.includes("missing_design_binding")
		? "unbound"
		: archiveBlockedIssueCodes.includes("missing_design_binding") === false && change.stage === "verifying"
			? record !== null
				? "bound"
				: "unknown"
			: "unknown";

	return {
		stage: change.stage,
		verificationSubstate: vs.substate,
		archiveReady,
		bindingStatus,
		totalTasks: change.totalTasks,
		doneTasks: change.doneTasks,
		assessmentFreshness: freshness,
		nextAction: vs.nextAction,
	};
}

// ─── Spec Summary ────────────────────────────────────────────────────────────

/**
 * Count total scenarios across all spec files in a change.
 */
export function countScenarios(specs: SpecFile[]): number {
	let count = 0;
	for (const spec of specs) {
		for (const section of spec.sections) {
			for (const req of section.requirements) {
				count += req.scenarios.length;
			}
		}
	}
	return count;
}

/**
 * Summarize a change's specs as a human-readable string.
 */
export function summarizeSpecs(specs: SpecFile[]): string {
	if (specs.length === 0) return "No specs";

	const domains = specs.map((s) => s.domain);
	const totalReqs = specs.reduce(
		(sum, s) => sum + s.sections.reduce(
			(sSum, sec) => sSum + sec.requirements.length, 0,
		), 0,
	);
	const totalScenarios = countScenarios(specs);

	return `${domains.length} domain(s), ${totalReqs} requirement(s), ${totalScenarios} scenario(s)`;
}

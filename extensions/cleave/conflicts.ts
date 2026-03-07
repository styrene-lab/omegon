/**
 * cleave/conflicts — 4-step conflict detection for reunification.
 *
 * Ported from styrene-lab/cleave src/cleave/core/conflicts.py.
 * Pure functions with no runtime dependencies.
 *
 * Conflict types:
 * 1. File Overlap — multiple children modified same file
 * 2. Decision Contradiction — incompatible technology/approach choices
 * 3. Interface Mismatch — published signatures differ
 * 4. Assumption Violation — contradicts sibling's decision
 */

import type { Conflict, TaskResult } from "./types.ts";

// ─── Task result parsing ────────────────────────────────────────────────────

/**
 * Parse a task markdown file to extract result info for conflict detection.
 *
 * Expects the task file format used by the cleave workspace:
 * - **Status:** SUCCESS|PARTIAL|FAILED|PENDING
 * - **Summary:** ...
 * - **Artifacts:** bullet list of file paths
 * - **Decisions Made:** bullet list
 * - **Assumptions:** bullet list
 * - Interface signatures in backticks: `funcName(params) -> ReturnType`
 */
export function parseTaskResult(content: string, filePath: string): TaskResult {
	const result: TaskResult = {
		path: filePath,
		status: "PENDING",
		summary: null,
		fileClaims: [],
		interfacesPublished: [],
		decisions: [],
		assumptions: [],
	};

	// Status
	if (content.includes("**Status:** SUCCESS")) result.status = "SUCCESS";
	else if (content.includes("**Status:** PARTIAL")) result.status = "PARTIAL";
	else if (content.includes("**Status:** FAILED")) result.status = "FAILED";

	// Summary
	const summaryMatch = content.match(/\*\*Summary:\*\*\s*(.+?)(?:\n\*\*|\n##|$)/s);
	if (summaryMatch) {
		const summary = summaryMatch[1].trim();
		if (!summary.startsWith("[")) result.summary = summary;
	}

	// Interfaces: `function_name(params) -> return_type`
	const ifaceRe = /`([a-zA-Z_]\w*\([^)]*\)\s*->\s*[^`]+)`/g;
	let ifaceMatch: RegExpExecArray | null;
	while ((ifaceMatch = ifaceRe.exec(content)) !== null) {
		result.interfacesPublished.push(ifaceMatch[1]);
	}

	// File claims from Artifacts section
	const artifactsMatch = content.match(/\*\*Artifacts:\*\*\s*\n((?:\s*-\s*.+\n?)+)/);
	if (artifactsMatch) {
		const section = artifactsMatch[1];
		// Quoted paths
		for (const m of section.matchAll(/-\s*[`"']([^`"']+)[`"']/g)) {
			const cleaned = m[1].trim().replace(/[,;:]$/, "");
			if (cleaned && !cleaned.startsWith("[")) result.fileClaims.push(cleaned);
		}
		// Unquoted paths with extension
		for (const m of section.matchAll(/-\s*([a-zA-Z0-9_./-]+\.[a-zA-Z0-9]+)(?:\s|$|,)/g)) {
			const cleaned = m[1].trim().replace(/[,;:]$/, "");
			if (cleaned && !cleaned.startsWith("[")) result.fileClaims.push(cleaned);
		}
		result.fileClaims = [...new Set(result.fileClaims)];
	}

	// Decisions
	const decisionsMatch = content.match(/\*\*Decisions Made:\*\*\s*\n((?:\s*-\s*.+\n?)+)/);
	if (decisionsMatch) {
		result.decisions = [...decisionsMatch[1].matchAll(/-\s*(.+)/g)]
			.map((m) => m[1].trim())
			.filter((d) => d && !d.startsWith("["));
	}

	// Assumptions
	const assumptionsMatch = content.match(/\*\*Assumptions:\*\*\s*\n((?:\s*-\s*.+\n?)+)/);
	if (assumptionsMatch) {
		result.assumptions = [...assumptionsMatch[1].matchAll(/-\s*(.+)/g)]
			.map((m) => m[1].trim())
			.filter((a) => a && !a.startsWith("["));
	}

	return result;
}

// ─── Conflict detection ─────────────────────────────────────────────────────

/** Technology contradiction pairs for decision conflict detection. */
const CONTRADICTION_PAIRS: [string[], string[]][] = [
	[["redis", "use redis"], ["memcached", "use memcached"]],
	[["sql", "postgresql", "mysql"], ["nosql", "mongodb", "dynamodb"]],
	[["sync", "synchronous"], ["async", "asynchronous"]],
	[["rest", "restful"], ["graphql", "grpc"]],
	[["jwt"], ["session", "cookie-based"]],
];

const REJECTION_PHRASES = ["instead of", "not ", "rather than", "over ", "rejected"];

/**
 * Run 4-step conflict detection on parsed task results.
 *
 * Steps:
 * 1. File Overlap — multiple children modified same file
 * 2. Decision Contradiction — opposing technology choices
 * 3. Interface Mismatch — conflicting function signatures
 * 4. Assumption Violation — assumption contradicts sibling's decision
 */
export function detectConflicts(results: TaskResult[]): Conflict[] {
	const conflicts: Conflict[] = [];

	// ── Step 1: File Overlap ──────────────────────────────────────────────
	// Collect ALL claimants per file, then emit one conflict per file with
	// all involved children (handles N-way overlaps, not just pairwise).
	const fileClaims = new Map<string, number[]>();
	for (let i = 0; i < results.length; i++) {
		for (const file of results[i].fileClaims) {
			const existing = fileClaims.get(file);
			if (existing) {
				existing.push(i);
			} else {
				fileClaims.set(file, [i]);
			}
		}
	}
	for (const [file, claimants] of fileClaims) {
		if (claimants.length > 1) {
			conflicts.push({
				type: "file_overlap",
				description: `Multiple children modified ${file}`,
				involved: claimants,
				resolution: "3way_merge",
			});
		}
	}

	// ── Step 2: Decision Contradiction ────────────────────────────────────
	const allDecisions: Array<[number, string]> = [];
	for (let i = 0; i < results.length; i++) {
		for (const d of results[i].decisions) {
			allDecisions.push([i, d.toLowerCase()]);
		}
	}

	for (const [termsA, termsB] of CONTRADICTION_PAIRS) {
		const siblingsA = new Set<number>();
		const siblingsB = new Set<number>();

		for (const [i, d] of allDecisions) {
			const hasA = termsA.some((t) => d.includes(t));
			const rejectedA = termsB.some((t) => d.includes(t)) && REJECTION_PHRASES.some((r) => d.includes(r));
			if (hasA && !rejectedA) siblingsA.add(i);

			const hasB = termsB.some((t) => d.includes(t));
			const rejectedB = termsA.some((t) => d.includes(t)) && REJECTION_PHRASES.some((r) => d.includes(r));
			if (hasB && !rejectedB) siblingsB.add(i);
		}

		// Only flag if DIFFERENT siblings made opposing choices
		const uniqueA = new Set([...siblingsA].filter((x) => !siblingsB.has(x)));
		const uniqueB = new Set([...siblingsB].filter((x) => !siblingsA.has(x)));

		if (uniqueA.size > 0 && uniqueB.size > 0) {
			conflicts.push({
				type: "decision_contradiction",
				description: `Contradictory decisions: ${termsA[0]} vs ${termsB[0]}`,
				involved: [...uniqueA, ...uniqueB],
				resolution: "escalate_to_parent",
			});
		}
	}

	// ── Step 3: Interface Mismatch ────────────────────────────────────────
	const published = new Map<string, { signature: string; source: number }>();
	for (let i = 0; i < results.length; i++) {
		for (const iface of results[i].interfacesPublished) {
			const funcName = iface.includes("(") ? iface.split("(")[0] : iface;
			const existing = published.get(funcName);
			if (existing && existing.signature !== iface) {
				conflicts.push({
					type: "interface_mismatch",
					description: `Interface '${funcName}' has conflicting signatures`,
					involved: [existing.source, i],
					resolution: "adapter_required",
				});
			} else if (!existing) {
				published.set(funcName, { signature: iface, source: i });
			}
		}
	}

	// ── Step 4: Assumption Violation ──────────────────────────────────────
	// Only check assumption violations between siblings that have overlapping
	// file scopes. In greenfield projects where each child creates entirely
	// new files, cross-child assumption checking produces false positives
	// from generic task description language.
	const allAssumptions: Array<[number, string]> = [];
	for (let i = 0; i < results.length; i++) {
		for (const a of results[i].assumptions) {
			allAssumptions.push([i, a.toLowerCase()]);
		}
	}

	for (const [assIdx, assumption] of allAssumptions) {
		for (const [decIdx, decision] of allDecisions) {
			if (assIdx === decIdx) continue;

			// Skip if the two siblings have zero file scope overlap.
			// Non-overlapping children are unlikely to have real assumption
			// violations — the detector would fire on generic phrasing.
			const filesA = new Set(results[assIdx].fileClaims);
			const filesB = results[decIdx].fileClaims;
			const hasOverlap = filesB.some((f) => filesA.has(f));
			if (filesA.size > 0 && filesB.length > 0 && !hasOverlap) continue;

			// Check negation patterns
			const negInAssumption =
				assumption.includes("not ") &&
				assumption
					.replace("not ", "")
					.split(/\s+/)
					.some((w) => decision.includes(w));
			const negInDecision =
				decision.includes("not ") &&
				decision
					.replace("not ", "")
					.split(/\s+/)
					.some((w) => assumption.includes(w));

			if (negInAssumption || negInDecision) {
				conflicts.push({
					type: "assumption_violation",
					description: `Sibling ${assIdx}'s assumption may conflict with sibling ${decIdx}'s decision`,
					involved: [assIdx, decIdx],
					resolution: "verify_with_parent",
				});
			}
		}
	}

	return conflicts;
}

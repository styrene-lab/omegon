/**
 * design-tree/tree — Pure domain logic for design tree operations.
 *
 * No pi dependency — can be tested standalone. All functions operate
 * on the filesystem and return plain data structures.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import type {
	AcceptanceCriteria,
	AcceptanceCriteriaConstraint,
	AcceptanceCriteriaFalsifiability,
	AcceptanceCriteriaScenario,
	DesignNode,
	DesignTree,
	DesignDecision,
	DocumentSections,
	FileScope,
	IssueType,
	NodeStatus,
	Priority,
	ResearchEntry,
} from "./types.ts";
import { VALID_ISSUE_TYPES, VALID_STATUSES, SECTION_HEADINGS } from "./types.ts";

// ─── Frontmatter Parsing ─────────────────────────────────────────────────────

export function parseFrontmatter(content: string): Record<string, unknown> | null {
	const match = content.match(/^---\n([\s\S]*?)\n---/);
	if (!match) return null;

	const yaml = match[1];
	const result: Record<string, unknown> = {};

	let currentKey: string | null = null;
	let currentArray: string[] | null = null;

	for (const line of yaml.split("\n")) {
		// Array item: "  - something"
		const arrayMatch = line.match(/^\s+-\s+(.+)/);
		if (arrayMatch && currentKey) {
			if (!currentArray) currentArray = [];
			currentArray.push(arrayMatch[1].trim().replace(/^["']|["']$/g, ""));
			continue;
		}

		// Flush previous array when we hit a non-array line
		if (currentKey && currentArray !== null) {
			result[currentKey] = currentArray;
			currentArray = null;
			currentKey = null;
		}
		if (currentKey && currentArray === null) {
			result[currentKey] = [];
			currentKey = null;
		}

		// Key-value pair
		const kvMatch = line.match(/^(\w[\w_]*):\s*(.*)/);
		if (kvMatch) {
			const key = kvMatch[1];
			const value = kvMatch[2].trim();

			if (value === "" || value === "[]") {
				if (value === "[]") {
					result[key] = [];
				} else {
					currentKey = key;
					currentArray = null;
				}
			} else if (value.startsWith("[") && value.endsWith("]")) {
				result[key] = value
					.slice(1, -1)
					.split(",")
					.map((s) => s.trim().replace(/^["']|["']$/g, ""))
					.filter(Boolean);
			} else {
				// Strip inline YAML comments (# ...) unless value is quoted
				const stripped = /^["']/.test(value)
					? value.replace(/^["'](.*)["']$/, "$1")
					: value.replace(/\s+#.*$/, "").trim();
				result[key] = stripped;
			}
		}
	}

	if (currentKey && currentArray !== null) {
		result[currentKey] = currentArray;
	} else if (currentKey) {
		result[currentKey] = [];
	}

	return result;
}

/** Quote a YAML value if it contains special characters */
export function yamlQuote(value: string): string {
	if (/[:#\[\]{}&*!|>'"%@`/]/.test(value) || value.startsWith("- ")) {
		return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
	}
	return value;
}

export function generateFrontmatter(node: Omit<DesignNode, "filePath" | "lastModified">): string {
	let fm = "---\n";
	fm += `id: ${node.id}\n`;
	fm += `title: ${yamlQuote(node.title)}\n`;
	fm += `status: ${node.status}\n`;
	if (node.parent) fm += `parent: ${node.parent}\n`;
	if (node.dependencies.length > 0) {
		fm += `dependencies: [${node.dependencies.join(", ")}]\n`;
	}
	if (node.related.length > 0) {
		fm += `related: [${node.related.join(", ")}]\n`;
	}
	if (node.tags.length > 0) {
		fm += `tags: [${node.tags.join(", ")}]\n`;
	}
	if (node.open_questions.length > 0) {
		fm += "open_questions:\n";
		for (const q of node.open_questions) {
			fm += `  - ${yamlQuote(q)}\n`;
		}
	} else {
		fm += "open_questions: []\n";
	}
	if (node.branch) {
		fm += `branch: ${yamlQuote(node.branch)}\n`;
	}
	if (node.branches && node.branches.length > 0) {
		fm += `branches: [${node.branches.map((b) => yamlQuote(b)).join(", ")}]\n`;
	}
	if (node.openspec_change) {
		fm += `openspec_change: ${node.openspec_change}\n`;
	}
	if (node.issue_type) {
		fm += `issue_type: ${node.issue_type}\n`;
	}
	if (node.priority !== undefined) {
		fm += `priority: ${node.priority}\n`;
	}
	fm += "---\n";
	return fm;
}

// ─── Body Section Parsing ────────────────────────────────────────────────────

/**
 * Parse the document body (after frontmatter) into structured sections.
 *
 * Recognized sections:
 *   ## Overview         → overview text
 *   ## Research         → ### subheadings with content
 *   ## Decisions        → ### Decision: Title blocks with status/rationale
 *   ## Open Questions   → bullet list of questions
 *   ## Implementation Notes → file scope, constraints, raw content
 *
 * Unrecognized ## headings are captured as extraSections.
 */
export function parseSections(body: string): DocumentSections {
	const sections: DocumentSections = {
		overview: "",
		research: [],
		decisions: [],
		openQuestions: [],
		implementationNotes: { fileScope: [], constraints: [], rawContent: "" },
		acceptanceCriteria: { scenarios: [], falsifiability: [], constraints: [] },
		extraSections: [],
	};

	// Split body on ## headings, keeping the heading text
	const parts = body.split(/^(## .+)$/m);

	// First part (before any ##) could be content after the title
	let preamble = parts[0].trim();
	// Strip the # title line if present
	preamble = preamble.replace(/^# .+\n?/, "").trim();
	if (preamble && !parts.some((p) => p.startsWith("## Overview"))) {
		sections.overview = preamble;
	}

	// Process heading+content pairs
	for (let i = 1; i < parts.length; i += 2) {
		const heading = parts[i].trim();
		const content = (parts[i + 1] || "").trim();

		if (heading === SECTION_HEADINGS.overview) {
			sections.overview = content;
		} else if (heading === SECTION_HEADINGS.research) {
			sections.research = parseResearchSection(content);
		} else if (heading === SECTION_HEADINGS.decisions) {
			sections.decisions = parseDecisionsSection(content);
		} else if (heading === SECTION_HEADINGS.openQuestions) {
			sections.openQuestions = parseOpenQuestionsSection(content);
		} else if (heading === SECTION_HEADINGS.implementationNotes) {
			sections.implementationNotes = parseImplementationNotesSection(content);
		} else if (heading === SECTION_HEADINGS.acceptanceCriteria) {
			sections.acceptanceCriteria = parseAcceptanceCriteriaSection(content);
		} else {
			sections.extraSections.push({
				heading: heading.replace(/^## /, ""),
				content,
			});
		}
	}

	return sections;
}

function parseResearchSection(content: string): ResearchEntry[] {
	const entries: ResearchEntry[] = [];
	const parts = content.split(/^(### .+)$/m);

	// Content before first ### is a general research note
	const preamble = parts[0].trim();
	if (preamble) {
		entries.push({ heading: "General", content: preamble });
	}

	for (let i = 1; i < parts.length; i += 2) {
		const heading = parts[i].replace(/^### /, "").trim();
		const body = (parts[i + 1] || "").trim();
		entries.push({ heading, content: body });
	}
	return entries;
}

function parseDecisionsSection(content: string): DesignDecision[] {
	const decisions: DesignDecision[] = [];
	// Split on ### Decision: headings
	const parts = content.split(/^(### Decision: .+)$/m);

	for (let i = 1; i < parts.length; i += 2) {
		const title = parts[i].replace(/^### Decision:\s*/, "").trim();
		const body = (parts[i + 1] || "").trim();

		// Extract status
		const statusMatch = body.match(/\*\*Status:\*\*\s*(\w+)/);
		let status: DesignDecision["status"] = "exploring";
		if (statusMatch) {
			const s = statusMatch[1].toLowerCase();
			if (s === "decided" || s === "rejected" || s === "exploring") {
				status = s;
			}
		}

		// Extract rationale
		const rationaleMatch = body.match(/\*\*Rationale:\*\*\s*([\s\S]*?)(?=\n\*\*|\n###|$)/);
		const rationale = rationaleMatch ? rationaleMatch[1].trim() : body;

		decisions.push({ title, status, rationale });
	}
	return decisions;
}

function parseOpenQuestionsSection(content: string): string[] {
	const questions: string[] = [];
	for (const line of content.split("\n")) {
		// Match: - Question text  or  * Question text  or  1. Question text
		const m = line.match(/^\s*[-*]\s+(.+)/) || line.match(/^\s*\d+\.\s+(.+)/);
		if (m) {
			questions.push(m[1].trim());
		}
	}
	return questions;
}

function parseImplementationNotesSection(
	content: string,
): DocumentSections["implementationNotes"] {
	const result: DocumentSections["implementationNotes"] = {
		fileScope: [],
		constraints: [],
		rawContent: content,
	};

	// Parse ### File Scope sub-section
	const fileScopeMatch = content.match(
		/### File Scope\s*\n([\s\S]*?)(?=\n###|\n## |$)/,
	);
	if (fileScopeMatch) {
		for (const line of fileScopeMatch[1].split("\n")) {
			// Match: - `path/to/file` (action) — description  or  - `path/to/file` — description
			const m = line.match(/^\s*[-*]\s+`([^`]+)`\s*(?:\((\w+)\)\s*)?(?:—|-)\s*(.+)/);
			if (m) {
				const action = parseFileAction(m[2]);
				result.fileScope.push({ path: m[1], description: m[3].trim(), ...(action && { action }) });
			} else {
				// Match: - path/to/file — description (no backticks)
				const m2 = line.match(/^\s*[-*]\s+(\S+)\s+(?:—|-)\s+(.+)/);
				if (m2 && m2[1].includes("/")) {
					result.fileScope.push({ path: m2[1], description: m2[2].trim() });
				}
			}
		}
	}

	// Parse ### Constraints sub-section
	const constraintsMatch = content.match(
		/### Constraints\s*\n([\s\S]*?)(?=\n###|\n## |$)/,
	);
	if (constraintsMatch) {
		for (const line of constraintsMatch[1].split("\n")) {
			const m = line.match(/^\s*[-*]\s+(.+)/);
			if (m) {
				result.constraints.push(m[1].trim());
			}
		}
	}

	return result;
}

/** Parse a file action string from markdown */
function parseFileAction(raw: string | undefined): FileScope["action"] | undefined {
	if (!raw) return undefined;
	const s = raw.toLowerCase();
	if (s === "new" || s === "created" || s === "create") return "new";
	if (s === "modified" || s === "updated" || s === "modify") return "modified";
	if (s === "deleted" || s === "removed" || s === "delete") return "deleted";
	return undefined;
}

// ─── Acceptance Criteria Parsing ──────────────────────────────────────────────

/**
 * Parse the ## Acceptance Criteria section into structured scenarios,
 * falsifiability conditions, and checkbox constraints.
 *
 * ### Scenarios — bold Given/When/Then blocks:
 *   **Given** some context
 *   **When** something happens
 *   **Then** expected outcome
 *
 * ### Falsifiability — bullet list with "This decision is wrong if:" prefix:
 *   - This decision is wrong if: some condition
 *   - some condition (bare, without prefix)
 *
 * ### Constraints — GFM checkboxes:
 *   - [ ] unchecked constraint
 *   - [x] checked constraint
 */
function parseAcceptanceCriteriaSection(content: string): AcceptanceCriteria {
	const result: AcceptanceCriteria = {
		scenarios: [],
		falsifiability: [],
		constraints: [],
	};

	// Split on ### sub-headings
	const parts = content.split(/^(### .+)$/m);

	for (let i = 1; i < parts.length; i += 2) {
		const subHeading = parts[i].replace(/^### /, "").trim();
		const body = (parts[i + 1] || "").trim();

		if (subHeading === "Scenarios") {
			result.scenarios = parseScenariosBlock(body);
		} else if (subHeading === "Falsifiability") {
			result.falsifiability = parseFalsifiabilityBlock(body);
		} else if (subHeading === "Constraints") {
			result.constraints = parseCheckboxConstraints(body);
		}
	}

	return result;
}

/**
 * Parse bold Given/When/Then scenario blocks. Each scenario may optionally
 * have a title on a line before the Given keyword, or be titled by sequence.
 *
 * Accepts both single-line (**Given** text) and multi-line formats.
 */
function parseScenariosBlock(content: string): AcceptanceCriteriaScenario[] {
	const scenarios: AcceptanceCriteriaScenario[] = [];
	const blocks: Array<{ title: string; content: string }> = [];

	// Try explicit #### or "Scenario:" headings first
	const headingMatches = [...content.matchAll(/^(?:####\s+(.+)|Scenario:\s*(.+))$/gm)];
	if (headingMatches.length > 0) {
		for (let i = 0; i < headingMatches.length; i++) {
			const m = headingMatches[i];
			const title = (m[1] || m[2] || "").trim();
			const start = (m.index ?? 0) + m[0].length;
			const end = i + 1 < headingMatches.length ? (headingMatches[i + 1].index ?? content.length) : content.length;
			blocks.push({ title, content: content.slice(start, end).trim() });
		}
	} else {
		// Split on "**Given**" lines (non-zero-width match to avoid infinite loop)
		const parts = content.split(/^(?=\*\*Given\*\*)/m);
		if (parts.length <= 1) {
			// No split happened — whole content is one block
			blocks.push({ title: "", content: content.trim() });
		} else {
			for (const part of parts) {
				const trimmed = part.trim();
				// Skip preamble segments that don't start with **Given** (e.g. intro text)
				if (trimmed && /^\*\*Given\*\*/.test(trimmed)) {
					blocks.push({ title: "", content: trimmed });
				}
			}
		}
	}

	for (let idx = 0; idx < blocks.length; idx++) {
		const { title, content: block } = blocks[idx];
		const givenMatch = block.match(/\*\*Given\*\*\s*(.+)/);
		const whenMatch = block.match(/\*\*When\*\*\s*(.+)/);
		const thenMatch = block.match(/\*\*Then\*\*\s*(.+)/);

		if (givenMatch || whenMatch || thenMatch) {
			scenarios.push({
				title: title || `Scenario ${idx + 1}`,
				given: givenMatch ? givenMatch[1].trim() : "",
				when: whenMatch ? whenMatch[1].trim() : "",
				then: thenMatch ? thenMatch[1].trim() : "",
			});
		}
	}

	return scenarios;
}

/**
 * Parse falsifiability bullet list.
 * Strips the "This decision is wrong if:" prefix when present.
 */
function parseFalsifiabilityBlock(content: string): AcceptanceCriteriaFalsifiability[] {
	const results: AcceptanceCriteriaFalsifiability[] = [];
	const PREFIX = /^this decision is wrong if:\s*/i;

	for (const line of content.split("\n")) {
		const m = line.match(/^\s*[-*]\s+(.+)/);
		if (m) {
			const raw = m[1].trim();
			const condition = raw.replace(PREFIX, "").trim();
			results.push({ condition });
		}
	}
	return results;
}

/**
 * Parse GFM checkbox list into AcceptanceCriteriaConstraint items.
 */
function parseCheckboxConstraints(content: string): AcceptanceCriteriaConstraint[] {
	const results: AcceptanceCriteriaConstraint[] = [];
	for (const line of content.split("\n")) {
		const m = line.match(/^\s*-\s+\[([ xX])\]\s+(.+)/);
		if (m) {
			results.push({
				checked: m[1].toLowerCase() === "x",
				text: m[2].trim(),
			});
		}
	}
	return results;
}

// ─── Body Generation ─────────────────────────────────────────────────────────

/**
 * Generate a complete document body from structured sections.
 */
export function generateBody(title: string, sections: DocumentSections): string {
	const parts: string[] = [`# ${title}`, ""];

	// Overview
	parts.push(SECTION_HEADINGS.overview, "");
	parts.push(sections.overview || "*To be explored.*");
	parts.push("");

	// Research (only if non-empty)
	if (sections.research.length > 0) {
		parts.push(SECTION_HEADINGS.research, "");
		for (const entry of sections.research) {
			if (entry.heading !== "General") {
				parts.push(`### ${entry.heading}`, "");
			}
			parts.push(entry.content, "");
		}
	}

	// Decisions (only if non-empty)
	if (sections.decisions.length > 0) {
		parts.push(SECTION_HEADINGS.decisions, "");
		for (const d of sections.decisions) {
			parts.push(`### Decision: ${d.title}`, "");
			parts.push(`**Status:** ${d.status}`);
			parts.push(`**Rationale:** ${d.rationale}`, "");
		}
	}

	// Open Questions
	parts.push(SECTION_HEADINGS.openQuestions, "");
	if (sections.openQuestions.length > 0) {
		for (const q of sections.openQuestions) {
			parts.push(`- ${q}`);
		}
	} else {
		parts.push("*No open questions.*");
	}
	parts.push("");

	// Implementation Notes (only if has content)
	if (
		sections.implementationNotes.fileScope.length > 0 ||
		sections.implementationNotes.constraints.length > 0
	) {
		parts.push(SECTION_HEADINGS.implementationNotes, "");
		if (sections.implementationNotes.fileScope.length > 0) {
			parts.push("### File Scope", "");
			for (const f of sections.implementationNotes.fileScope) {
				const actionTag = f.action ? ` (${f.action})` : "";
				parts.push(`- \`${f.path}\`${actionTag} — ${f.description}`);
			}
			parts.push("");
		}
		if (sections.implementationNotes.constraints.length > 0) {
			parts.push("### Constraints", "");
			for (const c of sections.implementationNotes.constraints) {
				parts.push(`- ${c}`);
			}
			parts.push("");
		}
	}

	// Acceptance Criteria (only if has content)
	const ac = sections.acceptanceCriteria;
	if (ac.scenarios.length > 0 || ac.falsifiability.length > 0 || ac.constraints.length > 0) {
		parts.push(SECTION_HEADINGS.acceptanceCriteria, "");

		if (ac.scenarios.length > 0) {
			parts.push("### Scenarios", "");
			for (const s of ac.scenarios) {
				if (s.title && !s.title.match(/^Scenario \d+$/)) {
					parts.push(`#### ${s.title}`, "");
				}
				if (s.given) parts.push(`**Given** ${s.given}`);
				if (s.when) parts.push(`**When** ${s.when}`);
				if (s.then) parts.push(`**Then** ${s.then}`);
				parts.push("");
			}
		}

		if (ac.falsifiability.length > 0) {
			parts.push("### Falsifiability", "");
			for (const f of ac.falsifiability) {
				parts.push(`- This decision is wrong if: ${f.condition}`);
			}
			parts.push("");
		}

		if (ac.constraints.length > 0) {
			parts.push("### Constraints", "");
			for (const c of ac.constraints) {
				parts.push(`- [${c.checked ? "x" : " "}] ${c.text}`);
			}
			parts.push("");
		}
	}

	// Extra sections
	for (const extra of sections.extraSections) {
		parts.push(`## ${extra.heading}`, "");
		parts.push(extra.content, "");
	}

	return parts.join("\n");
}

// ─── Tree Scanning ───────────────────────────────────────────────────────────

/**
 * Scan a docs/ directory for design documents and build a DesignTree.
 */
export function scanDesignDocs(docsDir: string): DesignTree {
	const tree: DesignTree = { nodes: new Map(), docsDir };
	if (!fs.existsSync(docsDir)) return tree;

	// Scan both docs/ and docs/design/ for design documents
	let files = fs.readdirSync(docsDir).filter((f) => f.endsWith(".md"));
	const designSubdir = path.join(docsDir, "design");
	if (fs.existsSync(designSubdir)) {
		const archiveFiles = fs.readdirSync(designSubdir)
			.filter((f) => f.endsWith(".md"))
			.map((f) => path.join("design", f));
		files = files.concat(archiveFiles);
	}

	for (const file of files) {
		const filePath = path.join(docsDir, file);
		const content = fs.readFileSync(filePath, "utf-8");
		const fm = parseFrontmatter(content);

		if (fm && fm.id) {
			const rawStatus = fm.status as string;
			const status: NodeStatus = VALID_STATUSES.includes(rawStatus as NodeStatus)
				? (rawStatus as NodeStatus)
				: "exploring";

			// Parse body sections to sync open_questions from body
			const body = extractBody(content);
			const sections = parseSections(body);
			const bodyQuestions = sections.openQuestions;

			// Body is source of truth for open questions; merge with frontmatter
			// Prefer body questions, but keep frontmatter-only questions too
			const fmQuestions = (fm.open_questions as string[]) || [];
			const mergedQuestions = mergeQuestions(bodyQuestions, fmQuestions);

			// Validate optional branch override — discard and warn if invalid
			const rawBranch = fm.branch as string | undefined;
			let validatedBranch: string | undefined;
			if (rawBranch !== undefined) {
				validatedBranch = sanitizeBranchName(rawBranch) ?? undefined;
				if (validatedBranch === undefined) {
					console.warn(
						`[design-tree] Node '${fm.id}': invalid 'branch' value '${rawBranch}' — ` +
						`contains disallowed characters. Field ignored; fix the frontmatter.`,
					);
				}
			}

			const rawIssueType = fm.issue_type as string | undefined;
			const issue_type: IssueType | undefined =
				rawIssueType && VALID_ISSUE_TYPES.includes(rawIssueType as IssueType)
					? (rawIssueType as IssueType)
					: undefined;

			// parseInt handles both numeric strings ("3") and quoted strings — explicit radix avoids octal ambiguity
			const rawPriority = fm.priority !== undefined ? parseInt(String(fm.priority), 10) : undefined;
			const priority: Priority | undefined =
				rawPriority !== undefined && rawPriority >= 1 && rawPriority <= 5
					? (rawPriority as Priority)
					: undefined;

			const node: DesignNode = {
				id: fm.id as string,
				title: (fm.title as string) || file.replace(".md", ""),
				status,
				parent: fm.parent as string | undefined,
				dependencies: (fm.dependencies as string[]) || [],
				related: (fm.related as string[]) || [],
				tags: (fm.tags as string[]) || [],
				open_questions: mergedQuestions,
				branch: validatedBranch,
				branches: (fm.branches as string[]) || [],
				openspec_change: fm.openspec_change as string | undefined,
				issue_type,
				priority,
				filePath,
				lastModified: fs.statSync(filePath).mtimeMs,
			};
			tree.nodes.set(node.id, node);
		}
	}

	return tree;
}

/** Merge body questions (source of truth) with frontmatter questions (legacy) */
function mergeQuestions(bodyQuestions: string[], fmQuestions: string[]): string[] {
	if (bodyQuestions.length > 0) return bodyQuestions;
	return fmQuestions; // fallback to frontmatter if body section empty/missing
}

/** Extract body content after frontmatter */
export function extractBody(content: string): string {
	const match = content.match(/^---\n[\s\S]*?\n---\n([\s\S]*)/);
	return match ? match[1].trim() : content.trim();
}

// ─── Tree Queries ────────────────────────────────────────────────────────────

export function getChildren(tree: DesignTree, parentId: string): DesignNode[] {
	return Array.from(tree.nodes.values()).filter((n) => n.parent === parentId);
}

export function getRoots(tree: DesignTree): DesignNode[] {
	return Array.from(tree.nodes.values()).filter((n) => !n.parent);
}

export function getAllOpenQuestions(
	tree: DesignTree,
): Array<{ node: DesignNode; question: string }> {
	const questions: Array<{ node: DesignNode; question: string }> = [];
	for (const node of tree.nodes.values()) {
		for (const q of node.open_questions) {
			questions.push({ node, question: q });
		}
	}
	return questions;
}

/** Get document body, optionally truncated */
export function getDocBody(filePath: string, maxChars: number = 4000): string {
	const content = fs.readFileSync(filePath, "utf-8");
	const body = extractBody(content);
	if (body.length <= maxChars) return body;
	return body.slice(0, maxChars) + "\n\n[...truncated]";
}

/** Get fully parsed sections from a node's document */
export function getNodeSections(node: DesignNode): DocumentSections {
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	return parseSections(body);
}

/**
 * Lightweight acceptance-criteria counter for the `list` hot path.
 * Avoids full section parse by scanning for the AC section and counting
 * structural markers (Given blocks, bullet items, checkboxes) with regex.
 * Returns null when no Acceptance Criteria section exists.
 */
export function countAcceptanceCriteria(
	node: DesignNode,
): { scenarios: number; falsifiability: number; constraints: number } | null {
	let content: string;
	try {
		content = fs.readFileSync(node.filePath, "utf-8");
	} catch {
		return null;
	}

	// Find the ## Acceptance Criteria section via string search (regex lookahead
	// for end-of-string is unreliable across JS engines with multiline mode).
	const ACH = "\n## Acceptance Criteria\n";
	const acStart = content.indexOf(ACH);
	if (acStart === -1) return null;
	const acBodyStart = acStart + ACH.length;
	const nextH2 = content.indexOf("\n## ", acBodyStart);
	const acBody = nextH2 >= 0 ? content.slice(acBodyStart, nextH2) : content.slice(acBodyStart);

	// Count **Given** occurrences for scenarios
	const scenarioCount = (acBody.match(/^\*\*Given\*\*/gm) ?? []).length;

	// Find the ### Falsifiability sub-section and count list items
	const falsifiabilityCount = (() => {
		const h = "\n### Falsifiability\n";
		const start = acBody.indexOf(h);
		if (start === -1) return 0;
		const bodyStart = start + h.length;
		const nextH3 = acBody.indexOf("\n### ", bodyStart);
		const sub = nextH3 >= 0 ? acBody.slice(bodyStart, nextH3) : acBody.slice(bodyStart);
		return (sub.match(/^\s*-\s+\S/gm) ?? []).length;
	})();

	// Find the ### Constraints sub-section and count checkboxes
	const constraintCount = (() => {
		const h = "\n### Constraints\n";
		const start = acBody.indexOf(h);
		if (start === -1) return 0;
		const bodyStart = start + h.length;
		const nextH3 = acBody.indexOf("\n### ", bodyStart);
		const sub = nextH3 >= 0 ? acBody.slice(bodyStart, nextH3) : acBody.slice(bodyStart);
		return (sub.match(/^\s*-\s+\[[ xX]\]/gm) ?? []).length;
	})();

	const total = scenarioCount + falsifiabilityCount + constraintCount;
	if (total === 0) return null;

	return {
		scenarios: scenarioCount,
		falsifiability: falsifiabilityCount,
		constraints: constraintCount,
	};
}

// ─── Tree Mutations ──────────────────────────────────────────────────────────

/**
 * Create a new design node document.
 * Returns the created node.
 */
export function createNode(
	docsDir: string,
	opts: {
		id: string;
		title: string;
		parent?: string;
		status?: NodeStatus;
		tags?: string[];
		overview?: string;
		issue_type?: IssueType;
		priority?: Priority;
		spawnedFrom?: { parentTitle: string; parentFile: string; question: string };
	},
): DesignNode {
	const idError = validateNodeId(opts.id);
	if (idError) throw new Error(`Invalid node ID '${opts.id}': ${idError}`);

	if (!fs.existsSync(docsDir)) {
		fs.mkdirSync(docsDir, { recursive: true });
	}

	const node: Omit<DesignNode, "filePath" | "lastModified"> = {
		id: opts.id,
		title: opts.title,
		status: opts.status || "seed",
		parent: opts.parent,
		dependencies: [],
		related: [],
		tags: opts.tags || [],
		open_questions: [],
		branch: undefined,
		branches: [],
		issue_type: opts.issue_type,
		priority: opts.priority,
	};

	const sections: DocumentSections = {
		overview: opts.overview || "*To be explored.*",
		research: [],
		decisions: [],
		openQuestions: [],
		implementationNotes: { fileScope: [], constraints: [], rawContent: "" },
		acceptanceCriteria: { scenarios: [], falsifiability: [], constraints: [] },
		extraSections: [],
	};

	// If spawned from a parent question, add context
	if (opts.spawnedFrom) {
		sections.overview =
			`> Parent: [${opts.spawnedFrom.parentTitle}](${opts.spawnedFrom.parentFile})\n` +
			`> Spawned from: "${opts.spawnedFrom.question}"\n\n` +
			(opts.overview || "*To be explored.*");
		sections.openQuestions = [`${opts.spawnedFrom.question}`];
		node.open_questions = [...sections.openQuestions];
	}

	const fm = generateFrontmatter(node);
	const body = generateBody(opts.title, sections);
	const filePath = path.join(docsDir, `${opts.id}.md`);

	fs.writeFileSync(filePath, fm + "\n" + body);

	return {
		...node,
		filePath,
		lastModified: Date.now(),
	};
}

/**
 * Set a node's status. Writes to disk.
 * Returns the updated node.
 */
export function setNodeStatus(node: DesignNode, newStatus: NodeStatus): DesignNode {
	let content = fs.readFileSync(node.filePath, "utf-8");
	content = content.replace(
		/^(---\n[\s\S]*?\nstatus:\s*)\S+/m,
		`$1${newStatus}`,
	);
	fs.writeFileSync(node.filePath, content);
	return { ...node, status: newStatus };
}

/**
 * Add an open question to a node. Updates both the body ## Open Questions
 * section and the frontmatter.
 */
export function addOpenQuestion(node: DesignNode, question: string): DesignNode {
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);

	sections.openQuestions.push(question);
	const updatedNode = {
		...node,
		open_questions: [...sections.openQuestions],
	};

	writeNodeDocument(updatedNode, sections);
	return updatedNode;
}

/**
 * Remove an open question from a node by index or text match.
 */
export function removeOpenQuestion(
	node: DesignNode,
	questionOrIndex: string | number,
): DesignNode {
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);

	// Seed body sections from node's in-memory merged state when body has no Open Questions
	// section yet (e.g. frontmatter-only nodes). Keeps body + frontmatter in sync.
	if (sections.openQuestions.length === 0 && node.open_questions.length > 0) {
		sections.openQuestions = [...node.open_questions];
	}

	if (typeof questionOrIndex === "number") {
		if (questionOrIndex >= 0 && questionOrIndex < sections.openQuestions.length) {
			sections.openQuestions.splice(questionOrIndex, 1);
		}
	} else {
		sections.openQuestions = sections.openQuestions.filter(
			(q) => q !== questionOrIndex,
		);
	}

	const updatedNode = {
		...node,
		open_questions: [...sections.openQuestions],
	};

	writeNodeDocument(updatedNode, sections);
	return updatedNode;
}

/**
 * Add a research entry to a node.
 */
export function addResearch(
	node: DesignNode,
	heading: string,
	content: string,
): void {
	const fileContent = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(fileContent);
	const sections = parseSections(body);

	sections.research.push({ heading, content });
	writeNodeDocument(node, sections);
}

/**
 * Add a decision to a node.
 */
export function addDecision(
	node: DesignNode,
	decision: DesignDecision,
): void {
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);

	sections.decisions.push(decision);
	writeNodeDocument(node, sections);
}

/**
 * Add a dependency to a node.
 */
export function addDependency(node: DesignNode, depId: string): DesignNode {
	if (node.dependencies.includes(depId)) return node;
	const updatedNode = {
		...node,
		dependencies: [...node.dependencies, depId],
	};
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);
	writeNodeDocument(updatedNode, sections);
	return updatedNode;
}

/**
 * Add a related node reference.
 * If `reciprocal` is provided, also adds the reverse link on the target node.
 */
export function addRelated(node: DesignNode, relatedId: string, reciprocal?: DesignNode): DesignNode {
	if (node.related.includes(relatedId)) return node;
	const updatedNode = {
		...node,
		related: [...node.related, relatedId],
	};
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);
	writeNodeDocument(updatedNode, sections);

	// Add reverse link if reciprocal node provided and not already linked
	if (reciprocal && !reciprocal.related.includes(node.id)) {
		const recipUpdated = {
			...reciprocal,
			related: [...reciprocal.related, node.id],
		};
		const recipContent = fs.readFileSync(reciprocal.filePath, "utf-8");
		const recipBody = extractBody(recipContent);
		const recipSections = parseSections(recipBody);
		writeNodeDocument(recipUpdated, recipSections);
	}

	return updatedNode;
}

/**
 * Update a node's overview text.
 */
export function updateOverview(node: DesignNode, overview: string): void {
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);
	sections.overview = overview;
	writeNodeDocument(node, sections);
}

/**
 * Add implementation notes (file scope and/or constraints).
 */
export function addImplementationNotes(
	node: DesignNode,
	opts: { fileScope?: FileScope[]; constraints?: string[] },
): void {
	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);

	if (opts.fileScope) {
		sections.implementationNotes.fileScope.push(...opts.fileScope);
	}
	if (opts.constraints) {
		sections.implementationNotes.constraints.push(...opts.constraints);
	}

	writeNodeDocument(node, sections);
}

// ─── Document Write-Back ─────────────────────────────────────────────────────

/**
 * Write a node's full document to disk (frontmatter + body).
 * Syncs open_questions between sections and frontmatter.
 */
export function writeNodeDocument(node: DesignNode, sections: DocumentSections): void {
	// Sync open questions from sections to node
	const syncedNode = {
		...node,
		open_questions: sections.openQuestions,
	};

	const fm = generateFrontmatter(syncedNode);
	const body = generateBody(syncedNode.title, sections);
	fs.writeFileSync(node.filePath, fm + "\n" + body);
}

// ─── Branch ──────────────────────────────────────────────────────────────────

/**
 * Branch a child node from a parent's open question.
 *
 * Creates the child doc and optionally removes the question from the parent.
 */
export function branchFromQuestion(
	tree: DesignTree,
	parentId: string,
	question: string,
	childId: string,
	childTitle: string,
	removeFromParent: boolean = true,
): DesignNode | null {
	const parent = tree.nodes.get(parentId);
	if (!parent) return null;
	if (!parent.open_questions.includes(question)) return null;

	const childIdError = validateNodeId(childId);
	if (childIdError) return null;

	const child = createNode(tree.docsDir, {
		id: childId,
		title: childTitle,
		parent: parentId,
		spawnedFrom: {
			parentTitle: parent.title,
			parentFile: path.basename(parent.filePath),
			question,
		},
	});

	if (removeFromParent) {
		removeOpenQuestion(parent, question);
	}

	return child;
}

// ─── Validation & Helpers ────────────────────────────────────────────────────

/** Strict pattern for node IDs — no path traversal, no dots, no slashes */
const VALID_ID_RE = /^[a-z0-9][a-z0-9_-]*$/;

/**
 * Validate a git branch name — reject shell metacharacters and invalid git ref chars.
 * Minimum length 2 (single-char names rejected by the allowlist regex).
 * Returns the name if valid, null if rejected.
 */
export function sanitizeBranchName(name: string): string | null {
	if (!name || name.length > 200) return null;
	// Only allow: alphanumeric, hyphens, underscores, dots, forward slashes
	if (!/^[a-zA-Z0-9][a-zA-Z0-9._\-/]*[a-zA-Z0-9]$/.test(name)) return null;
	// Reject: consecutive dots, slash-dot, dot-slash, double slash, @{, backslash, space, ~, ^, :, ?, *, [
	if (/\.{2}|\/\.|\.\/|\/\/|@\{|\\|\s|[~^:?*[\]]/.test(name)) return null;
	// Reject .lock suffix on any component
	if (/\.lock(\/|$)/.test(name)) return null;
	return name;
}

/**
 * Validate a node ID. Rejects path traversal attempts, dots, slashes,
 * uppercase, spaces, and empty strings.
 * Returns null if valid, or an error message string if invalid.
 */
export function validateNodeId(id: string): string | null {
	if (!id) return "Node ID cannot be empty";
	if (id.length > 80) return "Node ID too long (max 80 characters)";
	if (id.includes("/") || id.includes("\\")) return "Node ID cannot contain path separators";
	if (id.includes("..")) return "Node ID cannot contain '..'";
	if (id.startsWith(".")) return "Node ID cannot start with '.'";
	if (!VALID_ID_RE.test(id)) return "Node ID must match /^[a-z0-9][a-z0-9_-]*$/ (lowercase alphanumeric, hyphens, underscores)";
	return null;
}

/** Convert a title or question to a URL-safe slug */
export function toSlug(text: string, maxLen: number = 40): string {
	return text
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "")
		.slice(0, maxLen);
}

// ─── Branch Association ──────────────────────────────────────────────────────

/**
 * Match a git branch name to a design node using segment-aware matching.
 *
 * Algorithm:
 *   1. Split branch on "/" to get path segments (e.g. "feature/auth-strategy" → ["feature", "auth-strategy"])
 *   2. For each implementing node, check if any segment starts with the node ID
 *      when both are split on hyphens (segment-aware prefix match)
 *   3. Longest matching node ID wins (prevents "auth" matching when "auth-strategy" exists)
 *
 * Only matches nodes with status "implementing" — association stops once a node
 * transitions to "implemented".
 *
 * @returns The matched DesignNode, or null if no match.
 */
export function matchBranchToNode(tree: DesignTree, branchName: string): DesignNode | null {
	if (!branchName || branchName === "main" || branchName === "detached") return null;

	// Split branch on "/" to get path segments
	const branchSegments = branchName.split("/");

	let bestMatch: DesignNode | null = null;
	let bestMatchLength = 0;

	for (const node of tree.nodes.values()) {
		if (node.status !== "implementing") continue;

		const nodeIdParts = node.id.split("-");

		for (const segment of branchSegments) {
			const segmentParts = segment.split("-");

			// Check if node ID parts are a prefix of this segment's parts
			if (nodeIdParts.length <= segmentParts.length &&
				nodeIdParts.every((part, i) => part === segmentParts[i])) {
				if (node.id.length > bestMatchLength) {
					bestMatch = node;
					bestMatchLength = node.id.length;
				}
			}
		}
	}

	return bestMatch;
}

/**
 * Append a branch name to a node's branches list and write to disk.
 * Skips if the branch is already listed.
 */
export function appendBranch(node: DesignNode, branchName: string): DesignNode {
	if (node.branches.includes(branchName)) return node;

	const updatedNode = {
		...node,
		branches: [...node.branches, branchName],
	};

	const content = fs.readFileSync(node.filePath, "utf-8");
	const body = extractBody(content);
	const sections = parseSections(body);
	writeNodeDocument(updatedNode, sections);

	return updatedNode;
}

/**
 * Read the current git branch from .git/HEAD.
 * Returns null if not in a git repo, "detached" for detached HEAD.
 */
export function readGitBranch(cwd: string): string | null {
	try {
		const gitPath = path.join(cwd, ".git");
		if (!fs.existsSync(gitPath)) return null;

		let headPath: string;
		const stat = fs.statSync(gitPath);
		if (stat.isFile()) {
			// Worktree: .git is a file pointing to the real git dir
			const content = fs.readFileSync(gitPath, "utf-8").trim();
			if (!content.startsWith("gitdir: ")) return null;
			const gitDir = content.slice(8);
			headPath = path.resolve(cwd, gitDir, "HEAD");
		} else {
			headPath = path.join(gitPath, "HEAD");
		}

		if (!fs.existsSync(headPath)) return null;
		const headContent = fs.readFileSync(headPath, "utf-8").trim();
		return headContent.startsWith("ref: refs/heads/") ? headContent.slice(16) : "detached";
	} catch {
		return null;
	}
}

// ─── OpenSpec Bridge ─────────────────────────────────────────────────────────

/**
 * Scaffold an OpenSpec change directory from a decided design node.
 *
 * Creates:
 *   openspec/changes/<node-id>/
 *     proposal.md   — from the node's overview and title
 *     design.md     — from the node's decisions and research
 *     tasks.md      — from child nodes or decisions (task groups)
 *
 * The generated tasks.md is compatible with cleave's openspec parser,
 * completing the pipeline: design → specify → parallelize → verify.
 */
export function scaffoldOpenSpecChange(
	cwd: string,
	tree: DesignTree,
	node: DesignNode,
): { message: string; changePath: string; files: string[] } {
	const sections = getNodeSections(node);
	const changePath = path.join(cwd, "openspec", "changes", node.id);
	const files: string[] = [];

	// Check for existing change directory — refuse to overwrite
	if (fs.existsSync(changePath)) {
		const existing = fs.readdirSync(changePath).filter((f) => f.endsWith(".md"));
		if (existing.length > 0) {
			return {
				message:
					`OpenSpec change directory already exists at ${changePath}\n` +
					`Existing files: ${existing.join(", ")}\n\n` +
					`To regenerate, delete the directory first:\n  rm -rf ${changePath}`,
				changePath,
				files: [],
			};
		}
	}

	fs.mkdirSync(changePath, { recursive: true });

	// ── proposal.md ──────────────────────────────────────────────
	const proposalLines = [
		`# ${node.title}`,
		"",
		"## Intent",
		"",
		sections.overview || "*Implement the design as specified.*",
		"",
	];

	if (node.dependencies.length > 0) {
		proposalLines.push("## Dependencies", "");
		for (const depId of node.dependencies) {
			const dep = tree.nodes.get(depId);
			proposalLines.push(`- ${dep ? dep.title : depId} (${dep?.status || "unknown"})`);
		}
		proposalLines.push("");
	}

	const proposalPath = path.join(changePath, "proposal.md");
	fs.writeFileSync(proposalPath, proposalLines.join("\n"));
	files.push("proposal.md");

	// ── design.md ────────────────────────────────────────────────
	const designLines = [
		`# ${node.title} — Design`,
		"",
	];

	if (sections.decisions.length > 0) {
		designLines.push("## Architecture Decisions", "");
		for (const d of sections.decisions) {
			designLines.push(`### Decision: ${d.title}`, "");
			designLines.push(`**Status:** ${d.status}`);
			if (d.rationale) designLines.push(`**Rationale:** ${d.rationale}`);
			designLines.push("");
		}
	}

	if (sections.research.length > 0) {
		designLines.push("## Research Context", "");
		for (const r of sections.research) {
			designLines.push(`### ${r.heading}`, "");
			designLines.push(r.content, "");
		}
	}

	if (sections.implementationNotes.fileScope.length > 0) {
		designLines.push("## File Changes", "");
		for (const f of sections.implementationNotes.fileScope) {
			const action = f.action || "new";
			designLines.push(`- \`${f.path}\` (${action}) — ${f.description}`);
		}
		designLines.push("");
	}

	if (sections.implementationNotes.constraints.length > 0) {
		designLines.push("## Constraints", "");
		for (const c of sections.implementationNotes.constraints) {
			designLines.push(`- ${c}`);
		}
		designLines.push("");
	}

	const designPath = path.join(changePath, "design.md");
	fs.writeFileSync(designPath, designLines.join("\n"));
	files.push("design.md");

	// ── tasks.md ─────────────────────────────────────────────────
	const children = getChildren(tree, node.id);
	const taskLines = [`# ${node.title} — Tasks`, ""];

	if (children.length > 0) {
		// ── Child-node-driven groups ───────────────────────────────
		let groupNum = 1;
		for (const child of children) {
			taskLines.push(`## ${groupNum}. ${child.title}`, "");
			const childSections = getNodeSections(child);

			if (childSections.openQuestions.length > 0) {
				let taskNum = 1;
				for (const q of childSections.openQuestions) {
					taskLines.push(`- [ ] ${groupNum}.${taskNum} ${q}`);
					taskNum++;
				}
			} else if (childSections.decisions.length > 0) {
				let taskNum = 1;
				for (const d of childSections.decisions) {
					taskLines.push(`- [ ] ${groupNum}.${taskNum} Implement: ${d.title}`);
					taskNum++;
				}
			} else {
				taskLines.push(`- [ ] ${groupNum}.1 Implement ${child.title}`);
			}
			taskLines.push("");
			groupNum++;
		}
	} else if (sections.implementationNotes.fileScope.length > 0) {
		// ── File-scope-driven groups (preferred over bare decisions) ─
		// Each file in impl_notes becomes a task group.  Constraints that
		// mention the file's basename are attached to that group; any
		// remaining constraints land in a final "Cross-cutting" group.
		const constraints = sections.implementationNotes.constraints;
		const usedConstraints = new Set<number>();

		let groupNum = 1;
		for (const f of sections.implementationNotes.fileScope) {
			const baseName = f.path.split("/").pop() ?? f.path;
			const actionTag = f.action ? ` (${f.action})` : "";
			taskLines.push(`## ${groupNum}. ${f.path}${actionTag}`, "");
			taskLines.push(`- [ ] ${groupNum}.1 ${f.description}`);

			// Attach constraints that reference this file by basename or path
			let taskNum = 2;
			for (let i = 0; i < constraints.length; i++) {
				if (
					!usedConstraints.has(i) &&
					(constraints[i].toLowerCase().includes(baseName.toLowerCase()) ||
						constraints[i].toLowerCase().includes(f.path.toLowerCase()))
				) {
					taskLines.push(`- [ ] ${groupNum}.${taskNum} ${constraints[i]}`);
					usedConstraints.add(i);
					taskNum++;
				}
			}

			// Attach research entries whose heading references this file
			for (const r of sections.research) {
				if (
					r.heading.toLowerCase().includes(baseName.toLowerCase()) ||
					r.heading.toLowerCase().includes(f.path.toLowerCase())
				) {
					const firstLine = r.content.split("\n").find((l) => l.trim()) ?? "";
					if (firstLine) {
						taskLines.push(`- [ ] ${groupNum}.${taskNum} ${firstLine.replace(/^[-*]\s*/, "")}`);
						taskNum++;
					}
				}
			}

			taskLines.push("");
			groupNum++;
		}

		// Emit any constraints not matched to a specific file
		const orphanConstraints = constraints.filter((_, i) => !usedConstraints.has(i));
		if (orphanConstraints.length > 0) {
			taskLines.push(`## ${groupNum}. Cross-cutting constraints`, "");
			let taskNum = 1;
			for (const c of orphanConstraints) {
				taskLines.push(`- [ ] ${groupNum}.${taskNum} ${c}`);
				taskNum++;
			}
			taskLines.push("");
		}
	} else if (sections.decisions.length > 0) {
		// ── Decision-driven groups (fallback when no file scope) ──────
		let groupNum = 1;
		for (const d of sections.decisions) {
			taskLines.push(`## ${groupNum}. ${d.title}`, "");
			taskLines.push(`- [ ] ${groupNum}.1 Implement ${d.title}`);
			taskLines.push("");
			groupNum++;
		}
	} else {
		taskLines.push(`## 1. ${node.title}`, "");
		taskLines.push(`- [ ] 1.1 Implement ${node.title}`);
		taskLines.push("");
	}

	const tasksContent = taskLines.join("\n");
	const tasksPath = path.join(changePath, "tasks.md");
	fs.writeFileSync(tasksPath, tasksContent);
	files.push("tasks.md");

	// Surface the generated tasks.md content so the agent is forced to read
	// and refine it before proceeding — do not skip this review.
	const message =
		`Scaffolded OpenSpec change at ${changePath}\n\n` +
		`Files created:\n${files.map((f) => `  - ${f}`).join("\n")}\n\n` +
		`⚠️  REVIEW REQUIRED — tasks.md draft (read before proceeding):\n` +
		`${"─".repeat(60)}\n` +
		`${tasksContent}\n` +
		`${"─".repeat(60)}\n\n` +
		`The tasks above are a scaffold, not a final plan. Before running /cleave:\n` +
		`  1. Verify every file in impl_notes has at least one concrete subtask\n` +
		`  2. Check that each constraint appears in at least one task\n` +
		`  3. Expand any one-liner tasks that need numbered subtasks\n` +
		`  4. Add spec domain annotations if specs exist (<!-- specs: domain/name -->)\n\n` +
		`When satisfied:\n` +
		`  - Run \`/cleave\` to parallelize execution via git worktrees\n` +
		`  - After implementation, run \`/assess spec ${node.id}\` to verify against specs`;

	return { message, changePath, files };
}

// ─── Design-phase OpenSpec Scaffolding ──────────────────────────────────────

/**
 * Scaffold a design-phase OpenSpec change at openspec/design/<node-id>/.
 * Called on set_status(exploring) transition — idempotent (returns early if
 * the directory already has files).
 *
 * Generated files:
 *   proposal.md — one-liner intent + link to design doc
 *   spec.md     — template with Scenarios / Falsifiability / Constraints subsections
 *   tasks.md    — Open Questions mirrored as unchecked tasks
 */
export function scaffoldDesignOpenSpecChange(
	cwd: string,
	node: DesignNode,
): { message: string; changePath: string; created: boolean } {
	const changePath = path.join(cwd, "openspec", "design", node.id);

	// Idempotent: if directory already has markdown files, skip
	if (fs.existsSync(changePath)) {
		const existing = fs.readdirSync(changePath).filter((f) => f.endsWith(".md"));
		if (existing.length > 0) {
			return {
				message: `Design OpenSpec change already exists at openspec/design/${node.id}/ — skipping scaffold.`,
				changePath,
				created: false,
			};
		}
	}

	fs.mkdirSync(changePath, { recursive: true });

	// C4: guard against missing file (e.g. freshly-created node not yet flushed)
	let sections: DocumentSections;
	try {
		sections = getNodeSections(node);
	} catch {
		// Fall back to empty sections so scaffold can still proceed
		sections = {
			overview: "",
			research: [],
			decisions: [],
			openQuestions: node.open_questions ?? [],
			implementationNotes: { fileScope: [], constraints: [], rawContent: "" },
			acceptanceCriteria: { scenarios: [], falsifiability: [], constraints: [] },
			extraSections: [],
		};
	}
	const docRelPath = `docs/${node.id}.md`;

	// ── proposal.md ──────────────────────────────────────────────
	const intentLine = sections.overview
		? sections.overview.split("\n").find((l) => l.trim()) ?? sections.overview
		: `Explore and decide the design of: ${node.title}`;

	const proposal = [
		`# ${node.title}`,
		"",
		"## Intent",
		"",
		intentLine,
		"",
		`See [${node.title} design doc](../../../${docRelPath}) for full context.`,
		"",
	].join("\n");

	fs.writeFileSync(path.join(changePath, "proposal.md"), proposal);

	// ── spec.md ───────────────────────────────────────────────────
	const spec = [
		`# ${node.title} — Design Spec`,
		"",
		"> This spec defines acceptance criteria for the design phase.",
		"> Add Given/When/Then scenarios that must be true before marking this node 'decided'.",
		"",
		"## Scenarios",
		"",
		"### Scenario 1 (replace with a real scenario)",
		"",
		"Given this node is in the exploring state",
		"When the design questions are answered and a decision is recorded",
		"Then the node can be transitioned to decided",
		"",
		"## Falsifiability",
		"",
		"<!-- What would disprove this design? List concrete failure conditions. -->",
		"",
		"## Constraints",
		"",
		"<!-- Non-negotiable constraints this design must satisfy. -->",
		"",
	].join("\n");

	fs.writeFileSync(path.join(changePath, "spec.md"), spec);

	// ── tasks.md ──────────────────────────────────────────────────
	const tasks = buildDesignTasksContent(node, sections);
	fs.writeFileSync(path.join(changePath, "tasks.md"), tasks);

	return {
		message: `Scaffolded design OpenSpec change at openspec/design/${node.id}/ (proposal.md, spec.md, tasks.md).`,
		changePath,
		created: true,
	};
}

/**
 * Build tasks.md content from a node's Open Questions.
 * Used both during initial scaffold and for mirroring on question mutations.
 */
export function buildDesignTasksContent(node: DesignNode, sections: DocumentSections): string {
	const lines = [`# ${node.title} — Design Tasks`, ""];

	if (sections.openQuestions.length === 0) {
		lines.push("## 1. Design exploration", "");
		lines.push(`- [ ] 1.1 Explore and decide: ${node.title}`);
		lines.push("");
	} else {
		lines.push("## 1. Open Questions", "");
		let i = 1;
		for (const q of sections.openQuestions) {
			lines.push(`- [ ] 1.${i} ${q}`);
			i++;
		}
		lines.push("");
	}

	return lines.join("\n");
}

/**
 * Mirror the node's Open Questions to tasks.md in the design OpenSpec change
 * directory (openspec/design/<node-id>/tasks.md), if that directory exists.
 * Idempotent — overwrites tasks.md on every call.
 */
export function mirrorOpenQuestionsToDesignSpec(cwd: string, node: DesignNode): void {
	const tasksPath = path.join(cwd, "openspec", "design", node.id, "tasks.md");
	if (!fs.existsSync(tasksPath)) return;

	// W1: use node.open_questions directly — avoids redundant disk read and
	// potential race if the file write from add/removeOpenQuestion hasn't flushed.
	const syntheticSections: Pick<DocumentSections, "openQuestions"> = {
		openQuestions: node.open_questions ?? [],
	};
	const content = buildDesignTasksContent(node, syntheticSections as DocumentSections);
	fs.writeFileSync(tasksPath, content);
}

/**
 * Extract a design-spec artifact from the doc's structured content.
 * Deterministic: no LLM pass, no placeholders. If the doc has thin content,
 * the extracted spec reflects that honestly.
 *
 * Creates openspec/design/{id}/ with proposal.md, spec.md, and archives it
 * immediately to openspec/design-archive/{date}-{id}/.
 *
 * Returns { created, archived, message }.
 */
export function extractAndArchiveDesignSpec(
	cwd: string,
	node: DesignNode,
): { created: boolean; archived: boolean; message: string } {
	const designDir = path.join(cwd, "openspec", "design", node.id);
	const archiveBaseDir = path.join(cwd, "openspec", "design-archive");

	// Already archived? Nothing to do.
	if (fs.existsSync(archiveBaseDir)) {
		for (const entry of fs.readdirSync(archiveBaseDir, { withFileTypes: true })) {
			if (!entry.isDirectory()) continue;
			const match = entry.name.match(/^\d{4}-\d{2}-\d{2}-(.+)$/);
			if (match && match[1] === node.id) {
				return { created: false, archived: true, message: "Design spec already archived" };
			}
		}
	}

	let sections: DocumentSections;
	try {
		sections = getNodeSections(node);
	} catch {
		sections = {
			overview: "",
			research: [],
			decisions: [],
			openQuestions: node.open_questions ?? [],
			implementationNotes: { fileScope: [], constraints: [], rawContent: "" },
			acceptanceCriteria: { scenarios: [], falsifiability: [], constraints: [] },
			extraSections: [],
		};
	}

	// ── Build spec content from doc sections ──────────────────────
	const specLines: string[] = [
		`# ${node.title} — Design Spec (extracted)`,
		"",
		`> Auto-extracted from docs/${node.id}.md at decide-time.`,
		"",
	];

	if (sections.decisions.length > 0) {
		specLines.push("## Decisions", "");
		for (const d of sections.decisions) {
			specLines.push(`### ${d.title} (${d.status})`);
			if (d.rationale) specLines.push("", d.rationale);
			specLines.push("");
		}
	}

	const ac = sections.acceptanceCriteria;
	if (ac.scenarios.length > 0 || ac.falsifiability.length > 0 || ac.constraints.length > 0) {
		specLines.push("## Acceptance Criteria", "");
		if (ac.scenarios.length > 0) {
			specLines.push("### Scenarios", "");
			for (const s of ac.scenarios) specLines.push(`- ${s}`);
			specLines.push("");
		}
		if (ac.falsifiability.length > 0) {
			specLines.push("### Falsifiability", "");
			for (const f of ac.falsifiability) specLines.push(`- ${f}`);
			specLines.push("");
		}
		if (ac.constraints.length > 0) {
			specLines.push("### Constraints", "");
			for (const c of ac.constraints) specLines.push(`- ${c}`);
			specLines.push("");
		}
	}

	if (sections.research.length > 0) {
		specLines.push("## Research Summary", "");
		for (const r of sections.research) {
			specLines.push(`### ${r.heading}`, "", r.content.slice(0, 500) + (r.content.length > 500 ? "…" : ""), "");
		}
	}

	// ── Write to openspec/design/{id}/ ──────────────────────────
	fs.mkdirSync(designDir, { recursive: true });
	fs.writeFileSync(
		path.join(designDir, "proposal.md"),
		`# ${node.title}\n\n## Intent\n\n${sections.overview?.split("\n").find(l => l.trim()) ?? node.title}\n\nSee [design doc](../../../docs/${node.id}.md).\n`,
	);
	fs.writeFileSync(path.join(designDir, "spec.md"), specLines.join("\n"));

	// ── Immediately archive ─────────────────────────────────────
	const today = new Date().toISOString().split("T")[0];
	const archiveDir = path.join(archiveBaseDir, `${today}-${node.id}`);
	fs.mkdirSync(archiveDir, { recursive: true });

	for (const file of fs.readdirSync(designDir)) {
		fs.copyFileSync(path.join(designDir, file), path.join(archiveDir, file));
	}
	fs.rmSync(designDir, { recursive: true, force: true });

	return {
		created: true,
		archived: true,
		message: `Extracted design spec from docs/${node.id}.md (${sections.decisions.length} decisions, ${sections.research.length} research sections) → archived to openspec/design-archive/${today}-${node.id}/`,
	};
}

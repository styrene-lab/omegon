/**
 * design-tree/tree — Pure domain logic for design tree operations.
 *
 * No pi dependency — can be tested standalone. All functions operate
 * on the filesystem and return plain data structures.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import type {
	DesignNode,
	DesignTree,
	DesignDecision,
	DocumentSections,
	FileScope,
	NodeStatus,
	ResearchEntry,
} from "./types.ts";
import { VALID_STATUSES, SECTION_HEADINGS } from "./types.ts";

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
				result[key] = value.replace(/^["'](.*)["']$/, "$1");
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

	const files = fs.readdirSync(docsDir).filter((f) => f.endsWith(".md"));

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
	};

	const sections: DocumentSections = {
		overview: opts.overview || "*To be explored.*",
		research: [],
		decisions: [],
		openQuestions: [],
		implementationNotes: { fileScope: [], constraints: [], rawContent: "" },
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
	} else if (sections.decisions.length > 0) {
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

	const tasksPath = path.join(changePath, "tasks.md");
	fs.writeFileSync(tasksPath, taskLines.join("\n"));
	files.push("tasks.md");

	const message =
		`Scaffolded OpenSpec change at ${changePath}\n\n` +
		`Files created:\n${files.map((f) => `  - ${f}`).join("\n")}\n\n` +
		`Next steps:\n` +
		`  1. Review and refine tasks.md (add specific subtasks, adjust grouping)\n` +
		`  2. Run \`/cleave\` to parallelize execution via git worktrees\n` +
		`  3. After implementation, run \`/assess spec ${node.id}\` to verify against specs`;

	return { message, changePath, files };
}

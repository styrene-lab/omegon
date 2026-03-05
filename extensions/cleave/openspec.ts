/**
 * cleave/openspec — OpenSpec tasks.md parser.
 *
 * Parses OpenSpec's tasks.md format into ChildPlan[] for cleave execution.
 * OpenSpec tasks.md uses numbered, grouped tasks with checkboxes:
 *
 *   ## 1. Theme Infrastructure
 *   - [ ] 1.1 Create ThemeContext with light/dark state
 *   - [ ] 1.2 Add CSS custom properties for colors
 *
 *   ## 2. UI Components
 *   - [ ] 2.1 Create ThemeToggle component
 *   - [ ] 2.2 Add toggle to settings page
 *
 * Each top-level group (## N. Title) becomes a ChildPlan.
 * Subtasks within a group become the scope/description.
 * Group ordering defines dependencies (later groups may depend on earlier).
 */

import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, basename } from "node:path";
import type { ChildPlan, SplitPlan } from "./types.js";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface OpenSpecChange {
	/** Change directory name (e.g., "add-dark-mode") */
	name: string;
	/** Full path to the change directory */
	path: string;
	/** Whether tasks.md exists */
	hasTasks: boolean;
	/** Whether proposal.md exists */
	hasProposal: boolean;
	/** Whether design.md exists */
	hasDesign: boolean;
}

export interface TaskGroup {
	/** Group number (1-based) */
	number: number;
	/** Group title (e.g., "Theme Infrastructure") */
	title: string;
	/** Individual tasks within the group */
	tasks: Array<{
		id: string;      // e.g., "1.1"
		text: string;    // e.g., "Create ThemeContext with light/dark state"
		done: boolean;   // checkbox state
	}>;
}

/**
 * Rich context extracted from an OpenSpec change, beyond just tasks.
 * Carries design decisions, file scope, and spec scenarios for
 * child enrichment and post-merge verification.
 */
export interface OpenSpecContext {
	/** The change directory path */
	changePath: string;
	/** Full design.md content (null if absent) */
	designContent: string | null;
	/** Architecture decisions extracted from design.md */
	decisions: string[];
	/** Explicit file changes from design.md "File Changes" section */
	fileChanges: Array<{ path: string; action: "new" | "modified" | "deleted" | "unknown" }>;
	/** Delta spec scenarios for post-merge verification */
	specScenarios: Array<{ domain: string; requirement: string; scenarios: string[] }>;
}

// ─── Detection ──────────────────────────────────────────────────────────────

/**
 * Detect whether an OpenSpec workspace exists in the given repo.
 * Returns the path to openspec/ if found, null otherwise.
 */
export function detectOpenSpec(repoPath: string): string | null {
	const openspecDir = join(repoPath, "openspec");
	if (existsSync(openspecDir)) return openspecDir;
	return null;
}

/**
 * List active (non-archived) OpenSpec changes.
 */
export function listChanges(openspecDir: string): OpenSpecChange[] {
	const changesDir = join(openspecDir, "changes");
	if (!existsSync(changesDir)) return [];

	const entries = readdirSync(changesDir, { withFileTypes: true });
	const changes: OpenSpecChange[] = [];

	for (const entry of entries) {
		if (!entry.isDirectory() || entry.name === "archive") continue;

		const changePath = join(changesDir, entry.name);
		changes.push({
			name: entry.name,
			path: changePath,
			hasTasks: existsSync(join(changePath, "tasks.md")),
			hasProposal: existsSync(join(changePath, "proposal.md")),
			hasDesign: existsSync(join(changePath, "design.md")),
		});
	}

	return changes;
}

/**
 * Find changes that have tasks.md ready for execution.
 */
export function findExecutableChanges(openspecDir: string): OpenSpecChange[] {
	return listChanges(openspecDir).filter((c) => c.hasTasks);
}

// ─── Parsing ────────────────────────────────────────────────────────────────

/**
 * Parse an OpenSpec tasks.md into task groups.
 *
 * Supports formats:
 *   ## 1. Group Title
 *   - [ ] 1.1 Task description
 *   - [x] 1.2 Completed task
 *
 * Also handles unnumbered groups:
 *   ## Group Title
 *   - [ ] Task description
 */
export function parseTasksFile(content: string): TaskGroup[] {
	const groups: TaskGroup[] = [];
	let currentGroup: TaskGroup | null = null;

	const lines = content.split("\n");

	for (const line of lines) {
		// Match group header: ## 1. Title or ## Title
		const groupMatch = line.match(/^##\s+(?:(\d+)\.\s+)?(.+)$/);
		if (groupMatch) {
			if (currentGroup) groups.push(currentGroup);
			currentGroup = {
				number: groupMatch[1] ? parseInt(groupMatch[1], 10) : groups.length + 1,
				title: groupMatch[2].trim(),
				tasks: [],
			};
			continue;
		}

		// Match task item: - [ ] 1.1 Description or - [x] 1.2 Description
		const taskMatch = line.match(/^\s*-\s+\[([ xX])\]\s+(?:(\d+(?:\.\d+)?)\s+)?(.+)$/);
		if (taskMatch && currentGroup) {
			currentGroup.tasks.push({
				id: taskMatch[2] || `${currentGroup.number}.${currentGroup.tasks.length + 1}`,
				text: taskMatch[3].trim(),
				done: taskMatch[1] !== " ",
			});
			continue;
		}

		// Match unnumbered bullet task under a group: - Task text (no checkbox)
		const bulletMatch = line.match(/^\s*-\s+(?!\[)(.+)$/);
		if (bulletMatch && currentGroup) {
			currentGroup.tasks.push({
				id: `${currentGroup.number}.${currentGroup.tasks.length + 1}`,
				text: bulletMatch[1].trim(),
				done: false,
			});
		}
	}

	if (currentGroup) groups.push(currentGroup);
	return groups;
}

// ─── Conversion ─────────────────────────────────────────────────────────────

/**
 * Convert OpenSpec task groups to cleave ChildPlan[].
 *
 * Each group becomes a child. Dependencies are inferred from:
 * - Explicit markers in title: "after X", "requires X", "depends on X"
 * - Task text references to earlier group titles
 *
 * Groups where ALL tasks are already done are filtered out.
 *
 * Returns null if fewer than 2 executable groups (not worth cleaving).
 */
export function taskGroupsToChildPlans(groups: TaskGroup[]): ChildPlan[] | null {
	// Filter out groups where all tasks are done
	const activeGroups = groups.filter((g) =>
		g.tasks.length === 0 || g.tasks.some((t) => !t.done),
	);

	if (activeGroups.length < 2) return null;

	// Cap at 4 children (cleave limit)
	const effectiveGroups = activeGroups.length > 4 ? mergeSmallGroups(activeGroups, 4) : activeGroups;

	const plans: ChildPlan[] = effectiveGroups.map((group) => {
		const label = group.title
			.toLowerCase()
			.replace(/[^\w\s-]/g, "")
			.replace(/[\s_]+/g, "-")
			.replace(/-+/g, "-")
			.replace(/^-|-$/g, "")
			.slice(0, 40);

		const taskDescriptions = group.tasks
			.filter((t) => !t.done) // Skip already-completed tasks
			.map((t) => `- ${t.text}`);

		const description = taskDescriptions.length > 0
			? `${group.title}:\n${taskDescriptions.join("\n")}`
			: group.title;

		// Infer scope from task text: look for file paths and patterns
		const scope = inferScope(group.tasks.map((t) => t.text));

		return {
			label,
			description,
			scope,
			dependsOn: [] as string[],
		};
	});

	// Infer dependencies from explicit markers and title references
	inferDependencies(plans);

	return plans;
}

/**
 * Infer inter-group dependencies from explicit markers in descriptions.
 *
 * Looks for patterns like:
 * - "after <label>" or "after <title words>"
 * - "requires <label>"
 * - "depends on <label>"
 * - Task text referencing an earlier group's title
 */
function inferDependencies(plans: ChildPlan[]): void {
	const labelSet = new Set(plans.map((p) => p.label));

	for (let i = 0; i < plans.length; i++) {
		const text = plans[i].description.toLowerCase();

		for (let j = 0; j < plans.length; j++) {
			if (i === j) continue;
			const otherLabel = plans[j].label;
			// Convert label back to words for fuzzy matching: "database-layer" → "database layer"
			const otherWords = otherLabel.replace(/-/g, " ");

			// Explicit markers: "after X", "requires X", "depends on X"
			const markers = [
				`after ${otherLabel}`, `after ${otherWords}`,
				`requires ${otherLabel}`, `requires ${otherWords}`,
				`depends on ${otherLabel}`, `depends on ${otherWords}`,
			];

			if (markers.some((m) => text.includes(m))) {
				if (labelSet.has(otherLabel) && !plans[i].dependsOn.includes(otherLabel)) {
					plans[i].dependsOn.push(otherLabel);
				}
			}
		}
	}
}

/**
 * Full pipeline: read an OpenSpec change and convert to SplitPlan.
 *
 * Returns null if the change doesn't have tasks or has fewer than 2 groups.
 */
export function openspecChangeToSplitPlan(changePath: string): SplitPlan | null {
	const tasksPath = join(changePath, "tasks.md");
	if (!existsSync(tasksPath)) return null;

	const content = readFileSync(tasksPath, "utf-8");
	const groups = parseTasksFile(content);
	const children = taskGroupsToChildPlans(groups);
	if (!children) return null;

	// Read proposal for rationale if available
	let rationale = `From OpenSpec change: ${basename(changePath)}`;
	const proposalPath = join(changePath, "proposal.md");
	if (existsSync(proposalPath)) {
		const proposal = readFileSync(proposalPath, "utf-8");
		// Extract intent section
		const intentMatch = proposal.match(/##\s+Intent\s*\n([\s\S]*?)(?=\n##|$)/);
		if (intentMatch) {
			rationale = intentMatch[1].trim().slice(0, 200);
		}
	}

	return { children, rationale };
}

// ─── Design Context ─────────────────────────────────────────────────────────

/**
 * Parse the "File Changes" section from design.md.
 *
 * Supports formats:
 *   - `src/contexts/ThemeContext.tsx` (new)
 *   - `src/styles/globals.css` (modified)
 *   - src/old/file.ts (deleted)
 *   - `path/to/file.ts`  (no action → unknown)
 */
export function parseDesignFileChanges(
	designContent: string,
): Array<{ path: string; action: "new" | "modified" | "deleted" | "unknown" }> {
	const results: Array<{ path: string; action: "new" | "modified" | "deleted" | "unknown" }> = [];

	// Find the File Changes section
	const sectionMatch = designContent.match(
		/##\s+File\s+Changes?\s*\n([\s\S]*?)(?=\n##\s|\n#\s|$)/i,
	);
	if (!sectionMatch) return results;

	const section = sectionMatch[1];
	// Match lines like: - `path/to/file` (action)  or  - path/to/file (action)
	const lineRe = /^[\s-]*[`"']?([a-zA-Z0-9_./-]+\.[a-zA-Z0-9]+)[`"']?\s*(?:\((\w+)\))?/gm;
	let m: RegExpExecArray | null;
	while ((m = lineRe.exec(section)) !== null) {
		const filePath = m[1];
		const rawAction = (m[2] || "").toLowerCase();
		let action: "new" | "modified" | "deleted" | "unknown" = "unknown";
		if (rawAction === "new" || rawAction === "created" || rawAction === "create") action = "new";
		else if (rawAction === "modified" || rawAction === "updated" || rawAction === "modify") action = "modified";
		else if (rawAction === "deleted" || rawAction === "removed" || rawAction === "delete") action = "deleted";
		results.push({ path: filePath, action });
	}

	return results;
}

/**
 * Extract architecture decisions from design.md.
 *
 * Looks for "### Decision:" headers and captures the title + rationale.
 */
export function parseDesignDecisions(designContent: string): string[] {
	const decisions: string[] = [];
	const re = /###\s+Decision:\s*(.+?)(?:\n[\s\S]*?(?=\n###|\n##|$))/g;
	let m: RegExpExecArray | null;
	while ((m = re.exec(designContent)) !== null) {
		// Capture the decision title and first line of rationale
		const title = m[0];
		const lines = title.split("\n").filter((l) => l.trim());
		const summary = lines.length > 1
			? `${lines[0].replace(/^###\s+Decision:\s*/, "").trim()}: ${lines.slice(1).find((l) => !l.startsWith("#"))?.trim() || ""}`
			: lines[0].replace(/^###\s+Decision:\s*/, "").trim();
		decisions.push(summary);
	}
	return decisions;
}

// ─── Spec Scenarios ─────────────────────────────────────────────────────────

/**
 * Read delta spec files from a change and extract scenarios for verification.
 *
 * Parses Given/When/Then scenarios from ADDED and MODIFIED requirements
 * in the change's specs/ directory.
 */
export function readSpecScenarios(
	changePath: string,
): Array<{ domain: string; requirement: string; scenarios: string[] }> {
	const specsDir = join(changePath, "specs");
	if (!existsSync(specsDir)) return [];

	const results: Array<{ domain: string; requirement: string; scenarios: string[] }> = [];

	// Recursively find spec.md files
	const specFiles = findSpecFiles(specsDir);

	for (const specFile of specFiles) {
		const content = readFileSync(specFile, "utf-8");
		const domain = specFile
			.replace(specsDir + "/", "")
			.replace(/\/spec\.md$/, "")
			.replace(/\.md$/, "");

		// Only extract from ADDED and MODIFIED sections (these need verification)
		const relevantSections = content.match(
			/##\s+(?:ADDED|MODIFIED)\s+Requirements?\s*\n([\s\S]*?)(?=\n##\s+(?:ADDED|MODIFIED|REMOVED)|$)/gi,
		);
		if (!relevantSections) continue;

		for (const section of relevantSections) {
			// Find requirements with scenarios
			const reqRe = /###\s+Requirement:\s*(.+)/g;
			let reqMatch: RegExpExecArray | null;
			while ((reqMatch = reqRe.exec(section)) !== null) {
				const reqName = reqMatch[1].trim();
				// Find scenarios after this requirement until next requirement or section end
				const afterReq = section.slice(reqMatch.index + reqMatch[0].length);
				const nextReq = afterReq.search(/\n###\s+Requirement:/);
				const scenarioBlock = nextReq >= 0 ? afterReq.slice(0, nextReq) : afterReq;

				const scenarios: string[] = [];
				const scenarioRe = /####\s+Scenario:\s*(.+?)(?:\n[\s\S]*?)(?=\n####|\n###|$)/g;
				let scenMatch: RegExpExecArray | null;
				while ((scenMatch = scenarioRe.exec(scenarioBlock)) !== null) {
					// Extract the full scenario including Given/When/Then
					const scenarioText = scenMatch[0]
						.replace(/^####\s+Scenario:\s*/, "")
						.trim();
					scenarios.push(scenarioText);
				}

				if (scenarios.length > 0) {
					results.push({ domain, requirement: reqName, scenarios });
				}
			}
		}
	}

	return results;
}

/** Recursively find spec.md files under a directory. */
function findSpecFiles(dir: string): string[] {
	const files: string[] = [];
	if (!existsSync(dir)) return files;

	const entries = readdirSync(dir, { withFileTypes: true });
	for (const entry of entries) {
		const fullPath = join(dir, entry.name);
		if (entry.isDirectory()) {
			files.push(...findSpecFiles(fullPath));
		} else if (entry.name.endsWith(".md")) {
			files.push(fullPath);
		}
	}
	return files;
}

// ─── Full Context ───────────────────────────────────────────────────────────

/**
 * Build full OpenSpec context from a change directory.
 *
 * Reads design.md (decisions, file changes), delta specs (scenarios),
 * and returns a structured context object that cleave uses to:
 * - Enrich child task files with design context
 * - Supply exact file scope from design file changes
 * - Verify implementation against spec scenarios post-merge
 */
export function buildOpenSpecContext(changePath: string): OpenSpecContext {
	const ctx: OpenSpecContext = {
		changePath,
		designContent: null,
		decisions: [],
		fileChanges: [],
		specScenarios: [],
	};

	// Design
	const designPath = join(changePath, "design.md");
	if (existsSync(designPath)) {
		ctx.designContent = readFileSync(designPath, "utf-8");
		ctx.decisions = parseDesignDecisions(ctx.designContent);
		ctx.fileChanges = parseDesignFileChanges(ctx.designContent);
	}

	// Specs
	ctx.specScenarios = readSpecScenarios(changePath);

	return ctx;
}

/**
 * Full pipeline: read an OpenSpec change and convert to SplitPlan + context.
 *
 * Returns null if the change doesn't have tasks or has fewer than 2 groups.
 */
export function openspecChangeToSplitPlanWithContext(
	changePath: string,
): { plan: SplitPlan; context: OpenSpecContext } | null {
	const plan = openspecChangeToSplitPlan(changePath);
	if (!plan) return null;

	const context = buildOpenSpecContext(changePath);

	// Supplement scope from design.md file changes when available
	if (context.fileChanges.length > 0) {
		supplementScopeFromDesign(plan.children, context.fileChanges);
	}

	return { plan, context };
}

/**
 * Supplement child scope with explicit file changes from design.md.
 *
 * For each child, if design.md lists files that match the child's description
 * or existing scope patterns, add them. This replaces heuristic guessing
 * with author-declared intent.
 */
function supplementScopeFromDesign(
	children: ChildPlan[],
	fileChanges: Array<{ path: string; action: string }>,
): void {
	// If there's only one group of files, distribute to the closest-matching child
	// If files are clearly separated by directory, match by path prefix

	const filePaths = fileChanges
		.filter((f) => f.action !== "deleted")
		.map((f) => f.path);

	if (filePaths.length === 0) return;

	for (const child of children) {
		const descLower = child.description.toLowerCase();
		const labelWords = child.label.replace(/-/g, " ").split(" ");

		const matched: string[] = [];
		for (const fp of filePaths) {
			const fpLower = fp.toLowerCase();
			// Match if: file path contains a label word, or child description mentions the file
			const pathParts = fpLower.split("/");
			const isMatch =
				labelWords.some((w) => w.length > 2 && pathParts.some((p) => p.includes(w))) ||
				descLower.includes(fpLower) ||
				child.scope.some((s) => {
					const pattern = s.replace(/\*\*/g, "").replace(/\*/g, "");
					return fpLower.startsWith(pattern) || pattern.startsWith(fpLower.split("/").slice(0, -1).join("/"));
				});

			if (isMatch) matched.push(fp);
		}

		// Add matched files to scope (deduplicated)
		const existingScope = new Set(child.scope);
		for (const fp of matched) {
			if (!existingScope.has(fp)) {
				child.scope.push(fp);
			}
		}
	}
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/**
 * Merge small groups to fit within maxGroups.
 * Combines the smallest adjacent groups until we're at the limit.
 */
function mergeSmallGroups(groups: TaskGroup[], maxGroups: number): TaskGroup[] {
	const result = [...groups];

	while (result.length > maxGroups) {
		// Find the smallest group by task count
		let smallestIdx = 0;
		let smallestSize = Infinity;
		for (let i = 0; i < result.length - 1; i++) {
			const combined = result[i].tasks.length + result[i + 1].tasks.length;
			if (combined < smallestSize) {
				smallestSize = combined;
				smallestIdx = i;
			}
		}

		// Merge with next group
		const merged: TaskGroup = {
			number: result[smallestIdx].number,
			title: `${result[smallestIdx].title} + ${result[smallestIdx + 1].title}`,
			tasks: [...result[smallestIdx].tasks, ...result[smallestIdx + 1].tasks],
		};
		result.splice(smallestIdx, 2, merged);
	}

	return result;
}

// ─── Task Write-Back ────────────────────────────────────────────────────────

/**
 * After a successful cleave merge, mark completed child tasks as done
 * in the original OpenSpec tasks.md.
 *
 * Maps completed child labels back to task groups and checks off their
 * unchecked tasks. Returns the number of tasks marked done.
 */
export function writeBackTaskCompletion(
	changePath: string,
	completedLabels: string[],
): { updated: number; totalTasks: number; allDone: boolean } {
	const tasksPath = join(changePath, "tasks.md");
	if (!existsSync(tasksPath)) return { updated: 0, totalTasks: 0, allDone: false };

	const content = readFileSync(tasksPath, "utf-8");
	const groups = parseTasksFile(content);

	// Build a set of completed label slugs for matching
	const completedSet = new Set(completedLabels.map((l) => l.toLowerCase()));

	// Track which group numbers are completed
	const completedGroupNumbers = new Set<number>();
	for (const group of groups) {
		const groupSlug = group.title
			.toLowerCase()
			.replace(/[^\w\s-]/g, "")
			.replace(/[\s_]+/g, "-")
			.replace(/-+/g, "-")
			.replace(/^-|-$/g, "")
			.slice(0, 40);

		if (completedSet.has(groupSlug)) {
			completedGroupNumbers.add(group.number);
		}
	}

	if (completedGroupNumbers.size === 0) {
		const totalTasks = groups.reduce((sum, g) => sum + g.tasks.length, 0);
		return { updated: 0, totalTasks, allDone: false };
	}

	// Rewrite tasks.md line by line, checking off tasks in completed groups
	const lines = content.split("\n");
	let currentGroupNumber = -1;
	let updated = 0;

	for (let i = 0; i < lines.length; i++) {
		// Detect group header
		const groupMatch = lines[i].match(/^##\s+(?:(\d+)\.\s+)?(.+)$/);
		if (groupMatch) {
			currentGroupNumber = groupMatch[1] ? parseInt(groupMatch[1], 10) : -1;
			// If unnumbered, find by title match
			if (currentGroupNumber === -1) {
				const title = groupMatch[2].trim();
				const g = groups.find((g) => g.title === title);
				if (g) currentGroupNumber = g.number;
			}
			continue;
		}

		// Check off unchecked tasks in completed groups
		if (completedGroupNumbers.has(currentGroupNumber)) {
			const taskMatch = lines[i].match(/^(\s*-\s+)\[ \](\s+.*)$/);
			if (taskMatch) {
				lines[i] = `${taskMatch[1]}[x]${taskMatch[2]}`;
				updated++;
			}
		}
	}

	if (updated > 0) {
		writeFileSync(tasksPath, lines.join("\n"), "utf-8");
	}

	// Check if all tasks are now done
	const totalTasks = groups.reduce((sum, g) => sum + g.tasks.length, 0);
	const wasDone = groups.reduce((sum, g) => sum + g.tasks.filter((t) => t.done).length, 0);
	const allDone = wasDone + updated >= totalTasks;

	return { updated, totalTasks, allDone };
}

// ─── Active Changes Status ──────────────────────────────────────────────────

export interface ChangeStatus {
	name: string;
	path: string;
	totalTasks: number;
	doneTasks: number;
	hasProposal: boolean;
	hasDesign: boolean;
	hasSpecs: boolean;
}

/**
 * Summarize all active OpenSpec changes and their task completion status.
 *
 * Returns a list of changes with their task progress, suitable for
 * session-start status display.
 */
export function getActiveChangesStatus(repoPath: string): ChangeStatus[] {
	const openspecDir = detectOpenSpec(repoPath);
	if (!openspecDir) return [];

	const changes = listChanges(openspecDir);
	const result: ChangeStatus[] = [];

	for (const change of changes) {
		let totalTasks = 0;
		let doneTasks = 0;

		if (change.hasTasks) {
			const content = readFileSync(join(change.path, "tasks.md"), "utf-8");
			const groups = parseTasksFile(content);
			for (const group of groups) {
				totalTasks += group.tasks.length;
				doneTasks += group.tasks.filter((t) => t.done).length;
			}
		}

		const specsDir = join(change.path, "specs");
		result.push({
			name: change.name,
			path: change.path,
			totalTasks,
			doneTasks,
			hasProposal: change.hasProposal,
			hasDesign: change.hasDesign,
			hasSpecs: existsSync(specsDir),
		});
	}

	return result;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/**
 * Infer file scope patterns from task descriptions.
 * Looks for quoted paths, file extensions, and common patterns.
 */
function inferScope(taskTexts: string[]): string[] {
	const scope = new Set<string>();
	const combined = taskTexts.join("\n");

	// Backtick-quoted paths: `src/auth/login.ts`
	for (const m of combined.matchAll(/`([a-zA-Z0-9_./-]+\.[a-zA-Z0-9]+)`/g)) {
		scope.add(m[1]);
	}

	// Directory references: src/auth/, components/
	for (const m of combined.matchAll(/\b((?:src|lib|app|components|pages|api|tests?|spec)\/?[a-zA-Z0-9_/-]*)\b/g)) {
		const dir = m[1].replace(/\/$/, "");
		if (dir.includes("/")) {
			scope.add(dir + "/**");
		}
	}

	return [...scope].slice(0, 10); // Cap at 10 patterns
}

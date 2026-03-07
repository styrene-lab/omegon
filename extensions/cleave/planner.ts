/**
 * cleave/planner — Split planning via LLM.
 *
 * Builds a planning prompt and delegates to either:
 * - Local model (via ask_local_model) for faster/cheaper planning
 * - Cloud model (via sendUserMessage) for complex splits
 *
 * The planner analyzes the directive + repo structure and produces
 * a JSON split strategy with 2-4 children.
 */

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import type { ChildPlan, SplitPlan } from "./types.ts";

/**
 * Build the planning prompt.
 *
 * Adapted from styrene-lab/cleave orchestrator/prompt.py build_planner_prompt().
 */
export function buildPlannerPrompt(
	directive: string,
	repoTree: string,
	successCriteria: string[],
): string {
	const criteriaBlock = successCriteria.length > 0
		? "\n\nSuccess criteria:\n" + successCriteria.map((c) => `- ${c}`).join("\n")
		: "";

	return `Analyze this directive and produce a JSON split strategy for parallel execution.

Directive: ${directive}${criteriaBlock}

Repository structure (top-level):
${repoTree}

Respond with ONLY a JSON object (no markdown fences, no explanation) matching this schema:

{
  "children": [
    {
      "label": "short-kebab-case-label",
      "description": "What this child task should accomplish in detail",
      "scope": ["list", "of", "file/dir", "glob patterns", "this child owns"],
      "depends_on": ["label-of-child-that-must-finish-first"]
    }
  ],
  "rationale": "Brief explanation of why this decomposition makes sense"
}

Rules:
- 2-4 children
- Use depends_on only when one child's output feeds another. Omit or leave empty when children are independent.
- Children should have minimal file overlap
- Labels must be unique, kebab-case, max 40 chars
- Scope patterns use glob syntax (e.g., "src/auth/**", "tests/test_auth*")
- Each child must be completable with standard development tools`;
}

/**
 * Get the top-level repo tree for context (directories and key files).
 */
export async function getRepoTree(
	pi: ExtensionAPI,
	repoPath: string,
): Promise<string> {
	const result = await pi.exec(
		"find",
		[repoPath, "-maxdepth", "2", "-not", "-path", "*/.*", "-not", "-path", "*/node_modules/*",
		 "-not", "-path", "*/__pycache__/*", "-not", "-path", "*/dist/*"],
		{ timeout: 5_000 },
	);

	if (result.code !== 0) return "(unable to list directory)";

	const lines = result.stdout
		.split("\n")
		.filter(Boolean)
		.map((p) => {
			const rel = p.replace(repoPath + "/", "").replace(repoPath, ".");
			return rel;
		})
		.slice(0, 50);

	return lines.join("\n");
}

/**
 * Parse the planner's JSON response, handling common issues.
 */
export function parsePlanResponse(response: string): SplitPlan {
	let text = response.trim();

	// Strip markdown code fences
	if (text.startsWith("```")) {
		const lines = text.split("\n");
		lines.shift(); // remove opening fence
		if (lines.length > 0 && lines[lines.length - 1].trim() === "```") {
			lines.pop();
		}
		text = lines.join("\n").trim();
	}

	let data: any;
	try {
		data = JSON.parse(text);
	} catch {
		// Try to extract JSON object from surrounding text
		const start = text.indexOf("{");
		const end = text.lastIndexOf("}");
		if (start >= 0 && end > start) {
			try {
				data = JSON.parse(text.slice(start, end + 1));
			} catch {
				throw new Error(`Could not parse planner response as JSON:\n${text.slice(0, 500)}`);
			}
		} else {
			throw new Error(`No JSON found in planner response:\n${text.slice(0, 500)}`);
		}
	}

	if (!data || typeof data !== "object") {
		throw new Error("Planner response is not a JSON object");
	}

	const children: ChildPlan[] = data.children;
	if (!Array.isArray(children) || children.length < 2) {
		throw new Error(`Planner must produce at least 2 children, got ${children?.length ?? 0}`);
	}

	// Truncate to 4 max
	if (children.length > 4) children.length = 4;

	// Normalize and validate
	const allLabels = new Set<string>();
	for (const child of children) {
		if (!child.label || !child.description) {
			throw new Error("Each child must have 'label' and 'description'");
		}
		// Normalize label
		child.label = child.label
			.toLowerCase()
			.replace(/[\s_]+/g, "-")
			.replace(/[^a-z0-9-]/g, "")
			.slice(0, 40);

		child.scope = child.scope ?? [];
		child.specDomains = child.specDomains ?? [];
		child.skills = child.skills ?? [];
		// Accept both camelCase and snake_case from LLM output
		const rawDeps = child.dependsOn ?? (child as any).depends_on ?? [];
		child.dependsOn = Array.isArray(rawDeps) ? rawDeps.map(String).filter(Boolean) : [];
		delete (child as any).depends_on;

		allLabels.add(child.label);
	}

	// Validate and prune dependencies
	for (const child of children) {
		// Remove self-dependencies
		child.dependsOn = child.dependsOn.filter((d) => d !== child.label);
		// Remove unknown references
		child.dependsOn = child.dependsOn.filter((d) => allLabels.has(d));
	}

	// Cycle detection via Kahn's algorithm
	detectAndBreakCycles(children);

	return {
		children,
		rationale: data.rationale || "",
	};
}

/**
 * Detect dependency cycles via Kahn's algorithm and clear deps on cyclic nodes.
 *
 * Only modifies children if an actual cycle is detected — non-cyclic
 * dependency chains are preserved.
 */
function detectAndBreakCycles(children: ChildPlan[]): void {
	const allLabels = new Set(children.map((c) => c.label));

	// Compute in-degree: how many deps does each node have?
	const inDegree = new Map<string, number>();
	for (const c of children) {
		// Only count deps that reference known labels
		const validDeps = c.dependsOn.filter((d) => allLabels.has(d));
		inDegree.set(c.label, validDeps.length);
	}

	// Start with nodes that have no dependencies (in-degree 0)
	const queue = [...inDegree.entries()]
		.filter(([, deg]) => deg === 0)
		.map(([label]) => label);
	const visited = new Set<string>();

	while (queue.length > 0) {
		const node = queue.shift()!;
		visited.add(node);
		// For each child that depends on this node, decrement its in-degree
		for (const child of children) {
			if (child.dependsOn.includes(node)) {
				const newDeg = (inDegree.get(child.label) ?? 1) - 1;
				inDegree.set(child.label, newDeg);
				if (newDeg === 0) {
					queue.push(child.label);
				}
			}
		}
	}

	// Nodes not visited are in a cycle
	const cyclic = new Set([...allLabels].filter((l) => !visited.has(l)));
	if (cyclic.size > 0) {
		for (const child of children) {
			if (cyclic.has(child.label)) {
				child.dependsOn = [];
			}
		}
	}
}

/**
 * Compute dispatch waves from children with dependency ordering.
 *
 * Children with no unmet deps go in wave 0, children whose deps
 * are all in earlier waves go in the next wave, etc.
 */
export function computeDispatchWaves(children: Array<{ label: string; dependsOn: string[] }>): string[][] {
	if (children.length === 0) return [];

	const remaining = new Set(children.map((c) => c.label));
	const satisfied = new Set<string>();
	const childMap = new Map(children.map((c) => [c.label, c]));
	const waves: string[][] = [];

	while (remaining.size > 0) {
		const ready: string[] = [];
		for (const label of [...remaining].sort()) {
			const child = childMap.get(label)!;
			const deps = new Set(child.dependsOn.filter((d) => childMap.has(d)));
			if ([...deps].every((d) => satisfied.has(d))) {
				ready.push(label);
			}
		}

		if (ready.length === 0) {
			// Deadlock breaker: dispatch all remaining
			ready.push(...[...remaining].sort());
		}

		waves.push(ready);
		for (const label of ready) {
			remaining.delete(label);
			satisfied.add(label);
		}
	}

	return waves;
}

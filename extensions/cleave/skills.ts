/**
 * cleave/skills — Skill matching and resolution for child tasks.
 *
 * Maps child task scope (file patterns) and explicit annotations to
 * skill directives. Skills are specialized instruction sets (SKILL.md)
 * that guide child agents on language-specific conventions, tooling,
 * and best practices.
 *
 * Two matching strategies:
 * 1. **Annotation**: `<!-- skills: python, k8s-operations -->` in tasks.md
 *    overrides auto-matching for that group
 * 2. **Auto-match**: File scope patterns (*.py→python, *.rs→rust, etc.)
 *    matched against DEFAULT_MAPPINGS
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import type { ChildPlan } from "./types.ts";

// ─── Types ──────────────────────────────────────────────────────────────────

/**
 * Maps file patterns to a skill name and optional model tier hint.
 *
 * The glob patterns follow minimatch-style syntax but are matched
 * with a simplified engine (see `globMatches`).
 */
export interface SkillMapping {
	/** Glob patterns that trigger this skill (e.g., "*.py", "Containerfile") */
	patterns: string[];
	/** Skill name (matches the directory name under skills/) */
	skill: string;
	/** Optional preferred model tier for this skill's complexity */
	preferredTier?: "haiku" | "sonnet" | "opus";
}

// ─── Default Mappings ───────────────────────────────────────────────────────

/**
 * Default file-pattern-to-skill mappings.
 *
 * Ordered by specificity — more specific patterns first.
 * Projects can extend this via future config.
 */
export const DEFAULT_MAPPINGS: SkillMapping[] = [
	// Python
	{
		patterns: ["*.py", "pyproject.toml", "setup.py", "setup.cfg", "requirements*.txt", "Pipfile"],
		skill: "python",
		preferredTier: "sonnet",
	},
	// Rust
	{
		patterns: ["*.rs", "Cargo.toml", "Cargo.lock"],
		skill: "rust",
		preferredTier: "sonnet",
	},
	// OCI / Containers
	{
		patterns: ["Containerfile", "Dockerfile", "*.containerfile", "*.dockerfile", "docker-compose*.yml", "docker-compose*.yaml"],
		skill: "oci",
		preferredTier: "sonnet",
	},
	// Kubernetes / Helm
	{
		patterns: ["k8s/**", "kubernetes/**", "helm/**", "charts/**", "Chart.yaml", "values*.yaml", "**/templates/*.yaml"],
		skill: "k8s-operations",
		preferredTier: "sonnet",
	},
	// TypeScript
	{
		patterns: ["*.ts", "*.tsx", "tsconfig.json", "tsconfig.*.json"],
		skill: "typescript",
		preferredTier: "sonnet",
	},
	// Pi extensions (more specific than generic TypeScript — matched first by specificity)
	{
		patterns: ["extensions/**/*.ts", "extensions/*/index.ts"],
		skill: "pi-extensions",
		preferredTier: "sonnet",
	},
	// Git (rarely auto-matched, usually annotated)
	{
		patterns: [".gitignore", ".gitattributes", ".gitmodules"],
		skill: "git",
	},
	// OpenSpec
	{
		patterns: ["openspec/**", "**/spec.md", "**/proposal.md"],
		skill: "openspec",
	},
	// Style / visual
	{
		patterns: ["*.excalidraw", "*.d2"],
		skill: "style",
	},
];

// ─── Matching ───────────────────────────────────────────────────────────────

/**
 * Match skills to a child plan based on its scope and annotations.
 *
 * If the child has explicit `skills` from a `<!-- skills: ... -->` annotation,
 * those are used as-is (annotation overrides auto-match, per design D1).
 *
 * Otherwise, file scope patterns are matched against DEFAULT_MAPPINGS
 * (or a custom mappings array) to auto-detect relevant skills.
 *
 * Returns the list of skill names (deduplicated, stable order).
 */
export function matchSkillsToChild(
	child: ChildPlan,
	mappings: SkillMapping[] = DEFAULT_MAPPINGS,
): string[] {
	// Annotation override: if child already has skills from <!-- skills: ... -->,
	// use those directly and skip auto-matching
	if (child.skills && child.skills.length > 0) {
		return child.skills;
	}

	// Auto-match from scope patterns
	const matched = new Set<string>();

	for (const scopePattern of child.scope) {
		for (const mapping of mappings) {
			if (mapping.patterns.some((p) => globMatches(scopePattern, p))) {
				matched.add(mapping.skill);
			}
		}
	}

	// Also check description for file references (backtick-quoted paths and known filenames)
	const fileRefs = child.description.match(/`([a-zA-Z0-9_./-]+)`/g) ?? [];
	for (const ref of fileRefs) {
		const cleanRef = ref.replace(/`/g, "");
		for (const mapping of mappings) {
			if (mapping.patterns.some((p) => globMatches(cleanRef, p))) {
				matched.add(mapping.skill);
			}
		}
	}

	return [...matched];
}

/**
 * Match skills to all children in a plan, mutating their `skills` arrays.
 *
 * This is the top-level entry point called before task file generation.
 */
export function matchSkillsToAllChildren(
	children: ChildPlan[],
	mappings: SkillMapping[] = DEFAULT_MAPPINGS,
): void {
	for (const child of children) {
		const skills = matchSkillsToChild(child, mappings);
		child.skills = skills;
	}
}

// ─── Resolution ─────────────────────────────────────────────────────────────

/**
 * Well-known skill search paths, in priority order.
 *
 * 1. Package-local skills (relative to pi-kit root)
 * 2. User-installed skills under ~/.pi/agent/
 */
function getSkillSearchPaths(piKitRoot?: string): string[] {
	const paths: string[] = [];

	// Package-local skills
	if (piKitRoot) {
		paths.push(join(piKitRoot, "skills"));
	}

	// User skill symlinks (loaded via ~/.pi/agent/git/)
	const home = process.env.HOME || process.env.USERPROFILE || "";
	if (home) {
		paths.push(join(home, ".pi", "agent", "skills"));
		// Also check external skills loaded via git symlinks
		const gitSkillsBase = join(home, ".pi", "agent", "git");
		if (existsSync(gitSkillsBase)) {
			paths.push(gitSkillsBase);
		}
	}

	return paths;
}

/**
 * Resolve a skill name to its absolute SKILL.md path.
 *
 * Searches well-known skill directories for `{skillName}/SKILL.md`.
 * Returns null if the skill is not found.
 */
export function resolveSkillPath(
	skillName: string,
	piKitRoot?: string,
): string | null {
	const searchPaths = getSkillSearchPaths(piKitRoot);

	for (const base of searchPaths) {
		const candidate = join(base, skillName, "SKILL.md");
		if (existsSync(candidate)) {
			return candidate;
		}
	}

	return null;
}

/**
 * Resolve multiple skill names to their SKILL.md paths.
 *
 * Returns an array of { skill, path } for each skill that was found.
 * Skills that cannot be resolved are omitted with a warning in the
 * returned `notFound` array.
 */
export function resolveSkillPaths(
	skillNames: string[],
	piKitRoot?: string,
): { resolved: Array<{ skill: string; path: string }>; notFound: string[] } {
	const resolved: Array<{ skill: string; path: string }> = [];
	const notFound: string[] = [];

	for (const name of skillNames) {
		const path = resolveSkillPath(name, piKitRoot);
		if (path) {
			resolved.push({ skill: name, path });
		} else {
			notFound.push(name);
		}
	}

	return { resolved, notFound };
}

/**
 * Get the preferred model tier for a set of skills.
 *
 * Returns the highest tier among all matched skills:
 * opus > sonnet > haiku > undefined (default)
 *
 * This allows skills to hint at the model complexity needed
 * for good results (e.g., Rust/Python → sonnet, complex architecture → opus).
 */
export function getPreferredTier(
	skillNames: string[],
	mappings: SkillMapping[] = DEFAULT_MAPPINGS,
): "haiku" | "sonnet" | "opus" | undefined {
	const tierRank = { haiku: 1, sonnet: 2, opus: 3 };
	let maxRank = 0;
	let maxTier: "haiku" | "sonnet" | "opus" | undefined;

	for (const name of skillNames) {
		const mapping = mappings.find((m) => m.skill === name);
		if (mapping?.preferredTier) {
			const rank = tierRank[mapping.preferredTier] ?? 0;
			if (rank > maxRank) {
				maxRank = rank;
				maxTier = mapping.preferredTier;
			}
		}
	}

	return maxTier;
}

// ─── Glob Matching ──────────────────────────────────────────────────────────

/**
 * Simplified glob matching for file patterns.
 *
 * Supports:
 * - `*` matches any characters within a path segment (no slashes)
 * - `**` matches any number of path segments
 * - Exact matches (e.g., "Containerfile")
 *
 * The `filePath` is the scope entry (which may itself contain globs),
 * and `pattern` is the mapping pattern.
 *
 * Matching is bidirectional: the scope pattern may be a glob ("src/models/*.py")
 * and the mapping pattern may also be a glob ("*.py"). We check if they could
 * match the same files.
 */
export function globMatches(filePath: string, pattern: string): boolean {
	const fpLower = filePath.toLowerCase();
	const patLower = pattern.toLowerCase();

	// Exact match
	if (fpLower === patLower) return true;

	// If pattern has no wildcards, check if filePath ends with it or contains it as a segment
	if (!patLower.includes("*")) {
		const fpParts = fpLower.split("/");
		return fpParts.some((part) => part === patLower) || fpLower.endsWith("/" + patLower);
	}

	// Pattern is a simple extension glob: *.ext
	const extMatch = patLower.match(/^\*\.(\w+)$/);
	if (extMatch) {
		const ext = "." + extMatch[1];
		// Check if filePath ends with this extension (ignoring glob chars in filePath)
		const fpClean = fpLower.replace(/\*/g, "x"); // neutralize globs in filePath for extension check
		if (fpClean.endsWith(ext)) return true;
		// Also match if filePath is a glob that would produce files with this extension
		// e.g., "src/models/*.py" should match "*.py"
		if (fpLower.endsWith("*" + ext)) return true;
		return false;
	}

	// Pattern uses **: directory-recursive match
	if (patLower.includes("**")) {
		// Convert ** pattern to a check: does filePath start with the prefix?
		const parts = patLower.split("**");
		if (parts.length === 2) {
			const prefix = parts[0].replace(/\/$/, "");
			const suffix = parts[1].replace(/^\//, "");
			// Check if filePath starts with the prefix directory
			if (prefix && !fpLower.startsWith(prefix) && !fpLower.startsWith(prefix + "/")) {
				return false;
			}
			// If there's a suffix, check if filePath (or its glob expansion) could match
			if (suffix) {
				// Suffix may contain path segments + globs, e.g., "templates/*.yaml"
				// Convert the suffix into a regex for matching against the relevant part of filePath
				const suffixRegex = suffix
					.replace(/[.+?^${}()|[\]\\]/g, "\\$&")
					.replace(/\*/g, "[^/]*");
				const re = new RegExp("(?:^|/)" + suffixRegex + "$");
				if (re.test(fpLower)) return true;
				// Also check extension-only: "*.yaml" suffix against file ending
				const suffixExtMatch = suffix.match(/^\*\.(\w+)$/);
				if (suffixExtMatch) {
					const ext = "." + suffixExtMatch[1];
					return fpLower.endsWith(ext) || fpLower.endsWith("*" + ext);
				}
				// Check for path-based suffix like "templates/*.yaml"
				return fpLower.includes(suffix.replace(/\*/g, ""));
			}
			// No suffix — prefix/** matches anything under that directory
			return true;
		}
	}

	// Pattern with single *: segment match (e.g., "docker-compose*.yml")
	// Convert to a regex
	const escaped = patLower
		.replace(/[.+?^${}()|[\]\\]/g, "\\$&")
		.replace(/\*/g, "[^/]*");
	const re = new RegExp("(?:^|/)" + escaped + "$");
	return re.test(fpLower);
}

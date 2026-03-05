/**
 * cleave/assessment — Pattern library and complexity calculation.
 *
 * Ported from styrene-lab/cleave src/cleave/core/assessment.py.
 * Pure functions with zero runtime dependencies.
 */

import type { AssessmentFlags, AssessmentResult, PatternDefinition, PatternMatch } from "./types.js";

// ═══════════════════════════════════════════════════════════════════════════
// PATTERN LIBRARY — 12 domain patterns for fast-path assessment
// ═══════════════════════════════════════════════════════════════════════════

export const PATTERNS: Record<string, PatternDefinition> = {
	full_stack_crud: {
		name: "Full-Stack CRUD",
		description: "Create/read/update/delete operations spanning UI, API, and database layers",
		keywords: [
			"form", "page", "crud", "create", "update", "delete", "add", "edit", "remove",
			"table", "migration", "endpoint", "api", "frontend", "backend", "react", "vue",
			"angular", "express", "django", "rails", "postgres", "mysql", "mongodb",
		],
		requiredAny: ["crud", "create", "add", "update", "delete", "form", "page"],
		expectedComponents: {
			ui_layer: ["react", "vue", "angular", "frontend", "form", "page", "component"],
			api_layer: ["api", "endpoint", "express", "django", "rails", "backend", "route"],
			db_layer: ["postgres", "mysql", "mongodb", "table", "database", "schema"],
		},
		systemsBase: 3,
		modifiersDefault: ["data_migration"],
		splitStrategy: ["Database layer", "API layer", "UI layer"],
	},
	authentication: {
		name: "Authentication System",
		description: "User authentication with credential storage, token management, protected routes",
		keywords: [
			"auth", "authentication", "login", "register", "logout", "jwt", "token", "session",
			"oauth", "saml", "bcrypt", "password", "credential", "sign", "verify", "middleware",
			"guard", "protected", "secure",
		],
		requiredAny: ["auth", "login", "jwt", "oauth", "session", "password"],
		expectedComponents: {
			mechanism: ["jwt", "session", "oauth", "saml", "token"],
			storage: ["postgres", "mysql", "mongodb", "database", "store", "bcrypt", "hash"],
			protection: ["middleware", "guard", "protected", "route", "endpoint"],
		},
		systemsBase: 2,
		modifiersDefault: ["state_coordination", "error_handling", "security_critical"],
		splitStrategy: ["Credential storage", "Token generation/validation", "Route protection"],
	},
	external_integration: {
		name: "External Service Integration",
		description: "Third-party API integration with error handling and state management",
		keywords: [
			"stripe", "twilio", "sendgrid", "aws", "s3", "lambda", "google", "api", "integrate",
			"webhook", "callback", "payment", "email", "sms", "storage", "analytics", "external",
			"third-party", "provider",
		],
		requiredAny: ["stripe", "twilio", "sendgrid", "aws", "s3", "webhook", "integrate"],
		expectedComponents: {
			provider: ["stripe", "twilio", "sendgrid", "aws", "s3", "google", "azure"],
			operation: ["webhook", "callback", "payment", "email", "sms", "upload"],
			error_handling: ["retry", "error", "idempotent", "handle", "fallback"],
		},
		systemsBase: 4,
		modifiersDefault: ["state_coordination", "error_handling", "third_party_api"],
		splitStrategy: ["Database schema", "API client + error handling", "Webhook handlers", "UI integration"],
	},
	database_migration: {
		name: "Database Migration",
		description: "Schema changes, data transformations, or backfill operations",
		keywords: [
			"migrate", "migration", "schema", "column", "table", "index", "alter", "backfill",
			"transform", "constraint", "rollback", "transaction",
		],
		requiredAny: ["migrate", "migration", "schema", "column", "alter", "backfill"],
		expectedComponents: {
			change_type: ["column", "table", "index", "constraint", "alter", "add", "remove"],
			strategy: ["migration", "backfill", "rollback", "transaction"],
		},
		systemsBase: 1,
		modifiersDefault: ["data_migration"],
		splitStrategy: ["Migration script", "Backfill script", "Validation + rollback"],
	},
	performance_optimization: {
		name: "Performance Optimization",
		description: "Caching, query optimization, or performance improvements with SLAs",
		keywords: [
			"optimize", "cache", "redis", "memcached", "cdn", "performance", "latency",
			"throughput", "p95", "p99", "response time", "query", "index", "slow",
		],
		requiredAny: ["optimize", "cache", "redis", "performance", "latency"],
		expectedComponents: {
			technology: ["redis", "memcached", "cdn", "cache", "index"],
			sla: ["p95", "p99", "latency", "ms", "response time", "throughput"],
			invalidation: ["invalidat", "expire", "ttl", "refresh"],
		},
		systemsBase: 3,
		modifiersDefault: ["state_coordination", "concurrency", "performance_critical"],
		splitStrategy: ["Caching layer", "Cache invalidation", "Monitoring + metrics"],
	},
	breaking_api_change: {
		name: "Breaking API Change",
		description: "API contract modifications with versioning or backwards compatibility",
		keywords: [
			"version", "v1", "v2", "deprecate", "deprecated", "breaking", "rename", "endpoint",
			"contract", "backward", "migration", "client",
		],
		requiredAny: ["version", "v2", "deprecate", "breaking", "backward"],
		expectedComponents: {
			change: ["rename", "remove", "modify", "change", "breaking"],
			versioning: ["version", "v1", "v2", "v3", "deprecat"],
			client_plan: ["client", "consumer", "update", "migrat"],
		},
		systemsBase: 2,
		modifiersDefault: ["breaking_changes"],
		splitStrategy: ["New versioned endpoint", "Deprecation + dual-support", "Client migration"],
	},
	simple_refactor: {
		name: "Simple Refactor",
		description: "Code cleanup, renaming, or structural changes without functional modifications",
		keywords: [
			"refactor", "rename", "reorganize", "cleanup", "extract", "inline", "move",
			"restructure", "mechanical", "no functional",
		],
		requiredAny: ["refactor", "rename", "cleanup", "extract", "reorganize"],
		expectedComponents: {
			operation: ["rename", "extract", "inline", "move", "reorganize"],
			scope: ["function", "class", "file", "module", "component", "method"],
		},
		systemsBase: 1,
		modifiersDefault: [],
		splitStrategy: ["By module/scope", "Update tests"],
	},
	bug_fix: {
		name: "Bug Fix",
		description: "Fix broken functionality, crashes, or incorrect behavior",
		keywords: [
			"fix", "bug", "broken", "issue", "error", "crash", "regression", "defect",
			"incorrect", "failing", "problem",
		],
		requiredAny: ["fix", "bug", "broken", "crash", "error", "regression"],
		expectedComponents: {
			problem: ["crash", "error", "broken", "failing", "incorrect", "regression"],
			area: ["auth", "api", "database", "ui", "workflow", "validation"],
		},
		systemsBase: 0.5,
		modifiersDefault: ["error_handling"],
		splitStrategy: ["Reproduce bug", "Fix implementation", "Add regression test"],
	},
	greenfield_project: {
		name: "Greenfield Project",
		description: "New project scaffolding with multiple modules, crates, or packages from scratch",
		keywords: [
			"new project", "greenfield", "scaffold", "bootstrap", "from scratch", "create",
			"initialize", "init", "setup", "starter", "boilerplate", "foundation",
			"workspace", "monorepo", "crate", "package", "module", "library", "app",
			"application", "binary", "build", "architecture", "project structure",
		],
		requiredAny: ["new project", "greenfield", "scaffold", "bootstrap", "from scratch", "initialize", "init"],
		expectedComponents: {
			structure: ["workspace", "monorepo", "crate", "package", "module", "directory", "layout"],
			build: ["cargo", "npm", "pip", "gradle", "cmake", "makefile", "build", "compile"],
			modules: ["library", "app", "binary", "core", "foundation", "model", "render", "engine"],
		},
		systemsBase: 3,
		modifiersDefault: [],
		splitStrategy: ["Foundation/core module", "Domain modules (parallel)", "Application integration"],
	},
	multi_module_library: {
		name: "Multi-Module Library",
		description: "Building a library or framework with interdependent modules/crates/packages",
		keywords: [
			"library", "framework", "crate", "package", "module", "api", "sdk",
			"trait", "interface", "abstraction", "export", "publish", "public api",
			"dependency", "workspace", "monorepo", "multi-crate", "multi-package",
		],
		requiredAny: ["library", "framework", "crate", "sdk", "multi-crate", "multi-package"],
		expectedComponents: {
			modules: ["crate", "package", "module", "workspace", "monorepo"],
			api: ["trait", "interface", "abstraction", "export", "public", "api"],
			build: ["cargo", "npm", "pip", "gradle", "build", "compile", "link"],
		},
		systemsBase: 2,
		modifiersDefault: [],
		splitStrategy: ["Core traits/interfaces", "Module implementations (parallel)", "Integration + public API"],
	},
	application_bootstrap: {
		name: "Application Bootstrap",
		description: "Setting up a new application with its full architecture (UI, document model, rendering, etc.)",
		keywords: [
			"application", "app", "gui", "tui", "cli", "desktop", "web app", "mobile",
			"document model", "rendering", "pipeline", "event loop", "main loop",
			"window", "canvas", "layout", "state management", "architecture",
			"egui", "iced", "tauri", "electron", "react", "svelte",
		],
		requiredAny: ["application", "app", "gui", "tui", "desktop", "web app"],
		expectedComponents: {
			ui: ["gui", "tui", "window", "canvas", "component", "layout", "render", "egui", "iced"],
			model: ["model", "state", "document", "data", "store", "management"],
			infra: ["event", "loop", "pipeline", "architecture", "main", "entry"],
		},
		systemsBase: 3,
		modifiersDefault: ["state_coordination"],
		splitStrategy: ["Core data model", "Rendering/UI layer", "Event handling + state management", "Application shell"],
	},
	refactor: {
		name: "Refactor",
		description: "Replace or rewrite implementation while preserving behavior",
		keywords: [
			"refactor", "replace", "rewrite", "improve", "clean", "restructure", "reorganize",
			"swap", "substitute", "modernize",
		],
		requiredAny: ["replace", "rewrite", "swap", "substitute", "refactor"],
		expectedComponents: {
			operation: ["replace", "rewrite", "swap", "substitute"],
			target: ["implementation", "approach", "library", "framework", "pattern"],
		},
		systemsBase: 1.0,
		modifiersDefault: [],
		splitStrategy: ["Implement replacement", "Update call sites", "Remove old implementation"],
	},
};

// ═══════════════════════════════════════════════════════════════════════════
// MODIFIERS — complexity multipliers
// ═══════════════════════════════════════════════════════════════════════════

export const MODIFIERS: Record<string, string[]> = {
	state_coordination: ["transaction", "cache invalidation", "eventual consistency", "sync", "distributed"],
	error_handling: ["rollback", "compensation", "recovery", "retry", "error handling"],
	concurrency: ["concurrent", "lock", "atomic", "race condition", "thread"],
	security_critical: ["auth", "encrypt", "secret", "pii", "credential", "password", "token"],
	breaking_changes: ["breaking", "backward compat", "deprecate", "migration path"],
	data_migration: ["migration", "schema", "backfill", "transform", "alter"],
	third_party_api: ["stripe", "twilio", "sendgrid", "aws", "external api", "webhook"],
	performance_critical: ["sla", "latency", "p95", "p99", "throughput", "<100ms"],
};

// ═══════════════════════════════════════════════════════════════════════════
// SYSTEM SIGNALS — architectural boundary detection for heuristic fallback
// ═══════════════════════════════════════════════════════════════════════════

const SYSTEM_SIGNALS: Record<string, string[]> = {
	abstraction: ["interface", "abstraction", "abstract", "protocol", "contract"],
	discovery: ["registry", "discover", "plugin", "loader", "dynamic"],
	observability: ["decorator", "instrument", "monitor", "metrics", "analytics", "telemetry", "profil"],
	streaming: ["stream", "real-time", "realtime", "websocket", "sse", "event-driven"],
	backend_layer: ["backend", "api", "endpoint", "route", "microservice"],
	frontend_layer: ["frontend", "ui", "component", "page", "form", "tui", "terminal"],
	data_layer: ["database", "db", "schema", "migration", "orm", "table"],
	auth_layer: ["auth", "token", "session", "credential", "jwt", "oauth"],
	infrastructure: ["cache", "queue", "worker", "job", "celery"],
	io_layer: ["export", "import", "format", "parse", "serializ"],
	cli_layer: ["cli", "subcommand", "argparse", "click"],
	security_layer: ["security", "validation", "scan", "audit", "policy"],
	config_layer: ["config", "setting", "preference"],
};

// ═══════════════════════════════════════════════════════════════════════════
// CORE FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════

/** Source pattern for file extension detection — shared by the safe wrappers below. */
const FILE_EXT_SOURCE = String.raw`\w+\.(ts|tsx|js|jsx|py|java|go|rs|rb|php|cpp|c|h|kt|swift|cs|m|scala)\b`;

/** Safe wrapper: always returns fresh matches, no global state leakage. */
function fileExtMatches(text: string): RegExpMatchArray[] {
	return [...text.matchAll(new RegExp(FILE_EXT_SOURCE, "g"))];
}

/** Safe wrapper for test: uses a non-global regex, no lastIndex mutation. */
function hasFileExt(text: string): boolean {
	return new RegExp(FILE_EXT_SOURCE).test(text);
}

/**
 * Estimate the number of architectural system boundaries in a directive.
 *
 * Single file mention → cap at 1.
 * Multiple file mentions → max(signal count, file count).
 * No file mentions → signal-based estimation.
 */
export function estimateSystems(directive: string): number {
	const lower = directive.toLowerCase();

	const fileMatches = fileExtMatches(lower);

	const baseCount = Object.values(SYSTEM_SIGNALS).filter((keywords) =>
		keywords.some((kw) => lower.includes(kw)),
	).length;

	if (fileMatches.length === 1) return 1;
	if (fileMatches.length > 1) return Math.max(1, Math.max(baseCount, fileMatches.length));
	return Math.max(1, baseCount);
}

/**
 * Match directive against the 12 core patterns.
 *
 * Confidence scoring:
 * - Base 0.55 for having a required keyword
 * - +0.05 per keyword matched (up to 0.20)
 * - +0.08 per expected component category covered
 * - +0.05 for all components covered
 * - +0.05 for specific technology names
 * - +0.15 for specific file mention (Bug Fix boost)
 * - -0.15 for vague terms
 */
export function matchPattern(directive: string): PatternMatch | null {
	const lower = directive.toLowerCase();
	let best: PatternMatch | null = null;

	for (const [patternId, pattern] of Object.entries(PATTERNS)) {
		const hasRequired = pattern.requiredAny.some((req) => lower.includes(req));
		if (!hasRequired) continue;

		const keywordsMatched = pattern.keywords.filter((kw) => lower.includes(kw));

		// Base confidence
		let confidence = 0.55;

		// +0.05 per keyword (capped at 0.20)
		confidence += Math.min(0.20, keywordsMatched.length * 0.05);

		// Component coverage
		const expectedEntries = Object.values(pattern.expectedComponents);
		let componentsCovered = 0;
		for (const componentKeywords of expectedEntries) {
			if (componentKeywords.some((kw) => lower.includes(kw))) {
				componentsCovered++;
			}
		}
		if (expectedEntries.length > 0) {
			confidence += componentsCovered * 0.08;
			if (componentsCovered === expectedEntries.length) {
				confidence += 0.05;
			}
		}

		// Bug Fix: file-specific boost
		if (patternId === "bug_fix" && hasFileExt(lower)) {
			confidence += 0.15;
		}

		// Specific technology bonus
		const specificTech = ["postgres", "mysql", "mongodb", "redis", "stripe", "aws", "jwt", "oauth"];
		if (specificTech.some((tech) => lower.includes(tech))) {
			confidence += 0.05;
		}

		// Vague penalty
		const vagueTerms = ["better", "improve", "fix issues", "make it", "update"];
		if (vagueTerms.some((vague) => lower.includes(vague))) {
			confidence -= 0.15;
		}

		confidence = Math.min(0.98, Math.max(0.0, confidence));

		const threshold = patternId === "bug_fix" ? 0.65 : 0.80;
		if (confidence < threshold) continue;

		if (!best || confidence > best.confidence) {
			best = {
				patternId,
				name: pattern.name,
				confidence,
				keywordsMatched,
				systems: pattern.systemsBase,
				modifiers: pattern.modifiersDefault,
			};
		}
	}

	return best;
}

/** Detect which complexity modifiers apply to a directive. */
export function detectModifiers(directive: string): string[] {
	const lower = directive.toLowerCase();
	return Object.entries(MODIFIERS)
		.filter(([, keywords]) => keywords.some((kw) => lower.includes(kw)))
		.map(([name]) => name);
}

/**
 * Calculate complexity: (1 + systems) × (1 + 0.5 × modifiers).
 * Capped at 100.0.
 */
export function calculateComplexity(systems: number, modifiers: string[]): number {
	const raw = (1 + systems) * (1 + 0.5 * modifiers.length);
	return Math.round(Math.min(raw, 100.0) * 10) / 10;
}

/** Effective complexity with validation offset for threshold comparison. */
export function effectiveComplexity(complexity: number, validate = true): number {
	return complexity + (validate ? 1 : 0);
}

/** Detect special flags in directive text. */
export function detectFlags(directive: string): AssessmentFlags {
	const lower = directive.toLowerCase();
	return {
		robust: lower.includes("cleave-robust") || lower.includes("cleave_robust"),
	};
}

/**
 * Assess directive complexity using fast-path pattern matching.
 *
 * Returns a structured assessment with complexity, matched pattern,
 * confidence, and decision (execute | cleave | needs_assessment).
 */
export function assessDirective(
	directive: string,
	threshold = 2.0,
	validate = true,
): AssessmentResult {
	if (!directive?.trim()) {
		throw new Error("Directive cannot be empty or whitespace-only");
	}

	const match = matchPattern(directive);

	if (match) {
		const detectedModifiers = detectModifiers(directive);
		const allModifiers = [...new Set([...match.modifiers, ...detectedModifiers])];

		// File detection can override pattern systems
		const lower = directive.toLowerCase();
		const fileMatches = fileExtMatches(lower);

		let systemsForCalc = match.systems;
		let systemsForDisplay = match.systems;

		if (fileMatches.length === 1) {
			systemsForCalc = Math.max(1, match.systems);
			systemsForDisplay = 1;
		} else if (fileMatches.length > 1) {
			systemsForCalc = Math.max(match.systems, fileMatches.length);
			systemsForDisplay = Math.max(match.systems, fileMatches.length);
		} else {
			systemsForDisplay = Math.max(1, match.systems);
		}

		const complexity = calculateComplexity(systemsForCalc, allModifiers);
		const effComplexity = effectiveComplexity(complexity, validate);
		const decision = effComplexity <= threshold ? "execute" : "cleave";

		const result: AssessmentResult = {
			complexity,
			systems: systemsForDisplay,
			modifiers: allModifiers,
			method: "fast-path",
			pattern: match.name,
			confidence: match.confidence,
			decision,
			reasoning:
				`Pattern '${match.name}' matched with ${(match.confidence * 100).toFixed(0)}% confidence. ` +
				`Systems: ${systemsForDisplay}, Modifiers: ${allModifiers.length}. ` +
				`Formula: (1 + ${systemsForCalc}) × (1 + 0.5 × ${allModifiers.length}) = ${complexity}. ` +
				`Effective (validate=${validate}): ${effComplexity}`,
			skipInterrogation: false,
		};

		// Tier 0: trivial tasks skip interrogation
		if (
			match.confidence >= 0.90 &&
			effComplexity <= threshold &&
			match.name === "Simple Refactor"
		) {
			result.skipInterrogation = true;
		}

		return result;
	}

	// No pattern matched — heuristic fallback.
	// If complexity exceeds threshold, recommend cleave rather than
	// needs_assessment. A high-complexity directive without a matching
	// pattern still benefits from decomposition.
	const detectedModifiers = detectModifiers(directive);
	const estimatedSystems = estimateSystems(directive);
	const complexity = calculateComplexity(estimatedSystems, detectedModifiers);
	const effComplexity = effectiveComplexity(complexity, validate);

	const decision = effComplexity > threshold ? "cleave" : "needs_assessment";

	return {
		complexity,
		systems: estimatedSystems,
		modifiers: detectedModifiers,
		method: "heuristic",
		pattern: null,
		confidence: 0,
		decision,
		reasoning:
			`No pattern matched with sufficient confidence. ` +
			`Heuristic estimate: ${estimatedSystems} systems, ${detectedModifiers.length} modifiers. ` +
			`Complexity: ${complexity}, effective: ${effComplexity}.` +
			(decision === "cleave" ? ` Exceeds threshold ${threshold} — recommending decomposition.` : ""),
		skipInterrogation: false,
	};
}

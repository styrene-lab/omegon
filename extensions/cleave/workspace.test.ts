/**
 * Tests for cleave/workspace — scenario matching, orphan detection,
 * skill injection, and model resolution.
 */

import { describe, it } from "node:test";
import * as assert from "node:assert/strict";
import { matchScenariosToChildren, generateTaskFile, buildSkillSection, buildGuardrailSection, classifyDirtyPaths } from "./workspace.ts";
import type { SkillDirective } from "./workspace.ts";
import { buildChildPrompt, resolveExecuteModel, classifyByScope, applyEffortFloor } from "./dispatcher.ts";
import type { ChildPlan, ModelTier } from "./types.ts";
import type { OpenSpecContext } from "./openspec.ts";

function makeCtx(scenarios: OpenSpecContext["specScenarios"]): OpenSpecContext {
	return {
		changePath: "/tmp/test",
		designContent: null,
		decisions: [],
		fileChanges: [],
		specScenarios: scenarios,
		apiContract: null,
	};
}

function makeChild(overrides: Partial<ChildPlan> & { label: string }): ChildPlan {
	return {
		description: overrides.description ?? overrides.label,
		scope: overrides.scope ?? [],
		dependsOn: [],
		specDomains: [],
		skills: [],
		...overrides,
	};
}

// ─── matchScenariosToChildren ───────────────────────────────────────────────

describe("matchScenariosToChildren", () => {
	it("returns empty maps when no scenarios", () => {
		const children = [makeChild({ label: "task-a" })];
		const result = matchScenariosToChildren(children, makeCtx([]));
		assert.equal(result.get(0)!.length, 0);
	});

	it("returns empty maps when no context", () => {
		const children = [makeChild({ label: "task-a" })];
		const result = matchScenariosToChildren(children, null);
		assert.equal(result.get(0)!.length, 0);
	});

	// Tier 1: Annotation match
	it("matches scenario by spec-domain annotation", () => {
		const children = [
			makeChild({ label: "rbac-enforcement", specDomains: ["relay/rbac"] }),
			makeChild({ label: "rate-limits", specDomains: ["relay/limits"] }),
		];
		const ctx = makeCtx([
			{ domain: "relay/rbac", requirement: "Check capability", scenarios: ["Given..."] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		assert.equal(result.get(0)!.length, 1);
		assert.equal(result.get(0)![0].crossCutting, false);
		assert.equal(result.get(1)!.length, 0);
	});

	it("annotation match takes precedence over word overlap", () => {
		const children = [
			makeChild({ label: "rbac-enforcement", description: "handle RBAC and capabilities", specDomains: ["relay/rbac"] }),
			makeChild({ label: "service-layer", description: "RBAC checks in service layer", specDomains: ["relay/service"] }),
		];
		const ctx = makeCtx([
			{ domain: "relay/rbac", requirement: "RBAC gating", scenarios: ["Given a user..."] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// Should go to child 0 (annotation match), not child 1 (word overlap on "RBAC")
		assert.equal(result.get(0)!.length, 1);
		assert.equal(result.get(1)!.length, 0);
	});

	// Tier 2: Scope match
	it("falls back to scope match when no annotation", () => {
		const children = [
			makeChild({ label: "models", scope: ["rbac.py"] }),
			makeChild({ label: "service", scope: ["relay_service.py"] }),
		];
		const ctx = makeCtx([
			{ domain: "relay/rbac", requirement: "Check capability in relay_service", scenarios: ["When create_session is called on relay_service"] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// "relay_service" appears in scenario text and matches child 1's scope
		assert.equal(result.get(1)!.length, 1);
	});

	// Tier 3: Word overlap fallback
	it("falls back to word overlap when no annotation or scope match", () => {
		const children = [
			makeChild({ label: "database", description: "Database migrations and models" }),
			makeChild({ label: "authentication", description: "RBAC enforcement and capability checks" }),
		];
		const ctx = makeCtx([
			{ domain: "auth/rbac", requirement: "Capability enforcement", scenarios: ["Given..."] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// "enforcement" and "capability" match child 1
		assert.equal(result.get(1)!.length, 1);
	});

	// Orphan detection
	it("auto-injects orphan scenario with cross-cutting marker", () => {
		const children = [
			makeChild({ label: "models", description: "Add model fields", scope: ["models/"] }),
			makeChild({ label: "config", description: "Configuration parsing", scope: ["config/"] }),
		];
		const ctx = makeCtx([
			{ domain: "obscure/domain", requirement: "Unrelated requirement", scenarios: ["Given something unrelated"] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// Should be injected as orphan into some child
		const allAssigned = [...result.values()].flat();
		assert.equal(allAssigned.length, 1);
		assert.equal(allAssigned[0].crossCutting, true);
	});

	it("orphan injection prefers scope match on When clause", () => {
		const children = [
			makeChild({ label: "models", scope: ["rbac.py"] }),
			makeChild({ label: "service", scope: ["relay_service.py"] }),
		];
		const ctx = makeCtx([
			{
				domain: "obscure/niche",
				requirement: "Niche requirement",
				scenarios: ["Niche scenario\nWhen relay_service handles the request\nThen it succeeds"],
			},
		]);
		const result = matchScenariosToChildren(children, ctx);
		// Orphan, but When clause mentions relay_service → child 1
		assert.equal(result.get(1)!.length, 1);
		assert.equal(result.get(1)![0].crossCutting, true);
	});

	it("all scenarios assigned — no scenario left unmatched", () => {
		const children = [
			makeChild({ label: "rbac", specDomains: ["relay/rbac"] }),
			makeChild({ label: "limits", specDomains: ["relay/limits"] }),
		];
		const ctx = makeCtx([
			{ domain: "relay/rbac", requirement: "Check caps", scenarios: ["S1"] },
			{ domain: "relay/limits", requirement: "Rate limit", scenarios: ["S2"] },
			{ domain: "relay/unknown", requirement: "Mystery", scenarios: ["S3"] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		const total = [...result.values()].reduce((sum, arr) => sum + arr.length, 0);
		// All 3 scenarios assigned (2 by annotation, 1 orphan auto-injected)
		assert.equal(total, 3);
	});

	it("domain prefix matching is segment-aware", () => {
		const children = [
			makeChild({ label: "relay", specDomains: ["relay"] }),
			makeChild({ label: "admin", specDomains: ["relay-admin"] }),
		];
		const ctx = makeCtx([
			{ domain: "relay-admin/permissions", requirement: "Admin perms", scenarios: ["S1"] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// "relay" should NOT match "relay-admin/permissions" — different path segment
		assert.equal(result.get(0)!.length, 0);
		// "relay-admin" SHOULD match "relay-admin/permissions"
		assert.equal(result.get(1)!.length, 1);
	});

	it("scope match requires word boundary, not substring", () => {
		const children = [
			makeChild({ label: "utils", scope: ["src/utils.py"] }),
			makeChild({ label: "main", scope: ["src/main.py"] }),
		];
		const ctx = makeCtx([
			{ domain: "core/utility", requirement: "Utility functions provide main functionality", scenarios: ["Given utility..."] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// "utils.py" should NOT match "utility" — different word
		assert.equal(result.get(0)!.length, 0);
		// "main.py" should NOT match "main" as a casual English word... 
		// actually "main.py" with word boundary WILL match "main" — this is a known limitation
		// but at least "utils.py" won't match "utility"
	});

	it("orphan falls back to last child when no match at all", () => {
		const children = [
			makeChild({ label: "aaa", description: "xxx" }),
			makeChild({ label: "bbb", description: "yyy" }),
			makeChild({ label: "zzz", description: "integration" }),
		];
		const ctx = makeCtx([
			{ domain: "q/r", requirement: "w", scenarios: ["s"] },
		]);
		const result = matchScenariosToChildren(children, ctx);
		// No word overlap, no scope — should go to last child
		assert.equal(result.get(2)!.length, 1);
		assert.equal(result.get(2)![0].crossCutting, true);
	});
});

// ─── generateTaskFile — Specialist Skills section ───────────────────────────

describe("classifyDirtyPaths", () => {
	it("treats operator-profile runtime churn as volatile", () => {
		const result = classifyDirtyPaths([".pi/runtime/operator-profile.json"]);
		assert.equal(result.volatile.length, 1);
		assert.equal(result.volatile[0]?.path, ".pi/runtime/operator-profile.json");
		assert.equal(result.checkpointFiles.length, 0);
	});
});

describe("generateTaskFile", () => {
	it("includes Specialist Skills section when skills are provided", () => {
		const child = makeChild({ label: "models", scope: ["src/models/*.py"], description: "Build data models" });
		const skills: SkillDirective[] = [
			{ skill: "python", path: "/home/user/skills/python/SKILL.md" },
			{ skill: "oci", path: "/home/user/skills/oci/SKILL.md" },
		];
		const result = generateTaskFile(0, child, [child], "Build the thing", null, [], skills);

		assert.ok(result.includes("## Specialist Skills"), "Should contain Specialist Skills heading");
		assert.ok(result.includes("**python**"), "Should list python skill");
		assert.ok(result.includes("**oci**"), "Should list oci skill");
		assert.ok(result.includes("/home/user/skills/python/SKILL.md"), "Should contain python path");
		assert.ok(result.includes("/home/user/skills/oci/SKILL.md"), "Should contain oci path");
		assert.ok(result.includes("Before starting, read these skill files"), "Should have reading instruction");
	});

	it("omits Specialist Skills section when no skills", () => {
		const child = makeChild({ label: "models", scope: ["README.md"], description: "Update docs" });
		const result = generateTaskFile(0, child, [child], "Update docs", null, [], []);

		assert.ok(!result.includes("## Specialist Skills"), "Should NOT contain Specialist Skills heading");
	});

	it("omits Specialist Skills section when skills param is undefined", () => {
		const child = makeChild({ label: "models", scope: ["README.md"], description: "Update docs" });
		const result = generateTaskFile(0, child, [child], "Update docs", null, [], undefined);

		assert.ok(!result.includes("## Specialist Skills"), "Should NOT contain Specialist Skills heading");
	});

	it("Specialist Skills appears before Design Context", () => {
		const child = makeChild({ label: "rbac", specDomains: ["auth/rbac"], description: "RBAC impl" });
		const ctx = makeCtx([
			{ domain: "auth/rbac", requirement: "Check perms", scenarios: ["Given a user..."] },
		]);
		const openspecCtx: OpenSpecContext = {
			changePath: "/tmp/test",
			designContent: null,
			decisions: ["Use JWT tokens"],
			fileChanges: [],
			specScenarios: ctx.specScenarios,
			apiContract: null,
		};
		const skills: SkillDirective[] = [
			{ skill: "python", path: "/skills/python/SKILL.md" },
		];

		// Generate with both skills and scenarios
		const scenarios = matchScenariosToChildren([child], openspecCtx);
		const assigned = scenarios.get(0) ?? [];
		const result = generateTaskFile(0, child, [child], "Impl RBAC", openspecCtx, assigned, skills);

		const skillIdx = result.indexOf("## Specialist Skills");
		const designIdx = result.indexOf("## Design Context");

		assert.ok(skillIdx > 0, "Should have Specialist Skills section");
		assert.ok(designIdx > 0, "Should have Design Context section");
		assert.ok(skillIdx < designIdx, "Specialist Skills should appear before Design Context");
	});

	it("skill paths contain absolute paths for agent file reading", () => {
		const child = makeChild({ label: "api", scope: ["src/api.rs"], description: "Build API" });
		const skills: SkillDirective[] = [
			{ skill: "rust", path: "/Users/dev/.pi/agent/skills/rust/SKILL.md" },
		];
		const result = generateTaskFile(0, child, [child], "Build API", null, [], skills);

		// Verify the path is absolute and looks actionable
		assert.ok(result.includes("`/Users/dev/.pi/agent/skills/rust/SKILL.md`"), "Path should be absolute and code-quoted");
	});
});

// ─── buildSkillSection ──────────────────────────────────────────────────────

describe("buildSkillSection", () => {
	it("returns empty string for empty skills", () => {
		assert.equal(buildSkillSection([]), "");
	});

	it("returns empty string for undefined", () => {
		assert.equal(buildSkillSection(undefined), "");
	});

	it("renders skill entries with name and path", () => {
		const result = buildSkillSection([
			{ skill: "python", path: "/a/b/SKILL.md" },
			{ skill: "rust", path: "/c/d/SKILL.md" },
		]);
		assert.ok(result.includes("## Specialist Skills"));
		assert.ok(result.includes("**python**: `/a/b/SKILL.md`"));
		assert.ok(result.includes("**rust**: `/c/d/SKILL.md`"));
	});
});

// ─── buildChildPrompt — skill directives ────────────────────────────────────

describe("buildChildPrompt", () => {
	it("adds skill contract item when task file has Specialist Skills section", () => {
		const taskContent = [
			"# Task 0: models",
			"",
			"## Specialist Skills",
			"",
			"- **python**: `/skills/python/SKILL.md`",
			"",
			"## Mission",
			"Build models",
		].join("\n");

		const prompt = buildChildPrompt(taskContent, "Build the thing", "/workspace");

		assert.ok(prompt.includes("7. **Skills**"), "Should add skills contract item");
		assert.ok(prompt.includes("read` tool to load"), "Should instruct to use read tool");
		assert.ok(prompt.includes("SKILL.md file before starting"), "Should mention reading before starting");
	});

	it("does NOT add skill contract item when no Specialist Skills section", () => {
		const taskContent = [
			"# Task 0: models",
			"",
			"## Mission",
			"Build models",
		].join("\n");

		const prompt = buildChildPrompt(taskContent, "Build the thing", "/workspace");

		assert.ok(!prompt.includes("7. **Skills**"), "Should NOT add skills contract item");
	});

	it("preserves the sandwich pattern: contract, directive, task, reminder", () => {
		const taskContent = "# Task 0: test\n\n## Mission\nDo stuff";
		const prompt = buildChildPrompt(taskContent, "Test directive", "/ws");

		const contractIdx = prompt.indexOf("## Contract");
		const directiveIdx = prompt.indexOf("## Root Directive");
		const taskIdx = prompt.indexOf("## Your Task");
		const reminderIdx = prompt.indexOf("## REMINDER");

		assert.ok(contractIdx < directiveIdx, "Contract before Directive");
		assert.ok(directiveIdx < taskIdx, "Directive before Task");
		assert.ok(taskIdx < reminderIdx, "Task before Reminder");
	});
});

// ─── resolveExecuteModel ────────────────────────────────────────────────────

describe("resolveExecuteModel", () => {
	it("defaults to victory when no hints", () => {
		const result = resolveExecuteModel(
			{ skills: [], executeModel: undefined },
			false,
			false,
		);
		assert.equal(result, "victory");
	});

	it("returns local when preferLocal is true and local model available", () => {
		const result = resolveExecuteModel(
			{ skills: ["python"], executeModel: undefined },
			true,
			true,
			() => "victory",
		);
		assert.equal(result, "local");
	});

	it("does NOT return local when local model unavailable even if preferred", () => {
		const result = resolveExecuteModel(
			{ skills: [], executeModel: undefined },
			true,
			false,
		);
		assert.equal(result, "victory");
	});

	it("explicit executeModel takes precedence over skill tier", () => {
		const result = resolveExecuteModel(
			{ skills: ["python"], executeModel: "gloriana" },
			false,
			false,
			() => "victory",
		);
		assert.equal(result, "gloriana");
	});

	it("skill tier used when no explicit executeModel", () => {
		const result = resolveExecuteModel(
			{ skills: ["complex-arch"], executeModel: undefined },
			false,
			false,
			(skills) => skills.includes("complex-arch") ? "gloriana" : undefined,
		);
		assert.equal(result, "gloriana");
	});

	it("explicit executeModel beats local override", () => {
		const result = resolveExecuteModel(
			{ skills: [], executeModel: "gloriana" },
			true,
			true,
		);
		assert.equal(result, "gloriana");
	});

	it("local override applies when no explicit executeModel", () => {
		const result = resolveExecuteModel(
			{ skills: [] },
			true,
			true,
		);
		assert.equal(result, "local");
	});

	it("handles undefined skills gracefully", () => {
		const result = resolveExecuteModel(
			{ skills: undefined, executeModel: undefined },
			false,
			false,
			() => "gloriana",
		);
		assert.equal(result, "victory");
	});

	it("handles empty skills with tier function", () => {
		const result = resolveExecuteModel(
			{ skills: [], executeModel: undefined },
			false,
			false,
			() => "gloriana",
		);
		assert.equal(result, "victory");
	});

	it("skill tier function returning undefined falls through to default", () => {
		const result = resolveExecuteModel(
			{ skills: ["unknown-skill"], executeModel: undefined },
			false,
			false,
			() => undefined,
		);
		assert.equal(result, "victory");
	});
});

// ─── classifyByScope ────────────────────────────────────────────────────────

describe("classifyByScope", () => {
	it("returns local for 1 file", () => {
		assert.equal(classifyByScope(["src/api.ts"]), "local");
	});

	it("returns local for 3 non-test files", () => {
		assert.equal(classifyByScope(["src/a.ts", "src/b.ts", "src/c.ts"]), "local");
	});

	it("returns victory for 4 non-test files", () => {
		assert.equal(classifyByScope(["src/a.ts", "src/b.ts", "src/c.ts", "src/d.ts"]), "victory");
	});

	it("returns victory for 8 non-test files", () => {
		const scope = Array.from({ length: 8 }, (_, i) => `src/file${i}.ts`);
		assert.equal(classifyByScope(scope), "victory");
	});

	it("returns gloriana for 9+ non-test files", () => {
		const scope = Array.from({ length: 9 }, (_, i) => `src/file${i}.ts`);
		assert.equal(classifyByScope(scope), "gloriana");
	});

	it("test files are excluded from non-test count", () => {
		// 2 non-test + 3 test = 5 total but only 2 effective → local
		assert.equal(classifyByScope([
			"src/api.ts", "src/types.ts",
			"src/api.test.ts", "src/types.test.ts", "src/integration.test.ts",
		]), "local");
	});

	it("returns undefined for empty scope", () => {
		assert.equal(classifyByScope([]), undefined);
	});

	it(".spec.ts files are also excluded from non-test count", () => {
		assert.equal(classifyByScope([
			"src/api.ts", "src/api.spec.ts",
		]), "local");
	});
});

// ─── resolveExecuteModel with scope autoclassification ──────────────────────

describe("resolveExecuteModel — scope autoclassification", () => {
	it("classifies small scope (≤3 files) as local when local available", () => {
		const result = resolveExecuteModel(
			{ scope: ["src/api.ts", "src/types.ts"], skills: [] },
			false,
			true,
		);
		assert.equal(result, "local");
	});

	it("classifies medium scope (4-8 files) as victory", () => {
		const scope = Array.from({ length: 5 }, (_, i) => `src/file${i}.ts`);
		const result = resolveExecuteModel(
			{ scope, skills: [] },
			false,
			true,
		);
		assert.equal(result, "victory");
	});

	it("classifies large scope (9+ files) as gloriana", () => {
		const scope = Array.from({ length: 10 }, (_, i) => `src/file${i}.ts`);
		const result = resolveExecuteModel(
			{ scope, skills: [] },
			false,
			true,
		);
		assert.equal(result, "gloriana");
	});

	it("explicit annotation overrides scope classification", () => {
		const result = resolveExecuteModel(
			{ scope: ["src/a.ts"], skills: [], executeModel: "gloriana" },
			false,
			true,
		);
		assert.equal(result, "gloriana", "explicit gloriana should override scope-based local");
	});

	it("preferLocal caps scope classification to local", () => {
		const scope = Array.from({ length: 6 }, (_, i) => `src/file${i}.ts`);
		const result = resolveExecuteModel(
			{ scope, skills: [] },
			true, // preferLocal
			true,
		);
		assert.equal(result, "local", "preferLocal should force local regardless of scope size");
	});

	it("scope classification skipped when local model unavailable", () => {
		const result = resolveExecuteModel(
			{ scope: ["src/a.ts"], skills: [] },
			false,
			false, // no local model
		);
		assert.equal(result, "victory", "should fall through to default when no local model");
	});

	it("no scope falls through to preferLocal", () => {
		const result = resolveExecuteModel(
			{ skills: [] },
			true,
			true,
		);
		assert.equal(result, "local");
	});

	it("no scope, no preferLocal, no skills → default victory", () => {
		const result = resolveExecuteModel(
			{ skills: [] },
			false,
			true,
		);
		assert.equal(result, "victory");
	});
});

// ─── generateTaskFile — guardrail section ───────────────────────────────────

describe("generateTaskFile with guardrails", () => {
	it("includes guardrail section when provided", () => {
		const child = makeChild({ label: "api", scope: ["src/api.ts"], description: "Build API" });
		const guardrailSection = [
			"",
			"## Project Guardrails",
			"",
			"Before reporting success, run these deterministic checks and fix any failures:",
			"",
			"1. **typecheck**: `npx tsc --noEmit`",
			"2. **lint**: `npm run lint`",
			"",
			"Include command output in the Verification section. If any check fails, fix the errors before completing your task.",
			"",
		].join("\n");

		const result = generateTaskFile(0, child, [child], "Build API", null, [], [], guardrailSection);

		assert.ok(result.includes("## Project Guardrails"), "Should contain Project Guardrails heading");
		assert.ok(result.includes("npx tsc --noEmit"), "Should contain typecheck command");
		assert.ok(result.includes("npm run lint"), "Should contain lint command");
	});

	it("guardrail section appears before Contract", () => {
		const child = makeChild({ label: "api", scope: ["src/api.ts"], description: "Build API" });
		const guardrailSection = "\n## Project Guardrails\n\nRun checks.\n";

		const result = generateTaskFile(0, child, [child], "Build API", null, [], [], guardrailSection);

		const guardrailIdx = result.indexOf("## Project Guardrails");
		const contractIdx = result.indexOf("## Contract");
		assert.ok(guardrailIdx > 0, "Should have guardrail section");
		assert.ok(contractIdx > 0, "Should have contract section");
		assert.ok(guardrailIdx < contractIdx, "Guardrails should appear before Contract");
	});

	it("omits guardrail section when not provided", () => {
		const child = makeChild({ label: "api", scope: ["src/api.ts"], description: "Build API" });
		const result = generateTaskFile(0, child, [child], "Build API", null, [], []);
		assert.ok(!result.includes("## Project Guardrails"), "Should NOT contain guardrail section");
	});
});

// ─── buildGuardrailSection ──────────────────────────────────────────────────

describe("buildGuardrailSection", () => {
	it("returns guardrail section for a project with tsconfig.json", () => {
		// Use the current repo root which has tsconfig.json + package.json
		const section = buildGuardrailSection(process.cwd());
		assert.ok(section.includes("## Project Guardrails"), "Should contain guardrails header");
		assert.ok(section.includes("typecheck"), "Should contain typecheck guardrail");
		assert.ok(section.includes("Before reporting success"), "Should contain instruction text");
	});

	it("returns empty string for directory with no project files", () => {
		const section = buildGuardrailSection("/tmp");
		assert.equal(section, "");
	});

	it("includes command in guardrail section", () => {
		const section = buildGuardrailSection(process.cwd());
		assert.ok(section.includes("`"), "Should contain backtick-wrapped command");
	});
});

// ─── mapModelTierToFlag removed ─────────────────────────────────────────────
// mapModelTierToFlag() was a deprecated internal-only function that returned
// fuzzy tier aliases (e.g. "gloriana", "retribution"). It has been unexported per C5
// review finding: "Dead exports are test surface for wrong behavior."
// Use resolveModelIdForTier() (tested in dispatcher.test.ts) instead.

// ─── applyEffortFloor — effort tier integration ─────────────────────────────

describe("applyEffortFloor", () => {
	const SHARED_KEY = Symbol.for("pi-kit-shared-state");

	function setEffort(effort: any) {
		(globalThis as any)[SHARED_KEY].effort = effort;
	}

	function clearEffort() {
		delete (globalThis as any)[SHARED_KEY].effort;
	}

	it("returns classified unchanged when effort is undefined", () => {
		clearEffort();
		assert.equal(applyEffortFloor("victory"), "victory");
		assert.equal(applyEffortFloor("local"), "local");
		assert.equal(applyEffortFloor("gloriana"), "gloriana");
	});

	it("forces local when cleavePreferLocal is true", () => {
		setEffort({ cleavePreferLocal: true, cleaveFloor: "local" });
		try {
			assert.equal(applyEffortFloor("victory"), "local");
			assert.equal(applyEffortFloor("gloriana"), "local");
			// Already local stays local
			assert.equal(applyEffortFloor("local"), "local");
		} finally {
			clearEffort();
		}
	});

	it("raises tier to floor when classified is below", () => {
		setEffort({ cleavePreferLocal: false, cleaveFloor: "victory" });
		try {
			// local → victory (raised to floor)
			assert.equal(applyEffortFloor("local"), "victory");
			// victory stays victory (at floor)
			assert.equal(applyEffortFloor("victory"), "victory");
			// gloriana stays gloriana (above floor)
			assert.equal(applyEffortFloor("gloriana"), "gloriana");
		} finally {
			clearEffort();
		}
	});

	it("gloriana floor raises everything to gloriana", () => {
		setEffort({ cleavePreferLocal: false, cleaveFloor: "gloriana" });
		try {
			assert.equal(applyEffortFloor("local"), "gloriana");
			assert.equal(applyEffortFloor("victory"), "gloriana");
			assert.equal(applyEffortFloor("gloriana"), "gloriana");
		} finally {
			clearEffort();
		}
	});
});

// ─── resolveExecuteModel — effort integration ──────────────────────────────

describe("resolveExecuteModel — effort integration", () => {
	const SHARED_KEY = Symbol.for("pi-kit-shared-state");

	function setEffort(effort: any) {
		(globalThis as any)[SHARED_KEY].effort = effort;
	}

	function clearEffort() {
		delete (globalThis as any)[SHARED_KEY].effort;
	}

	it("Servitor tier forces all children local regardless of scope", () => {
		setEffort({
			level: 1,
			name: "Servitor",
			cleavePreferLocal: true,
			cleaveFloor: "local",
		});
		try {
			// 5-file scope would normally classify as victory
			const result = resolveExecuteModel(
				{ scope: Array.from({ length: 5 }, (_, i) => `src/file${i}.ts`), skills: [] },
				false,
				true,
			);
			assert.equal(result, "local");
		} finally {
			clearEffort();
		}
	});

	it("Absolute tier raises floor to victory for small-scope children", () => {
		setEffort({
			level: 6,
			name: "Absolute",
			cleavePreferLocal: false,
			cleaveFloor: "victory",
		});
		try {
			// 2-file scope would normally classify as local
			const result = resolveExecuteModel(
				{ scope: ["src/a.ts", "src/b.ts"], skills: [] },
				false,
				true,
			);
			assert.equal(result, "victory");
		} finally {
			clearEffort();
		}
	});

	it("undefined effort falls back to normal scope-based behavior", () => {
		clearEffort();
		// 2-file child → local (normal scope-based classification)
		const result = resolveExecuteModel(
			{ scope: ["src/a.ts", "src/b.ts"], skills: [] },
			false,
			true,
		);
		assert.equal(result, "local");
	});

	it("explicit annotation bypasses effort floor", () => {
		setEffort({
			level: 1,
			name: "Servitor",
			cleavePreferLocal: true,
			cleaveFloor: "local",
		});
		try {
			// Explicit gloriana annotation should NOT be forced to local
			const result = resolveExecuteModel(
				{ scope: ["src/a.ts"], skills: [], executeModel: "gloriana" },
				false,
				true,
			);
			assert.equal(result, "gloriana");
		} finally {
			clearEffort();
		}
	});

	it("effort cleavePreferLocal overrides caller preferLocal=false", () => {
		setEffort({
			level: 2,
			name: "Average",
			cleavePreferLocal: true,
			cleaveFloor: "local",
		});
		try {
			// preferLocal=false but effort says cleavePreferLocal=true
			const result = resolveExecuteModel(
				{ scope: Array.from({ length: 6 }, (_, i) => `src/f${i}.ts`), skills: [] },
				false, // caller says no prefer-local
				true,
			);
			assert.equal(result, "local");
		} finally {
			clearEffort();
		}
	});
});

/**
 * Tests for cleave/openspec — OpenSpec tasks.md parser and conversion.
 */

import { describe, it, beforeEach, afterEach } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import * as crypto from "node:crypto";
import {
	parseTasksFile,
	taskGroupsToChildPlans,
	openspecChangeToSplitPlan,
	openspecChangeToSplitPlanWithContext,
	detectOpenSpec,
	listChanges,
	findExecutableChanges,
	parseDesignFileChanges,
	parseDesignDecisions,
	readSpecScenarios,
	buildOpenSpecContext,
	writeBackTaskCompletion,
	getActiveChangesStatus,
} from "./openspec.js";

function tmpDir(): string {
	const dir = path.join(os.tmpdir(), `cleave-openspec-test-${crypto.randomBytes(6).toString("hex")}`);
	fs.mkdirSync(dir, { recursive: true });
	return dir;
}

// ─── parseTasksFile ─────────────────────────────────────────────────────────

describe("parseTasksFile", () => {
	it("parses numbered groups with checkbox tasks", () => {
		const content = `# Tasks

## 1. Theme Infrastructure
- [ ] 1.1 Create ThemeContext with light/dark state
- [ ] 1.2 Add CSS custom properties for colors
- [x] 1.3 Implement localStorage persistence

## 2. UI Components
- [ ] 2.1 Create ThemeToggle component
- [ ] 2.2 Add toggle to settings page
`;
		const groups = parseTasksFile(content);
		assert.equal(groups.length, 2);
		assert.equal(groups[0].number, 1);
		assert.equal(groups[0].title, "Theme Infrastructure");
		assert.equal(groups[0].tasks.length, 3);
		assert.equal(groups[0].tasks[0].id, "1.1");
		assert.equal(groups[0].tasks[0].text, "Create ThemeContext with light/dark state");
		assert.equal(groups[0].tasks[0].done, false);
		assert.equal(groups[0].tasks[2].done, true);
		assert.equal(groups[1].number, 2);
		assert.equal(groups[1].tasks.length, 2);
	});

	it("parses unnumbered groups", () => {
		const content = `## Database Changes
- [ ] Add migration for users table
- [ ] Create seed data

## API Endpoints
- [ ] Implement /users REST routes
`;
		const groups = parseTasksFile(content);
		assert.equal(groups.length, 2);
		assert.equal(groups[0].title, "Database Changes");
		assert.equal(groups[0].number, 1);
		assert.equal(groups[1].number, 2);
	});

	it("handles tasks without IDs", () => {
		const content = `## Setup
- [ ] Install dependencies
- [ ] Configure environment
`;
		const groups = parseTasksFile(content);
		assert.equal(groups[0].tasks[0].id, "1.1");
		assert.equal(groups[0].tasks[1].id, "1.2");
	});

	it("handles bullet tasks without checkboxes", () => {
		const content = `## Cleanup
- Remove old files
- Update documentation
`;
		const groups = parseTasksFile(content);
		assert.equal(groups[0].tasks.length, 2);
		assert.equal(groups[0].tasks[0].done, false);
	});

	it("returns empty for no groups", () => {
		assert.deepEqual(parseTasksFile("just some text\nno groups here"), []);
	});

	it("returns empty for empty content", () => {
		assert.deepEqual(parseTasksFile(""), []);
	});

	it("handles uppercase X in checkboxes", () => {
		const content = `## Tasks
- [X] Done task
- [ ] Pending task
`;
		const groups = parseTasksFile(content);
		assert.equal(groups[0].tasks[0].done, true);
		assert.equal(groups[0].tasks[1].done, false);
	});
});

// ─── taskGroupsToChildPlans ─────────────────────────────────────────────────

describe("taskGroupsToChildPlans", () => {
	it("returns null for fewer than 2 groups", () => {
		const groups = [{ number: 1, title: "Solo", tasks: [{ id: "1.1", text: "do thing", done: false }] }];
		assert.equal(taskGroupsToChildPlans(groups), null);
	});

	it("converts groups to child plans", () => {
		const groups = [
			{ number: 1, title: "Database Layer", tasks: [
				{ id: "1.1", text: "Create migration", done: false },
				{ id: "1.2", text: "Add indexes", done: false },
			]},
			{ number: 2, title: "API Layer", tasks: [
				{ id: "2.1", text: "Implement endpoints", done: false },
			]},
		];
		const plans = taskGroupsToChildPlans(groups);
		assert.ok(plans);
		assert.equal(plans.length, 2);
		assert.equal(plans[0].label, "database-layer");
		assert.ok(plans[0].description.includes("Create migration"));
		assert.equal(plans[1].label, "api-layer");
	});

	it("skips completed tasks in descriptions", () => {
		const groups = [
			{ number: 1, title: "Setup", tasks: [
				{ id: "1.1", text: "Install deps", done: true },
				{ id: "1.2", text: "Configure env", done: false },
			]},
			{ number: 2, title: "Build", tasks: [
				{ id: "2.1", text: "Implement feature", done: false },
			]},
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.ok(!plans[0].description.includes("Install deps"));
		assert.ok(plans[0].description.includes("Configure env"));
	});

	it("merges groups when more than 4", () => {
		const groups = Array.from({ length: 6 }, (_, i) => ({
			number: i + 1,
			title: `Group ${i + 1}`,
			tasks: [{ id: `${i + 1}.1`, text: `Task ${i + 1}`, done: false }],
		}));
		const plans = taskGroupsToChildPlans(groups)!;
		assert.ok(plans);
		assert.ok(plans.length <= 4, `Expected <= 4, got ${plans.length}`);
	});

	it("filters out groups where all tasks are done", () => {
		const groups = [
			{ number: 1, title: "Done Group", tasks: [
				{ id: "1.1", text: "Already done", done: true },
			]},
			{ number: 2, title: "Active A", tasks: [
				{ id: "2.1", text: "Do this", done: false },
			]},
			{ number: 3, title: "Active B", tasks: [
				{ id: "3.1", text: "Do that", done: false },
			]},
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.ok(plans);
		assert.equal(plans.length, 2);
		assert.equal(plans[0].label, "active-a");
		assert.equal(plans[1].label, "active-b");
	});

	it("returns null when all groups are done except one", () => {
		const groups = [
			{ number: 1, title: "Done", tasks: [{ id: "1.1", text: "x", done: true }] },
			{ number: 2, title: "Active", tasks: [{ id: "2.1", text: "y", done: false }] },
		];
		assert.equal(taskGroupsToChildPlans(groups), null);
	});

	it("infers dependencies from 'after' markers", () => {
		const groups = [
			{ number: 1, title: "Database", tasks: [{ id: "1.1", text: "Create tables", done: false }] },
			{ number: 2, title: "API", tasks: [{ id: "2.1", text: "Build endpoints after database", done: false }] },
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.ok(plans);
		assert.deepEqual(plans[1].dependsOn, ["database"]);
	});

	it("infers dependencies from 'requires' markers", () => {
		const groups = [
			{ number: 1, title: "Auth Layer", tasks: [{ id: "1.1", text: "Add JWT", done: false }] },
			{ number: 2, title: "Protected Routes", tasks: [{ id: "2.1", text: "Requires auth layer middleware", done: false }] },
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.ok(plans);
		assert.deepEqual(plans[1].dependsOn, ["auth-layer"]);
	});

	it("does not infer self-dependencies", () => {
		const groups = [
			{ number: 1, title: "Setup", tasks: [{ id: "1.1", text: "Setup requires setup tools", done: false }] },
			{ number: 2, title: "Build", tasks: [{ id: "2.1", text: "Build it", done: false }] },
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.deepEqual(plans[0].dependsOn, []);
	});

	it("normalizes labels to kebab-case", () => {
		const groups = [
			{ number: 1, title: "My Cool Feature!", tasks: [{ id: "1.1", text: "a", done: false }] },
			{ number: 2, title: "Another Thing", tasks: [{ id: "2.1", text: "b", done: false }] },
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.equal(plans[0].label, "my-cool-feature");
		assert.equal(plans[1].label, "another-thing");
	});

	it("infers scope from file references in tasks", () => {
		const groups = [
			{ number: 1, title: "Auth", tasks: [
				{ id: "1.1", text: "Update `src/auth/login.ts` for OAuth", done: false },
				{ id: "1.2", text: "Modify `src/auth/session.ts`", done: false },
			]},
			{ number: 2, title: "Tests", tasks: [
				{ id: "2.1", text: "Add tests for auth", done: false },
			]},
		];
		const plans = taskGroupsToChildPlans(groups)!;
		assert.ok(plans[0].scope.some((s) => s.includes("src/auth")));
	});
});

// ─── detectOpenSpec / listChanges / findExecutableChanges ────────────────────

describe("detectOpenSpec", () => {
	let dir: string;

	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("returns null when no openspec/ exists", () => {
		assert.equal(detectOpenSpec(dir), null);
	});

	it("returns path when openspec/ exists", () => {
		fs.mkdirSync(path.join(dir, "openspec"), { recursive: true });
		assert.equal(detectOpenSpec(dir), path.join(dir, "openspec"));
	});
});

describe("listChanges", () => {
	let dir: string;

	beforeEach(() => {
		dir = tmpDir();
		fs.mkdirSync(path.join(dir, "changes", "add-auth"), { recursive: true });
		fs.mkdirSync(path.join(dir, "changes", "fix-bug"), { recursive: true });
		fs.mkdirSync(path.join(dir, "changes", "archive"), { recursive: true });
		fs.writeFileSync(path.join(dir, "changes", "add-auth", "tasks.md"), "## 1. Auth\n- [ ] Do thing");
		fs.writeFileSync(path.join(dir, "changes", "add-auth", "proposal.md"), "# Proposal");
	});
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("lists non-archived changes", () => {
		const changes = listChanges(dir);
		assert.equal(changes.length, 2);
		const names = changes.map((c) => c.name).sort();
		assert.deepEqual(names, ["add-auth", "fix-bug"]);
	});

	it("detects tasks.md presence", () => {
		const changes = listChanges(dir);
		const auth = changes.find((c) => c.name === "add-auth")!;
		assert.equal(auth.hasTasks, true);
		assert.equal(auth.hasProposal, true);
		const bug = changes.find((c) => c.name === "fix-bug")!;
		assert.equal(bug.hasTasks, false);
	});

	it("excludes archive directory", () => {
		const changes = listChanges(dir);
		assert.ok(!changes.some((c) => c.name === "archive"));
	});
});

describe("findExecutableChanges", () => {
	let dir: string;

	beforeEach(() => {
		dir = tmpDir();
		fs.mkdirSync(path.join(dir, "changes", "ready"), { recursive: true });
		fs.mkdirSync(path.join(dir, "changes", "not-ready"), { recursive: true });
		fs.writeFileSync(path.join(dir, "changes", "ready", "tasks.md"), "## 1. A\n- [ ] x\n## 2. B\n- [ ] y");
	});
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("returns only changes with tasks.md", () => {
		const exec = findExecutableChanges(dir);
		assert.equal(exec.length, 1);
		assert.equal(exec[0].name, "ready");
	});
});

// ─── openspecChangeToSplitPlan ──────────────────────────────────────────────

describe("openspecChangeToSplitPlan", () => {
	let dir: string;

	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("returns null when tasks.md missing", () => {
		assert.equal(openspecChangeToSplitPlan(dir), null);
	});

	it("returns null when fewer than 2 groups", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), "## 1. Solo\n- [ ] Only task");
		assert.equal(openspecChangeToSplitPlan(dir), null);
	});

	it("converts full change to SplitPlan", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), `# Tasks

## 1. Database
- [ ] 1.1 Create users table migration
- [ ] 1.2 Add indexes

## 2. API
- [ ] 2.1 Implement REST endpoints
- [ ] 2.2 Add validation middleware

## 3. Frontend
- [ ] 3.1 Build login form
- [ ] 3.2 Add protected routes
`);
		fs.writeFileSync(path.join(dir, "proposal.md"), `# Proposal

## Intent
Add user authentication with login, registration, and session management.

## Scope
In scope: login, register, logout
`);

		const plan = openspecChangeToSplitPlan(dir);
		assert.ok(plan);
		assert.equal(plan.children.length, 3);
		assert.equal(plan.children[0].label, "database");
		assert.equal(plan.children[1].label, "api");
		assert.equal(plan.children[2].label, "frontend");
		assert.ok(plan.rationale.includes("authentication"));
	});

	it("uses change dirname in rationale when no proposal", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), "## 1. A\n- [ ] x\n## 2. B\n- [ ] y");
		const plan = openspecChangeToSplitPlan(dir)!;
		assert.ok(plan.rationale.includes("OpenSpec change"));
	});

	it("extracts intent from proposal without trailing newline", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), "## 1. A\n- [ ] x\n## 2. B\n- [ ] y");
		fs.writeFileSync(path.join(dir, "proposal.md"), "## Intent\nAdd dark mode support.");
		const plan = openspecChangeToSplitPlan(dir)!;
		assert.ok(plan.rationale.includes("dark mode"), `Got: ${plan.rationale}`);
	});
});

// ─── parseDesignFileChanges ─────────────────────────────────────────────────

describe("parseDesignFileChanges", () => {
	it("parses backtick-quoted paths with actions", () => {
		const design = `## File Changes\n- \`src/foo.ts\` (new)\n- \`src/bar.ts\` (modified)\n`;
		const result = parseDesignFileChanges(design);
		assert.equal(result.length, 2);
		assert.deepEqual(result[0], { path: "src/foo.ts", action: "new" });
		assert.deepEqual(result[1], { path: "src/bar.ts", action: "modified" });
	});

	it("parses unquoted paths", () => {
		const design = `## File Changes\n- src/foo.ts (new)\n- src/bar.ts (deleted)\n`;
		const result = parseDesignFileChanges(design);
		assert.equal(result.length, 2);
		assert.equal(result[0].action, "new");
		assert.equal(result[1].action, "deleted");
	});

	it("handles missing action as unknown", () => {
		const design = `## File Changes\n- \`src/foo.ts\`\n`;
		const result = parseDesignFileChanges(design);
		assert.equal(result.length, 1);
		assert.equal(result[0].action, "unknown");
	});

	it("handles synonym actions (created, updated, removed)", () => {
		const design = `## File Changes\n- \`a.ts\` (created)\n- \`b.ts\` (updated)\n- \`c.ts\` (removed)\n`;
		const result = parseDesignFileChanges(design);
		assert.equal(result[0].action, "new");
		assert.equal(result[1].action, "modified");
		assert.equal(result[2].action, "deleted");
	});

	it("returns empty when no File Changes section", () => {
		const design = `## Architecture\nSome text.\n`;
		assert.equal(parseDesignFileChanges(design).length, 0);
	});

	it("stops at next section heading", () => {
		const design = `## File Changes\n- \`a.ts\` (new)\n## Architecture\n- \`b.ts\` (new)\n`;
		const result = parseDesignFileChanges(design);
		assert.equal(result.length, 1);
		assert.equal(result[0].path, "a.ts");
	});

	it("handles singular 'File Change' heading", () => {
		const design = `## File Change\n- \`a.ts\` (new)\n`;
		assert.equal(parseDesignFileChanges(design).length, 1);
	});
});

// ─── parseDesignDecisions ───────────────────────────────────────────────────

describe("parseDesignDecisions", () => {
	it("extracts decision titles with rationale", () => {
		const design = `### Decision: Use React Context\nAvoids prop drilling.\n### Decision: CSS variables\nRuntime switching.\n`;
		const decisions = parseDesignDecisions(design);
		assert.equal(decisions.length, 2);
		assert.ok(decisions[0].includes("Use React Context"));
		assert.ok(decisions[0].includes("Avoids prop drilling"));
		assert.ok(decisions[1].includes("CSS variables"));
	});

	it("returns empty when no decisions", () => {
		assert.equal(parseDesignDecisions("# Design\nSome text.").length, 0);
	});

	it("handles decision with no rationale line", () => {
		const design = `### Decision: Single-line decision\n## Next Section\n`;
		const decisions = parseDesignDecisions(design);
		assert.equal(decisions.length, 1);
		assert.ok(decisions[0].startsWith("Single-line decision"));
	});
});

// ─── readSpecScenarios ──────────────────────────────────────────────────────

describe("readSpecScenarios", () => {
	let dir: string;
	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("extracts scenarios from ADDED requirements", () => {
		const specsDir = path.join(dir, "specs", "auth");
		fs.mkdirSync(specsDir, { recursive: true });
		fs.writeFileSync(path.join(specsDir, "spec.md"), [
			"## ADDED Requirements",
			"### Requirement: Login",
			"#### Scenario: Valid credentials",
			"Given a registered user",
			"When they submit correct credentials",
			"Then they are authenticated",
			"#### Scenario: Invalid credentials",
			"Given a registered user",
			"When they submit wrong password",
			"Then they see an error",
		].join("\n"));

		const scenarios = readSpecScenarios(dir);
		assert.equal(scenarios.length, 1);
		assert.equal(scenarios[0].domain, "auth");
		assert.equal(scenarios[0].requirement, "Login");
		assert.equal(scenarios[0].scenarios.length, 2);
		assert.ok(scenarios[0].scenarios[0].includes("Valid credentials"));
		assert.ok(scenarios[0].scenarios[1].includes("Invalid credentials"));
	});

	it("ignores REMOVED requirements", () => {
		const specsDir = path.join(dir, "specs", "ui");
		fs.mkdirSync(specsDir, { recursive: true });
		fs.writeFileSync(path.join(specsDir, "spec.md"), [
			"## REMOVED Requirements",
			"### Requirement: Legacy Feature",
			"#### Scenario: Old behavior",
			"Given something",
			"When removed",
			"Then gone",
		].join("\n"));

		assert.equal(readSpecScenarios(dir).length, 0);
	});

	it("returns empty when no specs dir", () => {
		assert.equal(readSpecScenarios(dir).length, 0);
	});

	it("extracts from MODIFIED requirements", () => {
		const specsDir = path.join(dir, "specs", "api");
		fs.mkdirSync(specsDir, { recursive: true });
		fs.writeFileSync(path.join(specsDir, "spec.md"), [
			"## MODIFIED Requirements",
			"### Requirement: Rate Limiting",
			"#### Scenario: Exceeds limit",
			"Given a user at max requests",
			"When they make another request",
			"Then they get 429 response",
		].join("\n"));

		const scenarios = readSpecScenarios(dir);
		assert.equal(scenarios.length, 1);
		assert.equal(scenarios[0].requirement, "Rate Limiting");
	});
});

// ─── buildOpenSpecContext ───────────────────────────────────────────────────

describe("buildOpenSpecContext", () => {
	let dir: string;
	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("builds full context with all artifacts", () => {
		fs.writeFileSync(path.join(dir, "design.md"), [
			"### Decision: Use PostgreSQL",
			"Better for relational data.",
			"## File Changes",
			"- `src/db/schema.ts` (new)",
			"- `src/api/routes.ts` (modified)",
		].join("\n"));

		const specsDir = path.join(dir, "specs", "data");
		fs.mkdirSync(specsDir, { recursive: true });
		fs.writeFileSync(path.join(specsDir, "spec.md"), [
			"## ADDED Requirements",
			"### Requirement: Data Persistence",
			"#### Scenario: Save record",
			"Given valid data",
			"When submitted",
			"Then stored in database",
		].join("\n"));

		const ctx = buildOpenSpecContext(dir);
		assert.equal(ctx.changePath, dir);
		assert.ok(ctx.designContent);
		assert.equal(ctx.decisions.length, 1);
		assert.ok(ctx.decisions[0].includes("PostgreSQL"));
		assert.equal(ctx.fileChanges.length, 2);
		assert.equal(ctx.specScenarios.length, 1);
	});

	it("handles missing design.md gracefully", () => {
		const ctx = buildOpenSpecContext(dir);
		assert.equal(ctx.designContent, null);
		assert.equal(ctx.decisions.length, 0);
		assert.equal(ctx.fileChanges.length, 0);
	});

	it("handles missing specs gracefully", () => {
		fs.writeFileSync(path.join(dir, "design.md"), "### Decision: Something\nReason.\n");
		const ctx = buildOpenSpecContext(dir);
		assert.equal(ctx.specScenarios.length, 0);
		assert.equal(ctx.decisions.length, 1);
	});
});

// ─── openspecChangeToSplitPlanWithContext ────────────────────────────────────

describe("openspecChangeToSplitPlanWithContext", () => {
	let dir: string;
	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("returns plan + context together", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), "## 1. A\n- [ ] task a\n## 2. B\n- [ ] task b\n");
		fs.writeFileSync(path.join(dir, "design.md"), "### Decision: Use X\nBecause Y.\n## File Changes\n- `src/a.ts` (new)\n");

		const result = openspecChangeToSplitPlanWithContext(dir);
		assert.ok(result);
		assert.equal(result.plan.children.length, 2);
		assert.equal(result.context.decisions.length, 1);
		assert.equal(result.context.fileChanges.length, 1);
	});

	it("returns null when tasks.md missing", () => {
		assert.equal(openspecChangeToSplitPlanWithContext(dir), null);
	});

	it("returns null when fewer than 2 groups", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), "## 1. Only\n- [ ] lone task\n");
		assert.equal(openspecChangeToSplitPlanWithContext(dir), null);
	});

	it("supplements scope from design file changes", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), [
			"## 1. Auth Module",
			"- [ ] Create auth middleware",
			"## 2. Users Module",
			"- [ ] Build user profile page",
		].join("\n"));
		fs.writeFileSync(path.join(dir, "design.md"), [
			"## File Changes",
			"- `src/auth/middleware.ts` (new)",
			"- `src/users/profile.ts` (new)",
		].join("\n"));

		const result = openspecChangeToSplitPlanWithContext(dir)!;
		// "auth-module" child should pick up auth/middleware.ts, "users-module" should pick up users/profile.ts
		const authChild = result.plan.children.find(c => c.label === "auth-module")!;
		const usersChild = result.plan.children.find(c => c.label === "users-module")!;
		assert.ok(authChild.scope.includes("src/auth/middleware.ts"), `auth scope: ${authChild.scope}`);
		assert.ok(usersChild.scope.includes("src/users/profile.ts"), `users scope: ${usersChild.scope}`);
	});
});

// ─── writeBackTaskCompletion ────────────────────────────────────────────────

describe("writeBackTaskCompletion", () => {
	let dir: string;
	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("marks tasks done for completed labels", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), [
			"## 1. Auth",
			"- [ ] 1.1 Create login form",
			"- [ ] 1.2 Add JWT middleware",
			"## 2. API",
			"- [ ] 2.1 Create user endpoints",
			"- [ ] 2.2 Add pagination",
		].join("\n"));

		const result = writeBackTaskCompletion(dir, ["auth"]);
		assert.equal(result.updated, 2);
		assert.equal(result.allDone, false);

		const content = fs.readFileSync(path.join(dir, "tasks.md"), "utf-8");
		assert.ok(content.includes("[x] 1.1 Create login form"));
		assert.ok(content.includes("[x] 1.2 Add JWT middleware"));
		assert.ok(content.includes("[ ] 2.1 Create user endpoints"));
	});

	it("marks all tasks done and sets allDone=true", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), [
			"## 1. Auth",
			"- [ ] 1.1 Create login form",
			"## 2. API",
			"- [ ] 2.1 Create endpoints",
		].join("\n"));

		const result = writeBackTaskCompletion(dir, ["auth", "api"]);
		assert.equal(result.updated, 2);
		assert.equal(result.allDone, true);
	});

	it("skips already-checked tasks", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), [
			"## 1. Auth",
			"- [x] 1.1 Already done",
			"- [ ] 1.2 Still todo",
		].join("\n"));

		const result = writeBackTaskCompletion(dir, ["auth"]);
		assert.equal(result.updated, 1); // Only the unchecked one
	});

	it("returns zero when no labels match", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), [
			"## 1. Auth",
			"- [ ] 1.1 Create login form",
		].join("\n"));

		const result = writeBackTaskCompletion(dir, ["nonexistent"]);
		assert.equal(result.updated, 0);
	});

	it("returns zero when no tasks.md", () => {
		const result = writeBackTaskCompletion(dir, ["auth"]);
		assert.equal(result.updated, 0);
		assert.equal(result.totalTasks, 0);
	});

	it("handles multi-word label slugs", () => {
		fs.writeFileSync(path.join(dir, "tasks.md"), [
			"## 1. Database Layer",
			"- [ ] 1.1 Create schema",
			"- [ ] 1.2 Add migrations",
		].join("\n"));

		const result = writeBackTaskCompletion(dir, ["database-layer"]);
		assert.equal(result.updated, 2);
	});

	it("preserves non-task lines unchanged", () => {
		const original = [
			"# Tasks for add-auth",
			"",
			"## 1. Auth",
			"- [ ] 1.1 Create login form",
			"",
			"Some notes here.",
			"",
			"## 2. API",
			"- [ ] 2.1 Create endpoints",
		].join("\n");
		fs.writeFileSync(path.join(dir, "tasks.md"), original);

		writeBackTaskCompletion(dir, ["auth"]);
		const content = fs.readFileSync(path.join(dir, "tasks.md"), "utf-8");
		assert.ok(content.includes("# Tasks for add-auth"));
		assert.ok(content.includes("Some notes here."));
		assert.ok(content.includes("[x] 1.1 Create login form"));
		assert.ok(content.includes("[ ] 2.1 Create endpoints"));
	});
});

// ─── getActiveChangesStatus ─────────────────────────────────────────────────

describe("getActiveChangesStatus", () => {
	let dir: string;
	beforeEach(() => { dir = tmpDir(); });
	afterEach(() => { fs.rmSync(dir, { recursive: true, force: true }); });

	it("returns empty when no openspec dir", () => {
		const result = getActiveChangesStatus(dir);
		assert.equal(result.length, 0);
	});

	it("returns change status with task progress", () => {
		const changeDir = path.join(dir, "openspec", "changes", "add-auth");
		fs.mkdirSync(changeDir, { recursive: true });
		fs.writeFileSync(path.join(changeDir, "tasks.md"), [
			"## 1. Auth",
			"- [x] 1.1 Done task",
			"- [ ] 1.2 Todo task",
			"## 2. API",
			"- [ ] 2.1 Another task",
		].join("\n"));
		fs.writeFileSync(path.join(changeDir, "proposal.md"), "# Proposal");

		const result = getActiveChangesStatus(dir);
		assert.equal(result.length, 1);
		assert.equal(result[0].name, "add-auth");
		assert.equal(result[0].totalTasks, 3);
		assert.equal(result[0].doneTasks, 1);
		assert.equal(result[0].hasProposal, true);
		assert.equal(result[0].hasDesign, false);
	});

	it("reports all artifacts present", () => {
		const changeDir = path.join(dir, "openspec", "changes", "add-feature");
		const specsDir = path.join(changeDir, "specs");
		fs.mkdirSync(specsDir, { recursive: true });
		fs.writeFileSync(path.join(changeDir, "tasks.md"), "## 1. A\n- [x] done");
		fs.writeFileSync(path.join(changeDir, "proposal.md"), "p");
		fs.writeFileSync(path.join(changeDir, "design.md"), "d");
		fs.writeFileSync(path.join(specsDir, "spec.md"), "s");

		const result = getActiveChangesStatus(dir);
		assert.equal(result[0].hasProposal, true);
		assert.equal(result[0].hasDesign, true);
		assert.equal(result[0].hasSpecs, true);
		assert.equal(result[0].doneTasks, 1);
		assert.equal(result[0].totalTasks, 1);
	});

	it("handles changes without tasks.md", () => {
		const changeDir = path.join(dir, "openspec", "changes", "wip-change");
		fs.mkdirSync(changeDir, { recursive: true });
		fs.writeFileSync(path.join(changeDir, "proposal.md"), "# WIP");

		const result = getActiveChangesStatus(dir);
		assert.equal(result.length, 1);
		assert.equal(result[0].totalTasks, 0);
		assert.equal(result[0].doneTasks, 0);
	});

	it("lists multiple changes", () => {
		for (const name of ["change-a", "change-b", "change-c"]) {
			const changeDir = path.join(dir, "openspec", "changes", name);
			fs.mkdirSync(changeDir, { recursive: true });
			fs.writeFileSync(path.join(changeDir, "tasks.md"), "## 1. X\n- [ ] task");
		}

		const result = getActiveChangesStatus(dir);
		assert.equal(result.length, 3);
	});

	it("excludes archive directory", () => {
		const archiveDir = path.join(dir, "openspec", "changes", "archive", "old-change");
		fs.mkdirSync(archiveDir, { recursive: true });
		fs.writeFileSync(path.join(archiveDir, "tasks.md"), "## 1. X\n- [ ] task");

		const activeDir = path.join(dir, "openspec", "changes", "current");
		fs.mkdirSync(activeDir, { recursive: true });
		fs.writeFileSync(path.join(activeDir, "tasks.md"), "## 1. Y\n- [ ] task");

		const result = getActiveChangesStatus(dir);
		assert.equal(result.length, 1);
		assert.equal(result[0].name, "current");
	});
});

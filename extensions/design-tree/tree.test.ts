/**
 * Tests for design-tree/tree — pure domain logic.
 */

import { describe, it, before, after } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import {
	parseFrontmatter,
	yamlQuote,
	generateFrontmatter,
	parseSections,
	generateBody,
	scanDesignDocs,
	getChildren,
	getRoots,
	getAllOpenQuestions,
	getDocBody,
	getNodeSections,
	createNode,
	setNodeStatus,
	addOpenQuestion,
	removeOpenQuestion,
	addResearch,
	addDecision,
	addDependency,
	addRelated,
	addImplementationNotes,
	branchFromQuestion,
	toSlug,
	extractBody,
	validateNodeId,
	scaffoldOpenSpecChange,
} from "./tree.js";

// ─── Test Helpers ────────────────────────────────────────────────────────────

function makeTmpDir(): string {
	return fs.mkdtempSync(path.join(os.tmpdir(), "design-tree-test-"));
}

function writeDoc(docsDir: string, filename: string, content: string): string {
	if (!fs.existsSync(docsDir)) fs.mkdirSync(docsDir, { recursive: true });
	const filePath = path.join(docsDir, filename);
	fs.writeFileSync(filePath, content);
	return filePath;
}

const SAMPLE_DOC = `---
id: auth-strategy
title: "Authentication Strategy"
status: exploring
parent: security
dependencies: [user-model, session-mgmt]
related: [api-design]
tags: [security, auth]
open_questions:
  - "JWT vs session tokens?"
  - "Which OAuth provider?"
---

# Authentication Strategy

## Overview

Evaluating authentication approaches for the platform.

## Research

### JWT Analysis

JWTs provide stateless authentication but have revocation challenges.
Token size grows with claims.

### OAuth2 Providers

Evaluated: Auth0, Keycloak, Cognito.

## Decisions

### Decision: Use Keycloak for IdP

**Status:** decided
**Rationale:** Self-hosted, OIDC-compliant, active community.

### Decision: Short-lived access tokens

**Status:** exploring
**Rationale:** 15-minute TTL reduces exposure window.

## Open Questions

- JWT vs session tokens?
- Which OAuth provider?
- Rate limiting strategy for auth endpoints?

## Implementation Notes

### File Scope

- \`src/auth/\` — Auth module root
- \`src/middleware/jwt.ts\` — JWT validation middleware

### Constraints

- Must support SAML 2.0 for enterprise clients
- Token TTL < 15 minutes per security policy
`;

// ─── Frontmatter ─────────────────────────────────────────────────────────────

describe("parseFrontmatter", () => {
	it("parses scalar values", () => {
		const fm = parseFrontmatter(SAMPLE_DOC);
		assert.ok(fm);
		assert.equal(fm.id, "auth-strategy");
		assert.equal(fm.title, "Authentication Strategy");
		assert.equal(fm.status, "exploring");
		assert.equal(fm.parent, "security");
	});

	it("parses inline arrays", () => {
		const fm = parseFrontmatter(SAMPLE_DOC);
		assert.ok(fm);
		assert.deepEqual(fm.dependencies, ["user-model", "session-mgmt"]);
		assert.deepEqual(fm.related, ["api-design"]);
		assert.deepEqual(fm.tags, ["security", "auth"]);
	});

	it("parses block arrays", () => {
		const fm = parseFrontmatter(SAMPLE_DOC);
		assert.ok(fm);
		assert.deepEqual(fm.open_questions, [
			"JWT vs session tokens?",
			"Which OAuth provider?",
		]);
	});

	it("returns null for no frontmatter", () => {
		assert.equal(parseFrontmatter("# Just a heading"), null);
	});
});

describe("yamlQuote", () => {
	it("leaves simple values unquoted", () => {
		assert.equal(yamlQuote("simple-value"), "simple-value");
	});

	it("quotes values with special characters", () => {
		assert.equal(yamlQuote("has: colon"), '"has: colon"');
		assert.equal(yamlQuote("has # hash"), '"has # hash"');
	});

	it("escapes quotes within quoted values", () => {
		assert.equal(yamlQuote('has "quotes"'), '"has \\"quotes\\""');
	});
});

describe("generateFrontmatter", () => {
	it("round-trips through parse", () => {
		const node = {
			id: "test-node",
			title: "Test Node",
			status: "exploring" as const,
			parent: "parent-node",
			dependencies: ["dep1", "dep2"],
			related: ["rel1"],
			tags: ["tag1"],
			open_questions: ["Question 1?", "Question 2?"],
		};
		const fm = generateFrontmatter(node);
		const parsed = parseFrontmatter(fm + "\n# Content");
		assert.ok(parsed);
		assert.equal(parsed.id, "test-node");
		assert.equal(parsed.title, "Test Node");
		assert.equal(parsed.status, "exploring");
		assert.equal(parsed.parent, "parent-node");
		assert.deepEqual(parsed.dependencies, ["dep1", "dep2"]);
		assert.deepEqual(parsed.open_questions, ["Question 1?", "Question 2?"]);
	});
});

// ─── Section Parsing ─────────────────────────────────────────────────────────

describe("parseSections", () => {
	const body = extractBody(SAMPLE_DOC);

	it("parses overview", () => {
		const sections = parseSections(body);
		assert.ok(sections.overview.includes("Evaluating authentication approaches"));
	});

	it("parses research entries", () => {
		const sections = parseSections(body);
		assert.equal(sections.research.length, 2);
		assert.equal(sections.research[0].heading, "JWT Analysis");
		assert.ok(sections.research[0].content.includes("stateless"));
		assert.equal(sections.research[1].heading, "OAuth2 Providers");
	});

	it("parses decisions", () => {
		const sections = parseSections(body);
		assert.equal(sections.decisions.length, 2);
		assert.equal(sections.decisions[0].title, "Use Keycloak for IdP");
		assert.equal(sections.decisions[0].status, "decided");
		assert.ok(sections.decisions[0].rationale.includes("Self-hosted"));
		assert.equal(sections.decisions[1].title, "Short-lived access tokens");
		assert.equal(sections.decisions[1].status, "exploring");
	});

	it("parses open questions from body", () => {
		const sections = parseSections(body);
		assert.equal(sections.openQuestions.length, 3);
		assert.ok(sections.openQuestions.includes("JWT vs session tokens?"));
		assert.ok(sections.openQuestions.includes("Rate limiting strategy for auth endpoints?"));
	});

	it("parses implementation notes", () => {
		const sections = parseSections(body);
		assert.equal(sections.implementationNotes.fileScope.length, 2);
		assert.equal(sections.implementationNotes.fileScope[0].path, "src/auth/");
		assert.equal(sections.implementationNotes.constraints.length, 2);
		assert.ok(sections.implementationNotes.constraints[0].includes("SAML 2.0"));
	});
});

describe("generateBody", () => {
	it("produces valid markdown with all sections", () => {
		const sections = parseSections(extractBody(SAMPLE_DOC));
		const body = generateBody("Authentication Strategy", sections);

		assert.ok(body.includes("# Authentication Strategy"));
		assert.ok(body.includes("## Overview"));
		assert.ok(body.includes("## Research"));
		assert.ok(body.includes("### JWT Analysis"));
		assert.ok(body.includes("## Decisions"));
		assert.ok(body.includes("### Decision: Use Keycloak for IdP"));
		assert.ok(body.includes("## Open Questions"));
		assert.ok(body.includes("- JWT vs session tokens?"));
		assert.ok(body.includes("## Implementation Notes"));
		assert.ok(body.includes("`src/auth/`"));
	});

	it("round-trips through parse", () => {
		const original = parseSections(extractBody(SAMPLE_DOC));
		const body = generateBody("Auth Strategy", original);
		const reparsed = parseSections(body);

		assert.equal(reparsed.overview, original.overview);
		assert.equal(reparsed.research.length, original.research.length);
		assert.equal(reparsed.decisions.length, original.decisions.length);
		assert.equal(reparsed.openQuestions.length, original.openQuestions.length);
		assert.equal(reparsed.implementationNotes.fileScope.length, original.implementationNotes.fileScope.length);
		assert.equal(reparsed.implementationNotes.constraints.length, original.implementationNotes.constraints.length);
	});
});

// ─── Tree Scanning ───────────────────────────────────────────────────────────

describe("scanDesignDocs", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
		writeDoc(docsDir, "auth-strategy.md", SAMPLE_DOC);
		writeDoc(
			docsDir,
			"user-model.md",
			`---
id: user-model
title: User Model
status: decided
open_questions: []
---

# User Model

## Overview

User data model design.

## Open Questions

*No open questions.*
`,
		);
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("finds all nodes", () => {
		const tree = scanDesignDocs(docsDir);
		assert.equal(tree.nodes.size, 2);
		assert.ok(tree.nodes.has("auth-strategy"));
		assert.ok(tree.nodes.has("user-model"));
	});

	it("parses node metadata", () => {
		const tree = scanDesignDocs(docsDir);
		const auth = tree.nodes.get("auth-strategy")!;
		assert.equal(auth.status, "exploring");
		assert.equal(auth.parent, "security");
		assert.deepEqual(auth.dependencies, ["user-model", "session-mgmt"]);
	});

	it("syncs open questions from body", () => {
		const tree = scanDesignDocs(docsDir);
		const auth = tree.nodes.get("auth-strategy")!;
		// Body has 3 questions (includes "Rate limiting..." not in frontmatter)
		assert.equal(auth.open_questions.length, 3);
	});
});

// ─── Tree Queries ────────────────────────────────────────────────────────────

describe("tree queries", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
		writeDoc(docsDir, "root.md", `---\nid: root\ntitle: Root\nstatus: exploring\nopen_questions:\n  - "Q1"\n---\n\n# Root\n\n## Open Questions\n\n- Q1\n`);
		writeDoc(docsDir, "child1.md", `---\nid: child1\ntitle: Child 1\nstatus: seed\nparent: root\nopen_questions:\n  - "CQ1"\n---\n\n# Child 1\n\n## Open Questions\n\n- CQ1\n`);
		writeDoc(docsDir, "child2.md", `---\nid: child2\ntitle: Child 2\nstatus: decided\nparent: root\nopen_questions: []\n---\n\n# Child 2\n\n## Open Questions\n\n*No open questions.*\n`);
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("getChildren returns children", () => {
		const tree = scanDesignDocs(docsDir);
		const children = getChildren(tree, "root");
		assert.equal(children.length, 2);
	});

	it("getRoots returns root nodes", () => {
		const tree = scanDesignDocs(docsDir);
		const roots = getRoots(tree);
		assert.equal(roots.length, 1);
		assert.equal(roots[0].id, "root");
	});

	it("getAllOpenQuestions aggregates", () => {
		const tree = scanDesignDocs(docsDir);
		const questions = getAllOpenQuestions(tree);
		assert.equal(questions.length, 2); // Q1 from root, CQ1 from child1
	});
});

// ─── Mutations ───────────────────────────────────────────────────────────────

describe("createNode", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("creates a node with structured sections", () => {
		const node = createNode(docsDir, {
			id: "new-node",
			title: "New Design Node",
			overview: "This is a test node.",
		});

		assert.equal(node.id, "new-node");
		assert.equal(node.status, "seed");
		assert.ok(fs.existsSync(node.filePath));

		const content = fs.readFileSync(node.filePath, "utf-8");
		assert.ok(content.includes("## Overview"));
		assert.ok(content.includes("This is a test node."));
		assert.ok(content.includes("## Open Questions"));
	});

	it("creates a branched node with spawn context", () => {
		const node = createNode(docsDir, {
			id: "branched",
			title: "Branched Node",
			parent: "new-node",
			spawnedFrom: {
				parentTitle: "New Design Node",
				parentFile: "new-node.md",
				question: "What about edge cases?",
			},
		});

		assert.equal(node.parent, "new-node");
		const content = fs.readFileSync(node.filePath, "utf-8");
		assert.ok(content.includes("Spawned from:"));
		assert.ok(content.includes("What about edge cases?"));
	});
});

describe("setNodeStatus", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("changes status in frontmatter", () => {
		const node = createNode(docsDir, { id: "status-test", title: "Status Test" });
		assert.equal(node.status, "seed");

		const updated = setNodeStatus(node, "exploring");
		assert.equal(updated.status, "exploring");

		const content = fs.readFileSync(node.filePath, "utf-8");
		const fm = parseFrontmatter(content);
		assert.equal(fm?.status, "exploring");
	});
});

describe("addOpenQuestion / removeOpenQuestion", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("adds and removes questions in body and frontmatter", () => {
		const node = createNode(docsDir, { id: "q-test", title: "Question Test" });
		assert.equal(node.open_questions.length, 0);

		const after1 = addOpenQuestion(node, "First question?");
		assert.equal(after1.open_questions.length, 1);

		const after2 = addOpenQuestion(after1, "Second question?");
		assert.equal(after2.open_questions.length, 2);

		// Verify it's in the body
		const content = fs.readFileSync(node.filePath, "utf-8");
		assert.ok(content.includes("- First question?"));
		assert.ok(content.includes("- Second question?"));

		// Verify frontmatter is synced
		const fm = parseFrontmatter(content);
		assert.ok(fm);
		assert.ok((fm.open_questions as string[]).includes("First question?"));

		// Remove
		const after3 = removeOpenQuestion(after2, "First question?");
		assert.equal(after3.open_questions.length, 1);
		assert.equal(after3.open_questions[0], "Second question?");
	});
});

describe("addResearch", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("adds research entry to document", () => {
		const node = createNode(docsDir, { id: "research-test", title: "Research Test" });
		addResearch(node, "Performance Analysis", "Benchmarks show 2x improvement with caching.");

		const content = fs.readFileSync(node.filePath, "utf-8");
		assert.ok(content.includes("## Research"));
		assert.ok(content.includes("### Performance Analysis"));
		assert.ok(content.includes("Benchmarks show 2x improvement"));
	});
});

describe("addDecision", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("adds decision to document", () => {
		const node = createNode(docsDir, { id: "decision-test", title: "Decision Test" });
		addDecision(node, {
			title: "Use PostgreSQL",
			status: "decided",
			rationale: "Best fit for relational data with JSONB support.",
		});

		const content = fs.readFileSync(node.filePath, "utf-8");
		assert.ok(content.includes("## Decisions"));
		assert.ok(content.includes("### Decision: Use PostgreSQL"));
		assert.ok(content.includes("**Status:** decided"));
		assert.ok(content.includes("JSONB support"));
	});
});

describe("addImplementationNotes", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("adds file scope and constraints", () => {
		const node = createNode(docsDir, { id: "impl-test", title: "Impl Test" });
		addImplementationNotes(node, {
			fileScope: [
				{ path: "src/db/schema.ts", description: "Database schema definitions" },
			],
			constraints: ["Must support SQLite fallback"],
		});

		const content = fs.readFileSync(node.filePath, "utf-8");
		assert.ok(content.includes("## Implementation Notes"));
		assert.ok(content.includes("`src/db/schema.ts`"));
		assert.ok(content.includes("SQLite fallback"));
	});
});

describe("branchFromQuestion", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("creates child and removes question from parent", () => {
		const parent = createNode(docsDir, { id: "branch-parent", title: "Parent" });
		addOpenQuestion(parent, "Should we use caching?");
		addOpenQuestion(parent, "What about rate limiting?");

		const tree = scanDesignDocs(docsDir);
		const child = branchFromQuestion(
			tree, "branch-parent", "Should we use caching?",
			"caching-strategy", "Caching Strategy",
		);

		assert.ok(child);
		assert.equal(child.id, "caching-strategy");
		assert.equal(child.parent, "branch-parent");

		// Parent should no longer have the branched question
		const parentContent = fs.readFileSync(parent.filePath, "utf-8");
		assert.ok(!parentContent.includes("Should we use caching?"));
		assert.ok(parentContent.includes("What about rate limiting?"));

		// Child should have the question
		const childContent = fs.readFileSync(child.filePath, "utf-8");
		assert.ok(childContent.includes("Should we use caching?"));
		assert.ok(childContent.includes("Spawned from:"));
	});

	it("returns null for non-existent parent", () => {
		const tree = scanDesignDocs(docsDir);
		const result = branchFromQuestion(tree, "nonexistent", "Q?", "child", "Child");
		assert.equal(result, null);
	});
});

// ─── Validation ──────────────────────────────────────────────────────────────

describe("validateNodeId", () => {
	it("accepts valid IDs", () => {
		assert.equal(validateNodeId("auth-strategy"), null);
		assert.equal(validateNodeId("user-model"), null);
		assert.equal(validateNodeId("a"), null);
		assert.equal(validateNodeId("foo_bar-123"), null);
	});

	it("rejects path traversal", () => {
		assert.ok(validateNodeId("../etc/passwd"));
		assert.ok(validateNodeId("foo/bar"));
		assert.ok(validateNodeId(".."));
	});

	it("rejects dot-prefixed IDs", () => {
		assert.ok(validateNodeId(".hidden"));
		assert.ok(validateNodeId(".ssh"));
	});

	it("rejects empty and too-long IDs", () => {
		assert.ok(validateNodeId(""));
		assert.ok(validateNodeId("a".repeat(81)));
	});

	it("rejects uppercase and special characters", () => {
		assert.ok(validateNodeId("UpperCase"));
		assert.ok(validateNodeId("has spaces"));
		assert.ok(validateNodeId("has@special"));
	});
});

describe("createNode validation", () => {
	let tmpDir: string;

	before(() => { tmpDir = makeTmpDir(); });
	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("throws on invalid ID", () => {
		assert.throws(
			() => createNode(path.join(tmpDir, "docs"), { id: "../evil", title: "Evil" }),
			/Invalid node ID/,
		);
	});

	it("throws on uppercase ID", () => {
		assert.throws(
			() => createNode(path.join(tmpDir, "docs"), { id: "BadId", title: "Bad" }),
			/Invalid node ID/,
		);
	});
});

// ─── Slug ────────────────────────────────────────────────────────────────────

describe("toSlug", () => {
	it("converts title to slug", () => {
		assert.equal(toSlug("Authentication Strategy"), "authentication-strategy");
	});

	it("handles special characters", () => {
		assert.equal(toSlug("What about rate limiting?"), "what-about-rate-limiting");
	});

	it("truncates to maxLen", () => {
		const slug = toSlug("This is a very long title that should be truncated", 20);
		assert.ok(slug.length <= 20);
	});
});

// ─── Open Questions Edge Cases ───────────────────────────────────────────────

describe("parseOpenQuestionsSection edge cases", () => {
	it("handles empty section", () => {
		const sections = parseSections("# Title\n\n## Open Questions\n\n");
		assert.equal(sections.openQuestions.length, 0);
	});

	it("ignores placeholder text", () => {
		const sections = parseSections("# Title\n\n## Open Questions\n\n*No open questions.*\n");
		assert.equal(sections.openQuestions.length, 0);
	});

	it("parses numbered lists", () => {
		const sections = parseSections("# Title\n\n## Open Questions\n\n1. First\n2. Second\n");
		assert.equal(sections.openQuestions.length, 2);
		assert.equal(sections.openQuestions[0], "First");
		assert.equal(sections.openQuestions[1], "Second");
	});

	it("parses asterisk bullets", () => {
		const sections = parseSections("# Title\n\n## Open Questions\n\n* Bullet one\n* Bullet two\n");
		assert.equal(sections.openQuestions.length, 2);
	});
});

// ─── Bidirectional Related ───────────────────────────────────────────────────

describe("addRelated bidirectional", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("adds reciprocal link when target node provided", () => {
		const nodeA = createNode(docsDir, { id: "node-a", title: "Node A" });
		const nodeB = createNode(docsDir, { id: "node-b", title: "Node B" });

		addRelated(nodeA, "node-b", nodeB);

		const tree = scanDesignDocs(docsDir);
		const a = tree.nodes.get("node-a")!;
		const b = tree.nodes.get("node-b")!;

		assert.ok(a.related.includes("node-b"));
		assert.ok(b.related.includes("node-a"));
	});

	it("does not duplicate existing reciprocal", () => {
		const tree = scanDesignDocs(docsDir);
		const a = tree.nodes.get("node-a")!;
		const b = tree.nodes.get("node-b")!;

		// Call again — should not duplicate
		addRelated(a, "node-b", b);

		const tree2 = scanDesignDocs(docsDir);
		const a2 = tree2.nodes.get("node-a")!;
		const b2 = tree2.nodes.get("node-b")!;

		assert.equal(a2.related.filter((r) => r === "node-b").length, 1);
		assert.equal(b2.related.filter((r) => r === "node-a").length, 1);
	});
});

// ─── File Scope Action Parsing ───────────────────────────────────────────────

describe("file scope action parsing", () => {
	it("parses action from markdown", () => {
		const body = `# Title\n\n## Implementation Notes\n\n### File Scope\n\n- \`src/new.ts\` (new) — New file\n- \`src/mod.ts\` (modified) — Modified file\n- \`src/del.ts\` (deleted) — Removed file\n- \`src/plain.ts\` — No action\n`;
		const sections = parseSections(body);
		assert.equal(sections.implementationNotes.fileScope.length, 4);
		assert.equal(sections.implementationNotes.fileScope[0].action, "new");
		assert.equal(sections.implementationNotes.fileScope[1].action, "modified");
		assert.equal(sections.implementationNotes.fileScope[2].action, "deleted");
		assert.equal(sections.implementationNotes.fileScope[3].action, undefined);
	});

	it("round-trips action through generate/parse", () => {
		const sections = parseSections(
			generateBody("Test", {
				overview: "Test",
				research: [],
				decisions: [],
				openQuestions: [],
				implementationNotes: {
					fileScope: [
						{ path: "src/a.ts", description: "A file", action: "modified" },
						{ path: "src/b.ts", description: "B file" },
					],
					constraints: [],
					rawContent: "",
				},
				extraSections: [],
			}),
		);
		assert.equal(sections.implementationNotes.fileScope[0].action, "modified");
		assert.equal(sections.implementationNotes.fileScope[1].action, undefined);
	});
});

// ─── scaffoldOpenSpecChange ──────────────────────────────────────────────────

describe("scaffoldOpenSpecChange", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("scaffolds proposal, design, and tasks from a decided node with decisions", () => {
		const node = createNode(docsDir, { id: "auth-strategy", title: "Auth Strategy", status: "decided" });
		addDecision(node, { title: "Use JWT", status: "decided", rationale: "Stateless auth" });
		addDecision(node, { title: "Use refresh tokens", status: "decided", rationale: "Security" });
		addImplementationNotes(node, { fileScope: [{ path: "src/auth.ts", description: "Auth module", action: "new" }], constraints: ["Must support OIDC"] });

		const tree = scanDesignDocs(docsDir);
		const result = scaffoldOpenSpecChange(tmpDir, tree, tree.nodes.get("auth-strategy")!);

		assert.deepStrictEqual(result.files, ["proposal.md", "design.md", "tasks.md"]);

		// Check tasks.md format — decisions become task groups
		const tasks = fs.readFileSync(path.join(result.changePath, "tasks.md"), "utf-8");
		assert.ok(tasks.includes("## 1. Use JWT"));
		assert.ok(tasks.includes("- [ ] 1.1 Implement Use JWT"));
		assert.ok(tasks.includes("## 2. Use refresh tokens"));

		// Check design.md has decisions, file changes, and constraints
		const design = fs.readFileSync(path.join(result.changePath, "design.md"), "utf-8");
		assert.ok(design.includes("### Decision: Use JWT"));
		assert.ok(design.includes("`src/auth.ts` (new)"));
		assert.ok(design.includes("Must support OIDC"));

		// Check proposal.md
		const proposal = fs.readFileSync(path.join(result.changePath, "proposal.md"), "utf-8");
		assert.ok(proposal.includes("# Auth Strategy"));
	});

	it("scaffolds with child nodes as task groups", () => {
		const parent = createNode(docsDir, { id: "data-layer", title: "Data Layer", status: "decided" });
		createNode(docsDir, { id: "data-models", title: "Data Models", parent: "data-layer" });
		createNode(docsDir, { id: "data-access", title: "Data Access", parent: "data-layer" });

		const tree = scanDesignDocs(docsDir);
		const result = scaffoldOpenSpecChange(tmpDir, tree, tree.nodes.get("data-layer")!);

		const tasks = fs.readFileSync(path.join(result.changePath, "tasks.md"), "utf-8");
		// Children are returned in scan order (alphabetical by filename)
		assert.ok(tasks.includes("## 1. Data Access"));
		assert.ok(tasks.includes("## 2. Data Models"));
		assert.ok(tasks.includes("- [ ] 1.1 Implement Data Access"));
	});

	it("refuses to overwrite existing scaffold", () => {
		const tree = scanDesignDocs(docsDir);
		const result = scaffoldOpenSpecChange(tmpDir, tree, tree.nodes.get("auth-strategy")!);

		assert.deepStrictEqual(result.files, []);
		assert.ok(result.message.includes("already exists"));
	});

	it("generates single task group for node without children or decisions", () => {
		const node = createNode(docsDir, { id: "simple-task", title: "Simple Task", status: "decided" });
		const tree = scanDesignDocs(docsDir);
		const result = scaffoldOpenSpecChange(tmpDir, tree, tree.nodes.get("simple-task")!);

		const tasks = fs.readFileSync(path.join(result.changePath, "tasks.md"), "utf-8");
		assert.ok(tasks.includes("## 1. Simple Task"));
		assert.ok(tasks.includes("- [ ] 1.1 Implement Simple Task"));
	});

	it("tasks.md format is compatible with cleave numbered group pattern", () => {
		const tree = scanDesignDocs(docsDir);
		const tasks = fs.readFileSync(path.join(tmpDir, "openspec", "changes", "auth-strategy", "tasks.md"), "utf-8");

		// Verify cleave's expected patterns: `## N. Title` and `- [ ] N.M description`
		const groupPattern = /^## \d+\. .+$/m;
		const taskPattern = /^- \[ \] \d+\.\d+ .+$/m;
		assert.ok(groupPattern.test(tasks), "tasks.md must have ## N. Title groups");
		assert.ok(taskPattern.test(tasks), "tasks.md must have - [ ] N.M task items");
	});
});

// ─── Full Round-Trip ─────────────────────────────────────────────────────────

describe("full round-trip: create → mutate → scan → verify", () => {
	let tmpDir: string;
	let docsDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		docsDir = path.join(tmpDir, "docs");
	});

	after(() => {
		fs.rmSync(tmpDir, { recursive: true, force: true });
	});

	it("creates, modifies, and scans correctly", () => {
		// Create root node
		createNode(docsDir, {
			id: "api-design",
			title: "API Design",
			overview: "Designing the REST API.",
			tags: ["api", "rest"],
		});

		// Add questions
		let tree = scanDesignDocs(docsDir);
		let node = tree.nodes.get("api-design")!;
		addOpenQuestion(node, "Pagination strategy?");
		addOpenQuestion(node, "Authentication model?");

		// Add research
		addResearch(node, "REST vs GraphQL", "REST is simpler for our use case. GraphQL adds complexity.");

		// Add decision
		addDecision(node, {
			title: "Use REST with versioned endpoints",
			status: "decided",
			rationale: "Simpler, better tooling support, team familiarity.",
		});

		// Set status
		tree = scanDesignDocs(docsDir);
		node = tree.nodes.get("api-design")!;
		setNodeStatus(node, "exploring");

		// Branch
		tree = scanDesignDocs(docsDir);
		branchFromQuestion(tree, "api-design", "Authentication model?", "auth-model", "Auth Model");

		// Add implementation notes
		tree = scanDesignDocs(docsDir);
		node = tree.nodes.get("api-design")!;
		addImplementationNotes(node, {
			fileScope: [{ path: "src/api/routes.ts", description: "Route definitions" }],
			constraints: ["Must support API versioning via URL prefix"],
		});

		// Final scan — verify everything
		tree = scanDesignDocs(docsDir);

		// Root node
		const api = tree.nodes.get("api-design")!;
		assert.equal(api.status, "exploring");
		assert.equal(api.open_questions.length, 1); // "Pagination strategy?" remains
		assert.ok(api.open_questions.includes("Pagination strategy?"));
		assert.deepEqual(api.tags, ["api", "rest"]);

		// Child node
		const auth = tree.nodes.get("auth-model")!;
		assert.equal(auth.parent, "api-design");
		assert.ok(auth.open_questions.includes("Authentication model?"));

		// Sections
		const sections = getNodeSections(api);
		assert.equal(sections.research.length, 1);
		assert.equal(sections.decisions.length, 1);
		assert.equal(sections.decisions[0].status, "decided");
		assert.equal(sections.implementationNotes.fileScope.length, 1);
		assert.equal(sections.implementationNotes.constraints.length, 1);

		// Children query
		const children = getChildren(tree, "api-design");
		assert.equal(children.length, 1);
		assert.equal(children[0].id, "auth-model");
	});
});

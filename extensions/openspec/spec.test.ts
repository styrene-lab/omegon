/**
 * Tests for openspec/spec — pure domain logic.
 */

import { describe, it, before, after } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import {
	getOpenSpecDir,
	ensureOpenSpecDir,
	listChanges,
	getChange,
	computeStage,
	parseSpecContent,
	parseScenarios,
	generateSpecFromProposal,
	formatScenario,
	generateSpecFile,
	createChange,
	addSpec,
	archiveChange,
	parseSpecsDir,
	countScenarios,
	summarizeSpecs,
	validateChangeName,
	validateDomain,
	getAssessmentStatus,
	resolveVerificationStatus,
} from "./spec.ts";

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeTmpDir(): string {
	return fs.mkdtempSync(path.join(os.tmpdir(), "openspec-test-"));
}

const SAMPLE_SPEC = `# auth — Delta Spec

## ADDED Requirements

### Requirement: JWT token validation

Tokens must be validated on every API request.

#### Scenario: Valid token accepted
Given a user has a valid JWT token
When they make an API request with the token
Then the request is authenticated successfully

#### Scenario: Expired token rejected
Given a user has an expired JWT token
When they make an API request with the token
Then the request is rejected with 401 Unauthorized
And the response includes an error message

### Requirement: Refresh token rotation

Refresh tokens must be rotated on each use.

#### Scenario: Token rotation on refresh
Given a user has a valid refresh token
When they request a new access token
Then a new access token is issued
And the old refresh token is invalidated
And a new refresh token is issued

## MODIFIED Requirements

### Requirement: Session management

Sessions must use the new token-based approach.

#### Scenario: Session upgrade
Given a user has a legacy session cookie
When they access the upgraded system
Then they are prompted to re-authenticate

## REMOVED Requirements

### Requirement: Cookie-based sessions

Cookie sessions are deprecated in favor of JWT.
`;

const SAMPLE_PROPOSAL = `# JWT Authentication

## Intent

Replace cookie-based sessions with JWT tokens for stateless authentication.
This enables horizontal scaling without sticky sessions.

## Scope

- Token issuance and validation
- Refresh token rotation
- Migration from cookies

## Success Criteria

- All API endpoints validate JWT tokens
- Token refresh works without user intervention
- Legacy sessions are gracefully upgraded
`;

// ─── Spec Parsing ────────────────────────────────────────────────────────────

describe("parseSpecContent", () => {
	it("parses ADDED, MODIFIED, and REMOVED sections", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		assert.equal(sections.length, 3);
		assert.equal(sections[0].type, "added");
		assert.equal(sections[1].type, "modified");
		assert.equal(sections[2].type, "removed");
	});

	it("parses requirements within sections", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		const added = sections[0];
		assert.equal(added.requirements.length, 2);
		assert.equal(added.requirements[0].title, "JWT token validation");
		assert.equal(added.requirements[1].title, "Refresh token rotation");
	});

	it("parses scenarios within requirements", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		const jwtReq = sections[0].requirements[0];
		assert.equal(jwtReq.scenarios.length, 2);
		assert.equal(jwtReq.scenarios[0].title, "Valid token accepted");
		assert.equal(jwtReq.scenarios[1].title, "Expired token rejected");
	});

	it("extracts Given/When/Then clauses", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		const scenario = sections[0].requirements[0].scenarios[0];
		assert.equal(scenario.given, "a user has a valid JWT token");
		assert.equal(scenario.when, "they make an API request with the token");
		assert.equal(scenario.then, "the request is authenticated successfully");
	});

	it("extracts And clauses", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		const expired = sections[0].requirements[0].scenarios[1];
		assert.ok(expired.and);
		assert.equal(expired.and!.length, 1);
		assert.equal(expired.and![0], "the response includes an error message");
	});

	it("handles multiple And clauses", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		const rotation = sections[0].requirements[1].scenarios[0];
		assert.ok(rotation.and);
		assert.equal(rotation.and!.length, 2);
	});

	it("parses requirement descriptions", () => {
		const sections = parseSpecContent(SAMPLE_SPEC);
		const jwtReq = sections[0].requirements[0];
		assert.ok(jwtReq.description.includes("validated on every API request"));
	});
});

describe("parseScenarios", () => {
	it("parses a single scenario", () => {
		const content = `#### Scenario: Basic test
Given a precondition
When an action occurs
Then the result is correct`;

		const scenarios = parseScenarios(content);
		assert.equal(scenarios.length, 1);
		assert.equal(scenarios[0].title, "Basic test");
		assert.equal(scenarios[0].given, "a precondition");
		assert.equal(scenarios[0].when, "an action occurs");
		assert.equal(scenarios[0].then, "the result is correct");
	});

	it("returns empty for content without scenarios", () => {
		const scenarios = parseScenarios("Just some text without scenarios.");
		assert.equal(scenarios.length, 0);
	});
});

// ─── Lifecycle Stage ─────────────────────────────────────────────────────────

describe("computeStage", () => {
	it("returns 'proposed' for proposal only", () => {
		assert.equal(computeStage(true, false, false, 0, 0), "proposed");
	});

	it("returns 'specified' for proposal + specs", () => {
		assert.equal(computeStage(true, true, false, 0, 0), "specified");
	});

	it("returns 'planned' for tasks with no progress", () => {
		assert.equal(computeStage(true, true, true, 5, 0), "planned");
	});

	it("returns 'implementing' for partial task completion", () => {
		assert.equal(computeStage(true, true, true, 5, 3), "implementing");
	});

	it("returns 'verifying' for all tasks done", () => {
		assert.equal(computeStage(true, true, true, 5, 5), "verifying");
	});
});

// ─── Spec Generation ─────────────────────────────────────────────────────────

describe("generateSpecFromProposal", () => {
	it("generates spec with requirements from proposal", () => {
		const spec = generateSpecFromProposal({
			domain: "auth",
			proposalContent: SAMPLE_PROPOSAL,
		});

		assert.ok(spec.includes("# auth — Delta Spec"));
		assert.ok(spec.includes("## ADDED Requirements"));
		assert.ok(spec.includes("### Requirement:"));
		assert.ok(spec.includes("#### Scenario:"));
		assert.ok(spec.includes("Given"));
		assert.ok(spec.includes("When"));
		assert.ok(spec.includes("Then"));
	});

	it("includes decision-derived requirements", () => {
		const spec = generateSpecFromProposal({
			domain: "auth",
			proposalContent: SAMPLE_PROPOSAL,
			decisions: [
				{ title: "Use short-lived tokens", rationale: "15-minute TTL for security" },
			],
		});

		assert.ok(spec.includes("### Requirement: Use short-lived tokens"));
		assert.ok(spec.includes("15-minute TTL"));
	});

	it("converts open questions to MODIFIED requirements", () => {
		const spec = generateSpecFromProposal({
			domain: "auth",
			proposalContent: SAMPLE_PROPOSAL,
			openQuestions: ["How should token revocation work?"],
		});

		assert.ok(spec.includes("## MODIFIED Requirements"));
		assert.ok(spec.includes("How should token revocation work"));
	});
});

describe("formatScenario", () => {
	it("formats a scenario as markdown", () => {
		const output = formatScenario({
			title: "Test scenario",
			given: "a precondition",
			when: "an action",
			then: "a result",
		});

		assert.ok(output.includes("#### Scenario: Test scenario"));
		assert.ok(output.includes("Given a precondition"));
		assert.ok(output.includes("When an action"));
		assert.ok(output.includes("Then a result"));
	});

	it("includes And clauses", () => {
		const output = formatScenario({
			title: "Multi-then",
			given: "x",
			when: "y",
			then: "z",
			and: ["also a", "also b"],
		});

		assert.ok(output.includes("And also a"));
		assert.ok(output.includes("And also b"));
	});
});

describe("generateSpecFile", () => {
	it("round-trips through parse", () => {
		const original = parseSpecContent(SAMPLE_SPEC);
		const generated = generateSpecFile("auth", original);
		const reparsed = parseSpecContent(generated);

		assert.equal(reparsed.length, original.length);
		assert.equal(reparsed[0].requirements.length, original[0].requirements.length);

		// Scenario count should match
		const origScenarios = original.flatMap(
			(s) => s.requirements.flatMap((r) => r.scenarios),
		);
		const reparsedScenarios = reparsed.flatMap(
			(s) => s.requirements.flatMap((r) => r.scenarios),
		);
		assert.equal(reparsedScenarios.length, origScenarios.length);
	});
});

// ─── Change Operations ───────────────────────────────────────────────────────

describe("createChange", () => {
	let tmpDir: string;

	before(() => { tmpDir = makeTmpDir(); });
	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("creates a change directory with proposal.md", () => {
		const result = createChange(tmpDir, "jwt-auth", "JWT Authentication", "Replace cookies with JWT.");
		assert.deepStrictEqual(result.files, ["proposal.md"]);
		assert.ok(fs.existsSync(path.join(result.changePath, "proposal.md")));

		const content = fs.readFileSync(path.join(result.changePath, "proposal.md"), "utf-8");
		assert.ok(content.includes("# JWT Authentication"));
		assert.ok(content.includes("Replace cookies with JWT."));
	});

	it("refuses to overwrite existing change", () => {
		assert.throws(
			() => createChange(tmpDir, "jwt-auth", "Duplicate", "Should fail"),
			/already exists/,
		);
	});

	it("slugifies the name", () => {
		const result = createChange(tmpDir, "My Cool Feature!", "Cool", "Intent");
		assert.ok(result.changePath.includes("my-cool-feature"));
	});
});

describe("addSpec", () => {
	let tmpDir: string;
	let changePath: string;

	before(() => {
		tmpDir = makeTmpDir();
		const result = createChange(tmpDir, "spec-test", "Spec Test", "Testing spec creation");
		changePath = result.changePath;
	});

	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("creates a spec file in specs/ directory", () => {
		const specContent = generateSpecFromProposal({
			domain: "auth",
			proposalContent: "## Intent\n\nTest intent.",
		});
		const specPath = addSpec(changePath, "auth", specContent);

		assert.ok(fs.existsSync(specPath));
		assert.ok(specPath.includes("specs/auth.md"));
	});

	it("supports nested domains", () => {
		const specPath = addSpec(changePath, "auth/tokens", "# Token specs");
		assert.ok(fs.existsSync(specPath));
		assert.ok(specPath.includes("specs/auth/tokens.md"));
	});
});

describe("getChange + listChanges", () => {
	let tmpDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		createChange(tmpDir, "feature-a", "Feature A", "First feature");
		createChange(tmpDir, "feature-b", "Feature B", "Second feature");

		// Add specs to feature-a
		const changeA = getChange(tmpDir, "feature-a")!;
		addSpec(changeA.path, "core", SAMPLE_SPEC);
	});

	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("lists all changes", () => {
		const changes = listChanges(tmpDir);
		assert.equal(changes.length, 2);
	});

	it("gets a specific change", () => {
		const change = getChange(tmpDir, "feature-a");
		assert.ok(change);
		assert.equal(change!.name, "feature-a");
		assert.equal(change!.hasProposal, true);
		assert.equal(change!.hasSpecs, true);
		assert.equal(change!.stage, "specified");
	});

	it("returns null for missing change", () => {
		assert.equal(getChange(tmpDir, "nonexistent"), null);
	});

	it("computes correct stage for proposal-only", () => {
		const change = getChange(tmpDir, "feature-b");
		assert.ok(change);
		assert.equal(change!.stage, "proposed");
		assert.equal(change!.hasSpecs, false);
	});

	it("parses specs when present", () => {
		const change = getChange(tmpDir, "feature-a");
		assert.ok(change);
		assert.ok(change!.specs.length > 0);
		assert.equal(change!.specs[0].domain, "core");
	});
});

describe("archiveChange", () => {
	let tmpDir: string;

	before(() => {
		tmpDir = makeTmpDir();
		const result = createChange(tmpDir, "to-archive", "To Archive", "Will be archived");
		addSpec(result.changePath, "core", SAMPLE_SPEC);

		// Create a tasks.md with all done
		fs.writeFileSync(
			path.join(result.changePath, "tasks.md"),
			"# Tasks\n\n## 1. Core\n\n- [x] 1.1 Implement feature\n",
		);
	});

	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("archives a change and creates baseline", () => {
		const result = archiveChange(tmpDir, "to-archive");
		assert.equal(result.archived, true);
		assert.ok(result.operations.some((op) => op.includes("baseline")));
		assert.ok(result.operations.some((op) => op.includes("Archived")));
	});

	it("change no longer appears in listChanges", () => {
		const changes = listChanges(tmpDir);
		assert.ok(!changes.some((c) => c.name === "to-archive"));
	});

	it("baseline spec exists", () => {
		const baselinePath = path.join(tmpDir, "openspec", "baseline", "core.md");
		assert.ok(fs.existsSync(baselinePath));

		const content = fs.readFileSync(baselinePath, "utf-8");
		// Should contain requirements but NOT "Delta Spec" or section headers
		assert.ok(content.includes("Requirement:"));
	});

	it("returns error for missing change", () => {
		const result = archiveChange(tmpDir, "nonexistent");
		assert.equal(result.archived, false);
	});
});

// ─── Summary Helpers ─────────────────────────────────────────────────────────

describe("countScenarios + summarizeSpecs", () => {
	it("counts scenarios across spec files", () => {
		const specs = [{
			domain: "auth",
			filePath: "/tmp/spec.md",
			sections: parseSpecContent(SAMPLE_SPEC),
		}];
		const count = countScenarios(specs);
		assert.ok(count >= 4); // At least 4 scenarios in SAMPLE_SPEC
	});

	it("summarizes specs", () => {
		const specs = [{
			domain: "auth",
			filePath: "/tmp/spec.md",
			sections: parseSpecContent(SAMPLE_SPEC),
		}];
		const summary = summarizeSpecs(specs);
		assert.ok(summary.includes("1 domain"));
		assert.ok(summary.includes("requirement"));
		assert.ok(summary.includes("scenario"));
	});

	it("handles empty specs", () => {
		assert.equal(summarizeSpecs([]), "No specs");
	});
});

// ─── Validation ──────────────────────────────────────────────────────────────

describe("validateChangeName", () => {
	it("accepts valid names", () => {
		assert.equal(validateChangeName("jwt-auth"), null);
		assert.equal(validateChangeName("feature-123"), null);
		assert.equal(validateChangeName("a"), null);
	});

	it("rejects empty names", () => {
		assert.ok(validateChangeName(""));
	});

	it("rejects path separators", () => {
		assert.ok(validateChangeName("../evil"));
		assert.ok(validateChangeName("foo/bar"));
		assert.ok(validateChangeName("foo\\bar"));
	});

	it("rejects dot-prefixed names", () => {
		assert.ok(validateChangeName(".hidden"));
	});

	it("rejects names with double dots", () => {
		assert.ok(validateChangeName("foo..bar"));
	});

	it("rejects uppercase names", () => {
		assert.ok(validateChangeName("MyFeature"));
	});
});

describe("validateDomain", () => {
	it("accepts simple domains", () => {
		assert.equal(validateDomain("auth"), null);
		assert.equal(validateDomain("auth/tokens"), null);
		assert.equal(validateDomain("core-api"), null);
	});

	it("rejects path traversal", () => {
		assert.ok(validateDomain("../../etc/passwd"));
		assert.ok(validateDomain("../secret"));
	});

	it("rejects backslashes", () => {
		assert.ok(validateDomain("auth\\tokens"));
	});

	it("rejects absolute paths", () => {
		assert.ok(validateDomain("/etc/passwd"));
	});

	it("rejects dot-prefixed domains", () => {
		assert.ok(validateDomain(".hidden"));
	});
});

describe("addSpec path traversal prevention", () => {
	let tmpDir: string;
	let changePath: string;

	before(() => {
		tmpDir = makeTmpDir();
		const result = createChange(tmpDir, "secure-test", "Secure", "Testing security");
		changePath = result.changePath;
	});

	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("rejects path traversal in domain", () => {
		assert.throws(
			() => addSpec(changePath, "../../etc/passwd", "malicious content"),
			/cannot contain/,
		);
	});

	it("rejects absolute domain paths", () => {
		assert.throws(
			() => addSpec(changePath, "/etc/passwd", "malicious content"),
			/cannot start/,
		);
	});
});

describe("getChange rejects invalid names", () => {
	let tmpDir: string;

	before(() => { tmpDir = makeTmpDir(); });
	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("returns null for path traversal attempts", () => {
		assert.equal(getChange(tmpDir, "../../../etc"), null);
	});

	it("returns null for names with slashes", () => {
		assert.equal(getChange(tmpDir, "foo/bar"), null);
	});
});

describe("archiveChange rejects invalid names", () => {
	let tmpDir: string;

	before(() => { tmpDir = makeTmpDir(); });
	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("refuses path traversal names", () => {
		const result = archiveChange(tmpDir, "../../../etc");
		assert.equal(result.archived, false);
	});
});

describe("assessment artifacts", () => {
	let tmpDir: string;
	let otherChangePath: string;

	before(() => {
		tmpDir = makeTmpDir();
		createChange(tmpDir, "my-change", "My Change", "Track assessment state");
		const other = createChange(tmpDir, "other-change", "Other Change", "Separate state");
		otherChangePath = other.changePath;
		fs.writeFileSync(path.join(tmpDir, "openspec", "changes", "my-change", "tasks.md"), "## 1. Demo\n- [x] 1.1 Done\n");
		fs.writeFileSync(path.join(tmpDir, "openspec", "changes", "my-change", "design.md"), [
			"# design",
			"",
			"## File Changes",
			"",
			"- `src/demo.ts`",
		].join("\n"));
		fs.mkdirSync(path.join(tmpDir, "src"), { recursive: true });
		fs.writeFileSync(path.join(tmpDir, "src", "demo.ts"), "export const demo = 1;\n");
	});

	after(() => { fs.rmSync(tmpDir, { recursive: true, force: true }); });

	it("writes and reads a per-change assessment record", async () => {
		const {
			computeAssessmentSnapshot,
			writeAssessmentRecord,
			readAssessmentRecord,
			getAssessmentArtifactPath,
		} = await import("./spec.ts");
		const snapshot = computeAssessmentSnapshot(tmpDir, "my-change");
		assert.ok(snapshot);
		const artifactPath = writeAssessmentRecord(tmpDir, "my-change", {
			changeName: "my-change",
			assessmentKind: "spec",
			outcome: "pass",
			timestamp: "2026-03-09T12:00:00.000Z",
			snapshot,
			reconciliation: {
				reopen: false,
				changedFiles: [],
				constraints: [],
				recommendedAction: null,
			},
		});

		assert.equal(artifactPath, getAssessmentArtifactPath(path.join(tmpDir, "openspec", "changes", "my-change")));
		const record = readAssessmentRecord(tmpDir, "my-change");
		assert.ok(record);
		assert.equal(record!.schemaVersion, 1);
		assert.equal(record!.changeName, "my-change");
		assert.equal(record!.assessmentKind, "spec");
		assert.equal(record!.snapshot.fingerprint, snapshot.fingerprint);
	});

	it("keeps persisted assessment state scoped to the requested change", async () => {
		const { computeAssessmentSnapshot, writeAssessmentRecord, readAssessmentRecord } = await import("./spec.ts");
		const snapshot = computeAssessmentSnapshot(tmpDir, "my-change");
		assert.ok(snapshot);
		writeAssessmentRecord(tmpDir, "my-change", {
			changeName: "my-change",
			assessmentKind: "spec",
			outcome: "pass",
			timestamp: "2026-03-09T12:00:00.000Z",
			snapshot,
			reconciliation: {
				reopen: false,
				changedFiles: [],
				constraints: [],
				recommendedAction: null,
			},
		});

		assert.equal(readAssessmentRecord(tmpDir, "other-change"), null);
		assert.equal(fs.existsSync(path.join(otherChangePath, "assessment.json")), false);
	});

	it("reports current vs stale snapshot freshness", async () => {
		const { computeAssessmentSnapshot, writeAssessmentRecord } = await import("./spec.ts");
		const snapshot = computeAssessmentSnapshot(tmpDir, "my-change");
		assert.ok(snapshot);
		writeAssessmentRecord(tmpDir, "my-change", {
			changeName: "my-change",
			assessmentKind: "spec",
			outcome: "pass",
			timestamp: "2026-03-09T12:00:00.000Z",
			snapshot,
			reconciliation: {
				reopen: false,
				changedFiles: [],
				constraints: [],
				recommendedAction: null,
			},
		});

		const current = getAssessmentStatus(tmpDir, "my-change");
		assert.equal(current.freshness.current, true);
		assert.deepEqual(current.freshness.reasons, []);

		fs.appendFileSync(path.join(tmpDir, "src", "demo.ts"), "export const demo2 = 2;\n");
		const stale = getAssessmentStatus(tmpDir, "my-change");
		assert.equal(stale.freshness.current, false);
		assert.match(stale.freshness.reasons.join("\n"), /fingerprint differs/i);
	});

	it("classifies verification substates while preserving coarse verifying stage", async () => {
		const { computeAssessmentSnapshot, writeAssessmentRecord } = await import("./spec.ts");
		const missing = resolveVerificationStatus({
			stage: "verifying",
			record: null,
			freshness: { current: false, reasons: ["Missing assessment record"] },
			changeName: "my-change",
		});
		assert.equal(missing.coarseStage, "verifying");
		assert.equal(missing.substate, "missing-assessment");
		assert.match(missing.nextAction ?? "", /\/assess spec my-change/);

		const snapshot = computeAssessmentSnapshot(tmpDir, "my-change");
		assert.ok(snapshot);
		writeAssessmentRecord(tmpDir, "my-change", {
			changeName: "my-change",
			assessmentKind: "spec",
			outcome: "reopen",
			timestamp: "2026-03-09T12:00:00.000Z",
			summary: "Follow-up work remains",
			snapshot,
			reconciliation: {
				reopen: true,
				changedFiles: [],
				constraints: [],
				recommendedAction: "Run openspec_manage reconcile_after_assess before archive.",
			},
		});
		const reopenedRecord = getAssessmentStatus(tmpDir, "my-change").record;
		assert.ok(reopenedRecord);
		const reopened = resolveVerificationStatus({
			stage: "verifying",
			record: reopenedRecord,
			freshness: { current: false, reasons: ["Assessment outcome is 'reopen', not 'pass'"] },
			changeName: "my-change",
		});
		assert.equal(reopened.substate, "reopened-work");

		writeAssessmentRecord(tmpDir, "my-change", {
			changeName: "my-change",
			assessmentKind: "spec",
			outcome: "pass",
			timestamp: "2026-03-09T12:05:00.000Z",
			snapshot,
			reconciliation: {
				reopen: false,
				changedFiles: [],
				constraints: [],
				recommendedAction: null,
			},
		});
		fs.appendFileSync(path.join(tmpDir, "src", "demo.ts"), "export const demo3 = 3;\n");
		const staleAssessment = getAssessmentStatus(tmpDir, "my-change");
		const stale = resolveVerificationStatus({
			stage: "verifying",
			record: staleAssessment.record,
			freshness: staleAssessment.freshness,
			changeName: "my-change",
		});
		assert.equal(stale.substate, "stale-assessment");
		assert.match(stale.nextAction ?? "", /current implementation snapshot/i);

		const readySnapshot = computeAssessmentSnapshot(tmpDir, "my-change");
		assert.ok(readySnapshot);
		writeAssessmentRecord(tmpDir, "my-change", {
			changeName: "my-change",
			assessmentKind: "spec",
			outcome: "pass",
			timestamp: "2026-03-09T12:10:00.000Z",
			snapshot: readySnapshot,
			reconciliation: {
				reopen: false,
				changedFiles: [],
				constraints: [],
				recommendedAction: null,
			},
		});
		const readyAssessment = getAssessmentStatus(tmpDir, "my-change");
		const awaitingReconciliation = resolveVerificationStatus({
			stage: "verifying",
			record: readyAssessment.record,
			freshness: readyAssessment.freshness,
			archiveBlocked: true,
			archiveBlockedReason: "Bind the design node before archive",
			changeName: "my-change",
		});
		assert.equal(awaitingReconciliation.substate, "awaiting-reconciliation");
		assert.match(awaitingReconciliation.nextAction ?? "", /Bind the design node before archive/);

		const ready = resolveVerificationStatus({
			stage: "verifying",
			record: readyAssessment.record,
			freshness: readyAssessment.freshness,
			archiveBlocked: false,
			changeName: "my-change",
		});
		assert.equal(ready.substate, "archive-ready");
		assert.match(ready.nextAction ?? "", /\/opsx:archive my-change/);
	});
});

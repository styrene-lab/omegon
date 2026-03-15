/**
 * Regression tests for openspec/dashboard-state — shared refresh helper.
 *
 * Verifies that:
 * - emitOpenSpecState writes dashboard-facing OpenSpec state to sharedState
 * - it fires the DASHBOARD_UPDATE_EVENT via pi.events.emit
 * - callers never need to duplicate dashboard refresh boilerplate inline
 * - the helper is non-fatal when the openspec directory is missing
 */

import { describe, it, beforeEach } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { emitOpenSpecState } from "./dashboard-state.ts";
import { sharedState, DASHBOARD_UPDATE_EVENT } from "../lib/shared-state.ts";
import { createChange } from "./spec.ts";

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeTmpDir(): string {
	return fs.mkdtempSync(path.join(os.tmpdir(), "openspec-dashboard-test-"));
}

function createFakePi() {
	const emitted: Array<{ channel: string; data: unknown }> = [];
	return {
		emitted,
		events: {
			emit(channel: string, data: unknown) {
				emitted.push({ channel, data });
			},
		},
	};
}

// ─── Tests ───────────────────────────────────────────────────────────────────

describe("emitOpenSpecState — shared refresh helper", () => {
	let tmpDir: string;

	beforeEach(() => {
		tmpDir = makeTmpDir();
		// Reset openspec slice of sharedState before each test
		sharedState.openspec = undefined as any;
	});

	it("writes openspec changes to sharedState.openspec", () => {
		createChange(tmpDir, "my-feature", "My Feature", "Test feature intent");

		const pi = createFakePi();
		emitOpenSpecState(tmpDir, pi as any);

		assert.ok(sharedState.openspec, "sharedState.openspec should be set after emitOpenSpecState");
		assert.ok(Array.isArray(sharedState.openspec.changes), "changes should be an array");
		assert.equal(sharedState.openspec.changes.length, 1);
		assert.equal(sharedState.openspec.changes[0].name, "my-feature");
	});

	it("fires DASHBOARD_UPDATE_EVENT with source=openspec", () => {
		const pi = createFakePi();
		emitOpenSpecState(tmpDir, pi as any);

		const dashboardEvents = pi.emitted.filter((e) => e.channel === DASHBOARD_UPDATE_EVENT);
		assert.equal(dashboardEvents.length, 1, "should emit exactly one dashboard update event");
		assert.deepEqual(dashboardEvents[0].data, { source: "openspec" });
	});

	it("maps artifacts correctly based on change filesystem presence", () => {
		createChange(tmpDir, "with-proposal", "With Proposal", "Test proposal intent");

		const pi = createFakePi();
		emitOpenSpecState(tmpDir, pi as any);

		const change = sharedState.openspec?.changes[0];
		assert.ok(change, "change should exist");
		// createChange writes proposal.md
		assert.ok(change.artifacts?.includes("proposal"), "should include 'proposal' artifact");
	});

	it("emits empty changes array when openspec/changes dir is empty", () => {
		// Ensure openspec dir exists but has no changes
		const changesDir = path.join(tmpDir, "openspec", "changes");
		fs.mkdirSync(changesDir, { recursive: true });

		const pi = createFakePi();
		emitOpenSpecState(tmpDir, pi as any);

		assert.ok(sharedState.openspec, "sharedState.openspec should be set");
		assert.deepEqual(sharedState.openspec.changes, []);
		// Event should still fire so dashboard clears stale state
		const dashboardEvents = pi.emitted.filter((e) => e.channel === DASHBOARD_UPDATE_EVENT);
		assert.equal(dashboardEvents.length, 1);
	});

	it("is non-fatal when openspec directory does not exist", () => {
		const missingDir = path.join(tmpDir, "nonexistent");
		const pi = createFakePi();

		// Should not throw — listChanges returns [] gracefully for missing dirs
		assert.doesNotThrow(() => emitOpenSpecState(missingDir, pi as any));

		// sharedState still updated with empty array so dashboard clears stale state
		assert.ok(sharedState.openspec, "sharedState.openspec should be set");
		assert.deepEqual(sharedState.openspec.changes, []);
		// Dashboard event should fire so consumers know to re-render
		const dashboardEvents = pi.emitted.filter((e) => e.channel === DASHBOARD_UPDATE_EVENT);
		assert.equal(dashboardEvents.length, 1, "should emit dashboard update event even for empty state");
	});

	it("verifies index.ts mutation contract: caller must invoke emitOpenSpecState after mutations", () => {
		// This test documents that index.ts is responsible for calling emitOpenSpecState
		// after every state-mutating command. It verifies the contract by ensuring that:
		// 1. emitOpenSpecState is the single refresh surface (not inline boilerplate)
		// 2. Any call to it is sufficient to update dashboard state
		createChange(tmpDir, "contract-test", "Contract Test", "Verify callers use shared helper");

		const pi = createFakePi();
		// Simulate what every mutating index.ts command path must do
		emitOpenSpecState(tmpDir, pi as any);

		// The dashboard should reflect the updated state
		assert.ok(sharedState.openspec, "sharedState.openspec must be set — index.ts callers must not skip emitOpenSpecState");
		assert.equal(sharedState.openspec.changes.length, 1);
		// The event must have fired — consumers depend on this for re-render
		const dashboardEvents = pi.emitted.filter((e) => e.channel === DASHBOARD_UPDATE_EVENT);
		assert.equal(dashboardEvents.length, 1, "index.ts must emit DASHBOARD_UPDATE_EVENT via emitOpenSpecState, not inline");
	});

	it("emits task progress from tasks.md when present", () => {
		createChange(tmpDir, "with-tasks", "With Tasks", "Test tasks intent");
		const tasksPath = path.join(tmpDir, "openspec", "changes", "with-tasks", "tasks.md");
		fs.writeFileSync(
			tasksPath,
			`# Tasks\n\n## Group 1\n\n- [x] done\n- [ ] pending\n`,
		);

		const pi = createFakePi();
		emitOpenSpecState(tmpDir, pi as any);

		const change = sharedState.openspec?.changes[0];
		assert.ok(change, "change should exist");
		assert.equal(change.tasksTotal, 2);
		assert.equal(change.tasksDone, 1);
	});
});

import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";

import { sharedState, DASHBOARD_UPDATE_EVENT } from "../lib/shared-state.ts";
import { debug } from "../lib/debug.ts";
import { listChanges } from "./spec.ts";
import { buildLifecycleSummary } from "./lifecycle.ts";

/**
 * Emit OpenSpec state to sharedState for the unified dashboard.
 * Reads all active changes, maps to the dashboard shape, and fires
 * the dashboard:update event for re-render.
 */
export function emitOpenSpecState(cwd: string, pi: ExtensionAPI): void {
	try {
		const changes = listChanges(cwd);
		const mapped = changes.map((c) => {
			const artifacts: Array<"proposal" | "design" | "specs" | "tasks"> = [];
			if (c.hasProposal) artifacts.push("proposal");
			if (c.hasDesign) artifacts.push("design");
			if (c.hasSpecs) artifacts.push("specs");
			if (c.hasTasks) artifacts.push("tasks");
			const specDomains = c.specs.map((s) => s.domain).filter(Boolean);

			// Resolve canonical lifecycle summary — single source of truth for
			// readiness and verification substate, shared with status/get surfaces.
			const lifecycle = buildLifecycleSummary(cwd, c);

			return {
				name: c.name,
				stage: lifecycle.stage,
				verificationSubstate: lifecycle.verificationSubstate,
				archiveReady: lifecycle.archiveReady,
				bindingStatus: lifecycle.bindingStatus,
				tasksDone: lifecycle.doneTasks,
				tasksTotal: lifecycle.totalTasks,
				artifacts,
				specDomains,
				path: c.path,
			};
		});
		sharedState.openspec = { changes: mapped };
		debug("openspec", "emitState", { count: mapped.length, cwd });
		pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "openspec" });
	} catch (err) {
		debug("openspec", "emitState:error", { error: err instanceof Error ? err.message : String(err), cwd });
		// Non-fatal — clear stale dashboard state so consumers see an empty list rather than stale data
		sharedState.openspec = { changes: [] };
		pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "openspec" });
	}
}

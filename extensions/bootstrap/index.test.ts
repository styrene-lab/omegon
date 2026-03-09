import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

import {
	buildGuidedProfile,
	loadOperatorProfile,
	needsOperatorProfileSetup,
	saveOperatorProfile,
	summarizeProviderReadiness,
	synthesizeSafeDefaultProfile,
	type OperatorCapabilityProfile,
} from "./index.ts";
import type { AuthResult } from "../01-auth/auth.ts";

function makeTmpDir(): string {
	return mkdtempSync(join(tmpdir(), "bootstrap-profile-"));
}

describe("bootstrap operator profile helpers", () => {
	it("reports setup needed when no operator profile exists", () => {
		const tmp = makeTmpDir();
		try {
			assert.equal(needsOperatorProfileSetup(tmp), true);
		} finally {
			rmSync(tmp, { recursive: true, force: true });
		}
	});

	it("persists operator profile without clobbering unrelated config keys", () => {
		const tmp = makeTmpDir();
		mkdirSync(join(tmp, ".pi"), { recursive: true });
		writeFileSync(join(tmp, ".pi", "config.json"), JSON.stringify({ editor: "vscode" }));
		const profile = buildGuidedProfile({
			primaryProvider: "openai",
			allowCloudCrossProviderFallback: true,
			automaticLightLocalFallback: false,
			heavyLocalFallback: "deny",
		});

		try {
			saveOperatorProfile(tmp, profile);
			const loaded = loadOperatorProfile(tmp);
			assert.deepEqual(loaded, profile);
			const config = JSON.parse(readFileSync(join(tmp, ".pi", "config.json"), "utf-8")) as {
				editor?: string;
				operatorProfile?: OperatorCapabilityProfile;
			};
			assert.equal(config.editor, "vscode");
			assert.deepEqual(config.operatorProfile, profile);
			assert.equal(needsOperatorProfileSetup(tmp), false);
		} finally {
			rmSync(tmp, { recursive: true, force: true });
		}
	});

	it("summarizes provider readiness from reused auth results", () => {
		const results: AuthResult[] = [
			{ provider: "github", status: "ok", detail: "ready" },
			{ provider: "gitlab", status: "expired", detail: "token expired" },
			{ provider: "aws", status: "missing", detail: "aws cli not installed" },
			{ provider: "git", status: "ok", detail: "ignored non-cloud provider" },
		];
		assert.deepEqual(summarizeProviderReadiness(results), {
			ready: ["github"],
			authAttention: ["gitlab"],
			missing: ["aws"],
		});
	});

	it("synthesizes conservative defaults when setup is skipped", () => {
		const profile = synthesizeSafeDefaultProfile([
			{ provider: "github", status: "ok", detail: "ready" },
			{ provider: "aws", status: "ok", detail: "ready" },
		]);
		assert.equal(profile.setupComplete, false);
		assert.equal(profile.setupState, "skipped-default");
		assert.deepEqual(profile.providerOrder, ["anthropic", "openai", "local"]);
		assert.equal(profile.providerPreferences.local, "avoid");
		assert.equal(profile.fallbackPolicy.sameRoleCrossProvider, "allow");
		assert.equal(profile.fallbackPolicy.crossSource, "ask");
		assert.equal(profile.fallbackPolicy.heavyLocal, "ask");
		assert.equal(profile.fallbackPolicy.unknownLocalPerformance, "ask");
	});

	it("builds guided profile from qualitative setup answers", () => {
		const profile = buildGuidedProfile({
			primaryProvider: "openai",
			allowCloudCrossProviderFallback: false,
			automaticLightLocalFallback: true,
			heavyLocalFallback: "deny",
		});
		assert.equal(profile.setupComplete, true);
		assert.equal(profile.setupState, "guided");
		assert.deepEqual(profile.providerOrder, ["openai", "anthropic", "local"]);
		assert.equal(profile.providerPreferences.openai, "prefer");
		assert.equal(profile.providerPreferences.local, "allow");
		assert.equal(profile.fallbackPolicy.sameRoleCrossProvider, "ask");
		assert.equal(profile.fallbackPolicy.crossSource, "ask");
		assert.equal(profile.fallbackPolicy.heavyLocal, "deny");
		assert.equal(profile.fallbackPolicy.unknownLocalPerformance, "ask");
	});

	it("ignores invalid operator profile payloads", () => {
		const tmp = makeTmpDir();
		mkdirSync(join(tmp, ".pi"), { recursive: true });
		writeFileSync(join(tmp, ".pi", "config.json"), JSON.stringify({
			operatorProfile: { version: 1, setupComplete: "yes" },
		}));
		try {
			assert.equal(loadOperatorProfile(tmp), undefined);
			assert.equal(needsOperatorProfileSetup(tmp), true);
		} finally {
			rmSync(tmp, { recursive: true, force: true });
		}
	});
});

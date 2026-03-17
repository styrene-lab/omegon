import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";

const OMEGON_BIN = join(process.cwd(), "bin", "omegon.mjs");
const PI_BIN = join(process.cwd(), "bin", "pi.mjs");

function makeTmpDir(prefix: string): string {
	return mkdtempSync(join(tmpdir(), prefix));
}

function makeEnv(overrides: Record<string, string | undefined> = {}): NodeJS.ProcessEnv {
	const env = { ...process.env, ...overrides };
	delete env.PI_CODING_AGENT_DIR;
	return env;
}

describe("omegon executable --where", () => {
	it("prints Omegon resolution metadata without starting interactive mode", () => {
		const result = spawnSync(process.execPath, [OMEGON_BIN, "--where"], {
			encoding: "utf8",
			env: makeEnv(),
		});
		assert.equal(result.status, 0, result.stderr);
		const data = JSON.parse(result.stdout);
		assert.match(data.omegonRoot, /omegon$/);
		assert.match(data.cli, /(packages[\\/]coding-agent|node_modules[\\/]@styrene-lab[\\/]pi-coding-agent)[\\/]dist[\\/]cli\.js$/);
		assert.ok(data.resolutionMode === "vendor" || data.resolutionMode === "npm");
		assert.equal(data.executable, "omegon");
		assert.equal(data.agentDir, data.stateDir);
		assert.match(data.stateDir, /[\\/]\.pi[\\/]agent$/);
	});

	it("honors explicit PI_CODING_AGENT_DIR overrides in --where metadata", () => {
		const customStateDir = join(makeTmpDir("omegon-state-"), "custom-agent");
		try {
			const result = spawnSync(process.execPath, [OMEGON_BIN, "--where"], {
				encoding: "utf8",
				env: { ...makeEnv(), PI_CODING_AGENT_DIR: customStateDir },
			});
			assert.equal(result.status, 0, result.stderr);
			const data = JSON.parse(result.stdout);
			assert.equal(data.stateDir, customStateDir);
			assert.equal(data.agentDir, customStateDir);
		} finally {
			rmSync(join(customStateDir, ".."), { recursive: true, force: true });
		}
	});

	it("lets the legacy pi alias re-enter the same omegon-owned runtime", () => {
		const result = spawnSync(process.execPath, [PI_BIN, "--where"], {
			encoding: "utf8",
			env: makeEnv(),
		});
		assert.equal(result.status, 0, result.stderr);
		const data = JSON.parse(result.stdout);
		assert.match(data.omegonRoot, /omegon$/);
		assert.equal(data.executable, "omegon");
		assert.equal(data.agentDir, data.stateDir);
	});
});

describe("omegon startup state migration", () => {
	it("migrates legacy package-root auth and settings into the shared state dir", () => {
		const fakeHome = makeTmpDir("omegon-home-");
		const fakeInstallRoot = makeTmpDir("omegon-install-");
		const fakeBinDir = join(fakeInstallRoot, "bin");
		const fakeNodeModulesDir = join(fakeInstallRoot, "node_modules", "@styrene-lab", "pi-coding-agent", "dist");
		const sharedStateDir = join(fakeHome, ".pi", "agent");
		const cliStub = join(fakeNodeModulesDir, "cli.js");
		const omegonShim = join(fakeBinDir, "omegon.mjs");
		try {
			mkdirSync(fakeBinDir, { recursive: true });
			mkdirSync(fakeNodeModulesDir, { recursive: true });
			writeFileSync(join(fakeInstallRoot, "auth.json"), JSON.stringify({ provider: "anthropic" }));
			writeFileSync(join(fakeInstallRoot, "settings.json"), JSON.stringify({ theme: "alpharius" }));
			writeFileSync(cliStub, "process.stdout.write(JSON.stringify({ agentDir: process.env.PI_CODING_AGENT_DIR, argv: process.argv.slice(2) }));\n");
			writeFileSync(omegonShim, readFileSync(OMEGON_BIN, "utf8"));

			const result = spawnSync(process.execPath, [omegonShim, "--print", "hello"], {
				encoding: "utf8",
				env: makeEnv({ HOME: fakeHome }),
			});
			assert.equal(result.status, 0, result.stderr);
			assert.deepEqual(JSON.parse(readFileSync(join(sharedStateDir, "auth.json"), "utf8")), { provider: "anthropic" });
			const migratedSettings = JSON.parse(readFileSync(join(sharedStateDir, "settings.json"), "utf8"));
			assert.equal(migratedSettings.theme, "alpharius");
			assert.equal(migratedSettings.quietStartup, true);
			assert.equal(migratedSettings.collapseChangelog, true);
			const data = JSON.parse(result.stdout);
			assert.equal(data.agentDir, sharedStateDir);
			assert.ok(data.argv.includes("--extension"));
			assert.ok(data.argv.some((value: string) => value.endsWith(fakeInstallRoot)));
			// Discovery suppression flags — omegon injects --no-* to disable
			// auto-discovery and only load manifest-declared resources.
			assert.ok(data.argv.includes("--no-skills"), "expected --no-skills flag");
			assert.ok(data.argv.includes("--no-prompt-templates"), "expected --no-prompt-templates flag");
			assert.ok(data.argv.includes("--no-themes"), "expected --no-themes flag");
		} finally {
			rmSync(fakeHome, { recursive: true, force: true });
			rmSync(fakeInstallRoot, { recursive: true, force: true });
		}
	});

	it("does not overwrite existing shared auth during migration", () => {
		const fakeHome = makeTmpDir("omegon-home-");
		const fakeInstallRoot = makeTmpDir("omegon-install-");
		const fakeBinDir = join(fakeInstallRoot, "bin");
		const fakeNodeModulesDir = join(fakeInstallRoot, "node_modules", "@styrene-lab", "pi-coding-agent", "dist");
		const sharedStateDir = join(fakeHome, ".pi", "agent");
		const cliStub = join(fakeNodeModulesDir, "cli.js");
		const omegonShim = join(fakeBinDir, "omegon.mjs");
		try {
			mkdirSync(fakeBinDir, { recursive: true });
			mkdirSync(fakeNodeModulesDir, { recursive: true });
			mkdirSync(sharedStateDir, { recursive: true });
			writeFileSync(join(fakeInstallRoot, "auth.json"), JSON.stringify({ provider: "legacy-install" }));
			writeFileSync(join(sharedStateDir, "auth.json"), JSON.stringify({ provider: "shared-user" }));
			writeFileSync(cliStub, "process.stdout.write(JSON.stringify({ agentDir: process.env.PI_CODING_AGENT_DIR }));\n");
			writeFileSync(omegonShim, readFileSync(OMEGON_BIN, "utf8"));

			const result = spawnSync(process.execPath, [omegonShim, "--print", "hello"], {
				encoding: "utf8",
				env: makeEnv({ HOME: fakeHome }),
			});
			assert.equal(result.status, 0, result.stderr);
			assert.deepEqual(JSON.parse(readFileSync(join(sharedStateDir, "auth.json"), "utf8")), { provider: "shared-user" });
		} finally {
			rmSync(fakeHome, { recursive: true, force: true });
			rmSync(fakeInstallRoot, { recursive: true, force: true });
		}
	});
});

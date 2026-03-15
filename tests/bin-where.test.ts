import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const BIN = join(process.cwd(), "bin", "pi.mjs");

describe("bin/pi.mjs --where", () => {
	it("prints Omegon resolution metadata without starting interactive mode", () => {
		const result = spawnSync(process.execPath, [BIN, "--where"], {
			encoding: "utf8",
			env: { ...process.env },
		});
		assert.equal(result.status, 0, result.stderr);
		const data = JSON.parse(result.stdout);
		assert.match(data.omegonRoot, /omegon$/);
		assert.match(data.cli, /(packages[\\/]coding-agent|node_modules[\\/]@cwilson613[\\/]pi-coding-agent)[\\/]dist[\\/]cli\.js$/);
		assert.ok(data.resolutionMode === "vendor" || data.resolutionMode === "npm");
	});
});

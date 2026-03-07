/**
 * bootstrap — First-time setup and dependency management for pi-kit.
 *
 * On first session start after install, presents a friendly checklist of
 * external dependencies grouped by tier (core / recommended / optional).
 * Offers interactive installation for missing deps.
 *
 * Commands:
 *   /bootstrap          — Run interactive setup (install missing deps)
 *   /bootstrap status   — Show dependency checklist without installing
 *   /bootstrap install  — Install all missing core + recommended deps
 *
 * Guards:
 *   - First-run detection via ~/.pi/agent/pi-kit-bootstrap-done marker
 *   - Re-running /bootstrap is always safe (idempotent checks)
 *   - Never auto-installs anything — always asks or requires explicit command
 */

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { DEPS, checkAll, formatReport, bestInstallCmd, sortByRequires, type DepStatus, type DepTier } from "./deps.js";

const AGENT_DIR = join(homedir(), ".pi", "agent");
const MARKER_PATH = join(AGENT_DIR, "pi-kit-bootstrap-done");
const MARKER_VERSION = "1"; // bump to re-trigger bootstrap after adding new core deps

function isFirstRun(): boolean {
	if (!existsSync(MARKER_PATH)) return true;
	try {
		const version = readFileSync(MARKER_PATH, "utf8").trim();
		return version !== MARKER_VERSION;
	} catch {
		return true;
	}
}

function markDone(): void {
	mkdirSync(AGENT_DIR, { recursive: true });
	writeFileSync(MARKER_PATH, MARKER_VERSION + "\n", "utf8");
}

interface CommandContext {
	say: (msg: string) => void;
	hasUI?: boolean;
	ui?: {
		notify: (msg: string, level: string) => void;
		confirm: (title: string, message: string) => Promise<boolean>;
	};
}

export default function (pi: ExtensionAPI) {
	// --- First-run detection on session start ---
	pi.on("session_start", async (_event, ctx) => {
		if (!isFirstRun()) return;
		if (!ctx.hasUI) return;

		const statuses = checkAll();
		const missing = statuses.filter((s) => !s.available);

		if (missing.length === 0) {
			markDone();
			return;
		}

		const coreMissing = missing.filter((s) => s.dep.tier === "core");
		const recMissing = missing.filter((s) => s.dep.tier === "recommended");

		let msg = "Welcome to pi-kit! ";
		if (coreMissing.length > 0) {
			msg += `${coreMissing.length} core dep${coreMissing.length > 1 ? "s" : ""} missing. `;
		}
		if (recMissing.length > 0) {
			msg += `${recMissing.length} recommended dep${recMissing.length > 1 ? "s" : ""} missing. `;
		}
		msg += "Run /bootstrap to set up.";

		ctx.ui.notify(msg, coreMissing.length > 0 ? "warning" : "info");
	});

	pi.registerCommand("bootstrap", {
		description: "First-time setup — check and install pi-kit external dependencies",
		handler: async (args, ctx) => {
			const sub = args.trim().toLowerCase();
			const cmdCtx: CommandContext = {
				say: (msg: string) => ctx.ui.notify(msg, "info"),
				hasUI: true,
				ui: {
					notify: (msg: string, level: string) => ctx.ui.notify(msg, level as "info"),
					confirm: (title: string, message: string) => ctx.ui.confirm(title, message),
				},
			};

			if (sub === "status") {
				const statuses = checkAll();
				cmdCtx.say(formatReport(statuses));
				return;
			}

			if (sub === "install") {
				await installMissing(cmdCtx, ["core", "recommended"]);
				return;
			}

			await interactiveSetup(cmdCtx);
		},
	});
}

async function interactiveSetup(ctx: CommandContext): Promise<void> {
	const statuses = checkAll();
	const missing = statuses.filter((s) => !s.available);

	ctx.say(formatReport(statuses));

	if (missing.length === 0) {
		markDone();
		return;
	}

	if (!ctx.hasUI || !ctx.ui) {
		ctx.say("\nRun individual install commands above, or use `/bootstrap install` to install all core + recommended deps.");
		return;
	}

	const coreMissing = missing.filter((s) => s.dep.tier === "core");
	const recMissing = missing.filter((s) => s.dep.tier === "recommended");
	const optMissing = missing.filter((s) => s.dep.tier === "optional");

	if (coreMissing.length > 0) {
		const names = coreMissing.map((s) => s.dep.name).join(", ");
		const proceed = await ctx.ui.confirm(
			"Install core dependencies?",
			`${coreMissing.length} missing: ${names}`,
		);
		if (proceed) {
			await installDeps(ctx, coreMissing);
		}
	}

	if (recMissing.length > 0) {
		const names = recMissing.map((s) => s.dep.name).join(", ");
		const proceed = await ctx.ui.confirm(
			"Install recommended dependencies?",
			`${recMissing.length} missing: ${names}`,
		);
		if (proceed) {
			await installDeps(ctx, recMissing);
		}
	}

	if (optMissing.length > 0) {
		ctx.say(
			`\n${optMissing.length} optional dep${optMissing.length > 1 ? "s" : ""} not installed: ${optMissing.map((s) => s.dep.name).join(", ")}.\n` +
			`Install individually when needed — see \`/bootstrap status\` for commands.`,
		);
	}

	const recheck = checkAll();
	const stillMissing = recheck.filter((s) => !s.available && (s.dep.tier === "core" || s.dep.tier === "recommended"));

	if (stillMissing.length === 0) {
		ctx.say("\n🎉 Setup complete! All core and recommended dependencies are available.");
		markDone();
	} else {
		ctx.say(
			`\n⚠️  ${stillMissing.length} dep${stillMissing.length > 1 ? "s" : ""} still missing. ` +
			`Run \`/bootstrap\` again after installing manually.`,
		);
	}
}

async function installMissing(ctx: CommandContext, tiers: DepTier[]): Promise<void> {
	const statuses = checkAll();
	const toInstall = statuses.filter(
		(s) => !s.available && tiers.includes(s.dep.tier),
	);

	if (toInstall.length === 0) {
		ctx.say("All core and recommended dependencies are already installed. ✅");
		markDone();
		return;
	}

	await installDeps(ctx, toInstall);

	const recheck = checkAll();
	const stillMissing = recheck.filter(
		(s) => !s.available && tiers.includes(s.dep.tier),
	);
	if (stillMissing.length === 0) {
		ctx.say("\n🎉 All core and recommended dependencies installed!");
		markDone();
	} else {
		ctx.say(
			`\n⚠️  ${stillMissing.length} dep${stillMissing.length > 1 ? "s" : ""} failed to install:`,
		);
		for (const s of stillMissing) {
			const cmd = bestInstallCmd(s.dep);
			ctx.say(`  ❌ ${s.dep.name}: try manually → \`${cmd}\``);
		}
	}
}

/** Run a shell command asynchronously with streaming output, returning exit code */
function runAsync(cmd: string, timeoutMs: number = 300_000): Promise<number> {
	return new Promise((resolve) => {
		const child = spawn("sh", ["-c", cmd], {
			stdio: "inherit",
			env: { ...process.env, NONINTERACTIVE: "1", HOMEBREW_NO_AUTO_UPDATE: "1" },
		});

		const timer = setTimeout(() => {
			child.kill("SIGTERM");
			resolve(124); // timeout exit code
		}, timeoutMs);

		child.on("exit", (code) => {
			clearTimeout(timer);
			resolve(code ?? 1);
		});

		child.on("error", () => {
			clearTimeout(timer);
			resolve(1);
		});
	});
}

async function installDeps(ctx: CommandContext, deps: DepStatus[]): Promise<void> {
	// Sort so prerequisites come first (e.g., cargo before mdserve)
	const sorted = sortByRequires(deps);

	for (const { dep } of sorted) {
		// Check prerequisites — re-verify availability live (not from stale array)
		if (dep.requires?.length) {
			const unmet = dep.requires.filter((reqId) => {
				const reqDep = DEPS.find((d) => d.id === reqId);
				return reqDep ? !reqDep.check() : false;
			});
			if (unmet.length > 0) {
				ctx.say(`\n⚠️  Skipping ${dep.name} — requires ${unmet.join(", ")} (not available)`);
				continue;
			}
		}

		const cmd = bestInstallCmd(dep);
		if (!cmd) {
			ctx.say(`\n⚠️  No install command available for ${dep.name} on this platform`);
			continue;
		}

		ctx.say(`\n📦 Installing ${dep.name}...`);
		ctx.say(`   → \`${cmd}\``);

		const exitCode = await runAsync(cmd);

		if (exitCode === 0 && dep.check()) {
			ctx.say(`   ✅ ${dep.name} installed successfully`);
		} else if (exitCode === 124) {
			ctx.say(`   ❌ ${dep.name} install timed out (5 min limit)`);
		} else if (exitCode === 0) {
			ctx.say(`   ⚠️  Command succeeded but ${dep.name} not found on PATH. You may need to restart your shell.`);
		} else {
			ctx.say(`   ❌ Failed to install ${dep.name} (exit code ${exitCode})`);
			const hints = dep.install.filter((o) => o.cmd !== cmd);
			if (hints.length > 0) {
				ctx.say(`   Alternative: \`${hints[0].cmd}\``);
			}
			if (dep.url) {
				ctx.say(`   Manual install: ${dep.url}`);
			}
		}
	}
}

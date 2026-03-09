/**
 * bootstrap — First-time setup and dependency management for pi-kit.
 *
 * On first session start after install, presents a friendly checklist of
 * external dependencies grouped by tier (core / recommended / optional).
 * Offers interactive installation for missing deps and captures a safe
 * operator capability profile for routing/fallback defaults.
 *
 * Commands:
 *   /bootstrap          — Run interactive setup (install missing deps + profile)
 *   /bootstrap status   — Show dependency checklist without installing
 *   /bootstrap install  — Install all missing core + recommended deps
 *
 * Guards:
 *   - First-run detection via ~/.pi/agent/pi-kit-bootstrap-done marker
 *   - Re-running /bootstrap is always safe (idempotent checks)
 *   - Never auto-installs anything — always asks or requires explicit command
 */

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { homedir, tmpdir } from "node:os";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { checkAllProviders, type AuthResult } from "../01-auth/auth.ts";
import { loadPiConfig, savePiConfig } from "../lib/model-preferences.ts";
import { DEPS, checkAll, formatReport, bestInstallCmd, sortByRequires, type DepStatus, type DepTier } from "./deps.ts";

const AGENT_DIR = join(homedir(), ".pi", "agent");
const MARKER_PATH = join(AGENT_DIR, "pi-kit-bootstrap-done");
const MARKER_VERSION = "2"; // bump to re-trigger bootstrap after adding operator profile capture
const OPERATOR_PROFILE_VERSION = 1;

export type ProviderPreference = "prefer" | "allow" | "avoid";
export type LocalFallbackPolicy = "allow" | "ask" | "deny";

export interface OperatorCapabilityProfile {
	version: number;
	setupComplete: boolean;
	setupState: "guided" | "skipped-default";
	providerOrder: Array<"anthropic" | "openai" | "local">;
	providerPreferences: {
		anthropic: ProviderPreference;
		openai: ProviderPreference;
		local: ProviderPreference;
	};
	fallbackPolicy: {
		sameRoleCrossProvider: "allow" | "ask" | "deny";
		crossSource: "allow" | "ask" | "deny";
		heavyLocal: LocalFallbackPolicy;
		unknownLocalPerformance: LocalFallbackPolicy;
	};
}

interface PiConfigWithProfile {
	operatorProfile?: unknown;
	[key: string]: unknown;
}

interface ProviderReadinessSummary {
	ready: string[];
	authAttention: string[];
	missing: string[];
}

interface SetupAnswers {
	primaryProvider: "anthropic" | "openai" | "no-preference";
	allowCloudCrossProviderFallback: boolean;
	automaticLightLocalFallback: boolean;
	heavyLocalFallback: LocalFallbackPolicy;
}

interface CommandContext {
	say: (msg: string) => void;
	hasUI: boolean;
	cwd?: string;
	ui: {
		notify: (msg: string, level?: string) => void;
		confirm: (title: string, message: string) => Promise<boolean>;
		input?: (label: string, initial?: string) => Promise<string>;
		select?: (title: string, options: string[]) => Promise<string | undefined>;
	};
}

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

function parseOperatorProfile(value: unknown): OperatorCapabilityProfile | undefined {
	if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
	const record = value as Record<string, unknown>;
	if (record.version !== OPERATOR_PROFILE_VERSION) return undefined;
	if (typeof record.setupComplete !== "boolean") return undefined;
	if (record.setupState !== "guided" && record.setupState !== "skipped-default") return undefined;
	if (!Array.isArray(record.providerOrder) || record.providerOrder.some((item) => item !== "anthropic" && item !== "openai" && item !== "local")) {
		return undefined;
	}
	const providerPreferences = record.providerPreferences;
	const fallbackPolicy = record.fallbackPolicy;
	if (!providerPreferences || typeof providerPreferences !== "object" || Array.isArray(providerPreferences)) return undefined;
	if (!fallbackPolicy || typeof fallbackPolicy !== "object" || Array.isArray(fallbackPolicy)) return undefined;

	const prefs = providerPreferences as Record<string, unknown>;
	const fallback = fallbackPolicy as Record<string, unknown>;
	const validPreference = (valueToCheck: unknown): valueToCheck is ProviderPreference => valueToCheck === "prefer" || valueToCheck === "allow" || valueToCheck === "avoid";
	const validPolicy = (valueToCheck: unknown): valueToCheck is LocalFallbackPolicy => valueToCheck === "allow" || valueToCheck === "ask" || valueToCheck === "deny";
	if (!validPreference(prefs.anthropic) || !validPreference(prefs.openai) || !validPreference(prefs.local)) return undefined;
	if (!validPolicy(fallback.sameRoleCrossProvider) || !validPolicy(fallback.crossSource)
		|| !validPolicy(fallback.heavyLocal) || !validPolicy(fallback.unknownLocalPerformance)) return undefined;

	return record as unknown as OperatorCapabilityProfile;
}

export function loadOperatorProfile(root: string): OperatorCapabilityProfile | undefined {
	const config = loadPiConfig(root) as PiConfigWithProfile;
	return parseOperatorProfile(config.operatorProfile);
}

export function needsOperatorProfileSetup(root: string): boolean {
	const profile = loadOperatorProfile(root);
	return !profile;
}

export function summarizeProviderReadiness(results: AuthResult[]): ProviderReadinessSummary {
	const summary: ProviderReadinessSummary = { ready: [], authAttention: [], missing: [] };
	for (const result of results) {
		if (result.provider !== "github" && result.provider !== "gitlab" && result.provider !== "aws") continue;
		if (result.status === "ok") summary.ready.push(result.provider);
		else if (result.status === "missing") summary.missing.push(result.provider);
		else summary.authAttention.push(result.provider);
	}
	return summary;
}

export function synthesizeSafeDefaultProfile(readiness?: AuthResult[]): OperatorCapabilityProfile {
	const summary = readiness ? summarizeProviderReadiness(readiness) : { ready: [], authAttention: [], missing: [] };
	const preferredCloud: Array<"anthropic" | "openai"> = [];
	if (summary.ready.includes("github")) preferredCloud.push("anthropic");
	if (summary.ready.includes("aws") || summary.ready.includes("gitlab")) preferredCloud.push("openai");
	for (const provider of ["anthropic", "openai"] as const) {
		if (!preferredCloud.includes(provider)) preferredCloud.push(provider);
	}

	return {
		version: OPERATOR_PROFILE_VERSION,
		setupComplete: false,
		setupState: "skipped-default",
		providerOrder: [...preferredCloud, "local"],
		providerPreferences: {
			anthropic: preferredCloud[0] === "anthropic" ? "prefer" : "allow",
			openai: preferredCloud[0] === "openai" ? "prefer" : "allow",
			local: "avoid",
		},
		fallbackPolicy: {
			sameRoleCrossProvider: "allow",
			crossSource: "ask",
			heavyLocal: "ask",
			unknownLocalPerformance: "ask",
		},
	};
}

export function buildGuidedProfile(answers: SetupAnswers): OperatorCapabilityProfile {
	const providerOrder: Array<"anthropic" | "openai" | "local"> =
		answers.primaryProvider === "anthropic"
			? ["anthropic", "openai", "local"]
			: answers.primaryProvider === "openai"
				? ["openai", "anthropic", "local"]
				: ["anthropic", "openai", "local"];

	return {
		version: OPERATOR_PROFILE_VERSION,
		setupComplete: true,
		setupState: "guided",
		providerOrder,
		providerPreferences: {
			anthropic: answers.primaryProvider === "anthropic" ? "prefer" : "allow",
			openai: answers.primaryProvider === "openai" ? "prefer" : "allow",
			local: answers.automaticLightLocalFallback ? "allow" : "avoid",
		},
		fallbackPolicy: {
			sameRoleCrossProvider: answers.allowCloudCrossProviderFallback ? "allow" : "ask",
			crossSource: answers.automaticLightLocalFallback ? "ask" : "deny",
			heavyLocal: answers.heavyLocalFallback,
			unknownLocalPerformance: "ask",
		},
	};
}

export function saveOperatorProfile(root: string, profile: OperatorCapabilityProfile): void {
	const config = loadPiConfig(root) as PiConfigWithProfile;
	config.operatorProfile = profile;
	savePiConfig(root, config);
}

function formatProviderSetupSummary(results: AuthResult[]): string {
	const summary = summarizeProviderReadiness(results);
	const parts: string[] = [];
	if (summary.ready.length > 0) parts.push(`ready: ${summary.ready.join(", ")}`);
	if (summary.authAttention.length > 0) parts.push(`needs auth: ${summary.authAttention.join(", ")}`);
	if (summary.missing.length > 0) parts.push(`missing CLI: ${summary.missing.join(", ")}`);
	return parts.length > 0 ? parts.join(" · ") : "No cloud providers detected yet";
}

function getConfigRoot(ctx: { cwd?: string }): string {
	return ctx.cwd || process.cwd();
}

async function ensureOperatorProfile(pi: ExtensionAPI, ctx: CommandContext): Promise<OperatorCapabilityProfile> {
	const root = getConfigRoot(ctx);
	const existing = loadOperatorProfile(root);
	if (existing) return existing;

	const readiness = await checkAllProviders(pi);
	if (!ctx.hasUI || !ctx.ui.confirm || !ctx.ui.select) {
		const fallback = synthesizeSafeDefaultProfile(readiness);
		saveOperatorProfile(root, fallback);
		return fallback;
	}

	ctx.ui.notify(`Operator capability setup — ${formatProviderSetupSummary(readiness)}`, "info");
	const proceed = await ctx.ui.confirm(
		"Configure operator capability profile?",
		"This captures cloud/local fallback preferences so pi-kit avoids unsafe automatic model switches.",
	);
	if (!proceed) {
		const fallback = synthesizeSafeDefaultProfile(readiness);
		saveOperatorProfile(root, fallback);
		ctx.ui.notify("Saved a conservative default operator profile. You can rerun /bootstrap later to customize it.", "info");
		return fallback;
	}

	const primarySelection = await ctx.ui.select(
		"Preferred cloud provider for normal work:",
		[
			"Anthropic first",
			"OpenAI first",
			"No preference",
		],
	);
	const primaryProvider = primarySelection === "OpenAI first"
		? "openai"
		: primarySelection === "No preference"
			? "no-preference"
			: "anthropic";
	const allowCloudCrossProviderFallback = await ctx.ui.confirm(
		"Allow same-role cloud fallback?",
		"If your preferred cloud provider is unavailable, may pi-kit retry the same capability role with another cloud provider?",
	);
	const automaticLightLocalFallback = await ctx.ui.confirm(
		"Allow automatic light local fallback?",
		"Allow pi-kit to use local models automatically for lightweight work when cloud options are unavailable?",
	);
	const heavyLocalSelection = await ctx.ui.select(
		"Heavy local fallback policy:",
		[
			"Ask before heavy local fallback",
			"Deny heavy local fallback",
			"Allow heavy local fallback",
		],
	);
	const heavyLocalFallback = heavyLocalSelection === "Deny heavy local fallback"
		? "deny"
		: heavyLocalSelection === "Allow heavy local fallback"
			? "allow"
			: "ask";

	const profile = buildGuidedProfile({
		primaryProvider,
		allowCloudCrossProviderFallback,
		automaticLightLocalFallback,
		heavyLocalFallback,
	});
	saveOperatorProfile(root, profile);
	ctx.ui.notify("Saved operator capability profile to .pi/config.json", "info");
	return profile;
}

export default function (pi: ExtensionAPI) {
	// --- First-run detection on session start ---
	pi.on("session_start", async (_event, ctx) => {
		if (!isFirstRun()) return;
		if (!ctx.hasUI) return;

		const statuses = checkAll();
		const missing = statuses.filter((s) => !s.available);
		const needsProfile = needsOperatorProfileSetup(getConfigRoot(ctx));

		if (missing.length === 0 && !needsProfile) {
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
		if (needsProfile) {
			msg += "Operator capability setup is still pending. ";
		}
		msg += "Run /bootstrap to set up.";

		ctx.ui.notify(msg, coreMissing.length > 0 ? "warning" : "info");
	});

	pi.registerCommand("bootstrap", {
		description: "First-time setup — check/install pi-kit dependencies and capture operator fallback preferences",
		handler: async (args, ctx) => {
			const sub = args.trim().toLowerCase();
			const cmdCtx: CommandContext = {
				say: (msg: string) => ctx.ui.notify(msg, "info"),
				hasUI: true,
				cwd: ctx.cwd,
				ui: {
					notify: (msg: string, level?: string) => ctx.ui.notify(msg, (level ?? "info") as "info"),
					confirm: (title: string, message: string) => ctx.ui.confirm(title, message),
					input: ctx.ui.input ? async (label: string, initial?: string) => (await ctx.ui.input(label, initial)) ?? "" : undefined,
					select: ctx.ui.select ? (title: string, options: string[]) => ctx.ui.select(title, options) : undefined,
				},
			};

			if (sub === "status") {
				const statuses = checkAll();
				cmdCtx.say(formatReport(statuses));
				const profile = loadOperatorProfile(getConfigRoot(cmdCtx));
				cmdCtx.say(profile
					? `\nOperator capability profile: ${profile.setupComplete ? "configured" : "defaulted"}`
					: "\nOperator capability profile: not configured");
				return;
			}

			if (sub === "install") {
				await installMissing(cmdCtx, ["core", "recommended"]);
				await ensureOperatorProfile(pi, cmdCtx);
				return;
			}

			await interactiveSetup(pi, cmdCtx);
		},
	});

	// --- /refresh: clear jiti transpilation cache + reload ---
	// jiti's fs cache uses path-based hashing, so source changes aren't
	// detected on /reload. /refresh clears the cache first.
	pi.registerCommand("refresh", {
		description: "Clear transpilation cache and reload extensions",
		handler: async (_args, ctx) => {
			const jitiCacheDir = join(tmpdir(), "jiti");
			let cleared = 0;
			if (existsSync(jitiCacheDir)) {
				try {
					const files = readdirSync(jitiCacheDir);
					cleared = files.length;
					rmSync(jitiCacheDir, { recursive: true, force: true });
				} catch { /* best-effort */ }
			}
			ctx.ui.notify(cleared > 0
				? `Cleared ${cleared} cached transpilations. Reloading…`
				: "No transpilation cache found. Reloading…", "info");
			await ctx.reload();
		},
	});
}

async function interactiveSetup(pi: ExtensionAPI, ctx: CommandContext): Promise<void> {
	const statuses = checkAll();
	const missing = statuses.filter((s) => !s.available);

	ctx.ui.notify(formatReport(statuses));

	if (missing.length === 0 && !needsOperatorProfileSetup(getConfigRoot(ctx))) {
		markDone();
		return;
	}

	if (!ctx.hasUI || !ctx.ui) {
		ctx.ui.notify("\nRun individual install commands above, or use `/bootstrap install` to install all core + recommended deps.");
		await ensureOperatorProfile(pi, ctx);
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
		ctx.ui.notify(
			`\n${optMissing.length} optional dep${optMissing.length > 1 ? "s" : ""} not installed: ${optMissing.map((s) => s.dep.name).join(", ")}.\n`
			+ "Install individually when needed — see `/bootstrap status` for commands.",
		);
	}

	await ensureOperatorProfile(pi, ctx);

	const recheck = checkAll();
	const stillMissing = recheck.filter((s) => !s.available && (s.dep.tier === "core" || s.dep.tier === "recommended"));

	if (stillMissing.length === 0) {
		ctx.ui.notify("\n🎉 Setup complete! All core and recommended dependencies are available.");
		markDone();
	} else {
		ctx.ui.notify(
			`\n⚠️  ${stillMissing.length} dep${stillMissing.length > 1 ? "s" : ""} still missing. `
			+ "Run `/bootstrap` again after installing manually.",
		);
	}
}

async function installMissing(ctx: CommandContext, tiers: DepTier[]): Promise<void> {
	const statuses = checkAll();
	const toInstall = statuses.filter(
		(s) => !s.available && tiers.includes(s.dep.tier),
	);

	if (toInstall.length === 0) {
		ctx.ui.notify("All core and recommended dependencies are already installed. ✅");
		return;
	}

	await installDeps(ctx, toInstall);

	const recheck = checkAll();
	const stillMissing = recheck.filter(
		(s) => !s.available && tiers.includes(s.dep.tier),
	);
	if (stillMissing.length === 0) {
		ctx.ui.notify("\n🎉 All core and recommended dependencies installed!");
	} else {
		ctx.ui.notify(
			`\n⚠️  ${stillMissing.length} dep${stillMissing.length > 1 ? "s" : ""} failed to install:`,
		);
		for (const s of stillMissing) {
			const cmd = bestInstallCmd(s.dep);
			ctx.ui.notify(`  ❌ ${s.dep.name}: try manually → \`${cmd}\``);
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
				ctx.ui.notify(`\n⚠️  Skipping ${dep.name} — requires ${unmet.join(", ")} (not available)`);
				continue;
			}
		}

		const cmd = bestInstallCmd(dep);
		if (!cmd) {
			ctx.ui.notify(`\n⚠️  No install command available for ${dep.name} on this platform`);
			continue;
		}

		ctx.ui.notify(`\n📦 Installing ${dep.name}...`);
		ctx.ui.notify(`   → \`${cmd}\``);

		const exitCode = await runAsync(cmd);

		if (exitCode === 0 && dep.check()) {
			ctx.ui.notify(`   ✅ ${dep.name} installed successfully`);
		} else if (exitCode === 124) {
			ctx.ui.notify(`   ❌ ${dep.name} install timed out (5 min limit)`);
		} else if (exitCode === 0) {
			ctx.ui.notify(`   ⚠️  Command succeeded but ${dep.name} not found on PATH. You may need to restart your shell.`);
		} else {
			ctx.ui.notify(`   ❌ Failed to install ${dep.name} (exit code ${exitCode})`);
			const hints = dep.install.filter((o) => o.cmd !== cmd);
			if (hints.length > 0) {
				ctx.ui.notify(`   Alternative: \`${hints[0].cmd}\``);
			}
			if (dep.url) {
				ctx.ui.notify(`   Manual install: ${dep.url}`);
			}
		}
	}
}

/**
 * bootstrap — First-time setup and dependency management for Omegon.
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
 *   /update-pi          — Update pi binary to latest @cwilson613/pi-coding-agent release
 *   /update-pi --dry-run — Check for update without installing
 *
 * Guards:
 *   - First-run detection via ~/.pi/agent/omegon-bootstrap-done marker (checks pi-kit-bootstrap-done as legacy fallback)
 *   - Re-running /bootstrap is always safe (idempotent checks)
 *   - Never auto-installs anything — always asks or requires explicit command
 */

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir, tmpdir } from "node:os";
import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import { checkAllProviders, type AuthResult } from "../01-auth/auth.ts";
import { loadPiConfig } from "../lib/model-preferences.ts";
import {
	getDefaultOperatorProfile,
	parseOperatorProfile as parseCapabilityProfile,
	writeOperatorProfile as persistOperatorProfile,
	type OperatorCapabilityProfile,
	type OperatorProfileCandidate,
} from "../lib/operator-profile.ts";
import { sharedState } from "../lib/shared-state.ts";
import { getDefaultPolicy, type ProviderRoutingPolicy } from "../lib/model-routing.ts";
import { DEPS, checkAll, formatReport, bestInstallCmd, sortByRequires, type DepStatus, type DepTier } from "./deps.ts";

const AGENT_DIR = join(homedir(), ".pi", "agent");
const MARKER_PATH = join(AGENT_DIR, "omegon-bootstrap-done");
const MARKER_PATH_LEGACY = join(AGENT_DIR, "pi-kit-bootstrap-done"); // legacy — treat as done if present
const MARKER_VERSION = "2"; // bump to re-trigger bootstrap after adding operator profile capture

export type { OperatorCapabilityProfile } from "../lib/operator-profile.ts";
export type LocalFallbackPolicy = "allow" | "ask" | "deny";

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
	// Check new marker first, then legacy pi-kit marker (omegon renamed from pi-kit) (migration: existing installs skip re-run)
	if (existsSync(MARKER_PATH)) {
		try {
			const version = readFileSync(MARKER_PATH, "utf8").trim();
			return version !== MARKER_VERSION;
		} catch {
			return true;
		}
	}
	if (existsSync(MARKER_PATH_LEGACY)) return false;
	return true;
}

function markDone(): void {
	mkdirSync(AGENT_DIR, { recursive: true });
	writeFileSync(MARKER_PATH, MARKER_VERSION + "\n", "utf8");
}

function reorderCandidates(
	candidates: OperatorProfileCandidate[],
	primaryProvider: "anthropic" | "openai" | "no-preference",
): OperatorProfileCandidate[] {
	if (primaryProvider === "no-preference") return [...candidates];
	const rank = (candidate: OperatorProfileCandidate): number => {
		if (candidate.provider === primaryProvider) return 0;
		if (candidate.provider === "local") return 2;
		return 1;
	};
	return [...candidates].sort((a, b) => rank(a) - rank(b));
}

function applyPreferredProviderOrder(
	profile: OperatorCapabilityProfile,
	primaryProvider: "anthropic" | "openai" | "no-preference",
): void {
	for (const role of ["archmagos", "magos", "adept", "servitor", "servoskull"] as const) {
		profile.roles[role] = reorderCandidates(profile.roles[role], primaryProvider);
	}
}

function ensureAutomaticLightLocalFallback(profile: OperatorCapabilityProfile): void {
	const localSeed = profile.roles.servoskull.find((candidate) => candidate.source === "local");
	if (!localSeed) return;
	const servitorHasLocal = profile.roles.servitor.some((candidate) => candidate.source === "local");
	if (!servitorHasLocal) {
		profile.roles.servitor.push({
			id: localSeed.id,
			provider: localSeed.provider,
			source: "local",
			weight: "light",
			maxThinking: "minimal",
		});
	}
}

export function loadOperatorProfile(root: string): OperatorCapabilityProfile | undefined {
	const config = loadPiConfig(root) as PiConfigWithProfile;
	const raw = config.operatorProfile;
	if (!raw || typeof raw !== "object" || Array.isArray(raw)) return undefined;
	if (!Object.prototype.hasOwnProperty.call(raw, "roles") && !Object.prototype.hasOwnProperty.call(raw, "fallback")) {
		return undefined;
	}
	return parseCapabilityProfile(raw);
}

export function needsOperatorProfileSetup(root: string): boolean {
	return !loadOperatorProfile(root);
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
	const profile = getDefaultOperatorProfile();
	profile.setupComplete = false;

	const primaryProvider = summary.ready.includes("github")
		? "anthropic"
		: summary.ready.includes("aws") || summary.ready.includes("gitlab")
			? "openai"
			: "no-preference";
	applyPreferredProviderOrder(profile, primaryProvider);
	profile.fallback.sameRoleCrossProvider = "allow";
	profile.fallback.crossSource = "ask";
	profile.fallback.heavyLocal = "ask";
	profile.fallback.unknownLocalPerformance = "ask";
	return profile;
}

export function buildGuidedProfile(answers: SetupAnswers): OperatorCapabilityProfile {
	const profile = getDefaultOperatorProfile();
	profile.setupComplete = true;
	applyPreferredProviderOrder(profile, answers.primaryProvider);
	profile.fallback.sameRoleCrossProvider = answers.allowCloudCrossProviderFallback ? "allow" : "ask";
	profile.fallback.crossSource = answers.automaticLightLocalFallback ? "ask" : "deny";
	profile.fallback.heavyLocal = answers.heavyLocalFallback;
	profile.fallback.unknownLocalPerformance = "ask";
	if (answers.automaticLightLocalFallback) ensureAutomaticLightLocalFallback(profile);
	return profile;
}

export function saveOperatorProfile(root: string, profile: OperatorCapabilityProfile): void {
	persistOperatorProfile(root, profile);
}

export function routingPolicyFromProfile(profile: OperatorCapabilityProfile | undefined): ProviderRoutingPolicy {
	const policy = getDefaultPolicy();
	if (!profile) return policy;

	const providerOrder: Array<"anthropic" | "openai" | "local"> = [];
	for (const role of ["archmagos", "magos", "adept", "servitor", "servoskull"] as const) {
		for (const candidate of profile.roles[role]) {
			const provider = candidate.provider === "ollama" ? "local" : candidate.provider;
			if ((provider === "anthropic" || provider === "openai" || provider === "local") && !providerOrder.includes(provider)) {
				providerOrder.push(provider);
			}
		}
	}
	for (const provider of ["anthropic", "openai", "local"] as const) {
		if (!providerOrder.includes(provider)) providerOrder.push(provider);
	}

	const automaticLocalFallback = profile.roles.servitor.some((candidate) => candidate.source === "local");
	const avoidProviders = new Set(policy.avoidProviders);
	if (!automaticLocalFallback) avoidProviders.add("local");

	return {
		...policy,
		providerOrder,
		avoidProviders: [...avoidProviders],
		cheapCloudPreferredOverLocal: !automaticLocalFallback,
		notes: profile.setupComplete
			? "routing policy sourced from operator capability profile"
			: "routing policy sourced from default operator capability profile",
	};
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
		"This captures cloud/local fallback preferences so Omegon avoids unsafe automatic model switches.",
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
		"If your preferred cloud provider is unavailable, may Omegon retry the same capability role with another cloud provider?",
	);
	const automaticLightLocalFallback = await ctx.ui.confirm(
		"Allow automatic light local fallback?",
		"Allow Omegon to use local models automatically for lightweight work when cloud options are unavailable?",
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
		sharedState.routingPolicy = routingPolicyFromProfile(loadOperatorProfile(getConfigRoot(ctx)));

		if (!isFirstRun()) return;
		if (!ctx.hasUI) return;

		// Signal other extensions to suppress redundant "no providers" warnings
		sharedState.bootstrapPending = true;

		const statuses = checkAll();
		const missing = statuses.filter((s) => !s.available);
		const needsProfile = needsOperatorProfileSetup(getConfigRoot(ctx));

		if (missing.length === 0 && !needsProfile) {
			markDone();
			return;
		}

		const coreMissing = missing.filter((s) => s.dep.tier === "core");
		const recMissing = missing.filter((s) => s.dep.tier === "recommended");

		let msg = "Welcome to Omegon! ";
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
		description: "First-time setup — check/install Omegon dependencies and capture operator fallback preferences",
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

	// --- /update: unified update command ---
	// Detects dev vs installed mode and runs the appropriate lifecycle:
	//   Dev mode  (.git exists): pull → submodule sync → build → dependency refresh → relink → verify → restart handoff
	//   Installed (no .git):     npm install -g omegon@latest → verify → restart handoff
	// Replaces the old split update mental model with a singular-package lifecycle.
	pi.registerCommand("update", {
		description: "Run the authoritative Omegon update lifecycle, then hand off to restart",
		handler: async (args, ctx) => {
			const dryRun = args.trim() === "--dry-run";
			const omegonRoot = process.env.PI_CODING_AGENT_DIR ?? join(import.meta.dirname ?? ".", "..");
			const isDevMode = existsSync(join(omegonRoot, ".git"));

			if (isDevMode) {
				await updateDevMode(omegonRoot, dryRun, ctx);
			} else {
				await updateInstalledMode(dryRun, ctx);
			}
		},
	});

	// --- /refresh: lightweight cache clear + reload only ---
	pi.registerCommand("refresh", {
		description: "Clear transpilation cache and reload extensions without package/runtime mutation",
		handler: async (_args, ctx) => {
			clearJitiCache(ctx);
			await ctx.reload();
		},
	});
}

// ── /update helpers ──────────────────────────────────────────────────────

/** Run a command, collect stdout+stderr, resolve with exit code. */
function run(
	cmd: string, args: string[], opts?: { cwd?: string },
): Promise<{ code: number; stdout: string; stderr: string }> {
	return new Promise((resolve) => {
		let stdout = "", stderr = "";
		const child = spawn(cmd, args, { cwd: opts?.cwd, stdio: ["ignore", "pipe", "pipe"] });
		child.stdout.on("data", (d: Buffer) => { stdout += d.toString(); });
		child.stderr.on("data", (d: Buffer) => { stderr += d.toString(); });
		child.on("close", (code: number) => resolve({ code: code ?? 1, stdout, stderr }));
	});
}

/** Clear jiti transpilation cache. Returns count of cleared entries. */
function clearJitiCache(_ctx?: unknown): number {
	const jitiCacheDir = join(tmpdir(), "jiti");
	let cleared = 0;
	if (existsSync(jitiCacheDir)) {
		try {
			cleared = readdirSync(jitiCacheDir).length;
			rmSync(jitiCacheDir, { recursive: true, force: true });
		} catch { /* best-effort */ }
	}
	return cleared;
}

export interface PiResolutionInfo {
	omegonRoot: string;
	cli: string;
	resolutionMode: "vendor" | "npm";
	agentDir: string;
}

export interface PiBinaryVerification {
	ok: boolean;
	piPath: string;
	realPiPath: string;
	resolution?: PiResolutionInfo;
	reason?: string;
}

export function normalizePiPath(piPath: string): string {
	if (!piPath) return "";
	try {
		return realpathSync(piPath);
	} catch {
		return piPath;
	}
}

async function getActivePiPath(): Promise<string> {
	const which = await run("which", ["pi"]);
	return which.code === 0 ? which.stdout.trim() : "";
}

export function validatePiBinaryVerification(
	piPath: string,
	realPiPath: string,
	resolution: PiResolutionInfo,
): PiBinaryVerification {
	const binaryLooksOwnedByOmegon = /[\\/]omegon[\\/]/.test(realPiPath) || /[\\/]omegon[\\/]bin[\\/]pi(?:\.mjs)?$/.test(realPiPath);
	if (!/omegon(?:[\\/]|$)/.test(resolution.omegonRoot)) {
		return { ok: false, piPath, realPiPath, resolution, reason: `active pi resolved to non-Omegon root: ${resolution.omegonRoot}` };
	}
	if (!binaryLooksOwnedByOmegon) {
		return { ok: false, piPath, realPiPath, resolution, reason: `active pi symlink target does not appear to point at Omegon: ${realPiPath}` };
	}
	return { ok: true, piPath, realPiPath, resolution };
}

async function inspectActivePiBinary(): Promise<PiBinaryVerification> {
	const piPath = await getActivePiPath();
	if (!piPath) {
		return { ok: false, piPath: "", realPiPath: "", reason: "`pi` command not found on PATH" };
	}
	const realPiPath = normalizePiPath(piPath);
	const probe = await run(piPath, ["--where"]);
	if (probe.code !== 0) {
		return { ok: false, piPath, realPiPath, reason: "active pi binary did not return Omegon resolution metadata" };
	}
	try {
		const resolution = JSON.parse(probe.stdout.trim()) as PiResolutionInfo;
		return validatePiBinaryVerification(piPath, realPiPath, resolution);
	} catch {
		return { ok: false, piPath, realPiPath, reason: "active pi returned invalid verification metadata" };
	}
}

function formatVerification(verification: PiBinaryVerification): string {
	if (!verification.ok || !verification.resolution) {
		return `✗ pi target verification failed${verification.reason ? `: ${verification.reason}` : ""}`;
	}
	return [
		`✓ active pi: ${verification.piPath}`,
		`✓ binary target: ${verification.realPiPath}`,
		`✓ runtime root: ${verification.resolution.omegonRoot}`,
		`✓ core resolution: ${verification.resolution.resolutionMode} (${verification.resolution.cli})`,
	].join("\n");
}

/** Dev mode: git pull → submodule update → build → install deps → relink → verify → restart handoff. */
async function updateDevMode(
	omegonRoot: string,
	dryRun: boolean,
	ctx: { ui: { notify: (message: string, type?: "error" | "warning" | "info") => void } },
): Promise<void> {
	const steps: string[] = [];

	// ── Step 1: git pull omegon ──────────────────────────────────────
	ctx.ui.notify("▸ Pulling omegon…", "info");
	const pull = await run("git", ["pull", "--ff-only"], { cwd: omegonRoot });
	if (pull.code !== 0) {
		// Non-ff merge needed — not fatal, just skip
		const msg = pull.stderr.includes("fatal")
			? `git pull failed: ${pull.stderr.trim().split("\n")[0]}`
			: "git pull: non-fast-forward — skipping (merge manually if needed)";
		steps.push(`⚠ ${msg}`);
	} else {
		const summary = pull.stdout.trim().split("\n").pop() ?? "";
		const upToDate = pull.stdout.includes("Already up to date");
		steps.push(upToDate ? "✓ omegon: already up to date" : `✓ omegon: ${summary}`);
	}

	// ── Step 2: update submodule (pi-mono fork) ──────────────────────
	ctx.ui.notify("▸ Updating pi-mono submodule…", "info");
	const sub = await run(
		"git", ["submodule", "update", "--init", "--recursive"],
		{ cwd: omegonRoot },
	);
	if (sub.code !== 0) {
		steps.push(`⚠ submodule update failed: ${sub.stderr.trim().split("\n")[0]}`);
	} else {
		steps.push("✓ pi-mono submodule synced");
	}

	// ── Step 3: build pi-mono ────────────────────────────────────────
	if (dryRun) {
		steps.push("· build: skipped (dry run)");
	} else {
		ctx.ui.notify("▸ Building pi-mono…", "info");
		const piMonoRoot = join(omegonRoot, "vendor/pi-mono");
		const build = await run("npm", ["run", "build"], { cwd: piMonoRoot });
		if (build.code !== 0) {
			const errLine = build.stderr.trim().split("\n").filter(l => !l.startsWith("npm warn")).pop() ?? "unknown error";
			steps.push(`✗ build failed: ${errLine}`);
			ctx.ui.notify(`Update incomplete:\n${steps.join("\n")}`, "warning");
			return;
		}
		steps.push("✓ pi-mono built");
	}

	// ── Step 4: npm install (pick up any new deps) ───────────────────
	if (dryRun) {
		steps.push("· npm install: skipped (dry run)");
	} else {
		ctx.ui.notify("▸ Refreshing omegon dependencies…", "info");
		const inst = await run("npm", ["install", "--install-links=false"], { cwd: omegonRoot });
		if (inst.code !== 0) {
			steps.push(`⚠ npm install had issues (non-fatal)`);
		} else {
			steps.push("✓ omegon dependencies refreshed");
		}
	}

	// ── Step 5: relink omegon globally ───────────────────────────────
	if (dryRun) {
		steps.push("· npm link --force: skipped (dry run)");
	} else {
		ctx.ui.notify("▸ Relinking omegon globally…", "info");
		const link = await run("npm", ["link", "--force"], { cwd: omegonRoot });
		if (link.code !== 0) {
			steps.push(`✗ npm link failed: ${(link.stderr.trim().split("\n").filter((l) => !l.startsWith("npm warn")).pop() ?? "unknown error")}`);
			ctx.ui.notify(`Update incomplete:\n${steps.join("\n")}`, "warning");
			return;
		}
		steps.push("✓ omegon relinked globally");
	}

	// ── Step 6: verify active binary target ──────────────────────────
	if (dryRun) {
		steps.push("· pi target verification: skipped (dry run)");
		ctx.ui.notify(`Dry run:\n${steps.join("\n")}`, "info");
		return;
	}
	const verification = await inspectActivePiBinary();
	if (!verification.ok) {
		steps.push(formatVerification(verification));
		ctx.ui.notify(`Update incomplete:\n${steps.join("\n")}`, "warning");
		return;
	}
	steps.push(formatVerification(verification));

	// ── Step 7: clear cache + explicit restart handoff ───────────────
	const cleared = clearJitiCache(ctx);
	if (cleared > 0) steps.push(`✓ cleared ${cleared} cached transpilations`);
	steps.push("✓ update complete — restart pi now (/exit, then `pi`) to load the rebuilt runtime");
	ctx.ui.notify(steps.join("\n"), "info");
}

/** Installed mode: npm install -g omegon@latest → verify → cache clear → restart handoff. */
async function updateInstalledMode(
	dryRun: boolean,
	ctx: {
		ui: {
			notify: (message: string, type?: "error" | "warning" | "info") => void;
			confirm: (title: string, message: string) => Promise<boolean>;
		};
	},
): Promise<void> {
	const PKG = "omegon";

	// Check latest version on npm
	ctx.ui.notify(`Checking latest version of ${PKG}…`, "info");
	const view = await run("npm", ["view", PKG, "version", "--json"]);
	if (view.code !== 0) {
		ctx.ui.notify("Failed to query npm registry. Are you online?", "warning");
		return;
	}
	const latestVersion = JSON.parse(view.stdout.trim());

	// Determine installed version
	const list = await run("npm", ["list", "-g", PKG, "--json", "--depth=0"]);
	let installedVersion = "unknown";
	try {
		const data = JSON.parse(list.stdout);
		installedVersion = data.dependencies?.[PKG]?.version ?? "unknown";
	} catch { /* ignore */ }

	if (installedVersion === latestVersion) {
		ctx.ui.notify(`Already on latest: ${PKG}@${latestVersion} ✅`, "info");
		return;
	}

	ctx.ui.notify(
		`Update available: ${installedVersion} → ${latestVersion}` +
		(dryRun ? "\n(dry run — not installing)" : ""),
		"info"
	);
	if (dryRun) return;

	const confirmed = await ctx.ui.confirm(
		"Update omegon?",
		`Install ${PKG}@${latestVersion} globally via npm?\n\nThis will update pi core, extensions, themes, and skills.\nRestart pi after the update completes.`,
	);
	if (!confirmed) {
		ctx.ui.notify("Update cancelled.", "info");
		return;
	}

	ctx.ui.notify("Installing…", "info");
	const inst = await run("npm", ["install", "-g", `${PKG}@${latestVersion}`]);
	if (inst.code !== 0) {
		ctx.ui.notify(`npm install failed:\n${inst.stderr}`, "warning");
		return;
	}

	const verification = await inspectActivePiBinary();
	if (!verification.ok) {
		ctx.ui.notify(
			`Updated to ${PKG}@${latestVersion}, but post-install verification failed.\n${formatVerification(verification)}\nResolve the binary target before restarting pi.`,
			"warning",
		);
		return;
	}

	const cleared = clearJitiCache(ctx);
	ctx.ui.notify(
		`✅ Updated to ${PKG}@${latestVersion}.` +
		`\n${formatVerification(verification)}` +
		(cleared > 0 ? `\nCleared ${cleared} cached transpilations.` : "") +
		"\nRestart pi to use the new version (/exit, then pi).",
		"info"
	);
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

	// API key guidance — check if any cloud provider is configured
	const providerReadiness = await checkAllProviders(pi);
	const hasAnyCloudKey = providerReadiness.some(
		(r: AuthResult) => r.status === "ok" && r.provider !== "local",
	);
	if (!hasAnyCloudKey) {
		ctx.ui.notify(
			"\n🔑 **No cloud API keys detected.**\n" +
			"Omegon needs at least one provider key to function. The fastest options:\n" +
			"  • Anthropic: `/secrets configure ANTHROPIC_API_KEY` (get key at console.anthropic.com)\n" +
			"  • OpenAI: `/secrets configure OPENAI_API_KEY` (get key at platform.openai.com)\n" +
			"  • GitHub Copilot: `/login github` (requires Copilot subscription)\n",
			"warning"
		);
	}

	await ensureOperatorProfile(pi, ctx);

	const recheck = checkAll();
	const stillMissing = recheck.filter((s) => !s.available && (s.dep.tier === "core" || s.dep.tier === "recommended"));

	if (stillMissing.length === 0 && hasAnyCloudKey) {
		ctx.ui.notify("\n🎉 Setup complete! All core and recommended dependencies are available.");
		markDone();
	} else if (stillMissing.length === 0) {
		ctx.ui.notify(
			"\n✅ Dependencies installed. Configure an API key (see above) to start using Omegon.",
		);
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

/**
 * Determine whether a command string requires a shell interpreter.
 *
 * Commands that contain shell operators (pipes, redirects, logical
 * connectors, glob expansions, subshells, environment variable
 * assignments, or quoted whitespace) cannot be safely split into
 * argv tokens without a shell.  Everything else can be dispatched
 * directly via execve-style spawn.
 */
export function requiresShell(cmd: string): boolean {
	// Shell metacharacters that need sh -c interpretation.
	// `#` is only a shell comment when it appears at the start of a word
	// (preceded by whitespace or at string start) — inside a URL fragment
	// like https://host/path#anchor it is plain data and must NOT trigger
	// the shell path.  All other listed chars are unambiguous metacharacters.
	return /[|&;<>()$`\\!*?[\]{}~]|(^|\s)#/.test(cmd);
}

/**
 * Split a simple (no-shell) command string into [executable, ...args].
 *
 * Only call this after confirming `requiresShell(cmd) === false`.
 * Splitting is naive whitespace-based — sufficient for the dep install
 * commands in deps.ts which do not use quoting.
 */
export function parseCommandArgv(cmd: string): [string, ...string[]] {
	const parts = cmd.trim().split(/\s+/).filter(Boolean);
	if (parts.length === 0) throw new Error("Empty command");
	return parts as [string, ...string[]];
}

/**
 * Strip ANSI escape sequences from a string so we can display raw text
 * through pi's notification system without garbled control codes.
 */
function stripAnsi(str: string): string {
	// Covers CSI sequences, OSC, simple escapes, and reset codes.
	// eslint-disable-next-line no-control-regex
	return str.replace(/\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[^[]/g, "");
}

/**
 * Decide whether a captured output line is worth forwarding to the operator.
 *
 * Filters out progress-bar-only lines (filled entirely with ═ = > # etc.)
 * and carriage-return-overwritten lines that cargo/rustup use for spinners.
 */
function isSignificantLine(raw: string): boolean {
	const s = stripAnsi(raw).trim();
	if (s.length === 0) return false;
	// Pure progress bar characters — not meaningful as text
	if (/^[=>\-#.·⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ]+$/.test(s)) return false;
	// Very long lines are likely binary blobs or encoded data
	if (s.length > 300) return false;
	return true;
}

/**
 * Run a helper command asynchronously, streaming output through `onLine`.
 *
 * stdin is closed (no interactive prompts).  stdout and stderr are both
 * piped so output is captured and forwarded through pi's notification
 * system rather than fighting with the TUI renderer.
 *
 * A heartbeat tick fires every `heartbeatMs` so the operator knows the
 * process is still alive during long compilations (e.g. cargo build).
 *
 * The install commands come exclusively from the static `deps.ts`
 * registry and are never influenced by operator input.
 *
 * Returns the process exit code (124 = timeout).
 */
export function runAsync(
	cmd: string,
	onLine: (line: string) => void,
	timeoutMs: number = 600_000,
	heartbeatMs: number = 15_000,
): Promise<number> {
	return new Promise((resolve) => {
		const env = {
			...process.env,
			// Homebrew / generic non-interactive suppression
			NONINTERACTIVE: "1",
			HOMEBREW_NO_AUTO_UPDATE: "1",
			// Rustup: skip the interactive "1) Proceed / 2) Customise / 3) Cancel"
			// prompt entirely.  Belt-and-suspenders alongside the -y flag in the
			// install command.
			RUSTUP_INIT_SKIP_PATH_CHECK: "yes",
		};

		let child;
		if (requiresShell(cmd)) {
			child = spawn("sh", ["-c", cmd], { stdio: ["ignore", "pipe", "pipe"], env });
		} else {
			const [exe, ...args] = parseCommandArgv(cmd);
			child = spawn(exe, args, { stdio: ["ignore", "pipe", "pipe"], env });
		}

		let settled = false;
		let sigkillTimer: ReturnType<typeof setTimeout> | undefined;
		let elapsedSec = 0;

		const settle = (code: number) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			clearInterval(heartbeat);
			clearTimeout(sigkillTimer);
			resolve(code);
		};

		// Heartbeat — fires every heartbeatMs while the process is running.
		const heartbeat = setInterval(() => {
			elapsedSec += heartbeatMs / 1000;
			onLine(`   ⏳ still running… (${elapsedSec}s)`);
		}, heartbeatMs);

		// Forward captured lines from both streams.
		const attachStream = (stream: NodeJS.ReadableStream | null) => {
			if (!stream) return;
			let buf = "";
			stream.on("data", (chunk: Buffer) => {
				// Strip carriage returns so spinner overwrites don't stack.
				buf += chunk.toString().replace(/\r/g, "\n");
				const parts = buf.split("\n");
				buf = parts.pop() ?? "";
				for (const part of parts) {
					if (isSignificantLine(part)) onLine("   " + stripAnsi(part).trim());
				}
			});
			stream.on("end", () => {
				if (buf && isSignificantLine(buf)) onLine("   " + stripAnsi(buf).trim());
			});
		};
		attachStream(child.stdout);
		attachStream(child.stderr);

		const timer = setTimeout(() => {
			child.kill("SIGTERM");
			sigkillTimer = setTimeout(() => {
				try { child.kill("SIGKILL"); } catch { /* already exited */ }
			}, 5_000);
			settle(124);
		}, timeoutMs);

		child.on("exit", (code) => settle(code ?? 1));
		child.on("error", () => settle(1));
	});
}

/**
 * After rustup installs, the cargo binaries land in ~/.cargo/bin which is
 * NOT in the current process's PATH (only added to future shells via
 * .profile/.bashrc).  Source it now so subsequent deps (e.g. mdserve) can
 * find cargo without the operator having to open a new terminal.
 */
function patchPathForCargo(): void {
	const cargoBin = join(homedir(), ".cargo", "bin");
	const current = process.env.PATH ?? "";
	if (!current.split(":").includes(cargoBin)) {
		process.env.PATH = `${cargoBin}:${current}`;
	}
}

async function installDeps(ctx: CommandContext, deps: DepStatus[]): Promise<void> {
	// Sort so prerequisites come first (e.g., cargo before mdserve)
	const sorted = sortByRequires(deps);
	const total = sorted.length;

	for (let i = 0; i < sorted.length; i++) {
		const { dep } = sorted[i];
		const step = `[${i + 1}/${total}]`;

		// Check prerequisites — re-verify availability live (not from stale array)
		if (dep.requires?.length) {
			const unmet = dep.requires.filter((reqId) => {
				const reqDep = DEPS.find((d) => d.id === reqId);
				return reqDep ? !reqDep.check() : false;
			});
			if (unmet.length > 0) {
				ctx.ui.notify(`\n${step} ⚠️  Skipping ${dep.name} — requires ${unmet.join(", ")} (not yet available)`);
				continue;
			}
		}

		const cmd = bestInstallCmd(dep);
		if (!cmd) {
			ctx.ui.notify(`\n${step} ⚠️  No install command available for ${dep.name} on this platform`);
			continue;
		}

		ctx.ui.notify(`\n${step} 📦 Installing ${dep.name}…`);
		ctx.ui.notify(`   → \`${cmd}\``);

		const exitCode = await runAsync(
			cmd,
			(line) => ctx.ui.notify(line),
		);

		// Rustup installs to ~/.cargo/bin — patch PATH immediately so the rest
		// of the install sequence (e.g. mdserve) can find cargo.
		if (dep.id === "cargo" && exitCode === 0) {
			patchPathForCargo();
		}

		if (exitCode === 0 && dep.check()) {
			ctx.ui.notify(`${step} ✅ ${dep.name} installed successfully`);
		} else if (exitCode === 124) {
			ctx.ui.notify(`${step} ❌ ${dep.name} install timed out (10 min limit)`);
		} else if (exitCode === 0) {
			ctx.ui.notify(`${step} ⚠️  Command succeeded but ${dep.name} not found on PATH — you may need to open a new shell.`);
		} else {
			ctx.ui.notify(`${step} ❌ Failed to install ${dep.name} (exit ${exitCode})`);
			const hints = dep.install.filter((o) => o.cmd !== cmd);
			if (hints.length > 0) ctx.ui.notify(`   Alternative: \`${hints[0]!.cmd}\``);
			if (dep.url) ctx.ui.notify(`   Manual install: ${dep.url}`);
		}
	}
}

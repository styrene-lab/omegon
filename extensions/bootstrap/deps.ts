/**
 * Dependency registry — declarative external dependency catalog.
 *
 * Each dep has a check function (is it available?), install hint,
 * tier (core vs optional), and the extensions that need it.
 */

import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type DepTier = "core" | "recommended" | "optional";

export interface Dep {
	/** Short identifier */
	id: string;
	/** Human-readable name */
	name: string;
	/** What it does in Omegon context */
	purpose: string;
	/** Which extensions use it */
	usedBy: string[];
	/** core = most users need it, recommended = common workflows, optional = niche */
	tier: DepTier;
	/** Check if the dep is available */
	check: () => boolean;
	/** Shell command(s) to install, in preference order per platform */
	install: InstallOption[];
	/** URL for manual install instructions */
	url?: string;
	/** Dep IDs that must be installed first */
	requires?: string[];
	/**
	 * Optional preflight check. If it returns a string, that string is a
	 * blocking message shown to the operator explaining what they need to do
	 * manually before this dep can be installed. Return undefined if ready.
	 */
	preflight?: () => string | undefined;
}

export interface InstallOption {
	/** Platform: "darwin", "linux", or "any" */
	platform: "darwin" | "linux" | "any";
	/** Shell command */
	cmd: string;
}

/**
 * Ensure well-known tool paths are on PATH before probing.
 *
 * Tools installed by Nix, Cargo, etc. land in directories that may not be
 * in the inherited PATH (e.g. Omegon launched from a shell that predates
 * the install). We patch once at module load so every hasCmd() call sees them.
 */
function ensureToolPaths(): void {
	const home = homedir();
	const dirs = [
		"/nix/var/nix/profiles/default/bin",
		join(home, ".nix-profile", "bin"),
		join(home, ".cargo", "bin"),
		// Linuxbrew
		"/home/linuxbrew/.linuxbrew/bin",
		join(home, ".linuxbrew", "bin"),
		// macOS Homebrew
		"/opt/homebrew/bin",
		"/usr/local/bin",
	];
	const current = process.env.PATH ?? "";
	const parts = current.split(":");
	const missing = dirs.filter(d => existsSync(d) && !parts.includes(d));
	if (missing.length > 0) {
		process.env.PATH = [...missing, current].join(":");
	}
}
ensureToolPaths();

/** Detect ostree-based immutable Linux (Bazzite, Silverblue, Kinoite, Bluefin, etc.) */
function isOstree(): boolean {
	if (process.platform !== "linux") return false;
	try {
		execSync("which rpm-ostree", { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

/**
 * On Fedora 42+ ostree systems, `/` is read-only by default (composefs).
 * Nix needs to create `/nix` which requires `root.transient = true` in the
 * ostree prepare-root config. Returns blocking instructions if not ready.
 */
function checkOstreeReadyForNix(): string | undefined {
	// If /nix already exists, we're good (previous install or already configured)
	if (existsSync("/nix")) return undefined;

	// Check if root.transient is enabled
	try {
		const conf = execSync("cat /etc/ostree/prepare-root.conf 2>/dev/null", { encoding: "utf-8" });
		if (/transient\s*=\s*true/i.test(conf)) return undefined;
	} catch { /* file doesn't exist */ }

	return [
		"⚠️  Your system has a read-only root filesystem (ostree/composefs).",
		"Nix needs `/nix` to exist, which requires enabling root.transient.",
		"",
		"Run these commands in your terminal, then reboot and run /bootstrap again:",
		"",
		"```",
		"sudo tee /etc/ostree/prepare-root.conf <<'EOL'",
		"[composefs]",
		"enabled = yes",
		"[root]",
		"transient = true",
		"EOL",
		"",
		"sudo rpm-ostree initramfs-etc --track=/etc/ostree/prepare-root.conf",
		"systemctl reboot",
		"```",
	].join("\n");
}

function hasCmd(cmd: string): boolean {
	try {
		execSync(`which ${cmd}`, { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

/** Get the best install command for the current platform */
export function bestInstallCmd(dep: Dep): string | undefined {
	const plat = process.platform === "darwin" ? "darwin" : "linux";
	const candidates = dep.install.filter((o) => o.platform === plat || o.platform === "any");
	if (candidates.length === 0) return dep.install[0]?.cmd;

	// If nix is available, prefer nix commands. Otherwise prefer brew.
	const hasNix = hasCmd("nix");
	const hasBrew = hasCmd("brew");
	if (hasNix) {
		const nix = candidates.find((o) => o.cmd.startsWith("nix "));
		if (nix) return nix.cmd;
	}
	if (hasBrew) {
		const brew = candidates.find((o) => o.cmd.startsWith("brew "));
		if (brew) return brew.cmd;
	}
	return candidates[0]?.cmd;
}

/** Get all install options formatted for display */
export function installHints(dep: Dep): string[] {
	return dep.install.map((o) =>
		o.platform === "any" ? o.cmd : `${o.cmd}  (${o.platform})`,
	);
}

/**
 * The canonical dependency registry.
 *
 * Extensions should NOT duplicate these checks — import from here.
 * Order matters: displayed in this order during bootstrap.
 */
export const DEPS: Dep[] = [
	// --- Core: package manager + essential tools ---
	{
		id: "nix",
		name: "Nix",
		purpose: "Universal package manager — installs all other dependencies on any OS",
		usedBy: ["bootstrap"],
		tier: "core",
		// Either nix or brew satisfies this — we just need a working package manager
		check: () => hasCmd("nix") || hasCmd("brew"),
		install: isOstree()
			? [{ platform: "linux", cmd: '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"' }]
			: [
				{ platform: "any", cmd: "curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install --no-confirm" },
				{ platform: "any", cmd: '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"' },
			],
		url: "https://zero-to-nix.com",
	},
	{
		id: "ollama",
		name: "Ollama",
		purpose: "Local model inference, embeddings for semantic memory search",
		usedBy: ["local-inference", "project-memory", "cleave", "offline-driver"],
		tier: "core",
		check: () => hasCmd("ollama"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#ollama" },
			{ platform: "any", cmd: "brew install ollama" },
		],
		url: "https://ollama.com",
	},
	{
		id: "d2",
		name: "D2",
		purpose: "Diagram rendering (architecture, flowcharts, ER diagrams)",
		usedBy: ["render", "view"],
		tier: "core",
		check: () => hasCmd("d2"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#d2" },
			{ platform: "any", cmd: "brew install d2" },
		],
		url: "https://d2lang.com",
	},

	// --- Recommended: common workflows ---
	{
		id: "vault",
		name: "Vault CLI",
		purpose: "HashiCorp Vault authentication status checking and secret management",
		usedBy: ["01-auth"],
		tier: "recommended",
		check: () => hasCmd("vault"),
		install: [
			{ platform: "darwin", cmd: "brew install hashicorp/tap/vault" },
			{ platform: "linux", cmd: "brew install hashicorp/tap/vault" },
			{ platform: "linux", cmd: "wget -O- https://apt.releases.hashicorp.com/gpg | sudo gpg --yes --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main\" | sudo tee /etc/apt/sources.list.d/hashicorp.list && sudo apt update && sudo apt install -y vault" },
		],
		url: "https://developer.hashicorp.com/vault/install",
	},
	{
		id: "gh",
		name: "GitHub CLI",
		purpose: "GitHub authentication, PR creation, issue management",
		usedBy: ["01-auth"],
		tier: "recommended",
		check: () => hasCmd("gh"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#gh" },
			{ platform: "any", cmd: "brew install gh" },
		],
		url: "https://cli.github.com",
	},
	{
		id: "pandoc",
		name: "Pandoc",
		purpose: "Document conversion (DOCX, PPTX, EPUB → Markdown)",
		usedBy: ["view"],
		tier: "recommended",
		check: () => hasCmd("pandoc"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#pandoc" },
			{ platform: "any", cmd: "brew install pandoc" },
		],
		url: "https://pandoc.org",
	},
	{
		id: "cargo",
		name: "Rust toolchain",
		purpose: "Required to build mdserve from source",
		usedBy: ["vault (build dep)"],
		tier: "recommended",
		check: () => hasCmd("cargo"),
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#rustup && rustup default stable" },
			{ platform: "any", cmd: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y" },
		],
		url: "https://rustup.rs",
	},
	{
		id: "mdserve",
		name: "mdserve",
		purpose: "Markdown viewport with wikilinks and graph view (/vault)",
		usedBy: ["vault"],
		tier: "recommended",
		check: () => hasCmd("mdserve"),
		requires: ["cargo"],
		install: [
			{ platform: "any", cmd: "cargo install --git https://github.com/cwilson613/mdserve --branch feature/wikilinks-graph" },
		],
		url: "https://github.com/cwilson613/mdserve",
	},

	// --- Optional: niche or platform-specific ---
	{
		id: "rsvg-convert",
		name: "librsvg",
		purpose: "SVG rendering in terminal",
		usedBy: ["view"],
		tier: "optional",
		check: () => hasCmd("rsvg-convert"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#librsvg" },
			{ platform: "any", cmd: "brew install librsvg" },
		],
	},
	{
		id: "pdftoppm",
		name: "Poppler",
		purpose: "PDF rendering in terminal",
		usedBy: ["view"],
		tier: "optional",
		check: () => hasCmd("pdftoppm"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#poppler_utils" },
			{ platform: "any", cmd: "brew install poppler" },
		],
	},
	{
		id: "uv",
		name: "uv",
		purpose: "Python package manager for mflux (local image generation)",
		usedBy: ["render"],
		tier: "optional",
		check: () => hasCmd("uv"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#uv" },
			{ platform: "any", cmd: "brew install uv" },
		],
		url: "https://docs.astral.sh/uv/",
	},
	{
		id: "aws",
		name: "AWS CLI",
		purpose: "AWS authentication and ECR access",
		usedBy: ["01-auth"],
		tier: "optional",
		check: () => hasCmd("aws"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#awscli2" },
			{ platform: "any", cmd: "brew install awscli" },
		],
	},
	{
		id: "kubectl",
		name: "kubectl",
		purpose: "Kubernetes cluster access",
		usedBy: ["01-auth"],
		tier: "optional",
		check: () => hasCmd("kubectl"),
		requires: ["nix"],
		install: [
			{ platform: "any", cmd: "nix profile install nixpkgs#kubectl" },
			{ platform: "any", cmd: "brew install kubectl" },
		],
	},
];

export type DepStatus = { dep: Dep; available: boolean };

/** Check all deps and return their status */
export function checkAll(): DepStatus[] {
	return DEPS.map((dep) => ({
		dep,
		available: dep.check(),
	}));
}

/**
 * Detect whether the terminal supports Unicode emoji rendering.
 *
 * Returns true for modern terminals (Windows Terminal, VS Code, xterm-256color,
 * iTerm2, etc.) and false for legacy consoles (Windows conhost.exe) where emoji
 * render as blank boxes.  Errs on the side of ASCII when uncertain.
 */
function supportsEmoji(): boolean {
	// Windows Terminal sets WT_SESSION; conhost.exe does not
	if (process.env["WT_SESSION"]) return true;
	// VS Code integrated terminal
	if (process.env["TERM_PROGRAM"] === "vscode") return true;
	// iTerm2, Hyper, and other macOS/Linux terminals advertising 256-color
	if (process.env["TERM_PROGRAM"] === "iTerm.app") return true;
	// xterm-256color and similar modern TERM values
	const term = process.env["TERM"] ?? "";
	if (term.includes("256color") || term === "xterm-kitty") return true;
	// COLORTERM=truecolor or 24bit signals a modern terminal
	const colorterm = process.env["COLORTERM"] ?? "";
	if (colorterm === "truecolor" || colorterm === "24bit") return true;
	// CI environments typically render emoji correctly
	if (process.env["CI"]) return true;
	// Non-Windows: default to emoji; on Windows without the above signals, use ASCII
	return process.platform !== "win32";
}

/** Format a single dep status as a line, with install hint if missing */
function formatStatus(s: DepStatus): string {
	const emoji = supportsEmoji();
	const icon = s.available ? (emoji ? "✅" : "[ok]") : (emoji ? "❌" : "[x]");
	let line = `${icon}  ${s.dep.name} — ${s.dep.purpose}`;
	if (!s.available) {
		const cmd = bestInstallCmd(s.dep);
		if (cmd) line += `\n      ${emoji ? "→" : "->"} \`${cmd}\``;
	}
	return line;
}

/** Format full report grouped by tier */
export function formatReport(statuses: DepStatus[]): string {
	const tiers: DepTier[] = ["core", "recommended", "optional"];
	const tierLabels: Record<DepTier, string> = {
		core: "Core (most users need these)",
		recommended: "Recommended (common workflows)",
		optional: "Optional (niche / platform-specific)",
	};

	const lines: string[] = ["# Omegon Dependencies\n"];

	for (const tier of tiers) {
		const group = statuses.filter((s) => s.dep.tier === tier);
		if (group.length === 0) continue;

		lines.push(`## ${tierLabels[tier]}\n`);
		for (const s of group) {
			lines.push(formatStatus(s));
		}
		lines.push("");
	}

	const missing = statuses.filter((s) => !s.available);
	const emoji = supportsEmoji();
	if (missing.length === 0) {
		lines.push(emoji ? "🎉 All dependencies are available!" : "[ok] All dependencies are available!");
	} else {
		lines.push(`${emoji ? "⚠️ " : "[!] "}**${missing.length} missing** — run \`/bootstrap\` to install interactively.`);
	}

	return lines.join("\n");
}

/** Topological sort: deps with `requires` come after their prerequisites */
export function sortByRequires(deps: DepStatus[]): DepStatus[] {
	const byId = new Map(deps.map((s) => [s.dep.id, s]));
	const sorted: DepStatus[] = [];
	const visited = new Set<string>();

	function visit(s: DepStatus) {
		if (visited.has(s.dep.id)) return;
		visited.add(s.dep.id);
		for (const reqId of s.dep.requires ?? []) {
			const req = byId.get(reqId);
			if (req && !req.available) visit(req);
		}
		sorted.push(s);
	}

	for (const s of deps) visit(s);
	return sorted;
}

// @secret GITHUB_TOKEN "GitHub personal access token (alternative to gh auth login)"
// @secret GITLAB_TOKEN "GitLab personal access token (alternative to glab auth login)"
// @secret AWS_ACCESS_KEY_ID "AWS access key ID (alternative to aws sso login)"
// @secret AWS_SECRET_ACCESS_KEY "AWS secret access key"

/**
 * Auth Extension — authentication status, diagnosis, and refresh across dev tools.
 *
 * Registers:
 *   - `whoami` tool: LLM-callable auth status check
 *   - `/auth` command: interactive auth management
 *     - `/auth` or `/auth status` — check all providers
 *     - `/auth check <provider>` — check a specific provider
 *     - `/auth refresh <provider>` — show refresh command + offer /secrets path
 *     - `/auth list` — list available providers
 *
 * Security model:
 *   - Auth NEVER stores, caches, or manipulates secret values directly.
 *   - All credential storage flows through 00-secrets (`/secrets configure`).
 *   - Auth reads process.env (populated by 00-secrets at init) to check
 *     whether token env vars are set.
 *   - Auth runs CLI tools (`gh`, `glab`, `aws`, etc.) to check session state
 *     and parse error output for specific failure reasons.
 *
 * Load order: 01-auth loads after 00-secrets, so process.env is populated.
 */

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Text } from "@mariozechner/pi-tui";
import { Type } from "@sinclair/typebox";

// Import domain logic from auth.ts (testable without pi-tui dependency)
import {
	diagnoseError,
	extractErrorLine,
	ALL_PROVIDERS,
	STATUS_ICONS,
	formatResults,
	findProvider,
	checkAllProviders,
	type AuthStatus,
	type AuthResult,
	type AuthProvider,
} from "./auth.ts";

// Re-export types for backward compatibility
export type { AuthStatus, AuthResult, AuthProvider };

// ─── Extension ───────────────────────────────────────────────────

export default function authExtension(pi: ExtensionAPI) {

	// ── Tool: whoami (LLM-callable) ───────────────────────────────

	pi.registerTool({
		name: "whoami",
		label: "Auth Status",
		description:
			"Check authentication status across development tools " +
			"(git, GitHub, GitLab, AWS, Kubernetes, OCI registries). " +
			"Returns structured status with error diagnosis and refresh " +
			"commands for expired or missing sessions.",
		promptSnippet:
			"Check auth status across dev tools (git, GitHub, GitLab, AWS, k8s, OCI registries)",

		parameters: Type.Object({}),

		async execute(_toolCallId, _params, signal, _onUpdate, _ctx) {
			const results = await checkAllProviders(pi, signal);
			const text = formatResults(results);
			return {
				content: [{ type: "text", text }],
				details: {
					checks: results.map(r => ({
						provider: r.provider,
						status: r.status,
						detail: r.detail,
						error: r.error,
					})),
				},
			};
		},

		renderCall(_args, theme) {
			return new Text(theme.fg("toolTitle", theme.bold("whoami")), 0, 0);
		},

		renderResult(result, _options, theme) {
			if (result.isError) {
				return new Text(theme.fg("error", result.content?.[0]?.text || "Error"), 0, 0);
			}
			const checks = (result.details?.checks || []) as Array<{ provider: string; status: string; detail: string }>;
			const parts = checks.map(c => {
				const icon = STATUS_ICONS[c.status as AuthStatus] || "?";
				const color = c.status === "ok" ? "success"
					: c.status === "expired" ? "warning"
					: c.status === "missing" ? "muted"
					: "error";
				return theme.fg(color as Parameters<typeof theme.fg>[0], `${icon} ${c.provider}`);
			});
			return new Text(parts.join(theme.fg("dim", " · ")), 0, 0);
		},
	});

	// ── Command: /auth ────────────────────────────────────────────

	pi.registerCommand("auth", {
		description: "Auth management: status | check <provider> | refresh <provider> | list",
		getArgumentCompletions: (prefix: string) => {
			const parts = prefix.split(/\s+/);
			if (parts.length <= 1) {
				const subs = ["status", "check", "refresh", "list"];
				const filtered = subs.filter(s => s.startsWith(parts[0] || ""));
				return filtered.length > 0
					? filtered.map(s => ({ value: s, label: s }))
					: null;
			}
			const sub = parts[0];
			if ((sub === "check" || sub === "refresh") && parts.length === 2) {
				const partial = parts[1] || "";
				return ALL_PROVIDERS
					.filter(p => p.id.startsWith(partial) || p.name.toLowerCase().startsWith(partial))
					.map(p => ({
						value: `${sub} ${p.id}`,
						label: `${p.id} — ${p.name}`,
					}));
			}
			return null;
		},

		handler: async (args, ctx) => {
			const parts = (args || "status").trim().split(/\s+/);
			const subcommand = parts[0];
			const providerArg = parts.slice(1).join(" ");

			switch (subcommand) {
				case "status":
				case "": {
					const results = await checkAllProviders(pi);
					const text = formatResults(results);
					pi.sendMessage({ customType: "view", content: text, display: true });
					break;
				}

				case "check": {
					if (!providerArg) {
						ctx.ui.notify("Usage: /auth check <provider>  (try /auth list)", "error");
						return;
					}
					const provider = findProvider(providerArg);
					if (!provider) {
						ctx.ui.notify(
							`Unknown provider: ${providerArg}\nAvailable: ${ALL_PROVIDERS.map(p => p.id).join(", ")}`,
							"error"
						);
						return;
					}
					try {
						const result = await provider.check(pi);
						const text = formatResults([result]);
						pi.sendMessage({ customType: "view", content: text, display: true });
					} catch (e: any) {
						ctx.ui.notify(`Check failed: ${e.message}`, "error");
					}
					break;
				}

				case "refresh": {
					if (!providerArg) {
						ctx.ui.notify("Usage: /auth refresh <provider>  (try /auth list)", "error");
						return;
					}
					const provider = findProvider(providerArg);
					if (!provider) {
						ctx.ui.notify(
							`Unknown provider: ${providerArg}\nAvailable: ${ALL_PROVIDERS.map(p => p.id).join(", ")}`,
							"error"
						);
						return;
					}

					// Check current state first
					let current: AuthResult;
					try {
						current = await provider.check(pi);
					} catch (e: any) {
						ctx.ui.notify(`Check failed: ${e.message}`, "error");
						return;
					}

					if (current.status === "ok") {
						ctx.ui.notify(`${provider.name} is already authenticated: ${current.detail}`, "info");
						return;
					}

					if (current.status === "missing") {
						ctx.ui.notify(
							`${provider.name}: ${provider.cli} CLI is not installed.\n` +
							(provider.tokenEnvVar
								? `You can set ${provider.tokenEnvVar} instead: /secrets configure ${provider.tokenEnvVar}`
								: `Install ${provider.cli} first.`),
							"warning"
						);
						return;
					}

					// Show refresh instructions — don't execute interactive commands
					// because pi.exec runs without a TTY. CLI login commands (gh auth login,
					// glab auth login, aws sso login) require browser interaction.
					const statusLabel = current.status === "expired"
						? "expired"
						: current.status === "invalid"
							? "invalid"
							: "not authenticated";

					const lines = [
						`**${provider.name}** — ${statusLabel}`,
					];
					if (current.error) {
						lines.push(`Error: ${current.error.split("\n")[0].slice(0, 120)}`);
					}
					lines.push("", "**Options:**");
					lines.push(`  1. Run in your terminal: \`${provider.refreshCommand}\``);
					if (provider.tokenEnvVar) {
						lines.push(`  2. Configure token: \`/secrets configure ${provider.tokenEnvVar}\``);
					}
					lines.push("", "After authenticating, run `/auth check " + provider.id + "` to verify.");

					pi.sendMessage({ customType: "view", content: lines.join("\n"), display: true });
					break;
				}

				case "list": {
					const lines = ALL_PROVIDERS.map(p => {
						let line = `  ${p.id} — ${p.name} (${p.cli})`;
						if (p.tokenEnvVar) line += `  [env: ${p.tokenEnvVar}]`;
						return line;
					});
					ctx.ui.notify("Available auth providers:\n\n" + lines.join("\n"), "info");
					break;
				}

				default:
					ctx.ui.notify(
						"Usage: /auth <status|check|refresh|list> [provider]\n\n" +
						"  /auth               — check all providers\n" +
						"  /auth check github  — check a specific provider\n" +
						"  /auth refresh aws   — show refresh instructions\n" +
						"  /auth list          — list available providers",
						"info"
					);
			}
		},
	});

	// ── Backward compat: /whoami command ──────────────────────────

	pi.registerCommand("whoami", {
		description: "Alias for /auth status — check authentication across dev tools",
		handler: async (_args, _ctx) => {
			const results = await checkAllProviders(pi);
			const text = formatResults(results);
			pi.sendMessage({ customType: "view", content: text, display: true });
		},
	});
}

// Domain logic re-exported from auth.ts for backward compatibility
export {
	diagnoseError,
	extractErrorLine,
	ALL_PROVIDERS,
	STATUS_ICONS,
	formatResults,
	findProvider,
	checkAllProviders,
} from "./auth.ts";

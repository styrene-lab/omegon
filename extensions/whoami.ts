/**
 * whoami — Check authentication status across development tools
 *
 * Registers a `whoami` tool and `/whoami` command that checks login state
 * across git, GitHub, AWS, Kubernetes, and OCI registries.
 *
 * Consolidates bin/identity.sh and prompts/whoami.md into a single extension.
 */

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";

interface AuthCheck {
	domain: string;
	status: "ok" | "expired" | "none" | "missing";
	detail: string;
	refresh?: string;
}

async function runCheck(
	pi: ExtensionAPI,
	signal?: AbortSignal,
): Promise<AuthCheck[]> {
	const checks: AuthCheck[] = [];

	// Git identity
	const gitName = await pi.exec("git", ["config", "user.name"], { signal, timeout: 5_000 });
	const gitEmail = await pi.exec("git", ["config", "user.email"], { signal, timeout: 5_000 });
	const name = gitName.stdout.trim() || "not set";
	const email = gitEmail.stdout.trim() || "not set";
	checks.push({
		domain: "Git",
		status: name !== "not set" ? "ok" : "none",
		detail: `${name} <${email}>`,
	});

	// GitHub CLI
	const ghWhich = await pi.exec("which", ["gh"], { signal, timeout: 3_000 });
	if (ghWhich.code === 0) {
		const gh = await pi.exec("gh", ["auth", "status"], { signal, timeout: 10_000 });
		const output = (gh.stdout + "\n" + gh.stderr).trim();
		if (gh.code === 0) {
			// Extract account from output
			const accountMatch = output.match(/Logged in to .+ as (\S+)/);
			checks.push({
				domain: "GitHub",
				status: "ok",
				detail: accountMatch ? accountMatch[1] : "authenticated",
				refresh: "gh auth login",
			});
		} else {
			checks.push({
				domain: "GitHub",
				status: "none",
				detail: "Not authenticated",
				refresh: "gh auth login",
			});
		}
	} else {
		checks.push({ domain: "GitHub", status: "missing", detail: "gh CLI not installed" });
	}

	// AWS
	const awsWhich = await pi.exec("which", ["aws"], { signal, timeout: 3_000 });
	if (awsWhich.code === 0) {
		const aws = await pi.exec("aws", ["sts", "get-caller-identity", "--output", "json"], { signal, timeout: 10_000 });
		if (aws.code === 0) {
			try {
				const identity = JSON.parse(aws.stdout.trim());
				checks.push({
					domain: "AWS",
					status: "ok",
					detail: `${identity.Arn || identity.Account || "authenticated"}`,
					refresh: "aws sso login --profile <profile>",
				});
			} catch {
				checks.push({ domain: "AWS", status: "ok", detail: "authenticated", refresh: "aws sso login" });
			}
		} else {
			const stderr = aws.stderr || "";
			const expired = stderr.includes("expired") || stderr.includes("ExpiredToken");
			checks.push({
				domain: "AWS",
				status: expired ? "expired" : "none",
				detail: expired ? "Token expired" : "Not authenticated",
				refresh: "aws sso login --profile <profile>",
			});
		}
	} else {
		checks.push({ domain: "AWS", status: "missing", detail: "aws CLI not installed" });
	}

	// Kubernetes
	const kWhich = await pi.exec("which", ["kubectl"], { signal, timeout: 3_000 });
	if (kWhich.code === 0) {
		const kCtx = await pi.exec("kubectl", ["config", "current-context"], { signal, timeout: 5_000 });
		if (kCtx.code === 0) {
			checks.push({
				domain: "Kubernetes",
				status: "ok",
				detail: `context: ${kCtx.stdout.trim()}`,
			});
		} else {
			checks.push({
				domain: "Kubernetes",
				status: "none",
				detail: "No context set",
			});
		}
	} else {
		checks.push({ domain: "Kubernetes", status: "missing", detail: "kubectl not installed" });
	}

	// OCI registries (ghcr.io via podman or docker)
	const podmanWhich = await pi.exec("which", ["podman"], { signal, timeout: 3_000 });
	const dockerWhich = await pi.exec("which", ["docker"], { signal, timeout: 3_000 });
	const containerCmd = podmanWhich.code === 0 ? "podman" : dockerWhich.code === 0 ? "docker" : null;
	if (containerCmd) {
		const ghcr = await pi.exec(containerCmd, ["login", "--get-login", "ghcr.io"], { signal, timeout: 5_000 });
		if (ghcr.code === 0) {
			checks.push({
				domain: "ghcr.io",
				status: "ok",
				detail: ghcr.stdout.trim(),
				refresh: `gh auth token | ${containerCmd} login ghcr.io -u $(gh api user --jq .login) --password-stdin`,
			});
		} else {
			checks.push({
				domain: "ghcr.io",
				status: "none",
				detail: "Not logged in",
				refresh: `gh auth token | ${containerCmd} login ghcr.io -u $(gh api user --jq .login) --password-stdin`,
			});
		}
	}

	return checks;
}

function formatChecks(checks: AuthCheck[]): string {
	const icons: Record<string, string> = { ok: "✓", expired: "⚠", none: "✗", missing: "·" };

	const lines: string[] = ["**Identity & Auth Status**", ""];

	for (const c of checks) {
		const icon = icons[c.status] || "?";
		lines.push(`  ${icon}  **${c.domain}**: ${c.detail}`);
	}

	// Refresh suggestions
	const actionable = checks.filter((c) => (c.status === "expired" || c.status === "none") && c.refresh);
	if (actionable.length > 0) {
		lines.push("", "**Refresh:**");
		for (const c of actionable) {
			lines.push(`  ${c.domain}: \`${c.refresh}\``);
		}
	}

	return lines.join("\n");
}

export default function whoamiExtension(pi: ExtensionAPI) {

	// ------------------------------------------------------------------
	// whoami tool — callable by the LLM
	// ------------------------------------------------------------------
	pi.registerTool({
		name: "whoami",
		label: "Who Am I",
		description:
			"Check authentication status across development tools (git, GitHub, AWS, Kubernetes, OCI registries). " +
			"Returns structured status with refresh commands for expired or missing sessions.",
		promptSnippet:
			"Check auth status across dev tools (git, GitHub, AWS, k8s, OCI registries)",

		parameters: Type.Object({}),

		async execute(_toolCallId, _params, signal, _onUpdate, _ctx) {
			const checks = await runCheck(pi, signal);
			const text = formatChecks(checks);

			return {
				content: [{ type: "text", text }],
				details: {
					checks: checks.map((c) => ({
						domain: c.domain,
						status: c.status,
						detail: c.detail,
					})),
				},
			};
		},
	});

	// ------------------------------------------------------------------
	// /whoami command — interactive
	// ------------------------------------------------------------------
	pi.registerCommand("whoami", {
		description: "Check authentication status across dev tools",
		handler: async (_args, _ctx) => {
			const checks = await runCheck(pi);
			const text = formatChecks(checks);

			pi.sendMessage({
				customType: "view",
				content: text,
				display: true,
			});
		},
	});
}

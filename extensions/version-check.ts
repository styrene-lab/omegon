/**
 * version-check — Polls GitHub for new pi-kit releases and notifies the operator.
 *
 * Checks on session start, then hourly. Compares the installed version
 * (from package.json) against the latest GitHub release tag. If a newer
 * version is found, sends a notification suggesting `pi update`.
 *
 * Respects PI_SKIP_VERSION_CHECK and PI_OFFLINE environment variables.
 */

import type { ExtensionAPI } from "@anthropic-ai/pi-coding-agent";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_OWNER = "cwilson613";
const REPO_NAME = "pi-kit";
const CHECK_INTERVAL_MS = 60 * 60 * 1000; // 1 hour
const FETCH_TIMEOUT_MS = 10_000;

/** Read installed version from package.json */
function getInstalledVersion(): string {
  const pkgPath = join(dirname(fileURLToPath(import.meta.url)), "..", "package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf-8"));
  return pkg.version;
}

/** Fetch the latest release tag from GitHub. Returns version string or null. */
async function fetchLatestRelease(): Promise<string | null> {
  if (process.env.PI_SKIP_VERSION_CHECK || process.env.PI_OFFLINE) return null;

  try {
    const response = await fetch(
      `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`,
      {
        headers: { Accept: "application/vnd.github+json" },
        signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
      },
    );
    if (!response.ok) return null;
    const data = (await response.json()) as { tag_name?: string };
    return data.tag_name?.replace(/^v/, "") ?? null;
  } catch {
    return null;
  }
}

/** Simple semver comparison. Returns true if latest > current. */
function isNewer(latest: string, current: string): boolean {
  const parse = (v: string) => v.split(".").map((n) => parseInt(n, 10) || 0);
  const l = parse(latest);
  const c = parse(current);
  for (let i = 0; i < 3; i++) {
    if ((l[i] ?? 0) > (c[i] ?? 0)) return true;
    if ((l[i] ?? 0) < (c[i] ?? 0)) return false;
  }
  return false;
}

export default function versionCheck(pi: ExtensionAPI) {
  let timer: ReturnType<typeof setInterval> | null = null;
  let notifiedVersion: string | null = null;

  async function check() {
    const installed = getInstalledVersion();
    const latest = await fetchLatestRelease();
    if (!latest || !isNewer(latest, installed)) return;
    if (latest === notifiedVersion) return; // don't spam

    notifiedVersion = latest;
    pi.sendMessage({
      customType: "view",
      content: `**pi-kit update available:** v${installed} → v${latest}\n\nRun \`pi update\` to upgrade.`,
      display: true,
    });
  }

  pi.on("session_start", async () => {
    // Fire-and-forget — don't block session start
    check();
    timer = setInterval(check, CHECK_INTERVAL_MS);
  });

  pi.on("session_shutdown", async () => {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  });
}

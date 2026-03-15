/**
 * Shared debug logging for Omegon extensions.
 *
 * Output goes to a log file (~/.pi/agent/omegon-debug.log) so it doesn't
 * corrupt the TUI. Tail the file in a separate terminal to watch live:
 *   tail -f ~/.pi/agent/omegon-debug.log
 *
 * Controlled by PI_DEBUG environment variable:
 *   PI_DEBUG=1           — all extensions
 *   PI_DEBUG=dashboard   — only dashboard
 *   PI_DEBUG=openspec,cleave — comma-separated list
 *
 * Each log line: [HH:mm:ss.SSS scope:tag] {json}
 */

import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

const PI_DEBUG = process.env.PI_DEBUG ?? "";
const debugAll = PI_DEBUG === "1" || PI_DEBUG === "*" || PI_DEBUG === "true";
const debugScopes = new Set(
  debugAll ? [] : PI_DEBUG.split(",").map((s) => s.trim().toLowerCase()).filter(Boolean),
);

const LOG_DIR = join(homedir(), ".pi", "agent");
const LOG_PATH = join(LOG_DIR, "omegon-debug.log");
let dirEnsured = false;

function ensureDir(): void {
  if (dirEnsured) return;
  try {
    mkdirSync(LOG_DIR, { recursive: true });
  } catch {
    // best effort
  }
  dirEnsured = true;
}

function isEnabled(scope: string): boolean {
  if (debugAll) return true;
  if (debugScopes.size === 0) return false;
  return debugScopes.has(scope.toLowerCase());
}

/** Path to the debug log file, for display to users. */
export const DEBUG_LOG_PATH = LOG_PATH;

/**
 * Log a debug message to the Omegon debug log file.
 *
 * @param scope - Extension name (e.g. "dashboard", "openspec", "cleave")
 * @param tag - Sub-tag for the message (e.g. "render", "emitState", "session_start")
 * @param data - Optional structured data to include
 */
export function debug(scope: string, tag: string, data?: Record<string, unknown>): void {
  if (!isEnabled(scope)) return;
  ensureDir();
  const ts = new Date().toISOString().slice(11, 23); // HH:mm:ss.SSS
  const prefix = `[${ts} ${scope}:${tag}]`;
  const line = data && Object.keys(data).length > 0
    ? `${prefix} ${JSON.stringify(data)}\n`
    : `${prefix}\n`;
  try {
    appendFileSync(LOG_PATH, line);
  } catch {
    // best effort — don't crash extensions over logging
  }
}

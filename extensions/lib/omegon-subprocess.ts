import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

export interface OmegonSubprocessSpec {
  command: string;
  argvPrefix: string[];
  omegonEntry: string;
}

let cached: OmegonSubprocessSpec | null = null;

/**
 * Resolve the canonical Omegon-owned subprocess entrypoint without relying on PATH.
 *
 * Internal helpers should spawn `process.execPath` with `bin/omegon.mjs` explicitly,
 * rather than assuming a `pi` or `omegon` binary on PATH points back to this install.
 */
export function resolveOmegonSubprocess(): OmegonSubprocessSpec {
  if (cached) return cached;

  const here = dirname(fileURLToPath(import.meta.url));
  const omegonEntry = join(here, "..", "..", "bin", "omegon.mjs");
  cached = {
    command: process.execPath,
    argvPrefix: [omegonEntry],
    omegonEntry,
  };
  return cached;
}

// ─── Native agent binary ────────────────────────────────────────────────────

/**
 * Resolved native agent binary specification.
 *
 * When available, cleave children can be dispatched to this binary instead
 * of spawning a full Node.js + Omegon TS process. The native binary is
 * faster to start, uses less memory, and has the core 4 tools built in
 * (read, write, edit, bash).
 */
export interface NativeAgentSpec {
  /** Path to the omegon-agent binary */
  binaryPath: string;
  /** Path to the LLM bridge script (passed via --bridge) */
  bridgePath: string;
}

let nativeCached: NativeAgentSpec | null | undefined;

/**
 * Resolve the native omegon-agent binary if available.
 *
 * Search order:
 * 1. OMEGON_AGENT_BINARY env var (explicit override for CI/testing)
 * 2. core/target/release/omegon-agent (local development build)
 * 3. Adjacent to the Omegon package: node_modules/.omegon/omegon-agent (npm install)
 *
 * Returns null if no binary is found — callers must fall back to TS subprocess.
 * Result is cached for the process lifetime.
 */
export function resolveNativeAgent(): NativeAgentSpec | null {
  // NO CACHING. The binary may be built mid-session (cargo build --release).
  // A stale null cache was the reason native dispatch silently never activated.

  const here = dirname(fileURLToPath(import.meta.url));
  const repoRoot = resolve(here, "..", "..");

  // Bridge is always relative to the repo/package root
  const bridgePath = join(repoRoot, "core", "bridge", "llm-bridge.mjs");

  // 1. Explicit override via env var
  const envPath = process.env.OMEGON_AGENT_BINARY;
  if (envPath && existsSync(envPath)) {
    nativeCached = { binaryPath: envPath, bridgePath };
    return nativeCached;
  }

  // 2. Local development build (cargo build --release in core/)
  const devBinary = join(repoRoot, "core", "target", "release", "omegon-agent");
  if (existsSync(devBinary)) {
    nativeCached = { binaryPath: devBinary, bridgePath };
    return nativeCached;
  }

  // 3. npm platform package install location
  const npmBinary = join(repoRoot, "node_modules", ".omegon", "omegon-agent");
  if (existsSync(npmBinary)) {
    nativeCached = { binaryPath: npmBinary, bridgePath };
    return nativeCached;
  }

  nativeCached = null;
  return null;
}

/**
 * Clear the cached native agent spec. For testing only.
 * @internal
 */
export function _clearNativeAgentCache(): void {
  nativeCached = undefined;
}

#!/usr/bin/env node
/**
 * Omegon pi entry point.
 *
 * Sets PI_CODING_AGENT_DIR to the omegon package root so pi loads all agent
 * configuration (extensions, themes, skills, AGENTS.md) from omegon rather
 * than from ~/.pi/agent/.
 *
 * Resolution order for pi core:
 *   1. vendor/pi-mono (dev mode — git submodule present)
 *   2. node_modules/@cwilson613/pi-coding-agent (installed via npm)
 */
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const omegonRoot = dirname(dirname(__filename));

// Resolve pi core: prefer vendor/ (dev), fall back to node_modules/ (installed)
const vendorCli = join(omegonRoot, "vendor/pi-mono/packages/coding-agent/dist/cli.js");
const npmCli = join(omegonRoot, "node_modules/@cwilson613/pi-coding-agent/dist/cli.js");
const cli = existsSync(vendorCli) ? vendorCli : npmCli;
const resolutionMode = cli === vendorCli ? "vendor" : "npm";

if (process.argv.includes("--where")) {
  process.stdout.write(JSON.stringify({
    omegonRoot,
    cli,
    resolutionMode,
    agentDir: process.env.PI_CODING_AGENT_DIR ?? omegonRoot,
  }, null, 2) + "\n");
  process.exit(0);
}

// Only set if not already overridden
if (!process.env.PI_CODING_AGENT_DIR) {
  process.env.PI_CODING_AGENT_DIR = omegonRoot;
}

await import(cli);

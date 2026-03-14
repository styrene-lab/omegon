/**
 * Secrets Extension
 *
 * Resolves secrets from user-configured sources (env vars, shell commands, keychains)
 * without duplicating or storing secret values. Provides:
 *
 * Layer 1: resolveSecret() — extensions call this to get secrets from user-configured recipes
 * Layer 2: Output redaction — scrubs known secret values from tool results before they reach the agent
 * Layer 3: Tool guard — blocks/confirms tool calls that access sensitive paths or secret stores
 * Layer 4: Recipe file — stores resolution recipes, never literal secrets
 * Layer 5: Local model scrub — redacts secrets from outbound ask_local_model prompts
 * Layer 6: Audit log — append-only record of all guard decisions
 *
 * Commands: /secrets list, /secrets configure <name>, /secrets rm <name>, /secrets test <name>
 */

import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import { existsSync, readFileSync, realpathSync, writeFileSync, appendFileSync, mkdirSync, readdirSync } from "fs";
import { dirname, join, resolve } from "path";
import { fileURLToPath } from "url";
import { homedir } from "os";
import { execSync, execFileSync } from "child_process";

// ============================================================================
// Config
// ============================================================================

const SECRETS_DIR = join(homedir(), ".pi", "agent");
const SECRETS_FILE = join(SECRETS_DIR, "secrets.json");
const AUDIT_LOG_FILE = join(SECRETS_DIR, "secrets-audit.jsonl");

/** Fallback secrets not tied to a specific extension */
const BUILTIN_SECRETS: Record<string, string> = {};

/** Fallback config vars not tied to a specific extension */
const BUILTIN_CONFIGS: Record<string, { description: string; default?: string }> = {};

/**
 * Scan extension directories for annotations:
 *   // @secret NAME "description"
 *   // @config NAME "description" [default: value]
 *
 * @secret — sensitive values (API keys, tokens) that need redaction and guarded access
 * @config — non-sensitive env var overrides (paths, URLs, feature flags) surfaced in /secrets list
 */
function scanAnnotations(): {
  secrets: Record<string, string>;
  configs: Record<string, { description: string; default?: string }>;
} {
  const secrets: Record<string, string> = { ...BUILTIN_SECRETS };
  const configs: Record<string, { description: string; default?: string }> = { ...BUILTIN_CONFIGS };
  const secretPattern = /^\/\/\s*@secret\s+([A-Z_][A-Z0-9_]*)\s+"([^"]+)"/;
  const configPattern = /^\/\/\s*@config\s+([A-Z_][A-Z0-9_]*)\s+"([^"]+)"(?:\s+\[default:\s*([^\]]*)\])?/;

  // Extension directories to scan (deduplicated by realpath to avoid
  // double-walking when dev checkout overlaps with git-installed package)
  const seen = new Set<string>();
  const extensionDirs: string[] = [];
  function addDir(dir: string) {
    try {
      const real = realpathSync(dir);
      if (!seen.has(real)) { seen.add(real); extensionDirs.push(dir); }
    } catch {
      // realpathSync fails if dir doesn't exist — skip silently
    }
  }

  addDir(join(homedir(), ".pi", "agent", "extensions"));
  addDir(join(homedir(), ".pi", "agent", "git"));  // Omegon and other git packages

  // Scan the package's own extensions/ directory (where this file lives).
  // Covers both dev (repo checkout) and npm-installed modes.
  try {
    const thisDir = dirname(fileURLToPath(import.meta.url));
    addDir(resolve(thisDir, ".."));  // 00-secrets/ → extensions/
  } catch {}

  // Also scan project-local extensions
  try {
    const cwd = process.cwd();
    addDir(join(cwd, ".pi", "extensions"));
  } catch {}

  function scanFile(filePath: string) {
    try {
      const content = readFileSync(filePath, "utf-8");
      // Only scan the first 30 lines for annotations (they should be at the top)
      const lines = content.split("\n").slice(0, 30);
      for (const line of lines) {
        const secretMatch = line.match(secretPattern);
        if (secretMatch) {
          secrets[secretMatch[1]] = secretMatch[2];
          continue;
        }
        const configMatch = line.match(configPattern);
        if (configMatch) {
          configs[configMatch[1]] = {
            description: configMatch[2],
            default: configMatch[3]?.trim(),
          };
        }
      }
    } catch {}
  }

  function walkDir(dir: string) {
    if (!existsSync(dir)) return;
    try {
      const entries = readdirSync(dir, { withFileTypes: true });
      for (const entry of entries) {
        const fullPath = join(dir, entry.name);
        if (entry.isFile() && (entry.name.endsWith(".ts") || entry.name.endsWith(".js"))) {
          scanFile(fullPath);
        } else if (entry.isDirectory() && !entry.name.startsWith(".") && entry.name !== "node_modules") {
          walkDir(fullPath);
        }
      }
    } catch {}
  }

  for (const dir of extensionDirs) {
    walkDir(dir);
  }

  return { secrets, configs };
}

/** Discovered annotations — scanned once at load time */
const { secrets: KNOWN_SECRETS, configs: KNOWN_CONFIGS } = scanAnnotations();

// ============================================================================
// Recipe types
// ============================================================================

/**
 * Recipe format:
 * - "!command args"  → shell command, stdout is the secret
 * - "ENV_VAR_NAME"   → read from environment variable
 * - "literal:value"  → literal value (discouraged, warned about)
 */
type RecipeMap = Record<string, string>;

// ============================================================================
// State — resolved secrets cached in memory, never written to disk
// ============================================================================

let recipes: RecipeMap = {};
const resolvedCache = new Map<string, string>();

// ============================================================================
// Core: Recipe loading
// ============================================================================

function loadRecipes(): RecipeMap {
  if (!existsSync(SECRETS_FILE)) return {};
  try {
    const raw = readFileSync(SECRETS_FILE, "utf-8");
    return JSON.parse(raw) as RecipeMap;
  } catch {
    return {};
  }
}

function saveRecipes(r: RecipeMap): void {
  mkdirSync(SECRETS_DIR, { recursive: true });
  writeFileSync(SECRETS_FILE, JSON.stringify(r, null, 2) + "\n", { mode: 0o600 });
}

// ============================================================================
// Core: Secret resolution
// ============================================================================

function executeRecipe(recipe: string): string | undefined {
  // Shell command
  if (recipe.startsWith("!")) {
    try {
      const cmd = recipe.slice(1).trim();
      const result = execSync(cmd, {
        encoding: "utf-8",
        timeout: 10_000,
        stdio: ["pipe", "pipe", "pipe"],
      }).trim();
      return result || undefined;
    } catch {
      return undefined;
    }
  }

  // Literal value (discouraged)
  if (recipe.startsWith("literal:")) {
    return recipe.slice(8);
  }

  // Environment variable name
  return process.env[recipe] || undefined;
}

/**
 * Resolve a secret by name. Resolution order:
 * 1. In-memory cache (already resolved this session)
 * 2. process.env[name] — always checked first for CI/container compat
 * 3. Recipe from secrets.json
 * 4. undefined — caller handles missing secret gracefully
 */
export function resolveSecret(name: string): string | undefined {
  // Check cache
  const cached = resolvedCache.get(name);
  if (cached) return cached;

  // Always check env first (CI, containers, explicit overrides)
  const envVal = process.env[name];
  if (envVal) {
    resolvedCache.set(name, envVal);
    return envVal;
  }

  // Check recipe
  const recipe = recipes[name];
  if (!recipe) return undefined;

  const value = executeRecipe(recipe);
  if (value) {
    resolvedCache.set(name, value);
  }
  return value;
}

// ============================================================================
// Layer 6: Audit log
// ============================================================================

function logGuardDecision(event: {
  tool: string;
  target: string;
  action: "blocked" | "allowed" | "confirmed";
  reason: string;
}): void {
  try {
    mkdirSync(SECRETS_DIR, { recursive: true });
    const entry = JSON.stringify({ ...event, timestamp: new Date().toISOString() }) + "\n";
    appendFileSync(AUDIT_LOG_FILE, entry, { mode: 0o600 });
  } catch {}
}

// ============================================================================
// Layer 2: Output redaction
// ============================================================================

/** Minimum secret length for redaction. Shorter values cause too many false positives. */
const MIN_REDACT_LENGTH = 4;

function redactString(input: string, secrets: Array<{ name: string; value: string }>): string {
  let result = input;
  for (const { name, value } of secrets) {
    if (value.length < MIN_REDACT_LENGTH) continue;

    // Escape regex special characters in the secret value
    const escaped = value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const marker = `[REDACTED:${name}]`;

    // Replace all occurrences of the full value
    result = result.replace(new RegExp(escaped, "g"), marker);

    // Also check for base64-encoded form of the secret
    try {
      const b64 = Buffer.from(value).toString("base64");
      if (b64.length >= 8) {
        const b64Escaped = b64.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        result = result.replace(new RegExp(b64Escaped, "g"), `[REDACTED:${name}:base64]`);
      }
    } catch {}

    // Also redact partial prefixes for very long secrets (40+ chars)
    // Use only first 12 chars to reduce false positives from standard prefixes
    if (value.length > 40) {
      const partialEscaped = value.slice(0, 12).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      result = result.replace(new RegExp(partialEscaped, "g"), marker);
    }
  }
  return result;
}

function getRedactableSecrets(): Array<{ name: string; value: string }> {
  return Array.from(resolvedCache.entries())
    .filter(([_, v]) => v.length >= MIN_REDACT_LENGTH)
    .map(([name, value]) => ({ name, value }));
}

function redactContent(content: any[]): any[] {
  const secrets = getRedactableSecrets();
  if (secrets.length === 0) return content;

  return content.map((block: any) => {
    if (block.type === "text" && typeof block.text === "string") {
      const redacted = redactString(block.text, secrets);
      if (redacted !== block.text) {
        return { ...block, text: redacted };
      }
    }
    return block;
  });
}

// ============================================================================
// Layer 3: Sensitive path detection
// ============================================================================

/**
 * Sensitive path patterns. Each entry defines:
 * - pattern: regex to match against resolved absolute paths
 * - description: human-readable label for UI prompts
 * - action: "block" = hard block (no override), "confirm" = prompt user
 */
const SENSITIVE_PATH_PATTERNS: Array<{
  pattern: RegExp;
  description: string;
  action: "block" | "confirm";
}> = [
  // The recipe store itself — hard block, only /secrets commands may access
  { pattern: /\.pi\/agent\/secrets\.json$/i, description: "secrets recipe store", action: "block" },
  // The audit log — hard block, prevent tampering
  { pattern: /\.pi\/agent\/secrets-audit\.jsonl$/i, description: "secrets audit log", action: "block" },
  // Dotenv files
  { pattern: /\.env(\.[a-z]+)?$/i, description: "dotenv file", action: "confirm" },
  // SSH keys and config
  { pattern: /\.ssh\/(id_[a-z0-9]+|config|known_hosts|authorized_keys)(\.pub)?$/i, description: "SSH credential/config", action: "confirm" },
  // AWS credentials
  { pattern: /\.aws\/(credentials|config)$/i, description: "AWS credentials", action: "confirm" },
  // GCP credentials
  { pattern: /\.config\/gcloud\/.*(credentials|tokens|properties)/i, description: "GCP credentials", action: "confirm" },
  // Azure credentials
  { pattern: /\.azure\/(credentials|accessTokens\.json)/i, description: "Azure credentials", action: "confirm" },
  // GPG private data
  { pattern: /\.gnupg\/(secring|trustdb|private-keys)/i, description: "GPG private data", action: "confirm" },
  // Netrc
  { pattern: /\.(netrc|curlrc)$/i, description: "netrc/curlrc file", action: "confirm" },
  // Generic credential files
  { pattern: /\bcredentials?\.(json|yaml|yml|toml|xml)$/i, description: "credentials file", action: "confirm" },
  // Token/secret/key files
  { pattern: /\b(token|secret|private[_-]?key)\.(json|pem|key|txt)$/i, description: "token/key file", action: "confirm" },
  // Docker config (contains registry auth tokens)
  { pattern: /\.docker\/config\.json$/i, description: "Docker config (may contain auth)", action: "confirm" },
  // NPM auth
  { pattern: /\.npmrc$/i, description: "npm config (may contain auth tokens)", action: "confirm" },
  // Kubernetes config
  { pattern: /\.kube\/config$/i, description: "Kubernetes config", action: "confirm" },
];

/**
 * Check if a path matches any sensitive path pattern.
 * Returns the matching entry or undefined.
 */
function matchSensitivePath(filePath: string): { description: string; action: "block" | "confirm" } | undefined {
  // Normalize: resolve to absolute, then check patterns
  let normalized: string;
  try {
    normalized = resolve(filePath);
  } catch {
    normalized = filePath;
  }

  for (const entry of SENSITIVE_PATH_PATTERNS) {
    if (entry.pattern.test(normalized) || entry.pattern.test(filePath)) {
      return { description: entry.description, action: entry.action };
    }
  }
  return undefined;
}

// ============================================================================
// Layer 3: Bash guard patterns
// ============================================================================

const SECRET_ACCESS_PATTERNS = [
  // ── Direct secret store access ──
  // macOS Keychain
  /\bsecurity\s+find-generic-password/i,
  /\bsecurity\s+find-internet-password/i,
  // 1Password
  /\bop\s+(read|get|item)\b/i,
  // pass (GPG password store)
  /\bpass\s+(show|ls)\b/i,
  // Vault
  /\bvault\s+(read|kv\s+get)\b/i,

  // ── Environment variable dumping ──
  // Targeted env access with secret-adjacent keywords
  /\benv\b.*\b(key|token|secret|password|credential)/i,
  /\bprintenv\b.*\b(key|token|secret|password|credential)/i,
  // Echo/printf of known secret env vars
  /\b(echo|printf)\s+.*\$[A-Z_]*(KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL)/i,
  // Full env dumps (these can leak all injected secrets)
  /\bnode\s+-e\s+.*process\.env/i,
  /\bpython[23]?\s+-c\s+.*os\.environ/i,
  /\bruby\s+-e\s+.*ENV/i,
  /\bperl\s+-e\s+.*%ENV/i,

  // ── File readers on sensitive paths ──
  // cat/less/more/head/tail/bat on secret-adjacent files
  /\b(cat|less|more|head|tail|bat|batcat)\b.*(secrets?\.json|\bcredentials?\b|\.env\b)/i,
  // jq on secret files
  /\bjq\b.*\b(secrets?\.json|credentials?)\b/i,
  // sed/awk/grep reading secret files
  /\b(sed|awk|grep)\b.*\b(secrets?\.json|credentials?)\b/i,
  // Our own secrets file — match the specific path
  /\.pi\/agent\/secrets\.json/i,
  // Writing to secrets file (via tee, redirect, etc.)
  />\s*.*\.pi\/agent\/secrets\.json/i,
  // AWS/GCP credential file access
  /\b(cat|less|more|head|tail)\b.*\.(aws|gcloud)\/(credentials|config)/i,

  // ── Command wrapping (shell indirection) ──
  // sh/bash/zsh -c wrapping with secret-adjacent content
  /\b(sh|bash|zsh)\s+-c\s+.*\b(security|op\s+read|pass\s+show|vault\s+read|keychain|credential|secret)/i,
  // Python/Ruby/Node/Perl subprocess wrappers accessing secret stores
  /\b(python[23]?|ruby|node|perl)\b.*\b(security\s+find|op\s+read|find-generic-password|secrets?\.json)/i,
  // Base64 decode piped to shell (obfuscation technique)
  /\bbase64\s+(-d|--decode)\b.*\|\s*(sh|bash|zsh)\b/i,
  // eval with encoded content
  /\beval\b.*\$\(.*base64/i,
];

function isSecretAccessCommand(command: string): boolean {
  return SECRET_ACCESS_PATTERNS.some((p) => p.test(command));
}

// ============================================================================
// Clipboard helpers — read secret values without showing them on screen
// ============================================================================

/**
 * Detect which clipboard read command is available.
 * Returns the command string or undefined if none found.
 */
function detectClipboardCommand(): string | undefined {
  // macOS
  try { execSync("which pbpaste", { stdio: "pipe" }); return "pbpaste"; } catch {}
  // Linux (X11)
  try { execSync("which xclip", { stdio: "pipe" }); return "xclip -selection clipboard -o"; } catch {}
  // Linux (X11/Wayland)
  try { execSync("which xsel", { stdio: "pipe" }); return "xsel --clipboard --output"; } catch {}
  // Wayland
  try { execSync("which wl-paste", { stdio: "pipe" }); return "wl-paste --no-newline"; } catch {}
  return undefined;
}

/**
 * Read the current clipboard contents. Returns undefined on failure.
 */
function readClipboard(): string | undefined {
  const cmd = detectClipboardCommand();
  if (!cmd) return undefined;
  try {
    return execSync(cmd, { encoding: "utf-8", timeout: 5_000, stdio: ["pipe", "pipe", "pipe"] }).trim();
  } catch {
    return undefined;
  }
}

/**
 * Prompt the user to provide a secret value via clipboard (preferred) or
 * fallback to direct input if clipboard is unavailable.
 *
 * The clipboard flow avoids showing the secret on screen — the user copies
 * the value first, then confirms, and we read it from the clipboard.
 */
async function promptForSecretValue(
  ctx: any,
  secretName: string,
  promptMessage: string,
): Promise<string | undefined> {
  const clipCmd = detectClipboardCommand();

  if (clipCmd) {
    // Clipboard-based flow — value never shown on screen
    const confirmed = await ctx.ui.confirm(
      `🔐 ${secretName}`,
      `${promptMessage}\n\n` +
      `Copy the value to your clipboard, then confirm.\n` +
      `The value will be read from the clipboard and will not be displayed.`
    );
    if (!confirmed) return undefined;

    const value = readClipboard();
    if (!value) {
      ctx.ui.notify(`❌ Clipboard is empty or unreadable. Copy the value and try again.`, "error");
      return undefined;
    }

    // Show length confirmation without revealing the value
    const charDesc = `${value.length} character${value.length !== 1 ? "s" : ""}`;
    const confirmValue = await ctx.ui.confirm(
      `🔐 ${secretName}`,
      `Read ${charDesc} from clipboard. Use this value?`
    );
    if (!confirmValue) return undefined;

    return value;
  }

  // Fallback: no clipboard command available — warn and use direct input
  ctx.ui.notify(
    `⚠️ No clipboard command found (pbpaste, xclip, xsel, wl-paste).\n` +
    `The value will be visible as you type.`,
    "warning"
  );
  return ctx.ui.input(promptMessage);
}

// ============================================================================
// macOS Keychain helpers
// ============================================================================

const KEYCHAIN_ACCOUNT = "pi-kit";
const KEYCHAIN_SERVICE_PREFIX = "pi-kit/";

/**
 * Store a value in the macOS login keychain under service "pi-kit/<name>".
 * macOS will prompt Touch ID / password / smart card automatically if the
 * keychain is locked — the OS owns the auth flow, we just call the command.
 *
 * Uses execFileSync (no shell) to avoid bash interpreting $, `, \, ! in
 * the secret value. JSON.stringify + execSync was silently eating characters
 * like $FOO (expanded as empty variable).
 */
function storeInKeychain(secretName: string, value: string): void {
  // Use -U to update if item already exists
  execFileSync("security", [
    "add-generic-password",
    "-U",
    "-a", KEYCHAIN_ACCOUNT,
    "-s", KEYCHAIN_SERVICE_PREFIX + secretName,
    "-w", value,
  ], { stdio: ["pipe", "pipe", "pipe"], timeout: 30_000 });
}

function keychainRecipe(secretName: string): string {
  return `!security find-generic-password -a ${JSON.stringify(KEYCHAIN_ACCOUNT)} -ws ${JSON.stringify(KEYCHAIN_SERVICE_PREFIX + secretName)}`;
}

// ============================================================================
// Extension
// ============================================================================

export default function (pi: ExtensionAPI) {
  // Load recipes on init
  recipes = loadRecipes();

  // Warn about literal recipes at load time
  const literalRecipes = Object.entries(recipes).filter(([_, r]) => r.startsWith("literal:"));

  // Pre-resolve all configured secrets at init time (Layer 1)
  // Resolved values are injected into process.env so other extensions
  // can keep using process.env.X without importing from this module.
  // This means the secrets extension MUST load before other extensions
  // that consume secrets (pi loads extensions in alphabetical order by
  // directory name, so "secrets" loads before "web-search" etc.)
  for (const name of Object.keys(recipes)) {
    const value = resolveSecret(name);
    if (value && !process.env[name]) {
      process.env[name] = value;
    }
  }
  // Also track known secrets already in env (for CI compat + redaction)
  for (const name of Object.keys(KNOWN_SECRETS)) {
    if (process.env[name] && !resolvedCache.has(name)) {
      resolvedCache.set(name, process.env[name]!);
    }
  }

  pi.on("session_start", async (_event, ctx) => {
    const resolved = Array.from(resolvedCache.keys());
    const failed = Object.keys(recipes).filter(k => !resolvedCache.has(k));

    if (resolved.length > 0) {
      // Don't leak secret names to the agent — just show count
      ctx.ui.notify(
        `🔐 ${resolved.length} secret${resolved.length !== 1 ? "s" : ""} resolved`,
        "info"
      );
    }

    // Surface failures prominently — don't let broken secrets go unnoticed
    if (failed.length > 0) {
      ctx.ui.notify(
        `❌ ${failed.length} secret${failed.length !== 1 ? "s" : ""} failed to resolve.\n` +
        `Run /secrets list to see details, then /secrets configure <name> to fix.`,
        "error"
      );
    }

    // Warn about literal recipes (plaintext secrets in secrets.json)
    if (literalRecipes.length > 0) {
      ctx.ui.notify(
        `⚠️ ${literalRecipes.length} secret${literalRecipes.length !== 1 ? "s" : ""} stored as plaintext literal${literalRecipes.length !== 1 ? "s" : ""}.\n` +
        `Run /secrets configure <name> to migrate to Keychain or another secure backend.`,
        "error"
      );
    }

    // Warn about secrets resolved from bare env vars (no recipe — set in
    // .bashrc/.zshrc or shell profile). These are insecure: visible in
    // /proc/*/environ, inherited by every child process, persisted in
    // dotfile repos and shell history. A keychain-backed recipe resolves
    // at runtime with biometric/password auth and doesn't leak on disk.
    //
    // Skip in CI environments where env vars are the expected mechanism.
    // Exempt tokens managed by their own CLI credential stores (e.g.
    // GH_TOKEN set by `gh auth login`, GITHUB_TOKEN from gh, COPILOT_GITHUB_TOKEN
    // from the Copilot extension) — these are already secured by the tool.
    const isCI = !!(process.env.CI || process.env.GITHUB_ACTIONS || process.env.GITLAB_CI);
    if (!isCI) {
      // Tokens managed by CLI tools that handle their own credential storage
      const CLI_MANAGED_TOKENS = new Set([
        "GH_TOKEN", "GITHUB_TOKEN", "COPILOT_GITHUB_TOKEN",  // gh auth login
        "GITLAB_TOKEN",     // glab auth login
        "AWS_PROFILE",      // aws configure / SSO profile name (not a secret)
      ]);
      const bareEnvSecrets = Object.keys(KNOWN_SECRETS).filter(name =>
        resolvedCache.has(name) && !recipes[name] && !CLI_MANAGED_TOKENS.has(name)
      );
      if (bareEnvSecrets.length > 0) {
        // Show at most 3 names to avoid wall-of-text, never show values
        const examples = bareEnvSecrets.slice(0, 3).join(", ");
        const more = bareEnvSecrets.length > 3 ? ` (+${bareEnvSecrets.length - 3} more)` : "";
        ctx.ui.notify(
          `🔓 ${bareEnvSecrets.length} secret${bareEnvSecrets.length !== 1 ? "s" : ""} loaded from plain env vars: ${examples}${more}\n` +
          `Run \`/secrets configure <name>\` to migrate to a secure backend.`,
          "warning"
        );
      }
    }
  });

  // ──────────────────────────────────────────────────────────────
  // Layer 2: Redact secrets from tool results
  // ──────────────────────────────────────────────────────────────

  pi.on("tool_result", async (event, _ctx) => {
    if (!event.content || resolvedCache.size === 0) return undefined;

    const redacted = redactContent(event.content);
    // Only return if we actually changed something
    const changed = redacted.some(
      (block: any, i: number) =>
        block.type === "text" &&
        event.content[i]?.type === "text" &&
        block.text !== (event.content[i] as any).text
    );

    if (changed) {
      return { content: redacted };
    }
    return undefined;
  });

  // ──────────────────────────────────────────────────────────────
  // Layer 3: Tool guards (read, grep, find, ls, bash, write, edit, ask_local_model)
  // ──────────────────────────────────────────────────────────────

  /**
   * Helper: given a tool name, a sensitive path match, and the context,
   * handle the block/confirm logic and return the appropriate ToolCallEventResult.
   */
  async function handleSensitivePathAccess(
    toolName: string,
    filePath: string,
    match: { description: string; action: "block" | "confirm" },
    ctx: any,
  ): Promise<{ block: boolean; reason: string } | undefined> {
    if (match.action === "block") {
      logGuardDecision({ tool: toolName, target: filePath, action: "blocked", reason: match.description });
      return {
        block: true,
        reason: `🔐 Blocked: ${toolName} access to ${match.description} (${filePath}). Use /secrets commands to manage secrets.`,
      };
    }

    // action === "confirm"
    if (!ctx.hasUI) {
      logGuardDecision({ tool: toolName, target: filePath, action: "blocked", reason: `${match.description} (no UI)` });
      return {
        block: true,
        reason: `🔐 Blocked: ${toolName} access to ${match.description} (no UI for confirmation)`,
      };
    }

    const choice = await ctx.ui.select(
      `🔐 This ${toolName} accesses a sensitive file:\n\n` +
      `  ${filePath}\n  (${match.description})\n\nAllow?`,
      ["Yes, allow this time", "No, block it"]
    );

    if (choice !== "Yes, allow this time") {
      logGuardDecision({ tool: toolName, target: filePath, action: "blocked", reason: `${match.description} (user denied)` });
      return { block: true, reason: `🔐 Blocked by user: ${match.description}` };
    }

    logGuardDecision({ tool: toolName, target: filePath, action: "confirmed", reason: match.description });
    return undefined;
  }

  pi.on("tool_call", async (event, ctx) => {
    // ── Guard: read tool ──
    if (event.toolName === "read") {
      const path = (event.input as any).path as string;
      if (path) {
        const match = matchSensitivePath(path);
        if (match) {
          return handleSensitivePathAccess("read", path, match, ctx);
        }
      }
      return undefined;
    }

    // ── Guard: write/edit tools — block writes to secrets.json and audit log ──
    if (event.toolName === "write" || event.toolName === "edit") {
      const path = (event.input as any).path as string;
      if (path) {
        const match = matchSensitivePath(path);
        if (match && match.action === "block") {
          logGuardDecision({ tool: event.toolName, target: path, action: "blocked", reason: match.description });
          return {
            block: true,
            reason: `🔐 Blocked: use /secrets configure to manage secret recipes, not direct file ${event.toolName}s.`,
          };
        }
      }
      return undefined;
    }

    // ── Guard: grep tool ──
    if (event.toolName === "grep") {
      const input = event.input as any;
      const searchPath = input.path as string | undefined;
      const pattern = input.pattern as string | undefined;

      // Check if grep target path is sensitive
      if (searchPath) {
        const match = matchSensitivePath(searchPath);
        if (match) {
          return handleSensitivePathAccess("grep", searchPath, match, ctx);
        }
      }

      // Check if the grep pattern itself contains a known secret value
      if (pattern && resolvedCache.size > 0) {
        for (const [name, value] of resolvedCache.entries()) {
          if (value.length >= MIN_REDACT_LENGTH && pattern.includes(value)) {
            logGuardDecision({ tool: "grep", target: `pattern containing ${name}`, action: "blocked", reason: "grep pattern contains secret value" });
            return {
              block: true,
              reason: `🔐 Blocked: grep pattern contains the value of secret ${name}. Don't search for secret values directly.`,
            };
          }
        }
      }
      return undefined;
    }

    // ── Guard: find tool ──
    if (event.toolName === "find") {
      const input = event.input as any;
      const searchPath = input.path as string | undefined;

      if (searchPath) {
        const match = matchSensitivePath(searchPath);
        if (match) {
          return handleSensitivePathAccess("find", searchPath, match, ctx);
        }
      }
      return undefined;
    }

    // ── Guard: ls tool ──
    if (event.toolName === "ls") {
      const input = event.input as any;
      const lsPath = input.path as string | undefined;

      if (lsPath) {
        const match = matchSensitivePath(lsPath);
        if (match) {
          return handleSensitivePathAccess("ls", lsPath, match, ctx);
        }
      }
      return undefined;
    }

    // ── Guard: bash tool ──
    if (event.toolName === "bash") {
      const command = (event.input as any).command as string;
      if (isSecretAccessCommand(command)) {
        if (!ctx.hasUI) {
          logGuardDecision({ tool: "bash", target: command, action: "blocked", reason: "secret store access (no UI)" });
          return {
            block: true,
            reason: "🔐 Blocked: command accesses secret store (no UI for confirmation)",
          };
        }

        const choice = await ctx.ui.select(
          `🔐 This command accesses a secret store:\n\n  ${command}\n\nAllow?`,
          ["Yes, allow this time", "No, block it"]
        );

        if (choice !== "Yes, allow this time") {
          logGuardDecision({ tool: "bash", target: command, action: "blocked", reason: "secret store access (user denied)" });
          return { block: true, reason: "🔐 Blocked by user: secret store access" };
        }

        logGuardDecision({ tool: "bash", target: command, action: "confirmed", reason: "secret store access" });
      }
      return undefined;
    }

    // ── Layer 5: Scrub secrets from local model prompts ──
    if (event.toolName === "ask_local_model") {
      const input = event.input as any;
      if (!input.prompt || resolvedCache.size === 0) return undefined;

      const secrets = getRedactableSecrets();
      const cleanPrompt = redactString(input.prompt, secrets);
      const cleanSystem = input.system ? redactString(input.system, secrets) : input.system;

      if (cleanPrompt !== input.prompt || cleanSystem !== input.system) {
        logGuardDecision({ tool: "ask_local_model", target: "(prompt)", action: "blocked", reason: "prompt contains secret values" });
        return {
          block: true,
          reason:
            "🔐 Blocked: prompt to local model contains secret values. " +
            "Remove sensitive data before delegating to local inference.",
        };
      }
    }

    return undefined;
  });

  // ──────────────────────────────────────────────────────────────
  // Commands: /secrets list | configure | rm | test
  // ──────────────────────────────────────────────────────────────

  pi.registerCommand("secrets", {
    description: "Manage secret resolution recipes: list, configure <name>, rm <name>, test <name>",
    getArgumentCompletions: (prefix: string) => {
      const parts = prefix.split(/\s+/);
      if (parts.length <= 1) {
        // Complete subcommand
        const subs = ["list", "configure", "rm", "test"];
        const filtered = subs.filter(s => s.startsWith(parts[0] || ""));
        return filtered.length > 0 ? filtered.map(s => ({ value: s, label: s })) : null;
      }
      const sub = parts[0];
      if (sub === "configure" || sub === "rm" || sub === "test") {
        // Complete secret name
        const namePrefix = parts.slice(1).join(" ");
        const allNames = [
          ...Object.keys(KNOWN_SECRETS),
          ...Object.keys(recipes).filter(k => !(k in KNOWN_SECRETS)),
        ];
        const filtered = allNames.filter(n => n.startsWith(namePrefix));
        return filtered.length > 0
          ? filtered.map(n => ({ value: `${sub} ${n}`, label: `${n}  ${KNOWN_SECRETS[n] || "custom"}` }))
          : null;
      }
      return null;
    },
    handler: async (args, ctx) => {
      const parts = (args || "").trim().split(/\s+/);
      const subcommand = parts[0] || "list";
      const secretName = parts.slice(1).join(" ");

      switch (subcommand) {
        case "list": {
          const lines: string[] = ["Secret recipes (~/.pi/agent/secrets.json):", ""];

          for (const [name, desc] of Object.entries(KNOWN_SECRETS)) {
            const recipe = recipes[name];
            const resolved = resolvedCache.has(name);
            const source = recipe
              ? recipe.startsWith("!")
                ? `command: ${recipe.slice(1, 40)}${recipe.length > 41 ? "..." : ""}`
                : recipe.startsWith("literal:")
                  ? "⚠️  literal value (insecure — run /secrets configure to migrate)"
                  : `env: ${recipe}`
              : resolved
                ? "🔓 plain env var (run /secrets configure to use a secure backend)"
                : "not configured";

            const status = resolved ? "✅" : "❌";
            lines.push(`  ${status} ${name}`);
            lines.push(`     ${desc}`);
            lines.push(`     Source: ${source}`);
            lines.push("");
          }

          // Show any non-known secrets
          for (const name of Object.keys(recipes)) {
            if (name in KNOWN_SECRETS) continue;
            const recipe = recipes[name];
            const resolved = resolvedCache.has(name);
            const status = resolved ? "✅" : "❌";
            lines.push(`  ${status} ${name} (custom)`);
            lines.push(
              `     Source: ${recipe.startsWith("!") ? `command: ${recipe.slice(1, 40)}` : recipe.startsWith("literal:") ? "⚠️  literal (insecure)" : `env: ${recipe}`}`
            );
            lines.push("");
          }

          // Show @config entries
          const configEntries = Object.entries(KNOWN_CONFIGS);
          if (configEntries.length > 0) {
            lines.push("", "Configuration overrides (@config):", "");
            for (const [name, { description, default: defaultVal }] of configEntries) {
              const envVal = process.env[name];
              const isOverridden = !!envVal && envVal !== defaultVal;
              const status = isOverridden ? "⚙️" : "  ";
              lines.push(`  ${status} ${name}`);
              lines.push(`     ${description}`);
              if (defaultVal) {
                lines.push(`     Default: ${defaultVal}`);
              }
              if (isOverridden) {
                lines.push(`     Override: ${envVal}`);
              } else if (!envVal && !defaultVal) {
                lines.push(`     Value: (not set)`);
              }
              lines.push("");
            }
          }

          ctx.ui.notify(lines.join("\n"), "info");
          break;
        }

        case "configure": {
          if (!secretName) {
            ctx.ui.notify("Usage: /secrets configure <NAME>", "error");
            return;
          }

          if (!ctx.hasUI) {
            ctx.ui.notify("Cannot configure secrets without interactive UI", "error");
            return;
          }

          const desc = KNOWN_SECRETS[secretName] || "Custom secret";
          const currentRecipe = recipes[secretName];

          // Check which backends are available
          let hasOp = false;
          let hasKeychain = false;
          try { execSync("which op", { stdio: "pipe" }); hasOp = true; } catch {}
          try { execSync("which security", { stdio: "pipe" }); hasKeychain = true; } catch {}

          const options: string[] = [];

          if (hasKeychain) {
            options.push("macOS Keychain (recommended)");
          }
          if (hasOp) {
            options.push("1Password — read via op CLI");
          }
          options.push(
            `Environment variable — reads $${secretName} at runtime`,
            "Shell command — custom command (stdout = secret value)",
          );
          if (!hasKeychain) {
            options.push("Paste value — enter the value now (⚠️ stored in plaintext)");
          }
          if (currentRecipe) {
            options.push("Remove this secret's recipe");
          }

          const statusLine = currentRecipe
            ? `Current: ${currentRecipe.startsWith("literal:") ? "literal (hidden)" : currentRecipe}`
            : "Not configured";

          const choice = await ctx.ui.select(
            `Configure: ${secretName}\n${desc}\n${statusLine}\n\nChoose how to resolve this secret:`,
            options
          );

          if (!choice) return;

          if (choice.startsWith("macOS Keychain")) {
            // Unified Keychain flow: try to read existing → if missing, prompt and store → verify
            const service = KEYCHAIN_SERVICE_PREFIX + secretName;

            // 1. Try to read from keychain first
            let existing: string | undefined;
            try {
              existing = execFileSync("security", [
                "find-generic-password", "-a", KEYCHAIN_ACCOUNT, "-ws", service,
              ], { encoding: "utf-8", timeout: 10_000, stdio: ["pipe", "pipe", "pipe"] }).trim();
            } catch {
              // Not found — that's fine, we'll create it
            }

            if (existing) {
              // Found it — confirm use or replace
              const masked = existing.length > 8
                ? existing.slice(0, 4) + "•".repeat(Math.min(existing.length - 4, 16)) + ` (${existing.length} chars)`
                : "•".repeat(existing.length) + ` (${existing.length} chars)`;

              const action = await ctx.ui.select(
                `Found existing Keychain entry for "${service}":\n  ${masked}\n\nUse this value?`,
                ["Yes, use it", "Replace with a new value", "Cancel"]
              );
              if (!action || action === "Cancel") return;

              if (action === "Replace with a new value") {
                const val = await promptForSecretValue(ctx, secretName, `Provide the new value for ${secretName}:`);
                if (!val) return;
                try {
                  storeInKeychain(secretName, val);
                } catch (e: any) {
                  ctx.ui.notify(`❌ Failed to store in Keychain: ${e.message}`, "error");
                  return;
                }
              }
            } else {
              // Not in keychain — prompt for value and store it
              const val = await promptForSecretValue(
                ctx,
                secretName,
                `No Keychain entry found for "${service}".\n\n` +
                `Provide the value for ${secretName} — it will be stored in your login keychain\n` +
                `(protected by Touch ID / password).`
              );
              if (!val) return;

              try {
                storeInKeychain(secretName, val);
              } catch (e: any) {
                ctx.ui.notify(`❌ Failed to store in Keychain: ${e.message}`, "error");
                return;
              }
            }

            // Set recipe to read from keychain
            recipes[secretName] = keychainRecipe(secretName);

          } else if (choice.startsWith("1Password")) {
            const ref = await ctx.ui.input(
              `Enter the 1Password item reference for ${secretName}:\n\n` +
              `Format: op://vault/item/field\n` +
              `Example: op://Private/API Keys/brave-search`
            );
            if (!ref) return;
            recipes[secretName] = ref.startsWith("op://") ? `!op read "${ref}"` : `!op read "op://${ref}"`;
          } else if (choice.startsWith("Environment variable")) {
            recipes[secretName] = secretName;
          } else if (choice.startsWith("Shell command")) {
            const cmd = await ctx.ui.input(
              `Enter shell command for ${secretName}:\n\n` +
              `The command's stdout will be used as the secret value.\n` +
              `Examples:\n` +
              `  security find-generic-password -ws 'service-name'\n` +
              `  op read "op://vault/item/field"\n` +
              `  cat ~/.config/some-tool/token`
            );
            if (!cmd) return;
            recipes[secretName] = cmd.startsWith("!") ? cmd : `!${cmd}`;
          } else if (choice.startsWith("Paste value")) {
            const val = await promptForSecretValue(
              ctx,
              secretName,
              `⚠️  This will be stored in plaintext in ~/.pi/agent/secrets.json.\n` +
              `Consider using Keychain instead.`
            );
            if (!val) return;
            recipes[secretName] = `literal:${val}`;
          } else if (choice.startsWith("Remove")) {
            delete recipes[secretName];
            resolvedCache.delete(secretName);
            saveRecipes(recipes);
            ctx.ui.notify(`Removed recipe for ${secretName}`, "info");
            return;
          }

          saveRecipes(recipes);

          // Verify it actually resolves — this is the moment of truth
          resolvedCache.delete(secretName);
          const value = resolveSecret(secretName);
          if (value) {
            process.env[secretName] = value;
            const masked = value.length > 8
              ? value.slice(0, 4) + "•".repeat(Math.min(value.length - 4, 16)) + ` (${value.length} chars)`
              : "•".repeat(value.length) + ` (${value.length} chars)`;
            ctx.ui.notify(`✅ ${secretName} configured and verified: ${masked}`, "info");
          } else {
            // Don't just warn — this is a failure. Remove the broken recipe.
            delete recipes[secretName];
            saveRecipes(recipes);
            ctx.ui.notify(
              `❌ ${secretName} failed to resolve after configuration. Recipe removed.\n` +
              `Try again with /secrets configure ${secretName}`,
              "error"
            );
          }
          break;
        }

        case "rm":
        case "remove":
        case "delete": {
          if (!secretName) {
            ctx.ui.notify("Usage: /secrets rm <NAME>", "error");
            return;
          }
          if (recipes[secretName]) {
            delete recipes[secretName];
            resolvedCache.delete(secretName);
            saveRecipes(recipes);
            ctx.ui.notify(`Removed recipe for ${secretName}`, "info");
          } else {
            ctx.ui.notify(`No recipe found for ${secretName}`, "error");
          }
          break;
        }

        case "test": {
          if (!secretName) {
            ctx.ui.notify("Usage: /secrets test <NAME>", "error");
            return;
          }
          const recipe = recipes[secretName];
          if (!recipe && !process.env[secretName]) {
            ctx.ui.notify(`No recipe or env var found for ${secretName}`, "error");
            return;
          }

          // Re-resolve (bypass cache)
          resolvedCache.delete(secretName);
          const value = resolveSecret(secretName);
          if (value) {
            // Show masked value: first 4 chars + masked rest
            const masked =
              value.length > 8
                ? value.slice(0, 4) + "•".repeat(Math.min(value.length - 4, 20)) + ` (${value.length} chars)`
                : "•".repeat(value.length) + ` (${value.length} chars)`;
            ctx.ui.notify(`✅ ${secretName} resolved: ${masked}`, "info");
          } else {
            const source = recipe || `env:${secretName}`;
            ctx.ui.notify(`❌ ${secretName} failed to resolve from: ${source}`, "error");
          }
          break;
        }

        default:
          ctx.ui.notify(
            "Usage: /secrets <list|configure|rm|test> [name]\n\n" +
              "  /secrets list              — show all configured secrets\n" +
              "  /secrets configure <NAME>  — set up resolution for a secret\n" +
              "  /secrets rm <NAME>         — remove a secret recipe\n" +
              "  /secrets test <NAME>       — test if a secret resolves",
            "info"
          );
      }
    },
  });
}

// ============================================================================
// Exports for testing
// ============================================================================

export {
  matchSensitivePath,
  isSecretAccessCommand,
  redactString,
  MIN_REDACT_LENGTH,
  SENSITIVE_PATH_PATTERNS,
};

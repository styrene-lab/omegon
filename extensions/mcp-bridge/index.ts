// @secret GITHUB_TOKEN "GitHub personal access token for MCP server auth (Scribe, etc.)"

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import * as fs from "node:fs";
import * as path from "node:path";
import { homedir } from "node:os";
import {
  type ServerConfig,
  type HttpServerConfig,
  type StdioServerConfig,
  type ConfigSource,
  type SourcedConfig,
  isHttpConfig,
  resolveEnvVars,
  resolveEnvObj,
  isAuthError,
  isTransportError,
  extractText,
  loadMergedConfig,
  configFileForScope,
  slugifyUrl,
  buildHttpConfig,
  buildStdioConfig,
  parseCommand,
  extractSecretRefs,
  AUTH_REMEDIATION,
} from "./lib.ts";

// ---------------------------------------------------------------------------
// Runtime types
// ---------------------------------------------------------------------------

interface ConnectedServer {
  name: string;
  client: Client;
  transport: StdioClientTransport | StreamableHTTPClientTransport;
  config: ServerConfig;
  tools: Array<{ name: string; description?: string; inputSchema: any }>;
}

const DEFAULT_CONNECT_TIMEOUT_MS = 15_000;
const USER_DIR = path.join(homedir(), ".pi", "agent");
const EXTENSION_DIR = import.meta.dirname;

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default async function (pi: ExtensionAPI) {
  const servers: Record<string, ConnectedServer> = {};

  // Track connection outcomes for session_start notification
  const connectionErrors: Array<{ name: string; message: string }> = [];
  let totalTools = 0;

  // Resolved config with source tracking
  let configResult: SourcedConfig;

  // In-flight reconnect promises, keyed by server name. Prevents concurrent
  // reconnect attempts from racing and leaking duplicate connections.
  const reconnecting = new Map<string, Promise<ConnectedServer | null>>();

  // ── Timeout helper ──────────────────────────────────────────────────────

  /**
   * Race a promise against a deadline. On timeout, attempts to close the
   * transport to avoid leaking child processes or HTTP connections.
   */
  function withTimeout(
    promise: Promise<ConnectedServer>,
    ms: number,
    label: string
  ): Promise<ConnectedServer> {
    let settled = false;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!settled) {
          settled = true;
          reject(new Error(`[mcp-bridge] ${label}: timed out after ${ms}ms`));
          // Best-effort cleanup: the inner promise may still resolve with a
          // ConnectedServer whose transport is alive. Close it.
          promise.then(
            (s) => { try { s.transport.close(); } catch {} },
            () => {} // inner already failed, nothing to clean up
          );
        }
      }, ms);
      promise.then(
        (v) => { if (!settled) { settled = true; clearTimeout(timer); resolve(v); } },
        (e) => { if (!settled) { settled = true; clearTimeout(timer); reject(e); } }
      );
    });
  }

  // ── Server connection ───────────────────────────────────────────────────

  async function connectStdioServer(
    name: string,
    config: StdioServerConfig
  ): Promise<ConnectedServer> {
    const resolvedEnv = config.env ? resolveEnvObj(config.env) : {};

    const transport = new StdioClientTransport({
      command: config.command,
      args: config.args ?? [],
      env: { ...process.env, ...resolvedEnv } as Record<string, string>,
    });

    const client = new Client({
      name: `pi-mcp-bridge/${name}`,
      version: "1.0.0",
    });
    await client.connect(transport);
    const { tools } = await client.listTools();

    return { name, client, transport, config, tools };
  }

  async function connectHttpServer(
    name: string,
    config: HttpServerConfig
  ): Promise<ConnectedServer> {
    const resolvedHeaders = config.headers
      ? resolveEnvObj(config.headers)
      : {};

    const transport = new StreamableHTTPClientTransport(
      new URL(resolveEnvVars(config.url)),
      {
        requestInit: {
          headers: resolvedHeaders,
        },
      }
    );

    const client = new Client({
      name: `pi-mcp-bridge/${name}`,
      version: "1.0.0",
    });
    await client.connect(transport);
    const { tools } = await client.listTools();

    return { name, client, transport, config, tools };
  }

  async function connectServer(
    name: string,
    config: ServerConfig
  ): Promise<ConnectedServer> {
    const timeoutMs = isHttpConfig(config)
      ? config.timeout ?? DEFAULT_CONNECT_TIMEOUT_MS
      : DEFAULT_CONNECT_TIMEOUT_MS;

    const inner = isHttpConfig(config)
      ? connectHttpServer(name, config)
      : connectStdioServer(name, config as StdioServerConfig);

    return withTimeout(inner, timeoutMs, name);
  }

  // ── Reconnection ───────────────────────────────────────────────────────

  /**
   * Reconnect a server, deduplicating concurrent attempts. If a reconnect
   * is already in flight for this server, returns the existing promise.
   */
  function reconnectServer(
    name: string,
    config: ServerConfig
  ): Promise<ConnectedServer | null> {
    const inflight = reconnecting.get(name);
    if (inflight) return inflight;

    const attempt = (async (): Promise<ConnectedServer | null> => {
      // Tear down old connection
      const old = servers[name];
      if (old) {
        try { await old.client.close(); } catch {}
        delete servers[name];
      }

      try {
        const fresh = await connectServer(name, config);
        servers[name] = fresh;
        return fresh;
      } catch (err: any) {
        console.error(`[mcp-bridge] Reconnect failed for ${name}: ${err.message}`);
        return null;
      }
    })();

    // Clear the mutex when done regardless of outcome
    attempt.finally(() => reconnecting.delete(name));
    reconnecting.set(name, attempt);

    return attempt;
  }

  // ── Tool registration ──────────────────────────────────────────────────

  function jsonSchemaToTypebox(schema: any): any {
    if (!schema || typeof schema !== "object") return Type.Object({});
    return Type.Unsafe(schema);
  }

  function registerToolsForServer(server: ConnectedServer): number {
    const serverName = server.name;
    const serverConfig = server.config;
    let count = 0;

    for (const tool of server.tools) {
      const toolName = tool.name;
      const piToolName = `mcp_${serverName}_${toolName}`;

      pi.registerTool({
        name: piToolName,
        label: `${serverName}/${toolName}`,
        description: tool.description ?? `MCP tool from ${serverName}`,
        parameters: jsonSchemaToTypebox(tool.inputSchema),

        async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
          // Always read current server — may have been replaced by reconnect
          const current = servers[serverName];
          if (!current) {
            return {
              content: [{ type: "text", text: `Error: server ${serverName} is not connected` }],
              details: { server: serverName, tool: toolName, error: true },
            };
          }

          try {
            const result = await current.client.callTool({
              name: toolName,
              arguments: params,
            });

            return {
              content: [{ type: "text", text: extractText(result) }],
              details: { server: serverName, tool: toolName },
            };
          } catch (err: any) {
            // Auth errors — reconnecting won't help
            if (isAuthError(err)) {
              return {
                content: [
                  {
                    type: "text",
                    text: `[mcp-bridge] ${serverName}: authentication failed.\n${AUTH_REMEDIATION}`,
                  },
                ],
                details: { server: serverName, tool: toolName, error: true, auth: true },
              };
            }

            // Transport errors — attempt one reconnect + retry
            if (isTransportError(err)) {
              const reconnected = await reconnectServer(serverName, serverConfig);
              if (reconnected) {
                try {
                  const retry = await reconnected.client.callTool({
                    name: toolName,
                    arguments: params,
                  });
                  return {
                    content: [{ type: "text", text: extractText(retry) }],
                    details: { server: serverName, tool: toolName, reconnected: true },
                  };
                } catch (retryErr: any) {
                  const msg = isAuthError(retryErr)
                    ? `[mcp-bridge] ${serverName}: authentication failed.\n${AUTH_REMEDIATION}`
                    : `Error after reconnect: ${retryErr.message}`;
                  return {
                    content: [{ type: "text", text: msg }],
                    details: {
                      server: serverName,
                      tool: toolName,
                      error: true,
                      ...(isAuthError(retryErr) && { auth: true }),
                    },
                  };
                }
              }
            }

            return {
              content: [{ type: "text", text: `Error: ${err.message}` }],
              details: { server: serverName, tool: toolName, error: true },
            };
          }
        },
      });

      count++;
    }
    return count;
  }

  // ── Config file helpers ─────────────────────────────────────────────────

  /**
   * Read an mcp.json file, returning its parsed content or a fresh skeleton.
   */
  function readConfigFile(filePath: string): any {
    if (fs.existsSync(filePath)) {
      try {
        return JSON.parse(fs.readFileSync(filePath, "utf-8"));
      } catch {
        return { servers: {} };
      }
    }
    return { servers: {} };
  }

  /**
   * Write a server entry to the appropriate mcp.json.
   */
  function writeServerToConfig(
    scope: "project" | "user",
    serverName: string,
    config: ServerConfig,
  ): string | null {
    const projectDir = process.cwd();
    const filePath = configFileForScope(scope, projectDir, USER_DIR);
    if (!filePath) return null;

    // Ensure parent directory exists
    const dir = path.dirname(filePath);
    fs.mkdirSync(dir, { recursive: true });

    const raw = readConfigFile(filePath);
    raw.servers[serverName] = config;
    fs.writeFileSync(filePath, JSON.stringify(raw, null, 2) + "\n");
    return filePath;
  }

  /**
   * Remove a server entry from the appropriate mcp.json.
   */
  function removeServerFromConfig(
    scope: "project" | "user",
    serverName: string,
  ): boolean {
    const projectDir = process.cwd();
    const filePath = configFileForScope(scope, projectDir, USER_DIR);
    if (!filePath || !fs.existsSync(filePath)) return false;

    const raw = readConfigFile(filePath);
    if (!raw.servers || !(serverName in raw.servers)) return false;

    delete raw.servers[serverName];
    fs.writeFileSync(filePath, JSON.stringify(raw, null, 2) + "\n");
    return true;
  }

  // ── Connect and register tools during factory (before tool snapshot) ───

  const projectDir = process.cwd();
  configResult = loadMergedConfig(projectDir, USER_DIR, EXTENSION_DIR);

  for (const err of configResult.errors) {
    connectionErrors.push({ name: err.server, message: err.message });
  }

  const entries = Object.entries(configResult.servers);
  if (entries.length > 0) {
    const results = await Promise.allSettled(
      entries.map(([name, serverConfig]) => connectServer(name, serverConfig))
    );

    for (let i = 0; i < entries.length; i++) {
      const [name] = entries[i];
      const result = results[i];

      if (result.status === "rejected") {
        const reason = result.reason;
        connectionErrors.push({
          name,
          message: isAuthError(reason)
            ? `authentication failed.\n${AUTH_REMEDIATION}`
            : reason?.message ?? String(reason),
        });
        continue;
      }

      const connected = result.value;
      servers[name] = connected;
      totalTools += registerToolsForServer(connected);
    }
  }

  // ── Lifecycle ──────────────────────────────────────────────────────────

  pi.on("session_start", async (_event, ctx) => {
    // Report connection outcomes (connections already established in factory)
    for (const err of connectionErrors) {
      ctx.ui.notify(`[mcp-bridge] ${err.name}: ${err.message}`, "error");
    }

    if (totalTools > 0) {
      ctx.ui.notify(
        `[mcp-bridge] ${totalTools} tools from ${Object.keys(servers).length} server(s)`,
        "info"
      );
    } else if (connectionErrors.length === 0 && entries.length === 0) {
      // No servers configured anywhere — that's fine, just silent
    }
  });

  pi.on("session_shutdown", async () => {
    await Promise.allSettled(
      Object.values(servers).map((s) => s.client.close())
    );
  });

  // ── Commands ───────────────────────────────────────────────────────────

  pi.registerCommand("mcp", {
    description: "Manage MCP servers: list, add, remove, test, reconnect",
    getArgumentCompletions: (prefix: string) => {
      const parts = prefix.split(/\s+/);
      if (parts.length <= 1) {
        const subs = ["list", "add", "remove", "test", "reconnect"];
        const filtered = subs.filter(s => s.startsWith(parts[0] || ""));
        return filtered.length > 0 ? filtered.map(s => ({ value: s, label: s })) : null;
      }
      const sub = parts[0];
      if (sub === "remove" || sub === "test" || sub === "reconnect") {
        const namePrefix = parts.slice(1).join(" ");
        const allNames = [
          ...Object.keys(servers),
          ...Object.keys(configResult.servers).filter(k => !(k in servers)),
        ];
        const filtered = allNames.filter(n => n.startsWith(namePrefix));
        return filtered.length > 0
          ? filtered.map(n => ({ value: `${sub} ${n}`, label: n }))
          : null;
      }
      return null;
    },
    handler: async (args, ctx) => {
      const parts = (args || "").trim().split(/\s+/);
      const subcommand = parts[0] || "list";
      const serverName = parts.slice(1).join(" ");

      switch (subcommand) {

        // ── /mcp list ──────────────────────────────────────────────────
        case "list": {
          // Reload config to show current state
          const currentConfig = loadMergedConfig(process.cwd(), USER_DIR, EXTENSION_DIR);
          const allServerNames = new Set([
            ...Object.keys(currentConfig.servers),
            ...Object.keys(servers),
          ]);

          if (allServerNames.size === 0) {
            ctx.ui.notify(
              "No MCP servers configured.\n\n" +
              "Run /mcp add to connect a server, or create:\n" +
              `  ~/.pi/agent/mcp.json          (user-level)\n` +
              `  .pi/mcp.json                  (project-level)`,
              "info"
            );
            return;
          }

          const lines: string[] = ["MCP Servers:", ""];

          for (const name of allServerNames) {
            const connected = servers[name];
            const config = currentConfig.servers[name];
            const source = currentConfig.sources[name] || "unknown";

            const status = connected ? "✅ connected" : "❌ disconnected";
            const toolCount = connected ? `${connected.tools.length} tools` : "—";
            const transport = config
              ? isHttpConfig(config)
                ? config.url
                : `stdio: ${(config as StdioServerConfig).command}`
              : "config missing";

            lines.push(`  ${status}  ${name}  (${toolCount})`);
            lines.push(`     Transport: ${transport}`);
            lines.push(`     Source: ${source}`);

            // Show secret status for referenced secrets
            if (config) {
              const refs = extractSecretRefs(config);
              if (refs.length > 0) {
                const secretStatus = refs.map(ref => {
                  const resolved = !!process.env[ref];
                  return `${resolved ? "✅" : "❌"} ${ref}`;
                }).join(", ");
                lines.push(`     Secrets: ${secretStatus}`);
              }
            }
            lines.push("");
          }

          lines.push("Commands: /mcp add | remove <name> | test <name> | reconnect <name>");
          ctx.ui.notify(lines.join("\n"), "info");
          break;
        }

        // ── /mcp add ──────────────────────────────────────────────────
        case "add": {
          if (!ctx.hasUI) {
            ctx.ui.notify("Cannot add servers without interactive UI", "error");
            return;
          }

          // Step 1: Transport type
          const transport = await ctx.ui.select(
            "Add MCP Server\n\nSelect transport type:",
            [
              "HTTP — remote server (Streamable HTTP)",
              "Stdio — local process (stdin/stdout)",
            ]
          );
          if (!transport) return;

          if (transport.startsWith("HTTP")) {
            // ── HTTP flow ──────────────────────────────────────────

            // Step 2: URL
            const url = await ctx.ui.input(
              "Enter the MCP server URL:\n\n" +
              "This is the Streamable HTTP endpoint, e.g.:\n" +
              "  https://scribe.recrocog.com/mcp/transport/"
            );
            if (!url) return;

            // Validate URL
            try {
              new URL(url);
            } catch {
              ctx.ui.notify(`❌ Invalid URL: ${url}`, "error");
              return;
            }

            // Step 3: Server name
            const suggestedName = slugifyUrl(url);
            const name = await ctx.ui.input(
              `Server name (used as prefix for tools, e.g. mcp_${suggestedName}_*):\n\n` +
              `Leave blank for "${suggestedName}"`,
            );
            const finalName = (name || suggestedName).toLowerCase().replace(/[^a-z0-9_-]/g, "-");

            // Check for collision
            if (configResult.servers[finalName]) {
              const overwrite = await ctx.ui.confirm(
                "Server exists",
                `"${finalName}" is already configured (source: ${configResult.sources[finalName]}). Replace it?`
              );
              if (!overwrite) return;
            }

            // Step 4: Authentication
            const authChoice = await ctx.ui.select(
              `Authentication for ${finalName}:`,
              [
                "Bearer token — Authorization: Bearer $SECRET",
                "API key header — custom header with secret",
                "No authentication",
              ]
            );
            if (!authChoice) return;

            let authType: "bearer" | "api-key" | "none";
            let secretName: string | undefined;
            let headerName: string | undefined;

            if (authChoice.startsWith("Bearer")) {
              authType = "bearer";
              secretName = await ctx.ui.input(
                "Secret name for the Bearer token:\n\n" +
                "This is the environment variable / secret recipe name.\n" +
                "Example: GITHUB_TOKEN, SCRIBE_API_KEY"
              );
              if (!secretName) return;
              secretName = secretName.trim().toUpperCase().replace(/[^A-Z0-9_]/g, "_");
            } else if (authChoice.startsWith("API key")) {
              authType = "api-key";
              headerName = await ctx.ui.input("Header name (e.g. X-Api-Key):");
              if (!headerName) return;
              secretName = await ctx.ui.input(
                `Secret name for ${headerName}:\n\n` +
                "Environment variable / secret recipe name."
              );
              if (!secretName) return;
              secretName = secretName.trim().toUpperCase().replace(/[^A-Z0-9_]/g, "_");
            } else {
              authType = "none";
            }

            // Step 5: Scope
            const scope = await ctx.ui.select(
              "Where should this server config be saved?",
              [
                "User-level (~/.pi/agent/mcp.json) — available in all projects",
                "Project-level (.pi/mcp.json) — only this project",
              ]
            );
            if (!scope) return;
            const configScope: "user" | "project" = scope.startsWith("User") ? "user" : "project";

            // Build and write config
            const serverConfig = buildHttpConfig(url, authType, secretName, headerName);
            const writtenTo = writeServerToConfig(configScope, finalName, serverConfig);

            if (!writtenTo) {
              ctx.ui.notify("❌ Failed to write config", "error");
              return;
            }

            // Check secret availability
            let secretWarning = "";
            if (secretName && !process.env[secretName]) {
              secretWarning = `\n\n⚠️  Secret "${secretName}" is not configured.\n` +
                `Run /secrets configure ${secretName} to set it up.`;
            }

            // Offer reload
            ctx.ui.notify(
              `✅ Server "${finalName}" added to ${configScope} config.\n` +
              `   Written to: ${writtenTo}${secretWarning}`,
              "info"
            );

            const reload = await ctx.ui.confirm(
              "Reload required",
              "Reload pi to connect and register tools from the new server?"
            );
            if (reload) {
              await ctx.reload();
            }

          } else {
            // ── Stdio flow ─────────────────────────────────────────

            // Step 2: Command
            const commandInput = await ctx.ui.input(
              "Enter the command to start the MCP server:\n\n" +
              "Examples:\n" +
              "  npx -y @example/mcp-server\n" +
              "  python -m my_mcp_server\n" +
              "  /usr/local/bin/my-tool serve"
            );
            if (!commandInput) return;

            const { command, args: cmdArgs } = parseCommand(commandInput);
            if (!command) {
              ctx.ui.notify("❌ No command provided", "error");
              return;
            }

            // Step 3: Server name
            const suggestedName = path.basename(command).replace(/\.[^.]+$/, "");
            const name = await ctx.ui.input(
              `Server name (used as prefix for tools, e.g. mcp_${suggestedName}_*):\n\n` +
              `Leave blank for "${suggestedName}"`,
            );
            const finalName = (name || suggestedName).toLowerCase().replace(/[^a-z0-9_-]/g, "-");

            // Check for collision
            if (configResult.servers[finalName]) {
              const overwrite = await ctx.ui.confirm(
                "Server exists",
                `"${finalName}" is already configured (source: ${configResult.sources[finalName]}). Replace it?`
              );
              if (!overwrite) return;
            }

            // Step 4: Environment variables (optional)
            let env: Record<string, string> | undefined;
            const addEnv = await ctx.ui.confirm(
              "Environment variables",
              "Does this server need environment variables (e.g. API keys)?"
            );
            if (addEnv) {
              env = {};
              let more = true;
              while (more) {
                const pair = await ctx.ui.input(
                  "Enter environment variable (KEY=value or KEY=${SECRET_NAME}):\n\n" +
                  "Use ${SECRET_NAME} to reference a managed secret."
                );
                if (pair) {
                  const eqIdx = pair.indexOf("=");
                  if (eqIdx > 0) {
                    env[pair.slice(0, eqIdx).trim()] = pair.slice(eqIdx + 1).trim();
                  }
                }
                more = await ctx.ui.confirm("More?", "Add another environment variable?");
              }
            }

            // Step 5: Scope
            const scope = await ctx.ui.select(
              "Where should this server config be saved?",
              [
                "User-level (~/.pi/agent/mcp.json) — available in all projects",
                "Project-level (.pi/mcp.json) — only this project",
              ]
            );
            if (!scope) return;
            const configScope: "user" | "project" = scope.startsWith("User") ? "user" : "project";

            // Build and write config
            const serverConfig = buildStdioConfig(command, cmdArgs, env);
            const writtenTo = writeServerToConfig(configScope, finalName, serverConfig);

            if (!writtenTo) {
              ctx.ui.notify("❌ Failed to write config", "error");
              return;
            }

            // Check secret availability for env refs
            const refs = extractSecretRefs(serverConfig);
            let secretWarning = "";
            const missing = refs.filter(r => !process.env[r]);
            if (missing.length > 0) {
              secretWarning = `\n\n⚠️  Missing secrets: ${missing.join(", ")}\n` +
                missing.map(s => `Run /secrets configure ${s}`).join("\n");
            }

            ctx.ui.notify(
              `✅ Server "${finalName}" added to ${configScope} config.\n` +
              `   Written to: ${writtenTo}${secretWarning}`,
              "info"
            );

            const reload = await ctx.ui.confirm(
              "Reload required",
              "Reload pi to connect and register tools from the new server?"
            );
            if (reload) {
              await ctx.reload();
            }
          }
          break;
        }

        // ── /mcp remove <name> ────────────────────────────────────────
        case "remove":
        case "rm": {
          if (!serverName) {
            // Interactive: let user pick
            if (!ctx.hasUI) {
              ctx.ui.notify("Usage: /mcp remove <server-name>", "error");
              return;
            }
            const currentConfig = loadMergedConfig(process.cwd(), USER_DIR, EXTENSION_DIR);
            const removable = Object.entries(currentConfig.sources)
              .filter(([_, source]) => source !== "bundled")
              .map(([name, source]) => `${name}  (${source})`);

            if (removable.length === 0) {
              ctx.ui.notify("No removable servers. Bundled servers can only be overridden, not removed.", "info");
              return;
            }

            const choice = await ctx.ui.select("Remove which server?", removable);
            if (!choice) return;

            const chosenName = choice.split(/\s+/)[0];
            const chosenSource = currentConfig.sources[chosenName] as "project" | "user";

            const confirm = await ctx.ui.confirm(
              "Confirm removal",
              `Remove "${chosenName}" from ${chosenSource} config?`
            );
            if (!confirm) return;

            if (removeServerFromConfig(chosenSource, chosenName)) {
              ctx.ui.notify(`✅ Removed "${chosenName}" from ${chosenSource} config.`, "info");
              const reload = await ctx.ui.confirm("Reload?", "Reload pi to apply changes?");
              if (reload) await ctx.reload();
            } else {
              ctx.ui.notify(`❌ Failed to remove "${chosenName}"`, "error");
            }
            return;
          }

          // Named removal
          const currentConfig = loadMergedConfig(process.cwd(), USER_DIR, EXTENSION_DIR);
          const source = currentConfig.sources[serverName];
          if (!source) {
            ctx.ui.notify(`❌ No server named "${serverName}" found`, "error");
            return;
          }
          if (source === "bundled") {
            ctx.ui.notify(
              `❌ "${serverName}" is a bundled server. You can override it with /mcp add, but not remove it.`,
              "error"
            );
            return;
          }

          if (ctx.hasUI) {
            const confirm = await ctx.ui.confirm(
              "Confirm removal",
              `Remove "${serverName}" from ${source} config?`
            );
            if (!confirm) return;
          }

          if (removeServerFromConfig(source as "project" | "user", serverName)) {
            ctx.ui.notify(`✅ Removed "${serverName}" from ${source} config.`, "info");
            if (ctx.hasUI) {
              const reload = await ctx.ui.confirm("Reload?", "Reload pi to apply changes?");
              if (reload) await ctx.reload();
            }
          } else {
            ctx.ui.notify(`❌ Failed to remove "${serverName}"`, "error");
          }
          break;
        }

        // ── /mcp test <name> ──────────────────────────────────────────
        case "test": {
          if (!serverName) {
            // Interactive: let user pick
            if (!ctx.hasUI) {
              ctx.ui.notify("Usage: /mcp test <server-name>", "error");
              return;
            }
            const currentConfig = loadMergedConfig(process.cwd(), USER_DIR, EXTENSION_DIR);
            const names = Object.keys(currentConfig.servers);
            if (names.length === 0) {
              ctx.ui.notify("No servers configured to test.", "info");
              return;
            }
            const choice = await ctx.ui.select("Test which server?", names);
            if (!choice) return;
            // Recurse with the chosen name
            await handleTest(choice, ctx);
            return;
          }
          await handleTest(serverName, ctx);
          break;
        }

        // ── /mcp reconnect <name> ────────────────────────────────────
        case "reconnect": {
          if (!serverName) {
            if (!ctx.hasUI) {
              ctx.ui.notify("Usage: /mcp reconnect <server-name>", "error");
              return;
            }
            const connectedNames = Object.keys(servers);
            const allNames = Object.keys(configResult.servers);
            const names = [...new Set([...connectedNames, ...allNames])];
            if (names.length === 0) {
              ctx.ui.notify("No servers to reconnect.", "info");
              return;
            }
            const choice = await ctx.ui.select("Reconnect which server?", names);
            if (!choice) return;
            await handleReconnect(choice, ctx);
            return;
          }
          await handleReconnect(serverName, ctx);
          break;
        }

        // ── /mcp (no args or unknown) ────────────────────────────────
        default: {
          ctx.ui.notify(
            "Usage: /mcp <list|add|remove|test|reconnect> [name]\n\n" +
            "  /mcp list                — show all configured servers and status\n" +
            "  /mcp add                 — guided setup for a new server\n" +
            "  /mcp remove [name]       — remove a server from config\n" +
            "  /mcp test [name]         — test connection to a server\n" +
            "  /mcp reconnect [name]    — reconnect a server",
            "info"
          );
        }
      }
    },
  });

  // ── Command helpers (avoid deep nesting) ────────────────────────────────

  async function handleTest(name: string, ctx: any) {
    const currentConfig = loadMergedConfig(process.cwd(), USER_DIR, EXTENSION_DIR);
    const config = currentConfig.servers[name];
    if (!config) {
      ctx.ui.notify(`❌ No server named "${name}" in config`, "error");
      return;
    }

    // Check secret availability first
    const refs = extractSecretRefs(config);
    const missing = refs.filter(r => !process.env[r]);
    if (missing.length > 0) {
      ctx.ui.notify(
        `❌ Cannot test "${name}" — missing secrets: ${missing.join(", ")}\n` +
        missing.map((s: string) => `  Run /secrets configure ${s}`).join("\n"),
        "error"
      );
      return;
    }

    ctx.ui.notify(`Testing connection to "${name}"...`, "info");

    try {
      const connected = await connectServer(name, config);
      const toolCount = connected.tools.length;
      const toolNames = connected.tools.slice(0, 5).map((t: any) => t.name).join(", ");
      const suffix = toolCount > 5 ? `, ... (+${toolCount - 5} more)` : "";

      // Clean up test connection
      try { await connected.client.close(); } catch {}

      ctx.ui.notify(
        `✅ "${name}" connected successfully!\n` +
        `   ${toolCount} tools: ${toolNames}${suffix}`,
        "info"
      );
    } catch (err: any) {
      const msg = isAuthError(err)
        ? `Authentication failed.\n${AUTH_REMEDIATION}`
        : err.message;
      ctx.ui.notify(`❌ "${name}" connection failed:\n   ${msg}`, "error");
    }
  }

  async function handleReconnect(name: string, ctx: any) {
    const config = configResult.servers[name];
    if (!config) {
      ctx.ui.notify(`❌ No server named "${name}" in config`, "error");
      return;
    }

    ctx.ui.notify(`Reconnecting "${name}"...`, "info");
    const result = await reconnectServer(name, config);
    if (result) {
      ctx.ui.notify(
        `✅ "${name}" reconnected (${result.tools.length} tools)`,
        "info"
      );
    } else {
      ctx.ui.notify(`❌ "${name}" reconnection failed`, "error");
    }
  }
}

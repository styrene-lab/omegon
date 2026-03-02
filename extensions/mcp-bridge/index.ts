// @secret GITHUB_TOKEN "GitHub personal access token for MCP server auth (Scribe, etc.)"

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import * as fs from "node:fs";
import * as path from "node:path";

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

interface StdioServerConfig {
  command: string;
  args?: string[];
  env?: Record<string, string>;
}

interface HttpServerConfig {
  url: string;
  headers?: Record<string, string>;
  /** Connection timeout in ms (default: 15000) */
  timeout?: number;
}

type ServerConfig = StdioServerConfig | HttpServerConfig;

function isHttpConfig(config: ServerConfig): config is HttpServerConfig {
  return "url" in config;
}

interface McpConfig {
  servers: Record<string, ServerConfig>;
}

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

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function (pi: ExtensionAPI) {
  const servers: Record<string, ConnectedServer> = {};
  const configPath = path.join(
    path.dirname(new URL(import.meta.url).pathname),
    "mcp.json"
  );

  // ── Env var resolution ──────────────────────────────────────────────────

  function resolveEnvVars(value: string): string {
    return value.replace(/\$\{(\w+)\}/g, (_, key) => process.env[key] ?? "");
  }

  function resolveEnvObj(
    obj: Record<string, string>
  ): Record<string, string> {
    const resolved: Record<string, string> = {};
    for (const [k, v] of Object.entries(obj)) {
      resolved[k] = resolveEnvVars(v);
    }
    return resolved;
  }

  // ── Timeout helper ──────────────────────────────────────────────────────

  function withTimeout<T>(
    promise: Promise<T>,
    ms: number,
    label: string
  ): Promise<T> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`[mcp-bridge] ${label}: timed out after ${ms}ms`)),
        ms
      );
      promise.then(
        (v) => { clearTimeout(timer); resolve(v); },
        (e) => { clearTimeout(timer); reject(e); }
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

  async function reconnectServer(
    name: string,
    config: ServerConfig
  ): Promise<ConnectedServer | null> {
    // Tear down old connection if present
    const old = servers[name];
    if (old) {
      try { await old.client.close(); } catch {}
      delete servers[name];
    }

    try {
      return await connectServer(name, config);
    } catch (err: any) {
      console.error(`[mcp-bridge] Reconnect failed for ${name}: ${err.message}`);
      return null;
    }
  }

  // ── Tool registration ──────────────────────────────────────────────────

  function jsonSchemaToTypebox(schema: any): any {
    if (!schema || typeof schema !== "object") return Type.Object({});
    return Type.Unsafe(schema);
  }

  function registerToolsForServer(server: ConnectedServer): number {
    let count = 0;
    for (const tool of server.tools) {
      const piToolName = `mcp_${server.name}_${tool.name}`;

      pi.registerTool({
        name: piToolName,
        label: `${server.name}/${tool.name}`,
        description: tool.description ?? `MCP tool from ${server.name}`,
        parameters: jsonSchemaToTypebox(tool.inputSchema),

        async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
          try {
            const result = await server.client.callTool({
              name: tool.name,
              arguments: params,
            });

            const textParts = (result.content as any[])
              .filter((c: any) => c.type === "text")
              .map((c: any) => c.text)
              .join("\n");

            return {
              content: [
                { type: "text", text: textParts || "(empty response)" },
              ],
              details: { server: server.name, tool: tool.name },
            };
          } catch (err: any) {
            // Attempt one reconnect on transport-level errors
            const isTransportError =
              err.message?.includes("not connected") ||
              err.message?.includes("aborted") ||
              err.message?.includes("ECONNREFUSED") ||
              err.message?.includes("fetch failed") ||
              err.message?.includes("network") ||
              err.code === "ECONNRESET";

            if (isTransportError) {
              const reconnected = await reconnectServer(
                server.name,
                server.config
              );
              if (reconnected) {
                servers[server.name] = reconnected;
                // Retry the call once against the fresh connection
                try {
                  const retry = await reconnected.client.callTool({
                    name: tool.name,
                    arguments: params,
                  });
                  const retryText = (retry.content as any[])
                    .filter((c: any) => c.type === "text")
                    .map((c: any) => c.text)
                    .join("\n");
                  return {
                    content: [
                      {
                        type: "text",
                        text: retryText || "(empty response)",
                      },
                    ],
                    details: {
                      server: server.name,
                      tool: tool.name,
                      reconnected: true,
                    },
                  };
                } catch (retryErr: any) {
                  return {
                    content: [
                      {
                        type: "text",
                        text: `Error after reconnect: ${retryErr.message}`,
                      },
                    ],
                    details: {
                      server: server.name,
                      tool: tool.name,
                      error: true,
                    },
                  };
                }
              }
            }

            return {
              content: [{ type: "text", text: `Error: ${err.message}` }],
              details: { server: server.name, tool: tool.name, error: true },
            };
          }
        },
      });

      count++;
    }
    return count;
  }

  // ── Lifecycle ──────────────────────────────────────────────────────────

  pi.on("session_start", async (_event, ctx) => {
    if (!fs.existsSync(configPath)) {
      ctx.ui.notify("[mcp-bridge] No mcp.json found", "warning");
      return;
    }

    const config: McpConfig = JSON.parse(
      fs.readFileSync(configPath, "utf-8")
    );

    // Connect all servers in parallel with independent timeouts
    const entries = Object.entries(config.servers);
    const results = await Promise.allSettled(
      entries.map(([name, serverConfig]) => connectServer(name, serverConfig))
    );

    let totalTools = 0;
    for (let i = 0; i < entries.length; i++) {
      const [name] = entries[i];
      const result = results[i];

      if (result.status === "rejected") {
        ctx.ui.notify(
          `[mcp-bridge] Failed: ${name} — ${result.reason?.message ?? result.reason}`,
          "error"
        );
        continue;
      }

      const connected = result.value;
      servers[name] = connected;
      totalTools += registerToolsForServer(connected);
    }

    if (totalTools > 0) {
      ctx.ui.notify(
        `[mcp-bridge] ${totalTools} tools from ${Object.keys(servers).length} server(s)`,
        "info"
      );
    }
  });

  pi.on("session_shutdown", async () => {
    await Promise.allSettled(
      Object.values(servers).map((s) => s.client.close())
    );
  });

  // ── Commands ───────────────────────────────────────────────────────────

  pi.registerCommand("mcp", {
    description: "List MCP servers and tools",
    handler: async (_args, ctx) => {
      if (Object.keys(servers).length === 0) {
        ctx.ui.notify("No MCP servers connected", "warning");
        return;
      }

      const lines: string[] = [];
      for (const [name, server] of Object.entries(servers)) {
        const kind =
          server.transport instanceof StreamableHTTPClientTransport
            ? "http"
            : "stdio";
        lines.push(
          `\n${name} [${kind}] (${server.tools.length} tools):`
        );
        for (const tool of server.tools) {
          lines.push(
            `  mcp_${name}_${tool.name} — ${tool.description ?? "(no description)"}`
          );
        }
      }
      ctx.ui.notify(lines.join("\n"), "info");
    },
  });
}

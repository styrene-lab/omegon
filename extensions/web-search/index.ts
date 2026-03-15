// @secret BRAVE_API_KEY "Brave Search API key"
// @secret TAVILY_API_KEY "Tavily Search API key"
// @secret SERPER_API_KEY "Serper/Google Search API key"

import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { StringEnum } from "../lib/typebox-helpers";
import { getAvailableProviders, getProvider, type SearchResult } from "./providers.ts";

function deduplicateResults(results: SearchResult[]): SearchResult[] {
  const seen = new Map<string, SearchResult>();
  for (const r of results) {
    const key = r.url.replace(/\/$/, "").toLowerCase();
    if (seen.has(key)) {
      // Merge provider attribution
      const existing = seen.get(key)!;
      if (!existing.provider.includes(r.provider)) {
        existing.provider += `, ${r.provider}`;
      }
      // Prefer longer snippet
      if (r.snippet.length > existing.snippet.length) {
        existing.snippet = r.snippet;
      }
      if (r.content && (!existing.content || r.content.length > existing.content.length)) {
        existing.content = r.content;
      }
    } else {
      seen.set(key, { ...r });
    }
  }
  return Array.from(seen.values());
}

function formatResults(results: SearchResult[], mode: string): string {
  if (results.length === 0) return "No results found.";

  const lines: string[] = [];
  for (const r of results) {
    lines.push(`### ${r.title}`);
    lines.push(`**URL:** ${r.url}`);
    lines.push(`**Source:** ${r.provider}`);
    lines.push(`${r.snippet}`);
    if (r.content) {
      lines.push(`\n<extracted_content>\n${r.content.slice(0, 2000)}\n</extracted_content>`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

export default function (pi: ExtensionAPI) {
  // Secrets are resolved into process.env by the 00-secrets extension
  // before this extension loads. No .env files needed.

  pi.registerTool({
    name: "web_search",
    label: "Web Search",
    description: `Search the web using multiple providers. Available modes:
- "quick": Use a single provider (fastest)
- "deep": Use a single provider, more results
- "compare": Fan out to ALL configured providers in parallel, deduplicate results. Best for research and verification.

Available providers: brave (independent index), tavily (AI-optimized, extracts content), serper (Google results).
Only providers with configured API keys are available.`,
    promptSnippet: "Search the web via multiple providers (brave, tavily, serper) with quick/deep/compare modes",
    promptGuidelines: [
      "Use 'compare' mode for research requiring cross-source verification",
    ],
    parameters: Type.Object({
      query: Type.String({ description: "Search query" }),
      provider: Type.Optional(
        StringEnum(["brave", "tavily", "serper"], {
          description: "Specific provider. Omit to auto-select (quick) or fan out (compare).",
        })
      ),
      mode: Type.Optional(
        StringEnum(["quick", "deep", "compare"], {
          description: "Search mode. Default: quick",
        })
      ),
      max_results: Type.Optional(
        Type.Number({ description: "Max results per provider. Default: 5", minimum: 1, maximum: 20 })
      ),
      topic: Type.Optional(
        StringEnum(["general", "news"], {
          description: "Search topic. Default: general",
        })
      ),
    }),
    async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
      const mode = params.mode || "quick";
      const maxResults = params.max_results || (mode === "deep" ? 10 : 5);
      const topic = params.topic || "general";
      const available = getAvailableProviders();

      if (available.length === 0) {
        return {
          content: [{
            type: "text",
            text: "No search providers configured. Run `/secrets configure BRAVE_API_KEY` (or TAVILY_API_KEY, SERPER_API_KEY) to set up at least one provider.",
          }],
          details: { error: true },
        };
      }

      let results: SearchResult[] = [];
      let providersUsed: string[] = [];

      if (mode === "compare") {
        // Fan out to all available providers in parallel
        const settled = await Promise.allSettled(
          available.map((p) => p.search(params.query, maxResults, topic))
        );
        for (let i = 0; i < settled.length; i++) {
          const outcome = settled[i];
          if (outcome.status === "fulfilled") {
            results.push(...outcome.value);
            providersUsed.push(available[i].name);
          } else {
            providersUsed.push(`${available[i].name} (error: ${outcome.reason?.message || "unknown"})`);
          }
        }
        results = deduplicateResults(results);
      } else {
        // Single provider
        let provider;
        if (params.provider) {
          provider = getProvider(params.provider);
          if (!provider) {
            return {
              content: [{
                type: "text",
                text: `Provider "${params.provider}" not available. Configured: ${available.map((p) => p.name).join(", ")}`,
              }],
              details: { error: true },
            };
          }
        } else {
          // Auto-select: prefer tavily (content extraction), then serper (google), then brave
          provider =
            available.find((p) => p.name === "tavily") ||
            available.find((p) => p.name === "serper") ||
            available[0];
        }
        try {
          results = await provider.search(params.query, maxResults, topic);
          providersUsed.push(provider.name);
        } catch (err: any) {
          return {
            content: [{ type: "text", text: `Search error (${provider.name}): ${err.message}` }],
            details: { error: true },
          };
        }
      }

      const header = `**Query:** ${params.query}\n**Mode:** ${mode} | **Providers:** ${providersUsed.join(", ")} | **Results:** ${results.length}\n\n---\n\n`;
      const body = formatResults(results, mode);

      return {
        content: [{ type: "text", text: header + body }],
        details: {
          resultCount: results.length,
          providers: providersUsed,
          mode,
        },
      };
    },
  });

  // Notify on load with provider status
  pi.on("session_start", async (_event, ctx) => {
    const available = getAvailableProviders();
    if (available.length > 0) {
      ctx.ui.notify(
        `Web Search: ${available.map((p) => p.name).join(", ")} ready`,
        "info"
      );
    } else {
      const { sharedState } = await import("../lib/shared-state.ts");
      if (!sharedState.bootstrapPending) {
        ctx.ui.notify("Web Search: No API keys configured", "warning");
      }
    }
  });
}

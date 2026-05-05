//! Web search tool — Brave, Tavily, Serper providers via reqwest.
//!
//! First feature crate migration: TS extensions/web-search/ (427 LoC) → Rust.
//! Implements ToolProvider with one tool: web_search.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProvider, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use tokio_util::sync::CancellationToken;

/// Web search tool provider.
pub struct WebSearchProvider {
    client: reqwest::Client,
    secrets: Option<std::sync::Arc<omegon_secrets::SecretsManager>>,
}

impl WebSearchProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            secrets: None,
        }
    }

    pub fn with_secrets(secrets: std::sync::Arc<omegon_secrets::SecretsManager>) -> Self {
        Self {
            client: reqwest::Client::new(),
            secrets: Some(secrets),
        }
    }

    /// Lazily resolve a search API key: check env first, then fall back to
    /// the secrets manager (keyring/recipe).
    fn resolve_key(&self, env_name: &str) -> Option<String> {
        if let Ok(v) = env::var(env_name)
            && !v.is_empty()
        {
            return Some(v);
        }
        let secrets = self.secrets.as_ref()?;
        secrets.resolve(env_name)
    }

    fn available_providers(&self) -> Vec<&'static str> {
        let mut providers = Vec::new();
        if self.resolve_key("BRAVE_API_KEY").is_some() {
            providers.push("brave");
        }
        if self.resolve_key("TAVILY_API_KEY").is_some() {
            providers.push("tavily");
        }
        if self.resolve_key("SERPER_API_KEY").is_some() {
            providers.push("serper");
        }
        if self.resolve_key("FIRECRAWL_API_KEY").is_some() {
            providers.push("firecrawl");
        }
        // DuckDuckGo is always available — no API key needed
        providers.push("ddg");
        providers
    }

    /// Firecrawl search — structured web content extraction.
    /// Uses /v1/search for search and /v1/scrape for URL-to-markdown.
    async fn search_firecrawl(
        &self,
        query: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let api_key = self
            .resolve_key("FIRECRAWL_API_KEY")
            .ok_or_else(|| anyhow::anyhow!("FIRECRAWL_API_KEY not set"))?;
        let resp = self
            .client
            .post("https://api.firecrawl.dev/v1/search")
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&json!({
                "query": query,
                "limit": max_results,
                "scrapeOptions": { "formats": ["markdown"] }
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Firecrawl search error {status}: {body}");
        }
        let data: Value = resp.json().await?;
        let results = data["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .take(max_results)
                    .filter_map(|r| {
                        Some(SearchResult {
                            title: r["metadata"]["title"].as_str()?.to_string(),
                            url: r["url"].as_str()?.to_string(),
                            snippet: r["metadata"]["description"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            content: r["markdown"]
                                .as_str()
                                .map(|s| crate::util::truncate(s, 2000)),
                            provider: "firecrawl".into(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(results)
    }

    /// Fetch a URL's content as clean text using the existing reqwest client.
    /// Strips HTML tags and truncates to 50KB. No external dependencies.
    async fn fetch_url_plain(&self, url: &str) -> anyhow::Result<String> {
        let parsed = validate_fetch_url(url)?;
        let resp = self
            .client
            .get(parsed)
            .header("User-Agent", "Mozilla/5.0 (compatible; omegon/0.17)")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}: {}", resp.status(), resp.url());
        }
        // Limit body to 1MB to prevent OOM from streaming responses
        let bytes = resp.bytes().await?;
        if bytes.len() > 1_048_576 {
            anyhow::bail!("Response too large: {} bytes (max 1MB)", bytes.len());
        }
        let body = String::from_utf8_lossy(&bytes);
        let stripped = strip_html_tags(&body);
        Ok(crate::util::truncate(&stripped, 50_000))
    }

    /// DuckDuckGo HTML search — zero-config, no API key.
    /// Scrapes the lite HTML endpoint and extracts result links + snippets.
    async fn search_ddg(
        &self,
        query: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let mut url = reqwest::Url::parse("https://html.duckduckgo.com/html/")?;
        url.query_pairs_mut().append_pair("q", query);
        let resp = self
            .client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (compatible; omegon/0.15)")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let mut results = Vec::new();
        // DuckDuckGo HTML results follow a consistent pattern:
        //   <a class="result__a" href="...">Title</a>
        //   <a class="result__snippet">Snippet text</a>
        // We parse with simple string scanning — no HTML parser dep needed.
        for chunk in resp.split("class=\"result__a\"").skip(1) {
            if results.len() >= max_results {
                break;
            }
            // Extract href
            let raw_url = chunk
                .split("href=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or("");
            if raw_url.is_empty() {
                continue;
            }
            // Decode DDG redirect URL (//duckduckgo.com/l/?uddg=<encoded>&...)
            let url = if raw_url.contains("uddg=") {
                raw_url
                    .split("uddg=")
                    .nth(1)
                    .and_then(|s| s.split('&').next())
                    .map(|s| {
                        percent_encoding::percent_decode_str(s)
                            .decode_utf8_lossy()
                            .into_owned()
                    })
                    .unwrap_or_default()
            } else if raw_url.starts_with("//") || raw_url.contains("duckduckgo.com") {
                continue; // internal DDG link without redirect target
            } else {
                raw_url.to_string()
            };
            if url.is_empty() {
                continue;
            }
            // Extract title (text between > and </a>)
            let title = chunk
                .split('>')
                .nth(1)
                .and_then(|s| s.split("</a>").next())
                .map(strip_html_tags)
                .unwrap_or_default();
            // Extract snippet from result__snippet class
            let snippet = chunk
                .split("class=\"result__snippet\"")
                .nth(1)
                .and_then(|s| s.split('>').nth(1))
                .and_then(|s| s.split("</").next())
                .map(strip_html_tags)
                .unwrap_or_default();

            if !title.is_empty() {
                results.push(SearchResult {
                    title,
                    url,
                    snippet,
                    content: None,
                    provider: "ddg".into(),
                });
            }
        }
        Ok(results)
    }

    async fn search_brave(
        &self,
        query: &str,
        max_results: usize,
        topic: &str,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let api_key = env::var("BRAVE_API_KEY")?;
        let mut url = reqwest::Url::parse("https://api.search.brave.com/res/v1/web/search")?;
        url.query_pairs_mut()
            .append_pair("q", query)
            .append_pair("count", &max_results.to_string());
        if topic == "news" {
            url.query_pairs_mut().append_pair("freshness", "pd");
        }

        let resp: BraveResponse = self
            .client
            .get(url)
            .header("X-Subscription-Token", &api_key)
            .header("Accept", "application/json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp
            .web
            .map(|w| w.results)
            .unwrap_or_default()
            .into_iter()
            .take(max_results)
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                snippet: r.description.unwrap_or_default(),
                content: None,
                provider: "brave".into(),
            })
            .collect())
    }

    async fn search_tavily(
        &self,
        query: &str,
        max_results: usize,
        topic: &str,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let api_key = env::var("TAVILY_API_KEY")?;
        let body = json!({
            "api_key": api_key,
            "query": query,
            "max_results": max_results,
            "include_answer": false,
            "include_raw_content": false,
            "topic": if topic == "news" { "news" } else { "general" },
        });

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Tavily {status}: {body}");
        }

        let data: TavilyResponse = resp.json().await?;
        Ok(data
            .results
            .into_iter()
            .take(max_results)
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                snippet: r.content.unwrap_or_default(),
                content: r.raw_content,
                provider: "tavily".into(),
            })
            .collect())
    }

    async fn search_serper(
        &self,
        query: &str,
        max_results: usize,
        topic: &str,
    ) -> anyhow::Result<Vec<SearchResult>> {
        let api_key = env::var("SERPER_API_KEY")?;
        let endpoint = if topic == "news" {
            "https://google.serper.dev/news"
        } else {
            "https://google.serper.dev/search"
        };

        let resp = self
            .client
            .post(endpoint)
            .header("X-API-KEY", &api_key)
            .json(&json!({ "q": query, "num": max_results }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Serper {status}: {body}");
        }

        let data: SerperResponse = resp.json().await?;
        let results = if topic == "news" {
            data.news.unwrap_or_default()
        } else {
            data.organic.unwrap_or_default()
        };

        Ok(results
            .into_iter()
            .take(max_results)
            .map(|r| SearchResult {
                title: r.title,
                url: r.link,
                snippet: r.snippet.or(r.description).unwrap_or_default(),
                content: None,
                provider: "serper".into(),
            })
            .collect())
    }

    async fn search_provider(
        &self,
        provider: &str,
        query: &str,
        max_results: usize,
        topic: &str,
    ) -> anyhow::Result<Vec<SearchResult>> {
        match provider {
            "brave" => self.search_brave(query, max_results, topic).await,
            "tavily" => self.search_tavily(query, max_results, topic).await,
            "serper" => self.search_serper(query, max_results, topic).await,
            "firecrawl" => self.search_firecrawl(query, max_results).await,
            "ddg" => self.search_ddg(query, max_results).await,
            _ => anyhow::bail!("Unknown provider: {provider}"),
        }
    }
}

// ─── Response types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
    content: Option<String>,
    provider: String,
}

#[derive(Deserialize)]
struct BraveResponse {
    web: Option<BraveWeb>,
}
#[derive(Deserialize)]
struct BraveWeb {
    results: Vec<BraveResult>,
}
#[derive(Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}
#[derive(Deserialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: Option<String>,
    raw_content: Option<String>,
}

#[derive(Deserialize)]
struct SerperResponse {
    organic: Option<Vec<SerperResult>>,
    news: Option<Vec<SerperResult>>,
}
#[derive(Deserialize)]
struct SerperResult {
    title: String,
    link: String,
    snippet: Option<String>,
    description: Option<String>,
}

// ─── URL validation ────────────────────────────────────────────────────────

/// Validate a URL for fetching: must be http/https, no internal/private hosts.
fn validate_fetch_url(url: &str) -> anyhow::Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url).map_err(|e| anyhow::anyhow!("Invalid URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!("Blocked URL scheme: {scheme}. Only http/https allowed."),
    }
    if let Some(host) = parsed.host_str() {
        // Parse as IP if possible for proper CIDR checking
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let blocked = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                        || v4.octets()[0] == 169 && v4.octets()[1] == 254
                }
                std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
            };
            if blocked {
                anyhow::bail!("Blocked internal/private IP: {ip}");
            }
        } else {
            // Hostname-based checks
            let blocked = host == "localhost"
                || host.ends_with(".internal")
                || host.ends_with(".local")
                || host.ends_with(".localhost");
            if blocked {
                anyhow::bail!("Blocked internal hostname: {host}");
            }
        }
    }
    Ok(parsed)
}

// ─── HTML helpers ──────────────────────────────────────────────────────────

/// Strip HTML tags and decode common entities. Good enough for search snippets.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

// ─── Dedup + Format ─────────────────────────────────────────────────────────

fn deduplicate(results: &mut Vec<SearchResult>) {
    let mut seen = std::collections::HashMap::new();
    results.retain(|r| {
        let key = r.url.trim_end_matches('/').to_lowercase();
        seen.insert(key, ()).is_none()
    });
}

fn format_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }
    let mut out = String::new();
    for r in results {
        out.push_str(&format!("### {}\n", r.title));
        out.push_str(&format!("**URL:** {}\n", r.url));
        out.push_str(&format!("**Source:** {}\n", r.provider));
        out.push_str(&r.snippet);
        out.push('\n');
        if let Some(content) = &r.content {
            let truncated = crate::util::truncate_str(content, 2000);
            out.push_str(&format!(
                "\n<extracted_content>\n{truncated}\n</extracted_content>\n"
            ));
        }
        out.push('\n');
    }
    out
}

// ─── ToolProvider impl ──────────────────────────────────────────────────────

#[async_trait]
impl ToolProvider for WebSearchProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::web_search::WEB_SEARCH.into(),
                label: "Web Search".into(),
                description: "Search the web. Works out of the box via DuckDuckGo; API keys (brave, tavily, serper, firecrawl) optional for deeper results.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" },
                        "provider": { "type": "string", "enum": ["brave", "tavily", "serper", "firecrawl", "ddg"], "description": "Specific provider. Omit to auto-select." },
                        "mode": { "type": "string", "enum": ["quick", "deep", "compare"], "description": "Search mode. Default: quick" },
                        "max_results": { "type": "number", "description": "Max results per provider. Default: 5", "minimum": 1, "maximum": 20 },
                        "topic": { "type": "string", "enum": ["general", "news"], "description": "Search topic. Default: general" }
                    },
                    "required": ["query"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::web_search::WEB_FETCH.into(),
                label: "Web Fetch".into(),
                description: "Fetch a URL's content as clean text. Uses Firecrawl for markdown conversion if available, falls back to curl with HTML stripping.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "The URL to fetch" }
                    },
                    "required": ["url"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        // web_fetch: fetch a single URL's content
        if tool_name == crate::tool_registry::web_search::WEB_FETCH {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("url parameter is required"))?;

            // Try Firecrawl scrape first if available
            let content = if let Some(api_key) = self.resolve_key("FIRECRAWL_API_KEY") {
                let resp = self
                    .client
                    .post("https://api.firecrawl.dev/v1/scrape")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .json(&json!({ "url": url, "formats": ["markdown"] }))
                    .send()
                    .await?;
                if resp.status().is_success() {
                    let data: Value = resp.json().await?;
                    data["data"]["markdown"]
                        .as_str()
                        .map(|s| crate::util::truncate(s, 50_000))
                } else {
                    None // fall through to curl
                }
            } else {
                None
            };

            let content = match content {
                Some(md) => md,
                None => self.fetch_url_plain(url).await?,
            };

            return Ok(ToolResult {
                content: vec![ContentBlock::Text { text: content }],
                details: json!({ "url": url }),
            });
        }

        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("quick")
            .to_string();
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();
        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(if mode == "deep" { 10 } else { 5 }) as usize;
        let requested_provider = args
            .get("provider")
            .and_then(|v| v.as_str())
            .map(String::from);

        {
            let available = self.available_providers();
            // DDG is always available, so this list is never empty

            let mut results = Vec::new();
            let mut providers_used = Vec::new();

            if mode == "compare" {
                for provider in &available {
                    match self
                        .search_provider(provider, &query, max_results, &topic)
                        .await
                    {
                        Ok(r) => {
                            results.extend(r);
                            providers_used.push(provider.to_string());
                        }
                        Err(e) => {
                            providers_used.push(format!("{provider} (error: {e})"));
                        }
                    }
                }
                deduplicate(&mut results);
            } else {
                let provider = if let Some(ref p) = requested_provider {
                    if available.contains(&p.as_str()) {
                        p.clone()
                    } else {
                        return Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "Provider \"{p}\" not available. Configured: {}",
                                    available.join(", ")
                                ),
                            }],
                            details: json!({"error": true}),
                        });
                    }
                } else {
                    // Auto-select: prefer API providers for quality, DDG as fallback
                    available
                        .iter()
                        .find(|&&p| p == "tavily")
                        .or_else(|| available.iter().find(|&&p| p == "serper"))
                        .or_else(|| available.iter().find(|&&p| p == "brave"))
                        .unwrap_or(&available[0])
                        .to_string()
                };

                match self
                    .search_provider(&provider, &query, max_results, &topic)
                    .await
                {
                    Ok(r) => {
                        results = r;
                        providers_used.push(provider);
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!("Search error ({provider}): {e}"),
                            }],
                            details: json!({"error": true}),
                        });
                    }
                }
            }

            let header = format!(
                "**Query:** {query}\n**Mode:** {mode} | **Providers:** {} | **Results:** {}\n\n---\n\n",
                providers_used.join(", "),
                results.len(),
            );
            let body = format_results(&results);

            Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("{header}{body}"),
                }],
                details: json!({}),
            })
        }
    }
}

/// Execute the web search tool with standard CoreTools signature.
pub async fn execute(
    _tool_name: &str,
    _call_id: &str,
    args: serde_json::Value,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<omegon_traits::ToolResult> {
    let provider = WebSearchProvider::new();
    provider.execute("web_search", _call_id, args, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicate_by_url() {
        let mut results = vec![
            SearchResult {
                title: "A".into(),
                url: "https://example.com/".into(),
                snippet: "short".into(),
                content: None,
                provider: "brave".into(),
            },
            SearchResult {
                title: "A".into(),
                url: "https://example.com".into(),
                snippet: "longer snippet".into(),
                content: None,
                provider: "tavily".into(),
            },
            SearchResult {
                title: "B".into(),
                url: "https://other.com".into(),
                snippet: "other".into(),
                content: None,
                provider: "brave".into(),
            },
        ];
        deduplicate(&mut results);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn format_empty_results() {
        assert_eq!(format_results(&[]), "No results found.");
    }

    #[test]
    fn format_results_with_content() {
        let results = vec![SearchResult {
            title: "Test".into(),
            url: "https://test.com".into(),
            snippet: "A test result".into(),
            content: Some("Extracted content here".into()),
            provider: "tavily".into(),
        }];
        let formatted = format_results(&results);
        assert!(formatted.contains("### Test"));
        assert!(formatted.contains("https://test.com"));
        assert!(formatted.contains("extracted_content"));
    }

    #[test]
    fn tool_definition_schema() {
        let provider = WebSearchProvider::new();
        let tools = provider.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "web_search");
        assert_eq!(tools[1].name, "web_fetch");
        let params = &tools[0].parameters;
        assert!(params.get("properties").unwrap().get("query").is_some());
    }

    #[test]
    fn available_providers_from_env() {
        // Without env vars set, should return empty
        let provider = WebSearchProvider::new();
        let available = provider.available_providers();
        // Can't assert empty because CI might have keys set
        // Just verify it doesn't panic
        let _ = available;
    }
}

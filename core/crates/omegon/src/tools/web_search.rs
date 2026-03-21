//! Web search tool — Brave, Tavily, Serper providers via reqwest.
//!
//! First feature crate migration: TS extensions/web-search/ (427 LoC) → Rust.
//! Implements ToolProvider with one tool: web_search.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProvider, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use tokio_util::sync::CancellationToken;

/// Web search tool provider.
pub struct WebSearchProvider {
    client: reqwest::Client,
}

impl WebSearchProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn available_providers(&self) -> Vec<&'static str> {
        let mut providers = Vec::new();
        if env::var("BRAVE_API_KEY").is_ok() {
            providers.push("brave");
        }
        if env::var("TAVILY_API_KEY").is_ok() {
            providers.push("tavily");
        }
        if env::var("SERPER_API_KEY").is_ok() {
            providers.push("serper");
        }
        providers
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
            let truncated = if content.len() > 2000 { &content[..2000] } else { content };
            out.push_str(&format!("\n<extracted_content>\n{truncated}\n</extracted_content>\n"));
        }
        out.push('\n');
    }
    out
}

// ─── ToolProvider impl ──────────────────────────────────────────────────────

#[async_trait]
impl ToolProvider for WebSearchProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: crate::tool_registry::web_search::WEB_SEARCH.into(),
            label: "Web Search".into(),
            description: "Search the web using multiple providers (brave, tavily, serper). Modes: quick (single provider), deep (more results), compare (all providers, deduplicated).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "provider": { "type": "string", "enum": ["brave", "tavily", "serper"], "description": "Specific provider. Omit to auto-select." },
                    "mode": { "type": "string", "enum": ["quick", "deep", "compare"], "description": "Search mode. Default: quick" },
                    "max_results": { "type": "number", "description": "Max results per provider. Default: 5", "minimum": 1, "maximum": 20 },
                    "topic": { "type": "string", "enum": ["general", "news"], "description": "Search topic. Default: general" }
                },
                "required": ["query"]
            }),
        }]
    }

    async fn execute(
        &self,
        _tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("quick").to_string();
        let topic = args.get("topic").and_then(|v| v.as_str()).unwrap_or("general").to_string();
        let max_results = args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(if mode == "deep" { 10 } else { 5 }) as usize;
        let requested_provider = args.get("provider").and_then(|v| v.as_str()).map(String::from);

        {
            let available = self.available_providers();
            if available.is_empty() {
                return Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "No search providers configured. Set BRAVE_API_KEY, TAVILY_API_KEY, or SERPER_API_KEY.".into(),
                    }],
                    details: json!({"error": true}),
                });
            }

            let mut results = Vec::new();
            let mut providers_used = Vec::new();

            if mode == "compare" {
                for provider in &available {
                    match self.search_provider(provider, &query, max_results, &topic).await {
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
                                text: format!("Provider \"{p}\" not available. Configured: {}", available.join(", ")),
                            }],
                            details: json!({"error": true}),
                        });
                    }
                } else {
                    // Auto-select: prefer tavily, then serper, then brave
                    available
                        .iter()
                        .find(|&&p| p == "tavily")
                        .or_else(|| available.iter().find(|&&p| p == "serper"))
                        .unwrap_or(&available[0])
                        .to_string()
                };

                match self.search_provider(&provider, &query, max_results, &topic).await {
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
            SearchResult { title: "A".into(), url: "https://example.com/".into(), snippet: "short".into(), content: None, provider: "brave".into() },
            SearchResult { title: "A".into(), url: "https://example.com".into(), snippet: "longer snippet".into(), content: None, provider: "tavily".into() },
            SearchResult { title: "B".into(), url: "https://other.com".into(), snippet: "other".into(), content: None, provider: "brave".into() },
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
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "web_search");
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

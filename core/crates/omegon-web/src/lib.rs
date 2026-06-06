//! Zero-config web search and content extraction.
//!
//! Provides best-effort zero-key web search via DuckDuckGo HTML scraping.
//! The scraper parses DuckDuckGo's static HTML endpoint with CSS selectors via
//! the `scraper` crate. Configure authenticated providers in Omegon for
//! reliable search.
//!
//! # Usage
//!
//! ```ignore
//! use omegon_web::{WebClient, SearchOptions};
//!
//! let client = WebClient::new();
//! let results = client.search("rust async runtime", &SearchOptions::default()).await?;
//! for r in &results {
//!     println!("{} — {}", r.title, r.url);
//! }
//! ```
//!
//! # Privacy
//!
//! Zero-key search sends queries to DuckDuckGo servers. Your IP address and
//! search terms are visible to DuckDuckGo. For better reliability and privacy
//! controls, configure API-based providers (Tavily, Serper, Brave, Firecrawl).

mod engines;
mod extract;
pub mod http;

pub use engines::{Engine, SearchError, SearchResult};
pub use extract::extract_content;

/// Shared web client — reuses connections, cookies, and TLS sessions.
/// Create once and pass to all search/fetch calls.
pub struct WebClient {
    client: reqwest::Client,
}

impl WebClient {
    pub fn new() -> Self {
        Self {
            client: http::build_client(),
        }
    }

    /// Create from an existing reqwest::Client (for sharing with other code).
    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for WebClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Search options.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub max_results: usize,
    /// Which engines to use. Empty = all in priority order with failover.
    pub engines: Vec<Engine>,
    /// Aggregate results from all engines (concurrent) instead of failover.
    pub aggregate: bool,
    /// Search topic: "general" (default) or "news".
    pub topic: String,
    /// Region hint (e.g., "US", "GB", "DE"). Empty = engine default.
    pub region: String,
    /// Language hint (e.g., "en", "de", "ja"). Empty = engine default.
    pub language: String,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 5,
            engines: Vec::new(),
            aggregate: false,
            topic: "general".into(),
            region: String::new(),
            language: String::new(),
        }
    }
}

const MAX_FETCH_SIZE: usize = 2_097_152; // 2 MB

impl WebClient {
    /// Perform a web search. Engines run concurrently; first success wins in
    /// failover mode, all results merged in aggregate mode.
    pub async fn search(
        &self,
        query: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let engines = if opts.engines.is_empty() {
            vec![Engine::DuckDuckGo]
        } else {
            opts.engines.clone()
        };

        if opts.aggregate {
            self.search_aggregate(&engines, query, opts).await
        } else {
            self.search_failover(&engines, query, opts).await
        }
    }

    /// Concurrent aggregate — fire all engines, merge results.
    async fn search_aggregate(
        &self,
        engines: &[Engine],
        query: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let futures: Vec<_> = engines
            .iter()
            .map(|e| e.search(&self.client, query, opts))
            .collect();
        let outcomes = futures::future::join_all(futures).await;

        let mut all = Vec::new();
        for outcome in outcomes {
            match outcome {
                Ok(results) => all.extend(results),
                Err(e) => tracing::debug!(error = %e, "engine failed in aggregate mode"),
            }
        }
        deduplicate(&mut all);
        all.truncate(opts.max_results);
        if all.is_empty() {
            return Err(SearchError::AllEnginesFailed(
                "all engines returned zero results in aggregate mode".into(),
            ));
        }
        Ok(all)
    }

    /// Concurrent failover — fire all engines, return first success.
    async fn search_failover(
        &self,
        engines: &[Engine],
        query: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, SearchError> {
        use futures::future::select_all;

        let mut futs: Vec<_> = engines
            .iter()
            .map(|e| Box::pin(e.search(&self.client, query, opts)))
            .collect();

        let mut errors = Vec::new();

        while !futs.is_empty() {
            let (result, _index, remaining) = select_all(futs).await;
            match result {
                Ok(results) if !results.is_empty() => return Ok(results),
                Ok(_) => errors.push("returned zero results".into()),
                Err(e) => errors.push(e.to_string()),
            }
            futs = remaining;
        }

        let detail = errors.join("; ");
        Err(SearchError::AllEnginesFailed(format!(
            "All search engines failed. {detail}\n\
             Tip: configure TAVILY_API_KEY or SERPER_API_KEY for reliable search."
        )))
    }

    /// Fetch a URL and extract its main content as clean text.
    /// Validates URL against SSRF (blocks private IPs and internal hostnames).
    pub async fn fetch_content(&self, url: &str) -> anyhow::Result<String> {
        http::validate_url(url)?;

        let resp = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?
            .error_for_status()?;

        // Early size check from Content-Length header
        if let Some(len) = resp.content_length()
            && len > MAX_FETCH_SIZE as u64
        {
            anyhow::bail!("response too large: {len} bytes (max {MAX_FETCH_SIZE})");
        }

        let bytes = resp.bytes().await?;
        if bytes.len() > MAX_FETCH_SIZE {
            anyhow::bail!(
                "response too large: {} bytes (max {MAX_FETCH_SIZE})",
                bytes.len()
            );
        }
        let html = String::from_utf8_lossy(&bytes);
        Ok(extract::extract_content(&html))
    }
}

/// Legacy convenience function — creates a temporary client per call.
/// Prefer `WebClient::new()` for repeated use.
pub async fn search(query: &str, opts: &SearchOptions) -> Result<Vec<SearchResult>, SearchError> {
    WebClient::new().search(query, opts).await
}

/// Legacy convenience function.
pub async fn fetch_content(url: &str) -> anyhow::Result<String> {
    WebClient::new().fetch_content(url).await
}

fn deduplicate(results: &mut Vec<SearchResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| {
        let key = r.url.trim_end_matches('/').to_lowercase();
        seen.insert(key)
    });
}

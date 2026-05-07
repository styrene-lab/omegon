//! Zero-config web search and content extraction.
//!
//! Provides web search via Google, Bing, and DuckDuckGo HTML scraping —
//! no API keys required. Each engine parses search result pages with
//! CSS selectors via the `scraper` crate for robustness against markup changes.
//!
//! # Usage
//!
//! ```ignore
//! use omegon_web::{search, SearchOptions, Engine};
//!
//! let results = search("rust async runtime", &SearchOptions::default()).await?;
//! for r in &results {
//!     println!("{} — {}", r.title, r.url);
//! }
//! ```
//!
//! # Engine priority
//!
//! By default, engines are tried in order: Google → Bing → DuckDuckGo.
//! If one fails (blocked, rate-limited, parse error), the next is tried.
//! All three are always attempted in `aggregate` mode.

mod engines;
mod extract;
mod http;

pub use engines::{Engine, SearchResult};
pub use extract::extract_content;

/// Search options.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Maximum results to return.
    pub max_results: usize,
    /// Which engines to use. Empty = all in priority order with failover.
    pub engines: Vec<Engine>,
    /// Aggregate results from all engines instead of using first success.
    pub aggregate: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 5,
            engines: Vec::new(),
            aggregate: false,
        }
    }
}

/// Perform a web search. Returns deduplicated results from one or more engines.
pub async fn search(
    query: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    let client = http::client();
    let engines = if opts.engines.is_empty() {
        vec![Engine::Google, Engine::Bing, Engine::DuckDuckGo]
    } else {
        opts.engines.clone()
    };

    if opts.aggregate {
        let mut all = Vec::new();
        for engine in &engines {
            match engine.search(&client, query, opts.max_results).await {
                Ok(results) => all.extend(results),
                Err(e) => tracing::debug!(?engine, error = %e, "engine failed in aggregate mode"),
            }
        }
        deduplicate(&mut all);
        all.truncate(opts.max_results);
        Ok(all)
    } else {
        // Failover: try each engine in order, return first success
        let mut last_err = None;
        for engine in &engines {
            match engine.search(&client, query, opts.max_results).await {
                Ok(results) if !results.is_empty() => return Ok(results),
                Ok(_) => {
                    tracing::debug!(?engine, "returned zero results, trying next");
                }
                Err(e) => {
                    tracing::debug!(?engine, error = %e, "search failed, trying next");
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("all search engines returned zero results")))
    }
}

/// Fetch a URL and extract its main content as clean text.
pub async fn fetch_content(url: &str) -> anyhow::Result<String> {
    let client = http::client();
    let resp = client
        .get(url)
        .headers(http::browser_headers())
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?;

    let bytes = resp.bytes().await?;
    if bytes.len() > 2_097_152 {
        anyhow::bail!("response too large: {} bytes", bytes.len());
    }
    let html = String::from_utf8_lossy(&bytes);
    Ok(extract::extract_content(&html))
}

fn deduplicate(results: &mut Vec<SearchResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| {
        let key = r.url.trim_end_matches('/').to_lowercase();
        seen.insert(key)
    });
}

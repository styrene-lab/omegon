//! Search engine implementations — Google, Bing, DuckDuckGo.
//!
//! Each engine scrapes the HTML search results page with CSS selectors.
//! No API keys. No accounts. Just HTTP + HTML parsing.

mod google;
mod bing;
mod duckduckgo;

/// A search engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Google,
    Bing,
    DuckDuckGo,
}

impl Engine {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Bing => "bing",
            Self::DuckDuckGo => "ddg",
        }
    }

    pub(crate) async fn search(
        &self,
        client: &reqwest::Client,
        query: &str,
        max_results: usize,
    ) -> anyhow::Result<Vec<SearchResult>> {
        match self {
            Self::Google => google::search(client, query, max_results).await,
            Self::Bing => bing::search(client, query, max_results).await,
            Self::DuckDuckGo => duckduckgo::search(client, query, max_results).await,
        }
    }
}

/// A single search result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub engine: &'static str,
}

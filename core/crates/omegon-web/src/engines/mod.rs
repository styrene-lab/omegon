//! Search engine implementations — DuckDuckGo zero-key search.

mod duckduckgo;

use crate::SearchOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    DuckDuckGo,
}

impl Engine {
    pub fn name(&self) -> &'static str {
        match self {
            Self::DuckDuckGo => "ddg",
        }
    }

    pub(crate) async fn search(
        &self,
        client: &reqwest::Client,
        query: &str,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let result = match self {
            Self::DuckDuckGo => duckduckgo::search(client, query, opts).await,
        };
        result.map_err(|e| classify_error(self.name(), e))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub engine: &'static str,
}

/// Structured error for search failures.
#[derive(Debug)]
pub enum SearchError {
    BotDetected { engine: String, detail: String },
    RateLimited { engine: String },
    ParseFailed { engine: String },
    AllEnginesFailed(String),
    Http { engine: String, source: String },
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BotDetected { engine, detail } => {
                write!(f, "bot detection by {engine}: {detail}")
            }
            Self::RateLimited { engine } => write!(f, "rate limited by {engine}"),
            Self::ParseFailed { engine } => write!(
                f,
                "no results parsed from {engine} (possible markup change)"
            ),
            Self::AllEnginesFailed(msg) => write!(f, "{msg}"),
            Self::Http { engine, source } => write!(f, "{engine}: {source}"),
        }
    }
}

impl std::error::Error for SearchError {}

fn classify_error(engine: &str, err: anyhow::Error) -> SearchError {
    let msg = err.to_string();
    if msg.contains("bot detection") || msg.contains("consent") || msg.contains("CAPTCHA") {
        SearchError::BotDetected {
            engine: engine.into(),
            detail: msg,
        }
    } else if msg.contains("429") || msg.contains("202") || msg.contains("rate limit") {
        SearchError::RateLimited {
            engine: engine.into(),
        }
    } else if msg.contains("no results parsed")
        || msg.contains("unexpected HTML shell")
        || msg.contains("no results returned")
    {
        SearchError::ParseFailed {
            engine: engine.into(),
        }
    } else {
        SearchError::Http {
            engine: engine.into(),
            source: msg,
        }
    }
}

//! Google HTML search scraper.
//!
//! Scrapes google.com/search with CSS selectors. No API key.
//! Reference: metasearch2 (CC0 licensed).

use scraper::{Html, Selector};
use url::Url;

use super::SearchResult;
use crate::SearchOptions;

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut params = vec![
        ("q", query.to_string()),
        ("nfpr", "1".into()),
        ("num", opts.max_results.to_string()),
    ];
    if opts.topic == "news" {
        params.push(("tbm", "nws".into()));
    }
    if !opts.language.is_empty() {
        params.push(("hl", opts.language.clone()));
    }
    if !opts.region.is_empty() {
        params.push(("gl", opts.region.clone()));
    }

    let url = Url::parse_with_params(
        "https://www.google.com/search",
        &params
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect::<Vec<_>>(),
    )?;

    let resp = client.get(url).send().await?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("rate limited by Google (429). Try again in 60 seconds.");
    }

    let body = resp.error_for_status()?.text().await?;

    // Detect bot walls
    if body.contains("detected unusual traffic")
        || body.contains("/sorry/")
        || body.contains("CAPTCHA")
    {
        anyhow::bail!("bot detection by Google — CAPTCHA or abuse page served");
    }

    parse_results(&body, opts.max_results)
}

fn parse_results(body: &str, max_results: usize) -> anyhow::Result<Vec<SearchResult>> {
    let dom = Html::parse_document(body);
    let mut results = Vec::new();

    // Try primary selector first, fall back to div.g if it matches nothing
    let primary = Selector::parse("[jscontroller=SC7lYd]").unwrap();
    let fallback = Selector::parse("div.g").unwrap();
    let title_sel = Selector::parse("h3").unwrap();
    let link_sel = Selector::parse("a[href]").unwrap();
    let desc_sel = Selector::parse(
        "div[data-sncf='2'], div[data-sncf='1,2'], div[style='-webkit-line-clamp:2'], span.st, div.VwiC3b"
    ).unwrap();

    let result_els: Vec<_> = dom.select(&primary).collect();
    let result_els = if result_els.is_empty() {
        dom.select(&fallback).collect()
    } else {
        result_els
    };

    for result_el in result_els {
        if results.len() >= max_results {
            break;
        }

        let title = result_el
            .select(&title_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();

        let raw_url = result_el
            .select(&link_sel)
            .next()
            .and_then(|el| el.value().attr("href"))
            .unwrap_or_default();

        let url = clean_google_url(raw_url);
        if url.is_empty() || title.is_empty() {
            continue;
        }
        if url.starts_with('/') || url.contains("google.com/search") {
            continue;
        }

        let snippet = result_el
            .select(&desc_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();

        results.push(SearchResult {
            title: title.trim().to_string(),
            url,
            snippet: snippet.trim().to_string(),
            engine: "google",
        });
    }

    if results.is_empty() {
        anyhow::bail!("google: no results parsed (possible markup change or bot detection)");
    }

    Ok(results)
}

fn clean_google_url(raw: &str) -> String {
    if raw.starts_with("/url?")
        && let Ok(url) = Url::parse(&format!("https://www.google.com{raw}"))
        && let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "q")
    {
        return v.to_string();
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_google_redirect_url() {
        let raw = "/url?q=https://docs.rs/tokio&sa=U&ved=2ah";
        assert_eq!(clean_google_url(raw), "https://docs.rs/tokio");
    }

    #[test]
    fn clean_direct_url() {
        let raw = "https://example.com/page";
        assert_eq!(clean_google_url(raw), raw);
    }
}

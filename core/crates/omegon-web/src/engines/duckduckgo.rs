//! DuckDuckGo HTML search scraper.
//!
//! Scrapes html.duckduckgo.com/html/ with CSS selectors.
//! No API key. More robust than the previous string-scanning approach.

use scraper::{Html, Selector};

use super::SearchResult;

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let resp = client
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    parse_results(&resp, max_results)
}

fn parse_results(body: &str, max_results: usize) -> anyhow::Result<Vec<SearchResult>> {
    let dom = Html::parse_document(body);
    let mut results = Vec::new();

    let result_sel = Selector::parse(".result").unwrap();
    let title_sel = Selector::parse("a.result__a").unwrap();
    let snippet_sel = Selector::parse("a.result__snippet, .result__snippet").unwrap();

    for result_el in dom.select(&result_sel) {
        if results.len() >= max_results {
            break;
        }

        let title_el = match result_el.select(&title_sel).next() {
            Some(el) => el,
            None => continue,
        };

        let title = title_el.text().collect::<String>();
        let raw_url = title_el.value().attr("href").unwrap_or_default();
        let url = decode_ddg_url(raw_url);

        if url.is_empty() || title.is_empty() {
            continue;
        }
        if url.contains("duckduckgo.com") {
            continue;
        }

        let snippet = result_el
            .select(&snippet_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default();

        results.push(SearchResult {
            title: title.trim().to_string(),
            url,
            snippet: snippet.trim().to_string(),
            engine: "ddg",
        });
    }

    if results.is_empty() {
        anyhow::bail!("ddg: no results parsed");
    }

    Ok(results)
}

fn decode_ddg_url(raw: &str) -> String {
    // DDG redirects through //duckduckgo.com/l/?uddg=<encoded>&...
    if raw.contains("uddg=") {
        if let Some(encoded) = raw.split("uddg=").nth(1).and_then(|s| s.split('&').next()) {
            return percent_encoding::percent_decode_str(encoded)
                .decode_utf8_lossy()
                .into_owned();
        }
    }
    if raw.starts_with("//") {
        return String::new();
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_redirect_url() {
        let raw = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Ftokio&rut=abc";
        assert_eq!(decode_ddg_url(raw), "https://docs.rs/tokio");
    }

    #[test]
    fn decode_direct_url() {
        let raw = "https://example.com/page";
        assert_eq!(decode_ddg_url(raw), raw);
    }

    #[test]
    fn skip_internal_ddg_link() {
        let raw = "//duckduckgo.com/some-internal";
        assert_eq!(decode_ddg_url(raw), "");
    }
}

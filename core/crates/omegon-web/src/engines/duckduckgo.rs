//! DuckDuckGo HTML search scraper.

use scraper::{Html, Selector};

use super::SearchResult;
use crate::SearchOptions;

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    let effective_query = if opts.topic == "news" {
        format!("{query} !news")
    } else {
        query.to_string()
    };

    let resp = client
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", effective_query.as_str())])
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        || resp.status() == reqwest::StatusCode::FORBIDDEN
        || resp.status() == reqwest::StatusCode::ACCEPTED
    {
        anyhow::bail!("rate limited by DuckDuckGo ({})", resp.status());
    }

    let body = resp.error_for_status()?.text().await?;

    parse_results(&body, opts.max_results)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageClass {
    Results,
    NoResults,
    BotOrChallenge,
    ConsentOrRegion,
    UnexpectedShell,
}

fn classify_page(body: &str) -> PageClass {
    let lower = body.to_ascii_lowercase();

    if lower.contains("captcha")
        || lower.contains("automated")
        || lower.contains("unusual traffic")
        || lower.contains("rate limit")
        || lower.contains("ratelimit")
        || (lower.contains("blocked") && lower.contains("duckduckgo"))
    {
        return PageClass::BotOrChallenge;
    }

    if lower.contains("consent")
        || lower.contains("region") && lower.contains("settings")
        || lower.contains("please enable cookies")
    {
        return PageClass::ConsentOrRegion;
    }

    let dom = Html::parse_document(body);
    let title_sel = Selector::parse("a.result__a").unwrap();
    if dom.select(&title_sel).next().is_some() {
        return PageClass::Results;
    }

    if lower.contains("no results") || lower.contains("not many results") {
        return PageClass::NoResults;
    }

    PageClass::UnexpectedShell
}

fn parse_results(body: &str, max_results: usize) -> anyhow::Result<Vec<SearchResult>> {
    match classify_page(body) {
        PageClass::Results => {}
        PageClass::NoResults => anyhow::bail!("ddg: no results returned"),
        PageClass::BotOrChallenge => anyhow::bail!("bot detection by DuckDuckGo"),
        PageClass::ConsentOrRegion => anyhow::bail!("consent or region interstitial by DuckDuckGo"),
        PageClass::UnexpectedShell => {
            anyhow::bail!("ddg: unexpected HTML shell; no result markup found")
        }
    }

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

        if url.is_empty() || title.is_empty() || url.contains("duckduckgo.com") {
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
    if raw.contains("uddg=")
        && let Some(encoded) = raw.split("uddg=").nth(1).and_then(|s| s.split('&').next())
    {
        return percent_encoding::percent_decode_str(encoded)
            .decode_utf8_lossy()
            .into_owned();
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
        assert_eq!(decode_ddg_url("https://example.com"), "https://example.com");
    }

    #[test]
    fn skip_internal() {
        assert_eq!(decode_ddg_url("//duckduckgo.com/foo"), "");
    }

    #[test]
    fn classify_result_page() {
        let html = r#"
            <html><body>
              <div class="result">
                <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Ftokio&rut=abc">
                  Tokio - docs.rs
                </a>
                <a class="result__snippet">An event-driven async runtime for Rust.</a>
              </div>
            </body></html>
        "#;
        assert_eq!(classify_page(html), PageClass::Results);
    }

    #[test]
    fn parse_classic_result_fixture() {
        let html = r#"
            <html><body>
              <div class="result results_links_deep web-result">
                <h2 class="result__title">
                  <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&rut=abc">
                    Rust Programming Language
                  </a>
                </h2>
                <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust-lang.org%2F&rut=abc">
                  A language empowering everyone to build reliable and efficient software.
                </a>
              </div>
              <div class="result">
                <a class="result__a" href="https://docs.rs/scraper">scraper - Rust</a>
                <span class="result__snippet">HTML parsing and CSS selection.</span>
              </div>
            </body></html>
        "#;

        let results = parse_results(html, 10).expect("fixture parses");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert_eq!(results[0].engine, "ddg");
        assert!(results[0].snippet.contains("reliable and efficient"));
        assert_eq!(results[1].url, "https://docs.rs/scraper");
    }

    #[test]
    fn classify_bot_challenge_before_parse_failure() {
        let html = r#"
            <html><body>
              <h1>DuckDuckGo</h1>
              <p>Requests from this client appear automated and are blocked.</p>
            </body></html>
        "#;
        assert_eq!(classify_page(html), PageClass::BotOrChallenge);
        let err = parse_results(html, 5).unwrap_err().to_string();
        assert!(err.contains("bot detection"), "{err}");
    }

    #[test]
    fn classify_consent_or_region_interstitial() {
        let html = r#"
            <html><body>
              <form><h1>Consent</h1><p>Please enable cookies to continue.</p></form>
            </body></html>
        "#;
        assert_eq!(classify_page(html), PageClass::ConsentOrRegion);
    }

    #[test]
    fn classify_unexpected_shell_without_result_markup() {
        let html = r#"
            <html><body><main id="react-root"></main><script src="/serp.js"></script></body></html>
        "#;
        assert_eq!(classify_page(html), PageClass::UnexpectedShell);
        let err = parse_results(html, 5).unwrap_err().to_string();
        assert!(err.contains("unexpected HTML shell"), "{err}");
    }
}

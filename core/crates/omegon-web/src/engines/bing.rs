//! Bing HTML search scraper.
//!
//! Scrapes bing.com/search with CSS selectors. No API key.
//! Uses a random CVID to avoid session tracking.
//! Reference: metasearch2 (CC0 licensed).

use scraper::{ElementRef, Html, Selector};
use url::Url;

use super::SearchResult;

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let cvid = generate_cvid();
    let url = Url::parse_with_params(
        "https://www.bing.com/search",
        &[
            ("q", query),
            ("cvid", &cvid),
            ("FORM", "PERE"),
        ],
    )?;

    let body = client
        .get(url)
        .header("Cookie", format!("SRCHHPGUSR=IG={cvid}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    parse_results(&body, max_results)
}

fn generate_cvid() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn parse_results(body: &str, max_results: usize) -> anyhow::Result<Vec<SearchResult>> {
    let dom = Html::parse_document(body);
    let mut results = Vec::new();

    let result_sel = Selector::parse("#b_results > li.b_algo").unwrap();
    let title_sel = Selector::parse("h2 > a").unwrap();
    let desc_sel = Selector::parse(".b_caption > p, p.b_algoSlug, .b_caption .ipText").unwrap();

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
        let url = clean_bing_url(raw_url);

        if url.is_empty() || title.is_empty() {
            continue;
        }

        let snippet = result_el
            .select(&desc_sel)
            .next()
            .map(|el| extract_text_skipping_icons(&el))
            .unwrap_or_default();

        results.push(SearchResult {
            title: title.trim().to_string(),
            url,
            snippet: snippet.trim().to_string(),
            engine: "bing",
        });
    }

    if results.is_empty() {
        anyhow::bail!("bing: no results parsed");
    }

    Ok(results)
}

fn extract_text_skipping_icons(el: &ElementRef) -> String {
    let mut text = String::new();
    for child in el.children() {
        match child.value() {
            scraper::Node::Text(t) => text.push_str(&t.text),
            scraper::Node::Element(inner) => {
                if !inner.has_class("algoSlug_icon", scraper::CaseSensitivity::CaseSensitive) {
                    if let Some(el_ref) = ElementRef::wrap(child) {
                        text.push_str(&el_ref.text().collect::<String>());
                    }
                }
            }
            _ => {}
        }
    }
    text
}

fn clean_bing_url(raw: &str) -> String {
    if raw.starts_with("https://www.bing.com/ck/a?") {
        if let Ok(url) = Url::parse(raw) {
            if let Some((_, u)) = url.query_pairs().find(|(k, _)| k == "u") {
                if u.len() > 2 {
                    if let Ok(decoded) = base64::Engine::decode(
                        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                        &u[2..],
                    ) {
                        return String::from_utf8_lossy(&decoded).to_string();
                    }
                }
            }
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvid_format() {
        let cvid = generate_cvid();
        assert_eq!(cvid.len(), 32);
        assert!(cvid.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

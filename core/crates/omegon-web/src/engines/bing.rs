//! Bing HTML search scraper.

use scraper::{ElementRef, Html, Selector};
use url::Url;

use super::SearchResult;
use crate::SearchOptions;

pub async fn search(
    client: &reqwest::Client,
    query: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    let cvid = generate_cvid();
    let mut params = vec![
        ("q", query.to_string()),
        ("cvid", cvid.clone()),
        ("FORM", "PERE".into()),
    ];
    if opts.topic == "news" {
        params.push(("qft", "sortbydate".into()));
        params.push(("form", "QBNH".into()));
    }
    if !opts.region.is_empty() {
        params.push(("cc", opts.region.clone()));
    }
    if !opts.language.is_empty() {
        params.push(("setlang", opts.language.clone()));
    }

    let url = Url::parse_with_params(
        "https://www.bing.com/search",
        &params
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect::<Vec<_>>(),
    )?;

    let resp = client
        .get(url)
        .header("Cookie", format!("SRCHHPGUSR=IG={cvid}; _EDGE_V=1"))
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("rate limited by Bing (429)");
    }

    let body = resp.error_for_status()?.text().await?;

    // Detect consent wall
    if body.contains("bnp_container")
        || body.contains("consent.bing.com")
        || body.contains("Before you continue")
    {
        anyhow::bail!("bot detection / consent wall by Bing");
    }

    parse_results(&body, opts.max_results)
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
            scraper::Node::Element(inner)
                if !inner.has_class("algoSlug_icon", scraper::CaseSensitivity::CaseSensitive)
                    && let Some(el_ref) = ElementRef::wrap(child) =>
            {
                text.push_str(&el_ref.text().collect::<String>());
            }
            _ => {}
        }
    }
    text
}

fn clean_bing_url(raw: &str) -> String {
    if raw.starts_with("https://www.bing.com/ck/a?")
        && let Ok(url) = Url::parse(raw)
        && let Some((_, u)) = url.query_pairs().find(|(k, _)| k == "u")
        && u.len() > 2
        && let Ok(decoded) =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &u[2..])
    {
        return String::from_utf8_lossy(&decoded).to_string();
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

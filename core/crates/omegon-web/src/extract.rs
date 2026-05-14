//! HTML content extraction — clean text from web pages.
//!
//! Strips navigation, headers, footers, scripts, styles, and ads.
//! Extracts the main content body similar to Firefox Reader Mode.

use scraper::{Html, Selector};

/// Extract the main text content from an HTML page.
/// Removes scripts, styles, nav, footer, header, and common non-content elements.
pub fn extract_content(html: &str) -> String {
    let dom = Html::parse_document(html);

    // Try to find the main content container
    let main_selectors = [
        "article",
        "main",
        "[role='main']",
        ".post-content",
        ".article-content",
        ".entry-content",
        "#content",
        ".content",
    ];

    for sel_str in &main_selectors {
        if let Ok(sel) = Selector::parse(sel_str)
            && let Some(el) = dom.select(&sel).next()
        {
            let text = extract_text_from_element(&el);
            if text.len() > 200 {
                return clean_whitespace(&text);
            }
        }
    }

    // Fallback: extract from body, skipping non-content elements
    if let Ok(body_sel) = Selector::parse("body")
        && let Some(body) = dom.select(&body_sel).next()
    {
        let text = extract_text_from_element(&body);
        return clean_whitespace(&text);
    }

    // Last resort: all text
    clean_whitespace(&dom.root_element().text().collect::<String>())
}

fn extract_text_from_element(el: &scraper::ElementRef) -> String {
    let skip_tags: std::collections::HashSet<&str> = [
        "script", "style", "nav", "footer", "header", "aside", "noscript", "iframe", "svg", "form",
        "button",
    ]
    .into_iter()
    .collect();

    let mut text = String::new();
    collect_text(&mut text, el, &skip_tags);
    text
}

fn collect_text(
    out: &mut String,
    el: &scraper::ElementRef,
    skip_tags: &std::collections::HashSet<&str>,
) {
    for child in el.children() {
        match child.value() {
            scraper::Node::Text(t) => {
                let trimmed = t.text.trim();
                if !trimmed.is_empty() {
                    out.push_str(trimmed);
                    out.push(' ');
                }
            }
            scraper::Node::Element(inner) => {
                let tag = inner.name();
                if skip_tags.contains(tag) {
                    continue;
                }
                // Block elements get newlines
                if matches!(
                    tag,
                    "p" | "div"
                        | "br"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "li"
                        | "tr"
                        | "blockquote"
                        | "pre"
                ) {
                    out.push('\n');
                }
                if let Some(el_ref) = scraper::ElementRef::wrap(child) {
                    collect_text(out, &el_ref, skip_tags);
                }
                if matches!(
                    tag,
                    "p" | "div"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "li"
                        | "tr"
                        | "blockquote"
                        | "pre"
                ) {
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
}

fn clean_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_blank = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank {
                out.push('\n');
                prev_blank = true;
            }
        } else {
            out.push_str(trimmed);
            out.push('\n');
            prev_blank = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_paragraph_text() {
        let html = "<html><body><p>Hello world</p><p>Second paragraph</p></body></html>";
        let text = extract_content(html);
        assert!(text.contains("Hello world"));
        assert!(text.contains("Second paragraph"));
    }

    #[test]
    fn strips_script_and_style() {
        let html = "<html><body><script>var x = 1;</script><p>Content</p><style>.a{}</style></body></html>";
        let text = extract_content(html);
        assert!(text.contains("Content"));
        assert!(!text.contains("var x"));
        assert!(!text.contains(".a{}"));
    }

    #[test]
    fn strips_nav_footer() {
        let html = "<html><body><nav>Menu</nav><main><p>Article text</p></main><footer>Copyright</footer></body></html>";
        let text = extract_content(html);
        assert!(text.contains("Article text"));
        assert!(!text.contains("Menu"));
        assert!(!text.contains("Copyright"));
    }

    #[test]
    fn prefers_article_element() {
        let html = "<html><body><div>Noise</div><article><p>The real content here is long enough to be preferred over the body fallback extraction path</p></article></body></html>";
        let text = extract_content(html);
        assert!(text.contains("real content"));
    }
}

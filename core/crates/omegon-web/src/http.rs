//! Shared HTTP client with browser-like headers.

use reqwest::header::{HeaderMap, HeaderValue};

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers(browser_headers())
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .expect("http client")
}

pub fn browser_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("User-Agent", HeaderValue::from_static(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
    ));
    h.insert("Accept", HeaderValue::from_static(
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
    ));
    h.insert("Accept-Language", HeaderValue::from_static("en-US,en;q=0.9"));
    h.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
    h.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    h.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
    h.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
    h.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
    h
}

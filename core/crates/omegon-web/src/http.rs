//! Shared HTTP client with browser-like headers and SSRF protection.

use reqwest::header::{HeaderMap, HeaderValue};

/// Build a reusable reqwest::Client with browser-like defaults.
pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers(browser_headers())
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .gzip(true)
        .build()
        .expect("omegon-web http client")
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
    h.insert("Accept-Encoding", HeaderValue::from_static("gzip, deflate, br"));
    // Sec-Fetch headers — required by modern browsers
    h.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
    h.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
    h.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
    h.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
    // Client Hints — absence is a bot fingerprint when UA claims Chrome
    h.insert("Sec-CH-UA", HeaderValue::from_static(
        "\"Google Chrome\";v=\"131\", \"Chromium\";v=\"131\", \"Not_A Brand\";v=\"24\""
    ));
    h.insert("Sec-CH-UA-Mobile", HeaderValue::from_static("?0"));
    h.insert("Sec-CH-UA-Platform", HeaderValue::from_static("\"macOS\""));
    h.insert("Upgrade-Insecure-Requests", HeaderValue::from_static("1"));
    h
}

/// Validate a URL for fetching — blocks SSRF targets.
pub fn validate_url(url: &str) -> anyhow::Result<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| anyhow::anyhow!("invalid URL: {e}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!("blocked URL scheme: {scheme}"),
    }

    if let Some(host) = parsed.host_str() {
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let blocked = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                        || (v4.octets()[0] == 169 && v4.octets()[1] == 254)
                }
                std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
            };
            if blocked {
                anyhow::bail!("blocked internal/private IP: {ip}");
            }
        } else {
            let blocked = host == "localhost"
                || host.ends_with(".internal")
                || host.ends_with(".local")
                || host.ends_with(".localhost");
            if blocked {
                anyhow::bail!("blocked internal hostname: {host}");
            }
        }
    }
    Ok(())
}

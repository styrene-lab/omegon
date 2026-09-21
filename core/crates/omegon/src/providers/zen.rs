//! Explicit anonymous OpenCode Zen routes. Public credentials cannot spend money.
//!
//! Eligibility is curated from models.dev provider metadata and Zen's documented
//! free offerings, then intersected with the current gateway inventory. An ID or
//! a `-free` suffix alone is not capability, pricing, or anonymous-access evidence.
//! Sources reviewed 2026-09-07:
//! https://opencode.ai/docs/zen
//! https://github.com/anomalyco/models.dev/tree/dev/providers/opencode/models
//! https://github.com/anomalyco/opencode/blob/5e3100a46a6ffe8062aedb2a9649cc7bcc0926ad/packages/console/app/src/routes/zen/util/handler.ts

use std::sync::Mutex;
use std::time::{Duration, Instant};

static CATALOG_CACHE: Mutex<Option<(Instant, Vec<FreeModel>)>> = Mutex::new(None);

use anyhow::Context;
use serde::Deserialize;

pub(crate) const PROVIDER_ID: &str = "opencode-zen";
const MODELS_URL: &str = "https://opencode.ai/zen/v1/models";
const MAX_CATALOG_BYTES: usize = 512 * 1024;
const PRIVACY_NOTICE: &str = "Prompts and completions may be used to improve the model. Free availability is temporary. https://opencode.ai/docs/zen#privacy";

#[derive(Clone, Debug)]
pub(crate) struct FreeModel {
    pub id: &'static str,
    pub name: &'static str,
    pub context_window: usize,
    pub privacy_notice: &'static str,
    input_price: u32,
    output_price: u32,
    anonymous: bool,
    tools: bool,
}

const CURATED: &[FreeModel] = &[
    FreeModel {
        id: "mimo-v2.5-free",
        name: "MiMo V2.5 Free",
        context_window: 200_000,
        privacy_notice: PRIVACY_NOTICE,
        input_price: 0,
        output_price: 0,
        anonymous: true,
        tools: true,
    },
    FreeModel {
        id: "big-pickle",
        name: "Big Pickle",
        context_window: 200_000,
        privacy_notice: PRIVACY_NOTICE,
        input_price: 0,
        output_price: 0,
        anonymous: true,
        tools: true,
    },
];

fn eligible(model: &FreeModel) -> bool {
    model.input_price == 0 && model.output_price == 0 && model.anonymous && model.tools
}

pub(crate) fn model(id: &str) -> Option<&'static FreeModel> {
    CURATED
        .iter()
        .find(|model| model.id == id && eligible(model))
}

pub(crate) fn supports_model(id: &str) -> bool {
    model(id).is_some()
}

#[derive(Deserialize)]
struct Catalog {
    data: Vec<CatalogModel>,
}

#[derive(Deserialize)]
struct CatalogModel {
    id: String,
}

fn available_models(bytes: &[u8]) -> anyhow::Result<Vec<FreeModel>> {
    anyhow::ensure!(
        bytes.len() <= MAX_CATALOG_BYTES,
        "OpenCode Zen catalog is too large"
    );
    let catalog: Catalog = serde_json::from_slice(bytes).context("Invalid OpenCode Zen catalog")?;
    Ok(CURATED
        .iter()
        .filter(|model| eligible(model) && catalog.data.iter().any(|item| item.id == model.id))
        .cloned()
        .collect())
}

/// Public metadata GET only; no credentials, inference, or workspace data.
/// Called on explicit free-model discovery and before each inference request so
/// withdrawn routes fail closed instead of silently selecting another model.
pub(crate) async fn refresh_models() -> anyhow::Result<Vec<FreeModel>> {
    let result = refresh_from_url(MODELS_URL).await;
    if let Ok(mut cache) = CATALOG_CACHE.lock() {
        *cache = result
            .as_ref()
            .ok()
            .map(|models| (Instant::now(), models.clone()));
    }
    result
}

pub(crate) async fn ensure_available(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        supports_model(id),
        "Choose an available free model with /connect free"
    );
    let cached = CATALOG_CACHE.lock().ok().and_then(|cache| {
        cache
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
            .map(|(_, models)| models.clone())
    });
    let models = match cached {
        Some(models) => models,
        None => refresh_models().await?,
    };
    anyhow::ensure!(
        models.iter().any(|model| model.id == id),
        "OpenCode Zen free model {id} has been withdrawn; refresh /connect free. No paid fallback was used."
    );
    Ok(())
}

#[cfg(test)]
pub(crate) struct TestCatalogGuard(Option<(Instant, Vec<FreeModel>)>);
#[cfg(test)]
impl Drop for TestCatalogGuard {
    fn drop(&mut self) {
        *CATALOG_CACHE.lock().unwrap() = self.0.take();
    }
}
#[cfg(test)]
pub(crate) fn test_catalog(ids: &[&str]) -> TestCatalogGuard {
    let mut cache = CATALOG_CACHE.lock().unwrap();
    let prior = cache.take();
    *cache = Some((
        Instant::now(),
        ids.iter().filter_map(|id| model(id).cloned()).collect(),
    ));
    TestCatalogGuard(prior)
}

pub(super) async fn refresh_from_url(url: &str) -> anyhow::Result<Vec<FreeModel>> {
    refresh_with_deadline(url, Duration::from_secs(5)).await
}

async fn refresh_with_deadline(url: &str, deadline: Duration) -> anyhow::Result<Vec<FreeModel>> {
    let client = reqwest::Client::builder()
        .timeout(deadline)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut response = client
        .get(url)
        .send()
        .await
        .context("Could not refresh OpenCode Zen free models; retry /connect free")?;
    anyhow::ensure!(
        response.status().is_success(),
        "OpenCode Zen catalog unavailable ({}); retry /connect free",
        response.status()
    );
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|len| len <= MAX_CATALOG_BYTES as u64),
        "OpenCode Zen catalog is too large"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            body.len().saturating_add(chunk.len()) <= MAX_CATALOG_BYTES,
            "OpenCode Zen catalog is too large"
        );
        body.extend_from_slice(&chunk);
    }
    available_models(&body)
}

pub(super) fn validate_model(options: &crate::bridge::StreamOptions) -> anyhow::Result<&str> {
    let spec = options.model.as_deref().unwrap_or_default();
    let id = spec.strip_prefix("opencode-zen:").unwrap_or(spec);
    anyhow::ensure!(
        supports_model(id),
        "Choose an available free model with /connect free; OpenCode Zen does not permit paid or unknown models on this route"
    );
    anyhow::ensure!(
        !options.extra_body.contains_key("model"),
        "OpenCode Zen model overrides are not allowed"
    );
    Ok(id)
}

pub(super) fn withdrawn(id: &str) -> crate::bridge::ProviderRouteUnavailable {
    crate::bridge::ProviderRouteUnavailable {
        model: format!("{PROVIDER_ID}:{id}"),
        message: format!(
            "OpenCode Zen free model {id} has been withdrawn; refresh /connect free. No paid fallback was used."
        ),
    }
}

pub(super) fn definitive_failure(
    status: reqwest::StatusCode,
    id: &str,
) -> Option<crate::bridge::ProviderRouteUnavailable> {
    matches!(status.as_u16(), 401..=404).then(|| crate::bridge::ProviderRouteUnavailable {
        model: format!("{PROVIDER_ID}:{id}"),
        message: failure_hint(status).into(),
    })
}

pub(super) fn failure_hint(status: reqwest::StatusCode) -> &'static str {
    match status.as_u16() {
        429 => {
            "OpenCode Zen free capacity is rate limited. Retry later or choose another connection with /connect. No paid fallback was used."
        }
        401..=404 => {
            "This OpenCode Zen free route is unavailable or has been withdrawn. Refresh /connect free. No paid fallback was used."
        }
        _ => {
            "OpenCode Zen is unavailable. Retry later or choose another connection with /connect. No paid fallback was used."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zen_catalog_requires_curated_eligibility_and_current_availability() {
        let models = available_models(
            &serde_json::to_vec(&json!({"data":[
                {"id":"big-pickle"}, {"id":"gpt-paid"}, {"id":"unknown-free"}, {"id":"big-pickle"}
            ]}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "big-pickle");
        assert!(available_models(br#"{"data":[]}"#).unwrap().is_empty());
        assert!(available_models(br#"{"models":[]}"#).is_err());
        let mut paid = CURATED[0].clone();
        paid.output_price = 1;
        assert!(!eligible(&paid));
        paid.output_price = 0;
        paid.input_price = 1;
        assert!(!eligible(&paid));
        paid.input_price = 0;
        paid.tools = false;
        assert!(!eligible(&paid));
        paid.tools = true;
        paid.anonymous = false;
        assert!(!eligible(&paid));
    }

    #[test]
    fn zen_route_rejects_paid_unknown_and_wire_model_overrides() {
        let mut opts = crate::bridge::StreamOptions::default();
        for id in [
            "",
            "opencode-zen:paid",
            "opencode-go:big-pickle",
            "unknown-free",
        ] {
            opts.model = Some(id.into());
            assert!(validate_model(&opts).is_err(), "accepted {id}");
        }
        opts.model = Some("opencode-zen:big-pickle".into());
        assert_eq!(validate_model(&opts).unwrap(), "big-pickle");
        opts.extra_body.insert("model".into(), json!("paid"));
        assert!(validate_model(&opts).is_err());
    }

    #[test]
    fn zen_failure_messages_preserve_free_boundary() {
        assert!(failure_hint(reqwest::StatusCode::TOO_MANY_REQUESTS).contains("rate limited"));
        assert!(failure_hint(reqwest::StatusCode::NOT_FOUND).contains("withdrawn"));
        assert!(failure_hint(reqwest::StatusCode::PAYMENT_REQUIRED).contains("No paid fallback"));
    }

    #[tokio::test]
    async fn zen_refresh_is_bounded_public_get_and_intersects_inventory() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/models", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let n = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..n]);
            assert!(request.starts_with("GET /v1/models "));
            assert!(!request.to_ascii_lowercase().contains("authorization:"));
            let body = r#"{"data":[{"id":"big-pickle"},{"id":"paid"}]}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let models = refresh_from_url(&url).await.unwrap();
        assert_eq!(
            models.iter().map(|m| m.id).collect::<Vec<_>>(),
            ["big-pickle"]
        );
        server.await.unwrap();
        assert!(available_models(&vec![b' '; MAX_CATALOG_BYTES + 1]).is_err());
    }
    async fn mock_stream(
        status: &str,
        response_body: &str,
    ) -> anyhow::Result<Vec<crate::bridge::LlmEvent>> {
        use crate::bridge::{LlmBridge, StreamOptions};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let status = status.to_string();
        let response_body = response_body.to_string();
        let server = tokio::spawn(async move {
            for step in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 4096];
                    let n = stream.read(&mut bytes).await.unwrap();
                    assert!(n > 0);
                    request.extend_from_slice(&bytes[..n]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let len = headers
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|s| s.parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(request).unwrap();
                let (code, body) = if step == 0 {
                    assert!(request.starts_with("GET /v1/models "));
                    ("200 OK", r#"{"data":[{"id":"big-pickle"}]}"#)
                } else {
                    assert!(request.starts_with("POST /v1/chat/completions "));
                    assert!(
                        request
                            .to_ascii_lowercase()
                            .contains("authorization: bearer public")
                    );
                    let (_, body) = request.split_once("\r\n\r\n").unwrap();
                    let body: serde_json::Value = serde_json::from_str(body).unwrap();
                    assert_eq!(body["model"], "big-pickle");
                    assert_eq!(body["tools"][0]["function"]["name"], "read");
                    assert!(body.get("reasoning_effort").is_none());
                    (status.as_str(), response_body.as_str())
                };
                stream.write_all(format!("HTTP/1.1 {code}\r\nContent-Length: {}\r\nContent-Type: text/event-stream\r\nRetry-After: 2\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        let bridge =
            super::super::OpenAICompatClient::new("public".into(), base, PROVIDER_ID.into());
        let tools = vec![omegon_traits::ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "Read a file".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            capabilities: vec![],
        }];
        let options = StreamOptions {
            model: Some("opencode-zen:big-pickle".into()),
            reasoning: Some("medium".into()),
            ..Default::default()
        };
        let stream = bridge.stream("Fixture system", &[], &tools, &options).await;
        server.await.unwrap();
        let mut stream = stream?;
        let mut events = vec![];
        while let Some(event) = stream.recv().await {
            events.push(event);
        }
        Ok(events)
    }

    #[tokio::test]
    async fn zen_bridge_streams_text_and_tool_calls_with_public_credential() {
        let first = json!({"choices":[{"delta":{"content":"Reading"}}]});
        let tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read","arguments":"{\"path\":\"README.md\"}"}}]}}]});
        let finish = json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]});
        let events = mock_stream(
            "200 OK",
            &format!("data: {first}\n\ndata: {tool}\n\ndata: {finish}\n\ndata: [DONE]\n\n"),
        )
        .await
        .unwrap();
        assert!(events.iter().any(
            |e| matches!(e, crate::bridge::LlmEvent::TextDelta { delta } if delta == "Reading")
        ));
        assert!(events.iter().any(|e| matches!(e, crate::bridge::LlmEvent::ToolCallEnd { tool_call } if tool_call.name == "read" && tool_call.arguments["path"] == "README.md")));
    }

    #[tokio::test]
    async fn zen_bridge_preserves_throttling_and_never_uses_paid_credentials() {
        let events = mock_stream(
            "429 Too Many Requests",
            r#"{"error":{"message":"capacity exhausted"}}"#,
        )
        .await
        .unwrap();
        assert!(events.iter().any(|e| matches!(e, crate::bridge::LlmEvent::UpstreamFailure { failure } if failure.retry_after_ms == Some(2000) && failure.message.contains("rate limited") && failure.message.contains("No paid fallback"))));
    }

    #[tokio::test]
    async fn zen_managed_route_passes_exact_tool_admission_without_secret() {
        use crate::inference_inventory::{InventoryLayer, InventorySnapshot};
        use crate::provider_route_service::{ProviderRouteService, ProviderRouteServiceContract};
        let _catalog = test_catalog(&["big-pickle"]);
        let inventory = InventorySnapshot::build(
            1,
            vec![InventoryLayer::embedded_registry(
                crate::model_registry::ModelRegistry::global(),
            )],
        )
        .unwrap();
        let route = ProviderRouteService
            .resolve_exact_admitted(
                "opencode-zen:big-pickle",
                None,
                &inventory,
                &["tools".into()],
            )
            .await;
        assert!(
            route.is_some(),
            "anonymous managed route must bypass manifest bearer-token requirements"
        );
        assert!(
            ProviderRouteService
                .resolve_exact_admitted("opencode-zen:paid", None, &inventory, &[])
                .await
                .is_none()
        );
    }
    #[tokio::test]
    async fn zen_withdrawn_route_is_disconnected_even_with_paid_fallback_credentials() {
        use crate::route::{CredentialProbe, CredentialState, ProviderRoute, RouteController};
        struct NoProbe;
        impl CredentialProbe for NoProbe {
            fn probe_provider(&self, _: &str) -> CredentialState {
                panic!("free route must not probe paid fallback credentials");
            }
        }
        let _catalog = test_catalog(&[]);
        let route = RouteController::resolve_startup(
            "opencode-zen:big-pickle".into(),
            &["openai".into()],
            &NoProbe,
        )
        .await;
        assert!(matches!(route, ProviderRoute::Disconnected { .. }));
        assert!(
            ensure_available("big-pickle")
                .await
                .unwrap_err()
                .to_string()
                .contains("withdrawn")
        );
    }
    #[tokio::test]
    async fn zen_definitive_http_failure_returns_typed_route_unavailability() {
        let error = mock_stream("404 Not Found", r#"{"error":{"message":"removed"}}"#)
            .await
            .unwrap_err();
        let unavailable = error
            .downcast_ref::<crate::bridge::ProviderRouteUnavailable>()
            .unwrap();
        assert_eq!(unavailable.model, "opencode-zen:big-pickle");
        assert!(unavailable.message.contains("withdrawn"));
        assert!(definitive_failure(reqwest::StatusCode::TOO_MANY_REQUESTS, "big-pickle").is_none());
        assert!(
            definitive_failure(reqwest::StatusCode::SERVICE_UNAVAILABLE, "big-pickle").is_none()
        );
        assert!(
            anyhow::Error::from(withdrawn("big-pickle"))
                .is::<crate::bridge::ProviderRouteUnavailable>()
        );
    }
    #[tokio::test]
    async fn zen_catalog_deadline_covers_stalled_response_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/models", listener.local_addr().unwrap());
        let (headers_sent, headers_received) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 1024];
            assert!(stream.read(&mut bytes).await.unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
                .await
                .unwrap();
            let _ = headers_sent.send(());
            std::future::pending::<()>().await;
        });
        let request =
            tokio::spawn(
                async move { refresh_with_deadline(&url, Duration::from_millis(250)).await },
            );
        headers_received.await.unwrap();
        let result = request.await.unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        let error = result.unwrap_err();
        assert!(
            error
                .downcast_ref::<reqwest::Error>()
                .is_some_and(reqwest::Error::is_timeout),
            "{error:#}"
        );
    }
}

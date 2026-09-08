//! Ollama embedding service — implements `EmbeddingService` for hybrid search.
//!
//! Configuration priority: profile fields > env vars > defaults.
//! Designed for swarm deployments where multiple omegon instances share a
//! remote Ollama server. The reqwest client maintains a long-lived connection
//! pool for efficient reuse.

use async_trait::async_trait;
use omegon_memory::embedding::{EmbedError, EmbeddingService};
use serde::Deserialize;

const DEFAULT_EMBED_URL: &str = "http://localhost:11434";
const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

/// Embedding service backed by Ollama's `/api/embed` endpoint.
pub struct OllamaEmbeddingService {
    /// Lazy-initialized HTTP client. Deferred to avoid triggering macOS
    /// keychain prompts during TLS root store initialization at setup time.
    client: tokio::sync::OnceCell<reqwest::Client>,
    base_url: String,
    model: String,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<ModelIdentity>,
}
#[derive(Deserialize, PartialEq, Eq)]
struct ModelIdentity {
    name: String,
    digest: String,
    #[serde(default)]
    remote_model: Option<String>,
    #[serde(default)]
    remote_host: Option<String>,
}

async fn bounded_json<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, EmbedError> {
    const MAX_BYTES: usize = 2 * 1024 * 1024;
    if !response.status().is_success() {
        return Err(EmbedError::RequestFailed(format!(
            "embedding HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| EmbedError::RequestFailed(error.without_url().to_string()))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err(EmbedError::RequestFailed(
                "embedding response exceeds byte budget".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| EmbedError::RequestFailed("invalid embedding response schema".into()))
}

impl OllamaEmbeddingService {
    /// Construct from optional profile overrides + env vars + defaults.
    ///
    /// Resolution order per field:
    /// 1. `profile_*` argument (from `.omegon/profile.json`)
    /// 2. Environment variable (`OMEGON_EMBED_URL` / `OMEGON_EMBED_MODEL`)
    /// 3. Compile-time default
    pub fn from_config(profile_url: Option<&str>, profile_model: Option<&str>) -> Self {
        let base_url = profile_url
            .map(String::from)
            .or_else(|| std::env::var("OMEGON_EMBED_URL").ok())
            .unwrap_or_else(|| DEFAULT_EMBED_URL.to_string());
        let model = profile_model
            .map(String::from)
            .or_else(|| std::env::var("OMEGON_EMBED_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_EMBED_MODEL.to_string());

        Self {
            client: tokio::sync::OnceCell::new(),
            base_url,
            model,
        }
    }

    /// Get or initialize the HTTP client. Deferred so TLS root store loading
    /// only happens when we actually need to make an HTTP request.
    async fn client(&self) -> &reqwest::Client {
        self.client
            .get_or_init(|| async {
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .pool_max_idle_per_host(4)
                    .build()
                    .unwrap_or_default()
            })
            .await
    }

    /// Probe whether the embedding endpoint is reachable.
    ///
    /// Uses the lazy-initialized HTTP client to GET `/api/tags`. The client
    /// is only constructed on the first call, deferring TLS root store loading
    /// past the setup phase (avoids macOS keychain prompts during tests).
    pub async fn probe(&self) -> bool {
        let client = self.client().await;
        client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_millis(200))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn identity(&self) -> Result<ModelIdentity, EmbedError> {
        let response = self
            .client()
            .await
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map_err(|error| EmbedError::Unavailable(error.without_url().to_string()))?;
        let models: ModelsResponse = bounded_json(response).await?;
        let configured = self.model.strip_suffix(":latest").unwrap_or(&self.model);
        let mut identity = models
            .models
            .into_iter()
            .find(|model| model.name.strip_suffix(":latest").unwrap_or(&model.name) == configured)
            .ok_or_else(|| {
                EmbedError::Unavailable(
                    "configured embedding model is absent from model inventory".into(),
                )
            })?;
        let digest = identity
            .digest
            .strip_prefix("sha256:")
            .unwrap_or(&identity.digest);
        if identity.remote_model.is_some()
            || identity.remote_host.is_some()
            || digest.len() != 64
            || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(EmbedError::Unavailable(
                "verifiable embedding content digest is unavailable".into(),
            ));
        }
        identity.digest = digest.to_ascii_lowercase();
        Ok(identity)
    }
}

#[async_trait]
impl EmbeddingService for OllamaEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if text.len() > 65_536 {
            return Err(EmbedError::RequestFailed(
                "embedding input exceeds byte budget".into(),
            ));
        }
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let resp = self
            .client()
            .await
            .post(format!("{}/api/embed", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbedError::Unavailable(e.without_url().to_string()))?;

        let data: EmbedResponse = bounded_json(resp).await?;

        data.embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::RequestFailed("empty embeddings array".into()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn embed_identified(
        &self,
        text: &str,
    ) -> Result<omegon_memory::IdentifiedEmbedding, EmbedError> {
        let before = self.identity().await?;
        let values = self.embed(text).await?;
        if before != self.identity().await? {
            return Err(EmbedError::Unavailable(
                "embedding model changed during generation".into(),
            ));
        }
        let result = omegon_memory::IdentifiedEmbedding {
            space: omegon_memory::EmbeddingSpace {
                model: format!("ollama:{}", before.name),
                revision: before.digest,
                preprocessing: "ollama-api-embed/raw-v1".into(),
                dimensions: values.len() as u32,
            },
            values,
        };
        result
            .validate()
            .map_err(|_| EmbedError::RequestFailed("invalid identified embedding".into()))?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wave4_ollama_identity_is_checked_around_generation() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        for change in [false, true] {
            let changed = Arc::new(AtomicBool::new(false));
            let tags_changed = changed.clone();
            let app = axum::Router::new()
                .route("/api/tags", axum::routing::get(move || {
                    let flag = tags_changed.clone();
                    async move { axum::Json(serde_json::json!({"models":[{"name":"fixture:latest", "digest":if flag.load(Ordering::SeqCst) { "b".repeat(64) } else { "a".repeat(64) }}]})) }
                }))
                .route("/api/embed", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let flag = changed.clone();
                    async move {
                        assert_eq!(body["model"], "fixture");
                        assert_eq!(body["input"], "evidence");
                        if change { flag.store(true, Ordering::SeqCst); }
                        axum::Json(serde_json::json!({"embeddings":[[1.0,0.0]]}))
                    }
                }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let service = OllamaEmbeddingService::from_config(
                Some(&format!("http://{address}")),
                Some("fixture"),
            );
            let result = service.embed_identified("evidence").await;
            server.abort();
            let _ = server.await;
            if change {
                assert!(result.is_err());
            } else {
                let embedding = result.unwrap();
                assert_eq!(embedding.space.revision, "a".repeat(64));
                assert_eq!(embedding.space.dimensions, 2);
                assert_eq!(embedding.space.model, "ollama:fixture:latest");
            }
        }
    }

    #[test]
    fn config_uses_defaults_when_nothing_set() {
        // Profile overrides always win over env, so test with explicit None
        // to verify the default path without touching env vars.
        let svc = OllamaEmbeddingService::from_config(None, None);
        // When env vars aren't set, should use defaults
        // (we can't safely clear env in tests, but defaults are the fallback)
        assert!(!svc.base_url.is_empty());
        assert!(!svc.model.is_empty());
    }

    #[test]
    fn config_profile_overrides_everything() {
        let svc = OllamaEmbeddingService::from_config(
            Some("http://profile-host:11434"),
            Some("custom-model"),
        );
        assert_eq!(svc.base_url, "http://profile-host:11434");
        assert_eq!(svc.model, "custom-model");
    }

    /// Integration test — requires a running Ollama instance with nomic-embed-text.
    #[tokio::test]
    #[ignore]
    async fn ollama_embed_integration() {
        let svc = OllamaEmbeddingService::from_config(None, None);
        if !svc.probe().await {
            eprintln!("Ollama not reachable, skipping integration test");
            return;
        }
        let vec = svc.embed("test embedding").await.unwrap();
        assert!(!vec.is_empty(), "embedding should be non-empty");
        // nomic-embed-text produces 768-dim vectors
        assert_eq!(vec.len(), 768, "expected 768-dim from nomic-embed-text");
    }
}

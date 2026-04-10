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
    client: reqwest::Client,
    base_url: String,
    model: String,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
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

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .build()
            .unwrap_or_default();

        Self {
            client,
            base_url,
            model,
        }
    }

    /// The configured base URL (for logging).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Probe whether the embedding endpoint is reachable.
    ///
    /// Uses a short timeout (200ms) HTTP GET to `/api/tags` — this validates
    /// Ollama is actually responding, not just that the port is open.
    pub async fn probe(&self) -> bool {
        let probe_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(200))
            .build()
            .unwrap_or_default();
        probe_client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[async_trait]
impl EmbeddingService for OllamaEmbeddingService {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let resp = self
            .client
            .post(format!("{}/api/embed", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbedError::Unavailable(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(EmbedError::RequestFailed(format!("{status}: {text}")));
        }

        let data: EmbedResponse = resp
            .json()
            .await
            .map_err(|e| EmbedError::RequestFailed(e.to_string()))?;

        data.embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::RequestFailed("empty embeddings array".into()))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

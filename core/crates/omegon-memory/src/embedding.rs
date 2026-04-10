//! Embedding service abstraction for hybrid search.
//!
//! The trait lives in omegon-memory (no HTTP deps); implementations live in the
//! omegon binary crate where reqwest is available.

use async_trait::async_trait;

/// Errors from embedding generation.
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The embedding endpoint is not reachable or not configured.
    #[error("Embedding service unavailable: {0}")]
    Unavailable(String),
    /// The request was sent but failed (bad model, server error, etc.).
    #[error("Embedding request failed: {0}")]
    RequestFailed(String),
}

/// Generates embedding vectors from text.
///
/// Implementors must be safe to share across threads (`Send + Sync`) and should
/// maintain a long-lived HTTP client with connection pooling for swarm deployments
/// where multiple omegon instances share a remote embedding server.
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    /// Generate an embedding vector for the given text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError>;

    /// The model name this service uses (stored in `embedding_metadata`).
    fn model_name(&self) -> &str;
}

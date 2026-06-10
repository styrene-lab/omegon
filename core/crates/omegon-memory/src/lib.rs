//! omegon-memory — Memory backend for the Omegon agent loop.
//!
//! This crate defines the interface boundary for the memory system:
//! - [`MemoryBackend`] trait — storage abstraction (sqlite in prod, in-memory for tests)
//! - [`MemoryProvider`] — implements ToolProvider + ContextProvider + SessionHook
//!   by delegating to a MemoryBackend
//! - Type definitions mirroring `api-types.ts` — the canonical wire format
//!
//! # Architecture
//!
//! ```text
//! Agent Loop
//!   ├── ToolProvider::execute("memory_store", args)
//!   │     └── MemoryProvider → MemoryBackend::store_fact()
//!   ├── ContextProvider::provide_context(signals)
//!   │     └── MemoryProvider → MemoryBackend::render_context()
//!   └── SessionHook::on_session_start()
//!         └── MemoryProvider → MemoryBackend::import_jsonl() + render_context()
//! ```

pub mod backend;
pub mod decay;
pub mod embedding;
pub mod hash;
pub mod inmemory;
#[cfg(feature = "agent")]
pub mod provider;
pub mod renderer;
pub mod service;
pub mod sqlite;
pub mod types;
pub mod util;
pub mod vault_sync;
pub mod vectors;

#[cfg(test)]
mod tests;

// Re-exports for convenience
pub use backend::{ContextRenderer, MemoryBackend, MemoryError};
pub use decay::{DecayProfile, compute_confidence};
pub use embedding::{EmbedError, EmbeddingService};
pub use hash::{content_hash, normalize_for_hash};
pub use inmemory::InMemoryBackend;
#[cfg(feature = "agent")]
pub use provider::MemoryProvider;
pub use renderer::MarkdownRenderer;
pub use service::{MemoryMindService, expand_edges};
pub use sqlite::SqliteBackend;
/// Re-exports all types from the `types` module for convenience.
/// If you encounter name collisions with other crates (e.g. `Section`,
/// `Fact`, `Edge`), use qualified paths: `omegon_memory::types::Section`.
pub use types::*;
pub use vectors::{blob_to_vector, cosine_similarity, rrf_merge, vector_to_blob};

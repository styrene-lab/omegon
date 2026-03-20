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

pub mod types;
pub mod decay;
pub mod hash;
pub mod vectors;
pub mod backend;
pub mod util;
pub mod inmemory;
pub mod sqlite;
pub mod renderer;
pub mod provider;

#[cfg(test)]
mod tests;

// Re-exports for convenience
pub use backend::{MemoryBackend, ContextRenderer, MemoryError};
pub use inmemory::InMemoryBackend;
pub use sqlite::SqliteBackend;
pub use renderer::MarkdownRenderer;
pub use provider::MemoryProvider;
pub use types::*;
pub use decay::{compute_confidence, DecayProfile};
pub use hash::{content_hash, normalize_for_hash};
pub use vectors::{cosine_similarity, vector_to_blob, blob_to_vector};

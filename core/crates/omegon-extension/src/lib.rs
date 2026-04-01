//! Omegon Extension SDK
//!
//! This crate provides a safe, versioned interface for building extensions for Omegon.
//! Extension developers depend on this crate with a locked version matching their
//! target Omegon release. The version constraint ensures compatibility.
//!
//! # Safety Model
//!
//! Extensions run in isolated processes (either native binaries or OCI containers).
//! An extension crash does not crash Omegon. The extension protocol:
//!
//! 1. **Version checking** — omegon validates extension SDK version at install time
//! 2. **Manifest validation** — schema and capability checks before instantiation
//! 3. **RPC isolation** — all communication is via JSON-RPC over stdin/stdout
//! 4. **Timeout enforcement** — RPC calls have hard timeouts
//! 5. **Type safety** — Rust serde validation on every message
//!
//! # Building an Extension
//!
//! Implement [`Extension`] in your binary:
//!
//! ```ignore
//! use omegon_extension::{Extension, RpcMessage, rpc};
//!
//! #[derive(Default)]
//! struct MyExtension;
//!
//! #[async_trait::async_trait]
//! impl Extension for MyExtension {
//!     fn name(&self) -> &str { "my-extension" }
//!     fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
//!
//!     async fn handle_rpc(&self, method: &str, params: serde_json::Value) -> rpc::Result {
//!         match method {
//!             "get_tools" => Ok(serde_json::json!([])),
//!             "get_timeline" => Ok(serde_json::json!({"events": []})),
//!             _ => Err(rpc::ErrorCode::MethodNotFound.into()),
//!         }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let ext = MyExtension::default();
//!     omegon_extension::serve(ext).await.expect("failed to serve extension");
//! }
//! ```
//!
//! Create a `manifest.toml` in your extension directory:
//!
//! ```toml
//! [extension]
//! name = "my-extension"
//! version = "0.1.0"
//! description = "My custom extension"
//!
//! [runtime]
//! type = "native"
//! binary = "target/release/my-extension"
//!
//! [startup]
//! ping_method = "get_tools"
//! timeout_ms = 5000
//!
//! [widgets.timeline]
//! label = "Timeline"
//! kind = "stateful"
//! renderer = "timeline"
//! ```
//!
//! Place the entire directory in `~/.omegon/extensions/{name}/` and omegon
//! will discover it automatically.

mod error;
mod extension;
mod manifest;
mod rpc;

pub use error::{Error, ErrorCode, Result};
pub use extension::{Extension, ExtensionServe};
pub use manifest::{ExtensionManifest, ManifestError};
pub use rpc::{RpcMessage, RpcRequest, RpcResponse, RpcError};

/// Convenience type for RPC method results.
pub type RpcResult = Result<serde_json::Value>;

/// Serve an extension instance over RPC (stdin/stdout).
/// Blocks until the extension shuts down.
pub async fn serve<E: Extension>(ext: E) -> Result<()> {
    ExtensionServe::new(ext).run().await
}

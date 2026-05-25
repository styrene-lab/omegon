//! Omegon Extension SDK
//!
//! This crate provides a safe, versioned interface for building extensions for Omegon.
//! Extension developers depend on this crate with a locked version matching their
//! target Omegon release. The version constraint ensures compatibility.
//!
//! # Getting Started
//!
//! The fastest path to a working extension:
//!
//! ```bash
//! omegon extension init my-extension
//! cd my-extension
//! cargo build --release
//! omegon extension install .
//! ```
//!
//! This scaffolds a v1 extension with one tool and a manifest. Start there,
//! then read on for the full SDK surface.
//!
//! # Which Protocol Version?
//!
//! - **v1** — start here. Your extension responds to requests from Omegon.
//!   One trait, one `serve()` call. Covers tools, widgets, and resources.
//! - **v2** — use this when your extension needs to talk *back* to the host:
//!   progress notifications, sampling requests, or reading host state.
//!   Adds [`HostProxy`] and [`serve_v2()`].
//!
//! If you're unsure, use v1. You can migrate to v2 later by adding
//! `on_initialized()` and switching `serve()` to `serve_v2()`.
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
//! # Building a v1 Extension
//!
//! ```ignore
//! use omegon_extension::{Extension, serve};
//!
//! #[derive(Default)]
//! struct MyExtension;
//!
//! #[async_trait::async_trait]
//! impl Extension for MyExtension {
//!     fn name(&self) -> &str { "my-extension" }
//!     fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
//!
//!     async fn handle_rpc(&self, method: &str, params: serde_json::Value) -> omegon_extension::Result<serde_json::Value> {
//!         match method {
//!             "get_tools" => Ok(serde_json::json!([])),
//!             _ => Err(omegon_extension::Error::method_not_found(method)),
//!         }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     serve(MyExtension::default()).await.expect("failed to serve");
//! }
//! ```
//!
//! # Building a v2 Extension (bidirectional)
//!
//! ```ignore
//! use omegon_extension::{Extension, HostProxy, serve_v2};
//!
//! #[derive(Default)]
//! struct MyExtension {
//!     host: std::sync::OnceLock<HostProxy>,
//! }
//!
//! #[async_trait::async_trait]
//! impl Extension for MyExtension {
//!     fn name(&self) -> &str { "my-extension" }
//!     fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
//!
//!     async fn handle_rpc(&self, method: &str, params: serde_json::Value) -> omegon_extension::Result<serde_json::Value> {
//!         match method {
//!             "get_tools" => Ok(serde_json::json!([])),
//!             _ => Err(omegon_extension::Error::method_not_found(method)),
//!         }
//!     }
//!
//!     async fn on_initialized(&self, host: HostProxy) {
//!         let _ = self.host.set(host);
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     serve_v2(MyExtension::default()).await.expect("failed to serve");
//! }
//! ```

pub mod actions;
mod capabilities;
mod elicitation;
mod error;
mod extension;
mod host_actions;
mod manifest;
pub mod mcp_shim;
mod mind;
mod prompts;
mod resources;
mod rpc;
mod sampling;
mod streaming;

pub use capabilities::{
    Capabilities, ExtensionInfo, HostInfo, InitializeParams, InitializeResult, PROTOCOL_VERSION,
};
pub use elicitation::{ElicitationAction, ElicitationParams, ElicitationResult, ElicitationSource};
pub use error::{Error, ErrorCode, Result};
pub use extension::{Extension, HostProxy};
use extension::{ExtensionServe, MessageRouter};
pub use host_actions::{
    HostAction, HostActionError, HostActionExecution, HostActionOutcome, HostActionStatus,
    ToolContent, ToolResult,
};
pub use manifest::{
    ConfigField, ConfigFieldType, ExtensionManifest, HostActionPermissions, ManifestError,
    ManifestPermissions, TerminalCreatePermissions,
};
pub use mind::{
    AddFactResponse, Episode, Fact, FactOpResponse, GetMindResponse, LoadMindResponse,
    MindMetadata, StoreMindResponse,
};
pub use prompts::{
    GetPromptParams, GetPromptResult, ListPromptsParams, ListPromptsResult, Prompt, PromptArgument,
    PromptContent, PromptMessage,
};
pub use resources::{
    ListResourceTemplatesResult, ListResourcesParams, ListResourcesResult, ReadResourceParams,
    ReadResourceResult, Resource, ResourceContents, ResourceTemplate, SubscribeResourceParams,
};
pub use rpc::{RpcError, RpcIncoming, RpcMessage, RpcNotification, RpcRequest, RpcResponse};
pub use sampling::{
    CreateMessageParams, CreateMessageResult, ModelHint, ModelPreference, SamplingContent,
    SamplingMessage, SamplingRoute, TokenUsage,
};
pub use streaming::{CancelledParams, ProgressContent, ProgressReporter, ToolProgressParams};

/// Convenience type for RPC method results.
pub type RpcResult = Result<serde_json::Value>;

/// Serve an extension instance over RPC (stdin/stdout) — v1 protocol.
///
/// Simple request/response loop. Extension cannot send notifications or
/// requests back to the host. Blocks until the extension shuts down.
pub async fn serve<E: Extension>(ext: E) -> Result<()> {
    ExtensionServe::new(ext).run().await
}

/// Serve an extension instance over RPC (stdin/stdout) — v2 protocol.
///
/// Bidirectional message router. Extension receives a [`HostProxy`] via
/// [`Extension::on_initialized()`] and can use it to send notifications
/// and requests back to the host. Blocks until the extension shuts down.
pub async fn serve_v2<E: Extension + 'static>(ext: E) -> Result<()> {
    MessageRouter::new(ext).run().await
}

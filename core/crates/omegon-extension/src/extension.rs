//! Extension trait — what developers implement to build extensions.

use crate::rpc::{RpcMessage, RpcRequest, RpcResponse};
use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// Extension trait — implement this to create an extension.
/// 
/// The extension SDK will handle all RPC protocol details. You only need
/// to implement method dispatch and return JSON results.
#[async_trait]
pub trait Extension: Send + Sync {
    /// Extension name (must match manifest).
    fn name(&self) -> &str;

    /// Extension version (should match manifest).
    fn version(&self) -> &str;

    /// Handle an RPC method call.
    /// 
    /// Return `Ok(Value)` on success, or an `Err(Error)` with a typed error code.
    /// Unknown methods should return `Error::method_not_found(method)`.
    ///
    /// # Safety
    ///
    /// - Parameter validation happens here. Return `Error::invalid_params()` if args don't make sense.
    /// - Panics in this method will crash the extension (not omegon).
    /// - Timeouts are enforced by the parent process — don't block indefinitely.
    async fn handle_rpc(&self, method: &str, params: Value) -> crate::Result<Value>;
}

/// Extension serving loop. Created by [`crate::serve()`], runs until shutdown.
pub struct ExtensionServe<E: Extension> {
    ext: E,
}

impl<E: Extension> ExtensionServe<E> {
    pub fn new(ext: E) -> Self {
        Self { ext }
    }

    /// Run the extension serving loop.
    pub async fn run(self) -> crate::Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let mut reader = tokio::io::BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);

        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;

            // EOF
            if n == 0 {
                return Ok(());
            }

            // Parse incoming RPC message
            let msg: RpcMessage = match serde_json::from_str(line.trim()) {
                Ok(msg) => msg,
                Err(e) => {
                    let error_response = RpcResponse::error(
                        None,
                        crate::ErrorCode::ParseError,
                        e.to_string(),
                    );
                    let response_json = serde_json::to_string(&error_response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                    continue;
                }
            };

            match msg {
                RpcMessage::Request(req) => {
                    let response = self.handle_request(&req).await;
                    let response_json = serde_json::to_string(&response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                }
                RpcMessage::Notification(_notif) => {
                    // Ignore notifications — extension doesn't process them
                }
            }
        }
    }

    async fn handle_request(&self, req: &RpcRequest) -> RpcResponse {
        let result = self.ext.handle_rpc(&req.method, req.params.clone()).await;

        match result {
            Ok(value) => RpcResponse::success(req.id.clone(), value),
            Err(e) => RpcResponse::error(req.id.clone(), e.code(), e.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestExtension;

    #[async_trait]
    impl Extension for TestExtension {
        fn name(&self) -> &str {
            "test-extension"
        }

        fn version(&self) -> &str {
            "0.1.0"
        }

        async fn handle_rpc(&self, method: &str, _params: Value) -> crate::Result<Value> {
            match method {
                "echo" => Ok(serde_json::json!({"status": "ok"})),
                "get_tools" => Ok(serde_json::json!([])),
                _ => Err(crate::Error::method_not_found(method)),
            }
        }
    }

    #[tokio::test]
    async fn test_extension_dispatch() {
        let ext = TestExtension;

        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            method: "echo".to_string(),
            params: serde_json::json!({}),
        };

        let serve = ExtensionServe::new(ext);
        let response = serve.handle_request(&req).await;

        assert_eq!(response.id, Some("1".to_string()));
        assert!(response.result.is_some());
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let ext = TestExtension;

        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            method: "unknown".to_string(),
            params: serde_json::json!({}),
        };

        let serve = ExtensionServe::new(ext);
        let response = serve.handle_request(&req).await;

        assert_eq!(response.id, Some("1".to_string()));
        assert!(response.error.is_some());
    }
}

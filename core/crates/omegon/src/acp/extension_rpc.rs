//! Narrow ACP extension invocation seam.
//!
//! Production continues to use [`ExtensionPollingHandle`], while ACP tests can
//! inject an in-memory invoker without spawning an extension process.

use std::sync::Arc;

use serde_json::Value;

#[async_trait::async_trait]
pub(super) trait ExtensionRpcInvoker: Send + Sync {
    fn extension_name(&self) -> &str;

    async fn rpc_call(&self, method: &str, params: Value) -> anyhow::Result<Value>;
}

#[async_trait::async_trait]
impl ExtensionRpcInvoker for crate::extensions::ExtensionPollingHandle {
    fn extension_name(&self) -> &str {
        self.extension_name()
    }

    async fn rpc_call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.rpc_call(method, params).await
    }
}

pub(super) type ExtensionRpcHandle = Arc<dyn ExtensionRpcInvoker>;

pub(super) fn erase_extension_rpc_handles(
    handles: std::collections::BTreeMap<String, crate::extensions::ExtensionPollingHandle>,
) -> std::collections::BTreeMap<String, ExtensionRpcHandle> {
    handles
        .into_iter()
        .map(|(name, handle)| (name, Arc::new(handle) as ExtensionRpcHandle))
        .collect()
}

pub(super) async fn call_extension_rpc(
    handles: &std::cell::RefCell<std::collections::BTreeMap<String, ExtensionRpcHandle>>,
    extension: &str,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    if method.trim().is_empty() {
        anyhow::bail!("invalid_request: 'method' field must not be empty");
    }
    let handle = handles.borrow().get(extension).cloned();
    let Some(handle) = handle else {
        anyhow::bail!(
            "extension_not_loaded: extension '{extension}' is not loaded or is not callable"
        );
    };
    handle
        .rpc_call(method, params)
        .await
        .map_err(|err| anyhow::anyhow!("method_failed: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct FakeInvoker {
        calls: Arc<Mutex<Vec<(String, Value)>>>,
        result: anyhow::Result<Value>,
    }

    #[async_trait::async_trait]
    impl ExtensionRpcInvoker for FakeInvoker {
        fn extension_name(&self) -> &str {
            "fake"
        }

        async fn rpc_call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((method.to_string(), params));
            match &self.result {
                Ok(value) => Ok(value.clone()),
                Err(err) => Err(anyhow::anyhow!(err.to_string())),
            }
        }
    }

    #[tokio::test]
    async fn invokes_fake_with_method_and_params() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let handles = std::cell::RefCell::new(std::collections::BTreeMap::from([(
            "fake".to_string(),
            Arc::new(FakeInvoker {
                calls: calls.clone(),
                result: Ok(serde_json::json!({"pong": true})),
            }) as ExtensionRpcHandle,
        )]));

        let result = call_extension_rpc(&handles, "fake", "ping", serde_json::json!({"value": 7}))
            .await
            .expect("RPC succeeds");

        assert_eq!(result, serde_json::json!({"pong": true}));
        assert_eq!(
            *calls.lock().expect("calls lock"),
            vec![("ping".to_string(), serde_json::json!({"value": 7}))]
        );
    }

    #[tokio::test]
    async fn maps_fake_failure_to_method_failed() {
        let handles = std::cell::RefCell::new(std::collections::BTreeMap::from([(
            "fake".to_string(),
            Arc::new(FakeInvoker {
                calls: Arc::new(Mutex::new(Vec::new())),
                result: Err(anyhow::anyhow!("transport unavailable")),
            }) as ExtensionRpcHandle,
        )]));

        let error = call_extension_rpc(&handles, "fake", "ping", serde_json::json!({}))
            .await
            .expect_err("RPC fails");

        assert_eq!(error.to_string(), "method_failed: transport unavailable");
    }
}

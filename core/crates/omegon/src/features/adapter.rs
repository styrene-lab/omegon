//! Feature adapters — wraps ToolProvider / ContextProvider implementations
//! as Feature trait objects for EventBus registration.
//!
//! These adapters let any crate implementing ToolProvider or ContextProvider
//! (e.g., omegon-memory) participate in the bus without implementing Feature directly.

use async_trait::async_trait;
use omegon_traits::{
    ContextInjection, ContextSignals, Feature, ToolDefinition, ToolProgressSink, ToolResult,
};
use serde_json::Value;

/// Wraps a ToolProvider as a Feature.
pub struct ToolAdapter {
    name: String,
    provider: Box<dyn omegon_traits::ToolProvider>,
}

impl ToolAdapter {
    pub fn new(name: impl Into<String>, provider: Box<dyn omegon_traits::ToolProvider>) -> Self {
        Self {
            name: name.into(),
            provider,
        }
    }
}

#[async_trait]
impl Feature for ToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.provider.tools()
    }

    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        self.provider
            .execute(tool_name, call_id, args, cancel)
            .await
    }

    async fn execute_with_sink(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: ToolProgressSink,
    ) -> anyhow::Result<ToolResult> {
        self.provider
            .execute_with_sink(tool_name, call_id, args, cancel, sink)
            .await
    }
}

/// Wraps a ContextProvider as a Feature.
pub struct ContextAdapter {
    name: String,
    provider: Box<dyn omegon_traits::ContextProvider>,
}

impl ContextAdapter {
    pub fn new(name: impl Into<String>, provider: Box<dyn omegon_traits::ContextProvider>) -> Self {
        Self {
            name: name.into(),
            provider,
        }
    }
}

#[async_trait]
impl Feature for ContextAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        self.provider.provide_context(signals)
    }
}

/// Wraps a type that implements BOTH ToolProvider + ContextProvider as a single Feature.
pub struct ToolContextAdapter {
    name: String,
    tool_provider: Box<dyn omegon_traits::ToolProvider>,
    context_provider: Option<Box<dyn omegon_traits::ContextProvider>>,
}

impl ToolContextAdapter {
    pub fn new(
        name: impl Into<String>,
        tool_provider: Box<dyn omegon_traits::ToolProvider>,
        context_provider: Option<Box<dyn omegon_traits::ContextProvider>>,
    ) -> Self {
        Self {
            name: name.into(),
            tool_provider,
            context_provider,
        }
    }
}

#[async_trait]
impl Feature for ToolContextAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.tool_provider.tools()
    }

    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        self.tool_provider
            .execute(tool_name, call_id, args, cancel)
            .await
    }

    async fn execute_with_sink(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: ToolProgressSink,
    ) -> anyhow::Result<ToolResult> {
        self.tool_provider
            .execute_with_sink(tool_name, call_id, args, cancel, sink)
            .await
    }

    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        self.context_provider.as_ref()?.provide_context(signals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct DummyTool;

    #[async_trait]
    impl omegon_traits::ToolProvider for DummyTool {
        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "dummy".into(),
                label: "dummy".into(),
                description: "test".into(),
                parameters: json!({"type": "object"}),
            }]
        }
        async fn execute(
            &self,
            _: &str,
            _: &str,
            _: Value,
            _: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
                details: json!(null),
            })
        }
    }

    #[test]
    fn tool_adapter_wraps_provider() {
        let feature = ToolAdapter::new("test", Box::new(DummyTool));
        assert_eq!(feature.name(), "test");
        assert_eq!(feature.tools().len(), 1);
        assert_eq!(feature.tools()[0].name, "dummy");
    }

    #[tokio::test]
    async fn tool_adapter_executes() {
        let feature = ToolAdapter::new("test", Box::new(DummyTool));
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("dummy", "tc1", json!({}), cancel)
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text().unwrap(), "ok");
    }
}

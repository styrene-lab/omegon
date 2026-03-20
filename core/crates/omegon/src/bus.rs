//! EventBus — typed coordination layer between the agent loop and features.
//!
//! The bus is the backbone of feature integration. Events flow down from the
//! agent loop to features; requests flow up from features to the runtime.
//!
//! ```text
//! Agent Loop
//!   │
//!   ├─emit(BusEvent)──→ EventBus ──deliver──→ Feature::on_event(&mut self)
//!   │                       │                          │
//!   │                       │                  BusRequest (accumulated)
//!   │                       │                          │
//!   │                       ←── drain_requests() ──────┘
//!   │
//!   └─ handle requests (inject message, notify, compact)
//! ```
//!
//! # Concurrency model
//!
//! The bus is NOT thread-safe. It lives in the agent loop task and processes
//! events synchronously. Features get `&mut self` — no interior mutability
//! needed. The TUI receives events via a separate `tokio::broadcast` channel.

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContextInjection,
    ContextSignals, Feature, ToolDefinition,
};
use serde_json::Value;

/// The event bus — owns all features and dispatches events to them.
pub struct EventBus {
    features: Vec<Box<dyn Feature>>,
    /// Accumulated requests from the most recent event delivery.
    pending_requests: Vec<BusRequest>,
    /// Cached tool definitions — rebuilt when features change.
    tool_defs: Vec<(usize, ToolDefinition)>, // (feature_index, def)
    /// Cached command definitions.
    command_defs: Vec<(usize, CommandDefinition)>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            features: Vec::new(),
            pending_requests: Vec::new(),
            tool_defs: Vec::new(),
            command_defs: Vec::new(),
        }
    }

    /// Register a feature. Call during setup before the agent loop starts.
    pub fn register(&mut self, feature: Box<dyn Feature>) {
        tracing::info!(feature = feature.name(), "registered feature");
        self.features.push(feature);
    }

    /// Finalize registration — cache tool and command definitions.
    /// Call after all features are registered, before the agent loop starts.
    pub fn finalize(&mut self) {
        self.tool_defs.clear();
        self.command_defs.clear();

        for (idx, feature) in self.features.iter().enumerate() {
            for def in feature.tools() {
                tracing::debug!(feature = feature.name(), tool = %def.name, "registered tool");
                self.tool_defs.push((idx, def));
            }
            for cmd in feature.commands() {
                tracing::debug!(feature = feature.name(), command = %cmd.name, "registered command");
                self.command_defs.push((idx, cmd));
            }
        }

        tracing::info!(
            features = self.features.len(),
            tools = self.tool_defs.len(),
            commands = self.command_defs.len(),
            "event bus finalized"
        );
    }

    // ─── Event delivery ─────────────────────────────────────────────

    /// Deliver an event to all features. Requests are accumulated
    /// and can be drained with `drain_requests()`.
    pub fn emit(&mut self, event: &BusEvent) {
        for feature in &mut self.features {
            let requests = feature.on_event(event);
            self.pending_requests.extend(requests);
        }
    }

    /// Drain accumulated requests from the most recent event deliveries.
    pub fn drain_requests(&mut self) -> Vec<BusRequest> {
        std::mem::take(&mut self.pending_requests)
    }

    // ─── Tool dispatch ──────────────────────────────────────────────

    /// All tool definitions across all features.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_defs.iter().map(|(_, d)| d.clone()).collect()
    }

    /// Find which feature owns a tool and execute it.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                return self.features[*idx]
                    .execute(tool_name, call_id, args, cancel)
                    .await;
            }
        }
        anyhow::bail!("no feature provides tool '{tool_name}'")
    }

    // ─── Context injection ──────────────────────────────────────────

    /// Collect context injections from all features.
    pub fn collect_context(&self, signals: &ContextSignals<'_>) -> Vec<ContextInjection> {
        self.features
            .iter()
            .filter_map(|f| f.provide_context(signals))
            .collect()
    }

    // ─── Command dispatch ───────────────────────────────────────────

    /// All registered command definitions (for the command palette).
    pub fn command_definitions(&self) -> &[(usize, CommandDefinition)] {
        &self.command_defs
    }

    /// Dispatch a slash command to the feature that owns it.
    /// Returns the result from the first feature that handles it.
    pub fn dispatch_command(&mut self, name: &str, args: &str) -> CommandResult {
        // Find features that registered this command and try them
        let owning_indices: Vec<usize> = self.command_defs
            .iter()
            .filter(|(_, def)| def.name == name)
            .map(|(idx, _)| *idx)
            .collect();

        for idx in owning_indices {
            let result = self.features[idx].handle_command(name, args);
            if !matches!(result, CommandResult::NotHandled) {
                return result;
            }
        }
        CommandResult::NotHandled
    }

    // ─── Introspection ──────────────────────────────────────────────

    /// Number of registered features.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Feature names for logging/debugging.
    pub fn feature_names(&self) -> Vec<&str> {
        self.features.iter().map(|f| f.name()).collect()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use omegon_traits::{Feature, ToolDefinition, ToolResult, ContentBlock};
    use serde_json::json;

    /// Test feature that counts events and provides a tool.
    struct CounterFeature {
        event_count: u32,
    }

    #[async_trait]
    impl Feature for CounterFeature {
        fn name(&self) -> &str { "counter" }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "count".into(),
                label: "count".into(),
                description: "Returns the event count".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }]
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _call_id: &str,
            _args: serde_json::Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("count: {}", self.event_count),
                }],
                details: json!(null),
            })
        }

        fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
            self.event_count += 1;
            vec![]
        }
    }

    /// Feature that emits requests on specific events.
    struct NotifierFeature;

    #[async_trait]
    impl Feature for NotifierFeature {
        fn name(&self) -> &str { "notifier" }

        fn commands(&self) -> Vec<CommandDefinition> {
            vec![CommandDefinition {
                name: "notify".into(),
                description: "Send a test notification".into(),
                subcommands: vec![],
            }]
        }

        fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
            if name == "notify" {
                CommandResult::Display(format!("Notified: {args}"))
            } else {
                CommandResult::NotHandled
            }
        }

        fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
            if matches!(event, BusEvent::SessionEnd { .. }) {
                vec![BusRequest::Notify {
                    message: "Session ended".into(),
                    level: omegon_traits::NotifyLevel::Info,
                }]
            } else {
                vec![]
            }
        }
    }

    #[test]
    fn register_and_finalize() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        assert_eq!(bus.feature_count(), 2);
        assert_eq!(bus.tool_definitions().len(), 1);
        assert_eq!(bus.command_definitions().len(), 1);
    }

    #[test]
    fn event_delivery_is_sequential() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        bus.emit(&BusEvent::TurnStart { turn: 1 });
        bus.emit(&BusEvent::TurnEnd { turn: 1 });

        // Both features should have received both events
        // (Can't inspect directly, but drain_requests would show nothing)
        let requests = bus.drain_requests();
        assert!(requests.is_empty());
    }

    #[test]
    fn requests_accumulated_from_events() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        // No requests from TurnStart
        bus.emit(&BusEvent::TurnStart { turn: 1 });
        assert!(bus.drain_requests().is_empty());

        // SessionEnd triggers a notification request
        bus.emit(&BusEvent::SessionEnd {
            turns: 1,
            tool_calls: 0,
            duration_secs: 10.0,
        });
        let requests = bus.drain_requests();
        assert_eq!(requests.len(), 1);
        assert!(matches!(&requests[0], BusRequest::Notify { message, .. } if message == "Session ended"));
    }

    #[test]
    fn command_dispatch() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        let result = bus.dispatch_command("notify", "hello");
        assert!(matches!(result, CommandResult::Display(msg) if msg.contains("hello")));

        let result = bus.dispatch_command("nonexistent", "");
        assert!(matches!(result, CommandResult::NotHandled));
    }

    #[tokio::test]
    async fn tool_execution() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 42 }));
        bus.finalize();

        let cancel = tokio_util::sync::CancellationToken::new();
        let result = bus.execute_tool("count", "tc1", json!({}), cancel).await.unwrap();
        assert_eq!(result.content[0].as_text().unwrap(), "count: 42");
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let bus = EventBus::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        let err = bus.execute_tool("nonexistent", "tc1", json!({}), cancel).await;
        assert!(err.is_err());
    }

    #[test]
    fn feature_names() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(NotifierFeature));

        let names = bus.feature_names();
        assert_eq!(names, vec!["counter", "notifier"]);
    }

    #[test]
    fn drain_clears_requests() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        bus.emit(&BusEvent::SessionEnd { turns: 1, tool_calls: 0, duration_secs: 1.0 });
        assert_eq!(bus.drain_requests().len(), 1);
        // Second drain should be empty
        assert!(bus.drain_requests().is_empty());
    }
}

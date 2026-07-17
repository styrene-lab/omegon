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

use std::collections::HashMap;
use std::time::Duration;

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContextInjection, ContextSignals,
    Feature, ToolDefinition,
};
use serde_json::Value;

/// Core tools that are always present regardless of lazy injection.
/// These are the coding loop essentials — the tools the model needs on every turn.
fn is_core_tool(name: &str) -> bool {
    use crate::tool_registry as reg;
    matches!(
        name,
        reg::core::BASH
            | reg::core::READ
            | reg::core::WRITE
            | reg::core::EDIT
            | reg::core::VALIDATE
            | reg::core::COMMIT
            | reg::core::TERMINAL
            | reg::codescan::CODEBASE_SEARCH
            | reg::context::CONTEXT_STATUS
            | reg::context::REQUEST_CONTEXT
            | reg::manage_tools::MANAGE_TOOLS
            | reg::view::VIEW
    )
}

/// Dynamically registered tools come from runtime-discovered surfaces such as
/// native extensions, MCP servers, and plugin manifests. Keep them visible after
/// turn 1 so operators can ask for an installed extension by name without first
/// forcing a `manage_tools` or exact tool call.
fn is_dynamic_tool(name: &str) -> bool {
    !crate::tool_registry::all_static_names().contains(&name)
}

/// Tools registered in the runtime but hidden from the model-facing tool surface.
fn is_model_hidden_tool(name: &str) -> bool {
    use crate::tool_registry as reg;
    matches!(name, reg::core::CHANGE)
}

/// Strip `description` fields from tool parameter schemas to reduce token overhead.
/// Preserves type, enum, required, default, items — the structural information
/// models need to form correct tool calls. Reduces schema tokens by ~30-40%.
fn compact_tool_schema(def: &ToolDefinition) -> ToolDefinition {
    fn strip_descriptions(val: &Value) -> Value {
        match val {
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (key, value) in map {
                    if key == "description" {
                        continue; // strip parameter descriptions
                    }
                    out.insert(key.clone(), strip_descriptions(value));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(strip_descriptions).collect()),
            other => other.clone(),
        }
    }

    ToolDefinition {
        name: def.name.clone(),
        label: def.label.clone(),
        // Keep the top-level tool description (model needs to know what the tool does)
        // but strip parameter-level descriptions (model can infer from param names + types)
        description: def.description.clone(),
        parameters: strip_descriptions(&def.parameters),
        capabilities: def.capabilities.clone(),
    }
}

/// Default tool execution timeout (5 minutes).
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(300);
/// Absolute ceiling — no tool may run longer than 10 minutes, matching the
/// bash tool schema's max timeout (600000ms). Prevents infinite hangs even
/// if the model requests an absurd value.
const MAX_TOOL_TIMEOUT: Duration = Duration::from_secs(600);

/// The event bus — owns all features and dispatches events to them.
pub struct EventBus {
    features: Vec<Box<dyn Feature>>,
    /// Accumulated requests from the most recent event delivery.
    pending_requests: Vec<BusRequest>,
    /// Cached tool definitions — rebuilt when features change.
    tool_defs: Vec<(usize, ToolDefinition)>, // (feature_index, def)
    /// Cached command definitions.
    command_defs: Vec<(usize, CommandDefinition)>,
    /// Handle to the disabled tools set from ManageTools.
    disabled_tools: Option<crate::features::manage_tools::DisabledTools>,
    /// Handle to the registered tool inventory from ManageTools.
    tool_inventory: Option<crate::features::manage_tools::ToolInventory>,
    /// Per-tool execution timeouts. Tools not listed use DEFAULT_TOOL_TIMEOUT.
    tool_timeouts: HashMap<String, Duration>,
    /// Internal tool owners — maps tool names that may NOT be in tool_defs
    /// (because they're not LLM-visible) to the feature index that handles them.
    /// Populated explicitly via `register_internal_tool`.
    internal_tool_owners: HashMap<String, usize>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            features: Vec::new(),
            pending_requests: Vec::new(),
            tool_defs: Vec::new(),
            command_defs: Vec::new(),
            disabled_tools: None,
            internal_tool_owners: HashMap::new(),
            tool_inventory: None,
            tool_timeouts: HashMap::from([
                ("bash".into(), Duration::from_secs(600)),
                ("web_search".into(), Duration::from_secs(30)),
                ("web_fetch".into(), Duration::from_secs(60)),
            ]),
        }
    }

    pub fn apply_operator_tool_profile(
        &mut self,
        slim_mode: bool,
        posture_disabled: &[String],
        posture_enabled: &[String],
    ) {
        use crate::tool_registry as reg;
        let Some(handle) = self.disabled_tools.as_ref() else {
            return;
        };
        let mut disabled = handle.lock().unwrap();
        disabled.clear();

        // ── Base defaults (all modes): situational tools hidden from the
        // default schema. The agent discovers them via lazy injection if
        // it calls one after seeing the full surface on turn 1.
        disabled.insert(reg::persona::SWITCH_PERSONA.into());
        disabled.insert(reg::persona::SWITCH_TONE.into());
        disabled.insert(reg::persona::LIST_PERSONAS.into());
        disabled.insert(reg::auth::AUTH_STATUS.into());
        disabled.insert(reg::harness_settings::HARNESS_SETTINGS.into());
        disabled.insert(reg::memory::MEMORY_INGEST_LIFECYCLE.into());
        disabled.insert(reg::memory::MEMORY_CONNECT.into());
        disabled.insert(reg::memory::MEMORY_SEARCH_ARCHIVE.into());
        disabled.insert(reg::lifecycle::OPENSPEC_MANAGE.into());
        disabled.insert(reg::lifecycle::LIFECYCLE_DOCTOR.into());
        disabled.insert(reg::codescan::CODEBASE_INDEX.into());
        disabled.insert(reg::session_log::SESSION_LOG.into());
        disabled.insert(reg::model_budget::SET_MODEL_INTENT.into());
        disabled.insert(reg::model_budget::SWITCH_TO_OFFLINE_DRIVER.into());
        disabled.insert(reg::model_budget::SET_THINKING_LEVEL.into());

        if slim_mode {
            // Slim/explorator: additionally suppress delegation, orchestration,
            // lifecycle surfaces, and heavyweight tools beyond the base
            // defaults.  Hiding design_tree and openspec from the tool
            // list means the LLM cannot reference concepts the operator
            // hasn't learned yet (Cruise zone — see
            // design/junior-onramp-progressive-disclosure.md).
            disabled.insert(reg::delegate::DELEGATE.into());
            disabled.insert(reg::delegate::DELEGATE_RESULT.into());
            disabled.insert(reg::delegate::DELEGATE_STATUS.into());
            disabled.insert(reg::cleave::CLEAVE_ASSESS.into());
            disabled.insert(reg::cleave::CLEAVE_RUN.into());
            disabled.insert(reg::lifecycle::DESIGN_TREE.into());
            disabled.insert(reg::lifecycle::DESIGN_TREE_UPDATE.into());
            disabled.insert(reg::lifecycle::OPENSPEC_MANAGE.into());
            disabled.insert(reg::local_inference::LIST_LOCAL_MODELS.into());
            disabled.insert(reg::local_inference::MANAGE_OLLAMA.into());
            disabled.insert(reg::core::SERVE.into());
            disabled.insert(reg::view::VIEW.into());
            disabled.insert(reg::context::CONTEXT_COMPACT.into());
            disabled.insert(reg::context::CONTEXT_CLEAR.into());
        }

        // Custom posture tool overrides
        for tool in posture_disabled {
            disabled.insert(tool.clone());
        }

        // Whitelist mode: if posture_enabled is non-empty, disable everything
        // except the listed tools. This is applied last so it overrides all
        // other disable/enable decisions.
        if !posture_enabled.is_empty() {
            let all_tools: Vec<String> =
                self.tool_defs.iter().map(|(_, d)| d.name.clone()).collect();
            for tool in &all_tools {
                if !posture_enabled.contains(tool) {
                    disabled.insert(tool.clone());
                }
            }
            // Ensure enabled tools are NOT in the disabled set
            for tool in posture_enabled {
                disabled.remove(tool);
            }
        }
    }

    /// Set the disabled tools handle (called from setup after ManageTools is registered).
    pub fn set_disabled_tools(&mut self, handle: crate::features::manage_tools::DisabledTools) {
        self.disabled_tools = Some(handle);
    }

    /// Set the ManageTools inventory handle so finalize can keep its list in
    /// sync with the bus's current model-visible tool cache.
    pub fn set_tool_inventory(&mut self, handle: crate::features::manage_tools::ToolInventory) {
        self.tool_inventory = Some(handle);
        self.refresh_tool_inventory();
    }

    /// Register a feature. Call during setup before the agent loop starts.
    pub fn register(&mut self, feature: Box<dyn Feature>) {
        tracing::info!(feature = feature.name(), "registered feature");
        self.features.push(feature);
    }

    /// Replace an existing feature by name, or register it if absent.
    /// Call `finalize()` afterwards to rebuild cached command/tool definitions.
    pub fn replace_feature(&mut self, feature: Box<dyn Feature>) {
        let name = feature.name().to_string();
        if let Some(idx) = self.features.iter().position(|f| f.name() == name) {
            tracing::info!(feature = %name, "replaced feature");
            self.features[idx] = feature;
        } else {
            tracing::info!(feature = %name, "registered feature");
            self.features.push(feature);
        }
    }

    /// Register an internal tool name → feature mapping. Internal tools
    /// are NOT in the LLM-visible tool_defs but can be called via
    /// `execute_internal`. The feature_name must match a previously
    /// registered feature.
    pub fn register_internal_tool(&mut self, tool_name: &str, feature_name: &str) {
        if let Some(idx) = self.features.iter().position(|f| f.name() == feature_name) {
            tracing::debug!(
                tool = tool_name,
                feature = feature_name,
                "registered internal tool"
            );
            self.internal_tool_owners.insert(tool_name.to_string(), idx);
        } else {
            tracing::warn!(
                tool = tool_name,
                feature = feature_name,
                "cannot register internal tool — feature not found"
            );
        }
    }

    /// Finalize registration — cache tool and command definitions.
    /// Call after all features are registered, before the agent loop starts.
    /// Deduplicates tools by name (first registration wins) to prevent
    /// Anthropic API 400 "Tool names must be unique" errors.
    pub fn finalize(&mut self) {
        self.tool_defs.clear();
        self.command_defs.clear();

        let mut seen_tools = std::collections::HashSet::new();
        for (idx, feature) in self.features.iter().enumerate() {
            for def in feature.tools() {
                if seen_tools.contains(def.name.as_str()) {
                    tracing::warn!(
                        feature = feature.name(),
                        tool = %def.name,
                        "duplicate tool definition — skipping (first registration wins)"
                    );
                    continue;
                }
                tracing::debug!(feature = feature.name(), tool = %def.name, "registered tool");
                seen_tools.insert(def.name.clone());
                self.tool_defs.push((idx, def));
            }
            for cmd in feature.commands() {
                tracing::debug!(feature = feature.name(), command = %cmd.name, "registered command");
                self.command_defs.push((idx, cmd));
            }
        }

        self.refresh_tool_inventory();

        let tool_names: Vec<&str> = self
            .tool_defs
            .iter()
            .map(|(_, d)| d.name.as_str())
            .collect();
        tracing::info!(
            features = self.features.len(),
            tools = self.tool_defs.len(),
            commands = self.command_defs.len(),
            tool_names = ?tool_names,
            "event bus finalized"
        );
    }

    fn refresh_tool_inventory(&self) {
        let Some(handle) = &self.tool_inventory else {
            return;
        };
        let mut registered: Vec<String> = self
            .tool_defs
            .iter()
            .filter(|(_, def)| !is_model_hidden_tool(&def.name))
            .map(|(_, def)| def.name.clone())
            .collect();
        registered.sort();

        let mut callable: Vec<String> = self
            .tool_definitions_mode(false)
            .into_iter()
            .map(|def| def.name)
            .collect();
        callable.sort();

        if let Ok(mut inventory) = handle.lock() {
            *inventory = crate::features::manage_tools::ToolInventorySnapshot {
                registered,
                callable,
            };
        }
    }

    /// Visible tool names captured by ManageTools, ignoring disabled state.
    #[cfg(test)]
    fn tool_inventory_names(&self) -> Vec<String> {
        self.tool_inventory
            .as_ref()
            .and_then(|handle| {
                handle
                    .lock()
                    .ok()
                    .map(|snapshot| snapshot.registered.clone())
            })
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn callable_tool_inventory_names(&self) -> Vec<String> {
        self.tool_inventory
            .as_ref()
            .and_then(|handle| handle.lock().ok().map(|snapshot| snapshot.callable.clone()))
            .unwrap_or_default()
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

    /// Emit a HarnessStatusChanged event from an updated status snapshot.
    /// Also returns the serialized JSON for forwarding to AgentEvent broadcast.
    pub fn emit_harness_status(&mut self, status: &crate::status::HarnessStatus) -> Value {
        let status_json = serde_json::to_value(status).unwrap_or_default();
        self.emit(&BusEvent::HarnessStatusChanged {
            status_json: status_json.clone(),
        });
        status_json
    }

    // ─── Tool dispatch ──────────────────────────────────────────────

    /// All tool definitions across all features.
    /// When `compact` is true, strips parameter descriptions from JSON schemas
    /// to reduce token overhead (~30-40% savings on tool schema tokens).
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_definitions_mode(false)
    }

    pub fn has_tool(&self, tool_name: &str) -> bool {
        self.tool_defs.iter().any(|(_, def)| def.name == tool_name)
            || self.internal_tool_owners.contains_key(tool_name)
    }

    /// Authoritative producer for the feature that won tool-name arbitration.
    pub fn tool_provenance(&self, tool_name: &str) -> omegon_traits::ToolProvenance {
        let owner = self
            .tool_defs
            .iter()
            .find(|(_, def)| def.name == tool_name)
            .map(|(idx, _)| *idx)
            .or_else(|| self.internal_tool_owners.get(tool_name).copied());
        owner
            .and_then(|idx| self.features.get(idx))
            .map(|feature| feature.tool_provenance())
            .unwrap_or_default()
    }

    /// Tool definitions with optional schema compaction for token efficiency.
    pub fn tool_definitions_mode(&self, compact: bool) -> Vec<ToolDefinition> {
        let disabled = self.disabled_tools.as_ref().and_then(|d| d.lock().ok());
        self.tool_defs
            .iter()
            .filter(|(_, d)| disabled.as_ref().is_none_or(|set| !set.contains(&d.name)))
            .filter(|(_, d)| !is_model_hidden_tool(&d.name))
            .map(|(_, d)| {
                if compact {
                    compact_tool_schema(d)
                } else {
                    d.clone()
                }
            })
            .collect()
    }

    /// Lazy tool injection: returns a reduced tool set for token efficiency.
    ///
    /// - **Turn 1**: all enabled tools (model needs to see the full surface once)
    /// - **Turn 2+**: core tools always + extended tools only if previously used
    ///   or contextually relevant
    ///
    /// `used_tools` is the set of tool names called so far in this session.
    pub fn tool_definitions_lazy(
        &self,
        compact: bool,
        turn: u32,
        used_tools: &std::collections::HashSet<String>,
    ) -> Vec<ToolDefinition> {
        self.tool_definitions_lazy_inner(compact, turn, used_tools, false)
    }

    /// Like `tool_definitions_lazy` but with `core_only_turn1` = true, the
    /// first turn also only receives core tools. Use for constrained models
    /// (≤32B) where 50+ tool schemas overwhelm the context window.
    pub fn tool_definitions_lean(
        &self,
        turn: u32,
        used_tools: &std::collections::HashSet<String>,
    ) -> Vec<ToolDefinition> {
        self.tool_definitions_lazy_inner(true, turn, used_tools, true)
    }

    fn tool_definitions_lazy_inner(
        &self,
        compact: bool,
        turn: u32,
        used_tools: &std::collections::HashSet<String>,
        core_only_turn1: bool,
    ) -> Vec<ToolDefinition> {
        // Turn 1: full surface so the model knows what's available —
        // unless core_only_turn1 is set (constrained models with small ctx).
        if turn <= 1 && !core_only_turn1 {
            return self.tool_definitions_mode(compact);
        }

        let disabled = self.disabled_tools.as_ref().and_then(|d| d.lock().ok());
        self.tool_defs
            .iter()
            .filter(|(_, d)| disabled.as_ref().is_none_or(|set| !set.contains(&d.name)))
            .filter(|(_, d)| !is_model_hidden_tool(&d.name))
            .filter(|(_, d)| {
                is_core_tool(&d.name) || is_dynamic_tool(&d.name) || used_tools.contains(&d.name)
            })
            .map(|(_, d)| {
                if compact {
                    compact_tool_schema(d)
                } else {
                    d.clone()
                }
            })
            .collect()
    }

    /// All registered tool definitions, ignoring disabled state.
    /// Used for the manage_tools list command.
    #[allow(dead_code)]
    pub fn all_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_defs
            .iter()
            .filter(|(_, d)| !is_model_hidden_tool(&d.name))
            .map(|(_, d)| d.clone())
            .collect()
    }

    /// Returns true when a tool is registered in the runtime, including
    /// model-hidden tools and explicit internal tool owners.
    pub fn has_registered_tool(&self, tool_name: &str) -> bool {
        self.tool_defs.iter().any(|(_, d)| d.name == tool_name)
            || self.internal_tool_owners.contains_key(tool_name)
    }

    /// Find which feature owns a tool and execute it.
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        self.execute_tool_with_sink(
            tool_name,
            call_id,
            args,
            cancel,
            omegon_traits::ToolProgressSink::noop(),
        )
        .await
    }

    /// Like [`Self::execute_tool`] but also passes a `ToolProgressSink` so the
    /// runner can stream partial output. The dispatch loop in `loop.rs` uses
    /// this path; other call sites that just want a final result keep using
    /// [`Self::execute_tool`] (which constructs a no-op sink).
    pub async fn execute_tool_with_sink(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: omegon_traits::ToolProgressSink,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let default_timeout = self
            .tool_timeouts
            .get(tool_name)
            .copied()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT);

        // If the tool call includes a timeout parameter (bash: seconds),
        // use it instead of the hardcoded default — clamped to MAX_TOOL_TIMEOUT
        // so a runaway tool can't hang the agent forever. The tool's internal
        // timeout still applies (and should fire first for graceful cleanup),
        // but the bus layer must not silently kill the tool before the requested
        // timeout expires. Add 5s grace so the tool's own timeout fires first
        // with a clean error message rather than the bus's blunt cancellation.
        let timeout = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .map(|secs| Duration::from_secs(secs + 5).min(MAX_TOOL_TIMEOUT))
            .filter(|t| *t > default_timeout)
            .unwrap_or(default_timeout);

        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                return match tokio::time::timeout(
                    timeout,
                    self.features[*idx].execute_with_context(
                        tool_name,
                        call_id,
                        args,
                        cancel,
                        sink,
                        omegon_traits::ToolExecutionContext::default(),
                    ),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        tracing::error!(
                            tool = tool_name,
                            timeout_secs = timeout.as_secs(),
                            "tool execution timed out"
                        );
                        Ok(omegon_traits::ToolResult {
                            content: vec![omegon_traits::ContentBlock::Text {
                                text: format!(
                                    "Tool '{}' timed out after {} seconds. \
                                     The operation was cancelled.",
                                    tool_name,
                                    timeout.as_secs()
                                ),
                            }],
                            details: serde_json::json!({"is_error": true}),
                        })
                    }
                };
            }
        }
        anyhow::bail!("no feature provides tool '{tool_name}'")
    }

    /// Execute a tool with a host interaction context. Used by ACP-hosted
    /// sessions to route operator approval requests back to the client.
    pub async fn execute_tool_with_context(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
        sink: omegon_traits::ToolProgressSink,
        context: omegon_traits::ToolExecutionContext,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        let default_timeout = self
            .tool_timeouts
            .get(tool_name)
            .copied()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT);
        let timeout = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .map(|secs| Duration::from_secs(secs + 5).min(MAX_TOOL_TIMEOUT))
            .filter(|t| *t > default_timeout)
            .unwrap_or(default_timeout);

        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                return match tokio::time::timeout(
                    timeout,
                    self.features[*idx]
                        .execute_with_context(tool_name, call_id, args, cancel, sink, context),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_elapsed) => Ok(omegon_traits::ToolResult {
                        content: vec![omegon_traits::ContentBlock::Text {
                            text: format!(
                                "Tool '{}' timed out after {} seconds. The operation was cancelled.",
                                tool_name,
                                timeout.as_secs()
                            ),
                        }],
                        details: serde_json::json!({"is_error": true}),
                    }),
                };
            }
        }
        anyhow::bail!("no feature provides tool '{tool_name}'")
    }

    /// Execute an internal tool that may not be in the LLM-visible tool_defs.
    ///
    /// Execute an internal tool that may not be in the LLM-visible tool_defs.
    ///
    /// Unlike `execute_tool`, this doesn't require the tool to be in the
    /// registered definitions. It uses the `internal_tool_owners` map
    /// populated at registration time to find the owning feature directly.
    pub async fn execute_internal(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_traits::ToolResult> {
        // Check the internal tool owner map first
        if let Some(&idx) = self.internal_tool_owners.get(tool_name) {
            return self.features[idx]
                .execute(tool_name, call_id, args, cancel)
                .await;
        }
        // Fallback: check tool_defs (tool might be registered but disabled)
        for (idx, def) in &self.tool_defs {
            if def.name == tool_name {
                return self.features[*idx]
                    .execute(tool_name, call_id, args, cancel)
                    .await;
            }
        }
        anyhow::bail!("no feature handles internal tool '{tool_name}'")
    }

    /// Get the configured timeout for a tool.
    pub fn tool_timeout(&self, tool_name: &str) -> Duration {
        self.tool_timeouts
            .get(tool_name)
            .copied()
            .unwrap_or(DEFAULT_TOOL_TIMEOUT)
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
        let owning_indices: Vec<usize> = self
            .command_defs
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
    use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};
    use serde_json::json;

    /// Test feature that counts events and provides a tool.
    struct CounterFeature {
        event_count: u32,
    }

    #[async_trait]
    impl Feature for CounterFeature {
        fn name(&self) -> &str {
            "counter"
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "count".into(),
                label: "count".into(),
                description: "Returns the event count".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
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
        fn name(&self) -> &str {
            "notifier"
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            vec![CommandDefinition {
                name: "notify".into(),
                description: "Send a test notification".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
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

    struct ExtensionCounterFeature;

    #[async_trait]
    impl Feature for ExtensionCounterFeature {
        fn name(&self) -> &str {
            "recro-coe-agent"
        }

        fn tool_provenance(&self) -> omegon_traits::ToolProvenance {
            omegon_traits::ToolProvenance::Extension {
                name: self.name().into(),
            }
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "count".into(),
                label: "count".into(),
                description: "Extension override".into(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
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
                    text: "extension".into(),
                }],
                details: json!(null),
            })
        }
    }

    #[test]
    fn resolved_tool_provenance_tracks_the_collision_winner() {
        let mut bus = EventBus::new();
        bus.register(Box::new(ExtensionCounterFeature));
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        assert_eq!(
            bus.tool_provenance("count"),
            omegon_traits::ToolProvenance::Extension {
                name: "recro-coe-agent".into(),
            }
        );
        assert_eq!(
            bus.tool_provenance("unknown"),
            omegon_traits::ToolProvenance::BuiltIn
        );
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
        bus.emit(&BusEvent::TurnEnd(Box::new(
            omegon_traits::BusEventTurnEnd {
                turn: 1,
                model: None,
                provider: None,
                estimated_tokens: 0,
                context_window: 200_000,
                context_composition: omegon_traits::ContextComposition::default(),
                actual_input_tokens: 0,
                actual_output_tokens: 0,
                cache_read_tokens: 0,
                provider_telemetry: None,
                dominant_phase: None,
                drift_kind: None,
                progress_signal: omegon_traits::ProgressSignal::None,
            },
        )));

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
            initial_prompt: None,
            outcome_summary: None,
        });
        let requests = bus.drain_requests();
        assert_eq!(requests.len(), 1);
        assert!(
            matches!(&requests[0], BusRequest::Notify { message, .. } if message == "Session ended")
        );
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
        let result = bus
            .execute_tool("count", "tc1", json!({}), cancel)
            .await
            .unwrap();
        assert_eq!(result.content[0].as_text().unwrap(), "count: 42");
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let bus = EventBus::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        let err = bus
            .execute_tool("nonexistent", "tc1", json!({}), cancel)
            .await;
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
    fn finalize_deduplicates_tools() {
        let mut bus = EventBus::new();
        // Register two features that both provide "count"
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        // Should have only 1 tool (deduped), not 2
        assert_eq!(bus.tool_definitions().len(), 1);
    }

    #[test]
    fn set_tool_inventory_tracks_finalized_tools() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));

        let inventory = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolInventorySnapshot::default(),
        ));
        bus.set_tool_inventory(inventory);
        assert!(bus.tool_inventory_names().is_empty());

        bus.finalize();
        assert_eq!(bus.tool_inventory_names(), vec!["count".to_string()]);
        assert_eq!(
            bus.callable_tool_inventory_names(),
            vec!["count".to_string()]
        );
    }

    #[test]
    fn set_tool_inventory_tracks_disabled_tools_as_not_callable() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        let disabled = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        bus.set_disabled_tools(disabled.clone());
        let inventory = std::sync::Arc::new(std::sync::Mutex::new(
            crate::features::manage_tools::ToolInventorySnapshot::default(),
        ));
        bus.set_tool_inventory(inventory);

        disabled.lock().unwrap().insert("count".to_string());
        bus.finalize();

        assert_eq!(bus.tool_inventory_names(), vec!["count".to_string()]);
        assert!(bus.callable_tool_inventory_names().is_empty());
    }

    #[test]
    fn disabled_tools_filtered_from_definitions() {
        let mut bus = EventBus::new();
        bus.register(Box::new(CounterFeature { event_count: 0 }));
        bus.finalize();

        // Before disabling: tool is present
        assert_eq!(bus.tool_definitions().len(), 1);
        assert_eq!(bus.all_tool_definitions().len(), 1);

        // Disable the tool
        let disabled =
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::from([
                "count".to_string(),
            ])));
        bus.set_disabled_tools(disabled);

        // After disabling: filtered from tool_definitions but still in all_tool_definitions
        assert_eq!(
            bus.tool_definitions().len(),
            0,
            "disabled tool should be filtered"
        );
        assert_eq!(
            bus.all_tool_definitions().len(),
            1,
            "all_tool_definitions should still include it"
        );
    }

    #[test]
    fn disabled_tools_still_executable() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut bus = EventBus::new();
            bus.register(Box::new(CounterFeature { event_count: 0 }));
            bus.finalize();

            let disabled =
                std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::from([
                    "count".to_string(),
                ])));
            bus.set_disabled_tools(disabled);

            // Tool is filtered from definitions...
            assert_eq!(bus.tool_definitions().len(), 0);

            // ...but can still be executed
            let cancel = tokio_util::sync::CancellationToken::new();
            let result = bus.execute_tool("count", "tc1", json!({}), cancel).await;
            assert!(result.is_ok(), "disabled tools must still be executable");
        });
    }

    #[test]
    fn drain_clears_requests() {
        let mut bus = EventBus::new();
        bus.register(Box::new(NotifierFeature));
        bus.finalize();

        bus.emit(&BusEvent::SessionEnd {
            turns: 1,
            tool_calls: 0,
            duration_secs: 1.0,
            initial_prompt: None,
            outcome_summary: None,
        });
        assert_eq!(bus.drain_requests().len(), 1);
        // Second drain should be empty
        assert!(bus.drain_requests().is_empty());
    }

    #[test]
    fn compact_tool_schema_strips_descriptions() {
        let def = ToolDefinition {
            name: "test_tool".into(),
            label: "Test".into(),
            description: "A test tool that does things".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to read"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of lines to return",
                        "default": 100
                    }
                },
                "required": ["path"]
            }),
            capabilities: vec![],
        };

        let compact = compact_tool_schema(&def);

        // Top-level description preserved
        assert_eq!(compact.description, "A test tool that does things");

        // Parameter descriptions stripped
        let params = compact.parameters.as_object().unwrap();
        let props = params["properties"].as_object().unwrap();
        assert!(
            !props["path"]
                .as_object()
                .unwrap()
                .contains_key("description")
        );
        assert!(
            !props["limit"]
                .as_object()
                .unwrap()
                .contains_key("description")
        );

        // Structural info preserved
        assert_eq!(props["path"]["type"], "string");
        assert_eq!(props["limit"]["type"], "number");
        assert_eq!(props["limit"]["default"], 100);
        assert_eq!(params["required"][0], "path");

        // Compact schema is smaller
        let full_size = serde_json::to_string(&def.parameters).unwrap().len();
        let compact_size = serde_json::to_string(&compact.parameters).unwrap().len();
        assert!(
            compact_size < full_size,
            "compact ({compact_size}) should be smaller than full ({full_size})"
        );
    }

    // ─── Regression: slim mode tool filtering ───────────────────────────

    /// Helper: register N dummy tools with given names.
    fn bus_with_tools(names: &[&str]) -> EventBus {
        struct MultiToolFeature {
            tools: Vec<ToolDefinition>,
        }

        #[async_trait]
        impl Feature for MultiToolFeature {
            fn name(&self) -> &str {
                "multi"
            }
            fn tools(&self) -> Vec<ToolDefinition> {
                self.tools.clone()
            }
            async fn execute(
                &self,
                _: &str,
                _: &str,
                _: serde_json::Value,
                _: tokio_util::sync::CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    content: vec![],
                    details: json!(null),
                })
            }
        }

        let tools = names
            .iter()
            .map(|name| ToolDefinition {
                name: name.to_string(),
                label: name.to_string(),
                description: String::new(),
                parameters: json!({"type": "object", "properties": {}}),
                capabilities: vec![],
            })
            .collect();
        let disabled = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let mut bus = EventBus::new();
        bus.register(Box::new(MultiToolFeature { tools }));
        bus.finalize();
        bus.set_disabled_tools(disabled);
        bus
    }

    #[test]
    fn base_defaults_disable_situational_tools() {
        use crate::tool_registry as reg;

        // Tools disabled by default in ALL modes (base defaults)
        let base_disabled = [
            reg::persona::SWITCH_PERSONA,
            reg::harness_settings::HARNESS_SETTINGS,
            reg::session_log::SESSION_LOG,
            reg::lifecycle::OPENSPEC_MANAGE,
            reg::auth::AUTH_STATUS,
        ];

        // Tools that stay enabled in Full (non-slim) mode
        let full_enabled = [
            "bash",
            "read",
            reg::delegate::DELEGATE,
            reg::cleave::CLEAVE_RUN,
        ];

        let all_names: Vec<&str> = base_disabled
            .iter()
            .copied()
            .chain(full_enabled.iter().copied())
            .collect();

        // Non-slim: base defaults applied, delegation/cleave stay enabled
        let mut bus = bus_with_tools(&all_names);
        bus.apply_operator_tool_profile(false, &[], &[]);
        let defs = bus.tool_definitions();
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        for tool in &base_disabled {
            assert!(
                !def_names.contains(tool),
                "'{tool}' must be disabled by base defaults"
            );
        }
        assert!(
            def_names.contains(&reg::delegate::DELEGATE),
            "delegate stays enabled in Full mode"
        );
        assert!(
            def_names.contains(&reg::cleave::CLEAVE_RUN),
            "cleave stays enabled in Full mode"
        );
    }

    #[test]
    fn slim_mode_additionally_disables_delegation_and_orchestration() {
        use crate::tool_registry as reg;

        // Tools additionally disabled in slim mode (on top of base defaults)
        let slim_only_disabled = [
            reg::delegate::DELEGATE,
            reg::delegate::DELEGATE_RESULT,
            reg::cleave::CLEAVE_RUN,
            reg::cleave::CLEAVE_ASSESS,
        ];

        let always_enabled = ["bash", "read", "write", "edit", "commit"];

        let all_names: Vec<&str> = slim_only_disabled
            .iter()
            .copied()
            .chain(always_enabled.iter().copied())
            .collect();

        let mut bus = bus_with_tools(&all_names);
        bus.apply_operator_tool_profile(true, &[], &[]);

        let defs = bus.tool_definitions();
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        for tool in &always_enabled {
            assert!(
                def_names.contains(tool),
                "core tool '{tool}' must remain enabled in slim mode"
            );
        }
        for tool in &slim_only_disabled {
            assert!(
                !def_names.contains(tool),
                "'{tool}' must be disabled in slim mode"
            );
        }
    }

    #[test]
    fn lazy_tool_surface_keeps_dynamic_extension_tools_visible_after_turn_one() {
        let bus = bus_with_tools(&["bash", "reader_doctor", "reader_open"]);
        let used_tools = std::collections::HashSet::new();

        let defs = bus.tool_definitions_lazy(false, 2, &used_tools);
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(def_names.contains(&"bash"));
        assert!(
            def_names.contains(&"reader_doctor"),
            "dynamic native extension tools must stay visible after turn 1"
        );
        assert!(
            def_names.contains(&"reader_open"),
            "dynamic native extension tools must stay visible after turn 1"
        );
    }

    #[test]
    fn lazy_tool_surface_still_hides_unused_static_non_core_tools_after_turn_one() {
        use crate::tool_registry as reg;

        let bus = bus_with_tools(&["bash", reg::web_search::WEB_SEARCH]);
        let used_tools = std::collections::HashSet::new();

        let defs = bus.tool_definitions_lazy(false, 2, &used_tools);
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert!(def_names.contains(&"bash"));
        assert!(
            !def_names.contains(&reg::web_search::WEB_SEARCH),
            "unused static non-core tools should remain lazy-filtered"
        );
    }

    #[test]
    fn posture_whitelist_restricts_to_listed_tools_only() {
        let mut bus = bus_with_tools(&["bash", "read", "write", "edit", "delegate"]);
        let enabled = vec!["bash".to_string(), "read".to_string()];
        bus.apply_operator_tool_profile(false, &[], &enabled);

        let defs = bus.tool_definitions();
        let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();

        assert_eq!(def_names.len(), 2);
        assert!(def_names.contains(&"bash"));
        assert!(def_names.contains(&"read"));
        assert!(!def_names.contains(&"write"));
        assert!(!def_names.contains(&"delegate"));
    }
}

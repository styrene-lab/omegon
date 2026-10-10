//! Internal request admission, above the existing provider transport owners.
//!
//! Preparation freezes inputs and requires capability admission and owner evidence
//! before transport. It does not own retries, cancellation, result commitment, or
//! tool invocation permission. Never derive Debug: inputs may contain secrets.

use crate::bridge::{LlmBridge, LlmEvent, LlmMessage, StreamOptions};
use omegon_traits::ToolDefinition;

#[derive(Clone, Copy)]
pub(crate) enum RequestPolicy {
    /// The caller owns the captured surface and interactive budgets.
    Turn,
    /// Summary input and receive-idle limits belong to the compaction owner.
    Compaction,
    /// None retains the existing unbounded helper's EOF compatibility.
    Auxiliary { max_bytes: Option<usize> },
    /// Existing direct route callers retain their admitted tool surface.
    Compatibility,
}

pub(crate) struct RequestInputs<'a> {
    pub(crate) system: &'a str,
    pub(crate) messages: &'a [LlmMessage],
    pub(crate) tools: &'a [ToolDefinition],
    pub(crate) options: &'a StreamOptions,
    pub(crate) policy: RequestPolicy,
}

/// Evidence is the existing owner's receipt, not a new persisted request record.
pub(crate) struct PreparedModelRequest<'a, Evidence> {
    bridge: &'a dyn LlmBridge,
    inputs: RequestInputs<'a>,
    options: StreamOptions,
    evidence: Evidence,
}

impl<'a, Evidence> PreparedModelRequest<'a, Evidence> {
    pub(crate) fn prepare(
        bridge: &'a dyn LlmBridge,
        inputs: RequestInputs<'a>,
        native_model: Option<&str>,
        record_evidence: impl FnOnce(&RequestInputs<'_>) -> anyhow::Result<Evidence>,
    ) -> anyhow::Result<Self> {
        match inputs.policy {
            RequestPolicy::Compaction | RequestPolicy::Auxiliary { .. } => {
                anyhow::ensure!(inputs.tools.is_empty(), "no-tools request advertised tools");
            }
            RequestPolicy::Turn | RequestPolicy::Compatibility => {}
        }
        if let RequestPolicy::Auxiliary {
            max_bytes: Some(limit),
        } = inputs.policy
        {
            anyhow::ensure!(limit > 0, "completion byte budget must be positive");
        }
        let mut options = inputs.options.clone();
        // Only the resolved route supplies this transport-native substitution.
        if let Some(native_model) = native_model {
            options.model = Some(native_model.to_string());
        }
        let capture = options.policy_capture.clone().unwrap_or_else(|| {
            bridge.policy_capture(options.model.as_deref().unwrap_or("unknown:unknown"))
        });
        let mut policy =
            crate::providers::resolve_request_policy(&capture, &options, !inputs.tools.is_empty())?;
        policy.validate_input(crate::providers::estimate_policy_input(
            &capture,
            inputs.system,
            inputs.messages,
            inputs.tools,
        )?)?;
        if matches!(
            policy.reasoning.effective,
            crate::inference_policy::ReasoningValue::ProviderDefault
        ) || options.reasoning.as_deref() == Some("provider-default")
        {
            options.reasoning = None;
        } else if let Some(level) = options
            .reasoning
            .as_deref()
            .and_then(crate::settings::ThinkingLevel::parse)
        {
            options.reasoning = Some(level.as_str().into());
        }
        options.policy_capture = Some(policy.capture.clone());
        options.resolved_policy = Some(std::sync::Arc::new(policy));
        bridge.validate_request_capabilities(inputs.tools, &options)?;
        let evidence = record_evidence(&inputs)?;
        Ok(Self {
            bridge,
            inputs,
            options,
            evidence,
        })
    }

    pub(crate) fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    pub(crate) fn policy(&self) -> &crate::inference_policy::ResolvedPolicy {
        self.options
            .resolved_policy
            .as_deref()
            .expect("prepared request has policy")
    }

    pub(crate) fn record_usage(&self, tokens: (u64, u64, u64, u64)) {
        if tokens == (0, 0, 0, 0) {
            return;
        }
        if let Some(sink) = &self.options.policy_sink
            && let Ok(mut settings) = sink.lock()
        {
            let policy = self.policy();
            settings.last_provider_usage = Some(omegon_traits::InferenceProviderUsage {
                measured_at: Some(chrono::Utc::now().to_rfc3339()),
                snapshot_id: policy.snapshot_id.to_string(), route_key: policy.capture.route.storage_key(),
                model: format!("{}:{}", policy.capture.route.provider, policy.capture.route.native_model),
                input_tokens: tokens.0, output_tokens: tokens.1,
                cache_read_tokens: tokens.2, cache_creation_tokens: tokens.3,
                semantics: if policy.capture.route.provider == "anthropic" {
                    "Anthropic input excludes separate cache read/write counters; last-request measurement"
                } else { "provider input count retained as reported; cache counters are subsets/unknown and are not added; last-request measurement" }.into(),
            });
        }
    }

    pub(crate) fn output_byte_limit(&self) -> Option<usize> {
        match self.inputs.policy {
            RequestPolicy::Auxiliary { max_bytes } => max_bytes,
            _ => None,
        }
    }

    /// An unchanged transport retry reuses this envelope and its evidence.
    pub(crate) async fn stream(&self) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
        let captured = &self.policy().capture;
        let current = self.bridge.policy_capture(&captured.selected_model);
        anyhow::ensure!(
            current.route == captured.route
                && current.contribution_generation == captured.contribution_generation
                && current.inventory_generation == captured.inventory_generation,
            "inference route or metadata generation changed after capture; resolve and prepare again"
        );
        if let Some(sink) = &self.options.policy_sink {
            let mut settings = sink
                .lock()
                .map_err(|_| anyhow::anyhow!("policy projection lock poisoned"))?;
            settings.last_inference_policy = self.options.resolved_policy.clone();
        }
        self.bridge
            .stream(
                self.inputs.system,
                self.inputs.messages,
                self.inputs.tools,
                &self.options,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct InspectingBridge {
        reject: bool,
        evidence_writes: AtomicUsize,
        received: Mutex<Vec<serde_json::Value>>,
    }

    #[async_trait::async_trait]
    impl LlmBridge for InspectingBridge {
        fn validate_request_capabilities(
            &self,
            _: &[ToolDefinition],
            _: &StreamOptions,
        ) -> anyhow::Result<()> {
            anyhow::ensure!(!self.reject, "unsupported capability");
            Ok(())
        }

        async fn stream(
            &self,
            system: &str,
            messages: &[LlmMessage],
            tools: &[ToolDefinition],
            options: &StreamOptions,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
            assert_eq!(self.evidence_writes.load(Ordering::SeqCst), 1);
            self.received.lock().unwrap().push(serde_json::json!({
                "system": system, "messages": messages, "tools": tools,
                "model": options.model, "reasoning": options.reasoning,
                "extended": options.extended_context, "extra": options.extra_body,
            }));
            let (_, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    fn tool() -> ToolDefinition {
        ToolDefinition {
            name: "read".into(),
            label: "Read".into(),
            description: "Read file".into(),
            parameters: serde_json::json!({"type":"object"}),
            capabilities: vec![],
        }
    }

    #[test]
    fn resolved_policy_rejects_saved_off_before_evidence_or_transport() {
        let bridge = InspectingBridge::default();
        let options = StreamOptions {
            model: Some("openai-codex:gpt-6-astra".into()),
            reasoning: Some("off".into()),
            ..Default::default()
        };
        let result = PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "system",
                messages: &[],
                tools: &[],
                options: &options,
                policy: RequestPolicy::Turn,
            },
            None,
            |_| -> anyhow::Result<()> { panic!("unsupported Off reached evidence") },
        );
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("needs resolution")
        );
        assert_eq!(options.reasoning.as_deref(), Some("off"));
        assert!(bridge.received.lock().unwrap().is_empty());
    }

    #[test]
    fn resolved_policy_generation_cap_validates_complete_input_boundary() {
        use crate::inference_policy::*;
        let bridge = InspectingBridge::default();
        let mut capture = bridge.policy_capture("openai:gpt-6-astra");
        for fact in &mut capture.facts.capacity {
            fact.tokens = match fact.field {
                CapacityField::DefaultWindow
                | CapacityField::MaximumWindow
                | CapacityField::MaximumInput
                | CapacityField::MaximumTotal
                | CapacityField::MaximumOutput => 100_000,
                _ => fact.tokens,
            };
        }
        capture.intent.heuristic_generation = Some(8_000);
        let options = StreamOptions {
            model: Some("openai:gpt-6-astra".into()),
            policy_capture: Some(capture),
            extra_body: [("max_output_tokens".into(), serde_json::json!(20_000))].into(),
            ..Default::default()
        };
        for (tokens, fits) in [(90_000, false), (80_000, true)] {
            let system = "x".repeat(tokens * 4);
            let result = PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: &system,
                    messages: &[],
                    tools: &[],
                    options: &options,
                    policy: RequestPolicy::Turn,
                },
                None,
                |_| Ok(()),
            );
            assert_eq!(result.is_ok(), fits, "{tokens}-token complete input fit");
        }
    }

    #[test]
    fn complete_host_instructions_charge_oauth_prefix_schemas_and_generation_before_capture() {
        use crate::inference_policy::CapacityField;
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.md");
        let project = dir.path().join("AGENTS.md");
        let global_text = format!("{}GLOBAL END", "Global 界\n".repeat(1000));
        let project_text = format!("{}PROJECT END", "Project é\n".repeat(1000));
        std::fs::write(&global, &global_text).unwrap();
        std::fs::write(&project, &project_text).unwrap();
        let host = crate::prompt::assemble_host_prompt(dir.path(), &global).unwrap();
        let mut manager = crate::context::ContextManager::new(host.prompt, vec![]);
        let tools = [tool()];
        let assembly = crate::loop_context::compose_with_manager(
            &mut manager,
            &crate::conversation::ConversationState::new(),
            &tools,
            200_000,
        );
        assert!(assembly.system_prompt.contains(&global_text));
        assert!(assembly.system_prompt.contains(&project_text));
        let bridge = InspectingBridge::default();
        let mut capture = bridge.policy_capture("anthropic:claude-sonnet-4-6");
        capture.native_adapter = true;
        capture.route.authentication = "oauth".into();
        let input = crate::providers::estimate_policy_input(
            &capture,
            &assembly.system_prompt,
            &assembly.messages,
            &tools,
        )
        .unwrap();
        let actual_prefix = "You are Claude Code, Anthropic's official CLI for Claude.";
        assert_eq!(
            input.system,
            (assembly.system_prompt.len() + actual_prefix.len()) / 4
        );
        assert!(input.schemas > 0);
        let required = input.total().unwrap() + 16_384; // Existing Anthropic output envelope.
        for (capacity, fits) in [(required - 1, false), (required, true)] {
            let mut bounded = capture.clone();
            for fact in &mut bounded.facts.capacity {
                fact.tokens = match fact.field {
                    CapacityField::DefaultWindow
                    | CapacityField::MaximumWindow
                    | CapacityField::MaximumInput
                    | CapacityField::MaximumTotal => capacity,
                    CapacityField::MaximumOutput => 16_384,
                    _ => fact.tokens,
                };
            }
            let options = StreamOptions {
                model: Some("anthropic:claude-sonnet-4-6".into()),
                policy_capture: Some(bounded),
                ..Default::default()
            };
            let result = PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: &assembly.system_prompt,
                    messages: &assembly.messages,
                    tools: &tools,
                    options: &options,
                    policy: RequestPolicy::Turn,
                },
                None,
                |inputs| {
                    assert!(fits, "oversized input reached capture");
                    assert_eq!(inputs.system, assembly.system_prompt);
                    bridge.evidence_writes.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            );
            match result {
                Ok(prepared) => {
                    assert!(fits);
                    assert_eq!(
                        prepared
                            .policy()
                            .visible_input
                            .as_ref()
                            .unwrap()
                            .total()
                            .unwrap(),
                        input.total().unwrap()
                    );
                }
                Err(error) => {
                    assert!(!fits, "{error:#}");
                    assert!(error.to_string().contains("input"), "{error:#}");
                }
            }
        }
        assert_eq!(bridge.evidence_writes.load(Ordering::SeqCst), 1);
        assert!(bridge.received.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn retries_reuse_exact_inputs_and_one_evidence_receipt() {
        let bridge = InspectingBridge::default();
        let messages = [LlmMessage::User {
            content: "ordered evidence é".into(),
            images: vec![],
        }];
        let tools = [tool()];
        let options = StreamOptions {
            model: Some("selected:model".into()),
            reasoning: Some("high".into()),
            extended_context: true,
            extra_body: [("temperature".into(), serde_json::json!(0))].into(),
            ..Default::default()
        };
        let prepared = PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "exact system",
                messages: &messages,
                tools: &tools,
                options: &options,
                policy: RequestPolicy::Turn,
            },
            None,
            |inputs| {
                assert_eq!(inputs.system, "exact system");
                bridge.evidence_writes.fetch_add(1, Ordering::SeqCst);
                Ok("same-request-id")
            },
        )
        .unwrap();
        drop(prepared.stream().await.unwrap());
        drop(prepared.stream().await.unwrap());
        assert_eq!(*prepared.evidence(), "same-request-id");
        let received = bridge.received.lock().unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0], received[1]);
        assert_eq!(
            received[0],
            serde_json::json!({
                "system":"exact system", "messages":messages, "tools":tools,
                "model":"selected:model", "reasoning":null, "extended":true, "extra":{"temperature":0},
            })
        );
    }

    #[test]
    fn no_tools_and_invalid_budget_reject_before_evidence() {
        let bridge = InspectingBridge::default();
        let options = StreamOptions::default();
        let tools = [tool()];
        for (policy, tools) in [
            (RequestPolicy::Compaction, tools.as_slice()),
            (
                RequestPolicy::Auxiliary { max_bytes: Some(8) },
                tools.as_slice(),
            ),
            (RequestPolicy::Auxiliary { max_bytes: Some(0) }, &[]),
        ] {
            let result = PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: "",
                    messages: &[],
                    tools,
                    options: &options,
                    policy,
                },
                None,
                |_| -> anyhow::Result<()> { panic!("invalid input reached evidence writer") },
            );
            assert!(result.is_err());
        }
        assert!(bridge.received.lock().unwrap().is_empty());
    }

    #[test]
    fn capability_and_evidence_failures_cannot_produce_dispatch_inputs() {
        for reject in [true, false] {
            let bridge = InspectingBridge {
                reject,
                ..Default::default()
            };
            let options = StreamOptions::default();
            let result = PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: "sensitive input",
                    messages: &[],
                    tools: &[],
                    options: &options,
                    policy: RequestPolicy::Turn,
                },
                None,
                |_| -> anyhow::Result<()> {
                    assert!(!reject, "capability rejection must precede evidence");
                    anyhow::bail!("required evidence unavailable")
                },
            );
            let Err(error) = result else {
                panic!("failed preparation returned dispatchable inputs")
            };
            assert!(!error.to_string().contains("sensitive input"));
            assert!(bridge.received.lock().unwrap().is_empty());
        }
    }
}

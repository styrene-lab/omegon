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
        bridge.validate_request_capabilities(inputs.tools, inputs.options)?;
        let evidence = record_evidence(&inputs)?;
        let mut options = inputs.options.clone();
        // Only the resolved route supplies this transport-native substitution.
        if let Some(native_model) = native_model {
            options.model = Some(native_model.to_string());
        }
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

    pub(crate) fn output_byte_limit(&self) -> Option<usize> {
        match self.inputs.policy {
            RequestPolicy::Auxiliary { max_bytes } => max_bytes,
            _ => None,
        }
    }

    /// An unchanged transport retry reuses this envelope and its evidence.
    pub(crate) async fn stream(&self) -> anyhow::Result<tokio::sync::mpsc::Receiver<LlmEvent>> {
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
                "model":"selected:model", "reasoning":"high", "extended":true, "extra":{"temperature":0},
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

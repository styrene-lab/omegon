//! MQTT bridge — publishes AgentEvents to the Auspex MQTT broker.
//!
//! Subscribes to the `tokio::broadcast` AgentEvent channel (same as TUI and
//! IPC) and projects events through `IpcEventPayload` into typed MQTT publishes
//! on the Aether topic hierarchy.
//!
//! The broker is owned by Auspex, not by Omegon. Omegon connects as a remote
//! TCP client. If the broker is unreachable, the bridge is not started —
//! Omegon continues without MQTT.

use std::time::Duration;

use omegon_traits::{AgentEvent, IpcEventPayload};
use styrene_mqtt::{Client, ClientConfig, ConnectionTarget, QosOverride, ServiceIdentity};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Default MQTT broker port (Auspex listens here).
pub const DEFAULT_BROKER_PORT: u16 = 1883;

/// Handle to the running MQTT bridge. Drop to stop.
pub struct MqttBridgeHandle {
    _task: JoinHandle<()>,
}

/// Configuration for the MQTT bridge.
pub struct MqttBridgeConfig {
    /// Operator identity hash (from Styrene identity or config).
    pub operator_id: String,
    /// Unique session/instance ID for this Omegon process.
    pub instance_id: String,
    /// MQTT broker host. Default: "127.0.0.1".
    pub broker_host: String,
    /// MQTT broker port. Default: 1883.
    pub broker_port: u16,
}

impl Default for MqttBridgeConfig {
    fn default() -> Self {
        Self {
            operator_id: "local".into(),
            instance_id: String::new(),
            broker_host: "127.0.0.1".into(),
            broker_port: DEFAULT_BROKER_PORT,
        }
    }
}

/// Start the MQTT bridge task.
///
/// Connects to the Auspex-hosted MQTT broker as a TCP client. If the broker
/// is unreachable, the task exits and Omegon continues without MQTT.
pub fn start_mqtt_bridge(
    config: MqttBridgeConfig,
    events_tx: broadcast::Sender<AgentEvent>,
) -> MqttBridgeHandle {
    let task = tokio::spawn(async move {
        let addr = format!("{}:{}", config.broker_host, config.broker_port);
        match tokio::time::timeout(
            Duration::from_millis(250),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        {
            Ok(Ok(stream)) => drop(stream),
            Ok(Err(e)) => {
                tracing::debug!(broker = %addr, error = %e, "MQTT bridge broker unavailable");
                return;
            }
            Err(_) => {
                tracing::debug!(broker = %addr, "MQTT bridge broker connection timed out");
                return;
            }
        }

        let identity = ServiceIdentity {
            operator_id: config.operator_id,
            service: "omegon".into(),
            instance_id: config.instance_id,
        };

        let target = ConnectionTarget::Remote {
            host: config.broker_host,
            port: config.broker_port,
        };

        let client = match Client::connect(ClientConfig::new(identity, target)).await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("MQTT bridge: broker not available ({e}), running without MQTT");
                return;
            }
        };

        // Brief settle time for the connection to establish.
        tokio::time::sleep(Duration::from_millis(50)).await;

        run_bridge(client, events_tx).await;
    });

    MqttBridgeHandle { _task: task }
}

async fn run_bridge(client: Client, events_tx: broadcast::Sender<AgentEvent>) {
    let mut events_rx = events_tx.subscribe();

    loop {
        match events_rx.recv().await {
            Ok(ev) => {
                if let Some(ipc_ev) = project_event(&ev) {
                    let name = event_name(&ipc_ev);
                    let qos = QosOverride::default();

                    if let Err(e) = client.publish(name, &ipc_ev, qos).await {
                        tracing::debug!("MQTT publish failed for {name}: {e}");
                    }
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::debug!("MQTT bridge lagged, skipped {n} events");
            }
        }
    }
}

// ── Event projection (mirrors ipc/connection.rs) ────────────────────────────

/// Project an AgentEvent to the serializable IpcEventPayload.
/// Returns `None` for internal-only events that shouldn't be published.
fn project_event(ev: &AgentEvent) -> Option<IpcEventPayload> {
    match ev {
        AgentEvent::TurnStart { turn } => Some(IpcEventPayload::TurnStarted { turn: *turn }),
        AgentEvent::TurnEnd(te) => Some(IpcEventPayload::TurnEnded {
            turn: te.turn,
            estimated_tokens: te.estimated_tokens,
            actual_input_tokens: te.actual_input_tokens,
            actual_output_tokens: te.actual_output_tokens,
            cache_read_tokens: te.cache_read_tokens,
            provider_telemetry: te.provider_telemetry.clone(),
            streaks: te.streaks,
        }),
        AgentEvent::MessageChunk { text } => {
            Some(IpcEventPayload::MessageDelta { text: text.clone() })
        }
        AgentEvent::ThinkingChunk { text } => {
            Some(IpcEventPayload::ThinkingDelta { text: text.clone() })
        }
        AgentEvent::MessageEnd => Some(IpcEventPayload::MessageCompleted),
        AgentEvent::ToolStart { id, name, args } => Some(IpcEventPayload::ToolStarted {
            id: id.clone(),
            name: name.clone(),
            args: args.clone(),
        }),
        AgentEvent::ToolUpdate { id, partial } => Some(IpcEventPayload::ToolUpdated {
            id: id.clone(),
            partial: partial.clone(),
        }),
        AgentEvent::ToolEnd {
            id,
            name,
            result,
            is_error,
        } => {
            let summary: String = result
                .content
                .iter()
                .filter_map(|b| b.as_text())
                .collect::<Vec<_>>()
                .join("\n")
                .chars()
                .take(200)
                .collect();
            Some(IpcEventPayload::ToolEnded {
                id: id.clone(),
                name: name.clone(),
                is_error: *is_error,
                summary: if summary.is_empty() {
                    None
                } else {
                    Some(summary)
                },
            })
        }
        AgentEvent::AgentEnd => Some(IpcEventPayload::AgentCompleted),
        AgentEvent::PhaseChanged { phase } => Some(IpcEventPayload::PhaseChanged {
            phase: format!("{phase:?}"),
        }),
        AgentEvent::DecompositionStarted {
            children,
            operation,
        } => Some(IpcEventPayload::DecompositionStarted {
            children: children.clone(),
            operation: Some(operation.clone()),
        }),
        AgentEvent::DecompositionChildCompleted {
            label,
            success,
            operation,
        } => Some(IpcEventPayload::DecompositionChildCompleted {
            label: label.clone(),
            success: *success,
            operation: Some(operation.clone()),
        }),
        AgentEvent::DecompositionCompleted { merged, operation } => {
            Some(IpcEventPayload::DecompositionCompleted {
                merged: *merged,
                operation: Some(operation.clone()),
            })
        }
        AgentEvent::SkillActivation { event } => Some(IpcEventPayload::SystemNotification {
            message: format!(
                "skill active: {} · {}{}{}",
                event.active_ref,
                event.resolution,
                if event.suppressing.is_empty() {
                    String::new()
                } else {
                    format!(" · suppressing {}", event.suppressing.join(", "))
                },
                event
                    .recommendation
                    .as_ref()
                    .map(|r| format!(" · {r}"))
                    .unwrap_or_default()
            ),
        }),
        AgentEvent::SystemNotification { message } => Some(IpcEventPayload::SystemNotification {
            message: message.clone(),
        }),
        AgentEvent::OperatorCopyBlock { label, text, .. } => Some(IpcEventPayload::SystemNotification {
            message: format!("{label}: {text}"),
        }),
        AgentEvent::StreamIdle {
            provider,
            model,
            phase,
            idle_secs,
            ambiguous,
            message,
        } => Some(IpcEventPayload::StreamIdle {
            provider: provider.clone(),
            model: model.clone(),
            phase: phase.clone(),
            idle_secs: *idle_secs,
            ambiguous: *ambiguous,
            message: message.clone(),
        }),
        AgentEvent::ProviderRetry {
            provider,
            model,
            attempt,
            delay_ms,
            reason,
            message,
            recoverable,
        } => Some(IpcEventPayload::SystemNotification {
            message: format!(
                "provider retry: {provider}/{model} attempt {attempt} delay {delay_ms}ms reason={reason} recoverable={recoverable}: {message}"
            ),
        }),
        AgentEvent::ProviderFailure {
            provider,
            model,
            reason,
            attempts,
            message,
            retryable,
            recommended_action,
        } => Some(IpcEventPayload::SystemNotification {
            message: format!(
                "provider failure: {provider}/{model} attempts {attempts} reason={reason} retryable={retryable}: {message}; {recommended_action}"
            ),
        }),
        AgentEvent::TurnCancelled { reason } => Some(IpcEventPayload::SystemNotification {
            message: format!("turn cancelled: {reason}"),
        }),
        AgentEvent::RuntimeQueueUpdated { snapshot_json } => {
            Some(IpcEventPayload::RuntimeQueueUpdated {
                snapshot: snapshot_json.clone(),
            })
        }
        AgentEvent::RuntimePromptStarted { .. } => None,
        AgentEvent::FamilyVitalSignsUpdated { signs } => {
            Some(IpcEventPayload::FamilyVitalSignsUpdated {
                signs: signs.clone(),
            })
        }
        AgentEvent::PlanUpdated { projection } => Some(IpcEventPayload::PlanUpdated {
            // MQTT reuses the IPC compatibility envelope, so keep publishing the
            // legacy snapshot JSON derived from the typed runtime projection.
            snapshot: projection.legacy_snapshot_json(),
        }),
        AgentEvent::RouteChanged {
            state,
            selected,
            serving,
            warning,
            message,
        } => Some(IpcEventPayload::ProviderRouteChanged {
            state: state.clone(),
            selected: selected.clone(),
            serving: serving.clone(),
            warning: warning.clone(),
            message: message.clone(),
        }),
        AgentEvent::HarnessStatusChanged { .. } => Some(IpcEventPayload::HarnessChanged),
        AgentEvent::SessionReset => Some(IpcEventPayload::SessionReset),
        // Internal-only — not published to MQTT.
        AgentEvent::MessageStart { .. }
        | AgentEvent::MessageAbort { .. }
        | AgentEvent::ContextUpdated { .. }
        | AgentEvent::ContextCompaction(_)
        | AgentEvent::WebDashboardStarted { .. }
        | AgentEvent::OperatorWaitRequest { .. }
        | AgentEvent::PermissionRequest { .. } => None,
    }
}

/// Stable event name for a given payload (matches IpcEventPayload serde renames).
fn event_name(ev: &IpcEventPayload) -> &'static str {
    match ev {
        IpcEventPayload::TurnStarted { .. } => "turn.started",
        IpcEventPayload::TurnEnded { .. } => "turn.ended",
        IpcEventPayload::MessageDelta { .. } => "message.delta",
        IpcEventPayload::ThinkingDelta { .. } => "thinking.delta",
        IpcEventPayload::MessageCompleted => "message.completed",
        IpcEventPayload::ToolStarted { .. } => "tool.started",
        IpcEventPayload::ToolUpdated { .. } => "tool.updated",
        IpcEventPayload::ToolEnded { .. } => "tool.ended",
        IpcEventPayload::AgentCompleted => "agent.completed",
        IpcEventPayload::PhaseChanged { .. } => "phase.changed",
        IpcEventPayload::DecompositionStarted { .. } => "decomposition.started",
        IpcEventPayload::DecompositionChildCompleted { .. } => "decomposition.child_completed",
        IpcEventPayload::DecompositionCompleted { .. } => "decomposition.completed",
        IpcEventPayload::FamilyVitalSignsUpdated { .. } => "family.vital_signs",
        IpcEventPayload::PlanUpdated { .. } => "plan.updated",
        IpcEventPayload::StreamIdle { .. } => "stream.idle",
        IpcEventPayload::ProviderRouteChanged { .. } => "provider.route_changed",
        IpcEventPayload::RuntimeQueueUpdated { .. } => "runtime.queue_updated",
        IpcEventPayload::HarnessChanged => "harness.changed",
        IpcEventPayload::StateChanged { .. } => "state.changed",
        IpcEventPayload::SystemNotification { .. } => "system.notification",
        IpcEventPayload::SessionReset => "session.reset",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::{AgentEvent, OperationKind, OperationRef};

    #[test]
    fn projects_decomposition_operation_provenance() {
        let payload = project_event(&AgentEvent::DecompositionStarted {
            children: vec!["delegate_1".into()],
            operation: OperationRef::delegate("delegate_1"),
        })
        .expect("projected payload");

        match payload {
            IpcEventPayload::DecompositionStarted {
                children,
                operation: Some(operation),
            } => {
                assert_eq!(children, vec!["delegate_1"]);
                assert_eq!(operation.kind, OperationKind::Delegate);
                assert_eq!(operation.id.as_deref(), Some("delegate_1"));
            }
            other => panic!("expected decomposition payload with operation, got {other:?}"),
        }
    }

    #[test]
    fn projects_plan_updated_from_typed_projection_to_legacy_snapshot() {
        let payload = project_event(&AgentEvent::PlanUpdated {
            projection: omegon_traits::PlanSurfaceProjection {
                active: Some(omegon_traits::PlanLaneProjection {
                    plan_id: "session:current".into(),
                    mode: "executing".into(),
                    guidance: "keep going".into(),
                    status: "active".into(),
                    scope: "session".into(),
                    source: "session".into(),
                    progress: omegon_traits::PlanProgressProjection {
                        completed: 1,
                        total: 2,
                    },
                    items: vec![omegon_traits::PlanItemProjection {
                        label: "Patch".into(),
                        status: "active".into(),
                        ..Default::default()
                    }],
                }),
                workstreams: vec![omegon_traits::PlanWorkstreamProjection {
                    id: "openspec:demo".into(),
                    title: "demo".into(),
                    status: "paused".into(),
                    progress: omegon_traits::PlanProgressProjection {
                        completed: 3,
                        total: 5,
                    },
                }],
                ..Default::default()
            },
        })
        .expect("projected payload");

        match payload {
            IpcEventPayload::PlanUpdated { snapshot } => {
                assert_eq!(snapshot["mode"], "executing");
                assert_eq!(snapshot["completed"], 1);
                assert_eq!(snapshot["items"][0]["description"], "Patch");
                assert_eq!(snapshot["workstreams"][0]["id"], "openspec:demo");
                assert_eq!(snapshot["workstreams"][0]["completed"], 3);
            }
            other => panic!("expected plan payload, got {other:?}"),
        }
    }
}

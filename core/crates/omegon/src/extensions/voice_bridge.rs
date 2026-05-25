//! Voice notification bridge — converts trusted local voice extension notifications
//! into daemon prompt events.

use std::sync::{Arc, Mutex};

use omegon_traits::AgentEvent;
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use super::ExtensionNotification;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceStateUpdate {
    pub extension: String,
    pub state: String,
    pub mic_open: bool,
}

/// Start a push-driven bridge for one voice-capable extension.
pub fn start_voice_bridge(
    rx: mpsc::UnboundedReceiver<ExtensionNotification>,
    daemon_events: Arc<Mutex<Vec<omegon_traits::DaemonEventEnvelope>>>,
    cancel: CancellationToken,
) {
    start_voice_bridge_with_status(rx, daemon_events, None, cancel);
}

pub fn start_voice_bridge_with_status(
    mut rx: mpsc::UnboundedReceiver<ExtensionNotification>,
    daemon_events: Arc<Mutex<Vec<omegon_traits::DaemonEventEnvelope>>>,
    status_sink: Option<VoiceStatusSink>,
    cancel: CancellationToken,
) {
    crate::task_spawn::spawn_best_effort_result("voice-notification-bridge", async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("voice notification bridge shutting down");
                    return Ok(());
                }
                maybe_notification = rx.recv() => {
                    let Some(notification) = maybe_notification else {
                        tracing::debug!("voice notification bridge receiver closed");
                        return Ok(());
                    };
                    if let Some(update) = voice_notification_to_state(&notification) {
                        if let Some(sink) = &status_sink {
                            sink.publish(update);
                        }
                        continue;
                    }
                    let Some(envelope) = voice_notification_to_event(&notification) else {
                        continue;
                    };
                    tracing::info!(
                        extension = %notification.extension_name,
                        event_id = %envelope.event_id,
                        "voice bridge: injecting transcription"
                    );
                    match daemon_events.lock() {
                        Ok(mut queue) => queue.push(envelope),
                        Err(err) => tracing::error!(error = %err, "failed to push voice event to daemon queue"),
                    }
                }
            }
        }
    });
}

#[derive(Clone)]
pub struct VoiceStatusSink {
    harness_status: Arc<Mutex<crate::status::HarnessStatus>>,
    events_tx: broadcast::Sender<AgentEvent>,
}

impl VoiceStatusSink {
    pub fn new(
        harness_status: Arc<Mutex<crate::status::HarnessStatus>>,
        events_tx: broadcast::Sender<AgentEvent>,
    ) -> Self {
        Self {
            harness_status,
            events_tx,
        }
    }

    pub fn publish(&self, update: VoiceStateUpdate) {
        let status_json = match self.harness_status.lock() {
            Ok(mut status) => {
                status.voice_state = Some(crate::status::VoiceStateStatus {
                    extension: update.extension,
                    state: update.state,
                    mic_open: update.mic_open,
                });
                serde_json::to_value(&*status).unwrap_or_default()
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to update voice state status");
                return;
            }
        };
        let _ = self
            .events_tx
            .send(AgentEvent::HarnessStatusChanged { status_json });
    }
}

pub(crate) fn voice_notification_to_state(
    notification: &ExtensionNotification,
) -> Option<VoiceStateUpdate> {
    if notification.method != "voice/state" {
        return None;
    }
    let state = notification.params.get("state")?.as_str()?.trim();
    if state.is_empty() {
        return None;
    }
    let mic_open = notification.params.get("mic_open")?.as_bool()?;
    Some(VoiceStateUpdate {
        extension: notification.extension_name.clone(),
        state: state.to_string(),
        mic_open,
    })
}

pub(crate) fn voice_notification_to_event(
    notification: &ExtensionNotification,
) -> Option<omegon_traits::DaemonEventEnvelope> {
    if notification.method != "voice/transcription" {
        return None;
    }
    transcription_params_to_event(&notification.extension_name, &notification.params)
}

fn transcription_params_to_event(
    extension_name: &str,
    params: &Value,
) -> Option<omegon_traits::DaemonEventEnvelope> {
    let text = params.get("text")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let duration_s = params.get("duration_s").and_then(Value::as_f64);
    let utterance_id = params
        .get("utterance_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .unwrap_or("transcription");

    Some(omegon_traits::DaemonEventEnvelope {
        event_id: format!("voice-{extension_name}-{utterance_id}"),
        source: "voice".to_string(),
        trigger_kind: "prompt".to_string(),
        payload: json!({
            "text": text,
            "duration_s": duration_s,
            "utterance_id": utterance_id,
            "trust_level": "operator",
            "extension": extension_name,
        }),
        caller_role: Some("edit".to_string()),
        source_user: None,
        source_channel: Some("voice".to_string()),
        source_thread: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(method: &str, params: Value) -> ExtensionNotification {
        ExtensionNotification {
            extension_name: "omegon-voice".to_string(),
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn valid_transcription_becomes_operator_prompt_event() {
        let event = voice_notification_to_event(&notification(
            "voice/transcription",
            json!({"text": "open the reader", "duration_s": 1.9, "utterance_id": "u1"}),
        ))
        .expect("voice event");

        assert_eq!(event.event_id, "voice-omegon-voice-u1");
        assert_eq!(event.source, "voice");
        assert_eq!(event.trigger_kind, "prompt");
        assert_eq!(event.payload["text"], "open the reader");
        assert_eq!(event.payload["duration_s"], 1.9);
        assert_eq!(event.payload["utterance_id"], "u1");
        assert_eq!(event.payload["trust_level"], "operator");
        assert_eq!(event.payload["extension"], "omegon-voice");
        assert_eq!(event.caller_role.as_deref(), Some("edit"));
        assert_eq!(event.source_user, None);
        assert_eq!(event.source_channel.as_deref(), Some("voice"));
        assert_eq!(event.source_thread, None);
    }

    #[test]
    fn transcription_text_is_trimmed_and_empty_text_is_ignored() {
        let event = voice_notification_to_event(&notification(
            "voice/transcription",
            json!({"text": "  proceed  "}),
        ))
        .expect("voice event");
        assert_eq!(event.payload["text"], "proceed");

        assert!(
            voice_notification_to_event(&notification(
                "voice/transcription",
                json!({"text": "   "}),
            ))
            .is_none()
        );
    }

    #[test]
    fn malformed_transcription_is_ignored_without_panic() {
        assert!(
            voice_notification_to_event(&notification(
                "voice/transcription",
                json!({"duration_s": 1.0}),
            ))
            .is_none()
        );
        assert!(
            voice_notification_to_event(&notification("voice/transcription", json!({"text": 42}),))
                .is_none()
        );
    }

    #[test]
    fn voice_state_notification_becomes_status_update_without_prompt_event() {
        let update = voice_notification_to_state(&notification(
            "voice/state",
            json!({"state": "listening", "mic_open": true}),
        ))
        .expect("voice state");

        assert_eq!(update.extension, "omegon-voice");
        assert_eq!(update.state, "listening");
        assert!(update.mic_open);
        assert!(
            voice_notification_to_event(&notification(
                "voice/state",
                json!({"state": "listening", "mic_open": true}),
            ))
            .is_none(),
            "voice state must not become a daemon prompt event"
        );
    }

    #[test]
    fn malformed_voice_state_is_ignored() {
        assert!(
            voice_notification_to_state(&notification(
                "voice/state",
                json!({"state": "listening"}),
            ))
            .is_none()
        );
        assert!(
            voice_notification_to_state(&notification(
                "voice/state",
                json!({"state": "", "mic_open": true}),
            ))
            .is_none()
        );
        assert!(
            voice_notification_to_state(&notification(
                "voice/state",
                json!({"state": "idle", "mic_open": "false"}),
            ))
            .is_none()
        );
    }

    #[test]
    fn voice_status_sink_updates_harness_status_and_broadcasts() {
        let status = Arc::new(Mutex::new(crate::status::HarnessStatus::default()));
        let (events_tx, mut events_rx) = broadcast::channel(4);
        let sink = VoiceStatusSink::new(status.clone(), events_tx);

        sink.publish(VoiceStateUpdate {
            extension: "omegon-voice".to_string(),
            state: "processing".to_string(),
            mic_open: true,
        });

        let stored = status.lock().unwrap().voice_state.clone().unwrap();
        assert_eq!(stored.extension, "omegon-voice");
        assert_eq!(stored.state, "processing");
        assert!(stored.mic_open);

        match events_rx.try_recv().expect("status event") {
            AgentEvent::HarnessStatusChanged { status_json } => {
                assert_eq!(status_json["voice_state"]["state"], "processing");
                assert_eq!(status_json["voice_state"]["mic_open"], true);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn non_voice_transcription_notifications_are_ignored() {
        assert!(
            voice_notification_to_event(&notification(
                "voice/state",
                json!({"state": "listening", "mic_open": true}),
            ))
            .is_none()
        );
    }
}

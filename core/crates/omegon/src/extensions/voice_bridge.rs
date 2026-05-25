//! Voice notification bridge — converts trusted local voice extension notifications
//! into daemon prompt events.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::ExtensionNotification;

/// Start a push-driven bridge for one voice-capable extension.
pub fn start_voice_bridge(
    mut rx: mpsc::UnboundedReceiver<ExtensionNotification>,
    daemon_events: Arc<Mutex<Vec<omegon_traits::DaemonEventEnvelope>>>,
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

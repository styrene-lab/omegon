//! Versioned envelopes for semantic UI surface/action replay.
//!
//! These envelopes are internal runtime DTOs. They are intentionally separate
//! from ACP/Flynt/other external wire DTOs so transports can adapt naming,
//! redaction, and capability policy without leaking Rust internals.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::surfaces::layout::{
    LiveDetail, TelemetryDensity, TranscriptDensity, UiPresentationPolicy,
};

/// Internal schema version for UI runtime envelopes.
pub const UI_RUNTIME_ENVELOPE_VERSION: u32 = 1;

/// Stable surface names used by the internal replay/envelope layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiSurfaceKind {
    Conversation,
    Editor,
    Footer,
    Dashboard,
    Instruments,
    Layout,
    Presentation,
}

/// Versioned surface snapshot/update envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceEnvelope {
    pub protocol_version: u32,
    pub session_id: String,
    pub surface: UiSurfaceKind,
    pub revision: u64,
    pub payload: Value,
}

impl SurfaceEnvelope {
    pub fn new(
        session_id: impl Into<String>,
        surface: UiSurfaceKind,
        revision: u64,
        payload: Value,
    ) -> Self {
        Self {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id: session_id.into(),
            surface,
            revision,
            payload,
        }
    }
}

pub fn presentation_payload(policy: UiPresentationPolicy) -> Value {
    let transcript = match policy.transcript_density() {
        TranscriptDensity::Outcomes => "outcomes",
        TranscriptDensity::Evidence => "evidence",
    };
    let live_detail = match policy.live_detail() {
        LiveDetail::Status => "status",
        LiveDetail::Workflow => "workflow",
        LiveDetail::Diagnostic => "diagnostic",
    };
    let telemetry = match policy.telemetry_density() {
        TelemetryDensity::Essential => "essential",
        TelemetryDensity::Operational => "operational",
        TelemetryDensity::Diagnostic => "diagnostic",
    };
    serde_json::json!({
        "level": policy.level.name(),
        "preset": policy.preset_name(),
        "transcriptDensity": transcript,
        "liveDetail": live_detail,
        "telemetryDensity": telemetry,
        "supportedLevels": ["om", "active", "full"],
        "surfaces": {
            "dashboard": policy.surfaces.dashboard,
            "instruments": policy.surfaces.instruments,
            "footer": policy.surfaces.footer,
            "activity": policy.surfaces.activity,
        },
    })
}

/// Versioned action request envelope for replay/runtime boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiActionEnvelope {
    pub protocol_version: u32,
    pub session_id: String,
    pub client_id: String,
    pub action_id: String,
    pub action: Value,
}

impl UiActionEnvelope {
    pub fn new(
        session_id: impl Into<String>,
        client_id: impl Into<String>,
        action_id: impl Into<String>,
        action: Value,
    ) -> Self {
        Self {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id: session_id.into(),
            client_id: client_id.into(),
            action_id: action_id.into(),
            action,
        }
    }
}

/// Versioned action outcome envelope for replay/runtime boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiActionOutcomeEnvelope {
    pub protocol_version: u32,
    pub session_id: String,
    pub action_id: String,
    pub status: UiActionOutcomeStatus,
    pub revision_after: Option<u64>,
    pub message: Option<String>,
    pub error: Option<String>,
}

impl UiActionOutcomeEnvelope {
    pub fn accepted(
        session_id: impl Into<String>,
        action_id: impl Into<String>,
        revision_after: Option<u64>,
        message: Option<String>,
    ) -> Self {
        Self {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id: session_id.into(),
            action_id: action_id.into(),
            status: UiActionOutcomeStatus::Accepted,
            revision_after,
            message,
            error: None,
        }
    }

    pub fn rejected(
        session_id: impl Into<String>,
        action_id: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id: session_id.into(),
            action_id: action_id.into(),
            status: UiActionOutcomeStatus::Rejected,
            revision_after: None,
            message: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiActionOutcomeStatus {
    Accepted,
    Rejected,
    Noop,
    Deferred,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_envelope_uses_camel_case_wire_shape() {
        let envelope = SurfaceEnvelope::new(
            "session-1",
            UiSurfaceKind::Conversation,
            7,
            serde_json::json!({ "segments": [] }),
        );

        let value = serde_json::to_value(envelope).expect("serialize envelope");
        assert_eq!(value["protocolVersion"], UI_RUNTIME_ENVELOPE_VERSION);
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["surface"], "conversation");
        assert_eq!(value["revision"], 7);
        assert_eq!(value["payload"]["segments"], serde_json::json!([]));
    }

    #[test]
    fn presentation_envelope_advertises_semantic_levels_and_density() {
        let envelope = SurfaceEnvelope::new(
            "session-1",
            UiSurfaceKind::Presentation,
            9,
            presentation_payload(UiPresentationPolicy::active()),
        );
        let value = serde_json::to_value(envelope).expect("serialize envelope");
        assert_eq!(value["surface"], "presentation");
        assert_eq!(value["payload"]["level"], "active");
        assert_eq!(value["payload"]["liveDetail"], "workflow");
        assert_eq!(
            value["payload"]["supportedLevels"],
            serde_json::json!(["om", "active", "full"])
        );
    }

    #[test]
    fn action_outcome_envelope_separates_message_from_error() {
        let accepted = UiActionOutcomeEnvelope::accepted(
            "session-1",
            "action-1",
            Some(8),
            Some("done".into()),
        );
        let rejected = UiActionOutcomeEnvelope::rejected("session-1", "action-2", "bad action");

        let accepted = serde_json::to_value(accepted).expect("serialize accepted");
        let rejected = serde_json::to_value(rejected).expect("serialize rejected");

        assert_eq!(accepted["status"], "accepted");
        assert_eq!(accepted["revisionAfter"], 8);
        assert_eq!(accepted["message"], "done");
        assert!(accepted["error"].is_null());
        assert_eq!(rejected["status"], "rejected");
        assert_eq!(rejected["error"], "bad action");
    }
}

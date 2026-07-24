use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::features::delegate::{
    DelegateChildFailureKind, DelegateTask, DelegateTaskStatus,
};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
pub const MAX_REASON_BYTES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Envelope<T> {
    pub schema_version: u32,
    pub managed_run_id: String,
    pub worker_id: String,
    #[serde(flatten)]
    pub body: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchBody {
    pub request: DispatchRequest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DispatchRequest {
    pub directive: String,
    pub worker_profile: WorkerProfile,
    #[serde(default)]
    pub scope: std::collections::BTreeSet<String>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub supervisor_deadline_seconds: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerProfile { Scout, Patch, Verify }

impl WorkerProfile {
    pub fn as_str(self) -> &'static str {
        match self { Self::Scout => "scout", Self::Patch => "patch", Self::Verify => "verify" }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TaskBody { pub task_id: String }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CancelBody { pub task_id: String, pub reason: Option<String> }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SupervisorRejection {
    pub code: String,
    pub safe_message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DelegateDispatchResponse {
    pub schema_version: u32,
    pub managed_run_id: String,
    pub worker_id: String,
    pub accepted: bool,
    pub task_id: Option<String>,
    pub effective_policy: Option<EffectivePolicy>,
    pub rejection: Option<SupervisorRejection>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DelegateObservationResponse {
    pub schema_version: u32,
    pub managed_run_id: String,
    pub worker_id: String,
    pub observation: DelegateObservation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DelegateResultResponse {
    pub schema_version: u32,
    pub managed_run_id: String,
    pub worker_id: String,
    pub task_id: String,
    pub result: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DelegateCancelResponse {
    pub schema_version: u32,
    pub managed_run_id: String,
    pub worker_id: String,
    pub task_id: String,
    pub acknowledged: bool,
    pub termination_confirmed: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SupervisorErrorResponse {
    pub schema_version: u32,
    pub managed_run_id: String,
    pub worker_id: String,
    pub rejection: SupervisorRejection,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DelegateObservation {
    pub task_id: String,
    pub label: Option<String>,
    pub agent_name: Option<String>,
    pub task_description: String,
    pub status: DelegateWireStatus,
    pub result: Option<String>,
    pub result_viewed: bool,
    pub started_at_unix_ms: u64,
    pub completed_at_unix_ms: Option<u64>,
    pub last_tool: Option<ToolActivityObservation>,
    pub last_turn: Option<u32>,
    pub checklist: Vec<ChecklistItemObservation>,
    pub route: Option<RouteObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DelegateWireStatus {
    Running,
    Completed { success: bool },
    Failed { failure_kind: FailureKind, safe_message: String },
    Cancelled { reason: Option<String>, termination_confirmed: bool },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    MissingLocalModel,
    MissingCredential,
    ProviderStartup,
    WorkspaceStartup,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolActivityObservation {
    pub tool: String,
    pub args_summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChecklistItemObservation {
    pub label: String,
    pub done: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RouteObservation {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub fallback_used: bool,
}

impl From<DelegateChildFailureKind> for FailureKind {
    fn from(value: DelegateChildFailureKind) -> Self {
        match value {
            DelegateChildFailureKind::MissingLocalModel => Self::MissingLocalModel,
            DelegateChildFailureKind::MissingCredential => Self::MissingCredential,
            DelegateChildFailureKind::ProviderStartup => Self::ProviderStartup,
            DelegateChildFailureKind::WorkspaceStartup => Self::WorkspaceStartup,
            DelegateChildFailureKind::Unknown => Self::Unknown,
        }
    }
}

impl From<&DelegateTaskStatus> for DelegateWireStatus {
    fn from(value: &DelegateTaskStatus) -> Self {
        match value {
            DelegateTaskStatus::Running => Self::Running,
            DelegateTaskStatus::Completed { success } => Self::Completed { success: *success },
            DelegateTaskStatus::Failed { error, kind } => Self::Failed {
                failure_kind: (*kind).into(),
                safe_message: error.lines().next().unwrap_or("Delegate worker failed").to_string(),
            },
            DelegateTaskStatus::Cancelled { reason, termination_confirmed } => Self::Cancelled {
                reason: reason.clone(),
                termination_confirmed: *termination_confirmed,
            },
        }
    }
}

impl From<DelegateTask> for DelegateObservation {
    fn from(task: DelegateTask) -> Self {
        let status = DelegateWireStatus::from(&task.status);
        let route = task.route_decision.map(|route| RouteObservation {
            provider: Some(crate::providers::infer_provider_id(&route.selected_model)),
            model: Some(route.selected_model),
            fallback_used: route.fallback_reason.is_some(),
        });
        Self {
            task_id: task.task_id,
            label: task.label,
            agent_name: task.agent_name,
            task_description: task.task_description,
            status,
            result: task.result,
            result_viewed: task.result_viewed,
            started_at_unix_ms: unix_millis(task.started_at),
            completed_at_unix_ms: task.completed_at.map(unix_millis),
            last_tool: task.last_tool.map(|tool| ToolActivityObservation {
                tool,
                args_summary: task.last_tool_activity.and_then(|activity| activity.args_summary),
            }),
            last_turn: task.last_turn,
            checklist: task.tasks.into_iter().map(|item| ChecklistItemObservation {
                label: item.description,
                done: item.done,
            }).collect(),
            route,
        }
    }
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub worker_profile: WorkerProfile,
    pub max_turns: u32,
    pub wall_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub enabled_tools: Vec<String>,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
}

pub fn validate_identity<T>(request: &Envelope<T>) -> Result<(), String> {
    if request.schema_version != SCHEMA_VERSION { return Err(format!("unsupported_schema_version:{}", request.schema_version)); }
    if request.managed_run_id.trim().is_empty() || request.worker_id.trim().is_empty() { return Err("missing_identity".into()); }
    Ok(())
}

#[derive(Debug)]
pub enum SupervisorOperation {
    Execute { tool: &'static str, args: Value },
    GetObservation { task_id: String },
}

pub fn parse_operation(method: &str, payload: &Value) -> Result<(Envelope<Value>, SupervisorOperation), String> {
    let envelope: Envelope<Value> = serde_json::from_value(payload.clone()).map_err(|e| format!("invalid_request:{e}"))?;
    validate_identity(&envelope)?;
    let body = &envelope.body;
    let operation = match method {
        "delegate_dispatch" => {
            let body: DispatchBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_dispatch:{e}"))?;
            if body.request.directive.trim().is_empty() || body.request.supervisor_deadline_seconds == 0 { return Err("invalid_dispatch_request".into()); }
            SupervisorOperation::Execute { tool: crate::tool_registry::delegate::DELEGATE, args: serde_json::json!({
                "task": body.request.directive,
                "worker_profile": body.request.worker_profile.as_str(),
                "scope": body.request.scope,
                "model": body.request.model,
                "thinking_level": body.request.thinking_level,
                "background": true
            }) }
        }
        "delegate_get" => {
            let body: TaskBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_task_request:{e}"))?;
            SupervisorOperation::GetObservation { task_id: body.task_id }
        }
        "delegate_result" => {
            let body: TaskBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_task_request:{e}"))?;
            SupervisorOperation::Execute { tool: crate::tool_registry::delegate::DELEGATE_RESULT, args: serde_json::json!({"task_id": body.task_id}) }
        }
        "delegate_cancel" => {
            let body: CancelBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_cancel_request:{e}"))?;
            if body.reason.as_ref().is_some_and(|r| r.len() > MAX_REASON_BYTES) { return Err("oversized_reason".into()); }
            SupervisorOperation::Execute { tool: crate::tool_registry::delegate::DELEGATE_CANCEL, args: serde_json::json!({"task_id": body.task_id, "reason": body.reason}) }
        }
        _ => return Err("unsupported_method".into()),
    };
    Ok((envelope, operation))
}

pub fn observation_response(envelope: &Envelope<Value>, observation: DelegateObservation) -> Value {
    serde_json::to_value(DelegateObservationResponse {
        schema_version: SCHEMA_VERSION,
        managed_run_id: envelope.managed_run_id.clone(),
        worker_id: envelope.worker_id.clone(),
        observation,
    }).expect("supervisor observation response serializes")
}

pub fn error_response(_method: &str, envelope: &Envelope<Value>, error: &str) -> Value {
    serde_json::to_value(SupervisorErrorResponse {
        schema_version: SCHEMA_VERSION,
        managed_run_id: envelope.managed_run_id.clone(),
        worker_id: envelope.worker_id.clone(),
        rejection: SupervisorRejection { code: error.to_string(), safe_message: safe_error_message(error) },
    }).expect("supervisor error response serializes")
}

fn safe_error_message(error: &str) -> String {
    match error {
        "unknown_task" => "The requested delegate task does not exist".into(),
        "unsupported_schema" => "The supervisor schema version is unsupported".into(),
        "oversized_result" => "The delegate result exceeds the transport limit".into(),
        "oversized_reason" => "The cancellation reason exceeds the transport limit".into(),
        _ => "The managed delegate request could not be completed".into(),
    }
}

pub fn response(method: &str, envelope: &Envelope<Value>, result: omegon_traits::ToolResult) -> Result<Value, String> {
    let details = result.details;
    let task_id = details.get("task_id").and_then(Value::as_str).map(str::to_string);
    let text = result.content.iter().find_map(|block| match block { omegon_traits::ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap_or_default();
    if method == "delegate_result" && text.len() > MAX_RESULT_BYTES { return Err("oversized_result".into()); }
    let response = match method {
        "delegate_dispatch" => serde_json::to_value(DelegateDispatchResponse {
            schema_version: SCHEMA_VERSION,
            managed_run_id: envelope.managed_run_id.clone(),
            worker_id: envelope.worker_id.clone(),
            accepted: task_id.is_some(),
            task_id,
            effective_policy: details.get("effective_policy").cloned().map(serde_json::from_value).transpose().map_err(|error| format!("invalid_effective_policy:{error}"))?,
            rejection: None,
        }),
        "delegate_result" => serde_json::to_value(DelegateResultResponse {
            schema_version: SCHEMA_VERSION,
            managed_run_id: envelope.managed_run_id.clone(),
            worker_id: envelope.worker_id.clone(),
            task_id: task_id.ok_or_else(|| "missing_task_id".to_string())?,
            result: text,
        }),
        "delegate_cancel" => serde_json::to_value(DelegateCancelResponse {
            schema_version: SCHEMA_VERSION,
            managed_run_id: envelope.managed_run_id.clone(),
            worker_id: envelope.worker_id.clone(),
            task_id: task_id.ok_or_else(|| "missing_task_id".to_string())?,
            acknowledged: true,
            termination_confirmed: details.get("termination_confirmed").and_then(Value::as_bool).unwrap_or(false),
            reason: details.get("reason").and_then(Value::as_str).map(str::to_string),
        }),
        _ => return Err("unsupported_method".into()),
    };
    response.map_err(|error| format!("response_serialization:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cross_contract_versions_and_oversized_reasons() {
        let bad = serde_json::json!({"schema_version": 2, "managed_run_id":"r", "worker_id":"w", "task_id":"delegate_1"});
        assert!(parse_operation("delegate_get", &bad).unwrap_err().contains("unsupported_schema"));
        let reason = "x".repeat(MAX_REASON_BYTES + 1);
        let bad = serde_json::json!({"schema_version": 1, "managed_run_id":"r", "worker_id":"w", "task_id":"delegate_1", "reason":reason});
        assert_eq!(parse_operation("delegate_cancel", &bad).unwrap_err(), "oversized_reason");
    }

    #[test]
    fn responses_match_auspex_flat_envelope() {
        let envelope = envelope();
        let response = observation_response(&envelope, observation(DelegateWireStatus::Running, None));
        assert_eq!(response["observation"]["task_id"], "delegate_1");
        assert!(response.get("body").is_none());
    }

    fn envelope() -> Envelope<Value> {
        Envelope { schema_version: 1, managed_run_id: "11111111-1111-4111-8111-111111111111".into(), worker_id: "22222222-2222-4222-8222-222222222222".into(), body: Value::Null }
    }

    fn observation(status: DelegateWireStatus, result: Option<&str>) -> DelegateObservation {
        DelegateObservation {
            task_id: "delegate_1".into(), label: Some("worker".into()), agent_name: Some("scout".into()),
            task_description: "inspect".into(), status, result: result.map(str::to_string), result_viewed: false,
            started_at_unix_ms: 1000, completed_at_unix_ms: None, last_tool: None, last_turn: None,
            checklist: vec![], route: None,
        }
    }

    #[test]
    fn golden_fixtures_match_typed_serialization() {
        let cases = [
            ("01-running.json", observation(DelegateWireStatus::Running, None)),
            ("02-completed-success.json", DelegateObservation { completed_at_unix_ms: Some(2000), ..observation(DelegateWireStatus::Completed { success: true }, Some("done")) }),
            ("03-completed-unsuccessful.json", DelegateObservation { completed_at_unix_ms: Some(2000), ..observation(DelegateWireStatus::Completed { success: false }, Some("checks failed")) }),
            ("04-typed-failure.json", DelegateObservation { completed_at_unix_ms: Some(2000), ..observation(DelegateWireStatus::Failed { failure_kind: FailureKind::ProviderStartup, safe_message: "provider unavailable".into() }, None) }),
            ("05-cancel-acknowledged.json", observation(DelegateWireStatus::Cancelled { reason: Some("operator request".into()), termination_confirmed: false }, None)),
            ("06-cancel-confirmed.json", DelegateObservation { completed_at_unix_ms: Some(2000), ..observation(DelegateWireStatus::Cancelled { reason: Some("operator request".into()), termination_confirmed: true }, None) }),
        ];
        for (name, observation) in cases {
            let actual = observation_response(&envelope(), observation);
            let expected: Value = serde_json::from_str(fixture(name)).unwrap();
            assert_eq!(actual, expected, "fixture {name}");
        }
    }

    fn fixture(name: &str) -> &'static str {
        match name {
            "01-running.json" => include_str!("../tests/fixtures/managed_agent/01-running.json"),
            "02-completed-success.json" => include_str!("../tests/fixtures/managed_agent/02-completed-success.json"),
            "03-completed-unsuccessful.json" => include_str!("../tests/fixtures/managed_agent/03-completed-unsuccessful.json"),
            "04-typed-failure.json" => include_str!("../tests/fixtures/managed_agent/04-typed-failure.json"),
            "05-cancel-acknowledged.json" => include_str!("../tests/fixtures/managed_agent/05-cancel-acknowledged.json"),
            "06-cancel-confirmed.json" => include_str!("../tests/fixtures/managed_agent/06-cancel-confirmed.json"),
            _ => unreachable!(),
        }
    }
}

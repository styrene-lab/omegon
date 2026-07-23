use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Serialize)]
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

pub fn tool_args(method: &str, payload: &Value) -> Result<(Envelope<Value>, &'static str, Value), String> {
    let envelope: Envelope<Value> = serde_json::from_value(payload.clone()).map_err(|e| format!("invalid_request:{e}"))?;
    validate_identity(&envelope)?;
    let body = &envelope.body;
    let (tool, args) = match method {
        "delegate_dispatch" => {
            let body: DispatchBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_dispatch:{e}"))?;
            if body.request.directive.trim().is_empty() || body.request.supervisor_deadline_seconds == 0 { return Err("invalid_dispatch_request".into()); }
            (crate::tool_registry::delegate::DELEGATE, serde_json::json!({
                "task": body.request.directive,
                "worker_profile": body.request.worker_profile.as_str(),
                "scope": body.request.scope,
                "model": body.request.model,
                "thinking_level": body.request.thinking_level,
                "background": true
            }))
        }
        "delegate_get" => {
            let body: TaskBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_task_request:{e}"))?;
            (crate::tool_registry::delegate::DELEGATE_STATUS, serde_json::json!({"task_id": body.task_id}))
        }
        "delegate_result" => {
            let body: TaskBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_task_request:{e}"))?;
            (crate::tool_registry::delegate::DELEGATE_RESULT, serde_json::json!({"task_id": body.task_id}))
        }
        "delegate_cancel" => {
            let body: CancelBody = serde_json::from_value(body.clone()).map_err(|e| format!("invalid_cancel_request:{e}"))?;
            if body.reason.as_ref().is_some_and(|r| r.len() > MAX_REASON_BYTES) { return Err("oversized_reason".into()); }
            (crate::tool_registry::delegate::DELEGATE_CANCEL, serde_json::json!({"task_id": body.task_id, "reason": body.reason}))
        }
        _ => return Err("unsupported_method".into()),
    };
    Ok((envelope, tool, args))
}

pub fn observation_response(envelope: &Envelope<Value>, observation: Value) -> Value {
    serde_json::json!({"type": "delegate_get_result", "schema_version": SCHEMA_VERSION, "managed_run_id": envelope.managed_run_id, "worker_id": envelope.worker_id, "observation": observation})
}

pub fn error_response(method: &str, envelope: &Envelope<Value>, error: &str) -> Value {
    serde_json::json!({"type": format!("{method}_result"), "schema_version": SCHEMA_VERSION, "managed_run_id": envelope.managed_run_id, "worker_id": envelope.worker_id, "accepted": false, "rejection": {"code": error, "safe_message": error}})
}

pub fn response(method: &str, envelope: &Envelope<Value>, result: omegon_traits::ToolResult) -> Result<Value, String> {
    let details = result.details;
    let task_id = details.get("task_id").and_then(Value::as_str).map(str::to_string);
    let text = result.content.iter().find_map(|block| match block { omegon_traits::ContentBlock::Text { text } => Some(text.clone()), _ => None }).unwrap_or_default();
    if method == "delegate_result" && text.len() > MAX_RESULT_BYTES { return Err("oversized_result".into()); }
    let body = match method {
        "delegate_dispatch" => serde_json::json!({"accepted": task_id.is_some(), "task_id": task_id, "effective_policy": details.get("effective_policy"), "rejection": Value::Null}),
        "delegate_get" => serde_json::json!({"observation": details}),
        "delegate_result" => serde_json::json!({"task_id": task_id, "result": text}),
        "delegate_cancel" => serde_json::json!({"task_id": task_id, "acknowledged": true, "termination_confirmed": details.get("termination_confirmed").and_then(Value::as_bool).unwrap_or(false), "reason": details.get("reason")}),
        _ => return Err("unsupported_method".into()),
    };
    let mut response = serde_json::json!({"type": format!("{method}_result"), "schema_version": SCHEMA_VERSION, "managed_run_id": envelope.managed_run_id, "worker_id": envelope.worker_id});
    if let (Some(target), Value::Object(fields)) = (response.as_object_mut(), body) {
        target.extend(fields);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cross_contract_versions_and_oversized_reasons() {
        let bad = serde_json::json!({"schema_version": 2, "managed_run_id":"r", "worker_id":"w", "task_id":"delegate_1"});
        assert!(tool_args("delegate_get", &bad).unwrap_err().contains("unsupported_schema"));
        let reason = "x".repeat(MAX_REASON_BYTES + 1);
        let bad = serde_json::json!({"schema_version": 1, "managed_run_id":"r", "worker_id":"w", "task_id":"delegate_1", "reason":reason});
        assert_eq!(tool_args("delegate_cancel", &bad).unwrap_err(), "oversized_reason");
    }

    #[test]
    fn responses_match_auspex_flat_envelope() {
        let envelope = Envelope { schema_version: 1, managed_run_id: "run".into(), worker_id: "worker".into(), body: Value::Null };
        let response = observation_response(&envelope, serde_json::json!({"task_id":"delegate_1"}));
        assert_eq!(response["observation"]["task_id"], "delegate_1");
        assert!(response.get("body").is_none());
    }
}

use std::path::Path;

use serde_json::Value;

pub fn projection_json(repo_root: &Path) -> Value {
    let projection = crate::tools::lifecycle_plan_projection(repo_root);
    serde_json::json!({
        "plans": projection.entries,
        "tasks": projection.tasks,
        "task_identity_findings": projection.task_identity_findings,
    })
}

pub fn plan_show_json(projection: &Value, plan_id: &str) -> Value {
    let plans = projection
        .get("plans")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let task_identity_findings = projection
        .get("task_identity_findings")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let tasks = projection
        .get("tasks")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|task| task.get("plan_id").and_then(|v| v.as_str()) == Some(plan_id))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::json!({
        "plan": plans.as_array().and_then(|items| items.iter().find(|plan| plan.get("plan_id").and_then(|v| v.as_str()) == Some(plan_id))).cloned(),
        "tasks": tasks,
        "task_identity_findings": task_identity_findings,
        "stale_copy": if plan_id.is_empty() { Value::Null } else { serde_json::json!(crate::conversation::STALE_PLAN_COPY) },
    })
}

pub fn task_list_json(projection: &Value, params: &Value) -> Value {
    let plan_filter = params
        .get("plan_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let source_filter = params
        .get("source")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let status_filter = params
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let task_identity_findings = projection
        .get("task_identity_findings")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let tasks = projection
        .get("tasks")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|task| {
                    plan_filter.is_none_or(|plan_id| {
                        task.get("plan_id").and_then(|v| v.as_str()) == Some(plan_id)
                    })
                })
                .filter(|task| {
                    source_filter.is_none_or(|source| {
                        task.get("source")
                            .and_then(|v| v.get("kind"))
                            .and_then(|v| v.as_str())
                            == Some(source)
                    })
                })
                .filter(|task| {
                    status_filter.is_none_or(|status| {
                        task.get("status").and_then(|v| v.as_str()) == Some(status)
                    })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::json!({
        "tasks": tasks,
        "filters": {
            "plan_id": plan_filter,
            "source": source_filter,
            "status": status_filter
        },
        "pagination": {
            "supported": false,
            "cursor": Value::Null
        },
        "task_identity_findings": task_identity_findings
    })
}

pub fn task_show_json(projection: &Value, task_id: &str) -> Value {
    let task = projection
        .get("tasks")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            items.iter().find(|task| {
                task.get("id").and_then(|v| v.as_str()) == Some(task_id)
                    || task.get("stable_id").and_then(|v| v.as_str()) == Some(task_id)
            })
        })
        .cloned();
    serde_json::json!({ "task": task })
}

pub fn task_events_json(bindings: &[crate::conversation::SessionTaskBinding]) -> Value {
    serde_json::json!({
        "events": bindings.iter().map(|binding| serde_json::json!({
            "type": "task.bound",
            "task_id": binding.task_id,
            "stable_id": binding.stable_id,
            "revision": binding.revision,
            "system": binding.system,
            "external_task_id": binding.external_task_id,
            "durability": "session"
        })).collect::<Vec<_>>(),
        "durability": "session",
        "note": "Session task binding events are local hints; repo-durable event cursors are not implemented yet."
    })
}

pub fn task_error(code: &str, error: &str, revision: Option<&str>) -> Value {
    let mut value = serde_json::json!({
        "accepted": false,
        "durability": "none",
        "code": code,
        "error": error,
    });
    if let Some(revision) = revision {
        value["revision"] = serde_json::json!(revision);
    }
    value
}

pub fn requested_bind_durability(params: &Value) -> crate::conversation::TaskBindingDurability {
    match params
        .get("requested_durability")
        .and_then(|v| v.as_str())
        .unwrap_or("session")
    {
        "repo" => crate::conversation::TaskBindingDurability::Repo,
        "none" => crate::conversation::TaskBindingDurability::None,
        _ => crate::conversation::TaskBindingDurability::Session,
    }
}

pub fn sanitize_external_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

pub fn external_import_revision(
    system: &str,
    external_id: &str,
    title: &str,
    body: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(system.as_bytes());
    hasher.update(b"\0");
    hasher.update(external_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(title.as_bytes());
    hasher.update(b"\0");
    hasher.update(body.as_bytes());
    format!("external-v1:sha256:{:x}", hasher.finalize())
}

pub fn current_binding_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection_fixture() -> Value {
        serde_json::json!({
            "plans": [{ "plan_id": "openspec:demo", "title": "demo" }],
            "tasks": [
                {
                    "id": "openspec:demo:group:1.1",
                    "stable_id": "stable-one",
                    "plan_id": "openspec:demo",
                    "status": "pending",
                    "source": { "kind": "openspec", "path": "openspec/changes/demo/tasks.md", "anchor": "1.1" }
                },
                {
                    "id": "design:node:question:1",
                    "stable_id": "design:node:question:1",
                    "plan_id": "design:node",
                    "status": "done",
                    "source": { "kind": "design", "path": "docs/node.md", "anchor": "question:1" }
                }
            ],
            "task_identity_findings": [{ "line": 2, "task_id": "1.2", "stable_id": "dup", "message": "duplicate" }]
        })
    }

    #[test]
    fn task_list_filters_by_plan_source_and_status() {
        let projection = projection_fixture();
        let response = task_list_json(
            &projection,
            &serde_json::json!({
                "plan_id": "openspec:demo",
                "source": "openspec",
                "status": "pending"
            }),
        );
        assert_eq!(response["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(response["tasks"][0]["stable_id"], "stable-one");
        assert_eq!(response["filters"]["source"], "openspec");
        assert_eq!(response["pagination"]["supported"], false);
        assert_eq!(
            response["task_identity_findings"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn task_show_accepts_projection_or_stable_id() {
        let projection = projection_fixture();
        let response = task_show_json(&projection, "openspec:demo:group:1.1");
        assert_eq!(response["task"]["stable_id"], "stable-one");
        let stable = task_show_json(&projection, "stable-one");
        assert_eq!(stable["task"]["id"], "openspec:demo:group:1.1");
        let missing = task_show_json(&projection, "missing");
        assert!(missing["task"].is_null());
    }

    #[test]
    fn requested_bind_durability_defaults_to_session() {
        assert_eq!(
            requested_bind_durability(&serde_json::json!({})),
            crate::conversation::TaskBindingDurability::Session
        );
        assert_eq!(
            requested_bind_durability(&serde_json::json!({ "requested_durability": "repo" })),
            crate::conversation::TaskBindingDurability::Repo
        );
    }

    #[test]
    fn sanitize_external_ids_for_stable_session_imports() {
        assert_eq!(sanitize_external_id("flynt task/1"), "flynt-task-1");
    }
}

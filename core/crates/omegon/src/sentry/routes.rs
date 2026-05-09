use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::mpsc;

use super::board::TaskBoard;
use super::state_db::StateDb;
use crate::triggers::TriggerEvent;

#[derive(Clone)]
pub struct SentryState {
    pub board: Arc<dyn TaskBoard>,
    pub state_db: Arc<StateDb>,
    pub event_tx: mpsc::Sender<TriggerEvent>,
}

pub fn sentry_router(state: SentryState) -> Router {
    Router::new()
        .route("/api/sentry/tasks", get(list_tasks))
        .route("/api/sentry/tasks/{id}", get(get_task))
        .route("/api/sentry/tasks/{id}/run", post(run_task))
        .route("/api/sentry/trigger/{name}", post(fire_trigger))
        .with_state(state)
}

#[derive(Serialize)]
struct TaskListItem {
    id: String,
    name: String,
    priority: u8,
    triggers: Vec<String>,
    last_run: Option<String>,
    run_count: u32,
    claimed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Serialize)]
struct TaskDetail {
    #[serde(flatten)]
    info: TaskListItem,
    runs: Vec<super::types::RunRecord>,
}

#[derive(Serialize)]
struct RunResponse {
    queued: bool,
    task_id: String,
}

#[derive(Serialize)]
struct TriggerResponse {
    fired: bool,
    trigger_name: String,
}

async fn list_tasks(State(state): State<SentryState>) -> impl IntoResponse {
    let tasks = match state.board.list_actionable() {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "failed to list sentry tasks");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(Vec::<TaskListItem>::new())).into_response();
        }
    };

    let items: Vec<TaskListItem> = tasks.iter().map(|t| {
        let trigger_summaries: Vec<String> = t.triggers.iter().map(|tr| match tr {
            super::types::Trigger::Cron { schedule } => format!("cron:{schedule}"),
            super::types::Trigger::Webhook { name } => format!("webhook:{name}"),
            super::types::Trigger::FileWatch { paths, .. } => {
                format!("file_watch:{}", paths.len())
            }
            super::types::Trigger::GitEvent { events, .. } => {
                format!("git:{}", events.len())
            }
            super::types::Trigger::Manual => "manual".into(),
        }).collect();

        let claimed = state.state_db.is_claimed(&t.id).unwrap_or(false);

        TaskListItem {
            id: t.id.clone(),
            name: t.name.clone(),
            priority: t.priority,
            triggers: trigger_summaries,
            last_run: t.last_run.map(|dt| dt.to_rfc3339()),
            run_count: t.run_count,
            claimed,
            warning: None,
        }
    }).collect();

    Json(items).into_response()
}

async fn get_task(
    State(state): State<SentryState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tasks = match state.board.list_actionable() {
        Ok(t) => t,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let Some(task) = tasks.iter().find(|t| t.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let runs = state.state_db.run_history(&id, 50).unwrap_or_default();
    let claimed = state.state_db.is_claimed(&id).unwrap_or(false);

    let trigger_summaries: Vec<String> = task.triggers.iter().map(|tr| match tr {
        super::types::Trigger::Cron { schedule } => format!("cron:{schedule}"),
        super::types::Trigger::Webhook { name } => format!("webhook:{name}"),
        super::types::Trigger::FileWatch { paths, .. } => format!("file_watch:{}", paths.len()),
        super::types::Trigger::GitEvent { events, .. } => format!("git:{}", events.len()),
        super::types::Trigger::Manual => "manual".into(),
    }).collect();

    Json(TaskDetail {
        info: TaskListItem {
            id: task.id.clone(),
            name: task.name.clone(),
            priority: task.priority,
            triggers: trigger_summaries,
            last_run: task.last_run.map(|dt| dt.to_rfc3339()),
            run_count: task.run_count,
            claimed,
            warning: None,
        },
        runs,
    }).into_response()
}

async fn run_task(
    State(state): State<SentryState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.event_tx.send(TriggerEvent::ForceRun { task_id: id.clone() }).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(RunResponse {
            queued: false,
            task_id: id,
        })).into_response();
    }

    Json(RunResponse {
        queued: true,
        task_id: id,
    }).into_response()
}

async fn fire_trigger(
    State(state): State<SentryState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if state.event_tx.send(TriggerEvent::Webhook {
        name: name.clone(),
        payload: serde_json::Value::Null,
    }).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(TriggerResponse {
            fired: false,
            trigger_name: name,
        })).into_response();
    }

    Json(TriggerResponse {
        fired: true,
        trigger_name: name,
    }).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_response_serializes() {
        let resp = TriggerResponse { fired: true, trigger_name: "deploy".into() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"fired\":true"));
    }

    #[test]
    fn run_response_serializes() {
        let resp = RunResponse { queued: true, task_id: "pr-review".into() };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"queued\":true"));
    }
}

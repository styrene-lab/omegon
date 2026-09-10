//! Frontend compatibility policy for loop permission and operator waits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use omegon_traits::{AgentEvent, ContentBlock};

use crate::loop_driver::{
    LoopInternalInvocationRequest, LoopInvocationContract, LoopToolApprovalRequest,
    LoopToolOwnerRetryRequest, LoopToolPresentation, LoopToolPresentationRequest,
};

#[derive(Debug)]
pub(crate) struct PermissionRecord {
    pub(crate) tool_name: String,
    pub(crate) path: String,
    pub(crate) decision: String,
    pub(crate) kind: omegon_traits::PermissionRequestKind,
    pub(crate) persistence: omegon_traits::PermissionPersistence,
    pub(crate) grant_path: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct LoopInvocationFrontend {
    host: Option<Arc<crate::host_context::HostContext>>,
}

impl LoopInvocationFrontend {
    pub(crate) fn acp(host: Arc<crate::host_context::HostContext>) -> Self {
        Self { host: Some(host) }
    }

    pub(crate) fn host_context(&self) -> Option<&crate::host_context::HostContext> {
        self.host.as_deref()
    }

    pub(crate) async fn acquire_tool_approval(
        &self,
        request: LoopToolApprovalRequest<'_>,
    ) -> Result<
        crate::invocation_service::ExecutionLease,
        crate::invocation_service::InvocationDenial,
    > {
        let requested = request.pending.requested.clone();
        let response = self
            .permission_response(
                request.visible_call_id,
                request.visible_tool_name,
                requested.clone(),
                request.events,
                request.cancel,
                omegon_traits::PermissionRequestKind::Policy,
                omegon_traits::PermissionPersistence::None,
                None,
            )
            .await;
        let approved = matches!(
            response,
            omegon_traits::PermissionResponse::Allow
                | omegon_traits::PermissionResponse::AllowSession
                | omegon_traits::PermissionResponse::AlwaysAllow
        );
        request.permission_log.push(PermissionRecord {
            tool_name: request.visible_tool_name.into(),
            path: requested,
            decision: if approved { "allow" } else { "deny" }.into(),
            kind: omegon_traits::PermissionRequestKind::Policy,
            persistence: omegon_traits::PermissionPersistence::None,
            grant_path: None,
        });
        request.pending.decide(approved)
    }

    pub(crate) fn tool_execution_context(&self) -> omegon_traits::ToolExecutionContext {
        let Some(host) = self.host.as_ref() else {
            return omegon_traits::ToolExecutionContext::default();
        };
        let proxy = host.proxy.clone();
        let approval_sink: omegon_traits::HostActionApprovalSink = Arc::new(move |request_json| {
            let proxy = proxy.clone();
            Box::pin(async move {
                let request = match serde_json::from_value::<
                    agent_client_protocol::schema::RequestPermissionRequest,
                >(request_json)
                {
                    Ok(request) => request,
                    Err(_) => {
                        return serde_json::to_value(
                            crate::extensions::approval::HostActionApprovalDecision::Unavailable,
                        )
                        .unwrap_or(serde_json::Value::String("unavailable".into()));
                    }
                };
                let decision = proxy.request_host_action_approval(request).await.unwrap_or(
                    crate::extensions::approval::HostActionApprovalDecision::Unavailable,
                );
                serde_json::to_value(decision)
                    .unwrap_or(serde_json::Value::String("unavailable".into()))
            })
        });
        omegon_traits::ToolExecutionContext {
            host_action_approval: Some(approval_sink),
            ..Default::default()
        }
    }

    pub(crate) async fn present_tool_owner_result(
        &self,
        invocations: &dyn LoopInvocationContract,
        request: LoopToolPresentationRequest<'_>,
    ) -> LoopToolPresentation {
        let error = match request.result {
            Ok(result) => {
                let is_error = result
                    .details
                    .get("is_error")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                return LoopToolPresentation::Resolved(result, is_error);
            }
            Err(error) => error,
        };

        if let Some(confirmation) =
            error
                .downcast_ref::<crate::features::memory::confirmation::MemoryConfirmationRequired>()
        {
            let approved = self
                .approve_memory_candidate(
                    confirmation,
                    request.visible_call_id,
                    request.events,
                    request.cancel.clone(),
                )
                .await;
            request.permission_log.push(PermissionRecord {
                tool_name: "memory_confirm".into(),
                path: format!("memory snapshot {}", confirmation.request.snapshot_hash),
                decision: if approved.is_some() {
                    "allow_once"
                } else {
                    "deny"
                }
                .into(),
                kind: omegon_traits::PermissionRequestKind::Policy,
                persistence: omegon_traits::PermissionPersistence::None,
                grant_path: None,
            });
            let Some(args) = approved else {
                return LoopToolPresentation::Resolved(omegon_traits::ToolResult {content:vec![ContentBlock::Text {text:"Candidate confirmation was denied, cancelled, or unavailable; memory was not changed.".into()}],details:serde_json::json!({"status":"not_confirmed"})},false);
            };
            let call_id = format!("{}:operator-confirmation", request.visible_call_id);
            return match invocations
                .dispatch_internal(LoopInternalInvocationRequest {
                    name: crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
                    call_id: &call_id,
                    args,
                    cancel: request.cancel,
                    principal: "kernel:memory-confirmation",
                    authority_scope: Some(request.invocation_scope),
                })
                .await
            {
                Ok(result) => LoopToolPresentation::Resolved(result, false),
                Err(error) => LoopToolPresentation::Resolved(error_result(error), true),
            };
        }

        if error
            .downcast_ref::<crate::tools::OperatorWaitRequired>()
            .is_some()
        {
            let wait = error
                .downcast::<crate::tools::OperatorWaitRequired>()
                .expect("checked operator wait downcast");
            let result = self
                .present_operator_wait(
                    wait,
                    Some(request.visible_call_id),
                    request.events,
                    request.cancel,
                    request.sink,
                )
                .await;
            let is_error = result
                .details
                .get("status")
                .and_then(serde_json::Value::as_str)
                != Some("completed");
            return LoopToolPresentation::Resolved(result, is_error);
        }

        if error
            .downcast_ref::<crate::tools::PathPermissionError>()
            .is_none()
        {
            return LoopToolPresentation::Unhandled(error);
        }
        let error = error
            .downcast::<crate::tools::PathPermissionError>()
            .expect("checked path permission downcast");
        let response = self
            .permission_response(
                request.visible_call_id,
                request.visible_tool_name,
                error.requested_path.clone(),
                request.events,
                request.cancel.clone(),
                omegon_traits::PermissionRequestKind::PathBoundary,
                omegon_traits::PermissionPersistence::ProjectDirectory,
                Some(error.directory.clone()),
            )
            .await;
        let (decision, persistence, grant_path, trust_path, scope) = match response {
            omegon_traits::PermissionResponse::Allow => (
                "allow_once",
                omegon_traits::PermissionPersistence::None,
                None,
                Some(crate::tools::canonicalize_existing_parent_for_permissions(
                    std::path::Path::new(&error.requested_path),
                )),
                "session",
            ),
            omegon_traits::PermissionResponse::AllowSession => (
                "allow_session",
                omegon_traits::PermissionPersistence::SessionDirectory,
                Some(error.directory.clone()),
                Some(std::path::PathBuf::from(&error.directory)),
                "session",
            ),
            omegon_traits::PermissionResponse::AlwaysAllow => (
                "always_allow",
                omegon_traits::PermissionPersistence::ProjectDirectory,
                Some(error.directory.clone()),
                Some(std::path::PathBuf::from(&error.directory)),
                "persistent",
            ),
            omegon_traits::PermissionResponse::Deny => (
                "deny",
                omegon_traits::PermissionPersistence::None,
                Some(error.directory.clone()),
                None,
                "session",
            ),
        };
        tracing::info!(path = %error.requested_path, decision, "permission decision");
        request.permission_log.push(PermissionRecord {
            tool_name: request.visible_tool_name.into(),
            path: error.requested_path.clone(),
            decision: decision.into(),
            kind: omegon_traits::PermissionRequestKind::PathBoundary,
            persistence,
            grant_path,
        });

        let Some(trust_path) = trust_path else {
            return LoopToolPresentation::Resolved(path_denied_result(&error), true);
        };
        let grant_call_id = format!("{}:permission-grant", request.visible_call_id);
        if let Err(error) = invocations
            .dispatch_internal(LoopInternalInvocationRequest {
                name: crate::tool_registry::core::TRUST_DIRECTORY,
                call_id: &grant_call_id,
                args: serde_json::json!({"path": trust_path, "scope": scope}),
                cancel: request.cancel.clone(),
                principal: "kernel:permission-grant",
                authority_scope: Some(request.invocation_scope),
            })
            .await
        {
            tracing::error!(%error, "permission grant persistence failed; retry may not take effect");
        }
        match invocations
            .retry_tool_owner(LoopToolOwnerRetryRequest {
                lease: request.lease,
                execution_tool_name: request.execution_tool_name,
                visible_call_id: request.visible_call_id,
                execution_args: request.execution_args,
                cancel: request.cancel,
                sink: request.sink,
                context: request.context,
            })
            .await
        {
            Ok(result) => LoopToolPresentation::Resolved(result, false),
            Err(error) => LoopToolPresentation::Resolved(error_result(error), true),
        }
    }

    async fn approve_memory_candidate(
        &self,
        confirmation: &crate::features::memory::confirmation::MemoryConfirmationRequired,
        call_id: &str,
        events: &tokio::sync::broadcast::Sender<AgentEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Option<serde_json::Value> {
        let response = tokio::time::timeout(
            Duration::from_secs(120),
            self.permission_response(
                call_id,
                "memory_confirm",
                confirmation.prompt.clone(),
                events,
                cancel.clone(),
                omegon_traits::PermissionRequestKind::Policy,
                omegon_traits::PermissionPersistence::None,
                None,
            ),
        )
        .await
        .ok()?;
        if cancel.is_cancelled() || response != omegon_traits::PermissionResponse::Allow {
            return None;
        }
        Some(
            serde_json::json!({"request":confirmation.request,"surface":if self.host.is_some() {"acp"} else {"native_event"}}),
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn permission_response(
        &self,
        call_id: &str,
        tool_name: &str,
        path: String,
        events: &tokio::sync::broadcast::Sender<AgentEvent>,
        cancel: tokio_util::sync::CancellationToken,
        kind: omegon_traits::PermissionRequestKind,
        persistence: omegon_traits::PermissionPersistence,
        grant_path: Option<String>,
    ) -> omegon_traits::PermissionResponse {
        if let Some(host) = self.host.as_ref() {
            return match host
                .proxy
                .request_permission(call_id.into(), tool_name.into(), path)
                .await
            {
                Ok(agent_client_protocol::schema::RequestPermissionOutcome::Selected(selected)) => {
                    match selected.option_id.0.as_ref() {
                        "allow_always"
                            if kind == omegon_traits::PermissionRequestKind::PathBoundary =>
                        {
                            omegon_traits::PermissionResponse::AlwaysAllow
                        }
                        "allow_always" | "allow_once" => omegon_traits::PermissionResponse::Allow,
                        _ => omegon_traits::PermissionResponse::Deny,
                    }
                }
                _ => omegon_traits::PermissionResponse::Deny,
            };
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = events.send(AgentEvent::PermissionRequest {
            tool_name: tool_name.into(),
            path,
            kind,
            persistence,
            grant_path,
            respond: Arc::new(std::sync::Mutex::new(Some(tx))),
        });
        wait_for_permission_response(rx, cancel).await
    }

    async fn present_operator_wait(
        &self,
        wait: crate::tools::OperatorWaitRequired,
        call_id: Option<&str>,
        events: &tokio::sync::broadcast::Sender<AgentEvent>,
        cancel: tokio_util::sync::CancellationToken,
        sink: omegon_traits::ToolProgressSink,
    ) -> omegon_traits::ToolResult {
        if self.host.is_some() {
            return unsupported_wait_result(
                "Manual action required, but interactive operator confirmation is only available in the TUI right now.",
                "operator_wait_requires_tui",
                &wait,
            );
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        let _ = events.send(AgentEvent::OperatorWaitRequest {
            call_id: call_id.map(str::to_owned),
            prompt: wait.prompt.clone(),
            timeout_secs: wait.timeout_secs,
            acknowledge: Arc::new(std::sync::Mutex::new(Some(ack_tx))),
            respond: Arc::new(std::sync::Mutex::new(Some(tx))),
        });
        let acknowledged = tokio::task::spawn_blocking(move || {
            ack_rx.recv_timeout(Duration::from_secs(2)).is_ok()
        })
        .await
        .unwrap_or(false);
        if !acknowledged {
            return unsupported_wait_result(
                "Manual action required, but no interactive operator surface acknowledged the wait request.",
                "operator_wait_not_acknowledged",
                &wait,
            );
        }

        let start = Instant::now();
        let mut initial = omegon_traits::PartialToolResult::content(
            format!(
                "Manual action required:\n{}\n\nWaiting for operator confirmation. Timeout: {} seconds.",
                wait.prompt, wait.timeout_secs
            ),
            0,
        );
        initial.progress.phase = Some("waiting_for_operator".into());
        initial.details = serde_json::json!({
            "status": "waiting", "prompt": wait.prompt, "timeoutSecs": wait.timeout_secs,
        });
        sink.send(initial);
        let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::task::spawn_blocking(move || {
            let _ = notify_tx.send(rx.recv());
        });
        let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let timeout = tokio::time::sleep(Duration::from_secs(wait.timeout_secs));
        tokio::pin!(timeout);
        let status = loop {
            tokio::select! {
                _ = cancel.cancelled() => break "cancelled",
                _ = &mut timeout => break "timed_out",
                response = notify_rx.recv() => match response {
                    Some(Ok(omegon_traits::OperatorWaitResponse::Completed)) => break "completed",
                    _ => break "cancelled",
                },
                _ = heartbeat.tick() => {
                    let mut partial = omegon_traits::PartialToolResult::heartbeat(start.elapsed().as_millis() as u64);
                    partial.progress.phase = Some("waiting_for_operator".into());
                    partial.details = serde_json::json!({
                        "status": "waiting", "elapsedSecs": start.elapsed().as_secs(),
                        "timeoutSecs": wait.timeout_secs,
                    });
                    sink.send(partial);
                }
            }
        };
        let elapsed_secs = start.elapsed().as_secs();
        let text = match status {
            "completed" => format!("Manual action completed after {elapsed_secs}s."),
            "timed_out" => format!(
                "Manual action timed out after {elapsed_secs}s without operator confirmation."
            ),
            _ => format!("Manual action cancelled after {elapsed_secs}s."),
        };
        omegon_traits::ToolResult {
            content: vec![ContentBlock::Text { text }],
            details: serde_json::json!({
                "status": status, "elapsedSecs": elapsed_secs, "timeoutSecs": wait.timeout_secs,
            }),
        }
    }
}

pub(crate) async fn wait_for_permission_response(
    rx: std::sync::mpsc::Receiver<omegon_traits::PermissionResponse>,
    cancel: tokio_util::sync::CancellationToken,
) -> omegon_traits::PermissionResponse {
    // The frontend contract uses std::mpsc. Keep the receiver in this future so
    // cancellation/timeout drops it, rather than leaving a blocking recv task alive.
    loop {
        if cancel.is_cancelled() {
            return omegon_traits::PermissionResponse::Deny;
        }
        match rx.try_recv() {
            Ok(response) => return response,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return omegon_traits::PermissionResponse::Deny;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        tokio::select! {
            _ = cancel.cancelled() => return omegon_traits::PermissionResponse::Deny,
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
    }
}

fn unsupported_wait_result(
    text: &str,
    reason: &str,
    wait: &crate::tools::OperatorWaitRequired,
) -> omegon_traits::ToolResult {
    omegon_traits::ToolResult {
        content: vec![ContentBlock::Text { text: text.into() }],
        details: serde_json::json!({
            "is_error": true, "status": "unsupported_surface", "reason": reason,
            "prompt": wait.prompt, "timeoutSecs": wait.timeout_secs,
        }),
    }
}

fn path_denied_result(error: &crate::tools::PathPermissionError) -> omegon_traits::ToolResult {
    omegon_traits::ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "BLOCKED: '{}' is outside the workspace. This operation was denied by the permission system. The operator can run /permissions add {} to allow access to this directory, then re-run the task.",
                error.requested_path, error.directory,
            ),
        }],
        details: serde_json::json!({
            "is_error": true, "blocked": true, "reason": "path_outside_workspace",
            "directory": error.directory,
        }),
    }
}

fn error_result(error: anyhow::Error) -> omegon_traits::ToolResult {
    omegon_traits::ToolResult {
        content: vec![ContentBlock::Text {
            text: error.to_string(),
        }],
        details: serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::{
        PermissionOptionId, RequestPermissionOutcome, SelectedPermissionOutcome,
    };

    fn acp_frontend() -> (
        LoopInvocationFrontend,
        tokio::sync::mpsc::Receiver<crate::host_context::HostProxyRequest>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let context = crate::host_context::HostContext {
            caps: Arc::new(crate::host_context::HostCapabilities::default()),
            proxy: crate::host_context::HostProxySender::new(tx),
            session_id: "permission-test".into(),
            cwd: std::path::PathBuf::from("."),
        };
        (LoopInvocationFrontend::acp(Arc::new(context)), rx)
    }

    #[tokio::test]
    async fn local_permission_uses_agent_event_channel_and_preserves_session_grant() {
        let frontend = LoopInvocationFrontend::default();
        let (events, mut event_rx) = tokio::sync::broadcast::channel(4);
        let response = frontend.permission_response(
            "call-local",
            "read",
            "/outside/file".into(),
            &events,
            tokio_util::sync::CancellationToken::new(),
            omegon_traits::PermissionRequestKind::PathBoundary,
            omegon_traits::PermissionPersistence::ProjectDirectory,
            Some("/outside".into()),
        );
        tokio::pin!(response);
        tokio::select! {
            event = event_rx.recv() => match event.unwrap() {
                AgentEvent::PermissionRequest { respond, .. } => {
                    respond.lock().unwrap().take().unwrap()
                        .send(omegon_traits::PermissionResponse::AllowSession).unwrap();
                }
                _ => panic!("expected permission request"),
            },
            _ = &mut response => panic!("permission completed before operator response"),
        }
        assert_eq!(
            response.await,
            omegon_traits::PermissionResponse::AllowSession
        );
    }

    fn memory_review() -> crate::features::memory::confirmation::MemoryConfirmationRequired {
        crate::features::memory::confirmation::MemoryConfirmationRequired {
            prompt: "Review the exact candidate".into(),
            request: crate::features::memory::confirmation::ConfirmationRequest {
                candidate: omegon_memory::FactPrecondition {
                    id: "candidate".into(),
                    expected_version: 7,
                },
                snapshot_hash: "a".repeat(64),
                session_id: "session".into(),
                request_id: "review".into(),
                supersedes: None,
            },
        }
    }

    #[tokio::test]
    async fn cancelled_permission_wait_releases_receiver_while_frontend_retains_sender() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        assert_eq!(
            wait_for_permission_response(receiver, cancel).await,
            omegon_traits::PermissionResponse::Deny
        );
        assert!(
            sender
                .send(omegon_traits::PermissionResponse::Allow)
                .is_err()
        );
    }

    #[tokio::test]
    async fn dropped_permission_wait_releases_receiver_without_an_operator_response() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut wait = Box::pin(wait_for_permission_response(
            receiver,
            tokio_util::sync::CancellationToken::new(),
        ));
        assert!(futures::poll!(&mut wait).is_pending());
        drop(wait);
        assert!(
            sender
                .send(omegon_traits::PermissionResponse::Allow)
                .is_err()
        );
    }

    #[tokio::test]
    async fn memory_confirmation_requires_current_single_request_operator_response() {
        for decision in [
            omegon_traits::PermissionResponse::Allow,
            omegon_traits::PermissionResponse::Deny,
            omegon_traits::PermissionResponse::AllowSession,
        ] {
            let frontend = LoopInvocationFrontend::default();
            let (events, mut receiver) = tokio::sync::broadcast::channel(4);
            let review = memory_review();
            let (approved, ()) = tokio::join!(
                frontend.approve_memory_candidate(
                    &review,
                    "call",
                    &events,
                    tokio_util::sync::CancellationToken::new()
                ),
                async {
                    let AgentEvent::PermissionRequest {
                        kind,
                        persistence,
                        respond,
                        ..
                    } = receiver.recv().await.unwrap()
                    else {
                        panic!("expected operator request");
                    };
                    assert_eq!(kind, omegon_traits::PermissionRequestKind::Policy);
                    assert_eq!(persistence, omegon_traits::PermissionPersistence::None);
                    respond
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .send(decision)
                        .unwrap();
                }
            );
            assert_eq!(
                approved.is_some(),
                decision == omegon_traits::PermissionResponse::Allow
            );
            if let Some(approved) = approved {
                assert_eq!(approved["surface"], "native_event");
                assert_eq!(approved["request"]["candidate"]["expected_version"], 7);
            }
        }
    }

    #[tokio::test]
    async fn memory_confirmation_acp_and_cancellation_preserve_authority_boundary() {
        let (frontend, mut receiver) = acp_frontend();
        let (events, _) = tokio::sync::broadcast::channel(4);
        let review = memory_review();
        let (approved, ()) = tokio::join!(
            frontend.approve_memory_candidate(
                &review,
                "call",
                &events,
                tokio_util::sync::CancellationToken::new()
            ),
            async {
                let crate::host_context::HostProxyRequest::RequestPermission { reply, .. } =
                    receiver.recv().await.unwrap()
                else {
                    panic!("expected host approval");
                };
                reply
                    .send(Ok(RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(PermissionOptionId::new("allow_once")),
                    )))
                    .unwrap();
            }
        );
        assert_eq!(approved.unwrap()["surface"], "acp");
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let local = LoopInvocationFrontend::default();
        assert!(
            local
                .approve_memory_candidate(&review, "cancelled", &events, cancel)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn acp_permission_delegates_to_host_and_maps_durable_grant() {
        let (frontend, mut host_rx) = acp_frontend();
        let (events, mut event_rx) = tokio::sync::broadcast::channel(4);
        let host = tokio::spawn(async move {
            let crate::host_context::HostProxyRequest::RequestPermission {
                tool_call_id,
                tool_name,
                path,
                reply,
            } = host_rx.recv().await.unwrap()
            else {
                panic!("expected host permission request")
            };
            assert_eq!(
                (tool_call_id.as_str(), tool_name.as_str(), path.as_str()),
                ("call-acp", "read", "/outside/file")
            );
            reply
                .send(Ok(RequestPermissionOutcome::Selected(
                    SelectedPermissionOutcome::new(PermissionOptionId::new("allow_always")),
                )))
                .unwrap();
        });
        let response = frontend
            .permission_response(
                "call-acp",
                "read",
                "/outside/file".into(),
                &events,
                tokio_util::sync::CancellationToken::new(),
                omegon_traits::PermissionRequestKind::PathBoundary,
                omegon_traits::PermissionPersistence::ProjectDirectory,
                Some("/outside".into()),
            )
            .await;
        host.await.unwrap();
        assert_eq!(response, omegon_traits::PermissionResponse::AlwaysAllow);
        assert!(matches!(
            event_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn acp_operator_wait_returns_exact_unsupported_surface_reason() {
        let (frontend, _host_rx) = acp_frontend();
        let (events, _) = tokio::sync::broadcast::channel(4);
        let result = frontend
            .present_operator_wait(
                crate::tools::OperatorWaitRequired {
                    prompt: "restart service".into(),
                    timeout_secs: 30,
                },
                None,
                &events,
                tokio_util::sync::CancellationToken::new(),
                omegon_traits::ToolProgressSink::noop(),
            )
            .await;
        assert_eq!(result.details["status"], "unsupported_surface");
        assert_eq!(result.details["reason"], "operator_wait_requires_tui");
    }

    #[tokio::test]
    async fn local_operator_wait_acknowledges_and_completes() {
        let frontend = LoopInvocationFrontend::default();
        let (events, mut event_rx) = tokio::sync::broadcast::channel(4);
        let wait = frontend.present_operator_wait(
            crate::tools::OperatorWaitRequired {
                prompt: "restart service".into(),
                timeout_secs: 30,
            },
            Some("wait-call-1"),
            &events,
            tokio_util::sync::CancellationToken::new(),
            omegon_traits::ToolProgressSink::noop(),
        );
        tokio::pin!(wait);
        tokio::select! {
            event = event_rx.recv() => match event.unwrap() {
                AgentEvent::OperatorWaitRequest { call_id, acknowledge, respond, .. } => {
                    assert_eq!(call_id.as_deref(), Some("wait-call-1"));
                    acknowledge.lock().unwrap().take().unwrap().send(()).unwrap();
                    respond.lock().unwrap().take().unwrap()
                        .send(omegon_traits::OperatorWaitResponse::Completed).unwrap();
                }
                _ => panic!("expected operator wait request"),
            },
            _ = &mut wait => panic!("wait completed before frontend response"),
        }
        let result = wait.await;
        assert_eq!(result.details["status"], "completed");
    }
}

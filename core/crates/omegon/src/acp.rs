//! ACP transport — thin layer that forwards prompts to the worker thread
//! and streams events back to the editor via ACP notifications.
//!
//! Architecture:
//! - ACP I/O runs on the main thread (LocalSet, !Send)
//! - Agent loop runs on a dedicated worker thread (own runtime)
//! - Communication via typed channels (WorkerRequest/WorkerResponse/WorkerEvent)

use std::cell::RefCell;
use std::rc::Rc;

use agent_client_protocol::JsonRpcMessage as _;
use agent_client_protocol::schema::*;
use agent_client_protocol::*;
use anyhow::Context;

use crate::acp_worker::{self, WorkerEvent, WorkerHandle, WorkerRequest};
use crate::host_context::{self, HostCapabilities, HostContext, HostProxySender};

#[path = "acp/extension_rpc.rs"]
mod extension_rpc;
#[path = "acp/labels.rs"]
mod labels;
#[path = "acp/model_options.rs"]
mod model_options;
#[path = "acp/resource_context.rs"]
mod resource_context;
#[path = "acp/surfaces.rs"]
mod surfaces;

use labels::compact_tool_call_label;
use model_options::{
    acp_model_provider_available, compact_model_label, unavailable_current_model_label,
};
use resource_context::prompt_blocks_to_text;
use surfaces::{
    ACP_CONVERSATION_SURFACE_METHOD, ACP_CONVERSATION_SURFACE_REDACTION, AcpConversationEvent,
    AcpConversationSurfaceAdapter, SurfaceRedaction,
};

type JsonRpcMessage = agent_client_protocol::jsonrpcmsg::Message;
type JsonRpcTx =
    futures::channel::mpsc::UnboundedSender<agent_client_protocol::Result<JsonRpcMessage>>;
type PendingResponseTx =
    futures::channel::oneshot::Sender<agent_client_protocol::Result<serde_json::Value>>;
type PendingResponses = Rc<RefCell<std::collections::BTreeMap<String, PendingResponseTx>>>;

fn acp_available_commands() -> Vec<AvailableCommand> {
    let mut commands = Vec::new();
    for definition in crate::command_registry::builtin_command_definitions()
        .into_iter()
        .filter(|definition| definition.availability.acp)
    {
        // Preserve ACP's already-advertised slash names while sourcing the
        // underlying command metadata from the shared registry. The local handler
        // accepts both these ACP names and the canonical TUI registry names.
        let name = match definition.name.as_str() {
            "think" => "thinking".to_string(),
            "auth" => "login".to_string(),
            _ => definition.name,
        };
        commands.push(AvailableCommand::new(name, definition.description));

        // ACP exposes posture as a client/workbench mode control, not a TUI
        // slash command. Keep it next to thinking, matching the historical
        // advertised command ordering.
        if commands
            .last()
            .is_some_and(|command| command.name == "thinking")
        {
            commands.push(AvailableCommand::new(
                "posture",
                "Show or set behavioral posture",
            ));
        }
    }
    commands
}

pub(crate) type SharedAcpClientConnection = Rc<RefCell<Option<AcpClientConnection>>>;

#[derive(Clone)]
pub(crate) struct AcpClientConnection {
    tx: JsonRpcTx,
    pending: PendingResponses,
}

impl AcpClientConnection {
    fn new(tx: JsonRpcTx) -> Self {
        Self {
            tx,
            pending: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
        }
    }

    pub(crate) fn send_notification<N>(&self, notification: N) -> agent_client_protocol::Result<()>
    where
        N: JsonRpcNotification,
    {
        let untyped = notification.to_untyped_message()?;
        let params: Option<agent_client_protocol::jsonrpcmsg::Params> =
            serde_json::from_value(untyped.params)
                .map_err(agent_client_protocol::Error::into_internal_error)?;
        self.tx
            .unbounded_send(Ok(JsonRpcMessage::Request(
                agent_client_protocol::jsonrpcmsg::Request::new_v2(untyped.method, params, None),
            )))
            .map_err(agent_client_protocol::Error::into_internal_error)
    }

    pub(crate) async fn send_request<Req>(
        &self,
        request: Req,
    ) -> agent_client_protocol::Result<Req::Response>
    where
        Req: JsonRpcRequest,
    {
        let method = request.method().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let untyped = request.to_untyped_message()?;
        let params: Option<agent_client_protocol::jsonrpcmsg::Params> =
            serde_json::from_value(untyped.params)
                .map_err(agent_client_protocol::Error::into_internal_error)?;
        let (tx, rx) = futures::channel::oneshot::channel();
        self.pending.borrow_mut().insert(id.clone(), tx);
        if let Err(error) = self.tx.unbounded_send(Ok(JsonRpcMessage::Request(
            agent_client_protocol::jsonrpcmsg::Request::new_v2(
                untyped.method,
                params,
                Some(agent_client_protocol::jsonrpcmsg::Id::String(id.clone())),
            ),
        ))) {
            self.pending.borrow_mut().remove(&id);
            return Err(agent_client_protocol::Error::into_internal_error(error));
        }
        let value = rx.await.map_err(|_| {
            agent_client_protocol::util::internal_error(format!(
                "ACP request `{method}` response channel closed"
            ))
        })??;
        Req::Response::from_value(&method, value)
    }

    fn handle_response(&self, response: agent_client_protocol::jsonrpcmsg::Response) {
        let Some(agent_client_protocol::jsonrpcmsg::Id::String(id)) = response.id else {
            return;
        };
        let Some(tx) = self.pending.borrow_mut().remove(&id) else {
            return;
        };
        let result = if let Some(error) = response.error {
            Err(agent_client_protocol::Error::new(error.code, error.message).data(error.data))
        } else {
            Ok(response.result.unwrap_or(serde_json::Value::Null))
        };
        let _ = tx.send(result);
    }

    fn fail_pending(&self, error: agent_client_protocol::Error) {
        for (_, tx) in std::mem::take(&mut *self.pending.borrow_mut()) {
            let _ = tx.send(Err(error.clone()));
        }
    }
}

pub(crate) async fn send_session_update(
    conn: &AcpClientConnection,
    session_id: SessionId,
    update: SessionUpdate,
) -> agent_client_protocol::Result<()> {
    conn.send_notification(SessionNotification::new(session_id, update))
}

fn send_acp_ext_notification(
    conn: &AcpClientConnection,
    method: &str,
    params: serde_json::Value,
) -> agent_client_protocol::Result<()> {
    let raw = serde_json::value::RawValue::from_string(params.to_string())
        .map_err(agent_client_protocol::Error::into_internal_error)?;
    conn.send_notification(AgentNotification::ExtNotification(ExtNotification::new(
        method,
        std::sync::Arc::from(raw),
    )))
}

#[derive(Debug, Clone)]
struct StreamIdlePayload {
    session_id: String,
    provider: String,
    model: String,
    phase: String,
    idle_secs: u64,
    ambiguous: bool,
    message: String,
}

#[derive(Debug, Clone)]
struct ProviderRetryPayload {
    session_id: String,
    provider: String,
    model: String,
    attempt: u32,
    delay_ms: u64,
    reason: String,
    message: String,
    recoverable: bool,
}

#[derive(Debug, Clone)]
struct ProviderFailurePayload {
    session_id: String,
    provider: String,
    model: String,
    reason: String,
    attempts: u32,
    message: String,
    retryable: bool,
    recommended_action: String,
}

fn stream_idle_payload(payload: StreamIdlePayload) -> serde_json::Value {
    serde_json::json!({
        "sessionId": payload.session_id,
        "provider": payload.provider,
        "model": payload.model,
        "phase": payload.phase,
        "idleSecs": payload.idle_secs,
        "ambiguous": payload.ambiguous,
        "message": payload.message,
    })
}

fn provider_retry_payload(payload: ProviderRetryPayload) -> serde_json::Value {
    serde_json::json!({
        "sessionId": payload.session_id,
        "provider": payload.provider,
        "model": payload.model,
        "attempt": payload.attempt,
        "delayMs": payload.delay_ms,
        "reason": payload.reason,
        "message": payload.message,
        "recoverable": payload.recoverable,
    })
}

fn provider_failure_payload(payload: ProviderFailurePayload) -> serde_json::Value {
    serde_json::json!({
        "sessionId": payload.session_id,
        "provider": payload.provider,
        "model": payload.model,
        "reason": payload.reason,
        "attempts": payload.attempts,
        "message": payload.message,
        "retryable": payload.retryable,
        "recommendedAction": payload.recommended_action,
    })
}

fn turn_cancelled_payload(
    session_id: impl Into<String>,
    reason: impl Into<String>,
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id.into(),
        "reason": reason.into(),
    })
}

pub(crate) fn connect_acp_agent(
    agent: Rc<OmegonAcpAgent>,
    outgoing: impl futures::AsyncWrite + Send + Unpin + 'static,
    incoming: impl futures::AsyncRead + Send + Unpin + 'static,
    _spawn: impl Fn(futures::future::LocalBoxFuture<'static, ()>) + 'static,
) -> impl std::future::Future<Output = agent_client_protocol::Result<()>> {
    use agent_client_protocol::ConnectTo;
    use futures::StreamExt;

    let (mut channel, io_task) = <ByteStreams<_, _> as ConnectTo<Agent>>::into_channel_and_future(
        ByteStreams::new(outgoing, incoming),
    );
    let client = AcpClientConnection::new(channel.tx.clone());
    agent.set_client(client.clone());

    async move {
        let io_task = tokio::task::spawn_local(io_task);
        while let Some(message) = channel.rx.next().await {
            match message? {
                JsonRpcMessage::Request(request) => {
                    if request.id.is_none() {
                        handle_acp_notification(agent.clone(), request).await?;
                    } else {
                        let tx = channel.tx.clone();
                        let agent = agent.clone();
                        tokio::task::spawn_local(async move {
                            if let Err(error) = handle_acp_request(agent, &tx, request).await {
                                tracing::warn!(
                                    ?error,
                                    "ACP request handler failed before response send"
                                );
                            }
                        });
                    }
                }
                JsonRpcMessage::Response(response) => {
                    client.handle_response(response);
                }
            }
        }
        let result = io_task
            .await
            .map_err(agent_client_protocol::Error::into_internal_error)?;
        if let Err(error) = &result {
            client.fail_pending(error.clone());
        }
        result
    }
}

fn request_params(params: Option<agent_client_protocol::jsonrpcmsg::Params>) -> serde_json::Value {
    serde_json::to_value(params).unwrap_or(serde_json::Value::Null)
}

fn send_json_response(
    tx: &JsonRpcTx,
    id: Option<agent_client_protocol::jsonrpcmsg::Id>,
    result: agent_client_protocol::Result<serde_json::Value>,
) -> agent_client_protocol::Result<()> {
    let response = match result {
        Ok(value) => agent_client_protocol::jsonrpcmsg::Response::success_v2(value, id),
        Err(error) => agent_client_protocol::jsonrpcmsg::Response::error_v2(
            agent_client_protocol::jsonrpcmsg::Error {
                code: i32::from(error.code),
                message: error.message,
                data: error.data,
            },
            id,
        ),
    };
    tx.unbounded_send(Ok(JsonRpcMessage::Response(response)))
        .map_err(agent_client_protocol::Error::into_internal_error)
}

async fn handle_acp_request(
    agent: Rc<OmegonAcpAgent>,
    tx: &JsonRpcTx,
    request: agent_client_protocol::jsonrpcmsg::Request,
) -> agent_client_protocol::Result<()> {
    let method = request.method.clone();
    let id = request.id.clone();
    let params = request_params(request.params);
    let result = handle_acp_request_result(agent, &method, &params).await;
    send_json_response(tx, id, result)
}

async fn handle_acp_request_result(
    agent: Rc<OmegonAcpAgent>,
    method: &str,
    params: &serde_json::Value,
) -> agent_client_protocol::Result<serde_json::Value> {
    match method {
        m if InitializeRequest::matches_method(m) => {
            let req = InitializeRequest::parse_message(method, params)?;
            InitializeResponse::into_json(agent.initialize(req).await?, method)
        }
        m if AuthenticateRequest::matches_method(m) => {
            let req = AuthenticateRequest::parse_message(method, params)?;
            AuthenticateResponse::into_json(agent.authenticate(req).await?, method)
        }
        m if NewSessionRequest::matches_method(m) => {
            let req = NewSessionRequest::parse_message(method, params)?;
            NewSessionResponse::into_json(agent.new_session(req).await?, method)
        }
        m if PromptRequest::matches_method(m) => {
            let req = PromptRequest::parse_message(method, params)?;
            PromptResponse::into_json(agent.prompt(req).await?, method)
        }
        m if LoadSessionRequest::matches_method(m) => {
            let req = LoadSessionRequest::parse_message(method, params)?;
            LoadSessionResponse::into_json(agent.load_session(req).await?, method)
        }
        m if ListSessionsRequest::matches_method(m) => {
            let req = ListSessionsRequest::parse_message(method, params)?;
            ListSessionsResponse::into_json(agent.list_sessions(req).await?, method)
        }
        m if CloseSessionRequest::matches_method(m) => {
            let req = CloseSessionRequest::parse_message(method, params)?;
            CloseSessionResponse::into_json(agent.close_session(req).await?, method)
        }
        m if SetSessionModeRequest::matches_method(m) => {
            let req = SetSessionModeRequest::parse_message(method, params)?;
            SetSessionModeResponse::into_json(agent.set_session_mode(req).await?, method)
        }
        m if SetSessionConfigOptionRequest::matches_method(m) => {
            let req = SetSessionConfigOptionRequest::parse_message(method, params)?;
            SetSessionConfigOptionResponse::into_json(
                agent.set_session_config_option(req).await?,
                method,
            )
        }
        m if m.starts_with('_') => route_ext_method(agent, m, params).await,
        _ => Err(agent_client_protocol::Error::method_not_found()),
    }
}

async fn route_ext_method(
    agent: Rc<OmegonAcpAgent>,
    method: &str,
    params: &serde_json::Value,
) -> agent_client_protocol::Result<serde_json::Value> {
    let ext_method = method
        .strip_prefix('_')
        .filter(|name| !name.is_empty())
        .ok_or_else(agent_client_protocol::Error::method_not_found)?;
    let raw = serde_json::value::RawValue::from_string(
        serde_json::to_string(params).map_err(agent_client_protocol::Error::into_internal_error)?,
    )
    .map_err(agent_client_protocol::Error::into_internal_error)?;
    let response = agent
        .ext_method(ExtRequest::new(ext_method.to_string(), raw.into()))
        .await?;
    serde_json::from_str(response.0.get())
        .map_err(agent_client_protocol::Error::into_internal_error)
}

async fn handle_acp_notification(
    agent: Rc<OmegonAcpAgent>,
    request: agent_client_protocol::jsonrpcmsg::Request,
) -> agent_client_protocol::Result<()> {
    let method = request.method.clone();
    let params = request_params(request.params);
    if CancelNotification::matches_method(&method) {
        let notification = CancelNotification::parse_message(&method, &params)?;
        agent.cancel(notification).await?;
    } else {
        tracing::debug!(method, "Ignoring unsupported ACP notification");
    }
    Ok(())
}

pub(crate) fn plan_entries_from_projection(
    projection: &omegon_traits::PlanSurfaceProjection,
) -> Vec<acp_worker::PlanEntryData> {
    projection
        .active
        .as_ref()
        .map(|lane| {
            lane.items
                .iter()
                .filter_map(|item| {
                    let content = item.label.trim();
                    if content.is_empty() {
                        return None;
                    }
                    let status = match item.status.as_str() {
                        "active" | "in_progress" | "executing" => {
                            acp_worker::PlanEntryState::InProgress
                        }
                        "done" | "completed" => acp_worker::PlanEntryState::Completed,
                        "skipped" | "failed" => acp_worker::PlanEntryState::Failed,
                        _ => acp_worker::PlanEntryState::Pending,
                    };
                    Some(acp_worker::PlanEntryData {
                        content: content.to_string(),
                        status,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn plan_entries_from_snapshot_json(
    snapshot_json: &serde_json::Value,
) -> Vec<acp_worker::PlanEntryData> {
    snapshot_json
        .get("items")
        .and_then(|items| items.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let content = item
                        .get("description")
                        .and_then(|value| value.as_str())
                        .map(str::trim)
                        .filter(|description| !description.is_empty())?;
                    let status = match item
                        .get("status")
                        .and_then(|value| value.as_str())
                        .unwrap_or("todo")
                    {
                        "active" | "in_progress" => acp_worker::PlanEntryState::InProgress,
                        "done" | "completed" => acp_worker::PlanEntryState::Completed,
                        "skipped" | "failed" => acp_worker::PlanEntryState::Failed,
                        _ => acp_worker::PlanEntryState::Pending,
                    };
                    Some(acp_worker::PlanEntryData {
                        content: content.to_string(),
                        status,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn merge_plan_entries(
    plan_state: &mut Vec<acp_worker::PlanEntryData>,
    entries: Vec<acp_worker::PlanEntryData>,
) {
    if entries.is_empty() {
        plan_state.clear();
    } else if entries.len() > 1 || plan_state.is_empty() {
        *plan_state = entries;
    } else {
        for update in entries {
            if let Some(existing) = plan_state
                .iter_mut()
                .find(|entry| entry.content == update.content)
            {
                existing.status = update.status;
            } else {
                plan_state.push(update);
            }
        }
    }
}

fn acp_plan_entry_status(status: acp_worker::PlanEntryState) -> PlanEntryStatus {
    match status {
        acp_worker::PlanEntryState::Pending => PlanEntryStatus::Pending,
        acp_worker::PlanEntryState::InProgress => PlanEntryStatus::InProgress,
        acp_worker::PlanEntryState::Completed | acp_worker::PlanEntryState::Failed => {
            PlanEntryStatus::Completed
        }
    }
}

fn acp_plan_entries(entries: &[acp_worker::PlanEntryData]) -> Vec<PlanEntry> {
    entries
        .iter()
        .map(|entry| {
            PlanEntry::new(
                &entry.content,
                PlanEntryPriority::Medium,
                acp_plan_entry_status(entry.status),
            )
        })
        .collect()
}

fn acp_status_message_text(msg: &str) -> Option<String> {
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains("Plan mode:") || trimmed.starts_with("Plan ") {
        let mode = trimmed
            .lines()
            .find_map(|line| line.trim().strip_prefix("Plan mode:"))
            .map(str::trim)
            .unwrap_or("");

        let text = if trimmed.starts_with("Plan set") && mode == "planning" {
            "Planning mode — edits blocked until approval.".to_string()
        } else if trimmed.starts_with("Plan approved") || mode == "approved" {
            "Plan approved — execution may proceed.".to_string()
        } else if trimmed.starts_with("Plan executing") || mode == "executing" {
            "Plan executing.".to_string()
        } else if trimmed.starts_with("Plan cleared") || mode == "off" {
            "Plan cleared.".to_string()
        } else if trimmed.starts_with("Plan item skipped") {
            "Plan item skipped.".to_string()
        } else if trimmed.starts_with("Plan progress") || mode == "complete" {
            "Plan progress updated.".to_string()
        } else {
            "Plan updated.".to_string()
        };

        return Some(text);
    }

    Some(trimmed.to_string())
}

fn acp_status_is_provider_telemetry(msg: &str) -> bool {
    msg.contains("— retrying") || msg.contains("transient upstream failures")
}

fn validate_active_session(
    active_session_id: &Option<SessionId>,
    requested_session_id: &SessionId,
) -> Result<()> {
    if active_session_id.as_ref() == Some(requested_session_id) {
        Ok(())
    } else {
        tracing::warn!(
            requested_session_id = %requested_session_id.0,
            active_session_id = active_session_id
                .as_ref()
                .map(|id| id.0.as_ref())
                .unwrap_or("<none>"),
            "rejected ACP request for non-active session"
        );
        Err(Error::invalid_params())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AcpTurnPhase {
    #[default]
    Idle,
    Running,
    Cancelling,
    Failed,
}

impl AcpTurnPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct AcpTurnState {
    phase: AcpTurnPhase,
    last_error: Option<String>,
}

pub struct OmegonAcpAgent {
    model: String,
    transport: &'static str,
    worker: RefCell<Option<WorkerHandle>>,
    conn: SharedAcpClientConnection,
    session_id: RefCell<Option<SessionId>>,
    session_cwd: RefCell<Option<std::path::PathBuf>>,
    turn_state: RefCell<AcpTurnState>,
    secrets: RefCell<Option<std::sync::Arc<omegon_secrets::SecretsManager>>>,
    host_caps: RefCell<HostCapabilities>,
    extension_metadata: Rc<RefCell<std::collections::BTreeMap<String, serde_json::Value>>>,
    extension_rpc_handles:
        Rc<RefCell<std::collections::BTreeMap<String, extension_rpc::ExtensionRpcHandle>>>,
    session_task_bindings: RefCell<Vec<crate::conversation::SessionTaskBinding>>,
    surface_updates_enabled: RefCell<bool>,
    dangerously_bypass_permissions: bool,
}

impl OmegonAcpAgent {
    pub fn new(model: &str) -> Self {
        Self::new_with_extension_metadata(model, Default::default())
    }

    pub fn new_with_safety(model: &str, dangerously_bypass_permissions: bool) -> Self {
        Self::new_with_extension_metadata_and_safety(
            model,
            Default::default(),
            dangerously_bypass_permissions,
        )
    }

    pub(crate) fn new_for_websocket(model: &str, dangerously_bypass_permissions: bool) -> Self {
        Self::new_with_transport(
            model,
            Default::default(),
            dangerously_bypass_permissions,
            "websocket",
        )
    }

    pub fn new_with_extension_metadata(
        model: &str,
        extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Self {
        Self::new_with_extension_metadata_and_safety(model, extension_metadata, false)
    }

    pub fn new_with_extension_metadata_and_safety(
        model: &str,
        extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
        dangerously_bypass_permissions: bool,
    ) -> Self {
        Self::new_with_transport(
            model,
            extension_metadata,
            dangerously_bypass_permissions,
            "stdio",
        )
    }

    fn new_with_transport(
        model: &str,
        extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
        dangerously_bypass_permissions: bool,
        transport: &'static str,
    ) -> Self {
        Self {
            model: model.to_string(),
            transport,
            worker: RefCell::new(None),
            conn: Rc::new(RefCell::new(None)),
            session_id: RefCell::new(None),
            session_cwd: RefCell::new(None),
            turn_state: RefCell::new(AcpTurnState::default()),
            secrets: RefCell::new(None),
            host_caps: RefCell::new(HostCapabilities::default()),
            extension_metadata: Rc::new(RefCell::new(extension_metadata)),
            extension_rpc_handles: Rc::new(RefCell::new(Default::default())),
            session_task_bindings: RefCell::new(Vec::new()),
            surface_updates_enabled: RefCell::new(false),
            dangerously_bypass_permissions,
        }
    }

    pub fn set_client(&self, c: AcpClientConnection) {
        *self.conn.borrow_mut() = Some(c);
    }

    #[cfg(test)]
    fn set_secrets_for_test(&self, secrets: std::sync::Arc<omegon_secrets::SecretsManager>) {
        *self.secrets.borrow_mut() = Some(secrets);
    }

    fn modes() -> SessionModeState {
        SessionModeState::new(
            "code",
            vec![
                SessionMode::new("code", "Code")
                    .description("Balanced coding — direct execution, delegates larger tasks"),
                SessionMode::new("architect", "Architect")
                    .description("Orchestrator — plans, delegates to local models, reviews"),
                SessionMode::new("ask", "Ask")
                    .description("Read-only exploration — lean, no file mutations"),
                SessionMode::new("agent", "Agent")
                    .description("Maximum force — deep reasoning, large context"),
            ],
        )
    }

    fn build_config_options(
        &self,
        current_model: &str,
        current_thinking: &str,
        current_context_class: &str,
        current_profile: &str,
        cwd: &std::path::Path,
    ) -> Vec<SessionConfigOption> {
        let mut model_options: Vec<SessionConfigSelectOption> = Vec::new();

        // Probe Ollama models synchronously
        let ollama_ok = std::net::TcpStream::connect_timeout(
            &"127.0.0.1:11434".parse().unwrap(),
            std::time::Duration::from_millis(100),
        )
        .is_ok();
        if ollama_ok && let Ok(stream) = std::net::TcpStream::connect("127.0.0.1:11434") {
            use std::io::{Read, Write};
            let mut s = stream;
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let _ = s.write_all(b"GET /api/tags HTTP/1.0\r\nHost: localhost\r\n\r\n");
            let mut buf = vec![0u8; 65536];
            let mut total = 0;
            while let Ok(n) = s.read(&mut buf[total..]) {
                if n == 0 {
                    break;
                }
                total += n;
            }
            let body = String::from_utf8_lossy(&buf[..total]);
            if let Some(start) = body.find('{')
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&body[start..])
                && let Some(models) = v["models"].as_array()
            {
                for m in models {
                    if let Some(name) = m["name"].as_str() {
                        let size = m["size"].as_u64().unwrap_or(0);
                        let gb = size as f64 / 1_000_000_000.0;
                        model_options.push(SessionConfigSelectOption::new(
                            format!("ollama:{name}"),
                            format!("{name} ({gb:.0}GB local)"),
                        ));
                    }
                }
            }
        }

        let registry = crate::model_registry::ModelRegistry::global();
        let mut registry_models: Vec<_> = registry.all_models().collect();
        registry_models.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.name.cmp(&b.name))
        });
        for model in registry_models {
            let id = format!("{}:{}", model.provider, model.id);
            if model_options.iter().any(|o| o.value.0.as_ref() == id) {
                continue;
            }
            if !acp_model_provider_available(&model.provider) {
                continue;
            }
            model_options.push(SessionConfigSelectOption::new(
                id,
                compact_model_label(&model.name, &model.provider),
            ));
        }

        if !model_options
            .iter()
            .any(|o| o.value.0.as_ref() == current_model)
        {
            model_options.insert(
                0,
                SessionConfigSelectOption::new(
                    current_model.to_string(),
                    unavailable_current_model_label(current_model),
                ),
            );
        }

        let thinking_options: Vec<SessionConfigSelectOption> = [
            ("off", "Off"),
            ("minimal", "Minimal"),
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
        ]
        .iter()
        .map(|(id, name)| SessionConfigSelectOption::new(*id, *name))
        .collect();

        let context_options: Vec<SessionConfigSelectOption> = crate::settings::ContextClass::all()
            .iter()
            .map(|class| {
                SessionConfigSelectOption::new(class.short().to_ascii_lowercase(), class.label())
            })
            .collect();

        let registry = crate::settings::ProfileRegistry::discover(cwd);
        let mut profile_options: Vec<SessionConfigSelectOption> = registry
            .entries
            .iter()
            .map(|entry| {
                let scope = entry.scope.as_str();
                SessionConfigSelectOption::new(
                    format!("{scope}:{}", entry.id),
                    format!(
                        "{} — {scope}",
                        entry.profile.compact_label().unwrap_or(&entry.id)
                    ),
                )
            })
            .collect();
        if !profile_options.iter().any(|option| {
            option
                .value
                .0
                .rsplit_once(':')
                .is_some_and(|(_, id)| id == current_profile)
                || option.value.0.as_ref() == current_profile
        }) {
            profile_options.insert(
                0,
                SessionConfigSelectOption::new(
                    current_profile.to_string(),
                    format!("{current_profile} — current"),
                ),
            );
        }

        vec![
            SessionConfigOption::new(
                "model",
                "Model",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_model.to_string(),
                    model_options,
                )),
            )
            .description("Language model used for subsequent turns")
            .category(SessionConfigOptionCategory::Model),
            SessionConfigOption::new(
                "thinking",
                "Thinking Level",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_thinking.to_string(),
                    thinking_options,
                )),
            )
            .description("Reasoning effort for subsequent turns")
            .category(SessionConfigOptionCategory::ThoughtLevel),
            SessionConfigOption::new(
                "profile",
                "Profile",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_profile.to_string(),
                    profile_options,
                )),
            )
            .description("Apply a named Omegon profile to this project session")
            .category(SessionConfigOptionCategory::Other("_omegon_profile".into())),
            SessionConfigOption::new(
                "context_class",
                "Context Window",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_context_class.to_string(),
                    context_options,
                )),
            )
            .description("Requested context-window class for subsequent turns")
            .category(SessionConfigOptionCategory::Other("_omegon_context".into())),
        ]
    }

    fn ensure_worker(&self, cwd: &std::path::Path) {
        if self.worker.borrow().is_none() {
            // Build HostContext if the client advertised any delegatable capabilities.
            let host_ctx = if self.host_caps.borrow().has_any_delegation() {
                let (proxy_tx, proxy_rx) = tokio::sync::mpsc::channel(64);
                let caps = std::sync::Arc::new(self.host_caps.borrow().clone());
                let session_id_str = self
                    .session_id
                    .borrow()
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let ctx = HostContext {
                    caps,
                    proxy: HostProxySender::new(proxy_tx),
                    session_id: session_id_str.clone(),
                };
                // Spawn the ACP-thread pump that services proxy requests.
                let sid = self
                    .session_id
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| SessionId::new("omegon-pending"));
                host_context::spawn_proxy_pump(proxy_rx, self.conn.clone(), sid);
                Some(ctx)
            } else {
                None
            };

            let mut handle = acp_worker::spawn_worker(
                self.model.clone(),
                cwd.to_path_buf(),
                host_ctx,
                self.dangerously_bypass_permissions,
            );
            // Drain the secrets channel asynchronously — the worker sends it
            // after AgentSetup completes. Store in self.secrets for redaction.
            let secrets_cell = self.secrets.clone();
            let rx = std::mem::replace(
                &mut handle.secrets_rx,
                // Replace with a dummy channel that will never fire
                tokio::sync::oneshot::channel().1,
            );
            tokio::task::spawn_local(async move {
                if let Ok(mgr) = rx.await {
                    *secrets_cell.borrow_mut() = Some(mgr);
                }
            });

            // Persistent lifecycle subscriber. Worker setup can emit extension
            // metadata/handles before any prompt is sent; prompt-time subscribers
            // are too late and broadcast receivers do not replay old events.
            // Use the original receiver created before the worker thread starts;
            // `resubscribe()` would start at the current tail and can still race
            // setup broadcasts emitted immediately after AgentSetup completes.
            let replacement_rx = handle.event_rx.resubscribe();
            let mut lifecycle_rx = std::mem::replace(&mut handle.event_rx, replacement_rx);
            let extension_metadata = self.extension_metadata.clone();
            let extension_rpc_handles = self.extension_rpc_handles.clone();
            let conn = self.conn.clone();
            let session_id = self.session_id.borrow().clone();
            tokio::task::spawn_local(async move {
                loop {
                    match lifecycle_rx.recv().await {
                        Ok(WorkerEvent::ExtensionMetadata(metadata)) => {
                            *extension_metadata.borrow_mut() = metadata.clone();
                            if let (Some(c), Some(sid)) =
                                (conn.borrow().as_ref(), session_id.clone())
                            {
                                let meta = extension_metadata_meta(&metadata);
                                let _ = send_session_update(
                                    c,
                                    sid,
                                    SessionUpdate::SessionInfoUpdate(
                                        SessionInfoUpdate::new().meta(meta),
                                    ),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::ExtensionHandles(handles)) => {
                            *extension_rpc_handles.borrow_mut() =
                                extension_rpc::erase_extension_rpc_handles(handles);
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            });
            *self.worker.borrow_mut() = Some(handle);
        }
    }

    /// Send a request to the worker. Panics if worker not initialized.
    async fn send_to_worker(&self, req: WorkerRequest) {
        let tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
        if let Some(tx) = tx {
            let _ = tx.send(req).await;
        }
    }

    /// Same as [`send_to_worker`], spelled out for clarity at sites that
    /// also await an ack channel separately.
    async fn send_to_worker_ack(&self, req: WorkerRequest) {
        self.send_to_worker(req).await;
    }

    /// Redact secret values from text before sending over ACP.
    fn redact(&self, text: &str) -> String {
        if let Some(ref mgr) = *self.secrets.borrow() {
            mgr.redact(text)
        } else {
            text.to_string()
        }
    }

    /// Read the worker's current settings — model/thinking/posture as actually
    /// applied. Falls back to the bootstrap defaults if the worker isn't up yet
    /// or the lock is poisoned.
    fn current_settings(&self) -> (String, String, String, String, String) {
        let settings_arc = self.worker.borrow().as_ref().map(|w| w.settings.clone());
        if let Some(s) = settings_arc
            && let Ok(g) = s.lock()
        {
            return (
                g.model.clone(),
                g.thinking.as_str().to_string(),
                g.posture.effective.as_str().to_string(),
                g.effective_requested_class().short().to_ascii_lowercase(),
                g.profile_name
                    .clone()
                    .unwrap_or_else(|| "built-in-default".into()),
            );
        }
        (
            self.model.clone(),
            "minimal".into(),
            "fabricator".into(),
            "standard".into(),
            "built-in-default".into(),
        )
    }

    fn runtime_status_json(&self) -> serde_json::Value {
        let (model, thinking, posture, context_class, profile) = self.current_settings();
        let cwd = std::env::current_dir().ok();
        let session_cwd = self.session_cwd.borrow().clone().or_else(|| cwd.clone());
        let session_id = self.session_id.borrow().as_ref().map(|id| id.to_string());
        let turn_state = self.turn_state.borrow().clone();
        let binary = std::env::current_exe().ok();
        serde_json::json!({
            "runtime": {
                "name": "omegon",
                "version": env!("CARGO_PKG_VERSION"),
                "commit": env!("OMEGON_GIT_SHA"),
                "build_date": env!("OMEGON_BUILD_DATE"),
                "binary": binary.as_ref().map(|p| p.display().to_string()),
                "cwd": cwd.as_ref().map(|p| p.display().to_string())
            },
            "acp": {
                "protocol_version": 1,
                "transport": self.transport,
                "session_id": session_id,
                "session_cwd": session_cwd.as_ref().map(|p| p.display().to_string()),
                "connected": self.conn.borrow().is_some(),
                "turn": {
                    "phase": turn_state.phase.as_str(),
                    "last_error": turn_state.last_error
                }
            },
            "agent": {
                "id": "default",
                "profile": profile,
                "model": model,
                "thinking": thinking,
                "posture": posture,
                "context_class": context_class
            },
            "memory": {
                "scope": "project",
                "root": session_cwd.as_ref().map(|p| p.join(".omegon").display().to_string())
            }
        })
    }

    fn provider_status_json(&self) -> serde_json::Value {
        let (model, _thinking, _posture, _context_class, _profile) = self.current_settings();
        let active_provider_id = crate::providers::infer_provider_id(&model);
        let providers: Vec<serde_json::Value> = crate::auth::PROVIDERS
            .iter()
            .map(|provider| {
                let status = crate::auth::provider_session_status(provider);
                let status_text = match status {
                    crate::auth::ProviderSessionStatus::Configured => "authenticated",
                    crate::auth::ProviderSessionStatus::Expired => "expired",
                    crate::auth::ProviderSessionStatus::Missing => "missing",
                };
                serde_json::json!({
                    "id": provider.id,
                    "name": provider.display_name,
                    "status": status_text,
                    "expires_at": serde_json::Value::Null,
                    "models": []
                })
            })
            .collect();
        let active_status = crate::auth::provider_by_id(&active_provider_id)
            .map(crate::auth::provider_session_status);
        let ready = matches!(
            active_status,
            Some(crate::auth::ProviderSessionStatus::Configured)
        ) || active_provider_id == "ollama";
        serde_json::json!({
            "providers": providers,
            "active": {
                "provider": active_provider_id,
                "model": model,
                "ready": ready
            }
        })
    }
}

fn extension_metadata_meta(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut meta = serde_json::Map::from_iter([(
        "omegon/extensions".to_string(),
        serde_json::json!(metadata),
    )]);

    if let Some(flynt) = metadata.get("flynt") {
        meta.insert("flynt".to_string(), flynt.clone());
    }

    meta
}

fn shadow_surface_update<F>(
    conn: Option<&AcpClientConnection>,
    surface_updates_enabled: bool,
    adapter: &mut AcpConversationSurfaceAdapter,
    event: AcpConversationEvent,
    redact: F,
) where
    F: Fn(&str) -> String,
{
    let updates = adapter.ingest(event, SurfaceRedaction::ExternalClient, redact);
    trace_shadow_surface_updates(&updates);
    maybe_send_shadow_surface_updates(conn, surface_updates_enabled, &updates);
}

fn acp_surface_updates_enabled() -> bool {
    acp_surface_updates_enabled_value(std::env::var("OMEGON_ACP_SURFACE_UPDATES").ok().as_deref())
}

fn acp_surface_updates_enabled_value(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "TRUE" | "yes" | "on"))
}

fn acp_client_is_flynt(client: Option<&Implementation>) -> bool {
    let Some(client) = client else {
        return false;
    };
    let name = client.name.to_ascii_lowercase();
    let title = client
        .title
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.contains("flynt") || title.contains("flynt")
}

fn acp_surface_updates_enabled_for_client(client: Option<&Implementation>) -> bool {
    acp_surface_updates_enabled() || acp_client_is_flynt(client)
}

fn acp_surface_metadata(enabled: bool) -> serde_json::Value {
    serde_json::json!({
        "conversation": {
            "version": surfaces::ACP_SURFACE_SCHEMA_VERSION,
            "enabled": enabled,
            "extensionMethod": ACP_CONVERSATION_SURFACE_METHOD,
            "redaction": ACP_CONVERSATION_SURFACE_REDACTION
        }
    })
}

fn maybe_send_shadow_surface_updates(
    conn: Option<&AcpClientConnection>,
    surface_updates_enabled: bool,
    updates: &[surfaces::AcpSurfaceUpdate],
) {
    if updates.is_empty() || !surface_updates_enabled {
        return;
    }
    let Some(conn) = conn else {
        return;
    };
    for update in updates {
        let Ok(payload) = serde_json::to_value(update) else {
            continue;
        };
        let _ = send_acp_ext_notification(conn, ACP_CONVERSATION_SURFACE_METHOD, payload);
    }
}

fn trace_shadow_surface_updates(updates: &[surfaces::AcpSurfaceUpdate]) {
    for update in updates {
        tracing::trace!(
            segment_id = %update.segment.identity.segment_id,
            turn_id = update.segment.identity.turn_id.as_deref(),
            sequence = update.segment.identity.sequence,
            revision = update.segment.identity.revision,
            role = %update.segment.role,
            complete = update.segment.complete,
            completed_segment_id = update.completed_segment_id.as_deref(),
            "ACP shadow conversation surface update"
        );
    }
}

fn acp_surface_tool_args(
    name: &str,
    args: Option<&serde_json::Value>,
) -> (Option<String>, Option<String>) {
    let summary = args.map(|value| compact_tool_call_label(name, Some(value)));
    let detail = args.map(serde_json::Value::to_string);
    (summary, detail)
}

fn acp_surface_tool_result(details: &serde_json::Value) -> Option<String> {
    (!details.is_null()).then(|| details.to_string())
}

impl OmegonAcpAgent {
    async fn initialize(&self, args: InitializeRequest) -> Result<InitializeResponse> {
        *self.host_caps.borrow_mut() = HostCapabilities::from_client(&args.client_capabilities);

        let caps = self.host_caps.borrow();
        tracing::info!(
            fs_read = caps.fs_read,
            fs_write = caps.fs_write,
            terminal = caps.terminal,
            "ACP client capabilities captured"
        );
        drop(caps);

        let surface_updates_enabled =
            acp_surface_updates_enabled_for_client(args.client_info.as_ref());
        *self.surface_updates_enabled.borrow_mut() = surface_updates_enabled;

        let mut response = InitializeResponse::new(args.protocol_version);
        response.agent_info =
            Some(Implementation::new("omegon", env!("CARGO_PKG_VERSION")).title("Omegon Agent"));
        response.agent_capabilities = AgentCapabilities::default()
            .prompt_capabilities(PromptCapabilities::new().embedded_context(true))
            .session_capabilities(
                SessionCapabilities::new()
                    .list(SessionListCapabilities::new())
                    .close(SessionCloseCapabilities::new()),
            );
        response.auth_methods = vec![AuthMethod::Agent(
            AuthMethodAgent::new("omegon-auth", "Omegon Authentication")
                .description("Run `omegon auth login` in a terminal or set API keys."),
        )];
        let mut meta = extension_metadata_meta(&self.extension_metadata.borrow());
        meta.insert(
            "omegon/surfaces".to_string(),
            acp_surface_metadata(surface_updates_enabled),
        );
        response.meta = Some(meta);
        Ok(response)
    }

    async fn authenticate(&self, _args: AuthenticateRequest) -> Result<AuthenticateResponse> {
        Ok(AuthenticateResponse::default())
    }

    async fn new_session(&self, args: NewSessionRequest) -> Result<NewSessionResponse> {
        let cwd = args.cwd.clone();
        *self.session_cwd.borrow_mut() = Some(cwd.clone());

        // Create session ID *before* ensure_worker so the proxy pump
        // receives the correct session ID for host RPC calls.
        let sid = SessionId::new(format!(
            "omegon-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        *self.session_id.borrow_mut() = Some(sid.clone());

        self.ensure_worker(&cwd);

        // Forward client-provided MCP servers to the worker.
        if !args.mcp_servers.is_empty() {
            let servers: Vec<(String, crate::plugins::mcp::McpServerConfig)> = args
                .mcp_servers
                .into_iter()
                .filter_map(convert_acp_mcp_server)
                .collect();
            if !servers.is_empty() {
                tracing::info!(
                    count = servers.len(),
                    "Forwarding client MCP servers to worker"
                );
                if let Some(w) = self.worker.borrow().as_ref() {
                    let tx = w.request_tx.clone();
                    tokio::task::spawn_local(async move {
                        let _ = tx.send(WorkerRequest::ConnectMcpServers { servers }).await;
                    });
                }
            }
        }

        let mut response = NewSessionResponse::new(sid.clone());
        response.modes = Some(Self::modes());

        // Read the *worker's* current settings, not self.model — the worker may
        // have already received SetModel/SetThinking/SetPosture before this
        // session started, and we need to advertise what's actually running.
        let (current_model, current_thinking, _current_posture, current_context, current_profile) =
            self.current_settings();
        response.config_options = Some(self.build_config_options(
            &current_model,
            &current_thinking,
            &current_context,
            &current_profile,
            &cwd,
        ));

        // Send available commands after response (via spawned task)
        let conn = self.conn.clone();
        let cmd_sid = sid.clone();
        tokio::task::spawn_local(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if let Some(c) = conn.borrow().as_ref() {
                let _ = send_session_update(
                    c,
                    cmd_sid,
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(
                        acp_available_commands(),
                    )),
                )
                .await;
            }
        });

        Ok(response)
    }

    async fn prompt(&self, args: PromptRequest) -> Result<PromptResponse> {
        let sid = args.session_id.clone();
        validate_active_session(&self.session_id.borrow(), &sid)?;

        let cwd = self
            .session_cwd
            .borrow()
            .clone()
            .ok_or_else(Error::invalid_params)?;
        self.ensure_worker(&cwd);

        // Extract user text and referenced context. ACP requires agents to
        // support ResourceLink prompt blocks, and Resource blocks are allowed
        // when embedded_context is advertised. Do not silently drop non-text
        // blocks; Zed sends @file mentions through this surface.
        let user_text = prompt_blocks_to_text(&args.prompt, &cwd);

        // Handle slash commands locally (no worker round-trip)
        if user_text.starts_with('/') {
            let response_text = self.handle_slash_command(&user_text);
            let conn = self.conn.clone();
            let notify_sid = sid.clone();
            tokio::task::spawn_local(async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = send_session_update(
                        c,
                        notify_sid,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(response_text),
                        ))),
                    )
                    .await;
                }
            });
            return Ok(PromptResponse::new(StopReason::EndTurn));
        }

        // Send prompt to worker and stream events back
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        *self.turn_state.borrow_mut() = AcpTurnState {
            phase: AcpTurnPhase::Running,
            last_error: None,
        };

        self.send_to_worker(WorkerRequest::Prompt {
            text: user_text,
            response_tx,
        })
        .await;

        // Subscribe to worker events and forward as ACP notifications.
        // The forwarder signals completion via done_tx so we don't return
        // PromptResponse (which tells Zed "turn is over") before all
        // streamed chunks have been delivered.
        let event_rx = self
            .worker
            .borrow()
            .as_ref()
            .map(|w| w.event_rx.resubscribe());

        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

        if let Some(mut event_rx) = event_rx {
            let conn = self.conn.clone();
            let stream_sid = sid.clone();
            let secrets_ref = self.secrets.clone();
            let surface_updates_enabled = *self.surface_updates_enabled.borrow();
            tokio::task::spawn_local(async move {
                // Closure to redact text through the secrets manager if available.
                let redact = |text: &str| -> String {
                    if let Some(ref mgr) = *secrets_ref.borrow() {
                        mgr.redact(text)
                    } else {
                        text.to_string()
                    }
                };
                let mut plan_state: Vec<acp_worker::PlanEntryData> = Vec::new();
                let mut surface_adapter =
                    AcpConversationSurfaceAdapter::with_turn_id(stream_sid.to_string());
                loop {
                    match event_rx.recv().await {
                        Ok(WorkerEvent::TextChunk(text)) => {
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::TextChunk(text.clone()),
                                |value| redact(value),
                            );
                            let text = redact(&text);
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new(text)),
                                    )),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::ThinkingChunk(text)) => {
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::ThinkingChunk(text.clone()),
                                |value| redact(value),
                            );
                            let text = redact(&text);
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new(text)),
                                    )),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::ToolStart { id, name, args }) => {
                            let (surface_args_summary, surface_detail_args) =
                                acp_surface_tool_args(&name, args.as_ref());
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::ToolStart {
                                    id: id.clone(),
                                    name: name.clone(),
                                    args_summary: surface_args_summary,
                                    detail_args: surface_detail_args,
                                },
                                |value| redact(value),
                            );
                            let args = args.map(|a| {
                                let s = redact(&a.to_string());
                                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
                            });
                            let display_name = compact_tool_call_label(&name, args.as_ref());
                            if let Some(c) = conn.borrow().as_ref() {
                                let mut tc = ToolCall::new(ToolCallId::new(id), display_name);
                                tc.status = ToolCallStatus::InProgress;
                                tc.raw_input = args;
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::ToolCall(tc),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::StatusUpdate(msg)) => {
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::StatusUpdate(msg.clone()),
                                |value| redact(value),
                            );
                            let msg = redact(&msg);
                            let Some(msg) = acp_status_message_text(&msg) else {
                                continue;
                            };
                            if acp_status_is_provider_telemetry(&msg) {
                                continue;
                            }
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new(format!("{msg}\n\n"))),
                                    )),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::ToolEnd {
                            id,
                            success,
                            details,
                        }) => {
                            let surface_result = acp_surface_tool_result(&details);
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::ToolEnd {
                                    id: id.clone(),
                                    success,
                                    result_summary: surface_result.clone(),
                                    detail_result: surface_result,
                                },
                                |value| redact(value),
                            );
                            if let Some(c) = conn.borrow().as_ref() {
                                let status = if success {
                                    ToolCallStatus::Completed
                                } else {
                                    ToolCallStatus::Failed
                                };
                                let fields = ToolCallUpdateFields::new()
                                    .status(status)
                                    .raw_output((!details.is_null()).then_some(details));
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                                        ToolCallId::new(id),
                                        fields,
                                    )),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::ToolOutput { id, text }) => {
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::ToolOutput {
                                    id: id.clone(),
                                    text: text.clone(),
                                },
                                |value| redact(value),
                            );
                            let text = redact(&text);
                            if let Some(c) = conn.borrow().as_ref() {
                                let content = ToolCallContent::Content(Content::new(
                                    ContentBlock::Text(TextContent::new(text)),
                                ));
                                let fields = ToolCallUpdateFields::new().content(vec![content]);
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                                        ToolCallId::new(id),
                                        fields,
                                    )),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::PlanUpdate { entries }) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                // Merge into running plan state.
                                // DecompositionStarted sends the full initial set;
                                // subsequent updates send single-entry patches.
                                // We maintain the full plan and re-emit it.
                                merge_plan_entries(&mut plan_state, entries);
                                let plan_entries = acp_plan_entries(&plan_state);
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::Plan(Plan::new(plan_entries)),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::StreamIdle {
                            provider,
                            model,
                            phase,
                            idle_secs,
                            ambiguous,
                            message,
                        }) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let payload = stream_idle_payload(StreamIdlePayload {
                                    session_id: stream_sid.to_string(),
                                    provider,
                                    model,
                                    phase,
                                    idle_secs,
                                    ambiguous,
                                    message: redact(&message),
                                });
                                let _ = send_acp_ext_notification(c, "_stream/idle", payload);
                            }
                        }
                        Ok(WorkerEvent::ProviderRetry {
                            provider,
                            model,
                            attempt,
                            delay_ms,
                            reason,
                            message,
                            recoverable,
                        }) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let payload = provider_retry_payload(ProviderRetryPayload {
                                    session_id: stream_sid.to_string(),
                                    provider,
                                    model,
                                    attempt,
                                    delay_ms,
                                    reason,
                                    message: redact(&message),
                                    recoverable,
                                });
                                let _ = send_acp_ext_notification(c, "_provider/retry", payload);
                            }
                        }
                        Ok(WorkerEvent::ProviderFailure {
                            provider,
                            model,
                            reason,
                            attempts,
                            message,
                            retryable,
                            recommended_action,
                        }) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let payload = provider_failure_payload(ProviderFailurePayload {
                                    session_id: stream_sid.to_string(),
                                    provider,
                                    model,
                                    reason,
                                    attempts,
                                    message: redact(&message),
                                    retryable,
                                    recommended_action,
                                });
                                let _ = send_acp_ext_notification(c, "_provider/failure", payload);
                            }
                        }
                        Ok(WorkerEvent::TurnCancelled { reason }) => {
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::TurnCancelled {
                                    reason: reason.clone(),
                                },
                                |value| redact(value),
                            );
                            if let Some(c) = conn.borrow().as_ref() {
                                let payload =
                                    turn_cancelled_payload(stream_sid.to_string(), reason);
                                let _ = send_acp_ext_notification(c, "_turn/cancelled", payload);
                            }
                        }
                        Ok(WorkerEvent::SessionTitle(title)) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::SessionInfoUpdate(
                                        SessionInfoUpdate::new().title(title),
                                    ),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::ExtensionMetadata(_))
                        | Ok(WorkerEvent::ExtensionHandles(_)) => {
                            // Setup/lifecycle extension state is handled by the persistent
                            // worker-event forwarder started in ensure_worker(). Prompt
                            // streaming subscribers may start after setup events and must not
                            // be the sole owner of this state.
                        }
                        Ok(WorkerEvent::TurnComplete) => {
                            shadow_surface_update(
                                conn.borrow().as_ref(),
                                surface_updates_enabled,
                                &mut surface_adapter,
                                AcpConversationEvent::TurnComplete,
                                |value| redact(value),
                            );
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
                let _ = done_tx.send(());
            });
        } else {
            let _ = done_tx.send(());
        }

        // Wait for the worker to finish AND the event forwarder to flush
        // all notifications to Zed before signaling end-of-turn.
        let worker_resp = match response_rx.await {
            Ok(response) => response,
            Err(_) => {
                *self.turn_state.borrow_mut() = AcpTurnState {
                    phase: AcpTurnPhase::Failed,
                    last_error: Some("ACP worker dropped the prompt response".into()),
                };
                return Err(Error::internal_error());
            }
        };
        let _ = done_rx.await;

        let next_turn_state = if let Some(error) = &worker_resp.error {
            AcpTurnState {
                phase: AcpTurnPhase::Failed,
                last_error: Some(self.redact(error)),
            }
        } else {
            AcpTurnState::default()
        };
        *self.turn_state.borrow_mut() = next_turn_state;

        // Send error after all chunks have been delivered
        if let Some(error) = &worker_resp.error {
            let conn = self.conn.clone();
            let err_sid = sid.clone();
            let err_text = self.redact(&format!("[Error: {error}]"));
            tokio::task::spawn_local(async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = send_session_update(
                        c,
                        err_sid,
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new(err_text),
                        ))),
                    )
                    .await;
                }
            });
        }

        let stop_reason = if worker_resp.cancelled {
            StopReason::Cancelled
        } else {
            StopReason::EndTurn
        };
        Ok(PromptResponse::new(stop_reason))
    }

    async fn cancel(&self, args: CancelNotification) -> Result<()> {
        if let Some(active) = self.session_id.borrow().as_ref()
            && active != &args.session_id
        {
            return Err(Error::invalid_params());
        }
        if self.turn_state.borrow().phase == AcpTurnPhase::Running {
            self.turn_state.borrow_mut().phase = AcpTurnPhase::Cancelling;
        }
        self.send_to_worker(WorkerRequest::Cancel).await;
        Ok(())
    }

    async fn set_session_mode(
        &self,
        args: SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse> {
        let posture = match args.mode_id.0.as_ref() {
            "code" => "fabricator",
            "architect" => "architect",
            "ask" => "explorator",
            "agent" => "devastator",
            _ => return Err(Error::invalid_params()),
        };
        self.send_to_worker_ack(WorkerRequest::SetPosture {
            value: posture.to_string(),
            ack: None,
        })
        .await;

        // Notify the client that the mode changed.
        let mode_id = args.mode_id.clone();
        if let Some(sid) = self.session_id.borrow().clone() {
            let conn = self.conn.clone();
            tokio::task::spawn_local(async move {
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = send_session_update(
                        c,
                        sid,
                        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode_id)),
                    )
                    .await;
                }
            });
        }

        Ok(SetSessionModeResponse::new())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse> {
        let config_id = args.config_id.0.to_string();
        let value = match &args.value {
            SessionConfigOptionValue::ValueId { value } => value.0.to_string(),
            SessionConfigOptionValue::Boolean { value } => value.to_string(),
            _ => return Err(Error::invalid_params()),
        };

        // Use acknowledgements so the complete config update is built only
        // after the worker has applied the requested mutation.
        let result = match config_id.as_str() {
            "model" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.send_to_worker_ack(WorkerRequest::SetModel {
                    value: value.clone(),
                    ack: Some(tx),
                })
                .await;
                rx.await.map_err(|_| Error::internal_error())?;
                Ok(())
            }
            "thinking" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.send_to_worker_ack(WorkerRequest::SetThinking {
                    value: value.clone(),
                    ack: Some(tx),
                })
                .await;
                rx.await.map_err(|_| Error::internal_error())?;
                Ok(())
            }
            "context_class" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.send_to_worker_ack(WorkerRequest::SetContextClass {
                    value: value.clone(),
                    ack: tx,
                })
                .await;
                rx.await.map_err(|_| Error::internal_error())?
            }
            "profile" => {
                let (scope, id) = value
                    .split_once(':')
                    .map_or((None, value.as_str()), |(scope, id)| (Some(scope), id));
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.send_to_worker_ack(WorkerRequest::ApplyProfile {
                    id: id.to_string(),
                    scope: scope.map(str::to_string),
                    ack: tx,
                })
                .await;
                rx.await.map_err(|_| Error::internal_error())?
            }
            _ => return Err(Error::invalid_params()),
        };
        result.map_err(|message| Error::new(i32::from(ErrorCode::InvalidParams), message))?;

        // Read back from the worker's settings — send_to_worker awaits the
        // mutation, so this captures the actually-applied state (which may
        // differ from `value` if the worker rejected/normalised the input).
        let (current_model, current_thinking, _current_posture, current_context, current_profile) =
            self.current_settings();
        let options = self.build_config_options(
            &current_model,
            &current_thinking,
            &current_context,
            &current_profile,
            self.session_cwd
                .borrow()
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new(".")),
        );

        // Also push a ConfigOptionUpdate notification so clients that don't
        // inspect the response value (e.g. flynt-app's set_config which
        // discards the response, or any client that triggers a config change
        // through a different path) still see the new state.
        if let Some(sid) = self.session_id.borrow().clone() {
            let conn = self.conn.clone();
            let push_options = options.clone();
            tokio::task::spawn_local(async move {
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = send_session_update(
                        c,
                        sid,
                        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(push_options)),
                    )
                    .await;
                }
            });
        }

        Ok(SetSessionConfigOptionResponse::new(options))
    }

    async fn load_session(&self, args: LoadSessionRequest) -> Result<LoadSessionResponse> {
        // Ensure worker is ready for this cwd
        self.ensure_worker(&args.cwd);

        let mut response = LoadSessionResponse::new();
        response.modes = Some(Self::modes());
        Ok(response)
    }

    async fn list_sessions(&self, args: ListSessionsRequest) -> Result<ListSessionsResponse> {
        let cwd = args
            .cwd
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let entries = crate::session::list_sessions(&cwd);
        let sessions: Vec<SessionInfo> = entries
            .into_iter()
            .take(50)
            .map(|e| {
                SessionInfo::new(
                    SessionId::new(e.meta.session_id.as_str()),
                    std::path::PathBuf::from(&e.meta.cwd),
                )
            })
            .collect();
        Ok(ListSessionsResponse::new(sessions))
    }

    async fn close_session(&self, args: CloseSessionRequest) -> Result<CloseSessionResponse> {
        validate_active_session(&self.session_id.borrow(), &args.session_id)?;
        self.send_to_worker(WorkerRequest::Cancel).await;
        self.send_to_worker(WorkerRequest::Shutdown).await;
        *self.worker.borrow_mut() = None;
        *self.session_id.borrow_mut() = None;
        *self.session_cwd.borrow_mut() = None;
        *self.turn_state.borrow_mut() = AcpTurnState::default();
        tracing::info!("ACP session closed");
        Ok(CloseSessionResponse::new())
    }

    async fn ext_method(&self, args: ExtRequest) -> Result<ExtResponse> {
        let params: serde_json::Value =
            serde_json::from_str(args.params.get()).unwrap_or(serde_json::Value::Null);
        let response_value = match self.handle_ext_method(&args.method, params).await {
            Ok(v) => v,
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        let raw =
            serde_json::value::RawValue::from_string(serde_json::to_string(&response_value)?)?;
        Ok(ExtResponse::new(raw.into()))
    }
}

impl OmegonAcpAgent {
    fn acp_plan_projection_json(&self) -> serde_json::Value {
        let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
        let repo_root = crate::setup::find_project_root(&cwd);
        crate::acp_plan_tasks::projection_json(&repo_root)
    }

    fn repo_relative_path(&self, path: &std::path::Path) -> String {
        let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
        let repo_root = crate::setup::find_project_root(&cwd);
        path.strip_prefix(&repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    fn acp_plan_show_json(&self, params: serde_json::Value) -> serde_json::Value {
        let plan_id = params.get("plan_id").and_then(|v| v.as_str()).unwrap_or("");
        let projection = self.acp_plan_projection_json();
        crate::acp_plan_tasks::plan_show_json(&projection, plan_id)
    }

    async fn acp_plan_control_json(&self, command: String) -> serde_json::Value {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let worker_tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
        let Some(worker_tx) = worker_tx else {
            return serde_json::json!({
                "accepted": false,
                "error": "ACP worker is not initialized",
            });
        };
        if worker_tx
            .send(WorkerRequest::ControlRequest {
                command,
                response_tx: tx,
            })
            .await
            .is_err()
        {
            return serde_json::json!({
                "accepted": false,
                "error": "ACP worker is not accepting requests",
            });
        }
        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(resp)) => serde_json::json!({
                "accepted": resp.error.is_none(),
                "text": resp.text,
                "error": resp.error,
                "cancelled": resp.cancelled,
                "mutation": "session_view_only",
            }),
            Ok(Err(_)) => serde_json::json!({
                "accepted": false,
                "error": "ACP worker dropped plan control response",
            }),
            Err(_) => serde_json::json!({
                "accepted": false,
                "error": "ACP plan control request timed out",
            }),
        }
    }

    fn acp_task_list_json(&self, params: serde_json::Value) -> serde_json::Value {
        let projection = self.acp_plan_projection_json();
        crate::acp_plan_tasks::task_list_json(&projection, &params)
    }

    fn acp_external_task_import_json(&self, params: serde_json::Value) -> serde_json::Value {
        let system = params
            .get("system")
            .and_then(|v| v.as_str())
            .unwrap_or("external")
            .trim();
        let external_id = params
            .get("external_task_id")
            .or_else(|| params.get("flynt_task_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let title = params
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if external_id.is_empty() || title.is_empty() {
            return crate::acp_plan_tasks::task_error(
                "target_required",
                "external_task_id and title are required",
                None,
            );
        }
        let target_kind = params
            .get("target")
            .and_then(|v| v.get("kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("session");
        if target_kind != "session" {
            return crate::acp_plan_tasks::task_error(
                "unsupported_source",
                "external task import currently supports only target.kind=session; use the Flynt agent promotion prompt for OpenSpec/design promotion",
                None,
            );
        }

        let body = params.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let stable_id = format!(
            "external:{}:{}",
            crate::acp_plan_tasks::sanitize_external_id(system),
            crate::acp_plan_tasks::sanitize_external_id(external_id)
        );
        let revision =
            crate::acp_plan_tasks::external_import_revision(system, external_id, title, body);
        let task_id = format!("session:external:{}", stable_id);
        self.session_task_bindings
            .borrow_mut()
            .push(crate::conversation::SessionTaskBinding {
                task_id: task_id.clone(),
                stable_id: Some(stable_id.clone()),
                system: system.to_string(),
                external_task_id: external_id.to_string(),
                revision: revision.clone(),
            });

        serde_json::json!({
            "accepted": true,
            "durability": "session",
            "created": {
                "task_id": task_id,
                "stable_id": stable_id,
                "revision": revision,
                "source": {
                    "kind": "session",
                    "path": serde_json::Value::Null,
                    "anchor": external_id
                },
                "title": title,
                "body": body
            },
            "binding": {
                "system": system,
                "external_task_id": external_id,
                "durability": "session"
            },
            "review": {
                "required": true,
                "reason": "Imported as session-local external task context; promote through the Flynt agent prompt for durable OpenSpec/design lifecycle state."
            }
        })
    }

    fn acp_task_bind_json(&self, params: serde_json::Value) -> serde_json::Value {
        let task_id = params
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let system = params
            .get("system")
            .and_then(|v| v.as_str())
            .unwrap_or("external")
            .trim();
        let external_id = params
            .get("external_task_id")
            .or_else(|| params.get("flynt_task_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if task_id.is_empty() || external_id.is_empty() {
            return crate::acp_plan_tasks::task_error(
                "not_found",
                "task_id and external_task_id are required",
                None,
            );
        }

        let projection = self.acp_plan_projection_json();
        let task = projection
            .get("tasks")
            .and_then(|v| v.as_array())
            .and_then(|items| {
                items.iter().find(|task| {
                    task.get("id").and_then(|v| v.as_str()) == Some(task_id)
                        || task.get("stable_id").and_then(|v| v.as_str()) == Some(task_id)
                })
            });
        let Some(task) = task else {
            return crate::acp_plan_tasks::task_error(
                "not_found",
                "task projection not found",
                None,
            );
        };

        let revision = task.get("revision").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(expected) = params.get("expected_revision").and_then(|v| v.as_str())
            && !expected.is_empty()
            && expected != revision
        {
            return crate::acp_plan_tasks::task_error(
                "stale_revision",
                "task revision does not match expected_revision",
                Some(revision),
            );
        }

        let requested_durability = crate::acp_plan_tasks::requested_bind_durability(&params);

        let resolved_task_id = task
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(task_id)
            .to_string();
        let stable_id = task
            .get("stable_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let binding = serde_json::json!({
            "task_id": resolved_task_id,
            "stable_id": stable_id,
            "system": system,
            "external_task_id": external_id
        });

        if requested_durability == crate::conversation::TaskBindingDurability::Repo {
            let Some(stable_id) = binding.get("stable_id").and_then(|v| v.as_str()) else {
                return crate::acp_plan_tasks::task_error(
                    "unsupported_source",
                    "repo-durable task bindings require stable_id",
                    Some(revision),
                );
            };
            if task.get("stable_id_quality").and_then(|v| v.as_str()) != Some("explicit") {
                return crate::acp_plan_tasks::task_error(
                    "unsupported_source",
                    "repo-durable task bindings require explicit stable task-id markers",
                    Some(revision),
                );
            }
            let source: crate::conversation::PlanTaskSourceRef = match serde_json::from_value(
                task.get("source")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({})),
            ) {
                Ok(source) => source,
                Err(err) => {
                    return crate::acp_plan_tasks::task_error(
                        "unsupported_source",
                        &format!("task source metadata is invalid: {err}"),
                        Some(revision),
                    );
                }
            };
            if source.kind == "session" || source.kind.is_empty() {
                return crate::acp_plan_tasks::task_error(
                    "unsupported_source",
                    "repo-durable task bindings require a repo-backed task source",
                    Some(revision),
                );
            }
            let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
            let repo_root = crate::setup::find_project_root(&cwd);
            let mut store = match crate::conversation::TaskBindingStore::load(&repo_root) {
                Ok(store) => store,
                Err(err) => {
                    return crate::acp_plan_tasks::task_error(
                        "conflict",
                        &format!("failed to load task binding store: {err}"),
                        Some(revision),
                    );
                }
            };
            let timestamp = crate::acp_plan_tasks::current_binding_timestamp();
            store.upsert(crate::conversation::TaskBindingRecord {
                stable_id: stable_id.to_string(),
                task_id: binding
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                system: system.to_string(),
                external_task_id: external_id.to_string(),
                source,
                revision: revision.to_string(),
                created_at: timestamp.clone(),
                updated_at: timestamp,
            });
            if let Err(err) = store.save(&repo_root) {
                return crate::acp_plan_tasks::task_error(
                    "conflict",
                    &format!("failed to save task binding store: {err}"),
                    Some(revision),
                );
            }
            return serde_json::json!({
                "accepted": true,
                "durability": "repo",
                "revision": revision,
                "binding": binding
            });
        }

        self.session_task_bindings
            .borrow_mut()
            .push(crate::conversation::SessionTaskBinding {
                task_id: binding
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                stable_id: binding
                    .get("stable_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                system: system.to_string(),
                external_task_id: external_id.to_string(),
                revision: revision.to_string(),
            });

        serde_json::json!({
            "accepted": true,
            "durability": "session",
            "revision": revision,
            "binding": binding,
            "warning": "Binding accepted as a session/local hint only; it is not repo-durable or authoritative for bidirectional sync."
        })
    }

    fn lifecycle_repo_root(&self) -> std::path::PathBuf {
        let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });
        crate::setup::find_project_root(&cwd)
    }

    fn lifecycle_read_handle(&self) -> crate::lifecycle::read_model::LifecycleReadHandle {
        let repo_root = self.lifecycle_repo_root();
        let provider = std::sync::Arc::new(std::sync::Mutex::new(
            crate::lifecycle::context::LifecycleContextProvider::new(&repo_root),
        ));
        let opsx = std::sync::Arc::new(std::sync::Mutex::new(
            omegon_opsx::Lifecycle::load(omegon_opsx::JsonFileStore::new(&repo_root))
                .expect("lifecycle store should load"),
        ));
        crate::lifecycle::read_model::LifecycleReadHandle::new(provider, opsx, repo_root)
    }

    fn acp_lifecycle_snapshot_json(
        &self,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let include_archived = params
            .get("include_archived")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_specs = params
            .get("include_specs")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let snapshot = self.lifecycle_read_handle().snapshot(
            crate::lifecycle::read_model::SnapshotOptions {
                include_archived,
                include_specs,
            },
        )?;
        Ok(serde_json::json!({
            "openspec": {
                "total_tasks": snapshot.openspec.total_tasks,
                "done_tasks": snapshot.openspec.done_tasks,
                "changes": snapshot.openspec.changes.into_iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "lifecycle_state": c.lifecycle_state,
                    "file_stage": c.file_stage,
                    "has_proposal": c.has_proposal,
                    "has_design": c.has_design,
                    "has_specs": c.has_specs,
                    "has_tasks": c.has_tasks,
                    "total_tasks": c.total_tasks,
                    "done_tasks": c.done_tasks,
                    "archived_on_disk": c.archived_on_disk,
                    "specs": c.specs.into_iter().map(|s| serde_json::json!({
                        "domain": s.domain,
                        "requirements": s.requirements,
                        "scenarios": s.scenarios,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            },
            "tasking": {
                "linked_task_refs": snapshot.tasking.linked_task_refs,
            },
            "drift": snapshot.drift.into_iter().map(|f| serde_json::json!({
                "entity_id": f.entity_id,
                "kind": f.kind,
                "detail": f.detail,
            })).collect::<Vec<_>>(),
        }))
    }

    fn acp_lifecycle_design_list_json(&self) -> anyhow::Result<serde_json::Value> {
        let repo_root = self.lifecycle_repo_root();
        let mut provider = crate::lifecycle::context::LifecycleContextProvider::new(&repo_root);
        provider.refresh();
        let nodes = provider.all_nodes();
        let list = nodes
            .values()
            .filter(|n| !crate::lifecycle::query::is_archived(n))
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "title": n.title,
                    "status": n.status.as_str(),
                    "parent": n.parent,
                    "tags": n.tags,
                    "open_questions": n.open_questions.len(),
                    "dependencies": n.dependencies,
                    "branches": n.branches,
                    "openspec_change": n.openspec_change,
                    "priority": n.priority,
                    "children": crate::lifecycle::design::get_children(nodes, &n.id).len(),
                })
            })
            .collect::<Vec<_>>();
        Ok(serde_json::json!({ "nodes": list }))
    }

    fn acp_lifecycle_design_get_json(
        &self,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let node_id = params
            .get("node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("node_id required"))?;
        let repo_root = self.lifecycle_repo_root();
        let provider = crate::lifecycle::context::LifecycleContextProvider::new(&repo_root);
        let node = provider
            .get_node(node_id)
            .ok_or_else(|| anyhow::anyhow!("Node '{node_id}' not found"))?;
        let sections = crate::lifecycle::design::read_node_sections(node);
        let children = crate::lifecycle::query::children(provider.all_nodes(), node_id);
        let mut result = serde_json::json!({
            "id": node.id,
            "title": node.title,
            "status": node.status.as_str(),
            "parent": node.parent,
            "tags": node.tags,
            "open_questions": node.open_questions,
            "dependencies": node.dependencies,
            "related": node.related,
            "branches": node.branches,
            "openspec_change": node.openspec_change,
            "priority": node.priority,
            "children": children.into_iter().map(|c| serde_json::json!({
                "id": c.id,
                "title": c.title,
                "status": c.status,
            })).collect::<Vec<_>>(),
        });
        if let Some(s) = sections {
            result["overview"] = serde_json::json!(s.overview);
            result["research"] = serde_json::json!(
                s.research
                    .into_iter()
                    .map(|r| serde_json::json!({
                        "heading": r.heading,
                        "content": r.content,
                    }))
                    .collect::<Vec<_>>()
            );
            result["decisions"] = serde_json::json!(
                s.decisions
                    .into_iter()
                    .map(|d| serde_json::json!({
                        "title": d.title,
                        "status": d.status,
                        "rationale": d.rationale,
                    }))
                    .collect::<Vec<_>>()
            );
            result["impl_constraints"] = serde_json::json!(s.impl_constraints);
        }
        Ok(result)
    }

    fn acp_lifecycle_design_query_json(&self, query: &str) -> anyhow::Result<serde_json::Value> {
        let repo_root = self.lifecycle_repo_root();
        let provider = crate::lifecycle::context::LifecycleContextProvider::new(&repo_root);
        let nodes = provider.all_nodes();
        match query {
            "ready" => Ok(serde_json::json!({
                "nodes": crate::lifecycle::query::ready(nodes).into_iter().map(|n| serde_json::json!({
                    "id": n.id,
                    "title": n.title,
                    "priority": n.priority,
                })).collect::<Vec<_>>()
            })),
            "blocked" => Ok(serde_json::json!({
                "nodes": crate::lifecycle::query::blocked(nodes).into_iter().map(|n| serde_json::json!({
                    "id": n.id,
                    "title": n.title,
                    "status": n.status,
                    "blocked_by": n.blocked_by,
                })).collect::<Vec<_>>()
            })),
            "frontier" => Ok(serde_json::json!({
                "nodes": crate::lifecycle::query::frontier(nodes).into_iter().map(|n| serde_json::json!({
                    "id": n.id,
                    "title": n.title,
                    "status": n.status,
                    "open_questions": n.open_questions,
                })).collect::<Vec<_>>()
            })),
            _ => anyhow::bail!("unknown lifecycle design query: {query}"),
        }
    }

    fn acp_task_show_json(&self, params: serde_json::Value) -> serde_json::Value {
        let task_id = params.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let projection = self.acp_plan_projection_json();
        crate::acp_plan_tasks::task_show_json(&projection, task_id)
    }

    async fn handle_ext_method(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        use crate::extensions::{ExtensionManifest, ExtensionState, config_store};

        let extensions_dir = crate::extension_cli::extensions_dir()?;

        match method {
            "runtime/status" => Ok(self.runtime_status_json()),

            "lifecycle/snapshot" => self.acp_lifecycle_snapshot_json(params),
            "lifecycle/design/list" => self.acp_lifecycle_design_list_json(),
            "lifecycle/design/get" => self.acp_lifecycle_design_get_json(params),
            "lifecycle/design/ready" => self.acp_lifecycle_design_query_json("ready"),
            "lifecycle/design/blocked" => self.acp_lifecycle_design_query_json("blocked"),
            "lifecycle/design/frontier" => self.acp_lifecycle_design_query_json("frontier"),

            "provider/status" => Ok(self.provider_status_json()),

            "plans/list" | "_plans/list" => Ok(self.acp_plan_projection_json()),
            "plans/show" | "_plans/show" => Ok(self.acp_plan_show_json(params)),
            "plans/events" | "_plans/events" => Ok(serde_json::json!({
                "events": [],
                "source": "lifecycle_projection_only",
                "note": "Live session plan events are available through the worker session state after a turn; this read surface is intentionally mutation-free."
            })),
            "plans/switch" | "_plans/switch" => {
                let plan_id = params
                    .get("plan_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if plan_id.is_empty() {
                    Ok(serde_json::json!({ "accepted": false, "error": "plan_id is required" }))
                } else {
                    Ok(self
                        .acp_plan_control_json(format!("plan switch {plan_id}"))
                        .await)
                }
            }
            "plans/detach" | "_plans/detach" => {
                let command = params
                    .get("plan_id")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(|id| format!("plan detach {id}"))
                    .unwrap_or_else(|| "plan detach".to_string());
                Ok(self.acp_plan_control_json(command).await)
            }
            "tasks/list" | "_tasks/list" => Ok(self.acp_task_list_json(params)),
            "tasks/show" | "_tasks/show" => Ok(self.acp_task_show_json(params)),
            "tasks/bind" | "_tasks/bind" => Ok(self.acp_task_bind_json(params)),
            "external_tasks/import" | "_external_tasks/import" => {
                Ok(self.acp_external_task_import_json(params))
            }
            "tasks/events" | "_tasks/events" => Ok(crate::acp_plan_tasks::task_events_json(
                &self.session_task_bindings.borrow(),
            )),

            "assistant_runs/list" | "_assistant_runs/list" => {
                let cwd = std::env::current_dir()?;
                let store = crate::capabilities::runs::SqliteAssistantRunStore::open(
                    &crate::paths::assistant_runs_db(&cwd),
                )?;
                Ok(serde_json::json!({ "runs": store.list()? }))
            }
            "assistant_runs/show" | "_assistant_runs/show" => {
                let run_id = params
                    .get("run_id")
                    .or_else(|| params.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let cwd = std::env::current_dir()?;
                let store = crate::capabilities::runs::SqliteAssistantRunStore::open(
                    &crate::paths::assistant_runs_db(&cwd),
                )?;
                match store.get(run_id)? {
                    Some(run) => Ok(serde_json::json!({ "run": run })),
                    None => Ok(serde_json::json!({ "error": "assistant_run_not_found" })),
                }
            }

            "runtime/capabilities" => Ok(serde_json::json!({
                "surfaces": crate::backend::acp_capability_surfaces_json(),
                "features": {
                    "capabilities_inventory": true,
                    "packages": true,
                    "extensions": true,
                    "host_actions": true,
                    "secrets": true,
                    "tools": true,
                    "memory": true,
                    "lifecycle": true,
                    "plans": true,
                    "plan_tasks": true,
                    "plan_tasks_contract": {
                        "compatibility": ["read_only", "manual_link", "session_bind", "repo_bind"],
                        "stable_id": true,
                        "revision": true,
                        "durable_bind": true,
                        "durable_bind_scope": "repo_backed_explicit_stable_id_only",
                        "structured_errors": true,
                        "pagination": false,
                        "filtering": true,
                        "external_import": {
                            "supported": true,
                            "durability": ["session"],
                            "targets": ["session"]
                        }
                    }
                },
                "protocols": {
                    "acp": ["1"]
                }
            })),

            "capabilities/assistant_readiness" | "_capabilities/assistant_readiness" => {
                let id = params
                    .get("id")
                    .or_else(|| params.get("assistant_id"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("id required"))?;
                let home = crate::paths::omegon_home()?;
                let cwd = std::env::current_dir()?;
                let armory_home = home.join("armory");
                let project_armory = cwd.join("../omegon-armory");
                let armory_root = if !armory_home.join("profiles").exists()
                    && project_armory.join("profiles").exists()
                {
                    project_armory.as_path()
                } else {
                    armory_home.as_path()
                };
                let roots = crate::capabilities::inventory::CapabilityInventoryRoots {
                    extensions_dir: &home.join("extensions"),
                    armory_root,
                    catalog_dir: &home.join("catalog"),
                };
                let secret_inputs = self
                    .secrets
                    .borrow()
                    .as_ref()
                    .map(
                        |secrets| crate::capabilities::secrets::SecretReadinessInputs {
                            session_diagnostics: secrets
                                .session_diagnostics()
                                .into_iter()
                                .map(
                                    |diag| crate::capabilities::secrets::SecretSessionDiagnostic {
                                        name: diag.name,
                                        warmed: diag.warmed,
                                    },
                                )
                                .collect(),
                            recipe_descriptors: secrets
                                .list_recipe_descriptors()
                                .into_iter()
                                .map(|descriptor| {
                                    crate::capabilities::secrets::SecretRecipeDescriptorSummary {
                                        name: descriptor.name,
                                        source: (descriptor.kind == "env")
                                            .then_some(descriptor.payload),
                                        kind: descriptor.kind,
                                    }
                                })
                                .collect(),
                            checked_names: Vec::new(),
                        },
                    )
                    .unwrap_or_default();
                let snapshot =
                    crate::capabilities::inventory::build_capability_inventory_snapshot_with_secrets(
                        roots,
                        secret_inputs,
                    )?;
                let Some(assistant) = snapshot
                    .assistant_list
                    .into_iter()
                    .find(|assistant| assistant.id == id)
                else {
                    return Ok(serde_json::json!({ "error": "assistant_not_found" }));
                };
                Ok(serde_json::json!({ "assistant": assistant }))
            }

            "capabilities/assistants" | "_capabilities/assistants" => {
                let home = crate::paths::omegon_home()?;
                let cwd = std::env::current_dir()?;
                let armory_home = home.join("armory");
                let project_armory = cwd.join("../omegon-armory");
                let armory_root = if !armory_home.join("profiles").exists()
                    && project_armory.join("profiles").exists()
                {
                    project_armory.as_path()
                } else {
                    armory_home.as_path()
                };
                let roots = crate::capabilities::inventory::CapabilityInventoryRoots {
                    extensions_dir: &home.join("extensions"),
                    armory_root,
                    catalog_dir: &home.join("catalog"),
                };
                let secret_inputs = self
                    .secrets
                    .borrow()
                    .as_ref()
                    .map(
                        |secrets| crate::capabilities::secrets::SecretReadinessInputs {
                            session_diagnostics: secrets
                                .session_diagnostics()
                                .into_iter()
                                .map(
                                    |diag| crate::capabilities::secrets::SecretSessionDiagnostic {
                                        name: diag.name,
                                        warmed: diag.warmed,
                                    },
                                )
                                .collect(),
                            recipe_descriptors: secrets
                                .list_recipe_descriptors()
                                .into_iter()
                                .map(|descriptor| {
                                    crate::capabilities::secrets::SecretRecipeDescriptorSummary {
                                        name: descriptor.name,
                                        source: (descriptor.kind == "env")
                                            .then_some(descriptor.payload),
                                        kind: descriptor.kind,
                                    }
                                })
                                .collect(),
                            checked_names: Vec::new(),
                        },
                    )
                    .unwrap_or_default();
                let snapshot =
                    crate::capabilities::inventory::build_capability_inventory_snapshot_with_secrets(
                        roots,
                        secret_inputs,
                    )?;
                Ok(serde_json::json!({ "assistants": snapshot.assistant_list }))
            }

            "capabilities/inventory" | "_capabilities/inventory" => {
                let home = crate::paths::omegon_home()?;
                let cwd = std::env::current_dir()?;
                let armory_home = home.join("armory");
                let project_armory = cwd.join("../omegon-armory");
                let armory_root = if !armory_home.join("profiles").exists()
                    && project_armory.join("profiles").exists()
                {
                    project_armory.as_path()
                } else {
                    armory_home.as_path()
                };
                let roots = crate::capabilities::inventory::CapabilityInventoryRoots {
                    extensions_dir: &home.join("extensions"),
                    armory_root,
                    catalog_dir: &home.join("catalog"),
                };
                let secret_inputs = self
                    .secrets
                    .borrow()
                    .as_ref()
                    .map(
                        |secrets| crate::capabilities::secrets::SecretReadinessInputs {
                            session_diagnostics: secrets
                                .session_diagnostics()
                                .into_iter()
                                .map(
                                    |diag| crate::capabilities::secrets::SecretSessionDiagnostic {
                                        name: diag.name,
                                        warmed: diag.warmed,
                                    },
                                )
                                .collect(),
                            recipe_descriptors: secrets
                                .list_recipe_descriptors()
                                .into_iter()
                                .map(|descriptor| {
                                    crate::capabilities::secrets::SecretRecipeDescriptorSummary {
                                        name: descriptor.name,
                                        source: (descriptor.kind == "env")
                                            .then_some(descriptor.payload),
                                        kind: descriptor.kind,
                                    }
                                })
                                .collect(),
                            checked_names: Vec::new(),
                        },
                    )
                    .unwrap_or_default();
                let snapshot =
                    crate::capabilities::inventory::build_capability_inventory_snapshot_with_secrets(
                        roots,
                        secret_inputs,
                    )?;
                Ok(serde_json::to_value(snapshot)?)
            }

            "extensions/list" => {
                let mut extensions = Vec::new();
                if extensions_dir.exists() {
                    for entry in std::fs::read_dir(&extensions_dir)?.flatten() {
                        let dir = entry.path();
                        if !dir.is_dir() {
                            continue;
                        }
                        let manifest_path = dir.join("manifest.toml");
                        if !manifest_path.exists() {
                            continue;
                        }
                        let Ok(manifest) = ExtensionManifest::from_extension_dir(&dir) else {
                            continue;
                        };
                        let state = ExtensionState::load(&dir).unwrap_or_default();
                        let config_values = config_store::read_config(&dir).unwrap_or_default();

                        let config_schema: serde_json::Map<String, serde_json::Value> = manifest
                            .config
                            .iter()
                            .map(|(name, field)| {
                                let mut entry = serde_json::to_value(field).unwrap_or_default();
                                if let Some(obj) = entry.as_object_mut()
                                    && let Some(val) = config_values.get(name)
                                {
                                    obj.insert(
                                        "current_value".into(),
                                        serde_json::Value::String(val.clone()),
                                    );
                                }
                                (name.clone(), entry)
                            })
                            .collect();

                        let secret_status = |names: &[String]| -> Vec<serde_json::Value> {
                            names
                                .iter()
                                .map(|name| {
                                    // Check recipe existence only — don't call resolve()
                                    // which can trigger keychain prompts or shell execution.
                                    let (has_recipe, source) = if let Some(ref mgr) =
                                        *self.secrets.borrow()
                                    {
                                        let recipes = mgr.list_recipes();
                                        match recipes.iter().find(|(n, _)| n == name) {
                                            Some((_, r)) => {
                                                let src = if r.starts_with("keyring:") {
                                                    "keyring"
                                                } else if r.starts_with("env:") {
                                                    "env"
                                                } else if r.starts_with("vault:") {
                                                    "vault"
                                                } else if r.starts_with("cmd:") {
                                                    "cmd"
                                                } else {
                                                    "recipe"
                                                };
                                                (true, Some(String::from(src)))
                                            }
                                            None => {
                                                // Fallback: check env var existence (cheap)
                                                let in_env = std::env::var(name).is_ok();
                                                (
                                                    in_env,
                                                    if in_env { Some("env".into()) } else { None },
                                                )
                                            }
                                        }
                                    } else {
                                        (false, None)
                                    };
                                    serde_json::json!({
                                        "name": name,
                                        "resolved": has_recipe,
                                        "source": source,
                                    })
                                })
                                .collect()
                        };

                        let id = manifest.extension.name.clone();
                        let metadata = self
                            .extension_metadata
                            .borrow()
                            .get(&id)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let callable = self.extension_rpc_handles.borrow().contains_key(&id);
                        let loaded = callable || self.extension_metadata.borrow().contains_key(&id);
                        extensions.push(serde_json::json!({
                            "id": id,
                            "name": manifest.extension.name,
                            "version": manifest.extension.version,
                            "description": manifest.extension.description,
                            "enabled": state.enabled,
                            "loaded": loaded,
                            "callable": callable,
                            "path": dir.display().to_string(),
                            "source": "installed",
                            "capabilities": {
                                "tools": false,
                                "resources": false,
                                "prompts": false,
                                "voice": manifest.capabilities.voice,
                            },
                            "metadata": metadata,
                            "last_error": state.stability.last_error,
                            "stability": {
                                "crashes_this_session": state.stability.crashes_this_session,
                                "health_check_failures": state.stability.health_check_failures,
                                "last_error": state.stability.last_error,
                                "last_error_at": state.stability.last_error_at,
                                "auto_disabled": state.stability.auto_disabled,
                            },
                            "config_schema": config_schema,
                            "secrets": {
                                "required": secret_status(&manifest.secrets.required),
                                "optional": secret_status(&manifest.secrets.optional),
                            },
                        }));
                    }
                }
                extensions.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                Ok(serde_json::json!({ "extensions": extensions }))
            }

            "extensions/call" => {
                let extension = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid_request: missing 'extension' field"))?;
                let rpc_method = params["method"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid_request: missing 'method' field"))?;
                let rpc_params = params
                    .get("params")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                extension_rpc::call_extension_rpc(
                    &self.extension_rpc_handles,
                    extension,
                    rpc_method,
                    rpc_params,
                )
                .await
            }

            "extensions/get" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                let ext_dir = extensions_dir.join(ext_name);
                if !ext_dir.exists() {
                    anyhow::bail!("extension '{ext_name}' not found");
                }
                let manifest = ExtensionManifest::from_extension_dir(&ext_dir)?;
                let state = ExtensionState::load(&ext_dir).unwrap_or_default();
                let config_values = config_store::read_config(&ext_dir).unwrap_or_default();

                let config_schema: serde_json::Map<String, serde_json::Value> = manifest
                    .config
                    .iter()
                    .map(|(name, field)| {
                        let mut entry = serde_json::to_value(field).unwrap_or_default();
                        if let Some(obj) = entry.as_object_mut()
                            && let Some(val) = config_values.get(name)
                        {
                            obj.insert(
                                "current_value".into(),
                                serde_json::Value::String(val.clone()),
                            );
                        }
                        (name.clone(), entry)
                    })
                    .collect();

                let secret_status = |names: &[String]| -> Vec<serde_json::Value> {
                    names
                        .iter()
                        .map(|name| {
                            let (has_recipe, source) = if let Some(ref mgr) = *self.secrets.borrow()
                            {
                                let recipes = mgr.list_recipes();
                                match recipes.iter().find(|(n, _)| n == name) {
                                    Some((_, r)) => {
                                        let src = if r.starts_with("keyring:") {
                                            "keyring"
                                        } else if r.starts_with("env:") {
                                            "env"
                                        } else if r.starts_with("vault:") {
                                            "vault"
                                        } else if r.starts_with("cmd:") {
                                            "cmd"
                                        } else {
                                            "recipe"
                                        };
                                        (true, Some(String::from(src)))
                                    }
                                    None => {
                                        let in_env = std::env::var(name).is_ok();
                                        (in_env, if in_env { Some("env".into()) } else { None })
                                    }
                                }
                            } else {
                                (false, None)
                            };
                            serde_json::json!({
                                "name": name,
                                "resolved": has_recipe,
                                "source": source,
                            })
                        })
                        .collect()
                };

                let runtime_type = match &manifest.runtime {
                    crate::extensions::manifest::RuntimeConfig::Native { .. } => "native",
                    crate::extensions::manifest::RuntimeConfig::Oci { .. } => "oci",
                };

                Ok(serde_json::json!({
                    "name": manifest.extension.name,
                    "version": manifest.extension.version,
                    "description": manifest.extension.description,
                    "runtime": runtime_type,
                    "enabled": state.enabled,
                    "stability": {
                        "crashes_this_session": state.stability.crashes_this_session,
                        "health_check_failures": state.stability.health_check_failures,
                        "last_error": state.stability.last_error,
                        "last_error_at": state.stability.last_error_at,
                        "auto_disabled": state.stability.auto_disabled,
                    },
                    "config_schema": config_schema,
                    "config_values": config_values,
                    "secrets": {
                        "required": secret_status(&manifest.secrets.required),
                        "optional": secret_status(&manifest.secrets.optional),
                    },
                    "path": ext_dir.display().to_string(),
                }))
            }

            "extensions/config_get" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                let ext_dir = extensions_dir.join(ext_name);
                if !ext_dir.exists() {
                    anyhow::bail!("extension '{ext_name}' not found");
                }
                let config = config_store::read_config(&ext_dir)?;
                Ok(serde_json::json!({ "config": config }))
            }

            "extensions/config_set" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                let key = params["key"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'key' field"))?;
                let value = params["value"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'value' field"))?;
                let ext_dir = extensions_dir.join(ext_name);
                if !ext_dir.exists() {
                    anyhow::bail!("extension '{ext_name}' not found");
                }
                let manifest = ExtensionManifest::from_extension_dir(&ext_dir)?;
                if !manifest.config.is_empty() {
                    let field = manifest.config.get(key).ok_or_else(|| {
                        anyhow::anyhow!(
                            "unknown config key '{key}' for extension '{ext_name}'. \
                             Declared keys: {:?}",
                            manifest.config.keys().collect::<Vec<_>>()
                        )
                    })?;
                    config_store::validate_field(field, value)?;
                }
                config_store::write_config_value(&ext_dir, key, value)?;
                Ok(serde_json::json!({ "ok": true }))
            }

            "extensions/secret_set" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let value = params["value"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'value' field"))?;
                let ext_dir = extensions_dir.join(ext_name);
                if !ext_dir.exists() {
                    anyhow::bail!("extension '{ext_name}' not found");
                }
                let manifest = ExtensionManifest::from_extension_dir(&ext_dir)?;
                let all_secrets: Vec<&str> = manifest
                    .secrets
                    .required
                    .iter()
                    .chain(manifest.secrets.optional.iter())
                    .map(|s| s.as_str())
                    .collect();
                if !all_secrets.is_empty() && !all_secrets.contains(&name) {
                    anyhow::bail!(
                        "secret '{name}' is not declared by extension '{ext_name}'. \
                         Declared secrets: {:?}",
                        all_secrets
                    );
                }
                if let Some(ref mgr) = *self.secrets.borrow() {
                    mgr.set_keyring_secret(name, value)?;
                    Ok(serde_json::json!({ "ok": true, "source": "keyring" }))
                } else {
                    anyhow::bail!("secrets manager not available — agent still initializing")
                }
            }

            "secrets/capabilities" => Ok(serde_json::json!({
                "version": 1,
                "operations": {
                    "list": true,
                    "check": true,
                    "set_value": true,
                    "set_recipe": true,
                    "extension_secret_set": true
                },
                "recipe_kinds": ["env", "keyring", "keychain", "vault", "file", "cmd", "unknown"],
                "storage": {
                    "preferred": "keyring",
                    "recipes": true,
                    "vault": true,
                    "env_fallback": true
                },
                "safety": {
                    "values_write_only": true,
                    "list_resolves_values": false,
                    "list_executes_recipes": false,
                    "check_returns_value": false
                }
            })),

            "secrets/list" => {
                if let Some(ref mgr) = *self.secrets.borrow() {
                    let items: Vec<serde_json::Value> = mgr
                        .list_recipe_descriptors()
                        .into_iter()
                        .map(|entry| {
                            serde_json::json!({
                                "name": entry.name,
                                "recipe": entry.recipe,
                                "kind": entry.kind,
                                "payload": entry.payload,
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({ "items": items }))
                } else {
                    anyhow::bail!("secrets manager not available — agent still initializing")
                }
            }

            "secrets/set_value" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let value = params["value"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'value' field"))?;
                if let Some(ref mgr) = *self.secrets.borrow() {
                    mgr.set_keyring_secret(name, value)?;
                    Ok(serde_json::json!({ "ok": true, "source": "keyring" }))
                } else {
                    anyhow::bail!("secrets manager not available — agent still initializing")
                }
            }

            "secrets/set_recipe" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let recipe = params["recipe"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'recipe' field"))?;
                if let Some(ref mgr) = *self.secrets.borrow() {
                    mgr.set_recipe(name, recipe)?;
                    Ok(serde_json::json!({ "ok": true, "source": "recipe" }))
                } else {
                    anyhow::bail!("secrets manager not available — agent still initializing")
                }
            }

            "secrets/check" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                if let Some(ref mgr) = *self.secrets.borrow() {
                    Ok(serde_json::json!({ "name": name, "resolved": mgr.resolve(name).is_some() }))
                } else {
                    anyhow::bail!("secrets manager not available — agent still initializing")
                }
            }

            "secrets/delete" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                if let Some(ref mgr) = *self.secrets.borrow() {
                    mgr.delete_recipe(name)?;
                    Ok(serde_json::json!({ "ok": true }))
                } else {
                    anyhow::bail!("secrets manager not available — agent still initializing")
                }
            }

            "extensions/secret_delete" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                if let Some(ref mgr) = *self.secrets.borrow() {
                    mgr.delete_recipe(name)?;
                    Ok(serde_json::json!({ "ok": true }))
                } else {
                    anyhow::bail!("secrets manager not available")
                }
            }

            "extensions/enable" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                let ext_dir = extensions_dir.join(ext_name);
                if !ext_dir.exists() {
                    anyhow::bail!("extension '{ext_name}' not found");
                }
                let mut state = ExtensionState::load(&ext_dir).unwrap_or_default();
                state.mark_enabled();
                state.save(&ext_dir)?;
                Ok(serde_json::json!({ "ok": true }))
            }

            "extensions/disable" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                let ext_dir = extensions_dir.join(ext_name);
                if !ext_dir.exists() {
                    anyhow::bail!("extension '{ext_name}' not found");
                }
                let mut state = ExtensionState::load(&ext_dir).unwrap_or_default();
                state.mark_disabled();
                state.save(&ext_dir)?;
                Ok(serde_json::json!({ "ok": true }))
            }

            // ── Extension CRUD ──────────────────────────────────────
            "extensions/install" => {
                let uri = params["uri"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'uri' field"))?;
                let result = crate::armory::install_extension(uri, None).await?;
                Ok(serde_json::json!({ "ok": true, "result": result }))
            }

            "extensions/remove" => {
                let ext_name = params["extension"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'extension' field"))?;
                crate::extension_cli::remove(ext_name)?;
                Ok(serde_json::json!({ "ok": true }))
            }

            "extensions/update" => {
                let ext_name = params.get("extension").and_then(|v| v.as_str());
                crate::extension_cli::update(ext_name)?;
                Ok(serde_json::json!({ "ok": true }))
            }

            // ── Discovery (armory + catalog) ──────────────────────
            "armory/browse" | "armory/search" => {
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(parse_armory_kind)
                    .transpose()?
                    .unwrap_or(crate::armory::ArmoryKind::All);
                let query = params.get("query").and_then(|v| v.as_str());
                let cwd = std::env::current_dir().unwrap_or_default();
                let items =
                    crate::armory::browse(crate::armory::BrowseOptions::new(kind, query, &cwd))
                        .await?;
                let items: Vec<serde_json::Value> =
                    items.into_iter().map(armory_search_item_json).collect();
                Ok(serde_json::json!({ "items": items }))
            }

            "armory/install" => {
                let target = params["target"]
                    .as_str()
                    .or_else(|| params["uri"].as_str())
                    .ok_or_else(|| anyhow::anyhow!("missing 'target' field"))?;
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(parse_armory_kind)
                    .transpose()?
                    .map(crate::armory::ArmoryInstallKind::from)
                    .unwrap_or(crate::armory::ArmoryInstallKind::Auto);
                let cwd = std::env::current_dir().unwrap_or_default();
                let result = crate::armory::install(target, kind, &cwd).await?;
                Ok(serde_json::json!({
                    "ok": true,
                    "installed": armory_install_result_json(&result),
                    "result": result,
                }))
            }

            "packages/plan" => {
                let request = crate::packages::request_from_params(&params)?;
                Ok(serde_json::to_value(crate::packages::plan(&request))?)
            }

            "packages/install" => {
                let request = crate::packages::request_from_params(&params)?;
                let cwd = std::env::current_dir().unwrap_or_default();
                Ok(serde_json::to_value(
                    crate::packages::install(request, &cwd).await?,
                )?)
            }

            "packages/search" => {
                let cwd = std::env::current_dir().unwrap_or_default();
                crate::packages::search(&params, &cwd).await
            }

            "packages/list" => crate::packages::list(),

            "packages/remove" => crate::packages::remove(&params),

            "packages/update" => crate::packages::update(&params),

            "extensions/search" => {
                let query = params.get("query").and_then(|v| v.as_str());
                let cwd = std::env::current_dir().unwrap_or_default();
                let items = crate::armory::browse(crate::armory::BrowseOptions::new(
                    crate::armory::ArmoryKind::Extensions,
                    query,
                    &cwd,
                ))
                .await?;
                let extensions: Vec<serde_json::Value> = items
                    .into_iter()
                    .map(|item| {
                        serde_json::json!({
                            "name": item.id,
                            "description": item.description,
                            "category": item.category,
                            "repo": item.source,
                            "installed": item.installed,
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "extensions": extensions }))
            }

            "catalog/list" => {
                let home = crate::paths::omegon_home()?;
                let entries: Vec<serde_json::Value> = crate::catalog::list(&home)
                    .into_iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.id,
                            "name": e.name,
                            "version": e.version,
                            "description": e.description,
                            "domain": e.domain,
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "agents": entries }))
            }

            "catalog/get" => {
                let agent_id = params["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'id' field"))?;
                let home = crate::paths::omegon_home()?;
                let resolved = crate::catalog::resolve(&home, agent_id)?;
                let m = &resolved.manifest;
                let mut result = serde_json::json!({
                    "id": m.agent.id,
                    "name": m.agent.name,
                    "version": m.agent.version,
                    "description": m.agent.description,
                    "domain": m.agent.domain,
                    "path": resolved.bundle_dir.display().to_string(),
                });
                let obj = result.as_object_mut().unwrap();
                if let Some(ref persona) = m.persona {
                    obj.insert(
                        "persona".into(),
                        serde_json::json!({
                            "badge": persona.badge,
                            "activated_skills": persona.activated_skills,
                            "disabled_tools": persona.disabled_tools,
                            "has_directive": resolved.persona_directive.is_some(),
                            "has_mind_facts": resolved.mind_facts_content.is_some(),
                        }),
                    );
                }
                if let Some(ref settings) = m.settings {
                    obj.insert(
                        "settings".into(),
                        serde_json::json!({
                            "model": settings.model,
                            "thinking_level": settings.thinking_level,
                            "context_class": settings.context_class,
                            "max_turns": settings.max_turns,
                        }),
                    );
                }
                if let Some(ref extensions) = m.extensions {
                    let ext_list: Vec<serde_json::Value> = extensions
                        .iter()
                        .map(|e| serde_json::json!({ "name": e.name, "version": e.version }))
                        .collect();
                    obj.insert("extensions".into(), serde_json::json!(ext_list));
                }
                if let Some(ref workflow) = m.workflow {
                    let phases: Vec<String> = workflow
                        .phases
                        .as_ref()
                        .map(|p| p.keys().cloned().collect())
                        .unwrap_or_default();
                    obj.insert(
                        "workflow".into(),
                        serde_json::json!({
                            "name": workflow.name,
                            "phases": phases,
                        }),
                    );
                }
                Ok(result)
            }

            "catalog/install" => {
                let offline = params
                    .get("offline")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                crate::catalog::cmd_install(offline).await?;
                Ok(serde_json::json!({ "ok": true }))
            }

            "catalog/remove" => {
                let agent_id = params["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'id' field"))?;
                if agent_id.contains('/')
                    || agent_id.contains('\\')
                    || agent_id.contains("..")
                    || agent_id.contains('\0')
                {
                    anyhow::bail!("invalid agent ID: path traversal rejected");
                }
                let home = crate::paths::omegon_home()?;
                let catalog_dir = home.join("catalog");
                // Find by directory name or by agent.id in manifests
                let entries = crate::catalog::list(&home);
                let entry = entries
                    .iter()
                    .find(|e| e.id == agent_id)
                    .ok_or_else(|| anyhow::anyhow!("catalog agent '{agent_id}' not found"))?;
                if entry.bundle_dir.exists() {
                    // Safety: ensure we're only removing from within catalog/
                    if !entry.bundle_dir.starts_with(&catalog_dir) {
                        anyhow::bail!("refusing to remove agent outside catalog directory");
                    }
                    std::fs::remove_dir_all(&entry.bundle_dir)?;
                }
                Ok(serde_json::json!({ "ok": true }))
            }

            // ── Personas ──────────────────────────────────────────
            "personas/list" => {
                let (personas, tones) = crate::plugins::persona_loader::scan_available();
                let persona_entries: Vec<serde_json::Value> = personas
                    .iter()
                    .map(|p| {
                        let directive =
                            std::fs::read_to_string(p.path.join("PERSONA.md")).unwrap_or_default();
                        serde_json::json!({
                            "id": p.id,
                            "name": p.name,
                            "description": p.description,
                            "directive_preview": if directive.len() > 500 {
                                format!("{}...", crate::util::truncate_str(&directive, 500))
                            } else {
                                directive
                            },
                            "path": p.path.display().to_string(),
                        })
                    })
                    .collect();
                let tone_entries: Vec<serde_json::Value> = tones
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "id": t.id,
                            "name": t.name,
                            "description": t.description,
                            "path": t.path.display().to_string(),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({
                    "personas": persona_entries,
                    "tones": tone_entries,
                }))
            }

            "personas/get" => {
                let id = params["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'id' field"))?;
                let (personas, _) = crate::plugins::persona_loader::scan_available();
                let p = personas
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| anyhow::anyhow!("persona '{id}' not found"))?;
                let directive =
                    std::fs::read_to_string(p.path.join("PERSONA.md")).unwrap_or_default();
                let manifest_content =
                    std::fs::read_to_string(p.path.join("plugin.toml")).unwrap_or_default();

                // Parse disabled_tools and badge from manifest
                let manifest =
                    crate::plugins::armory::ArmoryManifest::parse(&manifest_content).ok();
                let disabled_tools: Vec<String> = manifest
                    .as_ref()
                    .and_then(|m| m.persona.as_ref())
                    .and_then(|p| p.tools.as_ref())
                    .map(|t| t.disable.clone())
                    .unwrap_or_default();
                let activated_skills: Vec<String> = manifest
                    .as_ref()
                    .and_then(|m| m.persona.as_ref())
                    .and_then(|p| p.skills.as_ref())
                    .map(|s| s.activate.clone())
                    .unwrap_or_default();
                let badge = manifest
                    .as_ref()
                    .and_then(|m| m.persona.as_ref())
                    .and_then(|p| p.style.as_ref())
                    .and_then(|s| s.badge.clone());

                Ok(serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "directive": directive,
                    "disabled_tools": disabled_tools,
                    "activated_skills": activated_skills,
                    "badge": badge,
                    "path": p.path.display().to_string(),
                }))
            }

            "personas/create" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let directive = params["directive"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'directive' field"))?;
                let description = params
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let badge = params.get("badge").and_then(|v| v.as_str());
                let disabled_tools: Vec<String> = params
                    .get("disabled_tools")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let slug: String = name
                    .to_lowercase()
                    .replace(' ', "-")
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
                if slug.is_empty() || slug.contains("..") {
                    anyhow::bail!("invalid persona name — must contain alphanumeric characters");
                }
                let home = crate::paths::omegon_home()?;
                let persona_dir = home.join("armory/personas").join(&slug);
                std::fs::create_dir_all(&persona_dir)?;

                let id = format!("user.{slug}");

                // Build plugin.toml via toml serialization to prevent injection
                let mut plugin = toml::Table::new();
                let mut plugin_section = toml::Table::new();
                plugin_section.insert("type".into(), "persona".into());
                plugin_section.insert("id".into(), id.clone().into());
                plugin_section.insert("name".into(), name.into());
                plugin_section.insert("version".into(), "1.0.0".into());
                plugin_section.insert("description".into(), description.into());
                plugin.insert("plugin".into(), toml::Value::Table(plugin_section));

                let mut persona = toml::Table::new();
                let mut identity = toml::Table::new();
                identity.insert("directive".into(), "PERSONA.md".into());
                persona.insert("identity".into(), toml::Value::Table(identity));

                if !disabled_tools.is_empty() {
                    let mut tools = toml::Table::new();
                    tools.insert(
                        "disable".into(),
                        toml::Value::Array(
                            disabled_tools
                                .iter()
                                .map(|s| toml::Value::String(s.clone()))
                                .collect(),
                        ),
                    );
                    persona.insert("tools".into(), toml::Value::Table(tools));
                }

                if let Some(b) = badge {
                    let mut style = toml::Table::new();
                    style.insert("badge".into(), b.into());
                    persona.insert("style".into(), toml::Value::Table(style));
                }

                plugin.insert("persona".into(), toml::Value::Table(persona));

                std::fs::write(
                    persona_dir.join("plugin.toml"),
                    toml::to_string_pretty(&plugin)?,
                )?;
                std::fs::write(persona_dir.join("PERSONA.md"), directive)?;

                Ok(serde_json::json!({
                    "ok": true,
                    "id": id,
                    "path": persona_dir.display().to_string(),
                }))
            }

            "personas/delete" => {
                let id = params["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'id' field"))?;
                let (personas, _) = crate::plugins::persona_loader::scan_available();
                match personas.iter().find(|p| p.id == id) {
                    Some(p) => {
                        if p.path.exists() {
                            std::fs::remove_dir_all(&p.path)?;
                        }
                        Ok(serde_json::json!({ "ok": true }))
                    }
                    None => anyhow::bail!("persona '{id}' not found"),
                }
            }

            "personas/update" => {
                let id = params["id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'id' field"))?;
                let (personas, _) = crate::plugins::persona_loader::scan_available();
                let p = personas
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| anyhow::anyhow!("persona '{id}' not found"))?;
                if !p.path.exists() {
                    anyhow::bail!("persona directory not found at {}", p.path.display());
                }

                // Update directive if provided
                if let Some(directive) = params.get("directive").and_then(|v| v.as_str()) {
                    std::fs::write(p.path.join("PERSONA.md"), directive)?;
                }

                // Update manifest fields if any are provided
                let manifest_path = p.path.join("plugin.toml");
                let manifest_content = std::fs::read_to_string(&manifest_path)?;
                let mut manifest: toml::Table = toml::from_str(&manifest_content)?;

                if let Some(name) = params.get("name").and_then(|v| v.as_str())
                    && let Some(plugin) = manifest.get_mut("plugin").and_then(|v| v.as_table_mut())
                {
                    plugin.insert("name".into(), name.into());
                }
                if let Some(desc) = params.get("description").and_then(|v| v.as_str())
                    && let Some(plugin) = manifest.get_mut("plugin").and_then(|v| v.as_table_mut())
                {
                    plugin.insert("description".into(), desc.into());
                }
                if let Some(badge) = params.get("badge").and_then(|v| v.as_str()) {
                    let persona = manifest
                        .entry("persona")
                        .or_insert(toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .unwrap();
                    let style = persona
                        .entry("style")
                        .or_insert(toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .unwrap();
                    style.insert("badge".into(), badge.into());
                }
                if let Some(disabled_tools) =
                    params.get("disabled_tools").and_then(|v| v.as_array())
                {
                    let tools_arr: Vec<toml::Value> = disabled_tools
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| toml::Value::String(s.to_string())))
                        .collect();
                    let persona = manifest
                        .entry("persona")
                        .or_insert(toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .unwrap();
                    let tools = persona
                        .entry("tools")
                        .or_insert(toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .unwrap();
                    tools.insert("disable".into(), toml::Value::Array(tools_arr));
                }
                if let Some(activated_skills) =
                    params.get("activated_skills").and_then(|v| v.as_array())
                {
                    let skills_arr: Vec<toml::Value> = activated_skills
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| toml::Value::String(s.to_string())))
                        .collect();
                    let persona = manifest
                        .entry("persona")
                        .or_insert(toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .unwrap();
                    let skills = persona
                        .entry("skills")
                        .or_insert(toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .unwrap();
                    skills.insert("activate".into(), toml::Value::Array(skills_arr));
                }

                std::fs::write(&manifest_path, toml::to_string_pretty(&manifest)?)?;

                Ok(serde_json::json!({
                    "ok": true,
                    "path": p.path.display().to_string(),
                }))
            }

            // ── Skills ────────────────────────────────────────────
            "skills/list" => {
                let entries = crate::skills::list_structured()?;
                Ok(serde_json::json!({ "skills": entries }))
            }

            "skills/get" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let (manifest, body, path) = crate::skills::get_skill(name)?;
                Ok(serde_json::json!({
                    "name": manifest.name,
                    "description": manifest.description,
                    "id": manifest.id,
                    "version": manifest.version,
                    "tags": manifest.tags,
                    "aliases": manifest.aliases,
                    "triggers": manifest.triggers,
                    "trusted_paths": manifest.trusted_paths,
                    "output_path": manifest.output_path,
                    "output_format": manifest.output_format,
                    "max_turns": manifest.max_turns,
                    "posture": manifest.posture,
                    "body": body,
                    "path": path.display().to_string(),
                }))
            }

            "skills/install" => {
                if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let result =
                        crate::armory::install(name, crate::armory::ArmoryInstallKind::Skill, &cwd)
                            .await?;
                    Ok(serde_json::json!({ "ok": true, "result": result }))
                } else {
                    crate::skills::cmd_install()?;
                    Ok(serde_json::json!({ "ok": true }))
                }
            }

            "skills/create" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'content' field (SKILL.md body)"))?;
                let project_local = params
                    .get("project_local")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let slug: String = name
                    .to_lowercase()
                    .replace(' ', "-")
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
                if slug.is_empty() || slug.contains("..") {
                    anyhow::bail!("invalid skill name");
                }

                let skill_dir = if project_local {
                    let cwd = std::env::current_dir()?;
                    cwd.join(".omegon/skills").join(&slug)
                } else {
                    let home = crate::paths::omegon_home()?;
                    home.join("skills").join(&slug)
                };
                std::fs::create_dir_all(&skill_dir)?;
                std::fs::write(skill_dir.join("SKILL.md"), content)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "path": skill_dir.display().to_string(),
                }))
            }

            "skills/update" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                if name.contains('/')
                    || name.contains('\\')
                    || name.contains("..")
                    || name.contains('\0')
                {
                    anyhow::bail!("invalid skill name: path traversal rejected");
                }

                // Find the skill — project-local first, then user-installed
                let cwd = std::env::current_dir()?;
                let project_path = cwd.join(".omegon/skills").join(name).join("SKILL.md");
                let home = crate::paths::omegon_home()?;
                let user_path = home.join("skills").join(name).join("SKILL.md");

                let skill_file = if project_path.exists() {
                    project_path
                } else if user_path.exists() {
                    user_path
                } else {
                    anyhow::bail!("skill '{name}' not found");
                };

                if let Some(content) = params.get("content").and_then(|v| v.as_str()) {
                    std::fs::write(&skill_file, content)?;
                } else {
                    // Partial update: merge provided manifest fields with existing content
                    let existing = std::fs::read_to_string(&skill_file)?;
                    let (mut manifest, body) = omegon_skills::parse_skill_file(&existing);

                    if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
                        manifest.description = desc.to_string();
                    }
                    if let Some(tags) = params.get("tags").and_then(|v| v.as_array()) {
                        manifest.tags = tags
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                    if let Some(aliases) = params.get("aliases").and_then(|v| v.as_array()) {
                        manifest.aliases = aliases
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                    if let Some(triggers) = params.get("triggers").and_then(|v| v.as_array()) {
                        manifest.triggers = triggers
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                    if let Some(posture) = params.get("posture").and_then(|v| v.as_str()) {
                        manifest.posture = Some(posture.to_string());
                    }
                    if let Some(turns) = params.get("max_turns").and_then(|v| v.as_u64()) {
                        manifest.max_turns = Some(turns as u32);
                    }
                    if let Some(body_update) = params.get("body").and_then(|v| v.as_str()) {
                        std::fs::write(&skill_file, manifest.to_skill_file(body_update))?;
                    } else {
                        std::fs::write(&skill_file, manifest.to_skill_file(&body))?;
                    }
                }

                Ok(serde_json::json!({
                    "ok": true,
                    "path": skill_file.display().to_string(),
                }))
            }

            "skills/delete" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                if name.contains('/')
                    || name.contains('\\')
                    || name.contains("..")
                    || name.contains('\0')
                {
                    anyhow::bail!("invalid skill name: path traversal rejected");
                }

                // Check project-local first, then user-installed
                let cwd = std::env::current_dir()?;
                let project_dir = cwd.join(".omegon/skills").join(name);
                let home = crate::paths::omegon_home()?;
                let user_dir = home.join("skills").join(name);

                if project_dir.exists() {
                    std::fs::remove_dir_all(&project_dir)?;
                    Ok(serde_json::json!({ "ok": true, "scope": "project" }))
                } else if user_dir.exists() {
                    std::fs::remove_dir_all(&user_dir)?;
                    Ok(serde_json::json!({ "ok": true, "scope": "user" }))
                } else {
                    anyhow::bail!("skill '{name}' not found")
                }
            }

            // ── Prompt definitions ───────────────────────────────
            "prompts/list" => {
                let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                Ok(
                    serde_json::json!({ "prompts": crate::prompts::list_structured_for_project(&cwd)? }),
                )
            }
            "prompts/get" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                let (manifest, body, path) = crate::prompts::get_prompt_for_project(&cwd, name)?;
                Ok(serde_json::json!({
                    "name": name,
                    "id": manifest.id,
                    "title": manifest.title,
                    "description": manifest.description,
                    "tags": manifest.tags,
                    "aliases": manifest.aliases,
                    "safety": crate::prompts::safety_verdict(&body),
                    "body": body,
                    "path": path.display().to_string(),
                }))
            }
            "prompts/create" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'content' field"))?;
                let project_local = params
                    .get("project_local")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                let path = crate::prompts::write_prompt_for_project(
                    &cwd,
                    name,
                    content,
                    project_local,
                    false,
                )?;
                Ok(serde_json::json!({ "ok": true, "path": path.display().to_string() }))
            }
            "prompts/update" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let content = params["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'content' field"))?;
                let project_local = params
                    .get("project_local")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                let path = crate::prompts::write_prompt_for_project(
                    &cwd,
                    name,
                    content,
                    project_local,
                    true,
                )?;
                Ok(serde_json::json!({ "ok": true, "path": path.display().to_string() }))
            }
            "prompts/delete" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                let scope = crate::prompts::delete_prompt_for_project(&cwd, name)?;
                Ok(serde_json::json!({ "ok": true, "scope": scope }))
            }
            "prompts/preview" | "prompts/resolve" | "prompts/submit" => {
                let name = params["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
                let cwd = self.session_cwd.borrow().clone().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                });
                let (_manifest, body, path) = crate::prompts::get_prompt_for_project(&cwd, name)?;
                let deprecated = method.ends_with("/submit");
                Ok(serde_json::json!({
                    "ok": true,
                    "action": "preview",
                    "deprecated": deprecated,
                    "replacement": if deprecated { Some("_prompts/preview") } else { None },
                    "execution_performed": false,
                    "safety": crate::prompts::safety_verdict(&body),
                    "prompt": body,
                    "path": path.display().to_string(),
                    "note": if deprecated {
                        "Deprecated compatibility alias for preview; no submit, queue, or execution was performed."
                    } else {
                        "Prompt resolved for preview; direct ACP turn enqueue requires a stronger confirmation/trust flow."
                    }
                }))
            }

            // ── Control requests (TUI parity) ────────────────────
            // Route through the worker thread which has access to
            // conversation state, settings, and secrets.
            "control/stats"
            | "control/max_turns"
            | "control/persona_list"
            | "control/persona_switch"
            | "control/profile_view"
            | "control/profile_capture"
            | "control/profile_apply"
            | "control/profile_mqtt"
            | "control/profile_extension_allow"
            | "control/profile_extension_deny"
            | "control/profile_extension_clear"
            | "control/profile_persona"
            | "control/profile_tone"
            | "control/context_status"
            | "control/context_class"
            | "control/runtime_mode"
            | "control/secrets_view"
            | "control/vault_status"
            | "control/auth_status"
            | "control/note_add"
            | "control/notes_view"
            | "control/notes_clear"
            | "control/workspace_status"
            | "control/workspace_list"
            | "control/workspace_new"
            | "control/workspace_destroy"
            | "control/workspace_adopt"
            | "control/workspace_release"
            | "control/workspace_archive"
            | "control/workspace_prune"
            | "control/workspace_bind_milestone"
            | "control/workspace_bind_node"
            | "control/workspace_bind_clear"
            | "control/workspace_role"
            | "control/workspace_role_set"
            | "control/workspace_role_clear"
            | "control/workspace_kind"
            | "control/workspace_kind_set"
            | "control/workspace_kind_clear"
            | "control/tree_view"
            | "control/provider_status" => {
                let control_cmd = method.strip_prefix("control/").unwrap_or(method);
                let arg = control_request_args(method, &params);
                let full_cmd = if arg.is_empty() {
                    control_cmd.to_string()
                } else {
                    format!("{control_cmd} {}", arg.trim())
                };

                let (tx, rx) = tokio::sync::oneshot::channel();
                let worker_tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
                if let Some(wtx) = worker_tx {
                    let _ = wtx
                        .send(WorkerRequest::ControlRequest {
                            command: full_cmd,
                            response_tx: tx,
                        })
                        .await;
                    let timeout_secs = if control_cmd.starts_with("workspace_") {
                        60
                    } else {
                        5
                    };
                    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx)
                        .await
                    {
                        Ok(Ok(resp)) => Ok(serde_json::json!({
                            "text": resp.text,
                            "error": resp.error,
                        })),
                        Ok(Err(_)) => anyhow::bail!("worker dropped response"),
                        Err(_) => anyhow::bail!("control request timed out"),
                    }
                } else {
                    anyhow::bail!("agent not initialized")
                }
            }

            _ => anyhow::bail!("unknown extension method: {method}"),
        }
    }

    fn request_worker_control(&self, command: &str) -> String {
        let worker_tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
        let Some(worker_tx) = worker_tx else {
            return "ACP worker is not initialized".into();
        };
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let runtime = tokio::runtime::Handle::current();
        if runtime
            .block_on(worker_tx.send(WorkerRequest::ControlRequest {
                command: command.to_string(),
                response_tx,
            }))
            .is_err()
        {
            return "ACP worker is not accepting requests".into();
        }
        match runtime.block_on(tokio::time::timeout(
            std::time::Duration::from_secs(5),
            response_rx,
        )) {
            Ok(Ok(response)) => response.text,
            Ok(Err(_)) => "ACP worker dropped control response".into(),
            Err(_) => "ACP control request timed out".into(),
        }
    }

    fn handle_slash_command(&self, input: &str) -> String {
        let trimmed = input.trim();
        let (cmd, args) = trimmed
            .split_once(char::is_whitespace)
            .unwrap_or((trimmed, ""));

        match cmd {
            "/model" if !args.is_empty() => {
                let rt = tokio::runtime::Handle::current();
                let tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
                if let Some(tx) = tx {
                    let _ = rt.block_on(tx.send(WorkerRequest::SetModel {
                        value: args.trim().to_string(),
                        ack: None,
                    }));
                }
                format!("Model set to: {}", args.trim())
            }
            "/model" => "Current model from CLI args. Use the model dropdown or /model <provider:model> to switch.".into(),
            "/thinking" | "/think" if !args.is_empty() => {
                let tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
                if let Some(tx) = tx {
                    let _ = tokio::runtime::Handle::current().block_on(tx.send(
                        WorkerRequest::SetThinking {
                            value: args.trim().to_string(),
                            ack: None,
                        },
                    ));
                }
                format!("Thinking set to: {}", args.trim())
            }
            "/thinking" | "/think" => "Use the thinking dropdown or /think <off|minimal|low|medium|high>".into(),
            "/posture" if !args.is_empty() => {
                let tx = self.worker.borrow().as_ref().map(|w| w.request_tx.clone());
                if let Some(tx) = tx {
                    let _ = tokio::runtime::Handle::current().block_on(tx.send(
                        WorkerRequest::SetPosture {
                            value: args.trim().to_string(),
                            ack: None,
                        },
                    ));
                }
                format!("Posture set to: {}", args.trim())
            }
            "/posture" => "Use the posture dropdown or /posture <fabricator|architect|explorator|devastator>".into(),
            "/compact" => "Context compaction happens automatically. The model manages its own context window.".into(),
            "/clear" => "Start a new thread via the + button to clear the conversation.".into(),
            "/status" => self.request_worker_control("status"),
            "/version" => format!("omegon {}", env!("CARGO_PKG_VERSION")),
            "/secrets" => {
                // Read recipes file for a diagnostic view — no values exposed
                let secrets_path = dirs::home_dir()
                    .unwrap_or_default()
                    .join(".omegon/secrets.json");
                let mut lines = vec!["**Configured secrets:**".to_string()];
                let mut found = false;
                if let Ok(data) = std::fs::read_to_string(&secrets_path)
                    && let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&data) {
                        for (name, source) in &map {
                            let src = source.as_str().unwrap_or("unknown");
                            let kind = if src.starts_with("keyring:") {
                                "keyring"
                            } else if src.starts_with("env:") {
                                "env"
                            } else if src.starts_with("vault:") {
                                "vault"
                            } else {
                                "recipe"
                            };
                            let available = std::env::var(name).is_ok();
                            let status = if available { "active" } else { "configured" };
                            lines.push(format!("- `{name}` — {kind} ({status})"));
                            found = true;
                        }
                    }
                // Check env-only keys not in recipes
                for key in &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY"] {
                    if std::env::var(key).is_ok() && !lines.iter().any(|l| l.contains(key)) {
                        lines.push(format!("- `{key}` — env (active)"));
                        found = true;
                    }
                }
                if !found {
                    lines.push("No secrets configured.".into());
                }
                lines.push(String::new());
                lines.push("To add or change secrets, run `omegon secrets configure` in a terminal.".into());
                lines.join("\n")
            }
            "/login" | "/auth" => "Omegon manages authentication independently.\nRun `omegon auth login` in a terminal or set API keys.".into(),
            "/skills" | "/skill" => {
                let args = args.trim();
                match args {
                    "" | "list" => "Use the **skills/list** RPC to get structured skill data, or type `/skills list` to see a summary.\nAvailable: list, reload, refresh, install [name|skills/name], create|new [--project|--user], import [--project|--user] <path>, get <name>, delete <name>".into(),
                    "reload" | "refresh" => "Reload user/project skills into the current TUI session with `/skills reload` (alias: `/skills refresh`). ACP sessions should start a new session until structured reload RPC support lands.".into(),
                    "install" => "Use the **skills/install** RPC to install bundled skills, or pass a skill name to install through Armory. Run `/skills reload` afterward to activate user/project changes in the current TUI session.".into(),
                    _ => format!("Skills subcommand: {args}. Available slash affordances: list, reload, refresh, install [name], create|new [--project|--user], import [--project|--user] <path>, get <name>, delete <name>."),
                }
            }
            "/extension" | "/ext" => {
                let args = args.trim();
                match args {
                    "" | "list" => "Use the **extensions/list** RPC for structured extension data.\nAvailable: list, get <name>, install <name|url|path>, remove <name>, update [name], enable <name>, disable <name>, search [query]".into(),
                    _ => format!("Extension subcommand: {args}. Use the **extensions/{args}** RPC for structured operations."),
                }
            }
            "/persona" => {
                let args = args.trim();
                match args {
                    "" | "list" => "Use the **personas/list** RPC for structured persona data.\nAvailable: list, get <id>, create, update <id>, delete <id>".into(),
                    _ => format!("Persona subcommand: {args}. Use the **personas/{args}** RPC for structured operations."),
                }
            }
            "/catalog" => {
                let args = args.trim();
                match args {
                    "" | "list" => "Use the **catalog/list** RPC for structured catalog data.\nAvailable: list, get <id>, install, remove <id>".into(),
                    _ => format!("Catalog subcommand: {args}. Use the **catalog/{args}** RPC for structured operations."),
                }
            }
            "/armory" => "Use the **armory/browse** RPC for structured upstream discovery across extensions, plugins, skills, and agents, and **armory/install** to install a registry item.\nBrowse parameters: kind, query. Install parameter: target.".into(),
            "/help" => {
                let commands = acp_available_commands()
                    .into_iter()
                    .map(|command| format!("/{}", command.name))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("Commands: {commands}\n\nFull CRUD is available via RPC ext_methods (armory/*, skills/*, extensions/*, personas/*, catalog/*, secrets/*).")
            }
            _ => format!("Unknown: {cmd}. Type /help"),
        }
    }
}

fn control_request_args(method: &str, params: &serde_json::Value) -> String {
    if let Some(args) = params.get("args").and_then(|v| v.as_str()) {
        return args.to_string();
    }

    let key = match method {
        "control/workspace_new" => Some("label"),
        "control/workspace_destroy" => Some("target"),
        "control/workspace_bind_milestone" => Some("milestone_id"),
        "control/workspace_bind_node" => Some("design_node_id"),
        "control/workspace_role_set" => Some("role"),
        "control/workspace_kind_set" => Some("kind"),
        "control/note_add" => Some("text"),
        "control/persona_switch" => Some("name"),
        "control/profile_extension_allow" | "control/profile_extension_deny" => Some("name"),
        "control/profile_persona" | "control/profile_tone" => Some("name"),
        _ => None,
    };

    key.and_then(|key| params.get(key).and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

// ── ACP → internal MCP server conversion ──────────────────────────────

fn mcp_config(
    command: Option<String>,
    url: Option<String>,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
) -> crate::plugins::mcp::McpServerConfig {
    crate::plugins::mcp::McpServerConfig {
        url,
        command,
        args,
        env,
        image: None,
        mount_cwd: false,
        network: true,
        docker_mcp: None,
        styrene_dest: None,
        timeout_secs: 30,
        host_actions: crate::plugins::mcp::McpHostActionPolicy::default(),
    }
}

fn parse_armory_kind(kind: &str) -> anyhow::Result<crate::armory::ArmoryKind> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "" | "all" => Ok(crate::armory::ArmoryKind::All),
        "extension" | "extensions" => Ok(crate::armory::ArmoryKind::Extensions),
        "plugin" | "plugins" => Ok(crate::armory::ArmoryKind::Plugins),
        "skill" | "skills" => Ok(crate::armory::ArmoryKind::Skills),
        "agent" | "agents" | "catalog" => Ok(crate::armory::ArmoryKind::Agents),
        other => anyhow::bail!(
            "invalid armory kind '{other}' (expected all, extensions, plugins, skills, or agents)"
        ),
    }
}

fn armory_item_kind_name(kind: crate::armory::ArmoryItemKind) -> &'static str {
    match kind {
        crate::armory::ArmoryItemKind::Extension => "extensions",
        crate::armory::ArmoryItemKind::Plugin => "plugins",
        crate::armory::ArmoryItemKind::Skill => "skills",
        crate::armory::ArmoryItemKind::Agent => "agents",
    }
}

fn armory_search_item_json(item: crate::armory::ArmoryItem) -> serde_json::Value {
    serde_json::json!({
        "id": item.id,
        "kind": armory_item_kind_name(item.kind),
        "name": item.name,
        "version": item.version,
        "description": item.description,
        "source": item.source,
        "tags": [item.category],
        "category": item.category,
        "manifest_id": item.manifest_id,
        "installed": item.installed,
        "install_hint": item.install_hint,
    })
}

fn armory_install_result_json(result: &crate::armory::ArmoryInstallResult) -> serde_json::Value {
    serde_json::json!({
        "id": result.id,
        "kind": armory_item_kind_name(result.kind),
        "path": result.path,
        "message": result.message,
    })
}

fn convert_acp_mcp_server(
    server: McpServer,
) -> Option<(String, crate::plugins::mcp::McpServerConfig)> {
    match server {
        McpServer::Stdio(s) => {
            let env: std::collections::HashMap<String, String> =
                s.env.into_iter().map(|e| (e.name, e.value)).collect();
            Some((
                s.name,
                mcp_config(
                    Some(s.command.to_string_lossy().to_string()),
                    None,
                    s.args,
                    env,
                ),
            ))
        }
        McpServer::Http(s) => Some((
            s.name,
            mcp_config(
                None,
                Some(s.url),
                Vec::new(),
                std::collections::HashMap::new(),
            ),
        )),
        McpServer::Sse(s) => Some((
            s.name,
            mcp_config(
                None,
                Some(s.url),
                Vec::new(),
                std::collections::HashMap::new(),
            ),
        )),
        _ => None,
    }
}

// ── Entry point ────────────────────────────────────────────────────────

#[cfg(test)]
mod extension_metadata_tests {
    use super::*;
    use agent_client_protocol::schema::{InitializeRequest, ProtocolVersion};

    #[tokio::test]
    async fn initialize_includes_extension_metadata() {
        let metadata = std::collections::BTreeMap::from([(
            "flynt".to_string(),
            serde_json::json!({"deployment": {"kind": "local", "path": "/tmp/flynt"}}),
        )]);
        let agent = OmegonAcpAgent::new_with_extension_metadata("test-model", metadata);
        let response = agent
            .initialize(InitializeRequest::new(ProtocolVersion::V1))
            .await
            .unwrap();

        let meta = response.meta.expect("metadata should be populated");
        assert_eq!(
            meta["omegon/extensions"]["flynt"]["deployment"]["kind"],
            "local"
        );
        assert_eq!(meta["flynt"]["deployment"]["kind"], "local");
    }

    #[tokio::test]
    async fn initialize_enables_conversation_surface_for_flynt_client() {
        let agent = OmegonAcpAgent::new("test-model");
        let response = agent
            .initialize(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_info(Implementation::new("flynt", "0.1.0").title("Flynt")),
            )
            .await
            .unwrap();

        let meta = response
            .meta
            .expect("surface metadata should be advertised");
        assert_eq!(meta["omegon/surfaces"]["conversation"]["enabled"], true);
        assert_eq!(
            meta["omegon/surfaces"]["conversation"]["extensionMethod"],
            ACP_CONVERSATION_SURFACE_METHOD
        );
        assert!(*agent.surface_updates_enabled.borrow());
    }

    #[tokio::test]
    async fn initialize_keeps_conversation_surface_disabled_for_zed_client() {
        let agent = OmegonAcpAgent::new("test-model");
        let response = agent
            .initialize(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_info(Implementation::new("zed", "0.1.0").title("Zed")),
            )
            .await
            .unwrap();

        let meta = response
            .meta
            .expect("surface metadata should be advertised");
        assert_eq!(meta["omegon/surfaces"]["conversation"]["enabled"], false);
        assert!(!*agent.surface_updates_enabled.borrow());
    }

    #[test]
    fn acp_available_commands_derive_from_shared_registry() {
        let names: Vec<String> = acp_available_commands()
            .into_iter()
            .map(|command| command.name)
            .collect();

        assert!(names.contains(&"model".to_string()), "{names:?}");
        assert!(names.contains(&"thinking".to_string()), "{names:?}");
        assert!(names.contains(&"login".to_string()), "{names:?}");
        assert!(names.contains(&"posture".to_string()), "{names:?}");
        assert!(!names.contains(&"think".to_string()), "{names:?}");
        assert!(!names.contains(&"auth".to_string()), "{names:?}");

        let definitions = crate::command_registry::builtin_command_definitions();
        for (advertised, registry_name) in [("thinking", "think"), ("login", "auth")] {
            let definition = definitions
                .iter()
                .find(|definition| definition.name == registry_name)
                .expect("ACP aliased command should come from shared registry");
            assert!(definition.availability.acp, "{definition:?}");
            assert!(names.contains(&advertised.to_string()), "{names:?}");
        }
    }

    #[test]
    fn acp_help_preserves_advertised_command_names() {
        let agent = OmegonAcpAgent::new("test-model");
        let text = agent.handle_slash_command("/help");

        let command_line = text.lines().next().expect("help should include commands");
        let commands: Vec<&str> = command_line
            .strip_prefix("Commands: ")
            .expect("help should start with command list")
            .split_whitespace()
            .collect();

        assert!(commands.contains(&"/thinking"), "{text}");
        assert!(commands.contains(&"/login"), "{text}");
        assert!(commands.contains(&"/posture"), "{text}");
        assert!(!commands.contains(&"/think"), "{text}");
        assert!(!commands.contains(&"/auth"), "{text}");
    }

    #[test]
    fn extension_metadata_meta_omits_flynt_alias_when_absent() {
        let metadata = std::collections::BTreeMap::from([(
            "other".to_string(),
            serde_json::json!({"deployment": {"kind": "local"}}),
        )]);
        let meta = extension_metadata_meta(&metadata);

        assert_eq!(
            meta["omegon/extensions"]["other"]["deployment"]["kind"],
            "local"
        );
        assert!(meta.get("flynt").is_none());
    }

    #[test]
    fn armory_search_item_json_matches_flynt_contract() {
        let item = crate::armory::ArmoryItem {
            kind: crate::armory::ArmoryItemKind::Extension,
            id: "recro/recro-omegon".to_string(),
            name: "recro-omegon".to_string(),
            description: "Recro integration".to_string(),
            category: "integrations".to_string(),
            version: Some("1.2.3".to_string()),
            source: "https://github.com/recro/recro-omegon".to_string(),
            manifest_id: Some("recro".to_string()),
            installed: false,
            install_hint: "armory install recro/recro-omegon".to_string(),
        };

        let json = armory_search_item_json(item);

        assert_eq!(json["id"], "recro/recro-omegon");
        assert_eq!(json["kind"], "extensions");
        assert_eq!(json["name"], "recro-omegon");
        assert_eq!(json["version"], "1.2.3");
        assert_eq!(json["description"], "Recro integration");
        assert_eq!(json["source"], "https://github.com/recro/recro-omegon");
        assert_eq!(json["tags"][0], "integrations");
    }

    #[test]
    fn armory_install_result_json_matches_flynt_contract() {
        let result = crate::armory::ArmoryInstallResult {
            kind: crate::armory::ArmoryItemKind::Extension,
            id: "recro/recro-omegon".to_string(),
            path: Some("/tmp/extensions/recro-omegon".to_string()),
            message: "Installed extension".to_string(),
        };

        let json = armory_install_result_json(&result);

        assert_eq!(json["id"], "recro/recro-omegon");
        assert_eq!(json["kind"], "extensions");
        assert_eq!(json["path"], "/tmp/extensions/recro-omegon");
        assert_eq!(json["message"], "Installed extension");
    }

    #[tokio::test]
    async fn runtime_status_reports_session_and_agent_snapshot() {
        let agent = Rc::new(OmegonAcpAgent::new("anthropic:claude-opus-4-6"));
        let response = handle_acp_request_result(agent, "_runtime/status", &serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(response["runtime"]["name"], "omegon");
        assert_eq!(response["runtime"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(response["acp"]["protocol_version"], 1);
        assert_eq!(response["acp"]["transport"], "stdio");
        assert_eq!(response["acp"]["connected"], false);
        assert_eq!(response["agent"]["model"], "anthropic:claude-opus-4-6");
        assert_eq!(response["agent"]["thinking"], "minimal");
        assert_eq!(response["agent"]["posture"], "fabricator");
        assert_eq!(response["memory"]["scope"], "project");
    }

    #[tokio::test]
    async fn provider_status_reports_active_model_without_prompting() {
        let agent = Rc::new(OmegonAcpAgent::new("anthropic:claude-opus-4-6"));
        let response = handle_acp_request_result(agent, "_provider/status", &serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(response["active"]["provider"], "anthropic");
        assert_eq!(response["active"]["model"], "anthropic:claude-opus-4-6");
        assert!(response["active"]["ready"].is_boolean());
        assert!(
            response["providers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|provider| {
                    provider["id"] == "anthropic" && provider["models"].as_array().is_some()
                })
        );
    }

    #[tokio::test]
    async fn runtime_capabilities_advertise_secret_surfaces() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response =
            handle_acp_request_result(agent, "_runtime/capabilities", &serde_json::json!({}))
                .await
                .unwrap();

        assert_eq!(response["surfaces"]["_runtime/status"]["version"], 1);
        assert_eq!(response["surfaces"]["_provider/status"]["version"], 1);
        assert_eq!(response["surfaces"]["_provider/retry"]["version"], 1);
        assert_eq!(response["surfaces"]["_provider/failure"]["version"], 1);
        assert_eq!(response["surfaces"]["_turn/cancelled"]["version"], 1);
        assert_eq!(response["surfaces"]["_lifecycle/snapshot"]["version"], 1);
        assert_eq!(response["surfaces"]["_lifecycle/design/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_lifecycle/design/get"]["version"], 1);
        assert_eq!(
            response["surfaces"]["_lifecycle/design/ready"]["version"],
            1
        );
        assert_eq!(
            response["surfaces"]["_lifecycle/design/blocked"]["version"],
            1
        );
        assert_eq!(
            response["surfaces"]["_lifecycle/design/frontier"]["version"],
            1
        );
        assert_eq!(response["surfaces"].get("_ui/dashboard/snapshot"), None);
        assert_eq!(response["surfaces"]["_extensions/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_extensions/call"]["version"], 1);
        assert_eq!(response["surfaces"]["_secrets/capabilities"]["version"], 1);
        assert_eq!(response["surfaces"]["_secrets/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_plans/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_plans/show"]["version"], 1);
        assert_eq!(response["surfaces"]["_plans/events"]["version"], 1);
        assert_eq!(response["surfaces"]["_plans/switch"]["version"], 1);
        assert_eq!(response["surfaces"]["_plans/detach"]["version"], 1);
        assert_eq!(response["surfaces"]["_tasks/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_tasks/show"]["version"], 1);
        assert_eq!(response["surfaces"]["_tasks/bind"]["version"], 1);
        assert_eq!(response["surfaces"]["_tasks/events"]["version"], 1);
        assert_eq!(response["surfaces"]["_skills/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_skills/get"]["version"], 1);
        assert_eq!(response["surfaces"]["_skills/create"]["version"], 1);
        assert_eq!(response["surfaces"]["_skills/update"]["version"], 1);
        assert_eq!(response["surfaces"]["_skills/delete"]["version"], 1);
        assert_eq!(response["surfaces"]["_skills/install"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/list"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/get"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/create"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/update"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/delete"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/preview"]["version"], 1);
        assert_eq!(response["surfaces"]["_prompts/submit"]["version"], 1);
        assert_eq!(response["surfaces"]["_external_tasks/import"]["version"], 1);
        assert_eq!(response["features"]["extensions"], true);
        assert_eq!(response["features"]["secrets"], true);
        assert_eq!(response["features"]["lifecycle"], true);
        assert_eq!(response["features"].get("ui_surfaces"), None);
        assert_eq!(response["features"]["plans"], true);
        assert_eq!(response["features"]["plan_tasks"], true);
        assert_eq!(
            response["features"]["plan_tasks_contract"]["compatibility"][0],
            "read_only"
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["stable_id"],
            true
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["revision"],
            true
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["durable_bind"],
            true
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["durable_bind_scope"],
            "repo_backed_explicit_stable_id_only"
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["structured_errors"],
            true
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["filtering"],
            true
        );
        assert_eq!(
            response["features"]["plan_tasks_contract"]["pagination"],
            false
        );
    }

    #[tokio::test]
    async fn acp_lifecycle_design_queries_return_headless_projection() {
        let home = tempfile::tempdir().unwrap();
        let docs = home.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("ready-node.md"),
            "---\nid: ready-node\ntitle: Ready Node\nstatus: decided\ndependencies: []\nopen_questions: []\n---\n\n## Overview\nReady.\n",
        )
        .unwrap();
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        *agent.session_cwd.borrow_mut() = Some(home.path().to_path_buf());

        let ready = handle_acp_request_result(
            agent.clone(),
            "_lifecycle/design/ready",
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(ready["nodes"][0]["id"], "ready-node");

        let list = handle_acp_request_result(
            agent.clone(),
            "_lifecycle/design/list",
            &serde_json::json!({}),
        )
        .await
        .unwrap();
        assert_eq!(list["nodes"][0]["title"], "Ready Node");

        let node = handle_acp_request_result(
            agent,
            "_lifecycle/design/get",
            &serde_json::json!({ "node_id": "ready-node" }),
        )
        .await
        .unwrap();
        assert_eq!(node["status"], "decided");
        assert_eq!(node["overview"], "Ready.");
    }

    #[tokio::test]
    async fn acp_lifecycle_snapshot_returns_openspec_projection() {
        let home = tempfile::tempdir().unwrap();
        let change_dir = home.path().join("openspec/changes/demo");
        std::fs::create_dir_all(&change_dir).unwrap();
        std::fs::write(change_dir.join("proposal.md"), "# Demo\n").unwrap();
        std::fs::write(
            change_dir.join("tasks.md"),
            "## 1. Work\n\n- [ ] 1.1 Pending\n",
        )
        .unwrap();
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        *agent.session_cwd.borrow_mut() = Some(home.path().to_path_buf());

        let snapshot =
            handle_acp_request_result(agent, "_lifecycle/snapshot", &serde_json::json!({}))
                .await
                .unwrap();

        assert_eq!(snapshot["openspec"]["changes"][0]["name"], "demo");
        assert_eq!(snapshot["openspec"]["total_tasks"], 1);
    }

    #[tokio::test]
    async fn acp_skill_surfaces_dispatch_existing_handlers() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));

        let list = handle_acp_request_result(agent.clone(), "_skills/list", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(list["skills"].as_array().is_some());

        let get = handle_acp_request_result(
            agent.clone(),
            "_skills/get",
            &serde_json::json!({ "name": "rust" }),
        )
        .await
        .unwrap();
        assert_eq!(get["name"], "rust");
        assert!(get["body"].as_str().unwrap_or_default().contains("Rust"));

        let prompt = handle_acp_request_result(agent, "_prompts/list", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(prompt["prompts"].as_array().is_some());
    }

    #[tokio::test]
    async fn acp_prompt_surfaces_crud_project_local_definitions() {
        let home = tempfile::tempdir().unwrap();
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        *agent.session_cwd.borrow_mut() = Some(home.path().to_path_buf());
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe {
            std::env::set_var("OMEGON_HOME", home.path());
        }

        let create = handle_acp_request_result(
            agent.clone(),
            "_prompts/create",
            &serde_json::json!({
                "name": "daily-review",
                "project_local": true,
                "content": "+++\ntitle = \"Daily Review\"\ndescription = \"Summarize the day\"\n+++\n\nReview today's work."
            }),
        )
        .await
        .unwrap();
        assert_eq!(create["ok"], true);

        let get = handle_acp_request_result(
            agent.clone(),
            "_prompts/get",
            &serde_json::json!({ "name": "daily-review" }),
        )
        .await
        .unwrap();
        assert_eq!(get["title"], "Daily Review");
        assert_eq!(get["body"], "Review today's work.");

        let submit = handle_acp_request_result(
            agent.clone(),
            "_prompts/submit",
            &serde_json::json!({ "name": "daily-review" }),
        )
        .await
        .unwrap();
        assert_eq!(submit["ok"], true);
        assert_eq!(submit["action"], "preview");
        assert_eq!(submit["deprecated"], true);
        assert_eq!(submit["replacement"], "_prompts/preview");
        assert_eq!(submit["execution_performed"], false);
        assert_eq!(submit["prompt"], "Review today's work.");

        let preview = handle_acp_request_result(
            agent.clone(),
            "_prompts/preview",
            &serde_json::json!({ "name": "daily-review" }),
        )
        .await
        .unwrap();
        assert_eq!(preview["ok"], true);
        assert_eq!(preview["action"], "preview");
        assert_eq!(preview["prompt"], "Review today's work.");

        let delete = handle_acp_request_result(
            agent,
            "_prompts/delete",
            &serde_json::json!({ "name": "daily-review" }),
        )
        .await
        .unwrap();
        assert_eq!(delete["ok"], true);
        assert_eq!(delete["scope"], "project");

        unsafe {
            if let Some(value) = previous_home {
                std::env::set_var("OMEGON_HOME", value);
            } else {
                std::env::remove_var("OMEGON_HOME");
            }
        }
    }

    #[tokio::test]
    async fn acp_plan_and_task_surfaces_are_read_only_shapes() {
        let home = tempfile::tempdir().unwrap();
        let change_dir = home.path().join("openspec/changes/demo");
        std::fs::create_dir_all(&change_dir).unwrap();
        std::fs::write(
            change_dir.join("proposal.md"),
            "# Demo
",
        )
        .unwrap();
        std::fs::write(
            change_dir.join("tasks.md"),
            "## 1. Group

- [ ] 1.1 Pending <!-- task-id: stable-pending -->
",
        )
        .unwrap();
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        *agent.session_cwd.borrow_mut() = Some(home.path().to_path_buf());

        let plans = handle_acp_request_result(agent.clone(), "_plans/list", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(plans["plans"][0]["plan_id"], "openspec:demo");
        assert_eq!(plans["tasks"][0]["plan_id"], "openspec:demo");
        assert_eq!(plans["tasks"][0]["stable_id"], "stable-pending");
        assert_eq!(plans["tasks"][0]["source"]["kind"], "openspec");
        assert_eq!(
            plans["tasks"][0]["source"]["path"],
            "openspec/changes/demo/tasks.md"
        );
        assert!(
            plans["tasks"][0]["revision"]
                .as_str()
                .unwrap()
                .starts_with("source-v1:openspec:demo:1.1:")
        );
        assert_eq!(
            plans["tasks"][0]["supported_mutations"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let shown = handle_acp_request_result(
            agent.clone(),
            "_plans/show",
            &serde_json::json!({ "plan_id": "openspec:demo" }),
        )
        .await
        .unwrap();
        assert_eq!(shown["plan"]["plan_id"], "openspec:demo");
        assert_eq!(shown["tasks"].as_array().unwrap().len(), 1);

        let task_id = plans["tasks"][0]["id"].as_str().unwrap().to_string();
        let task = handle_acp_request_result(
            agent.clone(),
            "_tasks/show",
            &serde_json::json!({ "task_id": task_id }),
        )
        .await
        .unwrap();
        assert_eq!(task["task"]["plan_id"], "openspec:demo");
        assert_eq!(task["task"]["stable_id"], "stable-pending");
        assert_eq!(task["task"]["source"]["anchor"], "1.1");

        let bind = handle_acp_request_result(
            agent.clone(),
            "_tasks/bind",
            &serde_json::json!({
                "task_id": task_id,
                "system": "flynt",
                "external_task_id": "flynt-task-1",
                "expected_revision": plans["tasks"][0]["revision"].as_str().unwrap()
            }),
        )
        .await
        .unwrap();
        assert_eq!(bind["accepted"], true);
        assert_eq!(bind["durability"], "session");
        assert_eq!(bind["binding"]["system"], "flynt");
        assert_eq!(bind["binding"]["external_task_id"], "flynt-task-1");
        assert_eq!(bind["binding"]["stable_id"], "stable-pending");
        assert!(
            bind["warning"]
                .as_str()
                .unwrap()
                .contains("not repo-durable")
        );

        let stale = handle_acp_request_result(
            agent,
            "_tasks/bind",
            &serde_json::json!({
                "task_id": "stable-pending",
                "system": "flynt",
                "external_task_id": "flynt-task-1",
                "expected_revision": "sha256:stale"
            }),
        )
        .await
        .unwrap();
        assert_eq!(stale["accepted"], false);
        assert_eq!(stale["durability"], "none");
        assert_eq!(stale["code"], "stale_revision");
    }

    #[tokio::test]
    async fn acp_plan_projection_reports_task_identity_findings() {
        let home = tempfile::tempdir().unwrap();
        let change_dir = home.path().join("openspec/changes/invalid");
        std::fs::create_dir_all(&change_dir).unwrap();
        std::fs::write(
            change_dir.join("proposal.md"),
            "# Invalid
",
        )
        .unwrap();
        std::fs::write(
            change_dir.join("tasks.md"),
            "## 1. Group

- [ ] 1.1 First <!-- task-id: duplicate -->
- [ ] 1.2 Second <!-- task-id: duplicate -->
",
        )
        .unwrap();

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        *agent.session_cwd.borrow_mut() = Some(home.path().to_path_buf());
        let response =
            handle_acp_request_result(agent.clone(), "_plans/list", &serde_json::json!({}))
                .await
                .unwrap();
        let findings = response["task_identity_findings"].as_array().unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["stable_id"], "duplicate");
        assert!(
            findings[0]["message"]
                .as_str()
                .unwrap()
                .contains("duplicate")
        );

        let tasks = handle_acp_request_result(agent, "_tasks/list", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(tasks["task_identity_findings"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn acp_external_task_import_accepts_session_target() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent.clone(),
            "_external_tasks/import",
            &serde_json::json!({
                "system": "flynt",
                "external_task_id": "flynt task/1",
                "title": "Promote this task",
                "body": "Original Flynt body",
                "target": { "kind": "session" }
            }),
        )
        .await
        .unwrap();
        assert_eq!(response["accepted"], true);
        assert_eq!(response["durability"], "session");
        assert_eq!(response["created"]["source"]["kind"], "session");
        assert_eq!(response["binding"]["system"], "flynt");
        assert_eq!(response["review"]["required"], true);

        let events = handle_acp_request_result(agent, "_tasks/events", &serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(events["events"].as_array().unwrap().len(), 1);
        assert_eq!(events["events"][0]["system"], "flynt");
    }

    #[tokio::test]
    async fn acp_external_task_import_rejects_non_session_target_for_now() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_external_tasks/import",
            &serde_json::json!({
                "system": "flynt",
                "external_task_id": "flynt-task-1",
                "title": "Promote this task",
                "target": { "kind": "openspec" }
            }),
        )
        .await
        .unwrap();
        assert_eq!(response["accepted"], false);
        assert_eq!(response["code"], "unsupported_source");
    }

    #[tokio::test]
    async fn extensions_list_reports_installed_not_callable_extension() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let ext_dir = home.path().join("extensions").join("dummy-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            r#"[extension]
name = "dummy-ext"
version = "0.1.0"
description = "Dummy extension"

[runtime]
type = "native"
binary = "bin/dummy"

[capabilities]
voice = true

[secrets]
required = ["DUMMY_TOKEN"]
optional = ["DUMMY_OPTIONAL"]
"#,
        )
        .unwrap();

        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(agent, "_extensions/list", &serde_json::json!({}))
            .await
            .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let extensions = response["extensions"].as_array().unwrap();
        let ext = extensions
            .iter()
            .find(|entry| entry["id"] == "dummy-ext")
            .expect("dummy extension listed");
        assert_eq!(ext["name"], "dummy-ext");
        assert_eq!(ext["version"], "0.1.0");
        assert_eq!(ext["enabled"], true);
        assert_eq!(ext["loaded"], false);
        assert_eq!(ext["callable"], false);
        assert_eq!(ext["capabilities"]["voice"], true);
        assert_eq!(ext["last_error"], serde_json::Value::Null);
        assert_eq!(ext["stability"]["crashes_this_session"], 0);
        assert_eq!(ext["stability"]["health_check_failures"], 0);
        assert_eq!(ext["stability"]["last_error"], serde_json::Value::Null);
        assert_eq!(ext["stability"]["last_error_at"], serde_json::Value::Null);
        assert_eq!(ext["stability"]["auto_disabled"], false);
        assert_eq!(ext["secrets"]["required"][0]["name"], "DUMMY_TOKEN");
    }

    #[tokio::test]
    async fn extensions_enable_clears_auto_disabled_stability_state() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let ext_dir = home.path().join("extensions").join("flynt");
        std::fs::create_dir_all(ext_dir.join(".omegon")).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            r#"[extension]
name = "flynt"
version = "0.1.0"
description = "Flynt"

[runtime]
type = "native"
binary = "bin/flynt"
"#,
        )
        .unwrap();
        std::fs::write(
            ext_dir.join(".omegon").join("state.toml"),
            r#"enabled = false

[stability]
crashes_this_session = 4
health_check_failures = 2
last_error = "transport failure: Broken pipe (os error 32)"
last_error_at = "2026-06-06T01:46:31.022584+00:00"
auto_disabled = true
"#,
        )
        .unwrap();

        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent.clone(),
            "_extensions/enable",
            &serde_json::json!({ "extension": "flynt" }),
        )
        .await
        .unwrap();
        assert_eq!(response["ok"], true);

        let list = handle_acp_request_result(agent, "_extensions/list", &serde_json::json!({}))
            .await
            .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let state = crate::extensions::ExtensionState::load(&ext_dir).unwrap();
        assert!(state.enabled);
        assert_eq!(state.stability.crashes_this_session, 0);
        assert_eq!(state.stability.health_check_failures, 0);
        assert_eq!(state.stability.last_error, None);
        assert_eq!(state.stability.last_error_at, None);
        assert!(!state.stability.auto_disabled);

        let ext = list["extensions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == "flynt")
            .expect("flynt listed");
        assert_eq!(ext["enabled"], true);
        assert_eq!(ext["stability"]["crashes_this_session"], 0);
        assert_eq!(ext["stability"]["health_check_failures"], 0);
        assert_eq!(ext["stability"]["last_error"], serde_json::Value::Null);
        assert_eq!(ext["stability"]["last_error_at"], serde_json::Value::Null);
        assert_eq!(ext["stability"]["auto_disabled"], false);
    }

    #[tokio::test]
    async fn extensions_call_missing_extension_returns_structured_error() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_extensions/call",
            &serde_json::json!({
                "extension": "missing-extension",
                "method": "ping",
                "params": {}
            }),
        )
        .await
        .unwrap();

        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .starts_with("extension_not_loaded:")
        );
    }

    #[tokio::test]
    async fn extensions_call_rejects_empty_method() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_extensions/call",
            &serde_json::json!({ "extension": "flynt", "method": "   " }),
        )
        .await
        .unwrap();

        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .starts_with("invalid_request:")
        );
    }

    #[tokio::test]
    async fn extensions_call_defaults_missing_params_to_object_before_lookup() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_extensions/call",
            &serde_json::json!({ "extension": "missing-extension", "method": "ping" }),
        )
        .await
        .unwrap();

        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .starts_with("extension_not_loaded:")
        );
    }

    #[tokio::test]
    async fn extensions_call_rejects_missing_method() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_extensions/call",
            &serde_json::json!({ "extension": "flynt" }),
        )
        .await
        .unwrap();

        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .starts_with("invalid_request:")
        );
    }

    #[tokio::test]
    async fn secrets_capabilities_describe_non_resolving_list_surface() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response =
            handle_acp_request_result(agent, "_secrets/capabilities", &serde_json::json!({}))
                .await
                .unwrap();

        assert_eq!(response["version"], 1);
        assert_eq!(response["operations"]["list"], true);
        assert_eq!(response["safety"]["values_write_only"], true);
        assert_eq!(response["safety"]["list_resolves_values"], false);
        assert_eq!(response["safety"]["list_executes_recipes"], false);
        assert!(
            response["recipe_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind == "vault")
        );
    }

    #[tokio::test]
    async fn secrets_list_returns_recipe_descriptors_without_values() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = std::sync::Arc::new(omegon_secrets::SecretsManager::new(dir.path()).unwrap());
        secrets
            .set_recipe("BRAVE_API_KEY", "env:OMEGON_TEST_BRAVE_KEY")
            .unwrap();

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        agent.set_secrets_for_test(secrets);
        let response = handle_acp_request_result(agent, "_secrets/list", &serde_json::json!({}))
            .await
            .unwrap();

        let items = response["items"].as_array().unwrap();
        let item = items
            .iter()
            .find(|item| item["name"] == "BRAVE_API_KEY")
            .expect("BRAVE_API_KEY descriptor");
        assert_eq!(item["recipe"], "env:OMEGON_TEST_BRAVE_KEY");
        assert_eq!(item["kind"], "env");
        assert_eq!(item["payload"], "OMEGON_TEST_BRAVE_KEY");
        assert!(!response.to_string().contains("brave-test-key"));
    }

    #[tokio::test]
    async fn assistant_runs_list_reports_empty_runtime_projection() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response =
            handle_acp_request_result(agent, "_assistant_runs/list", &serde_json::json!({}))
                .await
                .unwrap();

        assert!(response["runs"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn assistant_runs_show_reports_missing_runtime_run() {
        let home = tempfile::tempdir().unwrap();
        let _cwd = crate::test_support::cwd::CurrentDirGuard::enter_async(home.path()).await;
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_assistant_runs/show",
            &serde_json::json!({ "run_id": "missing" }),
        )
        .await
        .unwrap();

        assert_eq!(response["error"], "assistant_run_not_found");
    }

    #[tokio::test]
    async fn capabilities_inventory_reports_blocked_assistant_launch_readiness() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let agent_dir = home.path().join("catalog").join("blocked-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("agent.toml"),
            r#"[agent]
id = "blocked-agent"
name = "Blocked Agent"
version = "0.1.0"
description = "Requires a missing secret"
domain = "security"

[secrets]
required = ["MISSING_REQUIRED_TOKEN"]
"#,
        )
        .unwrap();
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response =
            handle_acp_request_result(agent, "_capabilities/inventory", &serde_json::json!({}))
                .await
                .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let profile = response["assistant_profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|profile| profile["id"] == "blocked-agent")
            .expect("blocked assistant profile");
        assert_eq!(profile["launch_readiness"]["status"], "blocked");
        assert!(
            profile["launch_readiness"]["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|blocker| blocker["kind"] == "required_secret_missing"
                    && blocker["id"] == "MISSING_REQUIRED_TOKEN")
        );
    }

    #[tokio::test]
    async fn capability_assistant_readiness_reports_single_assistant() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let agent_dir = home.path().join("catalog").join("blocked-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("agent.toml"),
            r#"[agent]
id = "blocked-agent"
name = "Blocked Agent"
version = "0.1.0"
description = "Requires a missing secret"
domain = "security"

[secrets]
required = ["MISSING_REQUIRED_TOKEN"]
"#,
        )
        .unwrap();
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_capabilities/assistant_readiness",
            &serde_json::json!({ "id": "blocked-agent" }),
        )
        .await
        .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        assert_eq!(response["assistant"]["id"], "blocked-agent");
        assert_eq!(
            response["assistant"]["launch_readiness"]["status"],
            "blocked"
        );
    }

    #[tokio::test]
    async fn capability_assistant_readiness_reports_missing_assistant() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_capabilities/assistant_readiness",
            &serde_json::json!({ "id": "missing" }),
        )
        .await
        .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        assert_eq!(response["error"], "assistant_not_found");
    }

    #[tokio::test]
    async fn capability_assistants_reports_compact_blocked_readiness() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let agent_dir = home.path().join("catalog").join("blocked-agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(
            agent_dir.join("agent.toml"),
            r#"[agent]
id = "blocked-agent"
name = "Blocked Agent"
version = "0.1.0"
description = "Requires a missing secret"
domain = "security"

[secrets]
required = ["MISSING_REQUIRED_TOKEN"]
"#,
        )
        .unwrap();
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response =
            handle_acp_request_result(agent, "_capabilities/assistants", &serde_json::json!({}))
                .await
                .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let assistant = response["assistants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|assistant| assistant["id"] == "blocked-agent")
            .expect("blocked assistant list item");
        assert_eq!(assistant["launch_readiness"]["status"], "blocked");
        assert_eq!(assistant["required_secret_count"], 1);
        assert_eq!(assistant["blocker_count"], 1);
    }

    #[tokio::test]
    async fn capabilities_inventory_reports_secret_metadata_without_values() {
        let _guard = crate::GLOBAL_TEST_ENV_LOCK.lock().await;
        let home = tempfile::tempdir().unwrap();
        let ext_dir = home.path().join("extensions").join("secure-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.toml"),
            r#"[extension]
name = "secure-ext"
version = "0.1.0"
description = "Secure extension"

[runtime]
type = "native"
binary = "bin/secure-ext"

[secrets]
required = ["BRAVE_API_KEY"]
"#,
        )
        .unwrap();
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(home.path()).unwrap());
        secrets
            .set_recipe("BRAVE_API_KEY", "env:OMEGON_TEST_BRAVE_KEY")
            .unwrap();
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe { std::env::set_var("OMEGON_HOME", home.path()) };

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        agent.set_secrets_for_test(secrets);
        let response =
            handle_acp_request_result(agent, "_capabilities/inventory", &serde_json::json!({}))
                .await
                .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        let readiness = response["secret_readiness"]["secrets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|secret| secret["name"] == "BRAVE_API_KEY")
            .expect("BRAVE_API_KEY readiness");
        assert_eq!(readiness["status"], "missing");
        assert_eq!(readiness["recipe_kind"], "env");
        assert!(!response.to_string().contains("brave-test-key"));
        assert!(!response.to_string().contains("OMEGON_TEST_BRAVE_KEY"));
    }

    #[tokio::test]
    async fn underscore_extension_method_routes_to_ext_method() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_armory/install",
            &serde_json::json!({ "kind": "extensions" }),
        )
        .await
        .unwrap();

        assert_eq!(response["error"], "missing 'target' field");
    }

    #[tokio::test]
    async fn underscore_packages_plan_routes_to_ext_method() {
        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_packages/plan",
            &serde_json::json!({
                "source": "https://github.com/recro/recro-omegon",
                "kind_hint": "skill"
            }),
        )
        .await
        .unwrap();

        assert_eq!(response["ok"], true);
        assert_eq!(response["package"]["id"], "recro-omegon");
        assert_eq!(response["contributions"][0]["kind"], "skill");
    }

    #[tokio::test]
    async fn underscore_packages_install_installs_local_plugin_package() {
        let home = tempfile::tempdir().unwrap();
        let package = tempfile::tempdir().unwrap();
        std::fs::write(
            package.path().join("plugin.toml"),
            "[plugin]\nid = \"recro-omegon\"\nname = \"recro-omegon\"\ntype = \"skill\"\nversion = \"0.1.0\"\ndescription = \"Recro workflows\"\n",
        )
        .unwrap();
        let _env_guard = crate::test_support::env::lock_async().await;
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe {
            std::env::set_var("OMEGON_HOME", home.path());
        }

        let agent = Rc::new(OmegonAcpAgent::new("test-model"));
        let response = handle_acp_request_result(
            agent,
            "_packages/install",
            &serde_json::json!({
                "source": package.path().display().to_string(),
                "kind_hint": "plugin"
            }),
        )
        .await
        .unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        assert_eq!(response["ok"], true, "{response:#}");
        assert_eq!(response["package"]["id"], "recro-omegon", "{response:#}");
        assert_eq!(
            response["contributions"][0]["kind"], "skill",
            "{response:#}"
        );
        assert_eq!(
            response["contributions"][0]["status"], "installed",
            "{response:#}"
        );
        assert!(
            response["package"]["path"].as_str().is_some(),
            "{response:#}"
        );
    }
}

pub async fn run(
    model: &str,
    agent_id: Option<&str>,
    cwd: &std::path::Path,
    dangerously_bypass_permissions: bool,
) -> anyhow::Result<()> {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    let extension_metadata = if let Some(id) = agent_id {
        let shared_settings = crate::settings::shared(model);
        crate::apply_agent_manifest_pre_setup(id, cwd, &shared_settings)?;
        crate::setup::AgentSetup::new_with_safety(
            cwd,
            None,
            Some(shared_settings),
            dangerously_bypass_permissions,
        )
        .await?
        .extension_metadata
    } else {
        Default::default()
    };

    let agent = Rc::new(OmegonAcpAgent::new_with_extension_metadata_and_safety(
        model,
        extension_metadata,
        dangerously_bypass_permissions,
    ));

    let stdout = tokio::io::stdout().compat_write();
    let stdin = tokio::io::stdin().compat();

    let io_task = connect_acp_agent(agent.clone(), stdout, stdin, |fut| {
        tokio::task::spawn_local(fut);
    });

    io_task.await.context("ACP IO task ended")?;

    Ok(())
}

/// Run ACP over WebSocket — standalone network server mode.
///
/// Starts a minimal HTTP server with:
/// - `GET /acp` — WebSocket ACP endpoint (authenticated)
/// - `GET /api/healthz` — liveness probe
/// - `GET /api/readyz` — readiness probe
pub async fn run_server(
    addr: &str,
    model: &str,
    agent_id: Option<&str>,
    cwd: &std::path::Path,
    tls: Option<crate::control_tls::ControlTlsConfig>,
    dangerously_bypass_permissions: bool,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    let bind_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid listen address '{addr}': {e}"))?;

    let web_auth = Arc::new(crate::web::WebAuthState::ephemeral_generated(
        crate::web::generate_token(),
    ));
    let token = web_auth.issue_query_token();
    let shutdown = CancellationToken::new();

    let acp_state = crate::web::acp_ws::AcpWebState {
        web_auth: web_auth.clone(),
        web_authority: crate::web::WebAuthorityConfig::default(),
        model: model.to_string(),
        cwd: std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
        agent_id: agent_id.map(String::from),
        dangerously_bypass_permissions,
        active_connections: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        shutdown: shutdown.clone(),
    };

    // Health probe handler (inline — no WebState needed)
    async fn healthz() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({"ok": true, "state": "ready"}))
    }

    let app = axum::Router::new()
        .route(
            "/acp",
            axum::routing::get(crate::web::acp_ws::acp_ws_handler),
        )
        .route("/api/healthz", axum::routing::get(healthz))
        .route("/api/readyz", axum::routing::get(healthz))
        .with_state(acp_state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind {bind_addr}: {e}"))?;
    let bound = listener.local_addr()?;
    let (http_scheme, ws_scheme) = crate::control_tls::schemes(tls.as_ref());

    // Emit startup JSON for orchestrator discovery
    let startup = serde_json::json!({
        "type": "omegon.startup",
        "schema_version": 3,
        "pid": std::process::id(),
        "acp_url": format!("{ws_scheme}://{bound}/acp?token={token}"),
        "health_url": format!("{http_scheme}://{bound}/api/healthz"),
        "ready_url": format!("{http_scheme}://{bound}/api/readyz"),
        "auth_mode": web_auth.mode_name(),
        "transport_security": if tls.is_some() { "secure" } else { "insecure-bootstrap" },
        "mtls": tls.as_ref().is_some_and(|config| config.is_mtls()),
    });
    println!("{startup}");

    tracing::info!(
        addr = %bound,
        "ACP WebSocket server listening — {ws_scheme}://{bound}/acp"
    );

    // Signal handlers: Ctrl-C + SIGTERM
    let cancel_ctrl_c = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        cancel_ctrl_c.cancel();
    });
    #[cfg(unix)]
    {
        let cancel_sigterm = shutdown.clone();
        tokio::spawn(async move {
            let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
            sig.recv().await;
            cancel_sigterm.cancel();
        });
    }

    crate::control_tls::serve_router_with_shutdown(listener, app, tls, shutdown.cancelled_owned())
        .await?;

    tracing::info!("ACP server shut down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_stdio_mcp_server() {
        let server = McpServer::Stdio(
            McpServerStdio::new("test-server", std::path::PathBuf::from("/usr/bin/test-mcp"))
                .args(vec!["--port".into(), "3000".into()]),
        );

        let (name, config) = convert_acp_mcp_server(server).expect("should convert");
        assert_eq!(name, "test-server");
        assert_eq!(config.command.as_deref(), Some("/usr/bin/test-mcp"));
        assert_eq!(config.args, vec!["--port", "3000"]);
        assert!(config.url.is_none());
    }

    #[test]
    fn convert_http_mcp_server() {
        let server = McpServer::Http(McpServerHttp::new(
            "remote-server",
            "https://mcp.example.com/v1",
        ));

        let (name, config) = convert_acp_mcp_server(server).expect("should convert");
        assert_eq!(name, "remote-server");
        assert_eq!(config.url.as_deref(), Some("https://mcp.example.com/v1"));
        assert!(config.command.is_none());
    }

    #[test]
    fn convert_sse_mcp_server() {
        let server = McpServer::Sse(McpServerSse::new(
            "sse-server",
            "https://mcp.example.com/sse",
        ));

        let (name, config) = convert_acp_mcp_server(server).expect("should convert");
        assert_eq!(name, "sse-server");
        assert_eq!(config.url.as_deref(), Some("https://mcp.example.com/sse"));
    }

    #[test]
    fn control_request_args_accepts_workspace_specific_fields() {
        assert_eq!(
            control_request_args(
                "control/workspace_new",
                &serde_json::json!({ "label": "feature-a" })
            ),
            "feature-a"
        );
        assert_eq!(
            control_request_args(
                "control/workspace_destroy",
                &serde_json::json!({ "target": "workspace-1" })
            ),
            "workspace-1"
        );
        assert_eq!(
            control_request_args(
                "control/workspace_role_set",
                &serde_json::json!({ "role": "release" })
            ),
            "release"
        );
        assert_eq!(
            control_request_args(
                "control/workspace_kind_set",
                &serde_json::json!({ "kind": "spec" })
            ),
            "spec"
        );
    }

    #[test]
    fn control_request_args_prefers_legacy_args_field() {
        assert_eq!(
            control_request_args(
                "control/workspace_new",
                &serde_json::json!({ "args": "from-args", "label": "from-label" })
            ),
            "from-args"
        );
    }

    #[test]
    fn plan_entry_state_mapping() {
        use crate::acp_worker::{PlanEntryData, PlanEntryState};

        let entries = [
            PlanEntryData {
                content: "Step 1".into(),
                status: PlanEntryState::Completed,
            },
            PlanEntryData {
                content: "Step 2".into(),
                status: PlanEntryState::InProgress,
            },
            PlanEntryData {
                content: "Step 3".into(),
                status: PlanEntryState::Pending,
            },
            PlanEntryData {
                content: "Step 4".into(),
                status: PlanEntryState::Failed,
            },
        ];

        let plan_entries = acp_plan_entries(&entries);

        assert_eq!(plan_entries.len(), 4);
        assert_eq!(plan_entries[0].status, PlanEntryStatus::Completed);
        assert_eq!(plan_entries[1].status, PlanEntryStatus::InProgress);
        assert_eq!(plan_entries[2].status, PlanEntryStatus::Pending);
        assert_eq!(plan_entries[3].status, PlanEntryStatus::Completed);
    }

    #[test]
    fn plan_projection_maps_work_plan_items() {
        use crate::acp_worker::PlanEntryState;

        let projection = omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                plan_id: "session:current".into(),
                mode: "executing".into(),
                guidance: "keep going".into(),
                status: "active".into(),
                scope: "session".into(),
                source: "session".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 1,
                    total: 4,
                },
                items: vec![
                    omegon_traits::PlanItemProjection {
                        label: "Inspect".into(),
                        status: "done".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        label: "Patch".into(),
                        status: "active".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        label: "Validate".into(),
                        status: "todo".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        label: "Deferred".into(),
                        status: "skipped".into(),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let entries = plan_entries_from_projection(&projection);

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].content, "Inspect");
        assert_eq!(entries[0].status, PlanEntryState::Completed);
        assert_eq!(entries[1].status, PlanEntryState::InProgress);
        assert_eq!(entries[2].status, PlanEntryState::Pending);
        assert_eq!(entries[3].status, PlanEntryState::Failed);
    }

    #[test]
    fn plan_projection_empty_items_clears_state() {
        let projection = omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                plan_id: "session:current".into(),
                mode: "off".into(),
                guidance: "No active work plan.".into(),
                status: "detached".into(),
                scope: "session".into(),
                source: "session".into(),
                progress: omegon_traits::PlanProgressProjection::default(),
                items: Vec::new(),
            }),
            ..Default::default()
        };

        let entries = plan_entries_from_projection(&projection);

        assert!(entries.is_empty());
    }

    #[test]
    fn acp_status_compresses_plan_set_receipt() {
        let raw = "Plan set
Plan mode: planning
Planning gate active: keep work to read/search/design until /plan approve.
Progress: 0/4

1. ◐ Inventory docs";

        assert_eq!(
            acp_status_message_text(raw).as_deref(),
            Some("Planning mode — edits blocked until approval.")
        );
    }

    #[test]
    fn acp_status_compresses_plan_approval_and_progress() {
        assert_eq!(
            acp_status_message_text(
                "Plan approved
Plan mode: approved
Progress: 0/2"
            )
            .as_deref(),
            Some("Plan approved — execution may proceed.")
        );
        assert_eq!(
            acp_status_message_text(
                "Plan progress
Plan mode: executing
Progress: 1/2"
            )
            .as_deref(),
            Some("Plan executing.")
        );
    }

    #[test]
    fn acp_status_preserves_non_plan_messages_plainly() {
        assert_eq!(
            acp_status_message_text("  Request aborted  ").as_deref(),
            Some("Request aborted")
        );
        assert_eq!(acp_status_message_text("   "), None);
    }

    #[test]
    fn acp_status_identifies_provider_retry_telemetry() {
        assert!(acp_status_is_provider_telemetry(
            "⚠ Upstream stream_stalled — retrying (attempt 2, delay 1000ms): idle"
        ));
        assert!(acp_status_is_provider_telemetry(
            "⚠ anthropic is seeing repeated transient upstream failures: 10 consecutive failures"
        ));
        assert!(!acp_status_is_provider_telemetry("Plan executing."));
    }

    #[test]
    fn stream_idle_payload_matches_flynt_contract() {
        let payload = stream_idle_payload(StreamIdlePayload {
            session_id: "s-1".to_string(),
            provider: "openai-codex".to_string(),
            model: "gpt-5.5".to_string(),
            phase: "ambiguous silent reasoning".to_string(),
            idle_secs: 600,
            ambiguous: true,
            message: "stream idle".to_string(),
        });

        assert_eq!(payload["sessionId"], "s-1");
        assert_eq!(payload["provider"], "openai-codex");
        assert_eq!(payload["model"], "gpt-5.5");
        assert_eq!(payload["phase"], "ambiguous silent reasoning");
        assert_eq!(payload["idleSecs"], 600);
        assert_eq!(payload["ambiguous"], true);
        assert_eq!(payload["message"], "stream idle");
    }

    #[test]
    fn provider_retry_ext_notification_serializes_as_acp_extension_event() {
        let raw = serde_json::value::RawValue::from_string(
            serde_json::json!({
                "sessionId": "s-1",
                "provider": "anthropic",
                "model": "claude",
                "attempt": 2,
                "delayMs": 1000,
                "reason": "stream_idle",
                "message": "idle",
                "recoverable": true
            })
            .to_string(),
        )
        .unwrap();
        let notification = AgentNotification::ExtNotification(ExtNotification::new(
            "_provider/retry",
            std::sync::Arc::from(raw),
        ));
        let untyped = notification.to_untyped_message().unwrap();

        assert_eq!(untyped.method, "_provider/retry");
        assert_eq!(untyped.params["sessionId"], "s-1");
        assert_eq!(untyped.params["provider"], "anthropic");
        assert_eq!(untyped.params["delayMs"], 1000);
        assert_eq!(untyped.params["recoverable"], true);
    }

    #[test]
    fn provider_failure_payload_matches_flynt_contract() {
        let payload = provider_failure_payload(ProviderFailurePayload {
            session_id: "s-1".to_string(),
            provider: "anthropic".to_string(),
            model: "claude".to_string(),
            reason: "stream_idle".to_string(),
            attempts: 8,
            message: "idle".to_string(),
            retryable: false,
            recommended_action: "switch_model".to_string(),
        });

        assert_eq!(payload["sessionId"], "s-1");
        assert_eq!(payload["provider"], "anthropic");
        assert_eq!(payload["model"], "claude");
        assert_eq!(payload["reason"], "stream_idle");
        assert_eq!(payload["attempts"], 8);
        assert_eq!(payload["message"], "idle");
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["recommendedAction"], "switch_model");
    }

    #[test]
    fn turn_cancelled_payload_matches_flynt_contract() {
        let payload = turn_cancelled_payload("s-1", "operator_cancelled");

        assert_eq!(payload["sessionId"], "s-1");
        assert_eq!(payload["reason"], "operator_cancelled");
    }

    #[test]
    fn cancelled_worker_response_maps_to_cancelled_stop_reason() {
        let response = crate::acp_worker::WorkerResponse {
            text: String::new(),
            error: None,
            cancelled: true,
        };
        let stop_reason = if response.cancelled {
            StopReason::Cancelled
        } else {
            StopReason::EndTurn
        };

        assert_eq!(stop_reason, StopReason::Cancelled);
    }

    #[tokio::test]
    async fn cancel_accepts_matching_session_id() {
        let agent = OmegonAcpAgent::new("test-model");
        let sid = SessionId::new("session-a");
        *agent.session_id.borrow_mut() = Some(sid.clone());

        agent.cancel(CancelNotification::new(sid)).await.unwrap();
    }

    #[tokio::test]
    async fn cancel_rejects_wrong_session_id() {
        let agent = OmegonAcpAgent::new("test-model");
        *agent.session_id.borrow_mut() = Some(SessionId::new("session-a"));

        let err = agent
            .cancel(CancelNotification::new(SessionId::new("session-b")))
            .await
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn mcp_config_defaults() {
        let config = mcp_config(
            Some("test-cmd".into()),
            None,
            vec!["arg1".into()],
            std::collections::HashMap::new(),
        );
        assert_eq!(config.command.as_deref(), Some("test-cmd"));
        assert!(config.url.is_none());
        assert_eq!(config.args, vec!["arg1"]);
        assert!(config.network);
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn plan_merge_multi_entry_replaces_state() {
        use crate::acp_worker::{PlanEntryData, PlanEntryState};

        let mut plan_state: Vec<PlanEntryData> = Vec::new();

        // Multi-entry = fresh plan
        let entries = vec![
            PlanEntryData {
                content: "A".into(),
                status: PlanEntryState::Pending,
            },
            PlanEntryData {
                content: "B".into(),
                status: PlanEntryState::Pending,
            },
            PlanEntryData {
                content: "C".into(),
                status: PlanEntryState::Pending,
            },
        ];
        merge_plan_entries(&mut plan_state, entries);
        assert_eq!(plan_state.len(), 3);

        // Single entry update merges
        let update = vec![PlanEntryData {
            content: "B".into(),
            status: PlanEntryState::Completed,
        }];
        merge_plan_entries(&mut plan_state, update);
        assert_eq!(plan_state[0].status, PlanEntryState::Pending);
        assert_eq!(plan_state[1].status, PlanEntryState::Completed);
        assert_eq!(plan_state[2].status, PlanEntryState::Pending);
    }

    #[test]
    fn flynt_client_enables_surface_updates_by_default() {
        let flynt = Implementation::new("flynt", "0.1.0").title("Flynt");
        let zed = Implementation::new("zed", "0.1.0").title("Zed");
        assert!(acp_surface_updates_enabled_for_client(Some(&flynt)));
        assert!(!acp_surface_updates_enabled_for_client(Some(&zed)));
        assert!(!acp_surface_updates_enabled_for_client(None));
    }

    #[test]
    fn surface_metadata_advertises_conversation_contract() {
        let metadata = acp_surface_metadata(true);
        assert_eq!(
            metadata["conversation"]["version"],
            surfaces::ACP_SURFACE_SCHEMA_VERSION
        );
        assert_eq!(metadata["conversation"]["enabled"], true);
        assert_eq!(
            metadata["conversation"]["extensionMethod"],
            ACP_CONVERSATION_SURFACE_METHOD
        );
    }

    #[test]
    fn surface_updates_flag_is_default_off() {
        assert!(!acp_surface_updates_enabled_value(None));
        assert!(!acp_surface_updates_enabled_value(Some("0")));
        assert!(!acp_surface_updates_enabled_value(Some("false")));
        assert!(acp_surface_updates_enabled_value(Some("1")));
        assert!(acp_surface_updates_enabled_value(Some("true")));
        assert!(acp_surface_updates_enabled_value(Some("yes")));
        assert!(acp_surface_updates_enabled_value(Some("on")));
    }

    #[test]
    fn surface_tool_args_project_summary_and_detail() {
        let args = serde_json::json!({"command":"cargo check"});
        let (summary, detail) = acp_surface_tool_args("bash", Some(&args));
        assert_eq!(summary.as_deref(), Some("bash — cargo check"));
        assert_eq!(detail.as_deref(), Some(r#"{"command":"cargo check"}"#));
    }

    #[test]
    fn surface_tool_result_omits_null_details() {
        assert_eq!(acp_surface_tool_result(&serde_json::Value::Null), None);
        assert_eq!(
            acp_surface_tool_result(&serde_json::json!({"ok":true})).as_deref(),
            Some(r#"{"ok":true}"#)
        );
    }

    #[tokio::test]
    async fn initialize_does_not_advertise_unimplemented_session_loading() {
        let agent = OmegonAcpAgent::new("test-model");
        let response = agent
            .initialize(InitializeRequest::new(ProtocolVersion::LATEST))
            .await
            .expect("initialize");

        assert!(!response.agent_capabilities.load_session);
    }

    #[test]
    fn config_options_expose_profile_context_and_semantic_categories() {
        let cwd = tempfile::tempdir().unwrap();
        let agent = OmegonAcpAgent::new("test-model");
        let options = agent.build_config_options(
            "test-model",
            "minimal",
            "standard",
            "built-in-default",
            cwd.path(),
        );

        let by_id = |id: &str| {
            options
                .iter()
                .find(|option| option.id.0.as_ref() == id)
                .expect("config option")
        };
        assert_eq!(
            by_id("model").category,
            Some(SessionConfigOptionCategory::Model)
        );
        assert_eq!(
            by_id("thinking").category,
            Some(SessionConfigOptionCategory::ThoughtLevel)
        );
        assert_eq!(
            by_id("profile").category,
            Some(SessionConfigOptionCategory::Other("_omegon_profile".into()))
        );
        assert_eq!(
            by_id("context_class").category,
            Some(SessionConfigOptionCategory::Other("_omegon_context".into()))
        );
        assert!(
            options
                .iter()
                .all(|option| option.id.0.as_ref() != "posture")
        );
    }

    #[test]
    fn runtime_status_reports_turn_phase_and_last_error() {
        let agent = OmegonAcpAgent::new("test-model");
        *agent.turn_state.borrow_mut() = AcpTurnState {
            phase: AcpTurnPhase::Failed,
            last_error: Some("provider unavailable".into()),
        };

        let status = agent.runtime_status_json();
        assert_eq!(status["acp"]["turn"]["phase"], "failed");
        assert_eq!(status["acp"]["turn"]["last_error"], "provider unavailable");
    }

    #[test]
    fn websocket_agent_reports_websocket_transport() {
        let agent = OmegonAcpAgent::new_for_websocket("test-model", false);
        assert_eq!(agent.transport, "websocket");
    }

    #[tokio::test]
    async fn prompt_rejects_non_active_session_before_worker_start() {
        let agent = OmegonAcpAgent::new("test-model");
        *agent.session_id.borrow_mut() = Some(SessionId::new("session-a"));
        *agent.session_cwd.borrow_mut() = Some(std::env::current_dir().unwrap());

        let result = agent
            .prompt(PromptRequest::new(
                SessionId::new("session-b"),
                vec![ContentBlock::Text(TextContent::new("hello"))],
            ))
            .await;

        assert!(result.is_err());
        assert!(agent.worker.borrow().is_none());
    }

    #[tokio::test]
    async fn close_rejects_non_active_session_without_clearing_state() {
        let agent = OmegonAcpAgent::new("test-model");
        *agent.session_id.borrow_mut() = Some(SessionId::new("session-a"));
        *agent.session_cwd.borrow_mut() = Some(std::env::current_dir().unwrap());

        let result = agent
            .close_session(CloseSessionRequest::new(SessionId::new("session-b")))
            .await;

        assert!(result.is_err());
        assert_eq!(
            agent.session_id.borrow().as_ref(),
            Some(&SessionId::new("session-a"))
        );
        assert!(agent.session_cwd.borrow().is_some());
    }

    #[test]
    fn plan_merge_single_entry_initializes_empty() {
        use crate::acp_worker::{PlanEntryData, PlanEntryState};

        let mut plan_state: Vec<PlanEntryData> = Vec::new();

        // Single entry with empty plan → should initialize
        let entries = vec![PlanEntryData {
            content: "Solo".into(),
            status: PlanEntryState::Pending,
        }];
        merge_plan_entries(&mut plan_state, entries);
        assert_eq!(plan_state.len(), 1);
        assert_eq!(plan_state[0].content, "Solo");
    }

    #[test]
    fn plan_merge_second_fresh_replaces_first() {
        use crate::acp_worker::{PlanEntryData, PlanEntryState};

        let mut plan_state = vec![
            PlanEntryData {
                content: "Old A".into(),
                status: PlanEntryState::Completed,
            },
            PlanEntryData {
                content: "Old B".into(),
                status: PlanEntryState::Pending,
            },
        ];

        // New multi-entry plan replaces old
        let new_entries = vec![
            PlanEntryData {
                content: "New X".into(),
                status: PlanEntryState::Pending,
            },
            PlanEntryData {
                content: "New Y".into(),
                status: PlanEntryState::Pending,
            },
        ];
        merge_plan_entries(&mut plan_state, new_entries);
        assert_eq!(plan_state.len(), 2);
        assert_eq!(plan_state[0].content, "New X");
    }

    #[test]
    fn extension_state_round_trip() {
        let dir = tempfile::tempdir().unwrap();

        // Default state
        let state = crate::extensions::ExtensionState::load(dir.path()).unwrap_or_default();
        assert!(state.enabled);

        // Disable and save
        let mut state = state;
        state.mark_disabled();
        state.save(dir.path()).unwrap();

        // Reload and verify
        let reloaded = crate::extensions::ExtensionState::load(dir.path()).unwrap();
        assert!(!reloaded.enabled);
        assert!(reloaded.last_disabled_at.is_some());

        // Re-enable and save
        let mut state = reloaded;
        state.mark_enabled();
        state.save(dir.path()).unwrap();

        let reloaded = crate::extensions::ExtensionState::load(dir.path()).unwrap();
        assert!(reloaded.enabled);
        assert!(reloaded.last_enabled_at.is_some());
    }

    #[test]
    fn skills_create_and_update_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        // Create
        let content = "+++\nname = \"test-skill\"\ndescription = \"A test\"\ntags = [\"testing\"]\n+++\n\n# Test Skill\n\nDo testing things.\n";
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        // Read back
        let raw = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        let (manifest, body) = omegon_skills::parse_skill_file(&raw);
        assert_eq!(manifest.name, "test-skill");
        assert_eq!(manifest.description, "A test");
        assert_eq!(manifest.tags, vec!["testing"]);
        assert!(body.contains("Do testing things."));

        // Update manifest fields
        let mut manifest = manifest;
        manifest.description = "Updated description".to_string();
        manifest.tags = vec!["testing".into(), "updated".into()];
        std::fs::write(skill_dir.join("SKILL.md"), manifest.to_skill_file(&body)).unwrap();

        // Verify update
        let raw = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        let (manifest, body) = omegon_skills::parse_skill_file(&raw);
        assert_eq!(manifest.description, "Updated description");
        assert_eq!(manifest.tags, vec!["testing", "updated"]);
        assert!(body.contains("Do testing things."));
    }

    #[test]
    fn persona_create_and_read_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let persona_dir = dir.path().join("test-persona");
        std::fs::create_dir_all(&persona_dir).unwrap();

        // Build plugin.toml
        let mut plugin = toml::Table::new();
        let mut plugin_section = toml::Table::new();
        plugin_section.insert("type".into(), "persona".into());
        plugin_section.insert("id".into(), "user.test-persona".into());
        plugin_section.insert("name".into(), "Test Persona".into());
        plugin_section.insert("version".into(), "1.0.0".into());
        plugin_section.insert("description".into(), "A test persona".into());
        plugin.insert("plugin".into(), toml::Value::Table(plugin_section));

        let mut persona = toml::Table::new();
        let mut identity = toml::Table::new();
        identity.insert("directive".into(), "PERSONA.md".into());
        persona.insert("identity".into(), toml::Value::Table(identity));
        let mut style = toml::Table::new();
        style.insert("badge".into(), "T".into());
        persona.insert("style".into(), toml::Value::Table(style));
        plugin.insert("persona".into(), toml::Value::Table(persona));

        std::fs::write(
            persona_dir.join("plugin.toml"),
            toml::to_string_pretty(&plugin).unwrap(),
        )
        .unwrap();
        std::fs::write(persona_dir.join("PERSONA.md"), "You are a test persona.\n").unwrap();

        // Load via persona_loader
        let loaded = crate::plugins::persona_loader::load_persona(&persona_dir).unwrap();
        assert_eq!(loaded.id, "user.test-persona");
        assert_eq!(loaded.name, "Test Persona");
        assert!(loaded.directive.contains("test persona"));
        assert_eq!(loaded.badge, Some("T".into()));

        // Update plugin.toml
        let manifest_content = std::fs::read_to_string(persona_dir.join("plugin.toml")).unwrap();
        let mut manifest: toml::Table = toml::from_str(&manifest_content).unwrap();
        if let Some(plugin) = manifest.get_mut("plugin").and_then(|v| v.as_table_mut()) {
            plugin.insert("description".into(), "Updated description".into());
        }
        std::fs::write(
            persona_dir.join("plugin.toml"),
            toml::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // Reload and verify
        let reloaded = crate::plugins::persona_loader::load_persona(&persona_dir).unwrap();
        assert_eq!(reloaded.id, "user.test-persona");
    }
}

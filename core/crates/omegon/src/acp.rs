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

#[path = "acp/labels.rs"]
mod labels;
#[path = "acp/model_options.rs"]
mod model_options;
#[path = "acp/resource_context.rs"]
mod resource_context;

use labels::compact_tool_call_label;
use model_options::{
    acp_model_provider_available, compact_model_label, unavailable_current_model_label,
};
use resource_context::prompt_blocks_to_text;

type JsonRpcMessage = agent_client_protocol::jsonrpcmsg::Message;
type JsonRpcTx =
    futures::channel::mpsc::UnboundedSender<agent_client_protocol::Result<JsonRpcMessage>>;
type PendingResponseTx =
    futures::channel::oneshot::Sender<agent_client_protocol::Result<serde_json::Value>>;
type PendingResponses = Rc<RefCell<std::collections::BTreeMap<String, PendingResponseTx>>>;

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
        _ => Err(agent_client_protocol::Error::method_not_found()),
    }
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

pub struct OmegonAcpAgent {
    model: String,
    worker: RefCell<Option<WorkerHandle>>,
    conn: SharedAcpClientConnection,
    session_id: RefCell<Option<SessionId>>,
    secrets: RefCell<Option<std::sync::Arc<omegon_secrets::SecretsManager>>>,
    host_caps: RefCell<HostCapabilities>,
    extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
}

impl OmegonAcpAgent {
    pub fn new(model: &str) -> Self {
        Self::new_with_extension_metadata(model, Default::default())
    }

    pub fn new_with_extension_metadata(
        model: &str,
        extension_metadata: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            model: model.to_string(),
            worker: RefCell::new(None),
            conn: Rc::new(RefCell::new(None)),
            session_id: RefCell::new(None),
            secrets: RefCell::new(None),
            host_caps: RefCell::new(HostCapabilities::default()),
            extension_metadata,
        }
    }

    pub fn set_client(&self, c: AcpClientConnection) {
        *self.conn.borrow_mut() = Some(c);
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
        current_posture: &str,
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
                .then_with(|| a.cost_tier.cmp(&b.cost_tier))
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

        let posture_options: Vec<SessionConfigSelectOption> = [
            ("fabricator", "Fabricator — balanced coding"),
            ("architect", "Architect — orchestrator"),
            ("explorator", "Explorator — lean, read-heavy"),
            ("devastator", "Devastator — maximum force"),
        ]
        .iter()
        .map(|(id, name)| SessionConfigSelectOption::new(*id, *name))
        .collect();

        vec![
            SessionConfigOption::new(
                "model",
                "Model",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_model.to_string(),
                    model_options,
                )),
            ),
            SessionConfigOption::new(
                "thinking",
                "Thinking Level",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_thinking.to_string(),
                    thinking_options,
                )),
            ),
            SessionConfigOption::new(
                "posture",
                "Posture",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    current_posture.to_string(),
                    posture_options,
                )),
            ),
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

            let mut handle =
                acp_worker::spawn_worker(self.model.clone(), cwd.to_path_buf(), host_ctx);
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
    fn current_settings(&self) -> (String, String, String) {
        let settings_arc = self.worker.borrow().as_ref().map(|w| w.settings.clone());
        if let Some(s) = settings_arc
            && let Ok(g) = s.lock()
        {
            return (
                g.model.clone(),
                g.thinking.as_str().to_string(),
                g.posture.effective.as_str().to_string(),
            );
        }
        (self.model.clone(), "minimal".into(), "fabricator".into())
    }
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

        let mut response = InitializeResponse::new(args.protocol_version);
        response.agent_info =
            Some(Implementation::new("omegon", env!("CARGO_PKG_VERSION")).title("Omegon Agent"));
        response.agent_capabilities = AgentCapabilities::default()
            .load_session(true)
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
        if !self.extension_metadata.is_empty() {
            response.meta = Some(serde_json::Map::from_iter([(
                "omegon/extensions".to_string(),
                serde_json::json!(self.extension_metadata),
            )]));
        }
        Ok(response)
    }

    async fn authenticate(&self, _args: AuthenticateRequest) -> Result<AuthenticateResponse> {
        Ok(AuthenticateResponse::default())
    }

    async fn new_session(&self, args: NewSessionRequest) -> Result<NewSessionResponse> {
        let cwd = args.cwd;

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
        let (current_model, current_thinking, current_posture) = self.current_settings();
        response.config_options =
            Some(self.build_config_options(&current_model, &current_thinking, &current_posture));

        // Send available commands after response (via spawned task)
        let conn = self.conn.clone();
        let cmd_sid = sid.clone();
        tokio::task::spawn_local(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if let Some(c) = conn.borrow().as_ref() {
                let _ = send_session_update(
                    c,
                    cmd_sid,
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                        AvailableCommand::new("model", "List or switch LLM model"),
                        AvailableCommand::new("thinking", "Show or set thinking level"),
                        AvailableCommand::new("posture", "Show or set behavioral posture"),
                        AvailableCommand::new(
                            "skills",
                            "Manage skills (list, get, create, delete)",
                        ),
                        AvailableCommand::new(
                            "extension",
                            "Manage extensions (list, install, enable, search)",
                        ),
                        AvailableCommand::new(
                            "armory",
                            "Browse upstream extensions, plugins, skills, and agents",
                        ),
                        AvailableCommand::new("persona", "Manage personas (list, create, switch)"),
                        AvailableCommand::new(
                            "catalog",
                            "Browse agent catalog (list, install, remove)",
                        ),
                        AvailableCommand::new("secrets", "Show configured secrets (no values)"),
                        AvailableCommand::new("status", "Session status"),
                        AvailableCommand::new("login", "Authentication help"),
                        AvailableCommand::new("help", "List all commands"),
                    ])),
                )
                .await;
            }
        });

        Ok(response)
    }

    async fn prompt(&self, args: PromptRequest) -> Result<PromptResponse> {
        let sid = args.session_id.clone();

        // Ensure worker exists
        let cwd = std::env::current_dir().unwrap_or_default();
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
                loop {
                    match event_rx.recv().await {
                        Ok(WorkerEvent::TextChunk(text)) => {
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
                            let msg = redact(&msg);
                            let Some(msg) = acp_status_message_text(&msg) else {
                                continue;
                            };
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
                        Ok(WorkerEvent::ExtensionMetadata(metadata)) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let meta = serde_json::Map::from_iter([(
                                    "omegon/extensions".to_string(),
                                    serde_json::json!(metadata),
                                )]);
                                let _ = send_session_update(
                                    c,
                                    stream_sid.clone(),
                                    SessionUpdate::SessionInfoUpdate(
                                        SessionInfoUpdate::new().meta(meta),
                                    ),
                                )
                                .await;
                            }
                        }
                        Ok(WorkerEvent::TurnComplete) => break,
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
        let worker_resp = response_rx.await.map_err(|_| Error::internal_error())?;
        let _ = done_rx.await;

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

        Ok(PromptResponse::new(StopReason::EndTurn))
    }

    async fn cancel(&self, _args: CancelNotification) -> Result<()> {
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

        // Use ack so we read shared_settings AFTER the worker has applied the
        // mutation. Without this the response would race the worker thread
        // and report the previous value.
        let (req, ack_rx) = match config_id.as_str() {
            "model" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (
                    WorkerRequest::SetModel {
                        value: value.clone(),
                        ack: Some(tx),
                    },
                    rx,
                )
            }
            "thinking" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (
                    WorkerRequest::SetThinking {
                        value: value.clone(),
                        ack: Some(tx),
                    },
                    rx,
                )
            }
            "posture" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (
                    WorkerRequest::SetPosture {
                        value: value.clone(),
                        ack: Some(tx),
                    },
                    rx,
                )
            }
            _ => return Err(Error::invalid_params()),
        };
        self.send_to_worker_ack(req).await;
        let _ = ack_rx.await;

        // Read back from the worker's settings — send_to_worker awaits the
        // mutation, so this captures the actually-applied state (which may
        // differ from `value` if the worker rejected/normalised the input).
        let (current_model, current_thinking, current_posture) = self.current_settings();
        let options =
            self.build_config_options(&current_model, &current_thinking, &current_posture);

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

    async fn close_session(&self, _args: CloseSessionRequest) -> Result<CloseSessionResponse> {
        self.send_to_worker(WorkerRequest::Cancel).await;
        self.send_to_worker(WorkerRequest::Shutdown).await;
        *self.worker.borrow_mut() = None;
        *self.session_id.borrow_mut() = None;
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
    async fn handle_ext_method(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        use crate::extensions::{ExtensionManifest, ExtensionState, config_store};

        let extensions_dir = crate::extension_cli::extensions_dir()?;

        match method {
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

                        extensions.push(serde_json::json!({
                            "name": manifest.extension.name,
                            "version": manifest.extension.version,
                            "description": manifest.extension.description,
                            "enabled": state.enabled,
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

            "secrets/list" => {
                if let Some(ref mgr) = *self.secrets.borrow() {
                    let items: Vec<serde_json::Value> = mgr
                        .list_recipes()
                        .into_iter()
                        .map(|(name, recipe)| serde_json::json!({ "name": name, "recipe": recipe }))
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
                Ok(serde_json::json!({ "ok": true, "result": result }))
            }

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
                    let (mut manifest, body) = crate::skills::parse_skill_file(&existing);

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
            "/thinking" if !args.is_empty() => {
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
            "/thinking" => "Use the thinking dropdown or /thinking <off|minimal|low|medium|high>".into(),
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
            "/status" => format!("omegon {} | ACP mode | Worker thread active", env!("CARGO_PKG_VERSION")),
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
            "/login" => "Omegon manages authentication independently.\nRun `omegon auth login` in a terminal or set API keys.".into(),
            "/skills" => {
                let args = args.trim();
                match args {
                    "" | "list" => "Use the **skills/list** RPC to get structured skill data, or type `/skills list` to see a summary.\nAvailable: list, get <name>, create, delete <name>, install [name|skills/name]".into(),
                    "install" => "Use the **skills/install** RPC to install bundled skills, or pass a skill name to install through Armory.".into(),
                    _ => format!("Skills subcommand: {args}. Use the **skills/{args}** RPC for structured operations."),
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
            "/help" => "Commands: /model /thinking /posture /skills /extension /armory /persona /catalog /secrets /status /login /help\n\nFull CRUD is available via RPC ext_methods (armory/*, skills/*, extensions/*, personas/*, catalog/*, secrets/*).".into(),
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
    }
}

pub async fn run(model: &str, agent_id: Option<&str>, cwd: &std::path::Path) -> anyhow::Result<()> {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    if let Some(id) = agent_id {
        let shared_settings = crate::settings::shared(model);
        crate::apply_agent_manifest_pre_setup(id, cwd, &shared_settings)?;
    }

    let agent = Rc::new(OmegonAcpAgent::new(model));

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
        model: model.to_string(),
        cwd: std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()),
        agent_id: agent_id.map(String::from),
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
    fn plan_snapshot_json_maps_work_plan_items() {
        use crate::acp_worker::PlanEntryState;

        let snapshot = serde_json::json!({
            "mode": "executing",
            "completed": 1,
            "total": 4,
            "items": [
                { "description": "Inspect", "status": "done" },
                { "description": "Patch", "status": "active" },
                { "description": "Validate", "status": "todo" },
                { "description": "Deferred", "status": "skipped" }
            ]
        });

        let entries = plan_entries_from_snapshot_json(&snapshot);

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].content, "Inspect");
        assert_eq!(entries[0].status, PlanEntryState::Completed);
        assert_eq!(entries[1].status, PlanEntryState::InProgress);
        assert_eq!(entries[2].status, PlanEntryState::Pending);
        assert_eq!(entries[3].status, PlanEntryState::Failed);
    }

    #[test]
    fn plan_snapshot_json_empty_items_clears_state() {
        let snapshot = serde_json::json!({
            "mode": "off",
            "completed": 0,
            "total": 0,
            "items": []
        });

        let entries = plan_entries_from_snapshot_json(&snapshot);

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
        let (manifest, body) = crate::skills::parse_skill_file(&raw);
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
        let (manifest, body) = crate::skills::parse_skill_file(&raw);
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

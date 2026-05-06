//! ACP transport — thin layer that forwards prompts to the worker thread
//! and streams events back to the editor via ACP notifications.
//!
//! Architecture:
//! - ACP I/O runs on the main thread (LocalSet, !Send)
//! - Agent loop runs on a dedicated worker thread (own runtime)
//! - Communication via typed channels (WorkerRequest/WorkerResponse/WorkerEvent)

use std::cell::RefCell;
use std::rc::Rc;

use agent_client_protocol::*;
use anyhow::Context;

use crate::acp_worker::{self, WorkerEvent, WorkerHandle, WorkerRequest};

pub struct OmegonAcpAgent {
    model: String,
    worker: RefCell<Option<WorkerHandle>>,
    conn: Rc<RefCell<Option<AgentSideConnection>>>,
    session_id: RefCell<Option<SessionId>>,
}

impl OmegonAcpAgent {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            worker: RefCell::new(None),
            conn: Rc::new(RefCell::new(None)),
            session_id: RefCell::new(None),
        }
    }

    pub fn set_client(&self, c: AgentSideConnection) {
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

        for (id, name) in [
            ("anthropic:claude-opus-4-7", "Claude Opus 4.7"),
            ("anthropic:claude-sonnet-4-7", "Claude Sonnet 4.7"),
            ("anthropic:claude-opus-4-6", "Claude Opus 4.6"),
            ("anthropic:claude-sonnet-4-6", "Claude Sonnet 4.6"),
            ("openai:gpt-5.4", "GPT-5.4"),
        ] {
            model_options.push(SessionConfigSelectOption::new(id, name));
        }

        if !model_options
            .iter()
            .any(|o| o.value.0.as_ref() == current_model)
        {
            model_options.insert(
                0,
                SessionConfigSelectOption::new(
                    current_model.to_string(),
                    current_model.to_string(),
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
            let handle = acp_worker::spawn_worker(self.model.clone(), cwd.to_path_buf());
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

#[async_trait::async_trait(?Send)]
impl Agent for OmegonAcpAgent {
    async fn initialize(&self, args: InitializeRequest) -> Result<InitializeResponse> {
        let mut response = InitializeResponse::new(args.protocol_version);
        response.agent_info =
            Some(Implementation::new("omegon", env!("CARGO_PKG_VERSION")).title("Omegon Agent"));
        response.agent_capabilities = AgentCapabilities::default().load_session(true);
        response.auth_methods = vec![AuthMethod::Agent(
            AuthMethodAgent::new("omegon-auth", "Omegon Authentication")
                .description("Run `omegon auth login` in a terminal or set API keys."),
        )];
        Ok(response)
    }

    async fn authenticate(&self, _args: AuthenticateRequest) -> Result<AuthenticateResponse> {
        Ok(AuthenticateResponse::default())
    }

    async fn new_session(&self, args: NewSessionRequest) -> Result<NewSessionResponse> {
        let cwd = args.cwd;
        self.ensure_worker(&cwd);

        let sid = SessionId::new(format!(
            "omegon-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        *self.session_id.borrow_mut() = Some(sid.clone());

        let mut response = NewSessionResponse::new(sid.clone());
        response.modes = Some(Self::modes());

        // Read the *worker's* current settings, not self.model — the worker may
        // have already received SetModel/SetThinking/SetPosture before this
        // session started, and we need to advertise what's actually running.
        let (current_model, current_thinking, current_posture) = self.current_settings();
        response.config_options = Some(self.build_config_options(
            &current_model,
            &current_thinking,
            &current_posture,
        ));

        // Send available commands after response (via spawned task)
        let conn = self.conn.clone();
        let cmd_sid = sid.clone();
        tokio::task::spawn_local(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if let Some(c) = conn.borrow().as_ref() {
                let _ = c
                    .session_notification(SessionNotification::new(
                        cmd_sid,
                        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                            AvailableCommand::new("model", "List or switch LLM model"),
                            AvailableCommand::new("thinking", "Show or set thinking level"),
                            AvailableCommand::new("posture", "Show or set behavioral posture"),
                            AvailableCommand::new("compact", "Compact context window"),
                            AvailableCommand::new("clear", "Clear conversation"),
                            AvailableCommand::new("secrets", "Show configured secrets (no values)"),
                            AvailableCommand::new("status", "Session status"),
                            AvailableCommand::new("login", "Authentication help"),
                            AvailableCommand::new("help", "List all commands"),
                        ])),
                    ))
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

        // Extract user text
        let user_text: String = args
            .prompt
            .iter()
            .filter_map(|block| {
                if let ContentBlock::Text(text) = block {
                    Some(text.text.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Handle slash commands locally (no worker round-trip)
        if user_text.starts_with('/') {
            let response_text = self.handle_slash_command(&user_text);
            let conn = self.conn.clone();
            let notify_sid = sid.clone();
            tokio::task::spawn_local(async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = c
                        .session_notification(SessionNotification::new(
                            notify_sid,
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new(response_text)),
                            )),
                        ))
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
            tokio::task::spawn_local(async move {
                loop {
                    match event_rx.recv().await {
                        Ok(WorkerEvent::TextChunk(text)) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = c
                                    .session_notification(SessionNotification::new(
                                        stream_sid.clone(),
                                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                            ContentBlock::Text(TextContent::new(text)),
                                        )),
                                    ))
                                    .await;
                            }
                        }
                        Ok(WorkerEvent::ThinkingChunk(text)) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = c
                                    .session_notification(SessionNotification::new(
                                        stream_sid.clone(),
                                        SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                                            ContentBlock::Text(TextContent::new(text)),
                                        )),
                                    ))
                                    .await;
                            }
                        }
                        Ok(WorkerEvent::ToolStart { id, name, args }) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let mut tc = ToolCall::new(ToolCallId::new(id), name);
                                tc.status = ToolCallStatus::InProgress;
                                tc.raw_input = args;
                                let _ = c
                                    .session_notification(SessionNotification::new(
                                        stream_sid.clone(),
                                        SessionUpdate::ToolCall(tc),
                                    ))
                                    .await;
                            }
                        }
                        Ok(WorkerEvent::StatusUpdate(msg)) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let _ = c
                                    .session_notification(SessionNotification::new(
                                        stream_sid.clone(),
                                        SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                            ContentBlock::Text(TextContent::new(format!(
                                                "_{msg}_\n\n"
                                            ))),
                                        )),
                                    ))
                                    .await;
                            }
                        }
                        Ok(WorkerEvent::ToolEnd { id, success }) => {
                            if let Some(c) = conn.borrow().as_ref() {
                                let status = if success {
                                    ToolCallStatus::Completed
                                } else {
                                    ToolCallStatus::Failed
                                };
                                let fields = ToolCallUpdateFields::new().status(status);
                                let _ = c
                                    .session_notification(SessionNotification::new(
                                        stream_sid.clone(),
                                        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                                            ToolCallId::new(id),
                                            fields,
                                        )),
                                    ))
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
            let err_text = format!("[Error: {error}]");
            tokio::task::spawn_local(async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = c
                        .session_notification(SessionNotification::new(
                            err_sid,
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                ContentBlock::Text(TextContent::new(err_text)),
                            )),
                        ))
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
        Ok(SetSessionModeResponse::new())
    }

    async fn set_session_config_option(
        &self,
        args: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse> {
        let config_id = args.config_id.0.to_string();
        let value = args.value.0.to_string();

        // Use ack so we read shared_settings AFTER the worker has applied the
        // mutation. Without this the response would race the worker thread
        // and report the previous value.
        let (req, ack_rx) = match config_id.as_str() {
            "model" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (
                    WorkerRequest::SetModel { value: value.clone(), ack: Some(tx) },
                    rx,
                )
            }
            "thinking" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (
                    WorkerRequest::SetThinking { value: value.clone(), ack: Some(tx) },
                    rx,
                )
            }
            "posture" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (
                    WorkerRequest::SetPosture { value: value.clone(), ack: Some(tx) },
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
        let options = self.build_config_options(&current_model, &current_thinking, &current_posture);

        // Also push a ConfigOptionUpdate notification so clients that don't
        // inspect the response value (e.g. flynt-app's set_config which
        // discards the response, or any client that triggers a config change
        // through a different path) still see the new state.
        if let Some(sid) = self.session_id.borrow().clone() {
            let conn = self.conn.clone();
            let push_options = options.clone();
            tokio::task::spawn_local(async move {
                if let Some(c) = conn.borrow().as_ref() {
                    let _ = c
                        .session_notification(SessionNotification::new(
                            sid,
                            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
                                push_options,
                            )),
                        ))
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
}

impl OmegonAcpAgent {
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
            "/help" => "Commands: /model /thinking /posture /secrets /status /version /login /help\nUse the dropdowns at the bottom to switch model, thinking, and posture.".into(),
            _ => format!("Unknown: {cmd}. Type /help"),
        }
    }
}

// ── Entry point ────────────────────────────────────────────────────────

pub async fn run(
    model: &str,
    agent_id: Option<&str>,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    if let Some(id) = agent_id {
        let shared_settings = crate::settings::shared(model);
        crate::apply_agent_manifest_pre_setup(id, cwd, &shared_settings)?;
    }

    let agent = Rc::new(OmegonAcpAgent::new(model));

    let stdout = tokio::io::stdout().compat_write();
    let stdin = tokio::io::stdin().compat();

    let agent_clone = agent.clone();
    let (conn, io_task) = AgentSideConnection::new(agent_clone, stdout, stdin, |fut| {
        tokio::task::spawn_local(fut);
    });

    agent.set_client(conn);

    io_task.await.context("ACP IO task ended")?;

    Ok(())
}

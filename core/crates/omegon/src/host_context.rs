//! Host-aware capability layer for ACP clients.
//!
//! When omegon runs under an ACP host (Zed, JetBrains, etc.) the host may
//! advertise capabilities for file I/O, terminal execution, and permission
//! mediation. This module captures those capabilities and provides a
//! channel-based proxy so the worker thread can call host methods without
//! holding a reference to the !Send ACP client connection.

use std::path::PathBuf;
use std::sync::Arc;

use crate::extensions::approval::{HostActionApprovalDecision, decision_from_permission_outcome};
use agent_client_protocol::schema::*;
use omegon_traits::{ContentBlock, ToolResult};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

// ---------------------------------------------------------------------------
// Capabilities snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct HostCapabilities {
    pub fs_read: bool,
    pub fs_write: bool,
    pub terminal: bool,
}

impl HostCapabilities {
    pub fn from_client(caps: &ClientCapabilities) -> Self {
        Self {
            fs_read: caps.fs.read_text_file,
            fs_write: caps.fs.write_text_file,
            terminal: caps.terminal,
        }
    }

    pub fn has_any_delegation(&self) -> bool {
        self.fs_read || self.fs_write || self.terminal
    }
}

// ---------------------------------------------------------------------------
// Proxy request/response types
// ---------------------------------------------------------------------------

pub enum HostProxyRequest {
    ReadTextFile {
        path: PathBuf,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    WriteTextFile {
        path: PathBuf,
        content: String,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    CreateTerminal {
        command: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        output_byte_limit: Option<u64>,
        reply: oneshot::Sender<anyhow::Result<TerminalId>>,
    },
    TerminalOutput {
        terminal_id: TerminalId,
        reply: oneshot::Sender<anyhow::Result<TerminalOutputResponse>>,
    },
    WaitForTerminalExit {
        terminal_id: TerminalId,
        reply: oneshot::Sender<anyhow::Result<WaitForTerminalExitResponse>>,
    },
    KillTerminal {
        terminal_id: TerminalId,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    ReleaseTerminal {
        terminal_id: TerminalId,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    RequestPermission {
        tool_call_id: String,
        tool_name: String,
        path: String,
        reply: oneshot::Sender<anyhow::Result<RequestPermissionOutcome>>,
    },
    RequestHostActionApproval {
        request: Box<RequestPermissionRequest>,
        reply: oneshot::Sender<anyhow::Result<HostActionApprovalDecision>>,
    },
}

// ---------------------------------------------------------------------------
// Worker-side sender handle
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct HostProxySender {
    tx: mpsc::Sender<HostProxyRequest>,
}

impl HostProxySender {
    pub fn new(tx: mpsc::Sender<HostProxyRequest>) -> Self {
        Self { tx }
    }

    pub async fn read_text_file(&self, path: PathBuf) -> anyhow::Result<String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::ReadTextFile { path, reply })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn write_text_file(&self, path: PathBuf, content: String) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::WriteTextFile {
                path,
                content,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn create_terminal(
        &self,
        command: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        output_byte_limit: Option<u64>,
    ) -> anyhow::Result<TerminalId> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::CreateTerminal {
                command,
                args,
                cwd,
                output_byte_limit,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn terminal_output(
        &self,
        terminal_id: TerminalId,
    ) -> anyhow::Result<TerminalOutputResponse> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::TerminalOutput { terminal_id, reply })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn wait_for_terminal_exit(
        &self,
        terminal_id: TerminalId,
    ) -> anyhow::Result<WaitForTerminalExitResponse> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::WaitForTerminalExit { terminal_id, reply })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn kill_terminal(&self, terminal_id: TerminalId) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::KillTerminal { terminal_id, reply })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn release_terminal(&self, terminal_id: TerminalId) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::ReleaseTerminal { terminal_id, reply })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn request_host_action_approval(
        &self,
        request: RequestPermissionRequest,
    ) -> anyhow::Result<HostActionApprovalDecision> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::RequestHostActionApproval {
                request: Box::new(request),
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }

    pub async fn request_permission(
        &self,
        tool_call_id: String,
        tool_name: String,
        path: String,
    ) -> anyhow::Result<RequestPermissionOutcome> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(HostProxyRequest::RequestPermission {
                tool_call_id,
                tool_name,
                path,
                reply,
            })
            .await
            .map_err(|_| anyhow::anyhow!("host proxy channel closed"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("host proxy reply dropped"))?
    }
}

// ---------------------------------------------------------------------------
// Composite context passed to the worker
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct HostContext {
    pub caps: Arc<HostCapabilities>,
    pub proxy: HostProxySender,
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// ACP client operations boundary. Keep direct SDK method calls here so the
// 0.12 migration can replace this layer without disturbing worker-side host
// delegation semantics.
// ---------------------------------------------------------------------------

async fn acp_read_text_file(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    path: PathBuf,
) -> agent_client_protocol::Result<ReadTextFileResponse> {
    client
        .send_request(ReadTextFileRequest::new(session_id, path))
        .await
}

async fn acp_write_text_file(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    path: PathBuf,
    content: String,
) -> agent_client_protocol::Result<WriteTextFileResponse> {
    client
        .send_request(WriteTextFileRequest::new(session_id, path, content))
        .await
}

async fn acp_create_terminal(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    command: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    output_byte_limit: Option<u64>,
) -> agent_client_protocol::Result<CreateTerminalResponse> {
    let mut request = CreateTerminalRequest::new(session_id, command);
    if !args.is_empty() {
        request = request.args(args);
    }
    if let Some(cwd) = cwd {
        request = request.cwd(cwd);
    }
    if let Some(limit) = output_byte_limit {
        request = request.output_byte_limit(limit);
    }
    client.send_request(request).await
}

async fn acp_terminal_output(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    terminal_id: TerminalId,
) -> agent_client_protocol::Result<TerminalOutputResponse> {
    client
        .send_request(TerminalOutputRequest::new(session_id, terminal_id))
        .await
}

async fn acp_wait_for_terminal_exit(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    terminal_id: TerminalId,
) -> agent_client_protocol::Result<WaitForTerminalExitResponse> {
    client
        .send_request(WaitForTerminalExitRequest::new(session_id, terminal_id))
        .await
}

async fn acp_kill_terminal(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    terminal_id: TerminalId,
) -> agent_client_protocol::Result<KillTerminalResponse> {
    client
        .send_request(KillTerminalRequest::new(session_id, terminal_id))
        .await
}

async fn acp_release_terminal(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    terminal_id: TerminalId,
) -> agent_client_protocol::Result<ReleaseTerminalResponse> {
    client
        .send_request(ReleaseTerminalRequest::new(session_id, terminal_id))
        .await
}

async fn acp_request_permission(
    client: &crate::acp::AcpClientConnection,
    session_id: SessionId,
    tool_call_id: String,
    tool_name: String,
    path: String,
) -> agent_client_protocol::Result<RequestPermissionResponse> {
    let tool_call = ToolCallUpdate::new(
        tool_call_id,
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::InProgress)
            .title(tool_name)
            .raw_input(Value::String(format!("Access path: {path}"))),
    );
    let options = vec![
        PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
        PermissionOption::new(
            "allow_always",
            "Allow always (save)",
            PermissionOptionKind::AllowAlways,
        ),
        PermissionOption::new("reject_once", "Reject", PermissionOptionKind::RejectOnce),
    ];
    client
        .send_request(RequestPermissionRequest::new(
            session_id, tool_call, options,
        ))
        .await
}

// ---------------------------------------------------------------------------
// ACP-thread pump — receives HostProxyRequests and executes them on the
// ACP client connection.
//
// The pump borrows conn across .await just like the existing event
// forwarder in acp.rs (which has #[allow(clippy::await_holding_refcell_ref)]
// on the module). Both run on the same single-threaded LocalSet.
// The event forwarder's borrows are non-blocking channel sends that
// complete in-line without yielding, so they never overlap with the
// pump's longer-lived borrows during RPC round-trips.
// ---------------------------------------------------------------------------

pub fn spawn_proxy_pump(
    mut rx: mpsc::Receiver<HostProxyRequest>,
    conn: crate::acp::SharedAcpClientConnection,
    session_id: SessionId,
) {
    tokio::task::spawn_local(async move {
        while let Some(req) = rx.recv().await {
            let client = conn.borrow().as_ref().cloned();
            let Some(client) = client else {
                macro_rules! no_conn {
                    () => {
                        Err(anyhow::anyhow!("no ACP connection"))
                    };
                }
                match req {
                    HostProxyRequest::ReadTextFile { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::WriteTextFile { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::CreateTerminal { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::TerminalOutput { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::WaitForTerminalExit { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::KillTerminal { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::ReleaseTerminal { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::RequestPermission { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                    HostProxyRequest::RequestHostActionApproval { reply, .. } => {
                        let _ = reply.send(no_conn!());
                    }
                }
                continue;
            };

            match req {
                HostProxyRequest::ReadTextFile { path, reply } => {
                    let result =
                        acp_read_text_file(&client, session_id.clone(), path.clone()).await;
                    let _ = reply.send(match result {
                        Ok(resp) => Ok(resp.content),
                        Err(e) => Err(anyhow::anyhow!(
                            "host read_text_file {}: {}",
                            path.display(),
                            e.message
                        )),
                    });
                }
                HostProxyRequest::WriteTextFile {
                    path,
                    content,
                    reply,
                } => {
                    let result =
                        acp_write_text_file(&client, session_id.clone(), path.clone(), content)
                            .await;
                    let _ = reply.send(match result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(anyhow::anyhow!(
                            "host write_text_file {}: {}",
                            path.display(),
                            e.message
                        )),
                    });
                }
                HostProxyRequest::CreateTerminal {
                    command,
                    args,
                    cwd,
                    output_byte_limit,
                    reply,
                } => {
                    let result = acp_create_terminal(
                        &client,
                        session_id.clone(),
                        command,
                        args,
                        cwd,
                        output_byte_limit,
                    )
                    .await;
                    let _ = reply.send(match result {
                        Ok(resp) => Ok(resp.terminal_id),
                        Err(e) => Err(anyhow::anyhow!("host create_terminal: {}", e.message)),
                    });
                }
                HostProxyRequest::TerminalOutput { terminal_id, reply } => {
                    let result =
                        acp_terminal_output(&client, session_id.clone(), terminal_id).await;
                    let _ = reply.send(match result {
                        Ok(resp) => Ok(resp),
                        Err(e) => Err(anyhow::anyhow!("host terminal_output: {}", e.message)),
                    });
                }
                HostProxyRequest::WaitForTerminalExit { terminal_id, reply } => {
                    let result =
                        acp_wait_for_terminal_exit(&client, session_id.clone(), terminal_id).await;
                    let _ = reply.send(match result {
                        Ok(resp) => Ok(resp),
                        Err(e) => Err(anyhow::anyhow!(
                            "host wait_for_terminal_exit: {}",
                            e.message
                        )),
                    });
                }
                HostProxyRequest::KillTerminal { terminal_id, reply } => {
                    let result = acp_kill_terminal(&client, session_id.clone(), terminal_id).await;
                    let _ = reply.send(match result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(anyhow::anyhow!("host kill_terminal: {}", e.message)),
                    });
                }
                HostProxyRequest::ReleaseTerminal { terminal_id, reply } => {
                    let result =
                        acp_release_terminal(&client, session_id.clone(), terminal_id).await;
                    let _ = reply.send(match result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(anyhow::anyhow!("host release_terminal: {}", e.message)),
                    });
                }
                HostProxyRequest::RequestPermission {
                    tool_call_id,
                    tool_name,
                    path,
                    reply,
                } => {
                    let result = acp_request_permission(
                        &client,
                        session_id.clone(),
                        tool_call_id,
                        tool_name,
                        path,
                    )
                    .await;
                    let _ = reply.send(match result {
                        Ok(resp) => Ok(resp.outcome),
                        Err(e) => Err(anyhow::anyhow!("host request_permission: {}", e.message)),
                    });
                }
                HostProxyRequest::RequestHostActionApproval { request, reply } => {
                    let result = client.send_request(*request).await;
                    let _ = reply.send(match result {
                        Ok(resp) => Ok(decision_from_permission_outcome(resp.outcome)),
                        Err(e) => Err(anyhow::anyhow!(
                            "host request_host_action_approval: {}",
                            e.message
                        )),
                    });
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tool delegation — intercepts tool calls that can be served by the host.
// Returns None if the tool should execute locally.
// ---------------------------------------------------------------------------

const READ_MAX_LINES: usize = 2000;
const READ_MAX_BYTES: usize = 50 * 1024;

fn validate_delegation_path(path_str: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(path_str);
    if !path.is_absolute() {
        anyhow::bail!("delegation requires absolute path, got: {path_str}");
    }
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        anyhow::bail!("path traversal rejected: {path_str}");
    }
    Ok(path)
}

pub async fn try_delegate_to_host(
    ctx: &HostContext,
    tool_name: &str,
    args: &Value,
) -> Option<anyhow::Result<ToolResult>> {
    match tool_name {
        "read" if ctx.caps.fs_read => {
            let path_str = args.get("path").and_then(|v| v.as_str())?;
            let offset = crate::tools::lenient_usize_arg(args, "offset");
            let limit = crate::tools::lenient_usize_arg(args, "limit");
            let path = match validate_delegation_path(path_str) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            Some(delegate_read(ctx, path, path_str, offset, limit).await)
        }
        "write" if ctx.caps.fs_write => {
            let path_str = args.get("path").and_then(|v| v.as_str())?;
            let content = args.get("content").and_then(|v| v.as_str())?;
            let path = match validate_delegation_path(path_str) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            Some(delegate_write(ctx, path, path_str, content).await)
        }
        "bash" if ctx.caps.terminal => {
            let command = args.get("command").and_then(|v| v.as_str())?;
            let timeout_ms = args.get("timeout").and_then(|v| v.as_u64());
            Some(delegate_bash(ctx, command, timeout_ms).await)
        }
        _ => None,
    }
}

fn host_read_fallback_allowed(path: &std::path::Path, error: &anyhow::Error) -> bool {
    if !path.is_file() {
        return false;
    }
    let message = error.to_string().to_ascii_lowercase();
    [
        "method not found",
        "not implemented",
        "unsupported",
        "host proxy channel closed",
        "host proxy reply dropped",
        "client connection not ready",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

async fn delegate_read(
    ctx: &HostContext,
    path: PathBuf,
    path_str: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> anyhow::Result<ToolResult> {
    // Prefer the host when available so editor clients can apply their own
    // workspace/security semantics. Fall back only when the host capability is
    // unavailable at runtime; permission denials and ordinary host read errors
    // remain authoritative and must not be bypassed with a local read.
    let (content, delegated, fallback_reason) = match ctx.proxy.read_text_file(path.clone()).await {
        Ok(content) => (content, true, None),
        Err(host_err) if host_read_fallback_allowed(&path, &host_err) => {
            let fallback_reason = host_err.to_string();
            let content = std::fs::read_to_string(&path).map_err(|local_err| {
                anyhow::anyhow!(
                    "host read was unavailable for {path_str}: {host_err}; local fallback failed: {local_err}"
                )
            })?;
            tracing::warn!(
                path = %path.display(),
                error = %host_err,
                "ACP host read unavailable; used local fallback"
            );
            (content, false, Some(fallback_reason))
        }
        Err(host_err) => return Err(host_err),
    };
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let start = offset.unwrap_or(1).saturating_sub(1);
    let max = limit.unwrap_or(READ_MAX_LINES).min(READ_MAX_LINES);

    let selected: Vec<&str> = lines.iter().skip(start).take(max).copied().collect();
    let mut text = selected.join("\n");

    if text.len() > READ_MAX_BYTES {
        text.truncate(text.floor_char_boundary(READ_MAX_BYTES));
        if let Some(last_newline) = text.rfind('\n') {
            text.truncate(last_newline);
        }
    }

    let shown_lines = text.lines().count();
    let remaining = total_lines.saturating_sub(start + shown_lines);

    if remaining > 0 {
        text.push_str(&format!(
            "\n\n[{remaining} more lines in file. Use offset={} to continue.]",
            start + shown_lines + 1
        ));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: serde_json::json!({
            "path": path_str,
            "totalLines": total_lines,
            "shownLines": shown_lines,
            "offset": start + 1,
            "delegated": delegated,
            "fallbackReason": fallback_reason,
        }),
    })
}

async fn delegate_write(
    ctx: &HostContext,
    path: PathBuf,
    path_str: &str,
    content: &str,
) -> anyhow::Result<ToolResult> {
    let permission = ctx
        .proxy
        .request_permission(
            format!("write:{}", path.display()),
            "write".to_string(),
            path_str.to_string(),
        )
        .await?;
    if !matches!(
        permission,
        RequestPermissionOutcome::Selected(sel) if sel.option_id.0.as_ref() == "allow_once" || sel.option_id.0.as_ref() == "allow_always"
    ) {
        anyhow::bail!("write denied by ACP host for {path_str}");
    }

    let host_result = ctx
        .proxy
        .write_text_file(path.clone(), content.to_string())
        .await;
    if let Err(host_err) = host_result {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|local_err| {
                anyhow::anyhow!(
                    "host write failed for {path_str}: {host_err}; local parent creation failed: {local_err}"
                )
            })?;
        }
        std::fs::write(&path, content).map_err(|local_err| {
            anyhow::anyhow!(
                "host write failed for {path_str}: {host_err}; local fallback failed: {local_err}"
            )
        })?;
    }

    let line_count = content.lines().count();
    let byte_count = content.len();
    let result_text = format!("Wrote {path_str} ({line_count} lines, {byte_count} bytes)");

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: result_text }],
        details: serde_json::json!({
            "path": path_str,
            "lines": line_count,
            "bytes": byte_count,
            "delegated": true,
        }),
    })
}

async fn delegate_bash(
    ctx: &HostContext,
    command: &str,
    timeout_ms: Option<u64>,
) -> anyhow::Result<ToolResult> {
    let output_byte_limit = Some(50 * 1024u64);
    let terminal_id = ctx
        .proxy
        .create_terminal(
            "bash".into(),
            vec!["-c".into(), command.into()],
            None,
            output_byte_limit,
        )
        .await?;

    let timeout_dur = timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(600));

    // Use wait_for_terminal_exit instead of polling — the host blocks until
    // the command finishes, which is both faster and cheaper than polling.
    let exit_result = tokio::time::timeout(timeout_dur, async {
        ctx.proxy.wait_for_terminal_exit(terminal_id.clone()).await
    })
    .await;

    let (output, exit_code, timed_out) = match exit_result {
        Ok(Ok(_exit_resp)) => {
            let out = ctx.proxy.terminal_output(terminal_id.clone()).await;
            let _ = ctx.proxy.release_terminal(terminal_id).await;
            let resp = out.unwrap_or_else(|_| TerminalOutputResponse::new("", false));
            let code = resp.exit_status.as_ref().and_then(|s| s.exit_code);
            (resp.output, code, false)
        }
        Ok(Err(e)) => {
            let _ = ctx.proxy.release_terminal(terminal_id).await;
            return Err(e);
        }
        Err(_elapsed) => {
            let _ = ctx.proxy.kill_terminal(terminal_id.clone()).await;
            let out = ctx.proxy.terminal_output(terminal_id.clone()).await;
            let _ = ctx.proxy.release_terminal(terminal_id).await;
            let text = out.map(|r| r.output).unwrap_or_default();
            (text, None, true)
        }
    };

    let mut text = output;
    if timed_out {
        text.push_str(&format!(
            "\n[timed out after {}ms]",
            timeout_ms.unwrap_or(600_000)
        ));
    } else if let Some(code) = exit_code
        && code != 0
    {
        text.push_str(&format!("\n[exit code: {code}]"));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: serde_json::json!({
            "exit_code": exit_code,
            "timed_out": timed_out,
            "delegated": true,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::host_read_fallback_allowed;

    #[test]
    fn host_read_fallback_is_limited_to_unavailable_capabilities() {
        let file = tempfile::NamedTempFile::new().unwrap();

        assert!(host_read_fallback_allowed(
            file.path(),
            &anyhow::anyhow!("method not found: fs/read_text_file")
        ));
        assert!(host_read_fallback_allowed(
            file.path(),
            &anyhow::anyhow!("host proxy channel closed")
        ));
        assert!(!host_read_fallback_allowed(
            file.path(),
            &anyhow::anyhow!("permission denied by host")
        ));
        assert!(!host_read_fallback_allowed(
            file.path(),
            &anyhow::anyhow!("read failed: EACCES")
        ));
    }

    #[test]
    fn host_read_fallback_requires_a_local_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!host_read_fallback_allowed(
            dir.path(),
            &anyhow::anyhow!("unsupported")
        ));
        assert!(!host_read_fallback_allowed(
            &dir.path().join("missing.txt"),
            &anyhow::anyhow!("unsupported")
        ));
    }
}

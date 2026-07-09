//! Interactive background terminal sessions.
//!
//! This is the middle ground between `bash` and `serve`: a bounded,
//! permission-gated process session that can accept later stdin and expose a
//! live/tail transcript without blocking the agent turn on a single command.

use anyhow::{Context, Result, anyhow};
use omegon_traits::{ContentBlock, ToolResult};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{WorkspaceBoundary, bash};
use crate::tools::permissions::{
    PathWarning, WorkspaceRelation, classify_privilege_intent, resolve_intent_target,
    suspicious_low_confidence_shell_path,
};

const MAX_TAIL_BYTES: usize = 64 * 1024;
const DEFAULT_READ_BYTES: usize = 16 * 1024;
const MAX_SESSIONS: usize = 8;
const MAX_COMMAND_BYTES: usize = 8 * 1024;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_TRANSCRIPT_BYTES: u64 = 10 * 1024 * 1024;

fn terminal_shell() -> &'static str {
    if Path::new("/bin/bash").exists() {
        "/bin/bash"
    } else if Path::new("/usr/bin/bash").exists() {
        "/usr/bin/bash"
    } else {
        "bash"
    }
}

static TERMINALS: OnceLock<Mutex<HashMap<String, Arc<TerminalSession>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<TerminalSession>>> {
    TERMINALS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct TerminalSession {
    id: String,
    name: String,
    command: String,
    cwd: PathBuf,
    pid: u32,
    started: Instant,
    transcript_path: PathBuf,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    writer: Mutex<Box<dyn Write + Send>>,
    _master: Mutex<Box<dyn MasterPty + Send>>,
    tail: Mutex<String>,
    exit_recorded: Mutex<bool>,
    transcript_truncated: Mutex<bool>,
}

/// Input for host-action terminal creation. Unlike the direct terminal tool,
/// this is argv-based and never routes through `bash -lc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTerminalCreateRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub name: Option<String>,
}

/// Result of a host-action terminal creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTerminalCreateResponse {
    pub terminal_id: String,
    pub backend: String,
    pub actual_placement: String,
    pub warnings: Vec<String>,
    pub transcript: String,
    pub tail: String,
    pub inspect_hint: String,
}

pub fn host_terminal_runtime_available() -> Result<(), String> {
    runtime_available()
}

pub async fn start_host_terminal(
    request: HostTerminalCreateRequest,
) -> Result<HostTerminalCreateResponse, String> {
    runtime_available()?;

    let name = request
        .name
        .as_deref()
        .map(sanitize_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify_command(&request.command));

    prune_exited_named(&name);

    if running_session_count() >= MAX_SESSIONS {
        return Err(format!(
            "too many terminal sessions are open (max {MAX_SESSIONS})"
        ));
    }

    if find_session(&name).is_some() {
        return Err(format!(
            "terminal '{name}' already exists; stop it before reusing the name"
        ));
    }

    let transcript_dir = terminal_dir();
    std::fs::create_dir_all(&transcript_dir).map_err(|err| err.to_string())?;
    secure_dir_permissions(&transcript_dir).map_err(|err| err.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let transcript_path = transcript_dir.join(format!("{name}-{id}.log"));
    std::fs::write(
        &transcript_path,
        format!(
            "$ {} {}\n# cwd: {}\n# started: {}\n\n",
            request.command,
            request.args.join(" "),
            request.cwd.display(),
            chrono::Utc::now().to_rfc3339()
        ),
    )
    .map_err(|err| err.to_string())?;
    secure_file_permissions(&transcript_path).map_err(|err| err.to_string())?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("failed to allocate terminal pty: {err}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("failed to clone terminal pty reader: {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("failed to take terminal pty writer: {err}"))?;

    let mut cmd = CommandBuilder::new(&request.command);
    cmd.args(request.args.iter().map(String::as_str));
    cmd.cwd(request.cwd.as_os_str());
    for (key, value) in request.env {
        cmd.env(key, value);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| format!("failed to start terminal command: {err}"))?;
    let pid = child.process_id().unwrap_or(0);
    drop(pair.slave);

    let session = Arc::new(TerminalSession {
        id: id.clone(),
        name: name.clone(),
        command: format!("{} {}", request.command, request.args.join(" ")),
        cwd: request.cwd,
        pid,
        started: Instant::now(),
        transcript_path: transcript_path.clone(),
        child: Mutex::new(child),
        writer: Mutex::new(writer),
        _master: Mutex::new(pair.master),
        tail: Mutex::new(String::new()),
        exit_recorded: Mutex::new(false),
        transcript_truncated: Mutex::new(false),
    });

    spawn_reader(session.clone(), reader);
    registry().lock().unwrap().insert(id.clone(), session);

    let inspect_hint = format!(
        "Use terminal.read with session_id={id} to inspect output, terminal.stop with session_id={id} to stop it, or open transcript {}.",
        transcript_path.display()
    );

    Ok(HostTerminalCreateResponse {
        terminal_id: id,
        backend: "portable_pty".to_string(),
        actual_placement: "background_session".to_string(),
        warnings: Vec::new(),
        transcript: transcript_path.display().to_string(),
        tail: String::new(),
        inspect_hint,
    })
}

pub async fn execute(
    action: &str,
    args: &serde_json::Value,
    cwd: &Path,
    boundary: Option<WorkspaceBoundary>,
) -> Result<ToolResult> {
    if !enabled_by_env() {
        return err(
            "Terminal tool is disabled by OMEGON_TERMINAL_TOOL/OMEGON_DISABLE_TERMINAL_TOOL."
                .into(),
        );
    }

    match action {
        "start" => start(args, cwd, boundary.as_ref()).await,
        "send" => send(args).await,
        "read" => read(args).await,
        "stop" => stop(args).await,
        "list" => list().await,
        _ => err(format!(
            "Unknown action: {action}. Valid: start, send, read, stop, list"
        )),
    }
}

pub fn enabled_by_env() -> bool {
    if truthy_env("OMEGON_DISABLE_TERMINAL_TOOL") {
        return false;
    }
    std::env::var("OMEGON_TERMINAL_TOOL")
        .map(|value| !matches_disabled(&value))
        .unwrap_or(true)
}

pub fn runtime_available() -> Result<(), String> {
    if !enabled_by_env() {
        return Err("disabled by OMEGON_TERMINAL_TOOL/OMEGON_DISABLE_TERMINAL_TOOL".into());
    }

    let transcript_dir = terminal_dir();
    std::fs::create_dir_all(&transcript_dir)
        .map_err(|e| format!("terminal transcript directory is not writable: {e}"))?;
    secure_dir_permissions(&transcript_dir)
        .map_err(|e| format!("terminal transcript directory permissions failed: {e}"))?;

    native_pty_system()
        .openpty(PtySize {
            rows: 1,
            cols: 1,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map(|_| ())
        .map_err(|e| format!("PTY allocation failed; check /dev/pts in the container: {e}"))
}

async fn start(
    args: &serde_json::Value,
    cwd: &Path,
    boundary: Option<&WorkspaceBoundary>,
) -> Result<ToolResult> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("'command' is required"))?;

    if command.len() > MAX_COMMAND_BYTES {
        return err(format!(
            "Terminal command is too large ({} bytes; max {MAX_COMMAND_BYTES}).",
            command.len()
        ));
    }

    if let Some(blocked) = blocked_interactive_command(command) {
        return Ok(blocked_interactive_result(&blocked));
    }

    if let Some(boundary) = boundary {
        let intents = bash::extract_shell_fs_intents(command);
        let mut violations = Vec::new();
        for intent in &intents {
            let resolved = resolve_intent_target(intent, cwd, boundary);
            if matches!(
                resolved.relation,
                WorkspaceRelation::InsideWorkspace
                    | WorkspaceRelation::TrustedExternal
                    | WorkspaceRelation::SpecialAllowed
            ) {
                continue;
            }
            if suspicious_low_confidence_shell_path(intent, &resolved) {
                return Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "BLOCKED: suspicious terminal filesystem intent\n\nPath: {}\nOperation: {:?}\nSource: {}\nConfidence: {:?}\n{}\n\nThis low-confidence shell extraction looks malformed or truncated. Rewrite the command with an explicit valid path.",
                            intent.raw_path(),
                            intent.operation,
                            intent.source.description(),
                            intent.confidence,
                            terminal_warning_text(&resolved),
                        ),
                    }],
                    details: json!({
                        "is_error": true,
                        "blocked": true,
                        "reason": "suspicious_filesystem_intent",
                        "path": intent.raw_path(),
                    }),
                });
            }
            let warning_text = terminal_warning_text(&resolved);
            violations.push(if warning_text.is_empty() {
                intent.raw_path().to_string()
            } else {
                format!(
                    "{}\n    {}",
                    intent.raw_path(),
                    warning_text.replace('\n', "\n    ")
                )
            });
        }
        if !violations.is_empty() {
            return Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!(
                        "BLOCKED: terminal command targets paths outside the workspace boundary:\n{}",
                        violations
                            .iter()
                            .map(|v| format!("  - {v}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                }],
                details: json!({
                    "is_error": true,
                    "blocked": true,
                    "reason": "workspace_boundary_violation",
                    "paths": violations,
                }),
            });
        }
    }

    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(sanitize_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify_command(command));

    prune_exited_named(&name);

    if running_session_count() >= MAX_SESSIONS {
        return err(format!(
            "Too many terminal sessions are open (max {MAX_SESSIONS}). Stop an existing session first."
        ));
    }

    if find_session(&name).is_some() {
        return err(format!(
            "Terminal '{name}' already exists. Stop it before reusing the name."
        ));
    }

    let transcript_dir = terminal_dir();
    std::fs::create_dir_all(&transcript_dir)?;
    secure_dir_permissions(&transcript_dir)?;

    let id = uuid::Uuid::new_v4().to_string();
    let transcript_path = transcript_dir.join(format!("{name}-{id}.log"));
    std::fs::write(
        &transcript_path,
        format!(
            "$ {command}\n# cwd: {}\n# started: {}\n\n",
            cwd.display(),
            chrono::Utc::now().to_rfc3339()
        ),
    )?;
    secure_file_permissions(&transcript_path)?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("failed to allocate terminal pty")?;
    let reader = pair
        .master
        .try_clone_reader()
        .context("failed to clone terminal pty reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("failed to take terminal pty writer")?;

    let mut cmd = CommandBuilder::new(terminal_shell());
    cmd.args(["-lc", command]);
    cmd.cwd(cwd.as_os_str());
    for (key, value) in bash::git_discovery_env(cwd) {
        cmd.env(key, value);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .with_context(|| format!("failed to start terminal command: {command}"))?;
    let pid = child.process_id().unwrap_or(0);
    drop(pair.slave);

    let session = Arc::new(TerminalSession {
        id: id.clone(),
        name: name.clone(),
        command: command.to_string(),
        cwd: cwd.to_path_buf(),
        pid,
        started: Instant::now(),
        transcript_path: transcript_path.clone(),
        child: Mutex::new(child),
        writer: Mutex::new(writer),
        _master: Mutex::new(pair.master),
        tail: Mutex::new(String::new()),
        exit_recorded: Mutex::new(false),
        transcript_truncated: Mutex::new(false),
    });

    spawn_reader(session.clone(), reader);

    registry().lock().unwrap().insert(id.clone(), session);

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Started terminal '{name}' ({id}) PID {pid}\nCommand: {command}\nTranscript: {}",
                transcript_path.display()
            ),
        }],
        details: json!({
            "session_id": id,
            "name": name,
            "pid": pid,
            "command": command,
            "transcript": transcript_path.display().to_string(),
            "status": "running",
        }),
    })
}

async fn send(args: &serde_json::Value) -> Result<ToolResult> {
    let session = requested_session(args)?;
    let input = args
        .get("input")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("'input' is required"))?;
    let append_newline = args
        .get("newline")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if !session_alive(&session) {
        return err(format!("Terminal '{}' is not running.", session.name));
    }

    if input.len() > MAX_INPUT_BYTES {
        return err(format!(
            "Terminal input is too large ({} bytes; max {MAX_INPUT_BYTES}).",
            input.len()
        ));
    }

    let bytes = if append_newline && !input.ends_with('\n') {
        format!("{input}\n").into_bytes()
    } else {
        input.as_bytes().to_vec()
    };
    {
        let mut writer = session.writer.lock().unwrap();
        writer.write_all(&bytes)?;
        writer.flush()?;
    }
    append_transcript_marker(
        &session,
        &format!(
            "sent {} byte(s) to terminal stdin at {}",
            bytes.len(),
            chrono::Utc::now().to_rfc3339()
        ),
    );

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Sent {} byte(s) to terminal '{}' ({})\nTranscript: {}",
                bytes.len(),
                session.name,
                session.id,
                session.transcript_path.display()
            ),
        }],
        details: json!({
            "session_id": session.id,
            "name": session.name,
            "bytes": bytes.len(),
            "status": status_label(&session),
        }),
    })
}

async fn read(args: &serde_json::Value) -> Result<ToolResult> {
    let session = requested_session(args)?;
    let max_bytes = args
        .get("max_bytes")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_READ_BYTES)
        .clamp(1, MAX_TAIL_BYTES);
    let tail = session.tail.lock().unwrap().clone();
    let text = tail_bytes(&tail, max_bytes);
    let status = status_label(&session);

    let display_tail = if text.is_empty() {
        "(no output yet)".to_string()
    } else {
        text.clone()
    };

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Terminal '{}' ({}) — {status}\nTranscript: {}\n\n{}",
                session.name,
                session.id,
                session.transcript_path.display(),
                display_tail
            ),
        }],
        details: json!({
            "session_id": session.id,
            "name": session.name,
            "pid": session.pid,
            "status": status,
            "transcript": session.transcript_path.display().to_string(),
            "tail": text,
        }),
    })
}

async fn stop(args: &serde_json::Value) -> Result<ToolResult> {
    let session = requested_session(args)?;
    let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
    let was_running = session_alive(&session);
    if was_running {
        let _ = session.child.lock().unwrap().kill();
        std::thread::sleep(Duration::from_millis(150));
    }
    append_transcript_marker(
        &session,
        &format!(
            "stopped terminal at {} (force: {force}, was_running: {was_running})",
            chrono::Utc::now().to_rfc3339()
        ),
    );
    registry().lock().unwrap().remove(&session.id);

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Stopped terminal '{}' ({}){}.\nTranscript: {}",
                session.name,
                session.id,
                if was_running { "" } else { " (already exited)" },
                session.transcript_path.display()
            ),
        }],
        details: json!({
            "session_id": session.id,
            "name": session.name,
            "was_running": was_running,
            "transcript": session.transcript_path.display().to_string(),
        }),
    })
}

async fn list() -> Result<ToolResult> {
    let sessions: Vec<_> = registry().lock().unwrap().values().cloned().collect();
    if sessions.is_empty() {
        return ok("No terminal sessions.".into());
    }

    let mut lines = Vec::new();
    let mut details = Vec::new();
    for session in sessions {
        let status = status_label(&session);
        let age = session.started.elapsed().as_secs();
        lines.push(format!(
            "  {:<16} {:<8} pid {:<8} {:>4}s {} — transcript: {}",
            session.name,
            status,
            session.pid,
            age,
            session.command,
            session.transcript_path.display()
        ));
        details.push(json!({
            "session_id": session.id,
            "name": session.name,
            "pid": session.pid,
            "status": status,
            "command": session.command,
            "cwd": session.cwd.display().to_string(),
            "transcript": session.transcript_path.display().to_string(),
        }));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!("Terminal sessions:\n{}", lines.join("\n")),
        }],
        details: json!({ "sessions": details }),
    })
}

pub fn cleanup_session_terminals() {
    let sessions: Vec<_> = registry().lock().unwrap().values().cloned().collect();
    for session in sessions {
        if session_alive(&session) {
            let _ = session.child.lock().unwrap().kill();
        }
    }
    registry().lock().unwrap().clear();
}

fn requested_session(args: &serde_json::Value) -> Result<Arc<TerminalSession>> {
    let id = args
        .get("session_id")
        .or_else(|| args.get("id"))
        .or_else(|| args.get("name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("'session_id' or 'name' is required"))?;
    find_session(id).ok_or_else(|| anyhow!("No terminal session found for '{id}'"))
}

fn find_session(id_or_name: &str) -> Option<Arc<TerminalSession>> {
    let sessions = registry().lock().unwrap();
    sessions
        .get(id_or_name)
        .cloned()
        .or_else(|| sessions.values().find(|s| s.name == id_or_name).cloned())
}

fn session_alive(session: &Arc<TerminalSession>) -> bool {
    let mut child = session.child.lock().unwrap();
    match child.try_wait().ok().flatten() {
        Some(status) => {
            record_exit_status(session, &status);
            false
        }
        None => true,
    }
}

fn status_label(session: &Arc<TerminalSession>) -> &'static str {
    if session_alive(session) {
        "running"
    } else {
        "exited"
    }
}

fn prune_exited_named(name: &str) {
    registry().lock().unwrap().retain(|_, session| {
        if session.name == name {
            session_alive(session)
        } else {
            true
        }
    });
}

fn running_session_count() -> usize {
    registry()
        .lock()
        .unwrap()
        .values()
        .filter(|session| session_alive(session))
        .count()
}

fn spawn_reader<R>(session: Arc<TerminalSession>, mut reader: R)
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                    append_output(&session, &chunk);
                }
                Err(_) => break,
            }
        }
    });
}

fn append_output(session: &TerminalSession, chunk: &str) {
    let chunk = bash::strip_terminal_noise(chunk);
    append_transcript_bytes(session, chunk.as_bytes());
    let mut tail = session.tail.lock().unwrap();
    tail.push_str(&chunk);
    if tail.len() > MAX_TAIL_BYTES {
        let keep_from = tail.len().saturating_sub(MAX_TAIL_BYTES);
        let trimmed = tail
            .char_indices()
            .find(|(idx, _)| *idx >= keep_from)
            .map(|(idx, _)| idx)
            .unwrap_or(keep_from);
        tail.drain(..trimmed);
    }
}

fn append_transcript_marker(session: &TerminalSession, marker: &str) {
    let marker = format!("\n# omegon: {marker}\n");
    append_transcript_bytes(session, marker.as_bytes());

    let mut tail = session.tail.lock().unwrap();
    tail.push_str(&marker);
    if tail.len() > MAX_TAIL_BYTES {
        let keep_from = tail.len().saturating_sub(MAX_TAIL_BYTES);
        let trimmed = tail
            .char_indices()
            .find(|(idx, _)| *idx >= keep_from)
            .map(|(idx, _)| idx)
            .unwrap_or(keep_from);
        tail.drain(..trimmed);
    }
}

fn append_transcript_bytes(session: &TerminalSession, bytes: &[u8]) {
    let current_len = std::fs::metadata(&session.transcript_path)
        .map(|m| m.len())
        .unwrap_or(0);
    if current_len >= MAX_TRANSCRIPT_BYTES {
        let mut truncated = session.transcript_truncated.lock().unwrap();
        if *truncated {
            return;
        }
        *truncated = true;
        let marker = format!(
            "\n# omegon: transcript reached {} byte limit at {}; further output omitted\n",
            MAX_TRANSCRIPT_BYTES,
            chrono::Utc::now().to_rfc3339()
        );
        append_transcript_unchecked(session, marker.as_bytes());
        return;
    }

    let remaining = (MAX_TRANSCRIPT_BYTES - current_len) as usize;
    let write_len = bytes.len().min(remaining);
    if write_len > 0 {
        append_transcript_unchecked(session, &bytes[..write_len]);
    }
    if write_len < bytes.len() {
        let mut truncated = session.transcript_truncated.lock().unwrap();
        if !*truncated {
            *truncated = true;
            let marker = format!(
                "\n# omegon: transcript reached {} byte limit at {}; further output omitted\n",
                MAX_TRANSCRIPT_BYTES,
                chrono::Utc::now().to_rfc3339()
            );
            append_transcript_unchecked(session, marker.as_bytes());
        }
    }
}

fn append_transcript_unchecked(session: &TerminalSession, bytes: &[u8]) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&session.transcript_path)
    {
        let _ = file.write_all(bytes);
    }
}

fn record_exit_status(session: &TerminalSession, status: &portable_pty::ExitStatus) {
    let mut recorded = session.exit_recorded.lock().unwrap();
    if *recorded {
        return;
    }
    *recorded = true;
    let suffix = if let Some(signal) = status.signal() {
        format!("signal {signal}")
    } else {
        format!("exit code {}", status.exit_code())
    };
    append_transcript_marker(
        session,
        &format!(
            "terminal exited with {suffix} at {}",
            chrono::Utc::now().to_rfc3339()
        ),
    );
}

fn terminal_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("omegon")
        .join("terminal")
}

#[cfg(unix)]
fn secure_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn secure_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn ok(text: String) -> Result<ToolResult> {
    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: json!({}),
    })
}

fn err(text: String) -> Result<ToolResult> {
    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: json!({ "is_error": true }),
    })
}

fn terminal_warning_text(resolved: &crate::tools::permissions::ResolvedFsTarget) -> String {
    let mut lines = Vec::new();
    for warning in &resolved.warnings {
        match warning {
            PathWarning::RootDotPath { suggested_workspace_relative } => lines.push(format!(
                "Warning: `{}` is host-absolute and looks like workspace-relative `{}` with an accidental leading slash.",
                resolved.raw, suggested_workspace_relative
            )),
            PathWarning::ShortRootPath => lines.push(format!(
                "Warning: `{}` is a short root path that may be a malformed or truncated token.",
                resolved.raw
            )),
            PathWarning::DynamicShellPath => lines.push(format!(
                "Warning: `{}` contains shell expansion or substitution, so the runtime path cannot be proven before execution.",
                resolved.raw
            )),
            PathWarning::WindowsDriveRelative => lines.push(format!(
                "Warning: `{}` is Windows drive-relative (`C:foo`), not drive-absolute (`C:\\foo`).",
                resolved.raw
            )),
            PathWarning::WindowsDriveAbsolutePath => lines.push(format!(
                "Warning: `{}` is a Windows drive-absolute path; it requires exact host-boundary approval and is not treated as workspace-relative.",
                resolved.raw
            )),
            PathWarning::WindowsRootRelative => lines.push(format!(
                "Warning: `{}` is Windows root-relative on the current drive and is not treated as workspace-relative.",
                resolved.raw
            )),
            PathWarning::WindowsVerbatimPath => lines.push(format!(
                "Warning: `{}` is a Windows absolute/verbatim path; it requires exact host-boundary approval and is not treated as workspace-relative.",
                resolved.raw
            )),
            PathWarning::WindowsUncPath => lines.push(format!(
                "Warning: `{}` is a Windows UNC/network path; it requires exact host-boundary approval.",
                resolved.raw
            )),
            PathWarning::WindowsDeviceName => lines.push(format!(
                "Warning: `{}` is a Windows device name and is blocked unless explicitly supported.",
                resolved.raw
            )),
            PathWarning::WslWindowsDriveMount => lines.push(format!(
                "Warning: `{}` is a WSL Windows-drive mount path; verify whether you intended Linux workspace storage or Windows host storage.",
                resolved.raw
            )),
            PathWarning::MsysWindowsDriveMount => lines.push(format!(
                "Warning: `{}` is an MSYS/Git-Bash Windows-drive mount path; verify whether you intended POSIX workspace storage or Windows host storage.",
                resolved.raw
            )),
            PathWarning::CygwinWindowsDriveMount => lines.push(format!(
                "Warning: `{}` is a Cygwin Windows-drive mount path; verify whether you intended POSIX workspace storage or Windows host storage.",
                resolved.raw
            )),
            other => lines.push(format!(
                "Warning: `{}` has permission context warning: {:?}.",
                resolved.raw, other
            )),
        }
    }
    if !resolved.risks.is_empty() {
        lines.push(format!("Risks: {:?}", resolved.risks));
    }
    lines.join("\n")
}

fn sanitize_name(name: &str) -> String {
    name.replace(['/', '\\'], "")
        .replace("..", "")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.' || *c == '_')
        .take(64)
        .collect::<String>()
}

fn slugify_command(cmd: &str) -> String {
    let slug = cmd
        .split_whitespace()
        .find(|w| !w.starts_with('-') && *w != "bash" && *w != "sh")
        .unwrap_or("terminal")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
        .take(16)
        .collect::<String>()
        .to_lowercase();
    if slug.is_empty() {
        "terminal".to_string()
    } else {
        slug
    }
}

fn tail_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let start = text.len() - max_bytes;
    let start = text
        .char_indices()
        .find(|(idx, _)| *idx >= start)
        .map(|(idx, _)| idx)
        .unwrap_or(start);
    text[start..].to_string()
}

fn blocked_interactive_command(command: &str) -> Option<String> {
    let trimmed = command.trim_start();
    if let Some(privilege) = classify_privilege_intent(trimmed) {
        return truthy_env("OMEGON_DENY_PRIVILEGE_ESCALATION").then(|| {
            format!(
                "{} privilege escalation (blocked by OMEGON_DENY_PRIVILEGE_ESCALATION)",
                privilege.program_name()
            )
        });
    }
    if let Some(blocked) = credential_command_token(trimmed) {
        return Some(blocked);
    }
    let command = command_after_env_assignments(trimmed)?;
    match command.as_str() {
        "passwd" | "kinit" => Some(command),
        "ssh" if !ssh_batch_mode_enabled(trimmed) => Some("ssh".into()),
        _ => None,
    }
}

fn command_after_env_assignments(trimmed: &str) -> Option<String> {
    let mut saw_env = false;
    let mut skip_next_env_arg = false;
    for token in trimmed.split_whitespace() {
        if skip_next_env_arg {
            skip_next_env_arg = false;
            continue;
        }

        let name = command_basename(token);
        if matches!(
            name,
            "env" | "exec" | "command" | "builtin" | "time" | "nohup"
        ) {
            saw_env = saw_env || name == "env";
            continue;
        }

        if saw_env {
            if matches!(token, "-u" | "--unset" | "-C" | "--chdir") {
                skip_next_env_arg = true;
                continue;
            }
            if token.starts_with("--unset=") || token.starts_with("--chdir=") {
                continue;
            }
            if token.starts_with('-') {
                continue;
            }
        }

        if token.contains('=') && !token.starts_with('-') {
            continue;
        }
        return Some(name.to_string());
    }
    None
}

fn command_basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

fn credential_command_token(command: &str) -> Option<String> {
    command
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '&' | '|' | '(' | ')'))
        .filter_map(|token| {
            let token = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
            (!token.is_empty()).then_some(command_basename(token))
        })
        .find_map(|name| match name {
            "sudo" | "doas" | "su" => None,
            "passwd" | "kinit" => Some(name.to_string()),
            "ssh" if !ssh_batch_mode_enabled(command) => Some("ssh".into()),
            _ => None,
        })
}

fn ssh_batch_mode_enabled(command: &str) -> bool {
    command.contains("BatchMode=yes")
        || command.contains("BatchMode yes")
        || command.contains("-oBatchMode=yes")
}

fn blocked_interactive_result(command_name: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Blocked: `{command_name}` may request credentials or attach to a remote interactive session. \
                 Use `wait_for_operator` for manual checkpoints, or run a non-credential command."
            ),
        }],
        details: json!({
            "is_error": true,
            "blocked": true,
            "reason": "credential_prompt_risk",
        }),
    }
}

fn truthy_env(key: &str) -> bool {
    std::env::var(key)
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn matches_disabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off" | "disabled" | "disable"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(result: &ToolResult) -> String {
        result
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    async fn wait_for_terminal_text(cwd: &Path, session_id: &str, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = String::new();
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let read_result = execute("read", &json!({"session_id": session_id}), cwd, None)
                .await
                .unwrap();
            observed = text(&read_result);
            if observed.contains(needle) {
                break;
            }
        }
        observed
    }

    async fn wait_for_file_text(path: &Path, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observed = String::new();
        while Instant::now() < deadline {
            observed = std::fs::read_to_string(path).unwrap_or_default();
            if observed.contains(needle) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        observed
    }

    async fn wait_for_session_exit(session_id: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !session_alive(&requested_session(&json!({"session_id": session_id})).unwrap()) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    #[tokio::test]
    async fn terminal_can_send_input_and_read_output() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "terminal-test-echo",
                "command": "read line; echo got:$line"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();
        let id = result.details["session_id"].as_str().unwrap().to_string();
        let transcript = result.details["transcript"].as_str().unwrap().to_string();

        execute(
            "send",
            &json!({"session_id": id, "input": "hello"}),
            cwd.path(),
            None,
        )
        .await
        .unwrap();

        let observed = wait_for_terminal_text(cwd.path(), &id, "got:hello").await;

        assert!(observed.contains("got:hello"), "output was: {observed}");
        let transcript_text = wait_for_file_text(Path::new(&transcript), "omegon: sent").await;
        assert!(
            transcript_text.contains("omegon: sent"),
            "transcript should include stdin audit marker: {transcript_text}"
        );
        let _ = execute("stop", &json!({"session_id": id}), cwd.path(), None).await;
    }

    #[tokio::test]
    async fn terminal_process_observes_tty() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "terminal-test-tty",
                "command": "[ -t 0 ] && echo tty:yes || echo tty:no"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();
        let id = result.details["session_id"].as_str().unwrap().to_string();

        let observed = wait_for_terminal_text(cwd.path(), &id, "tty:").await;

        assert!(
            observed.contains("tty:yes"),
            "PTY-backed terminal should expose stdin as a TTY: {observed}"
        );
        let _ = execute("stop", &json!({"session_id": id}), cwd.path(), None).await;
    }

    #[tokio::test]
    async fn exited_terminal_name_can_be_reused() {
        let cwd = tempfile::tempdir().unwrap();
        let args = json!({
            "name": "terminal-test-reuse",
            "command": "echo once"
        });
        let first = execute(
            "start",
            &args,
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();
        let first_id = first.details["session_id"].as_str().unwrap().to_string();

        assert!(
            wait_for_session_exit(&first_id).await,
            "first terminal session should exit before name reuse"
        );

        let second = execute(
            "start",
            &args,
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();
        assert!(
            second.details["session_id"].as_str().is_some(),
            "second start should create a fresh session: {}",
            text(&second)
        );
        let second_id = second.details["session_id"].as_str().unwrap().to_string();
        assert_ne!(first_id, second_id);
        let _ = execute("stop", &json!({"session_id": second_id}), cwd.path(), None).await;
    }

    #[tokio::test]
    async fn terminal_start_enforces_workspace_boundary_scan() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "blocked-terminal-test",
                "command": "echo no > /etc/omegon-terminal-test"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();

        assert_eq!(
            result.details.get("blocked").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(text(&result).contains("BLOCKED"));
    }

    #[tokio::test]
    async fn terminal_short_root_path_is_diagnostic_block() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "blocked-short-root-terminal-test",
                "command": "echo no > /Ig"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();

        assert_eq!(
            result.details.get("blocked").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            result.details.get("reason").and_then(|v| v.as_str()),
            Some("suspicious_filesystem_intent")
        );
        let body = text(&result);
        assert!(body.contains("/Ig"));
        assert!(body.contains("malformed or truncated"));
    }

    #[tokio::test]
    async fn terminal_root_dot_path_includes_workspace_relative_diagnostic() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "blocked-root-dot-terminal-test",
                "command": "mkdir -p /.omegon/runtime"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();

        assert_eq!(
            result.details.get("blocked").and_then(|v| v.as_bool()),
            Some(true)
        );
        let body = text(&result);
        assert!(body.contains("/.omegon/runtime"));
        assert!(body.contains(".omegon/runtime"));
        assert!(body.contains("accidental leading slash"));
    }

    #[tokio::test]
    async fn terminal_allows_temp_directory_redirect() {
        let cwd = tempfile::tempdir().unwrap();
        let temp_file = std::env::temp_dir().join(format!(
            "omegon-terminal-permission-intent-{}.log",
            uuid::Uuid::new_v4()
        ));
        let result = execute(
            "start",
            &json!({
                "name": "allowed-temp-terminal-test",
                "command": format!("echo temp-ok > {}", temp_file.display())
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();

        assert!(
            result.details["session_id"].as_str().is_some(),
            "temp redirect should be allowed: {}",
            text(&result)
        );
        let id = result.details["session_id"].as_str().unwrap().to_string();
        let _ = wait_for_session_exit(&id).await;
        let _ = std::fs::remove_file(temp_file);
    }

    #[tokio::test]
    async fn terminal_allows_trusted_directory_redirect() {
        let cwd = tempfile::tempdir().unwrap();
        let trusted = tempfile::tempdir().unwrap();
        let target = trusted.path().join("trusted.log");
        let boundary = WorkspaceBoundary::new(cwd.path().to_path_buf());
        boundary.approve_directory(trusted.path().to_path_buf());
        let result = execute(
            "start",
            &json!({
                "name": "allowed-trusted-terminal-test",
                "command": format!("echo trusted-ok > {}", target.display())
            }),
            cwd.path(),
            Some(boundary),
        )
        .await
        .unwrap();

        assert!(
            result.details["session_id"].as_str().is_some(),
            "trusted redirect should be allowed: {}",
            text(&result)
        );
        let id = result.details["session_id"].as_str().unwrap().to_string();
        let _ = wait_for_session_exit(&id).await;
    }

    #[tokio::test]
    async fn terminal_allows_standard_fd_redirect() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "allowed-fd-terminal-test",
                "command": "echo fd-ok > /dev/stdout"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();

        assert!(
            result.details["session_id"].as_str().is_some(),
            "standard fd redirect should be allowed: {}",
            text(&result)
        );
        let id = result.details["session_id"].as_str().unwrap().to_string();
        let _ = wait_for_session_exit(&id).await;
    }

    #[test]
    fn terminal_allows_operator_mediated_privilege_commands_by_default() {
        for command in [
            "sudo true",
            "/usr/bin/sudo true",
            "env FOO=bar sudo true",
            "/usr/bin/env -i /usr/bin/sudo true",
            "exec doas true",
            "bash -lc 'sudo true'",
        ] {
            assert_eq!(blocked_interactive_command(command), None, "{command}");
        }
    }

    #[tokio::test]
    async fn terminal_still_blocks_non_privilege_credential_prompt_commands() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "blocked-kinit-terminal-test",
                "command": "kinit"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();

        assert_eq!(
            result.details.get("blocked").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            result.details.get("reason").and_then(|v| v.as_str()),
            Some("credential_prompt_risk")
        );
    }

    #[tokio::test]
    async fn terminal_rejects_oversized_stdin() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "terminal-test-input-limit",
                "command": "cat"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();
        let id = result.details["session_id"].as_str().unwrap().to_string();
        let result = execute(
            "send",
            &json!({"session_id": id, "input": "x".repeat(MAX_INPUT_BYTES + 1)}),
            cwd.path(),
            None,
        )
        .await
        .unwrap();

        assert!(text(&result).contains("too large"));
    }

    #[tokio::test]
    async fn terminal_output_strips_control_sequences_from_tail_and_transcript() {
        let cwd = tempfile::tempdir().unwrap();
        let result = execute(
            "start",
            &json!({
                "name": "terminal-test-control-strip",
                "command": "printf 'before\\033]0;owned\\007after\\n'"
            }),
            cwd.path(),
            Some(WorkspaceBoundary::new(cwd.path().to_path_buf())),
        )
        .await
        .unwrap();
        let id = result.details["session_id"].as_str().unwrap().to_string();
        let transcript = result.details["transcript"].as_str().unwrap().to_string();

        let mut observed = String::new();
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let read_result = execute("read", &json!({"session_id": id}), cwd.path(), None)
                .await
                .unwrap();
            observed = text(&read_result);
            if observed.contains("beforeafter") {
                break;
            }
        }

        assert!(observed.contains("beforeafter"), "output was: {observed}");
        assert!(!observed.contains("\x1b]0;owned"));
        let transcript_text = std::fs::read_to_string(&transcript).unwrap();
        assert!(!transcript_text.contains("\x1b]0;owned"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_transcript_is_owner_private() {
        use std::os::unix::fs::PermissionsExt;

        let transcript_dir = terminal_dir();
        std::fs::create_dir_all(&transcript_dir).unwrap();
        secure_dir_permissions(&transcript_dir).unwrap();
        let transcript =
            transcript_dir.join(format!("terminal-test-perms-{}.log", uuid::Uuid::new_v4()));
        std::fs::write(&transcript, "test transcript\n").unwrap();
        secure_file_permissions(&transcript).unwrap();
        let file_mode = std::fs::metadata(&transcript).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(transcript.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn tail_bytes_preserves_utf8_boundary() {
        assert_eq!(tail_bytes("alpha βeta", 5), "βeta");
    }

    #[test]
    fn host_terminal_request_is_argv_based() {
        let request = HostTerminalCreateRequest {
            command: "bookokrat".to_string(),
            args: vec!["/books/a.epub".to_string()],
            cwd: PathBuf::from("/workspace"),
            env: Vec::new(),
            name: None,
        };

        assert_eq!(request.command, "bookokrat");
        assert_eq!(request.args, vec!["/books/a.epub"]);
        assert_ne!(request.command, "bash");
        assert!(!request.args.iter().any(|arg| arg == "-lc"));
    }

    fn slugify_command_falls_back_when_command_has_no_safe_chars() {
        assert_eq!(slugify_command("!!!"), "terminal");
    }
}

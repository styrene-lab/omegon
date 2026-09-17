use crate::{
    ExtensionManifest, RuntimeConfig, SdkCompatibilityDiagnostic, SdkCompatibilityStatus,
    classify_initialize_metadata,
};
use anyhow::{Context, Result, anyhow};
use omegon_traits::ToolDefinition;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

const SAFE_INHERIT_ENVS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TERM",
    "SHELL",
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "LD_LIBRARY_PATH",
    "RUST_LOG",
    "RUST_BACKTRACE",
    "OMEGON_PROJECT_ROOT",
    "FLYNT_VAULT",
    "CODEX_VAULT",
];

#[derive(Debug, Clone)]
pub struct ExtensionNotification {
    pub extension_name: String,
    pub method: String,
    pub params: Value,
}

pub trait HostRequestHandler: Send + Sync {
    fn handle(&self, request: &omegon_extension::RpcRequest) -> Option<Value>;
}

pub trait ReadinessValidator: Send + Sync {
    fn validate(&self, method: &str, response: &Value) -> Result<()>;
}

#[derive(Clone)]
pub struct LaunchSpec {
    pub manifest: ExtensionManifest,
    pub extension_dir: PathBuf,
    pub project_root: Option<PathBuf>,
    pub resolved_config: Map<String, Value>,
    pub resolved_secrets: Vec<(String, String)>,
    pub source_digest: String,
    pub notification_tx: Option<mpsc::UnboundedSender<ExtensionNotification>>,
    pub host_request_handler: Option<Arc<dyn HostRequestHandler>>,
    pub readiness_validator: Option<Arc<dyn ReadinessValidator>>,
}

#[derive(Debug, Clone, Default)]
pub enum RpcRequestPolicy {
    #[default]
    HandleHostRequests,
    RejectHostRequests,
}

#[derive(Debug, Clone)]
pub struct ExtensionHandshake {
    pub tools: Vec<ToolDefinition>,
    pub metadata: Option<Value>,
    pub sdk_compatibility: SdkCompatibilityDiagnostic,
}

struct ProcessHandles {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    recent_stderr: Arc<std::sync::Mutex<String>>,
    next_id: u64,
}

impl ProcessHandles {
    fn new(
        child: tokio::process::Child,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
        recent_stderr: Arc<std::sync::Mutex<String>>,
    ) -> Self {
        Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            recent_stderr,
            next_id: 1,
        }
    }

    fn diagnostic(&mut self) -> String {
        let process = match self.child.try_wait() {
            Ok(Some(status)) => format!("process exited with {status}"),
            Ok(None) => "process still running".to_string(),
            Err(error) => format!("process status unavailable: {error}"),
        };
        let stderr = self
            .recent_stderr
            .lock()
            .map(|stderr| stderr.trim().to_string())
            .unwrap_or_else(|_| "<stderr lock poisoned>".to_string());
        if stderr.is_empty() {
            format!("{process}; stderr: none")
        } else {
            format!("{process}; stderr: {stderr}")
        }
    }

    async fn rpc_call(&mut self, spec: &LaunchSpec, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        write_json(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await?;
        loop {
            let line = read_line(&mut self.reader).await?;
            let response: Value = serde_json::from_str(&line)?;
            if let Ok(omegon_extension::RpcIncoming::Notification(notification)) =
                omegon_extension::RpcIncoming::parse(&line)
            {
                send_notification(spec, notification);
                continue;
            }
            if response.get("id").and_then(Value::as_u64) == Some(id) {
                return response_result(response);
            }
        }
    }

    async fn shutdown(&mut self, grace: std::time::Duration) -> Result<()> {
        let pid = self.child.id();
        let _ = self.stdin.shutdown().await;
        let deadline = tokio::time::Instant::now() + grace;
        loop {
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        kill_process_group(pid);
        let _ = self.child.start_kill();
        tokio::time::timeout(
            grace.max(std::time::Duration::from_millis(500)),
            self.child.wait(),
        )
        .await
        .map_err(|_| anyhow!("extension process did not exit after forced termination"))??;
        Ok(())
    }
}

impl Drop for ProcessHandles {
    fn drop(&mut self) {
        kill_process_group(self.child.id());
        let _ = self.child.start_kill();
    }
}

pub struct ExtensionSupervisor {
    spec: LaunchSpec,
    handles: Mutex<Option<ProcessHandles>>,
    accepting_calls: AtomicBool,
    shutdown_signal: CancellationToken,
    request_id: AtomicU64,
    pid: AtomicU64,
    process_state: AtomicU8,
    last_error: std::sync::Mutex<Option<String>>,
    expected_tools: Value,
}

impl ExtensionSupervisor {
    const RUNNING: u8 = 0;
    const UNAVAILABLE: u8 = 1;
    const REPLACING: u8 = 2;
    const SHUTTING_DOWN: u8 = 3;

    pub async fn launch(spec: LaunchSpec) -> Result<(Arc<Self>, ExtensionHandshake)> {
        let mut handles = spawn_process(&spec).await?;
        let handshake = match handshake(&mut handles, &spec).await {
            Ok(handshake) => handshake,
            Err(error) => {
                let _ = handles.shutdown(std::time::Duration::ZERO).await;
                return Err(error);
            }
        };
        let pid = handles.child.id().map_or(0, u64::from);
        let request_id = handles.next_id;
        let expected_tools = serde_json::to_value(&handshake.tools)?;
        Ok((
            Arc::new(Self {
                spec,
                handles: Mutex::new(Some(handles)),
                accepting_calls: AtomicBool::new(true),
                shutdown_signal: CancellationToken::new(),
                request_id: AtomicU64::new(request_id),
                pid: AtomicU64::new(pid),
                process_state: AtomicU8::new(Self::RUNNING),
                last_error: std::sync::Mutex::new(None),
                expected_tools,
            }),
            handshake,
        ))
    }

    pub fn name(&self) -> &str {
        &self.spec.manifest.extension.name
    }

    pub fn source_digest(&self) -> &str {
        &self.spec.source_digest
    }

    pub fn ensure_accepting(&self) -> Result<()> {
        if self.accepting_calls.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(anyhow!("extension '{}' is shutting down", self.name()))
        }
    }

    pub async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        self.rpc_call_with_cancel(
            method,
            params,
            CancellationToken::new(),
            Some(std::time::Duration::from_secs(120)),
            RpcRequestPolicy::HandleHostRequests,
            None,
        )
        .await
    }

    pub async fn rpc_call_with_cancel(
        &self,
        method: &str,
        params: Value,
        cancel: CancellationToken,
        idle_timeout: Option<std::time::Duration>,
        request_policy: RpcRequestPolicy,
        cancellation_params: Option<Value>,
    ) -> Result<Value> {
        self.ensure_accepting()?;
        let mut guard = tokio::select! {
            guard = self.handles.lock() => guard,
            _ = cancel.cancelled() => anyhow::bail!("extension '{}' RPC '{}' cancelled before dispatch", self.name(), method),
            _ = self.shutdown_signal.cancelled() => anyhow::bail!("extension '{}' is shutting down", self.name()),
        };
        self.ensure_accepting()?;
        if cancel.is_cancelled() {
            anyhow::bail!(
                "extension '{}' RPC '{}' cancelled before dispatch",
                self.name(),
                method
            );
        }
        let handles = guard
            .as_mut()
            .ok_or_else(|| anyhow!("extension process not running"))?;
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        write_json(
            &mut handles.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await?;
        let started = std::time::Instant::now();
        let mut last_notification: Option<String> = None;
        loop {
            let read = read_line(&mut handles.reader);
            let line = if let Some(timeout) = idle_timeout {
                tokio::select! {
                    result = tokio::time::timeout(timeout, read) => match result {
                        Ok(result) => result?,
                        Err(_) => {
                            send_cancel(&mut handles.stdin, id, cancellation_params.clone()).await?;
                            anyhow::bail!("extension '{}' RPC '{}' id {} timed out after {}ms waiting for response (last_notification={})", self.name(), method, id, started.elapsed().as_millis(), last_notification.as_deref().unwrap_or("none"));
                        }
                    },
                    _ = cancel.cancelled() => {
                        send_cancel(&mut handles.stdin, id, cancellation_params.clone()).await?;
                        anyhow::bail!("extension '{}' RPC '{}' id {} cancelled after {}ms (last_notification={})", self.name(), method, id, started.elapsed().as_millis(), last_notification.as_deref().unwrap_or("none"));
                    }
                    _ = self.shutdown_signal.cancelled() => anyhow::bail!("extension '{}' is shutting down", self.name()),
                }
            } else {
                tokio::select! {
                    result = read => result?,
                    _ = cancel.cancelled() => {
                        send_cancel(&mut handles.stdin, id, cancellation_params.clone()).await?;
                        anyhow::bail!("extension '{}' RPC '{}' cancelled", self.name(), method);
                    }
                    _ = self.shutdown_signal.cancelled() => anyhow::bail!("extension '{}' is shutting down", self.name()),
                }
            };
            let response: Value = serde_json::from_str(&line)?;
            if let Ok(incoming) = omegon_extension::RpcIncoming::parse(&line) {
                match incoming {
                    omegon_extension::RpcIncoming::Request(request) => {
                        let response = match request_policy {
                            RpcRequestPolicy::HandleHostRequests => self
                                .spec
                                .host_request_handler
                                .as_ref()
                                .and_then(|handler| handler.handle(&request))
                                .unwrap_or_else(|| method_not_found(&request)),
                            RpcRequestPolicy::RejectHostRequests => method_not_found(&request),
                        };
                        write_json(&mut handles.stdin, &response).await?;
                        continue;
                    }
                    omegon_extension::RpcIncoming::Notification(notification) => {
                        last_notification = Some(notification.method.clone());
                        send_notification(&self.spec, notification);
                        continue;
                    }
                    omegon_extension::RpcIncoming::Response(_) => {}
                }
            }
            if response.get("id").and_then(Value::as_u64) == Some(id) {
                return response_result(response);
            }
        }
    }

    pub async fn pump_notifications_for(&self, idle_timeout: std::time::Duration) -> Result<()> {
        self.ensure_accepting()?;
        let mut guard = self.handles.lock().await;
        let handles = guard
            .as_mut()
            .ok_or_else(|| anyhow!("extension process not running"))?;
        let line = tokio::select! {
            result = tokio::time::timeout(idle_timeout, read_line(&mut handles.reader)) => match result {
                Ok(result) => Some(result?),
                Err(_) => None,
            },
            _ = self.shutdown_signal.cancelled() => anyhow::bail!("extension '{}' is shutting down", self.name()),
        };
        if let Some(line) = line
            && let Ok(omegon_extension::RpcIncoming::Notification(notification)) =
                omegon_extension::RpcIncoming::parse(&line)
        {
            send_notification(&self.spec, notification);
        }
        Ok(())
    }

    pub fn health(&self) -> ExtensionProcessHealth {
        let state = self.process_state.load(Ordering::Acquire);
        if state == Self::RUNNING
            && let Ok(mut guard) = self.handles.try_lock()
            && let Some(handles) = guard.as_mut()
            && let Some(status) = handles.child.try_wait().ok().flatten()
        {
            guard.take();
            self.pid.store(0, Ordering::Release);
            self.accepting_calls.store(false, Ordering::Release);
            self.process_state
                .store(Self::UNAVAILABLE, Ordering::Release);
            *self
                .last_error
                .lock()
                .expect("extension health lock poisoned") =
                Some(format!("process exited with {status}"));
        }
        let state = self.process_state.load(Ordering::Acquire);
        ExtensionProcessHealth {
            name: self.name().to_string(),
            state: match state {
                Self::RUNNING => ExtensionProcessState::Healthy,
                Self::REPLACING => ExtensionProcessState::Replacing,
                Self::SHUTTING_DOWN => ExtensionProcessState::ShuttingDown,
                _ => ExtensionProcessState::Unavailable,
            },
            pid: u32::try_from(self.pid.load(Ordering::Acquire))
                .ok()
                .filter(|pid| *pid != 0),
            detail: self
                .last_error
                .lock()
                .expect("extension health lock poisoned")
                .clone(),
        }
    }

    pub fn mark_unavailable(&self, detail: impl Into<String>) {
        self.accepting_calls.store(false, Ordering::Release);
        self.process_state
            .store(Self::UNAVAILABLE, Ordering::Release);
        *self
            .last_error
            .lock()
            .expect("extension health lock poisoned") = Some(detail.into());
    }

    pub async fn replace(&self) -> Result<u32> {
        if self.shutdown_signal.is_cancelled()
            || self.process_state.load(Ordering::Acquire) == Self::SHUTTING_DOWN
        {
            anyhow::bail!("extension '{}' is shutting down", self.name());
        }
        let state = self.process_state.load(Ordering::Acquire);
        if state == Self::REPLACING {
            anyhow::bail!(
                "extension '{}' replacement is already in progress",
                self.name()
            );
        }
        self.process_state
            .compare_exchange(state, Self::REPLACING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                anyhow!(
                    "extension '{}' state changed before replacement",
                    self.name()
                )
            })?;
        self.accepting_calls.store(false, Ordering::Release);
        let old_pid = self.pid.swap(0, Ordering::AcqRel);
        kill_process_group(u32::try_from(old_pid).ok().filter(|pid| *pid != 0));
        let result = self.replace_locked().await;
        match &result {
            Ok(_) => {
                *self
                    .last_error
                    .lock()
                    .expect("extension health lock poisoned") = None;
                self.accepting_calls.store(true, Ordering::Release);
                self.process_state.store(Self::RUNNING, Ordering::Release);
            }
            Err(error) => {
                *self
                    .last_error
                    .lock()
                    .expect("extension health lock poisoned") = Some(error.to_string());
                self.process_state
                    .store(Self::UNAVAILABLE, Ordering::Release);
            }
        }
        result
    }

    async fn replace_locked(&self) -> Result<u32> {
        let mut guard = self.handles.lock().await;
        if let Some(mut stale) = guard.take() {
            stale.shutdown(std::time::Duration::ZERO).await?;
        }
        let mut candidate = spawn_process(&self.spec).await?;
        let handshake = match handshake(&mut candidate, &self.spec).await {
            Ok(handshake) => handshake,
            Err(error) => {
                let _ = candidate.shutdown(std::time::Duration::ZERO).await;
                return Err(error);
            }
        };
        if serde_json::to_value(&handshake.tools)? != self.expected_tools {
            let _ = candidate.shutdown(std::time::Duration::ZERO).await;
            anyhow::bail!(
                "extension '{}' replacement changed its published tool definitions",
                self.name()
            );
        }
        if self.shutdown_signal.is_cancelled() {
            let _ = candidate.shutdown(std::time::Duration::ZERO).await;
            anyhow::bail!("extension '{}' shut down during replacement", self.name());
        }
        self.request_id.store(candidate.next_id, Ordering::SeqCst);
        let pid = candidate
            .child
            .id()
            .ok_or_else(|| anyhow!("extension '{}' replacement has no process id", self.name()))?;
        self.pid.store(u64::from(pid), Ordering::Release);
        *guard = Some(candidate);
        Ok(pid)
    }

    pub async fn shutdown(&self, grace: std::time::Duration) -> Result<()> {
        self.process_state
            .store(Self::SHUTTING_DOWN, Ordering::Release);
        self.accepting_calls.store(false, Ordering::Release);
        self.shutdown_signal.cancel();
        let mut guard = match tokio::time::timeout(grace, self.handles.lock()).await {
            Ok(guard) => guard,
            Err(_) => {
                kill_process_group(
                    u32::try_from(self.pid.load(Ordering::Acquire))
                        .ok()
                        .filter(|pid| *pid != 0),
                );
                tokio::time::timeout(
                    grace.max(std::time::Duration::from_millis(100)),
                    self.handles.lock(),
                )
                .await
                .map_err(|_| {
                    anyhow!(
                        "extension '{}' RPC did not release for shutdown",
                        self.name()
                    )
                })?
            }
        };
        if let Some(mut handles) = guard.take() {
            handles.shutdown(grace).await?;
        }
        self.pid.store(0, Ordering::Release);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionProcessState {
    Healthy,
    Unavailable,
    Replacing,
    ShuttingDown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionProcessHealth {
    pub name: String,
    pub state: ExtensionProcessState,
    pub pid: Option<u32>,
    pub detail: Option<String>,
}

pub async fn shutdown_supervisors(
    supervisors: &[Arc<ExtensionSupervisor>],
    grace: std::time::Duration,
) -> Vec<String> {
    let mut failures = Vec::new();
    for supervisor in supervisors {
        if let Err(error) = supervisor.shutdown(grace).await {
            failures.push(format!("{}: {error}", supervisor.name()));
        }
    }
    failures
}

async fn handshake(handles: &mut ProcessHandles, spec: &LaunchSpec) -> Result<ExtensionHandshake> {
    let manifest = &spec.manifest;
    let name = &manifest.extension.name;
    let started = tokio::time::Instant::now();
    let startup_budget = std::time::Duration::from_millis(manifest.startup.timeout_ms.max(1));
    let deadline = started + startup_budget;
    let initialize_budget = (startup_budget / 2)
        .max(std::time::Duration::from_millis(1))
        .min(std::time::Duration::from_secs(2));
    let metadata = match tokio::time::timeout_at(
        deadline.min(started + initialize_budget),
        handles.rpc_call(spec, "initialize", json!({})),
    )
    .await
    {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            tracing::debug!(extension = name, %error, "extension initialize metadata unavailable");
            None
        }
        Err(_) => None,
    };
    let sdk_compatibility = classify_initialize_metadata(metadata.as_ref());
    if sdk_compatibility.is_blocking() {
        anyhow::bail!(
            "extension '{}' SDK contract is incompatible: {}",
            name,
            sdk_compatibility.message
        );
    }
    if sdk_compatibility.status == SdkCompatibilityStatus::MissingLegacy {
        tracing::warn!(
            extension = name,
            "extension did not advertise SDK contract version; treating as legacy"
        );
    }
    let tools_response =
        timed_handshake_call(handles, spec, deadline, "get_tools", json!({})).await?;
    let tools = normalize_tool_definitions(&tools_response).map_err(|error| {
        anyhow!("extension '{name}' returned invalid get_tools response: {error}")
    })?;
    if !spec.resolved_config.is_empty() {
        timed_handshake_call(
            handles,
            spec,
            deadline,
            "bootstrap_config",
            Value::Object(spec.resolved_config.clone()),
        )
        .await
        .map_err(|error| {
            anyhow!("extension '{name}' failed to accept bootstrap_config: {error}")
        })?;
    }
    if !spec.resolved_secrets.is_empty() {
        let secrets = spec
            .resolved_secrets
            .iter()
            .map(|(name, value)| (name.clone(), Value::String(value.clone())))
            .collect();
        timed_handshake_call(
            handles,
            spec,
            deadline,
            "bootstrap_secrets",
            Value::Object(secrets),
        )
        .await
        .map_err(|error| {
            anyhow!("extension '{name}' failed to accept bootstrap_secrets: {error}")
        })?;
    }
    if let Some(method) = manifest
        .startup
        .ping_method
        .as_deref()
        .filter(|method| *method != "get_tools")
    {
        let response = timed_handshake_call(handles, spec, deadline, method, json!({}))
            .await
            .map_err(|error| {
                anyhow!("extension '{name}' readiness probe '{method}' failed: {error}")
            })?;
        if let Some(validator) = &spec.readiness_validator {
            validator.validate(method, &response)?;
        }
    }
    Ok(ExtensionHandshake {
        tools,
        metadata,
        sdk_compatibility,
    })
}

async fn timed_handshake_call(
    handles: &mut ProcessHandles,
    spec: &LaunchSpec,
    deadline: tokio::time::Instant,
    method: &str,
    params: Value,
) -> Result<Value> {
    match tokio::time::timeout_at(deadline, handles.rpc_call(spec, method, params)).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!(
            "extension '{}' readiness timed out during {} after {}ms ({})",
            spec.manifest.extension.name,
            method,
            spec.manifest.startup.timeout_ms.max(1),
            handles.diagnostic(),
        )),
    }
}

async fn spawn_process(spec: &LaunchSpec) -> Result<ProcessHandles> {
    let manifest = &spec.manifest;
    let mut child = match &manifest.runtime {
        RuntimeConfig::Native { .. } => {
            let binary = manifest.native_binary_path(&spec.extension_dir)?;
            let mut command = clean_command(&binary, manifest)?;
            if let Some(root) = &spec.project_root {
                command.env("OMEGON_PROJECT_ROOT", root);
            }
            configure_process(&mut command);
            command
                .arg("--rpc")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        }
        RuntimeConfig::Oci { .. } => {
            let mut command = clean_command("podman", manifest)?;
            configure_process(&mut command);
            command.args(["run", "--rm", "-i"]);
            for (name, value) in resolved_runtime_env(manifest)? {
                command.args(["--env", &format!("{name}={value}")]);
            }
            if let Some(root) = &spec.project_root {
                command.args(["--env", &format!("OMEGON_PROJECT_ROOT={}", root.display())]);
            }
            command
                .arg(manifest.oci_image()?)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?
        }
    };
    let recent_stderr = Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
        drain_stderr(
            manifest.extension.name.clone(),
            stderr,
            recent_stderr.clone(),
        );
    }
    let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
    Ok(ProcessHandles::new(child, stdin, stdout, recent_stderr))
}

fn clean_command(
    program: impl AsRef<std::ffi::OsStr>,
    manifest: &ExtensionManifest,
) -> Result<tokio::process::Command> {
    let mut command = tokio::process::Command::new(program);
    command.env_clear();
    for name in SAFE_INHERIT_ENVS {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    for (name, value) in resolved_runtime_env(manifest)? {
        command.env(name, value);
    }
    Ok(command)
}

fn resolved_runtime_env(manifest: &ExtensionManifest) -> Result<Vec<(String, String)>> {
    let mut env = Vec::new();
    for (name, value) in manifest.runtime.env() {
        validate_env_name(name)?;
        env.push((name.clone(), value.clone()));
    }
    for name in manifest.runtime.env_passthrough() {
        validate_env_name(name)?;
        if let Ok(value) = std::env::var(name) {
            env.push((name.clone(), value));
        }
    }
    Ok(env)
}

fn validate_env_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_uppercase() || ch.is_ascii_digit())
        || ["SECRET", "TOKEN", "PASSWORD", "KEY"]
            .iter()
            .any(|word| name.contains(word))
    {
        anyhow::bail!(
            "runtime env var '{name}' is not allowed; manifest runtime.env is for non-secret uppercase names only"
        );
    }
    Ok(())
}

pub fn normalize_tool_definitions(value: &Value) -> Result<Vec<ToolDefinition>> {
    value
        .as_array()
        .ok_or_else(|| anyhow!("get_tools result must be an array"))?
        .iter()
        .map(normalize_tool_definition)
        .collect()
}

fn normalize_tool_definition(value: &Value) -> Result<ToolDefinition> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("tool definition must be an object"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("tool definition missing non-empty name"))?
        .to_string();
    let label = object
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.is_empty())
        .unwrap_or(&name)
        .to_string();
    let raw_description = object
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let description = if raw_description.is_empty() {
        "Extension tool. Semantics are owned by the extension, not Omegon core.".to_string()
    } else {
        format!(
            "Extension tool (not Omegon core; semantics are owned by the extension): {raw_description}"
        )
    };
    let parameters = object
        .get("parameters")
        .or_else(|| object.get("inputSchema"))
        .or_else(|| object.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    let capabilities = object
        .get("capabilities")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .with_context(|| format!("tool '{name}' has invalid capabilities"))?
        .unwrap_or_default();
    Ok(ToolDefinition {
        name,
        label,
        description,
        parameters,
        capabilities,
    })
}

fn configure_process(command: &mut tokio::process::Command) {
    command.kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
}

#[cfg(unix)]
fn kill_process_group(pid: Option<u32>) {
    let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    // SAFETY: extension commands are leaders of dedicated process groups.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: Option<u32>) {}

fn drain_stderr(
    name: String,
    stderr: tokio::process::ChildStderr,
    recent_stderr: Arc<std::sync::Mutex<String>>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) if !line.trim_end().is_empty() => {
                    record_recent_stderr(&recent_stderr, &line);
                    tracing::debug!(extension = %name, message = line.trim_end(), "extension stderr");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::debug!(extension = %name, %error, "failed to read extension stderr");
                    break;
                }
            }
        }
    });
}

fn record_recent_stderr(recent_stderr: &std::sync::Mutex<String>, line: &str) {
    if let Ok(mut recent) = recent_stderr.lock() {
        recent.push_str(line);
        if recent.len() > 4096 {
            let excess = recent.len() - 4096;
            let split = recent
                .char_indices()
                .map(|(index, _)| index)
                .find(|index| *index >= excess)
                .unwrap_or(recent.len());
            recent.drain(..split);
        }
    }
}

async fn write_json(stdin: &mut tokio::process::ChildStdin, value: &Value) -> Result<()> {
    stdin.write_all(format!("{value}\n").as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

async fn read_line(reader: &mut BufReader<tokio::process::ChildStdout>) -> Result<String> {
    let mut line = String::new();
    if reader.read_line(&mut line).await? == 0 {
        anyhow::bail!("extension closed connection");
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return read_nonempty_line(reader).await;
    }
    Ok(trimmed.to_string())
}

async fn read_nonempty_line(reader: &mut BufReader<tokio::process::ChildStdout>) -> Result<String> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            anyhow::bail!("extension closed connection");
        }
        if !line.trim().is_empty() {
            return Ok(line.trim().to_string());
        }
    }
}

fn response_result(response: Value) -> Result<Value> {
    if let Some(result) = response.get("result") {
        Ok(result.clone())
    } else if let Some(error) = response.get("error") {
        Err(anyhow!("RPC error: {error}"))
    } else {
        Err(anyhow!("invalid RPC response: no result or error"))
    }
}

fn send_notification(spec: &LaunchSpec, notification: omegon_extension::RpcNotification) {
    if let Some(tx) = &spec.notification_tx {
        let _ = tx.send(ExtensionNotification {
            extension_name: spec.manifest.extension.name.clone(),
            method: notification.method,
            params: notification.params,
        });
    }
}

fn method_not_found(request: &omegon_extension::RpcRequest) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request.id,
        "error": {"code": -32601, "message": format!("unknown host request method '{}'", request.method)}
    })
}

async fn send_cancel(
    stdin: &mut tokio::process::ChildStdin,
    request_id: u64,
    params: Option<Value>,
) -> Result<()> {
    let mut params = params.unwrap_or_else(|| json!({}));
    let object = params
        .as_object_mut()
        .ok_or_else(|| anyhow!("cancellation parameters must be an object"))?;
    object.insert("request_id".to_string(), Value::from(request_id));
    write_json(
        stdin,
        &json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": params}),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_environment_does_not_include_secret_names() {
        for name in SAFE_INHERIT_ENVS {
            assert!(
                !["KEY", "TOKEN", "SECRET", "PASSWORD"]
                    .iter()
                    .any(|word| name.contains(word))
            );
        }
    }

    #[test]
    fn recent_stderr_is_bounded_at_character_boundaries() {
        let recent = std::sync::Mutex::new(String::new());
        record_recent_stderr(&recent, &"x".repeat(4095));
        record_recent_stderr(&recent, "é");

        let recent = recent.into_inner().unwrap();
        assert!(recent.len() <= 4096);
        assert!(recent.ends_with('é'));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_extension_can_use_remaining_readiness_budget_after_initialize_timeout() {
        use std::os::unix::fs::PermissionsExt;

        let extension_dir = std::env::temp_dir().join(format!(
            "omegon-host-readiness-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&extension_dir).unwrap();
        let script = extension_dir.join("fixture.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *get_tools*) printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":[]}' ;;
  esac
done
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Allow real process scheduling and pipe I/O on loaded hosts. The fixture
        // still withholds initialize forever, so consuming the entire startup
        // budget there must fail even with this more generous watchdog.
        let manifest = toml::from_str(
            r#"
[extension]
name = "legacy-readiness-fixture"
version = "0.1.0"
[runtime]
type = "native"
binary = "fixture.sh"
[startup]
timeout_ms = 2000
"#,
        )
        .unwrap();
        let spec = LaunchSpec {
            manifest,
            extension_dir: extension_dir.clone(),
            project_root: None,
            resolved_config: Map::new(),
            resolved_secrets: Vec::new(),
            source_digest: "fixture".to_string(),
            notification_tx: None,
            host_request_handler: None,
            readiness_validator: None,
        };

        let launched = ExtensionSupervisor::launch(spec).await;
        if let Ok((supervisor, _)) = &launched {
            supervisor
                .shutdown(std::time::Duration::from_millis(100))
                .await
                .unwrap();
        }
        std::fs::remove_dir_all(extension_dir).unwrap();
        if let Err(error) = launched {
            panic!("{error:#}");
        }
    }
}

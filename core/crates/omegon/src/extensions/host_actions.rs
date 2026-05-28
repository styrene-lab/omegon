use super::approval::{self, HostActionApprovalDecision};
use super::manifest::ExtensionManifest;
use crate::tools::terminal;
use omegon_extension::{HostAction, HostActionOutcome, HostActionStatus};
use serde_json::Value;

/// Host-attached origin for an untrusted HostAction candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostActionOriginKind {
    NativeExtension,
    Mcp,
    Internal,
}

/// Trusted runtime origin attached by Omegon before policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostActionOrigin {
    pub kind: HostActionOriginKind,
    pub identity: String,
}

impl HostActionOrigin {
    pub fn native_extension(identity: impl Into<String>) -> Self {
        Self {
            kind: HostActionOriginKind::NativeExtension,
            identity: identity.into(),
        }
    }

    #[allow(dead_code)]
    pub fn mcp(identity: impl Into<String>) -> Self {
        Self {
            kind: HostActionOriginKind::Mcp,
            identity: identity.into(),
        }
    }

    #[allow(dead_code)]
    pub fn internal(identity: impl Into<String>) -> Self {
        Self {
            kind: HostActionOriginKind::Internal,
            identity: identity.into(),
        }
    }
}

/// Session/tool-call scoped action identity. Extension-provided action ids are
/// local labels only; this type is the runtime identity used for policy/audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopedHostActionId {
    pub origin: HostActionOrigin,
    pub session_id: String,
    pub tool_call_id: String,
    pub action_id: String,
}

/// Policy gates that are external to the extension manifest.
#[derive(Debug, Clone, Default)]
pub(super) struct RuntimeHostActionPolicy {
    pub project_allows_auto: bool,
    pub runtime_allows_auto: bool,
    pub origin_trusted_for_auto: bool,
    pub operator_approved: bool,
}

#[derive(Default)]
pub(super) struct HostActionExecutorRegistry {
    supported_types: Vec<String>,
    terminal_create_registry: Option<TerminalBackendRegistry>,
}

impl HostActionExecutorRegistry {
    pub fn with_supported_types(types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            supported_types: types.into_iter().map(Into::into).collect(),
            terminal_create_registry: None,
        }
    }

    pub fn default_supported() -> Self {
        Self::with_supported_types(["terminal.create@1"])
    }

    pub(super) fn with_terminal_backend(
        backend: Box<dyn TerminalCreateBackend + Send + Sync>,
    ) -> Self {
        Self {
            supported_types: vec![
                omegon_extension::actions::terminal::TERMINAL_CREATE_V1.to_string(),
            ],
            terminal_create_registry: Some(TerminalBackendRegistry::new(vec![backend])),
        }
    }

    pub fn with_real_terminal_backend(workspace_cwd: impl Into<std::path::PathBuf>) -> Self {
        Self::with_terminal_registry(TerminalBackendRegistry::new(vec![Box::new(
            RealTerminalCreateBackend {
                workspace_cwd: workspace_cwd.into(),
            },
        )]))
    }

    pub(super) fn with_terminal_registry(
        terminal_create_registry: TerminalBackendRegistry,
    ) -> Self {
        Self {
            supported_types: vec![
                omegon_extension::actions::terminal::TERMINAL_CREATE_V1.to_string(),
            ],
            terminal_create_registry: Some(terminal_create_registry),
        }
    }

    fn supports(&self, action_type: &str) -> bool {
        self.supported_types.iter().any(|ty| ty == action_type)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum HostActionPreparedCandidate {
    Ready {
        action: HostAction,
        candidate: Value,
    },
    Rejected(HostActionOutcome),
}

pub(super) fn prepare_host_action_candidate(
    candidate: Value,
    manifest: &ExtensionManifest,
    scoped_id: &ScopedHostActionId,
    executors: &HostActionExecutorRegistry,
) -> HostActionPreparedCandidate {
    let action: HostAction = match serde_json::from_value(candidate.clone()) {
        Ok(action) => action,
        Err(err) => {
            return HostActionPreparedCandidate::Rejected(audited_outcome(
                scoped_id,
                None,
                "<invalid>",
                HostActionStatus::Invalid,
                "invalid_action",
                format!("invalid HostAction candidate: {err}"),
            ));
        }
    };

    if !action.action_type.contains('@') {
        return HostActionPreparedCandidate::Rejected(audited_outcome(
            scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Invalid,
            "invalid_action_type",
            "HostAction type must include an explicit version suffix",
        ));
    }

    if !executors.supports(&action.action_type) {
        return HostActionPreparedCandidate::Rejected(audited_outcome(
            scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Unsupported,
            "unsupported_action",
            format!("unsupported HostAction type '{}'", action.action_type),
        ));
    }

    if !manifest.allows_host_action_type(&action.action_type) {
        return HostActionPreparedCandidate::Rejected(audited_outcome(
            scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Denied,
            "manifest_denied",
            format!(
                "manifest does not allow HostAction type '{}'",
                action.action_type
            ),
        ));
    }

    HostActionPreparedCandidate::Ready { action, candidate }
}

fn action_requires_manual_approval(
    action: &HostAction,
    runtime_policy: &RuntimeHostActionPolicy,
) -> bool {
    matches!(
        action.execution,
        None | Some(omegon_extension::HostActionExecution::Manual)
            | Some(omegon_extension::HostActionExecution::AutoIfAllowed)
    ) && !(runtime_policy.project_allows_auto
        && runtime_policy.runtime_allows_auto
        && runtime_policy.origin_trusted_for_auto
        && runtime_policy.operator_approved)
}

pub(super) fn prepare_host_action_for_approval(
    candidate: Value,
    manifest: &ExtensionManifest,
    scoped_id: &ScopedHostActionId,
    runtime_policy: &RuntimeHostActionPolicy,
    executors: &HostActionExecutorRegistry,
) -> Result<Option<(HostAction, Value)>, HostActionOutcome> {
    match prepare_host_action_candidate(candidate, manifest, scoped_id, executors) {
        HostActionPreparedCandidate::Rejected(outcome) => Err(outcome),
        HostActionPreparedCandidate::Ready { action, candidate } => {
            if action_requires_manual_approval(&action, runtime_policy) {
                Ok(Some((action, candidate)))
            } else {
                Ok(None)
            }
        }
    }
}

pub(super) fn process_host_action_candidate_with_approval_decision(
    candidate: Value,
    manifest: &ExtensionManifest,
    scoped_id: ScopedHostActionId,
    runtime_policy: &RuntimeHostActionPolicy,
    executors: &HostActionExecutorRegistry,
    approval_decision: HostActionApprovalDecision,
) -> HostActionOutcome {
    let prepared = prepare_host_action_candidate(candidate, manifest, &scoped_id, executors);
    let HostActionPreparedCandidate::Ready { action, candidate } = prepared else {
        return match prepared {
            HostActionPreparedCandidate::Rejected(outcome) => outcome,
            HostActionPreparedCandidate::Ready { .. } => unreachable!(),
        };
    };

    if action_requires_manual_approval(&action, runtime_policy) {
        match approval_decision {
            HostActionApprovalDecision::Approved => {
                let mut approved_policy = runtime_policy.clone();
                approved_policy.operator_approved = true;
                approved_policy.project_allows_auto = true;
                approved_policy.runtime_allows_auto = true;
                approved_policy.origin_trusted_for_auto = true;
                return process_host_action_candidate(
                    candidate,
                    manifest,
                    scoped_id,
                    &approved_policy,
                    executors,
                );
            }
            other => return approval::denied_approval_outcome(&scoped_id, &action, other),
        }
    }

    process_host_action_candidate(candidate, manifest, scoped_id, runtime_policy, executors)
}

pub(super) fn process_native_extension_action_execute(
    action: Value,
    manifest: &ExtensionManifest,
    extension_name: &str,
) -> HostActionOutcome {
    process_host_action_candidate(
        action,
        manifest,
        ScopedHostActionId {
            origin: HostActionOrigin::native_extension(extension_name),
            session_id: "extension-rpc".to_string(),
            tool_call_id: "actions/execute".to_string(),
            action_id: "<pending-parse>".to_string(),
        },
        &RuntimeHostActionPolicy::default(),
        &HostActionExecutorRegistry::with_real_terminal_backend(
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        ),
    )
}

pub(super) fn process_host_action_candidate(
    candidate: Value,
    manifest: &ExtensionManifest,
    scoped_id: ScopedHostActionId,
    runtime_policy: &RuntimeHostActionPolicy,
    executors: &HostActionExecutorRegistry,
) -> HostActionOutcome {
    let action: HostAction = match serde_json::from_value(candidate) {
        Ok(action) => action,
        Err(err) => {
            return audited_outcome(
                &scoped_id,
                None,
                "<invalid>",
                HostActionStatus::Invalid,
                "invalid_action",
                format!("invalid HostAction candidate: {err}"),
            );
        }
    };

    if !action.action_type.contains('@') {
        return audited_outcome(
            &scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Invalid,
            "invalid_action_type",
            "HostAction type must include an explicit version suffix",
        );
    }

    if !executors.supports(&action.action_type) {
        return audited_outcome(
            &scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Unsupported,
            "unsupported_action",
            format!("unsupported HostAction type '{}'", action.action_type),
        );
    }

    if !manifest.allows_host_action_type(&action.action_type) {
        return audited_outcome(
            &scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Denied,
            "manifest_denied",
            format!(
                "manifest does not allow HostAction type '{}'",
                action.action_type
            ),
        );
    }

    if matches!(
        action.execution,
        Some(omegon_extension::HostActionExecution::AutoIfAllowed)
    ) && !(runtime_policy.project_allows_auto
        && runtime_policy.runtime_allows_auto
        && runtime_policy.origin_trusted_for_auto
        && runtime_policy.operator_approved)
    {
        return audited_outcome(
            &scoped_id,
            Some(&action.action_type),
            action.id,
            HostActionStatus::Denied,
            "auto_not_allowed",
            "auto_if_allowed requires manifest, project, runtime, origin, and operator approval",
        );
    }

    if action.action_type == omegon_extension::actions::terminal::TERMINAL_CREATE_V1
        && let Some(registry) = executors.terminal_create_registry.as_ref()
    {
        let outcome = execute_terminal_create_with_registry(&action, manifest, registry);
        audit_host_action_outcome(
            &scoped_id,
            Some(&action.action_type),
            &outcome.action_id,
            &outcome.status,
            outcome
                .error
                .as_ref()
                .map(|error| error.code.as_str())
                .unwrap_or("completed"),
        );
        return outcome;
    }

    audited_outcome(
        &scoped_id,
        Some(&action.action_type),
        action.id,
        HostActionStatus::Unsupported,
        "executor_unavailable",
        "HostAction executor registry seam is present, but no executor is configured",
    )
}

pub(super) fn process_declarative_host_actions(
    actions: Vec<Value>,
    manifest: &ExtensionManifest,
    extension_name: &str,
    tool_call_id: &str,
) -> Vec<Value> {
    actions
        .into_iter()
        .enumerate()
        .map(|(idx, action)| {
            let scoped = ScopedHostActionId {
                origin: HostActionOrigin::native_extension(extension_name),
                session_id: "tool-result".to_string(),
                tool_call_id: tool_call_id.to_string(),
                action_id: action
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("<pending-parse-{idx}>")),
            };
            let outcome = process_host_action_candidate_with_approval_decision(
                action,
                manifest,
                scoped,
                &RuntimeHostActionPolicy::default(),
                &HostActionExecutorRegistry::with_real_terminal_backend(
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                ),
                HostActionApprovalDecision::Unavailable,
            );
            serde_json::to_value(outcome).unwrap_or_else(|err| {
                serde_json::json!({
                    "action_id": "<serialization-error>",
                    "status": "invalid",
                    "error": {
                        "code": "serialization_error",
                        "message": err.to_string()
                    }
                })
            })
        })
        .collect()
}

pub(super) async fn process_declarative_host_actions_with_context(
    actions: Vec<Value>,
    manifest: &ExtensionManifest,
    extension_name: &str,
    tool_call_id: &str,
    context: &omegon_traits::ToolExecutionContext,
) -> Vec<Value> {
    let mut outcomes = Vec::new();
    let executors = HostActionExecutorRegistry::with_real_terminal_backend(
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    );
    let runtime_policy = RuntimeHostActionPolicy::default();

    for (idx, action) in actions.into_iter().enumerate() {
        let scoped = ScopedHostActionId {
            origin: HostActionOrigin::native_extension(extension_name),
            session_id: "tool-result".to_string(),
            tool_call_id: tool_call_id.to_string(),
            action_id: action
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("<pending-parse-{idx}>")),
        };

        let prepared_for_approval = match prepare_host_action_for_approval(
            action.clone(),
            manifest,
            &scoped,
            &runtime_policy,
            &executors,
        ) {
            Err(outcome) => {
                outcomes.push(serde_json::to_value(outcome).unwrap_or_else(|err| {
                    serde_json::json!({
                        "action_id": "<serialization-error>",
                        "status": "invalid",
                        "error": {
                            "code": "serialization_error",
                            "message": err.to_string()
                        }
                    })
                }));
                continue;
            }
            Ok(Some((prepared_action, _candidate))) => Some(prepared_action),
            Ok(None) if context.host_action_approval.is_some() => {
                // A visual ACP host gets first refusal for all declarative
                // HostActions, including auto-eligible actions. This surfaces
                // terminal.create@1 candidates before local execution so the
                // host can own review, placement, lifecycle, and rendering.
                match serde_json::from_value::<HostAction>(action.clone()) {
                    Ok(action) => Some(action),
                    Err(err) => {
                        outcomes.push(serialization_error_outcome(err));
                        continue;
                    }
                }
            }
            Ok(None) => None,
        };

        let approval_decision = if let Some(prepared_action) = prepared_for_approval {
            if let Some(sink) = &context.host_action_approval {
                let request = approval::build_host_action_permission_request(
                    scoped.session_id.clone(),
                    extension_name,
                    &scoped,
                    &prepared_action,
                    "host action requires approval",
                );
                let request_json = serde_json::to_value(request).unwrap_or_else(|err| {
                    serde_json::json!({
                        "error": {
                            "code": "approval_request_serialization",
                            "message": err.to_string()
                        }
                    })
                });
                let decision_json = sink(request_json).await;
                serde_json::from_value::<HostActionApprovalDecision>(decision_json)
                    .unwrap_or(HostActionApprovalDecision::Unavailable)
            } else {
                HostActionApprovalDecision::Unavailable
            }
        } else {
            HostActionApprovalDecision::Approved
        };

        let outcome = process_host_action_candidate_with_approval_decision(
            action,
            manifest,
            scoped,
            &runtime_policy,
            &executors,
            approval_decision,
        );
        outcomes.push(serde_json::to_value(outcome).unwrap_or_else(|err| {
            serde_json::json!({
                "action_id": "<serialization-error>",
                "status": "invalid",
                "error": {
                    "code": "serialization_error",
                    "message": err.to_string()
                }
            })
        }));
    }

    outcomes
}

pub(crate) fn process_mcp_host_actions_typed(
    actions: &Value,
    server_name: &str,
    tool_name: &str,
) -> Vec<HostActionOutcome> {
    let Some(actions) = actions.as_array() else {
        let scoped = ScopedHostActionId {
            origin: HostActionOrigin::mcp(server_name),
            session_id: "mcp-tool-result".to_string(),
            tool_call_id: tool_name.to_string(),
            action_id: "omegon/hostActions".to_string(),
        };
        let outcome = audited_outcome(
            &scoped,
            None,
            "omegon/hostActions",
            HostActionStatus::Invalid,
            "invalid_host_actions_metadata",
            "_meta[\"omegon/hostActions\"] must be an array",
        );
        return vec![outcome];
    };

    let manifest = mcp_deny_by_default_manifest();
    actions
        .iter()
        .enumerate()
        .map(|(idx, action)| {
            let scoped = ScopedHostActionId {
                origin: HostActionOrigin::mcp(server_name),
                session_id: "mcp-tool-result".to_string(),
                tool_call_id: tool_name.to_string(),
                action_id: action
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("<pending-parse-{idx}>")),
            };
            process_host_action_candidate(
                action.clone(),
                &manifest,
                scoped,
                &RuntimeHostActionPolicy::default(),
                &HostActionExecutorRegistry::default_supported(),
            )
        })
        .collect()
}

pub(crate) fn process_mcp_host_actions(
    actions: &Value,
    server_name: &str,
    tool_name: &str,
) -> Vec<Value> {
    process_mcp_host_actions_typed(actions, server_name, tool_name)
        .into_iter()
        .map(|outcome| serde_json::to_value(outcome).unwrap_or_else(serialization_error_outcome))
        .collect()
}

fn mcp_deny_by_default_manifest() -> ExtensionManifest {
    toml::from_str(
        r#"
[extension]
name = "mcp"
version = "0.0.0"

[runtime]
type = "native"
binary = "mcp"

[permissions.host_actions]
allowed = []
"#,
    )
    .expect("static MCP HostAction manifest is valid")
}

fn serialization_error_outcome(err: serde_json::Error) -> Value {
    serde_json::json!({
        "action_id": "<serialization-error>",
        "status": "invalid",
        "error": {
            "code": "serialization_error",
            "message": err.to_string()
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalPlacementCapability {
    BackgroundSession,
    SidePane,
    BottomPane,
    NewTab,
}

impl TerminalPlacementCapability {
    fn as_result_str(self) -> &'static str {
        match self {
            Self::BackgroundSession => "background_session",
            Self::SidePane => "side_pane",
            Self::BottomPane => "bottom_pane",
            Self::NewTab => "new_tab",
        }
    }
}

pub(super) trait TerminalCreateBackend {
    fn name(&self) -> &'static str;

    fn supports_placement(&self, placement: TerminalPlacementCapability) -> bool;

    fn create(
        &self,
        plan: TerminalCreateLaunchPlan,
    ) -> Result<omegon_extension::actions::terminal::TerminalCreateResult, String>;
}

pub(super) struct TerminalBackendRegistry {
    backends: Vec<Box<dyn TerminalCreateBackend + Send + Sync>>,
}

impl TerminalBackendRegistry {
    pub(super) fn new(backends: Vec<Box<dyn TerminalCreateBackend + Send + Sync>>) -> Self {
        Self { backends }
    }

    fn select(
        &self,
        requested: TerminalPlacementCapability,
    ) -> Option<&(dyn TerminalCreateBackend + Send + Sync)> {
        self.backends
            .iter()
            .find(|backend| backend.supports_placement(requested))
            .or_else(|| {
                self.backends.iter().find(|backend| {
                    backend.supports_placement(TerminalPlacementCapability::BackgroundSession)
                })
            })
            .map(|backend| backend.as_ref())
    }
}

pub(super) struct UnavailableTerminalCreateBackend {
    pub reason: String,
}

impl TerminalCreateBackend for UnavailableTerminalCreateBackend {
    fn name(&self) -> &'static str {
        "unavailable"
    }

    fn supports_placement(&self, _placement: TerminalPlacementCapability) -> bool {
        true
    }

    fn create(
        &self,
        _plan: TerminalCreateLaunchPlan,
    ) -> Result<omegon_extension::actions::terminal::TerminalCreateResult, String> {
        Err(self.reason.clone())
    }
}

pub(super) struct FakeTerminalCreateBackend {
    pub result: omegon_extension::actions::terminal::TerminalCreateResult,
}

impl TerminalCreateBackend for FakeTerminalCreateBackend {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn supports_placement(&self, _placement: TerminalPlacementCapability) -> bool {
        true
    }

    fn create(
        &self,
        _plan: TerminalCreateLaunchPlan,
    ) -> Result<omegon_extension::actions::terminal::TerminalCreateResult, String> {
        Ok(self.result.clone())
    }
}

pub(super) struct RealTerminalCreateBackend {
    pub workspace_cwd: std::path::PathBuf,
}

impl TerminalCreateBackend for RealTerminalCreateBackend {
    fn name(&self) -> &'static str {
        "portable_pty"
    }

    fn supports_placement(&self, placement: TerminalPlacementCapability) -> bool {
        matches!(placement, TerminalPlacementCapability::BackgroundSession)
    }

    fn create(
        &self,
        plan: TerminalCreateLaunchPlan,
    ) -> Result<omegon_extension::actions::terminal::TerminalCreateResult, String> {
        let request = terminal_backend_request_from_plan(
            plan.clone(),
            &self.workspace_cwd,
            plan.name.clone(),
        );
        let response = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| format!("failed to create terminal backend runtime: {err}"))?;
            runtime.block_on(terminal::start_host_terminal(request))
        })
        .join()
        .map_err(|_| "terminal backend worker panicked".to_string())??;
        let mut warnings = response.warnings;
        if response.actual_placement == "background_session" {
            warnings.push(response.inspect_hint.clone());
        }
        Ok(omegon_extension::actions::terminal::TerminalCreateResult {
            terminal_id: response.terminal_id,
            backend: response.backend,
            actual_placement: response.actual_placement,
            warnings,
        })
    }
}

pub(super) fn execute_terminal_create_with_backend(
    action: &HostAction,
    manifest: &ExtensionManifest,
    backend: &(dyn TerminalCreateBackend + Send + Sync),
) -> HostActionOutcome {
    let plan = match validate_terminal_create_policy(action, manifest) {
        Ok(plan) => plan,
        Err(outcome) => return outcome,
    };
    execute_terminal_create_plan(action, plan, backend)
}

pub(super) fn execute_terminal_create_with_registry(
    action: &HostAction,
    manifest: &ExtensionManifest,
    registry: &TerminalBackendRegistry,
) -> HostActionOutcome {
    let plan = match validate_terminal_create_policy(action, manifest) {
        Ok(plan) => plan,
        Err(outcome) => return outcome,
    };

    let requested = plan.requested_placement();
    let Some(backend) = registry.select(requested) else {
        return outcome(
            action.id.clone(),
            HostActionStatus::Unsupported,
            "terminal_backend_unavailable",
            "no terminal backend is available",
        );
    };
    execute_terminal_create_plan(action, plan, backend)
}

fn execute_terminal_create_plan(
    action: &HostAction,
    plan: TerminalCreateLaunchPlan,
    backend: &(dyn TerminalCreateBackend + Send + Sync),
) -> HostActionOutcome {
    let requested = plan.requested_placement();
    let requested_placement = requested.as_result_str();
    match backend.create(plan) {
        Ok(mut result) => {
            if result.actual_placement != requested_placement
                && requested != TerminalPlacementCapability::BackgroundSession
            {
                result.warnings.push(format!(
                    "requested {requested_placement} but backend '{}' provided {}; placement degraded",
                    backend.name(),
                    result.actual_placement
                ));
            }
            HostActionOutcome {
                action_id: action.id.clone(),
                status: HostActionStatus::Completed,
                result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                error: None,
            }
        }
        Err(reason) => outcome(
            action.id.clone(),
            HostActionStatus::Unsupported,
            "terminal_backend_unavailable",
            reason,
        ),
    }
}

pub(super) fn terminal_backend_request_from_plan(
    plan: TerminalCreateLaunchPlan,
    workspace_cwd: &std::path::Path,
    name: Option<String>,
) -> terminal::HostTerminalCreateRequest {
    let cwd = plan
        .cwd
        .as_deref()
        .map(|cwd| {
            let path = std::path::Path::new(cwd);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                workspace_cwd.join(path)
            }
        })
        .unwrap_or_else(|| workspace_cwd.to_path_buf());

    terminal::HostTerminalCreateRequest {
        command: plan.command,
        args: plan.args,
        cwd,
        env: plan.env,
        name,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TerminalCreateLaunchPlan {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub placement: Option<omegon_extension::actions::terminal::TerminalPlacement>,
    pub name: Option<String>,
}

impl TerminalCreateLaunchPlan {
    fn requested_placement(&self) -> TerminalPlacementCapability {
        match self.placement {
            Some(omegon_extension::actions::terminal::TerminalPlacement::SidePane) => {
                TerminalPlacementCapability::SidePane
            }
            Some(omegon_extension::actions::terminal::TerminalPlacement::BottomPane) => {
                TerminalPlacementCapability::BottomPane
            }
            Some(omegon_extension::actions::terminal::TerminalPlacement::NewTab) => {
                TerminalPlacementCapability::NewTab
            }
            Some(omegon_extension::actions::terminal::TerminalPlacement::Default) | None => {
                TerminalPlacementCapability::BackgroundSession
            }
        }
    }
}

pub(super) fn validate_terminal_create_policy(
    action: &HostAction,
    manifest: &ExtensionManifest,
) -> Result<TerminalCreateLaunchPlan, HostActionOutcome> {
    let params: omegon_extension::actions::terminal::TerminalCreateParams =
        serde_json::from_value(action.params.clone()).map_err(|err| {
            outcome(
                action.id.clone(),
                HostActionStatus::Invalid,
                "invalid_terminal_create_params",
                format!("terminal.create@1 params failed validation: {err}"),
            )
        })?;

    let policy = &manifest.permissions.host_actions.terminal_create;
    if !policy
        .allowed_commands
        .iter()
        .any(|allowed| allowed == &params.command)
    {
        return Err(outcome(
            action.id.clone(),
            HostActionStatus::Denied,
            "terminal_command_denied",
            format!(
                "terminal command '{}' is not allowed by manifest policy",
                params.command
            ),
        ));
    }

    for key in params.env.keys() {
        if !policy.allow_env.iter().any(|allowed| allowed == key) {
            return Err(outcome(
                action.id.clone(),
                HostActionStatus::Denied,
                "terminal_env_denied",
                format!("terminal env key '{key}' is not allowed by manifest policy"),
            ));
        }
    }

    if let Some(cwd) = &params.cwd {
        validate_terminal_cwd(cwd, &policy.allowed_cwd_roots).map_err(|message| {
            outcome(
                action.id.clone(),
                HostActionStatus::Denied,
                "terminal_cwd_denied",
                message,
            )
        })?;
    }

    Ok(TerminalCreateLaunchPlan {
        command: params.command,
        args: params.args,
        cwd: params.cwd,
        env: params.env.into_iter().collect(),
        placement: params.placement,
        name: params.reuse_key.or(params.title),
    })
}

fn validate_terminal_cwd(cwd: &str, allowed_roots: &[String]) -> Result<(), String> {
    if allowed_roots.is_empty() {
        return Err(format!(
            "terminal cwd '{cwd}' was requested but no cwd roots are allowed by manifest policy"
        ));
    }

    let cwd_path = std::path::Path::new(cwd);
    for root in allowed_roots {
        if root == "${workspace}" {
            if cwd_path.is_relative() {
                return Ok(());
            }
            continue;
        }
        let root_path = std::path::Path::new(root);
        if cwd_path.starts_with(root_path) {
            return Ok(());
        }
    }

    Err(format!(
        "terminal cwd '{cwd}' is outside allowed manifest roots"
    ))
}

fn audited_outcome(
    scoped_id: &ScopedHostActionId,
    action_type: Option<&str>,
    action_id: impl Into<String>,
    status: HostActionStatus,
    code: impl Into<String>,
    message: impl Into<String>,
) -> HostActionOutcome {
    let code = code.into();
    let message = message.into();
    let action_id = action_id.into();
    audit_host_action_outcome(scoped_id, action_type, &action_id, &status, &code);
    outcome(action_id, status, code, message)
}

fn audit_host_action_outcome(
    scoped_id: &ScopedHostActionId,
    action_type: Option<&str>,
    action_id: &str,
    status: &HostActionStatus,
    code: &str,
) {
    tracing::info!(
        target: "omegon::host_actions",
        origin_kind = ?scoped_id.origin.kind,
        origin_identity = %scoped_id.origin.identity,
        session_id = %scoped_id.session_id,
        tool_call_id = %scoped_id.tool_call_id,
        local_action_id = %scoped_id.action_id,
        action_id = %action_id,
        action_type = action_type.unwrap_or("<invalid>"),
        status = ?status,
        error_code = %code,
        "host action outcome"
    );
}

fn outcome(
    action_id: impl Into<String>,
    status: HostActionStatus,
    code: impl Into<String>,
    message: impl Into<String>,
) -> HostActionOutcome {
    HostActionOutcome {
        action_id: action_id.into(),
        status,
        result: None,
        error: Some(omegon_extension::HostActionError {
            code: code.into(),
            message: message.into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(allowed: &[&str]) -> ExtensionManifest {
        let allowed = allowed
            .iter()
            .map(|allowed| format!("\"{allowed}\""))
            .collect::<Vec<_>>()
            .join(", ");
        toml::from_str(&format!(
            r#"
[extension]
name = "reader"
version = "0.1.0"

[runtime]
type = "native"
binary = "target/release/reader"

[permissions.host_actions]
allowed = [{allowed}]
"#
        ))
        .unwrap()
    }

    fn scoped() -> ScopedHostActionId {
        ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader"),
            session_id: "session-1".to_string(),
            tool_call_id: "call-1".to_string(),
            action_id: "open-reader".to_string(),
        }
    }

    fn registry() -> HostActionExecutorRegistry {
        HostActionExecutorRegistry::default_supported()
    }

    #[test]
    fn malformed_action_candidate_returns_invalid_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "broken", "params": {}}),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Invalid);
        assert_eq!(outcome.error.unwrap().code, "invalid_action");
    }

    #[test]
    fn unversioned_action_type_returns_invalid_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "open-reader", "type": "terminal.create", "params": {}}),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Invalid);
        assert_eq!(outcome.error.unwrap().code, "invalid_action_type");
    }

    #[test]
    fn unsupported_action_type_returns_unsupported_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "open-file", "type": "file.open@1", "params": {}}),
            &manifest(&["file.open@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Unsupported);
        assert_eq!(outcome.error.unwrap().code, "unsupported_action");
    }

    #[test]
    fn manifest_denied_action_returns_denied_outcome() {
        let outcome = process_host_action_candidate(
            json!({"id": "open-file", "type": "file.open@1", "params": {}}),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &HostActionExecutorRegistry::with_supported_types(["file.open@1"]),
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "manifest_denied");
    }

    #[test]
    fn auto_if_allowed_is_conservative() {
        let outcome = process_host_action_candidate(
            json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "execution": "auto_if_allowed",
                "params": {"command": "bookokrat"}
            }),
            &manifest(&["terminal.create@1"]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "auto_not_allowed");
    }

    #[test]
    fn imperative_action_execute_uses_same_manifest_denial_policy() {
        let outcome = process_native_extension_action_execute(
            json!({"id": "open-file", "type": "file.open@1", "params": {}}),
            &manifest(&["terminal.create@1"]),
            "reader",
        );

        assert_eq!(outcome.status, HostActionStatus::Unsupported);
        assert_eq!(outcome.error.unwrap().code, "unsupported_action");
    }

    #[test]
    fn imperative_action_execute_returns_invalid_outcome() {
        let outcome = process_native_extension_action_execute(
            json!({"id": "broken", "params": {}}),
            &manifest(&["terminal.create@1"]),
            "reader",
        );

        assert_eq!(outcome.status, HostActionStatus::Invalid);
    }

    #[test]
    fn imperative_action_execute_returns_denied_for_supported_but_manifest_denied() {
        let outcome = process_native_extension_action_execute(
            json!({"id": "open-reader", "type": "terminal.create@1", "params": {}}),
            &manifest(&[]),
            "reader",
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "manifest_denied");
    }

    #[test]
    fn declarative_actions_produce_deterministic_outcomes_for_headless_details() {
        let outcomes = process_declarative_host_actions(
            vec![json!({"id": "open-reader", "type": "terminal.create@1", "params": {}})],
            &manifest(&[]),
            "reader",
            "call-1",
        );

        assert_eq!(outcomes[0]["action_id"], "open-reader");
        assert_eq!(outcomes[0]["status"], "denied");
        assert_eq!(outcomes[0]["error"]["code"], "manifest_denied");
    }

    #[test]
    fn mcp_non_array_metadata_returns_invalid_outcome() {
        let outcomes = process_mcp_host_actions(&json!("bad"), "server", "tool");

        assert_eq!(outcomes[0]["action_id"], "omegon/hostActions");
        assert_eq!(outcomes[0]["status"], "invalid");
        assert_eq!(
            outcomes[0]["error"]["code"],
            "invalid_host_actions_metadata"
        );
    }

    #[test]
    fn mcp_malformed_action_returns_invalid_outcome() {
        let outcomes =
            process_mcp_host_actions(&json!([{ "id": "broken", "params": {} }]), "server", "tool");

        assert_eq!(outcomes[0]["status"], "invalid");
        assert_eq!(outcomes[0]["error"]["code"], "invalid_action");
    }

    #[test]
    fn mcp_unsupported_action_returns_unsupported_outcome() {
        let outcomes = process_mcp_host_actions(
            &json!([{ "id": "open-file", "type": "file.open@1", "params": {} }]),
            "server",
            "tool",
        );

        assert_eq!(outcomes[0]["action_id"], "open-file");
        assert_eq!(outcomes[0]["status"], "unsupported");
        assert_eq!(outcomes[0]["error"]["code"], "unsupported_action");
    }

    #[test]
    fn mcp_supported_action_is_denied_by_default_policy() {
        let outcomes = process_mcp_host_actions(
            &json!([{ "id": "open-reader", "type": "terminal.create@1", "params": {"command": "bookokrat"} }]),
            "server",
            "tool",
        );

        assert_eq!(outcomes[0]["action_id"], "open-reader");
        assert_eq!(outcomes[0]["status"], "denied");
        assert_eq!(outcomes[0]["error"]["code"], "manifest_denied");
    }

    #[test]
    fn mcp_auto_if_allowed_is_denied_before_execution() {
        let outcomes = process_mcp_host_actions(
            &json!([{ "id": "open-reader", "type": "terminal.create@1", "execution": "auto_if_allowed", "params": {"command": "bookokrat"} }]),
            "server",
            "tool",
        );

        assert_eq!(outcomes[0]["status"], "denied");
        assert_eq!(outcomes[0]["error"]["code"], "manifest_denied");
    }

    #[test]
    fn terminal_create_backend_unavailable_returns_unsupported() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "bookokrat"}));
        let backend = UnavailableTerminalCreateBackend {
            reason: "PTY unavailable".to_string(),
        };

        let outcome = execute_terminal_create_with_backend(&action, &manifest, &backend);

        assert_eq!(outcome.status, HostActionStatus::Unsupported);
        assert_eq!(outcome.error.unwrap().code, "terminal_backend_unavailable");
    }

    #[test]
    fn terminal_create_fake_backend_returns_completed_result_shape() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "bookokrat"}));
        let backend = FakeTerminalCreateBackend {
            result: omegon_extension::actions::terminal::TerminalCreateResult {
                terminal_id: "term_123".to_string(),
                backend: "fake".to_string(),
                actual_placement: "background_session".to_string(),
                warnings: vec!["placement degraded".to_string()],
            },
        };

        let outcome = execute_terminal_create_with_backend(&action, &manifest, &backend);

        assert_eq!(outcome.status, HostActionStatus::Completed);
        assert_eq!(outcome.result.as_ref().unwrap()["terminal_id"], "term_123");
        assert_eq!(outcome.result.as_ref().unwrap()["backend"], "fake");
        assert_eq!(
            outcome.result.as_ref().unwrap()["actual_placement"],
            "background_session"
        );
        assert_eq!(
            outcome.result.as_ref().unwrap()["warnings"][0],
            "placement degraded"
        );
    }

    #[test]
    fn terminal_backend_request_from_plan_preserves_argv_without_shell() {
        let request = terminal_backend_request_from_plan(
            TerminalCreateLaunchPlan {
                command: "bookokrat".to_string(),
                args: vec!["/books/a.epub".to_string()],
                cwd: Some("books".to_string()),
                env: vec![("BOOKOKRAT_THEME".to_string(), "dark".to_string())],
                placement: None,
                name: None,
            },
            std::path::Path::new("/workspace"),
            Some("reader".to_string()),
        );

        assert_eq!(request.command, "bookokrat");
        assert_eq!(request.args, vec!["/books/a.epub"]);
        assert_eq!(request.cwd, std::path::PathBuf::from("/workspace/books"));
        assert_eq!(request.name.as_deref(), Some("reader"));
        assert_ne!(request.command, "bash");
        assert!(!request.args.iter().any(|arg| arg == "-lc"));
    }

    fn terminal_manifest(
        allowed_commands: &[&str],
        allowed_roots: &[&str],
        allow_env: &[&str],
    ) -> ExtensionManifest {
        let mut manifest = manifest(&["terminal.create@1"]);
        manifest
            .permissions
            .host_actions
            .terminal_create
            .allowed_commands = allowed_commands
            .iter()
            .map(|value| value.to_string())
            .collect();
        manifest
            .permissions
            .host_actions
            .terminal_create
            .allowed_cwd_roots = allowed_roots
            .iter()
            .map(|value| value.to_string())
            .collect();
        manifest.permissions.host_actions.terminal_create.allow_env =
            allow_env.iter().map(|value| value.to_string()).collect();
        manifest
    }

    fn terminal_action(params: serde_json::Value) -> HostAction {
        HostAction::new("open-reader", "terminal.create@1", params).unwrap()
    }

    #[test]
    fn terminal_create_policy_allows_manifest_command() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "bookokrat", "args": ["/books/a.epub"]}));

        let plan = validate_terminal_create_policy(&action, &manifest).unwrap();

        assert_eq!(plan.command, "bookokrat");
        assert_eq!(plan.args, vec!["/books/a.epub"]);
    }

    #[test]
    fn terminal_create_policy_denies_disallowed_command_before_spawn() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "sh", "args": ["-c", "echo no"]}));

        let outcome = validate_terminal_create_policy(&action, &manifest).unwrap_err();

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "terminal_command_denied");
    }

    #[test]
    fn terminal_create_policy_denies_env_by_default() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({
            "command": "bookokrat",
            "env": {"BOOKOKRAT_THEME": "dark"}
        }));

        let outcome = validate_terminal_create_policy(&action, &manifest).unwrap_err();

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "terminal_env_denied");
    }

    #[test]
    fn terminal_create_policy_allows_allowlisted_env() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &["BOOKOKRAT_THEME"]);
        let action = terminal_action(json!({
            "command": "bookokrat",
            "env": {"BOOKOKRAT_THEME": "dark"}
        }));

        let plan = validate_terminal_create_policy(&action, &manifest).unwrap();

        assert_eq!(
            plan.env,
            vec![("BOOKOKRAT_THEME".to_string(), "dark".to_string())]
        );
    }

    #[test]
    fn terminal_create_policy_denies_cwd_outside_allowed_roots() {
        let manifest = terminal_manifest(&["bookokrat"], &["/workspace/books"], &[]);
        let action = terminal_action(json!({"command": "bookokrat", "cwd": "/tmp"}));

        let outcome = validate_terminal_create_policy(&action, &manifest).unwrap_err();

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "terminal_cwd_denied");
    }

    #[test]
    fn terminal_create_policy_accepts_relative_workspace_cwd_token() {
        let manifest = terminal_manifest(&["bookokrat"], &["${workspace}"], &[]);
        let action = terminal_action(json!({"command": "bookokrat", "cwd": "books"}));

        let plan = validate_terminal_create_policy(&action, &manifest).unwrap();

        assert_eq!(plan.cwd.as_deref(), Some("books"));
    }

    #[test]
    fn declarative_terminal_create_requires_approval_before_executor() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let registry = HostActionExecutorRegistry::with_terminal_backend(Box::new(
            FakeTerminalCreateBackend {
                result: omegon_extension::actions::terminal::TerminalCreateResult {
                    terminal_id: "term_decl".to_string(),
                    backend: "fake".to_string(),
                    actual_placement: "background_session".to_string(),
                    warnings: Vec::new(),
                },
            },
        ));

        let outcome = process_host_action_candidate_with_approval_decision(
            json!({"id": "open-reader", "type": "terminal.create@1", "params": {"command": "bookokrat"}}),
            &manifest,
            ScopedHostActionId {
                origin: HostActionOrigin::native_extension("reader"),
                session_id: "tool-result".to_string(),
                tool_call_id: "call-1".to_string(),
                action_id: "open-reader".to_string(),
            },
            &RuntimeHostActionPolicy::default(),
            &registry,
            HostActionApprovalDecision::Unavailable,
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.as_ref().unwrap().code, "approval_unavailable");
    }

    #[test]
    fn imperative_terminal_create_reaches_same_executor_path() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let registry = HostActionExecutorRegistry::with_terminal_backend(Box::new(
            FakeTerminalCreateBackend {
                result: omegon_extension::actions::terminal::TerminalCreateResult {
                    terminal_id: "term_rpc".to_string(),
                    backend: "fake".to_string(),
                    actual_placement: "background_session".to_string(),
                    warnings: vec!["placement degraded".to_string()],
                },
            },
        ));

        let outcome = process_host_action_candidate(
            json!({"id": "open-reader", "type": "terminal.create@1", "params": {"command": "bookokrat"}}),
            &manifest,
            ScopedHostActionId {
                origin: HostActionOrigin::native_extension("reader"),
                session_id: "extension-rpc".to_string(),
                tool_call_id: "actions/execute".to_string(),
                action_id: "open-reader".to_string(),
            },
            &RuntimeHostActionPolicy::default(),
            &registry,
        );

        assert_eq!(outcome.status, HostActionStatus::Completed);
        assert_eq!(outcome.result.as_ref().unwrap()["terminal_id"], "term_rpc");
        assert_eq!(
            outcome.result.as_ref().unwrap()["warnings"][0],
            "placement degraded"
        );
    }

    struct SelectiveFakeTerminalCreateBackend {
        name: &'static str,
        placement: TerminalPlacementCapability,
        actual_placement: &'static str,
    }

    impl TerminalCreateBackend for SelectiveFakeTerminalCreateBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        fn supports_placement(&self, placement: TerminalPlacementCapability) -> bool {
            placement == self.placement
        }

        fn create(
            &self,
            _plan: TerminalCreateLaunchPlan,
        ) -> Result<omegon_extension::actions::terminal::TerminalCreateResult, String> {
            Ok(omegon_extension::actions::terminal::TerminalCreateResult {
                terminal_id: format!("term-{}", self.name),
                backend: self.name.to_string(),
                actual_placement: self.actual_placement.to_string(),
                warnings: Vec::new(),
            })
        }
    }

    struct CountingBackend {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl TerminalCreateBackend for CountingBackend {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn supports_placement(&self, _placement: TerminalPlacementCapability) -> bool {
            true
        }

        fn create(
            &self,
            _plan: TerminalCreateLaunchPlan,
        ) -> Result<omegon_extension::actions::terminal::TerminalCreateResult, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(omegon_extension::actions::terminal::TerminalCreateResult {
                terminal_id: "term-counting".to_string(),
                backend: "counting".to_string(),
                actual_placement: "side_pane".to_string(),
                warnings: Vec::new(),
            })
        }
    }

    #[test]
    fn terminal_create_side_pane_degrades_to_background_when_no_visual_backend_exists() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "bookokrat", "placement": "side_pane"}));
        let registry =
            TerminalBackendRegistry::new(vec![Box::new(SelectiveFakeTerminalCreateBackend {
                name: "portable_pty",
                placement: TerminalPlacementCapability::BackgroundSession,
                actual_placement: "background_session",
            })]);

        let outcome = execute_terminal_create_with_registry(&action, &manifest, &registry);

        assert_eq!(outcome.status, HostActionStatus::Completed);
        let result = outcome.result.unwrap();
        assert_eq!(result["backend"], "portable_pty");
        assert_eq!(result["actual_placement"], "background_session");
        let warnings = result["warnings"].as_array().expect("warnings array");
        assert!(
            warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("placement degraded"))),
            "warnings: {warnings:?}"
        );
        assert!(
            result["warnings"][0]
                .as_str()
                .unwrap()
                .contains("requested side_pane")
        );
    }

    #[test]
    fn terminal_create_visual_backend_is_preferred_for_side_pane() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "bookokrat", "placement": "side_pane"}));
        let registry = TerminalBackendRegistry::new(vec![
            Box::new(SelectiveFakeTerminalCreateBackend {
                name: "flynt_side_pane",
                placement: TerminalPlacementCapability::SidePane,
                actual_placement: "side_pane",
            }),
            Box::new(SelectiveFakeTerminalCreateBackend {
                name: "portable_pty",
                placement: TerminalPlacementCapability::BackgroundSession,
                actual_placement: "background_session",
            }),
        ]);

        let outcome = execute_terminal_create_with_registry(&action, &manifest, &registry);

        assert_eq!(outcome.status, HostActionStatus::Completed);
        let result = outcome.result.unwrap();
        assert_eq!(result["backend"], "flynt_side_pane");
        assert_eq!(result["actual_placement"], "side_pane");
        assert!(result["warnings"].as_array().is_none_or(Vec::is_empty));
    }

    #[test]
    fn terminal_create_background_result_includes_inspection_hint() {
        let manifest = terminal_manifest(&["printf"], &["${workspace}"], &[]);
        let outcome = process_host_action_candidate_with_approval_decision(
            json!({
                "id": "open-bg",
                "type": "terminal.create@1",
                "execution": "manual",
                "params": {
                    "command": "printf",
                    "args": ["visible"],
                    "cwd": ".",
                    "placement": "background_session"
                }
            }),
            &manifest,
            ScopedHostActionId {
                origin: HostActionOrigin::native_extension("reader"),
                session_id: "test-session".into(),
                tool_call_id: "tc1".into(),
                action_id: "open-bg".into(),
            },
            &RuntimeHostActionPolicy::default(),
            &HostActionExecutorRegistry::with_real_terminal_backend(
                std::env::current_dir().unwrap(),
            ),
            crate::extensions::approval::HostActionApprovalDecision::Approved,
        );

        assert_eq!(
            outcome.status,
            omegon_extension::HostActionStatus::Completed,
            "outcome: {outcome:?}"
        );
        let result = outcome.result.expect("result");
        assert_eq!(result["actual_placement"], "background_session");
        let warnings = result["warnings"].as_array().expect("warnings array");
        let warning_text = warnings
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !warning_text.contains("placement degraded"),
            "{warning_text}"
        );
        assert!(warning_text.contains("terminal.read"), "{warning_text}");
        assert!(warning_text.contains("terminal.stop"), "{warning_text}");
        assert!(warning_text.contains("open transcript"), "{warning_text}");
    }

    #[test]
    fn terminal_create_side_pane_degradation_includes_inspection_hint() {
        let manifest = terminal_manifest(&["printf"], &["${workspace}"], &[]);
        let terminal_name = format!(
            "side-visible-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        );
        let outcome = process_host_action_candidate_with_approval_decision(
            json!({
                "id": "open-side",
                "type": "terminal.create@1",
                "execution": "manual",
                "params": {
                    "command": "printf",
                    "args": ["side-visible"],
                    "cwd": ".",
                    "placement": "side_pane",
                    "reuse_key": terminal_name
                }
            }),
            &manifest,
            ScopedHostActionId {
                origin: HostActionOrigin::native_extension("reader"),
                session_id: "test-session".into(),
                tool_call_id: "tc1".into(),
                action_id: "open-side".into(),
            },
            &RuntimeHostActionPolicy::default(),
            &HostActionExecutorRegistry::with_real_terminal_backend(
                std::env::current_dir().unwrap(),
            ),
            crate::extensions::approval::HostActionApprovalDecision::Approved,
        );

        assert_eq!(
            outcome.status,
            omegon_extension::HostActionStatus::Completed,
            "outcome: {outcome:?}"
        );
        let result = outcome.result.expect("result");
        assert_eq!(result["actual_placement"], "background_session");
        let warning_text = result["warnings"]
            .as_array()
            .expect("warnings array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            warning_text.contains("placement degraded"),
            "{warning_text}"
        );
        assert!(warning_text.contains("terminal.read"), "{warning_text}");
        assert!(warning_text.contains("terminal.stop"), "{warning_text}");
        assert!(warning_text.contains("open transcript"), "{warning_text}");
    }

    #[test]
    fn terminal_create_policy_denial_prevents_backend_execution() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let action = terminal_action(json!({"command": "sh", "placement": "side_pane"}));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registry = TerminalBackendRegistry::new(vec![Box::new(CountingBackend {
            calls: calls.clone(),
        })]);

        let outcome = execute_terminal_create_with_registry(&action, &manifest, &registry);

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "terminal_command_denied");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn production_registry_installs_real_terminal_backend() {
        let registry = HostActionExecutorRegistry::with_real_terminal_backend("/workspace");
        assert!(registry.supports("terminal.create@1"));
        assert!(registry.terminal_create_registry.is_some());
    }

    #[test]
    fn audited_outcomes_preserve_scoped_identity_inputs() {
        let scoped = ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader"),
            session_id: "session-a".to_string(),
            tool_call_id: "call-a".to_string(),
            action_id: "local-a".to_string(),
        };
        let outcome = process_host_action_candidate(
            json!({"id": "open-reader", "type": "terminal.create@1", "params": {}}),
            &manifest(&[]),
            scoped,
            &RuntimeHostActionPolicy::default(),
            &registry(),
        );

        assert_eq!(outcome.action_id, "open-reader");
        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "manifest_denied");
    }

    #[test]
    fn scoped_action_ids_preserve_local_id_but_distinguish_origin() {
        let left = ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader-a"),
            session_id: "session".to_string(),
            tool_call_id: "call".to_string(),
            action_id: "open-reader".to_string(),
        };
        let right = ScopedHostActionId {
            origin: HostActionOrigin::native_extension("reader-b"),
            session_id: "session".to_string(),
            tool_call_id: "call".to_string(),
            action_id: "open-reader".to_string(),
        };

        assert_ne!(left, right);
    }

    #[tokio::test]
    async fn declarative_auto_action_is_sent_to_host_before_execution_when_context_exists() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let approvals = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let approvals_for_sink = approvals.clone();
        let sink: omegon_traits::HostActionApprovalSink =
            std::sync::Arc::new(move |request_json| {
                let approvals = approvals_for_sink.clone();
                Box::pin(async move {
                    approvals.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let payload = &request_json["_meta"]["omegon/hostActionApproval"];
                    assert_eq!(payload["kind"], "host_action");
                    assert_eq!(payload["origin"], "native_extension");
                    assert_eq!(payload["extension"], "reader");
                    assert_eq!(payload["server"], serde_json::Value::Null);
                    assert_eq!(payload["tool"], "reader");
                    assert_eq!(payload["tool_call_id"], "call-1");
                    assert_eq!(payload["action"]["id"], "open-reader");
                    assert_eq!(payload["action"]["type"], "terminal.create@1");
                    assert_eq!(payload["action"]["execution"], "auto_if_allowed");
                    assert_eq!(payload["action"]["params"]["command"], "bookokrat");
                    assert_eq!(payload["action"]["params"]["placement"], "side_pane");
                    serde_json::to_value(HostActionApprovalDecision::Rejected).unwrap()
                })
            });
        let context = omegon_traits::ToolExecutionContext {
            host_action_approval: Some(sink),
        };

        let outcomes = process_declarative_host_actions_with_context(
            vec![json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "execution": "auto_if_allowed",
                "params": {"command": "bookokrat", "placement": "side_pane"}
            })],
            &manifest,
            "reader",
            "call-1",
            &context,
        )
        .await;

        assert_eq!(approvals.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(outcomes[0]["status"], "denied");
        assert_eq!(outcomes[0]["error"]["code"], "operator_denied");
    }

    #[test]
    fn host_action_approval_approved_executes_through_canonical_executor() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let registry = HostActionExecutorRegistry::with_terminal_backend(Box::new(
            FakeTerminalCreateBackend {
                result: omegon_extension::actions::terminal::TerminalCreateResult {
                    terminal_id: "term-approved".to_string(),
                    backend: "fake".to_string(),
                    actual_placement: "background_session".to_string(),
                    warnings: Vec::new(),
                },
            },
        ));

        let outcome = process_host_action_candidate_with_approval_decision(
            json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "execution": "auto_if_allowed",
                "params": {"command": "bookokrat"}
            }),
            &manifest,
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry,
            HostActionApprovalDecision::Approved,
        );

        assert_eq!(outcome.status, HostActionStatus::Completed);
        assert_eq!(outcome.result.unwrap()["terminal_id"], "term-approved");
    }

    #[test]
    fn host_action_approval_rejected_does_not_call_executor() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registry =
            HostActionExecutorRegistry::with_terminal_backend(Box::new(CountingBackend {
                calls: calls.clone(),
            }));

        let outcome = process_host_action_candidate_with_approval_decision(
            json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "execution": "manual",
                "params": {"command": "bookokrat"}
            }),
            &manifest,
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry,
            HostActionApprovalDecision::Rejected,
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "operator_denied");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn host_action_approval_unavailable_denies_without_executor() {
        let manifest = terminal_manifest(&["bookokrat"], &[], &[]);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registry =
            HostActionExecutorRegistry::with_terminal_backend(Box::new(CountingBackend {
                calls: calls.clone(),
            }));

        let outcome = process_host_action_candidate_with_approval_decision(
            json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "params": {"command": "bookokrat"}
            }),
            &manifest,
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry,
            HostActionApprovalDecision::Unavailable,
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "approval_unavailable");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn host_action_approval_cannot_override_manifest_denial() {
        let outcome = process_host_action_candidate_with_approval_decision(
            json!({
                "id": "open-reader",
                "type": "terminal.create@1",
                "params": {"command": "bookokrat"}
            }),
            &manifest(&[]),
            scoped(),
            &RuntimeHostActionPolicy::default(),
            &registry(),
            HostActionApprovalDecision::Approved,
        );

        assert_eq!(outcome.status, HostActionStatus::Denied);
        assert_eq!(outcome.error.unwrap().code, "manifest_denied");
    }
}

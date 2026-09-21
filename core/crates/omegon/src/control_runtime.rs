use std::path::Path;
use std::sync::Arc;

use tokio::sync::{broadcast, oneshot};

use crate::auth;
use crate::bridge::LlmBridge;
use crate::providers;
use crate::session;
use crate::settings;
use crate::{CliRuntimeView, InteractiveAgentHost, InteractiveAgentState};
use omegon_traits::{AgentEvent, SlashCommandResponse};

pub struct ControlContext<'a> {
    pub runtime_state: &'a mut InteractiveAgentState,
    pub agent: &'a mut InteractiveAgentHost,
    pub shared_settings: &'a settings::SharedSettings,
    pub bridge: &'a Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    pub route_controller: Option<Arc<crate::route::RouteController>>,
    pub login_prompt_tx: &'a std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
    pub events_tx: &'a broadcast::Sender<AgentEvent>,
    pub cli: &'a CliRuntimeView<'a>,
    pub invocation_scope: crate::invocation_service::InvocationScope,
    pub supervisor: Option<&'a mut crate::runtime_supervisor::InteractiveRuntimeSupervisor>,
    pub dynamic_contributions:
        Option<&'a mut crate::contribution_lifecycle::DynamicContributionGenerationOwner>,
    pub dynamic_extension_publication:
        Option<&'a mut crate::contribution_lifecycle::DynamicExtensionPublicationCoordinator>,
}

pub use crate::operator_commands::InterfaceControlRequest as ControlRequest;

pub fn control_request_from_slash(
    command: &crate::runtime_commands::CanonicalSlashCommand,
) -> Option<ControlRequest> {
    Some(match command {
        crate::runtime_commands::CanonicalSlashCommand::ModelView => ControlRequest::ModelView,
        crate::runtime_commands::CanonicalSlashCommand::ModelList => ControlRequest::ModelList,
        crate::runtime_commands::CanonicalSlashCommand::ModelUnpin => {
            ControlRequest::ClearModelOverride
        }
        crate::runtime_commands::CanonicalSlashCommand::SetModel(requested_model) => {
            ControlRequest::SetModel {
                requested_model: requested_model.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::SetModelGrade(grade) => {
            ControlRequest::SetModelIntent {
                grade: grade.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::SetModelProvider(provider) => {
            ControlRequest::SetModelProvider {
                provider: provider.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::SetModelPolicy(policy) => {
            ControlRequest::SetModelPolicy {
                policy: policy.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ThinkingView => {
            ControlRequest::ThinkingView
        }
        crate::runtime_commands::CanonicalSlashCommand::SetThinking(level) => {
            ControlRequest::SetThinking { level: *level }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileView => ControlRequest::ProfileView,
        crate::runtime_commands::CanonicalSlashCommand::ProfileExport => {
            ControlRequest::ProfileExport
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileCapture(target) => {
            ControlRequest::ProfileCapture {
                target: target.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileApply => {
            ControlRequest::ProfileApply
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileUse { id, scope } => {
            ControlRequest::ProfileUse {
                id: id.clone(),
                scope: scope.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileSetMqtt(enabled) => {
            ControlRequest::ProfileSetMqtt { enabled: *enabled }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileExtensionAllow(name) => {
            ControlRequest::ProfileExtensionAllow { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileExtensionDeny(name) => {
            ControlRequest::ProfileExtensionDeny { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileExtensionClear => {
            ControlRequest::ProfileExtensionClear
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileComponentEnable(selector) => {
            ControlRequest::ProfileComponentEnable {
                selector: selector.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileComponentDisable(selector) => {
            ControlRequest::ProfileComponentDisable {
                selector: selector.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileComponentsView => {
            ControlRequest::ProfileComponentsView
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileSetPersona(name) => {
            ControlRequest::ProfileSetPersona { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ProfileSetTone(name) => {
            ControlRequest::ProfileSetTone { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::AutomationView => {
            ControlRequest::AutomationView
        }
        crate::runtime_commands::CanonicalSlashCommand::AutomationSet(level) => {
            ControlRequest::AutomationSet { level: *level }
        }
        crate::runtime_commands::CanonicalSlashCommand::PermissionsView => {
            ControlRequest::PermissionsView
        }
        crate::runtime_commands::CanonicalSlashCommand::PermissionTrustAdd(path) => {
            ControlRequest::PermissionTrustAdd { path: path.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::PermissionTrustRemove(path) => {
            ControlRequest::PermissionTrustRemove { path: path.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::StatusView => ControlRequest::StatusView,
        crate::runtime_commands::CanonicalSlashCommand::RuntimeDoctor => {
            ControlRequest::RuntimeDoctor
        }
        crate::runtime_commands::CanonicalSlashCommand::SetRuntimeMode { slim } => {
            ControlRequest::SetRuntimeMode { slim: *slim }
        }
        crate::runtime_commands::CanonicalSlashCommand::RuntimeInventoryStatus => {
            ControlRequest::RuntimeInventoryStatus
        }
        crate::runtime_commands::CanonicalSlashCommand::RuntimeSubstrateRefresh => {
            ControlRequest::RuntimeSubstrateRefresh
        }
        crate::runtime_commands::CanonicalSlashCommand::RuntimeExtensionReplace(name) => {
            ControlRequest::RuntimeExtensionReplace { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceStatusView => {
            ControlRequest::WorkspaceStatusView
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceListView => {
            ControlRequest::WorkspaceListView
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceNew(label) => {
            ControlRequest::WorkspaceNew {
                label: label.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceDestroy(target) => {
            ControlRequest::WorkspaceDestroy {
                target: target.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceAdopt => {
            ControlRequest::WorkspaceAdopt
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceRelease => {
            ControlRequest::WorkspaceRelease
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceArchive => {
            ControlRequest::WorkspaceArchive
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspacePrune => {
            ControlRequest::WorkspacePrune
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceBindMilestone(milestone_id) => {
            ControlRequest::WorkspaceBindMilestone {
                milestone_id: milestone_id.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceBindNode(design_node_id) => {
            ControlRequest::WorkspaceBindNode {
                design_node_id: design_node_id.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceBindClear => {
            ControlRequest::WorkspaceBindClear
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceRoleView => {
            ControlRequest::WorkspaceRoleView
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceRoleSet(role) => {
            ControlRequest::WorkspaceRoleSet { role: *role }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceRoleClear => {
            ControlRequest::WorkspaceRoleClear
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceKindView => {
            ControlRequest::WorkspaceKindView
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceKindSet(kind) => {
            ControlRequest::WorkspaceKindSet { kind: *kind }
        }
        crate::runtime_commands::CanonicalSlashCommand::WorkspaceKindClear => {
            ControlRequest::WorkspaceKindClear
        }
        crate::runtime_commands::CanonicalSlashCommand::SetMaxTurns { max_turns } => {
            ControlRequest::SetMaxTurns {
                max_turns: *max_turns,
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::SessionStatsView => {
            ControlRequest::SessionStatsView
        }
        crate::runtime_commands::CanonicalSlashCommand::TreeView { args } => {
            ControlRequest::TreeView { args: args.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::NoteAdd { text } => {
            ControlRequest::NoteAdd { text: text.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::NotesView => ControlRequest::NotesView,
        crate::runtime_commands::CanonicalSlashCommand::NotesClear => ControlRequest::NotesClear,
        crate::runtime_commands::CanonicalSlashCommand::CheckinView => ControlRequest::CheckinView,
        crate::runtime_commands::CanonicalSlashCommand::ContextStatus => {
            ControlRequest::ContextStatus
        }
        crate::runtime_commands::CanonicalSlashCommand::ContextCompact => {
            ControlRequest::ContextCompact
        }
        crate::runtime_commands::CanonicalSlashCommand::ContextClear => {
            ControlRequest::ContextClear
        }
        crate::runtime_commands::CanonicalSlashCommand::ContextRequest { kind, query } => {
            ControlRequest::ContextRequest {
                kind: kind.clone(),
                query: query.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ContextRequestJson(raw) => {
            ControlRequest::ContextRequestJson { raw: raw.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::SetContextClass(class) => {
            ControlRequest::SetContextClass { class: *class }
        }
        crate::runtime_commands::CanonicalSlashCommand::NewSession => ControlRequest::NewSession,
        crate::runtime_commands::CanonicalSlashCommand::ListSessions => {
            ControlRequest::ListSessions
        }
        crate::runtime_commands::CanonicalSlashCommand::ResumeSession(id) => {
            ControlRequest::ResumeSession { id: id.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::AuthView => return None,
        crate::runtime_commands::CanonicalSlashCommand::AuthStatus => ControlRequest::AuthStatus,
        crate::runtime_commands::CanonicalSlashCommand::AuthUnlock => ControlRequest::AuthUnlock,
        crate::runtime_commands::CanonicalSlashCommand::AuthLogin(provider) => {
            ControlRequest::AuthLogin {
                provider: provider.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::AuthLogout(provider) => {
            ControlRequest::AuthLogout {
                provider: provider.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::SkillsView => ControlRequest::SkillsView,
        crate::runtime_commands::CanonicalSlashCommand::SkillsHelp => ControlRequest::SkillsHelp,
        crate::runtime_commands::CanonicalSlashCommand::RuntimeProcessRestart => return None,
        crate::runtime_commands::CanonicalSlashCommand::SkillsReload => return None,
        crate::runtime_commands::CanonicalSlashCommand::SkillsInstall(name) => {
            ControlRequest::SkillsInstall { name: name.clone() }
        }
        // SkillCreate/SkillImport are handled directly in the TUI (queues a prompt) —
        // they never reach control_runtime. Return None to signal this.
        crate::runtime_commands::CanonicalSlashCommand::SkillCreate(_)
        | crate::runtime_commands::CanonicalSlashCommand::SkillImport { .. } => return None,
        crate::runtime_commands::CanonicalSlashCommand::SkillGet(name) => {
            ControlRequest::SkillGet { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::SkillDelete(name) => {
            ControlRequest::SkillDelete { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::PlanView
        | crate::runtime_commands::CanonicalSlashCommand::PlanList
        | crate::runtime_commands::CanonicalSlashCommand::PlanShow(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanSwitch(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanResume(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanBackground(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanDetach(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanPromote(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanBind(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanLedger(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanSet(_)
        | crate::runtime_commands::CanonicalSlashCommand::PlanApprove
        | crate::runtime_commands::CanonicalSlashCommand::PlanExecute
        | crate::runtime_commands::CanonicalSlashCommand::PlanAdvance
        | crate::runtime_commands::CanonicalSlashCommand::PlanSkip
        | crate::runtime_commands::CanonicalSlashCommand::PlanClear => return None,
        crate::runtime_commands::CanonicalSlashCommand::ExtensionView => {
            ControlRequest::ExtensionView
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionInit(name) => {
            ControlRequest::ExtensionInit { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionGet(name) => {
            ControlRequest::ExtensionGet { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionInstall(uri) => {
            ControlRequest::ExtensionInstall { uri: uri.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionRemove(name) => {
            ControlRequest::ExtensionRemove { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionUpdate(name) => {
            ControlRequest::ExtensionUpdate { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionEnable(name) => {
            ControlRequest::ExtensionEnable { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionDisable(name) => {
            ControlRequest::ExtensionDisable { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::ExtensionSearch(query) => {
            ControlRequest::ExtensionSearch {
                query: query.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ArmoryBrowse(query) => {
            ControlRequest::ArmoryBrowse {
                query: query.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::ArmoryInstall(target) => {
            ControlRequest::ArmoryInstall {
                target: target.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::PersonaList => ControlRequest::PersonaList,
        crate::runtime_commands::CanonicalSlashCommand::CatalogView => ControlRequest::CatalogView,
        crate::runtime_commands::CanonicalSlashCommand::CatalogInstall => {
            ControlRequest::CatalogInstall
        }
        crate::runtime_commands::CanonicalSlashCommand::CatalogRemove(id) => {
            ControlRequest::CatalogRemove { id: id.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::PluginView => ControlRequest::PluginView,
        crate::runtime_commands::CanonicalSlashCommand::PluginInstall(uri) => {
            ControlRequest::PluginInstall { uri: uri.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::PluginRemove(name) => {
            ControlRequest::PluginRemove { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::PluginUpdate(name) => {
            ControlRequest::PluginUpdate { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::SecretsView => ControlRequest::SecretsView,
        crate::runtime_commands::CanonicalSlashCommand::SecretsSet { name, value } => {
            ControlRequest::SecretsSet {
                name: name.clone(),
                value: value.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::SecretsGet(name) => {
            ControlRequest::SecretsGet { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::SecretsDelete(name) => {
            ControlRequest::SecretsDelete { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::VariablesView => {
            ControlRequest::VariablesView
        }
        crate::runtime_commands::CanonicalSlashCommand::VariablesSet { name, value } => {
            ControlRequest::VariablesSet {
                name: name.clone(),
                value: value.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::VariablesGet(name) => {
            ControlRequest::VariablesGet { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::VariablesDelete(name) => {
            ControlRequest::VariablesDelete { name: name.clone() }
        }
        crate::runtime_commands::CanonicalSlashCommand::VaultStatus => ControlRequest::VaultStatus,
        crate::runtime_commands::CanonicalSlashCommand::VaultConfigure => {
            ControlRequest::VaultConfigure
        }
        crate::runtime_commands::CanonicalSlashCommand::VaultInitPolicy => {
            ControlRequest::VaultInitPolicy
        }
        crate::runtime_commands::CanonicalSlashCommand::CleaveStatus => {
            ControlRequest::CleaveStatus
        }
        crate::runtime_commands::CanonicalSlashCommand::Smoke(command) => {
            ControlRequest::Smoke(*command)
        }
        crate::runtime_commands::CanonicalSlashCommand::CleaveCancelChild(label) => {
            ControlRequest::CleaveCancelChild {
                label: label.clone(),
            }
        }
        crate::runtime_commands::CanonicalSlashCommand::DelegateStatus => {
            ControlRequest::DelegateStatus
        }
    })
}

/// Shared handler for stateless control requests that need at most
/// shared_settings, secrets, cwd, and dashboard handles — no TUI or
/// runtime state. Called by both `execute_control` and `execute_daemon_control`.
pub(crate) async fn execute_stateless_control(
    request: &ControlRequest,
    shared_settings: &settings::SharedSettings,
    secrets: &Arc<omegon_secrets::SecretsManager>,
    cwd: &Path,
    handles: &crate::runtime_state::RuntimeStateHandles,
) -> Option<SlashCommandResponse> {
    let resp = match request {
        ControlRequest::ModelView => model_view_response(shared_settings).await,
        ControlRequest::ModelList => model_list_response().await,
        ControlRequest::ThinkingView => thinking_view_response(shared_settings).await,
        ControlRequest::AuthStatus => auth_status_response().await,
        ControlRequest::AuthUnlock => auth_unlock_response().await,
        ControlRequest::AuthLogout { provider } => {
            let resp = auth_logout_response(provider).await;
            if resp.accepted {
                let env_vars = crate::auth::provider_env_vars(provider);
                secrets.evict_secrets(env_vars);
            }
            resp
        }
        ControlRequest::SkillsView => skills_view_response().await,
        ControlRequest::SkillsHelp => skills_help_response(),
        ControlRequest::SkillsInstall { name } => skills_install_response(name.as_deref()).await,
        ControlRequest::SkillGet { name } => skill_get_response(name).await,
        ControlRequest::SkillDelete { name } => skill_delete_response(name).await,
        ControlRequest::ExtensionView => extension_view_response().await,
        ControlRequest::ExtensionInit { name } => extension_init_response(name).await,
        ControlRequest::ExtensionGet { name } => extension_get_response(name).await,
        ControlRequest::ExtensionInstall { uri } => extension_install_response(uri).await,
        ControlRequest::ExtensionRemove { name } => extension_remove_response(name).await,
        ControlRequest::ExtensionUpdate { name } => {
            extension_update_response(name.as_deref()).await
        }
        ControlRequest::ExtensionEnable { name } => extension_enable_response(name).await,
        ControlRequest::ExtensionDisable { name } => extension_disable_response(name).await,
        ControlRequest::ExtensionSearch { query } => {
            extension_search_response(query.as_deref()).await
        }
        ControlRequest::ArmoryBrowse { query } => armory_browse_response(query.as_deref()).await,
        ControlRequest::ArmoryInstall { target } => armory_install_response(target).await,
        ControlRequest::CatalogView => catalog_view_response().await,
        ControlRequest::CatalogInstall => catalog_install_response().await,
        ControlRequest::CatalogRemove { id } => catalog_remove_response(id).await,
        ControlRequest::PluginView => plugin_view_response().await,
        ControlRequest::PluginInstall { uri } => plugin_install_response(uri).await,
        ControlRequest::PluginRemove { name } => plugin_remove_response(name).await,
        ControlRequest::PluginUpdate { name } => plugin_update_response(name.as_deref()).await,
        ControlRequest::SecretsView => {
            crate::control::secrets::secrets_view_response(secrets.as_ref()).await
        }
        ControlRequest::SecretsSet { name, value } => {
            crate::control::secrets::secrets_set_response(secrets.as_ref(), name, value).await
        }
        ControlRequest::SecretsGet { name } => {
            crate::control::secrets::secrets_get_response(secrets.as_ref(), name).await
        }
        ControlRequest::SecretsDelete { name } => {
            crate::control::secrets::secrets_delete_response(secrets.as_ref(), name).await
        }
        ControlRequest::VariablesView => crate::control::variables::variables_view_response().await,
        ControlRequest::VariablesSet { name, value } => {
            crate::control::variables::variables_set_response(name, value).await
        }
        ControlRequest::VariablesGet { name } => {
            crate::control::variables::variables_get_response(name).await
        }
        ControlRequest::VariablesDelete { name } => {
            crate::control::variables::variables_delete_response(name).await
        }
        ControlRequest::VaultUnseal => vault_unseal_response().await,
        ControlRequest::VaultLogin => vault_login_response().await,
        ControlRequest::VaultConfigure => vault_configure_response().await,
        ControlRequest::VaultInitPolicy => vault_init_policy_response().await,
        ControlRequest::SetMaxTurns { max_turns } => {
            set_max_turns_response(shared_settings, cwd, *max_turns).await
        }
        ControlRequest::ProfileView => profile_view_response(shared_settings, cwd).await,
        ControlRequest::ProfileExport => {
            profile_export_response(shared_settings, cwd, handles).await
        }
        ControlRequest::ProfileCapture { target } => {
            profile_capture_response(shared_settings, cwd, target.clone()).await
        }
        ControlRequest::ProfileSetMqtt { enabled } => {
            profile_set_mqtt_response(cwd, *enabled).await
        }
        ControlRequest::ProfileExtensionAllow { name } => {
            profile_extension_allow_response(cwd, name).await
        }
        ControlRequest::ProfileExtensionDeny { name } => {
            profile_extension_deny_response(cwd, name).await
        }
        ControlRequest::ProfileExtensionClear => profile_extension_clear_response(cwd).await,
        ControlRequest::ProfileComponentEnable { selector } => {
            profile_component_mutation_response(cwd, selector, true).await
        }
        ControlRequest::ProfileComponentDisable { selector } => {
            profile_component_mutation_response(cwd, selector, false).await
        }
        ControlRequest::ProfileComponentsView => profile_components_view_response(cwd).await,
        ControlRequest::ProfileSetPersona { name } => {
            profile_set_persona_response(cwd, name.as_deref()).await
        }
        ControlRequest::ProfileSetTone { name } => {
            profile_set_tone_response(cwd, name.as_deref()).await
        }
        ControlRequest::AutomationView => automation_view_response(shared_settings, cwd).await,
        ControlRequest::AutomationSet { level } => {
            automation_set_response(shared_settings, cwd, *level).await
        }
        ControlRequest::PermissionsView => permissions_view_response(shared_settings, cwd).await,
        ControlRequest::PermissionTrustAdd { path } => {
            permission_trust_add_response(shared_settings, cwd, path).await
        }
        ControlRequest::PermissionTrustRemove { path } => {
            permission_trust_remove_response(shared_settings, cwd, path).await
        }
        ControlRequest::PersonaList => persona_list_response(handles, cwd).await,
        ControlRequest::PersonaSwitch { name } => persona_switch_response(name).await,
        _ => return None,
    };
    Some(resp)
}

pub struct HarnessControlContext<'a> {
    pub shared_settings: &'a settings::SharedSettings,
    pub secrets: &'a Arc<omegon_secrets::SecretsManager>,
    pub cwd: &'a Path,
    pub dashboard_handles: &'a crate::runtime_state::RuntimeStateHandles,
    pub route_controller: Option<Arc<crate::route::RouteController>>,
    pub dynamic_contribution_control:
        Option<&'a crate::contribution_lifecycle::DynamicContributionControl>,
}

pub enum ActiveHarnessCommandResult {
    Handled,
    Unsupported(crate::operator_commands::OperatorCommand),
}

/// Execute an inference-independent typed command while an agent worker is
/// active. Unsupported commands are returned intact so the coordinator can
/// preserve ordering without duplicating response plumbing.
pub async fn execute_active_harness_command(
    ctx: &HarnessControlContext<'_>,
    command: crate::operator_commands::OperatorCommand,
    events_tx: &broadcast::Sender<AgentEvent>,
) -> ActiveHarnessCommandResult {
    use crate::operator_commands::{InterfaceControlRequest, OperatorCommand};

    let (request, respond_to) = match command {
        OperatorCommand::ModelView { respond_to } => {
            (InterfaceControlRequest::ModelView, respond_to)
        }
        OperatorCommand::ModelList { respond_to } => {
            (InterfaceControlRequest::ModelList, respond_to)
        }
        OperatorCommand::ModelUnpin { respond_to } => {
            (InterfaceControlRequest::ClearModelOverride, respond_to)
        }
        OperatorCommand::SetModelGrade { grade, respond_to } => (
            InterfaceControlRequest::SetModelIntent { grade },
            respond_to,
        ),
        OperatorCommand::SetModelProvider {
            provider,
            respond_to,
        } => (
            InterfaceControlRequest::SetModelProvider { provider },
            respond_to,
        ),
        OperatorCommand::SetModelPolicy { policy, respond_to } => (
            InterfaceControlRequest::SetModelPolicy { policy },
            respond_to,
        ),
        OperatorCommand::SetThinking { level, respond_to } => {
            (InterfaceControlRequest::SetThinking { level }, respond_to)
        }
        OperatorCommand::RunSlashCommand {
            name,
            args,
            respond_to,
        } => {
            let Some(canonical) = crate::runtime_commands::canonical_slash_command(&name, &args)
            else {
                return ActiveHarnessCommandResult::Unsupported(OperatorCommand::RunSlashCommand {
                    name,
                    args,
                    respond_to,
                });
            };
            let Some(request) = control_request_from_slash(&canonical) else {
                return ActiveHarnessCommandResult::Unsupported(OperatorCommand::RunSlashCommand {
                    name,
                    args,
                    respond_to,
                });
            };
            let Some(response) = execute_harness_control(ctx, &request).await else {
                return ActiveHarnessCommandResult::Unsupported(OperatorCommand::RunSlashCommand {
                    name,
                    args,
                    respond_to,
                });
            };
            if let Some(output) = response.output.clone() {
                let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
            }
            if let Some(reply) = respond_to {
                let _ = reply.send(response);
            }
            return ActiveHarnessCommandResult::Handled;
        }
        OperatorCommand::ExecuteControl {
            request,
            respond_to,
        } => {
            let Some(response) = execute_harness_control(ctx, &request).await else {
                return ActiveHarnessCommandResult::Unsupported(OperatorCommand::ExecuteControl {
                    request,
                    respond_to,
                });
            };
            finish_active_harness_response(response, respond_to, events_tx);
            return ActiveHarnessCommandResult::Handled;
        }
        OperatorCommand::ExecuteControlFrom {
            request,
            respond_to,
            surface,
        } => {
            let Some(response) = execute_harness_control(ctx, &request).await else {
                return ActiveHarnessCommandResult::Unsupported(
                    OperatorCommand::ExecuteControlFrom {
                        request,
                        respond_to,
                        surface,
                    },
                );
            };
            finish_active_harness_response(response, respond_to, events_tx);
            return ActiveHarnessCommandResult::Handled;
        }
        other => return ActiveHarnessCommandResult::Unsupported(other),
    };

    let response = execute_harness_control(ctx, &request)
        .await
        .expect("typed harness command must have a supervisor-owned handler");
    finish_active_harness_response(response, respond_to, events_tx);
    ActiveHarnessCommandResult::Handled
}

fn finish_active_harness_response(
    response: SlashCommandResponse,
    respond_to: Option<oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    events_tx: &broadcast::Sender<AgentEvent>,
) {
    if let Some(output) = response.output.clone() {
        let _ = events_tx.send(AgentEvent::SystemNotification { message: output });
    }
    if let Some(reply) = respond_to {
        let _ = reply.send(omegon_traits::ControlOutputResponse {
            accepted: response.accepted,
            output: response.output,
        });
    }
}

/// Execute controls whose dependencies are entirely supervisor-owned and do
/// not require conversation, context, or an inference worker.
pub async fn execute_harness_control(
    ctx: &HarnessControlContext<'_>,
    request: &ControlRequest,
) -> Option<SlashCommandResponse> {
    if let Some(response) = execute_stateless_control(
        request,
        ctx.shared_settings,
        ctx.secrets,
        ctx.cwd,
        ctx.dashboard_handles,
    )
    .await
    {
        return Some(response);
    }

    Some(match request {
        ControlRequest::SetModelIntent { grade } => {
            set_model_intent_control_response(ctx.route_controller.clone(), ctx.cwd, grade).await
        }
        ControlRequest::SetModelProvider { provider } => {
            set_model_provider_control_response(ctx.route_controller.clone(), ctx.cwd, provider)
                .await
        }
        ControlRequest::SetModelPolicy { policy } => {
            set_model_policy_control_response(ctx.route_controller.clone(), ctx.cwd, policy).await
        }
        ControlRequest::SetThinking { level } => {
            set_thinking_response(ctx.shared_settings, ctx.cwd, *level).await
        }
        ControlRequest::RuntimeDoctor => runtime_doctor_response(ctx.dynamic_contribution_control),
        ControlRequest::RuntimeExtensionReplace { name } => {
            runtime_extension_replace_response(ctx.dynamic_contribution_control, name).await
        }
        ControlRequest::ClearModelOverride => {
            let controller = ctx.route_controller.as_ref()?;
            let snapshot = controller.clear_exact_model_override().await;
            if let Err(error) = settings::persist_model_intent(ctx.cwd, &snapshot.intent) {
                return Some(SlashCommandResponse {
                    accepted: false,
                    output: Some(format!(
                        "Model override cleared in memory but persistence failed: {error}"
                    )),
                });
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(format!(
                    "Model exact override cleared — {}",
                    snapshot.intent.summary()
                )),
            }
        }
        _ => return None,
    })
}

pub(crate) fn runtime_doctor_response(
    control: Option<&crate::contribution_lifecycle::DynamicContributionControl>,
) -> SlashCommandResponse {
    let Some(control) = control else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Runtime diagnostics are unavailable on this surface.".into()),
        };
    };
    let health = control.extension_health();
    if health.is_empty() {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Runtime doctor: no published extension processes.".into()),
        };
    }

    let mut lines = vec!["Runtime doctor".to_string()];
    for extension in health {
        let pid = extension
            .pid
            .map(|pid| format!(" (pid {pid})"))
            .unwrap_or_default();
        match extension.state {
            crate::extensions::ExtensionProcessState::Healthy => {
                lines.push(format!("- {}: healthy{pid}", extension.name));
            }
            crate::extensions::ExtensionProcessState::Replacing => {
                lines.push(format!(
                    "- {}: replacement in progress{pid}",
                    extension.name
                ));
            }
            crate::extensions::ExtensionProcessState::ShuttingDown => {
                lines.push(format!("- {}: shutting down{pid}", extension.name));
            }
            crate::extensions::ExtensionProcessState::Unavailable => {
                let detail = extension
                    .detail
                    .as_deref()
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "- {}: unavailable{detail}. Recommended: `/runtime replace {}`",
                    extension.name, extension.name
                ));
            }
        }
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(lines.join("\n")),
    }
}

pub(crate) async fn runtime_extension_replace_response(
    control: Option<&crate::contribution_lifecycle::DynamicContributionControl>,
    name: &str,
) -> SlashCommandResponse {
    let Some(control) = control else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Runtime replacement is unavailable on this surface.".into()),
        };
    };
    match control.replace_extension(name).await {
        Ok(pid) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Replaced extension `{name}` from its admitted snapshot (pid {pid})."
            )),
        },
        Err(error) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Could not replace extension `{name}`: {error}")),
        },
    }
}

pub async fn execute_control(
    ctx: &mut ControlContext<'_>,
    request: ControlRequest,
) -> SlashCommandResponse {
    // Try stateless handlers first (shared with daemon mode).
    if let Some(resp) = execute_harness_control(
        &HarnessControlContext {
            shared_settings: ctx.shared_settings,
            secrets: &ctx.agent.secrets,
            cwd: &ctx.agent.cwd,
            dashboard_handles: &ctx.agent.dashboard_handles,
            route_controller: ctx.route_controller.clone(),
            dynamic_contribution_control: Some(&ctx.agent.dynamic_contribution_control),
        },
        &request,
    )
    .await
    {
        return resp;
    }

    match request {
        ControlRequest::SetModel { requested_model } => {
            let inventory = ctx.runtime_state.inference_runtime.snapshot().await;
            set_model_response(
                ctx.agent,
                ctx.shared_settings,
                ctx.bridge,
                ctx.route_controller.clone(),
                &requested_model,
                &inventory,
            )
            .await
        }
        ControlRequest::SetModelIntent { grade } => {
            set_model_intent_control_response(ctx.route_controller.clone(), &ctx.agent.cwd, &grade).await
        }
        ControlRequest::SetModelProvider { provider } => {
            set_model_provider_control_response(ctx.route_controller.clone(), &ctx.agent.cwd, &provider).await
        }
        ControlRequest::SetModelPolicy { policy } => {
            set_model_policy_control_response(ctx.route_controller.clone(), &ctx.agent.cwd, &policy).await
        }
        ControlRequest::ClearModelOverride => SlashCommandResponse {
            accepted: true,
            output: Some("Model exact override clear requested; interactive route state clears this through /model unpin.".into()),
        },
        ControlRequest::SwitchDispatcher {
            request_id,
            profile,
            model,
        } => {
            switch_dispatcher_response(
                ctx.agent,
                ctx.shared_settings,
                ctx.bridge,
                &request_id,
                &profile,
                model.as_deref(),
                ctx.events_tx,
            )
            .await
        }
        ControlRequest::SetThinking { level } => {
            set_thinking_response(ctx.shared_settings, &ctx.agent.cwd, level).await
        }
        ControlRequest::ProfileApply => {
            profile_apply_response(
                ctx.agent,
                ctx.runtime_state,
                ctx.shared_settings,
                ctx.bridge,
                ctx.route_controller.clone(),
                ctx.events_tx,
            )
            .await
        }
        ControlRequest::ProfileUse { id, scope } => {
            if let Err(error) = settings::save_project_active_profile_selection(
                &ctx.agent.cwd,
                &settings::ActiveProfileSelection {
                    id: id.clone(),
                    scope: scope.clone(),
                },
            ) {
                SlashCommandResponse {
                    accepted: false,
                    output: Some(format!("Failed to select profile: {error}")),
                }
            } else {
                let mut response = profile_apply_response(
                    ctx.agent,
                    ctx.runtime_state,
                    ctx.shared_settings,
                    ctx.bridge,
                    ctx.route_controller.clone(),
                    ctx.events_tx,
                )
                .await;
                if response.accepted {
                    if let Some(output) = response.output.as_mut() {
                        output.insert_str(0, &format!("Profile selected: `{id}`.\n\n"));
                    }
                } else if let Some(output) = response.output.as_mut() {
                    output.insert_str(
                        0,
                        &format!(
                            "Profile `{id}` was selected for the next startup but was not applied to the live runtime.\n\n"
                        ),
                    );
                }
                response
            }
        }
        ControlRequest::StatusView => {
            status_view_response(ctx.runtime_state, ctx.agent, ctx.shared_settings).await
        }
        ControlRequest::RuntimeInventoryStatus => {
            runtime_inventory_status_response(ctx.runtime_state).await
        }
        ControlRequest::RuntimeSubstrateRefresh => {
            runtime_substrate_refresh_with_generations(ctx).await
        }
        ControlRequest::WorkspaceStatusView => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_status_view_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceListView => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_list_view_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceNew { label } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_new_response(&workspace_ctx, &label).await
        }
        ControlRequest::WorkspaceDestroy { target } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_destroy_response(&workspace_ctx, &target).await
        }
        ControlRequest::WorkspaceAdopt => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_adopt_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceRelease => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_release_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceArchive => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_archive_response(&workspace_ctx)
        }
        ControlRequest::WorkspacePrune => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_prune_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceBindMilestone { milestone_id } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_bind_milestone_response(
                &workspace_ctx,
                &milestone_id,
            )
        }
        ControlRequest::WorkspaceBindNode { design_node_id } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_bind_node_response(&workspace_ctx, &design_node_id)
        }
        ControlRequest::WorkspaceBindClear => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_bind_clear_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceRoleView => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_role_view_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceRoleSet { role } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_role_set_response(&workspace_ctx, role)
        }
        ControlRequest::WorkspaceRoleClear => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_role_clear_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceKindView => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_kind_view_response(&workspace_ctx)
        }
        ControlRequest::WorkspaceKindSet { kind } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_kind_set_response(&workspace_ctx, kind)
        }
        ControlRequest::WorkspaceKindClear => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_kind_clear_response(&workspace_ctx)
        }
        ControlRequest::SessionStatsView => {
            session_stats_view_response(ctx.runtime_state, ctx.shared_settings, ctx.agent).await
        }
        ControlRequest::TreeView { args } => {
            tree_view_response(ctx.runtime_state, &args, &ctx.invocation_scope).await
        }
        ControlRequest::NoteAdd { text } => note_add_response(ctx.agent, &text).await,
        ControlRequest::NotesView => notes_view_response(ctx.agent).await,
        ControlRequest::NotesClear => notes_clear_response(ctx.agent).await,
        ControlRequest::CheckinView => checkin_view_response(ctx.agent, ctx.runtime_state).await,
        ControlRequest::ContextStatus => {
            context_status_response(ctx.runtime_state, ctx.shared_settings).await
        }
        ControlRequest::ContextCompact => {
            context_compact_response(
                ctx.runtime_state,
                ctx.agent,
                ctx.shared_settings,
                ctx.bridge,
                ctx.events_tx,
                &ctx.invocation_scope,
            )
            .await
        }
        ControlRequest::ContextClear => {
            context_clear_response(
                ctx.runtime_state,
                ctx.agent,
                ctx.cli,
                ctx.events_tx,
                ctx.supervisor.as_deref_mut(),
            )
            .await
        }
        ControlRequest::ContextRequest { kind, query } => {
            context_request_response(ctx.runtime_state, &kind, &query).await
        }
        ControlRequest::ContextRequestJson { raw } => {
            context_request_json_response(ctx.runtime_state, &raw).await
        }
        ControlRequest::SetContextClass { class } => {
            set_context_class_response(ctx.agent, ctx.shared_settings, class).await
        }
        ControlRequest::SetRuntimeMode { slim } => {
            set_runtime_mode_response(ctx.runtime_state, ctx.shared_settings, ctx.events_tx, slim)
                .await
        }
        ControlRequest::SetPresentationLevel { level } => {
            let persisted = ctx
                .shared_settings
                .lock()
                .map(|mut settings| settings.ui_presentation = level)
                .is_ok();
            SlashCommandResponse {
                accepted: persisted,
                output: Some(if persisted {
                    format!(
                        "UI presentation → {} (client projection; runtime posture unchanged)",
                        level.name()
                    )
                } else {
                    "UI presentation update failed: settings lock unavailable".to_string()
                }),
            }
        },
        ControlRequest::NewSession => {
            new_session_response(
                ctx.runtime_state,
                ctx.agent,
                ctx.cli,
                ctx.events_tx,
                ctx.supervisor.as_deref_mut(),
            )
            .await
        }
        ControlRequest::ListSessions => list_sessions_response(ctx.agent).await,
        ControlRequest::ResumeSession { id } => {
            resume_session_response(
                ctx.runtime_state,
                ctx.agent,
                ctx.cli,
                ctx.events_tx,
                &id,
                ctx.supervisor.as_deref_mut(),
            )
            .await
        }
        ControlRequest::AuthLogin { provider } => {
            auth_login_response(
                ctx.shared_settings,
                ctx.bridge,
                ctx.login_prompt_tx,
                ctx.events_tx,
                &provider,
                AuthLoginRouteContext {
                    cwd: &ctx.agent.cwd,
                    fallback_model: ctx.cli.model,
                    inference_runtime: &ctx.runtime_state.inference_runtime,
                    secrets: &ctx.agent.secrets,
                },
            )
            .await
        }
        ControlRequest::VaultStatus => vault_status_response(ctx.agent).await,
        ControlRequest::CleaveStatus => {
            cleave_status_response(ctx.runtime_state, &ctx.invocation_scope).await
        }
        ControlRequest::Smoke(crate::smoke_surface::SmokeCommand::List) => SlashCommandResponse {
            accepted: true,
            output: Some(crate::smoke_surface::smoke_list_text()),
        },
        ControlRequest::Smoke(crate::smoke_surface::SmokeCommand::Scenario(scenario)) => {
            crate::smoke_surface::launch_surface_smoke(
                &mut ctx.agent.dashboard_handles,
                scenario,
                Some(ctx.events_tx.clone()),
                None,
            )
        }
        ControlRequest::CleaveCancelChild { label } => {
            cleave_cancel_child_response(ctx.runtime_state, &label, &ctx.invocation_scope).await
        }
        ControlRequest::DelegateStatus => {
            delegate_status_response(ctx.runtime_state, &ctx.invocation_scope).await
        }
        // Stateless variants already handled above; catch remaining
        other => SlashCommandResponse {
            accepted: false,
            output: Some(format!("unhandled control request: {:?}", other)),
        },
    }
}

/// Lightweight control executor for daemon mode. Handles operations that
/// don't require TUI-specific state (InteractiveAgentState, InteractiveAgentHost).
pub async fn execute_daemon_control(
    request: ControlRequest,
    shared_settings: &settings::SharedSettings,
    secrets: &Arc<omegon_secrets::SecretsManager>,
    cwd: &Path,
    handles: &crate::runtime_state::RuntimeStateHandles,
    events_tx: &broadcast::Sender<AgentEvent>,
) -> omegon_traits::ControlOutputResponse {
    let is_settings_mutation = matches!(
        request,
        ControlRequest::SetModel { .. }
            | ControlRequest::SetModelIntent { .. }
            | ControlRequest::SetModelProvider { .. }
            | ControlRequest::SetModelPolicy { .. }
            | ControlRequest::ClearModelOverride
            | ControlRequest::SetThinking { .. }
            | ControlRequest::SetContextClass { .. }
            | ControlRequest::SetRuntimeMode { .. }
            | ControlRequest::SetPresentationLevel { .. }
            | ControlRequest::SetMaxTurns { .. }
            | ControlRequest::ProfileApply
            | ControlRequest::ProfileUse { .. }
            | ControlRequest::ProfileCapture { .. }
            | ControlRequest::ProfileSetMqtt { .. }
            | ControlRequest::ProfileExtensionAllow { .. }
            | ControlRequest::ProfileExtensionDeny { .. }
            | ControlRequest::ProfileExtensionClear
            | ControlRequest::ProfileComponentEnable { .. }
            | ControlRequest::ProfileComponentDisable { .. }
            | ControlRequest::ProfileSetPersona { .. }
            | ControlRequest::ProfileSetTone { .. }
    );
    // Try stateless handlers first (shared with TUI mode).
    let resp = if let Some(resp) =
        execute_stateless_control(&request, shared_settings, secrets, cwd, handles).await
    {
        resp
    } else {
        match request {
            // ── Daemon-specific overrides (different handler than TUI) ──
            ControlRequest::SetModel { requested_model } => {
                set_model_daemon_response(shared_settings, cwd, &requested_model).await
            }
            ControlRequest::SetModelIntent { grade } => set_model_intent_response(&grade),
            ControlRequest::SetModelProvider { provider } => set_model_provider_response(&provider),
            ControlRequest::SetModelPolicy { policy } => set_model_policy_response(&policy),
            ControlRequest::ClearModelOverride => SlashCommandResponse {
                accepted: true,
                output: Some("Model exact override clear requested; daemon route state does not yet persist model intent.".into()),
            },
            ControlRequest::SetThinking { level } => {
                set_thinking_daemon_response(shared_settings, cwd, level).await
            }
            ControlRequest::SetContextClass { class } => {
                set_context_class_daemon_response(shared_settings, cwd, class).await
            }
            ControlRequest::SetRuntimeMode { slim } => {
                set_runtime_mode_daemon_response(shared_settings, cwd, slim).await
            }
            ControlRequest::SetPresentationLevel { level } => {
                let persisted = shared_settings
                    .lock()
                    .map(|mut settings| settings.ui_presentation = level)
                    .is_ok();
                SlashCommandResponse {
                    accepted: persisted,
                    output: Some(if persisted {
                        format!(
                            "UI presentation → {} (client projection; runtime posture unchanged)",
                            level.name()
                        )
                    } else {
                        "UI presentation update failed: settings lock unavailable".to_string()
                    }),
                }
            },
            ControlRequest::ProfileApply => profile_apply_daemon_response(shared_settings, cwd).await,
            ControlRequest::ProfileUse { id, scope } => {
                let selection = settings::ActiveProfileSelection { id, scope };
                match settings::save_project_active_profile_selection(cwd, &selection) {
                    Ok(_) => {
                        let mut response = profile_apply_daemon_response(shared_settings, cwd).await;
                        if let Some(output) = response.output.as_mut() {
                            output.insert_str(0, &format!("Profile selected: `{}`.\n\n", selection.id));
                        }
                        response
                    }
                    Err(error) => SlashCommandResponse {
                        accepted: false,
                        output: Some(format!("Failed to select profile: {error}")),
                    },
                }
            }
            ControlRequest::AuthLogin { provider } => auth_login_daemon_response(&provider).await,
            ControlRequest::ListSessions => {
                let msg = list_sessions_message(cwd);
                SlashCommandResponse {
                    accepted: true,
                    output: Some(msg),
                }
            }
            // ── Operations requiring TUI state ──────────────────────────
            other => SlashCommandResponse {
                accepted: false,
                output: Some(format!("/{:?} requires interactive mode", other)),
            },
        }
    };
    // Emit HarnessStatusChanged for mutations so WebSocket/IPC clients see
    // updated state without polling.
    if resp.accepted
        && is_settings_mutation
        && let Ok(Some(((), status))) = handles.mutate_harness(|status| {
            // Refresh settings-derived fields in the live harness status.
            if let Ok(s) = shared_settings.lock() {
                status.context_class = s.effective_requested_class().label().to_string();
                status.thinking_level = s.thinking.as_str().to_string();
            }
        })
        && let Ok(status_json) = serde_json::to_value(status)
    {
        let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json });
    }
    omegon_traits::ControlOutputResponse {
        accepted: resp.accepted,
        output: resp.output,
    }
}

pub fn list_sessions_message(cwd: &Path) -> String {
    let sessions = session::list_sessions(cwd);
    if sessions.is_empty() {
        "No saved sessions for this directory.".to_string()
    } else {
        let lines: Vec<String> = sessions
            .iter()
            .take(10)
            .map(|s| {
                format!(
                    "  {} — {} — {} turns, {} tools — id {}",
                    session::session_display_name(&s.meta),
                    session::session_display_description(&s.meta),
                    s.meta.turns,
                    s.meta.tool_calls,
                    s.meta.session_id
                )
            })
            .collect();
        format!("Recent sessions:\n{}", lines.join("\n"))
    }
}

pub async fn model_view_response(
    shared_settings: &settings::SharedSettings,
) -> SlashCommandResponse {
    let s = shared_settings.lock().unwrap().clone();
    let provider = s.provider().to_string();
    let connected = if s.provider_connected { "Yes" } else { "No" };
    let thinking = {
        let raw = s.thinking.as_str();
        let mut chars = raw.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Model\n  Current Model:   {}\n  Provider:        {}\n  Connected:       {}\n  Context Window:  {} tokens\n  Context Class:   {}\n  Thinking Level:  {}\n\nActions\n  /model list                Show available models\n  /model <provider:model>    Switch model\n  /think <level>             Change reasoning depth\n  /context                   Show context posture",
            s.model,
            provider,
            connected,
            s.context_window,
            s.context_class.label(),
            thinking,
        )),
    }
}

pub async fn model_list_response() -> SlashCommandResponse {
    let catalog = crate::model_catalog::ModelCatalog::discover();
    let grouped = catalog.by_conceptual_model();
    let mut output = String::from("Available Models\n");
    if !catalog.freshness.is_empty() {
        output.push_str("\nInventory freshness (refresh with /runtime refresh):\n");
        for (provider, state) in &catalog.freshness {
            output.push_str(&format!("  {provider}: {state}\n"));
        }
    }
    for (conceptual_model_id, routes) in grouped {
        output.push_str(&format!("\n{}\n", conceptual_model_id));
        for model in routes {
            let producer = model.producer.as_deref().unwrap_or("unknown");
            let execution_class = model.execution_class.as_deref().unwrap_or("unknown");
            let availability = if model.available {
                "available"
            } else {
                "unavailable"
            };
            output.push_str(&format!(
                "  {} ({}) — provider={}, producer={}, execution={}, {}, admission={}\n",
                model.name,
                model.id,
                model.provider,
                producer,
                execution_class,
                availability,
                model.admission.as_str()
            ));
        }
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

pub(crate) async fn set_model_intent_control_response(
    route_controller: Option<Arc<crate::route::RouteController>>,
    cwd: &std::path::Path,
    grade: &str,
) -> SlashCommandResponse {
    let Some(controller) = route_controller else {
        return set_model_intent_response(grade);
    };
    let Some(parsed) = crate::route::ModelGrade::parse(grade) else {
        return set_model_intent_response(grade);
    };
    let snapshot = controller
        .set_model_intent(crate::route::ModelIntent::with_grade(parsed))
        .await;
    let persist_note = settings::persist_model_intent(cwd, &snapshot.intent)
        .err()
        .map(|err| format!(" Failed to persist model intent: {err}"))
        .unwrap_or_default();
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Model intent updated — {}.{persist_note}",
            snapshot.intent.summary()
        )),
    }
}

pub(crate) async fn set_model_provider_control_response(
    route_controller: Option<Arc<crate::route::RouteController>>,
    cwd: &std::path::Path,
    provider: &str,
) -> SlashCommandResponse {
    let Some(controller) = route_controller else {
        return set_model_provider_response(provider);
    };
    let Some(selection) = crate::route::ProviderSelection::parse(provider) else {
        return set_model_provider_response(provider);
    };
    let snapshot = controller.set_provider_selection(selection).await;
    let persist_note = settings::persist_model_intent(cwd, &snapshot.intent)
        .err()
        .map(|err| format!(" Failed to persist model intent: {err}"))
        .unwrap_or_default();
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Model provider intent updated — {}.{persist_note}",
            snapshot.intent.summary()
        )),
    }
}

pub(crate) async fn set_model_policy_control_response(
    route_controller: Option<Arc<crate::route::RouteController>>,
    cwd: &std::path::Path,
    policy: &str,
) -> SlashCommandResponse {
    let Some(controller) = route_controller else {
        return set_model_policy_response(policy);
    };
    if let Some(provider_policy) = crate::semantic_route::ProviderPolicy::parse(policy) {
        let snapshot = controller.set_provider_policy(Some(provider_policy)).await;
        let persist_note = settings::persist_model_intent(cwd, &snapshot.intent)
            .err()
            .map(|err| format!(" Failed to persist model intent: {err}"))
            .unwrap_or_default();
        return SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Model provider policy updated — {}.{persist_note}",
                snapshot.intent.summary()
            )),
        };
    }
    let Some(grade_policy) = crate::route::GradePolicy::parse(policy) else {
        return set_model_policy_response(policy);
    };
    let snapshot = controller.set_grade_policy(grade_policy).await;
    let persist_note = settings::persist_model_intent(cwd, &snapshot.intent)
        .err()
        .map(|err| format!(" Failed to persist model intent: {err}"))
        .unwrap_or_default();
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Model grade policy updated — {}.{persist_note}",
            snapshot.intent.summary()
        )),
    }
}

fn set_model_policy_response(policy: &str) -> SlashCommandResponse {
    match crate::route::GradePolicy::parse(policy) {
        Some(parsed) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Model grade policy intent requested — {}. Interactive route state will preserve this intent without pinning a concrete model.",
                crate::route::ModelIntent {
                    grade_policy: parsed,
                    exact_model_override: None,
                    ..crate::route::ModelIntent::default()
                }
                .summary()
            )),
        },
        None => SlashCommandResponse {
            accepted: false,
            output: Some("Invalid model policy. Use exact, minimum, or nearest.".into()),
        },
    }
}

fn set_model_provider_response(provider: &str) -> SlashCommandResponse {
    match crate::route::ProviderSelection::parse(provider) {
        Some(selection) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Model provider intent requested — {}. Interactive route state will preserve this intent without pinning a concrete model.",
                crate::route::ModelIntent {
                    provider_selection: selection,
                    exact_model_override: None,
                    ..crate::route::ModelIntent::default()
                }
                .summary()
            )),
        },
        None => SlashCommandResponse {
            accepted: false,
            output: Some(
                "Invalid model provider selector. Use auto, local, upstream, or an endpoint id."
                    .into(),
            ),
        },
    }
}

fn set_model_intent_response(grade: &str) -> SlashCommandResponse {
    match crate::route::ModelGrade::parse(grade) {
        Some(parsed) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Model intent requested — grade {}, provider auto. Interactive route state will preserve this intent without pinning a concrete model.",
                parsed.as_str()
            )),
        },
        None => SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Invalid model grade: {grade}. Use F, D, C, B, A, or S. Use /model provider local for local endpoints."
            )),
        },
    }
}

pub async fn set_model_response(
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    route_controller: Option<Arc<crate::route::RouteController>>,
    requested_model: &str,
    inventory: &crate::inference_inventory::InventorySnapshot,
) -> SlashCommandResponse {
    let intent_policy = if let Some(controller) = route_controller.as_ref() {
        controller.snapshot().await.intent.to_provider_policy()
    } else {
        crate::semantic_route::ProviderPolicy::Auto
    };
    let effective_model = crate::semantic_route::resolve_semantic_model_route(
        crate::model_registry::ModelRegistry::global(),
        requested_model,
        intent_policy,
    )
    .map(|route| route.qualified_model)
    .ok()
    .unwrap_or_else(|| requested_model.to_string());
    let (old_model, old_provider) = shared_settings
        .lock()
        .ok()
        .map(|s| {
            (
                s.model.clone(),
                crate::providers::infer_provider_id(&s.model),
            )
        })
        .unwrap_or_else(|| (String::new(), String::new()));
    let new_provider = crate::providers::infer_provider_id(&effective_model);
    if let Some(controller) = route_controller {
        if let Err(rejection) =
            crate::provider_route_service::admit_exact_route(inventory, &effective_model, &[])
        {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!(
                    "Model switch to {effective_model} refused by active inventory: {rejection}"
                )),
            };
        }
        let new_bridge = crate::session_execution::boot_execution_binding()
            .resolve_exact_admitted_provider_route(
                &effective_model,
                Some(agent.secrets.as_ref()),
                inventory,
                &[],
            )
            .await
            .map(crate::provider_route_service::ResolvedProviderRoute::into_unleased_bridge);
        let snapshot = match controller
            .switch_model(
                effective_model.clone(),
                &crate::route::CredentialLedger,
                new_bridge,
            )
            .await
        {
            Ok(snapshot) => snapshot,
            Err(err) => {
                return SlashCommandResponse {
                    accepted: false,
                    output: Some(format!("Model switch failed: {err}")),
                };
            }
        };
        let serving_matches = snapshot.serving_model() == Some(effective_model.as_str());
        if !serving_matches {
            return SlashCommandResponse {
                accepted: false,
                output: Some(snapshot.operator_status()),
            };
        }
        if let Ok(mut s) = shared_settings.lock() {
            s.set_model(&effective_model);
            s.provider_connected = serving_matches;
            let mut profile = settings::Profile::load(&agent.cwd);
            profile.capture_from(&s);
            let _ = profile.save(&agent.cwd);
        }
        let provider_label = crate::auth::provider_by_id(&new_provider)
            .map(|p| p.display_name)
            .unwrap_or(new_provider.as_str());
        let mut messages = Vec::new();
        if effective_model != requested_model {
            messages.push(format!(
                "Requested {requested_model}; using executable route {effective_model} via {provider_label}."
            ));
        }
        messages.push(format!(
            "Provider route switched to {provider_label} ({effective_model})."
        ));
        return SlashCommandResponse {
            accepted: true,
            output: Some(messages.join("\n")),
        };
    }
    if let Ok(mut s) = shared_settings.lock() {
        s.set_model(&effective_model);
        let mut profile = settings::Profile::load(&agent.cwd);
        profile.capture_from(&s);
        let _ = profile.save(&agent.cwd);
    }
    let mut messages = Vec::new();
    if effective_model != requested_model {
        let provider_label = crate::auth::provider_by_id(&new_provider)
            .map(|p| p.display_name)
            .unwrap_or(new_provider.as_str());
        messages.push(format!(
            "Requested {requested_model}; using executable route {effective_model} via {provider_label}."
        ));
    }
    if old_provider != new_provider {
        let provider = crate::providers::infer_provider_id(&effective_model);
        if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await {
            let mut guard = bridge.write().await;
            *guard = new_bridge;
            if let Ok(mut s) = shared_settings.lock() {
                s.provider_connected = crate::auth::provider_connected_for_model(&effective_model);
            }
            let provider_label = crate::auth::provider_by_id(&provider)
                .map(|p| p.display_name)
                .unwrap_or(provider.as_str());
            messages.push(format!(
                "Provider switched to {provider_label} ({effective_model})."
            ));
        } else {
            if let Ok(mut s) = shared_settings.lock() {
                s.provider_connected = crate::auth::provider_connected_for_model(&effective_model);
            }
            let provider_label = crate::auth::provider_by_id(&provider)
                .map(|p| p.display_name)
                .unwrap_or(provider.as_str());
            messages.push(format!(
                "⚠ No credentials for {provider_label}. Use /login to authenticate."
            ));
        }
    } else if old_model != effective_model {
        let provider_label = crate::auth::provider_by_id(&new_provider)
            .map(|p| p.display_name)
            .unwrap_or(new_provider.as_str());
        messages.push(format!(
            "Model switched to {effective_model} via {provider_label}."
        ));
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(if messages.is_empty() {
            format!("Model unchanged: {effective_model}")
        } else {
            messages.join("\n")
        }),
    }
}

pub async fn switch_dispatcher_response(
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    request_id: &str,
    profile: &str,
    model: Option<&str>,
    events_tx: &broadcast::Sender<AgentEvent>,
) -> SlashCommandResponse {
    let normalized_profile = profile.trim().to_ascii_uppercase();
    let allowed = ["F", "D", "C", "B", "A", "S"];
    if !allowed.contains(&normalized_profile.as_str()) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Unknown dispatcher grade '{profile}'. Expected one of: {}",
                allowed.join(", ")
            )),
        };
    }

    let requested_model = model.map(str::trim).filter(|m| !m.is_empty());
    let current_model = shared_settings
        .lock()
        .ok()
        .map(|s| s.model.clone())
        .unwrap_or_default();
    let current_provider = crate::providers::infer_provider_id(&current_model);
    let reg = crate::model_registry::ModelRegistry::global();
    let requested_model_spec = requested_model.map(ToOwned::to_owned).unwrap_or_else(|| {
        if let Some(tier_model) = reg.grade_model(&normalized_profile, &current_provider) {
            format!("{current_provider}:{tier_model}")
        } else {
            current_model.clone()
        }
    });
    let effective_model = requested_model_spec.clone();

    if let Ok(mut s) = shared_settings.lock() {
        if !effective_model.is_empty() {
            s.set_model(&effective_model);
        }
        let mut profile_doc = settings::Profile::load(&agent.cwd);
        profile_doc.capture_from(&s);
        let _ = profile_doc.save(&agent.cwd);
    }

    if !effective_model.is_empty() {
        let new_bridge = providers::auto_detect_bridge(&effective_model).await;
        let route = crate::route::RouteController::resolve_startup(
            effective_model.clone(),
            &[],
            &crate::route::CredentialLedger,
        )
        .await;
        let connected =
            new_bridge.is_some() && matches!(route, crate::route::ProviderRoute::Serving { .. });
        if let Some(new_bridge) = new_bridge {
            let mut guard = bridge.write().await;
            *guard = new_bridge;
        }
        if let Ok(mut s) = shared_settings.lock() {
            s.provider_connected = connected;
        }
    }

    let mut status = crate::status::HarnessStatus::assemble(&agent.cwd);
    let settings_snapshot = shared_settings.lock().ok().map(|s| s.clone());
    let (
        context_class,
        thinking_level,
        posture,
        operating_profile,
        principal_id,
        identity_issuer,
        session_kind,
        authorization,
    ) = if let Some(settings) = settings_snapshot {
        let profile = settings.operating_profile();
        let principal_id = profile
            .identity
            .principal_id
            .clone()
            .unwrap_or_else(|| "anonymous".into());
        let identity_issuer = profile
            .identity
            .issuer
            .clone()
            .unwrap_or_else(|| "unknown".into());
        let session_kind = profile
            .identity
            .session_kind
            .clone()
            .unwrap_or_else(|| "unknown".into());
        let authorization = profile.authorization.summary();
        (
            settings.effective_requested_class().label().to_string(),
            settings.thinking.as_str().to_string(),
            profile.posture.effective.display_name().to_string(),
            profile.summary(),
            principal_id,
            identity_issuer,
            session_kind,
            authorization,
        )
    } else {
        (
            status.context_class.clone(),
            status.thinking_level.clone(),
            status.posture.clone(),
            status.operating_profile.clone(),
            status.principal_id.clone(),
            status.identity_issuer.clone(),
            status.session_kind.clone(),
            status.authorization.clone(),
        )
    };
    status.update_routing(
        &context_class,
        &thinking_level,
        &normalized_profile,
        &posture,
        &operating_profile,
        &principal_id,
        &identity_issuer,
        &session_kind,
        &authorization,
    );
    status.update_runtime_posture(
        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
        omegon_traits::OmegonAutonomyMode::OperatorDriven,
    );
    status.update_dispatcher_state(
        Some(request_id.to_string()),
        Some(normalized_profile.clone()),
        if effective_model.is_empty() {
            None
        } else {
            Some(effective_model.clone())
        },
        "accepted",
        None,
        Some("dispatcher switch applied locally".into()),
    );
    status.dispatcher.active_profile = Some(normalized_profile.clone());
    status.dispatcher.active_model = if effective_model.is_empty() {
        None
    } else {
        Some(effective_model.clone())
    };
    let auth_status = auth::probe_all_providers().await;
    status.providers = crate::auth::auth_status_to_provider_statuses(&auth_status);
    status.annotate_provider_runtime_health();
    if let Ok(json) = serde_json::to_value(&status) {
        let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json: json });
    }

    SlashCommandResponse {
        accepted: true,
        output: Some(match requested_model_spec.as_str() {
            _s if requested_model.is_some() => format!(
                "Dispatcher switched to {normalized_profile} (request {request_id}) using {effective_model}."
            ),
            _ => format!("Dispatcher switched to {normalized_profile} (request {request_id})."),
        }),
    }
}

pub async fn thinking_view_response(
    shared_settings: &settings::SharedSettings,
) -> SlashCommandResponse {
    use crate::surfaces::palette::{
        PaletteBadgeTone, PaletteGroupProjection, PaletteProjection, PaletteRowProjection,
    };

    let current = shared_settings
        .lock()
        .ok()
        .map(|settings| settings.thinking);
    let mut rows = Vec::new();
    for &level in crate::settings::ThinkingLevel::all() {
        let mut row = PaletteRowProjection::action(
            format!("think.{}", level.as_str()),
            format!("/think {}", level.as_str()),
            thinking_level_description(level),
        )
        .with_badge(
            format!("{} {}", level.icon(), level.as_str()),
            PaletteBadgeTone::Info,
        );
        if current == Some(level) {
            row = row.with_badge("current", PaletteBadgeTone::Success);
        }
        rows.push(row);
    }

    let summary = current
        .map(|level| {
            format!(
                "Current thinking level: {} {}",
                level.icon(),
                level.as_str()
            )
        })
        .unwrap_or_else(|| "Current thinking level unavailable".into());

    SlashCommandResponse {
        accepted: true,
        output: Some(
            PaletteProjection::new("Thinking levels")
                .with_summary(summary)
                .with_group(
                    PaletteGroupProjection::new("Actions")
                        .with_description("`command` · level · state")
                        .with_rows(rows),
                )
                .with_footer("Use `/think <level>` to apply a level directly.")
                .render_markdown(),
        ),
    }
}

fn thinking_level_description(level: crate::settings::ThinkingLevel) -> &'static str {
    match level {
        crate::settings::ThinkingLevel::Off => "disable explicit reasoning budget",
        crate::settings::ThinkingLevel::Minimal => "use the smallest reasoning budget",
        crate::settings::ThinkingLevel::Low => "use light reasoning for simple work",
        crate::settings::ThinkingLevel::Medium => "use the default balanced reasoning level",
        crate::settings::ThinkingLevel::High => "use deeper reasoning for complex work",
        crate::settings::ThinkingLevel::XHigh => "use extra reasoning for difficult work",
        crate::settings::ThinkingLevel::Max => "use maximum reasoning depth",
    }
}

pub async fn set_thinking_response(
    shared_settings: &settings::SharedSettings,
    _cwd: &Path,
    level: crate::settings::ThinkingLevel,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.thinking = level;
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Thinking → {} {} (live override; use /profile save to persist)",
            level.icon(),
            level.as_str()
        )),
    }
}

pub async fn set_runtime_mode_response(
    runtime_state: &mut InteractiveAgentState,
    shared_settings: &settings::SharedSettings,
    events_tx: &broadcast::Sender<AgentEvent>,
    slim: bool,
) -> SlashCommandResponse {
    if let Ok(mut s) = shared_settings.lock() {
        if slim {
            s.set_posture(settings::PosturePreset::Explorator);
        } else {
            s.set_posture(settings::PosturePreset::Architect);
        }
    }
    runtime_state.conversation.set_slim_mode(slim);
    let (posture_disabled, posture_enabled) = shared_settings
        .lock()
        .ok()
        .map(|s| {
            (
                s.posture_disabled_tools.clone(),
                s.posture_enabled_tools.clone(),
            )
        })
        .unwrap_or_default();
    runtime_state
        .bus
        .apply_operator_tool_profile(slim, &posture_disabled, &posture_enabled);

    let mut status = crate::status::HarnessStatus::assemble(runtime_state.bus.project_root());
    let settings = shared_settings.lock().unwrap().clone();
    let operating_profile = settings.operating_profile();
    let operating_profile_label = operating_profile.summary();
    let principal_id = operating_profile
        .identity
        .principal_id
        .clone()
        .unwrap_or_else(|| "anonymous".into());
    let identity_issuer = operating_profile
        .identity
        .issuer
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let session_kind = operating_profile
        .identity
        .session_kind
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let authorization = operating_profile.authorization.summary();
    status.update_routing(
        settings.effective_requested_class().label(),
        settings.thinking.as_str(),
        &status.capability_grade.clone(),
        operating_profile.posture.effective.display_name(),
        &operating_profile_label,
        &principal_id,
        &identity_issuer,
        &session_kind,
        &authorization,
    );
    status.update_runtime_posture(
        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
        omegon_traits::OmegonAutonomyMode::OperatorDriven,
    );
    let auth_status = auth::probe_all_providers().await;
    status.providers = crate::auth::auth_status_to_provider_statuses(&auth_status);
    status.annotate_provider_runtime_health();
    status.update_from_bus(&runtime_state.bus);
    let status_json = runtime_state.bus.emit_harness_status(&status);
    let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json });

    SlashCommandResponse {
        accepted: true,
        output: Some(if slim {
            "Runtime profile → om (slim, familiar, copy-friendly; memory + orientation tools preserved).".into()
        } else {
            "Runtime profile → omegon (full harness, broader observability and advanced surfaces)."
                .into()
        }),
    }
}

pub async fn runtime_inventory_status_response(
    runtime_state: &InteractiveAgentState,
) -> SlashCommandResponse {
    let projection = runtime_state.inference_runtime.projection().await;
    SlashCommandResponse {
        accepted: true,
        output: Some(projection.render_text()),
    }
}

async fn runtime_substrate_refresh_with_generations(
    ctx: &mut ControlContext<'_>,
) -> SlashCommandResponse {
    let mut response = runtime_substrate_refresh_response(ctx.runtime_state, ctx.agent).await;
    if !response.accepted {
        return response;
    }
    let candidate_inventory = match crate::setup::runtime_substrate_refresh_candidate(
        &ctx.agent.cwd,
    ) {
        Ok(candidate) => candidate,
        Err(error) => {
            response.accepted = false;
            response.output = Some(format!(
                "{} Extension generation inspection failed after the other runtime refreshes completed: {error}",
                response.output.unwrap_or_default()
            ));
            return response;
        }
    };
    let (Some(publication), Some(active_owner), Some(supervisor)) = (
        ctx.dynamic_extension_publication.as_deref_mut(),
        ctx.dynamic_contributions.as_deref_mut(),
        ctx.supervisor.as_deref(),
    ) else {
        if let Some(output) = response.output.as_mut() {
            output.push_str(
                " Changed extension generations were inspected but cannot publish on this surface.",
            );
        }
        return response;
    };

    let mut published = Vec::new();
    let mut failures = Vec::new();
    for name in candidate_inventory.extension_candidate_names {
        let staged = match crate::setup::stage_installed_extension_replacement(
            &ctx.agent.cwd,
            &name,
            ctx.agent.secrets.clone(),
            active_owner.inventory(),
        )
        .await
        {
            Ok(crate::setup::InstalledExtensionReplacement::Unchanged) => continue,
            Ok(crate::setup::InstalledExtensionReplacement::Changed(candidate)) => candidate,
            Err(error) => {
                failures.push(format!("{name}: staging failed: {error}"));
                continue;
            }
        };
        if let Err(error) = publication.accept(staged).await {
            failures.push(format!("{name}: pending candidate rejected: {error}"));
            continue;
        }
        let id = omegon_traits::RuntimeContributionId::new(format!("extension:{name}"))
            .expect("admitted extension name forms a valid contribution id");
        match publication
            .commit_at_quiescence(&id, supervisor, &mut ctx.runtime_state.bus, active_owner)
            .await
        {
            Ok(outcome) => {
                published.push(name.clone());
                if !outcome.retirement_failures.is_empty() {
                    failures.push(format!(
                        "{name}: old-generation retirement degraded: {}",
                        outcome.retirement_failures.join("; ")
                    ));
                }
            }
            Err(error) => {
                let cleanup = publication
                    .reject_pending(&id, "explicit runtime refresh could not publish")
                    .await;
                failures.push(format!(
                    "{name}: publication failed: {error}{}",
                    if cleanup.is_empty() {
                        String::new()
                    } else {
                        format!("; candidate cleanup: {}", cleanup.join("; "))
                    }
                ));
            }
        }
    }

    if let Some(output) = response.output.as_mut() {
        if published.is_empty() {
            output.push_str(" No changed extension generation required publication.");
        } else {
            output.push_str(&format!(
                " Published changed extension generation(s) at quiescence: {}.",
                published.join(", ")
            ));
        }
        if !failures.is_empty() {
            output.push_str(&format!(
                " Extension generation failures: {}",
                failures.join(" | ")
            ));
        }
    }
    if !failures.is_empty() {
        response.accepted = false;
    }
    response
}

pub async fn runtime_substrate_refresh_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &InteractiveAgentHost,
) -> SlashCommandResponse {
    let substrate = match crate::setup::runtime_substrate_refresh_candidate(&agent.cwd) {
        Ok(candidate) => candidate,
        Err(error) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Runtime refresh rejected: {error}")),
            };
        }
    };
    // Explicit operator refresh bypasses discovery TTL (spec:
    // inference/catalog-unification "Explicit refresh bypasses TTL").
    let discovery_diagnostics = runtime_state
        .inference_runtime
        .refresh_discovery(true)
        .await;
    let inference = runtime_state.inference_runtime.refresh().await;
    runtime_state
        .inference_runtime
        .record_refresh_report(&inference)
        .await;
    if !inference.activated {
        let mut output = format!(
            "Runtime refresh rejected. Inference generation {} retained; {} endpoints, {} offerings; {} previously active manifest source(s). Extension and skill refresh was not promoted.",
            inference.active_generation,
            inference.endpoint_count,
            inference.offering_count,
            inference.loaded_sources.len(),
        );
        if !inference.diagnostics.is_empty() {
            output.push_str(" Inference diagnostics:");
            for diagnostic in &inference.diagnostics {
                output.push_str(&format!(
                    "\n- {:?} {}: {}",
                    diagnostic.phase,
                    diagnostic.path.display(),
                    diagnostic.message
                ));
            }
        }
        return SlashCommandResponse {
            accepted: false,
            output: Some(output),
        };
    }
    let mut output = format!(
        "Runtime refresh activated. Inference generation {} → {}; {} endpoints, {} offerings; {} manifest source(s) loaded. Extensions: {} candidate(s), {} skipped by policy, {} disabled.",
        inference.previous_generation,
        inference.active_generation,
        inference.endpoint_count,
        inference.offering_count,
        inference.loaded_sources.len(),
        substrate.extension_candidates,
        substrate.skipped_by_policy,
        substrate.disabled_extensions,
    );
    if !substrate.invalid_manifests.is_empty() {
        output.push_str(&format!(
            " {} extension manifest(s) were invalid.",
            substrate.invalid_manifests.len()
        ));
    }
    if !discovery_diagnostics.is_empty() {
        output.push_str(" Discovery diagnostics (last-known-good retained):");
        for diagnostic in &discovery_diagnostics {
            output.push_str(&format!("\n- {diagnostic}"));
        }
    }
    if !inference.diagnostics.is_empty() {
        output.push_str(" Inference diagnostics:");
        for diagnostic in &inference.diagnostics {
            output.push_str(&format!(
                "\n- {:?} {}: {}",
                diagnostic.phase,
                diagnostic.path.display(),
                diagnostic.message
            ));
        }
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

pub async fn status_view_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
) -> SlashCommandResponse {
    let mut status = agent
        .dashboard_handles
        .observe_harness()
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::status::HarnessStatus::assemble(&agent.cwd));
    let settings = shared_settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let operating_profile = settings.operating_profile();
    let operating_profile_label = operating_profile.summary();
    let principal_id = operating_profile
        .identity
        .principal_id
        .clone()
        .unwrap_or_else(|| "anonymous".into());
    let identity_issuer = operating_profile
        .identity
        .issuer
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let session_kind = operating_profile
        .identity
        .session_kind
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let authorization = operating_profile.authorization.summary();
    status.update_routing(
        settings.effective_requested_class().label(),
        settings.thinking.as_str(),
        &status.capability_grade.clone(),
        operating_profile.posture.effective.display_name(),
        &operating_profile_label,
        &principal_id,
        &identity_issuer,
        &session_kind,
        &authorization,
    );
    let bootstrap_markdown = crate::bootstrap_projection::render_bootstrap(&status, false);
    let projection = crate::surfaces::diagnostics::HarnessStatusProjection::new(
        serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
        agent.runtime_generation,
        agent.session_id.clone(),
        agent.instance_id.clone(),
        settings.automation_level.as_str(),
        settings.automation_level.summary(),
        bootstrap_markdown,
    );
    let mut panel = projection.render_markdown();
    if let Some(composition) = runtime_state.bus.composition_diagnostic_projection() {
        panel.push_str(&composition.render_markdown());
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(panel),
    }
}

fn workspace_control_context(
    agent: &InteractiveAgentHost,
) -> crate::workspace::control::WorkspaceControlContext<'_> {
    crate::workspace::control::WorkspaceControlContext::new(
        &agent.cwd,
        &agent.session_id,
        &agent.instance_id,
    )
    .with_git(&agent.git_binding)
}

pub async fn workspace_status_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_status_view_response(&ctx)
}

pub async fn workspace_list_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_list_view_response(&ctx)
}

pub async fn workspace_new_response(
    agent: &InteractiveAgentHost,
    label: &str,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_new_response(&ctx, label).await
}

pub async fn workspace_destroy_response(
    agent: &InteractiveAgentHost,
    target: &str,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_destroy_response(&ctx, target).await
}

pub async fn workspace_adopt_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_adopt_response(&ctx)
}

pub async fn workspace_release_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_release_response(&ctx)
}

pub async fn workspace_archive_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_archive_response(&ctx)
}

pub async fn workspace_prune_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_prune_response(&ctx)
}

pub async fn workspace_bind_milestone_response(
    agent: &InteractiveAgentHost,
    milestone_id: &str,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_bind_milestone_response(&ctx, milestone_id)
}

pub async fn workspace_bind_node_response(
    agent: &InteractiveAgentHost,
    design_node_id: &str,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_bind_node_response(&ctx, design_node_id)
}

pub async fn workspace_bind_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_bind_clear_response(&ctx)
}

pub async fn workspace_role_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_role_view_response(&ctx)
}

pub async fn workspace_role_set_response(
    agent: &InteractiveAgentHost,
    role: crate::workspace::types::WorkspaceRole,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_role_set_response(&ctx, role)
}

pub async fn workspace_role_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_role_clear_response(&ctx)
}

pub async fn workspace_kind_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_kind_view_response(&ctx)
}

pub async fn workspace_kind_set_response(
    agent: &InteractiveAgentHost,
    kind: crate::workspace::types::WorkspaceKind,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_kind_set_response(&ctx, kind)
}

pub async fn workspace_kind_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_kind_clear_response(&ctx)
}

pub async fn session_stats_view_response(
    runtime_state: &InteractiveAgentState,
    shared_settings: &settings::SharedSettings,
    agent: &InteractiveAgentHost,
) -> SlashCommandResponse {
    let settings = shared_settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let est = runtime_state.conversation.estimate_tokens();
    let session = agent
        .dashboard_handles
        .session()
        .observe()
        .unwrap_or_default();
    let turns = session.turns.max(runtime_state.conversation.turn_count());
    let tool_calls = session.tool_calls;
    let live_harness = agent
        .dashboard_handles
        .observe_harness()
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::status::HarnessStatus::assemble(&agent.cwd));
    let persona = live_harness
        .active_persona
        .as_ref()
        .map(|persona| format!("{} {}", persona.badge, persona.name))
        .unwrap_or_else(|| "none".to_string());
    let tone = live_harness
        .active_tone
        .as_ref()
        .map(|tone| tone.name.clone())
        .unwrap_or_else(|| "none".to_string());
    let authenticated_providers = live_harness
        .providers
        .iter()
        .filter(|provider| provider.authenticated)
        .count();
    let projection = crate::surfaces::diagnostics::SessionStatsProjection {
        version: crate::surfaces::diagnostics::DIAGNOSTIC_PROJECTION_VERSION,
        turns,
        tool_calls: Some(tool_calls),
        model: settings.model_short(),
        thinking: format!(
            "{} {}",
            settings.thinking.icon(),
            settings.thinking.as_str()
        ),
        posture: settings.posture.effective.as_str().to_string(),
        estimated_context_tokens: est,
        context_window: settings.context_window,
        max_turns: settings.max_turns,
        persona: Some(persona),
        tone: Some(tone),
        authenticated_providers: Some(authenticated_providers),
        provider_count: Some(live_harness.providers.len()),
        mcp_servers: Some(live_harness.mcp_servers.len()),
        memory_available: Some(live_harness.memory_available),
        cleave_available: Some(live_harness.cleave_available),
    };

    SlashCommandResponse {
        accepted: true,
        output: Some(projection.render_markdown()),
    }
}

pub async fn tree_view_response(
    runtime_state: &mut InteractiveAgentState,
    args: &str,
    invocation_scope: &crate::invocation_service::InvocationScope,
) -> SlashCommandResponse {
    match dispatch_control_feature_command(&mut runtime_state.bus, "design", args, invocation_scope)
    {
        omegon_traits::CommandResult::Display(msg) => SlashCommandResponse {
            accepted: true,
            output: Some(msg),
        },
        omegon_traits::CommandResult::Handled => SlashCommandResponse {
            accepted: true,
            output: Some("Design tree command handled.".into()),
        },
        omegon_traits::CommandResult::NotHandled => SlashCommandResponse {
            accepted: false,
            output: Some("Design tree command was not handled.".into()),
        },
    }
}

fn notes_path(agent: &InteractiveAgentHost) -> std::path::PathBuf {
    agent.cwd.join(".omegon").join("notes.md")
}

fn count_notes_file(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .ok()
        .map(|content| content.lines().filter(|l| l.starts_with("- [")).count())
        .unwrap_or(0)
}

pub async fn note_add_response(agent: &InteractiveAgentHost, text: &str) -> SlashCommandResponse {
    let notes_path = notes_path(agent);
    if let Some(parent) = notes_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("✗ Can't create .omegon/: {e}")),
        };
    }
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let entry = format!("- [{timestamp}] {text}\n");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notes_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()))
    {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "📌 Noted. ({} entries)",
                count_notes_file(&notes_path)
            )),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("✗ Failed to save note: {e}")),
        },
    }
}

pub async fn notes_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let notes_path = notes_path(agent);
    match std::fs::read_to_string(&notes_path) {
        Ok(content) if !content.trim().is_empty() => {
            let count = content.lines().filter(|l| l.starts_with("- [")).count();
            SlashCommandResponse {
                accepted: true,
                output: Some(format!(
                    "📌 Pending notes ({count}):\n\n{content}\nClear with /notes clear"
                )),
            }
        }
        _ => SlashCommandResponse {
            accepted: true,
            output: Some(
                "No pending notes. Use /note <text> to capture something for later.".into(),
            ),
        },
    }
}

pub async fn notes_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let notes_path = notes_path(agent);
    let _ = std::fs::remove_file(&notes_path);
    SlashCommandResponse {
        accepted: true,
        output: Some("📌 Notes cleared.".into()),
    }
}

pub async fn checkin_view_response(
    agent: &InteractiveAgentHost,
    _runtime_state: &InteractiveAgentState,
) -> SlashCommandResponse {
    let mut sections: Vec<String> = Vec::new();

    if let Ok(output) = std::process::Command::new("git")
        .args(["--no-optional-locks", "status", "--short"])
        .current_dir(&agent.cwd)
        .stderr(std::process::Stdio::null())
        .output()
    {
        let status = String::from_utf8_lossy(&output.stdout);
        if !status.trim().is_empty() {
            let count = status.lines().count();
            sections.push(format!(
                "📂 Git: {count} uncommitted change{}",
                if count == 1 { "" } else { "s" }
            ));
        }
    }

    if let Ok(output) = std::process::Command::new("git")
        .args(["--no-optional-locks", "log", "--oneline", "@{u}..", "--"])
        .current_dir(&agent.cwd)
        .stderr(std::process::Stdio::null())
        .output()
    {
        let unpushed = String::from_utf8_lossy(&output.stdout);
        if !unpushed.trim().is_empty() {
            let count = unpushed.lines().count();
            sections.push(format!(
                "⬆ {count} unpushed commit{}",
                if count == 1 { "" } else { "s" }
            ));
        }
    }

    let note_count = count_notes_file(&notes_path(agent));
    if note_count > 0 {
        sections.push(format!(
            "📌 {note_count} pending note{}",
            if note_count == 1 { "" } else { "s" }
        ));
    }

    if let Ok(observation) = agent.dashboard_handles.lifecycle_service.observe()
        && let Some(repository) = observation.repository
    {
        let mut active = repository
            .lifecycle
            .openspec
            .changes
            .iter()
            .map(|change| change.name.clone())
            .collect::<Vec<_>>();
        active.sort();
        if !active.is_empty() {
            sections.push(format!(
                "📋 {} OpenSpec change{}: {}",
                active.len(),
                if active.len() == 1 { "" } else { "s" },
                active.join(", ")
            ));
        }
    }

    let facts = crate::status::HarnessStatus::assemble(&agent.cwd)
        .memory
        .total_facts;
    let working = crate::status::HarnessStatus::assemble(&agent.cwd)
        .memory
        .working_facts;
    if facts > 0 {
        sections.push(format!("🧠 {facts} facts ({working} working)"));
    }

    if sections.is_empty() {
        SlashCommandResponse {
            accepted: true,
            output: Some("✓ All clear — nothing needs attention.".into()),
        }
    } else {
        SlashCommandResponse {
            accepted: true,
            output: Some(format!("🔍 Check-in:\n\n{}", sections.join("\n"))),
        }
    }
}

pub async fn context_status_response(
    runtime_state: &InteractiveAgentState,
    shared_settings: &settings::SharedSettings,
) -> SlashCommandResponse {
    let est = runtime_state.conversation.estimate_tokens();
    let settings = shared_settings.lock().unwrap();
    let ctx_window = settings.context_window;
    let pct = if ctx_window > 0 {
        ((est as f64 / ctx_window as f64) * 100.0).min(100.0) as u32
    } else {
        0
    };

    // Per-category breakdown from prompt telemetry
    let telemetry = runtime_state.context_manager.last_prompt_telemetry();
    let base_tokens = crate::util::estimate_chars_to_tokens(telemetry.base_prompt_chars);
    let hud_tokens = crate::util::estimate_chars_to_tokens(telemetry.session_hud_chars);
    let intent_tokens = crate::util::estimate_chars_to_tokens(telemetry.intent_chars);
    let external_tokens = crate::util::estimate_chars_to_tokens(telemetry.external_injection_chars);
    let tool_guidance_tokens = crate::util::estimate_chars_to_tokens(telemetry.tool_guidance_chars);
    let file_guidance_tokens = crate::util::estimate_chars_to_tokens(telemetry.file_guidance_chars);
    let injection_total = external_tokens + tool_guidance_tokens + file_guidance_tokens;
    let conversation_tokens =
        est.saturating_sub(base_tokens + hud_tokens + intent_tokens + injection_total);
    let telemetry_total =
        base_tokens + hud_tokens + intent_tokens + injection_total + conversation_tokens;

    let requested_class = settings.effective_requested_class();
    let actual_class = settings.context_class;
    let thinking = settings.thinking;
    let model = settings.model.clone();

    SlashCommandResponse {
        accepted: true,
        output: Some(
            context_status_projection(
                est,
                ctx_window,
                pct,
                requested_class,
                actual_class,
                &model,
                thinking,
                telemetry_total,
            )
            .render_markdown(),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn context_status_projection(
    est: usize,
    ctx_window: usize,
    pct: u32,
    requested_class: settings::ContextClass,
    actual_class: settings::ContextClass,
    model: &str,
    thinking: settings::ThinkingLevel,
    telemetry_total: usize,
) -> crate::surfaces::palette::PaletteProjection {
    use crate::surfaces::palette::{
        PaletteBadgeTone, PaletteGroupProjection, PaletteProjection, PaletteRowProjection,
    };

    let context_actions = vec![
        PaletteRowProjection::action(
            "context.compact",
            "/context compact",
            "compact older turns through the context manager",
        ),
        PaletteRowProjection::action(
            "context.reset",
            "/context reset",
            "archive this session and start fresh context",
        ),
        PaletteRowProjection::action("context.new", "/new", "alias for `/context reset`"),
        PaletteRowProjection::action(
            "context.request",
            "/context request <kind> <query>",
            "pull a mediated context pack",
        ),
    ];

    let class_rows = settings::ContextClass::all()
        .iter()
        .copied()
        .map(|class| {
            let mut row = PaletteRowProjection::action(
                format!("context.class.{}", class.short().to_lowercase()),
                format!("/context {}", class.short().to_lowercase()),
                format!("set requested context policy to {}", class.label()),
            )
            .with_badge(class.label(), PaletteBadgeTone::Info);
            if class == requested_class {
                row = row.with_badge("requested", PaletteBadgeTone::Success);
            }
            if class == actual_class {
                row = row.with_badge("actual", PaletteBadgeTone::Neutral);
            }
            row
        })
        .collect();

    PaletteProjection::new("Context")
        .with_summary(format!(
            "{est}/{ctx_window} tokens ({pct}%) · requested {} · actual {} · model {model} · thinking {}",
            requested_class.label(),
            actual_class.label(),
            thinking.as_str()
        ))
        .with_group(
            PaletteGroupProjection::new("Actions")
                .with_description("`command` · effect")
                .with_rows(context_actions),
        )
        .with_group(
            PaletteGroupProjection::new("Context classes")
                .with_description("`command` · requested/actual markers")
                .with_rows(class_rows),
        )
        .with_footer(format!(
            "Last prompt telemetry accounts for ~{telemetry_total} local tokens. Use `/context request <kind> <query>` for targeted retrieval instead of dumping full state."
        ))
}

pub async fn context_compact_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    events_tx: &broadcast::Sender<AgentEvent>,
    invocation_scope: &crate::invocation_service::InvocationScope,
) -> SlashCommandResponse {
    let bridge_guard = bridge.read().await;
    let stream_options = {
        let s = shared_settings.lock().unwrap();
        crate::bridge::StreamOptions {
            model: Some(s.model.clone()),
            reasoning: Some(s.thinking.as_str().to_string()),
            extended_context: false,
            ..Default::default()
        }
    };
    runtime_state
        .context_manager
        .set_selector_policy(shared_settings.lock().unwrap().selector_policy());
    let retained_budget = runtime_state.context_manager.retained_context_budget();
    let before_tokens = runtime_state.conversation.estimate_tokens() as u64;
    let planning = runtime_state
        .context_compaction
        .plan(
            runtime_state
                .conversation
                .context_compaction_snapshot()
                .with_retained_token_budget(retained_budget),
            crate::context_compaction_service::ContextCompactionModeV1::Manual,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
    let plan = match planning {
        Ok(plan) => plan,
        Err(error) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Compression unavailable: {error:?}")),
            };
        }
    };
    if let Some(plan) = plan {
        let payload = &plan.payload;
        let evict_count = plan.evict_count;
        let retention_reason = plan.reason.clone();
        let authority_compaction = match (
            invocation_scope.authority.clone(),
            invocation_scope.turn_id,
            invocation_scope.session_id.as_deref(),
        ) {
            (Some(authority), None, Some(session_id)) if authority.session_id() == session_id => {
                match crate::session_compaction::SessionCompaction::begin_idle(authority, &plan) {
                    Ok(Some(compaction)) => Some(compaction),
                    Ok(None) => {
                        return SlashCommandResponse {
                            accepted: false,
                            output: Some(
                                "Compression failed: exact authority compaction input is unavailable"
                                    .into(),
                            ),
                        };
                    }
                    Err(error) => {
                        return SlashCommandResponse {
                            accepted: false,
                            output: Some(format!("Compression failed: {error}")),
                        };
                    }
                }
            }
            (None, None, None) => None,
            _ => {
                return SlashCommandResponse {
                    accepted: false,
                    output: Some(
                        "Compression failed: manual compaction requires an idle complete session scope"
                            .into(),
                    ),
                };
            }
        };
        let _ = events_tx.send(AgentEvent::ContextCompaction(
            omegon_traits::ContextCompactionEvent {
                trigger: omegon_traits::ContextCompactionTrigger::Manual,
                status: omegon_traits::ContextCompactionStatus::Started,
                before_tokens,
                after_tokens: None,
                evicted_messages: Some(evict_count),
                summary_chars: None,
                reason: retention_reason.clone(),
            },
        ));
        let compact_result = if let Some(authority) = authority_compaction.as_ref() {
            crate::session_execution::boot_execution_binding()
                .compact_scoped(
                    bridge_guard.as_ref(),
                    payload,
                    &stream_options,
                    invocation_scope,
                    authority,
                )
                .await
        } else {
            crate::session_execution::boot_execution_binding()
                .compact(bridge_guard.as_ref(), payload, &stream_options)
                .await
        };
        match compact_result {
            Ok(summary) => {
                let summary_chars = summary.chars().count();
                plan.apply(&mut runtime_state.conversation, summary);
                let est = runtime_state.conversation.estimate_tokens();
                let settings = shared_settings.lock().unwrap();
                if let Ok(mut metrics) = agent.context_metrics.lock() {
                    metrics.update(
                        est,
                        settings.context_window,
                        settings.effective_requested_class().label(),
                        settings.thinking.as_str(),
                    );
                }
                let _ = events_tx.send(AgentEvent::ContextCompaction(
                    omegon_traits::ContextCompactionEvent {
                        trigger: omegon_traits::ContextCompactionTrigger::Manual,
                        status: omegon_traits::ContextCompactionStatus::Succeeded,
                        before_tokens,
                        after_tokens: Some(est as u64),
                        evicted_messages: Some(evict_count),
                        summary_chars: Some(summary_chars),
                        reason: retention_reason.clone(),
                    },
                ));
                SlashCommandResponse {
                    accepted: true,
                    output: Some(format!("Context compressed. Now using {est} tokens.")),
                }
            }
            Err(e) => {
                let message = e.to_string();
                let _ = events_tx.send(AgentEvent::ContextCompaction(
                    omegon_traits::ContextCompactionEvent {
                        trigger: omegon_traits::ContextCompactionTrigger::Manual,
                        status: omegon_traits::ContextCompactionStatus::Failed,
                        before_tokens,
                        after_tokens: None,
                        evicted_messages: Some(evict_count),
                        summary_chars: None,
                        reason: Some(message.clone()),
                    },
                ));
                SlashCommandResponse {
                    accepted: false,
                    output: Some(format!("Compression failed: {message}")),
                }
            }
        }
    } else {
        let _ = events_tx.send(AgentEvent::ContextCompaction(
            omegon_traits::ContextCompactionEvent {
                trigger: omegon_traits::ContextCompactionTrigger::Manual,
                status: omegon_traits::ContextCompactionStatus::NoPayload,
                before_tokens,
                after_tokens: Some(before_tokens),
                evicted_messages: Some(0),
                summary_chars: None,
                reason: Some(
                    "no complete older turns eligible under the retention budget".to_string(),
                ),
            },
        ));
        SlashCommandResponse {
            accepted: true,
            output: Some(
                "Nothing to compress yet — compaction preserves the current turn and complete tool exchanges; older turns are summarized according to the retention budget.".to_string(),
            ),
        }
    }
}

pub async fn context_clear_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    cli: &CliRuntimeView<'_>,
    events_tx: &broadcast::Sender<AgentEvent>,
    supervisor: Option<&mut crate::runtime_supervisor::InteractiveRuntimeSupervisor>,
) -> SlashCommandResponse {
    let response = replace_interactive_session(
        runtime_state,
        agent,
        cli,
        supervisor,
        crate::session_replacement::fresh_request(
            crate::session_replacement::SessionReplacementKind::ContextClear,
            &agent.cwd,
        ),
    );
    if let Err(error) = response {
        return replacement_failure(error);
    }
    let context_window = if let Ok(mut metrics) = agent.context_metrics.lock() {
        let context_window = metrics.context_window;
        metrics.update(0, context_window, "Compact", "off");
        context_window
    } else {
        200_000
    };
    let _ = events_tx.send(AgentEvent::ContextUpdated {
        tokens: 0,
        context_window: context_window as u64,
        context_class: "Compact".to_string(),
        thinking_level: "off".to_string(),
    });
    let _ = events_tx.send(AgentEvent::SessionReset);
    SlashCommandResponse {
        accepted: true,
        output: Some("Context cleared. Starting fresh conversation.".to_string()),
    }
}

pub async fn context_request_response(
    runtime_state: &mut InteractiveAgentState,
    kind: &str,
    query: &str,
) -> SlashCommandResponse {
    let args = serde_json::json!({
        "requests": [{
            "kind": kind,
            "query": query,
            "reason": "Operator-requested direct context inspection from slash command"
        }]
    });
    match runtime_state.context_service.request_context(args).await {
        Ok(result) => {
            let text = result
                .content
                .iter()
                .filter_map(|c| match c {
                    omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            SlashCommandResponse {
                accepted: true,
                output: Some(text),
            }
        }
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Context request failed: {e}")),
        },
    }
}

pub async fn context_request_json_response(
    runtime_state: &mut InteractiveAgentState,
    raw: &str,
) -> SlashCommandResponse {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(args) if args.get("requests").and_then(|v| v.as_array()).is_some() => {
            match runtime_state.context_service.request_context(args).await {
                Ok(result) => {
                    let text = result
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    SlashCommandResponse {
                        accepted: true,
                        output: Some(text),
                    }
                }
                Err(e) => SlashCommandResponse {
                    accepted: false,
                    output: Some(format!("Context request failed: {e}")),
                },
            }
        }
        _ => SlashCommandResponse {
            accepted: false,
            output: Some(
                "Usage: /context request <kind> <query> or /context request {\"requests\":[...]}"
                    .to_string(),
            ),
        },
    }
}

pub async fn set_context_class_response(
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    class: crate::settings::ContextClass,
) -> SlashCommandResponse {
    let _ = agent;
    if let Ok(mut s) = shared_settings.lock() {
        s.set_requested_context_class(class);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Context policy → {} (live override; model capacity unchanged; use /profile save to persist)",
            class.label()
        )),
    }
}

pub async fn new_session_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    cli: &CliRuntimeView<'_>,
    events_tx: &broadcast::Sender<AgentEvent>,
    supervisor: Option<&mut crate::runtime_supervisor::InteractiveRuntimeSupervisor>,
) -> SlashCommandResponse {
    let response = replace_interactive_session(
        runtime_state,
        agent,
        cli,
        supervisor,
        crate::session_replacement::fresh_request(
            crate::session_replacement::SessionReplacementKind::New,
            &agent.cwd,
        ),
    );
    if let Err(error) = response {
        return replacement_failure(error);
    }
    let _ = events_tx.send(AgentEvent::SessionReset);
    SlashCommandResponse {
        accepted: true,
        output: Some("Started a fresh session.".to_string()),
    }
}

pub async fn list_sessions_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some(list_sessions_message(&agent.cwd)),
    }
}

pub async fn resume_session_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    cli: &CliRuntimeView<'_>,
    events_tx: &broadcast::Sender<AgentEvent>,
    id: &str,
    supervisor: Option<&mut crate::runtime_supervisor::InteractiveRuntimeSupervisor>,
) -> SlashCommandResponse {
    let id = id.trim();
    if id.is_empty() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Usage: /resume <session-id>".to_string()),
        };
    }
    let Some(path) = session::find_session(&agent.cwd, Some(id)) else {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "No saved session matches '{id}'. Use /sessions to list recent sessions."
            )),
        };
    };
    match crate::session_replacement::resume_request(&agent.cwd, &path).and_then(|request| {
        let description = request
            .resume_info
            .as_ref()
            .map(|info| info.description.clone())
            .unwrap_or_default();
        replace_interactive_session(runtime_state, agent, cli, supervisor, Ok(request))
            .map(|outcome| (outcome, description))
    }) {
        Ok((outcome, description)) => {
            let _ = events_tx.send(AgentEvent::SessionReset);
            SlashCommandResponse {
                accepted: true,
                output: Some(format!(
                    "Resumed session {}: {description}",
                    outcome.session_id
                )),
            }
        }
        Err(error) => SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Failed to resume session '{}': {error}",
                path.display()
            )),
        },
    }
}

fn replace_interactive_session(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    cli: &CliRuntimeView<'_>,
    supervisor: Option<&mut crate::runtime_supervisor::InteractiveRuntimeSupervisor>,
    request: Result<
        crate::session_replacement::SessionReplacementRequest,
        crate::session_replacement::SessionReplacementError,
    >,
) -> Result<
    crate::session_replacement::SessionReplacementOutcome,
    crate::session_replacement::SessionReplacementError,
> {
    let request = request?;
    if cli.no_session {
        let outcome = crate::session_replacement::replace_sessionless(
            &mut runtime_state.conversation,
            &mut agent.session_id,
            &mut agent.resume_info,
            request,
        );
        publish_session_view_binding(agent, &outcome);
        crate::session_replacement::emit_canonical_session_start(
            &mut runtime_state.bus,
            &agent.cwd,
            &outcome,
        );
        return Ok(outcome);
    }
    let supervisor = supervisor.ok_or_else(|| {
        crate::session_replacement::SessionReplacementError::Target(
            "host session owner is unavailable".into(),
        )
    })?;
    let runtime_generation_id = runtime_state
        .bus
        .composition_generation_id()
        .ok_or_else(|| {
            crate::session_replacement::SessionReplacementError::Target(
                "runtime composition has no generation".into(),
            )
        })?
        .as_str()
        .to_string();
    let outcome = crate::session_replacement::replace(
        crate::session_replacement::HostSessionPublication {
            supervisor,
            conversation: &mut runtime_state.conversation,
            displayed_session_id: &mut agent.session_id,
            resume_info: &mut agent.resume_info,
        },
        request,
        crate::session_replacement::SessionReplacementEnvironment {
            cwd: &agent.cwd,
            persist_current: true,
            workspace_identity: &agent.workspace_state.lease.workspace_id,
            runtime_generation_id: &runtime_generation_id,
            actor: crate::session_authority::ActorIdentity {
                principal: "local-operator".into(),
                ingress: "interactive".into(),
            },
        },
    )?;
    publish_session_view_binding(agent, &outcome);
    crate::session_replacement::emit_canonical_session_start(
        &mut runtime_state.bus,
        &agent.cwd,
        &outcome,
    );
    Ok(outcome)
}

fn publish_session_view_binding(
    agent: &InteractiveAgentHost,
    outcome: &crate::session_replacement::SessionReplacementOutcome,
) {
    let Some(snapshot) = crate::session_consumers::snapshot_path(&agent.cwd, &outcome.session_id)
    else {
        return;
    };
    let kind = match outcome.kind {
        crate::session_replacement::SessionReplacementKind::Resume => {
            crate::session_consumers::SessionViewKind::Resume
        }
        crate::session_replacement::SessionReplacementKind::New => {
            crate::session_consumers::SessionViewKind::New
        }
        crate::session_replacement::SessionReplacementKind::ContextClear => {
            crate::session_consumers::SessionViewKind::ContextClear
        }
    };
    agent
        .session_view_binding
        .replace(crate::session_consumers::SessionViewTarget {
            snapshot,
            session_id: outcome.session_id.clone(),
            stream_id: (outcome.projection.stream_id != uuid::Uuid::nil())
                .then_some(outcome.projection.stream_id),
            generation: outcome.host_generation,
            kind,
        });
}

fn replacement_failure(
    error: crate::session_replacement::SessionReplacementError,
) -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: false,
        output: Some(format!("Session was not replaced: {error}")),
    }
}

pub async fn auth_status_response() -> SlashCommandResponse {
    let status = auth::probe_all_providers().await;
    SlashCommandResponse {
        accepted: true,
        output: Some(format_auth_status(&status)),
    }
}

pub async fn auth_unlock_response() -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some("🔒 Secrets store unlock not yet implemented".to_string()),
    }
}

pub struct AuthLoginRouteContext<'a> {
    pub cwd: &'a Path,
    pub fallback_model: &'a str,
    pub inference_runtime: &'a crate::inference_runtime::InferenceRuntimeState,
    pub secrets: &'a std::sync::Arc<omegon_secrets::SecretsManager>,
}

pub async fn auth_login_response(
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    login_prompt_tx: &std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
    events_tx: &broadcast::Sender<AgentEvent>,
    provider: &str,
    route_context: AuthLoginRouteContext<'_>,
) -> SlashCommandResponse {
    let provider = provider.trim();
    let provider = if provider.is_empty() {
        "anthropic"
    } else {
        crate::auth::canonical_provider_id(provider)
    };
    if provider == "openai" {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                auth::operator_api_key_login_guidance("openai", "OPENAI_API_KEY", "OpenAI API")
                    + " For headless automation, set OPENAI_API_KEY.",
            ),
        };
    }
    if login_prompt_tx.lock().await.is_some() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Login is already waiting for interactive input in the TUI.".to_string()),
        };
    }
    let events_tx_clone = events_tx.clone();
    let progress_tx = events_tx.clone();
    let prompt_tx_for_login = events_tx.clone();
    let login_prompt_slot = login_prompt_tx.clone();
    let provider_clone = provider.to_string();
    let bridge_clone = bridge.clone();
    let model_for_redetect = shared_settings
        .lock()
        .ok()
        .map(|s| s.model.clone())
        .unwrap_or_else(|| route_context.fallback_model.to_string());
    let cwd_for_profile = route_context.cwd.to_path_buf();
    let settings_for_login = shared_settings.clone();
    let inference_runtime = route_context.inference_runtime.clone();
    let secrets = route_context.secrets.clone();
    tokio::spawn(async move {
        let progress: auth::LoginProgress = Box::new(move |msg| {
            let _ = progress_tx.send(AgentEvent::SystemNotification {
                message: msg.to_string(),
            });
        });
        let prompt: auth::LoginPrompt = Box::new(move |msg| {
            let slot = login_prompt_slot.clone();
            let tx = prompt_tx_for_login.clone();
            Box::pin(async move {
                let (otx, orx) = tokio::sync::oneshot::channel();
                {
                    let mut guard = slot.lock().await;
                    *guard = Some(otx);
                }
                let _ = tx.send(AgentEvent::SystemNotification { message: msg });
                orx.await
                    .map_err(|_| anyhow::anyhow!("Login prompt cancelled"))
            })
        });
        let result = match provider_clone.as_str() {
            "anthropic" | "claude" => auth::login_anthropic_with_callbacks(progress, prompt).await,
            "openai-codex" | "chatgpt" | "codex" => {
                auth::login_openai_with_callbacks(progress, prompt).await
            }
            "github-copilot" | "copilot" => {
                let copy_tx = events_tx_clone.clone();
                let copy_block: auth::LoginCopyBlock =
                    Box::new(move |label, text, kind, copy_attempt| {
                        let _ = copy_tx.send(AgentEvent::OperatorCopyBlock {
                            label,
                            text,
                            kind,
                            copy_attempt,
                        });
                    });
                auth::login_github_copilot_with_copy_callback(progress, prompt, copy_block).await
            }
            "openai" => Err(anyhow::anyhow!(auth::operator_api_key_login_guidance(
                "openai",
                "OPENAI_API_KEY",
                "OpenAI API"
            ))),
            "openrouter" => Err(anyhow::anyhow!(auth::operator_api_key_login_guidance(
                "openrouter",
                "OPENROUTER_API_KEY",
                "OpenRouter"
            ))),
            "ollama-cloud" => Err(anyhow::anyhow!(auth::operator_api_key_login_guidance(
                "ollama-cloud",
                "OLLAMA_API_KEY",
                "Ollama Cloud"
            ))),
            _ => Err(anyhow::anyhow!(
                auth::operator_auth_unknown_provider_message(&provider_clone)
            )),
        };
        let provider_label = crate::auth::provider_by_id(&provider_clone)
            .map(|p| p.display_name)
            .unwrap_or(provider_clone.as_str())
            .to_string();
        let env_conflict = if result.is_ok() && provider_clone == "anthropic" {
            std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|_| {
                    "Anthropic OAuth login succeeded, but ANTHROPIC_API_KEY is also set. Requests will continue to prefer the API key. If you want Claude subscription auth for this session, unset ANTHROPIC_API_KEY and retry /connect anthropic."
                        .to_string()
                })
        } else {
            None
        };
        let message = match &result {
            Ok(_) => format!("✓ Successfully logged in to {provider_label}"),
            Err(e) => format!("✗ Login failed: {}", e),
        };
        let _ = events_tx_clone.send(AgentEvent::SystemNotification { message });
        if let Some(conflict) = env_conflict {
            let _ = events_tx_clone.send(AgentEvent::SystemNotification { message: conflict });
        }
        if result.is_ok() {
            // Use the provider that was just logged into, not the pre-login
            // model setting (which may reference a different provider entirely).
            let login_provider_model = providers::default_model_for_provider(&provider_clone)
                .unwrap_or(model_for_redetect.clone());
            let effective_model = login_provider_model;
            let inventory = inference_runtime.snapshot().await;
            if let Some(new_bridge) = crate::session_execution::boot_execution_binding()
                .resolve_exact_admitted_provider_route(
                    &effective_model,
                    Some(secrets.as_ref()),
                    &inventory,
                    &[],
                )
                .await
                .map(crate::provider_route_service::ResolvedProviderRoute::into_unleased_bridge)
            {
                let mut guard = bridge_clone.write().await;
                *guard = new_bridge;
                if let Ok(mut s) = settings_for_login.lock() {
                    s.set_model(&effective_model);
                    s.provider_connected =
                        crate::auth::provider_connected_for_model(&effective_model);
                    let mut profile = settings::Profile::load(&cwd_for_profile);
                    profile.capture_from(&s);
                    let _ = profile.save(&cwd_for_profile);
                }
                let _ = events_tx_clone.send(AgentEvent::SystemNotification {
                    message: auth::operator_provider_connected_message(&effective_model),
                });
            }
        }
    });
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Login started for {provider}. Complete any interactive prompts in the TUI."
        )),
    }
}

pub async fn auth_logout_response(provider: &str) -> SlashCommandResponse {
    let provider = provider.trim();
    if provider.is_empty() {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Provider required for logout. Use one of: {}",
                auth::operator_auth_provider_help_list()
            )),
        };
    }
    let provider = crate::auth::canonical_provider_id(provider);
    let Some(provider_info) = crate::auth::provider_by_id(provider) else {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "✗ {}",
                auth::operator_auth_unknown_provider_message(provider)
            )),
        };
    };
    let provider_label = provider_info.display_name;
    match auth::logout_provider(provider) {
        Ok(()) => {
            auth::clear_provider_auth_env(provider);
            let message = auth::operator_logout_success_message(
                provider_label,
                !auth::provider_env_vars(provider).is_empty(),
            );
            SlashCommandResponse {
                accepted: true,
                output: Some(message),
            }
        }
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("✗ Logout failed for {provider_label}: {}", e)),
        },
    }
}

/// Daemon-mode auth login. OAuth providers return guidance (browser flow
/// must be initiated by the client). API key providers are not yet
/// supported via WebSocket — the client should write auth.json directly
/// or use `omegon auth login` from a terminal.
pub async fn auth_login_daemon_response(provider: &str) -> SlashCommandResponse {
    let provider = provider.trim();
    let provider = if provider.is_empty() {
        "anthropic"
    } else {
        crate::auth::canonical_provider_id(provider)
    };
    let Some(provider_info) = crate::auth::provider_by_id(provider) else {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "✗ {}",
                auth::operator_auth_unknown_provider_message(provider)
            )),
        };
    };
    match provider_info.auth_method {
        auth::AuthMethod::Anonymous => SlashCommandResponse {
            accepted: false,
            output: Some("Open om or omegon and use /connect free to choose a free model and review its data terms. No API key is required.".into()),
        },
        auth::AuthMethod::OAuth => SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "{} uses OAuth login which requires a browser. \
                     Run `omegon auth login {}` from a terminal with browser access, \
                     or mount a grant-backed provider auth file and set \
                     `OMEGON_AUTH_JSON_PATH=/config/omegon/auth.json`. \
                     The daemon will pick up credentials on the next request.",
                provider_info.display_name, provider,
            )),
        },
        auth::AuthMethod::ApiKey | auth::AuthMethod::Dynamic => {
            let env_hint = provider_info.env_vars.first().copied().unwrap_or("API_KEY");
            SlashCommandResponse {
                accepted: false,
                output: Some(format!(
                    "{} uses API key auth. Set {} in the environment or run \
                     `omegon auth login {}` from a terminal to store the key. \
                     For Auspex-managed agents, project provider credentials via \
                     `OMEGON_AUTH_JSON_PATH=/config/omegon/auth.json`. \
                     The daemon will pick up credentials on the next request.",
                    provider_info.display_name, env_hint, provider,
                )),
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Auspex fleet control — daemon-safe handlers
// ═══════════════════════════════════════════════════════════════════════════

pub async fn set_thinking_daemon_response(
    shared_settings: &settings::SharedSettings,
    _cwd: &Path,
    level: crate::settings::ThinkingLevel,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.thinking = level;
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Thinking → {} {} (live override; use /profile save to persist)",
            level.icon(),
            level.as_str()
        )),
    }
}

pub async fn set_model_daemon_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    requested_model: &str,
) -> SlashCommandResponse {
    let effective = requested_model.to_string();
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.set_model(&effective);
    s.provider_connected = crate::auth::provider_connected_for_model(&effective);
    let mut profile = settings::Profile::load(cwd);
    profile.capture_from(&s);
    let _ = profile.save(cwd);
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Model → {effective}")),
    }
}

pub async fn set_context_class_daemon_response(
    shared_settings: &settings::SharedSettings,
    _cwd: &Path,
    class: crate::settings::ContextClass,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.set_requested_context_class(class);
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Context policy → {} (live override; use /profile save to persist)",
            class.label()
        )),
    }
}

pub async fn set_runtime_mode_daemon_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    slim: bool,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    if slim {
        s.set_posture(settings::PosturePreset::Explorator);
    } else {
        s.set_posture(settings::PosturePreset::Architect);
    }
    let mut profile = settings::Profile::load(cwd);
    profile.capture_from(&s);
    let _ = profile.save(cwd);
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Runtime mode → {}. Takes effect on next turn.",
            if slim { "slim" } else { "full" }
        )),
    }
}

pub async fn set_max_turns_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    max_turns: u32,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.max_turns = max_turns;
    let mut profile = settings::Profile::load(cwd);
    profile.capture_from(&s);
    let _ = profile.save(cwd);
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Max turns → {}",
            if max_turns == 0 {
                "unlimited".to_string()
            } else {
                max_turns.to_string()
            }
        )),
    }
}

pub async fn profile_view_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
) -> SlashCommandResponse {
    let loaded = settings::Profile::load_with_source(cwd);
    let output = if let Ok(s) = shared_settings.lock() {
        let drift = crate::surfaces::profile::ProfileDriftProjection::from_profile_and_settings(
            &loaded.profile,
            loaded.source.clone(),
            &s,
        );
        render_profile_view(&loaded.profile, &drift, &s)
    } else {
        "failed to read settings".to_string()
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

fn render_profile_view(
    profile: &settings::Profile,
    drift: &crate::surfaces::profile::ProfileDriftProjection,
    settings: &settings::Settings,
) -> String {
    let mut out = String::new();
    out.push_str(
        "## Profile

",
    );
    out.push_str(&format!(
        "Source: {}
",
        drift.source
    ));
    if drift.dirty {
        out.push_str(&format!(
            "Runtime drift: Δ{} unsaved change(s)

",
            drift.changed_count
        ));
        out.push_str(
            "| Setting | Profile | Runtime | Persistence |
",
        );
        out.push_str(
            "|---|---:|---:|---|
",
        );
        for row in &drift.rows {
            out.push_str(&format!(
                "| {} | `{}` | `{}` | {} |
",
                row.label,
                row.profile_value,
                row.runtime_value,
                row.persistence.label()
            ));
        }
        out.push_str(
            "
Actions:
",
        );
        out.push_str(
            "- `/profile save` — save current runtime to the active profile source
",
        );
        out.push_str(
            "- `/profile save --project` — save current runtime as project defaults
",
        );
        out.push_str(
            "- `/profile save --user` — save current runtime as user defaults
",
        );
        out.push_str(
            "- `/profile apply` — revert runtime to profile defaults
",
        );
    } else {
        out.push_str(
            "Runtime drift: clean

",
        );
        out.push_str(
            "Actions:
",
        );
        out.push_str(
            "- `/profile save --project` — save current runtime as project defaults
",
        );
        out.push_str(
            "- `/profile save --user` — save current runtime as user defaults
",
        );
    }

    out.push_str(
        "
### Live runtime
",
    );
    out.push_str(&format!(
        "- Model: `{}`
",
        settings.model
    ));
    out.push_str(&format!(
        "- Thinking: `{}`
",
        settings.thinking.as_str()
    ));
    out.push_str(&format!(
        "- Requested context: `{}`
",
        settings.effective_requested_class().short().to_lowercase()
    ));
    out.push_str(&format!(
        "- Context window: `{}` tokens
",
        settings.context_window
    ));
    out.push_str(&format!(
        "- Max turns: `{}`
",
        settings.max_turns
    ));

    out.push_str(
        "
### Saved profile
",
    );
    out.push_str(
        "```json
",
    );
    out.push_str(&serde_json::to_string_pretty(profile).unwrap_or_else(|_| "null".to_string()));
    out.push_str(
        "
```
",
    );
    out
}

pub async fn profile_capture_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    target: settings::ProfileSaveTarget,
) -> SlashCommandResponse {
    let (profile, current_source) = {
        let Ok(s) = shared_settings.lock() else {
            return SlashCommandResponse {
                accepted: false,
                output: Some("failed to read settings".into()),
            };
        };
        let loaded = settings::Profile::load_with_source(cwd);
        let mut profile = loaded.profile;
        profile.capture_from(&s);
        (profile, loaded.source)
    };
    match profile.save_to_target(cwd, target, &current_source) {
        Ok(source) => {
            if let Ok(mut s) = shared_settings.lock() {
                s.profile_source = source.clone();
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(format!("Profile captured from live runtime ({source}).")),
            }
        }
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("failed to save profile: {e}")),
        },
    }
}

async fn apply_profile_model_intent(
    profile: &settings::Profile,
    route_controller: Option<&Arc<crate::route::RouteController>>,
) -> anyhow::Result<Option<String>> {
    let Some(intent) = profile
        .model_intent
        .as_ref()
        .and_then(settings::ProfileModelIntent::to_route_intent)
    else {
        return Ok(None);
    };
    let Some(controller) = route_controller else {
        anyhow::bail!("live route controller is unavailable");
    };

    if let Some(model) = intent.exact_model_override.as_deref() {
        let bridge = providers::auto_detect_bridge(model).await;
        let snapshot = controller
            .switch_model(model.to_string(), &crate::route::CredentialLedger, bridge)
            .await?;
        if snapshot.serving_model() != Some(model) {
            anyhow::bail!(snapshot.operator_status());
        }
        return Ok(Some(model.to_string()));
    }

    let mut inventory = crate::routing::ProviderInventory::probe();
    inventory.probe_ollama().await;
    let candidate = crate::route::select_candidate_for_intent_with_provider_order(
        &intent,
        &inventory,
        &profile.provider_order,
    )
    .ok_or_else(|| anyhow::anyhow!("no provider candidate satisfies {}", intent.summary()))?;
    let target = format!("{}:{}", candidate.provider_id, candidate.model_id);
    let bridge = providers::auto_detect_bridge(&target)
        .await
        .ok_or_else(|| anyhow::anyhow!("no executable bridge is available for {target}"))?;
    controller.set_model_intent(intent).await;
    let snapshot = controller
        .resolve_route_from_intent_candidate(candidate, bridge)
        .await?;
    Ok(snapshot.serving_model().map(ToOwned::to_owned))
}

pub async fn profile_apply_response(
    agent: &mut InteractiveAgentHost,
    runtime_state: &mut InteractiveAgentState,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    route_controller: Option<Arc<crate::route::RouteController>>,
    events_tx: &broadcast::Sender<AgentEvent>,
) -> SlashCommandResponse {
    let profile = settings::Profile::load(&agent.cwd);
    let old_model = shared_settings
        .lock()
        .ok()
        .map(|s| s.model.clone())
        .unwrap_or_default();
    if let Ok(mut s) = shared_settings.lock() {
        profile.apply_to_with_posture(&mut s, &agent.cwd);
    }

    let resolved_model = match apply_profile_model_intent(&profile, route_controller.as_ref()).await
    {
        Ok(model) => model,
        Err(error) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Profile could not be applied: {error}")),
            };
        }
    };
    if let Some(model) = resolved_model.as_deref()
        && let Ok(mut settings) = shared_settings.lock()
    {
        settings.set_model(model);
        // The controller confirmed this serving route; anonymous providers do
        // not have stored credentials to re-probe.
        settings.provider_connected = true;
    }

    let new_model = shared_settings
        .lock()
        .ok()
        .map(|s| s.model.clone())
        .unwrap_or_default();
    if !new_model.is_empty()
        && new_model != old_model
        && let Some(new_bridge) = providers::auto_detect_bridge(&new_model).await
    {
        let mut guard = bridge.write().await;
        *guard = new_bridge;
    }

    let (slim, posture_disabled, posture_enabled) = shared_settings
        .lock()
        .ok()
        .map(|s| {
            (
                s.is_slim(),
                s.posture_disabled_tools.clone(),
                s.posture_enabled_tools.clone(),
            )
        })
        .unwrap_or_default();
    runtime_state.conversation.set_slim_mode(slim);
    runtime_state
        .bus
        .apply_operator_tool_profile(slim, &posture_disabled, &posture_enabled);
    if let Some(persona) = profile.persona.as_deref() {
        let call_id = format!("profile-apply-persona:{}", uuid::Uuid::new_v4());
        let _ = runtime_state
            .bus
            .invoke_internal(
                crate::tool_registry::persona::SWITCH_PERSONA,
                &call_id,
                serde_json::json!({ "name": persona, "reason": "profile apply" }),
                tokio_util::sync::CancellationToken::new(),
                internal_control_scope("kernel:profile-apply-persona"),
            )
            .await;
    }
    if let Some(tone) = profile.tone.as_deref() {
        let call_id = format!("profile-apply-tone:{}", uuid::Uuid::new_v4());
        let _ = runtime_state
            .bus
            .invoke_internal(
                crate::tool_registry::persona::SWITCH_TONE,
                &call_id,
                serde_json::json!({ "name": tone, "reason": "profile apply" }),
                tokio_util::sync::CancellationToken::new(),
                internal_control_scope("kernel:profile-apply-tone"),
            )
            .await;
    }

    let mut status = crate::status::HarnessStatus::assemble(runtime_state.bus.project_root());
    if let Ok(settings) = shared_settings.lock().map(|s| s.clone()) {
        let operating_profile = settings.operating_profile();
        status.update_routing(
            settings.effective_requested_class().label(),
            settings.thinking.as_str(),
            &status.capability_grade.clone(),
            operating_profile.posture.effective.display_name(),
            &operating_profile.summary(),
            operating_profile
                .identity
                .principal_id
                .as_deref()
                .unwrap_or("anonymous"),
            operating_profile
                .identity
                .issuer
                .as_deref()
                .unwrap_or("unknown"),
            operating_profile
                .identity
                .session_kind
                .as_deref()
                .unwrap_or("unknown"),
            &operating_profile.authorization.summary(),
        );
    }
    status.update_from_bus(&runtime_state.bus);
    let status_json = runtime_state.bus.emit_harness_status(&status);
    let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json });

    SlashCommandResponse {
        accepted: true,
        output: Some(
            "Profile applied to live runtime. Integration and extension load policy changes take effect on next startup."
                .into(),
        ),
    }
}

fn internal_control_scope(principal: &str) -> crate::invocation_service::InvocationScope {
    crate::invocation_service::InvocationScope {
        principal: principal.into(),
        principal_class: omegon_traits::RuntimePrincipalClass::Internal,
        surface: omegon_traits::RuntimeSurface::Internal,
        ..Default::default()
    }
}

pub async fn profile_apply_daemon_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
) -> SlashCommandResponse {
    let profile = settings::Profile::load(cwd);
    if let Ok(mut s) = shared_settings.lock() {
        profile.apply_to_with_posture(&mut s, cwd);
        s.provider_connected = crate::auth::provider_connected_for_model(&s.model);
        SlashCommandResponse {
            accepted: true,
            output: Some(
                "Profile applied to daemon runtime. Integration and extension load policy changes take effect on next startup."
                    .into(),
            ),
        }
    } else {
        SlashCommandResponse {
            accepted: false,
            output: Some("failed to update settings".into()),
        }
    }
}

pub async fn profile_set_mqtt_response(cwd: &Path, enabled: Option<bool>) -> SlashCommandResponse {
    let loaded = settings::Profile::load_with_source(cwd);
    let mut profile = loaded.profile;
    if let Some(enabled) = enabled {
        profile.integrations.mqtt.enabled = Some(enabled);
        let output = if enabled {
            "MQTT bridge profile default enabled. Takes effect on next startup."
        } else {
            "MQTT bridge profile default disabled. Takes effect on next startup."
        };
        return save_selected_profile_response(cwd, profile, &loaded.source, output);
    }

    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "MQTT bridge profile default: {}",
            match profile.integrations.mqtt.enabled {
                Some(true) => "enabled",
                Some(false) => "disabled",
                None => "unset (disabled by default)",
            }
        )),
    }
}

pub async fn profile_extension_allow_response(cwd: &Path, name: &str) -> SlashCommandResponse {
    let name = name.trim();
    if name.is_empty() {
        return usage_response("Usage: /profile extension allow <name>");
    }
    let loaded = settings::Profile::load_with_source(cwd);
    let mut profile = loaded.profile;
    retain_not_equal(&mut profile.extensions.disabled, name);
    push_unique(&mut profile.extensions.enabled, name);
    save_selected_profile_response(
        cwd,
        profile,
        &loaded.source,
        "Extension allowed in profile. Extension load policy takes effect on next startup.",
    )
}

pub async fn profile_extension_deny_response(cwd: &Path, name: &str) -> SlashCommandResponse {
    let name = name.trim();
    if name.is_empty() {
        return usage_response("Usage: /profile extension deny <name>");
    }
    let loaded = settings::Profile::load_with_source(cwd);
    let mut profile = loaded.profile;
    retain_not_equal(&mut profile.extensions.enabled, name);
    push_unique(&mut profile.extensions.disabled, name);
    save_selected_profile_response(
        cwd,
        profile,
        &loaded.source,
        "Extension denied in profile. Extension load policy takes effect on next startup.",
    )
}

pub async fn profile_extension_clear_response(cwd: &Path) -> SlashCommandResponse {
    let loaded = settings::Profile::load_with_source(cwd);
    let mut profile = loaded.profile;
    profile.extensions.enabled.clear();
    profile.extensions.disabled.clear();
    save_selected_profile_response(
        cwd,
        profile,
        &loaded.source,
        "Extension profile policy cleared. Installed enabled extensions are loadable again on next startup.",
    )
}

pub async fn profile_component_disable_response(
    cwd: &Path,
    selector: &str,
) -> SlashCommandResponse {
    profile_component_mutation_response(cwd, selector, false).await
}

async fn profile_component_mutation_response(
    cwd: &Path,
    selector: &str,
    enabled: bool,
) -> SlashCommandResponse {
    let selector = selector.trim();
    let catalog = crate::component_policy::ComponentCatalog::product_v1();
    if let Err(error) = catalog.validate_profile_selector(selector, "profile component command") {
        return SlashCommandResponse {
            accepted: false,
            output: Some(error.to_string()),
        };
    }
    let loaded = settings::Profile::load_with_source(cwd);
    if matches!(loaded.source, settings::ProfileSource::BuiltInDefault) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                "The active profile is built-in and read-only; select an explicit project or user profile target before changing component policy."
                    .into(),
            ),
        };
    }
    let mut profile = loaded.profile;
    profile.components.insert(
        selector.to_string(),
        crate::component_policy::ComponentSwitch { enabled },
    );
    let source = match profile.save_to_target(
        cwd,
        settings::ProfileSaveTarget::ActiveSource,
        &loaded.source,
    ) {
        Ok(source) => source,
        Err(error) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("failed to save profile: {error}")),
            };
        }
    };
    match profile_components_projection(cwd) {
        Ok(components) => SlashCommandResponse {
            accepted: true,
            output: Some(
                serde_json::json!({
                    "changedSource": source.to_string(),
                    "selector": selector,
                    "requestedEnabled": enabled,
                    "restartRequired": true,
                    "components": components,
                })
                .to_string(),
            ),
        },
        Err(error) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Component policy saved to {source}; restart required. Effective policy could not be rendered: {error}"
            )),
        },
    }
}

pub async fn profile_components_view_response(cwd: &Path) -> SlashCommandResponse {
    match profile_components_projection(cwd) {
        Ok(components) => SlashCommandResponse {
            accepted: true,
            output: Some(serde_json::json!({ "components": components }).to_string()),
        },
        Err(error) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("failed to resolve component policy: {error}")),
        },
    }
}

fn profile_components_projection(
    cwd: &Path,
) -> anyhow::Result<Vec<crate::surfaces::component::ComponentStatusProjection>> {
    let home = crate::paths::omegon_home()?;
    let policy = crate::component_policy::resolve_product_boot_policy(cwd, &home)?;
    Ok(policy
        .decisions()
        .map(|decision| {
            let package = (decision.component_id == "core:codescan").then(|| {
                let source = crate::setup::release_coupled_codescan_dir()
                    .map(|path| path.display().to_string());
                crate::surfaces::component::ComponentPackageProjection {
                    identity: crate::codescan_service::CODESCAN_EXTENSION.into(),
                    present: source.is_some(),
                    source,
                }
            });
            crate::surfaces::component::ComponentStatusProjection::new(
                &decision.into(),
                package,
                crate::surfaces::component::ComponentRuntimeEvidence::NotObserved,
            )
        })
        .collect())
}

pub async fn profile_set_persona_response(cwd: &Path, name: Option<&str>) -> SlashCommandResponse {
    let mut profile = settings::Profile::load(cwd);
    profile.persona = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    save_profile_response(cwd, profile, "Profile default persona updated.")
}

pub async fn profile_set_tone_response(cwd: &Path, name: Option<&str>) -> SlashCommandResponse {
    let mut profile = settings::Profile::load(cwd);
    profile.tone = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    save_profile_response(cwd, profile, "Profile default tone updated.")
}

pub async fn permissions_view_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
) -> SlashCommandResponse {
    let profile = settings::Profile::load(cwd);
    let live_trusted = shared_settings
        .lock()
        .ok()
        .map(|s| s.trusted_directories.clone())
        .unwrap_or_default();
    let profile_trusted = profile.effective_trusted_directories();
    SlashCommandResponse {
        accepted: true,
        output: Some(
            serde_json::json!({
                "permissions": {
                    "workspace": cwd.display().to_string(),
                    "liveTrustedDirectories": live_trusted,
                    "profileTrustedDirectories": profile_trusted,
                    "commands": [
                        "/permissions list",
                        "/permissions add <path>",
                        "/permissions remove <path>"
                    ],
                    "aliases": ["/trust add <path>", "/trust remove <path>"],
                    "promptKeys": {
                        "y": "allow once for this session",
                        "a": "always allow and save to project profile permissions",
                        "n": "deny",
                        "Esc": "deny"
                    },
                    "persistence": "always-allow grants are saved under profile.permissions.trustedDirectories",
                    "hardBoundaries": [
                        "secrets are still redacted and guarded",
                        "auth.json material is provider credential material, not an identity grant",
                        "operator deny always wins"
                    ]
                }
            })
            .to_string(),
        ),
    }
}

pub async fn automation_view_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
) -> SlashCommandResponse {
    let profile = settings::Profile::load(cwd);
    let live_level = shared_settings
        .lock()
        .ok()
        .map(|s| s.automation_level)
        .unwrap_or_default();
    let profile_level = profile.automation.level.unwrap_or_default();
    let live_subagent_policy = crate::autonomy::subagent_policy_for_automation(live_level);
    let profile_subagent_policy = crate::autonomy::subagent_policy_for_automation(profile_level);
    SlashCommandResponse {
        accepted: true,
        output: Some(
            serde_json::json!({
                "automation": {
                    "liveLevel": live_level.as_str(),
                    "liveSummary": live_level.summary(),
                    "profileLevel": profile_level.as_str(),
                    "profileSummary": profile_level.summary(),
                    "subagents": {
                        "liveLevel": live_subagent_policy.level.as_str(),
                        "profileLevel": profile_subagent_policy.level.as_str(),
                        "delegate": {
                            "scout": live_subagent_policy.delegate_scout.prompt_label(),
                            "patch": live_subagent_policy.delegate_patch.prompt_label(),
                            "verify": live_subagent_policy.delegate_verify.prompt_label()
                        },
                        "cleave": {
                            "assess": live_subagent_policy.cleave_assess.prompt_label(),
                            "run": live_subagent_policy.cleave_run.prompt_label(),
                            "maxChildren": live_subagent_policy.max_children,
                            "maxParallel": live_subagent_policy.max_parallel
                        },
                        "note": "automation is the operator-facing knob; loop and scheduled-job envelopes may further constrain this policy but do not grant extra authority by being schedulers"
                    },
                    "commands": [
                        "/automation ask",
                        "/automation guarded",
                        "/automation flow",
                        "/automation autonomous",
                        "/autonomy flow"
                    ],
                    "hardBoundaries": [
                        "permissions",
                        "security",
                        "plan gates",
                        "operator interrupt",
                        "max turns"
                    ],
                    "levels": {
                        "ask": "never auto-continue text-only proceed prompts",
                        "guarded": "continue only through low-risk proceed stalls",
                        "flow": "continue through action-shaped stalls until task completion",
                        "autonomous": "run to completion within the same hard gates"
                    }
                }
            })
            .to_string(),
        ),
    }
}

pub async fn automation_set_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    level: settings::AutomationLevel,
) -> SlashCommandResponse {
    if let Ok(mut s) = shared_settings.lock() {
        s.automation_level = level;
    }
    let mut profile = settings::Profile::load(cwd);
    profile.automation.level = Some(level);
    save_profile_response(
        cwd,
        profile,
        &format!(
            "Automation → {} ({})\n\
             This tunes continuation and subagent posture only; permissions, loop/job envelopes, and plan gates remain hard boundaries.",
            level.as_str(),
            level.summary()
        ),
    )
}

pub async fn permission_trust_add_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    path: &str,
) -> SlashCommandResponse {
    let path = path.trim();
    if path.is_empty() {
        return usage_response("Usage: /permissions add <path>");
    }
    let mount_identity =
        crate::tools::permissions::profile_mount_identity_for_path(Path::new(path));
    let environment = crate::tools::permissions::profile_environment_for_current_process();
    if let Ok(mut s) = shared_settings.lock() {
        push_unique(&mut s.trusted_directories, path);
    }
    let loaded = settings::Profile::load_with_source(cwd);
    let mut profile = loaded.profile;
    profile.add_trusted_directory_grant(path.to_string(), mount_identity, environment);
    save_active_profile_response(
        cwd,
        profile,
        &loaded.source,
        &format!(
            "Trusted directory added to project permissions: {path}\n\
             The agent can now read/write files in this directory."
        ),
    )
}

pub async fn permission_trust_remove_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    path: &str,
) -> SlashCommandResponse {
    let path = path.trim();
    if path.is_empty() {
        return usage_response("Usage: /permissions remove <path>");
    }
    if let Ok(mut s) = shared_settings.lock() {
        retain_not_equal(&mut s.trusted_directories, path);
    }
    let loaded = settings::Profile::load_with_source(cwd);
    let mut profile = loaded.profile;
    profile.remove_trusted_directory(path);
    save_active_profile_response(
        cwd,
        profile,
        &loaded.source,
        &format!("Trusted directory removed from project permissions: {path}"),
    )
}

fn save_profile_response(
    cwd: &Path,
    profile: settings::Profile,
    success: &str,
) -> SlashCommandResponse {
    match profile.save(cwd) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(success.to_string()),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("failed to save profile: {e}")),
        },
    }
}

fn save_selected_profile_response(
    cwd: &Path,
    profile: settings::Profile,
    source: &settings::ProfileSource,
    success: &str,
) -> SlashCommandResponse {
    if matches!(source, settings::ProfileSource::BuiltInDefault) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                "The active profile is built-in and read-only; select an explicit project or user profile target before changing it."
                    .into(),
            ),
        };
    }
    match profile.save_to_target(cwd, settings::ProfileSaveTarget::ActiveSource, source) {
        Ok(saved_source) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("{success} Source: {saved_source}")),
        },
        Err(error) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("failed to save profile: {error}")),
        },
    }
}

fn save_active_profile_response(
    cwd: &Path,
    profile: settings::Profile,
    source: &settings::ProfileSource,
    success: &str,
) -> SlashCommandResponse {
    let target = match source {
        settings::ProfileSource::BuiltInDefault => settings::ProfileSaveTarget::Project,
        _ => settings::ProfileSaveTarget::ActiveSource,
    };
    match profile.save_to_target(cwd, target, source) {
        Ok(_) => SlashCommandResponse {
            accepted: true,
            output: Some(success.to_string()),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("failed to save profile: {e}")),
        },
    }
}

fn usage_response(message: &str) -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: false,
        output: Some(message.to_string()),
    }
}

fn retain_not_equal(values: &mut Vec<String>, target: &str) {
    values.retain(|value| !value.eq_ignore_ascii_case(target));
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(value))
    {
        values.push(value.to_string());
    }
}

pub async fn profile_export_response(
    shared_settings: &settings::SharedSettings,
    cwd: &Path,
    handles: &crate::runtime_state::RuntimeStateHandles,
) -> SlashCommandResponse {
    let settings_json = if let Ok(s) = shared_settings.lock() {
        serde_json::json!({
            "model": s.model,
            "thinking_level": s.thinking.as_str(),
            "context_class": s.effective_requested_class().label(),
            "max_turns": s.max_turns,
            "slim_mode": s.is_slim(),
            "provider_order": s.provider_order,
        })
    } else {
        serde_json::json!(null)
    };

    let persona_json = handles
        .observe_harness()
        .ok()
        .flatten()
        .and_then(|harness| harness.active_persona)
        .map(|persona| {
            serde_json::json!({
                "id": persona.id,
                "name": persona.name,
                "badge": persona.badge,
                "activated_skills": persona.activated_skills,
                "disabled_tools": persona.disabled_tools,
            })
        })
        .unwrap_or(serde_json::Value::Null);

    let profile = settings::Profile::load(cwd);

    let export = serde_json::json!({
        "format": "omegon-profile-export",
        "version": env!("CARGO_PKG_VERSION"),
        "settings": settings_json,
        "persona": persona_json,
        "profile": serde_json::to_value(&profile).unwrap_or(serde_json::json!(null)),
    });

    let output = render_profile_export(&export, &settings_json, &persona_json, &profile);

    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

fn render_profile_export(
    export: &serde_json::Value,
    settings_json: &serde_json::Value,
    persona_json: &serde_json::Value,
    profile: &settings::Profile,
) -> String {
    let mut out = String::new();
    out.push_str("## Profile Export\n\n");
    out.push_str(&format!(
        "Version: `{}`\n\n",
        export["version"].as_str().unwrap_or("?")
    ));

    // Settings
    out.push_str("### Settings\n");
    if let Some(model) = settings_json["model"].as_str() {
        out.push_str(&format!("- Model: `{model}`\n"));
    }
    if let Some(thinking) = settings_json["thinking_level"].as_str() {
        out.push_str(&format!("- Thinking: `{thinking}`\n"));
    }
    if let Some(ctx) = settings_json["context_class"].as_str() {
        out.push_str(&format!("- Context class: `{ctx}`\n"));
    }
    if let Some(turns) = settings_json["max_turns"].as_u64() {
        out.push_str(&format!("- Max turns: `{turns}`\n"));
    }
    if let Some(slim) = settings_json["slim_mode"].as_bool() {
        out.push_str(&format!(
            "- Slim mode: `{}`\n",
            if slim { "on" } else { "off" }
        ));
    }
    if let Some(order) = settings_json["provider_order"].as_array()
        && !order.is_empty()
    {
        let providers: Vec<&str> = order.iter().filter_map(|v| v.as_str()).collect();
        out.push_str(&format!("- Provider order: `{}`\n", providers.join(" → ")));
    }

    // Persona
    out.push_str("\n### Persona\n");
    if persona_json.is_null() {
        out.push_str("None active\n");
    } else {
        if let Some(name) = persona_json["name"].as_str() {
            out.push_str(&format!("- Name: `{name}`\n"));
        }
        if let Some(badge) = persona_json["badge"].as_str() {
            out.push_str(&format!("- Badge: {badge}\n"));
        }
        if let Some(skills) = persona_json["activated_skills"].as_array()
            && !skills.is_empty()
        {
            let names: Vec<&str> = skills.iter().filter_map(|v| v.as_str()).collect();
            out.push_str(&format!("- Skills: {}\n", names.join(", ")));
        }
        if let Some(disabled) = persona_json["disabled_tools"].as_array()
            && !disabled.is_empty()
        {
            let names: Vec<&str> = disabled.iter().filter_map(|v| v.as_str()).collect();
            out.push_str(&format!("- Disabled tools: {}\n", names.join(", ")));
        }
    }

    // Saved profile summary
    out.push_str("\n### Saved profile\n");
    out.push_str("```json\n");
    out.push_str(&serde_json::to_string_pretty(profile).unwrap_or_else(|_| "null".to_string()));
    out.push_str("\n```\n");

    out
}

pub async fn persona_list_response(
    handles: &crate::runtime_state::RuntimeStateHandles,
    cwd: &Path,
) -> SlashCommandResponse {
    let active_id = handles
        .observe_harness()
        .ok()
        .flatten()
        .and_then(|h| h.active_persona.map(|p| p.id));

    let output = crate::plugins::persona_loader::with_available(cwd, |personas, tones| {
        let persona_list: Vec<serde_json::Value> = personas
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "active": active_id.as_deref() == Some(&p.id),
                })
            })
            .collect();
        let tone_list: Vec<serde_json::Value> = tones
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "description": t.description,
                })
            })
            .collect();
        serde_json::json!({
            "personas": persona_list,
            "tones": tone_list,
        })
    });

    SlashCommandResponse {
        accepted: true,
        output: Some(output.to_string()),
    }
}

pub async fn persona_switch_response(name: &str) -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: false,
        output: Some(format!(
            "Remote persona switching requires SharedPersonaRegistry (planned for 0.15.27). \
             For now, send a prompt with `/persona {name}` to switch via the agent, \
             or run `omegon` interactively and use `/persona {name}` directly."
        )),
    }
}

pub fn skills_help_text() -> &'static str {
    "Usage: /skills [list|reload|refresh|install [name|skills/name]|create|new [--project|--user]|import [--project|--user] <path>|get <name>|delete <name>]\n\n/skills opens the active skills inventory menu in the TUI and renders a readout on remote/CLI surfaces.\n/skills --help shows this command syntax.\n\nTUI menu keys:\n  ↑/↓     navigate skills and actions\n  Enter   inspect selected skill or run selected action\n  i       install/refresh selected skill\n  /       filter by name, source, state, tag, or profile\n  Esc     close\n\nCommon commands:\n  /skills get <name>          inspect manifest, provenance, activation, shadow, and conflicts\n  /skills reload              reload user/project/extension skills into this TUI session\n  /skills install [name]      install/refresh bundled skills or one public skill\n  /skills create --project    author a project-local skill\n  /skills import --project <path>\n                              import a reviewed skill bundle"
}

pub fn skills_help_response() -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some(skills_help_text().into()),
    }
}

pub async fn skills_view_response() -> SlashCommandResponse {
    match crate::skills::list_structured() {
        Ok(entries) => {
            if entries.is_empty() {
                return SlashCommandResponse {
                    accepted: true,
                    output: Some(
                        "No skills found. Run /skills install to install bundled skills.".into(),
                    ),
                };
            }

            SlashCommandResponse {
                accepted: true,
                output: Some(render_skills_menu(&entries)),
            }
        }
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/skills list failed: {err}")),
        },
    }
}

fn render_skills_menu(entries: &[crate::skills::SkillEntry]) -> String {
    skills_menu_projection(entries).render_markdown()
}

pub(crate) fn skills_menu_projection(
    entries: &[crate::skills::SkillEntry],
) -> crate::surfaces::menu::MenuProjection {
    use crate::surfaces::menu::{
        MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
        MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
    };

    let bundled_total = entries.iter().filter(|entry| entry.bundled).count();
    let bundled_installed = entries
        .iter()
        .filter(|entry| entry.bundled && entry.installed)
        .count();
    let user_total = entries
        .iter()
        .filter(|entry| !entry.bundled && !entry.project_local)
        .count();
    let project_total = entries.iter().filter(|entry| entry.project_local).count();

    let skill_rows = entries
        .iter()
        .map(|entry| {
            let description = crate::util::truncate(entry.description.trim(), 88);
            MenuRowProjection {
                id: format!("skills.{}", entry.name),
                label: entry.name.clone(),
                description,
                value: Some("Enter: details · i: install/refresh · g: full inspect".into()),
                kind: MenuRowKind::Object,
                badges: vec![
                    MenuBadgeProjection {
                        label: skill_scope_label(entry).to_string(),
                        tone: MenuBadgeTone::Info,
                    },
                    MenuBadgeProjection {
                        label: skill_state_label(entry).to_string(),
                        tone: match skill_state_tone(entry) {
                            crate::surfaces::palette::PaletteBadgeTone::Neutral => {
                                MenuBadgeTone::Neutral
                            }
                            crate::surfaces::palette::PaletteBadgeTone::Success => {
                                MenuBadgeTone::Success
                            }
                            crate::surfaces::palette::PaletteBadgeTone::Warning => {
                                MenuBadgeTone::Warning
                            }
                            crate::surfaces::palette::PaletteBadgeTone::Danger => {
                                MenuBadgeTone::Danger
                            }
                            crate::surfaces::palette::PaletteBadgeTone::Info => MenuBadgeTone::Info,
                        },
                    },
                ],
                metadata: skill_palette_metadata(entry),
                primary_action: Some(MenuActionProjection::focus_row(
                    format!("skills.details.{}", entry.name),
                    "Details",
                    format!("skills.{}", entry.name),
                )),
                actions: vec![
                    {
                        let mut action = MenuActionProjection::command(
                            format!("skills.install.{}", entry.name),
                            "Install/refresh",
                            format!("/skills install {}", entry.name),
                        );
                        action.key = Some("i".into());
                        action
                    },
                    {
                        let mut action = MenuActionProjection::command(
                            format!("skills.get.{}", entry.name),
                            "Full inspect",
                            format!("/skills get {}", entry.name),
                        );
                        action.key = Some("g".into());
                        action
                    },
                ],
                safety: None,
                availability: None,
            }
        })
        .collect();

    let action_rows = vec![
        MenuRowProjection {
            id: "skills.reload".into(),
            label: "Reload active skills".into(),
            description: "reload user/project/extension skills into the current TUI session".into(),
            value: Some("/skills reload".into()),
            kind: MenuRowKind::Action,
            badges: Vec::new(),
            metadata: vec!["session".into()],
            primary_action: Some(MenuActionProjection::command(
                "skills.reload",
                "Reload",
                "/skills reload",
            )),
            actions: Vec::new(),
            safety: None,
            availability: None,
        },
        MenuRowProjection {
            id: "skills.install.all".into(),
            label: "Install/refresh bundled skills".into(),
            description: "install or refresh all bundled skills".into(),
            value: Some("/skills install".into()),
            kind: MenuRowKind::Action,
            badges: Vec::new(),
            metadata: vec!["bundled".into()],
            primary_action: Some(MenuActionProjection::command(
                "skills.install.all",
                "Install",
                "/skills install",
            )),
            actions: Vec::new(),
            safety: None,
            availability: None,
        },
        MenuRowProjection {
            id: "skills.create.project".into(),
            label: "Create project skill".into(),
            description: "author a project-local skill through the skill builder prompt".into(),
            value: Some("/skills create --project".into()),
            kind: MenuRowKind::Action,
            badges: Vec::new(),
            metadata: vec!["project".into(), "authoring".into()],
            primary_action: Some(MenuActionProjection::command(
                "skills.create.project",
                "Create",
                "/skills create --project",
            )),
            actions: Vec::new(),
            safety: None,
            availability: None,
        },
        MenuRowProjection {
            id: "skills.import.project".into(),
            label: "Import project skill".into(),
            description: "import a reviewed skill bundle into project-local skills".into(),
            value: Some("/skills import --project <path>".into()),
            kind: MenuRowKind::Action,
            badges: Vec::new(),
            metadata: vec!["project".into(), "import".into()],
            primary_action: None,
            actions: Vec::new(),
            safety: None,
            availability: None,
        },
    ];

    MenuProjection {
        id: "skills".into(),
        title: "Skills".into(),
        summary: Some(format!(
            "Bundled {bundled_installed}/{bundled_total} installed · User {user_total} · Project {project_total}"
        )),
        tabs: vec![MenuTabProjection {
            id: "overview".into(),
            label: "Overview".into(),
            groups: vec![
                MenuGroupProjection {
                    id: "skills".into(),
                    label: "Installed and available skills".into(),
                    description: Some(
                        "Enter shows bounded skill details; use g for full `/skills get` output; filter by name, source, state, tag, or profile."
                            .into(),
                    ),
                    rows: skill_rows,
                },
                MenuGroupProjection {
                    id: "actions".into(),
                    label: "Actions".into(),
                    description: Some("Session and project-level skill operations.".into()),
                    rows: action_rows,
                },
            ],
        }],
        actions: Vec::new(),
        footer: Some(
            "↑/↓ navigate · Enter details · g full inspect · i install selected skill · / filter · `/skills --help` syntax · Esc close"
                .into(),
        ),
    }
}

fn skill_scope_label(entry: &crate::skills::SkillEntry) -> &str {
    if entry.source.is_empty() {
        if entry.project_local {
            "project"
        } else if entry.bundled {
            "bundled"
        } else {
            "user"
        }
    } else {
        entry.source.as_str()
    }
}

fn skill_state_label(entry: &crate::skills::SkillEntry) -> &'static str {
    if entry.project_local {
        "local"
    } else if entry.installed {
        "installed"
    } else if entry.bundled {
        "available"
    } else {
        "installed"
    }
}

fn skill_state_tone(
    entry: &crate::skills::SkillEntry,
) -> crate::surfaces::palette::PaletteBadgeTone {
    if entry.project_local || entry.installed {
        crate::surfaces::palette::PaletteBadgeTone::Success
    } else if entry.bundled {
        crate::surfaces::palette::PaletteBadgeTone::Neutral
    } else {
        crate::surfaces::palette::PaletteBadgeTone::Info
    }
}

fn skill_palette_metadata(entry: &crate::skills::SkillEntry) -> Vec<String> {
    let mut metadata = Vec::new();
    if let Some(activation) = entry
        .activation
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        metadata.push(activation.to_string());
    }
    if !entry.profile.is_empty() {
        metadata.push(format!("profile:{}", entry.profile.join("/")));
    }
    if !entry.tags.is_empty() {
        metadata.push(format!("tags:{}", entry.tags.join(",")));
    }
    metadata.push(
        if entry.editable {
            "editable"
        } else {
            "read-only"
        }
        .into(),
    );
    if entry.reloadable {
        metadata.push("reloadable".into());
    }
    if !entry.shadows.is_empty() {
        metadata.push(format!("shadows:{}", entry.shadows.join(",")));
    }
    if !entry.conflicts.is_empty() {
        metadata.push(format!("conflicts:{}", entry.conflicts.join(",")));
        metadata.push("resolve:merge-recommended".into());
    }
    if metadata.is_empty() {
        vec!["manual".into()]
    } else {
        metadata
    }
}

pub async fn skills_install_response(name: Option<&str>) -> SlashCommandResponse {
    if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        return match crate::armory::install(name, crate::armory::ArmoryInstallKind::Skill, &cwd)
            .await
        {
            Ok(result) => SlashCommandResponse {
                accepted: true,
                output: Some(armory_install_output(result)),
            },
            Err(err) => SlashCommandResponse {
                accepted: false,
                output: Some(format!("/skills install failed: {err}")),
            },
        };
    }

    match crate::skills::cmd_install() {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(
                "Installed bundled skills to ~/.omegon/skills. Run /skills reload to activate user/project skill changes in this session, or start a new session."
                    .to_string(),
            ),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/skills install failed: {err}")),
        },
    }
}

pub async fn plugin_view_response() -> SlashCommandResponse {
    match crate::plugin_cli::list_summary() {
        Ok(output) => SlashCommandResponse {
            accepted: true,
            output: Some(output),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/plugin list failed: {err}")),
        },
    }
}

pub async fn plugin_install_response(uri: &str) -> SlashCommandResponse {
    match crate::plugin_cli::install(uri.trim()) {
        Ok(result) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Installed plugin {} from {}",
                result.name,
                uri.trim()
            )),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/plugin install failed: {err}")),
        },
    }
}

pub async fn plugin_remove_response(name: &str) -> SlashCommandResponse {
    match crate::plugin_cli::remove(name.trim()) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("Removed plugin {}", name.trim())),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/plugin remove failed: {err}")),
        },
    }
}

pub async fn plugin_update_response(name: Option<&str>) -> SlashCommandResponse {
    match crate::plugin_cli::update(name.map(str::trim)) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(match name.map(str::trim).filter(|s| !s.is_empty()) {
                Some(name) => format!("Updated plugin {name}"),
                None => "Updated installed plugins.".to_string(),
            }),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/plugin update failed: {err}")),
        },
    }
}

// ── Skill response handlers ──────────────────────────────────────

pub async fn skill_get_response(name: &str) -> SlashCommandResponse {
    match crate::skills::get_skill_details(name) {
        Ok(details) => {
            let manifest = &details.manifest;
            let body = &details.body;
            let mut out = format!("Skill: {}\n", manifest.name);
            if !manifest.description.is_empty() {
                out.push_str(&format!("Description: {}\n", manifest.description));
            }
            if let Some(ref version) = manifest.version {
                out.push_str(&format!("Version: {version}\n"));
            }
            if let Some(ref entry) = details.entry {
                out.push_str(&format!("Source: {}\n", entry.source));
                out.push_str(&format!("Editable: {}\n", entry.editable));
                out.push_str(&format!("Reloadable: {}\n", entry.reloadable));
                if !entry.shadows.is_empty() {
                    out.push_str(&format!("Shadows: {}\n", entry.shadows.join(", ")));
                }
                if !entry.conflicts.is_empty() {
                    out.push_str(&format!("Conflicts: {}\n", entry.conflicts.join(", ")));
                    out.push_str(
                        "Recommended resolution: merge into a project-local skill so one activation slot injects one merged directive.\n",
                    );
                }
            }
            if !manifest.tags.is_empty() {
                out.push_str(&format!("Tags: {}\n", manifest.tags.join(", ")));
            }
            if !manifest.aliases.is_empty() {
                out.push_str(&format!("Aliases: {}\n", manifest.aliases.join(", ")));
            }
            if !manifest.triggers.is_empty() {
                out.push_str(&format!("Triggers: {}\n", manifest.triggers.join(", ")));
            }
            if let Some(ref posture) = manifest.posture {
                out.push_str(&format!("Posture: {posture}\n"));
            }
            if let Some(turns) = manifest.max_turns {
                out.push_str(&format!("Max turns: {turns}\n"));
            }
            out.push_str(&format!("Path: {}\n", details.path.display()));
            let preview = crate::util::truncate_str(body, 500);
            out.push_str(&format!("\n{preview}"));
            if body.len() > 500 {
                out.push_str("...");
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(out),
            }
        }
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/skills get failed: {err}")),
        },
    }
}

pub async fn skill_delete_response(name: &str) -> SlashCommandResponse {
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains('\0') {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Invalid skill name: path traversal rejected".into()),
        };
    }

    let cwd = std::env::current_dir().unwrap_or_default();
    let project_dir = cwd.join(".omegon/skills").join(name);
    let home = match crate::paths::omegon_home() {
        Ok(h) => h,
        Err(e) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Cannot determine home: {e}")),
            };
        }
    };
    let user_dir = home.join("skills").join(name);

    if project_dir.exists() {
        match std::fs::remove_dir_all(&project_dir) {
            Ok(()) => SlashCommandResponse {
                accepted: true,
                output: Some(format!("Deleted project-local skill '{name}'")),
            },
            Err(e) => SlashCommandResponse {
                accepted: false,
                output: Some(format!("Failed to delete skill: {e}")),
            },
        }
    } else if user_dir.exists() {
        match std::fs::remove_dir_all(&user_dir) {
            Ok(()) => SlashCommandResponse {
                accepted: true,
                output: Some(format!("Deleted skill '{name}'")),
            },
            Err(e) => SlashCommandResponse {
                accepted: false,
                output: Some(format!("Failed to delete skill: {e}")),
            },
        }
    } else {
        SlashCommandResponse {
            accepted: false,
            output: Some(format!("Skill '{name}' not found")),
        }
    }
}

// ── Extension response handlers ─────────────────────────────────

pub async fn extension_view_response() -> SlashCommandResponse {
    match crate::extension_cli::list_summary() {
        Ok(output) => SlashCommandResponse {
            accepted: true,
            output: Some(output),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension list failed: {err}")),
        },
    }
}

pub async fn extension_init_response(name: &str) -> SlashCommandResponse {
    match crate::extension_cli::init(name.trim()) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("Created extension scaffold `{}`", name.trim())),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension init failed: {err}")),
        },
    }
}

pub async fn extension_get_response(name: &str) -> SlashCommandResponse {
    let extensions_dir = match crate::extension_cli::extensions_dir() {
        Ok(d) => d,
        Err(e) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Cannot determine extensions directory: {e}")),
            };
        }
    };
    let ext_dir = extensions_dir.join(name);
    if !ext_dir.exists() {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Extension '{name}' not found")),
        };
    }
    match crate::extensions::ExtensionManifest::from_extension_dir(&ext_dir) {
        Ok(manifest) => {
            let state = crate::extensions::ExtensionState::load(&ext_dir).unwrap_or_default();
            let config = crate::extensions::config_store::read_config(&ext_dir).unwrap_or_default();
            let mut out = format!(
                "Extension: {}\nVersion: {}\nDescription: {}\nEnabled: {}\n",
                manifest.extension.name,
                manifest.extension.version,
                manifest.extension.description,
                state.enabled,
            );
            if !manifest.config.is_empty() {
                out.push_str("\nConfiguration:\n");
                for (key, field) in &manifest.config {
                    let current = config.get(key).map(|v| v.as_str()).unwrap_or("(unset)");
                    out.push_str(&format!("  {key}: {current}  ({})\n", field.label));
                }
            }
            if !manifest.secrets.required.is_empty() {
                out.push_str(&format!(
                    "\nRequired secrets: {}\n",
                    manifest.secrets.required.join(", ")
                ));
            }
            out.push_str(&format!("Path: {}\n", ext_dir.display()));
            SlashCommandResponse {
                accepted: true,
                output: Some(out),
            }
        }
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to load manifest: {e}")),
        },
    }
}

pub async fn extension_install_response(uri: &str) -> SlashCommandResponse {
    match crate::armory::install_extension(uri.trim(), None).await {
        Ok(result) => SlashCommandResponse {
            accepted: true,
            output: Some(armory_install_output(result)),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension install failed: {err}")),
        },
    }
}

pub async fn extension_remove_response(name: &str) -> SlashCommandResponse {
    match crate::extension_cli::remove(name.trim()) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("Removed extension {}", name.trim())),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension remove failed: {err}")),
        },
    }
}

pub async fn extension_update_response(name: Option<&str>) -> SlashCommandResponse {
    match crate::extension_cli::update(name.map(str::trim)) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(match name.map(str::trim).filter(|s| !s.is_empty()) {
                Some(name) => format!("Updated extension {name}. Run `/extension refresh` while the session is idle to publish a compatible changed generation."),
                None => "Updated installed extensions. Run `/extension refresh` while the session is idle to publish compatible changed generations.".to_string(),
            }),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension update failed: {err}")),
        },
    }
}

pub async fn extension_enable_response(name: &str) -> SlashCommandResponse {
    match crate::extension_cli::enable(name.trim()) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Enabled extension {}. Run `/extension refresh` while the session is idle to publish a compatible generation.",
                name.trim()
            )),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension enable failed: {err}")),
        },
    }
}

pub async fn extension_disable_response(name: &str) -> SlashCommandResponse {
    match crate::extension_cli::disable(name.trim()) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Disabled extension {}. Run `/extension refresh` to reconcile the current session; a process restart can still be required for widget or voice side channels.",
                name.trim()
            )),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension disable failed: {err}")),
        },
    }
}

fn extension_installation_search_matches(
    query: Option<&str>,
) -> Vec<(String, &'static str, String)> {
    use crate::capabilities::extensions::ExtensionInstallationDiagnosis;
    let query = query.map(str::trim).filter(|query| !query.is_empty());
    let Some(query) = query else {
        return Vec::new();
    };
    let query = query.to_lowercase();
    crate::extension_cli::extensions_dir()
        .ok()
        .and_then(|dir| {
            crate::capabilities::extensions::list_extension_installations_from_dir(&dir).ok()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|installation| installation.filesystem_name.to_lowercase().contains(&query))
        .map(|installation| {
            let (state, problem) = match installation.diagnosis {
                ExtensionInstallationDiagnosis::Valid { capability } => (
                    "installed",
                    format!("v{} · {}", capability.version, capability.description),
                ),
                ExtensionInstallationDiagnosis::Invalid { problem } => ("invalid", problem),
                ExtensionInstallationDiagnosis::BrokenLink { problem } => ("broken link", problem),
                ExtensionInstallationDiagnosis::Unreadable { problem } => ("unreadable", problem),
            };
            (installation.filesystem_name, state, problem)
        })
        .collect()
}

pub async fn extension_search_response(query: Option<&str>) -> SlashCommandResponse {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let local_matches = extension_installation_search_matches(query);
    match crate::armory::browse(crate::armory::BrowseOptions::new(
        crate::armory::ArmoryKind::Extensions,
        query,
        &cwd,
    ))
    .await
    {
        Ok(items) => {
            let mut sections = Vec::new();
            if items.is_empty() {
                sections.push(match query {
                    Some(q) => format!("No Armory catalog extensions found matching '{q}'."),
                    None => "No extensions found in the Armory catalog.".into(),
                });
            } else {
                let mut catalog = format!("Armory catalog extensions ({}):\n", items.len());
                for item in &items {
                    catalog.push_str(&format!(
                        "\n  {:<28} {}\n    {}\n",
                        item.id, item.category, item.description
                    ));
                }
                catalog.push_str("\nInstall: /extension install <name>");
                sections.push(catalog);
            }
            if !local_matches.is_empty() {
                let mut local = format!("Installed extension matches ({}):\n", local_matches.len());
                for (name, state, problem) in local_matches {
                    local.push_str(&format!("\n  {name:<28} {state}\n    {problem}\n"));
                }
                sections.push(local);
            }
            SlashCommandResponse {
                accepted: true,
                output: Some(sections.join("\n\n")),
            }
        }
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Could not reach armory: {e}")),
        },
    }
}

pub async fn armory_browse_response(query: Option<&str>) -> SlashCommandResponse {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match crate::armory::browse(crate::armory::BrowseOptions::new(
        crate::armory::ArmoryKind::All,
        query,
        &cwd,
    ))
    .await
    {
        Ok(items) => {
            let mut output = crate::armory::render_items(&items);
            output.push_str(
                "\n\nTUI install: /armory install <item> (examples: /armory install skills/security, /extension install flynt).",
            );
            SlashCommandResponse {
                accepted: true,
                output: Some(output),
            }
        }
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Could not browse armory: {err}")),
        },
    }
}

pub async fn armory_install_response(target: &str) -> SlashCommandResponse {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match crate::armory::install(target, crate::armory::ArmoryInstallKind::Auto, &cwd).await {
        Ok(result) => SlashCommandResponse {
            accepted: true,
            output: Some(armory_install_output(result)),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/armory install failed: {err}")),
        },
    }
}

fn armory_install_output(result: crate::armory::ArmoryInstallResult) -> String {
    let followup = match result.kind {
        crate::armory::ArmoryItemKind::Extension => {
            "New sessions will discover the extension. Use /extension list to verify it is installed, or run /extension refresh while the current session is idle to publish a compatible generation."
        }
        crate::armory::ArmoryItemKind::Plugin => {
            "New sessions will discover the plugin. Use /plugin list, /persona list, or /armory search to verify the installed surface."
        }
        crate::armory::ArmoryItemKind::Skill => {
            "Run /skills reload to activate user/project skill changes in this session, or start a new session. Use /skills list to verify it is installed."
        }
        crate::armory::ArmoryItemKind::Agent => {
            "Use /catalog list to verify installed agent catalog entries."
        }
    };
    format!("{}\n\n{followup}", result.message)
}

// ── Catalog response handler ────────────────────────────────────

pub async fn catalog_view_response() -> SlashCommandResponse {
    let home = match crate::paths::omegon_home() {
        Ok(h) => h,
        Err(e) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Cannot determine home: {e}")),
            };
        }
    };
    let entries = match crate::catalog::list(&home) {
        Ok(entries) => entries,
        Err(error) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Catalog discovery failed: {error}")),
            };
        }
    };
    if entries.is_empty() {
        return SlashCommandResponse {
            accepted: true,
            output: Some(
                "No catalog agents installed.\nRun `omegon catalog install` to install bundled agents.".into()
            ),
        };
    }
    let mut out = format!("Catalog agents ({}):\n\n", entries.len());
    for entry in entries.iter() {
        out.push_str(&format!(
            "  {:<32} {}\n    {}\n\n",
            entry.id, entry.domain, entry.description
        ));
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(out),
    }
}

pub async fn catalog_install_response() -> SlashCommandResponse {
    match crate::catalog::cmd_install(false).await {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some("Catalog agents installed.".into()),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/catalog install failed: {err}")),
        },
    }
}

pub async fn catalog_remove_response(id: &str) -> SlashCommandResponse {
    let home = match crate::paths::omegon_home() {
        Ok(h) => h,
        Err(e) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Cannot determine home: {e}")),
            };
        }
    };
    match crate::catalog::remove(&home, id) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("Removed catalog agent '{id}'")),
        },
        Err(error) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to remove catalog agent: {error}")),
        },
    }
}

pub async fn vault_status_response(agent: &crate::InteractiveAgentHost) -> SlashCommandResponse {
    if let Some(status) = agent.secrets.vault_status().await {
        return SlashCommandResponse {
            accepted: true,
            output: Some(status),
        };
    }

    let addr = std::env::var("VAULT_ADDR").unwrap_or_default();
    if addr.is_empty() {
        return SlashCommandResponse {
            accepted: true,
            output: Some(
                "Vault: not configured (VAULT_ADDR not set)\n\nUse `/vault configure` or set VAULT_ADDR"
                    .to_string(),
            ),
        };
    }

    let config_dir = dirs::home_dir()
        .unwrap_or_else(|| agent.cwd.clone())
        .join(".omegon");
    if let Some(health) = omegon_secrets::SecretsManager::vault_health_probe(&config_dir).await {
        let icon = if health.sealed { "🔒" } else { "🔓" };
        return SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Vault {icon}\n  Address:  {addr}\n  Status:   {}\n  Initialized: {}\n  Standby:  {}",
                if health.sealed { "sealed" } else { "unsealed" },
                if health.initialized { "yes" } else { "no" },
                if health.standby { "yes" } else { "no" },
            )),
        };
    }

    SlashCommandResponse {
        accepted: false,
        output: Some(format!(
            "Vault ✗\n  Address:  {addr}\n  Status:   unreachable"
        )),
    }
}

pub async fn vault_unseal_response() -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some(
            "Vault Unseal:\n\n\
             Masked unseal input is not yet implemented in the TUI.\n\
             Use the vault CLI directly:\n\
             \n  vault operator unseal\n\
             \nThis will prompt for unseal keys without echoing them.\n\
             Repeat until the threshold is met."
                .to_string(),
        ),
    }
}

pub async fn vault_login_response() -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some(
            "Vault Login:\n\n\
             Interactive login is not yet implemented in the TUI.\n\
             Use the vault CLI:\n\
             \n  vault login                         # token (interactive)\n\
             \n  vault login -method=approle         # AppRole\n\
               role_id=<role> secret_id=<secret>\n\
             \nThe token will be stored in ~/.vault-token automatically."
                .to_string(),
        ),
    }
}

pub async fn vault_configure_response() -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some(
            "Vault Configuration:\n\n\
             Interactive setup flows:\n\
             \n  /vault configure env   # prime the editor with an env-based setup\n\
             \n  /vault configure file  # prime the editor with a ~/.omegon/vault.json setup\n\
             \nManual options:\n\
             \n  export VAULT_ADDR=https://vault.example.com\n\
             \nAuthenticate with:\n\
             \n  vault login                  # interactive\n\
             \n  vault login -method=approle  # AppRole\n\
             \nOr create ~/.omegon/vault.json:\n\
             \n  {\"addr\": \"https://vault.example.com\", \"auth\": \"token\", \"allowed_paths\": [\"secret/data/omegon/*\"], \"denied_paths\": []}"
                .to_string(),
        ),
    }
}

pub async fn vault_init_policy_response() -> SlashCommandResponse {
    SlashCommandResponse {
        accepted: true,
        output: Some(
            "# Omegon Agent Vault Policy\n\
             # Apply with: vault policy write omegon omegon-policy.hcl\n\n\
             ```hcl\n\
             # Read/write agent-scoped secrets\n\
             path \"secret/data/omegon/*\" {\n  capabilities = [\"read\", \"create\", \"update\"]\n}\n\
             path \"secret/metadata/omegon/*\" {\n  capabilities = [\"read\", \"list\"]\n}\n\n\
             # Read-only access to shared infra secrets\n\
             path \"secret/data/bootstrap/*\" {\n  capabilities = [\"read\"]\n}\n\n\
             # Allow minting child tokens for cleave\n\
             path \"auth/token/create\" {\n  capabilities = [\"create\", \"update\"]\n  allowed_parameters = {\n    \"policies\" = [\"omegon-child\"]\n    \"ttl\" = [\"30m\"]\n    \"num_uses\" = [\"100\"]\n  }\n}\n\
             ```\n\n\
             Save to a file and apply: `vault policy write omegon <file>`"
                .to_string(),
        ),
    }
}

pub async fn cleave_status_response(
    runtime_state: &mut InteractiveAgentState,
    invocation_scope: &crate::invocation_service::InvocationScope,
) -> SlashCommandResponse {
    match dispatch_control_feature_command(
        &mut runtime_state.bus,
        "cleave",
        "status",
        invocation_scope,
    ) {
        omegon_traits::CommandResult::Display(text) => SlashCommandResponse {
            accepted: true,
            output: Some(text),
        },
        omegon_traits::CommandResult::Handled => SlashCommandResponse {
            accepted: true,
            output: None,
        },
        omegon_traits::CommandResult::NotHandled => SlashCommandResponse {
            accepted: false,
            output: Some("Cleave feature is unavailable.".to_string()),
        },
    }
}

pub async fn cleave_cancel_child_response(
    runtime_state: &mut InteractiveAgentState,
    label: &str,
    invocation_scope: &crate::invocation_service::InvocationScope,
) -> SlashCommandResponse {
    match dispatch_control_feature_command(
        &mut runtime_state.bus,
        "cleave",
        &format!("cancel {label}"),
        invocation_scope,
    ) {
        omegon_traits::CommandResult::Display(text) => SlashCommandResponse {
            accepted: true,
            output: Some(text),
        },
        omegon_traits::CommandResult::Handled => SlashCommandResponse {
            accepted: true,
            output: None,
        },
        omegon_traits::CommandResult::NotHandled => SlashCommandResponse {
            accepted: false,
            output: Some("Cleave feature is unavailable.".to_string()),
        },
    }
}

pub async fn delegate_status_response(
    runtime_state: &mut InteractiveAgentState,
    invocation_scope: &crate::invocation_service::InvocationScope,
) -> SlashCommandResponse {
    match dispatch_control_feature_command(
        &mut runtime_state.bus,
        "delegate",
        "status",
        invocation_scope,
    ) {
        omegon_traits::CommandResult::Display(text) => SlashCommandResponse {
            accepted: true,
            output: Some(text),
        },
        omegon_traits::CommandResult::Handled => SlashCommandResponse {
            accepted: true,
            output: None,
        },
        omegon_traits::CommandResult::NotHandled => SlashCommandResponse {
            accepted: false,
            output: Some("Delegate feature is unavailable.".to_string()),
        },
    }
}

fn dispatch_control_feature_command(
    bus: &mut crate::bus::EventBus,
    name: &str,
    args: &str,
    invocation_scope: &crate::invocation_service::InvocationScope,
) -> omegon_traits::CommandResult {
    let call_id = format!("control-command:{}", uuid::Uuid::new_v4());
    bus.invoke_command(name, &call_id, args, invocation_scope.clone(), None)
        .unwrap_or_else(|denial| {
            omegon_traits::CommandResult::Display(format!(
                "{}: {}",
                denial.code.as_str(),
                denial.message
            ))
        })
}

pub(crate) fn format_auth_status(status: &auth::AuthStatus) -> String {
    let authenticated = status
        .providers
        .iter()
        .filter(|provider| matches!(provider.status, auth::ProviderAuthStatus::Authenticated))
        .count();
    let expired = status
        .providers
        .iter()
        .filter(|provider| matches!(provider.status, auth::ProviderAuthStatus::Expired))
        .count();
    let mut lines = vec![
        "Authentication Overview".to_string(),
        String::new(),
        format!(
            "Auth file\n  Path:            {}{}",
            auth::auth_json_path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unavailable".into()),
            if std::env::var("OMEGON_AUTH_JSON_PATH")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .is_some()
            {
                " (OMEGON_AUTH_JSON_PATH)"
            } else {
                ""
            }
        ),
        String::new(),
        format!(
            "Providers\n  Authenticated:   {authenticated}/{}",
            status.providers.len()
        ),
        format!("  Expired:         {expired}"),
    ];

    if status.providers.is_empty() {
        lines.push("  Status:          no providers detected".to_string());
        return lines.join("\n");
    }

    lines.push(String::new());
    lines.push("Provider Status".to_string());

    for provider in &status.providers {
        let state = match provider.status {
            auth::ProviderAuthStatus::Authenticated => {
                if provider.is_oauth {
                    "✓ authenticated (oauth)".to_string()
                } else {
                    "✓ authenticated".to_string()
                }
            }
            auth::ProviderAuthStatus::Expired => "⚠ expired — re-login required".to_string(),
            auth::ProviderAuthStatus::Missing => "✗ not authenticated".to_string(),
            auth::ProviderAuthStatus::Error => provider
                .details
                .as_ref()
                .map(|d| format!("✗ error ({d})"))
                .unwrap_or_else(|| "✗ error".to_string()),
        };
        lines.push(format!("  {:<18} {}", provider.name, state));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn zen_model_status_reports_authoritative_connection_state() {
        let shared = settings::shared("opencode-zen:big-pickle");
        for (ready, expected) in [(true, "Yes"), (false, "No")] {
            shared.lock().unwrap().provider_connected = ready;
            let response = model_view_response(&shared).await;
            assert!(
                response
                    .output
                    .unwrap()
                    .contains(&format!("Connected:       {expected}"))
            );
        }
    }

    struct EnvironmentGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvironmentGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvironmentGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    struct ControlCommandFeature;

    #[async_trait::async_trait]
    impl omegon_traits::Feature for ControlCommandFeature {
        fn name(&self) -> &str {
            "control-command-test"
        }

        fn commands(&self) -> Vec<omegon_traits::CommandDefinition> {
            vec![omegon_traits::CommandDefinition {
                name: "control_test".into(),
                description: "control command lease test".into(),
                subcommands: vec![],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
                surface: Default::default(),
            }]
        }

        fn handle_command(&mut self, name: &str, _args: &str) -> omegon_traits::CommandResult {
            if name == "control_test" {
                omegon_traits::CommandResult::Handled
            } else {
                omegon_traits::CommandResult::NotHandled
            }
        }
    }

    #[test]
    fn tui_control_feature_bridge_enforces_operator_lease_admission() {
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(ControlCommandFeature));
        bus.finalize();
        let model_scope = crate::invocation_service::InvocationScope {
            surface: omegon_traits::RuntimeSurface::Tui,
            ..Default::default()
        };
        let denied = dispatch_control_feature_command(&mut bus, "control_test", "", &model_scope);
        assert!(matches!(
            denied,
            omegon_traits::CommandResult::Display(message)
                if message.starts_with("invocation:rbac_denied:")
        ));

        let operator_scope = crate::invocation_service::InvocationScope {
            principal: "tui-operator".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Operator,
            surface: omegon_traits::RuntimeSurface::Tui,
            ..Default::default()
        };
        assert!(matches!(
            dispatch_control_feature_command(&mut bus, "control_test", "", &operator_scope,),
            omegon_traits::CommandResult::Handled
        ));

        let web_scope = crate::invocation_service::InvocationScope {
            principal: "web-operator".into(),
            principal_class: omegon_traits::RuntimePrincipalClass::Operator,
            surface: omegon_traits::RuntimeSurface::Web,
            ..Default::default()
        };
        assert!(matches!(
            dispatch_control_feature_command(&mut bus, "control_test", "", &web_scope),
            omegon_traits::CommandResult::Display(message)
                if message.starts_with("invocation:unsupported_surface:")
        ));
    }

    #[tokio::test]
    async fn active_harness_dispatch_responds_without_waiting_for_inference() {
        let temp = tempfile::tempdir().unwrap();
        let settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::default()));
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(temp.path()).unwrap());
        let handles = crate::runtime_state::RuntimeStateHandles::default();
        let context = HarnessControlContext {
            shared_settings: &settings,
            secrets: &secrets,
            cwd: temp.path(),
            dashboard_handles: &handles,
            route_controller: None,
            dynamic_contribution_control: None,
        };
        let (events_tx, _) = broadcast::channel(8);
        let (reply_tx, reply_rx) = oneshot::channel();
        let blocked_worker = tokio::spawn(std::future::pending::<()>());

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            execute_active_harness_command(
                &context,
                crate::operator_commands::OperatorCommand::SetThinking {
                    level: settings::ThinkingLevel::High,
                    respond_to: Some(reply_tx),
                },
                &events_tx,
            ),
        )
        .await
        .expect("harness control must not wait for inference");

        assert!(matches!(result, ActiveHarnessCommandResult::Handled));
        let response = tokio::time::timeout(std::time::Duration::from_millis(250), reply_rx)
            .await
            .expect("response must arrive while worker remains active")
            .unwrap();
        assert!(response.accepted);
        assert!(!blocked_worker.is_finished());
        assert_eq!(
            settings.lock().unwrap().thinking,
            settings::ThinkingLevel::High
        );
        blocked_worker.abort();
    }

    #[tokio::test]
    async fn runtime_doctor_is_read_only_and_unknown_replacement_is_rejected() {
        let control = crate::contribution_lifecycle::DynamicContributionControl::default();
        let diagnosis = runtime_doctor_response(Some(&control));
        assert!(diagnosis.accepted);
        assert_eq!(
            diagnosis.output.as_deref(),
            Some("Runtime doctor: no published extension processes.")
        );

        let replacement =
            runtime_extension_replace_response(Some(&control), "missing-extension").await;
        assert!(!replacement.accepted);
        assert!(
            replacement
                .output
                .is_some_and(|output| output.contains("not published in this runtime"))
        );
    }

    #[tokio::test]
    async fn active_harness_preserves_non_tui_control_surface_on_handoff() {
        let temp = tempfile::tempdir().unwrap();
        let settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::default()));
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(temp.path()).unwrap());
        let handles = crate::runtime_state::RuntimeStateHandles::default();
        let context = HarnessControlContext {
            shared_settings: &settings,
            secrets: &secrets,
            cwd: temp.path(),
            dashboard_handles: &handles,
            route_controller: None,
            dynamic_contribution_control: None,
        };
        let (events_tx, _) = broadcast::channel(8);

        let result = execute_active_harness_command(
            &context,
            crate::operator_commands::OperatorCommand::ExecuteControlFrom {
                request: ControlRequest::TreeView {
                    args: "status".into(),
                },
                respond_to: None,
                surface: omegon_traits::RuntimeSurface::Ipc,
            },
            &events_tx,
        )
        .await;

        assert!(matches!(
            result,
            ActiveHarnessCommandResult::Unsupported(
                crate::operator_commands::OperatorCommand::ExecuteControlFrom {
                    surface: omegon_traits::RuntimeSurface::Ipc,
                    ..
                }
            )
        ));
    }

    #[test]
    fn active_turn_harness_command_contract_covers_operator_controls() {
        for (name, args) in [
            ("model", "list"),
            ("think", "high"),
            ("secrets", ""),
            ("variables", ""),
        ] {
            let canonical = crate::runtime_commands::canonical_slash_command(name, args)
                .unwrap_or_else(|| panic!("/{name} {args} must be canonical"));
            let request = control_request_from_slash(&canonical)
                .unwrap_or_else(|| panic!("/{name} {args} must remain inference-independent"));
            assert!(
                matches!(
                    request,
                    ControlRequest::ModelView
                        | ControlRequest::ModelList
                        | ControlRequest::SetThinking { .. }
                        | ControlRequest::SecretsView
                        | ControlRequest::VariablesView
                ),
                "/{name} {args} mapped to unexpected request: {request:?}"
            );
        }
    }

    #[tokio::test]
    async fn active_slash_command_responds_while_worker_remains_active() {
        let temp = tempfile::tempdir().unwrap();
        let settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::default()));
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(temp.path()).unwrap());
        let handles = crate::runtime_state::RuntimeStateHandles::default();
        let context = HarnessControlContext {
            shared_settings: &settings,
            secrets: &secrets,
            cwd: temp.path(),
            dashboard_handles: &handles,
            route_controller: None,
            dynamic_contribution_control: None,
        };
        let (events_tx, _) = broadcast::channel(8);
        let (reply_tx, reply_rx) = oneshot::channel();
        let blocked_worker = tokio::spawn(std::future::pending::<()>());

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            execute_active_harness_command(
                &context,
                crate::operator_commands::OperatorCommand::RunSlashCommand {
                    name: "think".into(),
                    args: "high".into(),
                    respond_to: Some(reply_tx),
                },
                &events_tx,
            ),
        )
        .await
        .expect("slash control must not wait for inference");

        assert!(matches!(result, ActiveHarnessCommandResult::Handled));
        let response = tokio::time::timeout(std::time::Duration::from_millis(250), reply_rx)
            .await
            .expect("slash response must arrive while worker remains active")
            .unwrap();
        assert!(response.accepted);
        assert!(!blocked_worker.is_finished());
        assert_eq!(
            settings.lock().unwrap().thinking,
            settings::ThinkingLevel::High
        );
        blocked_worker.abort();
    }

    #[tokio::test]
    async fn unsupported_active_command_is_returned_intact_instead_of_disappearing() {
        let temp = tempfile::tempdir().unwrap();
        let settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings::default()));
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(temp.path()).unwrap());
        let handles = crate::runtime_state::RuntimeStateHandles::default();
        let context = HarnessControlContext {
            shared_settings: &settings,
            secrets: &secrets,
            cwd: temp.path(),
            dashboard_handles: &handles,
            route_controller: None,
            dynamic_contribution_control: None,
        };
        let (events_tx, _) = broadcast::channel(8);
        let result = execute_active_harness_command(
            &context,
            crate::operator_commands::OperatorCommand::Compact,
            &events_tx,
        )
        .await;
        assert!(matches!(
            result,
            ActiveHarnessCommandResult::Unsupported(
                crate::operator_commands::OperatorCommand::Compact
            )
        ));
    }

    #[test]
    fn secret_response_functions_stay_in_control_secrets_module() {
        let source = include_str!("control_runtime.rs");
        for suffix in [
            "view_response",
            "set_response",
            "get_response",
            "delete_response",
        ] {
            let forbidden = format!("pub async fn secrets_{suffix}");
            assert!(
                !source.contains(&forbidden),
                "secret response ownership belongs in control/secrets.rs, not control_runtime.rs: {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn applying_profile_updates_live_route_controller_model_intent() {
        let controller = Arc::new(crate::route::RouteController::with_initial_intent(
            crate::route::ProviderRoute::Serving {
                model: crate::route::ModelRouteSpec::parse("anthropic:claude-sonnet-4-6"),
            },
            Box::new(crate::bridge::MockBridge { events: vec![] }),
            None,
            crate::route::ModelIntent::pinned_model("anthropic:claude-sonnet-4-6".into()),
        ));
        let profile = settings::Profile {
            model_intent: Some(settings::ProfileModelIntent {
                grade: Some("B".into()),
                provider: Some("auto".into()),
                grade_policy: Some("minimum".into()),
                provider_policy: None,
                exact_model_override: Some("anthropic:claude-sonnet-4-6".into()),
            }),
            ..settings::Profile::default()
        };

        let applied_model = apply_profile_model_intent(&profile, Some(&controller))
            .await
            .expect("profile intent should apply");

        let snapshot = controller.snapshot().await;
        assert_eq!(snapshot.intent.grade, Some(crate::route::ModelGrade::B));
        assert_eq!(
            snapshot.intent.exact_model_override.as_deref(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(
            snapshot.serving_model(),
            Some("anthropic:claude-sonnet-4-6")
        );
        assert_eq!(applied_model.as_deref(), snapshot.serving_model());
    }

    #[test]
    fn context_status_projection_uses_palette_instead_of_dump() {
        let rendered = context_status_projection(
            23_271,
            1_000_000,
            2,
            settings::ContextClass::Compact,
            settings::ContextClass::Massive,
            "openai-codex:gpt-5.5",
            crate::settings::ThinkingLevel::High,
            12_345,
        )
        .render_markdown();

        assert!(rendered.starts_with("## Context"));
        assert!(rendered.contains("23271/1000000 tokens (2%)"));
        assert!(rendered.contains("requested Compact (128k)"));
        assert!(rendered.contains("actual Massive (1M+)"));
        assert!(rendered.contains("### Actions"));
        assert!(
            rendered
                .contains("- `/context compact` — compact older turns through the context manager")
        );
        assert!(
            rendered.contains("- `/context request <kind> <query>` — pull a mediated context pack")
        );
        assert!(rendered.contains("### Context classes"));
        assert!(rendered.contains("- `/context compact` — Compact (128k) · requested"));
        assert!(rendered.contains("- `/context massive` — Massive (1M+) · actual"));
        assert!(!rendered.contains("Meter:"));
        assert!(!rendered.contains("System prompt:"));
        assert!(!rendered.contains("Tool schemas:"));
    }

    #[tokio::test]
    async fn set_thinking_response_is_runtime_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"thinkingLevel":"medium"}"#,
        )
        .unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::Minimal,
            ..Default::default()
        }));

        let response = set_thinking_response(
            &shared_settings,
            tmp.path(),
            crate::settings::ThinkingLevel::High,
        )
        .await;

        assert!(response.accepted);
        assert!(
            response
                .output
                .unwrap_or_default()
                .contains("live override")
        );
        assert_eq!(
            shared_settings.lock().unwrap().thinking,
            crate::settings::ThinkingLevel::High
        );
        let profile = settings::Profile::load(tmp.path());
        assert_eq!(profile.thinking_level.as_deref(), Some("medium"));

        let view = profile_view_response(&shared_settings, tmp.path()).await;
        let output = view.output.unwrap_or_default();
        assert!(
            output.contains("| Thinking | `medium` | `high` | live only |"),
            "{output}"
        );
    }

    #[tokio::test]
    async fn set_thinking_response_does_not_roll_back_for_profile_write_errors() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".omegon"), "not a directory").unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::Minimal,
            ..Default::default()
        }));

        let response = set_thinking_response(
            &shared_settings,
            tmp.path(),
            crate::settings::ThinkingLevel::High,
        )
        .await;

        assert!(response.accepted);
        assert_eq!(
            shared_settings.lock().unwrap().thinking,
            crate::settings::ThinkingLevel::High,
            "runtime-only changes should not depend on profile persistence"
        );
    }

    #[tokio::test]
    async fn set_context_class_response_is_runtime_only() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"requestedContextClass":"extended"}"#,
        )
        .unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            requested_context_class: Some(crate::settings::ContextClass::Compact),
            ..Default::default()
        }));

        let response = set_context_class_daemon_response(
            &shared_settings,
            tmp.path(),
            crate::settings::ContextClass::Massive,
        )
        .await;

        assert!(response.accepted);
        assert!(
            response
                .output
                .unwrap_or_default()
                .contains("live override")
        );
        assert_eq!(
            shared_settings.lock().unwrap().requested_context_class,
            Some(crate::settings::ContextClass::Massive)
        );
        let profile = settings::Profile::load(tmp.path());
        assert_eq!(profile.requested_context_class.as_deref(), Some("extended"));

        let view = profile_view_response(&shared_settings, tmp.path()).await;
        let output = view.output.unwrap_or_default();
        assert!(
            output.contains("| Context class | `extended` | `massive` | live only |"),
            "{output}"
        );
    }

    #[tokio::test]
    async fn profile_save_clears_thinking_and_context_drift() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"thinkingLevel":"medium","requestedContextClass":"extended"}"#,
        )
        .unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::High,
            requested_context_class: Some(crate::settings::ContextClass::Massive),
            ..Default::default()
        }));

        let before = profile_view_response(&shared_settings, tmp.path()).await;
        let before_output = before.output.unwrap_or_default();
        assert!(
            before_output.contains("Runtime drift: Δ2"),
            "{before_output}"
        );

        let save = profile_capture_response(
            &shared_settings,
            tmp.path(),
            settings::ProfileSaveTarget::ActiveSource,
        )
        .await;

        assert!(save.accepted, "{save:?}");
        let after = profile_view_response(&shared_settings, tmp.path()).await;
        let after_output = after.output.unwrap_or_default();
        assert!(
            after_output.contains("Runtime drift: clean"),
            "{after_output}"
        );
    }

    #[tokio::test]
    async fn profile_view_response_renders_clean_drift_state() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"thinkingLevel":"high","requestedContextClass":"massive"}"#,
        )
        .unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::High,
            requested_context_class: Some(crate::settings::ContextClass::Massive),
            ..Default::default()
        }));

        let response = profile_view_response(&shared_settings, tmp.path()).await;

        assert!(response.accepted, "{response:?}");
        let output = response.output.unwrap_or_default();
        assert!(output.contains("## Profile"), "{output}");
        assert!(output.contains("Source: project:"), "{output}");
        assert!(output.contains("Runtime drift: clean"), "{output}");
    }

    #[tokio::test]
    async fn profile_view_response_renders_thinking_and_context_drift() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"thinkingLevel":"medium","requestedContextClass":"extended"}"#,
        )
        .unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::High,
            requested_context_class: Some(crate::settings::ContextClass::Massive),
            ..Default::default()
        }));

        let response = profile_view_response(&shared_settings, tmp.path()).await;

        assert!(response.accepted, "{response:?}");
        let output = response.output.unwrap_or_default();
        assert!(
            output.contains("Runtime drift: Δ2 unsaved change(s)"),
            "{output}"
        );
        assert!(
            output.contains("| Thinking | `medium` | `high` | live only |"),
            "{output}"
        );
        assert!(
            output.contains("| Context class | `extended` | `massive` | live only |"),
            "{output}"
        );
        assert!(output.contains("/profile save`"), "{output}");
        assert!(output.contains("/profile save --project`"), "{output}");
        assert!(output.contains("/profile save --user`"), "{output}");
        assert!(output.contains("/profile apply`"), "{output}");
    }

    #[tokio::test]
    async fn profile_capture_response_writes_explicit_project_target() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::High,
            requested_context_class: Some(crate::settings::ContextClass::Massive),
            ..Default::default()
        }));

        let response = profile_capture_response(
            &shared_settings,
            tmp.path(),
            settings::ProfileSaveTarget::Project,
        )
        .await;

        assert!(response.accepted, "{response:?}");
        let profile_path = tmp.path().join(".omegon/profile.json");
        assert!(profile_path.exists());
        let profile = settings::Profile::load(tmp.path());
        assert_eq!(profile.thinking_level.as_deref(), Some("high"));
        assert_eq!(profile.requested_context_class.as_deref(), Some("massive"));
    }

    #[tokio::test]
    async fn profile_capture_response_updates_runtime_profile_source_for_user_target() {
        const ISOLATED_HOME: &str = "OMEGON_TEST_USER_PROFILE_HOME";
        let Ok(home) = std::env::var(ISOLATED_HOME) else {
            // The user-target handler intentionally ignores cwd. Exercise it in
            // a separate process so parallel tests never see a changed HOME.
            let home = tempfile::tempdir().unwrap();
            let operator_profile = dirs::home_dir().unwrap().join(".omegon/profile.json");
            let before = std::fs::read(&operator_profile).ok();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "control_runtime::tests::profile_capture_response_updates_runtime_profile_source_for_user_target",
                    "--nocapture",
                ])
                .env(ISOLATED_HOME, home.path())
                .env("HOME", home.path())
                .env("OMEGON_HOME", home.path().join("omegon-home"))
                .env("XDG_CONFIG_HOME", home.path().join(".config"))
                .output()
                .unwrap();
            assert_eq!(
                std::fs::read(&operator_profile).ok(),
                before,
                "user-target test changed the operator profile"
            );
            assert!(
                output.status.success(),
                "isolated user-target test failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(home.path().join(".omegon/profile.json").is_file());
            return;
        };
        let expected_path = Path::new(&home).join(".omegon/profile.json");
        assert_eq!(dirs::home_dir().unwrap(), Path::new(&home));
        let tmp = tempfile::tempdir().unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let response =
            profile_capture_response(&settings, tmp.path(), settings::ProfileSaveTarget::User)
                .await;

        assert!(response.accepted, "{response:?}");
        let source = settings.lock().unwrap().profile_source.clone();
        assert!(
            matches!(&source, settings::ProfileSource::User(path) if path == &expected_path),
            "{source:?}"
        );
        let saved: settings::Profile =
            serde_json::from_slice(&std::fs::read(expected_path).unwrap()).unwrap();
        assert_eq!(saved.last_used_model.unwrap().model_id, "claude-sonnet-4-6");
        assert!(!tmp.path().join(".omegon/profile.json").exists());
    }

    #[tokio::test]
    async fn profile_capture_response_active_source_updates_existing_project_profile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let profile_path = tmp.path().join(".omegon/profile.json");
        std::fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
        std::fs::write(&profile_path, r#"{"thinkingLevel":"low"}"#).unwrap();
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::High,
            ..Default::default()
        }));

        let response = profile_capture_response(
            &shared_settings,
            tmp.path(),
            settings::ProfileSaveTarget::ActiveSource,
        )
        .await;

        assert!(response.accepted, "{response:?}");
        let profile = settings::Profile::load(tmp.path());
        assert_eq!(profile.thinking_level.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn thinking_view_renders_shared_palette_rows() {
        let shared_settings = std::sync::Arc::new(std::sync::Mutex::new(settings::Settings {
            thinking: crate::settings::ThinkingLevel::High,
            ..Default::default()
        }));

        let response = thinking_view_response(&shared_settings).await;
        let output = response.output.expect("thinking view output");

        assert!(response.accepted);
        assert!(output.starts_with("## Thinking levels"));
        assert!(output.contains("Current thinking level: ◉ high"));
        assert!(output.contains("### Actions"));
        assert!(output.contains("- `/think off` — ○ off · disable explicit reasoning budget"));
        assert!(output.contains(
            "- `/think high` — ◉ high · current · use deeper reasoning for complex work"
        ));
        assert!(output.contains("`/think xhigh`"));
        assert!(output.contains("`/think max`"));
        assert!(output.contains("Use `/think <level>` to apply a level directly."));
    }

    #[test]
    fn skills_menu_projection_renders_action_and_object_rows() {
        let entries = vec![
            crate::skills::SkillEntry {
                name: "rust".into(),
                description: "Conventions for Rust development".into(),
                id: None,
                version: None,
                tags: vec!["lang".into()],
                aliases: vec![],
                triggers: vec![],
                activation: Some("project_detected".into()),
                profile: vec!["coding".into()],
                project_signals: vec!["Cargo.toml".into()],
                posture: None,
                max_turns: None,
                installed: false,
                bundled: true,
                project_local: false,
                source: "bundled".into(),
                editable: false,
                reloadable: false,
                shadows: vec![],
                conflicts: vec![],
                path: String::new(),
            },
            crate::skills::SkillEntry {
                name: "team".into(),
                description: "Project team workflow".into(),
                id: None,
                version: None,
                tags: vec![],
                aliases: vec![],
                triggers: vec![],
                activation: Some("always".into()),
                profile: vec![],
                project_signals: vec![],
                posture: None,
                max_turns: None,
                installed: true,
                bundled: false,
                project_local: true,
                source: "project".into(),
                editable: true,
                reloadable: true,
                shadows: vec!["bundled".into()],
                conflicts: vec!["bundled/rust".into()],
                path: ".omegon/skills/team".into(),
            },
        ];

        let rendered = render_skills_menu(&entries);

        assert!(rendered.starts_with("## Skills"));
        assert!(rendered.contains("### Actions"));
        assert!(rendered.contains("### Installed and available skills"));
        assert!(rendered.contains("Enter: details"));
        assert!(rendered.contains("g: `/skills get rust`"));
        assert!(rendered.contains("### Actions"));
        assert!(rendered.contains("Enter: `/skills reload`"));
        assert!(rendered.contains("Enter: `/skills create --project`"));
        assert!(rendered.contains(
            "- `rust` — Enter: details · i: install/refresh · g: full inspect · bundled · available · project_detected · profile:coding · tags:lang · read-only"
        ));
        assert!(rendered.contains(
            "- `team` — Enter: details · i: install/refresh · g: full inspect · project · local · always · editable · reloadable · shadows:bundled · conflicts:bundled/rust · resolve:merge-recommended"
        ));
        assert!(!rendered.contains("+ = installed"));
        assert!(rendered.contains("`/skills --help` syntax"));
    }

    #[tokio::test]
    async fn permission_trust_add_remove_updates_live_settings_and_profile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "instructions").unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");

        let add = permission_trust_add_response(&settings, tmp.path(), "/tmp/vault").await;
        assert!(add.accepted);
        assert!(
            settings
                .lock()
                .unwrap()
                .trusted_directories
                .contains(&"/tmp/vault".to_string())
        );
        let profile = crate::settings::Profile::load(tmp.path());
        assert!(
            profile
                .permissions
                .trusted_directories
                .contains(&"/tmp/vault".to_string())
        );
        assert!(profile.trusted_directories.is_empty());

        let remove = permission_trust_remove_response(&settings, tmp.path(), "/tmp/vault").await;
        assert!(remove.accepted);
        assert!(
            !settings
                .lock()
                .unwrap()
                .trusted_directories
                .contains(&"/tmp/vault".to_string())
        );
        let profile = crate::settings::Profile::load(tmp.path());
        assert!(
            !profile
                .effective_trusted_directories()
                .contains(&"/tmp/vault".to_string())
        );
    }

    #[tokio::test]
    async fn permissions_view_prefers_canonical_permissions_commands() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "instructions").unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");

        let view = permissions_view_response(&settings, tmp.path()).await;
        let output = view.output.expect("permissions view output");
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        let commands = json["permissions"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        assert!(commands.contains(&"/permissions add <path>"), "{output}");
        assert!(!commands.contains(&"/trust add <path>"), "{output}");
        let aliases = json["permissions"]["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        assert!(aliases.contains(&"/trust add <path>"), "{output}");
        assert!(
            output.contains("profile.permissions.trustedDirectories"),
            "{output}"
        );
    }

    #[tokio::test]
    async fn automation_set_updates_live_settings_and_profile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "instructions").unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");

        let response =
            automation_set_response(&settings, tmp.path(), settings::AutomationLevel::Flow).await;
        assert!(response.accepted);
        assert_eq!(
            settings.lock().unwrap().automation_level,
            settings::AutomationLevel::Flow
        );
        let profile = crate::settings::Profile::load(tmp.path());
        assert_eq!(
            profile.automation.level,
            Some(settings::AutomationLevel::Flow)
        );

        let view = automation_view_response(&settings, tmp.path()).await;
        let output = view.output.unwrap_or_default();
        assert!(output.contains("\"liveLevel\":\"flow\""));
        assert!(output.contains("\"subagents\""));
        assert!(output.contains("\"liveLevel\":\"conservative\""));
        assert!(output.contains("\"maxChildren\":2"));
        assert!(output.contains("loop and scheduled-job envelopes"));
    }

    #[test]
    fn auth_status_includes_auth_file_surface() {
        let status = auth::AuthStatus {
            providers: vec![auth::ProviderInfo {
                name: "openai-codex".into(),
                status: auth::ProviderAuthStatus::Authenticated,
                is_oauth: true,
                details: Some("stored".into()),
            }],
            vault: vec![],
            secrets: vec![],
            mcp: vec![],
        };

        let rendered = format_auth_status(&status);
        assert!(rendered.contains("Auth file"));
        assert!(rendered.contains("Provider Status"));
        assert!(rendered.contains("openai-codex"));
    }

    #[test]
    fn profile_component_commands_map_to_shared_control_requests() {
        for (args, expected) in [
            ("component enable core:codescan", "enable"),
            ("component disable core:codescan", "disable"),
            ("components view", "view"),
        ] {
            let canonical = crate::runtime_commands::canonical_slash_command("profile", args)
                .unwrap_or_else(|| panic!("/profile {args} must be canonical"));
            let request = control_request_from_slash(&canonical)
                .unwrap_or_else(|| panic!("/profile {args} must be shared"));
            assert!(
                matches!(
                    (expected, request),
                    ("enable", ControlRequest::ProfileComponentEnable { .. })
                        | ("disable", ControlRequest::ProfileComponentDisable { .. })
                        | ("view", ControlRequest::ProfileComponentsView)
                ),
                "unexpected shared request for /profile {args}"
            );
        }
    }

    #[tokio::test]
    async fn profile_mutations_persist_to_named_active_project_profile() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let profile_dir = project.path().join(".omegon/profiles");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let active_path = profile_dir.join("compliance.json");
        std::fs::write(&active_path, r#"{"name":"compliance"}"#).unwrap();
        settings::save_project_active_profile_selection(
            project.path(),
            &settings::ActiveProfileSelection {
                id: "compliance".into(),
                scope: Some("project".into()),
            },
        )
        .unwrap();

        let component = profile_component_disable_response(project.path(), "core:codescan").await;
        assert!(component.accepted, "{:?}", component.output);
        let component_output: serde_json::Value =
            serde_json::from_str(component.output.as_deref().unwrap()).unwrap();
        assert_eq!(component_output["selector"], "core:codescan");
        assert_eq!(component_output["requestedEnabled"], false);
        assert_eq!(component_output["restartRequired"], true);
        let canonical_active_path = std::fs::canonicalize(&active_path).unwrap();
        assert_eq!(
            component_output["changedSource"],
            format!("project:{}", canonical_active_path.display())
        );
        assert_eq!(
            component_output["components"][0]["state"],
            "disabled-by-profile"
        );
        assert_eq!(
            component_output["components"][0]["determiningSource"]["profile"],
            "compliance"
        );
        let mqtt = profile_set_mqtt_response(project.path(), Some(true)).await;
        assert!(mqtt.accepted, "{:?}", mqtt.output);
        let extension = profile_extension_deny_response(project.path(), "vox").await;
        assert!(extension.accepted, "{:?}", extension.output);

        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&active_path).unwrap()).unwrap();
        assert_eq!(saved["components"]["core:codescan"]["enabled"], false);
        assert_eq!(saved["integrations"]["mqtt"]["enabled"], true);
        assert_eq!(saved["extensions"]["disabled"], serde_json::json!(["vox"]));
        assert!(
            !project.path().join(".omegon/profile.json").exists(),
            "active named profile must not be shadowed by a legacy singleton"
        );
    }

    #[tokio::test]
    async fn profile_component_mutation_persists_to_named_active_user_profile() {
        let _lock = crate::test_support::env::lock_async().await;
        let home = tempfile::tempdir().unwrap();
        let _home = EnvironmentGuard::set("HOME", home.path());
        let omegon_home = home.path().join(".omegon");
        std::fs::create_dir_all(omegon_home.join("profiles")).unwrap();
        let _omegon_home = EnvironmentGuard::set("OMEGON_HOME", &omegon_home);
        let user_profile = omegon_home.join("profiles/personal.json");
        std::fs::write(&user_profile, r#"{"name":"personal"}"#).unwrap();

        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        std::fs::create_dir_all(project.path().join(".omegon/profiles")).unwrap();
        std::fs::write(project.path().join(".omegon/profiles/project.json"), "{}").unwrap();
        settings::save_project_active_profile_selection(
            project.path(),
            &settings::ActiveProfileSelection {
                id: "personal".into(),
                scope: Some("user".into()),
            },
        )
        .unwrap();

        let response = profile_component_disable_response(project.path(), "core:codescan").await;
        assert!(response.accepted, "{:?}", response.output);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&user_profile).unwrap()).unwrap();
        assert_eq!(saved["components"]["core:codescan"]["enabled"], false);
        assert!(!project.path().join(".omegon/profile.json").exists());
    }

    #[tokio::test]
    async fn profile_component_mutation_validates_selector_before_writing() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        let response = profile_component_disable_response(project.path(), "core:codesan").await;
        assert!(!response.accepted);
        assert!(
            response
                .output
                .as_deref()
                .is_some_and(|output| output.contains("unknown component `core:codesan`"))
        );
        assert!(!project.path().join(".omegon/profile.json").exists());
    }

    #[tokio::test]
    async fn profile_component_mutation_rejects_ambiguous_built_in_target() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        std::fs::create_dir_all(project.path().join(".omegon/profiles")).unwrap();
        std::fs::write(project.path().join(".omegon/profiles/editable.json"), "{}").unwrap();
        settings::save_project_active_profile_selection(
            project.path(),
            &settings::ActiveProfileSelection {
                id: "built-in-default".into(),
                scope: Some("built-in".into()),
            },
        )
        .unwrap();
        let response = profile_component_disable_response(project.path(), "core:codescan").await;
        assert!(!response.accepted);
        assert!(
            response
                .output
                .as_deref()
                .is_some_and(|output| output.contains("explicit") && output.contains("target")),
            "{:?}",
            response.output
        );
        assert!(!project.path().join(".omegon/profile.json").exists());
    }
}

#[cfg(test)]
mod context_compaction_tests {
    use super::*;
    use crate::bridge::{LlmEvent, MockBridge};

    fn test_runtime_state_with_evictable_context() -> InteractiveAgentState {
        let mut conversation = crate::conversation::ConversationState::new();
        conversation.push_user("old context".into());
        conversation.intent.stats.turns = 99;
        InteractiveAgentState {
            bus: crate::bus::EventBus::new(),
            context_service: Arc::new(crate::features::context::ContextProvider::new(
                crate::features::context::SharedContextMetrics::new(),
                crate::features::context::new_shared_command_tx(),
            )),
            context_manager: crate::context::ContextManager::new(String::new(), Vec::new()),
            conversation,
            inference_runtime: crate::inference_runtime::InferenceRuntimeState::new(
                std::path::Path::new("."),
            ),
            work_snapshot: None,
            behavior_policy: None,
            memory_binding: Default::default(),
            context_compaction:
                crate::context_compaction_service::ContextCompactionBinding::direct_for_test(),
        }
    }

    fn test_agent() -> InteractiveAgentHost {
        use crate::workspace::types::{
            Mutability, WorkspaceBackendKind, WorkspaceBindings, WorkspaceKind, WorkspaceLease,
            WorkspaceRole,
        };
        let cwd = tempfile::tempdir().unwrap().keep();
        let secrets =
            std::sync::Arc::new(omegon_secrets::SecretsManager::new(&cwd.join("secrets")).unwrap());
        let session_id = crate::session::allocate_session_id();
        InteractiveAgentHost {
            session_view_binding: crate::session_consumers::SessionViewBinding::new(
                cwd.join(format!("{session_id}.json")),
                session_id.clone(),
            ),
            session_id,
            instance_id: "test-instance".into(),
            runtime_ownership: None,
            context_metrics: crate::features::context::SharedContextMetrics::new(),
            cwd: cwd.clone(),
            secrets,
            web_auth_state: crate::web::WebAuthState::ephemeral_generated("test-token".into()),
            dashboard_handles: Default::default(),
            resume_info: None,
            workspace_state: crate::setup::WorkspaceStartupState {
                lease: WorkspaceLease {
                    project_id: "test-project".into(),
                    workspace_id: "test-workspace".into(),
                    label: "test".into(),
                    path: cwd.display().to_string(),
                    backend_kind: WorkspaceBackendKind::LocalDir,
                    vcs_ref: None,
                    bindings: WorkspaceBindings::default(),
                    branch: "main".into(),
                    role: WorkspaceRole::Primary,
                    workspace_kind: WorkspaceKind::Code,
                    mutability: Mutability::Mutable,
                    owner_session_id: Some("test-session".into()),
                    owner_agent_id: Some("test-agent".into()),
                    created_at: "2026-05-14T00:00:00Z".into(),
                    last_heartbeat: "2026-05-14T00:00:00Z".into(),
                    archived: false,
                    archived_at: None,
                    archive_reason: None,
                    parent_workspace_id: None,
                    source: "test".into(),
                },
                admission: crate::workspace::types::AdmissionOutcome::GrantedMutable,
            },
            runtime_generation: 1,
            git_binding: Default::default(),
            dynamic_contribution_control: Default::default(),
        }
    }

    #[tokio::test]
    async fn model_list_projects_admission_labels() {
        let response = model_list_response().await;
        assert!(response.accepted);
        let output = response.output.expect("model list output");
        assert!(output.starts_with("Available Models\n"));
        assert!(
            output.lines().any(|line| line.contains("admission=")),
            "model list must disclose route admission: {output}"
        );
    }

    #[tokio::test]
    async fn zen_dispatcher_preserves_anonymous_bridge_readiness() {
        let _catalog = crate::providers::zen::test_catalog(&["big-pickle"]);
        let mut agent = test_agent();
        std::fs::create_dir_all(agent.cwd.join(".omegon")).unwrap();
        std::fs::write(agent.cwd.join(".omegon/profile.json"), "{}").unwrap();
        let shared = settings::shared("opencode-zen:big-pickle");
        let bridge = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockBridge { events: vec![] }) as Box<dyn LlmBridge>,
        ));
        let (events, _) = broadcast::channel(8);
        for requested in [Some("opencode-zen:big-pickle"), None] {
            let response = switch_dispatcher_response(
                &mut agent,
                &shared,
                &bridge,
                "zen-dispatch",
                "B",
                requested,
                &events,
            )
            .await;
            assert!(response.accepted, "{:?}", response.output);
            let current = shared.lock().unwrap();
            assert!(current.provider_connected);
            assert_eq!(current.model, "opencode-zen:big-pickle");
        }
        std::fs::remove_dir_all(&agent.cwd).unwrap();
    }

    #[tokio::test]
    async fn zen_profile_apply_preserves_authoritative_serving_route() {
        let _catalog = crate::providers::zen::test_catalog(&["big-pickle"]);
        let mut agent = test_agent();
        std::fs::create_dir_all(agent.cwd.join(".omegon")).unwrap();
        std::fs::write(agent.cwd.join(".omegon/profile.json"),
            r#"{"lastUsedModel":{"provider":"opencode-zen","modelId":"big-pickle"},"modelIntent":{"exactModelOverride":"opencode-zen:big-pickle"}}"#).unwrap();
        let shared = settings::shared("opencode-zen:big-pickle");
        let controller = Arc::new(crate::route::RouteController::new(
            crate::route::ProviderRoute::Serving {
                model: crate::route::ModelRouteSpec::parse("opencode-zen:big-pickle"),
            },
            Box::new(MockBridge { events: vec![] }),
            None,
        ));
        let bridge = controller.bridge();
        let mut state = test_runtime_state_with_evictable_context();
        let (events, _) = broadcast::channel(8);
        let response = profile_apply_response(
            &mut agent,
            &mut state,
            &shared,
            &bridge,
            Some(controller.clone()),
            &events,
        )
        .await;
        assert!(response.accepted, "{:?}", response.output);
        assert_eq!(
            controller.snapshot().await.serving_model(),
            Some("opencode-zen:big-pickle")
        );
        assert!(shared.lock().unwrap().provider_connected);
        std::fs::remove_dir_all(&agent.cwd).unwrap();
    }

    #[tokio::test]
    async fn context_request_uses_typed_read_only_service() {
        let mut state = test_runtime_state_with_evictable_context();

        let response = context_request_response(&mut state, "session_state", "current work").await;

        assert!(response.accepted);
        let output = response.output.expect("context response");
        assert!(output.contains("Retrieved 1 supported context pack"));
        assert!(output.contains("Session State"));
        assert!(
            !state
                .bus
                .has_tool(crate::tool_registry::context::REQUEST_CONTEXT)
        );
    }

    #[tokio::test]
    async fn manual_context_compact_emits_no_payload_diagnostic() {
        let mut state = InteractiveAgentState {
            bus: crate::bus::EventBus::new(),
            context_service: Arc::new(crate::features::context::ContextProvider::new(
                crate::features::context::SharedContextMetrics::new(),
                crate::features::context::new_shared_command_tx(),
            )),
            context_manager: crate::context::ContextManager::new(String::new(), Vec::new()),
            conversation: crate::conversation::ConversationState::new(),
            inference_runtime: crate::inference_runtime::InferenceRuntimeState::new(
                std::path::Path::new("."),
            ),
            work_snapshot: None,
            behavior_policy: None,
            memory_binding: Default::default(),
            context_compaction:
                crate::context_compaction_service::ContextCompactionBinding::direct_for_test(),
        };
        let mut agent = test_agent();
        let settings = crate::settings::shared("test:model");
        let bridge = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockBridge { events: vec![] }) as Box<dyn LlmBridge>,
        ));
        let (events_tx, mut events_rx) = broadcast::channel(8);

        let response = context_compact_response(
            &mut state,
            &mut agent,
            &settings,
            &bridge,
            &events_tx,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;

        assert!(response.accepted);
        let event = events_rx.recv().await.unwrap();
        match event {
            AgentEvent::ContextCompaction(event) => {
                assert_eq!(
                    event.trigger,
                    omegon_traits::ContextCompactionTrigger::Manual
                );
                assert_eq!(
                    event.status,
                    omegon_traits::ContextCompactionStatus::NoPayload
                );
                assert_eq!(event.evicted_messages, Some(0));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn token_retention_manual_handler_applies_selected_recent_boundary() {
        let mut state = test_runtime_state_with_evictable_context();
        state.conversation = crate::conversation::ConversationState::new();
        state
            .conversation
            .push_user("older large turn ".repeat(8_000));
        state.conversation.intent.stats.turns = 1;
        state.conversation.push_user("current task".into());
        let mut agent = test_agent();
        let settings = crate::settings::shared("test:model");
        settings.lock().unwrap().context_window = 32_000;
        let bridge = Arc::new(tokio::sync::RwLock::new(Box::new(MockBridge {
            events: vec![
                LlmEvent::TextDelta {
                    delta: "Prior work summarized".into(),
                },
                LlmEvent::Done {
                    message: serde_json::json!({}),
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                },
            ],
        }) as Box<dyn LlmBridge>));
        let (events, mut receive) = broadcast::channel(8);
        let response = context_compact_response(
            &mut state,
            &mut agent,
            &settings,
            &bridge,
            &events,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;
        assert!(response.accepted, "{response:?}");
        assert_eq!(state.conversation.message_count(), 1);
        assert_eq!(state.conversation.last_user_prompt(), "current task");
        assert_eq!(state.conversation.intent.stats.compactions, 1);
        let AgentEvent::ContextCompaction(started) = receive.recv().await.unwrap() else {
            panic!("expected compaction start");
        };
        assert_eq!(started.evicted_messages, Some(1));
    }

    #[tokio::test]
    async fn manual_context_compact_emits_started_and_succeeded_diagnostics() {
        let mut state = test_runtime_state_with_evictable_context();
        let mut agent = test_agent();
        let settings = crate::settings::shared("test:model");
        let bridge = Arc::new(tokio::sync::RwLock::new(Box::new(MockBridge {
            events: vec![
                LlmEvent::TextDelta {
                    delta: "summary".into(),
                },
                LlmEvent::Done {
                    message: serde_json::json!({}),
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                },
            ],
        }) as Box<dyn LlmBridge>));
        let (events_tx, mut events_rx) = broadcast::channel(8);

        let response = context_compact_response(
            &mut state,
            &mut agent,
            &settings,
            &bridge,
            &events_tx,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;

        assert!(response.accepted, "{response:?}");
        let first = events_rx.recv().await.unwrap();
        let second = events_rx.recv().await.unwrap();
        match first {
            AgentEvent::ContextCompaction(event) => {
                assert_eq!(
                    event.status,
                    omegon_traits::ContextCompactionStatus::Started
                );
                assert_eq!(event.evicted_messages, Some(1));
            }
            other => panic!("unexpected first event: {other:?}"),
        }
        match second {
            AgentEvent::ContextCompaction(event) => {
                assert_eq!(
                    event.status,
                    omegon_traits::ContextCompactionStatus::Succeeded
                );
                assert_eq!(event.summary_chars, Some(7));
                assert!(event.after_tokens.is_some());
            }
            other => panic!("unexpected second event: {other:?}"),
        }
    }
}

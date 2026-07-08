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
}

#[derive(Debug)]
pub enum ControlRequest {
    ModelView,
    ModelList,
    SetModel {
        requested_model: String,
    },
    SetModelIntent {
        grade: String,
    },
    SetModelProvider {
        provider: String,
    },
    SetModelPolicy {
        policy: String,
    },
    ClearModelOverride,
    SwitchDispatcher {
        request_id: String,
        profile: String,
        model: Option<String>,
    },
    ThinkingView,
    SetThinking {
        level: crate::settings::ThinkingLevel,
    },
    ProfileCapture {
        target: settings::ProfileSaveTarget,
    },
    ProfileApply,
    ProfileSetMqtt {
        enabled: Option<bool>,
    },
    ProfileExtensionAllow {
        name: String,
    },
    ProfileExtensionDeny {
        name: String,
    },
    ProfileExtensionClear,
    ProfileSetPersona {
        name: Option<String>,
    },
    ProfileSetTone {
        name: Option<String>,
    },
    AutomationView,
    AutomationSet {
        level: settings::AutomationLevel,
    },
    PermissionsView,
    PermissionTrustAdd {
        path: String,
    },
    PermissionTrustRemove {
        path: String,
    },
    StatusView,
    RuntimeSubstrateRefresh,
    WorkspaceStatusView,
    WorkspaceListView,
    WorkspaceNew {
        label: String,
    },
    WorkspaceDestroy {
        target: String,
    },
    WorkspaceAdopt,
    WorkspaceRelease,
    WorkspaceArchive,
    WorkspacePrune,
    WorkspaceBindMilestone {
        milestone_id: String,
    },
    WorkspaceBindNode {
        design_node_id: String,
    },
    WorkspaceBindClear,
    WorkspaceRoleView,
    WorkspaceRoleSet {
        role: crate::workspace::types::WorkspaceRole,
    },
    WorkspaceRoleClear,
    WorkspaceKindView,
    WorkspaceKindSet {
        kind: crate::workspace::types::WorkspaceKind,
    },
    WorkspaceKindClear,
    SessionStatsView,
    TreeView {
        args: String,
    },
    NoteAdd {
        text: String,
    },
    NotesView,
    NotesClear,
    CheckinView,
    ContextStatus,
    ContextCompact,
    ContextClear,
    ContextRequest {
        kind: String,
        query: String,
    },
    ContextRequestJson {
        raw: String,
    },
    SetContextClass {
        class: crate::settings::ContextClass,
    },
    SetRuntimeMode {
        slim: bool,
    },
    NewSession,
    ListSessions,
    ResumeSession {
        id: String,
    },
    AuthStatus,
    AuthUnlock,
    AuthLogin {
        provider: String,
    },
    AuthLogout {
        provider: String,
    },
    SkillsView,
    SkillsHelp,
    SkillsInstall {
        name: Option<String>,
    },
    SkillGet {
        name: String,
    },
    SkillDelete {
        name: String,
    },
    ExtensionView,
    ExtensionGet {
        name: String,
    },
    ExtensionInstall {
        uri: String,
    },
    ExtensionRemove {
        name: String,
    },
    ExtensionUpdate {
        name: Option<String>,
    },
    ExtensionEnable {
        name: String,
    },
    ExtensionDisable {
        name: String,
    },
    ExtensionSearch {
        query: Option<String>,
    },
    ArmoryBrowse {
        query: Option<String>,
    },
    ArmoryInstall {
        target: String,
    },
    CatalogView,
    CatalogInstall,
    CatalogRemove {
        id: String,
    },
    PluginView,
    PluginInstall {
        uri: String,
    },
    PluginRemove {
        name: String,
    },
    PluginUpdate {
        name: Option<String>,
    },
    SecretsView,
    SecretsSet {
        name: String,
        value: String,
    },
    SecretsGet {
        name: String,
    },
    SecretsDelete {
        name: String,
    },
    VariablesView,
    VariablesSet {
        name: String,
        value: String,
    },
    VariablesGet {
        name: String,
    },
    VariablesDelete {
        name: String,
    },
    VaultStatus,
    VaultUnseal,
    VaultLogin,
    VaultConfigure,
    VaultInitPolicy,
    CleaveStatus,
    Smoke(crate::smoke_surface::SmokeCommand),
    CleaveCancelChild {
        label: String,
    },
    DelegateStatus,
    // ── Auspex fleet control ────────────────────────────────────────
    SetMaxTurns {
        max_turns: u32,
    },
    ProfileView,
    ProfileExport,
    PersonaList,
    PersonaSwitch {
        name: String,
    },
}

pub fn control_request_from_slash(
    command: &crate::tui::CanonicalSlashCommand,
) -> Option<ControlRequest> {
    Some(match command {
        crate::tui::CanonicalSlashCommand::ModelView => ControlRequest::ModelView,
        crate::tui::CanonicalSlashCommand::ModelList => ControlRequest::ModelList,
        crate::tui::CanonicalSlashCommand::ModelUnpin => ControlRequest::ClearModelOverride,
        crate::tui::CanonicalSlashCommand::SetModel(requested_model) => ControlRequest::SetModel {
            requested_model: requested_model.clone(),
        },
        crate::tui::CanonicalSlashCommand::SetModelGrade(grade) => ControlRequest::SetModelIntent {
            grade: grade.clone(),
        },
        crate::tui::CanonicalSlashCommand::SetModelProvider(provider) => {
            ControlRequest::SetModelProvider {
                provider: provider.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::SetModelPolicy(policy) => {
            ControlRequest::SetModelPolicy {
                policy: policy.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::ThinkingView => ControlRequest::ThinkingView,
        crate::tui::CanonicalSlashCommand::SetThinking(level) => {
            ControlRequest::SetThinking { level: *level }
        }
        crate::tui::CanonicalSlashCommand::ProfileView => ControlRequest::ProfileView,
        crate::tui::CanonicalSlashCommand::ProfileExport => ControlRequest::ProfileExport,
        crate::tui::CanonicalSlashCommand::ProfileCapture(target) => {
            ControlRequest::ProfileCapture { target: *target }
        }
        crate::tui::CanonicalSlashCommand::ProfileApply => ControlRequest::ProfileApply,
        crate::tui::CanonicalSlashCommand::ProfileSetMqtt(enabled) => {
            ControlRequest::ProfileSetMqtt { enabled: *enabled }
        }
        crate::tui::CanonicalSlashCommand::ProfileExtensionAllow(name) => {
            ControlRequest::ProfileExtensionAllow { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ProfileExtensionDeny(name) => {
            ControlRequest::ProfileExtensionDeny { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ProfileExtensionClear => {
            ControlRequest::ProfileExtensionClear
        }
        crate::tui::CanonicalSlashCommand::ProfileSetPersona(name) => {
            ControlRequest::ProfileSetPersona { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ProfileSetTone(name) => {
            ControlRequest::ProfileSetTone { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::AutomationView => ControlRequest::AutomationView,
        crate::tui::CanonicalSlashCommand::AutomationSet(level) => {
            ControlRequest::AutomationSet { level: *level }
        }
        crate::tui::CanonicalSlashCommand::PermissionsView => ControlRequest::PermissionsView,
        crate::tui::CanonicalSlashCommand::PermissionTrustAdd(path) => {
            ControlRequest::PermissionTrustAdd { path: path.clone() }
        }
        crate::tui::CanonicalSlashCommand::PermissionTrustRemove(path) => {
            ControlRequest::PermissionTrustRemove { path: path.clone() }
        }
        crate::tui::CanonicalSlashCommand::StatusView => ControlRequest::StatusView,
        crate::tui::CanonicalSlashCommand::RuntimeSubstrateRefresh => return None,
        crate::tui::CanonicalSlashCommand::WorkspaceStatusView => {
            ControlRequest::WorkspaceStatusView
        }
        crate::tui::CanonicalSlashCommand::WorkspaceListView => ControlRequest::WorkspaceListView,
        crate::tui::CanonicalSlashCommand::WorkspaceNew(label) => ControlRequest::WorkspaceNew {
            label: label.clone(),
        },
        crate::tui::CanonicalSlashCommand::WorkspaceDestroy(target) => {
            ControlRequest::WorkspaceDestroy {
                target: target.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::WorkspaceAdopt => ControlRequest::WorkspaceAdopt,
        crate::tui::CanonicalSlashCommand::WorkspaceRelease => ControlRequest::WorkspaceRelease,
        crate::tui::CanonicalSlashCommand::WorkspaceArchive => ControlRequest::WorkspaceArchive,
        crate::tui::CanonicalSlashCommand::WorkspacePrune => ControlRequest::WorkspacePrune,
        crate::tui::CanonicalSlashCommand::WorkspaceBindMilestone(milestone_id) => {
            ControlRequest::WorkspaceBindMilestone {
                milestone_id: milestone_id.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::WorkspaceBindNode(design_node_id) => {
            ControlRequest::WorkspaceBindNode {
                design_node_id: design_node_id.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::WorkspaceBindClear => ControlRequest::WorkspaceBindClear,
        crate::tui::CanonicalSlashCommand::WorkspaceRoleView => ControlRequest::WorkspaceRoleView,
        crate::tui::CanonicalSlashCommand::WorkspaceRoleSet(role) => {
            ControlRequest::WorkspaceRoleSet { role: *role }
        }
        crate::tui::CanonicalSlashCommand::WorkspaceRoleClear => ControlRequest::WorkspaceRoleClear,
        crate::tui::CanonicalSlashCommand::WorkspaceKindView => ControlRequest::WorkspaceKindView,
        crate::tui::CanonicalSlashCommand::WorkspaceKindSet(kind) => {
            ControlRequest::WorkspaceKindSet { kind: *kind }
        }
        crate::tui::CanonicalSlashCommand::WorkspaceKindClear => ControlRequest::WorkspaceKindClear,
        crate::tui::CanonicalSlashCommand::SessionStatsView => ControlRequest::SessionStatsView,
        crate::tui::CanonicalSlashCommand::TreeView { args } => {
            ControlRequest::TreeView { args: args.clone() }
        }
        crate::tui::CanonicalSlashCommand::NoteAdd { text } => {
            ControlRequest::NoteAdd { text: text.clone() }
        }
        crate::tui::CanonicalSlashCommand::NotesView => ControlRequest::NotesView,
        crate::tui::CanonicalSlashCommand::NotesClear => ControlRequest::NotesClear,
        crate::tui::CanonicalSlashCommand::CheckinView => ControlRequest::CheckinView,
        crate::tui::CanonicalSlashCommand::ContextStatus => ControlRequest::ContextStatus,
        crate::tui::CanonicalSlashCommand::ContextCompact => ControlRequest::ContextCompact,
        crate::tui::CanonicalSlashCommand::ContextClear => ControlRequest::ContextClear,
        crate::tui::CanonicalSlashCommand::ContextRequest { kind, query } => {
            ControlRequest::ContextRequest {
                kind: kind.clone(),
                query: query.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::ContextRequestJson(raw) => {
            ControlRequest::ContextRequestJson { raw: raw.clone() }
        }
        crate::tui::CanonicalSlashCommand::SetContextClass(class) => {
            ControlRequest::SetContextClass { class: *class }
        }
        crate::tui::CanonicalSlashCommand::NewSession => ControlRequest::NewSession,
        crate::tui::CanonicalSlashCommand::ListSessions => ControlRequest::ListSessions,
        crate::tui::CanonicalSlashCommand::ResumeSession(id) => {
            ControlRequest::ResumeSession { id: id.clone() }
        }
        crate::tui::CanonicalSlashCommand::AuthStatus => ControlRequest::AuthStatus,
        crate::tui::CanonicalSlashCommand::AuthLogin(provider) => ControlRequest::AuthLogin {
            provider: provider.clone(),
        },
        crate::tui::CanonicalSlashCommand::AuthLogout(provider) => ControlRequest::AuthLogout {
            provider: provider.clone(),
        },
        crate::tui::CanonicalSlashCommand::SkillsView => ControlRequest::SkillsView,
        crate::tui::CanonicalSlashCommand::SkillsHelp => ControlRequest::SkillsHelp,
        crate::tui::CanonicalSlashCommand::SkillsReload => return None,
        crate::tui::CanonicalSlashCommand::SkillsInstall(name) => {
            ControlRequest::SkillsInstall { name: name.clone() }
        }
        // SkillCreate/SkillImport are handled directly in the TUI (queues a prompt) —
        // they never reach control_runtime. Return None to signal this.
        crate::tui::CanonicalSlashCommand::SkillCreate(_)
        | crate::tui::CanonicalSlashCommand::SkillImport { .. } => return None,
        crate::tui::CanonicalSlashCommand::SkillGet(name) => {
            ControlRequest::SkillGet { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::SkillDelete(name) => {
            ControlRequest::SkillDelete { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::PlanView
        | crate::tui::CanonicalSlashCommand::PlanList
        | crate::tui::CanonicalSlashCommand::PlanShow(_)
        | crate::tui::CanonicalSlashCommand::PlanSwitch(_)
        | crate::tui::CanonicalSlashCommand::PlanResume(_)
        | crate::tui::CanonicalSlashCommand::PlanBackground(_)
        | crate::tui::CanonicalSlashCommand::PlanDetach(_)
        | crate::tui::CanonicalSlashCommand::PlanPromote(_)
        | crate::tui::CanonicalSlashCommand::PlanBind(_)
        | crate::tui::CanonicalSlashCommand::PlanLedger(_)
        | crate::tui::CanonicalSlashCommand::PlanSet(_)
        | crate::tui::CanonicalSlashCommand::PlanApprove
        | crate::tui::CanonicalSlashCommand::PlanExecute
        | crate::tui::CanonicalSlashCommand::PlanAdvance
        | crate::tui::CanonicalSlashCommand::PlanSkip
        | crate::tui::CanonicalSlashCommand::PlanClear => return None,
        crate::tui::CanonicalSlashCommand::ExtensionView => ControlRequest::ExtensionView,
        crate::tui::CanonicalSlashCommand::ExtensionGet(name) => {
            ControlRequest::ExtensionGet { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ExtensionInstall(uri) => {
            ControlRequest::ExtensionInstall { uri: uri.clone() }
        }
        crate::tui::CanonicalSlashCommand::ExtensionRemove(name) => {
            ControlRequest::ExtensionRemove { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ExtensionUpdate(name) => {
            ControlRequest::ExtensionUpdate { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ExtensionEnable(name) => {
            ControlRequest::ExtensionEnable { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ExtensionDisable(name) => {
            ControlRequest::ExtensionDisable { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::ExtensionSearch(query) => {
            ControlRequest::ExtensionSearch {
                query: query.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::ArmoryBrowse(query) => ControlRequest::ArmoryBrowse {
            query: query.clone(),
        },
        crate::tui::CanonicalSlashCommand::ArmoryInstall(target) => ControlRequest::ArmoryInstall {
            target: target.clone(),
        },
        crate::tui::CanonicalSlashCommand::PersonaList => ControlRequest::PersonaList,
        crate::tui::CanonicalSlashCommand::CatalogView => ControlRequest::CatalogView,
        crate::tui::CanonicalSlashCommand::CatalogInstall => ControlRequest::CatalogInstall,
        crate::tui::CanonicalSlashCommand::CatalogRemove(id) => {
            ControlRequest::CatalogRemove { id: id.clone() }
        }
        crate::tui::CanonicalSlashCommand::PluginView => ControlRequest::PluginView,
        crate::tui::CanonicalSlashCommand::PluginInstall(uri) => {
            ControlRequest::PluginInstall { uri: uri.clone() }
        }
        crate::tui::CanonicalSlashCommand::PluginRemove(name) => {
            ControlRequest::PluginRemove { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::PluginUpdate(name) => {
            ControlRequest::PluginUpdate { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::SecretsView => ControlRequest::SecretsView,
        crate::tui::CanonicalSlashCommand::SecretsSet { name, value } => {
            ControlRequest::SecretsSet {
                name: name.clone(),
                value: value.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::SecretsGet(name) => {
            ControlRequest::SecretsGet { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::SecretsDelete(name) => {
            ControlRequest::SecretsDelete { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::VariablesView => ControlRequest::VariablesView,
        crate::tui::CanonicalSlashCommand::VariablesSet { name, value } => {
            ControlRequest::VariablesSet {
                name: name.clone(),
                value: value.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::VariablesGet(name) => {
            ControlRequest::VariablesGet { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::VariablesDelete(name) => {
            ControlRequest::VariablesDelete { name: name.clone() }
        }
        crate::tui::CanonicalSlashCommand::VaultStatus => ControlRequest::VaultStatus,
        crate::tui::CanonicalSlashCommand::VaultConfigure => ControlRequest::VaultConfigure,
        crate::tui::CanonicalSlashCommand::VaultInitPolicy => ControlRequest::VaultInitPolicy,
        crate::tui::CanonicalSlashCommand::CleaveStatus => ControlRequest::CleaveStatus,
        crate::tui::CanonicalSlashCommand::Smoke(command) => ControlRequest::Smoke(*command),
        crate::tui::CanonicalSlashCommand::CleaveCancelChild(label) => {
            ControlRequest::CleaveCancelChild {
                label: label.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::DelegateStatus => ControlRequest::DelegateStatus,
    })
}

/// Shared handler for stateless control requests that need at most
/// shared_settings, secrets, cwd, and dashboard handles — no TUI or
/// runtime state. Called by both `execute_control` and `execute_daemon_control`.
async fn try_stateless_control(
    request: &ControlRequest,
    shared_settings: &settings::SharedSettings,
    secrets: &Arc<omegon_secrets::SecretsManager>,
    cwd: &Path,
    handles: &crate::tui::dashboard::DashboardHandles,
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
            profile_capture_response(shared_settings, cwd, *target).await
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
        ControlRequest::PersonaList => persona_list_response(handles).await,
        ControlRequest::PersonaSwitch { name } => persona_switch_response(name).await,
        _ => return None,
    };
    Some(resp)
}

pub async fn execute_control(
    ctx: &mut ControlContext<'_>,
    request: ControlRequest,
) -> SlashCommandResponse {
    // Try stateless handlers first (shared with daemon mode).
    if let Some(resp) = try_stateless_control(
        &request,
        ctx.shared_settings,
        &ctx.agent.secrets,
        &ctx.agent.cwd,
        &ctx.agent.dashboard_handles,
    )
    .await
    {
        return resp;
    }

    match request {
        ControlRequest::SetModel { requested_model } => {
            set_model_response(
                ctx.agent,
                ctx.shared_settings,
                ctx.bridge,
                ctx.route_controller.clone(),
                &requested_model,
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
                ctx.events_tx,
            )
            .await
        }
        ControlRequest::StatusView => {
            status_view_response(ctx.runtime_state, ctx.agent, ctx.shared_settings).await
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
            crate::workspace::control::workspace_new_response(&workspace_ctx, &label)
        }
        ControlRequest::WorkspaceDestroy { target } => {
            let workspace_ctx = workspace_control_context(ctx.agent);
            crate::workspace::control::workspace_destroy_response(&workspace_ctx, &target)
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
        ControlRequest::TreeView { args } => tree_view_response(ctx.runtime_state, &args).await,
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
            )
            .await
        }
        ControlRequest::ContextClear => {
            context_clear_response(ctx.runtime_state, ctx.agent, ctx.cli, ctx.events_tx).await
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
        ControlRequest::NewSession => {
            new_session_response(ctx.runtime_state, ctx.agent, ctx.cli, ctx.events_tx).await
        }
        ControlRequest::ListSessions => list_sessions_response(ctx.agent).await,
        ControlRequest::ResumeSession { id } => {
            resume_session_response(ctx.runtime_state, ctx.agent, ctx.cli, ctx.events_tx, &id).await
        }
        ControlRequest::AuthLogin { provider } => {
            auth_login_response(
                ctx.shared_settings,
                ctx.bridge,
                ctx.login_prompt_tx,
                ctx.events_tx,
                ctx.cli,
                &ctx.agent.cwd,
                &provider,
            )
            .await
        }
        ControlRequest::VaultStatus => vault_status_response(ctx.agent).await,
        ControlRequest::CleaveStatus => cleave_status_response(ctx.runtime_state).await,
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
            cleave_cancel_child_response(ctx.runtime_state, &label).await
        }
        ControlRequest::DelegateStatus => delegate_status_response(ctx.runtime_state).await,
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
    handles: &crate::tui::dashboard::DashboardHandles,
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
            | ControlRequest::SetMaxTurns { .. }
            | ControlRequest::ProfileApply
            | ControlRequest::ProfileCapture { .. }
            | ControlRequest::ProfileSetMqtt { .. }
            | ControlRequest::ProfileExtensionAllow { .. }
            | ControlRequest::ProfileExtensionDeny { .. }
            | ControlRequest::ProfileExtensionClear
            | ControlRequest::ProfileSetPersona { .. }
            | ControlRequest::ProfileSetTone { .. }
    );
    // Try stateless handlers first (shared with TUI mode).
    let resp = if let Some(resp) =
        try_stateless_control(&request, shared_settings, secrets, cwd, handles).await
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
            ControlRequest::ProfileApply => {
                profile_apply_daemon_response(shared_settings, cwd).await
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
        && let Some(ref harness_handle) = handles.harness
        && let Ok(mut status) = harness_handle.lock()
    {
        // Refresh settings-derived fields in the live harness status.
        if let Ok(s) = shared_settings.lock() {
            status.context_class = s.effective_requested_class().label().to_string();
            status.thinking_level = s.thinking.as_str().to_string();
        }
        if let Ok(json) = serde_json::to_value(&*status) {
            let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json: json });
        }
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
    let connected = if crate::auth::provider_connected_for_model(&s.model) {
        "Yes"
    } else {
        "No"
    };
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
    let catalog = crate::tui::model_catalog::ModelCatalog::discover();
    let grouped = catalog.by_conceptual_model();
    let mut output = String::from("Available Models\n");
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
                "  {} ({}) — provider={}, producer={}, execution={}, {}\n",
                model.name, model.id, model.provider, producer, execution_class, availability
            ));
        }
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

async fn set_model_intent_control_response(
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

async fn set_model_provider_control_response(
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

async fn set_model_policy_control_response(
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
    let effective_model = if crate::providers::explicit_provider_id(&effective_model).as_deref()
        == Some("github-copilot")
    {
        effective_model
    } else {
        providers::resolve_execution_model_spec(&effective_model)
            .await
            .unwrap_or(effective_model)
    };
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
        let new_bridge = providers::auto_detect_bridge(&effective_model).await;
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
            s.provider_connected = crate::auth::provider_connected_for_model(&effective_model);
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
    let tier_model = reg
        .grade_model(&normalized_profile, &current_provider)
        .unwrap_or(&current_model)
        .to_string();
    let requested_model_spec = requested_model.map(ToOwned::to_owned).unwrap_or_else(|| {
        if current_provider.is_empty() || current_provider == "anthropic" {
            format!("anthropic:{tier_model}")
        } else {
            format!("{current_provider}:{tier_model}")
        }
    });
    let effective_model = providers::resolve_execution_model_spec(&requested_model_spec)
        .await
        .unwrap_or_else(|| requested_model_spec.clone());

    if let Ok(mut s) = shared_settings.lock() {
        if !effective_model.is_empty() {
            s.set_model(&effective_model);
        }
        let mut profile_doc = settings::Profile::load(&agent.cwd);
        profile_doc.capture_from(&s);
        let _ = profile_doc.save(&agent.cwd);
    }

    if !effective_model.is_empty() {
        if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await {
            let mut guard = bridge.write().await;
            *guard = new_bridge;
        }
        if let Ok(mut s) = shared_settings.lock() {
            s.provider_connected = crate::auth::provider_connected_for_model(&effective_model);
        }
    }

    let mut status = crate::status::HarnessStatus::assemble();
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
    for level in [
        crate::settings::ThinkingLevel::Off,
        crate::settings::ThinkingLevel::Minimal,
        crate::settings::ThinkingLevel::Low,
        crate::settings::ThinkingLevel::Medium,
        crate::settings::ThinkingLevel::High,
    ] {
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

    let mut status = crate::status::HarnessStatus::assemble();
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

pub async fn status_view_response(
    _runtime_state: &InteractiveAgentState,
    agent: &InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
) -> SlashCommandResponse {
    let mut status = crate::status::HarnessStatus::assemble();
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
    let panel = format!(
        "{}\nRuntime\n  Generation:   {}\n  Session:      {}\n  Instance:     {}\nAutomation\n  Level:        {} ({})",
        crate::tui::bootstrap::render_bootstrap(&status, false),
        agent.runtime_generation,
        agent.session_id,
        agent.instance_id,
        settings.automation_level.as_str(),
        settings.automation_level.summary()
    );
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
    crate::workspace::control::workspace_new_response(&ctx, label)
}

pub async fn workspace_destroy_response(
    agent: &InteractiveAgentHost,
    target: &str,
) -> SlashCommandResponse {
    let ctx = workspace_control_context(agent);
    crate::workspace::control::workspace_destroy_response(&ctx, target)
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
    let settings = shared_settings.lock().unwrap().clone();
    let est = runtime_state.conversation.estimate_tokens();
    let live_harness = agent
        .dashboard_handles
        .harness
        .as_ref()
        .and_then(|h| h.lock().ok().map(|status| status.clone()))
        .unwrap_or_else(crate::status::HarnessStatus::assemble);
    let usage_pct = if settings.context_window > 0 {
        (est as f64 / settings.context_window as f64) * 100.0
    } else {
        0.0
    };
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
    let provider_summary = if live_harness.providers.is_empty() {
        "none detected".to_string()
    } else {
        let authenticated = live_harness
            .providers
            .iter()
            .filter(|provider| provider.authenticated)
            .count();
        format!(
            "{authenticated}/{} authenticated",
            live_harness.providers.len()
        )
    };

    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Session Overview\n\nActivity\n  Turns:            {}\n  Tool calls:       {}\n  Model:            {}\n  Thinking:         {} {}\n\nContext\n  Usage:            {:.0}%\n  Window:           {} tokens\n\nHarness\n  Persona:          {}\n  Tone:             {}\n  Providers:        {}\n  MCP servers:      {}\n\nCapabilities\n  Memory:           {}\n  Cleave:           {}",
            runtime_state.conversation.turn_count(),
            0,
            settings.model_short(),
            settings.thinking.icon(),
            settings.thinking.as_str(),
            usage_pct,
            settings.context_window,
            persona,
            tone,
            provider_summary,
            live_harness.mcp_servers.len(),
            if live_harness.memory_available {
                "available"
            } else {
                "UNAVAILABLE"
            },
            if live_harness.cleave_available {
                "available"
            } else {
                "UNAVAILABLE"
            },
        )),
    }
}

pub async fn tree_view_response(
    runtime_state: &mut InteractiveAgentState,
    args: &str,
) -> SlashCommandResponse {
    match runtime_state.bus.dispatch_command("design", args) {
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

    let opsx_dir = agent.cwd.join("openspec").join("changes");
    if opsx_dir.exists()
        && let Ok(entries) = std::fs::read_dir(&opsx_dir)
    {
        let active: Vec<String> = entries
            .filter_map(|e| {
                let e = e.ok()?;
                if e.file_type().ok()?.is_dir() {
                    Some(e.file_name().to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        if !active.is_empty() {
            sections.push(format!(
                "📋 {} OpenSpec change{}: {}",
                active.len(),
                if active.len() == 1 { "" } else { "s" },
                active.join(", ")
            ));
        }
    }

    let facts = crate::status::HarnessStatus::assemble().memory.total_facts;
    let working = crate::status::HarnessStatus::assemble()
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

const MANUAL_COMPACTION_KEEP_RECENT_TURNS: u32 = 2;

pub async fn context_compact_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    events_tx: &broadcast::Sender<AgentEvent>,
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
    let before_tokens = runtime_state.conversation.estimate_tokens() as u64;
    if let Some((payload, evict_count)) = runtime_state
        .conversation
        .build_compaction_payload_keeping_recent(MANUAL_COMPACTION_KEEP_RECENT_TURNS)
    {
        let _ = events_tx.send(AgentEvent::ContextCompaction(
            omegon_traits::ContextCompactionEvent {
                trigger: omegon_traits::ContextCompactionTrigger::Manual,
                status: omegon_traits::ContextCompactionStatus::Started,
                before_tokens,
                after_tokens: None,
                evicted_messages: Some(evict_count),
                summary_chars: None,
                reason: None,
            },
        ));
        match crate::r#loop::compact_via_llm(bridge_guard.as_ref(), &payload, &stream_options).await
        {
            Ok(summary) => {
                let summary_chars = summary.chars().count();
                runtime_state
                    .conversation
                    .apply_compaction_keeping_recent(summary, MANUAL_COMPACTION_KEEP_RECENT_TURNS);
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
                        reason: None,
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
                reason: Some("no evictable messages older than decay window".to_string()),
            },
        ));
        SlashCommandResponse {
            accepted: true,
            output: Some(
                "Nothing to compress yet — manual compaction keeps the last two turns and summarizes older turns.".to_string(),
            ),
        }
    }
}

pub async fn context_clear_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    cli: &CliRuntimeView<'_>,
    events_tx: &broadcast::Sender<AgentEvent>,
) -> SlashCommandResponse {
    if !cli.no_session {
        let _ = session::save_session(
            &runtime_state.conversation,
            &agent.cwd,
            Some(agent.session_id.as_str()),
        );
    }
    runtime_state.conversation = crate::conversation::ConversationState::new();
    agent.session_id = crate::session::allocate_session_id();
    agent.resume_info = None;
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
    match runtime_state
        .bus
        .execute_tool(
            crate::tool_registry::context::REQUEST_CONTEXT,
            "slash-context-request",
            args,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
    {
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
            match runtime_state
                .bus
                .execute_tool(
                    crate::tool_registry::context::REQUEST_CONTEXT,
                    "slash-context-request",
                    args,
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
            {
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
) -> SlashCommandResponse {
    if !cli.no_session {
        let _ = session::save_session(
            &runtime_state.conversation,
            &agent.cwd,
            Some(agent.session_id.as_str()),
        );
    }
    runtime_state.conversation = crate::conversation::ConversationState::new();
    agent.session_id = crate::session::allocate_session_id();
    agent.resume_info = None;
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
) -> SlashCommandResponse {
    let id = id.trim();
    if id.is_empty() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Usage: /resume <session-id>".to_string()),
        };
    }
    if !cli.no_session {
        let _ = session::save_session(
            &runtime_state.conversation,
            &agent.cwd,
            Some(agent.session_id.as_str()),
        );
    }
    let Some(path) = session::find_session(&agent.cwd, Some(id)) else {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "No saved session matches '{id}'. Use /sessions to list recent sessions."
            )),
        };
    };
    match crate::conversation::ConversationState::load_session(&path) {
        Ok(conversation) => {
            let meta_path = path.with_extension("meta.json");
            let meta = std::fs::read_to_string(&meta_path)
                .ok()
                .and_then(|j| serde_json::from_str::<session::SessionMeta>(&j).ok());
            let session_id = meta
                .as_ref()
                .map(|m| m.session_id.clone())
                .or_else(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| id.to_string());
            let description = meta
                .as_ref()
                .map(session::session_display_description)
                .unwrap_or_else(|| format!("Session {session_id}"));
            agent.resume_info = meta.as_ref().map(|m| crate::setup::ResumeInfo {
                session_id: m.session_id.clone(),
                turns: m.turns,
                description: description.clone(),
                last_prompt_snippet: m.last_prompt_snippet.clone(),
                created_at: m.created_at.clone(),
            });
            agent.session_id = session_id.clone();
            runtime_state.conversation = conversation;
            let _ = events_tx.send(AgentEvent::SessionReset);
            SlashCommandResponse {
                accepted: true,
                output: Some(format!("Resumed session {session_id}: {description}")),
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

pub async fn auth_login_response(
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    login_prompt_tx: &std::sync::Arc<tokio::sync::Mutex<Option<oneshot::Sender<String>>>>,
    events_tx: &broadcast::Sender<AgentEvent>,
    cli: &CliRuntimeView<'_>,
    cwd: &Path,
    provider: &str,
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
        .unwrap_or_else(|| cli.model.to_string());
    let cwd_for_profile = cwd.to_path_buf();
    let settings_for_login = shared_settings.clone();
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
                    "Anthropic OAuth login succeeded, but ANTHROPIC_API_KEY is also set. Requests will continue to prefer the API key. If you want Claude subscription auth for this session, unset ANTHROPIC_API_KEY and retry /login anthropic."
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
            let effective_model = providers::resolve_execution_model_spec(&login_provider_model)
                .await
                .unwrap_or(login_provider_model);
            if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await {
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
    let effective = providers::resolve_execution_model_spec(requested_model)
        .await
        .unwrap_or_else(|| requested_model.to_string());
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

pub async fn profile_apply_response(
    agent: &mut InteractiveAgentHost,
    runtime_state: &mut InteractiveAgentState,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
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
        let _ = runtime_state
            .bus
            .execute_tool(
                crate::tool_registry::persona::SWITCH_PERSONA,
                "profile-apply-persona",
                serde_json::json!({ "name": persona, "reason": "profile apply" }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await;
    }
    if let Some(tone) = profile.tone.as_deref() {
        let _ = runtime_state
            .bus
            .execute_tool(
                crate::tool_registry::persona::SWITCH_TONE,
                "profile-apply-tone",
                serde_json::json!({ "name": tone, "reason": "profile apply" }),
                tokio_util::sync::CancellationToken::new(),
            )
            .await;
    }

    let mut status = crate::status::HarnessStatus::assemble();
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
    let mut profile = settings::Profile::load(cwd);
    if let Some(enabled) = enabled {
        profile.integrations.mqtt.enabled = Some(enabled);
        let output = if enabled {
            "MQTT bridge profile default enabled. Takes effect on next startup."
        } else {
            "MQTT bridge profile default disabled. Takes effect on next startup."
        };
        return save_profile_response(cwd, profile, output);
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
    let mut profile = settings::Profile::load(cwd);
    retain_not_equal(&mut profile.extensions.disabled, name);
    push_unique(&mut profile.extensions.enabled, name);
    save_profile_response(
        cwd,
        profile,
        "Extension allowed in profile. Extension load policy takes effect on next startup.",
    )
}

pub async fn profile_extension_deny_response(cwd: &Path, name: &str) -> SlashCommandResponse {
    let name = name.trim();
    if name.is_empty() {
        return usage_response("Usage: /profile extension deny <name>");
    }
    let mut profile = settings::Profile::load(cwd);
    retain_not_equal(&mut profile.extensions.enabled, name);
    push_unique(&mut profile.extensions.disabled, name);
    save_profile_response(
        cwd,
        profile,
        "Extension denied in profile. Extension load policy takes effect on next startup.",
    )
}

pub async fn profile_extension_clear_response(cwd: &Path) -> SlashCommandResponse {
    let mut profile = settings::Profile::load(cwd);
    profile.extensions.enabled.clear();
    profile.extensions.disabled.clear();
    save_profile_response(
        cwd,
        profile,
        "Extension profile policy cleared. Installed enabled extensions are loadable again on next startup.",
    )
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
    if let Ok(mut s) = shared_settings.lock() {
        push_unique(&mut s.trusted_directories, path);
    }
    let mut profile = settings::Profile::load(cwd);
    profile.add_trusted_directory(path.to_string());
    save_profile_response(
        cwd,
        profile,
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
    let mut profile = settings::Profile::load(cwd);
    profile.remove_trusted_directory(path);
    save_profile_response(
        cwd,
        profile,
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
    handles: &crate::tui::dashboard::DashboardHandles,
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

    let persona_json = if let Some(ref harness) = handles.harness {
        if let Ok(h) = harness.lock() {
            if let Some(ref p) = h.active_persona {
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "badge": p.badge,
                    "activated_skills": p.activated_skills,
                    "disabled_tools": p.disabled_tools,
                })
            } else {
                serde_json::json!(null)
            }
        } else {
            serde_json::json!(null)
        }
    } else {
        serde_json::json!(null)
    };

    let profile = settings::Profile::load(cwd);

    let export = serde_json::json!({
        "format": "omegon-profile-export",
        "version": env!("CARGO_PKG_VERSION"),
        "settings": settings_json,
        "persona": persona_json,
        "profile": serde_json::to_value(&profile).unwrap_or(serde_json::json!(null)),
    });

    SlashCommandResponse {
        accepted: true,
        output: Some(export.to_string()),
    }
}

pub async fn persona_list_response(
    handles: &crate::tui::dashboard::DashboardHandles,
) -> SlashCommandResponse {
    let (personas, tones) = crate::plugins::persona_loader::scan_available();

    let active_id = handles
        .harness
        .as_ref()
        .and_then(|h| h.lock().ok())
        .and_then(|h| h.active_persona.as_ref().map(|p| p.id.clone()));

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

    let output = serde_json::json!({
        "personas": persona_list,
        "tones": tone_list,
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
                value: Some("Enter: inspect · i: install/refresh".into()),
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
                primary_action: Some(MenuActionProjection::command(
                    format!("skills.get.{}", entry.name),
                    "Inspect",
                    format!("/skills get {}", entry.name),
                )),
                actions: vec![{
                    let mut action = MenuActionProjection::command(
                        format!("skills.install.{}", entry.name),
                        "Install/refresh",
                        format!("/skills install {}", entry.name),
                    );
                    action.key = Some("i".into());
                    action
                }],
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
                        "Enter inspects the selected skill; filter by name, source, state, tag, or profile."
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
            "↑/↓ navigate · Enter inspect/run · i install selected skill · / filter · `/skills --help` syntax · Esc close"
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
                Some(name) => format!("Updated extension {name}. Run `/extension refresh` to inspect the current-session refresh candidate."),
                None => "Updated installed extensions. Run `/extension refresh` to inspect the current-session refresh candidate.".to_string(),
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
                "Enabled extension {}. Run `/extension refresh` to inspect the current-session refresh candidate.",
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
                "Disabled extension {}. Run `/extension refresh` to inspect the current-session refresh candidate.",
                name.trim()
            )),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/extension disable failed: {err}")),
        },
    }
}

pub async fn extension_search_response(query: Option<&str>) -> SlashCommandResponse {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match crate::armory::browse(crate::armory::BrowseOptions::new(
        crate::armory::ArmoryKind::Extensions,
        query,
        &cwd,
    ))
    .await
    {
        Ok(items) => {
            if items.is_empty() {
                return SlashCommandResponse {
                    accepted: true,
                    output: Some(match query {
                        Some(q) => format!("No extensions found matching '{q}'"),
                        None => "No extensions found in registry.".into(),
                    }),
                };
            }

            let mut out = format!("Available extensions ({}):\n\n", items.len());
            for item in &items {
                out.push_str(&format!(
                    "  {:<28} {}\n    {}\n\n",
                    item.id, item.category, item.description
                ));
            }
            out.push_str("Install: /extension install <name>");
            SlashCommandResponse {
                accepted: true,
                output: Some(out),
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
            "New sessions will discover the extension. Use /extension list to verify it is installed, or /extension refresh to inspect the current-session refresh candidate."
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
    let entries = crate::catalog::list(&home);
    if entries.is_empty() {
        return SlashCommandResponse {
            accepted: true,
            output: Some(
                "No catalog agents installed.\nRun `omegon catalog install` to install bundled agents.".into()
            ),
        };
    }
    let mut out = format!("Catalog agents ({}):\n\n", entries.len());
    for entry in &entries {
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
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Invalid agent ID: path traversal rejected".into()),
        };
    }
    let home = match crate::paths::omegon_home() {
        Ok(h) => h,
        Err(e) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Cannot determine home: {e}")),
            };
        }
    };
    let catalog_dir = home.join("catalog");
    let entries = crate::catalog::list(&home);
    match entries.iter().find(|e| e.id == id) {
        Some(entry) => {
            if !entry.bundle_dir.starts_with(&catalog_dir) {
                return SlashCommandResponse {
                    accepted: false,
                    output: Some("Refusing to remove agent outside catalog directory".into()),
                };
            }
            match std::fs::remove_dir_all(&entry.bundle_dir) {
                Ok(()) => SlashCommandResponse {
                    accepted: true,
                    output: Some(format!("Removed catalog agent '{id}'")),
                },
                Err(e) => SlashCommandResponse {
                    accepted: false,
                    output: Some(format!("Failed to remove: {e}")),
                },
            }
        }
        None => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Catalog agent '{id}' not found")),
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
) -> SlashCommandResponse {
    match runtime_state.bus.dispatch_command("cleave", "status") {
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
) -> SlashCommandResponse {
    match runtime_state
        .bus
        .dispatch_command("cleave", &format!("cancel {label}"))
    {
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
) -> SlashCommandResponse {
    match runtime_state.bus.dispatch_command("delegate", "status") {
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
        let tmp = tempfile::tempdir().unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let response =
            profile_capture_response(&settings, tmp.path(), settings::ProfileSaveTarget::User)
                .await;

        assert!(response.accepted, "{response:?}");
        let source = settings.lock().unwrap().profile_source.clone();
        assert!(
            matches!(source, settings::ProfileSource::User(_)),
            "{source:?}"
        );
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
        assert!(rendered.contains("Enter: `/skills get rust`"));
        assert!(rendered.contains("i: `/skills install rust`"));
        assert!(rendered.contains("### Actions"));
        assert!(rendered.contains("Enter: `/skills reload`"));
        assert!(rendered.contains("Enter: `/skills create --project`"));
        assert!(rendered.contains(
            "- `rust` — Enter: inspect · i: install/refresh · bundled · available · project_detected · profile:coding · tags:lang · read-only"
        ));
        assert!(rendered.contains(
            "- `team` — Enter: inspect · i: install/refresh · project · local · always · editable · reloadable · shadows:bundled · conflicts:bundled/rust · resolve:merge-recommended"
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
        assert_eq!(
            profile.permissions.trusted_directories,
            vec!["/tmp/vault".to_string()]
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
        assert!(profile.effective_trusted_directories().is_empty());
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
            context_manager: crate::context::ContextManager::new(String::new(), Vec::new()),
            conversation,
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
        InteractiveAgentHost {
            session_id: crate::session::allocate_session_id(),
            instance_id: "test-instance".into(),
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
        }
    }

    #[tokio::test]
    async fn manual_context_compact_emits_no_payload_diagnostic() {
        let mut state = InteractiveAgentState {
            bus: crate::bus::EventBus::new(),
            context_manager: crate::context::ContextManager::new(String::new(), Vec::new()),
            conversation: crate::conversation::ConversationState::new(),
        };
        let mut agent = test_agent();
        let settings = crate::settings::shared("test:model");
        let bridge = Arc::new(tokio::sync::RwLock::new(
            Box::new(MockBridge { events: vec![] }) as Box<dyn LlmBridge>,
        ));
        let (events_tx, mut events_rx) = broadcast::channel(8);

        let response =
            context_compact_response(&mut state, &mut agent, &settings, &bridge, &events_tx).await;

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

        let response =
            context_compact_response(&mut state, &mut agent, &settings, &bridge, &events_tx).await;

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

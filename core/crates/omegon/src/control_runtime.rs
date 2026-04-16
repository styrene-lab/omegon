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
    SwitchDispatcher {
        request_id: String,
        profile: String,
        model: Option<String>,
    },
    SetThinking {
        level: crate::settings::ThinkingLevel,
    },
    StatusView,
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
    AuthStatus,
    AuthUnlock,
    AuthLogin {
        provider: String,
    },
    AuthLogout {
        provider: String,
    },
    SkillsView,
    SkillsInstall,
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
    VaultStatus,
    VaultUnseal,
    VaultLogin,
    VaultConfigure,
    VaultInitPolicy,
    CleaveStatus,
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
        crate::tui::CanonicalSlashCommand::ModelList => ControlRequest::ModelList,
        crate::tui::CanonicalSlashCommand::SetModel(requested_model) => ControlRequest::SetModel {
            requested_model: requested_model.clone(),
        },
        crate::tui::CanonicalSlashCommand::SetThinking(level) => {
            ControlRequest::SetThinking { level: *level }
        }
        crate::tui::CanonicalSlashCommand::StatusView => ControlRequest::StatusView,
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
        crate::tui::CanonicalSlashCommand::AuthStatus => ControlRequest::AuthStatus,
        crate::tui::CanonicalSlashCommand::AuthUnlock => ControlRequest::AuthUnlock,
        crate::tui::CanonicalSlashCommand::AuthLogin(provider) => ControlRequest::AuthLogin {
            provider: provider.clone(),
        },
        crate::tui::CanonicalSlashCommand::AuthLogout(provider) => ControlRequest::AuthLogout {
            provider: provider.clone(),
        },
        crate::tui::CanonicalSlashCommand::SkillsView => ControlRequest::SkillsView,
        crate::tui::CanonicalSlashCommand::SkillsInstall => ControlRequest::SkillsInstall,
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
        crate::tui::CanonicalSlashCommand::VaultStatus => ControlRequest::VaultStatus,
        crate::tui::CanonicalSlashCommand::VaultUnseal => ControlRequest::VaultUnseal,
        crate::tui::CanonicalSlashCommand::VaultLogin => ControlRequest::VaultLogin,
        crate::tui::CanonicalSlashCommand::VaultConfigure => ControlRequest::VaultConfigure,
        crate::tui::CanonicalSlashCommand::VaultInitPolicy => ControlRequest::VaultInitPolicy,
        crate::tui::CanonicalSlashCommand::CleaveStatus => ControlRequest::CleaveStatus,
        crate::tui::CanonicalSlashCommand::CleaveCancelChild(label) => {
            ControlRequest::CleaveCancelChild {
                label: label.clone(),
            }
        }
        crate::tui::CanonicalSlashCommand::DelegateStatus => ControlRequest::DelegateStatus,
    })
}

pub async fn execute_control(
    ctx: &mut ControlContext<'_>,
    request: ControlRequest,
) -> SlashCommandResponse {
    match request {
        ControlRequest::ModelView => model_view_response(ctx.shared_settings).await,
        ControlRequest::ModelList => model_list_response().await,
        ControlRequest::SetModel { requested_model } => {
            set_model_response(ctx.agent, ctx.shared_settings, ctx.bridge, &requested_model).await
        }
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
            set_thinking_response(ctx.shared_settings, level).await
        }
        ControlRequest::StatusView => {
            status_view_response(ctx.runtime_state, ctx.shared_settings).await
        }
        ControlRequest::WorkspaceStatusView => workspace_status_view_response(ctx.agent).await,
        ControlRequest::WorkspaceListView => workspace_list_view_response(ctx.agent).await,
        ControlRequest::WorkspaceNew { label } => workspace_new_response(ctx.agent, &label).await,
        ControlRequest::WorkspaceDestroy { target } => {
            workspace_destroy_response(ctx.agent, &target).await
        }
        ControlRequest::WorkspaceAdopt => workspace_adopt_response(ctx.agent).await,
        ControlRequest::WorkspaceRelease => workspace_release_response(ctx.agent).await,
        ControlRequest::WorkspaceArchive => workspace_archive_response(ctx.agent).await,
        ControlRequest::WorkspacePrune => workspace_prune_response(ctx.agent).await,
        ControlRequest::WorkspaceBindMilestone { milestone_id } => {
            workspace_bind_milestone_response(ctx.agent, &milestone_id).await
        }
        ControlRequest::WorkspaceBindNode { design_node_id } => {
            workspace_bind_node_response(ctx.agent, &design_node_id).await
        }
        ControlRequest::WorkspaceBindClear => workspace_bind_clear_response(ctx.agent).await,
        ControlRequest::WorkspaceRoleView => workspace_role_view_response(ctx.agent).await,
        ControlRequest::WorkspaceRoleSet { role } => {
            workspace_role_set_response(ctx.agent, role).await
        }
        ControlRequest::WorkspaceRoleClear => workspace_role_clear_response(ctx.agent).await,
        ControlRequest::WorkspaceKindView => workspace_kind_view_response(ctx.agent).await,
        ControlRequest::WorkspaceKindSet { kind } => {
            workspace_kind_set_response(ctx.agent, kind).await
        }
        ControlRequest::WorkspaceKindClear => workspace_kind_clear_response(ctx.agent).await,
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
        ControlRequest::AuthStatus => auth_status_response().await,
        ControlRequest::AuthUnlock => auth_unlock_response().await,
        ControlRequest::AuthLogin { provider } => {
            auth_login_response(
                ctx.shared_settings,
                ctx.bridge,
                ctx.login_prompt_tx,
                ctx.events_tx,
                ctx.cli,
                &provider,
            )
            .await
        }
        ControlRequest::AuthLogout { provider } => auth_logout_response(&provider).await,
        ControlRequest::SkillsView => skills_view_response().await,
        ControlRequest::SkillsInstall => skills_install_response().await,
        ControlRequest::PluginView => plugin_view_response().await,
        ControlRequest::PluginInstall { uri } => plugin_install_response(&uri).await,
        ControlRequest::PluginRemove { name } => plugin_remove_response(&name).await,
        ControlRequest::PluginUpdate { name } => plugin_update_response(name.as_deref()).await,
        ControlRequest::SecretsView => secrets_view_response(ctx.agent.secrets.as_ref()).await,
        ControlRequest::SecretsSet { name, value } => {
            secrets_set_response(ctx.agent.secrets.as_ref(), &name, &value).await
        }
        ControlRequest::SecretsGet { name } => {
            secrets_get_response(ctx.agent.secrets.as_ref(), &name).await
        }
        ControlRequest::SecretsDelete { name } => {
            secrets_delete_response(ctx.agent.secrets.as_ref(), &name).await
        }
        ControlRequest::VaultStatus => vault_status_response(ctx.agent).await,
        ControlRequest::VaultUnseal => vault_unseal_response().await,
        ControlRequest::VaultLogin => vault_login_response().await,
        ControlRequest::VaultConfigure => vault_configure_response().await,
        ControlRequest::VaultInitPolicy => vault_init_policy_response().await,
        ControlRequest::CleaveStatus => cleave_status_response(ctx.runtime_state).await,
        ControlRequest::CleaveCancelChild { label } => {
            cleave_cancel_child_response(ctx.runtime_state, &label).await
        }
        ControlRequest::DelegateStatus => delegate_status_response(ctx.runtime_state).await,
        // ── Auspex fleet control (same handlers as daemon mode) ─────
        ControlRequest::SetMaxTurns { max_turns } => {
            set_max_turns_response(ctx.shared_settings, &ctx.agent.cwd, max_turns).await
        }
        ControlRequest::ProfileView => profile_view_response(ctx.shared_settings).await,
        ControlRequest::ProfileExport => {
            profile_export_response(ctx.shared_settings, &ctx.agent.cwd, &ctx.agent.dashboard_handles).await
        }
        ControlRequest::PersonaList => persona_list_response(&ctx.agent.dashboard_handles).await,
        ControlRequest::PersonaSwitch { name } => persona_switch_response(&name).await,
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
            | ControlRequest::SetThinking { .. }
            | ControlRequest::SetContextClass { .. }
            | ControlRequest::SetRuntimeMode { .. }
            | ControlRequest::SetMaxTurns { .. }
    );
    let resp = match request {
        // ── Model & thinking ────────────────────────────────────────
        ControlRequest::ModelView => model_view_response(shared_settings).await,
        ControlRequest::ModelList => model_list_response().await,
        ControlRequest::SetModel { requested_model } => {
            set_model_daemon_response(shared_settings, cwd, &requested_model).await
        }
        ControlRequest::SetThinking { level } => {
            set_thinking_daemon_response(shared_settings, cwd, level).await
        }
        ControlRequest::SetContextClass { class } => {
            set_context_class_daemon_response(shared_settings, cwd, class).await
        }
        ControlRequest::SetRuntimeMode { slim } => {
            set_runtime_mode_daemon_response(shared_settings, cwd, slim).await
        }

        // ── Auth ────────────────────────────────────────────────────
        ControlRequest::AuthStatus => auth_status_response().await,
        ControlRequest::AuthLogin { provider } => auth_login_daemon_response(&provider).await,
        ControlRequest::AuthLogout { provider } => auth_logout_response(&provider).await,

        // ── Secrets ─────────────────────────────────────────────────
        ControlRequest::SecretsView => secrets_view_response(secrets.as_ref()).await,
        ControlRequest::SecretsSet { name, value } => {
            secrets_set_response(secrets.as_ref(), &name, &value).await
        }
        ControlRequest::SecretsGet { name } => secrets_get_response(secrets.as_ref(), &name).await,
        ControlRequest::SecretsDelete { name } => {
            secrets_delete_response(secrets.as_ref(), &name).await
        }

        // ── Vault ───────────────────────────────────────────────────
        ControlRequest::VaultUnseal => vault_unseal_response().await,
        ControlRequest::VaultLogin => vault_login_response().await,
        ControlRequest::VaultConfigure => vault_configure_response().await,
        ControlRequest::VaultInitPolicy => vault_init_policy_response().await,

        // ── Skills & plugins ────────────────────────────────────────
        ControlRequest::SkillsView => skills_view_response().await,
        ControlRequest::SkillsInstall => skills_install_response().await,
        ControlRequest::PluginView => plugin_view_response().await,
        ControlRequest::PluginInstall { uri } => plugin_install_response(&uri).await,
        ControlRequest::PluginRemove { name } => plugin_remove_response(&name).await,
        ControlRequest::PluginUpdate { name } => plugin_update_response(name.as_deref()).await,

        // ── Sessions ────────────────────────────────────────────────
        ControlRequest::ListSessions => {
            let msg = list_sessions_message(cwd);
            SlashCommandResponse { accepted: true, output: Some(msg) }
        }

        // ── Auspex fleet control ────────────────────────────────────
        ControlRequest::SetMaxTurns { max_turns } => {
            set_max_turns_response(shared_settings, cwd, max_turns).await
        }
        ControlRequest::ProfileView => profile_view_response(shared_settings).await,
        ControlRequest::ProfileExport => {
            profile_export_response(shared_settings, cwd, handles).await
        }
        ControlRequest::PersonaList => persona_list_response(handles).await,
        ControlRequest::PersonaSwitch { name } => {
            persona_switch_response(&name).await
        }

        // ── Operations requiring TUI state ──────────────────────────
        other => {
            SlashCommandResponse {
                accepted: false,
                output: Some(format!("/{:?} requires interactive mode", other)),
            }
        }
    };
    // Emit HarnessStatusChanged for mutations so WebSocket/IPC clients see
    // updated state without polling.
    if resp.accepted && is_settings_mutation {
        if let Some(ref harness_handle) = handles.harness {
            if let Ok(mut status) = harness_handle.lock() {
                // Refresh settings-derived fields in the live harness status.
                if let Ok(s) = shared_settings.lock() {
                    status.context_class = s.effective_requested_class().label().to_string();
                    status.thinking_level = s.thinking.as_str().to_string();
                }
                if let Ok(json) = serde_json::to_value(&*status) {
                    let _ = events_tx.send(AgentEvent::HarnessStatusChanged { status_json: json });
                }
            }
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
                    "  {} — {} turns, {} tools — {}",
                    s.meta.session_id, s.meta.turns, s.meta.tool_calls, s.meta.last_prompt_snippet
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
    let mut output = String::from("Available Models\n");
    for (provider_name, models) in &catalog.providers {
        output.push_str(&format!("\n{}\n", provider_name));
        for model in models {
            output.push_str(&format!("  {} ({})\n", model.name, model.id));
        }
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(output),
    }
}

pub async fn set_model_response(
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    requested_model: &str,
) -> SlashCommandResponse {
    let effective_model = providers::resolve_execution_model_spec(requested_model)
        .await
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
    let normalized_profile = profile.trim().to_ascii_lowercase();
    let allowed = ["retribution", "victory", "gloriana"];
    if !allowed.contains(&normalized_profile.as_str()) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Unknown dispatcher profile '{profile}'. Expected one of: {}",
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
    let tier_model = match normalized_profile.as_str() {
        "retribution" => {
            if current_provider == "openai-codex" {
                "gpt-5.4-mini".to_string()
            } else if current_provider == "openai" {
                "gpt-5-mini".to_string()
            } else {
                "claude-haiku-4-5-20251001".to_string()
            }
        }
        "victory" => {
            if current_provider == "openai-codex" {
                "gpt-5.4".to_string()
            } else if current_provider == "openai" {
                "gpt-5".to_string()
            } else {
                "claude-sonnet-4-6".to_string()
            }
        }
        "gloriana" => {
            if current_provider == "openai" {
                "gpt-5.4".to_string()
            } else {
                "claude-opus-4-6".to_string()
            }
        }
        _ => current_model.clone(),
    };
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

pub async fn set_thinking_response(
    shared_settings: &settings::SharedSettings,
    level: crate::settings::ThinkingLevel,
) -> SlashCommandResponse {
    if let Ok(mut s) = shared_settings.lock() {
        s.thinking = level;
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Thinking → {} {}", level.icon(), level.as_str())),
    }
}

pub async fn set_runtime_mode_response(
    runtime_state: &mut InteractiveAgentState,
    shared_settings: &settings::SharedSettings,
    events_tx: &broadcast::Sender<AgentEvent>,
    slim: bool,
) -> SlashCommandResponse {
    if let Ok(mut s) = shared_settings.lock() {
        s.set_slim_mode(slim);
    }
    runtime_state.conversation.set_slim_mode(slim);
    runtime_state.bus.apply_operator_tool_profile(slim);

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
        &status.capability_tier.clone(),
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
        &status.capability_tier.clone(),
        operating_profile.posture.effective.display_name(),
        &operating_profile_label,
        &principal_id,
        &identity_issuer,
        &session_kind,
        &authorization,
    );
    let panel = crate::tui::bootstrap::render_bootstrap(&status, false);
    SlashCommandResponse {
        accepted: true,
        output: Some(panel),
    }
}

pub async fn workspace_status_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let lease = crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten();
    let registry = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten();

    let Some(lease) = lease else {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace: no local runtime metadata yet.".into()),
        };
    };

    let occupancy = registry
        .as_ref()
        .map(|registry| registry.workspaces.len())
        .unwrap_or(1);
    let owner = lease.owner_session_id.as_deref().unwrap_or("(none)");
    let milestone = lease.bindings.milestone_id.as_deref().unwrap_or("(none)");
    let node = lease.bindings.design_node_id.as_deref().unwrap_or("(none)");
    let change = lease
        .bindings
        .openspec_change
        .as_deref()
        .unwrap_or("(none)");
    let text = format!(
        "Workspace\n  ID:           {}\n  Label:        {}\n  Project:      {}\n  Path:         {}\n  Backend:      {}\n  Branch:       {}\n  Role:         {:?}\n  Kind:         {:?}\n  Mutability:   {:?}\n  Owner:        {}\n  Source:       {}\n  Milestone:    {}\n  Design Node:  {}\n  OpenSpec:     {}\n  Local Views:  {}",
        lease.workspace_id,
        lease.label,
        lease.project_id,
        lease.path,
        lease.backend_kind.as_str(),
        lease.branch,
        lease.role,
        lease.workspace_kind,
        lease.mutability,
        owner,
        lease.source,
        milestone,
        node,
        change,
        occupancy,
    );
    SlashCommandResponse {
        accepted: true,
        output: Some(text),
    }
}

pub async fn workspace_role_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let lease = crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten();
    let Some(lease) = lease else {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace role: no lease metadata yet.".into()),
        };
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Workspace Role\n  Current:      {}",
            lease.role.as_str(),
        )),
    }
}

pub async fn workspace_role_set_response(
    agent: &InteractiveAgentHost,
    role: crate::workspace::types::WorkspaceRole,
) -> SlashCommandResponse {
    let mut lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace role cannot be set before workspace metadata exists.".into(),
                ),
            };
        }
    };
    lease.role = role;
    if let Err(err) = crate::workspace::runtime::write_workspace_lease(&agent.cwd, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to update workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.role = role;
            }
        }
        let _ = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Workspace role set to {}.", role.as_str())),
    }
}

pub async fn workspace_role_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let mut lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace role cannot be cleared before workspace metadata exists.".into(),
                ),
            };
        }
    };
    lease.role = crate::workspace::types::WorkspaceRole::Primary;
    if let Err(err) = crate::workspace::runtime::write_workspace_lease(&agent.cwd, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to update workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.role = crate::workspace::types::WorkspaceRole::Primary;
            }
        }
        let _ = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some("Workspace role reset to primary.".into()),
    }
}

pub async fn workspace_kind_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let lease = crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten();
    let inferred = crate::workspace::infer::infer_workspace_kind(&agent.cwd);
    let Some(lease) = lease else {
        return SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace kind: no lease metadata yet. Inferred kind would be {}.",
                inferred.as_str()
            )),
        };
    };
    let declared = lease.workspace_kind;
    let source = if declared == inferred {
        "inferred/default"
    } else {
        "operator-declared"
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Workspace Kind\n  Current:      {}\n  Inferred:     {}\n  Source:       {}",
            declared.as_str(),
            inferred.as_str(),
            source,
        )),
    }
}

fn find_workspace_target(
    registry: &crate::workspace::types::WorkspaceRegistry,
    target: &str,
) -> Result<crate::workspace::types::WorkspaceSummary, String> {
    if let Some(workspace) = registry
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == target)
    {
        return Ok(workspace.clone());
    }

    let matches = registry
        .workspaces
        .iter()
        .filter(|workspace| workspace.label == target)
        .cloned()
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("Workspace '{target}' not found in local registry.")),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => Err(format!(
            "Workspace label '{target}' is ambiguous; use workspace_id instead."
        )),
    }
}

fn safe_remove_workspace_dir(
    current_cwd: &Path,
    repo_root: &Path,
    workspace_path: &Path,
) -> Result<(), String> {
    if workspace_path == current_cwd {
        return Err("Refusing to destroy the current active workspace path.".into());
    }
    if workspace_path == repo_root {
        return Err("Refusing to destroy the project root workspace.".into());
    }
    if workspace_path.exists() {
        std::fs::remove_dir_all(workspace_path)
            .map_err(|err| format!("Failed to remove workspace directory: {err}"))?;
    }
    Ok(())
}

pub async fn workspace_destroy_response(
    agent: &InteractiveAgentHost,
    target: &str,
) -> SlashCommandResponse {
    let registry = match crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(registry) => registry,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some("Workspace destroy requires existing local registry metadata.".into()),
            };
        }
    };
    let workspace = match find_workspace_target(&registry, target) {
        Ok(workspace) => workspace,
        Err(message) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(message),
            };
        }
    };
    if workspace.path == agent.cwd.display().to_string() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Refusing to destroy the current active workspace.".into()),
        };
    }
    if workspace.role == crate::workspace::types::WorkspaceRole::Primary {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Refusing to destroy the primary workspace.".into()),
        };
    }
    if workspace.owner_session_id.is_some() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Workspace must be released before it can be destroyed.".into()),
        };
    }
    if !workspace.archived {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Workspace must be archived before it can be destroyed.".into()),
        };
    }

    let repo_root = Path::new(&registry.repo_root);
    let workspace_path = Path::new(&workspace.path);
    let removal = match workspace.backend_kind {
        crate::workspace::types::WorkspaceBackendKind::GitWorktree
        | crate::workspace::types::WorkspaceBackendKind::JjCheckout => {
            omegon_git::worktree::remove_smart(repo_root, &workspace.label, workspace_path)
                .map_err(|err| format!("Failed to remove workspace backend: {err}"))
        }
        crate::workspace::types::WorkspaceBackendKind::LocalDir
        | crate::workspace::types::WorkspaceBackendKind::GitClone => {
            safe_remove_workspace_dir(&agent.cwd, repo_root, workspace_path)
        }
        crate::workspace::types::WorkspaceBackendKind::RemoteDir
        | crate::workspace::types::WorkspaceBackendKind::PodVolume => Err(
            "Workspace destroy is not yet implemented for remote-dir/pod-volume backends.".into(),
        ),
    };
    if let Err(message) = removal {
        return SlashCommandResponse {
            accepted: false,
            output: Some(message),
        };
    }

    let mut updated = registry.clone();
    updated
        .workspaces
        .retain(|entry| entry.workspace_id != workspace.workspace_id);
    if let Err(err) = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &updated) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Destroyed workspace but failed to update registry: {err}"
            )),
        };
    }

    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Destroyed archived workspace {} ({}).",
            workspace.workspace_id, workspace.label
        )),
    }
}

pub async fn workspace_adopt_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let mut lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some("Workspace adopt requires existing local workspace metadata.".into()),
            };
        }
    };
    let heartbeat = crate::workspace::runtime::heartbeat_epoch_secs(&lease.last_heartbeat);
    let now_epoch = chrono::Utc::now().timestamp();
    let request = crate::workspace::types::WorkspaceAdmissionRequest {
        requested_role: lease.role,
        requested_kind: lease.workspace_kind,
        requested_mutability: lease.mutability,
        session_id: Some(agent.session_id.clone()),
        action: crate::workspace::types::WorkspaceActionKind::SessionStart,
    };
    let outcome = crate::workspace::admission::classify_admission(
        Some(&lease),
        &request,
        now_epoch,
        heartbeat,
    );
    if !matches!(
        outcome,
        crate::workspace::types::AdmissionOutcome::ConflictStaleLeaseAdoptable { .. }
    ) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Workspace adopt is only allowed for stale leases. Current admission state: {:?}",
                outcome
            )),
        };
    }
    lease.owner_session_id = Some(agent.session_id.clone());
    lease.owner_agent_id = Some("omegon-local".into());
    lease.last_heartbeat = crate::workspace::runtime::current_timestamp();
    if let Err(err) = crate::workspace::runtime::write_workspace_lease(&agent.cwd, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to adopt workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.workspace_id == lease.workspace_id {
                workspace.owner_session_id = lease.owner_session_id.clone();
                workspace.last_heartbeat = lease.last_heartbeat.clone();
                workspace.stale = false;
            }
        }
        let _ = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Adopted stale workspace lease for {} ({}).",
            lease.workspace_id, lease.label
        )),
    }
}

fn rewrite_current_workspace<F>(
    agent: &InteractiveAgentHost,
    mutator: F,
) -> Result<crate::workspace::types::WorkspaceLease, String>
where
    F: FnOnce(&mut crate::workspace::types::WorkspaceLease),
{
    let mut lease = crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .map_err(|err| format!("Failed to read workspace lease: {err}"))?
        .ok_or_else(|| "Workspace metadata does not exist yet.".to_string())?;
    mutator(&mut lease);
    crate::workspace::runtime::write_workspace_lease(&agent.cwd, &lease)
        .map_err(|err| format!("Failed to update workspace lease: {err}"))?;
    if let Some(mut registry) = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .map_err(|err| format!("Failed to read workspace registry: {err}"))?
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.bindings = lease.bindings.clone();
                workspace.role = lease.role;
                workspace.workspace_kind = lease.workspace_kind;
                workspace.owner_session_id = lease.owner_session_id.clone();
                workspace.last_heartbeat = lease.last_heartbeat.clone();
                workspace.archived = lease.archived;
                workspace.archived_at = lease.archived_at.clone();
                workspace.archive_reason = lease.archive_reason.clone();
            }
        }
        crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry)
            .map_err(|err| format!("Failed to update workspace registry: {err}"))?;
    }
    Ok(lease)
}

pub async fn workspace_release_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace release requires existing local workspace metadata.".into(),
                ),
            };
        }
    };
    if lease.owner_session_id.as_deref() != Some(agent.session_id.as_str()) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Workspace release requires current ownership. Current owner: {}",
                lease.owner_session_id.as_deref().unwrap_or("(none)")
            )),
        };
    }
    match rewrite_current_workspace(agent, |lease| {
        lease.owner_session_id = None;
        lease.owner_agent_id = None;
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Released workspace {} ({}). It remains registered but is no longer owned.",
                lease.workspace_id, lease.label
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub async fn workspace_archive_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace archive requires existing local workspace metadata.".into(),
                ),
            };
        }
    };
    if lease.owner_session_id.is_some() {
        return SlashCommandResponse {
            accepted: false,
            output: Some("Workspace must be released before it can be archived.".into()),
        };
    }
    match rewrite_current_workspace(agent, |lease| {
        lease.archived = true;
        lease.archived_at = Some(crate::workspace::runtime::current_timestamp());
        lease.archive_reason = Some("operator".into());
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Archived workspace {} ({}). It remains on disk but is retired from active use.",
                lease.workspace_id, lease.label
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub async fn workspace_prune_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let mut registry = match crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(registry) => registry,
        None => {
            return SlashCommandResponse {
                accepted: true,
                output: Some("Workspace registry: nothing to prune.".into()),
            };
        }
    };
    let before = registry.workspaces.len();
    registry.workspaces.retain(|workspace| {
        let path = Path::new(&workspace.path);
        path.exists() || !workspace.archived
    });
    let removed = before.saturating_sub(registry.workspaces.len());
    if let Err(err) = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to write workspace registry: {err}")),
        };
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Pruned {} workspace registry entries.", removed)),
    }
}

pub async fn workspace_bind_milestone_response(
    agent: &InteractiveAgentHost,
    milestone_id: &str,
) -> SlashCommandResponse {
    match rewrite_current_workspace(agent, |lease| {
        lease.bindings.milestone_id = Some(milestone_id.to_string());
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace {} ({}) bound to milestone {}.",
                lease.workspace_id, lease.label, milestone_id
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub async fn workspace_bind_node_response(
    agent: &InteractiveAgentHost,
    design_node_id: &str,
) -> SlashCommandResponse {
    match rewrite_current_workspace(agent, |lease| {
        lease.bindings.design_node_id = Some(design_node_id.to_string());
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace {} ({}) bound to design node {}.",
                lease.workspace_id, lease.label, design_node_id
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub async fn workspace_bind_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    match rewrite_current_workspace(agent, |lease| {
        lease.bindings = crate::workspace::types::WorkspaceBindings::default();
    }) {
        Ok(lease) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "Workspace {} ({}) lifecycle bindings cleared.",
                lease.workspace_id, lease.label
            )),
        },
        Err(message) => SlashCommandResponse {
            accepted: false,
            output: Some(message),
        },
    }
}

pub async fn workspace_new_response(
    agent: &InteractiveAgentHost,
    label: &str,
) -> SlashCommandResponse {
    let project_root = crate::setup::find_project_root(&agent.cwd);
    let parent = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace creation requires existing local workspace metadata.".into(),
                ),
            };
        }
    };
    let sanitized = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase();
    if sanitized.is_empty() {
        return SlashCommandResponse {
            accepted: false,
            output: Some(
                "Workspace label must contain at least one alphanumeric character.".into(),
            ),
        };
    }
    let workspace_path = project_root
        .parent()
        .unwrap_or(project_root.as_path())
        .join(format!(
            "{}-{}",
            project_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("workspace"),
            sanitized
        ));
    let branch = format!("workspace/{}", sanitized);
    let info = match omegon_git::worktree::create_smart(
        &project_root,
        &workspace_path,
        &sanitized,
        &branch,
    ) {
        Ok(info) => info,
        Err(err) => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(format!("Failed to create sibling workspace: {err}")),
            };
        }
    };
    let backend_kind = match info.backend {
        "jj" => crate::workspace::types::WorkspaceBackendKind::JjCheckout,
        _ => crate::workspace::types::WorkspaceBackendKind::GitWorktree,
    };
    let now = crate::workspace::runtime::current_timestamp();
    let new_workspace_id = crate::workspace::runtime::workspace_id_from_path(&workspace_path);
    let new_lease = crate::workspace::types::WorkspaceLease {
        project_id: parent.project_id.clone(),
        workspace_id: new_workspace_id.clone(),
        label: sanitized.clone(),
        path: workspace_path.display().to_string(),
        backend_kind,
        vcs_ref: Some(crate::workspace::types::WorkspaceVcsRef {
            vcs: if info.backend == "jj" {
                "jj".into()
            } else {
                "git".into()
            },
            branch: Some(info.branch.clone()),
            revision: None,
            remote: Some("origin".into()),
        }),
        bindings: parent.bindings.clone(),
        branch: info.branch.clone(),
        role: crate::workspace::types::WorkspaceRole::Feature,
        workspace_kind: parent.workspace_kind,
        mutability: crate::workspace::types::Mutability::Mutable,
        owner_session_id: None,
        owner_agent_id: None,
        created_at: now.clone(),
        last_heartbeat: now.clone(),
        archived: false,
        archived_at: None,
        archive_reason: None,
        parent_workspace_id: Some(parent.workspace_id.clone()),
        source: "operator".into(),
    };
    if let Err(err) = crate::workspace::runtime::write_workspace_lease(&workspace_path, &new_lease)
    {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Created workspace but failed to write lease metadata: {err}"
            )),
        };
    }
    let mut registry = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
        .unwrap_or(crate::workspace::types::WorkspaceRegistry {
            project_id: parent.project_id.clone(),
            repo_root: project_root.display().to_string(),
            workspaces: vec![],
        });
    registry
        .workspaces
        .retain(|ws| ws.workspace_id != new_workspace_id);
    registry
        .workspaces
        .push(crate::workspace::types::WorkspaceSummary {
            workspace_id: new_lease.workspace_id.clone(),
            label: new_lease.label.clone(),
            path: new_lease.path.clone(),
            backend_kind: new_lease.backend_kind,
            vcs_ref: new_lease.vcs_ref.clone(),
            bindings: new_lease.bindings.clone(),
            branch: new_lease.branch.clone(),
            role: new_lease.role,
            workspace_kind: new_lease.workspace_kind,
            mutability: new_lease.mutability,
            owner_session_id: new_lease.owner_session_id.clone(),
            last_heartbeat: new_lease.last_heartbeat.clone(),
            archived: new_lease.archived,
            archived_at: new_lease.archived_at.clone(),
            archive_reason: new_lease.archive_reason.clone(),
            stale: false,
        });
    let _ = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry);
    let _ = crate::workspace::runtime::write_workspace_registry(&workspace_path, &registry);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Created sibling workspace '{}' at {} using {}.",
            sanitized,
            workspace_path.display(),
            backend_kind.as_str()
        )),
    }
}

pub async fn workspace_list_view_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let registry = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten();
    let Some(registry) = registry else {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace registry: no local runtime metadata yet.".into()),
        };
    };
    if registry.workspaces.is_empty() {
        return SlashCommandResponse {
            accepted: true,
            output: Some("Workspace registry is empty.".into()),
        };
    }
    let mut lines = vec![format!(
        "Workspaces\n  Project:      {}\n  Repo Root:    {}\n  Count:        {}\n",
        registry.project_id,
        registry.repo_root,
        registry.workspaces.len()
    )];
    for workspace in registry.workspaces {
        let owner = workspace.owner_session_id.as_deref().unwrap_or("(none)");
        let milestone = workspace
            .bindings
            .milestone_id
            .as_deref()
            .unwrap_or("(none)");
        let node = workspace
            .bindings
            .design_node_id
            .as_deref()
            .unwrap_or("(none)");
        let archive = if workspace.archived {
            format!(
                "archived at {} ({})",
                workspace.archived_at.as_deref().unwrap_or("unknown"),
                workspace.archive_reason.as_deref().unwrap_or("no reason")
            )
        } else {
            "active".to_string()
        };
        lines.push(format!(
            "- {} ({})\n    path: {}\n    backend: {}\n    branch: {}\n    role/kind: {:?} / {:?}\n    milestone/node: {} / {}\n    mutability: {:?}\n    owner: {}\n    archive: {}\n    stale: {}",
            workspace.workspace_id,
            workspace.label,
            workspace.path,
            workspace.backend_kind.as_str(),
            workspace.branch,
            workspace.role,
            workspace.workspace_kind,
            milestone,
            node,
            workspace.mutability,
            owner,
            archive,
            workspace.stale,
        ));
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(lines.join("\n")),
    }
}

pub async fn workspace_kind_set_response(
    agent: &InteractiveAgentHost,
    kind: crate::workspace::types::WorkspaceKind,
) -> SlashCommandResponse {
    let mut lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace kind cannot be set before workspace metadata exists.".into(),
                ),
            };
        }
    };
    lease.workspace_kind = kind;
    if let Err(err) = crate::workspace::runtime::write_workspace_lease(&agent.cwd, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to update workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.workspace_kind = kind;
            }
        }
        let _ = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Workspace kind set to {}.", kind.as_str())),
    }
}

pub async fn workspace_kind_clear_response(agent: &InteractiveAgentHost) -> SlashCommandResponse {
    let mut lease = match crate::workspace::runtime::read_workspace_lease(&agent.cwd)
        .ok()
        .flatten()
    {
        Some(lease) => lease,
        None => {
            return SlashCommandResponse {
                accepted: false,
                output: Some(
                    "Workspace kind cannot be cleared before workspace metadata exists.".into(),
                ),
            };
        }
    };
    let inferred = crate::workspace::infer::infer_workspace_kind(&agent.cwd);
    lease.workspace_kind = inferred;
    if let Err(err) = crate::workspace::runtime::write_workspace_lease(&agent.cwd, &lease) {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!("Failed to update workspace lease: {err}")),
        };
    }
    if let Some(mut registry) = crate::workspace::runtime::read_workspace_registry(&agent.cwd)
        .ok()
        .flatten()
    {
        for workspace in &mut registry.workspaces {
            if workspace.path == lease.path {
                workspace.workspace_kind = inferred;
            }
        }
        let _ = crate::workspace::runtime::write_workspace_registry(&agent.cwd, &registry);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Workspace kind reset to inferred value {}.",
            inferred.as_str()
        )),
    }
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
            output: Some(format!("❌ Can't create .omegon/: {e}")),
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
            output: Some(format!("❌ Failed to save note: {e}")),
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
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Context: {}/{} tokens ({}%)\nPolicy: {}\nModel: {}\nThinking: {}",
            est,
            ctx_window,
            pct,
            settings.effective_requested_class().label(),
            settings.context_class.label(),
            settings.thinking.as_str()
        )),
    }
}

pub async fn context_compact_response(
    runtime_state: &mut InteractiveAgentState,
    agent: &mut InteractiveAgentHost,
    shared_settings: &settings::SharedSettings,
    bridge: &Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
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
    if let Some((payload, _)) = runtime_state.conversation.build_compaction_payload() {
        match crate::r#loop::compact_via_llm(bridge_guard.as_ref(), &payload, &stream_options).await
        {
            Ok(summary) => {
                runtime_state.conversation.apply_compaction(summary);
                let est = runtime_state.conversation.estimate_tokens();
                let settings = shared_settings.lock().unwrap();
                if let Ok(mut metrics) = agent.context_metrics.lock() {
                    metrics.update(
                        est,
                        settings.context_window,
                        &settings.effective_requested_class().label(),
                        settings.thinking.as_str(),
                    );
                }
                SlashCommandResponse {
                    accepted: true,
                    output: Some(format!("Context compressed. Now using {est} tokens.")),
                }
            }
            Err(e) => SlashCommandResponse {
                accepted: false,
                output: Some(format!("Compression failed: {e}")),
            },
        }
    } else {
        SlashCommandResponse {
            accepted: true,
            output: Some(
                "Nothing to compress yet — compaction only summarizes older turns after the decay window.".to_string(),
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
        metrics.update(0, context_window, "Squad", "off");
        context_window
    } else {
        200_000
    };
    let _ = events_tx.send(AgentEvent::ContextUpdated {
        tokens: 0,
        context_window: context_window as u64,
        context_class: "Squad".to_string(),
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
    if let Ok(mut s) = shared_settings.lock() {
        s.set_requested_context_class(class);
        let mut profile = settings::Profile::load(&agent.cwd);
        profile.capture_from(&s);
        let _ = profile.save(&agent.cwd);
    }
    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Context policy → {} (model capacity unchanged)",
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
            _ => Err(anyhow::anyhow!(auth::operator_auth_unknown_provider_message(
                &provider_clone
            ))),
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
            Err(e) => format!("❌ Login failed: {}", e),
        };
        let _ = events_tx_clone.send(AgentEvent::SystemNotification { message });
        if let Some(conflict) = env_conflict {
            let _ = events_tx_clone.send(AgentEvent::SystemNotification { message: conflict });
        }
        if result.is_ok() {
            let effective_model = providers::resolve_execution_model_spec(&model_for_redetect)
                .await
                .unwrap_or(model_for_redetect.clone());
            if let Some(new_bridge) = providers::auto_detect_bridge(&effective_model).await {
                let mut guard = bridge_clone.write().await;
                *guard = new_bridge;
                if let Ok(mut s) = settings_for_login.lock() {
                    s.set_model(&effective_model);
                    s.provider_connected = crate::auth::provider_connected_for_model(&effective_model);
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
            output: Some(
                format!(
                    "Provider required for logout. Use one of: {}",
                    auth::operator_auth_provider_help_list()
                ),
            ),
        };
    }
    let provider = crate::auth::canonical_provider_id(provider);
    let Some(provider_info) = crate::auth::provider_by_id(provider) else {
        return SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "❌ {}",
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
            output: Some(format!("❌ Logout failed for {provider_label}: {}", e)),
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
                "❌ {}",
                auth::operator_auth_unknown_provider_message(provider)
            )),
        };
    };
    match provider_info.auth_method {
        auth::AuthMethod::OAuth => {
            SlashCommandResponse {
                accepted: false,
                output: Some(format!(
                    "{} uses OAuth login which requires a browser. \
                     Run `omegon auth login {}` from a terminal with browser access, \
                     then the daemon will pick up the new credentials on the next request.",
                    provider_info.display_name,
                    provider,
                )),
            }
        }
        auth::AuthMethod::ApiKey | auth::AuthMethod::Dynamic => {
            let env_hint = provider_info.env_vars.first().copied().unwrap_or("API_KEY");
            SlashCommandResponse {
                accepted: false,
                output: Some(format!(
                    "{} uses API key auth. Set {} in the environment or run \
                     `omegon auth login {}` from a terminal to store the key. \
                     The daemon will pick up credentials on the next request.",
                    provider_info.display_name,
                    env_hint,
                    provider,
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
    cwd: &Path,
    level: crate::settings::ThinkingLevel,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.thinking = level;
    let mut profile = settings::Profile::load(cwd);
    profile.capture_from(&s);
    let _ = profile.save(cwd);
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Thinking → {} {}", level.icon(), level.as_str())),
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
    cwd: &Path,
    class: crate::settings::ContextClass,
) -> SlashCommandResponse {
    let Ok(mut s) = shared_settings.lock() else {
        return SlashCommandResponse {
            accepted: false,
            output: Some("failed to acquire settings lock".to_string()),
        };
    };
    s.set_requested_context_class(class);
    let mut profile = settings::Profile::load(cwd);
    profile.capture_from(&s);
    let _ = profile.save(cwd);
    drop(s);
    SlashCommandResponse {
        accepted: true,
        output: Some(format!("Context policy → {}", class.label())),
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
    s.set_slim_mode(slim);
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
            if max_turns == 0 { "unlimited".to_string() } else { max_turns.to_string() }
        )),
    }
}

pub async fn profile_view_response(
    shared_settings: &settings::SharedSettings,
) -> SlashCommandResponse {
    let output = if let Ok(s) = shared_settings.lock() {
        serde_json::json!({
            "model": s.model,
            "thinking_level": s.thinking.as_str(),
            "context_class": s.effective_requested_class().label(),
            "context_window": s.context_window,
            "max_turns": s.max_turns,
            "slim_mode": s.slim_mode,
            "posture": serde_json::to_value(&s.posture).unwrap_or(serde_json::json!(null)),
            "provider_order": s.provider_order,
            "provider_connected": s.provider_connected,
        })
        .to_string()
    } else {
        "failed to read settings".to_string()
    };
    SlashCommandResponse {
        accepted: true,
        output: Some(output),
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
            "slim_mode": s.slim_mode,
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

pub async fn skills_view_response() -> SlashCommandResponse {
    match crate::skills::list_summary() {
        Ok(output) => SlashCommandResponse {
            accepted: true,
            output: Some(output),
        },
        Err(err) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("/skills list failed: {err}")),
        },
    }
}

pub async fn skills_install_response() -> SlashCommandResponse {
    match crate::skills::cmd_install() {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(
                "Installed bundled skills to ~/.omegon/skills. New sessions will load them."
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
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("Installed plugin from {}", uri.trim())),
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

pub async fn secrets_view_response(
    secrets: &omegon_secrets::SecretsManager,
) -> SlashCommandResponse {
    let names = secrets.list_recipes();
    let mut out = String::new();
    if names.is_empty() {
        out.push_str("No secrets stored.\n");
    } else {
        out.push_str(&format!("🔐 Secrets ({})\n\n", names.len()));
        for (name, recipe) in &names {
            out.push_str(&format!("  {name:<24} {recipe}\n"));
        }
        out.push('\n');
    }
    out.push_str("Common secrets:\n");
    out.push_str("  /secrets set GITHUB_TOKEN cmd:gh auth token    always fresh from CLI\n");
    out.push_str("  /secrets set NPM_TOKEN cmd:npm token get       always fresh from CLI\n");
    out.push_str("  /secrets set AWS_SECRET env:AWS_SECRET_ACCESS_KEY  from environment\n\n");
    out.push_str("API keys (no CLI available — store directly):\n");
    out.push_str("  /secrets set OPENROUTER_KEY sk-or-...          free cloud AI\n");
    out.push_str("  /secrets set ANTHROPIC_API_KEY sk-ant-...      Anthropic API\n\n");
    out.push_str("Retrieve or remove:\n");
    out.push_str("  /secrets get GITHUB_TOKEN\n");
    out.push_str("  /secrets delete GITHUB_TOKEN");
    SlashCommandResponse {
        accepted: true,
        output: Some(out),
    }
}

pub async fn secrets_set_response(
    secrets: &omegon_secrets::SecretsManager,
    name: &str,
    value: &str,
) -> SlashCommandResponse {
    let result = if value.contains(':')
        && ["env:", "cmd:", "vault:", "keyring:", "file:"]
            .iter()
            .any(|p| value.starts_with(p))
    {
        secrets.set_recipe(name, value)
    } else {
        secrets.set_keyring_secret(name, value)
    };
    match result {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!(
                "✓ Secret '{name}' stored (encrypted in OS keyring).\n  The agent will redact this value from all output."
            )),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Error storing secret: {e}")),
        },
    }
}

pub async fn secrets_get_response(
    secrets: &omegon_secrets::SecretsManager,
    name: &str,
) -> SlashCommandResponse {
    match secrets.resolve(name) {
        Some(val) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("🔓 {name} = {val}")),
        },
        None => SlashCommandResponse {
            accepted: false,
            output: Some(format!(
                "Secret '{name}' not found.\n  Use /secrets to see stored secrets."
            )),
        },
    }
}

pub async fn secrets_delete_response(
    secrets: &omegon_secrets::SecretsManager,
    name: &str,
) -> SlashCommandResponse {
    match secrets.delete_recipe(name) {
        Ok(()) => SlashCommandResponse {
            accepted: true,
            output: Some(format!("✓ Secret '{name}' deleted.")),
        },
        Err(e) => SlashCommandResponse {
            accepted: false,
            output: Some(format!("Error: {e}")),
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
             # Apply with: vault policy write omegon-agent omegon-policy.hcl\n\n\
             ```hcl\n\
             # Read/write agent-scoped secrets\n\
             path \"secret/data/omegon/*\" {\n  capabilities = [\"read\", \"create\", \"update\"]\n}\n\
             path \"secret/metadata/omegon/*\" {\n  capabilities = [\"read\", \"list\"]\n}\n\n\
             # Read-only access to shared infra secrets\n\
             path \"secret/data/bootstrap/*\" {\n  capabilities = [\"read\"]\n}\n\n\
             # Allow minting child tokens for cleave\n\
             path \"auth/token/create\" {\n  capabilities = [\"create\", \"update\"]\n  allowed_parameters = {\n    \"policies\" = [\"omegon-child\"]\n    \"ttl\" = [\"30m\"]\n    \"num_uses\" = [\"100\"]\n  }\n}\n\
             ```\n\n\
             Save to a file and apply: `vault policy write omegon-agent <file>`"
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

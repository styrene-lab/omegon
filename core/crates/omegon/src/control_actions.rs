use crate::tui::canonical_slash_command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRole {
    Read,
    Edit,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlIngress {
    Slash,
    Cli,
    Ipc,
    WebDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalAction {
    ContextView,
    ContextCompact,
    ContextClear,
    ContextRequest,
    ContextSetClass,
    SkillsView,
    SkillsGet,
    SkillsCreate,
    SkillsUpdate,
    SkillsDelete,
    SkillsInstall,
    PromptsList,
    PromptsGet,
    PromptsCreate,
    PromptsUpdate,
    PromptsDelete,
    PromptsPreview,
    PromptsSubmit,
    ModelView,
    ModelList,
    ModelSetSameProvider,
    ProviderSwitch,
    DispatcherSwitch,
    ThinkingSet,
    StatusView,
    SessionStatsView,
    TreeView,
    NoteAdd,
    NotesView,
    NotesClear,
    CheckinView,
    SessionNew,
    SessionList,
    TurnCancel,
    RuntimeShutdown,
    PromptSubmit,
    AuthStatus,
    AuthLogin,
    AuthLogout,
    AuthUnlock,
    SecretsView,
    SecretsSet,
    SecretsGet,
    SecretsDelete,
    PluginView,
    PluginInstall,
    PluginRemove,
    PluginUpdate,
    CleaveView,
    CleaveCancelChild,
    DelegateStatus,
    MaxTurnsSet,
    ProfileView,
    ProfileExport,
    ProfileEdit,
    ProfileApply,
    PersonaList,
    PersonaSwitch,
    RuntimeModeSet,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedAction {
    pub ingress: ControlIngress,
    pub action: CanonicalAction,
    pub role: ControlRole,
    pub remote_safe: bool,
}

pub fn is_role_sufficient(actual: ControlRole, required: ControlRole) -> bool {
    role_rank(actual) >= role_rank(required)
}

fn role_rank(role: ControlRole) -> u8 {
    match role {
        ControlRole::Read => 0,
        ControlRole::Edit => 1,
        ControlRole::Admin => 2,
    }
}

pub fn classify_ipc_method(method: &str) -> ClassifiedAction {
    let (action, role, remote_safe) = match method {
        "get_state" | "get_graph" | "subscribe" | "unsubscribe" => {
            (CanonicalAction::StatusView, ControlRole::Read, true)
        }
        "context_status" => (CanonicalAction::ContextView, ControlRole::Read, true),
        "context_compact" => (CanonicalAction::ContextCompact, ControlRole::Edit, true),
        "context_clear" => (CanonicalAction::ContextClear, ControlRole::Edit, true),
        "new_session" => (CanonicalAction::SessionNew, ControlRole::Edit, true),
        "list_sessions" => (CanonicalAction::SessionList, ControlRole::Read, false),
        "auth_status" => (CanonicalAction::AuthStatus, ControlRole::Read, true),
        "model_view" => (CanonicalAction::ModelView, ControlRole::Read, true),
        "model_list" => (CanonicalAction::ModelList, ControlRole::Read, true),
        "skills_view" => (CanonicalAction::SkillsView, ControlRole::Read, true),
        "skills_get" => (CanonicalAction::SkillsGet, ControlRole::Read, true),
        "skills_create" => (CanonicalAction::SkillsCreate, ControlRole::Edit, false),
        "skills_update" => (CanonicalAction::SkillsUpdate, ControlRole::Edit, false),
        "skills_delete" => (CanonicalAction::SkillsDelete, ControlRole::Edit, false),
        "skills_install" => (CanonicalAction::SkillsInstall, ControlRole::Edit, false),
        "prompts_list" => (CanonicalAction::PromptsList, ControlRole::Read, true),
        "prompts_get" => (CanonicalAction::PromptsGet, ControlRole::Read, true),
        "prompts_preview" | "prompts_resolve" => {
            (CanonicalAction::PromptsPreview, ControlRole::Read, true)
        }
        "prompts_create" => (CanonicalAction::PromptsCreate, ControlRole::Edit, false),
        "prompts_update" => (CanonicalAction::PromptsUpdate, ControlRole::Edit, false),
        "prompts_delete" => (CanonicalAction::PromptsDelete, ControlRole::Edit, false),
        "prompts_submit" => (CanonicalAction::PromptsSubmit, ControlRole::Read, true),
        "_skills/list" => (CanonicalAction::SkillsView, ControlRole::Read, true),
        "_skills/get" => (CanonicalAction::SkillsGet, ControlRole::Read, true),
        "_skills/create" => (CanonicalAction::SkillsCreate, ControlRole::Edit, false),
        "_skills/update" => (CanonicalAction::SkillsUpdate, ControlRole::Edit, false),
        "_skills/delete" => (CanonicalAction::SkillsDelete, ControlRole::Edit, false),
        "_skills/install" => (CanonicalAction::SkillsInstall, ControlRole::Edit, false),
        "_prompts/list" => (CanonicalAction::PromptsList, ControlRole::Read, true),
        "_prompts/get" => (CanonicalAction::PromptsGet, ControlRole::Read, true),
        "_prompts/preview" | "_prompts/resolve" => {
            (CanonicalAction::PromptsPreview, ControlRole::Read, true)
        }
        "_prompts/create" => (CanonicalAction::PromptsCreate, ControlRole::Edit, false),
        "_prompts/update" => (CanonicalAction::PromptsUpdate, ControlRole::Edit, false),
        "_prompts/delete" => (CanonicalAction::PromptsDelete, ControlRole::Edit, false),
        "_prompts/submit" => (CanonicalAction::PromptsSubmit, ControlRole::Read, true),
        "plugin_view" => (CanonicalAction::PluginView, ControlRole::Read, true),
        "plugin_install" => (CanonicalAction::PluginInstall, ControlRole::Edit, false),
        "plugin_remove" => (CanonicalAction::PluginRemove, ControlRole::Edit, false),
        "plugin_update" => (CanonicalAction::PluginUpdate, ControlRole::Edit, false),
        "secrets_view" => (CanonicalAction::SecretsView, ControlRole::Edit, false),
        "secrets_set" => (CanonicalAction::SecretsSet, ControlRole::Edit, false),
        "secrets_get" => (CanonicalAction::SecretsGet, ControlRole::Edit, false),
        "secrets_delete" => (CanonicalAction::SecretsDelete, ControlRole::Edit, false),
        "vault_status" => (CanonicalAction::StatusView, ControlRole::Read, false),
        "vault_unseal" => (CanonicalAction::Unknown, ControlRole::Admin, false),
        "vault_login" => (CanonicalAction::Unknown, ControlRole::Admin, false),
        "vault_configure" => (CanonicalAction::Unknown, ControlRole::Admin, false),
        "vault_init_policy" => (CanonicalAction::Unknown, ControlRole::Admin, false),
        "cleave_status" => (CanonicalAction::CleaveView, ControlRole::Read, true),
        "cleave_cancel_child" => (CanonicalAction::CleaveCancelChild, ControlRole::Edit, true),
        "delegate_status" => (CanonicalAction::DelegateStatus, ControlRole::Read, true),
        "set_model" => (CanonicalAction::ProviderSwitch, ControlRole::Admin, false),
        "switch_dispatcher" => (CanonicalAction::DispatcherSwitch, ControlRole::Admin, false),
        "set_thinking" => (CanonicalAction::ThinkingSet, ControlRole::Edit, true),
        "set_context_class" => (CanonicalAction::ContextSetClass, ControlRole::Edit, true),
        "set_runtime_mode" => (CanonicalAction::RuntimeModeSet, ControlRole::Edit, true),
        "set_max_turns" => (CanonicalAction::MaxTurnsSet, ControlRole::Edit, true),
        "profile_view" => (CanonicalAction::ProfileView, ControlRole::Read, true),
        "profile_export" => (CanonicalAction::ProfileExport, ControlRole::Read, true),
        "profile_capture" => (CanonicalAction::ProfileEdit, ControlRole::Edit, true),
        "profile_apply" => (CanonicalAction::ProfileApply, ControlRole::Edit, true),
        "profile_mqtt" => (CanonicalAction::ProfileEdit, ControlRole::Edit, true),
        "profile_extension_allow" | "profile_extension_deny" | "profile_extension_clear" => {
            (CanonicalAction::ProfileEdit, ControlRole::Edit, true)
        }
        "profile_persona" | "profile_tone" => {
            (CanonicalAction::ProfileEdit, ControlRole::Edit, true)
        }
        "persona_list" => (CanonicalAction::PersonaList, ControlRole::Read, true),
        "persona_switch" => (CanonicalAction::PersonaSwitch, ControlRole::Edit, true),
        "submit_prompt" => (CanonicalAction::PromptSubmit, ControlRole::Edit, true),
        "cancel" => (CanonicalAction::TurnCancel, ControlRole::Edit, true),
        "run_slash_command" => (CanonicalAction::Unknown, ControlRole::Edit, false),
        "shutdown" => (CanonicalAction::RuntimeShutdown, ControlRole::Admin, true),
        _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
    };
    ClassifiedAction {
        ingress: ControlIngress::Ipc,
        action,
        role,
        remote_safe,
    }
}

pub fn classify_ipc_set_model_request(
    current_model: &str,
    requested_model: &str,
) -> ClassifiedAction {
    let requested = requested_model.trim();
    if requested.is_empty() {
        return ClassifiedAction {
            ingress: ControlIngress::Ipc,
            action: CanonicalAction::Unknown,
            role: ControlRole::Admin,
            remote_safe: false,
        };
    }

    let current_provider = crate::providers::infer_provider_id(current_model);
    let requested_provider = crate::providers::infer_provider_id(requested);
    let explicit_provider_switch =
        requested.contains(':') && requested_provider != current_provider;

    let (action, role, remote_safe) = if explicit_provider_switch {
        (CanonicalAction::ProviderSwitch, ControlRole::Admin, false)
    } else {
        (
            CanonicalAction::ModelSetSameProvider,
            ControlRole::Edit,
            true,
        )
    };

    ClassifiedAction {
        ingress: ControlIngress::Ipc,
        action,
        role,
        remote_safe,
    }
}

pub fn classify_web_method(method: &str) -> ClassifiedAction {
    let (action, role, remote_safe) = match method {
        "request_snapshot" => (CanonicalAction::StatusView, ControlRole::Read, true),
        "user_prompt" => (CanonicalAction::PromptSubmit, ControlRole::Edit, true),
        "cancel" => (CanonicalAction::TurnCancel, ControlRole::Edit, true),
        "new_session" => (CanonicalAction::SessionNew, ControlRole::Edit, true),
        "context_status" => (CanonicalAction::ContextView, ControlRole::Read, true),
        "context_compact" => (CanonicalAction::ContextCompact, ControlRole::Edit, true),
        "context_clear" => (CanonicalAction::ContextClear, ControlRole::Edit, true),
        "auth_status" => (CanonicalAction::AuthStatus, ControlRole::Read, true),
        "auth_login" => (CanonicalAction::AuthLogin, ControlRole::Admin, true),
        "auth_logout" => (CanonicalAction::AuthLogout, ControlRole::Admin, true),
        "model_view" => (CanonicalAction::ModelView, ControlRole::Read, true),
        "model_list" => (CanonicalAction::ModelList, ControlRole::Read, true),
        "set_context_class" => (CanonicalAction::ContextSetClass, ControlRole::Edit, true),
        "set_runtime_mode" => (CanonicalAction::RuntimeModeSet, ControlRole::Edit, true),
        "set_max_turns" => (CanonicalAction::MaxTurnsSet, ControlRole::Edit, true),
        "profile_view" => (CanonicalAction::ProfileView, ControlRole::Read, true),
        "profile_export" => (CanonicalAction::ProfileExport, ControlRole::Read, true),
        "profile_capture" => (CanonicalAction::ProfileEdit, ControlRole::Edit, true),
        "profile_apply" => (CanonicalAction::ProfileApply, ControlRole::Edit, true),
        "profile_mqtt" => (CanonicalAction::ProfileEdit, ControlRole::Edit, true),
        "profile_extension_allow" | "profile_extension_deny" | "profile_extension_clear" => {
            (CanonicalAction::ProfileEdit, ControlRole::Edit, true)
        }
        "profile_persona" | "profile_tone" => {
            (CanonicalAction::ProfileEdit, ControlRole::Edit, true)
        }
        "persona_list" => (CanonicalAction::PersonaList, ControlRole::Read, true),
        "persona_switch" => (CanonicalAction::PersonaSwitch, ControlRole::Edit, true),
        "skills_view" => (CanonicalAction::SkillsView, ControlRole::Read, true),
        "skills_get" => (CanonicalAction::SkillsGet, ControlRole::Read, true),
        "skills_create" => (CanonicalAction::SkillsCreate, ControlRole::Edit, false),
        "skills_update" => (CanonicalAction::SkillsUpdate, ControlRole::Edit, false),
        "skills_delete" => (CanonicalAction::SkillsDelete, ControlRole::Edit, false),
        "skills_install" => (CanonicalAction::SkillsInstall, ControlRole::Edit, false),
        "prompts_list" => (CanonicalAction::PromptsList, ControlRole::Read, true),
        "prompts_get" => (CanonicalAction::PromptsGet, ControlRole::Read, true),
        "prompts_preview" | "prompts_resolve" => {
            (CanonicalAction::PromptsPreview, ControlRole::Read, true)
        }
        "prompts_create" => (CanonicalAction::PromptsCreate, ControlRole::Edit, false),
        "prompts_update" => (CanonicalAction::PromptsUpdate, ControlRole::Edit, false),
        "prompts_delete" => (CanonicalAction::PromptsDelete, ControlRole::Edit, false),
        "prompts_submit" => (CanonicalAction::PromptsSubmit, ControlRole::Read, true),
        "plugin_view" => (CanonicalAction::PluginView, ControlRole::Read, true),
        "plugin_install" => (CanonicalAction::PluginInstall, ControlRole::Edit, false),
        "plugin_remove" => (CanonicalAction::PluginRemove, ControlRole::Edit, false),
        "plugin_update" => (CanonicalAction::PluginUpdate, ControlRole::Edit, false),
        "secrets_view" => (CanonicalAction::SecretsView, ControlRole::Edit, true),
        "secrets_set" => (CanonicalAction::SecretsSet, ControlRole::Edit, true),
        "secrets_get" => (CanonicalAction::SecretsGet, ControlRole::Edit, true),
        "secrets_delete" => (CanonicalAction::SecretsDelete, ControlRole::Edit, true),
        "vault_status" => (CanonicalAction::StatusView, ControlRole::Read, true),
        "vault_unseal" => (CanonicalAction::Unknown, ControlRole::Admin, true),
        "vault_login" => (CanonicalAction::Unknown, ControlRole::Admin, true),
        "vault_configure" => (CanonicalAction::Unknown, ControlRole::Admin, true),
        "vault_init_policy" => (CanonicalAction::Unknown, ControlRole::Admin, true),
        "cleave_status" => (CanonicalAction::CleaveView, ControlRole::Read, true),
        "cleave_cancel_child" => (CanonicalAction::CleaveCancelChild, ControlRole::Edit, true),
        "delegate_status" => (CanonicalAction::DelegateStatus, ControlRole::Read, true),
        "set_model" => (CanonicalAction::ProviderSwitch, ControlRole::Admin, false),
        "switch_dispatcher" => (CanonicalAction::DispatcherSwitch, ControlRole::Admin, false),
        "set_thinking" => (CanonicalAction::ThinkingSet, ControlRole::Edit, true),
        "shutdown" => (CanonicalAction::RuntimeShutdown, ControlRole::Admin, true),
        _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
    };
    ClassifiedAction {
        ingress: ControlIngress::WebDaemon,
        action,
        role,
        remote_safe,
    }
}

pub fn classify_web_set_model_request(
    current_model: &str,
    requested_model: &str,
) -> ClassifiedAction {
    let mut classified = classify_ipc_set_model_request(current_model, requested_model);
    classified.ingress = ControlIngress::WebDaemon;
    classified
}

pub fn classify_daemon_trigger(trigger_kind: &str) -> ClassifiedAction {
    let (action, role, remote_safe) = match trigger_kind {
        "prompt" => (CanonicalAction::PromptSubmit, ControlRole::Edit, true),
        "cancel" => (CanonicalAction::TurnCancel, ControlRole::Edit, true),
        "new-session" => (CanonicalAction::SessionNew, ControlRole::Edit, true),
        "shutdown" => (CanonicalAction::RuntimeShutdown, ControlRole::Admin, true),
        "slash-command" => (CanonicalAction::Unknown, ControlRole::Edit, false),
        "cancel-cleave-child" => (CanonicalAction::Unknown, ControlRole::Edit, true),
        _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
    };
    ClassifiedAction {
        ingress: ControlIngress::WebDaemon,
        action,
        role,
        remote_safe,
    }
}

pub fn classify_slash_command(name: &str, args: &str) -> ClassifiedAction {
    let classified = match name {
        "skills" | "skill" => {
            let trimmed = args.trim();
            if trimmed.is_empty() || trimmed == "list" {
                (CanonicalAction::SkillsView, ControlRole::Read, true)
            } else if trimmed.starts_with("get ") {
                (CanonicalAction::SkillsGet, ControlRole::Read, true)
            } else if trimmed == "install" || trimmed.starts_with("install ") {
                (CanonicalAction::SkillsInstall, ControlRole::Edit, false)
            } else if trimmed == "create" || trimmed == "new" {
                (CanonicalAction::SkillsCreate, ControlRole::Edit, false)
            } else if trimmed.starts_with("delete ") {
                (CanonicalAction::SkillsDelete, ControlRole::Edit, false)
            } else {
                (CanonicalAction::Unknown, ControlRole::Admin, false)
            }
        }
        "prompt" | "prompts" => {
            let trimmed = args.trim();
            if trimmed.is_empty() || trimmed == "list" {
                (CanonicalAction::PromptsList, ControlRole::Read, true)
            } else if trimmed.starts_with("get ") {
                (CanonicalAction::PromptsGet, ControlRole::Read, true)
            } else if trimmed.starts_with("preview ") || trimmed.starts_with("resolve ") {
                (CanonicalAction::PromptsPreview, ControlRole::Read, true)
            } else if trimmed.starts_with("create ") || trimmed.starts_with("update ") {
                (CanonicalAction::Unknown, ControlRole::Edit, false)
            } else if trimmed.starts_with("delete ") {
                (CanonicalAction::PromptsDelete, ControlRole::Edit, false)
            } else if trimmed.starts_with("run ") || trimmed.starts_with("submit ") {
                (CanonicalAction::PromptsSubmit, ControlRole::Edit, false)
            } else {
                (CanonicalAction::Unknown, ControlRole::Admin, false)
            }
        }
        "model" => {
            let trimmed = args.trim();
            if trimmed.is_empty() {
                (CanonicalAction::ModelView, ControlRole::Read, true)
            } else if trimmed == "list" {
                (CanonicalAction::ModelList, ControlRole::Read, true)
            } else if trimmed.contains(':') {
                (CanonicalAction::ProviderSwitch, ControlRole::Admin, false)
            } else {
                (
                    CanonicalAction::ModelSetSameProvider,
                    ControlRole::Edit,
                    true,
                )
            }
        }
        "think" => (CanonicalAction::ThinkingSet, ControlRole::Edit, true),
        "context" => match canonical_slash_command("context", args) {
            Some(crate::tui::CanonicalSlashCommand::ContextStatus) | None
                if args.trim().is_empty() =>
            {
                (CanonicalAction::ContextView, ControlRole::Read, true)
            }
            Some(crate::tui::CanonicalSlashCommand::ContextStatus) => {
                (CanonicalAction::ContextView, ControlRole::Read, true)
            }
            Some(crate::tui::CanonicalSlashCommand::ContextCompact) => {
                (CanonicalAction::ContextCompact, ControlRole::Edit, true)
            }
            Some(crate::tui::CanonicalSlashCommand::ContextClear) => {
                (CanonicalAction::ContextClear, ControlRole::Edit, true)
            }
            Some(crate::tui::CanonicalSlashCommand::ContextRequest { .. })
            | Some(crate::tui::CanonicalSlashCommand::ContextRequestJson(_)) => {
                (CanonicalAction::ContextRequest, ControlRole::Edit, true)
            }
            Some(crate::tui::CanonicalSlashCommand::SetContextClass(_)) => {
                (CanonicalAction::ContextSetClass, ControlRole::Edit, true)
            }
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "new" => (CanonicalAction::SessionNew, ControlRole::Edit, true),
        "sessions" => (CanonicalAction::SessionList, ControlRole::Read, false),
        "auth" => match canonical_slash_command("auth", args) {
            Some(crate::tui::CanonicalSlashCommand::AuthView)
            | Some(crate::tui::CanonicalSlashCommand::AuthStatus) => {
                (CanonicalAction::AuthStatus, ControlRole::Read, true)
            }
            Some(crate::tui::CanonicalSlashCommand::AuthUnlock)
            | Some(crate::tui::CanonicalSlashCommand::AuthLogin(_))
            | Some(crate::tui::CanonicalSlashCommand::AuthLogout(_)) => {
                (CanonicalAction::AuthLogin, ControlRole::Admin, false)
            }
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "login" => (CanonicalAction::AuthLogin, ControlRole::Admin, false),
        "logout" => (CanonicalAction::AuthLogout, ControlRole::Admin, false),
        "secrets" => match args.split_whitespace().next().unwrap_or("") {
            "" | "list" => (CanonicalAction::SecretsView, ControlRole::Edit, false),
            "set" => (CanonicalAction::SecretsSet, ControlRole::Edit, false),
            "get" => (CanonicalAction::SecretsGet, ControlRole::Edit, false),
            "delete" => (CanonicalAction::SecretsDelete, ControlRole::Edit, false),
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "note" => (CanonicalAction::NoteAdd, ControlRole::Edit, true),
        "notes" => match args.trim() {
            "" => (CanonicalAction::NotesView, ControlRole::Edit, true),
            "clear" => (CanonicalAction::NotesClear, ControlRole::Edit, true),
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "checkin" => (CanonicalAction::CheckinView, ControlRole::Edit, true),
        "status" | "stats" | "auspex" | "dash" => {
            (CanonicalAction::StatusView, ControlRole::Read, true)
        }
        "plugin" => match args.split_whitespace().next().unwrap_or("") {
            "" | "list" => (CanonicalAction::PluginView, ControlRole::Read, true),
            "install" => (CanonicalAction::PluginInstall, ControlRole::Edit, false),
            "remove" => (CanonicalAction::PluginRemove, ControlRole::Edit, false),
            "update" => (CanonicalAction::PluginUpdate, ControlRole::Edit, false),
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "cleave" => {
            let trimmed = args.trim();
            if trimmed.is_empty() || trimmed == "status" {
                (CanonicalAction::CleaveView, ControlRole::Read, true)
            } else if trimmed.starts_with("cancel ") {
                (CanonicalAction::CleaveCancelChild, ControlRole::Edit, true)
            } else {
                (CanonicalAction::Unknown, ControlRole::Admin, false)
            }
        }
        "delegate" => match args.trim() {
            "" | "status" => (CanonicalAction::DelegateStatus, ControlRole::Read, true),
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
    };

    ClassifiedAction {
        ingress: ControlIngress::Slash,
        action: classified.0,
        role: classified.1,
        remote_safe: classified.2,
    }
}

pub fn classify_remote_slash_command(name: &str, args: &str) -> ClassifiedAction {
    let mut classified = classify_slash_command(name, args);
    classified.ingress = ControlIngress::Ipc;
    classified
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_context_view_as_read() {
        let action = classify_slash_command("context", "");
        assert_eq!(action.action, CanonicalAction::ContextView);
        assert_eq!(action.role, ControlRole::Read);
    }

    #[test]
    fn classifies_context_compact_as_edit() {
        let action = classify_slash_command("context", "compact");
        assert_eq!(action.action, CanonicalAction::ContextCompact);
        assert_eq!(action.role, ControlRole::Edit);
    }

    #[test]
    fn classifies_skills_view_as_read() {
        let action = classify_slash_command("skills", "");
        assert_eq!(action.action, CanonicalAction::SkillsView);
        assert_eq!(action.role, ControlRole::Read);
    }

    #[test]
    fn classifies_skills_install_as_edit() {
        let action = classify_slash_command("skills", "install");
        assert_eq!(action.action, CanonicalAction::SkillsInstall);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_remote_context_view_as_remote_safe() {
        let action = classify_remote_slash_command("context", "");
        assert_eq!(action.action, CanonicalAction::ContextView);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_remote_skills_install_as_local_only() {
        let action = classify_remote_slash_command("skills", "install");
        assert_eq!(action.action, CanonicalAction::SkillsInstall);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_remote_skills_get_as_remote_safe_read() {
        let action = classify_remote_slash_command("skills", "get rust");
        assert_eq!(action.action, CanonicalAction::SkillsGet);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_remote_prompt_preview_as_remote_safe_read() {
        let action = classify_remote_slash_command("prompt", "preview init");
        assert_eq!(action.action, CanonicalAction::PromptsPreview);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_remote_prompt_submit_as_local_only_edit() {
        let action = classify_remote_slash_command("prompt", "submit init");
        assert_eq!(action.action, CanonicalAction::PromptsSubmit);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_prompt_backend_methods() {
        let preview = classify_ipc_method("_prompts/preview");
        assert_eq!(preview.action, CanonicalAction::PromptsPreview);
        assert_eq!(preview.role, ControlRole::Read);
        assert!(preview.remote_safe);

        let submit = classify_ipc_method("_prompts/submit");
        assert_eq!(submit.action, CanonicalAction::PromptsSubmit);
        assert_eq!(submit.role, ControlRole::Read);
        assert!(submit.remote_safe);
    }

    #[test]
    fn classifies_remote_login_as_local_only_admin() {
        let action = classify_remote_slash_command("login", "anthropic");
        assert_eq!(action.action, CanonicalAction::AuthLogin);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_remote_secrets_set_as_local_only() {
        let action = classify_remote_slash_command("secrets", "set api-key test");
        assert_eq!(action.action, CanonicalAction::SecretsSet);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_remote_plugin_install_as_local_only() {
        let action = classify_remote_slash_command("plugin", "install alpha");
        assert_eq!(action.action, CanonicalAction::PluginInstall);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_model_with_explicit_provider_as_provider_switch() {
        let action = classify_remote_slash_command("model", "anthropic:claude-sonnet-4-6");
        assert_eq!(action.action, CanonicalAction::ProviderSwitch);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_bare_model_id_as_same_provider_tuning() {
        let action = classify_remote_slash_command("model", "gpt-5.4");
        assert_eq!(action.action, CanonicalAction::ModelSetSameProvider);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_auth_login_as_admin() {
        let action = classify_slash_command("login", "anthropic");
        assert_eq!(action.action, CanonicalAction::AuthLogin);
        assert_eq!(action.role, ControlRole::Admin);
    }

    #[test]
    fn classifies_nested_auth_commands() {
        let view = classify_slash_command("auth", "");
        assert_eq!(view.action, CanonicalAction::AuthStatus);
        assert_eq!(view.role, ControlRole::Read);
        assert!(view.remote_safe);

        let status = classify_slash_command("auth", "status");
        assert_eq!(status.action, CanonicalAction::AuthStatus);
        assert_eq!(status.role, ControlRole::Read);
        assert!(status.remote_safe);

        let unlock = classify_slash_command("auth", "unlock");
        assert_eq!(unlock.action, CanonicalAction::AuthLogin);
        assert_eq!(unlock.role, ControlRole::Admin);
        assert!(!unlock.remote_safe);

        let login = classify_slash_command("auth", "login anthropic");
        assert_eq!(login.action, CanonicalAction::AuthLogin);
        assert_eq!(login.role, ControlRole::Admin);
        assert!(!login.remote_safe);

        let logout = classify_slash_command("auth", "logout anthropic");
        assert_eq!(logout.action, CanonicalAction::AuthLogin);
        assert_eq!(logout.role, ControlRole::Admin);
        assert!(!logout.remote_safe);
    }

    #[test]
    fn classifies_ipc_switch_dispatcher_as_admin_local_only() {
        let action = classify_ipc_method("switch_dispatcher");
        assert_eq!(action.action, CanonicalAction::DispatcherSwitch);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_web_switch_dispatcher_as_admin_local_only() {
        let action = classify_web_method("switch_dispatcher");
        assert_eq!(action.action, CanonicalAction::DispatcherSwitch);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_ipc_model_view_as_read() {
        let action = classify_ipc_method("model_view");
        assert_eq!(action.action, CanonicalAction::ModelView);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_ipc_model_list_as_read() {
        let action = classify_ipc_method("model_list");
        assert_eq!(action.action, CanonicalAction::ModelList);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_ipc_set_thinking_as_edit() {
        let action = classify_ipc_method("set_thinking");
        assert_eq!(action.action, CanonicalAction::ThinkingSet);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_ipc_list_sessions_as_local_only_read() {
        let action = classify_ipc_method("list_sessions");
        assert_eq!(action.action, CanonicalAction::SessionList);
        assert_eq!(action.role, ControlRole::Read);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_ipc_set_model_as_admin_local_only() {
        let action = classify_ipc_method("set_model");
        assert_eq!(action.action, CanonicalAction::ProviderSwitch);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_ipc_set_model_request_same_provider_as_edit() {
        let action =
            classify_ipc_set_model_request("anthropic:claude-sonnet-4-6", "claude-opus-4-6");
        assert_eq!(action.action, CanonicalAction::ModelSetSameProvider);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_ipc_set_model_request_explicit_provider_switch_as_admin() {
        let action =
            classify_ipc_set_model_request("anthropic:claude-sonnet-4-6", "openai:gpt-5.4");
        assert_eq!(action.action, CanonicalAction::ProviderSwitch);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_web_model_view_as_read() {
        let action = classify_web_method("model_view");
        assert_eq!(action.action, CanonicalAction::ModelView);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_web_set_model_request_same_provider_as_edit() {
        let action =
            classify_web_set_model_request("anthropic:claude-sonnet-4-6", "claude-opus-4-6");
        assert_eq!(action.action, CanonicalAction::ModelSetSameProvider);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_web_set_model_request_provider_switch_as_admin() {
        let action =
            classify_web_set_model_request("anthropic:claude-sonnet-4-6", "openai:gpt-5.4");
        assert_eq!(action.action, CanonicalAction::ProviderSwitch);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_ipc_shutdown_as_admin() {
        let action = classify_ipc_method("shutdown");
        assert_eq!(action.action, CanonicalAction::RuntimeShutdown);
        assert_eq!(action.role, ControlRole::Admin);
    }

    #[test]
    fn classifies_ipc_secrets_view_as_edit_local_only() {
        let action = classify_ipc_method("secrets_view");
        assert_eq!(action.action, CanonicalAction::SecretsView);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_ipc_vault_status_as_read_local_only() {
        let action = classify_ipc_method("vault_status");
        assert_eq!(action.action, CanonicalAction::StatusView);
        assert_eq!(action.role, ControlRole::Read);
        assert!(!action.remote_safe);
    }

    #[test]
    fn classifies_web_secrets_set_as_edit_remote_safe() {
        let action = classify_web_method("secrets_set");
        assert_eq!(action.action, CanonicalAction::SecretsSet);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_web_vault_login_as_admin_remote_safe() {
        let action = classify_web_method("vault_login");
        assert_eq!(action.action, CanonicalAction::Unknown);
        assert_eq!(action.role, ControlRole::Admin);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_web_exposed_sensitive_methods_as_remote_safe_with_roles() {
        let cases = [
            ("auth_login", ControlRole::Admin),
            ("auth_logout", ControlRole::Admin),
            ("secrets_view", ControlRole::Edit),
            ("secrets_set", ControlRole::Edit),
            ("secrets_get", ControlRole::Edit),
            ("secrets_delete", ControlRole::Edit),
            ("vault_status", ControlRole::Read),
            ("vault_unseal", ControlRole::Admin),
            ("vault_login", ControlRole::Admin),
            ("vault_configure", ControlRole::Admin),
            ("vault_init_policy", ControlRole::Admin),
        ];

        for (method, role) in cases {
            let action = classify_web_method(method);
            assert_eq!(action.role, role, "{method} role drifted");
            assert!(
                action.remote_safe,
                "{method} is exposed by /ws and must be classified remote-safe"
            );
        }
    }

    #[test]
    fn classifies_slash_cleave_status_as_read_remote_safe() {
        let action = classify_slash_command("cleave", "status");
        assert_eq!(action.action, CanonicalAction::CleaveView);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_slash_cleave_cancel_as_edit_remote_safe() {
        let action = classify_slash_command("cleave", "cancel alpha");
        assert_eq!(action.action, CanonicalAction::CleaveCancelChild);
        assert_eq!(action.role, ControlRole::Edit);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_slash_delegate_status_as_read_remote_safe() {
        let action = classify_slash_command("delegate", "status");
        assert_eq!(action.action, CanonicalAction::DelegateStatus);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_ipc_cleave_status_as_read_remote_safe() {
        let action = classify_ipc_method("cleave_status");
        assert_eq!(action.action, CanonicalAction::CleaveView);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_web_delegate_status_as_read_remote_safe() {
        let action = classify_web_method("delegate_status");
        assert_eq!(action.action, CanonicalAction::DelegateStatus);
        assert_eq!(action.role, ControlRole::Read);
        assert!(action.remote_safe);
    }

    #[test]
    fn classifies_daemon_new_session_as_edit() {
        let action = classify_daemon_trigger("new-session");
        assert_eq!(action.action, CanonicalAction::SessionNew);
        assert_eq!(action.role, ControlRole::Edit);
    }
}

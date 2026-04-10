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
    SkillsInstall,
    ModelView,
    ModelList,
    ModelSetSameProvider,
    ProviderSwitch,
    ThinkingSet,
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
    StatusView,
    PluginView,
    PluginInstall,
    PluginRemove,
    PluginUpdate,
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
        "skills" => match args.trim() {
            "" | "list" => (CanonicalAction::SkillsView, ControlRole::Read, true),
            "install" => (CanonicalAction::SkillsInstall, ControlRole::Edit, false),
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "model" => {
            let trimmed = args.trim();
            if trimmed.is_empty() {
                (CanonicalAction::ModelView, ControlRole::Read, true)
            } else if trimmed == "list" {
                (CanonicalAction::ModelList, ControlRole::Read, true)
            } else if trimmed.contains(':') {
                (CanonicalAction::ProviderSwitch, ControlRole::Admin, false)
            } else {
                (CanonicalAction::ModelSetSameProvider, ControlRole::Edit, true)
            }
        }
        "think" => (CanonicalAction::ThinkingSet, ControlRole::Edit, true),
        "context" => match canonical_slash_command("context", args) {
            Some(crate::tui::CanonicalSlashCommand::ContextStatus) | None if args.trim().is_empty() => {
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
            Some(crate::tui::CanonicalSlashCommand::AuthStatus) => {
                (CanonicalAction::AuthStatus, ControlRole::Read, true)
            }
            Some(crate::tui::CanonicalSlashCommand::AuthUnlock) => {
                (CanonicalAction::AuthUnlock, ControlRole::Admin, false)
            }
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "login" => (CanonicalAction::AuthLogin, ControlRole::Admin, false),
        "logout" => (CanonicalAction::AuthLogout, ControlRole::Admin, false),
        "secrets" => match args.trim().split_whitespace().next().unwrap_or("") {
            "" | "list" => (CanonicalAction::SecretsView, ControlRole::Edit, false),
            "set" => (CanonicalAction::SecretsSet, ControlRole::Edit, false),
            "get" => (CanonicalAction::SecretsGet, ControlRole::Edit, false),
            "delete" => (CanonicalAction::SecretsDelete, ControlRole::Edit, false),
            _ => (CanonicalAction::Unknown, ControlRole::Admin, false),
        },
        "status" | "stats" | "auspex" | "dash" => {
            (CanonicalAction::StatusView, ControlRole::Read, true)
        }
        "plugin" => match args.trim().split_whitespace().next().unwrap_or("") {
            "" | "list" => (CanonicalAction::PluginView, ControlRole::Read, true),
            "install" => (CanonicalAction::PluginInstall, ControlRole::Edit, false),
            "remove" => (CanonicalAction::PluginRemove, ControlRole::Edit, false),
            "update" => (CanonicalAction::PluginUpdate, ControlRole::Edit, false),
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
    fn classifies_ipc_shutdown_as_admin() {
        let action = classify_ipc_method("shutdown");
        assert_eq!(action.action, CanonicalAction::RuntimeShutdown);
        assert_eq!(action.role, ControlRole::Admin);
    }

    #[test]
    fn classifies_daemon_new_session_as_edit() {
        let action = classify_daemon_trigger("new-session");
        assert_eq!(action.action, CanonicalAction::SessionNew);
        assert_eq!(action.role, ControlRole::Edit);
    }
}

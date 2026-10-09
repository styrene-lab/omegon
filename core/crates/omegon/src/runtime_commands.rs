//! Surface-neutral canonical slash-command parsing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCreateScope {
    Project,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalSlashCommand {
    ModelView,
    ModelList,
    SetModel(String),
    SetModelGrade(String),
    SetModelProvider(String),
    SetModelPolicy(String),
    ModelUnpin,
    ThinkingView,
    SetThinking(crate::settings::ThinkingLevel),
    ProfileView,
    ProfileExport,
    ProfileCapture(crate::settings::ProfileSaveTarget),
    ProfileApply,
    ProfileUse {
        id: String,
        scope: Option<String>,
    },
    ProfileSetMqtt(Option<bool>),
    ProfileExtensionAllow(String),
    ProfileExtensionDeny(String),
    ProfileExtensionClear,
    ProfileComponentEnable(String),
    ProfileComponentDisable(String),
    ProfileComponentsView,
    ProfileSetPersona(Option<String>),
    ProfileSetTone(Option<String>),
    AutomationView,
    AutomationSet(crate::settings::AutomationLevel),
    PermissionsView,
    PermissionTrustAdd(String),
    PermissionTrustRemove(String),
    StatusView,
    RuntimeDoctor,
    SetRuntimeMode {
        slim: bool,
    },
    RuntimeInventoryStatus,
    RuntimeSubstrateRefresh,
    RuntimeExtensionReplace(String),
    RuntimeProcessRestart,
    WorkspaceStatusView,
    WorkspaceListView,
    WorkspaceNew(String),
    WorkspaceDestroy(String),
    WorkspaceAdopt,
    WorkspaceRelease,
    WorkspaceArchive,
    WorkspacePrune,
    WorkspaceBindMilestone(String),
    WorkspaceBindNode(String),
    WorkspaceBindClear,
    WorkspaceRoleView,
    WorkspaceRoleSet(crate::workspace::types::WorkspaceRole),
    WorkspaceRoleClear,
    WorkspaceKindView,
    WorkspaceKindSet(crate::workspace::types::WorkspaceKind),
    WorkspaceKindClear,
    SetMaxTurns {
        max_turns: u32,
    },
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
    ContextCapacity(String),
    ContextReasoning(String),
    ContextCompact,
    ContextClear,
    ContextRequest {
        kind: String,
        query: String,
    },
    ContextRequestJson(String),
    SetContextClass(crate::settings::ContextClass),
    NewSession,
    ListSessions,
    ResumeSession(String),
    AuthView,
    AuthStatus,
    AuthUnlock,
    AuthLogin(String),
    AuthLogout(String),
    SkillsView,
    SkillsHelp,
    SkillsReload,
    SkillsInstall(Option<String>),
    SkillCreate(Option<SkillCreateScope>),
    SkillImport {
        path: String,
        scope: Option<SkillCreateScope>,
    },
    SkillGet(String),
    SkillDelete(String),
    PlanView,
    PlanList,
    PlanShow(String),
    PlanSwitch(String),
    PlanResume(String),
    PlanBackground(Option<String>),
    PlanDetach(Option<String>),
    PlanPromote(Option<String>),
    PlanBind(String),
    PlanLedger(Option<String>),
    PlanSet(Vec<String>),
    PlanApprove,
    PlanExecute,
    PlanAdvance,
    PlanSkip,
    PlanClear,
    ExtensionView,
    ExtensionInit(String),
    ExtensionGet(String),
    ExtensionInstall(String),
    ExtensionRemove(String),
    ExtensionUpdate(Option<String>),
    ExtensionEnable(String),
    ExtensionDisable(String),
    ExtensionSearch(Option<String>),
    ArmoryBrowse(Option<String>),
    ArmoryInstall(String),
    PersonaList,
    CatalogView,
    CatalogInstall,
    CatalogRemove(String),
    PluginView,
    PluginInstall(String),
    PluginRemove(String),
    PluginUpdate(Option<String>),
    SecretsView,
    SecretsSet {
        name: String,
        value: String,
    },
    SecretsGet(String),
    SecretsDelete(String),
    VariablesView,
    VariablesSet {
        name: String,
        value: String,
    },
    VariablesGet(String),
    VariablesDelete(String),
    VaultStatus,
    VaultConfigure,
    VaultInitPolicy,
    CleaveStatus,
    CleaveCancelChild(String),
    DelegateStatus,
    Smoke(crate::smoke_surface::SmokeCommand),
}

pub(crate) fn canonical_slash_command(cmd: &str, args: &str) -> Option<CanonicalSlashCommand> {
    let args = args.trim();
    match cmd {
        "model" if args.is_empty() || args == "route" => None,
        "model" if matches!(args, "list" | "providers" | "status" | "view") => {
            Some(CanonicalSlashCommand::ModelList)
        }
        "model" if args == "unpin" => Some(CanonicalSlashCommand::ModelUnpin),
        "model" if args.starts_with("policy ") => {
            let policy = args.trim_start_matches("policy ").trim();
            if policy.is_empty() {
                None
            } else {
                Some(CanonicalSlashCommand::SetModelPolicy(policy.to_string()))
            }
        }
        "model" if args.starts_with("provider ") => {
            let provider = args.trim_start_matches("provider ").trim();
            if provider.is_empty() {
                None
            } else {
                Some(CanonicalSlashCommand::SetModelProvider(
                    provider.to_string(),
                ))
            }
        }
        "model" if args.starts_with("grade ") => {
            let grade = args.trim_start_matches("grade ").trim();
            if matches!(grade, "F" | "D" | "C" | "B" | "A" | "S") {
                Some(CanonicalSlashCommand::SetModelGrade(grade.to_string()))
            } else {
                None
            }
        }
        "model" if !args.is_empty() => Some(CanonicalSlashCommand::SetModel(args.to_string())),
        "think" if args == "list" || args == "status" => Some(CanonicalSlashCommand::ThinkingView),
        "think" => {
            crate::settings::ThinkingLevel::parse(args).map(CanonicalSlashCommand::SetThinking)
        }
        "profile" if args.is_empty() => None,
        "profile" if args == "status" || args == "view" => Some(CanonicalSlashCommand::ProfileView),
        "profile" if args == "export" => Some(CanonicalSlashCommand::ProfileExport),
        "profile"
            if matches!(
                args,
                "capture" | "save" | "capture --active" | "save --active"
            ) =>
        {
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::ActiveSource,
            ))
        }
        "profile" if matches!(args, "capture --project" | "save --project") => Some(
            CanonicalSlashCommand::ProfileCapture(crate::settings::ProfileSaveTarget::Project),
        ),
        "profile"
            if matches!(
                args,
                "capture --user" | "save --user" | "capture --global" | "save --global"
            ) =>
        {
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::User,
            ))
        }
        "profile" if (args.starts_with("save --name ") || args.starts_with("capture --name ")) => {
            // `/profile save --name <name>` → user scope (default)
            // `/profile save --name <name> --project` → project scope
            let rest = args
                .trim_start_matches("save --name ")
                .trim_start_matches("capture --name ");
            let (name, scope) = if let Some(n) = rest.strip_suffix(" --project") {
                (n, crate::settings::ProfileRegistryScope::Project)
            } else {
                let name = rest.split_whitespace().next().unwrap_or(rest);
                (name, crate::settings::ProfileRegistryScope::User)
            };
            if name.is_empty() {
                None
            } else {
                Some(CanonicalSlashCommand::ProfileCapture(
                    crate::settings::ProfileSaveTarget::Named {
                        name: name.to_string(),
                        scope,
                    },
                ))
            }
        }
        "profile" if args == "apply" || args == "load" => Some(CanonicalSlashCommand::ProfileApply),
        "profile" if args == "mqtt" || args == "mqtt status" => {
            Some(CanonicalSlashCommand::ProfileSetMqtt(None))
        }
        "profile" if args == "mqtt on" || args == "mqtt enable" => {
            Some(CanonicalSlashCommand::ProfileSetMqtt(Some(true)))
        }
        "profile" if args == "mqtt off" || args == "mqtt disable" => {
            Some(CanonicalSlashCommand::ProfileSetMqtt(Some(false)))
        }
        "profile" if args == "extensions clear" || args == "extension clear" => {
            Some(CanonicalSlashCommand::ProfileExtensionClear)
        }
        "profile" if args == "components view" => {
            Some(CanonicalSlashCommand::ProfileComponentsView)
        }
        "profile" if args.starts_with("component enable ") => args
            .strip_prefix("component enable ")
            .map(str::trim)
            .filter(|selector| !selector.is_empty())
            .map(|selector| CanonicalSlashCommand::ProfileComponentEnable(selector.to_string())),
        "profile" if args.starts_with("component disable ") => args
            .strip_prefix("component disable ")
            .map(str::trim)
            .filter(|selector| !selector.is_empty())
            .map(|selector| CanonicalSlashCommand::ProfileComponentDisable(selector.to_string())),
        "profile" => {
            if let Some(name) = args
                .strip_prefix("extension allow ")
                .or_else(|| args.strip_prefix("extensions allow "))
                .or_else(|| args.strip_prefix("extension enable "))
                .or_else(|| args.strip_prefix("extensions enable "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::ProfileExtensionAllow(
                    name.to_string(),
                ))
            } else if let Some(name) = args
                .strip_prefix("extension deny ")
                .or_else(|| args.strip_prefix("extensions deny "))
                .or_else(|| args.strip_prefix("extension disable "))
                .or_else(|| args.strip_prefix("extensions disable "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::ProfileExtensionDeny(
                    name.to_string(),
                ))
            } else if let Some(rest) = args
                .strip_prefix("use ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let parts = shlex::split(rest)?;
                let id = parts.first()?.clone();
                let scope = match parts.as_slice() {
                    [_] => None,
                    [_, scope] if !scope.starts_with("--") => Some(scope.clone()),
                    [_, flag] if flag.starts_with("--scope=") => {
                        Some(flag.trim_start_matches("--scope=").to_string())
                    }
                    [_, flag, scope] if flag == "--scope" => Some(scope.clone()),
                    _ => return None,
                };
                Some(CanonicalSlashCommand::ProfileUse { id, scope })
            } else if let Some(name) = args.strip_prefix("persona ").map(str::trim) {
                Some(CanonicalSlashCommand::ProfileSetPersona(
                    (!name.is_empty() && name != "off" && name != "clear")
                        .then(|| name.to_string()),
                ))
            } else {
                args.strip_prefix("tone ").map(str::trim).map(|name| {
                    CanonicalSlashCommand::ProfileSetTone(
                        (!name.is_empty() && name != "off" && name != "clear")
                            .then(|| name.to_string()),
                    )
                })
            }
        }
        "automation" | "autonomy" if args.is_empty() || args == "status" || args == "view" => {
            Some(CanonicalSlashCommand::AutomationView)
        }
        "automation" | "autonomy" => {
            crate::settings::AutomationLevel::parse(args).map(CanonicalSlashCommand::AutomationSet)
        }
        "permissions" | "permission"
            if args.is_empty() || args == "status" || args == "list" || args == "keys" =>
        {
            Some(CanonicalSlashCommand::PermissionsView)
        }
        "permissions" | "permission" | "trust" => {
            let normalized = args
                .strip_prefix("trusted ")
                .or_else(|| args.strip_prefix("trust "))
                .unwrap_or(args)
                .trim();
            if let Some(path) = normalized
                .strip_prefix("add ")
                .or_else(|| normalized.strip_prefix("allow "))
                .map(str::trim)
                .filter(|path| !path.is_empty())
            {
                Some(CanonicalSlashCommand::PermissionTrustAdd(path.to_string()))
            } else if let Some(path) = normalized
                .strip_prefix("remove ")
                .or_else(|| normalized.strip_prefix("rm "))
                .or_else(|| normalized.strip_prefix("revoke "))
                .or_else(|| normalized.strip_prefix("deny "))
                .map(str::trim)
                .filter(|path| !path.is_empty())
            {
                Some(CanonicalSlashCommand::PermissionTrustRemove(
                    path.to_string(),
                ))
            } else if normalized.is_empty() || normalized == "list" || normalized == "status" {
                Some(CanonicalSlashCommand::PermissionsView)
            } else {
                None
            }
        }
        "status" if args.is_empty() => Some(CanonicalSlashCommand::StatusView),
        "doctor" if args.is_empty() => Some(CanonicalSlashCommand::RuntimeDoctor),
        "runtime" if args == "doctor" => Some(CanonicalSlashCommand::RuntimeDoctor),
        "runtime" if args.starts_with("replace ") => args
            .strip_prefix("replace ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| CanonicalSlashCommand::RuntimeExtensionReplace(name.to_string())),
        "runtime" if matches!(args, "status" | "inventory") => {
            Some(CanonicalSlashCommand::RuntimeInventoryStatus)
        }
        "runtime"
            if matches!(
                args,
                "refresh" | "reload" | "hup" | "kick" | "restart" | "hot-restart"
            ) =>
        {
            // Preserve the process that owns the active TUI/ACP/harness transport.
            Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
        }
        "workspace" if args.is_empty() => Some(CanonicalSlashCommand::WorkspaceStatusView),
        "workspace" if args == "status" => Some(CanonicalSlashCommand::WorkspaceStatusView),
        "workspace" if args == "list" => Some(CanonicalSlashCommand::WorkspaceListView),
        "workspace" if args == "adopt" => Some(CanonicalSlashCommand::WorkspaceAdopt),
        "workspace" if args == "release" => Some(CanonicalSlashCommand::WorkspaceRelease),
        "workspace" if args == "archive" => Some(CanonicalSlashCommand::WorkspaceArchive),
        "workspace" if args == "prune" => Some(CanonicalSlashCommand::WorkspacePrune),
        "workspace" if args == "bind clear" => Some(CanonicalSlashCommand::WorkspaceBindClear),
        "workspace" if args == "role" => Some(CanonicalSlashCommand::WorkspaceRoleView),
        "workspace" if args == "role clear" => Some(CanonicalSlashCommand::WorkspaceRoleClear),
        "workspace" if args == "kind" => Some(CanonicalSlashCommand::WorkspaceKindView),
        "workspace" if args == "kind clear" => Some(CanonicalSlashCommand::WorkspaceKindClear),
        "workspace" => {
            if let Some(label) = args
                .strip_prefix("new ")
                .map(str::trim)
                .filter(|label| !label.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceNew(label.to_string()))
            } else if let Some(target) = args
                .strip_prefix("destroy ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceDestroy(target.to_string()))
            } else if let Some(milestone) = args
                .strip_prefix("bind milestone ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceBindMilestone(
                    milestone.to_string(),
                ))
            } else if let Some(node) = args
                .strip_prefix("bind node ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceBindNode(node.to_string()))
            } else if let Some(role) = args
                .strip_prefix("role set ")
                .and_then(crate::workspace::types::WorkspaceRole::parse)
            {
                Some(CanonicalSlashCommand::WorkspaceRoleSet(role))
            } else {
                args.strip_prefix("kind set ")
                    .and_then(crate::workspace::types::WorkspaceKind::parse)
                    .map(CanonicalSlashCommand::WorkspaceKindSet)
            }
        }
        "stats" if args.is_empty() => Some(CanonicalSlashCommand::SessionStatsView),
        "tree" => Some(CanonicalSlashCommand::TreeView {
            args: if args.is_empty() {
                "list".to_string()
            } else {
                args.to_string()
            },
        }),
        "note" if !args.is_empty() => Some(CanonicalSlashCommand::NoteAdd {
            text: args.to_string(),
        }),
        "notes" if args.is_empty() => Some(CanonicalSlashCommand::NotesView),
        "notes" if args == "clear" => Some(CanonicalSlashCommand::NotesClear),
        "checkin" if args.is_empty() => Some(CanonicalSlashCommand::CheckinView),
        "context" if args.is_empty() => None,
        "context" => {
            let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
            match sub {
                "status" => Some(CanonicalSlashCommand::ContextStatus),
                "capacity" => Some(CanonicalSlashCommand::ContextCapacity(rest.trim().into())),
                "reasoning" => Some(CanonicalSlashCommand::ContextReasoning(rest.trim().into())),
                "compact" | "compress" => Some(CanonicalSlashCommand::ContextCompact),
                "clear" | "reset" | "new" => Some(CanonicalSlashCommand::ContextClear),
                "request" => {
                    if rest.starts_with('{') {
                        match serde_json::from_str::<serde_json::Value>(rest) {
                            Ok(value)
                                if value.get("requests").and_then(|v| v.as_array()).is_some() =>
                            {
                                Some(CanonicalSlashCommand::ContextRequestJson(rest.to_string()))
                            }
                            _ => None,
                        }
                    } else {
                        let (kind, query) = rest.split_once(' ').unwrap_or((rest, ""));
                        if !kind.is_empty() && !query.trim().is_empty() {
                            Some(CanonicalSlashCommand::ContextRequest {
                                kind: kind.to_string(),
                                query: query.trim().to_string(),
                            })
                        } else {
                            None
                        }
                    }
                }
                _ => crate::settings::ContextClass::parse(sub)
                    .map(CanonicalSlashCommand::SetContextClass),
            }
        }
        "new" if args.is_empty() => Some(CanonicalSlashCommand::ContextClear),
        "sessions" if args.is_empty() => None,
        "sessions" if matches!(args, "list" | "all") => Some(CanonicalSlashCommand::ListSessions),
        "resume" if !args.is_empty() => {
            Some(CanonicalSlashCommand::ResumeSession(args.to_string()))
        }
        "sessions" if args.starts_with("resume ") => {
            let id = args.trim_start_matches("resume ").trim();
            (!id.is_empty()).then(|| CanonicalSlashCommand::ResumeSession(id.to_string()))
        }
        "auth" => match args {
            "" => Some(CanonicalSlashCommand::AuthView),
            "status" | "list" => Some(CanonicalSlashCommand::AuthStatus),
            "unlock" => Some(CanonicalSlashCommand::AuthUnlock),
            _ if args.starts_with("login ") => {
                let provider = args.trim_start_matches("login ").trim();
                (!provider.is_empty())
                    .then(|| CanonicalSlashCommand::AuthLogin(provider.to_string()))
            }
            _ if args.starts_with("logout ") => {
                let provider = args.trim_start_matches("logout ").trim();
                (!provider.is_empty())
                    .then(|| CanonicalSlashCommand::AuthLogout(provider.to_string()))
            }
            _ => None,
        },
        "connect" | "login" if !args.is_empty() => {
            Some(CanonicalSlashCommand::AuthLogin(args.to_string()))
        }
        "logout" if !args.is_empty() => Some(CanonicalSlashCommand::AuthLogout(args.to_string())),
        "skills" | "skill" => {
            if args.is_empty() || args == "list" {
                Some(CanonicalSlashCommand::SkillsView)
            } else if matches!(args, "--help" | "help" | "-h") {
                Some(CanonicalSlashCommand::SkillsHelp)
            } else if matches!(args, "reload" | "refresh") {
                Some(CanonicalSlashCommand::SkillsReload)
            } else if args == "install" {
                Some(CanonicalSlashCommand::SkillsInstall(None))
            } else if let Some(name) = args.strip_prefix("install ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::SkillsInstall(Some(name.to_string())))
            } else if args == "create" || args == "new" {
                Some(CanonicalSlashCommand::SkillCreate(None))
            } else if args == "create --project" || args == "new --project" {
                Some(CanonicalSlashCommand::SkillCreate(Some(
                    SkillCreateScope::Project,
                )))
            } else if args == "create --user" || args == "new --user" {
                Some(CanonicalSlashCommand::SkillCreate(Some(
                    SkillCreateScope::User,
                )))
            } else if let Some(path) = args.strip_prefix("import --project ") {
                let path = path.trim();
                (!path.is_empty()).then(|| CanonicalSlashCommand::SkillImport {
                    path: path.to_string(),
                    scope: Some(SkillCreateScope::Project),
                })
            } else if let Some(path) = args.strip_prefix("import --user ") {
                let path = path.trim();
                (!path.is_empty()).then(|| CanonicalSlashCommand::SkillImport {
                    path: path.to_string(),
                    scope: Some(SkillCreateScope::User),
                })
            } else if let Some(path) = args.strip_prefix("import ") {
                let path = path.trim();
                (!path.is_empty()).then(|| CanonicalSlashCommand::SkillImport {
                    path: path.to_string(),
                    scope: None,
                })
            } else if let Some(name) = args.strip_prefix("get ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::SkillGet(name.to_string()))
            } else if let Some(name) = args.strip_prefix("delete ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::SkillDelete(name.to_string()))
            } else {
                None
            }
        }
        "plan" => {
            if args.is_empty() || args == "status" {
                Some(CanonicalSlashCommand::PlanView)
            } else if args == "list" {
                Some(CanonicalSlashCommand::PlanList)
            } else if let Some(id) = args.strip_prefix("show ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::PlanShow(id.to_string()))
            } else if let Some(id) = args.strip_prefix("switch ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::PlanSwitch(id.to_string()))
            } else if let Some(id) = args.strip_prefix("resume ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::PlanResume(id.to_string()))
            } else if args == "background" {
                Some(CanonicalSlashCommand::PlanBackground(None))
            } else if let Some(id) = args.strip_prefix("background ") {
                let id = id.trim();
                Some(CanonicalSlashCommand::PlanBackground(
                    (!id.is_empty()).then(|| id.to_string()),
                ))
            } else if args == "detach" {
                Some(CanonicalSlashCommand::PlanDetach(None))
            } else if let Some(id) = args.strip_prefix("detach ") {
                let id = id.trim();
                Some(CanonicalSlashCommand::PlanDetach(
                    (!id.is_empty()).then(|| id.to_string()),
                ))
            } else if args == "promote" {
                Some(CanonicalSlashCommand::PlanPromote(None))
            } else if let Some(target) = args.strip_prefix("promote ") {
                let target = target.trim();
                Some(CanonicalSlashCommand::PlanPromote(
                    (!target.is_empty()).then(|| target.to_string()),
                ))
            } else if let Some(binding) = args.strip_prefix("bind ") {
                let binding = binding.trim();
                (!binding.is_empty()).then(|| CanonicalSlashCommand::PlanBind(binding.to_string()))
            } else if args == "ledger" {
                Some(CanonicalSlashCommand::PlanLedger(None))
            } else if let Some(id) = args.strip_prefix("ledger ") {
                let id = id.trim();
                Some(CanonicalSlashCommand::PlanLedger(
                    (!id.is_empty()).then(|| id.to_string()),
                ))
            } else if let Some(raw_items) = args.strip_prefix("set ") {
                let items = split_plan_items(raw_items);
                (!items.is_empty()).then_some(CanonicalSlashCommand::PlanSet(items))
            } else if args == "approve" {
                Some(CanonicalSlashCommand::PlanApprove)
            } else if args == "execute" || args == "exec" {
                Some(CanonicalSlashCommand::PlanExecute)
            } else if args == "advance" || args == "next" {
                Some(CanonicalSlashCommand::PlanAdvance)
            } else if args == "skip" {
                Some(CanonicalSlashCommand::PlanSkip)
            } else if args == "clear" || args == "off" {
                Some(CanonicalSlashCommand::PlanClear)
            } else {
                None
            }
        }
        "extension" | "ext" => {
            if matches!(args, "" | "list" | "view") {
                Some(CanonicalSlashCommand::ExtensionView)
            } else if let Some(name) = args.strip_prefix("init ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionInit(name.to_string()))
            } else if let Some(name) = args.strip_prefix("get ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionGet(name.to_string()))
            } else if let Some(uri) = args.strip_prefix("install ") {
                let uri = uri.trim();
                (!uri.is_empty()).then(|| CanonicalSlashCommand::ExtensionInstall(uri.to_string()))
            } else if let Some(name) = args.strip_prefix("remove ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionRemove(name.to_string()))
            } else if matches!(
                args,
                "refresh" | "reload" | "hup" | "kick" | "restart" | "hot-restart"
            ) {
                // Extension code and manifests are runtime substrates. Reload them
                // in-process so the process that owns the active harness transport
                // remains alive.
                Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
            } else if args == "update" {
                Some(CanonicalSlashCommand::ExtensionUpdate(None))
            } else if let Some(name) = args.strip_prefix("update ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::ExtensionUpdate(Some(name.to_string())))
            } else if let Some(name) = args.strip_prefix("enable ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionEnable(name.to_string()))
            } else if let Some(name) = args.strip_prefix("disable ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::ExtensionDisable(name.to_string()))
            } else if args == "search" {
                Some(CanonicalSlashCommand::ExtensionSearch(None))
            } else if let Some(query) = args.strip_prefix("search ") {
                let query = query.trim();
                Some(CanonicalSlashCommand::ExtensionSearch(
                    if query.is_empty() {
                        None
                    } else {
                        Some(query.to_string())
                    },
                ))
            } else {
                None
            }
        }
        "persona" => {
            if args == "list" {
                Some(CanonicalSlashCommand::PersonaList)
            } else {
                None // "off" and <name> are handled directly in TUI handler
            }
        }
        "armory" => {
            if args.is_empty() || args == "browse" || args == "search" || args == "list" {
                Some(CanonicalSlashCommand::ArmoryBrowse(None))
            } else if let Some(query) = args.strip_prefix("browse ") {
                let query = query.trim();
                Some(CanonicalSlashCommand::ArmoryBrowse(if query.is_empty() {
                    None
                } else {
                    Some(query.to_string())
                }))
            } else if let Some(query) = args.strip_prefix("search ") {
                let query = query.trim();
                Some(CanonicalSlashCommand::ArmoryBrowse(if query.is_empty() {
                    None
                } else {
                    Some(query.to_string())
                }))
            } else if let Some(target) = args.strip_prefix("install ") {
                let target = target.trim();
                (!target.is_empty())
                    .then(|| CanonicalSlashCommand::ArmoryInstall(target.to_string()))
            } else if args == "install" {
                None
            } else {
                Some(CanonicalSlashCommand::ArmoryBrowse(Some(args.to_string())))
            }
        }
        "catalog" => {
            if args.is_empty() || args == "list" {
                Some(CanonicalSlashCommand::CatalogView)
            } else if args == "install" {
                Some(CanonicalSlashCommand::CatalogInstall)
            } else if let Some(id) = args.strip_prefix("remove ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::CatalogRemove(id.to_string()))
            } else {
                None
            }
        }
        "plugin" => {
            if args.is_empty() || args == "list" {
                Some(CanonicalSlashCommand::PluginView)
            } else if let Some(uri) = args.strip_prefix("install ") {
                let uri = uri.trim();
                (!uri.is_empty()).then(|| CanonicalSlashCommand::PluginInstall(uri.to_string()))
            } else if let Some(name) = args.strip_prefix("remove ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::PluginRemove(name.to_string()))
            } else if args == "update" {
                Some(CanonicalSlashCommand::PluginUpdate(None))
            } else if let Some(name) = args.strip_prefix("update ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::PluginUpdate(Some(name.to_string())))
            } else {
                None
            }
        }

        "variables" | "vars" => {
            let args = args.trim();
            if args.is_empty() || matches!(args, "list" | "status") {
                Some(CanonicalSlashCommand::VariablesView)
            } else if let Some(rest) = args.strip_prefix("set").and_then(strip_command_separator) {
                let (name, value) = split_name_and_remainder(rest)?;
                Some(CanonicalSlashCommand::VariablesSet {
                    name: name.to_string(),
                    value: value.to_string(),
                })
            } else if let Some(rest) = args.strip_prefix("get").and_then(strip_command_separator) {
                single_name(rest).map(|name| CanonicalSlashCommand::VariablesGet(name.to_string()))
            } else if let Some(rest) = ["delete", "remove", "rm"]
                .into_iter()
                .find_map(|verb| args.strip_prefix(verb).and_then(strip_command_separator))
            {
                single_name(rest)
                    .map(|name| CanonicalSlashCommand::VariablesDelete(name.to_string()))
            } else {
                None
            }
        }
        "secrets" => {
            let parts: Vec<&str> = args.splitn(3, ' ').collect();
            match parts.first().copied().unwrap_or("") {
                "" | "list" | "status" => Some(CanonicalSlashCommand::SecretsView),
                "set" if parts.len() >= 3 && !parts[1].trim().is_empty() => {
                    let value = parts[2].trim();
                    (value.starts_with("env:")
                        || value.starts_with("cmd:")
                        || value.starts_with("vault:"))
                    .then(|| CanonicalSlashCommand::SecretsSet {
                        name: parts[1].trim().to_string(),
                        value: value.to_string(),
                    })
                }
                "get" if parts.len() >= 2 && !parts[1].trim().is_empty() => Some(
                    CanonicalSlashCommand::SecretsGet(parts[1].trim().to_string()),
                ),
                "delete" | "remove" | "rm" if parts.len() >= 2 && !parts[1].trim().is_empty() => {
                    Some(CanonicalSlashCommand::SecretsDelete(
                        parts[1].trim().to_string(),
                    ))
                }
                _ => None,
            }
        }
        "vault" => match args {
            "" | "status" => Some(CanonicalSlashCommand::VaultStatus),
            "configure" => Some(CanonicalSlashCommand::VaultConfigure),
            "init-policy" => Some(CanonicalSlashCommand::VaultInitPolicy),
            _ => None,
        },
        "cleave" => {
            if args.is_empty() || args == "status" {
                Some(CanonicalSlashCommand::CleaveStatus)
            } else if let Some(label) = args.strip_prefix("cancel ") {
                let label = label.trim();
                (!label.is_empty())
                    .then(|| CanonicalSlashCommand::CleaveCancelChild(label.to_string()))
            } else {
                None
            }
        }
        "delegate" | "subagent" => match args {
            "" | "status" => Some(CanonicalSlashCommand::DelegateStatus),
            _ => None,
        },
        "smoke" => {
            crate::smoke_surface::parse_smoke_command(args).map(CanonicalSlashCommand::Smoke)
        }
        _ => None,
    }
}

fn strip_command_separator(value: &str) -> Option<&str> {
    value
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
        .then(|| value.trim_start())
}

fn split_name_and_remainder(value: &str) -> Option<(&str, &str)> {
    let separator = value.find(char::is_whitespace)?;
    let name = &value[..separator];
    let remainder = value[separator..].trim();
    (!name.is_empty() && !remainder.is_empty()).then_some((name, remainder))
}

fn single_name(value: &str) -> Option<&str> {
    let mut parts = value.split_whitespace();
    let name = parts.next()?;
    (parts.next().is_none()).then_some(name)
}

fn split_plan_items(raw: &str) -> Vec<String> {
    raw.split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod variable_command_tests {
    use super::*;

    #[test]
    fn variables_parser_accepts_general_whitespace_and_preserves_value_words() {
        assert!(matches!(
            canonical_slash_command("vars", "set\tPROJECT_ENV\tlocal dev"),
            Some(CanonicalSlashCommand::VariablesSet { name, value })
                if name == "PROJECT_ENV" && value == "local dev"
        ));
    }

    #[test]
    fn variables_parser_rejects_trailing_arguments_for_single_name_commands() {
        assert!(canonical_slash_command("vars", "get ONE TWO").is_none());
        assert!(canonical_slash_command("variables", "delete ONE TWO").is_none());
    }

    #[test]
    fn runtime_doctor_aliases_and_replace_are_canonical() {
        assert_eq!(
            canonical_slash_command("doctor", ""),
            Some(CanonicalSlashCommand::RuntimeDoctor)
        );
        assert_eq!(
            canonical_slash_command("runtime", "doctor"),
            Some(CanonicalSlashCommand::RuntimeDoctor)
        );
        assert_eq!(
            canonical_slash_command("runtime", "replace omegon-codescan"),
            Some(CanonicalSlashCommand::RuntimeExtensionReplace(
                "omegon-codescan".into()
            ))
        );
        assert!(canonical_slash_command("runtime", "replace ").is_none());
    }

    #[test]
    fn profile_component_commands_are_canonical() {
        assert_eq!(
            canonical_slash_command("profile", "component enable core:codescan"),
            Some(CanonicalSlashCommand::ProfileComponentEnable(
                "core:codescan".into()
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "component disable core:codescan"),
            Some(CanonicalSlashCommand::ProfileComponentDisable(
                "core:codescan".into()
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "components view"),
            Some(CanonicalSlashCommand::ProfileComponentsView)
        );
        assert!(canonical_slash_command("profile", "component enable").is_none());
        assert!(canonical_slash_command("profile", "components disable core:codescan").is_none());
    }
}

use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand, ValueEnum, error::ErrorKind};
use omegon_maintenance_contracts::{
    ArtifactIdentityV1, AuthorityKey, CleanupCapability, CompositionIdentityV1, ContributionKind,
    ContributionSelector, DeadlineEvidenceV1, DiagnosticV1, ErrorV1, FileIdentityV1,
    LifecycleBoundary, ListScope, MaintenanceResultV1, OwnershipRecordV1, ResultStatus,
    SCHEMA_VERSION, Severity, canonical_json, derive_key, entry_key, file_identity,
    normalize_workspace_path, parse_record, path_key, resolve_list_scope, scope_key,
    validate_child_name, workspace_key,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod audit;
mod home_recovery;
mod mutation;
mod release;

const MAX_METADATA_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const SUPPORTED_TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
];
const EXCLUSIONS: &[&str] = &[
    "default_loop",
    "extension_runtime",
    "lifecycle",
    "mcp",
    "memory",
    "mutable_packs",
    "orchestration",
    "project_config",
    "project_contributions",
    "provider_clients",
    "tui",
];

const fn build_version() -> &'static str {
    concat!(
        env!("CARGO_PKG_VERSION"),
        " (",
        env!("OMEGON_MAINTAIN_GIT_SHA"),
        ")",
    )
}

#[derive(Parser)]
#[command(name = "omegon-maintain", version = build_version(), about)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true, value_parser = parse_duration)]
    deadline: Option<Duration>,
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[arg(long, global = true)]
    config_home: Option<PathBuf>,
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[arg(long, global = true)]
    dry_run: bool,
    #[arg(long, global = true)]
    request_id: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Identity,
    Doctor,
    Home {
        #[command(subcommand)]
        command: HomeCommand,
    },
    Composition {
        #[command(subcommand)]
        command: CompositionCommand,
    },
    Contribution {
        #[command(subcommand)]
        command: ContributionCommand,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Resource {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
}

#[derive(Subcommand)]
enum HomeCommand {
    /// Inspect installation identity without changing authority records.
    Inspect,
    /// Recover a changed home binding while preserving maintenance policy.
    Recover,
}

#[derive(Subcommand)]
enum CompositionCommand {
    Inspect,
}

#[derive(Clone, Copy, ValueEnum)]
enum ScopeArg {
    User,
    Project,
}

#[derive(Subcommand)]
enum ContributionCommand {
    List {
        #[arg(long)]
        scope: Option<ScopeArg>,
        #[arg(long)]
        cursor: Option<String>,
    },
    Inspect {
        selector: String,
        #[arg(long)]
        scope: ScopeArg,
    },
    Disable {
        selector: String,
        #[arg(long)]
        scope: ScopeArg,
    },
    Quarantine {
        selector: String,
        #[arg(long)]
        scope: ScopeArg,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    List {
        #[arg(long)]
        cursor: Option<String>,
    },
    Inspect {
        session_id: String,
    },
    Quarantine {
        session_id: String,
    },
}

#[derive(Subcommand)]
enum ResourceCommand {
    List {
        #[arg(long)]
        cursor: Option<String>,
    },
    PruneStale,
}

#[derive(Subcommand)]
enum ReleaseCommand {
    Inspect,
    Verify {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        extracted_root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AuditCommand {
    Inspect {
        #[arg(long)]
        cursor: Option<String>,
    },
    Verify {
        #[arg(long)]
        cursor: Option<String>,
    },
}

struct Context {
    home: AdmittedRoot,
    config_home: AdmittedRoot,
    workspace: Option<AdmittedRoot>,
    started: Instant,
    deadline: Duration,
}

struct AdmittedRoot {
    path: PathBuf,
    file: File,
    device: u64,
    inode: u64,
    unsafe_permissions: bool,
}

pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
    let args: Vec<OsString> = args.into_iter().collect();
    let requested_json = args.iter().any(|arg| arg == "--json");
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return 0;
            }
            if !requested_json {
                let _ = error.print();
                return 1;
            }
            let mut result = base_result("cli", Uuid::new_v4().to_string(), Duration::ZERO, None);
            fail(
                &mut result,
                "cli_invalid",
                "parse",
                true,
                &error.to_string(),
            );
            return emit(&result, true);
        }
    };

    let request_id = match cli.request_id.as_deref() {
        Some(value) if Uuid::parse_str(value).is_ok() => value.to_owned(),
        Some(_) => {
            let mut result = base_result("cli", Uuid::new_v4().to_string(), Duration::ZERO, None);
            fail(
                &mut result,
                "cli_invalid_request_id",
                "admission",
                true,
                "request ID must be a UUID",
            );
            return emit(&result, cli.json);
        }
        None => Uuid::new_v4().to_string(),
    };

    let command_name = command_name(&cli.command);
    let started = Instant::now();
    if is_mutation(&cli.command) && cli.deadline.is_none() {
        let mut result = base_result(command_name, request_id, Duration::ZERO, None);
        fail(
            &mut result,
            "deadline_required",
            "admission",
            true,
            "mutation commands require an explicit --deadline",
        );
        finalize(&mut result);
        return emit(&result, cli.json);
    }
    let deadline = cli.deadline.unwrap_or_else(|| {
        if matches!(
            cli.command,
            Command::Release {
                command: ReleaseCommand::Verify { .. }
            }
        ) {
            Duration::from_secs(300)
        } else {
            Duration::from_secs(30)
        }
    });
    let mut result = base_result(
        command_name,
        request_id,
        deadline,
        Some((started, deadline)),
    );
    if deadline.is_zero() || deadline > Duration::from_secs(600) {
        fail(
            &mut result,
            "deadline_invalid",
            "admission",
            true,
            "deadline must be greater than zero and at most 10 minutes",
        );
        return emit(&result, cli.json);
    }
    if cli.dry_run && !is_mutation(&cli.command) {
        fail(
            &mut result,
            "cli_dry_run_invalid",
            "admission",
            true,
            "--dry-run is valid only for mutation commands",
        );
        finalize(&mut result);
        return emit(&result, cli.json);
    }
    if let Command::Release {
        command:
            ReleaseCommand::Verify {
                archive,
                manifest,
                bundle,
                extracted_root,
            },
    } = &cli.command
    {
        release::verify_release(
            archive,
            manifest,
            bundle,
            extracted_root.as_deref(),
            started,
            deadline,
            &mut result,
        );
        settle_deadline(&mut result, started, deadline);
        finalize(&mut result);
        return emit(&result, cli.json);
    }
    if matches!(
        &cli.command,
        Command::Identity | Command::Composition { .. }
    ) {
        match &cli.command {
            Command::Identity => identity_diagnostics(&mut result),
            Command::Composition { .. } => composition_diagnostics(&mut result),
            _ => unreachable!(),
        }
        settle_deadline(&mut result, started, deadline);
        finalize(&mut result);
        return emit(&result, cli.json);
    }

    if admission_deadline_expired(&mut result, started, deadline) {
        return emit(&result, cli.json);
    }
    let home_path = match resolve_home(cli.home) {
        Ok(path) => path,
        Err(message) => {
            fail(
                &mut result,
                "root_home_invalid",
                "admission",
                true,
                &message,
            );
            return emit(&result, cli.json);
        }
    };
    let config_path = match resolve_config_home(cli.config_home) {
        Ok(path) => path,
        Err(message) => {
            fail(
                &mut result,
                "root_config_invalid",
                "admission",
                true,
                &message,
            );
            return emit(&result, cli.json);
        }
    };
    let workspace_path = match cli.workspace.map(validate_absolute_root).transpose() {
        Ok(path) => path,
        Err(message) => {
            fail(
                &mut result,
                "root_workspace_invalid",
                "admission",
                true,
                &message,
            );
            return emit(&result, cli.json);
        }
    };
    if admission_deadline_expired(&mut result, started, deadline) {
        return emit(&result, cli.json);
    }
    let home = match admit_root(home_path) {
        Ok(root) => root,
        Err(message) => {
            fail(
                &mut result,
                "root_home_invalid",
                "admission",
                true,
                &message,
            );
            return emit(&result, cli.json);
        }
    };
    if admission_deadline_expired(&mut result, started, deadline) {
        return emit(&result, cli.json);
    }
    let config_home = match admit_root(config_path) {
        Ok(root) => root,
        Err(message) => {
            fail(
                &mut result,
                "root_config_invalid",
                "admission",
                true,
                &message,
            );
            return emit(&result, cli.json);
        }
    };
    if admission_deadline_expired(&mut result, started, deadline) {
        return emit(&result, cli.json);
    }
    let workspace = match workspace_path.map(admit_root).transpose() {
        Ok(root) => root,
        Err(message) => {
            fail(
                &mut result,
                "root_workspace_invalid",
                "admission",
                true,
                &message,
            );
            return emit(&result, cli.json);
        }
    };
    if aliases(&home, &config_home)
        || workspace
            .as_ref()
            .is_some_and(|root| aliases(root, &home) || aliases(root, &config_home))
    {
        fail(
            &mut result,
            "root_alias_rejected",
            "admission",
            true,
            "granted roots resolve to the same directory identity",
        );
        return emit(&result, cli.json);
    }
    let context = Context {
        home,
        config_home,
        workspace,
        started,
        deadline,
    };
    diagnose_unsafe_roots(&context, &mut result);

    dispatch(&cli.command, &context, cli.dry_run, &mut result);
    settle_deadline(&mut result, started, deadline);
    finalize(&mut result);
    emit(&result, cli.json)
}

fn dispatch(command: &Command, context: &Context, dry_run: bool, result: &mut MaintenanceResultV1) {
    match command {
        Command::Identity => identity_diagnostics(result),
        Command::Doctor => doctor(context, result),
        Command::Home {
            command: home_command,
        } => {
            if matches!(home_command, HomeCommand::Inspect)
                || validate_mutation_admission(command, context, result)
            {
                home_recovery::execute(home_command, context, dry_run, result);
            }
        }
        Command::Composition { .. } => composition_diagnostics(result),
        Command::Contribution {
            command: ContributionCommand::List { scope, cursor },
        } => {
            contribution_list(context, *scope, None, result);
            let query = contribution_list_query(context, *scope);
            stage_list_page(result, query, cursor.as_deref());
        }
        Command::Contribution {
            command: ContributionCommand::Inspect { selector, scope },
        } => contribution_list(context, Some(*scope), Some(selector), result),
        Command::Session {
            command: SessionCommand::List { cursor },
        } => {
            session_diagnostics(context, None, result);
            let query = session_list_query(context);
            stage_list_page(result, query, cursor.as_deref());
        }
        Command::Session {
            command: SessionCommand::Inspect { session_id },
        } => session_diagnostics(context, Some(session_id), result),
        Command::Resource {
            command: ResourceCommand::List { cursor },
        } => {
            resource_diagnostics(context, result);
            let query = resource_list_query(context);
            stage_list_page(result, query, cursor.as_deref());
        }
        Command::Release {
            command: ReleaseCommand::Inspect,
        } => release_inspect(result),
        Command::Audit { command } => audit::execute(command, context, result),
        command if is_mutation(command) => {
            if validate_mutation_admission(command, context, result) {
                mutation::execute(command, context, dry_run, result);
            }
        }
        _ => fail(
            result,
            "cli_unsupported_slice_zero_operation",
            "admission",
            true,
            "operation is reserved for task 0.6 and performed no target mutation",
        ),
    }
}

fn validate_mutation_admission(
    command: &Command,
    context: &Context,
    result: &mut MaintenanceResultV1,
) -> bool {
    let valid_target = match command {
        Command::Contribution {
            command:
                ContributionCommand::Disable { selector, scope }
                | ContributionCommand::Quarantine { selector, scope },
        } => {
            if selector.parse::<ContributionSelector>().is_err() {
                fail(
                    result,
                    "record_selector_invalid",
                    "admission",
                    true,
                    "contribution selector is not canonical",
                );
                false
            } else if matches!(scope, ScopeArg::Project) && context.workspace.is_none() {
                fail(
                    result,
                    "root_workspace_required",
                    "admission",
                    true,
                    "project scope requires --workspace",
                );
                false
            } else {
                true
            }
        }
        Command::Session {
            command: SessionCommand::Quarantine { session_id },
        } => {
            if context.workspace.is_none() {
                fail(
                    result,
                    "root_workspace_required",
                    "admission",
                    true,
                    "session quarantine requires --workspace",
                );
                false
            } else if !canonical_session_id(session_id) {
                fail(
                    result,
                    "session_id_invalid",
                    "admission",
                    true,
                    "session ID is not canonical",
                );
                false
            } else {
                true
            }
        }
        Command::Resource {
            command: ResourceCommand::PruneStale,
        } if context.workspace.is_none() => {
            fail(
                result,
                "root_workspace_required",
                "admission",
                true,
                "resource pruning requires --workspace",
            );
            false
        }
        _ => true,
    };
    if valid_target
        && [&context.home, &context.config_home]
            .into_iter()
            .chain(context.workspace.as_ref())
            .any(|root| root.unsafe_permissions)
    {
        fail(
            result,
            "root_permissions_unsafe",
            "admission",
            true,
            "mutation commands reject group/other-writable roots",
        );
        return false;
    }
    valid_target
}

fn identity_diagnostics(result: &mut MaintenanceResultV1) {
    diagnostic(
        result,
        "record_artifact_identity",
        Severity::Info,
        "artifact",
        "maintenance artifact identity is available",
        Some(json!({
            "version": result.artifact.version,
            "commit": result.artifact.commit,
            "target": result.artifact.target,
            "digest": result.artifact.digest,
            "protocol_version": SCHEMA_VERSION,
            "supported_targets": SUPPORTED_TARGETS,
            "archive_formats": ["tar.gz"],
            "limits": {
                "metadata_bytes": MAX_METADATA_BYTES,
                "symlink_text_bytes": 4096,
                "inventory_entries": MAX_ENTRIES,
                "output_bytes": MAX_OUTPUT_BYTES,
            },
            "release_verification": "offline_bundle_v0_3",
        })),
    );
}

fn composition_diagnostics(result: &mut MaintenanceResultV1) {
    diagnostic(
        result,
        "record_compiled_composition",
        Severity::Info,
        "composition",
        "compiled maintenance profile excludes normal runtime inputs",
        Some(json!({
            "profile": result.composition.profile,
            "generation": result.composition.generation,
            "excluded_inputs": result.composition.excluded_inputs,
        })),
    );
}

fn doctor(context: &Context, result: &mut MaintenanceResultV1) {
    identity_diagnostics(result);
    composition_diagnostics(result);
    for (scope, root) in [
        ("home", &context.home),
        ("config_home", &context.config_home),
    ] {
        diagnostic(
            result,
            "root_diagnostic",
            Severity::Info,
            scope,
            if root.unsafe_permissions {
                "root permissions permit writes by group or other users"
            } else {
                "root is not group/other-writable"
            },
            Some(json!({"device": root.device, "inode": root.inode})),
        );
        if root.unsafe_permissions {
            result.status = ResultStatus::Degraded;
        }
    }
    if let Some(workspace) = &context.workspace {
        diagnostic(
            result,
            "root_diagnostic",
            Severity::Info,
            "workspace",
            if workspace.unsafe_permissions {
                "workspace exists with broad permissions"
            } else {
                "workspace exists"
            },
            None,
        );
    }
}

fn diagnose_unsafe_roots(context: &Context, result: &mut MaintenanceResultV1) {
    for (scope, root) in [
        ("home", &context.home),
        ("config_home", &context.config_home),
    ]
    .into_iter()
    .chain(context.workspace.as_ref().map(|root| ("workspace", root)))
    {
        if root.unsafe_permissions {
            diagnostic(
                result,
                "root_permissions_unsafe",
                Severity::Warning,
                scope,
                "root permissions permit writes by group or other users",
                Some(json!({"device": root.device, "inode": root.inode})),
            );
            result.status = ResultStatus::Degraded;
        }
    }
}

#[derive(Clone, Copy)]
struct ContributionRoot {
    kind: ContributionKind,
    scope: ScopeArg,
    suffix: Option<&'static str>,
    path_suffix: &'static str,
}

const CONTRIBUTION_ROOTS: &[ContributionRoot] = &[
    ContributionRoot {
        kind: ContributionKind::Extension,
        scope: ScopeArg::User,
        suffix: None,
        path_suffix: "extensions",
    },
    ContributionRoot {
        kind: ContributionKind::Plugin,
        scope: ScopeArg::User,
        suffix: None,
        path_suffix: "plugins",
    },
    ContributionRoot {
        kind: ContributionKind::Skill,
        scope: ScopeArg::User,
        suffix: None,
        path_suffix: "skills",
    },
    ContributionRoot {
        kind: ContributionKind::Prompt,
        scope: ScopeArg::User,
        suffix: Some(".md"),
        path_suffix: "prompts",
    },
    ContributionRoot {
        kind: ContributionKind::Catalog,
        scope: ScopeArg::User,
        suffix: None,
        path_suffix: "catalog",
    },
    ContributionRoot {
        kind: ContributionKind::Plugin,
        scope: ScopeArg::Project,
        suffix: None,
        path_suffix: ".omegon/plugins",
    },
    ContributionRoot {
        kind: ContributionKind::Skill,
        scope: ScopeArg::Project,
        suffix: None,
        path_suffix: ".omegon/skills",
    },
    ContributionRoot {
        kind: ContributionKind::Prompt,
        scope: ScopeArg::Project,
        suffix: Some(".md"),
        path_suffix: ".omegon/prompts",
    },
    ContributionRoot {
        kind: ContributionKind::Workflow,
        scope: ScopeArg::Project,
        suffix: Some(".toml"),
        path_suffix: ".omegon/workflows",
    },
];

fn contribution_list(
    context: &Context,
    requested_scope: Option<ScopeArg>,
    selector_filter: Option<&str>,
    result: &mut MaintenanceResultV1,
) {
    if let Some(selector) = selector_filter
        && selector.parse::<ContributionSelector>().is_err()
    {
        fail(
            result,
            "record_selector_invalid",
            "admission",
            true,
            "contribution selector is not canonical",
        );
        return;
    }
    if matches!(requested_scope, Some(ScopeArg::Project)) && context.workspace.is_none() {
        fail(
            result,
            "root_workspace_required",
            "admission",
            true,
            "project scope requires --workspace",
        );
        return;
    }
    let list_scope = if selector_filter.is_none() {
        match resolve_list_scope(requested_scope.map(scope_name), context.workspace.is_some()) {
            Ok(scope) => Some(scope),
            Err(error) => {
                fail(
                    result,
                    "cli_scope_invalid",
                    "admission",
                    true,
                    &error.to_string(),
                );
                return;
            }
        }
    } else {
        None
    };
    let scopes: &[ScopeArg] = match list_scope {
        Some(ListScope::User) => &[ScopeArg::User],
        Some(ListScope::Project) => &[ScopeArg::Project],
        Some(ListScope::UserAndProject) => &[ScopeArg::User, ScopeArg::Project],
        None => match requested_scope.expect("inspect requires scope") {
            ScopeArg::User => &[ScopeArg::User],
            ScopeArg::Project => &[ScopeArg::Project],
        },
    };
    let mut found = Vec::new();
    let mut examined = 0_usize;
    for root in CONTRIBUTION_ROOTS {
        if !scopes.iter().any(|scope| same_scope(*scope, root.scope)) {
            continue;
        }
        if context.expired() {
            degrade(
                result,
                "deadline_expired",
                "deadline expired during contribution scan",
            );
            return;
        }
        let base = match root.scope {
            ScopeArg::User => &context.home,
            ScopeArg::Project => context.workspace.as_ref().expect("project scope admitted"),
        };
        let parent = match open_dir_at(&base.file, root.path_suffix, context) {
            Ok(Some(parent)) => parent,
            Ok(None) => continue,
            Err(error) => {
                diagnostic(
                    result,
                    "record_contribution_root_unreadable",
                    Severity::Warning,
                    scope_name(root.scope),
                    &format!("contribution root is unreadable: {error}"),
                    None,
                );
                result.status = ResultStatus::Degraded;
                continue;
            }
        };
        let parent_path = match descriptor_path(&parent) {
            Ok(path) => path,
            Err(error) => {
                diagnostic(
                    result,
                    "record_contribution_root_unresolved",
                    Severity::Warning,
                    scope_name(root.scope),
                    &format!("contribution root identity is unavailable: {error}"),
                    None,
                );
                result.status = ResultStatus::Degraded;
                continue;
            }
        };
        let entries = match read_dir_at(&parent, context, MAX_ENTRIES - examined) {
            Ok(entries) => entries,
            Err(error) => {
                if error == "entry limit exceeded" {
                    fail(
                        result,
                        "limit_entries_exceeded",
                        "inventory",
                        true,
                        "contribution inventory exceeds 10000 entries",
                    );
                    return;
                }
                diagnostic(
                    result,
                    "record_contribution_root_unreadable",
                    Severity::Warning,
                    scope_name(root.scope),
                    &format!("contribution root is unreadable: {error}"),
                    None,
                );
                result.status = ResultStatus::Degraded;
                continue;
            }
        };
        examined += entries.len();
        for entry in entries {
            let raw_name = &entry.name;
            if raw_name == b".omegon-maintain-quarantine" {
                continue;
            }
            let (logical_name, force_opaque) = match strip_suffix(raw_name, root.suffix) {
                Some(name) => (name, false),
                None => (raw_name.as_slice(), true),
            };
            let selector = contribution_selector(
                root.kind,
                root.scope,
                &parent_path,
                logical_name,
                raw_name,
                force_opaque,
            );
            if selector_filter.is_some_and(|filter| filter != selector) {
                continue;
            }
            let type_name = match entry.kind {
                EntryType::Symlink => "symlink",
                EntryType::Directory => "directory",
                EntryType::File => "file",
                EntryType::Other => "other",
            };
            let link_text = if type_name == "symlink" {
                match read_link_at(&parent, raw_name) {
                    Ok(value) => Some(value),
                    Err(error) => {
                        diagnostic(
                            result,
                            "record_contribution_link_unreadable",
                            Severity::Warning,
                            scope_name(root.scope),
                            &format!("symlink text is unreadable: {error}"),
                            None,
                        );
                        result.status = ResultStatus::Degraded;
                        None
                    }
                }
            } else {
                None
            };
            found.push((
                root.kind,
                root.scope,
                raw_name.to_vec(),
                selector,
                type_name,
                link_text,
            ));
        }
    }
    found.sort_by(|left, right| {
        (left.0.as_str(), scope_order(left.1), &left.2).cmp(&(
            right.0.as_str(),
            scope_order(right.1),
            &right.2,
        ))
    });
    let matched = found.len();
    for (kind, scope, _, selector, type_name, link_text) in found {
        diagnostic(
            result,
            "record_contribution_entry",
            Severity::Info,
            scope_name(scope),
            &format!("{selector} is an inert {type_name} entry"),
            Some(json!({
                "kind": kind.as_str(),
                "selector": selector,
                "file_type": type_name,
                "link_text": link_text,
            })),
        );
    }
    if selector_filter.is_some() && matched != 1 {
        fail(
            result,
            "record_contribution_not_unique",
            "inspect",
            true,
            if matched == 0 {
                "contribution selector was not found"
            } else {
                "contribution selector was ambiguous"
            },
        );
    }
}

fn contribution_selector(
    kind: ContributionKind,
    scope: ScopeArg,
    parent: &Path,
    logical_name: &[u8],
    authority_name: &[u8],
    force_opaque: bool,
) -> String {
    if !force_opaque
        && let Ok(name) = std::str::from_utf8(logical_name)
        && !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return format!("{}:{name}", kind.as_str());
    }
    let parent_bytes = os_bytes(parent.as_os_str());
    let parent_key = path_key("unix", parent_bytes);
    let scope_key = scope_key(kind.as_str(), scope_name(scope), parent_key);
    format!(
        "entry:sha256:{}",
        entry_key(kind.as_str(), scope_key, authority_name)
    )
}

#[derive(Deserialize)]
struct SessionMeta {
    session_id: String,
    cwd: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCatalog {
    catalog_schema_version: u16,
    session_id: String,
    workspace_identity: String,
    metadata_revision: u64,
    friendly_name: Option<String>,
    description: Option<String>,
    created_at: String,
    turns: u64,
    tool_calls: u64,
    last_prompt_snippet: Option<String>,
    lineage: String,
    availability: String,
    semantic_frontier: Option<SessionCatalogFrontier>,
    source_selection: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCatalogFrontier {
    stream_id: String,
    sequence: u64,
    event_id: String,
}

fn session_diagnostics(
    context: &Context,
    selected_id: Option<&str>,
    result: &mut MaintenanceResultV1,
) {
    let workspace = match context.workspace.as_ref() {
        Some(root) => match normalize_workspace_path(os_bytes(root.path.as_os_str())) {
            Ok(path) => Some(path),
            Err(error) => {
                fail(
                    result,
                    "session_workspace_invalid",
                    "admission",
                    true,
                    &error.to_string(),
                );
                return;
            }
        },
        None if selected_id.is_some() => {
            fail(
                result,
                "root_workspace_required",
                "admission",
                true,
                "session inspect requires --workspace",
            );
            return;
        }
        None => None,
    };
    let sessions_root = match open_dir_at(&context.config_home.file, "sessions", context) {
        Ok(Some(root)) => root,
        Ok(None) => {
            if selected_id.is_some() {
                fail(
                    result,
                    "session_not_found",
                    "inspect",
                    true,
                    "session root is absent",
                );
            }
            return;
        }
        Err(error) => {
            diagnostic(
                result,
                "session_root_unreadable",
                Severity::Warning,
                "session",
                &format!("session root is unreadable: {error}"),
                None,
            );
            result.status = ResultStatus::Degraded;
            return;
        }
    };
    let mut workspace_dirs = match read_dir_at(&sessions_root, context, MAX_ENTRIES) {
        Ok(entries) => entries,
        Err(error) => {
            fail(
                result,
                "limit_entries_exceeded",
                "inventory",
                true,
                &format!("session inventory cannot be bounded: {error}"),
            );
            return;
        }
    };
    workspace_dirs.sort_by(|left, right| left.name.cmp(&right.name));
    let mut examined = workspace_dirs.len();
    let mut matches = 0_usize;
    for directory in workspace_dirs {
        if directory.kind != EntryType::Directory {
            diagnostic(
                result,
                "session_directory_invalid",
                Severity::Warning,
                "session",
                "session workspace entry is not a directory",
                Some(json!({"quarantine_available": false})),
            );
            result.status = ResultStatus::Degraded;
            continue;
        }
        let directory = match open_child_dir_at(&sessions_root, &directory.name, context) {
            Ok(Some(directory)) => directory,
            Ok(None) => continue,
            Err(error) => {
                diagnostic(
                    result,
                    "session_directory_unreadable",
                    Severity::Warning,
                    "session",
                    &format!("session directory is unreadable: {error}"),
                    None,
                );
                result.status = ResultStatus::Degraded;
                continue;
            }
        };
        let mut entries = match read_dir_at(&directory, context, MAX_ENTRIES - examined) {
            Ok(entries) => entries,
            Err(error) => {
                fail(
                    result,
                    "limit_entries_exceeded",
                    "inventory",
                    true,
                    &format!("session inventory cannot be bounded: {error}"),
                );
                return;
            }
        };
        examined += entries.len();
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let mut catalog_ids = BTreeSet::new();
        for entry in &entries {
            let Ok(name) = std::str::from_utf8(&entry.name) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".catalog.v1.json") else {
                continue;
            };
            catalog_ids.insert(id.to_string());
            if selected_id.is_some_and(|selected| selected != id) {
                continue;
            }
            match inspect_session_catalog(
                &directory,
                &entry.name,
                id,
                workspace.as_deref(),
                context,
            ) {
                Ok(Some(evidence)) => {
                    matches += 1;
                    diagnostic(
                        result,
                        "session_semantic_valid",
                        Severity::Info,
                        "session",
                        &format!("semantic session {id} catalog framing is valid"),
                        Some(evidence.evidence),
                    );
                }
                Ok(None) => {}
                Err(message) => {
                    diagnostic(
                        result,
                        "session_semantic_invalid",
                        Severity::Warning,
                        "session",
                        &message,
                        Some(json!({"quarantine_available": false})),
                    );
                    result.status = ResultStatus::Degraded;
                }
            }
        }
        for entry in &entries {
            let Ok(name) = std::str::from_utf8(&entry.name) else {
                diagnostic(
                    result,
                    "session_entry_invalid",
                    Severity::Warning,
                    "session",
                    "session entry name is not UTF-8",
                    Some(json!({"quarantine_available": false})),
                );
                result.status = ResultStatus::Degraded;
                continue;
            };
            let Some(id) = name.strip_suffix(".meta.json") else {
                continue;
            };
            if catalog_ids.contains(id) || selected_id.is_some_and(|selected| selected != id) {
                continue;
            }
            match inspect_session_pair(&directory, &entry.name, id, workspace.as_deref(), context) {
                Ok(Some(evidence)) => {
                    matches += 1;
                    diagnostic(
                        result,
                        "session_pair_valid",
                        Severity::Info,
                        "session",
                        &format!("session {id} framing is valid"),
                        Some(evidence.evidence),
                    );
                }
                Ok(None) => {}
                Err(message) => {
                    diagnostic(
                        result,
                        "session_pair_invalid",
                        Severity::Warning,
                        "session",
                        &message,
                        Some(json!({"quarantine_available": false})),
                    );
                    result.status = ResultStatus::Degraded;
                }
            }
        }
        for entry in &entries {
            let Ok(name) = std::str::from_utf8(&entry.name) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            if name.ends_with(".catalog.v1.json")
                || id.ends_with(".meta")
                || id.ends_with(".authority.snapshot")
                || catalog_ids.contains(id)
                || entries
                    .iter()
                    .any(|candidate| candidate.name == format!("{id}.meta.json").as_bytes())
                || selected_id.is_some_and(|selected| selected != id)
            {
                continue;
            }
            diagnostic(
                result,
                "session_pair_invalid",
                Severity::Warning,
                "session",
                &format!("session {id} snapshot has no metadata pair"),
                Some(json!({"quarantine_available": false})),
            );
            result.status = ResultStatus::Degraded;
        }
    }
    if selected_id.is_some() && matches != 1 {
        fail(
            result,
            "session_not_unique",
            "inspect",
            true,
            if matches == 0 {
                "session catalog or legacy pair was not found"
            } else {
                "session ID matched multiple workspace records"
            },
        );
    }
}

fn inspect_session_catalog(
    directory: &File,
    catalog_name: &[u8],
    filename_id: &str,
    workspace: Option<&[u8]>,
    context: &Context,
) -> Result<Option<InspectedSessionCatalog>, String> {
    if !canonical_session_id(filename_id)
        || catalog_name != format!("{filename_id}.catalog.v1.json").as_bytes()
    {
        return Err("semantic session catalog filename is not canonical".into());
    }
    let catalog_bytes =
        read_bounded_regular_at(directory, catalog_name, MAX_METADATA_BYTES, context)?;
    let catalog: SessionCatalog = serde_json::from_slice(&catalog_bytes.bytes)
        .map_err(|error| format!("semantic session catalog is malformed: {error}"))?;
    if catalog.catalog_schema_version != 1 {
        return Err(format!(
            "semantic session catalog schema version {} is unsupported",
            catalog.catalog_schema_version
        ));
    }
    if catalog.session_id != filename_id {
        return Err("semantic session catalog ID does not match filename".into());
    }
    if catalog.metadata_revision == 0
        || catalog.created_at.trim().is_empty()
        || catalog.source_selection != "semantic_authority_plus_host_stores"
        || !matches!(
            (catalog.lineage.as_str(), catalog.availability.as_str()),
            ("legacy", "legacy_compatibility") | ("mixed", "exact_suffix") | ("full", "exact")
        )
    {
        return Err("semantic session catalog fields are unsupported or invalid".into());
    }
    let frontier = catalog
        .semantic_frontier
        .as_ref()
        .ok_or_else(|| "semantic session catalog has no authority frontier".to_string())?;
    Uuid::parse_str(&frontier.stream_id)
        .map_err(|_| "semantic session catalog stream ID is invalid".to_string())?;
    Uuid::parse_str(&frontier.event_id)
        .map_err(|_| "semantic session catalog event ID is invalid".to_string())?;
    if frontier.sequence == 0 {
        return Err("semantic session catalog frontier sequence is invalid".into());
    }
    let normalized_catalog = normalize_workspace_path(catalog.workspace_identity.as_bytes())
        .map_err(|error| format!("semantic session catalog workspace is invalid: {error}"))?;
    if workspace.is_some_and(|expected| expected != normalized_catalog) {
        return Ok(None);
    }
    let mut evidence = json!({
        "session_id": filename_id,
        "workspace_key": workspace_key("unix", &normalized_catalog),
        "catalog_schema_version": catalog.catalog_schema_version,
        "catalog_size": catalog_bytes.bytes.len(),
        "catalog_digest": catalog_bytes.digest,
        "metadata_revision": catalog.metadata_revision,
        "turns": catalog.turns,
        "tool_calls": catalog.tool_calls,
        "semantic_frontier": {
            "stream_id": &frontier.stream_id,
            "sequence": frontier.sequence,
            "event_id": &frontier.event_id,
        },
    });
    evidence["lineage"] = Value::String(bounded(&catalog.lineage, 256));
    evidence["availability"] = Value::String(bounded(&catalog.availability, 256));
    let _operator_metadata = (
        &catalog.friendly_name,
        &catalog.description,
        &catalog.last_prompt_snippet,
    );
    Ok(Some(InspectedSessionCatalog {
        evidence,
        catalog_identity: catalog_bytes.identity,
        catalog_digest: catalog_bytes.digest,
    }))
}

fn inspect_session_pair(
    directory: &File,
    meta_name: &[u8],
    filename_id: &str,
    workspace: Option<&[u8]>,
    context: &Context,
) -> Result<Option<InspectedSessionPair>, String> {
    if !canonical_session_id(filename_id) {
        return Err("session filename is not canonical".into());
    }
    let meta_bytes = read_bounded_regular_at(directory, meta_name, MAX_METADATA_BYTES, context)?;
    let meta: SessionMeta = serde_json::from_slice(&meta_bytes.bytes)
        .map_err(|error| format!("session metadata is malformed: {error}"))?;
    if meta.session_id != filename_id {
        return Err("session metadata ID does not match filename".into());
    }
    let normalized_meta =
        normalize_workspace_path(meta.cwd.as_bytes()).map_err(|error| error.to_string())?;
    if workspace.is_some_and(|expected| expected != normalized_meta) {
        return Ok(None);
    }
    let snapshot_name = format!("{filename_id}.json");
    let snapshot = read_bounded_regular_at(
        directory,
        snapshot_name.as_bytes(),
        MAX_METADATA_BYTES,
        context,
    )?;
    let snapshot_value: Value = serde_json::from_slice(&snapshot.bytes)
        .map_err(|error| format!("session snapshot framing is malformed: {error}"))?;
    let schema = snapshot_value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "session snapshot lacks integer schema_version".to_string())?;
    if schema != 1 {
        return Err(format!(
            "session snapshot schema version {schema} is unsupported"
        ));
    }
    Ok(Some(InspectedSessionPair {
        evidence: json!({
            "session_id": filename_id,
            "workspace_key": workspace_key("unix", &normalized_meta),
            "schema_version": schema,
            "metadata_size": meta_bytes.bytes.len(),
            "metadata_digest": meta_bytes.digest,
            "snapshot_size": snapshot.bytes.len(),
            "snapshot_digest": snapshot.digest,
        }),
        metadata_identity: meta_bytes.identity,
        snapshot_identity: snapshot.identity,
        metadata_digest: meta_bytes.digest,
        snapshot_digest: snapshot.digest,
    }))
}

fn resource_diagnostics(context: &Context, result: &mut MaintenanceResultV1) {
    let Some(workspace) = &context.workspace else {
        fail(
            result,
            "root_workspace_required",
            "admission",
            true,
            "resource commands require --workspace",
        );
        return;
    };
    let normalized_workspace = match normalize_workspace_path(os_bytes(workspace.path.as_os_str()))
    {
        Ok(path) => path,
        Err(error) => {
            fail(
                result,
                "resource_workspace_invalid",
                "admission",
                true,
                &error.to_string(),
            );
            return;
        }
    };
    let expected_workspace_key = workspace_key("unix", &normalized_workspace);
    let runtime = match open_dir_at(&workspace.file, ".omegon/runtime", context) {
        Ok(Some(runtime)) => runtime,
        Ok(None) => return,
        Err(error) => {
            diagnostic(
                result,
                "resource_root_unreadable",
                Severity::Warning,
                "resource",
                &format!("resource root is unreadable: {error}"),
                None,
            );
            result.status = ResultStatus::Degraded;
            return;
        }
    };
    let mut entries = match read_dir_at(&runtime, context, MAX_ENTRIES) {
        Ok(entries) => entries,
        Err(error) => {
            fail(
                result,
                "limit_entries_exceeded",
                "inventory",
                true,
                &format!("resource inventory cannot be bounded: {error}"),
            );
            return;
        }
    };
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    let mut examined = entries.len();
    for entry in entries {
        if entry.kind != EntryType::Directory {
            if matches!(
                entry.name.as_slice(),
                b"workspace.json" | b"workspaces.json"
            ) {
                diagnose_legacy_resource(result);
            }
            continue;
        }
        let runtime_id = String::from_utf8_lossy(&entry.name);
        let directory = match open_child_dir_at(&runtime, &entry.name, context) {
            Ok(Some(directory)) => directory,
            Ok(None) => continue,
            Err(error) => {
                diagnose_invalid_resource(
                    result,
                    &format!("runtime {runtime_id} is unreadable: {error}"),
                );
                continue;
            }
        };
        let children = match read_dir_at(&directory, context, MAX_ENTRIES - examined) {
            Ok(entries) => entries,
            Err(error) => {
                fail(
                    result,
                    "limit_entries_exceeded",
                    "inventory",
                    true,
                    &format!("resource inventory cannot be bounded: {error}"),
                );
                return;
            }
        };
        examined += children.len();
        let Some(ownership) = children
            .iter()
            .find(|child| child.name == b"ownership-v1.json")
        else {
            if children.iter().any(|child| child.name == b"workspace.json") {
                diagnose_legacy_resource(result);
            }
            diagnose_invalid_resource(
                result,
                &format!("runtime {runtime_id} has no v1 ownership record"),
            );
            continue;
        };
        if ownership.kind != EntryType::File {
            diagnose_invalid_resource(result, "ownership record is not a regular file");
            continue;
        }
        match read_bounded_regular_at(&directory, &ownership.name, MAX_METADATA_BYTES, context)
            .and_then(|file| {
                parse_record::<OwnershipRecordV1>(&file.bytes).map_err(|error| error.to_string())
            }) {
            Ok(record)
                if record.runtime_id.as_bytes() == entry.name
                    && record.workspace_key == expected_workspace_key
                    && complete_ownership_evidence(&record) =>
            {
                diagnostic(
                    result,
                    "resource_ownership_v1",
                    Severity::Info,
                    "resource",
                    &format!(
                        "runtime {} has a valid v1 ownership record",
                        record.runtime_id
                    ),
                    Some(json!({"record": record})),
                );
            }
            Ok(_) => diagnose_invalid_resource(
                result,
                "ownership record identity does not match its runtime directory and workspace",
            ),
            Err(message) => diagnose_invalid_resource(result, &message),
        }
    }
}

fn complete_ownership_evidence(record: &OwnershipRecordV1) -> bool {
    !record.runtime_id.is_empty()
        && !record.generation_id.is_empty()
        && !record.boot_id.is_empty()
        && record.pid != 0
        && !record.process_start_token.is_empty()
        && !record.writer.version.is_empty()
        && !record.writer.commit.is_empty()
        && !record.writer.target.is_empty()
        && !matches!(
            (record.lifecycle_boundary, record.cleanup_capability),
            (LifecycleBoundary::CrossBoundary, CleanupCapability::Strict)
        )
}

fn diagnose_invalid_resource(result: &mut MaintenanceResultV1, message: &str) {
    diagnostic(
        result,
        "resource_record_invalid",
        Severity::Warning,
        "resource",
        message,
        None,
    );
    result.status = ResultStatus::Degraded;
}

fn diagnose_legacy_resource(result: &mut MaintenanceResultV1) {
    diagnostic(
        result,
        "resource_legacy_unverifiable",
        Severity::Warning,
        "resource",
        "legacy workspace record is inspect-only and unverifiable",
        None,
    );
    result.status = ResultStatus::Degraded;
}

fn release_inspect(result: &mut MaintenanceResultV1) {
    let manifest = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("package-manifest-v1.json"))
    });
    match manifest.filter(|path| path.exists()) {
        Some(path) => diagnostic(
            result,
            "release_manifest_present",
            Severity::Info,
            "release",
            "adjacent package manifest is present but not trusted as signed evidence",
            Some(json!({"path": path})),
        ),
        None => {
            diagnostic(
                result,
                "release_manifest_absent",
                Severity::Warning,
                "release",
                "adjacent package manifest is absent",
                None,
            );
            result.status = ResultStatus::Degraded;
        }
    }
}

struct BoundedFile {
    bytes: Vec<u8>,
    digest: AuthorityKey,
    identity: FileIdentityV1,
}

struct InspectedSessionPair {
    evidence: Value,
    metadata_identity: FileIdentityV1,
    snapshot_identity: FileIdentityV1,
    metadata_digest: AuthorityKey,
    snapshot_digest: AuthorityKey,
}

struct InspectedSessionCatalog {
    evidence: Value,
    catalog_identity: FileIdentityV1,
    catalog_digest: AuthorityKey,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryType {
    File,
    Directory,
    Symlink,
    Other,
}

struct RawEntry {
    name: Vec<u8>,
    kind: EntryType,
}

#[cfg(unix)]
fn open_dir_at(parent: &File, relative: &str, context: &Context) -> Result<Option<File>, String> {
    let mut current = parent.try_clone().map_err(|error| error.to_string())?;
    for component in relative
        .split('/')
        .filter(|component| !component.is_empty())
    {
        let Some(next) = open_child_dir_at(&current, component.as_bytes(), context)? else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current))
}

#[cfg(unix)]
fn open_child_dir_at(
    parent: &File,
    name: &[u8],
    context: &Context,
) -> Result<Option<File>, String> {
    use std::{ffi::CString, os::fd::FromRawFd};

    if context.expired() {
        return Err("deadline expired before directory open".into());
    }
    validate_child_name(name).map_err(|error| error.to_string())?;
    let name = CString::new(name).expect("validated component has no NUL");
    // SAFETY: descriptor/name are valid for the call; returned fd is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error.to_string())
        };
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(Some(unsafe { File::from_raw_fd(descriptor) }))
}

#[cfg(unix)]
fn read_dir_at(
    directory: &File,
    context: &Context,
    remaining: usize,
) -> Result<Vec<RawEntry>, String> {
    use std::{ffi::CStr, os::fd::AsRawFd};

    // SAFETY: dup returns a new descriptor consumed by fdopendir.
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: duplicate is an owned directory descriptor.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir did not consume duplicate on failure.
        unsafe { libc::close(duplicate) };
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut entries = Vec::new();
    loop {
        if context.expired() {
            // SAFETY: stream is a live DIR pointer and closed exactly once.
            unsafe { libc::closedir(stream) };
            return Err("deadline expired during directory enumeration".into());
        }
        clear_errno();
        // SAFETY: stream is valid until closed below.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error = current_errno();
            // SAFETY: stream is a live DIR pointer and closed exactly once.
            unsafe { libc::closedir(stream) };
            return if error == 0 {
                Ok(entries)
            } else {
                Err(std::io::Error::from_raw_os_error(error).to_string())
            };
        }
        // SAFETY: d_name is NUL-terminated for a successful readdir result.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if matches!(name, b"." | b"..") {
            continue;
        }
        let kind = entry_type_at(directory, name)?;
        entries.push(RawEntry {
            name: name.to_vec(),
            kind,
        });
        if entries.len() > remaining {
            // SAFETY: stream is a live DIR pointer and closed exactly once.
            unsafe { libc::closedir(stream) };
            return Err("entry limit exceeded".into());
        }
    }
}

#[cfg(unix)]
fn entry_type_at(parent: &File, name: &[u8]) -> Result<EntryType, String> {
    use std::ffi::CString;

    let name = CString::new(name).map_err(|_| "entry name contains NUL".to_string())?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstatat initializes metadata on success and retains no pointer.
    if unsafe {
        libc::fstatat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: fstatat succeeded.
    let mode = unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT;
    Ok(if mode == libc::S_IFREG {
        EntryType::File
    } else if mode == libc::S_IFDIR {
        EntryType::Directory
    } else if mode == libc::S_IFLNK {
        EntryType::Symlink
    } else {
        EntryType::Other
    })
}

#[cfg(unix)]
fn read_link_at(parent: &File, name: &[u8]) -> Result<String, String> {
    use std::ffi::CString;

    let name = CString::new(name).map_err(|_| "entry name contains NUL".to_string())?;
    let mut buffer = vec![0_u8; 4097];
    // SAFETY: pointers and lengths are valid; readlinkat writes at most buffer.len bytes.
    let read = unsafe {
        libc::readlinkat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if read < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if read as usize > 4096 {
        return Err("symlink text exceeds 4096 bytes".into());
    }
    buffer.truncate(read as usize);
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[cfg(unix)]
fn read_bounded_regular_at(
    parent: &File,
    name: &[u8],
    limit: usize,
    context: &Context,
) -> Result<BoundedFile, String> {
    use std::{ffi::CString, os::fd::FromRawFd, os::unix::fs::MetadataExt};

    if context.expired() {
        return Err("deadline expired before file read".into());
    }
    validate_child_name(name).map_err(|error| error.to_string())?;
    let name = CString::new(name).expect("validated name has no NUL");
    // SAFETY: parent/name are valid for the call; returned fd is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    let before = file.metadata().map_err(|error| error.to_string())?;
    if !before.is_file() || before.len() > limit as u64 {
        return Err("file is not regular or exceeds its framing limit".into());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if context.expired() {
        return Err("deadline expired during file read".into());
    }
    if bytes.len() > limit {
        return Err("file exceeds its framing limit".into());
    }
    let after = file.metadata().map_err(|error| error.to_string())?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err("file changed during inspection".into());
    }
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    Ok(BoundedFile {
        bytes,
        digest: AuthorityKey::from_bytes(digest),
        identity: file_identity(&file).map_err(|error| error.to_string())?,
    })
}

#[cfg(target_os = "macos")]
fn clear_errno() {
    // SAFETY: __error returns the calling thread's errno pointer.
    unsafe { *libc::__error() = 0 };
}

#[cfg(target_os = "macos")]
fn current_errno() -> i32 {
    // SAFETY: __error returns the calling thread's errno pointer.
    unsafe { *libc::__error() }
}

#[cfg(target_os = "linux")]
fn clear_errno() {
    // SAFETY: __errno_location returns the calling thread's errno pointer.
    unsafe { *libc::__errno_location() = 0 };
}

#[cfg(target_os = "linux")]
fn current_errno() -> i32 {
    // SAFETY: __errno_location returns the calling thread's errno pointer.
    unsafe { *libc::__errno_location() }
}

fn base_result(
    command: &str,
    request_id: String,
    deadline: Duration,
    budget: Option<(Instant, Duration)>,
) -> MaintenanceResultV1 {
    let artifact = artifact_identity(budget);
    let excluded_inputs: Vec<String> = EXCLUSIONS.iter().map(|value| (*value).to_owned()).collect();
    let generation = derive_key(
        "composition",
        &[b"maintenance", excluded_inputs.join("\0").as_bytes()],
    );
    MaintenanceResultV1 {
        schema_version: SCHEMA_VERSION,
        command: command.to_owned(),
        status: ResultStatus::Success,
        request_id,
        artifact,
        composition: CompositionIdentityV1 {
            profile: "maintenance".into(),
            generation,
            excluded_inputs,
        },
        deadline: DeadlineEvidenceV1 {
            requested_ms: deadline.as_millis() as u64,
            elapsed_ms: 0,
            expired: false,
        },
        diagnostics: Vec::new(),
        mutations: Vec::new(),
        errors: Vec::new(),
        truncated: false,
        next_cursor: None,
    }
}

fn artifact_identity(budget: Option<(Instant, Duration)>) -> ArtifactIdentityV1 {
    let digest = std::env::current_exe()
        .ok()
        .and_then(|path| hash_file(&path, budget).ok())
        .unwrap_or_else(|| derive_key("artifact-unavailable", &[]));
    ArtifactIdentityV1 {
        version: env!("CARGO_PKG_VERSION").into(),
        commit: env!("OMEGON_MAINTAIN_GIT_SHA").into(),
        target: env!("OMEGON_MAINTAIN_TARGET").into(),
        digest,
    }
}

fn hash_file(
    path: &Path,
    budget: Option<(Instant, Duration)>,
) -> Result<AuthorityKey, std::io::Error> {
    if budget.is_some_and(|(started, deadline)| started.elapsed() >= deadline) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "deadline expired before artifact open",
        ));
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if budget.is_some_and(|(started, deadline)| started.elapsed() >= deadline) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "deadline expired during artifact read",
            ));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(AuthorityKey::from_bytes(digest.finalize().into()))
}

fn diagnostic(
    result: &mut MaintenanceResultV1,
    code: &str,
    severity: Severity,
    scope: &str,
    message: &str,
    evidence: Option<Value>,
) {
    result.diagnostics.push(DiagnosticV1 {
        code: code.into(),
        severity,
        scope: scope.into(),
        message: bounded(message, 4096),
        evidence: evidence.map(|value| bounded(&value.to_string(), 4096)),
    });
}

fn fail(
    result: &mut MaintenanceResultV1,
    code: &str,
    phase: &str,
    retry_safe: bool,
    message: &str,
) {
    result.status = ResultStatus::Failure;
    result.errors.push(ErrorV1 {
        code: code.into(),
        phase: phase.into(),
        retry_safe,
        message: bounded(message, 4096),
    });
}

fn degrade(result: &mut MaintenanceResultV1, code: &str, message: &str) {
    result.status = ResultStatus::Degraded;
    diagnostic(result, code, Severity::Warning, "deadline", message, None);
}

fn admission_deadline_expired(
    result: &mut MaintenanceResultV1,
    started: Instant,
    deadline: Duration,
) -> bool {
    if started.elapsed() < deadline {
        return false;
    }
    result.deadline.elapsed_ms = started.elapsed().as_millis() as u64;
    result.deadline.expired = true;
    if is_mutation_command_name(&result.command) {
        fail(
            result,
            "deadline_expired",
            "admission",
            true,
            "deadline expired during root admission",
        );
    } else {
        degrade(
            result,
            "deadline_expired",
            "deadline expired during root admission",
        );
    }
    finalize(result);
    true
}

fn settle_deadline(result: &mut MaintenanceResultV1, started: Instant, deadline: Duration) {
    result.deadline.elapsed_ms = started.elapsed().as_millis() as u64;
    result.deadline.expired = started.elapsed() >= deadline;
    if result.deadline.expired && result.status == ResultStatus::Success {
        degrade(
            result,
            "deadline_expired",
            "deadline expired before diagnostic settlement",
        );
    }
}

fn finalize(result: &mut MaintenanceResultV1) {
    result.errors.sort_by(|left, right| {
        (&left.code, &left.phase, left.retry_safe, &left.message).cmp(&(
            &right.code,
            &right.phase,
            right.retry_safe,
            &right.message,
        ))
    });
    if is_paginated_list(&result.command)
        && result.errors.is_empty()
        && result.mutations.is_empty()
        && let Some(cursor) = result.next_cursor.take()
    {
        let cursor = decode_list_cursor(&result.command, &cursor, true)
            .expect("staged list cursor is generated internally");
        paginate_diagnostics(result, cursor);
    }
    if serde_json::to_vec(result).is_ok_and(|bytes| bytes.len() > MAX_OUTPUT_BYTES) {
        result.status = ResultStatus::Failure;
        result.diagnostics.clear();
        result.mutations.clear();
        result.errors.clear();
        result.truncated = false;
        result.next_cursor = None;
        result.errors.push(ErrorV1 {
            code: "output_limit_exceeded".into(),
            phase: "output".into(),
            retry_safe: true,
            message: "result exceeded the 4 MiB output limit; no partial inventory was emitted"
                .into(),
        });
    }
    if let Err(error) = result.validate() {
        result.status = ResultStatus::Failure;
        result.errors.clear();
        result.errors.push(ErrorV1 {
            code: "output_invalid".into(),
            phase: "output".into(),
            retry_safe: false,
            message: bounded(&error.to_string(), 4096),
        });
    }
}

fn emit(result: &MaintenanceResultV1, json_output: bool) -> i32 {
    if json_output {
        match canonical_json(result) {
            Ok(bytes) => print!("{}", String::from_utf8_lossy(&bytes)),
            Err(error) => {
                eprintln!("failed to encode maintenance result: {error}");
                return 2;
            }
        }
    } else {
        println!("{}: {:?}", result.command, result.status);
        for diagnostic in &result.diagnostics {
            println!(
                "[{:?}] {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            );
        }
        for error in &result.errors {
            println!("[error] {}: {}", error.code, error.message);
        }
    }
    i32::from(result.status.exit_code())
}

fn resolve_home(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return validate_absolute_root(path);
    }
    if let Some(path) = std::env::var_os("OMEGON_HOME") {
        return validate_absolute_root(path.into());
    }
    let home =
        std::env::var_os("HOME").ok_or_else(|| "HOME and OMEGON_HOME are unset".to_string())?;
    validate_absolute_root(PathBuf::from(home).join(".omegon"))
}

fn resolve_config_home(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return validate_absolute_root(path);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME is unset".to_string())?;
    validate_absolute_root(PathBuf::from(home).join(".config/omegon"))
}

fn validate_absolute_root(path: PathBuf) -> Result<PathBuf, String> {
    if !path.is_absolute() || path == Path::new("/") {
        return Err("root must be an absolute directory other than /".into());
    }
    Ok(path)
}

fn admit_root(path: PathBuf) -> Result<AdmittedRoot, String> {
    #[cfg(unix)]
    {
        use std::{
            ffi::CString, os::fd::FromRawFd, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt,
        };

        let encoded = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "root contains an interior NUL".to_string())?;
        // SAFETY: encoded remains valid for the call; the returned descriptor is owned below.
        let descriptor = unsafe {
            libc::open(
                encoded.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(format!(
                "cannot open root without following symlinks: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: open returned a new owned descriptor.
        let file = unsafe { File::from_raw_fd(descriptor) };
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err("root must be a directory owned by the effective user".into());
        }
        let canonical_path = descriptor_path(&file)?;
        if canonical_path == Path::new("/") {
            return Err("root descriptor resolves to /".into());
        }
        Ok(AdmittedRoot {
            path,
            file,
            device: metadata.dev(),
            inode: metadata.ino(),
            unsafe_permissions: metadata.mode() & 0o022 != 0,
        })
    }
    #[cfg(not(unix))]
    {
        let metadata = path.symlink_metadata().map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("root must be a non-symlink directory".into());
        }
        Ok(AdmittedRoot {
            file: File::open(&path).map_err(|error| error.to_string())?,
            path,
            device: 0,
            inode: 0,
            unsafe_permissions: false,
        })
    }
}

#[cfg(target_os = "macos")]
fn descriptor_path(file: &File) -> Result<PathBuf, String> {
    use std::{ffi::CStr, os::fd::AsRawFd, os::unix::ffi::OsStrExt};

    let mut buffer = vec![0_i8; libc::PATH_MAX as usize];
    // SAFETY: buffer is writable and large enough for F_GETPATH's documented result.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: successful F_GETPATH writes a NUL-terminated path.
    let bytes = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes();
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(target_os = "linux")]
fn descriptor_path(file: &File) -> Result<PathBuf, String> {
    use std::os::fd::AsRawFd;

    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|error| error.to_string())
}

fn aliases(left: &AdmittedRoot, right: &AdmittedRoot) -> bool {
    left.device == right.device && left.inode == right.inode
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    let (number, multiplier) = if let Some(value) = value.strip_suffix("ms") {
        (value, 1_u64)
    } else if let Some(value) = value.strip_suffix('s') {
        (value, 1000)
    } else if let Some(value) = value.strip_suffix('m') {
        (value, 60_000)
    } else {
        return Err("duration must end in ms, s, or m".into());
    };
    let number: u64 = number
        .parse()
        .map_err(|_| "duration must be an unsigned integer".to_string())?;
    number
        .checked_mul(multiplier)
        .map(Duration::from_millis)
        .ok_or_else(|| "duration overflows".into())
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Identity => "identity",
        Command::Doctor => "doctor",
        Command::Home {
            command: HomeCommand::Inspect,
        } => "home.inspect",
        Command::Home {
            command: HomeCommand::Recover,
        } => "home.recover",
        Command::Composition { .. } => "composition.inspect",
        Command::Contribution {
            command: ContributionCommand::List { .. },
        } => "contribution.list",
        Command::Contribution {
            command: ContributionCommand::Inspect { .. },
        } => "contribution.inspect",
        Command::Contribution {
            command: ContributionCommand::Disable { .. },
        } => "contribution.disable",
        Command::Contribution {
            command: ContributionCommand::Quarantine { .. },
        } => "contribution.quarantine",
        Command::Session {
            command: SessionCommand::List { .. },
        } => "session.list",
        Command::Session {
            command: SessionCommand::Inspect { .. },
        } => "session.inspect",
        Command::Session {
            command: SessionCommand::Quarantine { .. },
        } => "session.quarantine",
        Command::Resource {
            command: ResourceCommand::List { .. },
        } => "resource.list",
        Command::Resource {
            command: ResourceCommand::PruneStale,
        } => "resource.prune_stale",
        Command::Release {
            command: ReleaseCommand::Inspect,
        } => "release.inspect",
        Command::Release {
            command: ReleaseCommand::Verify { .. },
        } => "release.verify",
        Command::Audit {
            command: AuditCommand::Inspect { .. },
        } => "audit.inspect",
        Command::Audit {
            command: AuditCommand::Verify { .. },
        } => "audit.verify",
    }
}

fn is_mutation(command: &Command) -> bool {
    matches!(
        command,
        Command::Home {
            command: HomeCommand::Recover
        } | Command::Contribution {
            command: ContributionCommand::Disable { .. } | ContributionCommand::Quarantine { .. }
        } | Command::Session {
            command: SessionCommand::Quarantine { .. }
        } | Command::Resource {
            command: ResourceCommand::PruneStale
        }
    )
}

fn is_mutation_command_name(command: &str) -> bool {
    matches!(
        command,
        "contribution.disable"
            | "home.recover"
            | "contribution.quarantine"
            | "session.quarantine"
            | "resource.prune_stale"
    )
}

fn is_paginated_list(command: &str) -> bool {
    matches!(
        command,
        "contribution.list" | "session.list" | "resource.list"
    )
}

struct ListCursor {
    query: AuthorityKey,
    boundary: Option<AuthorityKey>,
}

fn decode_list_cursor(
    command: &str,
    cursor: &str,
    allow_empty_boundary: bool,
) -> Result<ListCursor, String> {
    let mut fields = cursor.split(':');
    if fields.next() != Some("list-v2")
        || fields.next() != Some(command)
        || fields.clone().count() != 2
    {
        return Err("list cursor is malformed or belongs to a different command".into());
    }
    let query = fields
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "list cursor query binding is malformed".to_string())?;
    let boundary = match fields.next() {
        Some("-") if allow_empty_boundary => None,
        Some(value) => Some(
            value
                .parse()
                .map_err(|_| "list cursor boundary is malformed".to_string())?,
        ),
        None => return Err("list cursor boundary is malformed".into()),
    };
    Ok(ListCursor { query, boundary })
}

fn list_cursor(command: &str, query: AuthorityKey, boundary: Option<AuthorityKey>) -> String {
    let boundary = boundary.map_or_else(|| "-".into(), |key| key.to_string());
    format!("list-v2:{command}:{query}:{boundary}")
}

fn stage_list_page(
    result: &mut MaintenanceResultV1,
    query: AuthorityKey,
    encoded_cursor: Option<&str>,
) {
    if !result.errors.is_empty() || !result.mutations.is_empty() {
        return;
    }
    let boundary = if let Some(encoded_cursor) = encoded_cursor {
        let cursor = match decode_list_cursor(&result.command, encoded_cursor, false) {
            Ok(cursor) if cursor.query == query => cursor,
            Ok(_) => {
                fail(
                    result,
                    "cli_cursor_invalid",
                    "admission",
                    true,
                    "list cursor belongs to a different query",
                );
                return;
            }
            Err(message) => {
                fail(result, "cli_cursor_invalid", "admission", true, &message);
                return;
            }
        };
        let boundary = cursor
            .boundary
            .expect("external cursors require a boundary");
        let mut positions = result
            .diagnostics
            .iter()
            .enumerate()
            .filter(|(_, diagnostic)| list_item_key(diagnostic) == boundary)
            .map(|(position, _)| position);
        let Some(position) = positions.next() else {
            result.diagnostics.clear();
            fail(
                result,
                "cli_cursor_invalid",
                "admission",
                true,
                "list cursor boundary is no longer present",
            );
            return;
        };
        if positions.next().is_some() {
            result.diagnostics.clear();
            fail(
                result,
                "cli_cursor_invalid",
                "admission",
                true,
                "list cursor boundary is ambiguous",
            );
            return;
        }
        result.diagnostics.drain(..=position);
        Some(boundary)
    } else {
        None
    };
    result.next_cursor = Some(list_cursor(&result.command, query, boundary));
}

fn list_item_key(diagnostic: &DiagnosticV1) -> AuthorityKey {
    let encoded = serde_json::to_vec(diagnostic).expect("diagnostics are serializable");
    derive_key("maintenance-list-item-v2", &[&encoded])
}

fn root_query_identity(root: &AdmittedRoot) -> AuthorityKey {
    derive_key(
        "maintenance-list-root-v2",
        &[
            &root.device.to_be_bytes(),
            &root.inode.to_be_bytes(),
            os_bytes(root.path.as_os_str()),
        ],
    )
}

fn contribution_list_query(context: &Context, scope: Option<ScopeArg>) -> AuthorityKey {
    let effective_scope = match (scope, context.workspace.is_some()) {
        (Some(ScopeArg::User), _) | (None, false) => "user",
        (Some(ScopeArg::Project), _) => "project",
        (None, true) => "user+project",
    };
    let home = root_query_identity(&context.home);
    let workspace = if effective_scope == "user" {
        None
    } else {
        context.workspace.as_ref().map(root_query_identity)
    };
    derive_key(
        "maintenance-list-query-v2",
        &[
            b"contribution.list",
            effective_scope.as_bytes(),
            home.as_bytes(),
            workspace
                .as_ref()
                .map_or(&[] as &[u8], |key| key.as_bytes()),
        ],
    )
}

fn session_list_query(context: &Context) -> AuthorityKey {
    let config_home = root_query_identity(&context.config_home);
    let workspace = context.workspace.as_ref().map(root_query_identity);
    derive_key(
        "maintenance-list-query-v2",
        &[
            b"session.list",
            config_home.as_bytes(),
            workspace
                .as_ref()
                .map_or(&[] as &[u8], |key| key.as_bytes()),
        ],
    )
}

fn resource_list_query(context: &Context) -> AuthorityKey {
    let workspace = context.workspace.as_ref().map(root_query_identity);
    derive_key(
        "maintenance-list-query-v2",
        &[
            b"resource.list",
            workspace
                .as_ref()
                .map_or(&[] as &[u8], |key| key.as_bytes()),
        ],
    )
}

fn paginate_diagnostics(result: &mut MaintenanceResultV1, cursor: ListCursor) {
    let diagnostics = std::mem::take(&mut result.diagnostics);
    let original_status = result.status;
    let mut lower = 0;
    let mut upper = diagnostics.len() + 1;
    while lower + 1 < upper {
        let candidate = lower + (upper - lower) / 2;
        set_diagnostic_prefix(result, &diagnostics, candidate, &cursor, original_status);
        if serde_json::to_vec(result).is_ok_and(|bytes| bytes.len() <= MAX_OUTPUT_BYTES) {
            lower = candidate;
        } else {
            upper = candidate;
        }
    }
    set_diagnostic_prefix(result, &diagnostics, lower, &cursor, original_status);
}

fn set_diagnostic_prefix(
    result: &mut MaintenanceResultV1,
    diagnostics: &[DiagnosticV1],
    length: usize,
    cursor: &ListCursor,
    original_status: ResultStatus,
) {
    result.diagnostics = diagnostics[..length].to_vec();
    result.truncated = length < diagnostics.len();
    if result.truncated {
        result.status = ResultStatus::Degraded;
        let boundary = length
            .checked_sub(1)
            .and_then(|index| diagnostics.get(index))
            .map(list_item_key)
            .or(cursor.boundary);
        result.next_cursor = Some(list_cursor(&result.command, cursor.query, boundary));
    } else {
        result.status = original_status;
        result.next_cursor = None;
    }
}

fn same_scope(left: ScopeArg, right: ScopeArg) -> bool {
    matches!(
        (left, right),
        (ScopeArg::User, ScopeArg::User) | (ScopeArg::Project, ScopeArg::Project)
    )
}

fn scope_name(scope: ScopeArg) -> &'static str {
    match scope {
        ScopeArg::User => "user",
        ScopeArg::Project => "project",
    }
}

fn scope_order(scope: ScopeArg) -> u8 {
    match scope {
        ScopeArg::User => 0,
        ScopeArg::Project => 1,
    }
}

fn strip_suffix<'a>(name: &'a [u8], suffix: Option<&str>) -> Option<&'a [u8]> {
    match suffix {
        Some(suffix) => name.strip_suffix(suffix.as_bytes()),
        None => Some(name),
    }
}

fn canonical_session_id(id: &str) -> bool {
    let Some((timestamp, suffix)) = id.split_once('_') else {
        return false;
    };
    timestamp.len() == 19
        && timestamp.as_bytes().get(4) == Some(&b'-')
        && timestamp.as_bytes().get(7) == Some(&b'-')
        && timestamp.as_bytes().get(10) == Some(&b'T')
        && timestamp.as_bytes().get(13) == Some(&b'-')
        && timestamp.as_bytes().get(16) == Some(&b'-')
        && timestamp.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 13 | 16) && byte == b'-'
                || index == 10 && byte == b'T'
                || byte.is_ascii_digit()
        })
        && suffix.len() == 8
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

impl Context {
    fn expired(&self) -> bool {
        self.started.elapsed() >= self.deadline
    }
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes()
}

#[cfg(not(unix))]
fn os_bytes(value: &OsStr) -> &[u8] {
    value.to_string_lossy().as_bytes()
}

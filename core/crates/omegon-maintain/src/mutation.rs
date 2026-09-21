use std::{collections::BTreeMap, fs::File, thread, time::Duration};

use chrono::{DateTime, Utc};
use omegon_maintenance_contracts::{
    AUDIT_SEGMENT_RECORDS, AuditCheckpointV1, AuditReceiptV1, AuditRecordV1, AuthorityKey,
    CommandSemanticsV1, ContractError, ContributionSelector, DenyRecordV1, DenyState, DenyStateV1,
    ErrorV1, FenceState, FenceV1, FileIdentityV1, LockMode, MAX_RECORD_BYTES, MaintenanceResultV1,
    MaintenanceStateV1, MutationResultV1, MutationState, OwnershipRecordV1, PathIdentityV1,
    PostStateV1, ProtocolLock, Record, ResultStatus, SCHEMA_VERSION, SessionDenyRecordV1,
    SessionDenyState, TransactionState, TransactionStepKind, TransactionStepState,
    TransactionStepV1, TransactionV1, append_bytes_at, audit_receipt, canonical_digest,
    canonical_json, command_fingerprint, contribution_domain_key, create_record_no_replace_at,
    derive_key, entry_identity_at, entry_key, normalize_workspace_path,
    open_or_create_secure_dir_at, open_secure_dir_at, path_identity, read_bytes_at, read_record_at,
    read_record_with_identity_at, record_identity_at, remove_record_at, rename_entry_no_replace_at,
    replace_record_at, resource_domain_key, scope_key, session_domain_key, session_key,
    workspace_key,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{
    CONTRIBUTION_ROOTS, Command, Context, ContributionCommand, EntryType, MAX_ENTRIES,
    ResourceCommand, ScopeArg, SessionCommand, canonical_session_id, contribution_selector,
    diagnostic, fail, os_bytes, read_dir_at, scope_name, strip_suffix,
};

pub(super) fn execute(
    command: &Command,
    context: &Context,
    dry_run: bool,
    result: &mut MaintenanceResultV1,
) {
    let outcome = match command {
        Command::Contribution {
            command: ContributionCommand::Disable { selector, scope },
        } => mutate_contribution(context, selector, *scope, false, dry_run, result),
        Command::Contribution {
            command: ContributionCommand::Quarantine { selector, scope },
        } => mutate_contribution(context, selector, *scope, true, dry_run, result),
        Command::Session {
            command: SessionCommand::Quarantine { session_id },
        } => quarantine_session(context, session_id, dry_run, result),
        Command::Resource {
            command: ResourceCommand::PruneStale,
        } => prune_resources(context, dry_run, result),
        _ => {
            fail(
                result,
                "cli_unsupported_slice_zero_operation",
                "admission",
                true,
                "operation remains deferred while task 0.6 mutation primitives are integrated",
            );
            return;
        }
    };
    if let Err(error) = outcome {
        match error.kind {
            MutationFailure::Unknown => {
                result.status = ResultStatus::Degraded;
                result.errors.push(ErrorV1 {
                    code: "transaction_unknown".into(),
                    phase: "settlement".into(),
                    retry_safe: false,
                    message: super::bounded(&error.message, 4096),
                });
            }
            MutationFailure::Degraded => {
                result.status = ResultStatus::Degraded;
                result.errors.push(ErrorV1 {
                    code: "transaction_aborted".into(),
                    phase: "settlement".into(),
                    retry_safe: true,
                    message: super::bounded(&error.message, 4096),
                });
            }
            MutationFailure::Refused => fail(
                result,
                "transaction_refused",
                "mutation",
                true,
                &error.message,
            ),
        }
    }
}

struct MutationError {
    message: String,
    kind: MutationFailure,
}

enum MutationFailure {
    Refused,
    Unknown,
    Degraded,
}

impl MutationError {
    fn before(error: impl ToString) -> Self {
        Self {
            message: error.to_string(),
            kind: MutationFailure::Refused,
        }
    }

    fn after(error: impl ToString) -> Self {
        Self {
            message: error.to_string(),
            kind: MutationFailure::Unknown,
        }
    }

    fn degraded(error: impl ToString) -> Self {
        Self {
            message: error.to_string(),
            kind: MutationFailure::Degraded,
        }
    }
}

impl From<ContractError> for MutationError {
    fn from(error: ContractError) -> Self {
        Self::before(error)
    }
}

struct ResolvedContribution {
    parent: File,
    parent_identity: PathIdentityV1,
    raw_name: Vec<u8>,
    kind: omegon_maintenance_contracts::ContributionKind,
    scope: ScopeArg,
    scope_key: AuthorityKey,
    entry_key: AuthorityKey,
    identity: FileIdentityV1,
    entry_type: EntryType,
}

fn mutate_contribution(
    context: &Context,
    selector: &str,
    scope: ScopeArg,
    quarantine: bool,
    dry_run: bool,
    result: &mut MaintenanceResultV1,
) -> Result<(), MutationError> {
    let operation = if quarantine {
        "contribution.quarantine"
    } else {
        "contribution.disable"
    };
    let mut state = bootstrap_state(context, &result.request_id)?;
    let semantics = command_semantics(context, operation, selector, scope)?;
    let fingerprint = command_fingerprint(&semantics).map_err(MutationError::before)?;
    let audit_command = audit_event_name(operation, fingerprint);
    let requested_audit_command = if dry_run {
        audit_event_name(&format!("{operation}.dry_run"), fingerprint)
    } else {
        audit_command.clone()
    };
    let transaction_name = format!("{}.json", result.request_id);
    if let Some(existing) =
        read_record_at::<TransactionV1>(&state.transactions, transaction_name.as_bytes())
            .map_err(MutationError::before)?
    {
        let scope_key = find_contribution_scope_for_domain(context, scope, existing.domain_key)?;
        let lock_name = format!("contribution-{scope_key}.lock");
        let _domain_lock = acquire_domain_lock(&state, lock_name.as_bytes(), context)?;
        let existing =
            read_record_at::<TransactionV1>(&state.transactions, transaction_name.as_bytes())
                .map_err(MutationError::before)?
                .ok_or_else(|| {
                    MutationError::before("request transaction disappeared under lock")
                })?;
        let fence_name = format!("{}.json", existing.domain_key);
        return settle_existing_terminal(
            &mut state,
            context,
            existing,
            fingerprint,
            &fence_name,
            operation,
            result,
        )
        .and_then(|settled| {
            if settled {
                Ok(())
            } else {
                Err(MutationError::after(
                    "existing contribution transaction did not reach a terminal state",
                ))
            }
        });
    }
    let preliminary =
        resolve_contribution(context, selector, scope).map_err(MutationError::before)?;
    let domain_key = contribution_domain_key(preliminary.scope_key);
    let lock_name = format!("contribution-{}.lock", preliminary.scope_key);
    let _domain_lock = acquire_domain_lock(&state, lock_name.as_bytes(), context)?;
    let target = resolve_contribution(context, selector, scope).map_err(MutationError::before)?;
    if target.scope_key != preliminary.scope_key {
        return Err(MutationError::before(
            "contribution authority changed while acquiring its domain lock",
        ));
    }
    let fence_name = format!("{}.json", domain_key);
    if let Some(outcome) = existing_audit_event(
        &state,
        context,
        &result.request_id,
        &requested_audit_command,
    )? {
        result.status = outcome;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: operation.into(),
            state: if dry_run {
                MutationState::Planned
            } else {
                MutationState::Settled
            },
            retry_safe: true,
        });
        return Ok(());
    }
    if let Some(existing) =
        unresolved_domain_transaction(&state, domain_key, &result.request_id, context)?
        && settle_existing_terminal(
            &mut state,
            context,
            existing,
            fingerprint,
            &fence_name,
            operation,
            result,
        )?
    {
        return Ok(());
    }
    if read_record_at::<FenceV1>(&state.fences, fence_name.as_bytes())
        .map_err(MutationError::before)?
        .is_some()
    {
        return Err(MutationError::before(
            "request transaction or contribution-domain fence already exists",
        ));
    }
    let scope_directory_name = target.scope_key.to_hex();
    let existing_directory = open_secure_dir_at(&state.deny, scope_directory_name.as_bytes())
        .map_err(MutationError::before)?;
    let (deny_directory, current, current_identity) = match existing_directory {
        Some(directory) => {
            let (current, identity): (DenyStateV1, FileIdentityV1) =
                read_record_with_identity_at(&directory, b"state.json")?.ok_or_else(|| {
                    MutationError::before("initialized deny scope lacks state.json")
                })?;
            (Some(directory), current, Some(identity))
        }
        None if dry_run => (None, empty_deny_state(target.scope_key), None),
        None => {
            let directory =
                open_or_create_secure_dir_at(&state.deny, scope_directory_name.as_bytes())
                    .map_err(MutationError::before)?;
            let empty = empty_deny_state(target.scope_key);
            create_record_no_replace_at(&directory, b"state.json", &empty, &result.request_id)
                .map_err(MutationError::before)?;
            let identity = record_identity_at(&directory, b"state.json")?
                .ok_or_else(|| MutationError::before("initialized deny state disappeared"))?;
            (Some(directory), empty, Some(identity))
        }
    };
    if current.scope_key != target.scope_key {
        return Err(MutationError::before(
            "deny state does not belong to its scope directory",
        ));
    }

    let already_denied = current.entries.contains_key(&target.entry_key.to_hex());
    if dry_run {
        append_audit(
            &mut state,
            context,
            &result.request_id,
            &requested_audit_command,
            ResultStatus::Success,
        )?;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: operation.into(),
            state: MutationState::Planned,
            retry_safe: true,
        });
        diagnostic(
            result,
            "deny_planned",
            omegon_maintenance_contracts::Severity::Info,
            scope_name(target.scope),
            if quarantine {
                "contribution deny and detach are planned"
            } else if already_denied {
                "contribution is already denied; dry-run planned no target write"
            } else {
                "contribution deny generation update is planned"
            },
            Some(json!({"selector": selector, "entry_key": target.entry_key})),
        );
        return Ok(());
    }

    if already_denied && !quarantine {
        append_audit(
            &mut state,
            context,
            &result.request_id,
            &audit_command,
            ResultStatus::Success,
        )?;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: "contribution.disable".into(),
            state: MutationState::Settled,
            retry_safe: true,
        });
        diagnostic(
            result,
            "deny_already_settled",
            omegon_maintenance_contracts::Severity::Info,
            scope_name(target.scope),
            "contribution was already denied; generation was unchanged",
            Some(json!({"selector": selector, "generation": current.generation})),
        );
        return Ok(());
    }

    let deny_directory = deny_directory.expect("non-dry-run initialized deny directory");
    let next = if already_denied {
        None
    } else {
        Some(next_deny_state(&current, &target, &result.request_id)?)
    };
    let intended_digest = next
        .as_ref()
        .map(canonical_digest)
        .transpose()
        .map_err(MutationError::before)?;
    let now = now_utc();
    let transaction_record_id = derive_key("transaction", &[result.request_id.as_bytes()]);
    let quarantine_directory = if quarantine {
        Some(
            open_or_create_secure_dir_at(&target.parent, b".omegon-maintain-quarantine")
                .map_err(MutationError::before)?,
        )
    } else {
        None
    };
    let quarantine_name = format!("{}-{}", result.request_id, target.entry_key);
    let mut steps = Vec::new();
    if let (Some(current_identity), Some(intended_digest)) = (current_identity, intended_digest) {
        let (basename_bytes, basename_digest) =
            TransactionStepV1::encode_basename(b"state.json").map_err(MutationError::before)?;
        steps.push(TransactionStepV1 {
            kind: TransactionStepKind::DenyStateReplace,
            parent: path_identity(&deny_directory).map_err(MutationError::before)?,
            basename_bytes,
            basename_digest,
            destination_parent: None,
            destination_basename_bytes: None,
            destination_basename_digest: None,
            expected_existing: Some(current_identity),
            expected_absence: false,
            intended_content_digest: Some(intended_digest),
            state: TransactionStepState::Prepared,
            observed: None,
        });
    }
    if let Some(directory) = &quarantine_directory {
        let (basename_bytes, basename_digest) =
            TransactionStepV1::encode_basename(&target.raw_name).map_err(MutationError::before)?;
        let (kind, destination_parent, destination_basename_bytes, destination_basename_digest) =
            if target.entry_type == EntryType::Symlink {
                (
                    TransactionStepKind::QuarantineSymlinkUnlink,
                    None,
                    None,
                    None,
                )
            } else {
                let (destination_bytes, destination_digest) =
                    TransactionStepV1::encode_basename(quarantine_name.as_bytes())
                        .map_err(MutationError::before)?;
                (
                    TransactionStepKind::QuarantineDetach,
                    Some(path_identity(directory).map_err(MutationError::before)?),
                    Some(destination_bytes),
                    Some(destination_digest),
                )
            };
        steps.push(TransactionStepV1 {
            kind,
            parent: target.parent_identity.clone(),
            basename_bytes,
            basename_digest,
            destination_parent,
            destination_basename_bytes,
            destination_basename_digest,
            expected_existing: Some(target.identity.clone()),
            expected_absence: false,
            intended_content_digest: None,
            state: TransactionStepState::Prepared,
            observed: None,
        });
    }
    let mut transaction = TransactionV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "transaction".into(),
        record_id: transaction_record_id,
        request_id: result.request_id.clone(),
        command_fingerprint: fingerprint,
        domain_key,
        roots: root_identities(context)?,
        steps,
        state: TransactionState::Prepared,
        created_at: now.clone(),
        updated_at: now,
        audit_sequence: None,
    };
    let fence = FenceV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "fence".into(),
        record_id: derive_key(
            "fence",
            &[domain_key.as_bytes(), transaction_record_id.as_bytes()],
        ),
        domain_key,
        transaction_record_id,
        state: FenceState::Active,
    };
    create_record_no_replace_at(
        &state.transactions,
        transaction_name.as_bytes(),
        &transaction,
        &result.request_id,
    )
    .map_err(MutationError::before)?;
    create_record_no_replace_at(
        &state.fences,
        fence_name.as_bytes(),
        &fence,
        &result.request_id,
    )
    .map_err(MutationError::before)?;
    result.mutations.push(MutationResultV1 {
        domain_key,
        kind: operation.into(),
        state: MutationState::Prepared,
        retry_safe: true,
    });

    let mut step_index = 0;
    if let Some(next) = &next {
        let current =
            record_identity_at(&deny_directory, b"state.json").map_err(MutationError::before)?;
        if current.as_ref() != transaction.steps[step_index].expected_existing.as_ref() {
            return Err(MutationError::before(
                "deny state identity changed before dispatch",
            ));
        }
        dispatch_step(
            &mut state,
            context,
            &transaction_name,
            &mut transaction,
            step_index,
            &result.request_id,
            &audit_command,
        )?;
        result.mutations[0].state = MutationState::Dispatched;
        result.mutations[0].retry_safe = false;
        if let Err(error) =
            replace_record_at(&deny_directory, b"state.json", next, &result.request_id)
        {
            persist_unknown(
                &state,
                &transaction_name,
                &mut transaction,
                &result.request_id,
            )?;
            result.mutations[0].state = MutationState::Unknown;
            return Err(MutationError::after(error));
        }
        let observed_identity = record_identity_at(&deny_directory, b"state.json")
            .map_err(MutationError::after)?
            .ok_or_else(|| MutationError::after("deny state absent after replacement"))?;
        let observed: DenyStateV1 = read_record_at(&deny_directory, b"state.json")
            .map_err(MutationError::after)?
            .ok_or_else(|| MutationError::after("deny state absent after replacement"))?;
        let intended_digest = intended_digest.expect("next deny state has digest");
        if canonical_digest(&observed).map_err(MutationError::after)? != intended_digest {
            persist_unknown(
                &state,
                &transaction_name,
                &mut transaction,
                &result.request_id,
            )?;
            result.mutations[0].state = MutationState::Unknown;
            return Err(MutationError::after(
                "deny state content differs after replacement",
            ));
        }
        settle_step(
            &state,
            &transaction_name,
            &mut transaction,
            step_index,
            PostStateV1 {
                source_present: true,
                destination: Some(observed_identity),
                destination_content_digest: Some(intended_digest),
            },
            &result.request_id,
        )?;
        step_index += 1;
    }

    if let Some(quarantine_directory) = &quarantine_directory {
        let current_identity = entry_identity_at(&target.parent, &target.raw_name)
            .map_err(MutationError::before)?
            .ok_or_else(|| MutationError::before("quarantine source disappeared before detach"))?;
        if current_identity != target.identity {
            if step_index > 0 {
                return abort_prepared_after_partial(
                    &mut state,
                    context,
                    &mut transaction,
                    step_index,
                    &audit_command,
                    "quarantine source identity changed after the deny step settled",
                );
            }
            return Err(MutationError::before(
                "quarantine source identity changed before detach",
            ));
        }
        dispatch_step(
            &mut state,
            context,
            &transaction_name,
            &mut transaction,
            step_index,
            &result.request_id,
            &audit_command,
        )?;
        result.mutations[0].state = MutationState::Dispatched;
        result.mutations[0].retry_safe = false;
        let mutation_result = if target.entry_type == EntryType::Symlink {
            remove_record_at(&target.parent, &target.raw_name)
        } else {
            rename_entry_no_replace_at(
                &target.parent,
                &target.raw_name,
                quarantine_directory,
                quarantine_name.as_bytes(),
            )
        };
        if let Err(error) = mutation_result {
            persist_unknown(
                &state,
                &transaction_name,
                &mut transaction,
                &result.request_id,
            )?;
            result.mutations[0].state = MutationState::Unknown;
            return Err(MutationError::after(error));
        }
        let source_present = entry_identity_at(&target.parent, &target.raw_name)
            .map_err(MutationError::after)?
            .is_some();
        let destination = if target.entry_type == EntryType::Symlink {
            None
        } else {
            entry_identity_at(quarantine_directory, quarantine_name.as_bytes())
                .map_err(MutationError::after)?
        };
        if source_present
            || (target.entry_type == EntryType::Symlink && destination.is_some())
            || (target.entry_type != EntryType::Symlink
                && destination.as_ref() != Some(&target.identity))
        {
            persist_unknown(
                &state,
                &transaction_name,
                &mut transaction,
                &result.request_id,
            )?;
            result.mutations[0].state = MutationState::Unknown;
            return Err(MutationError::after(
                "quarantine detach post-state could not be proven",
            ));
        }
        settle_step(
            &state,
            &transaction_name,
            &mut transaction,
            step_index,
            PostStateV1 {
                source_present,
                destination,
                destination_content_digest: None,
            },
            &result.request_id,
        )?;
    }
    result.mutations[0].state = MutationState::Applied;

    let audit_sequence = append_audit(
        &mut state,
        context,
        &result.request_id,
        &audit_command,
        ResultStatus::Success,
    )
    .map_err(|error| MutationError::after(error.message))?;
    transaction.state = TransactionState::Settled;
    transaction.audit_sequence = Some(audit_sequence);
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        &transaction,
        &result.request_id,
    )
    .map_err(MutationError::after)?;
    remove_transaction_fence(&state, &transaction)?;
    result.mutations[0].state = MutationState::Settled;
    diagnostic(
        result,
        if quarantine {
            "deny_quarantine_settled"
        } else {
            "deny_settled"
        },
        omegon_maintenance_contracts::Severity::Info,
        scope_name(target.scope),
        if quarantine {
            "deny record settled and the inert entry was detached; runtime enforcement lands in task 0.7"
        } else {
            "deny record settled without terminating running behavior; runtime enforcement lands in task 0.7"
        },
        Some(json!({
            "selector": selector,
            "entry_key": target.entry_key,
            "generation": next.as_ref().map_or(current.generation, |state| state.generation),
        })),
    );
    Ok(())
}

struct ResolvedSession {
    directory: File,
    framing: Vec<SelectedSessionFraming>,
    workspace_key: AuthorityKey,
    session_key: AuthorityKey,
}

struct SelectedSessionFraming {
    name: Vec<u8>,
    identity: FileIdentityV1,
    digest: AuthorityKey,
}

fn quarantine_session(
    context: &Context,
    session_id: &str,
    dry_run: bool,
    result: &mut MaintenanceResultV1,
) -> Result<(), MutationError> {
    if !canonical_session_id(session_id) {
        return Err(MutationError::before("session ID is not canonical"));
    }
    let workspace = context
        .workspace
        .as_ref()
        .ok_or_else(|| MutationError::before("session quarantine requires --workspace"))?;
    let normalized = normalize_workspace_path(os_bytes(workspace.path.as_os_str()))
        .map_err(MutationError::before)?;
    let workspace_key = workspace_key("unix", &normalized);
    let authority_session_key = session_key(session_id, workspace_key);
    let mut state = bootstrap_state(context, &result.request_id)?;
    let domain_key = session_domain_key(authority_session_key);
    let lock_name = format!("session-{authority_session_key}.lock");
    let _domain_lock = acquire_domain_lock(&state, lock_name.as_bytes(), context)?;
    let deny_name = format!("{authority_session_key}.json");
    let semantics = session_command_semantics(context, session_id, workspace_key)?;
    let fingerprint = command_fingerprint(&semantics).map_err(MutationError::before)?;
    let audit_command = audit_event_name("session.quarantine", fingerprint);
    let requested_audit_command = if dry_run {
        audit_event_name("session.quarantine.dry_run", fingerprint)
    } else {
        audit_command.clone()
    };
    let transaction_name = format!("{}.json", result.request_id);
    let fence_name = format!("{}.json", domain_key);
    if let Some(existing) =
        read_record_at::<TransactionV1>(&state.transactions, transaction_name.as_bytes())
            .map_err(MutationError::before)?
    {
        if existing.domain_key != domain_key {
            return Err(MutationError::before(
                "request_id_conflict: request transaction belongs to a different domain",
            ));
        }
        return settle_existing_terminal(
            &mut state,
            context,
            existing,
            fingerprint,
            &fence_name,
            "session.quarantine",
            result,
        )
        .and_then(|settled| {
            if settled {
                Ok(())
            } else {
                Err(MutationError::after(
                    "existing session transaction did not reach a terminal state",
                ))
            }
        });
    }
    if let Some(outcome) = existing_audit_event(
        &state,
        context,
        &result.request_id,
        &requested_audit_command,
    )? {
        result.status = outcome;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: "session.quarantine".into(),
            state: if dry_run {
                MutationState::Planned
            } else {
                MutationState::Settled
            },
            retry_safe: true,
        });
        return Ok(());
    }
    if let Some(existing_transaction) =
        unresolved_domain_transaction(&state, domain_key, &result.request_id, context)?
        && settle_existing_terminal(
            &mut state,
            context,
            existing_transaction,
            fingerprint,
            &fence_name,
            "session.quarantine",
            result,
        )?
    {
        return Ok(());
    }
    if read_record_at::<FenceV1>(&state.fences, fence_name.as_bytes())
        .map_err(MutationError::before)?
        .is_some()
    {
        return Err(MutationError::before(
            "request transaction or session-domain fence already exists",
        ));
    }
    let target = resolve_session(context, session_id)?;
    if target.session_key != authority_session_key || target.workspace_key != workspace_key {
        return Err(MutationError::before(
            "session authority changed while acquiring its domain lock",
        ));
    }
    let existing: Option<SessionDenyRecordV1> =
        read_record_at(&state.session_deny, deny_name.as_bytes()).map_err(MutationError::before)?;
    if let Some(existing) = &existing
        && (existing.session_key != target.session_key
            || existing.session_id != session_id
            || existing.workspace_key != target.workspace_key)
    {
        return Err(MutationError::before(
            "session deny record does not belong to its authority filename",
        ));
    }

    if dry_run {
        append_audit(
            &mut state,
            context,
            &result.request_id,
            &requested_audit_command,
            ResultStatus::Success,
        )?;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: "session.quarantine".into(),
            state: MutationState::Planned,
            retry_safe: true,
        });
        diagnostic(
            result,
            "session_quarantine_planned",
            omegon_maintenance_contracts::Severity::Info,
            "session",
            "session resume-deny creation is planned; session framing bytes remain unchanged",
            Some(json!({"session_id": session_id, "session_key": target.session_key})),
        );
        return Ok(());
    }
    if existing.is_some() {
        append_audit(
            &mut state,
            context,
            &result.request_id,
            &audit_command,
            ResultStatus::Success,
        )?;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: "session.quarantine".into(),
            state: MutationState::Settled,
            retry_safe: true,
        });
        diagnostic(
            result,
            "session_deny_already_settled",
            omegon_maintenance_contracts::Severity::Info,
            "session",
            "session resume was already denied; session framing bytes remain unchanged",
            Some(json!({"session_id": session_id, "session_key": target.session_key})),
        );
        return Ok(());
    }

    revalidate_session(context, &target)?;
    let record = SessionDenyRecordV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "session_deny".into(),
        record_id: derive_key(
            "session-deny",
            &[target.session_key.as_bytes(), result.request_id.as_bytes()],
        ),
        session_key: target.session_key,
        session_id: session_id.into(),
        workspace_key: target.workspace_key,
        state: SessionDenyState::ResumeDenied,
        request_id: result.request_id.clone(),
        created_at: now_utc(),
    };
    let intended_digest = canonical_digest(&record).map_err(MutationError::before)?;
    let now = now_utc();
    let transaction_record_id = derive_key("transaction", &[result.request_id.as_bytes()]);
    let (deny_basename_bytes, deny_basename_digest) =
        TransactionStepV1::encode_basename(deny_name.as_bytes()).map_err(MutationError::before)?;
    let mut transaction = TransactionV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "transaction".into(),
        record_id: transaction_record_id,
        request_id: result.request_id.clone(),
        command_fingerprint: fingerprint,
        domain_key,
        roots: root_identities(context)?,
        steps: vec![TransactionStepV1 {
            kind: TransactionStepKind::SessionDenyCreate,
            parent: path_identity(&state.session_deny).map_err(MutationError::before)?,
            basename_bytes: deny_basename_bytes,
            basename_digest: deny_basename_digest,
            destination_parent: None,
            destination_basename_bytes: None,
            destination_basename_digest: None,
            expected_existing: None,
            expected_absence: true,
            intended_content_digest: Some(intended_digest),
            state: TransactionStepState::Prepared,
            observed: None,
        }],
        state: TransactionState::Prepared,
        created_at: now.clone(),
        updated_at: now,
        audit_sequence: None,
    };
    let fence = FenceV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "fence".into(),
        record_id: derive_key(
            "fence",
            &[domain_key.as_bytes(), transaction_record_id.as_bytes()],
        ),
        domain_key,
        transaction_record_id,
        state: FenceState::Active,
    };
    create_record_no_replace_at(
        &state.transactions,
        transaction_name.as_bytes(),
        &transaction,
        &result.request_id,
    )
    .map_err(MutationError::before)?;
    create_record_no_replace_at(
        &state.fences,
        fence_name.as_bytes(),
        &fence,
        &result.request_id,
    )
    .map_err(MutationError::before)?;
    result.mutations.push(MutationResultV1 {
        domain_key,
        kind: "session.quarantine".into(),
        state: MutationState::Prepared,
        retry_safe: true,
    });

    revalidate_session(context, &target)?;
    dispatch_step(
        &mut state,
        context,
        &transaction_name,
        &mut transaction,
        0,
        &result.request_id,
        &audit_command,
    )?;
    result.mutations[0].state = MutationState::Dispatched;
    result.mutations[0].retry_safe = false;
    if let Err(error) = create_record_no_replace_at(
        &state.session_deny,
        deny_name.as_bytes(),
        &record,
        &result.request_id,
    ) {
        persist_unknown(
            &state,
            &transaction_name,
            &mut transaction,
            &result.request_id,
        )?;
        result.mutations[0].state = MutationState::Unknown;
        return Err(MutationError::after(error));
    }
    let observed_identity = record_identity_at(&state.session_deny, deny_name.as_bytes())
        .map_err(MutationError::after)?
        .ok_or_else(|| MutationError::after("session deny absent after create"))?;
    let observed: SessionDenyRecordV1 = read_record_at(&state.session_deny, deny_name.as_bytes())
        .map_err(MutationError::after)?
        .ok_or_else(|| MutationError::after("session deny absent after create"))?;
    if canonical_digest(&observed).map_err(MutationError::after)? != intended_digest {
        persist_unknown(
            &state,
            &transaction_name,
            &mut transaction,
            &result.request_id,
        )?;
        result.mutations[0].state = MutationState::Unknown;
        return Err(MutationError::after(
            "session deny content differs after creation",
        ));
    }
    settle_step(
        &state,
        &transaction_name,
        &mut transaction,
        0,
        PostStateV1 {
            source_present: false,
            destination: Some(observed_identity),
            destination_content_digest: Some(intended_digest),
        },
        &result.request_id,
    )?;
    result.mutations[0].state = MutationState::Applied;
    let audit_sequence = append_audit(
        &mut state,
        context,
        &result.request_id,
        &audit_command,
        ResultStatus::Success,
    )
    .map_err(|error| MutationError::after(error.message))?;
    transaction.state = TransactionState::Settled;
    transaction.audit_sequence = Some(audit_sequence);
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        &transaction,
        &result.request_id,
    )
    .map_err(MutationError::after)?;
    remove_transaction_fence(&state, &transaction)?;
    result.mutations[0].state = MutationState::Settled;
    diagnostic(
        result,
        "session_quarantine_settled",
        omegon_maintenance_contracts::Severity::Info,
        "session",
        "session resume-deny record settled and session framing bytes were preserved; runtime enforcement lands in task 0.7",
        Some(json!({"session_id": session_id, "session_key": target.session_key})),
    );
    Ok(())
}

fn resolve_session(context: &Context, session_id: &str) -> Result<ResolvedSession, MutationError> {
    if !canonical_session_id(session_id) {
        return Err(MutationError::before("session ID is not canonical"));
    }
    let workspace = context
        .workspace
        .as_ref()
        .ok_or_else(|| MutationError::before("session quarantine requires --workspace"))?;
    let normalized = normalize_workspace_path(os_bytes(workspace.path.as_os_str()))
        .map_err(MutationError::before)?;
    let workspace_key = workspace_key("unix", &normalized);
    let Some(sessions) = super::open_dir_at(&context.config_home.file, "sessions", context)
        .map_err(MutationError::before)?
    else {
        return Err(MutationError::before("session root is absent"));
    };
    let mut directories =
        read_dir_at(&sessions, context, MAX_ENTRIES).map_err(MutationError::before)?;
    directories.sort_by(|left, right| left.name.cmp(&right.name));
    let catalog_name = format!("{session_id}.catalog.v1.json").into_bytes();
    let metadata_name = format!("{session_id}.meta.json").into_bytes();
    let snapshot_name = format!("{session_id}.json").into_bytes();
    let mut candidates = Vec::new();
    let mut examined = directories.len();
    for entry in directories {
        if entry.kind != EntryType::Directory {
            continue;
        }
        let Some(directory) = super::open_child_dir_at(&sessions, &entry.name, context)
            .map_err(MutationError::before)?
        else {
            continue;
        };
        let entries = read_dir_at(&directory, context, MAX_ENTRIES - examined)
            .map_err(MutationError::before)?;
        examined += entries.len();
        candidates.push((directory, entries));
    }

    let mut matched = None;
    for (directory, entries) in &candidates {
        if !entries.iter().any(|entry| entry.name == catalog_name) {
            continue;
        }
        if let Some(catalog) = super::inspect_session_catalog(
            directory,
            &catalog_name,
            session_id,
            Some(&normalized),
            context,
        )
        .map_err(MutationError::before)?
        {
            if matched.is_some() {
                return Err(MutationError::before(
                    "session ID and workspace matched multiple catalogs",
                ));
            }
            matched = Some(ResolvedSession {
                directory: directory.try_clone().map_err(MutationError::before)?,
                framing: vec![SelectedSessionFraming {
                    name: catalog_name.clone(),
                    identity: catalog.catalog_identity,
                    digest: catalog.catalog_digest,
                }],
                workspace_key,
                session_key: session_key(session_id, workspace_key),
            });
        }
    }
    if let Some(matched) = matched {
        return Ok(matched);
    }

    let mut matched = None;
    for (directory, entries) in candidates {
        if entries.iter().any(|entry| entry.name == catalog_name)
            || !entries.iter().any(|entry| entry.name == metadata_name)
        {
            continue;
        }
        if let Some(pair) = super::inspect_session_pair(
            &directory,
            &metadata_name,
            session_id,
            Some(&normalized),
            context,
        )
        .map_err(MutationError::before)?
        {
            if matched.is_some() {
                return Err(MutationError::before(
                    "session ID and workspace matched multiple legacy pairs",
                ));
            }
            matched = Some(ResolvedSession {
                directory,
                framing: vec![
                    SelectedSessionFraming {
                        name: metadata_name.clone(),
                        identity: pair.metadata_identity,
                        digest: pair.metadata_digest,
                    },
                    SelectedSessionFraming {
                        name: snapshot_name.clone(),
                        identity: pair.snapshot_identity,
                        digest: pair.snapshot_digest,
                    },
                ],
                workspace_key,
                session_key: session_key(session_id, workspace_key),
            });
        }
    }
    matched.ok_or_else(|| MutationError::before("session catalog or legacy pair was not found"))
}

fn revalidate_session(context: &Context, target: &ResolvedSession) -> Result<(), MutationError> {
    for entry in &target.framing {
        let current = super::read_bounded_regular_at(
            &target.directory,
            &entry.name,
            super::MAX_METADATA_BYTES,
            context,
        )
        .map_err(MutationError::after)?;
        if current.identity != entry.identity || current.digest != entry.digest {
            return Err(MutationError::after(
                "session framing identity or content changed before deny dispatch",
            ));
        }
    }
    Ok(())
}

fn session_command_semantics(
    context: &Context,
    session_id: &str,
    workspace: AuthorityKey,
) -> Result<CommandSemanticsV1, MutationError> {
    let roots = root_identities(context)?;
    let mut semantic_options = BTreeMap::new();
    semantic_options.insert("workspace_key".into(), Value::String(workspace.to_hex()));
    Ok(CommandSemanticsV1 {
        command: "session.quarantine".into(),
        semantic_options,
        root_keys: roots.into_iter().map(|root| root.key).collect(),
        selector: Some(session_id.into()),
    })
}

struct ResourceCandidate {
    directory: File,
    runtime_id: Vec<u8>,
    identity: FileIdentityV1,
}

enum ResourceDecision {
    Prune(&'static str),
    Retain(&'static str),
    Unverifiable(String),
}

fn prune_resources(
    context: &Context,
    dry_run: bool,
    result: &mut MaintenanceResultV1,
) -> Result<(), MutationError> {
    let workspace = context
        .workspace
        .as_ref()
        .ok_or_else(|| MutationError::before("resource pruning requires --workspace"))?;
    let normalized = normalize_workspace_path(os_bytes(workspace.path.as_os_str()))
        .map_err(MutationError::before)?;
    let workspace_key = workspace_key("unix", &normalized);
    let domain_key = resource_domain_key(workspace_key);
    let mut state = bootstrap_state(context, &result.request_id)?;
    let lock_name = format!("resource-{workspace_key}.lock");
    let _domain_lock = acquire_domain_lock(&state, lock_name.as_bytes(), context)?;
    let mut semantic_options = BTreeMap::new();
    semantic_options.insert(
        "workspace_key".into(),
        Value::String(workspace_key.to_hex()),
    );
    let roots = root_identities(context)?;
    let fingerprint = command_fingerprint(&CommandSemanticsV1 {
        command: "resource.prune_stale".into(),
        semantic_options,
        root_keys: roots.iter().map(|root| root.key).collect(),
        selector: None,
    })
    .map_err(MutationError::before)?;
    let audit_command = audit_event_name("resource.prune_stale", fingerprint);
    let requested_audit_command = if dry_run {
        audit_event_name("resource.prune_stale.dry_run", fingerprint)
    } else {
        audit_command.clone()
    };
    let transaction_name = format!("{}.json", result.request_id);
    let fence_name = format!("{}.json", domain_key);
    if let Some(existing) =
        read_record_at::<TransactionV1>(&state.transactions, transaction_name.as_bytes())
            .map_err(MutationError::before)?
    {
        if existing.domain_key != domain_key {
            return Err(MutationError::before(
                "request_id_conflict: request transaction belongs to a different domain",
            ));
        }
        return settle_existing_terminal(
            &mut state,
            context,
            existing,
            fingerprint,
            &fence_name,
            "resource.prune_stale",
            result,
        )
        .and_then(|settled| {
            if settled {
                Ok(())
            } else {
                Err(MutationError::after(
                    "existing resource transaction did not reach a terminal state",
                ))
            }
        });
    }
    if let Some(outcome) = existing_audit_event(
        &state,
        context,
        &result.request_id,
        &requested_audit_command,
    )? {
        result.status = outcome;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: "resource.prune_stale".into(),
            state: if dry_run {
                MutationState::Planned
            } else {
                MutationState::Settled
            },
            retry_safe: true,
        });
        return Ok(());
    }
    if let Some(existing) =
        unresolved_domain_transaction(&state, domain_key, &result.request_id, context)?
        && settle_existing_terminal(
            &mut state,
            context,
            existing,
            fingerprint,
            &fence_name,
            &audit_command,
            result,
        )?
    {
        return Ok(());
    }
    if read_record_at::<FenceV1>(&state.fences, fence_name.as_bytes())
        .map_err(MutationError::before)?
        .is_some()
    {
        return Err(MutationError::before(
            "request transaction or resource-domain fence already exists",
        ));
    }
    let current_boot = current_boot_id();
    let mut candidates = Vec::new();
    if let Some(runtime) = super::open_dir_at(&workspace.file, ".omegon/runtime", context)
        .map_err(MutationError::before)?
    {
        let mut entries =
            read_dir_at(&runtime, context, MAX_ENTRIES).map_err(MutationError::before)?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let mut examined = entries.len();
        for entry in entries {
            if entry.kind != EntryType::Directory {
                continue;
            }
            let Some(directory) = super::open_child_dir_at(&runtime, &entry.name, context)
                .map_err(MutationError::before)?
            else {
                continue;
            };
            let children = read_dir_at(&directory, context, MAX_ENTRIES - examined)
                .map_err(MutationError::before)?;
            examined += children.len();
            let Some(ownership) = children
                .iter()
                .find(|child| child.name == b"ownership-v1.json")
            else {
                continue;
            };
            if ownership.kind != EntryType::File {
                diagnose_resource_unverifiable(result, "ownership record is not a regular file");
                continue;
            }
            let (record, identity): (OwnershipRecordV1, FileIdentityV1) =
                match read_record_with_identity_at(&directory, &ownership.name) {
                    Ok(Some(record)) => record,
                    Ok(None) => continue,
                    Err(error) => {
                        diagnose_resource_unverifiable(result, &error.to_string());
                        continue;
                    }
                };
            if record.runtime_id.as_bytes() != entry.name
                || record.workspace_key != workspace_key
                || !super::complete_ownership_evidence(&record)
            {
                diagnose_resource_unverifiable(
                    result,
                    "ownership record identity or required evidence is incomplete",
                );
                continue;
            }
            match resource_decision(&record, current_boot.as_deref()) {
                ResourceDecision::Prune(reason) => {
                    diagnostic(
                        result,
                        "resource_prune_candidate",
                        omegon_maintenance_contracts::Severity::Info,
                        "resource",
                        &format!("runtime {} is safely prunable: {reason}", record.runtime_id),
                        None,
                    );
                    candidates.push(ResourceCandidate {
                        directory,
                        runtime_id: entry.name,
                        identity,
                    });
                }
                ResourceDecision::Retain(reason) => diagnostic(
                    result,
                    "resource_retained",
                    omegon_maintenance_contracts::Severity::Info,
                    "resource",
                    &format!("runtime {} is retained: {reason}", record.runtime_id),
                    None,
                ),
                ResourceDecision::Unverifiable(reason) => {
                    diagnose_resource_unverifiable(result, &reason)
                }
            }
        }
    }

    if dry_run || candidates.is_empty() {
        append_audit(
            &mut state,
            context,
            &result.request_id,
            &requested_audit_command,
            result.status,
        )?;
        result.mutations.push(MutationResultV1 {
            domain_key,
            kind: "resource.prune_stale".into(),
            state: if dry_run {
                MutationState::Planned
            } else {
                MutationState::Settled
            },
            retry_safe: true,
        });
        return Ok(());
    }

    let now = now_utc();
    let transaction_record_id = derive_key("transaction", &[result.request_id.as_bytes()]);
    let mut transaction = TransactionV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "transaction".into(),
        record_id: transaction_record_id,
        request_id: result.request_id.clone(),
        command_fingerprint: fingerprint,
        domain_key,
        roots,
        steps: candidates
            .iter()
            .map(|candidate| {
                let (basename_bytes, basename_digest) =
                    TransactionStepV1::encode_basename(b"ownership-v1.json")?;
                Ok(TransactionStepV1 {
                    kind: TransactionStepKind::ResourceRecordPrune,
                    parent: path_identity(&candidate.directory)?,
                    basename_bytes,
                    basename_digest,
                    destination_parent: None,
                    destination_basename_bytes: None,
                    destination_basename_digest: None,
                    expected_existing: Some(candidate.identity.clone()),
                    expected_absence: false,
                    intended_content_digest: None,
                    state: TransactionStepState::Prepared,
                    observed: None,
                })
            })
            .collect::<omegon_maintenance_contracts::Result<Vec<_>>>()
            .map_err(MutationError::before)?,
        state: TransactionState::Prepared,
        created_at: now.clone(),
        updated_at: now,
        audit_sequence: None,
    };
    let fence = FenceV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "fence".into(),
        record_id: derive_key(
            "fence",
            &[domain_key.as_bytes(), transaction_record_id.as_bytes()],
        ),
        domain_key,
        transaction_record_id,
        state: FenceState::Active,
    };
    create_record_no_replace_at(
        &state.transactions,
        transaction_name.as_bytes(),
        &transaction,
        &result.request_id,
    )
    .map_err(MutationError::before)?;
    create_record_no_replace_at(
        &state.fences,
        fence_name.as_bytes(),
        &fence,
        &result.request_id,
    )
    .map_err(MutationError::before)?;
    result.mutations.push(MutationResultV1 {
        domain_key,
        kind: "resource.prune_stale".into(),
        state: MutationState::Prepared,
        retry_safe: true,
    });
    for (index, candidate) in candidates.iter().enumerate() {
        let current = record_identity_at(&candidate.directory, b"ownership-v1.json")
            .map_err(MutationError::before)?;
        if current.as_ref() != Some(&candidate.identity) {
            if index > 0 {
                return abort_prepared_after_partial(
                    &mut state,
                    context,
                    &mut transaction,
                    index,
                    &audit_command,
                    "ownership identity changed after an earlier prune step settled",
                );
            }
            return Err(MutationError::before(format!(
                "runtime {} ownership identity changed before prune",
                String::from_utf8_lossy(&candidate.runtime_id)
            )));
        }
        dispatch_step(
            &mut state,
            context,
            &transaction_name,
            &mut transaction,
            index,
            &result.request_id,
            &audit_command,
        )?;
        result.mutations[0].state = MutationState::Dispatched;
        result.mutations[0].retry_safe = false;
        if let Err(error) = remove_record_at(&candidate.directory, b"ownership-v1.json") {
            persist_unknown(
                &state,
                &transaction_name,
                &mut transaction,
                &result.request_id,
            )?;
            result.mutations[0].state = MutationState::Unknown;
            return Err(MutationError::after(error));
        }
        if record_identity_at(&candidate.directory, b"ownership-v1.json")
            .map_err(MutationError::after)?
            .is_some()
        {
            persist_unknown(
                &state,
                &transaction_name,
                &mut transaction,
                &result.request_id,
            )?;
            result.mutations[0].state = MutationState::Unknown;
            return Err(MutationError::after(
                "ownership record remains present after prune dispatch",
            ));
        }
        settle_step(
            &state,
            &transaction_name,
            &mut transaction,
            index,
            PostStateV1 {
                source_present: false,
                destination: None,
                destination_content_digest: None,
            },
            &result.request_id,
        )?;
    }
    result.mutations[0].state = MutationState::Applied;
    let audit_sequence = append_audit(
        &mut state,
        context,
        &result.request_id,
        &audit_command,
        result.status,
    )
    .map_err(|error| MutationError::after(error.message))?;
    transaction.state = TransactionState::Settled;
    transaction.audit_sequence = Some(audit_sequence);
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        &transaction,
        &result.request_id,
    )
    .map_err(MutationError::after)?;
    remove_transaction_fence(&state, &transaction)?;
    result.mutations[0].state = MutationState::Settled;
    Ok(())
}

fn diagnose_resource_unverifiable(result: &mut MaintenanceResultV1, message: &str) {
    diagnostic(
        result,
        "resource_record_unverifiable",
        omegon_maintenance_contracts::Severity::Warning,
        "resource",
        message,
        None,
    );
    result.status = ResultStatus::Degraded;
}

fn resource_decision(record: &OwnershipRecordV1, current_boot: Option<&str>) -> ResourceDecision {
    let heartbeat = match DateTime::parse_from_rfc3339(&record.heartbeat_utc) {
        Ok(value) => value.with_timezone(&Utc),
        Err(error) => return ResourceDecision::Unverifiable(error.to_string()),
    };
    let age = Utc::now().signed_duration_since(heartbeat).num_seconds();
    if age < -300 {
        return ResourceDecision::Unverifiable("heartbeat has unverifiable future skew".into());
    }
    if age <= 300 {
        return ResourceDecision::Retain("heartbeat has not expired");
    }
    if !ownership_tokens_match_current_platform(record) {
        return ResourceDecision::Unverifiable(
            "ownership record uses unsupported platform identity tokens".into(),
        );
    }
    let Some(current_boot) = current_boot else {
        return ResourceDecision::Unverifiable("current boot identity is unavailable".into());
    };
    if record.boot_id != current_boot {
        return ResourceDecision::Prune("expired record belongs to a different boot");
    }
    let Some(current_ticks) = current_monotonic_ns() else {
        return ResourceDecision::Unverifiable("current monotonic clock is unavailable".into());
    };
    let Some(monotonic_age) = current_ticks.checked_sub(record.heartbeat_monotonic_ticks) else {
        return ResourceDecision::Unverifiable("heartbeat monotonic clock is in the future".into());
    };
    if monotonic_age <= 300_000_000_000 {
        return ResourceDecision::Unverifiable(
            "UTC and monotonic heartbeat expiry evidence disagree".into(),
        );
    }
    match current_process_token(record.pid) {
        ProcessObservation::Absent => ResourceDecision::Prune("recorded PID is absent"),
        ProcessObservation::Present(token) if token != record.process_start_token => {
            ResourceDecision::Prune("recorded PID was reused with a different start token")
        }
        ProcessObservation::Present(_) => {
            ResourceDecision::Retain("process identity still matches")
        }
        ProcessObservation::Unavailable(message) => ResourceDecision::Unverifiable(message),
    }
}

fn ownership_tokens_match_current_platform(record: &OwnershipRecordV1) -> bool {
    #[cfg(target_os = "macos")]
    let prefix = "macos:";
    #[cfg(target_os = "linux")]
    let prefix = "linux:";
    record.boot_id.starts_with(prefix) && record.process_start_token.starts_with(prefix)
}

enum ProcessObservation {
    Absent,
    Present(String),
    Unavailable(String),
}

fn current_monotonic_ns() -> Option<u64> {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: value is a valid writable timespec.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } != 0 {
        return None;
    }
    u64::try_from(value.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(value.tv_nsec).ok()?)
}

#[cfg(target_os = "macos")]
fn current_boot_id() -> Option<String> {
    use std::ffi::CString;

    let name = CString::new("kern.boottime").ok()?;
    let mut value = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    // SAFETY: value/size are valid writable buffers for sysctlbyname.
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut libc::timeval).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size != std::mem::size_of::<libc::timeval>()
    {
        return None;
    }
    Some(format!("macos:{}:{}", value.tv_sec, value.tv_usec))
}

#[cfg(target_os = "linux")]
fn current_boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|value| format!("linux:{}", value.trim()))
}

#[cfg(target_os = "macos")]
fn current_process_token(pid: u32) -> ProcessObservation {
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    // SAFETY: info is a valid writable proc_bsdinfo buffer.
    let read = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            std::mem::size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if read == std::mem::size_of::<libc::proc_bsdinfo>() as i32 {
        return ProcessObservation::Present(format!(
            "macos:{}:{}",
            info.pbi_start_tvsec, info.pbi_start_tvusec
        ));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        ProcessObservation::Absent
    } else {
        ProcessObservation::Unavailable(format!("process identity unavailable: {error}"))
    }
}

#[cfg(target_os = "linux")]
fn current_process_token(pid: u32) -> ProcessObservation {
    let value = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProcessObservation::Absent;
        }
        Err(error) => return ProcessObservation::Unavailable(error.to_string()),
    };
    let Some(close) = value.rfind(')') else {
        return ProcessObservation::Unavailable("process stat command framing is malformed".into());
    };
    let Some(start) = value[close + 1..].split_whitespace().nth(19) else {
        return ProcessObservation::Unavailable("process stat lacks field 22".into());
    };
    if start.parse::<u64>().is_err() {
        return ProcessObservation::Unavailable("process start token is malformed".into());
    }
    ProcessObservation::Present(format!("linux:{start}"))
}

fn resolve_contribution(
    context: &Context,
    selector_text: &str,
    requested_scope: ScopeArg,
) -> Result<ResolvedContribution, String> {
    let selector: ContributionSelector = selector_text
        .parse()
        .map_err(|error: ContractError| error.to_string())?;
    let mut matched = None;
    let mut examined = 0_usize;
    for root in CONTRIBUTION_ROOTS {
        if !super::same_scope(root.scope, requested_scope)
            || matches!(&selector, ContributionSelector::Named { kind, .. } if *kind != root.kind)
        {
            continue;
        }
        let base = match root.scope {
            ScopeArg::User => &context.home,
            ScopeArg::Project => context
                .workspace
                .as_ref()
                .ok_or_else(|| "project scope requires --workspace".to_string())?,
        };
        let Some(parent) = super::open_dir_at(&base.file, root.path_suffix, context)? else {
            continue;
        };
        let parent_identity = path_identity(&parent).map_err(|error| error.to_string())?;
        let entries = read_dir_at(&parent, context, MAX_ENTRIES - examined)?;
        examined += entries.len();
        for entry in entries {
            if entry.name == b".omegon-maintain-quarantine" {
                continue;
            }
            let (logical_name, force_opaque) = match strip_suffix(&entry.name, root.suffix) {
                Some(name) => (name, false),
                None => (entry.name.as_slice(), true),
            };
            let generated = contribution_selector(
                root.kind,
                root.scope,
                &super::descriptor_path(&parent).map_err(|error| error.to_string())?,
                logical_name,
                &entry.name,
                force_opaque,
            );
            if generated != selector_text {
                continue;
            }
            if matched.is_some() {
                return Err("contribution selector is ambiguous".into());
            }
            let scope = scope_key(
                root.kind.as_str(),
                scope_name(root.scope),
                parent_identity.key,
            );
            let identity = entry_identity_at(&parent, &entry.name)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "contribution disappeared during resolution".to_string())?;
            matched = Some(ResolvedContribution {
                parent: parent.try_clone().map_err(|error| error.to_string())?,
                parent_identity: parent_identity.clone(),
                raw_name: entry.name.clone(),
                kind: root.kind,
                scope: root.scope,
                scope_key: scope,
                entry_key: entry_key(root.kind.as_str(), scope, &entry.name),
                identity,
                entry_type: entry.kind,
            });
        }
    }
    matched.ok_or_else(|| "contribution selector was not found".into())
}

fn find_contribution_scope_for_domain(
    context: &Context,
    requested_scope: ScopeArg,
    domain_key: AuthorityKey,
) -> Result<AuthorityKey, MutationError> {
    for root in CONTRIBUTION_ROOTS {
        if !super::same_scope(root.scope, requested_scope) {
            continue;
        }
        let base = match root.scope {
            ScopeArg::User => &context.home,
            ScopeArg::Project => context
                .workspace
                .as_ref()
                .ok_or_else(|| MutationError::before("project scope requires --workspace"))?,
        };
        let Some(parent) = super::open_dir_at(&base.file, root.path_suffix, context)
            .map_err(MutationError::before)?
        else {
            continue;
        };
        let parent_identity = path_identity(&parent).map_err(MutationError::before)?;
        let key = scope_key(
            root.kind.as_str(),
            scope_name(root.scope),
            parent_identity.key,
        );
        if contribution_domain_key(key) == domain_key {
            return Ok(key);
        }
    }
    Err(MutationError::after(
        "existing transaction domain does not match an admitted contribution root",
    ))
}

fn empty_deny_state(scope: AuthorityKey) -> DenyStateV1 {
    DenyStateV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "deny_state".into(),
        record_id: derive_key("deny-state", &[scope.as_bytes(), &0_u64.to_be_bytes()]),
        scope_key: scope,
        generation: 0,
        entries: BTreeMap::new(),
    }
}

fn next_deny_state(
    current: &DenyStateV1,
    target: &ResolvedContribution,
    request_id: &str,
) -> Result<DenyStateV1, MutationError> {
    let generation = current
        .generation
        .checked_add(1)
        .ok_or_else(|| MutationError::before("deny generation overflow"))?;
    let mut entries = current.entries.clone();
    for entry in entries.values_mut() {
        entry.generation = generation;
    }
    let record = DenyRecordV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "deny".into(),
        record_id: derive_key(
            "deny",
            &[
                target.scope_key.as_bytes(),
                target.entry_key.as_bytes(),
                request_id.as_bytes(),
            ],
        ),
        scope_key: target.scope_key,
        contribution_kind: target.kind,
        entry_key: target.entry_key,
        raw_name_digest: sha256_key(&target.raw_name),
        generation,
        state: DenyState::Denied,
        request_id: request_id.into(),
        created_at: now_utc(),
    };
    entries.insert(target.entry_key.to_hex(), record);
    let next = DenyStateV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "deny_state".into(),
        record_id: derive_key(
            "deny-state",
            &[target.scope_key.as_bytes(), &generation.to_be_bytes()],
        ),
        scope_key: target.scope_key,
        generation,
        entries,
    };
    next.validate().map_err(MutationError::before)?;
    Ok(next)
}

fn command_semantics(
    context: &Context,
    command: &str,
    selector: &str,
    scope: ScopeArg,
) -> Result<CommandSemanticsV1, MutationError> {
    let roots = root_identities(context)?;
    let mut semantic_options = BTreeMap::new();
    semantic_options.insert("scope".into(), Value::String(scope_name(scope).into()));
    Ok(CommandSemanticsV1 {
        command: command.into(),
        semantic_options,
        root_keys: roots.into_iter().map(|root| root.key).collect(),
        selector: Some(selector.into()),
    })
}

fn root_identities(context: &Context) -> Result<Vec<PathIdentityV1>, MutationError> {
    let mut roots = vec![
        path_identity(&context.home.file).map_err(MutationError::before)?,
        path_identity(&context.config_home.file).map_err(MutationError::before)?,
    ];
    if let Some(workspace) = &context.workspace {
        roots.push(path_identity(&workspace.file).map_err(MutationError::before)?);
    }
    Ok(roots)
}

fn bootstrap_state(
    context: &Context,
    candidate_installation_uuid: &str,
) -> Result<MaintenanceStateV1, MutationError> {
    let home_identity = path_identity(&context.home.file).map_err(MutationError::before)?;
    loop {
        if context.expired() {
            return Err(MutationError::before(
                "deadline expired while bootstrapping maintenance state",
            ));
        }
        match MaintenanceStateV1::bootstrap(
            &context.home.file,
            home_identity.clone(),
            candidate_installation_uuid,
            true,
        ) {
            Ok(state) => return Ok(state),
            Err(ContractError::Lock(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(MutationError::before(error)),
        }
    }
}

fn unresolved_domain_transaction(
    state: &MaintenanceStateV1,
    domain_key: AuthorityKey,
    request_id: &str,
    context: &Context,
) -> Result<Option<TransactionV1>, MutationError> {
    let entries =
        read_dir_at(&state.transactions, context, MAX_ENTRIES).map_err(MutationError::before)?;
    let mut matching_request = None;
    for entry in entries {
        if entry.kind != EntryType::File || !entry.name.ends_with(b".json") {
            continue;
        }
        let transaction: TransactionV1 = read_record_at(&state.transactions, &entry.name)
            .map_err(MutationError::before)?
            .ok_or_else(|| MutationError::before("transaction disappeared during scan"))?;
        if transaction.domain_key != domain_key {
            continue;
        }
        if transaction.request_id == request_id {
            matching_request = Some(transaction);
            continue;
        }
        if !matches!(
            transaction.state,
            TransactionState::Settled | TransactionState::Aborted
        ) {
            return Err(MutationError::before(
                "an unresolved orphan transaction blocks this mutation domain",
            ));
        }
    }
    Ok(matching_request)
}

fn settle_existing_terminal(
    state: &mut MaintenanceStateV1,
    context: &Context,
    mut transaction: TransactionV1,
    expected_fingerprint: AuthorityKey,
    fence_name: &str,
    operation: &str,
    result: &mut MaintenanceResultV1,
) -> Result<bool, MutationError> {
    if transaction.command_fingerprint != expected_fingerprint {
        return Err(MutationError::before(
            "request_id_conflict: request ID was already used with different command semantics",
        ));
    }
    let audit_command = audit_event_name(operation, transaction.command_fingerprint);
    if matches!(
        transaction.state,
        TransactionState::StepDispatched
            | TransactionState::StepSettled
            | TransactionState::TargetsSettled
            | TransactionState::Unknown
    ) {
        require_transaction_fence(state, fence_name, &transaction)?;
    }
    let transaction_name = format!("{}.json", transaction.request_id);
    if matches!(
        transaction.state,
        TransactionState::StepDispatched
            | TransactionState::StepSettled
            | TransactionState::Unknown
    ) {
        transaction = reconcile_active_steps(
            state,
            context,
            transaction,
            &transaction_name,
            &audit_command,
        )?;
    }
    match transaction.state {
        TransactionState::Settled => {
            remove_transaction_fence(state, &transaction)?;
        }
        TransactionState::TargetsSettled => {
            let request_id = transaction.request_id.clone();
            let audit_sequence = append_audit(
                state,
                context,
                &request_id,
                &audit_command,
                ResultStatus::Success,
            )
            .map_err(|error| MutationError::after(error.message))?;
            transaction.state = TransactionState::Settled;
            transaction.audit_sequence = Some(audit_sequence);
            transaction.updated_at = now_utc();
            replace_record_at(
                &state.transactions,
                transaction_name.as_bytes(),
                &transaction,
                &request_id,
            )
            .map_err(MutationError::after)?;
            remove_transaction_fence(state, &transaction)?;
        }
        TransactionState::Prepared => {
            let request_id = transaction.request_id.clone();
            let audit_sequence = append_audit(
                state,
                context,
                &request_id,
                &audit_command,
                ResultStatus::Failure,
            )?;
            transaction.steps[0].state = TransactionStepState::Aborted;
            transaction.state = TransactionState::Aborted;
            transaction.audit_sequence = Some(audit_sequence);
            transaction.updated_at = now_utc();
            replace_record_at(
                &state.transactions,
                transaction_name.as_bytes(),
                &transaction,
                &request_id,
            )?;
            remove_transaction_fence(state, &transaction)?;
            return Err(MutationError::before(
                "prepared transaction was durably aborted before target dispatch",
            ));
        }
        TransactionState::Aborted => {
            remove_transaction_fence(state, &transaction)?;
            return Err(MutationError::before(
                "request transaction was previously aborted",
            ));
        }
        TransactionState::StepDispatched
        | TransactionState::StepSettled
        | TransactionState::Unknown => unreachable!("active recovery returns a terminal frontier"),
    }
    result.mutations.push(MutationResultV1 {
        domain_key: transaction.domain_key,
        kind: operation.into(),
        state: MutationState::Settled,
        retry_safe: true,
    });
    diagnostic(
        result,
        "transaction_reconciled",
        omegon_maintenance_contracts::Severity::Info,
        "transaction",
        "existing request transaction was reconciled without repeating a target mutation",
        Some(json!({"request_id": transaction.request_id})),
    );
    Ok(true)
}

fn reconcile_active_steps(
    state: &mut MaintenanceStateV1,
    context: &Context,
    mut transaction: TransactionV1,
    transaction_name: &str,
    operation_name: &str,
) -> Result<TransactionV1, MutationError> {
    if let Some(index) = transaction.steps.iter().position(|step| {
        matches!(
            step.state,
            TransactionStepState::Dispatched | TransactionStepState::Unknown
        )
    }) {
        let observed = observe_step(context, &transaction.steps[index])?;
        let Some(observed) = observed else {
            let request_id = transaction.request_id.clone();
            persist_unknown(state, transaction_name, &mut transaction, &request_id)?;
            return Err(MutationError::after(
                "dispatched target post-state remains unknown; syscall was not repeated",
            ));
        };
        let request_id = transaction.request_id.clone();
        settle_step(
            state,
            transaction_name,
            &mut transaction,
            index,
            observed,
            &request_id,
        )?;
    }

    if transaction
        .steps
        .iter()
        .any(|step| step.state == TransactionStepState::Prepared)
    {
        let request_id = transaction.request_id.clone();
        let outcome = if transaction
            .steps
            .iter()
            .any(|step| step.state == TransactionStepState::Settled)
        {
            ResultStatus::Degraded
        } else {
            ResultStatus::Failure
        };
        let audit_sequence = append_audit(state, context, &request_id, operation_name, outcome)
            .map_err(|error| MutationError::after(error.message))?;
        if let Some(step) = transaction
            .steps
            .iter_mut()
            .find(|step| step.state == TransactionStepState::Prepared)
        {
            step.state = TransactionStepState::Aborted;
        }
        transaction.state = TransactionState::Aborted;
        transaction.audit_sequence = Some(audit_sequence);
        transaction.updated_at = now_utc();
        replace_record_at(
            &state.transactions,
            transaction_name.as_bytes(),
            &transaction,
            &request_id,
        )
        .map_err(MutationError::after)?;
        remove_transaction_fence(state, &transaction)?;
        return Err(if outcome == ResultStatus::Degraded {
            MutationError::degraded(
                "restart aborted prepared steps after an earlier target had settled",
            )
        } else {
            MutationError::before("restart aborted prepared steps before target dispatch")
        });
    }
    Ok(transaction)
}

fn observe_step(
    context: &Context,
    step: &TransactionStepV1,
) -> Result<Option<PostStateV1>, MutationError> {
    let parent = open_identity_directory(context, &step.parent)?;
    let basename = step.basename().map_err(MutationError::after)?;
    match step.kind {
        TransactionStepKind::DenyStateReplace | TransactionStepKind::SessionDenyCreate => {
            let Some(bytes) = read_bytes_at(&parent, &basename, MAX_RECORD_BYTES)
                .map_err(MutationError::after)?
            else {
                return Ok(None);
            };
            let Some(payload) = bytes.strip_suffix(b"\n") else {
                return Ok(None);
            };
            let digest = sha256_key(payload);
            if Some(digest) != step.intended_content_digest {
                return Ok(None);
            }
            let identity = entry_identity_at(&parent, &basename)
                .map_err(MutationError::after)?
                .ok_or_else(|| MutationError::after("record disappeared during observation"))?;
            Ok(Some(PostStateV1 {
                source_present: step.kind == TransactionStepKind::DenyStateReplace,
                destination: Some(identity),
                destination_content_digest: Some(digest),
            }))
        }
        TransactionStepKind::QuarantineDetach => {
            let source = entry_identity_at(&parent, &basename).map_err(MutationError::after)?;
            let destination_parent = open_identity_directory(
                context,
                step.destination_parent
                    .as_ref()
                    .expect("validated detach has destination parent"),
            )?;
            let destination_name = step
                .destination_basename()
                .map_err(MutationError::after)?
                .expect("validated detach has destination basename");
            let destination = entry_identity_at(&destination_parent, &destination_name)
                .map_err(MutationError::after)?;
            if source.is_none() && destination.as_ref() == step.expected_existing.as_ref() {
                Ok(Some(PostStateV1 {
                    source_present: false,
                    destination,
                    destination_content_digest: None,
                }))
            } else {
                Ok(None)
            }
        }
        TransactionStepKind::QuarantineSymlinkUnlink | TransactionStepKind::ResourceRecordPrune => {
            if entry_identity_at(&parent, &basename)
                .map_err(MutationError::after)?
                .is_none()
            {
                Ok(Some(PostStateV1 {
                    source_present: false,
                    destination: None,
                    destination_content_digest: None,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

fn open_identity_directory(
    context: &Context,
    expected: &PathIdentityV1,
) -> Result<File, MutationError> {
    let expected_path = expected.decoded_path().map_err(MutationError::after)?;
    let roots = [&context.home, &context.config_home]
        .into_iter()
        .chain(context.workspace.as_ref());
    for root in roots {
        let root_identity = path_identity(&root.file).map_err(MutationError::after)?;
        let root_path = root_identity.decoded_path().map_err(MutationError::after)?;
        if expected == &root_identity {
            return root.file.try_clone().map_err(MutationError::after);
        }
        if !expected_path.starts_with(&root_path)
            || expected_path.get(root_path.len()) != Some(&b'/')
        {
            continue;
        }
        let mut current = root.file.try_clone().map_err(MutationError::after)?;
        for component in expected_path[root_path.len() + 1..].split(|byte| *byte == b'/') {
            current = super::open_child_dir_at(&current, component, context)
                .map_err(MutationError::after)?
                .ok_or_else(|| MutationError::after("transaction parent is absent"))?;
        }
        if path_identity(&current).map_err(MutationError::after)? == *expected {
            return Ok(current);
        }
        return Err(MutationError::after(
            "transaction parent descriptor identity changed",
        ));
    }
    Err(MutationError::after(
        "transaction parent lies outside admitted roots",
    ))
}

fn remove_transaction_fence(
    state: &MaintenanceStateV1,
    transaction: &TransactionV1,
) -> Result<(), MutationError> {
    let fence_name = format!("{}.json", transaction.domain_key);
    let Some(fence) = read_record_at::<FenceV1>(&state.fences, fence_name.as_bytes())
        .map_err(MutationError::before)?
    else {
        return Ok(());
    };
    if fence.domain_key != transaction.domain_key
        || fence.transaction_record_id != transaction.record_id
    {
        return Err(MutationError::after(
            "domain fence does not belong to the settling transaction",
        ));
    }
    remove_record_at(&state.fences, fence_name.as_bytes()).map_err(MutationError::after)
}

fn require_transaction_fence(
    state: &MaintenanceStateV1,
    fence_name: &str,
    transaction: &TransactionV1,
) -> Result<(), MutationError> {
    let fence: FenceV1 = read_record_at(&state.fences, fence_name.as_bytes())
        .map_err(MutationError::after)?
        .ok_or_else(|| MutationError::after("active transaction fence is absent"))?;
    if fence.state != FenceState::Active
        || fence.domain_key != transaction.domain_key
        || fence.transaction_record_id != transaction.record_id
    {
        return Err(MutationError::after(
            "active transaction fence does not match its transaction",
        ));
    }
    Ok(())
}

fn acquire_domain_lock(
    state: &MaintenanceStateV1,
    name: &[u8],
    context: &Context,
) -> Result<ProtocolLock, MutationError> {
    loop {
        if context.expired() {
            return Err(MutationError::before(
                "deadline expired while acquiring contribution lock",
            ));
        }
        let bootstrap = match ProtocolLock::acquire_at(
            &state.locks,
            b"bootstrap.lock",
            LockMode::Exclusive,
            false,
            true,
        ) {
            Ok(lock) => lock,
            Err(ContractError::Lock(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(error) => return Err(MutationError::before(error)),
        };
        omegon_maintenance_contracts::ensure_home_recovery_settled(&state.root)
            .map_err(MutationError::before)?;
        let domain =
            match ProtocolLock::acquire_at(&state.locks, name, LockMode::Exclusive, true, true) {
                Ok(lock) => Ok(lock),
                Err(ContractError::Lock(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    ProtocolLock::acquire_at(&state.locks, name, LockMode::Exclusive, false, true)
                }
                Err(error) => Err(error),
            };
        drop(bootstrap);
        match domain {
            Ok(lock) => return Ok(lock),
            Err(ContractError::Lock(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(MutationError::before(error)),
        }
    }
}

fn acquire_audit_lock(
    state: &MaintenanceStateV1,
    context: &Context,
    allow_expired: bool,
) -> Result<ProtocolLock, MutationError> {
    loop {
        if context.expired() && !allow_expired {
            return Err(MutationError::before(
                "deadline expired while acquiring audit lock",
            ));
        }
        match ProtocolLock::acquire_at(
            &state.locks,
            b"audit.lock",
            LockMode::Exclusive,
            false,
            true,
        ) {
            Ok(lock) => return Ok(lock),
            Err(ContractError::Lock(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if allow_expired {
                    return Err(MutationError::before(
                        "audit lock is contended during deadline settlement",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(MutationError::before(error)),
        }
    }
}

fn append_audit(
    state: &mut MaintenanceStateV1,
    context: &Context,
    request_id: &str,
    command: &str,
    outcome: ResultStatus,
) -> Result<u64, MutationError> {
    append_audit_inner(state, context, request_id, command, outcome, false)
}

fn append_audit_after_deadline(
    state: &mut MaintenanceStateV1,
    context: &Context,
    request_id: &str,
    command: &str,
    outcome: ResultStatus,
) -> Result<u64, MutationError> {
    append_audit_inner(state, context, request_id, command, outcome, true)
}

fn existing_audit_event(
    state: &MaintenanceStateV1,
    context: &Context,
    request_id: &str,
    command: &str,
) -> Result<Option<ResultStatus>, MutationError> {
    let _audit_lock = acquire_audit_lock(state, context, false)?;
    let name = format!("{request_id}.json");
    let Some(receipt) = read_record_at::<AuditReceiptV1>(&state.audit_receipts, name.as_bytes())?
    else {
        return Ok(None);
    };
    if receipt.sequence >= state.installation.next_audit_sequence {
        remove_record_at(&state.audit_receipts, name.as_bytes()).map_err(MutationError::before)?;
        return Ok(None);
    }
    if receipt.installation_uuid != state.installation.installation_uuid {
        return Err(MutationError::before(
            "audit receipt belongs to a different installation",
        ));
    }
    if receipt.command == command {
        return Ok(Some(receipt.outcome));
    }
    Err(MutationError::before(
        "request_id_conflict: audit event has different command semantics",
    ))
}

fn append_audit_inner(
    state: &mut MaintenanceStateV1,
    context: &Context,
    request_id: &str,
    command: &str,
    outcome: ResultStatus,
    allow_expired: bool,
) -> Result<u64, MutationError> {
    let _audit_lock = acquire_audit_lock(state, context, allow_expired)?;
    append_audit_locked_inner(state, context, request_id, command, outcome, allow_expired)
}

/// Caller holds the existing audit protocol lock; used by whole-home recovery.
pub(super) fn append_home_recovery_audit_locked(
    state: &mut MaintenanceStateV1,
    context: &Context,
    request_id: &str,
) -> Result<u64, String> {
    append_audit_locked_inner(
        state,
        context,
        request_id,
        "home.recover",
        ResultStatus::Success,
        false,
    )
    .map_err(|error| error.message)
}

fn append_audit_locked_inner(
    state: &mut MaintenanceStateV1,
    context: &Context,
    request_id: &str,
    command: &str,
    outcome: ResultStatus,
    allow_expired: bool,
) -> Result<u64, MutationError> {
    state
        .reconcile_audit(request_id)
        .map_err(MutationError::before)?;
    if !allow_expired && context.expired() {
        return Err(MutationError::before(
            "deadline expired while validating the audit frontier",
        ));
    }
    let installation: omegon_maintenance_contracts::InstallationStateV1 =
        read_record_at(&state.root, b"state.json")?
            .ok_or_else(|| MutationError::before("installation state is absent"))?;
    let checkpoint: AuditCheckpointV1 = read_record_at(&state.audit, b"checkpoint.json")?
        .ok_or_else(|| MutationError::before("audit checkpoint is absent"))?;
    let sequence = installation.next_audit_sequence;
    let checkpoint_frontier = checkpoint
        .last_sequence
        .checked_add(1)
        .ok_or_else(|| MutationError::before("audit checkpoint sequence overflow"))?;
    if checkpoint.installation_uuid != installation.installation_uuid
        || checkpoint_frontier != sequence
    {
        return Err(MutationError::before(
            "audit checkpoint and installation sequence disagree",
        ));
    }
    let receipt_name = format!("{request_id}.json");
    if let Some(receipt) =
        read_record_at::<AuditReceiptV1>(&state.audit_receipts, receipt_name.as_bytes())?
    {
        if receipt.sequence > checkpoint.last_sequence {
            remove_record_at(&state.audit_receipts, receipt_name.as_bytes())
                .map_err(MutationError::before)?;
        } else if receipt.installation_uuid == installation.installation_uuid
            && receipt.command == command
            && receipt.outcome == outcome
        {
            return Ok(receipt.sequence);
        } else {
            return Err(MutationError::before(
                "request_id_conflict: audit event has different command semantics",
            ));
        }
    }
    state
        .prepare_audit_segment(sequence, request_id)
        .map_err(MutationError::before)?;
    let active_segment_first = ((sequence - 1) / AUDIT_SEGMENT_RECORDS)
        .checked_mul(AUDIT_SEGMENT_RECORDS)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| MutationError::before("audit segment sequence overflow"))?;
    let record = AuditRecordV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "audit".into(),
        record_id: derive_key(
            "audit",
            &[
                installation.installation_uuid.as_bytes(),
                &sequence.to_be_bytes(),
            ],
        ),
        installation_uuid: installation.installation_uuid.clone(),
        sequence,
        previous_digest: (sequence > 1).then_some(checkpoint.last_digest),
        request_id: request_id.into(),
        command: command.into(),
        outcome,
    };
    let bytes = canonical_json(&record).map_err(MutationError::before)?;
    let segment_name = format!("{active_segment_first}.jsonl");
    append_bytes_at(&state.audit_segments, segment_name.as_bytes(), &bytes)
        .map_err(MutationError::before)?;
    let digest = canonical_digest(&record).map_err(MutationError::before)?;
    let receipt = audit_receipt(&record, digest);
    create_record_no_replace_at(
        &state.audit_receipts,
        receipt_name.as_bytes(),
        &receipt,
        request_id,
    )
    .map_err(MutationError::before)?;
    let next_checkpoint = AuditCheckpointV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "audit_checkpoint".into(),
        record_id: derive_key(
            "audit-checkpoint",
            &[
                installation.installation_uuid.as_bytes(),
                &sequence.to_be_bytes(),
                digest.as_bytes(),
            ],
        ),
        installation_uuid: installation.installation_uuid.clone(),
        last_sequence: sequence,
        last_digest: digest,
    };
    replace_record_at(
        &state.audit,
        b"checkpoint.json",
        &next_checkpoint,
        request_id,
    )
    .map_err(MutationError::before)?;
    let mut next_installation = installation;
    next_installation.next_audit_sequence = sequence
        .checked_add(1)
        .ok_or_else(|| MutationError::before("audit sequence overflow"))?;
    replace_record_at(&state.root, b"state.json", &next_installation, request_id)
        .map_err(MutationError::before)?;
    state.installation = next_installation;
    Ok(sequence)
}

fn persist_unknown(
    state: &MaintenanceStateV1,
    transaction_name: &str,
    transaction: &mut TransactionV1,
    temporary_tag: &str,
) -> Result<(), MutationError> {
    transaction.state = TransactionState::Unknown;
    if let Some(step) = transaction
        .steps
        .iter_mut()
        .find(|step| step.state == TransactionStepState::Dispatched)
    {
        step.state = TransactionStepState::Unknown;
    }
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        transaction,
        temporary_tag,
    )
    .map_err(MutationError::after)
}

fn dispatch_step(
    state: &mut MaintenanceStateV1,
    context: &Context,
    transaction_name: &str,
    transaction: &mut TransactionV1,
    index: usize,
    temporary_tag: &str,
    operation: &str,
) -> Result<(), MutationError> {
    let prior_dispatch = transaction
        .steps
        .iter()
        .any(|step| step.state == TransactionStepState::Settled);
    if context.expired() {
        let outcome = if prior_dispatch {
            ResultStatus::Degraded
        } else {
            ResultStatus::Failure
        };
        let audit_sequence =
            append_audit_after_deadline(state, context, temporary_tag, operation, outcome)
                .map_err(|error| {
                    if prior_dispatch {
                        MutationError::degraded(error.message)
                    } else {
                        error
                    }
                })?;
        transaction.steps[index].state = TransactionStepState::Aborted;
        transaction.state = TransactionState::Aborted;
        transaction.audit_sequence = Some(audit_sequence);
        transaction.updated_at = now_utc();
        replace_record_at(
            &state.transactions,
            transaction_name.as_bytes(),
            transaction,
            temporary_tag,
        )?;
        remove_transaction_fence(state, transaction)?;
        return Err(if prior_dispatch {
            MutationError::degraded(
                "deadline expired after an earlier target settled; remaining step was aborted",
            )
        } else {
            MutationError::before(
                "deadline expired before dispatch; transaction was durably aborted",
            )
        });
    }
    transaction.state = TransactionState::StepDispatched;
    transaction.steps[index].state = TransactionStepState::Dispatched;
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        transaction,
        temporary_tag,
    )
    .map_err(|error| {
        if prior_dispatch {
            MutationError::after(error)
        } else {
            MutationError::before(error)
        }
    })
}

fn abort_prepared_after_partial(
    state: &mut MaintenanceStateV1,
    context: &Context,
    transaction: &mut TransactionV1,
    index: usize,
    audit_command: &str,
    message: &str,
) -> Result<(), MutationError> {
    let temporary_tag = transaction.request_id.clone();
    let transaction_name = format!("{}.json", transaction.request_id);
    let audit_sequence = append_audit(
        state,
        context,
        &temporary_tag,
        audit_command,
        ResultStatus::Degraded,
    )
    .map_err(|error| MutationError::degraded(error.message))?;
    transaction.steps[index].state = TransactionStepState::Aborted;
    transaction.state = TransactionState::Aborted;
    transaction.audit_sequence = Some(audit_sequence);
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        transaction,
        &temporary_tag,
    )
    .map_err(|error| MutationError::degraded(error.to_string()))?;
    remove_transaction_fence(state, transaction)
        .map_err(|error| MutationError::degraded(error.message))?;
    Err(MutationError::degraded(message))
}

fn settle_step(
    state: &MaintenanceStateV1,
    transaction_name: &str,
    transaction: &mut TransactionV1,
    index: usize,
    observed: PostStateV1,
    temporary_tag: &str,
) -> Result<(), MutationError> {
    transaction.steps[index].state = TransactionStepState::Settled;
    transaction.steps[index].observed = Some(observed);
    transaction.state = if transaction
        .steps
        .iter()
        .all(|step| step.state == TransactionStepState::Settled)
    {
        TransactionState::TargetsSettled
    } else {
        TransactionState::StepSettled
    };
    transaction.updated_at = now_utc();
    replace_record_at(
        &state.transactions,
        transaction_name.as_bytes(),
        transaction,
        temporary_tag,
    )
    .map_err(MutationError::after)
}

fn sha256_key(bytes: &[u8]) -> AuthorityKey {
    AuthorityKey::from_bytes(Sha256::digest(bytes).into())
}

fn audit_event_name(operation: &str, fingerprint: AuthorityKey) -> String {
    format!("{operation}#{}", fingerprint.to_hex())
}

fn now_utc() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod home_recovery_tests {
    use super::*;
    use std::{os::unix::fs::MetadataExt, time::Instant};

    #[test]
    fn home_recovery_pending_blocks_cached_maintenance_mutation() {
        let temporary = tempfile::tempdir().unwrap();
        let root = || {
            let file = omegon_maintenance_contracts::open_secure_root(temporary.path()).unwrap();
            let metadata = file.metadata().unwrap();
            super::super::AdmittedRoot {
                path: temporary.path().into(),
                file,
                device: metadata.dev(),
                inode: metadata.ino(),
                unsafe_permissions: false,
            }
        };
        let context = Context {
            home: root(),
            config_home: root(),
            workspace: None,
            started: Instant::now(),
            deadline: Duration::from_secs(10),
        };
        let state = MaintenanceStateV1::bootstrap(
            &context.home.file,
            path_identity(&context.home.file).unwrap(),
            "11111111-1111-1111-1111-111111111111",
            true,
        )
        .unwrap();
        let journal = omegon_maintenance_contracts::HomeRecoveryJournalV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "home_recovery_journal".into(),
            request_id: "22222222-2222-2222-2222-222222222222".into(),
            intent_key: derive_key("test", &[]),
            phase: omegon_maintenance_contracts::HomeRecoveryPhase::Prepared,
        };
        create_record_no_replace_at(&state.root, b"home-recovery.json", &journal, "test").unwrap();
        assert!(
            acquire_domain_lock(&state, b"contribution-fixture.lock", &context).is_err(),
            "cached maintenance state must honor pending recovery before domain mutation"
        );
    }
}

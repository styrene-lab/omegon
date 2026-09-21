//! Explicit whole-home identity recovery. It never enters normal runtime startup.
use super::{Context, HomeCommand, MAX_ENTRIES, diagnostic, fail, read_dir_at};
use omegon_maintenance_contracts::{
    ContractError, HomeContinuityV1, HomeRecoveryIntentV1, HomeRecoveryJournalV1,
    HomeRecoveryPhase, InstallationStateV1, LockMode, MaintenanceResultV1, MaintenanceStateV1,
    MutationResultV1, MutationState, PathIdentityV1, ProtocolLock, ResultStatus, SCHEMA_VERSION,
    Severity, TransactionState, TransactionV1, create_record_no_replace_at, derive_key,
    ensure_home_recovery_settled, home_binding_matches, open_or_create_secure_dir_at,
    open_secure_dir_at, open_secure_root, path_identity, read_record_at, replace_record_at,
    same_home_directory, stable_home_volume_uuid,
};
use serde_json::json;
use std::fs::File;

type RecoveryResult<T> = Result<T, String>;

pub(super) fn execute(
    command: &HomeCommand,
    context: &Context,
    dry_run: bool,
    result: &mut MaintenanceResultV1,
) {
    let request_id = result.request_id.clone();
    if let Err(error) = execute_inner(command, context, dry_run, &request_id, result) {
        let busy = error.starts_with("busy:");
        let pending = result
            .mutations
            .iter()
            .any(|m| m.state == MutationState::Unknown);
        fail(
            result,
            if busy {
                "home_recovery_busy"
            } else if error.starts_with("descriptor limit:") {
                "home_recovery_descriptor_limit"
            } else if pending {
                "home_recovery_pending"
            } else {
                "home_recovery_refused"
            },
            if result
                .mutations
                .iter()
                .any(|m| m.state == MutationState::Unknown)
            {
                "settlement"
            } else {
                "home_recovery"
            },
            !result
                .mutations
                .iter()
                .any(|m| m.state == MutationState::Unknown),
            &error,
        );
    }
}

fn required_dir(parent: &File, name: &[u8]) -> RecoveryResult<File> {
    open_secure_dir_at(parent, name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "maintenance directory {} is absent",
                String::from_utf8_lossy(name)
            )
        })
}

// One descriptor per bounded inventory entry plus room for admitted roots,
// metadata, audit/journal I/O, and the companion's existing process descriptors.
const RECOVERY_DESCRIPTOR_RESERVE: usize = 64;

struct DescriptorBudget {
    original: Option<libc::rlimit>,
}

impl DescriptorBudget {
    fn reserve(lock_count: usize) -> RecoveryResult<Self> {
        let required: libc::rlim_t = lock_count
            .checked_add(RECOVERY_DESCRIPTOR_RESERVE)
            .and_then(|count| count.try_into().ok())
            .ok_or("descriptor limit: recovery budget overflow")?;
        let mut original = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit writes the initialized, correctly sized allocation.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut original) } != 0 {
            return Err(format!(
                "descriptor limit: cannot inspect process limit: {}",
                std::io::Error::last_os_error()
            ));
        }
        if original.rlim_cur >= required {
            return Ok(Self { original: None });
        }
        if original.rlim_max < required {
            return Err(format!(
                "descriptor limit: recovery needs {required} descriptors for {lock_count} locks and a {RECOVERY_DESCRIPTOR_RESERVE}-descriptor reserve, but the process hard limit is {}; no recovery records changed",
                original.rlim_max
            ));
        }
        let expanded = libc::rlimit {
            rlim_cur: required,
            rlim_max: original.rlim_max,
        };
        // SAFETY: only this companion process's soft limit changes; the hard
        // limit is retained and the scoped guard restores the inherited value.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &expanded) } != 0 {
            return Err(format!(
                "descriptor limit: cannot reserve {required} descriptors within the existing hard limit: {}; no recovery records changed",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self {
            original: Some(original),
        })
    }
}

impl Drop for DescriptorBudget {
    fn drop(&mut self) {
        if let Some(original) = self.original.take() {
            // SAFETY: restoring the unchanged hard limit and previously admitted
            // soft limit is valid. This does not close or release any descriptor.
            if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &original) } != 0 {
                eprintln!(
                    "omegon-maintain: could not restore process descriptor limit: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

struct Access {
    state: MaintenanceStateV1,
    _locks: Vec<ProtocolLock>,
    observed: PathIdentityV1,
    root_identity: PathIdentityV1,
    // Rust drops fields in declaration order: release records and every lock
    // before returning the companion to its inherited descriptor budget.
    _descriptor_budget: DescriptorBudget,
}

impl Access {
    #[cfg(test)]
    fn acquire(context: &Context) -> RecoveryResult<Self> {
        Self::acquire_mode(context, false)
    }
    fn acquire_mode(context: &Context, inspect: bool) -> RecoveryResult<Self> {
        let observed = path_identity(&context.home.file).map_err(|e| e.to_string())?;
        let maintain = required_dir(&context.home.file, b"maintain")?;
        let root = required_dir(&maintain, b"v1")?;
        let locks = required_dir(&root, b"locks")?;
        let lock = |name: &[u8]| {
            ProtocolLock::acquire_at(
                &locks,
                name,
                if inspect {
                    LockMode::Shared
                } else {
                    LockMode::Exclusive
                },
                false,
                true,
            )
            .map_err(|e| match e {
                ContractError::Lock(ref io) if io.kind() == std::io::ErrorKind::WouldBlock => {
                    format!(
                        "busy: active maintenance or admission lock {}",
                        String::from_utf8_lossy(name)
                    )
                }
                _ => e.to_string(),
            })
        };
        let mut guards = vec![lock(b"bootstrap.lock")?];
        // No blocking acquisition while holding bootstrap: existing owners may
        // already hold a domain and be waiting for audit or bootstrap themselves.
        let mut entries = if inspect {
            Vec::new()
        } else {
            read_dir_at(&locks, context, MAX_ENTRIES)?
        };
        let descriptor_budget = if inspect {
            DescriptorBudget { original: None }
        } else {
            DescriptorBudget::reserve(entries.len())?
        };
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            if entry.name != b"bootstrap.lock" {
                guards.push(lock(&entry.name)?);
            }
        }
        let audit = required_dir(&root, b"audit")?;
        let state = MaintenanceStateV1 {
            installation: read_record_at::<InstallationStateV1>(&root, b"state.json")
                .map_err(|e| e.to_string())?
                .ok_or("installation state is absent")?,
            deny: required_dir(&root, b"deny")?,
            session_deny: required_dir(&root, b"session-deny")?,
            transactions: required_dir(&root, b"transactions")?,
            fences: required_dir(&root, b"fences")?,
            audit_segments: required_dir(&audit, b"segments")?,
            audit_receipts: required_dir(&audit, b"receipts")?,
            audit,
            locks,
            root,
        };
        let root_identity = path_identity(&state.root).map_err(|e| e.to_string())?;
        let access = Self {
            state,
            _locks: guards,
            observed,
            root_identity,
            _descriptor_budget: descriptor_budget,
        };
        access.recheck(context)?;
        Ok(access)
    }
    fn recheck(&self, context: &Context) -> RecoveryResult<()> {
        if context.expired() {
            return Err("deadline expired during home recovery".into());
        }
        let reopened_home = open_secure_root(&context.home.path).map_err(|e| e.to_string())?;
        let maintain = required_dir(&reopened_home, b"maintain")?;
        let root = required_dir(&maintain, b"v1")?;
        if path_identity(&reopened_home).map_err(|e| e.to_string())? != self.observed
            || path_identity(&context.home.file).map_err(|e| e.to_string())? != self.observed
            || path_identity(&root).map_err(|e| e.to_string())? != self.root_identity
        {
            return Err("home or maintenance pathname changed during recovery".into());
        }
        for (name, retained) in [
            (b"locks".as_slice(), &self.state.locks),
            (b"deny", &self.state.deny),
            (b"session-deny", &self.state.session_deny),
            (b"transactions", &self.state.transactions),
            (b"fences", &self.state.fences),
            (b"audit", &self.state.audit),
        ] {
            let current = required_dir(&root, name)?;
            if path_identity(&current).map_err(|e| e.to_string())?
                != path_identity(retained).map_err(|e| e.to_string())?
            {
                return Err("maintenance child directory changed during recovery".into());
            }
        }
        for (name, retained) in [
            (b"segments".as_slice(), &self.state.audit_segments),
            (b"receipts", &self.state.audit_receipts),
        ] {
            let current = required_dir(&self.state.audit, name)?;
            if path_identity(&current).map_err(|e| e.to_string())?
                != path_identity(retained).map_err(|e| e.to_string())?
            {
                return Err("maintenance audit directory changed during recovery".into());
            }
        }
        Ok(())
    }
    fn ensure_quiescent(&self, context: &Context) -> RecoveryResult<()> {
        if !read_dir_at(&self.state.fences, context, MAX_ENTRIES)?.is_empty() {
            return Err("unresolved maintenance fence prevents home recovery".into());
        }
        for entry in read_dir_at(&self.state.transactions, context, MAX_ENTRIES)? {
            let transaction: TransactionV1 = read_record_at(&self.state.transactions, &entry.name)
                .map_err(|e| e.to_string())?
                .ok_or("transaction disappeared")?;
            if !matches!(
                transaction.state,
                TransactionState::Settled | TransactionState::Aborted
            ) {
                return Err("unresolved maintenance transaction prevents home recovery".into());
            }
        }
        Ok(())
    }
}

fn execute_inner(
    command: &HomeCommand,
    context: &Context,
    dry_run: bool,
    request_id: &str,
    result: &mut MaintenanceResultV1,
) -> RecoveryResult<()> {
    let mut access = Access::acquire_mode(context, matches!(command, HomeCommand::Inspect))?;
    let journal: Option<HomeRecoveryJournalV1> =
        read_record_at(&access.state.root, b"home-recovery.json").map_err(|e| e.to_string())?;
    let pending = journal
        .as_ref()
        .is_some_and(|j| j.phase != HomeRecoveryPhase::Settled);
    let ready = !pending
        && home_binding_matches(
            &context.home.file,
            &access.state.root,
            &access.state.installation,
            &access.observed,
        )
        .map_err(|e| e.to_string())?;
    let quiescence = access.ensure_quiescent(context);
    let binding: Option<HomeContinuityV1> =
        read_record_at(&access.state.root, b"home-continuity.json").map_err(|e| e.to_string())?;
    let continuity_matches = binding.as_ref().is_none_or(|binding| {
        stable_home_volume_uuid(&context.home.file)
            .ok()
            .flatten()
            .as_deref()
            == Some(binding.volume_uuid.as_str())
    });
    let recoverable = same_home_directory(&access.state.installation.home, &access.observed)
        && continuity_matches
        && quiescence.is_ok();
    let evidence = json!({"installation_uuid": access.state.installation.installation_uuid,
        "stored_home": access.state.installation.home, "observed_home": access.observed,
        "recoverable": recoverable, "recovery": journal,
        "continuity": stable_home_volume_uuid(&context.home.file).map_err(|e| e.to_string())?,
        "blocker": quiescence.as_ref().err()});
    if matches!(command, HomeCommand::Inspect) || dry_run {
        if dry_run {
            quiescence?;
            if !recoverable {
                return Err("recovery requires the original home path, inode, and any persisted volume continuity".into());
            }
        } else if !ready || pending {
            result.status = ResultStatus::Degraded;
        }
        diagnostic(
            result,
            if pending {
                "home_recovery_pending"
            } else if ready {
                "home_identity_ready"
            } else {
                "home_identity_mismatch"
            },
            if ready && !pending {
                Severity::Info
            } else {
                Severity::Warning
            },
            "home",
            if ready && !pending {
                "Installation home identity is ready"
            } else {
                "Installation home identity requires explicit recovery"
            },
            Some(evidence),
        );
        if dry_run {
            result
                .mutations
                .push(home_mutation(&access, MutationState::Planned));
        }
        return Ok(());
    }
    quiescence?;
    if !recoverable {
        return Err(
            "recovery requires the original home path, inode, and any persisted volume continuity"
                .into(),
        );
    }
    if let Err(error) = recover(&mut access, context, request_id, journal, None) {
        if let Ok(Some(pending)) =
            read_record_at::<HomeRecoveryJournalV1>(&access.state.root, b"home-recovery.json")
            && pending.phase != HomeRecoveryPhase::Settled
        {
            let mut mutation = home_mutation(&access, MutationState::Unknown);
            mutation.retry_safe = false;
            result.mutations.push(mutation);
            return Err(format!(
                "{error}; resume home recover with --request-id {}",
                pending.request_id
            ));
        }
        return Err(error);
    }
    result
        .mutations
        .push(home_mutation(&access, MutationState::Settled));
    diagnostic(
        result,
        "home_recovery_settled",
        Severity::Info,
        "home",
        "Installation home identity recovered; existing policy and audit authority preserved",
        Some(
            json!({"installation_uuid":access.state.installation.installation_uuid,
            "home":access.state.installation.home, "request_id":request_id}),
        ),
    );
    Ok(())
}

fn home_mutation(access: &Access, state: MutationState) -> MutationResultV1 {
    MutationResultV1 {
        domain_key: derive_key(
            "home-recovery",
            &[
                access.state.installation.installation_uuid.as_bytes(),
                access.observed.key.as_bytes(),
            ],
        ),
        kind: "home.recover".into(),
        state,
        retry_safe: true,
    }
}

// Test-only fault points exercise the same durable transitions as production.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StopAfter {
    Intent,
    Prepared,
    Rebound,
    Audited,
}

fn recover(
    access: &mut Access,
    context: &Context,
    request_id: &str,
    previous: Option<HomeRecoveryJournalV1>,
    stop: Option<StopAfter>,
) -> RecoveryResult<()> {
    if previous
        .as_ref()
        .is_some_and(|j| j.phase != HomeRecoveryPhase::Settled && j.request_id != request_id)
    {
        return Err("another recovery request is pending; resume its request ID".into());
    }
    access.recheck(context)?;
    let directory = open_or_create_secure_dir_at(&access.state.root, b"home-recoveries")
        .map_err(|e| e.to_string())?;
    let intent_name = format!("{request_id}.json");
    let existing: Option<HomeRecoveryIntentV1> =
        read_record_at(&directory, intent_name.as_bytes()).map_err(|e| e.to_string())?;
    let intent = match existing {
        Some(intent) => intent,
        None => {
            if previous.as_ref().is_some_and(|j| {
                j.request_id == request_id && j.phase != HomeRecoveryPhase::Prepared
            }) {
                return Err("advanced recovery journal lacks its immutable intent".into());
            }
            if read_record_at::<omegon_maintenance_contracts::AuditReceiptV1>(
                &access.state.audit_receipts,
                intent_name.as_bytes(),
            )
            .map_err(|e| e.to_string())?
            .is_some()
            {
                return Err(
                    "request ID already has an audit receipt but no recovery intent".into(),
                );
            }
            // Existing stable evidence is authoritative. Explicit recovery must
            // not launder a different volume through a coincidentally equal inode.
            let current_binding: Option<HomeContinuityV1> =
                read_record_at(&access.state.root, b"home-continuity.json")
                    .map_err(|e| e.to_string())?;
            let volume = stable_home_volume_uuid(&context.home.file).map_err(|e| e.to_string())?;
            if let Some(binding) = current_binding
                && (binding.installation_uuid != access.state.installation.installation_uuid
                    || binding.home != access.state.installation.home
                    || volume.as_deref() != Some(binding.volume_uuid.as_str()))
            {
                return Err("persisted volume continuity conflicts with the opened home".into());
            }
            let continuity = volume.map(|volume_uuid| HomeContinuityV1 {
                schema_version: SCHEMA_VERSION,
                record_kind: "home_continuity".into(),
                installation_uuid: access.state.installation.installation_uuid.clone(),
                home: access.observed.clone(),
                volume_uuid,
            });
            let intent = HomeRecoveryIntentV1::new(
                request_id.into(),
                access.state.installation.clone(),
                access.observed.clone(),
                continuity,
            )
            .map_err(|e| e.to_string())?;
            if previous
                .as_ref()
                .is_some_and(|j| j.request_id == request_id && j.intent_key != intent.record_id)
            {
                return Err("prepared recovery no longer matches the original state".into());
            }
            let prepared = HomeRecoveryJournalV1 {
                schema_version: SCHEMA_VERSION,
                record_kind: "home_recovery_journal".into(),
                request_id: request_id.into(),
                intent_key: intent.record_id,
                phase: HomeRecoveryPhase::Prepared,
            };
            // Journal first closes admission even if the immutable-intent write
            // is interrupted. Missing intent can only be recreated from exactly
            // the original state whose digest the prepared journal names.
            access.recheck(context)?;
            replace_record_at(
                &access.state.root,
                b"home-recovery.json",
                &prepared,
                request_id,
            )
            .map_err(|e| e.to_string())?;
            create_record_no_replace_at(&directory, intent_name.as_bytes(), &intent, request_id)
                .map_err(|e| e.to_string())?;
            intent
        }
    };
    if stop == Some(StopAfter::Intent) {
        return Err("interrupted after intent".into());
    }
    if intent.request_id != request_id || intent.target != access.observed {
        return Err("immutable recovery intent does not match this request and home".into());
    }
    let current_binding: Option<HomeContinuityV1> =
        read_record_at(&access.state.root, b"home-continuity.json").map_err(|e| e.to_string())?;
    if let Some(binding) = &current_binding {
        if intent
            .continuity
            .as_ref()
            .is_none_or(|expected| binding.volume_uuid != expected.volume_uuid)
            || binding.installation_uuid != intent.original.installation_uuid
            || (binding.home != intent.original.home && binding.home != intent.target)
        {
            return Err("persisted continuity conflicts with the immutable recovery intent".into());
        }
    } else if intent.continuity.is_some()
        && previous.as_ref().is_some_and(|j| {
            j.request_id == request_id
                && matches!(
                    j.phase,
                    HomeRecoveryPhase::Rebound
                        | HomeRecoveryPhase::Audited
                        | HomeRecoveryPhase::Settled
                )
        })
    {
        return Err("advanced recovery lost its persisted volume continuity".into());
    }
    if let Some(previous) = &previous
        && previous.request_id == request_id
        && previous.intent_key != intent.record_id
    {
        return Err("recovery journal does not match immutable intent".into());
    }
    let current = read_record_at::<InstallationStateV1>(&access.state.root, b"state.json")
        .map_err(|e| e.to_string())?
        .ok_or("installation state disappeared")?;
    if previous
        .as_ref()
        .is_some_and(|j| j.request_id == request_id && j.phase == HomeRecoveryPhase::Settled)
    {
        ensure_home_recovery_settled(&access.state.root).map_err(|e| e.to_string())?;
        access.state.installation = current;
        return Ok(());
    }
    let mut target = intent.original.clone();
    target.home = intent.target.clone();
    let mut target_after_audit = target.clone();
    target_after_audit.next_audit_sequence = target
        .next_audit_sequence
        .checked_add(1)
        .ok_or("audit sequence overflow")?;
    if current != intent.original && current != target && current != target_after_audit {
        return Err("installation changed outside the recoverable original/target states".into());
    }
    if let Some(binding) = &intent.continuity
        && stable_home_volume_uuid(&context.home.file)
            .map_err(|e| e.to_string())?
            .as_deref()
            != Some(binding.volume_uuid.as_str())
    {
        return Err("volume continuity changed since recovery intent".into());
    }
    let mut journal = HomeRecoveryJournalV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "home_recovery_journal".into(),
        request_id: request_id.into(),
        intent_key: intent.record_id,
        phase: HomeRecoveryPhase::Prepared,
    };
    access.recheck(context)?;
    replace_record_at(
        &access.state.root,
        b"home-recovery.json",
        &journal,
        request_id,
    )
    .map_err(|e| e.to_string())?;
    if stop == Some(StopAfter::Prepared) {
        return Err("interrupted after prepared".into());
    }
    // Never overwrite the next sequence if audit already committed before a crash.
    if current == intent.original {
        access.recheck(context)?;
        replace_record_at(&access.state.root, b"state.json", &target, request_id)
            .map_err(|e| e.to_string())?;
    }
    if let Some(binding) = &intent.continuity {
        replace_record_at(
            &access.state.root,
            b"home-continuity.json",
            binding,
            request_id,
        )
        .map_err(|e| e.to_string())?;
    }
    journal.phase = HomeRecoveryPhase::Rebound;
    replace_record_at(
        &access.state.root,
        b"home-recovery.json",
        &journal,
        request_id,
    )
    .map_err(|e| e.to_string())?;
    if stop == Some(StopAfter::Rebound) {
        return Err("interrupted after rebound".into());
    }
    access.recheck(context)?;
    super::mutation::append_home_recovery_audit_locked(&mut access.state, context, request_id)?;
    journal.phase = HomeRecoveryPhase::Audited;
    replace_record_at(
        &access.state.root,
        b"home-recovery.json",
        &journal,
        request_id,
    )
    .map_err(|e| e.to_string())?;
    if stop == Some(StopAfter::Audited) {
        return Err("interrupted after audited".into());
    }
    access.recheck(context)?;
    journal.phase = HomeRecoveryPhase::Settled;
    replace_record_at(
        &access.state.root,
        b"home-recovery.json",
        &journal,
        request_id,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AdmittedRoot;
    use omegon_maintenance_contracts::{
        AuditReceiptV1, AuthorityKey, ContributionKind, DenyRecordV1, DenyState, DenyStateV1,
        contribution_domain_key, derive_key, entry_key, scope_key,
    };
    use sha2::{Digest, Sha256};
    use std::{
        os::unix::fs::MetadataExt,
        time::{Duration, Instant},
    };
    const INSTALL: &str = "11111111-1111-1111-1111-111111111111";
    const REQUEST: &str = "22222222-2222-2222-2222-222222222222";
    fn fixture() -> (tempfile::TempDir, Context) {
        let temp = tempfile::tempdir().unwrap();
        let file = open_secure_root(temp.path()).unwrap();
        let observed = path_identity(&file).unwrap();
        let state = MaintenanceStateV1::bootstrap(&file, observed, INSTALL, true).unwrap();
        let mut installation = state.installation.clone();
        installation.home.device ^= 2;
        replace_record_at(&state.root, b"state.json", &installation, "fixture").unwrap();
        // Model a legacy installation, which has never enrolled stable continuity.
        let binding = temp.path().join("maintain/v1/home-continuity.json");
        if binding.exists() {
            std::fs::remove_file(binding).unwrap();
        }
        let admitted = || {
            let file = open_secure_root(temp.path()).unwrap();
            let metadata = file.metadata().unwrap();
            AdmittedRoot {
                path: temp.path().into(),
                file,
                device: metadata.dev(),
                inode: metadata.ino(),
                unsafe_permissions: false,
            }
        };
        let context = Context {
            home: admitted(),
            config_home: admitted(),
            workspace: None,
            started: Instant::now(),
            deadline: Duration::from_secs(30),
        };
        (temp, context)
    }
    fn journal(access: &Access) -> Option<HomeRecoveryJournalV1> {
        read_record_at(&access.state.root, b"home-recovery.json").unwrap()
    }
    fn seed_deny(access: &Access) -> AuthorityKey {
        let scope = scope_key("skill", "user", access.observed.key);
        let name = b"denied-skill";
        let key = entry_key("skill", scope, name);
        let directory =
            open_or_create_secure_dir_at(&access.state.deny, scope.to_hex().as_bytes()).unwrap();
        let deny = DenyStateV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "deny_state".into(),
            record_id: derive_key("deny-state", &[scope.as_bytes(), &1_u64.to_be_bytes()]),
            scope_key: scope,
            generation: 1,
            entries: std::collections::BTreeMap::from([(
                key.to_hex(),
                DenyRecordV1 {
                    schema_version: SCHEMA_VERSION,
                    record_kind: "deny".into(),
                    record_id: derive_key(
                        "deny",
                        &[scope.as_bytes(), key.as_bytes(), REQUEST.as_bytes()],
                    ),
                    scope_key: scope,
                    contribution_kind: ContributionKind::Skill,
                    entry_key: key,
                    raw_name_digest: AuthorityKey::from_bytes(Sha256::digest(name).into()),
                    generation: 1,
                    state: DenyState::Denied,
                    request_id: REQUEST.into(),
                    created_at: "2026-09-07T00:00:00Z".into(),
                },
            )]),
        };
        replace_record_at(&directory, b"state.json", &deny, "fixture").unwrap();
        scope
    }
    #[test]
    fn home_recovery_interrupted_phases_preserve_denies_and_resume_once() {
        for phase in [
            StopAfter::Intent,
            StopAfter::Prepared,
            StopAfter::Rebound,
            StopAfter::Audited,
        ] {
            let (_temp, context) = fixture();
            let mut access = Access::acquire(&context).unwrap();
            seed_deny(&access);
            assert!(recover(&mut access, &context, REQUEST, None, Some(phase)).is_err());
            drop(access);
            assert!(
                MaintenanceStateV1::bootstrap(
                    &context.home.file,
                    path_identity(&context.home.file).unwrap(),
                    INSTALL,
                    true
                )
                .is_err()
            );
            let mut access = Access::acquire(&context).unwrap();
            let pending = journal(&access);
            recover(&mut access, &context, REQUEST, pending, None).unwrap();
            let settled = journal(&access);
            recover(&mut access, &context, REQUEST, settled, None).unwrap();
            assert_eq!(access.state.installation.installation_uuid, INSTALL);
            assert_eq!(access.state.installation.next_audit_sequence, 2);
            let receipt: AuditReceiptV1 = read_record_at(
                &access.state.audit_receipts,
                format!("{REQUEST}.json").as_bytes(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(receipt.sequence, 1);
            assert_eq!(receipt.command, "home.recover");
            drop(access);
            let state = MaintenanceStateV1::bootstrap(
                &context.home.file,
                path_identity(&context.home.file).unwrap(),
                INSTALL,
                true,
            )
            .unwrap();
            let guard = state
                .admit_contribution_scope(
                    ContributionKind::Skill,
                    "user",
                    &path_identity(&context.home.file).unwrap(),
                    "fixture",
                    true,
                )
                .unwrap();
            assert!(!guard.allows(b"denied-skill").unwrap());
        }
    }
    #[test]
    fn home_recovery_busy_admission_refuses_without_state_changes() {
        let (_temp, context) = fixture();
        let access = Access::acquire(&context).unwrap();
        let installation = access.state.installation.clone();
        let scope = scope_key("skill", "user", access.observed.key);
        let lock_name = format!("contribution-{scope}.lock");
        let held = ProtocolLock::acquire_at(
            &access.state.locks,
            lock_name.as_bytes(),
            LockMode::Shared,
            true,
            true,
        )
        .unwrap();
        drop(access);
        assert!(
            Access::acquire(&context)
                .err()
                .unwrap()
                .starts_with("busy:")
        );
        drop(held);
        let access = Access::acquire(&context).unwrap();
        assert_eq!(access.state.installation, installation);
        assert!(journal(&access).is_none());
    }
    #[test]
    fn home_recovery_refuses_active_fence_and_tampered_intent() {
        let (_temp, context) = fixture();
        let mut access = Access::acquire(&context).unwrap();
        let scope = scope_key("skill", "user", access.observed.key);
        // A malformed fence must be at least as restrictive as a valid active fence.
        let fence_name = format!("{}.json", contribution_domain_key(scope));
        use std::os::unix::fs::PermissionsExt;
        let root_path = context.home.path.join("maintain/v1");
        let fence_path = root_path.join("fences").join(fence_name);
        std::fs::write(&fence_path, b"{}").unwrap();
        std::fs::set_permissions(&fence_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(access.ensure_quiescent(&context).is_err());
        std::fs::remove_file(fence_path).unwrap();
        assert!(
            recover(
                &mut access,
                &context,
                REQUEST,
                None,
                Some(StopAfter::Prepared)
            )
            .is_err()
        );
        let intent_path = root_path
            .join("home-recoveries")
            .join(format!("{REQUEST}.json"));
        let mut intent: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&intent_path).unwrap()).unwrap();
        intent["target"]["device"] = json!(123);
        std::fs::write(intent_path, serde_json::to_vec(&intent).unwrap()).unwrap();
        let pending = journal(&access);
        assert!(recover(&mut access, &context, REQUEST, pending, None).is_err());
        assert_ne!(
            access.state.installation.home.device,
            access.observed.device
        );
    }
    #[test]
    fn home_recovery_prepared_without_intent_resumes_but_forged_settlement_blocks() {
        let (_temp, context) = fixture();
        let mut access = Access::acquire(&context).unwrap();
        assert!(
            recover(
                &mut access,
                &context,
                REQUEST,
                None,
                Some(StopAfter::Intent)
            )
            .is_err()
        );
        let intents = required_dir(&access.state.root, b"home-recoveries").unwrap();
        omegon_maintenance_contracts::remove_record_at(
            &intents,
            format!("{REQUEST}.json").as_bytes(),
        )
        .unwrap();
        let pending = journal(&access);
        recover(&mut access, &context, REQUEST, pending, None).unwrap();
        let settled = journal(&access).unwrap();
        omegon_maintenance_contracts::remove_record_at(
            &access.state.audit_receipts,
            format!("{REQUEST}.json").as_bytes(),
        )
        .unwrap();
        assert!(ensure_home_recovery_settled(&access.state.root).is_err());
        assert!(recover(&mut access, &context, REQUEST, Some(settled), None).is_err());
    }
    #[test]
    fn home_recovery_replay_after_later_audit_preserves_current_sequence() {
        let (_temp, context) = fixture();
        let mut access = Access::acquire(&context).unwrap();
        recover(&mut access, &context, REQUEST, None, None).unwrap();
        super::super::mutation::append_home_recovery_audit_locked(
            &mut access.state,
            &context,
            "33333333-3333-3333-3333-333333333333",
        )
        .unwrap();
        let pending = journal(&access);
        recover(&mut access, &context, REQUEST, pending, None).unwrap();
        assert_eq!(access.state.installation.next_audit_sequence, 3);
        ensure_home_recovery_settled(&access.state.root).unwrap();
    }
    #[test]
    fn home_recovery_retains_session_denial_and_rejects_changed_directory() {
        use omegon_maintenance_contracts::{SessionDenyRecordV1, SessionDenyState, session_key};
        let (_temp, context) = fixture();
        let mut access = Access::acquire(&context).unwrap();
        let session = "denied-session";
        let workspace = access.observed.key;
        let key = session_key(session, workspace);
        let deny = SessionDenyRecordV1 {
            schema_version: SCHEMA_VERSION,
            record_kind: "session_deny".into(),
            record_id: derive_key("session-deny", &[key.as_bytes(), REQUEST.as_bytes()]),
            session_key: key,
            session_id: session.into(),
            workspace_key: workspace,
            state: SessionDenyState::ResumeDenied,
            request_id: REQUEST.into(),
            created_at: "2026-09-07T00:00:00Z".into(),
        };
        create_record_no_replace_at(
            &access.state.session_deny,
            format!("{key}.json").as_bytes(),
            &deny,
            "test",
        )
        .unwrap();
        recover(&mut access, &context, REQUEST, None, None).unwrap();
        drop(access);
        let state = MaintenanceStateV1::bootstrap(
            &context.home.file,
            path_identity(&context.home.file).unwrap(),
            INSTALL,
            true,
        )
        .unwrap();
        assert!(matches!(
            state.admit_session_resume(session, workspace, true),
            Err(ContractError::SessionResumeDenied)
        ));
        let mut moved = state.installation.clone();
        moved.home.inode += 1;
        replace_record_at(&state.root, b"state.json", &moved, "test").unwrap();
        assert!(
            MaintenanceStateV1::bootstrap(
                &context.home.file,
                path_identity(&context.home.file).unwrap(),
                INSTALL,
                true
            )
            .is_err()
        );
    }

    #[test]
    fn home_recovery_active_transaction_refuses_unchanged() {
        let (_temp, context) = fixture();
        let access = Access::acquire(&context).unwrap();
        let transaction: TransactionV1 = omegon_maintenance_contracts::parse_record(
            include_bytes!("../../omegon-maintenance-contracts/tests/fixtures/transaction-v1.json"),
        )
        .unwrap();
        create_record_no_replace_at(
            &access.state.transactions,
            b"active.json",
            &transaction,
            "test",
        )
        .unwrap();
        assert!(access.ensure_quiescent(&context).is_err());
        assert!(journal(&access).is_none());
        assert_ne!(
            access.state.installation.home.device,
            access.observed.device
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn home_recovery_resume_rejects_conflicting_persisted_continuity() {
        let (_temp, context) = fixture();
        let mut access = Access::acquire(&context).unwrap();
        assert!(
            recover(
                &mut access,
                &context,
                REQUEST,
                None,
                Some(StopAfter::Rebound)
            )
            .is_err()
        );
        let mut binding: HomeContinuityV1 =
            read_record_at(&access.state.root, b"home-continuity.json")
                .unwrap()
                .unwrap();
        binding.volume_uuid = "11111111111111111111111111111111".into();
        replace_record_at(
            &access.state.root,
            b"home-continuity.json",
            &binding,
            "test",
        )
        .unwrap();
        let pending = journal(&access);
        assert!(
            recover(&mut access, &context, REQUEST, pending, None)
                .unwrap_err()
                .contains("continuity conflicts")
        );
        let retained: HomeContinuityV1 =
            read_record_at(&access.state.root, b"home-continuity.json")
                .unwrap()
                .unwrap();
        assert_eq!(retained, binding);
    }
}

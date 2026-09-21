use std::{fs::File, io::Read, path::Path, sync::atomic::AtomicU64};

#[cfg(unix)]
use std::{
    io::Write,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
pub fn open_secure_root(path: &Path) -> Result<File> {
    use std::{
        ffi::CString, os::fd::FromRawFd, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt,
    };

    if !path.is_absolute() || path == Path::new("/") {
        return Err(ContractError::InvalidValue(
            "protocol root must be an absolute directory other than /".into(),
        ));
    }
    let encoded = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        ContractError::InvalidValue("protocol root contains an interior NUL".into())
    })?;
    // SAFETY: encoded remains valid for the call; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::open(
            encoded.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    // SAFETY: open returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(ContractError::InvalidValue(
            "protocol root must be a directory owned by the effective user".into(),
        ));
    }
    Ok(file)
}

#[cfg(not(unix))]
pub fn open_secure_root(_path: &Path) -> Result<File> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 roots support Unix only".into(),
    ))
}

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    AUDIT_SEGMENT_RECORDS, AuditCheckpointV1, AuditFrontierV1, AuditReceiptV1, AuditRecordV1,
    AuthorityKey, ContractError, ContributionKind, DenyStateV1, FenceV1, FileIdentityV1,
    InstallationStateV1, LockMode, PathIdentityV1, ProtocolLock, Record, Result, SCHEMA_VERSION,
    SessionDenyRecordV1, canonical_digest, canonical_json, contribution_domain_key, derive_key,
    entry_key, parse_record, scope_key, session_domain_key, session_key, validate_child_name,
};

pub struct MaintenanceStateV1 {
    pub root: File,
    pub locks: File,
    pub deny: File,
    pub session_deny: File,
    pub transactions: File,
    pub fences: File,
    pub audit: File,
    pub audit_segments: File,
    pub audit_receipts: File,
    pub installation: InstallationStateV1,
}

pub struct SessionResumeGuard {
    _lock: ProtocolLock,
    pub session_key: AuthorityKey,
}

pub struct ContributionAdmissionGuard {
    _lock: ProtocolLock,
    pub scope_key: AuthorityKey,
    pub generation: u64,
    kind: ContributionKind,
    deny: DenyStateV1,
}

pub struct ContributionMutationGuard {
    _lock: ProtocolLock,
    pub scope_key: AuthorityKey,
    pub generation: u64,
}

impl ContributionAdmissionGuard {
    pub fn allows(&self, raw_name: &[u8]) -> Result<bool> {
        validate_child_name(raw_name)?;
        let authority = entry_key(self.kind.as_str(), self.scope_key, raw_name);
        let Some(deny) = self.deny.entries.get(&authority.to_hex()) else {
            return Ok(true);
        };
        let digest = AuthorityKey::from_bytes(Sha256::digest(raw_name).into());
        if deny.contribution_kind != self.kind
            || deny.entry_key != authority
            || deny.raw_name_digest != digest
        {
            return Err(ContractError::InvalidValue(
                "deny entry does not match requested contribution bytes".into(),
            ));
        }
        Ok(false)
    }
}

static TEMPORARY_NONCE: AtomicU64 = AtomicU64::new(0);

impl MaintenanceStateV1 {
    pub fn admit_contribution_scope(
        &self,
        kind: ContributionKind,
        scope: &str,
        parent: &PathIdentityV1,
        temporary_tag: &str,
        nonblocking: bool,
    ) -> Result<ContributionAdmissionGuard> {
        let authority = scope_key(kind.as_str(), scope, parent.key);
        let lock_name = format!("contribution-{authority}.lock");
        let directory_name = authority.to_hex();
        let mut lock = self.acquire_or_create_protocol_lock(
            lock_name.as_bytes(),
            LockMode::Shared,
            nonblocking,
        )?;
        if open_secure_dir_at(&self.deny, directory_name.as_bytes())?.is_none() {
            drop(lock);
            let _exclusive = self.acquire_or_create_protocol_lock(
                lock_name.as_bytes(),
                LockMode::Exclusive,
                nonblocking,
            )?;
            if open_secure_dir_at(&self.deny, directory_name.as_bytes())?.is_none() {
                let directory =
                    open_or_create_secure_dir_at(&self.deny, directory_name.as_bytes())?;
                let empty = DenyStateV1 {
                    schema_version: SCHEMA_VERSION,
                    record_kind: "deny_state".into(),
                    record_id: derive_key(
                        "deny-state",
                        &[authority.as_bytes(), &0_u64.to_be_bytes()],
                    ),
                    scope_key: authority,
                    generation: 0,
                    entries: Default::default(),
                };
                create_record_no_replace_at(&directory, b"state.json", &empty, temporary_tag)?;
            }
            drop(_exclusive);
            lock = self.acquire_or_create_protocol_lock(
                lock_name.as_bytes(),
                LockMode::Shared,
                nonblocking,
            )?;
        }
        let deny = self.read_contribution_deny_state(kind, authority)?;
        Ok(ContributionAdmissionGuard {
            _lock: lock,
            scope_key: authority,
            generation: deny.generation,
            kind,
            deny,
        })
    }

    pub fn lock_contribution_scope_mutation(
        &self,
        kind: ContributionKind,
        scope: &str,
        parent: &PathIdentityV1,
        temporary_tag: &str,
        nonblocking: bool,
    ) -> Result<ContributionMutationGuard> {
        let authority = scope_key(kind.as_str(), scope, parent.key);
        let lock_name = format!("contribution-{authority}.lock");
        let lock = self.acquire_or_create_protocol_lock(
            lock_name.as_bytes(),
            LockMode::Exclusive,
            nonblocking,
        )?;
        let directory_name = authority.to_hex();
        if open_secure_dir_at(&self.deny, directory_name.as_bytes())?.is_none() {
            let directory = open_or_create_secure_dir_at(&self.deny, directory_name.as_bytes())?;
            let empty = DenyStateV1 {
                schema_version: SCHEMA_VERSION,
                record_kind: "deny_state".into(),
                record_id: derive_key("deny-state", &[authority.as_bytes(), &0_u64.to_be_bytes()]),
                scope_key: authority,
                generation: 0,
                entries: Default::default(),
            };
            create_record_no_replace_at(&directory, b"state.json", &empty, temporary_tag)?;
        }
        let deny = self.read_contribution_deny_state(kind, authority)?;
        Ok(ContributionMutationGuard {
            _lock: lock,
            scope_key: authority,
            generation: deny.generation,
        })
    }

    fn read_contribution_deny_state(
        &self,
        kind: ContributionKind,
        authority: AuthorityKey,
    ) -> Result<DenyStateV1> {
        let fence_name = format!("{}.json", contribution_domain_key(authority));
        if read_record_at::<FenceV1>(&self.fences, fence_name.as_bytes())?.is_some() {
            return Err(ContractError::InvalidValue(
                "contribution access is blocked by an unresolved maintenance fence".into(),
            ));
        }
        let directory_name = authority.to_hex();
        let directory =
            open_secure_dir_at(&self.deny, directory_name.as_bytes())?.ok_or_else(|| {
                ContractError::InvalidValue("initialized deny scope disappeared".into())
            })?;
        let deny: DenyStateV1 = read_record_at(&directory, b"state.json")?.ok_or_else(|| {
            ContractError::InvalidValue("initialized deny scope lacks state.json".into())
        })?;
        if deny.scope_key != authority {
            return Err(ContractError::InvalidValue(
                "deny state does not belong to requested contribution scope".into(),
            ));
        }
        if deny
            .entries
            .values()
            .any(|entry| entry.contribution_kind != kind)
        {
            return Err(ContractError::InvalidValue(
                "deny state contains a different contribution kind".into(),
            ));
        }
        Ok(deny)
    }

    #[cfg(unix)]
    pub fn bootstrap(
        home: &File,
        home_identity: PathIdentityV1,
        candidate_installation_uuid: &str,
        nonblocking: bool,
    ) -> Result<Self> {
        let maintain = open_or_create_secure_dir_at(home, b"maintain")?;
        let root = open_or_create_secure_dir_at(&maintain, b"v1")?;
        let locks = open_or_create_secure_dir_at(&root, b"locks")?;
        let _bootstrap = acquire_or_create_lock(&locks, b"bootstrap.lock", nonblocking)?;
        crate::ensure_home_recovery_settled(&root)?;

        let deny = open_or_create_secure_dir_at(&root, b"deny")?;
        let session_deny = open_or_create_secure_dir_at(&root, b"session-deny")?;
        let transactions = open_or_create_secure_dir_at(&root, b"transactions")?;
        let fences = open_or_create_secure_dir_at(&root, b"fences")?;
        let audit = open_or_create_secure_dir_at(&root, b"audit")?;
        let audit_segments = open_or_create_secure_dir_at(&audit, b"segments")?;
        let audit_receipts = open_or_create_secure_dir_at(&audit, b"receipts")?;
        let _audit_lock = acquire_or_create_lock(&locks, b"audit.lock", nonblocking)?;

        let installation = match read_record_at::<InstallationStateV1>(&root, b"state.json")? {
            Some(state) => state,
            None => {
                let state = InstallationStateV1 {
                    schema_version: SCHEMA_VERSION,
                    record_kind: "installation_state".into(),
                    record_id: derive_key(
                        "installation",
                        &[candidate_installation_uuid.as_bytes()],
                    ),
                    installation_uuid: candidate_installation_uuid.into(),
                    home: home_identity.clone(),
                    next_audit_sequence: 1,
                };
                match create_record_no_replace_at(
                    &root,
                    b"state.json",
                    &state,
                    candidate_installation_uuid,
                ) {
                    Ok(()) => state,
                    Err(ContractError::Filesystem(error))
                        if error.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        read_record_at(&root, b"state.json")?.ok_or_else(|| {
                            ContractError::InvalidValue(
                                "installation state disappeared after create race".into(),
                            )
                        })?
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        if !crate::home_binding_matches(home, &root, &installation, &home_identity)? {
            return Err(ContractError::HomeIdentityMismatch {
                stored: Box::new(installation.home.clone()),
                observed: Box::new(home_identity),
            });
        }

        // Enrollment only follows strict matching legacy identity (or previously
        // verified stable continuity). A mismatched legacy home never reaches it.
        if installation.home == home_identity
            && read_record_at::<crate::HomeContinuityV1>(&root, b"home-continuity.json")?.is_none()
            && let Some(volume_uuid) = crate::stable_home_volume_uuid(home)?
        {
            let binding = crate::HomeContinuityV1 {
                schema_version: SCHEMA_VERSION,
                record_kind: "home_continuity".into(),
                installation_uuid: installation.installation_uuid.clone(),
                home: installation.home.clone(),
                volume_uuid,
            };
            create_record_no_replace_at(
                &root,
                b"home-continuity.json",
                &binding,
                candidate_installation_uuid,
            )?;
        }
        if read_record_at::<AuditCheckpointV1>(&audit, b"checkpoint.json")?.is_none() {
            let digest = AuthorityKey::from_bytes([0; 32]);
            let checkpoint = AuditCheckpointV1 {
                schema_version: SCHEMA_VERSION,
                record_kind: "audit_checkpoint".into(),
                record_id: derive_key(
                    "audit-checkpoint",
                    &[
                        installation.installation_uuid.as_bytes(),
                        &0_u64.to_be_bytes(),
                        digest.as_bytes(),
                    ],
                ),
                installation_uuid: installation.installation_uuid.clone(),
                last_sequence: 0,
                last_digest: digest,
            };
            match create_record_no_replace_at(
                &audit,
                b"checkpoint.json",
                &checkpoint,
                candidate_installation_uuid,
            ) {
                Ok(()) => {}
                Err(ContractError::Filesystem(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let checkpoint: AuditCheckpointV1 = read_record_at(&audit, b"checkpoint.json")?
            .ok_or_else(|| ContractError::InvalidValue("audit checkpoint is absent".into()))?;
        if read_record_at::<AuditFrontierV1>(&audit, b"frontier.json")?.is_none() {
            if checkpoint.last_sequence != 0 {
                return Err(ContractError::InvalidValue(
                    "nonempty audit chain lacks a segment frontier".into(),
                ));
            }
            let frontier = audit_frontier(&installation.installation_uuid, 1, None, None, None);
            create_record_no_replace_at(
                &audit,
                b"frontier.json",
                &frontier,
                candidate_installation_uuid,
            )?;
        }

        let mut state = Self {
            root,
            locks,
            deny,
            session_deny,
            transactions,
            fences,
            audit,
            audit_segments,
            audit_receipts,
            installation,
        };
        state.reconcile_audit(candidate_installation_uuid)?;
        Ok(state)
    }

    #[cfg(not(unix))]
    pub fn bootstrap(
        _home: &File,
        _home_identity: PathIdentityV1,
        _candidate_installation_uuid: &str,
        _nonblocking: bool,
    ) -> Result<Self> {
        Err(ContractError::InvalidValue(
            "maintenance protocol v1 state supports Unix only".into(),
        ))
    }
}

impl MaintenanceStateV1 {
    pub fn admit_session_resume(
        &self,
        session_id: &str,
        workspace_key: AuthorityKey,
        nonblocking: bool,
    ) -> Result<SessionResumeGuard> {
        let authority_key = session_key(session_id, workspace_key);
        let lock_name = format!("session-{authority_key}.lock");
        let lock = self.acquire_or_create_protocol_lock(
            lock_name.as_bytes(),
            LockMode::Shared,
            nonblocking,
        )?;
        let fence_name = format!("{}.json", session_domain_key(authority_key));
        if read_record_at::<FenceV1>(&self.fences, fence_name.as_bytes())?.is_some() {
            return Err(ContractError::InvalidValue(
                "session resume is blocked by an unresolved maintenance fence".into(),
            ));
        }
        let deny_name = format!("{authority_key}.json");
        if let Some(deny) =
            read_record_at::<SessionDenyRecordV1>(&self.session_deny, deny_name.as_bytes())?
        {
            if deny.session_key != authority_key
                || deny.session_id != session_id
                || deny.workspace_key != workspace_key
            {
                return Err(ContractError::InvalidValue(
                    "session deny record does not match its requested authority".into(),
                ));
            }
            return Err(ContractError::SessionResumeDenied);
        }
        Ok(SessionResumeGuard {
            _lock: lock,
            session_key: authority_key,
        })
    }

    fn acquire_or_create_protocol_lock(
        &self,
        name: &[u8],
        mode: LockMode,
        nonblocking: bool,
    ) -> Result<ProtocolLock> {
        let _bootstrap = ProtocolLock::acquire_at(
            &self.locks,
            b"bootstrap.lock",
            LockMode::Exclusive,
            false,
            nonblocking,
        )?;
        crate::ensure_home_recovery_settled(&self.root)?;
        match ProtocolLock::acquire_at(&self.locks, name, mode, true, nonblocking) {
            Ok(lock) => Ok(lock),
            Err(ContractError::Lock(error))
                if error.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                ProtocolLock::acquire_at(&self.locks, name, mode, false, nonblocking)
            }
            Err(error) => Err(error),
        }
    }

    pub fn reconcile_audit(&mut self, temporary_tag: &str) -> Result<()> {
        let installation: InstallationStateV1 = read_record_at(&self.root, b"state.json")?
            .ok_or_else(|| ContractError::InvalidValue("installation state is absent".into()))?;
        let checkpoint: AuditCheckpointV1 = read_record_at(&self.audit, b"checkpoint.json")?
            .ok_or_else(|| ContractError::InvalidValue("audit checkpoint is absent".into()))?;
        let frontier: AuditFrontierV1 = read_record_at(&self.audit, b"frontier.json")?
            .ok_or_else(|| ContractError::InvalidValue("audit frontier is absent".into()))?;
        self.installation =
            reconcile_audit_metadata(self, installation, checkpoint, frontier, temporary_tag)?;
        Ok(())
    }

    pub fn prepare_audit_segment(&self, sequence: u64, temporary_tag: &str) -> Result<()> {
        let frontier: AuditFrontierV1 = read_record_at(&self.audit, b"frontier.json")?
            .ok_or_else(|| ContractError::InvalidValue("audit frontier is absent".into()))?;
        let current_end = frontier
            .current_segment_start
            .checked_add(AUDIT_SEGMENT_RECORDS - 1)
            .ok_or_else(|| ContractError::InvalidValue("audit segment overflow".into()))?;
        if sequence <= current_end {
            if sequence < frontier.current_segment_start {
                return Err(ContractError::InvalidValue(
                    "audit append sequence precedes the current segment".into(),
                ));
            }
            return Ok(());
        }
        if sequence
            != current_end.checked_add(1).ok_or_else(|| {
                ContractError::InvalidValue("audit segment sequence overflow".into())
            })?
        {
            return Err(ContractError::InvalidValue(
                "audit append skipped a segment frontier".into(),
            ));
        }
        let checkpoint: AuditCheckpointV1 = read_record_at(&self.audit, b"checkpoint.json")?
            .ok_or_else(|| ContractError::InvalidValue("audit checkpoint is absent".into()))?;
        if checkpoint.last_sequence != current_end {
            return Err(ContractError::InvalidValue(
                "audit segment rotated before its predecessor was complete".into(),
            ));
        }
        let next = audit_frontier(
            &frontier.installation_uuid,
            sequence,
            Some(checkpoint.last_digest),
            Some(frontier.current_segment_start),
            frontier.current_segment_previous_digest,
        );
        replace_record_at(&self.audit, b"frontier.json", &next, temporary_tag)
    }
}

fn reconcile_audit_metadata(
    state: &MaintenanceStateV1,
    mut installation: InstallationStateV1,
    mut checkpoint: AuditCheckpointV1,
    frontier: AuditFrontierV1,
    temporary_tag: &str,
) -> Result<InstallationStateV1> {
    if checkpoint.installation_uuid != installation.installation_uuid {
        return Err(ContractError::InvalidValue(
            "audit checkpoint belongs to a different installation".into(),
        ));
    }
    if frontier.installation_uuid != installation.installation_uuid {
        return Err(ContractError::InvalidValue(
            "audit frontier belongs to a different installation".into(),
        ));
    }

    let mut tail = None;
    if let Some(previous_start) = frontier.previous_segment_start {
        let previous = scan_audit_segment(
            &state.audit_segments,
            previous_start,
            frontier.previous_segment_previous_digest,
            &installation.installation_uuid,
            false,
        )?;
        let expected_tail = frontier.current_segment_start - 1;
        let Some((record, digest)) = previous else {
            return Err(ContractError::InvalidValue(
                "previous audit segment is absent".into(),
            ));
        };
        if record.sequence != expected_tail
            || Some(digest) != frontier.current_segment_previous_digest
        {
            return Err(ContractError::InvalidValue(
                "rotated audit segment boundary is not authenticated".into(),
            ));
        }
        tail = Some((record, digest));
    }
    if let Some(current) = scan_audit_segment(
        &state.audit_segments,
        frontier.current_segment_start,
        frontier.current_segment_previous_digest,
        &installation.installation_uuid,
        true,
    )? {
        tail = Some(current);
    }
    let (tail_sequence, tail_digest, tail_record) = match tail {
        Some((record, digest)) => (record.sequence, digest, Some(record)),
        None => (0, AuthorityKey::from_bytes([0; 32]), None),
    };
    let checkpoint_frontier = checkpoint
        .last_sequence
        .checked_add(1)
        .ok_or_else(|| ContractError::InvalidValue("audit checkpoint sequence overflow".into()))?;
    if tail_sequence < checkpoint.last_sequence || tail_sequence > checkpoint_frontier {
        return Err(ContractError::InvalidValue(
            "audit segment tail is outside the recoverable checkpoint frontier".into(),
        ));
    }
    if tail_sequence == checkpoint.last_sequence {
        if tail_digest != checkpoint.last_digest {
            return Err(ContractError::InvalidValue(
                "audit checkpoint digest does not match segment tail".into(),
            ));
        }
    } else {
        let record = tail_record
            .as_ref()
            .expect("nonzero repaired tail has a record");
        let expected_previous = (record.sequence > 1).then_some(checkpoint.last_digest);
        if record.previous_digest != expected_previous {
            return Err(ContractError::InvalidValue(
                "recoverable audit tail does not extend its checkpoint".into(),
            ));
        }
        checkpoint.last_sequence = tail_sequence;
        checkpoint.last_digest = tail_digest;
        checkpoint.record_id = derive_key(
            "audit-checkpoint",
            &[
                checkpoint.installation_uuid.as_bytes(),
                &tail_sequence.to_be_bytes(),
                tail_digest.as_bytes(),
            ],
        );
        replace_record_at(&state.audit, b"checkpoint.json", &checkpoint, temporary_tag)?;
    }
    if let Some(record) = &tail_record {
        ensure_audit_receipt(state, record, tail_digest, temporary_tag)?;
    }
    let expected_next = tail_sequence
        .checked_add(1)
        .ok_or_else(|| ContractError::InvalidValue("audit sequence overflow".into()))?;
    if installation.next_audit_sequence != expected_next {
        if installation.next_audit_sequence != tail_sequence {
            return Err(ContractError::InvalidValue(
                "installation audit sequence is outside the recoverable frontier".into(),
            ));
        }
        installation.next_audit_sequence = expected_next;
        replace_record_at(&state.root, b"state.json", &installation, temporary_tag)?;
    }
    Ok(installation)
}

fn scan_audit_segment(
    segments: &File,
    segment_start: u64,
    starting_previous: Option<AuthorityKey>,
    installation_uuid: &str,
    repair_partial_tail: bool,
) -> Result<Option<(AuditRecordV1, AuthorityKey)>> {
    let segment_name = format!("{segment_start}.jsonl");
    let mut bytes =
        read_bytes_at(segments, segment_name.as_bytes(), 128 * 1024 * 1024)?.unwrap_or_default();
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        if !repair_partial_tail {
            return Err(ContractError::InvalidValue(
                "completed audit segment has a partial record".into(),
            ));
        }
        let committed_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        truncate_file_at(segments, segment_name.as_bytes(), committed_len as u64)?;
        bytes.truncate(committed_len);
    }
    let segment_end = segment_start
        .checked_add(AUDIT_SEGMENT_RECORDS - 1)
        .ok_or_else(|| ContractError::InvalidValue("audit segment overflow".into()))?;
    let mut expected_sequence = segment_start;
    let mut previous_digest = starting_previous;
    let mut tail = None;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let record: AuditRecordV1 = parse_record(line)?;
        if record.installation_uuid != installation_uuid
            || record.sequence != expected_sequence
            || record.sequence > segment_end
            || record.previous_digest != previous_digest
        {
            return Err(ContractError::InvalidValue(
                "audit segment chain is structurally invalid".into(),
            ));
        }
        let digest = canonical_digest(&record)?;
        previous_digest = Some(digest);
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or_else(|| ContractError::InvalidValue("audit sequence overflow".into()))?;
        tail = Some((record, digest));
    }
    Ok(tail)
}

fn ensure_audit_receipt(
    state: &MaintenanceStateV1,
    record: &AuditRecordV1,
    audit_digest: AuthorityKey,
    temporary_tag: &str,
) -> Result<()> {
    let receipt = audit_receipt(record, audit_digest);
    let name = format!("{}.json", record.request_id);
    if let Some(existing) =
        read_record_at::<AuditReceiptV1>(&state.audit_receipts, name.as_bytes())?
    {
        if existing != receipt {
            return Err(ContractError::InvalidValue(
                "audit receipt conflicts with the durable segment tail".into(),
            ));
        }
        return Ok(());
    }
    create_record_no_replace_at(
        &state.audit_receipts,
        name.as_bytes(),
        &receipt,
        temporary_tag,
    )
}

pub fn audit_receipt(record: &AuditRecordV1, audit_digest: AuthorityKey) -> AuditReceiptV1 {
    AuditReceiptV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "audit_receipt".into(),
        record_id: derive_key(
            "audit-receipt",
            &[
                record.installation_uuid.as_bytes(),
                record.request_id.as_bytes(),
                record.command.as_bytes(),
                match record.outcome {
                    crate::ResultStatus::Success => b"success",
                    crate::ResultStatus::Failure => b"failure",
                    crate::ResultStatus::Degraded => b"degraded",
                },
                &record.sequence.to_be_bytes(),
                audit_digest.as_bytes(),
            ],
        ),
        installation_uuid: record.installation_uuid.clone(),
        request_id: record.request_id.clone(),
        command: record.command.clone(),
        outcome: record.outcome,
        sequence: record.sequence,
        audit_digest,
    }
}

fn audit_frontier(
    installation_uuid: &str,
    current_segment_start: u64,
    current_segment_previous_digest: Option<AuthorityKey>,
    previous_segment_start: Option<u64>,
    previous_segment_previous_digest: Option<AuthorityKey>,
) -> AuditFrontierV1 {
    let zero = AuthorityKey::from_bytes([0; 32]);
    let previous_start = previous_segment_start.unwrap_or(0);
    AuditFrontierV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "audit_frontier".into(),
        record_id: derive_key(
            "audit-frontier",
            &[
                installation_uuid.as_bytes(),
                &current_segment_start.to_be_bytes(),
                current_segment_previous_digest.unwrap_or(zero).as_bytes(),
                &previous_start.to_be_bytes(),
                previous_segment_previous_digest.unwrap_or(zero).as_bytes(),
            ],
        ),
        installation_uuid: installation_uuid.into(),
        current_segment_start,
        current_segment_previous_digest,
        previous_segment_start,
        previous_segment_previous_digest,
    }
}

#[cfg(unix)]
fn truncate_file_at(parent: &File, name: &[u8], length: u64) -> Result<()> {
    use std::{ffi::CString, os::fd::FromRawFd, os::unix::fs::MetadataExt};

    validate_child_name(name)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("truncate name contains NUL".into()))?;
    // SAFETY: parent/name are valid; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    // SAFETY: openat returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ContractError::InvalidValue(
            "truncate target must be a user-owned regular file with mode 0600".into(),
        ));
    }
    file.set_len(length).map_err(ContractError::Filesystem)?;
    file.sync_all().map_err(ContractError::Filesystem)?;
    parent.sync_all().map_err(ContractError::Filesystem)
}

#[cfg(not(unix))]
fn truncate_file_at(_parent: &File, _name: &[u8], _length: u64) -> Result<()> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

#[cfg(unix)]
pub fn open_secure_dir_at(parent: &File, name: &[u8]) -> Result<Option<File>> {
    use std::{ffi::CString, os::fd::FromRawFd};

    validate_child_name(name)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("directory name contains NUL".into()))?;
    // SAFETY: parent/name are valid for the call; the returned descriptor is owned below.
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
            Err(ContractError::Filesystem(error))
        };
    }
    // SAFETY: openat returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    validate_secure_directory(&file)?;
    Ok(Some(file))
}

#[cfg(not(unix))]
pub fn open_secure_dir_at(_parent: &File, _name: &[u8]) -> Result<Option<File>> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

#[cfg(unix)]
pub fn open_or_create_secure_dir_at(parent: &File, name: &[u8]) -> Result<File> {
    use std::ffi::CString;

    if let Some(directory) = open_secure_dir_at(parent, name)? {
        return Ok(directory);
    }
    let encoded = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("directory name contains NUL".into()))?;
    // SAFETY: parent/name are valid for the call and no pointers are retained.
    if unsafe {
        libc::mkdirat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            encoded.as_ptr(),
            0o700,
        )
    } != 0
    {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(ContractError::Filesystem(error));
        }
    } else {
        parent.sync_all().map_err(ContractError::Filesystem)?;
    }
    open_secure_dir_at(parent, name)?
        .ok_or_else(|| ContractError::InvalidValue("directory disappeared after creation".into()))
}

#[cfg(not(unix))]
pub fn open_or_create_secure_dir_at(_parent: &File, _name: &[u8]) -> Result<File> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

pub fn read_record_at<T>(parent: &File, name: &[u8]) -> Result<Option<T>>
where
    T: Record + DeserializeOwned,
{
    read_record_with_identity_at(parent, name).map(|record| record.map(|(value, _)| value))
}

pub fn read_record_with_identity_at<T>(
    parent: &File,
    name: &[u8],
) -> Result<Option<(T, FileIdentityV1)>>
where
    T: Record + DeserializeOwned,
{
    let Some(mut file) = open_secure_file_at(parent, name)? else {
        return Ok(None);
    };
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    if metadata.len() > crate::MAX_RECORD_BYTES as u64 {
        return Err(ContractError::RecordTooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(crate::MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ContractError::Filesystem)?;
    let after = file.metadata().map_err(ContractError::Filesystem)?;
    if metadata.len() != after.len()
        || file_identity_from_metadata(&metadata)? != file_identity_from_metadata(&after)?
    {
        return Err(ContractError::InvalidValue(
            "record changed during read".into(),
        ));
    }
    let identity = file_identity_from_metadata(&after)?;
    parse_record(&bytes).map(|record| Some((record, identity)))
}

pub fn read_bytes_at(parent: &File, name: &[u8], limit: usize) -> Result<Option<Vec<u8>>> {
    let Some(mut file) = open_secure_file_at(parent, name)? else {
        return Ok(None);
    };
    let before = file.metadata().map_err(ContractError::Filesystem)?;
    if before.len() > limit as u64 {
        return Err(ContractError::InvalidValue(format!(
            "file exceeds the {limit}-byte limit"
        )));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ContractError::Filesystem)?;
    let after = file.metadata().map_err(ContractError::Filesystem)?;
    if bytes.len() > limit
        || file_identity_from_metadata(&before)? != file_identity_from_metadata(&after)?
    {
        return Err(ContractError::InvalidValue(
            "file exceeded its limit or changed during read".into(),
        ));
    }
    Ok(Some(bytes))
}

pub fn record_identity_at(parent: &File, name: &[u8]) -> Result<Option<FileIdentityV1>> {
    open_secure_file_at(parent, name)?
        .as_ref()
        .map(file_identity)
        .transpose()
}

#[cfg(unix)]
pub fn entry_identity_at(parent: &File, name: &[u8]) -> Result<Option<FileIdentityV1>> {
    use std::ffi::CString;

    validate_child_name(name)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("entry name contains NUL".into()))?;
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
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(ContractError::Filesystem(error))
        };
    }
    // SAFETY: fstatat succeeded.
    let metadata = unsafe { metadata.assume_init() };
    let modified_ns = metadata
        .st_mtime
        .checked_mul(1_000_000_000)
        .and_then(|seconds| seconds.checked_add(metadata.st_mtime_nsec))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| ContractError::InvalidValue("entry mtime cannot be represented".into()))?;
    // dev_t is not u64 on every supported Unix target.
    #[allow(clippy::unnecessary_cast)]
    let device = metadata.st_dev as u64;
    Ok(Some(FileIdentityV1 {
        device,
        inode: metadata.st_ino,
        size: metadata.st_size as u64,
        modified_ns,
    }))
}

#[cfg(not(unix))]
pub fn entry_identity_at(_parent: &File, _name: &[u8]) -> Result<Option<FileIdentityV1>> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 identity supports Unix only".into(),
    ))
}

#[cfg(unix)]
pub fn rename_entry_no_replace_at(
    source_parent: &File,
    source: &[u8],
    destination_parent: &File,
    destination: &[u8],
) -> Result<()> {
    use std::ffi::CString;

    validate_child_name(source)?;
    validate_child_name(destination)?;
    let source = CString::new(source)
        .map_err(|_| ContractError::InvalidValue("source name contains NUL".into()))?;
    let destination = CString::new(destination)
        .map_err(|_| ContractError::InvalidValue("destination name contains NUL".into()))?;
    let result =
        rename_no_replace_between(source_parent, &source, destination_parent, &destination);
    if result != 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    source_parent
        .sync_all()
        .map_err(ContractError::Filesystem)?;
    destination_parent
        .sync_all()
        .map_err(ContractError::Filesystem)
}

#[cfg(not(unix))]
pub fn rename_entry_no_replace_at(
    _source_parent: &File,
    _source: &[u8],
    _destination_parent: &File,
    _destination: &[u8],
) -> Result<()> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

#[cfg(unix)]
pub fn append_bytes_at(parent: &File, name: &[u8], bytes: &[u8]) -> Result<FileIdentityV1> {
    use std::{ffi::CString, os::fd::FromRawFd, os::unix::fs::MetadataExt};

    validate_child_name(name)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("append name contains NUL".into()))?;
    // SAFETY: parent/name are valid; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ContractError::InvalidValue(
            "append target must be a user-owned regular file with mode 0600".into(),
        ));
    }
    file.write_all(bytes).map_err(ContractError::Filesystem)?;
    file.sync_all().map_err(ContractError::Filesystem)?;
    parent.sync_all().map_err(ContractError::Filesystem)?;
    file_identity(&file)
}

#[cfg(not(unix))]
pub fn append_bytes_at(_parent: &File, _name: &[u8], _bytes: &[u8]) -> Result<FileIdentityV1> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

pub fn create_record_no_replace_at<T: Record + Serialize>(
    parent: &File,
    name: &[u8],
    value: &T,
    temporary_tag: &str,
) -> Result<()> {
    value.validate()?;
    let bytes = canonical_json(value)?;
    write_record_at(parent, name, &bytes, temporary_tag, false)
}

pub fn replace_record_at<T: Record + Serialize>(
    parent: &File,
    name: &[u8],
    value: &T,
    temporary_tag: &str,
) -> Result<()> {
    value.validate()?;
    let bytes = canonical_json(value)?;
    write_record_at(parent, name, &bytes, temporary_tag, true)
}

#[cfg(unix)]
pub fn remove_record_at(parent: &File, name: &[u8]) -> Result<()> {
    use std::ffi::CString;

    validate_child_name(name)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("record name contains NUL".into()))?;
    // SAFETY: parent/name are valid and unlinkat retains no pointer.
    if unsafe { libc::unlinkat(std::os::fd::AsRawFd::as_raw_fd(parent), name.as_ptr(), 0) } != 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    parent.sync_all().map_err(ContractError::Filesystem)
}

#[cfg(not(unix))]
pub fn remove_record_at(_parent: &File, _name: &[u8]) -> Result<()> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

#[cfg(unix)]
pub fn file_identity(file: &File) -> Result<FileIdentityV1> {
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    file_identity_from_metadata(&metadata)
}

#[cfg(unix)]
fn file_identity_from_metadata(metadata: &std::fs::Metadata) -> Result<FileIdentityV1> {
    use std::os::unix::fs::MetadataExt;

    let modified_ns = metadata
        .mtime()
        .checked_mul(1_000_000_000)
        .and_then(|seconds| seconds.checked_add(metadata.mtime_nsec()))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| ContractError::InvalidValue("file mtime cannot be represented".into()))?;
    Ok(FileIdentityV1 {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.len(),
        modified_ns,
    })
}

#[cfg(not(unix))]
pub fn file_identity(_file: &File) -> Result<FileIdentityV1> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 identity supports Unix only".into(),
    ))
}

#[cfg(not(unix))]
fn file_identity_from_metadata(_metadata: &std::fs::Metadata) -> Result<FileIdentityV1> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 identity supports Unix only".into(),
    ))
}

#[cfg(unix)]
pub fn path_identity(file: &File) -> Result<PathIdentityV1> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};

    let path = descriptor_path(file)?;
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    PathIdentityV1::unix(path.as_os_str().as_bytes(), metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
pub fn path_identity(_file: &File) -> Result<PathIdentityV1> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 identity supports Unix only".into(),
    ))
}

#[cfg(unix)]
fn open_secure_file_at(parent: &File, name: &[u8]) -> Result<Option<File>> {
    use std::{ffi::CString, os::fd::FromRawFd, os::unix::fs::MetadataExt};

    validate_child_name(name)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("record name contains NUL".into()))?;
    // SAFETY: parent/name are valid; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(ContractError::Filesystem(error))
        };
    }
    // SAFETY: openat returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ContractError::InvalidValue(
            "record must be a user-owned regular file with mode 0600".into(),
        ));
    }
    Ok(Some(file))
}

#[cfg(not(unix))]
fn open_secure_file_at(_parent: &File, _name: &[u8]) -> Result<Option<File>> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

#[cfg(unix)]
fn write_record_at(
    parent: &File,
    name: &[u8],
    bytes: &[u8],
    temporary_tag: &str,
    replace: bool,
) -> Result<()> {
    use std::{ffi::CString, os::fd::FromRawFd};

    validate_child_name(name)?;
    validate_child_name(temporary_tag.as_bytes())?;
    let counter = TEMPORARY_NONCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = Sha256::new();
    hasher.update(name);
    hasher.update(temporary_tag.as_bytes());
    hasher.update(std::process::id().to_be_bytes());
    hasher.update(timestamp.to_be_bytes());
    hasher.update(counter.to_be_bytes());
    let temporary = format!(".{:x}.tmp", hasher.finalize()).into_bytes();
    validate_child_name(&temporary)?;
    let name = CString::new(name)
        .map_err(|_| ContractError::InvalidValue("record name contains NUL".into()))?;
    let temporary = CString::new(temporary)
        .map_err(|_| ContractError::InvalidValue("temporary name contains NUL".into()))?;
    // SAFETY: parent/name are valid; the returned descriptor is owned below.
    let descriptor = unsafe {
        libc::openat(
            std::os::fd::AsRawFd::as_raw_fd(parent),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    let write_result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = unsafe {
            libc::unlinkat(
                std::os::fd::AsRawFd::as_raw_fd(parent),
                temporary.as_ptr(),
                0,
            )
        };
        return Err(ContractError::Filesystem(error));
    }
    let result = if replace {
        // SAFETY: both names are children of the same valid parent descriptor.
        unsafe {
            libc::renameat(
                std::os::fd::AsRawFd::as_raw_fd(parent),
                temporary.as_ptr(),
                std::os::fd::AsRawFd::as_raw_fd(parent),
                name.as_ptr(),
            )
        }
    } else {
        rename_no_replace(parent, &temporary, &name)
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: temporary is a validated child name and cleanup is best-effort.
        let _ = unsafe {
            libc::unlinkat(
                std::os::fd::AsRawFd::as_raw_fd(parent),
                temporary.as_ptr(),
                0,
            )
        };
        return Err(ContractError::Filesystem(error));
    }
    parent.sync_all().map_err(ContractError::Filesystem)
}

#[cfg(not(unix))]
fn write_record_at(
    _parent: &File,
    _name: &[u8],
    _bytes: &[u8],
    _temporary_tag: &str,
    _replace: bool,
) -> Result<()> {
    Err(ContractError::InvalidValue(
        "maintenance protocol v1 state supports Unix only".into(),
    ))
}

#[cfg(target_os = "linux")]
fn rename_no_replace(parent: &File, source: &std::ffi::CStr, destination: &std::ffi::CStr) -> i32 {
    rename_no_replace_between(parent, source, parent, destination)
}

#[cfg(target_os = "linux")]
fn rename_no_replace_between(
    source_parent: &File,
    source: &std::ffi::CStr,
    destination_parent: &File,
    destination: &std::ffi::CStr,
) -> i32 {
    // SAFETY: names and directory descriptors remain valid for the syscall.
    unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            std::os::fd::AsRawFd::as_raw_fd(source_parent),
            source.as_ptr(),
            std::os::fd::AsRawFd::as_raw_fd(destination_parent),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        ) as i32
    }
}

#[cfg(target_os = "macos")]
fn rename_no_replace(parent: &File, source: &std::ffi::CStr, destination: &std::ffi::CStr) -> i32 {
    rename_no_replace_between(parent, source, parent, destination)
}

#[cfg(target_os = "macos")]
fn rename_no_replace_between(
    source_parent: &File,
    source: &std::ffi::CStr,
    destination_parent: &File,
    destination: &std::ffi::CStr,
) -> i32 {
    // SAFETY: names and directory descriptors remain valid for the syscall.
    unsafe {
        libc::renameatx_np(
            std::os::fd::AsRawFd::as_raw_fd(source_parent),
            source.as_ptr(),
            std::os::fd::AsRawFd::as_raw_fd(destination_parent),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    }
}

#[cfg(unix)]
fn validate_secure_directory(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata().map_err(ContractError::Filesystem)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(ContractError::InvalidValue(
            "state directory must be user-owned with mode 0700".into(),
        ));
    }
    Ok(())
}

fn acquire_or_create_lock(parent: &File, name: &[u8], nonblocking: bool) -> Result<ProtocolLock> {
    match ProtocolLock::acquire_at(parent, name, LockMode::Exclusive, true, nonblocking) {
        Ok(lock) => Ok(lock),
        Err(ContractError::Lock(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ProtocolLock::acquire_at(parent, name, LockMode::Exclusive, false, nonblocking)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn descriptor_path(file: &File) -> Result<std::path::PathBuf> {
    use std::{ffi::CStr, os::fd::AsRawFd, os::unix::ffi::OsStrExt};

    let mut buffer = vec![0_i8; libc::PATH_MAX as usize];
    // SAFETY: buffer is writable and large enough for F_GETPATH's documented result.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } < 0 {
        return Err(ContractError::Filesystem(std::io::Error::last_os_error()));
    }
    // SAFETY: successful F_GETPATH writes a NUL-terminated path.
    let bytes = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes();
    Ok(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(target_os = "linux")]
fn descriptor_path(file: &File) -> Result<std::path::PathBuf> {
    use std::os::fd::AsRawFd;

    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(ContractError::Filesystem)
}

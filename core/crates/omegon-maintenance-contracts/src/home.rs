//! Persisted home continuity and recovery framing. Mutation orchestration belongs
//! to the maintenance companion; admission only validates these records.
use crate::{
    AuthorityKey, ContractError, InstallationStateV1, PathIdentityV1, Record, Result,
    SCHEMA_VERSION, canonical_json, derive_key, read_record_at,
};
use serde::{Deserialize, Serialize};
use std::fs::File;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeContinuityV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub installation_uuid: String,
    pub home: PathIdentityV1,
    pub volume_uuid: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeRecoveryIntentV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub record_id: AuthorityKey,
    pub request_id: String,
    pub original: InstallationStateV1,
    pub target: PathIdentityV1,
    pub continuity: Option<HomeContinuityV1>,
}
impl HomeRecoveryIntentV1 {
    pub fn new(
        request_id: String,
        original: InstallationStateV1,
        target: PathIdentityV1,
        continuity: Option<HomeContinuityV1>,
    ) -> Result<Self> {
        let mut value = Self {
            schema_version: SCHEMA_VERSION,
            record_kind: "home_recovery_intent".into(),
            record_id: derive_key("empty", &[]),
            request_id,
            original,
            target,
            continuity,
        };
        value.record_id = value.digest()?;
        value.validate()?;
        Ok(value)
    }
    fn digest(&self) -> Result<AuthorityKey> {
        Ok(derive_key(
            "home-recovery-intent",
            &[
                self.request_id.as_bytes(),
                &canonical_json(&self.original)?,
                &canonical_json(&self.target)?,
                &canonical_json(&self.continuity)?,
            ],
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeRecoveryPhase {
    Prepared,
    Rebound,
    Audited,
    Settled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeRecoveryJournalV1 {
    pub schema_version: u32,
    pub record_kind: String,
    pub request_id: String,
    pub intent_key: AuthorityKey,
    pub phase: HomeRecoveryPhase,
}

macro_rules! home_record {
    ($ty:ty, $kind:literal, $check:expr) => {
        impl Record for $ty {
            const RECORD_KIND: &'static str = $kind;
            fn schema_version(&self) -> u32 {
                self.schema_version
            }
            fn record_kind(&self) -> &str {
                &self.record_kind
            }
            fn validate(&self) -> Result<()> {
                if self.schema_version != SCHEMA_VERSION {
                    return Err(ContractError::UnsupportedSchema(self.schema_version));
                }
                if self.record_kind != Self::RECORD_KIND {
                    return Err(ContractError::RecordKind {
                        expected: Self::RECORD_KIND,
                        actual: self.record_kind.clone(),
                    });
                }
                ($check)(self)
            }
        }
    };
}
home_record!(
    HomeContinuityV1,
    "home_continuity",
    |value: &HomeContinuityV1| {
        value.home.validate()?;
        if value.installation_uuid.is_empty()
            || value.volume_uuid.len() != 32
            || !value.volume_uuid.bytes().all(|b| b.is_ascii_hexdigit())
            || value.volume_uuid.bytes().all(|b| b == b'0')
        {
            return Err(ContractError::InvalidValue(
                "invalid stable home continuity".into(),
            ));
        }
        Ok(())
    }
);
home_record!(
    HomeRecoveryIntentV1,
    "home_recovery_intent",
    |value: &HomeRecoveryIntentV1| {
        value.original.validate()?;
        value.target.validate()?;
        crate::validate_child_name(value.request_id.as_bytes())?;
        if !same_home_directory(&value.original.home, &value.target)
            || value.digest()? != value.record_id
        {
            return Err(ContractError::InvalidValue(
                "recovery intent identity or digest mismatch".into(),
            ));
        }
        if let Some(binding) = &value.continuity {
            binding.validate()?;
            if binding.home != value.target
                || binding.installation_uuid != value.original.installation_uuid
            {
                return Err(ContractError::InvalidValue(
                    "recovery continuity does not bind target installation".into(),
                ));
            }
        }
        Ok(())
    }
);
home_record!(
    HomeRecoveryJournalV1,
    "home_recovery_journal",
    |value: &HomeRecoveryJournalV1| { crate::validate_child_name(value.request_id.as_bytes()) }
);

pub fn same_home_directory(a: &PathIdentityV1, b: &PathIdentityV1) -> bool {
    a.dialect == b.dialect && a.path_bytes == b.path_bytes && a.key == b.key && a.inode == b.inode
}

pub fn ensure_home_recovery_settled(root: &File) -> Result<()> {
    let Some(journal) = read_record_at::<HomeRecoveryJournalV1>(root, b"home-recovery.json")?
    else {
        return Ok(());
    };
    if journal.phase != HomeRecoveryPhase::Settled {
        return Err(ContractError::HomeRecoveryPending);
    }
    let directory = crate::open_secure_dir_at(root, b"home-recoveries")?.ok_or_else(|| {
        ContractError::InvalidValue("settled recovery lacks immutable intent directory".into())
    })?;
    let intent: HomeRecoveryIntentV1 = read_record_at(
        &directory,
        format!("{}.json", journal.request_id).as_bytes(),
    )?
    .ok_or_else(|| ContractError::InvalidValue("settled recovery lacks immutable intent".into()))?;
    let installation: InstallationStateV1 =
        read_record_at(root, b"state.json")?.ok_or_else(|| {
            ContractError::InvalidValue("settled recovery lacks installation state".into())
        })?;
    let audit = crate::open_secure_dir_at(root, b"audit")?.ok_or_else(|| {
        ContractError::InvalidValue("settled recovery lacks audit directory".into())
    })?;
    let receipts = crate::open_secure_dir_at(&audit, b"receipts")?.ok_or_else(|| {
        ContractError::InvalidValue("settled recovery lacks audit receipts".into())
    })?;
    let receipt: crate::AuditReceiptV1 =
        read_record_at(&receipts, format!("{}.json", journal.request_id).as_bytes())?.ok_or_else(
            || ContractError::InvalidValue("settled recovery lacks audit receipt".into()),
        )?;
    if journal.intent_key != intent.record_id
        || journal.request_id != intent.request_id
        || installation.installation_uuid != intent.original.installation_uuid
        || installation.record_id != intent.original.record_id
        || installation.home != intent.target
        || receipt.installation_uuid != installation.installation_uuid
        || receipt.request_id != intent.request_id
        || receipt.command != "home.recover"
        || receipt.outcome != crate::ResultStatus::Success
        || receipt.sequence != intent.original.next_audit_sequence
        || installation.next_audit_sequence <= receipt.sequence
    {
        return Err(ContractError::InvalidValue(
            "settled recovery evidence does not match its intent and audit receipt".into(),
        ));
    }
    Ok(())
}

pub fn home_binding_matches(
    home: &File,
    root: &File,
    installation: &InstallationStateV1,
    observed: &PathIdentityV1,
) -> Result<bool> {
    let binding = read_record_at::<HomeContinuityV1>(root, b"home-continuity.json")?;
    if let Some(binding) = binding {
        if binding.installation_uuid != installation.installation_uuid
            || binding.home != installation.home
        {
            return Err(ContractError::InvalidValue(
                "stable continuity does not match installation state".into(),
            ));
        }
        return Ok(same_home_directory(&binding.home, observed)
            && stable_home_volume_uuid(home)?.as_deref() == Some(binding.volume_uuid.as_str()));
    }
    Ok(installation.home == *observed)
}

/// Descriptor-based macOS volume UUID. Unsupported/unavailable evidence is never
/// substituted with a device number, mount name, or guessed filesystem identity.
#[cfg(target_os = "macos")]
pub fn stable_home_volume_uuid(home: &File) -> Result<Option<String>> {
    use std::os::fd::AsRawFd;
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_UUID,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buffer = [0_u8; 20];
    // SAFETY: the opened descriptor and initialized attribute/buffer allocations
    // remain valid. This single volume attribute returns length (u32) + UUID (16).
    if unsafe {
        libc::fgetattrlist(
            home.as_raw_fd(),
            (&mut attributes as *mut libc::attrlist).cast(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            0,
        )
    } != 0
    {
        return Ok(None);
    }
    if u32::from_ne_bytes(buffer[..4].try_into().expect("four bytes")) != 20
        || buffer[4..].iter().all(|b| *b == 0)
    {
        return Ok(None);
    }
    Ok(Some(
        buffer[4..].iter().map(|b| format!("{b:02x}")).collect(),
    ))
}
#[cfg(not(target_os = "macos"))]
pub fn stable_home_volume_uuid(_home: &File) -> Result<Option<String>> {
    Ok(None)
}

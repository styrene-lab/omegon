use super::schema::EvidenceRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimSupportStatus {
    Supported,
    Refuted,
    Mixed,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimSupportSummary {
    pub claim_id: String,
    pub supports: Vec<EvidenceRecord>,
    pub refutes: Vec<EvidenceRecord>,
    pub stale: Vec<EvidenceRecord>,
    pub supersedes: Vec<EvidenceRecord>,
    pub status: ClaimSupportStatus,
}

impl ClaimSupportSummary {
    pub fn new(claim_id: String) -> Self {
        Self {
            claim_id,
            supports: Vec::new(),
            refutes: Vec::new(),
            stale: Vec::new(),
            supersedes: Vec::new(),
            status: ClaimSupportStatus::Unknown,
        }
    }

    pub fn finalize(mut self, claim_exists: bool) -> Self {
        self.status = match (
            !self.supports.is_empty(),
            !self.refutes.is_empty(),
            !self.stale.is_empty(),
            claim_exists,
        ) {
            (true, false, _, _) => ClaimSupportStatus::Supported,
            (false, true, _, _) => ClaimSupportStatus::Refuted,
            (true, true, _, _) => ClaimSupportStatus::Mixed,
            (false, false, true, _) => ClaimSupportStatus::Unsupported,
            (false, false, false, true) => ClaimSupportStatus::Unsupported,
            (false, false, false, false) => ClaimSupportStatus::Unknown,
        };
        self
    }
}

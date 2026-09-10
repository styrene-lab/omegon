//! Portable attribution for host-validated explicit lifecycle artifacts.
pub use crate::types::{LifecycleConclusionKind, LifecycleConclusionSource};
use crate::{Fact, MemoryError, MemoryMutation, Section};

const PREFIX: &str = "lifecycle-conclusion:v1:";

/// Digest of the exact persisted candidate shown for operator review.
pub fn candidate_snapshot_hash(fact: &Fact) -> crate::backend::Result<String> {
    let encoded =
        serde_json::to_string(fact).map_err(|error| MemoryError::Storage(error.into()))?;
    Ok(crate::retrieval::raw_content_hash(&encoded))
}

pub(crate) fn confirm_candidate(
    fact: &mut Fact,
    hash: &str,
    session_id: String,
    request_id: String,
    surface: crate::ConfirmationSurface,
    supersedes: Option<&crate::FactPrecondition>,
) -> crate::backend::Result<()> {
    if fact.status != crate::FactStatus::Pending || candidate_snapshot_hash(fact)? != hash {
        return Err(MemoryError::InvalidMutation(
            "candidate changed since operator review".into(),
        ));
    }
    let Some(inference) = fact.lifecycle_inference.as_mut() else {
        return Err(MemoryError::InvalidMutation(
            "candidate attribution is missing".into(),
        ));
    };
    if inference.proposed_supersedes.as_deref() != supersedes.map(|target| target.id.as_str()) {
        return Err(MemoryError::InvalidMutation(
            "confirmation must preserve reviewed supersession intent".into(),
        ));
    }
    let timestamp = crate::util::now_iso();
    let confirmation = crate::CandidateConfirmation {
        reviewed_version: fact.version,
        snapshot_sha256: hash.into(),
        session_id,
        request_id,
        surface,
        confirmed_at: timestamp.clone(),
        content_sha256: crate::retrieval::raw_content_hash(&fact.content),
    };
    confirmation.validate()?;
    inference.confirmation = Some(confirmation);
    fact.status = crate::FactStatus::Active;
    fact.confidence = 1.0;
    fact.reinforcement_count = 1;
    fact.last_reinforced = timestamp;
    fact.superseded_by = supersedes.map(|target| target.id.clone());
    Ok(())
}

impl LifecycleConclusionSource {
    pub fn validate(&self, content: &str, section: &Section) -> crate::backend::Result<()> {
        let expected = match self.kind {
            LifecycleConclusionKind::Decision => Section::Decisions,
            LifecycleConclusionKind::Constraint => Section::Constraints,
            LifecycleConclusionKind::Specification => Section::Specs,
        };
        let bounded = |text: &str| {
            !text.trim().is_empty() && text.len() <= 2048 && !text.chars().any(char::is_control)
        };
        let path_ok = self
            .artifact_path
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
            && !self.artifact_path.contains(['\\', ':']);
        if &expected != section
            || content.trim().is_empty()
            || content.len() > 65_536
            || !bounded(&self.artifact_path)
            || !path_ok
            || !bounded(&self.artifact_sub)
            || self.artifact_id.as_ref().is_some_and(|id| !bounded(id))
            || self.artifact_sha256.len() != 64
            || !self
                .artifact_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.statement_sha256 != crate::retrieval::raw_content_hash(content)
        {
            return Err(MemoryError::InvalidMutation(
                "invalid explicit lifecycle attribution".into(),
            ));
        }
        Ok(())
    }

    fn encode(&self) -> crate::backend::Result<String> {
        serde_json::to_string(self)
            .map(|json| format!("{PREFIX}{json}"))
            .map_err(|error| MemoryError::Storage(error.into()))
    }
}

impl Fact {
    /// Decode attribution without interpreting it as a capability or live source check.
    pub fn lifecycle_conclusion(
        &self,
    ) -> crate::backend::Result<Option<LifecycleConclusionSource>> {
        let Some(encoded) = self
            .source
            .as_deref()
            .and_then(|source| source.strip_prefix(PREFIX))
        else {
            return Ok(None);
        };
        let source: LifecycleConclusionSource =
            serde_json::from_str(encoded).map_err(|error| MemoryError::Storage(error.into()))?;
        source.validate(&self.content, &self.section)?;
        Ok(Some(source))
    }
}

pub(crate) fn requires_exact_source(source: Option<&str>) -> bool {
    source.is_some_and(|source| source.starts_with(PREFIX))
}

/// Called after receipt lookup, inside the backend's atomic mutation boundary.
pub(crate) fn lower(mutation: MemoryMutation) -> crate::backend::Result<MemoryMutation> {
    match mutation {
        MemoryMutation::StoreLifecycleConclusion {
            mut request,
            source,
            supersedes,
        } => {
            source.validate(&request.content, &request.section)?;
            request.source = Some(source.encode()?);
            Ok(match supersedes {
                Some(fact) => MemoryMutation::SupersedeFact {
                    fact,
                    replacement: request,
                },
                None => MemoryMutation::StoreFact { request },
            })
        }
        other => Ok(other),
    }
}

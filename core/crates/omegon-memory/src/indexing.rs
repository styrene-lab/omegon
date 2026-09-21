//! Shared indexing-attempt admission and completion rules.
use crate::{
    backend::{MemoryError, Result},
    types::*,
};

pub(crate) fn validate_record(record: &EmbeddingIndexingRecord) -> Result<()> {
    validate_attempt(&record.attempt_id)?;
    if let Some(space) = &record.space {
        space.validate()?;
    }
    Ok(())
}

pub(crate) fn validate_attempt(attempt: &str) -> Result<()> {
    if attempt.is_empty() || attempt.len() > 256 || attempt.chars().any(char::is_control) {
        return Err(MemoryError::InvalidMutation(
            "invalid embedding attempt identifier".into(),
        ));
    }
    Ok(())
}

pub(crate) fn admit_record(
    previous: Option<&EmbeddingIndexingRecord>,
    record: &EmbeddingIndexingRecord,
) -> Result<()> {
    validate_record(record)?;
    if record.reason != EmbeddingIndexingReason::Pending {
        let previous = previous.ok_or_else(|| {
            MemoryError::InvalidMutation("embedding attempt is no longer pending".into())
        })?;
        if previous.fact != record.fact
            || previous.attempt_id != record.attempt_id
            || previous.space != record.space
        {
            return Err(MemoryError::InvalidMutation(
                "embedding attempt was replaced".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn admit_completion(
    previous: Option<&EmbeddingIndexingRecord>,
    fact: &FactPrecondition,
    embedding: &IdentifiedEmbedding,
    attempt: Option<&str>,
) -> Result<()> {
    if let Some(attempt) = attempt {
        validate_attempt(attempt)?;
        if !previous.is_some_and(|record| record.fact == *fact && record.attempt_id == attempt) {
            return Err(MemoryError::InvalidMutation(
                "embedding attempt was replaced or completed".into(),
            ));
        }
    }
    if let Some(record) = previous
        && record.fact == *fact
        && record
            .space
            .as_ref()
            .is_some_and(|space| space != &embedding.space)
    {
        return Err(MemoryError::InvalidMutation(
            "embedding completion has incompatible space".into(),
        ));
    }
    Ok(())
}

impl EmbeddingIndexingSummary {
    pub(crate) fn observe(&mut self, reason: EmbeddingIndexingReason, count: usize) {
        self.pending += count;
        if reason.retryable() {
            self.retryable += count;
        } else {
            self.terminal += count;
        }
        *self.reasons.entry(reason).or_default() += count;
    }
}

//! Bounded evidence validation and candidate parsing. Provider clients belong to the host.
use crate::{MemoryError, types::*};
use std::collections::HashSet;

pub const MAX_EVIDENCE_ITEMS: usize = 64;
pub const MAX_EXCERPT_BYTES: usize = 1024;
pub const MAX_EVIDENCE_BYTES: usize = 32_768;
pub const MAX_CANDIDATES: usize = 32;
pub const MAX_EXTRACTION_BYTES: usize = 65_536;
pub const MAX_IDENTIFIER_BYTES: usize = 512;

pub const MAX_RECOVERY_BATCH: usize = 8;

pub(crate) fn validate_recovery_limit(limit: usize) -> crate::backend::Result<()> {
    if limit > MAX_RECOVERY_BATCH {
        return Err(MemoryError::InvalidMutation(
            "formation recovery batch exceeds limit".into(),
        ));
    }
    Ok(())
}

fn invalid() -> MemoryError {
    MemoryError::InvalidMutation("invalid or oversized formation evidence".into())
}
fn identifier(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_IDENTIFIER_BYTES
}

fn source_identifier(value: &str) -> bool {
    identifier(value) && !value.chars().any(char::is_control)
}

pub fn capture_key(request: &StoreEpisode) -> crate::backend::Result<FormationCaptureKey> {
    let formation = request.formation.as_deref().ok_or_else(invalid)?;
    formation.validate()?;
    let coverage = formation.coverage.as_ref().ok_or_else(invalid)?;
    let FormationSource::Available {
        session_id,
        stream_id,
        sequence,
        ..
    } = &formation.source
    else {
        return Err(invalid());
    };
    if *sequence > i64::MAX as u64 {
        return Err(invalid());
    }
    let model = match &formation.extraction {
        ExtractionOutcome::Pending { model } => Some(model.clone()),
        ExtractionOutcome::Disabled => None,
        _ => return Err(invalid()),
    };
    Ok(FormationCaptureKey {
        mind: request.mind.clone(),
        session_id: session_id.clone(),
        stream_id: stream_id.clone(),
        policy_version: coverage.policy_version,
        model,
    })
}

pub(crate) fn advance_capture(
    request: &StoreEpisode,
    expected: Option<&FormationCursor>,
    actual: Option<&FormationCursor>,
) -> crate::backend::Result<FormationCursor> {
    capture_key(request)?;
    let formation = request.formation.as_deref().ok_or_else(invalid)?;
    let coverage = formation.coverage.as_ref().ok_or_else(invalid)?;
    if expected != actual
        || actual
            .is_some_and(|cursor| cursor.sequence.checked_add(1) != Some(coverage.first_sequence))
    {
        return Err(MemoryError::InvalidMutation(
            "capture cursor conflict or coverage gap".into(),
        ));
    }
    let FormationSource::Available {
        sequence, event_id, ..
    } = &formation.source
    else {
        return Err(invalid());
    };
    Ok(FormationCursor {
        sequence: *sequence,
        event_id: event_id.clone(),
    })
}

impl EpisodeFormation {
    pub fn validate(&self) -> crate::backend::Result<()> {
        if self.evidence.len() > MAX_EVIDENCE_ITEMS || self.candidates.len() > MAX_CANDIDATES {
            return Err(invalid());
        }
        let frontier = match &self.source {
            FormationSource::Available {
                session_id,
                stream_id,
                sequence,
                event_id,
            } => {
                if !source_identifier(session_id)
                    || !source_identifier(stream_id)
                    || !source_identifier(event_id)
                    || *sequence == 0
                {
                    return Err(invalid());
                }
                *sequence
            }
            FormationSource::Unavailable { session_id, reason } => {
                if !source_identifier(session_id)
                    || !identifier(reason)
                    || !self.evidence.is_empty()
                    || !self.candidates.is_empty()
                {
                    return Err(invalid());
                }
                0
            }
        };
        let first_sequence = match (self.version, &self.coverage) {
            (1, None) => 1,
            (2, Some(coverage))
                if coverage.policy_version == 1
                    && coverage.first_sequence > 0
                    && coverage.first_sequence <= frontier =>
            {
                coverage.first_sequence
            }
            _ => return Err(invalid()),
        };
        let mut ids = HashSet::new();
        let mut bytes = 0usize;
        let mut previous_sequence = 0;
        for item in &self.evidence {
            bytes = bytes.saturating_add(item.excerpt.len());
            if !source_identifier(&item.event_id)
                || !ids.insert(item.event_id.as_str())
                || item.sequence <= previous_sequence
                || item.sequence < first_sequence
                || item.sequence > frontier
                || matches!(&self.source, FormationSource::Available { event_id, sequence, .. }
                    if item.sequence == *sequence && item.event_id != *event_id)
                || chrono::DateTime::parse_from_rfc3339(&item.recorded_at).is_err()
                || item.excerpt.len() > MAX_EXCERPT_BYTES
                || bytes > MAX_EVIDENCE_BYTES
                || (item.kind == EvidenceKind::ToolResult) != item.outcome.is_some()
            {
                return Err(invalid());
            }
            previous_sequence = item.sequence;
        }
        for candidate in &self.candidates {
            if !candidate_valid(candidate, &ids) {
                return Err(invalid());
            }
        }
        match &self.extraction {
            ExtractionOutcome::Complete { model }
                if identifier(model)
                    && !self.evidence.is_empty()
                    && matches!(self.source, FormationSource::Available { .. }) => {}
            ExtractionOutcome::Disabled if self.candidates.is_empty() => {}
            ExtractionOutcome::Pending { model }
                if identifier(model) && self.candidates.is_empty() => {}
            ExtractionOutcome::Unavailable { model, reason }
                if identifier(model) && identifier(reason) && self.candidates.is_empty() => {}
            _ => return Err(invalid()),
        }
        Ok(())
    }

    pub fn narrative(&self) -> String {
        let mut text = format!(
            "Source: {:?}\nExtraction: {:?}\n",
            self.source, self.extraction
        );
        if let Some(coverage) = &self.coverage {
            text.push_str(&format!("Declared coverage: {coverage:?}\n"));
        }
        if self.truncated {
            text.push_str(
                "Evidence is a bounded excerpt; consult the source for omitted content.\n",
            );
        }
        for item in &self.evidence {
            text.push_str(&format!(
                "\n[{}] {:?} {:?}: {}\n",
                item.event_id, item.kind, item.outcome, item.excerpt
            ));
        }
        if !self.candidates.is_empty() {
            text.push_str("\nPending inferred candidates (not admitted facts):\n");
            for candidate in &self.candidates {
                text.push_str(&format!(
                    "- {:?}: {} [evidence: {}]\n",
                    candidate.section,
                    candidate.content,
                    candidate.evidence_ids.join(", ")
                ));
            }
        }
        text
    }
}

pub(crate) fn validate_completion(
    prior: Option<&EpisodeFormation>,
    next: &EpisodeFormation,
) -> crate::backend::Result<()> {
    next.validate()?;
    let Some(prior) = prior else {
        return Err(invalid());
    };
    let ExtractionOutcome::Pending { model: expected } = &prior.extraction else {
        return Err(invalid());
    };
    let model = match &next.extraction {
        ExtractionOutcome::Complete { model } | ExtractionOutcome::Unavailable { model, .. } => {
            model
        }
        _ => return Err(invalid()),
    };
    if expected != model
        || prior.version != next.version
        || prior.coverage != next.coverage
        || prior.source != next.source
        || prior.evidence != next.evidence
        || prior.truncated != next.truncated
    {
        return Err(invalid());
    }
    Ok(())
}

fn candidate_valid(candidate: &MemoryCandidate, ids: &HashSet<&str>) -> bool {
    !candidate.content.trim().is_empty()
        && candidate.content.len() <= 2048
        && !candidate.evidence_ids.is_empty()
        && candidate.evidence_ids.len() <= 16
        && candidate
            .evidence_ids
            .iter()
            .all(|id| ids.contains(id.as_str()))
}

pub(crate) fn completes_import(
    prior: &Episode,
    incoming: &Episode,
) -> crate::backend::Result<bool> {
    let (Some(old), Some(new)) = (&prior.formation, &incoming.formation) else {
        return Ok(false);
    };
    if !matches!(old.extraction, ExtractionOutcome::Pending { .. })
        || !matches!(
            new.extraction,
            ExtractionOutcome::Complete { .. } | ExtractionOutcome::Unavailable { .. }
        )
    {
        return Ok(false);
    }
    if prior.mind != incoming.mind {
        return Err(invalid());
    }
    validate_completion(prior.formation.as_deref(), new)?;
    Ok(true)
}

/// Parse model output without letting the model supply authority or verification fields.
/// Invalid siblings are counted independently; malformed whole output is an error.
pub fn parse_candidates(
    text: &str,
    evidence: &[FormationEvidence],
) -> crate::backend::Result<(Vec<MemoryCandidate>, usize)> {
    if text.len() > MAX_EXTRACTION_BYTES {
        return Err(invalid());
    }
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .and_then(|s| s.strip_suffix("```"))
        .or_else(|| text.strip_prefix("```").and_then(|s| s.strip_suffix("```")))
        .unwrap_or(text)
        .trim();
    let values: Vec<serde_json::Value> = serde_json::from_str(text).map_err(|_| invalid())?;
    let ids = evidence.iter().map(|item| item.event_id.as_str()).collect();
    let mut accepted = Vec::new();
    let mut rejected = values.len().saturating_sub(MAX_CANDIDATES);
    for value in values.into_iter().take(MAX_CANDIDATES) {
        match serde_json::from_value::<MemoryCandidate>(value) {
            Ok(candidate) if candidate_valid(&candidate, &ids) => accepted.push(candidate),
            _ => rejected += 1,
        }
    }
    Ok((accepted, rejected))
}

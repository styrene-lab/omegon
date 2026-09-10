//! Status-neutral, read-only memory inspection shared by tool adapters.
use crate::*;

fn excerpt(text: &str, limit: usize) -> (String, bool) {
    let mut characters = text.chars();
    let output = characters.by_ref().take(limit).collect();
    (output, characters.next().is_some())
}

impl FactInspection {
    pub fn from_fact(fact: &Fact) -> Self {
        let (content_excerpt, content_truncated) = excerpt(&fact.content, 2048);
        let (source_excerpt, source_truncated) = match &fact.source {
            Some(source) => {
                let (text, truncated) = excerpt(source, 512);
                (Some(text), truncated)
            }
            None => (None, false),
        };
        let mut inspection = Self {
            id: fact.id.clone(),
            mind: fact.mind.clone(),
            section: fact.section.clone(),
            status: fact.status.clone(),
            version: fact.version,
            content_excerpt,
            content_truncated,
            content_sha256: retrieval::raw_content_hash(&fact.content),
            created_at: fact.created_at.clone(),
            created_session: fact.created_session.clone(),
            last_reinforced: fact.last_reinforced.clone(),
            reinforcement_count: fact.reinforcement_count,
            confidence: fact.confidence,
            supersedes: fact.superseded_by.clone(),
            superseded_at: fact.superseded_at.clone(),
            archived_at: fact.archived_at.clone(),
            basis: ProvenanceBasis::LegacyUnknown,
            artifact: None,
            inference: None,
            source_excerpt,
            source_truncated,
            evidence_availability: EvidenceAvailability::NoReference,
            diagnostic: None,
        };
        if let Some(inference) = &fact.lifecycle_inference {
            if types::validate_inference_status(&fact.status, Some(inference), &fact.content)
                .is_err()
            {
                inspection.basis = ProvenanceBasis::InvalidMetadata;
                inspection.diagnostic =
                    Some("stored inference metadata does not match its record".into());
                return inspection;
            }
            inspection.basis = if inference.confirmation.is_some() {
                ProvenanceBasis::OperatorConfirmedInference
            } else {
                ProvenanceBasis::UnconfirmedInference
            };
            if inference.artifact_ref_path.is_some() {
                inspection.evidence_availability = EvidenceAvailability::NotChecked;
            }
            inspection.inference = Some(inference.clone());
        } else {
            match fact.lifecycle_conclusion() {
                Ok(Some(source)) => {
                    inspection.basis = ProvenanceBasis::ExplicitArtifact;
                    inspection.artifact = Some(Box::new(source));
                    inspection.evidence_availability = EvidenceAvailability::NotChecked;
                }
                Ok(None) => {}
                Err(_) => {
                    inspection.basis = ProvenanceBasis::InvalidMetadata;
                    inspection.diagnostic =
                        Some("stored lifecycle source does not match its record".into());
                }
            }
        }
        inspection
    }
}

pub fn fact_id(args: &serde_json::Value) -> backend::Result<&str> {
    let id = args
        .get("fact_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| MemoryError::InvalidMutation("fact_id is required".into()))?;
    if args.as_object().is_none_or(|object| object.len() != 1) || id.is_empty() || id.len() > 2048 {
        return Err(MemoryError::InvalidMutation(
            "inspection accepts one bounded fact_id".into(),
        ));
    }
    Ok(id)
}

pub fn render(inspection: &FactInspection) -> backend::Result<String> {
    let json = serde_json::to_string_pretty(inspection)
        .map_err(|error| MemoryError::Storage(error.into()))?;
    Ok(format!(
        "Read-only memory inspection. Availability is not verification of a claim or execution outcome.\n{json}"
    ))
}

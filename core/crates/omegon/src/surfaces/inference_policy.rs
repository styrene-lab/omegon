//! Renderer-neutral projection of a captured policy. No global settings reads.
use crate::inference_policy::ResolvedPolicy;
use omegon_traits::{InferencePolicyProjection, InferenceProviderUsage};

pub fn project(
    policy: Result<Option<&ResolvedPolicy>, String>,
    measured: bool,
    usage: Option<InferenceProviderUsage>,
) -> InferencePolicyProjection {
    match policy {
        Ok(Some(policy)) => {
            let mut details =
                serde_json::to_value(policy).expect("policy projection serialization");
            use crate::inference_policy::{CapacityField, EvidenceStatus};
            details["capacity"] = serde_json::json!({
                "advertised_default": policy.selected_facts.get(&CapacityField::DefaultWindow).filter(|fact| matches!(fact.provenance.status, EvidenceStatus::Declared | EvidenceStatus::Discovered)),
                "maximum_window": policy.selected_facts.get(&CapacityField::MaximumWindow),
                "maximum_input": policy.selected_facts.get(&CapacityField::MaximumInput),
                "maximum_total": policy.selected_facts.get(&CapacityField::MaximumTotal),
                "maximum_output": policy.selected_facts.get(&CapacityField::MaximumOutput),
            });
            details["usage_scope"] = serde_json::json!(if measured {
                "prepared request"
            } else {
                "available policy preview; no request estimate"
            });
            let now = chrono::Utc::now();
            let today = now.date_naive();
            let now_seconds = u64::try_from(now.timestamp()).unwrap_or(0);
            let ages: Vec<_> = policy.selected_facts.values().map(|fact| {
                let age = fact.provenance.reviewed_at.as_deref()
                    .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                    .map(|reviewed| today.signed_duration_since(reviewed).num_days());
                let stale = fact.provenance.stale || fact.provenance.valid_until.is_some_and(|expiry| now_seconds >= expiry);
                serde_json::json!({"field": fact.field, "review_age_days": age,
                    "observation_age_seconds": fact.provenance.observed_at.map(|observed| now_seconds.saturating_sub(observed)),
                    "freshness": if stale { "stale, last-known bounds retained" } else if fact.provenance.valid_until.is_some() { "within source validity interval; not entitlement evidence" } else { "no expiry declared; review date is not entitlement evidence" }})
            }).collect();
            details["evidence_age"] = serde_json::json!(ages);
            let input = measured
                .then(|| {
                    policy
                        .visible_input
                        .as_ref()
                        .and_then(|input| input.total().ok())
                })
                .flatten();
            details["thresholds_crossed"] = serde_json::json!({
                "warning": input.map(|tokens| tokens as f32 >= policy.input_budget as f32 * policy.warning_threshold),
                "proactive_compaction": input.map(|tokens| tokens as f32 >= policy.input_budget as f32 * policy.proactive_threshold),
                "emergency_compaction": input.map(|tokens| tokens as f32 > policy.input_budget as f32 * policy.emergency_threshold),
            });
            InferencePolicyProjection {
                snapshot_id: measured.then(|| policy.snapshot_id.to_string()),
                route_key: Some(policy.capture.route.storage_key()),
                estimated_visible_input: input,
                assembly_budget: Some(policy.input_budget),
                estimated_percent: input
                    .map(|input| input as f32 / policy.input_budget as f32 * 100.0),
                effective_reasoning: policy.reasoning.label(),
                needs_resolution: None,
                details,
                last_provider_usage: usage,
            }
        }
        other => InferencePolicyProjection {
            effective_reasoning: "default/unknown".into(),
            needs_resolution: other.err(),
            last_provider_usage: usage,
            ..Default::default()
        },
    }
}

pub fn status(projection: &InferencePolicyProjection) -> String {
    let usage = match (
        projection.estimated_visible_input,
        projection.assembly_budget,
        projection.estimated_percent,
    ) {
        (Some(input), Some(budget), Some(percent)) => {
            format!("~{input}/{budget} visible input tokens ({percent:.1}% estimated)")
        }
        (_, Some(budget), _) => format!("usage unavailable; assembly budget {budget} tokens"),
        _ => "usage and budget unavailable".into(),
    };
    let error = projection.needs_resolution.as_deref().unwrap_or("resolved");
    format!(
        "Context: {usage}\nReasoning: {}\nPolicy: {error}\nNull capacity fields are unknown, not unlimited.\n\n```json\n{}\n```\n\n/context capacity <tokens|maximum|reset> sets a session route target. /context status explains its bounds and provenance.",
        projection.effective_reasoning,
        serde_json::to_string_pretty(projection).expect("projection serialization")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference_policy::*;

    fn snapshot() -> ResolvedPolicy {
        let mut capture = capture_route("openai-codex:gpt-6-astra", None, None, None, None);
        capture.intent.host_caps.push(("fixture".into(), 100_000));
        capture.intent.heuristic_generation = Some(0);
        let mut policy = capture
            .resolve(Some("minimal"), Default::default())
            .unwrap();
        policy
            .validate_input(VisibleInput {
                system: 10_000,
                schemas: 1_000,
                history_and_attachments: 39_000,
            })
            .unwrap();
        policy
    }

    #[test]
    fn resolved_policy_projection_has_one_denominator_including_mandatory_input() {
        let policy = snapshot();
        let projection = project(Ok(Some(&policy)), true, None);
        assert_eq!(projection.estimated_visible_input, Some(50_000));
        assert_eq!(projection.assembly_budget, Some(100_000));
        assert_eq!(projection.estimated_percent, Some(50.0));
        assert_eq!(projection.effective_reasoning, "low");
        assert_eq!(projection.snapshot_id, Some(policy.snapshot_id.to_string()));
        assert!(status(&projection).contains("872000"));
        assert!(status(&projection).contains("heuristic_generation"));
        let decoded: InferencePolicyProjection =
            serde_json::from_value(serde_json::to_value(&projection).unwrap()).unwrap();
        assert_eq!(decoded, projection);
    }

    #[test]
    fn resolved_policy_startup_and_old_usage_do_not_fabricate_current_percentage() {
        let policy = snapshot();
        let old = InferenceProviderUsage {
            measured_at: Some("2026-10-08T00:00:00Z".into()),
            snapshot_id: "request-A".into(),
            route_key: "route-A".into(),
            model: "old-model".into(),
            input_tokens: 70_000,
            output_tokens: 5_000,
            cache_read_tokens: 60_000,
            cache_creation_tokens: 0,
            semantics: "inclusive input".into(),
        };
        let startup = project(Ok(Some(&policy)), false, Some(old.clone()));
        assert_eq!(startup.estimated_visible_input, None);
        assert_eq!(startup.estimated_percent, None);
        assert!(status(&startup).contains("usage unavailable"));
        let current = project(Ok(Some(&policy)), true, Some(old.clone()));
        assert_eq!(current.estimated_percent, Some(50.0));
        assert_eq!(current.last_provider_usage, Some(old));
    }
}

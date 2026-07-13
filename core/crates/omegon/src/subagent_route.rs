use crate::inference_inventory::{InventorySnapshot, OfferingId};
use crate::inference_runtime::normalize_route_id_for_resolution;
use crate::routing::CapabilityGradeBand;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerProfile {
    Scout,
    Patch,
    Verify,
    General,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentRouteSource {
    ExplicitPin,
    PlanDefault,
    Inventory,
    CompiledFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentRouteRequest<'a> {
    pub profile: WorkerProfile,
    pub explicit_model: Option<&'a str>,
    pub plan_default_model: Option<&'a str>,
    pub parent_model: &'a str,
    pub only_providers: &'a [String],
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubagentRouteDecision {
    pub selected_model: String,
    pub requested_grade: CapabilityGradeBand,
    pub parent_grade_ceiling: CapabilityGradeBand,
    pub inventory_generation: u64,
    pub source: SubagentRouteSource,
    pub fallback_reason: Option<String>,
}

pub fn resolve_subagent_route(
    request: &SubagentRouteRequest<'_>,
    snapshot: &InventorySnapshot,
    compiled_fallback: &str,
) -> SubagentRouteDecision {
    let requested_grade = profile_grade(request.profile);
    let parent_grade_ceiling = crate::routing::infer_model_grade_band(request.parent_model);
    if let Some(model) = request.explicit_model {
        return decision(
            model,
            requested_grade,
            parent_grade_ceiling,
            snapshot.generation,
            SubagentRouteSource::ExplicitPin,
            None,
        );
    }
    if let Some(model) = request.plan_default_model {
        return decision(
            model,
            requested_grade,
            parent_grade_ceiling,
            snapshot.generation,
            SubagentRouteSource::PlanDefault,
            None,
        );
    }
    // An unpinned child inherits the route that is already serving the parent.
    // Inventory compatibility describes capability, not live launch readiness;
    // selecting an arbitrary compatible offering here can route a child to an
    // installed-but-unavailable local model. Explicit pins and plan defaults
    // remain authoritative, while omission is the safe same-provider path.
    let inherited = normalize_route_id_for_resolution(request.parent_model);
    if provider_allowed(&OfferingId(inherited.clone()), request.only_providers) {
        return decision(
            &inherited,
            requested_grade,
            parent_grade_ceiling,
            snapshot.generation,
            SubagentRouteSource::CompiledFallback,
            Some("inherited parent route".into()),
        );
    }
    decision(
        compiled_fallback,
        requested_grade,
        parent_grade_ceiling,
        snapshot.generation,
        SubagentRouteSource::CompiledFallback,
        Some("parent provider excluded by route constraints".into()),
    )
}

fn decision(
    model: &str,
    requested_grade: CapabilityGradeBand,
    parent_grade_ceiling: CapabilityGradeBand,
    generation: u64,
    source: SubagentRouteSource,
    fallback_reason: Option<String>,
) -> SubagentRouteDecision {
    SubagentRouteDecision {
        selected_model: normalize_route_id_for_resolution(model),
        requested_grade,
        parent_grade_ceiling,
        inventory_generation: generation,
        source,
        fallback_reason,
    }
}

fn profile_grade(profile: WorkerProfile) -> CapabilityGradeBand {
    match profile {
        WorkerProfile::Scout | WorkerProfile::Verify => CapabilityGradeBand::Mid,
        WorkerProfile::Patch => CapabilityGradeBand::Frontier,
        WorkerProfile::General => CapabilityGradeBand::Frontier,
    }
}

fn provider_allowed(offering: &OfferingId, allowed: &[String]) -> bool {
    allowed.is_empty()
        || offering.0.split_once(':').is_some_and(|(provider, _)| {
            allowed
                .iter()
                .any(|item| item.eq_ignore_ascii_case(provider))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_pin_precedes_plan_and_inventory() {
        let snapshot = InventorySnapshot::empty();
        let allowed = Vec::new();
        let request = SubagentRouteRequest {
            profile: WorkerProfile::Patch,
            explicit_model: Some("openai:gpt-5.6"),
            plan_default_model: Some("anthropic:claude"),
            parent_model: "openai:gpt-5.6",
            only_providers: &allowed,
        };
        let decision = resolve_subagent_route(&request, &snapshot, "fallback:model");
        assert_eq!(decision.source, SubagentRouteSource::ExplicitPin);
        assert_eq!(decision.selected_model, "openai:gpt-5.6");
    }

    #[test]
    fn unpinned_child_inherits_parent_instead_of_inventory_candidate() {
        let snapshot = InventorySnapshot::empty();
        let allowed = Vec::new();
        let request = SubagentRouteRequest {
            profile: WorkerProfile::Scout,
            explicit_model: None,
            plan_default_model: None,
            parent_model: "openai:gpt-5.6",
            only_providers: &allowed,
        };
        let decision = resolve_subagent_route(&request, &snapshot, "ollama:qwen3:32b");
        assert_eq!(decision.source, SubagentRouteSource::CompiledFallback);
        assert_eq!(decision.selected_model, "openai:gpt-5.6");
        assert_eq!(
            decision.fallback_reason.as_deref(),
            Some("inherited parent route")
        );
    }

    #[test]
    fn excluded_parent_provider_uses_compiled_fallback() {
        let snapshot = InventorySnapshot::empty();
        let allowed = vec!["anthropic".to_string()];
        let request = SubagentRouteRequest {
            profile: WorkerProfile::Scout,
            explicit_model: None,
            plan_default_model: None,
            parent_model: "openai:gpt-5.6",
            only_providers: &allowed,
        };
        let decision = resolve_subagent_route(&request, &snapshot, "anthropic:claude-sonnet-4-6");
        assert_eq!(decision.selected_model, "anthropic:claude-sonnet-4-6");
        assert_eq!(
            decision.fallback_reason.as_deref(),
            Some("parent provider excluded by route constraints")
        );
    }
}

//! Pure resource/reasoning resolution for the existing request gateway.
//! No credentials, request content, discovery I/O, or session authority live here.

#[cfg(test)]
#[path = "inference_policy_tests.rs"]
mod tests;

use crate::inference_inventory::InventorySource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteKey {
    pub provider: String,
    pub connection: String,
    pub authentication: String,
    pub endpoint: String,
    pub native_model: String,
}

impl RouteKey {
    /// Versioned, deterministic identity. Endpoint values must already be opaque.
    pub fn storage_key(&self) -> String {
        use sha2::{Digest, Sha256};
        let encoded = serde_json::to_vec(self).expect("route key serialization");
        format!("route-v1:{:x}", Sha256::digest(encoded))
    }
}

/// Embedded templates apply only to the named native integration. Manifest
/// endpoints supply their own facts and never borrow these by model basename.
pub fn registry_facts(model: &str) -> RouteFacts {
    use crate::model_registry::ModelRegistry;
    let registry = ModelRegistry::global();
    let Some(entry) = registry.model_info(model) else {
        return RouteFacts::default();
    };
    let provenance = Provenance {
        source: InventorySource::Embedded,
        authority: format!("registry:{model}"),
        uri: None,
        revision: None,
        sequence: None,
        reviewed_at: None,
        observed_at: None,
        valid_until: None,
        stale: false,
        status: EvidenceStatus::Assumed,
    };
    let mut facts = RouteFacts::default();
    if let Some(policy) = &entry.inference_policy {
        let provenance = Provenance {
            uri: Some(policy.source.clone()),
            revision: policy.revision.clone(),
            reviewed_at: Some(policy.reviewed_at.clone()),
            status: EvidenceStatus::Declared,
            ..provenance
        };
        for (field, tokens, basis) in [
            (
                CapacityField::DefaultWindow,
                policy.default_window,
                policy.basis,
            ),
            (
                CapacityField::MaximumWindow,
                policy.maximum_window,
                policy.basis,
            ),
            (
                CapacityField::MaximumInput,
                policy.maximum_input,
                WindowBasis::Input,
            ),
            (
                CapacityField::MaximumTotal,
                policy.maximum_total,
                WindowBasis::Total,
            ),
            (
                CapacityField::MaximumOutput,
                policy.maximum_output,
                WindowBasis::Unspecified,
            ),
        ] {
            if let Some(tokens) = tokens {
                let mut origin = provenance.clone();
                if field == CapacityField::DefaultWindow && entry.provider == "openai" {
                    origin.status = EvidenceStatus::Assumed;
                    origin.authority = "host conservative Astra working default".into();
                    origin.uri = None;
                }
                facts.capacity.push(CapacityFact {
                    field,
                    tokens,
                    basis,
                    provenance: origin,
                });
            }
        }
        facts.reasoning = Some(ReasoningCapabilities {
            disabled: policy.disabled,
            efforts: policy.efforts.clone(),
            token_bounds: None,
            adaptive: false,
            default: policy
                .default_effort
                .clone()
                .map(ReasoningValue::Categorical),
            minimal_to_low: true,
            provenance,
        });
    } else {
        // Legacy numbers have no reviewed input/total meaning. Retain a labeled
        // conservative working fallback, never claim an independent vendor bound.
        facts.capacity.push(CapacityFact {
            field: CapacityField::DefaultWindow,
            tokens: entry.context_input.min(200_000),
            basis: WindowBasis::Unspecified,
            provenance: provenance.clone(),
        });
        for (field, tokens) in [
            (CapacityField::MaximumWindow, entry.context_input),
            (CapacityField::MaximumOutput, entry.context_output),
        ] {
            facts.capacity.push(CapacityFact {
                field,
                tokens,
                basis: WindowBasis::Unspecified,
                provenance: provenance.clone(),
            });
        }
        let caps = match entry.provider.as_str() {
            "anthropic" => Some(ReasoningCapabilities {
                disabled: !matches!(entry.id.as_str(), "claude-fable-5-1" | "claude-mythos-5-1"),
                efforts: ["minimal", "low", "medium", "high", "xhigh", "max"]
                    .map(str::to_owned)
                    .to_vec(),
                token_bounds: if matches!(
                    entry.id.as_str(),
                    "claude-fable-5-1" | "claude-mythos-5-1"
                ) {
                    None
                } else {
                    Some((1024, 50_000))
                },
                adaptive: matches!(
                    entry.id.as_str(),
                    "claude-sonnet-4-6"
                        | "claude-sonnet-5"
                        | "claude-opus-4-6"
                        | "claude-opus-4-7"
                        | "claude-opus-4-8"
                        | "claude-opus-5"
                        | "claude-fable-5-1"
                        | "claude-mythos-5-1"
                ),
                default: None,
                minimal_to_low: false,
                provenance: provenance.clone(),
            }),
            "openai" | "openai-codex" => Some(ReasoningCapabilities {
                disabled: true,
                efforts: ["low", "medium", "high", "xhigh"]
                    .map(str::to_owned)
                    .to_vec(),
                token_bounds: None,
                adaptive: false,
                default: None,
                minimal_to_low: true,
                provenance: provenance.clone(),
            }),
            "ollama" | "ollama-cloud" => Some(ReasoningCapabilities {
                disabled: true,
                efforts: ["minimal", "low", "medium", "high", "xhigh", "max"]
                    .map(str::to_owned)
                    .to_vec(),
                token_bounds: None,
                adaptive: false,
                default: None,
                minimal_to_low: false,
                provenance: provenance.clone(),
            }),
            _ => None,
        };
        facts.reasoning = caps;
    }
    facts
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowBasis {
    Input,
    Total,
    #[default]
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Declared,
    Discovered,
    Configured,
    Assumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: InventorySource,
    pub authority: String,
    pub uri: Option<String>,
    pub revision: Option<String>,
    /// Only a publisher-issued sequence establishes ordering within an authority.
    pub sequence: Option<u64>,
    pub reviewed_at: Option<String>,
    #[serde(default)]
    pub observed_at: Option<u64>,
    #[serde(default)]
    pub valid_until: Option<u64>,
    pub stale: bool,
    pub status: EvidenceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityField {
    DefaultWindow,
    MaximumWindow,
    MaximumInput,
    MaximumTotal,
    MaximumOutput,
    TransportTotal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityFact {
    pub field: CapacityField,
    pub tokens: usize,
    pub basis: WindowBasis,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteFacts {
    #[serde(default)]
    pub inventory_generation: Option<u64>,
    pub capacity: Vec<CapacityFact>,
    pub reasoning: Option<ReasoningCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningCapabilities {
    pub disabled: bool,
    pub efforts: Vec<String>,
    pub token_bounds: Option<(usize, usize)>,
    pub adaptive: bool,
    pub default: Option<ReasoningValue>,
    pub minimal_to_low: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReasoningValue {
    Disabled,
    Enabled,
    Categorical(String),
    TokenBudget(usize),
    Adaptive(BTreeMap<String, String>),
    ProviderDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedReasoning {
    pub requested: Option<String>,
    pub effective: ReasoningValue,
    pub normalization: Option<String>,
    pub heuristic_tokens: usize,
}

impl ResolvedReasoning {
    pub fn label(&self) -> String {
        match &self.effective {
            ReasoningValue::Disabled => "off".into(),
            ReasoningValue::Enabled => "enabled".into(),
            ReasoningValue::Categorical(effort) => effort.clone(),
            ReasoningValue::TokenBudget(tokens) => format!("budget:{tokens}"),
            ReasoningValue::Adaptive(parameters) => parameters
                .get("effort")
                .map_or_else(|| "adaptive".into(), |effort| format!("adaptive/{effort}")),
            ReasoningValue::ProviderDefault => "default/unknown".into(),
        }
    }
}

pub fn resolve_reasoning(
    requested: Option<&str>,
    caps: Option<&ReasoningCapabilities>,
) -> anyhow::Result<ResolvedReasoning> {
    let heuristic_tokens = requested
        .and_then(crate::settings::ThinkingLevel::parse)
        .and_then(|level| level.budget_tokens())
        .unwrap_or(0) as usize;
    let mut result = ResolvedReasoning {
        requested: requested.map(str::to_owned),
        effective: ReasoningValue::ProviderDefault,
        normalization: None,
        heuristic_tokens,
    };
    let Some(request) = requested.filter(|value| *value != "provider-default") else {
        if let Some(default) = caps.and_then(|caps| caps.default.clone()) {
            result.effective = default;
            result.normalization = Some("advertised provider default (not measured)".into());
        }
        return Ok(result);
    };
    let raw_request = request;
    let request =
        crate::settings::ThinkingLevel::parse(request).map_or(request, |level| level.as_str());
    anyhow::ensure!(
        request != "ultra",
        "reasoning needs resolution: ultra is orchestration metadata, not a supported effort; choose a supported reasoning mode"
    );
    let Some(caps) = caps else {
        // Portable positive levels retain SDK compatibility on unknown routes.
        // Their concrete wire behavior is resolved by the adapter below. Explicit
        // Off cannot use this rule: omission is not evidence of disabling.
        if crate::settings::ThinkingLevel::parse(request)
            .is_some_and(|level| level != crate::settings::ThinkingLevel::Off)
        {
            result.normalization = Some("portable reasoning has no reviewed route control; effective provider default remains unknown".into());
            return Ok(result);
        }
        anyhow::bail!(
            "reasoning needs resolution: capabilities unknown; select provider-default instead of {request}"
        );
    };
    if matches!(request, "off" | "none") && caps.disabled {
        result.effective = ReasoningValue::Disabled;
    } else if let Some(tokens) = request.strip_prefix("budget:") {
        let tokens: usize = tokens
            .parse()
            .map_err(|_| anyhow::anyhow!("reasoning budget must be a positive integer"))?;
        let (min, max) = caps
            .token_bounds
            .ok_or_else(|| anyhow::anyhow!("route does not support token-budget reasoning"))?;
        anyhow::ensure!(
            (min..=max).contains(&tokens),
            "reasoning budget {tokens} outside {min}..={max}; choose a supported budget"
        );
        result.effective = ReasoningValue::TokenBudget(tokens);
    } else if request == "adaptive" && caps.adaptive {
        result.effective = ReasoningValue::Adaptive(BTreeMap::new());
    } else if request == "minimal" && caps.minimal_to_low {
        result.effective = ReasoningValue::Categorical("low".into());
        result.normalization = Some("portable Minimal maps to this route's low effort; 2000 is a host heuristic, not a wire cap".into());
    } else if caps.efforts.iter().any(|effort| effort == request) {
        result.effective = ReasoningValue::Categorical(request.into());
    } else {
        anyhow::bail!(
            "reasoning needs resolution: {request} is unsupported; choose provider-default or {:?}{}; saved intent is unchanged",
            caps.efforts,
            if caps.disabled { " or off" } else { "" }
        );
    }
    if raw_request != request {
        result.normalization = Some(format!(
            "legacy alias {raw_request} retains {request} intent; {}",
            result.normalization.unwrap_or_default()
        ));
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawNumericTarget", into = "RawNumericTarget")]
pub struct NumericTarget {
    pub route: RouteKey,
    pub tokens: usize,
    pub maximum_revision: Option<String>,
    pub invalid_tokens: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct RawNumericTarget {
    route: RouteKey,
    tokens: serde_json::Value,
    #[serde(default)]
    maximum_revision: Option<String>,
}

impl From<RawNumericTarget> for NumericTarget {
    fn from(raw: RawNumericTarget) -> Self {
        let tokens = raw
            .tokens
            .as_u64()
            .and_then(|tokens| usize::try_from(tokens).ok());
        Self {
            route: raw.route,
            tokens: tokens.unwrap_or(0),
            maximum_revision: raw.maximum_revision,
            invalid_tokens: tokens.is_none().then_some(raw.tokens),
        }
    }
}

impl From<NumericTarget> for RawNumericTarget {
    fn from(target: NumericTarget) -> Self {
        Self {
            route: target.route,
            tokens: target
                .invalid_tokens
                .unwrap_or_else(|| serde_json::json!(target.tokens)),
            maximum_revision: target.maximum_revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSource {
    Request,
    Session,
    Project,
    User,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PolicyIntent {
    pub targets: Vec<(TargetSource, NumericTarget)>,
    pub host_caps: Vec<(String, usize)>,
    pub heuristic_generation: Option<usize>,
    pub input_reserve: usize,
    pub proactive_threshold: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCapture {
    pub route: RouteKey,
    pub selected_model: String,
    pub contribution_generation: String,
    pub schema_dialect: String,
    pub native_adapter: bool,
    pub credential_source_class: String,
    pub inventory_generation: Option<u64>,
    pub credential_generation: Option<String>,
    pub facts: RouteFacts,
    pub intent: PolicyIntent,
    pub fallback_window: usize,
    pub selection_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputAccounting {
    pub wire_field: Option<String>,
    pub requested_cap: Option<usize>,
    pub enforced_total: Option<usize>,
    pub coverage: String,
}

impl Default for OutputAccounting {
    fn default() -> Self {
        Self {
            wire_field: None,
            requested_cap: None,
            enforced_total: None,
            coverage: "NotEnforced: total generation coverage unknown".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VisibleInput {
    pub system: usize,
    pub schemas: usize,
    pub history_and_attachments: usize,
}

impl VisibleInput {
    pub fn total(&self) -> anyhow::Result<usize> {
        self.system
            .checked_add(self.schemas)
            .and_then(|n| n.checked_add(self.history_and_attachments))
            .ok_or_else(|| anyhow::anyhow!("visible input estimate overflow"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPolicy {
    pub snapshot_id: uuid::Uuid,
    pub capture: PolicyCapture,
    pub selected_facts: BTreeMap<CapacityField, CapacityFact>,
    pub configured_constraints: BTreeMap<CapacityField, CapacityFact>,
    pub superseded_facts: Vec<CapacityFact>,
    pub targets: Vec<(TargetSource, NumericTarget)>,
    pub working_window: usize,
    pub window_basis: WindowBasis,
    pub working_status: EvidenceStatus,
    pub reductions: Vec<(String, usize)>,
    pub reasoning: ResolvedReasoning,
    pub output: OutputAccounting,
    pub heuristic_generation: usize,
    pub generation_allocation: usize,
    pub input_reserve: usize,
    pub input_budget: usize,
    pub visible_input: Option<VisibleInput>,
    pub warning_threshold: f32,
    pub proactive_threshold: f32,
    pub emergency_threshold: f32,
    pub estimator: String,
}

pub fn select_capacity(
    facts: &[CapacityFact],
) -> anyhow::Result<(BTreeMap<CapacityField, CapacityFact>, Vec<CapacityFact>)> {
    let mut selected: BTreeMap<CapacityField, CapacityFact> = BTreeMap::new();
    let mut superseded = Vec::new();
    let fields: std::collections::BTreeSet<_> = facts.iter().map(|fact| fact.field).collect();
    for field in fields {
        let group: Vec<_> = facts
            .iter()
            .enumerate()
            .filter(|(_, fact)| fact.field == field)
            .collect();
        // Establish the complete top group before checking conflicts. Older
        // contradictory observations may both be superseded by one publisher
        // revision, regardless of their input order.
        let top: Vec<_> = group
            .iter()
            .copied()
            .filter(|(_, candidate)| {
                !group.iter().any(|(_, other)| {
                    let a = &candidate.provenance;
                    let b = &other.provenance;
                    b.source > a.source
                        || (b.source == a.source
                            && b.authority == a.authority
                            && matches!((a.sequence, b.sequence), (Some(a), Some(b)) if b > a))
                })
            })
            .collect();
        let (winner_index, winner) = top[0];
        for (_, fact) in &top[1..] {
            anyhow::ensure!(
                winner.tokens == fact.tokens && winner.basis == fact.basis,
                "conflicting {:?} facts: {:?} and {:?}; supply a publisher-ordered revision",
                field,
                winner.provenance,
                fact.provenance
            );
        }
        selected.insert(field, winner.clone());
        superseded.extend(
            group
                .into_iter()
                .filter(|(index, _)| *index != winner_index)
                .map(|(_, fact)| fact.clone()),
        );
    }
    for fact in selected.values() {
        anyhow::ensure!(
            fact.tokens > 0,
            "invalid zero {:?} from {:?}",
            fact.field,
            fact.provenance
        );
    }
    if let (Some(default), Some(maximum)) = (
        selected.get(&CapacityField::DefaultWindow),
        selected.get(&CapacityField::MaximumWindow),
    ) {
        anyhow::ensure!(
            default.provenance.status == EvidenceStatus::Assumed
                || (default.basis == maximum.basis && default.tokens <= maximum.tokens),
            "inconsistent default/maximum: {:?} versus {:?}",
            default,
            maximum
        );
    }
    if let Some(total) = selected.get(&CapacityField::MaximumTotal) {
        for field in [CapacityField::MaximumInput, CapacityField::MaximumOutput] {
            if let Some(part) = selected.get(&field) {
                anyhow::ensure!(
                    part.tokens <= total.tokens,
                    "inconsistent {:?} and total facts: {:?} versus {:?}",
                    field,
                    part,
                    total
                );
            }
        }
    }
    Ok((selected, superseded))
}

impl PolicyCapture {
    pub fn resolve(
        &self,
        requested: Option<&str>,
        output: OutputAccounting,
    ) -> anyhow::Result<ResolvedPolicy> {
        if let Some(error) = &self.selection_error {
            anyhow::bail!("{error}");
        }
        let (provider_facts, configured): (Vec<_>, Vec<_>) =
            self.facts.capacity.iter().cloned().partition(|fact| {
                matches!(
                    fact.provenance.status,
                    EvidenceStatus::Declared | EvidenceStatus::Discovered
                ) || (fact.provenance.status == EvidenceStatus::Assumed
                    && fact.field == CapacityField::DefaultWindow)
            });
        let (selected, mut superseded) = select_capacity(&provider_facts)?;
        let (configured, superseded_configured) = select_capacity(&configured)?;
        superseded.extend(superseded_configured);
        let get = |field| selected.get(&field).map(|fact| fact.tokens);
        let default = selected.get(&CapacityField::DefaultWindow);
        let maximum = selected.get(&CapacityField::MaximumWindow);
        let basis = default
            .filter(|fact| fact.provenance.status != EvidenceStatus::Assumed)
            .or(maximum)
            .or(default)
            .map_or_else(
                || {
                    if selected.contains_key(&CapacityField::MaximumInput) {
                        WindowBasis::Input
                    } else {
                        WindowBasis::Unspecified
                    }
                },
                |fact| fact.basis,
            );
        let mut targets: Vec<_> = self
            .intent
            .targets
            .iter()
            .filter(|(_, target)| target.route == self.route)
            .cloned()
            .collect();
        targets.sort_by_key(|(source, _)| *source);
        let (mut window, status) = if let Some((source, target)) = targets.first() {
            anyhow::ensure!(
                target.tokens > 0,
                "{source:?} route target must be positive; use /context capacity <tokens> or reset"
            );
            if let Some(maximum) = maximum {
                anyhow::ensure!(
                    target.tokens <= maximum.tokens,
                    "{source:?} route target {} exceeds maximum {} from {:?}; reduce /context capacity",
                    target.tokens,
                    maximum.tokens,
                    maximum.provenance
                );
            }
            let comparable = match basis {
                WindowBasis::Input => Some(CapacityField::MaximumInput),
                WindowBasis::Total => Some(CapacityField::MaximumTotal),
                WindowBasis::Unspecified => None,
            };
            if let Some(bound) = comparable.and_then(|field| selected.get(&field)) {
                anyhow::ensure!(
                    target.tokens <= bound.tokens,
                    "{source:?} route target {} exceeds {:?} {} from {:?}; reduce /context capacity",
                    target.tokens,
                    bound.field,
                    bound.tokens,
                    bound.provenance
                );
            }
            (target.tokens, EvidenceStatus::Configured)
        } else {
            default.map_or((self.fallback_window, EvidenceStatus::Assumed), |fact| {
                (fact.tokens, fact.provenance.status)
            })
        };
        anyhow::ensure!(window > 0, "working window must be positive");
        let mut reductions = Vec::new();
        if targets.is_empty()
            && let Some(maximum) = maximum
            && window > maximum.tokens
        {
            window = maximum.tokens;
            reductions.push((
                "assumed fallback bounded by reviewed route maximum".into(),
                window,
            ));
        }
        for field in [CapacityField::DefaultWindow, CapacityField::MaximumWindow] {
            if let Some(fact) = configured.get(&field)
                && fact.tokens < window
            {
                window = fact.tokens;
                reductions.push((
                    format!("configured {:?} from {}", field, fact.provenance.authority),
                    window,
                ));
            }
        }
        for (name, cap) in &self.intent.host_caps {
            anyhow::ensure!(*cap > 0, "host cap {name} must be positive");
            if *cap < window {
                window = *cap;
                reductions.push((name.clone(), *cap));
            }
        }
        let reasoning = resolve_reasoning(requested, self.facts.reasoning.as_ref())?;
        let heuristic = self.intent.heuristic_generation.unwrap_or(
            8192usize
                .checked_add(reasoning.heuristic_tokens)
                .ok_or_else(|| anyhow::anyhow!("generation reserve overflow"))?,
        );
        if let Some(cap) = output.requested_cap {
            anyhow::ensure!(cap > 0, "output cap must be positive");
            if let Some(maximum) = get(CapacityField::MaximumOutput) {
                anyhow::ensure!(
                    cap <= maximum,
                    "output cap {cap} exceeds known output maximum {maximum}"
                );
            }
            if let Some(bound) = configured.get(&CapacityField::MaximumOutput) {
                anyhow::ensure!(
                    cap <= bound.tokens,
                    "output cap {cap} exceeds configured output constraint {}",
                    bound.tokens
                );
            }
        }
        let generation = heuristic.max(output.enforced_total.unwrap_or(0));
        let subtract = |window: usize| {
            window.checked_sub(generation).ok_or_else(|| {
                anyhow::anyhow!(
                    "context budget exhausted: window {window}, generation reserve {generation}"
                )
            })
        };
        let mut budget = if basis == WindowBasis::Input {
            window
        } else {
            subtract(window)?
        };
        if let Some(input) = get(CapacityField::MaximumInput) {
            budget = budget.min(input);
        }
        for field in [CapacityField::MaximumTotal, CapacityField::TransportTotal] {
            if let Some(total) = get(field) {
                budget = budget.min(subtract(total)?);
            }
        }
        if let Some(input) = configured.get(&CapacityField::MaximumInput) {
            budget = budget.min(input.tokens);
        }
        for field in [CapacityField::MaximumTotal, CapacityField::TransportTotal] {
            if let Some(total) = configured.get(&field) {
                budget = budget.min(subtract(total.tokens)?);
            }
        }
        budget = budget
            .checked_sub(self.intent.input_reserve)
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow::anyhow!("context budget exhausted by input reserve"))?;
        let proactive = self.intent.proactive_threshold.unwrap_or(0.75);
        anyhow::ensure!(
            proactive.is_finite() && proactive > 0.0 && proactive <= 1.0,
            "proactive compaction threshold must be in (0,1]"
        );
        Ok(ResolvedPolicy {
            snapshot_id: uuid::Uuid::new_v4(),
            capture: self.clone(),
            selected_facts: selected,
            configured_constraints: configured,
            superseded_facts: superseded,
            targets,
            working_window: window,
            window_basis: basis,
            working_status: status,
            reductions,
            reasoning,
            output,
            heuristic_generation: heuristic,
            generation_allocation: generation,
            input_reserve: self.intent.input_reserve,
            input_budget: budget,
            visible_input: None,
            warning_threshold: 0.70,
            proactive_threshold: proactive,
            emergency_threshold: 1.0,
            estimator: "local chars/4 estimate; cache counted once; hidden server prefixes unknown"
                .into(),
        })
    }

    pub fn maximum_target(&self) -> anyhow::Result<NumericTarget> {
        let declared: Vec<_> = self
            .facts
            .capacity
            .iter()
            .filter(|fact| {
                matches!(
                    fact.provenance.status,
                    EvidenceStatus::Declared | EvidenceStatus::Discovered
                )
            })
            .cloned()
            .collect();
        let (selected, _) = select_capacity(&declared)?;
        let maximum = selected.get(&CapacityField::MaximumWindow).ok_or_else(|| {
            anyhow::anyhow!("route maximum unknown; provide an explicit numeric token target")
        })?;
        Ok(NumericTarget {
            route: self.route.clone(),
            tokens: maximum.tokens,
            maximum_revision: maximum.provenance.revision.clone(),
            invalid_tokens: None,
        })
    }
}

impl ResolvedPolicy {
    pub fn validate_input(&mut self, input: VisibleInput) -> anyhow::Result<()> {
        let total = input.total()?;
        anyhow::ensure!(
            total <= self.input_budget,
            "context length exceeded: estimated visible input {total} (system {}, schemas {}, history/attachments {}) > assembly budget {}; compact history or reduce input; mandatory input was not truncated",
            input.system,
            input.schemas,
            input.history_and_attachments,
            self.input_budget
        );
        self.visible_input = Some(input);
        Ok(())
    }
}

pub fn capture_route(
    model: &str,
    selected: Option<&str>,
    native: Option<&str>,
    authentication: Option<&str>,
    endpoint: Option<&crate::bridge::EndpointRouteProvenance>,
) -> PolicyCapture {
    let provider = endpoint.map_or_else(
        || {
            model.split_once(':').map_or_else(
                || crate::providers::infer_provider_id(model),
                |(provider, _)| provider.to_owned(),
            )
        },
        |e| e.selected_provider_id.clone(),
    );
    let native = native.unwrap_or(model);
    let native_model = native
        .strip_prefix(&format!("{provider}:"))
        .unwrap_or(native)
        .to_owned();
    let native_model = if endpoint.is_none() && provider == "openai-codex" {
        crate::providers::codex_wire_model(&native_model).to_owned()
    } else {
        native_model
    };
    let connection = endpoint.map_or_else(
        || {
            format!(
                "legacy-managed-slot:{provider}:{}",
                authentication.unwrap_or("unclassified")
            )
        },
        |e| {
            e.connection_id
                .clone()
                .unwrap_or_else(|| e.endpoint_id.clone())
        },
    );
    let route = RouteKey {
        provider: provider.clone(),
        connection: connection.clone(),
        authentication: match authentication {
            Some(source) if source.contains("oauth") => "oauth".into(),
            Some(source)
                if source.contains("api_key")
                    || source == "declared_bearer_secret"
                    || source == "secrets_manager" =>
            {
                "api_key".into()
            }
            other => other.unwrap_or("unclassified").into(),
        },
        endpoint: connection,
        native_model,
    };
    PolicyCapture {
        route,
        selected_model: selected.unwrap_or(model).into(),
        native_adapter: endpoint.is_none(),
        schema_dialect: endpoint.map_or_else(
            || {
                crate::provider_contributions::registry()
                    .get(&provider)
                    .map_or_else(
                        || "unknown".into(),
                        |contribution| contribution.tools.dialect_name().into(),
                    )
            },
            |endpoint| endpoint.schema_dialect.clone(),
        ),
        credential_source_class: authentication.unwrap_or("unclassified").into(),
        contribution_generation: endpoint.map_or_else(
            || {
                crate::provider_contributions::registry()
                    .get(&provider)
                    .map_or_else(
                        || "unknown".into(),
                        |c| c.owner_generation_id.as_str().into(),
                    )
            },
            |e| e.contribution_generation_id.clone(),
        ),
        inventory_generation: endpoint.map(|e| e.inventory_generation),
        credential_generation: None,
        facts: if endpoint.is_none() {
            registry_facts(model)
        } else {
            RouteFacts::default()
        },
        intent: PolicyIntent::default(),
        fallback_window: 131_072,
        selection_error: None,
    }
}

pub fn opaque_identity(value: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

pub fn bind_transport(
    capture: &mut PolicyCapture,
    transport: Option<(String, bool)>,
    connection: Option<String>,
) {
    if let Some((identity, native)) = transport {
        capture.route.endpoint = identity;
        if !native {
            // Enumeration currently runs against the registry's native URLs.
            // Custom endpoints cannot inherit either native templates or those
            // observations merely because the logical provider id is shared.
            capture.facts.capacity.retain(|fact| {
                !matches!(
                    fact.provenance.source,
                    InventorySource::Embedded | InventorySource::Discovery
                )
            });
            if capture.facts.reasoning.as_ref().is_some_and(|caps| {
                matches!(
                    caps.provenance.source,
                    InventorySource::Embedded | InventorySource::Discovery
                )
            }) {
                capture.facts.reasoning = None;
            }
        }
    }
    if let Some(connection) = connection {
        capture.route.connection = connection;
    }
}

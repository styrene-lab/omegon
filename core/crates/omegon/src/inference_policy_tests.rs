use super::*;

fn codex() -> PolicyCapture {
    capture_route("openai-codex:gpt-6-astra", None, None, Some("oauth"), None)
}

fn target(capture: &PolicyCapture, tokens: usize) -> NumericTarget {
    NumericTarget {
        route: capture.route.clone(),
        tokens,
        maximum_revision: None,
        invalid_tokens: None,
    }
}

#[test]
fn resolved_policy_api_codex_isolation_and_legacy_fallback() {
    let codex = codex().resolve(None, Default::default()).unwrap();
    assert_eq!(codex.working_window, 272_000);
    assert_eq!(
        codex.selected_facts[&CapacityField::MaximumWindow].tokens,
        872_000
    );
    assert!(
        !codex
            .selected_facts
            .contains_key(&CapacityField::MaximumInput)
    );
    assert!(
        !codex
            .selected_facts
            .contains_key(&CapacityField::MaximumTotal)
    );
    let api = capture_route("openai:gpt-6-astra", None, None, None, None)
        .resolve(None, Default::default())
        .unwrap();
    assert_eq!(
        api.selected_facts[&CapacityField::MaximumInput].tokens,
        922_000
    );
    assert_eq!(
        api.selected_facts[&CapacityField::MaximumTotal].tokens,
        1_050_000
    );
    let unknown = capture_route("private:deployment", None, None, None, None)
        .resolve(None, Default::default())
        .unwrap();
    assert_eq!(unknown.working_status, EvidenceStatus::Assumed);
    assert!(unknown.selected_facts.is_empty());
    assert_eq!(unknown.reasoning.label(), "default/unknown");
}

#[test]
fn resolved_policy_target_precedence_invalid_winner_and_narrowing() {
    let mut capture = codex();
    capture.intent.targets = vec![
        (TargetSource::User, target(&capture, 500_000)),
        (TargetSource::Project, target(&capture, 600_000)),
        (TargetSource::Request, target(&capture, 300_000)),
    ];
    let policy = capture.resolve(None, Default::default()).unwrap();
    assert_eq!(policy.working_window, 300_000);
    assert_eq!(policy.targets.len(), 3);
    capture.intent.targets[2].1.tokens = 900_000;
    assert!(
        capture
            .resolve(None, Default::default())
            .unwrap_err()
            .to_string()
            .contains("872000")
    );
    capture.intent.targets.pop();
    capture.intent.host_caps.push(("posture".into(), 400_000));
    let policy = capture.resolve(None, Default::default()).unwrap();
    assert_eq!(policy.working_window, 400_000);
    assert_eq!(policy.targets[0].1.tokens, 600_000);
    capture.route.connection = "another connection".into();
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_window,
        272_000
    );
}

#[test]
fn resolved_policy_numeric_maximum_does_not_follow_refresh() {
    let mut capture = codex();
    let maximum = capture.maximum_target().unwrap();
    assert_eq!(maximum.tokens, 872_000);
    assert!(maximum.maximum_revision.is_some());
    capture
        .intent
        .targets
        .push((TargetSource::Session, maximum));
    capture.inventory_generation = Some(2);
    capture
        .facts
        .capacity
        .iter_mut()
        .find(|fact| fact.field == CapacityField::MaximumWindow)
        .unwrap()
        .tokens = 900_000;
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_window,
        872_000
    );
    capture.facts.capacity.clear();
    assert!(capture.maximum_target().is_err());
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_status,
        EvidenceStatus::Configured
    );
    capture.intent.targets[0].1.tokens = 0;
    assert!(capture.resolve(None, Default::default()).is_err());
}

#[test]
fn resolved_policy_discovery_supersedes_stale_and_unordered_conflicts_reject() {
    let mut capture = codex();
    let original = capture.facts.capacity[0].clone();
    let mut discovery = original.clone();
    discovery.tokens = 250_000;
    discovery.provenance.source = InventorySource::Discovery;
    discovery.provenance.status = EvidenceStatus::Discovered;
    discovery.provenance.revision = Some("publisher-7".into());
    discovery.provenance.sequence = Some(7);
    discovery.provenance.stale = true;
    capture.facts.capacity.push(discovery.clone());
    let policy = capture.resolve(None, Default::default()).unwrap();
    assert_eq!(policy.working_window, 250_000);
    assert!(
        policy.selected_facts[&CapacityField::DefaultWindow]
            .provenance
            .stale
    );
    assert!(policy.superseded_facts.contains(&original));
    discovery.tokens = 240_000;
    discovery.provenance.sequence = Some(8);
    capture.facts.capacity.push(discovery.clone());
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_window,
        240_000
    );
    discovery.tokens = 230_000;
    discovery.provenance.reviewed_at = Some("2026-10-09".into());
    capture.facts.capacity.push(discovery);
    assert!(
        capture
            .resolve(None, Default::default())
            .unwrap_err()
            .to_string()
            .contains("conflicting")
    );
}

#[test]
fn resolved_policy_exhaustion_and_capacity_semantics() {
    let mut capture = codex();
    capture
        .facts
        .capacity
        .retain(|fact| fact.field == CapacityField::DefaultWindow);
    capture.facts.capacity[0].tokens = 100_000;
    capture.intent.heuristic_generation = Some(8_000);
    let output = OutputAccounting {
        wire_field: Some("fixture total".into()),
        requested_cap: Some(20_000),
        enforced_total: Some(20_000),
        coverage: "total inclusive".into(),
    };
    for (basis, budget) in [
        (WindowBasis::Input, 100_000),
        (WindowBasis::Total, 80_000),
        (WindowBasis::Unspecified, 80_000),
    ] {
        capture.facts.capacity[0].basis = basis;
        assert_eq!(
            capture.resolve(None, output.clone()).unwrap().input_budget,
            budget
        );
    }
    capture.intent.heuristic_generation = Some(24_000);
    let policy = capture.resolve(None, output).unwrap();
    assert_eq!(policy.input_budget, 76_000);
    assert_eq!(policy.output.enforced_total, Some(20_000));
    let partial = capture
        .resolve(
            None,
            OutputAccounting {
                requested_cap: Some(20_000),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(partial.input_budget, 76_000);
    assert_eq!(partial.output.enforced_total, None);
    capture.intent.input_reserve = 76_000;
    assert!(capture.resolve(None, Default::default()).is_err());
    capture.intent.input_reserve = 0;
    capture.intent.heuristic_generation = Some(usize::MAX);
    assert!(capture.resolve(None, Default::default()).is_err());
}

#[test]
fn resolved_policy_stable_key_generation_and_legacy_nonconsent() {
    let mut capture = codex();
    let key = capture.route.storage_key();
    capture
        .intent
        .targets
        .push((TargetSource::User, target(&capture, 600_000)));
    capture.inventory_generation = Some(40);
    assert_eq!(capture.route.storage_key(), key);
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_window,
        600_000
    );
    capture.route.endpoint = "changed endpoint".into();
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_window,
        272_000
    );
    let mut settings = crate::settings::Settings::new("openai-codex:gpt-6-astra");
    settings.context_window = 1_000_000;
    settings.requested_context_class = Some(crate::settings::ContextClass::Massive);
    capture.intent = settings.inference_intent();
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_window,
        272_000
    );
}

#[test]
fn resolved_policy_named_configured_threshold_and_schema_reserve_replacement() {
    let mut settings = crate::settings::Settings::new("openai-codex:gpt-6-astra");
    settings.compaction_threshold = 0.60;
    settings.set_thinking(crate::settings::ThinkingLevel::Minimal);
    let mut capture = codex();
    capture.intent = settings.inference_intent();
    let policy = capture
        .resolve(Some("minimal"), Default::default())
        .unwrap();
    assert_eq!(policy.proactive_threshold, 0.60);
    assert_eq!(policy.warning_threshold, 0.70);
    assert_eq!(policy.emergency_threshold, 1.0);
    assert_eq!(policy.input_reserve, 0);
    assert_eq!(policy.heuristic_generation, 10_192);
    assert_eq!(policy.reasoning.label(), "low");
    assert_eq!(policy.reasoning.heuristic_tokens, 2_000);
    assert!(policy.reasoning.normalization.is_some());
}

#[test]
fn resolved_policy_inventory_merge_preserves_scope_and_metadata_generation() {
    use crate::inference_inventory::*;
    let embedded =
        InventoryLayer::embedded_registry(crate::model_registry::ModelRegistry::global());
    let id = OfferingId("openai-codex:gpt-6-astra".into());
    let mut discovery = InventoryLayer::new(InventorySource::Discovery, EvidenceKind::Discovered);
    let mut facts = registry_facts(&id.0);
    facts
        .capacity
        .retain(|fact| fact.field == CapacityField::DefaultWindow);
    facts.capacity[0].tokens = 250_000;
    facts.capacity[0].provenance.sequence = Some(2);
    facts.capacity[0].provenance.status = EvidenceStatus::Discovered;
    discovery.offerings.insert(
        id.clone(),
        OfferingPatch {
            policy_facts: Some(facts.clone()),
            ..Default::default()
        },
    );
    let snapshot = InventorySnapshot::build(42, vec![embedded.clone(), discovery.clone()]).unwrap();
    let offering = &snapshot.offerings[&id];
    assert_eq!(offering.policy_facts.inventory_generation, Some(42));
    let (selected, superseded) = select_capacity(&offering.policy_facts.capacity).unwrap();
    assert_eq!(selected[&CapacityField::DefaultWindow].tokens, 250_000);
    assert!(superseded.iter().any(|fact| fact.tokens == 272_000));
    discovery.offerings.get_mut(&id).unwrap().native_model_id = Some("foreign-deployment".into());
    let protected = InventorySnapshot::build(43, vec![embedded, discovery]).unwrap();
    assert_eq!(
        protected.offerings[&id].native_model_id.value,
        "gpt-6-astra"
    );
    assert_eq!(
        select_capacity(&protected.offerings[&id].policy_facts.capacity)
            .unwrap()
            .0[&CapacityField::DefaultWindow]
            .tokens,
        272_000
    );
}

#[test]
fn resolved_policy_configured_metadata_cannot_raise_declared_capacity() {
    let mut capture = codex();
    let mut configured = capture.facts.capacity[0].clone();
    configured.provenance.source = InventorySource::User;
    configured.provenance.status = EvidenceStatus::Configured;
    configured.tokens = 1_200_000;
    capture.facts.capacity.push(configured.clone());
    configured.field = CapacityField::MaximumWindow;
    capture.facts.capacity.push(configured);
    let resolved = capture.resolve(None, Default::default()).unwrap();
    assert_eq!(resolved.working_window, 272_000);
    assert_eq!(
        resolved.selected_facts[&CapacityField::MaximumWindow].tokens,
        872_000
    );
    assert_eq!(
        resolved.configured_constraints[&CapacityField::MaximumWindow].tokens,
        1_200_000
    );
    capture
        .intent
        .targets
        .push((TargetSource::Request, target(&capture, 900_000)));
    assert!(capture.resolve(None, Default::default()).is_err());
}

#[test]
fn resolved_policy_discovery_parser_preserves_meaning_expiry_and_old_cache_unknowns() {
    use crate::inference_discovery::*;
    use crate::inference_inventory::*;
    let models = parse_google(
        &serde_json::json!({"models":[{"name":"models/policy-fixture", "inputTokenLimit":100000, "outputTokenLimit":16000, "supportedGenerationMethods":["generateContent"]}]}),
    );
    let layer = build_discovery_layer(
        &[DiscoveredModels {
            endpoint_id: "google".into(),
            models,
            fetched_at: 1,
            ttl_secs: 1,
            cached: true,
        }],
        &Default::default(),
    );
    let snapshot = InventorySnapshot::build(
        72,
        vec![
            InventoryLayer::embedded_registry(crate::model_registry::ModelRegistry::global()),
            layer,
        ],
    )
    .unwrap();
    let mut capture = capture_route("google:policy-fixture", None, None, None, None);
    capture.facts = snapshot.offerings[&OfferingId("google:policy-fixture".into())]
        .policy_facts
        .clone();
    let policy = capture.resolve(None, Default::default()).unwrap();
    assert_eq!(policy.input_budget, 100_000);
    let input = &policy.selected_facts[&CapacityField::MaximumInput];
    assert_eq!(input.basis, WindowBasis::Input);
    assert!(input.provenance.authority.ends_with("inputTokenLimit"));
    assert_eq!(input.provenance.observed_at, Some(1));
    assert_eq!(input.provenance.valid_until, Some(2));
    assert!(input.provenance.stale);
    assert!(
        !policy
            .selected_facts
            .contains_key(&CapacityField::MaximumTotal)
    );
    assert!(
        !policy
            .selected_facts
            .contains_key(&CapacityField::DefaultWindow)
    );
    let old: DiscoveredModel = serde_json::from_value(serde_json::json!({"id":"old", "context_input":100000, "capabilities":{}, "non_chat":false})).unwrap();
    assert!(old.policy_capacity.is_empty());
}

#[test]
fn resolved_policy_custom_endpoint_does_not_inherit_native_discovery() {
    let mut capture = capture_route("openai:gpt-6-astra", None, None, None, None);
    let mut observed = capture.facts.capacity[0].clone();
    observed.provenance.source = InventorySource::Discovery;
    observed.provenance.status = EvidenceStatus::Discovered;
    capture.facts.capacity.push(observed);
    bind_transport(
        &mut capture,
        Some(("private-endpoint-fingerprint".into(), false)),
        None,
    );
    assert!(capture.facts.capacity.is_empty());
    assert!(capture.facts.reasoning.is_none());
    assert_eq!(
        capture
            .resolve(None, Default::default())
            .unwrap()
            .working_status,
        EvidenceStatus::Assumed
    );
}

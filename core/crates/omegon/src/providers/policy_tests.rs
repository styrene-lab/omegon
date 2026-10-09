use super::*;
use crate::inference_policy::*;
use crate::model_request::{PreparedModelRequest, RequestInputs, RequestPolicy};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct WireBridge {
    bodies: Mutex<Vec<Value>>,
    calls: AtomicUsize,
}

#[async_trait]
impl LlmBridge for WireBridge {
    async fn stream(
        &self,
        system: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let capture = options.policy_capture.as_ref().unwrap();
        let body = build_responses_body(
            &capture.route.provider,
            &capture.route.native_model,
            system,
            messages,
            tools,
            options,
        )?;
        self.bodies.lock().unwrap().push(body);
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (_, receiver) = mpsc::channel(1);
        Ok(receiver)
    }
}

#[tokio::test]
async fn resolved_policy_prepared_minimal_matches_wire_and_retry_capture() {
    let bridge = WireBridge::default();
    let options = StreamOptions {
        model: Some("openai-codex:gpt-6-astra".into()),
        reasoning: Some("minimal".into()),
        ..Default::default()
    };
    let prepared = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "system",
            messages: &[],
            tools: &[],
            options: &options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok("receipt"),
    )
    .unwrap();
    let id = prepared.policy().snapshot_id;
    assert_eq!(prepared.policy().reasoning.label(), "low");
    assert_eq!(prepared.policy().reasoning.heuristic_tokens, 2000);
    assert_eq!(prepared.policy().output.enforced_total, None);
    drop(prepared.stream().await.unwrap());
    drop(prepared.stream().await.unwrap());
    assert_eq!(prepared.policy().snapshot_id, id);
    let bodies = bridge.bodies.lock().unwrap();
    assert_eq!(bodies[0], bodies[1]);
    assert_eq!(bodies[0]["reasoning"]["effort"], "low");
}

#[tokio::test]
async fn resolved_policy_supported_codex_off_uses_responses_effort_field() {
    let bridge = WireBridge::default();
    let options = StreamOptions {
        model: Some("openai-codex:gpt-5.5".into()),
        reasoning: Some("off".into()),
        ..Default::default()
    };
    let prepared = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "system",
            messages: &[],
            tools: &[],
            options: &options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        prepared.policy().reasoning.effective,
        ReasoningValue::Disabled
    );
    drop(prepared.stream().await.unwrap());
    let mut body = bridge.bodies.lock().unwrap()[0].clone();
    assert_eq!(body["reasoning"]["effort"], "none");
    let bound = StreamOptions {
        resolved_policy: Some(std::sync::Arc::new(prepared.policy().clone())),
        ..Default::default()
    };
    body["reasoning"]["effort"] = json!("low");
    body["reasoning_effort"] = json!("none");
    assert!(
        validate_policy_wire(&bound, &body).is_err(),
        "an irrelevant root field must not mask the actual Responses effort"
    );
}

#[tokio::test]
async fn resolved_policy_gpt_oss_off_rejects_but_boolean_routes_send_false() {
    #[derive(Default)]
    struct OllamaWire(Mutex<Vec<Value>>);
    #[async_trait]
    impl LlmBridge for OllamaWire {
        async fn stream(
            &self,
            _: &str,
            _: &[LlmMessage],
            _: &[ToolDefinition],
            options: &StreamOptions,
        ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
            let policy = options.resolved_policy.as_ref().unwrap();
            let model = &policy.capture.route.native_model;
            let mut body = json!({"model": model, "think": ollama_think_value(model, options.reasoning.as_deref())});
            if let Some(cap) = policy
                .configured_constraints
                .get(&CapacityField::TransportTotal)
            {
                body["options"] = json!({"num_ctx":cap.tokens});
            }
            validate_policy_wire(options, &body)?;
            self.0.lock().unwrap().push(body);
            let (_, receiver) = mpsc::channel(1);
            Ok(receiver)
        }
    }
    for provider in ["ollama", "ollama-cloud"] {
        let bridge = OllamaWire::default();
        let profile: crate::settings::Profile =
            serde_json::from_value(json!({"thinkingLevel":"off"})).unwrap();
        let stored = serde_json::to_value(&profile).unwrap();
        let options = StreamOptions {
            model: Some(format!("{provider}:gpt-oss:20b")),
            reasoning: profile.thinking_level.clone(),
            ..Default::default()
        };
        let result = PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "system",
                messages: &[],
                tools: &[],
                options: &options,
                policy: RequestPolicy::Turn,
            },
            None,
            |_| -> anyhow::Result<()> { panic!("unsupported GPT-OSS Off reached evidence") },
        );
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("needs resolution")
        );
        assert_eq!(serde_json::to_value(&profile).unwrap(), stored);
        assert!(bridge.0.lock().unwrap().is_empty());
        for (model, intent, expected) in [
            ("gpt-oss:20b", "minimal", json!("low")),
            ("qwen3:32b", "off", json!(false)),
            ("qwen3:32b", "minimal", json!(true)),
        ] {
            let options = StreamOptions {
                model: Some(format!("{provider}:{model}")),
                reasoning: Some(intent.into()),
                ..Default::default()
            };
            let prepared = PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: "system",
                    messages: &[],
                    tools: &[],
                    options: &options,
                    policy: RequestPolicy::Turn,
                },
                None,
                |_| Ok(()),
            )
            .unwrap();
            drop(prepared.stream().await.unwrap());
            assert_eq!(bridge.0.lock().unwrap().last().unwrap()["think"], expected);
        }
    }
}

#[test]
fn resolved_policy_invalid_intent_and_late_overrides_never_open_transport() {
    let bridge = WireBridge::default();
    for reasoning in ["off", "none", "ultra", "typo"] {
        let options = StreamOptions {
            model: Some("openai-codex:gpt-6-astra".into()),
            reasoning: Some(reasoning.into()),
            ..Default::default()
        };
        assert!(
            PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: "system",
                    messages: &[],
                    tools: &[],
                    options: &options,
                    policy: RequestPolicy::Turn
                },
                None,
                |_| -> anyhow::Result<()> { panic!("invalid intent reached receipt") }
            )
            .is_err()
        );
    }
    for key in [
        "model",
        "reasoning",
        "thinking",
        "input",
        "previous_response_id",
    ] {
        let options = StreamOptions {
            model: Some("openai:gpt-6-astra".into()),
            extra_body: [(key.into(), json!("contradiction"))].into(),
            ..Default::default()
        };
        assert!(
            PreparedModelRequest::prepare(
                &bridge,
                RequestInputs {
                    system: "",
                    messages: &[],
                    tools: &[],
                    options: &options,
                    policy: RequestPolicy::Turn
                },
                None,
                |_| -> anyhow::Result<()> { panic!("late override reached receipt") }
            )
            .is_err()
        );
    }
    assert_eq!(bridge.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolved_policy_output_enforcement_and_final_wire_guard() {
    for (provider, enforced) in [("openai", Some(20_000)), ("openai-codex", None)] {
        let bridge = WireBridge::default();
        let options = StreamOptions {
            model: Some(format!("{provider}:gpt-6-astra")),
            extra_body: [("max_output_tokens".into(), json!(20_000))].into(),
            ..Default::default()
        };
        let prepared = PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "system",
                messages: &[],
                tools: &[],
                options: &options,
                policy: RequestPolicy::Auxiliary {
                    max_bytes: Some(1024),
                },
            },
            None,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(prepared.policy().output.enforced_total, enforced);
        drop(prepared.stream().await.unwrap());
        let mut body = bridge.bodies.lock().unwrap()[0].clone();
        assert_eq!(
            body.get("max_output_tokens").and_then(Value::as_u64),
            enforced.map(|n| n as u64)
        );
        let bound = StreamOptions {
            resolved_policy: Some(std::sync::Arc::new(prepared.policy().clone())),
            ..Default::default()
        };
        body["reasoning"]["effort"] = json!("ultra");
        assert!(validate_policy_wire(&bound, &body).is_err());
        body["model"] = json!("foreign-model");
        assert!(validate_policy_wire(&bound, &body).is_err());
    }
}

#[test]
fn resolved_policy_history_overflow_repair_and_independent_auxiliary() {
    let bridge = WireBridge::default();
    let options = StreamOptions {
        model: Some("openai-codex:gpt-6-astra".into()),
        ..Default::default()
    };
    let huge = [LlmMessage::User {
        content: "x".repeat(1_100_000),
        images: vec![],
    }];
    assert!(
        PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "small fixed context",
                messages: &huge,
                tools: &[],
                options: &options,
                policy: RequestPolicy::Turn
            },
            None,
            |_| -> anyhow::Result<()> { panic!("oversized history reached receipt") }
        )
        .is_err()
    );
    let first = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "small fixed context",
            messages: &[],
            tools: &[],
            options: &options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    let repaired = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "small fixed context",
            messages: &[],
            tools: &[],
            options: &options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    assert_ne!(first.policy().snapshot_id, repaired.policy().snapshot_id);
    for policy in [
        RequestPolicy::Compaction,
        RequestPolicy::Auxiliary {
            max_bytes: Some(100),
        },
    ] {
        let own = StreamOptions {
            model: Some("private:auxiliary".into()),
            ..Default::default()
        };
        let prepared = PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "classify",
                messages: &[],
                tools: &[],
                options: &own,
                policy,
            },
            None,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(prepared.policy().capture.route.native_model, "auxiliary");
        assert!(prepared.policy().selected_facts.is_empty());
        assert_eq!(prepared.policy().reasoning.label(), "default/unknown");
    }
}

#[test]
fn resolved_policy_boolean_and_anthropic_modes_match_adapter_fields() {
    for (model, requested, expected) in [
        ("ollama:qwen3:32b", "minimal", ReasoningValue::Enabled),
        ("ollama-cloud:qwen3:32b", "off", ReasoningValue::Disabled),
        (
            "ollama:gpt-oss:20b",
            "minimal",
            ReasoningValue::Categorical("low".into()),
        ),
        (
            "anthropic:claude-haiku-4-5-20251001",
            "budget:1024",
            ReasoningValue::TokenBudget(1024),
        ),
    ] {
        let capture = capture_route(model, None, None, None, None);
        let options = StreamOptions {
            model: Some(model.into()),
            reasoning: Some(requested.into()),
            ..Default::default()
        };
        let policy = resolve_request_policy(&capture, &options, false).unwrap();
        assert_eq!(policy.reasoning.effective, expected);
        let mut body = json!({"model": capture.route.native_model});
        if capture.route.provider == "anthropic" {
            body["max_tokens"] = json!(16384);
            apply_anthropic_thinking(&mut body, &capture.route.native_model, Some(requested));
        } else {
            body["think"] =
                ollama_think_value(&capture.route.native_model, Some(requested)).unwrap();
            if let Some(cap) = policy
                .configured_constraints
                .get(&CapacityField::TransportTotal)
            {
                body["options"] = json!({"num_ctx":cap.tokens});
            }
        }
        let bound = StreamOptions {
            resolved_policy: Some(std::sync::Arc::new(policy)),
            ..Default::default()
        };
        validate_policy_wire(&bound, &body).unwrap();
    }
    let capture = capture_route(
        "anthropic:claude-haiku-4-5-20251001",
        None,
        None,
        None,
        None,
    );
    for requested in ["budget:50000", "budget:0", "budget:999999", "ultra"] {
        assert!(
            resolve_request_policy(
                &capture,
                &StreamOptions {
                    reasoning: Some(requested.into()),
                    ..Default::default()
                },
                false
            )
            .is_err()
        );
    }
    let bound = capture_route("anthropic:claude-fable-5-1", None, None, None, None);
    let policy = resolve_request_policy(
        &bound,
        &StreamOptions {
            reasoning: Some("minimal".into()),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    let ReasoningValue::Adaptive(parameters) = policy.reasoning.effective else {
        panic!("expected bound adaptive")
    };
    assert_eq!(parameters["effort"], "low");
    assert!(parameters.contains_key("block_binding"));
}

#[test]
fn resolved_policy_unknown_default_and_omitted_controls_are_not_disabled() {
    for model in ["private:model", "google-antigravity:gemini-2.5-flash"] {
        let policy = resolve_request_policy(
            &capture_route(model, None, None, None, None),
            &StreamOptions::default(),
            true,
        )
        .unwrap();
        assert_eq!(policy.reasoning.effective, ReasoningValue::ProviderDefault);
        assert_eq!(policy.output.enforced_total, None);
    }
    let capture = capture_route("ollama:qwen3:32b", None, None, None, None);
    let options = StreamOptions {
        reasoning: Some("off".into()),
        extra_body: [("think".into(), json!(true))].into(),
        ..Default::default()
    };
    assert!(resolve_request_policy(&capture, &options, false).is_err());
}

#[tokio::test]
async fn resolved_policy_incompatible_generation_rejects_before_bridge_stream() {
    struct ChangingBridge {
        generation: AtomicUsize,
        calls: AtomicUsize,
    }
    #[async_trait]
    impl LlmBridge for ChangingBridge {
        fn policy_capture(&self, model: &str) -> PolicyCapture {
            let mut capture = capture_route(model, None, None, None, None);
            capture.inventory_generation = Some(self.generation.load(Ordering::SeqCst) as u64);
            capture
        }
        async fn stream(
            &self,
            _: &str,
            _: &[LlmMessage],
            _: &[ToolDefinition],
            _: &StreamOptions,
        ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (_, receiver) = mpsc::channel(1);
            Ok(receiver)
        }
    }
    let bridge = ChangingBridge {
        generation: AtomicUsize::new(1),
        calls: AtomicUsize::new(0),
    };
    let options = StreamOptions {
        model: Some("openai-codex:gpt-6-astra".into()),
        ..Default::default()
    };
    let prepared = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "s",
            messages: &[],
            tools: &[],
            options: &options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    bridge.generation.store(2, Ordering::SeqCst);
    assert!(
        prepared
            .stream()
            .await
            .unwrap_err()
            .to_string()
            .contains("prepare again")
    );
    assert_eq!(prepared.policy().capture.inventory_generation, Some(1));
    assert_eq!(bridge.calls.load(Ordering::SeqCst), 0);
    let next = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "s",
            messages: &[],
            tools: &[],
            options: &options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(next.policy().capture.inventory_generation, Some(2));
    drop(next.stream().await.unwrap());
    assert_eq!(bridge.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn resolved_policy_late_provider_usage_stays_with_its_request() {
    let bridge = WireBridge::default();
    let shared = crate::settings::shared("openai-codex:gpt-6-astra");
    let first_options = StreamOptions {
        model: Some("openai-codex:gpt-6-astra".into()),
        policy_sink: Some(shared.clone()),
        ..Default::default()
    };
    let first = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "first",
            messages: &[],
            tools: &[],
            options: &first_options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    drop(first.stream().await.unwrap());
    let next_options = StreamOptions {
        model: Some("openai:gpt-6-astra".into()),
        policy_sink: Some(shared.clone()),
        ..Default::default()
    };
    let next = PreparedModelRequest::prepare(
        &bridge,
        RequestInputs {
            system: "second",
            messages: &[],
            tools: &[],
            options: &next_options,
            policy: RequestPolicy::Turn,
        },
        None,
        |_| Ok(()),
    )
    .unwrap();
    drop(next.stream().await.unwrap());
    first.record_usage((70_000, 5_000, 60_000, 0));
    let settings = shared.lock().unwrap();
    assert_eq!(
        settings.last_inference_policy.as_ref().unwrap().snapshot_id,
        next.policy().snapshot_id
    );
    let usage = settings.last_provider_usage.as_ref().unwrap();
    assert_eq!(usage.snapshot_id, first.policy().snapshot_id.to_string());
    assert_eq!(usage.input_tokens, 70_000);
    assert_eq!(usage.cache_read_tokens, 60_000);
}

#[test]
fn resolved_policy_manifest_adapter_cannot_borrow_native_responses_semantics() {
    let endpoint = crate::bridge::EndpointRouteProvenance {
        selected_provider_id: "openai".into(),
        endpoint_id: "private".into(),
        adapter_id: "chat-completions".into(),
        inventory_generation: 72,
        contribution_generation_id: "manifest-v1".into(),
        schema_dialect: "open_ai".into(),
        connection_id: Some("private-connection".into()),
    };
    let capture = capture_route(
        "openai:gpt-6-astra",
        None,
        None,
        Some("declared_bearer_secret"),
        Some(&endpoint),
    );
    let options = StreamOptions {
        extra_body: [("max_output_tokens".into(), json!(20_000))].into(),
        ..Default::default()
    };
    let policy = resolve_request_policy(&capture, &options, false).unwrap();
    assert!(policy.selected_facts.is_empty());
    assert_eq!(policy.output.enforced_total, None);
    assert_eq!(policy.output.requested_cap, Some(20_000));
    assert_eq!(policy.reasoning.effective, ReasoningValue::ProviderDefault);
    assert!(!policy.capture.native_adapter);
}

#[test]
fn resolved_policy_native_alias_and_raw_replay_account_for_visible_input_once() {
    let capture = capture_route("openai-codex:gpt-5.6", None, None, None, None);
    assert_eq!(capture.route.native_model, "gpt-5.6-sol");
    let output = json!([
        {"type":"reasoning", "id":"rs_fixture", "encrypted_content":"x".repeat(40000), "summary":[]},
        {"type":"message", "id":"msg_fixture", "role":"assistant", "content":[{"type":"output_text", "text":"UNIQUE_VISIBLE_ANSWER"}]}
    ]);
    let messages = [LlmMessage::Assistant {
        text: vec!["UNIQUE_VISIBLE_ANSWER".into()],
        thinking: vec![],
        tool_calls: vec![],
        raw: Some(
            json!({"responses":{"provider":"openai-codex", "model":"gpt-5.6-sol", "output":output},"usage":{"input_tokens":50000,"cached_tokens":40000}}),
        ),
    }];
    let input = estimate_policy_input(&capture, "system", &messages, &[]).unwrap();
    assert!(
        input.history_and_attachments >= 10000,
        "encrypted continuation is visible input"
    );
    let wire = build_responses_body(
        "openai-codex",
        "gpt-5.6-sol",
        "system",
        &messages,
        &[],
        &StreamOptions::default(),
    )
    .unwrap();
    assert_eq!(
        wire["input"]
            .to_string()
            .matches("UNIQUE_VISIBLE_ANSWER")
            .count(),
        1
    );
    assert!(!wire["input"].to_string().contains("cached_tokens"));
}

#[test]
fn resolved_policy_top_revision_conflicts_are_order_independent_before_transport() {
    let bridge = WireBridge::default();
    let mut capture = capture_route("openai-codex:gpt-6-astra", None, None, None, None);
    let mut old = capture.facts.capacity[0].clone();
    old.provenance.source = crate::inference_inventory::InventorySource::Discovery;
    old.provenance.status = EvidenceStatus::Discovered;
    old.provenance.sequence = Some(7);
    old.tokens = 250_000;
    let mut conflicting_old = old.clone();
    conflicting_old.tokens = 240_000;
    let mut current = old.clone();
    current.tokens = 260_000;
    current.provenance.sequence = Some(8);
    let observations = [old, conflicting_old, current];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        capture.facts.capacity = order.map(|index| observations[index].clone()).to_vec();
        let options = StreamOptions {
            model: Some("openai-codex:gpt-6-astra".into()),
            policy_capture: Some(capture.clone()),
            ..Default::default()
        };
        let prepared = PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "s",
                messages: &[],
                tools: &[],
                options: &options,
                policy: RequestPolicy::Turn,
            },
            None,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(prepared.policy().working_window, 260_000);
        assert_eq!(prepared.policy().superseded_facts.len(), 2);
    }
    let mut conflict = observations[2].clone();
    conflict.tokens = 255_000;
    capture.facts.capacity = vec![observations[2].clone(), conflict];
    let options = StreamOptions {
        model: Some("openai-codex:gpt-6-astra".into()),
        policy_capture: Some(capture),
        ..Default::default()
    };
    assert!(
        PreparedModelRequest::prepare(
            &bridge,
            RequestInputs {
                system: "s",
                messages: &[],
                tools: &[],
                options: &options,
                policy: RequestPolicy::Turn
            },
            None,
            |_| -> anyhow::Result<()> { panic!("top conflict reached evidence") }
        )
        .is_err()
    );
    assert_eq!(bridge.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn resolved_policy_configured_endpoints_isolate_consent_and_ignore_token_rotation() {
    fn client(provider: &str, base: &str, key: &str) -> Box<dyn LlmBridge> {
        match provider {
            "openrouter" => Box::new(OpenRouterClient {
                inner: OpenAIClient {
                    client: reqwest::Client::new(),
                    api_key: key.into(),
                    base_url: base.into(),
                    endpoint_id: provider.into(),
                    request_url: None,
                },
            }),
            "ollama-cloud" => Box::new(OllamaCloudClient {
                client: reqwest::Client::new(),
                api_key: key.into(),
                base_url: base.into(),
            }),
            "github-copilot" => Box::new(GithubCopilotClient {
                client: reqwest::Client::new(),
                base_url: base.into(),
            }),
            _ => Box::new(OpenAICompatClient::new(
                key.into(),
                base.into(),
                provider.into(),
            )),
        }
    }
    for (provider, model, base) in [
        (
            "openrouter",
            "stealth/ox-alpha",
            "https://openrouter.ai/api",
        ),
        (
            "ollama-cloud",
            "qwen3-coder:480b-cloud",
            ollama_cloud_base_url(),
        ),
        (
            "github-copilot",
            "gpt-5.4",
            crate::github_copilot::DEFAULT_COPILOT_API_BASE_URL,
        ),
        (
            "groq",
            "llama-3.3-70b-versatile",
            compat_base_url("groq").unwrap(),
        ),
    ] {
        let spec = format!("{provider}:{model}");
        let original = client(provider, base, "first-secret").policy_capture(&spec);
        let refreshed = client(provider, base, "refreshed-secret").policy_capture(&spec);
        assert_eq!(original.route, refreshed.route);
        let target = NumericTarget {
            route: original.route.clone(),
            tokens: 600_000,
            maximum_revision: None,
            invalid_tokens: None,
        };
        let mut refreshed = refreshed;
        refreshed
            .intent
            .targets
            .push((TargetSource::User, target.clone()));
        assert_eq!(
            refreshed
                .resolve(None, Default::default())
                .unwrap()
                .targets
                .len(),
            1
        );
        let mut custom = client(
            provider,
            "https://user:secret@private.invalid/api",
            "first-secret",
        )
        .policy_capture(&spec);
        custom.intent.targets.push((TargetSource::User, target));
        assert_ne!(original.route.endpoint, custom.route.endpoint);
        assert!(custom.facts.capacity.is_empty());
        assert!(
            custom
                .resolve(None, Default::default())
                .unwrap()
                .targets
                .is_empty()
        );
        let encoded = serde_json::to_string(&custom.route).unwrap();
        assert!(!encoded.contains("secret") && !encoded.contains("private.invalid"));
    }
}

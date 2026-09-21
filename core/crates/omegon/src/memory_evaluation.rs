//! Opt-in bounded provider experiment; ordinary CI uses the deterministic tier.
#[cfg(test)]
#[path = "../../omegon-memory/tests/support/evaluation.rs"]
mod corpus;

#[cfg(test)]
mod tests {
    use super::corpus::*;
    use serde_json::json;
    use std::time::{Duration, Instant};

    const TOKEN_LIMIT: u64 = 100_000;
    const TIME_LIMIT: u64 = 600;
    const CALL_TIMEOUT: u64 = 45;
    const OUTPUT_RESERVATION: u64 = 16_384;

    struct Budget {
        start: Instant,
        charged: u64,
        measured: u64,
        stopped: Option<String>,
        calls: Vec<serde_json::Value>,
    }
    impl Budget {
        async fn call(
            &mut self,
            model: &str,
            prompt: &str,
            stage: &str,
            case: &str,
        ) -> (Option<String>, Cost) {
            // Reserve full provider output allowance plus conservative UTF-8 input and
            // routing/system overhead before dispatch. Unknown usage stops the run.
            let allowance = match dispatch_allowance(model) {
                Ok(allowance) => allowance,
                Err(reason) => {
                    self.stopped = Some(reason.into());
                    self.calls.push(json!({"case":case,"stage":stage,"requested_model":model,"status":"not_dispatched","reason":reason,"cost":Cost::offline()}));
                    return (None, Cost::unavailable());
                }
            };
            let reservation = prompt.len() as u64 + 4096 + allowance;
            let remaining = TIME_LIMIT.saturating_sub(self.start.elapsed().as_secs());
            if self.stopped.is_some()
                || remaining == 0
                || self.charged.saturating_add(reservation) > TOKEN_LIMIT
            {
                self.stopped
                    .get_or_insert("execution budget exhausted".into());
                return (None, Cost::unavailable());
            }
            self.charged += reservation;
            let start = Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(remaining.min(CALL_TIMEOUT)),
                completion(model, prompt),
            )
            .await;
            match result {
                Ok(Ok((text, input, output, serving)))
                    if input > 0 && output > 0 && input.saturating_add(output) <= reservation =>
                {
                    self.charged -= reservation - input - output;
                    self.measured += input + output;
                    let cost = Cost {
                        input_tokens: Some(input),
                        output_tokens: Some(output),
                        usd: None,
                        missing: vec!["provider pricing not configured; USD unknown".into()],
                    };
                    self.calls.push(json!({"case":case,"stage":stage,"requested_model":model,"serving_model":serving,"reserved_tokens":reservation,"output_ceiling":allowance,"latency_ms":start.elapsed().as_millis(),"cost":cost,"output":text}));
                    (Some(text), cost)
                }
                other => {
                    // Errors can contain upstream response bodies. Keep report diagnostics
                    // classified; never persist raw credential-bearing provider errors.
                    let observed_usage = if let Ok(Ok((_, input, output, _))) = &other {
                        let observed = input.saturating_add(*output);
                        self.measured = self.measured.saturating_add(observed);
                        self.charged = self
                            .charged
                            .saturating_add(observed.saturating_sub(reservation));
                        json!({"input_tokens":input,"output_tokens":output})
                    } else {
                        serde_json::Value::Null
                    };
                    let reason = match other {
                        Err(_) => "provider deadline exceeded",
                        Ok(Ok(_)) => "provider usage missing or exceeded reservation",
                        Ok(Err(ref error))
                            if error.to_string() == "no configured provider route" =>
                        {
                            "no configured provider route"
                        }
                        _ => "provider route or request failed",
                    };
                    self.stopped = Some(reason.into());
                    self.calls.push(json!({"case":case,"stage":stage,"requested_model":model,"status":"unavailable","reason":reason,"latency_ms":start.elapsed().as_millis(),"reserved_tokens":reservation,"observed_partial_or_excess_usage":observed_usage,"cost":Cost::unavailable()}));
                    (None, Cost::unavailable())
                }
            }
        }
    }

    /// Route-level safety, independent of the experiment-mode flag. Direct OpenAI
    /// paid-API routes are excluded: their Chat Completions path has incompatible
    /// limit/usage semantics. Subscription evaluation never falls back to that path.
    fn dispatch_allowance(model: &str) -> Result<u64, &'static str> {
        if model.starts_with("openai-codex:") {
            return crate::model_registry::ModelRegistry::global()
                .model_info(model)
                .map(|entry| entry.context_output as u64)
                .filter(|limit| *limit > 0)
                .ok_or("Codex model has no registered full output ceiling");
        }
        if model.starts_with("anthropic:") {
            return Ok(OUTPUT_RESERVATION);
        }
        Err("unsupported route: no verified bounded request with complete usage")
    }

    async fn completion(model: &str, prompt: &str) -> anyhow::Result<(String, u64, u64, String)> {
        dispatch_allowance(model).map_err(anyhow::Error::msg)?;
        let route = crate::session_execution::boot_execution_binding()
            .resolve_provider_route(model, None)
            .await
            .ok_or_else(|| anyhow::anyhow!("no configured provider route"))?;
        let serving = route.serving_model().to_string();
        anyhow::ensure!(
            serving == model,
            "evaluation requires the frozen exact serving model"
        );
        let recorder = crate::provider_route_service::StepRouteLeaseRecorder::for_ephemeral_step(
            uuid::Uuid::new_v4(),
        )?;
        let options = stream_options(&serving);
        let mut rx = route
            .stream(
                crate::provider_route_service::RouteLeaseOwner::Step(&recorder),
                "You are a concise evidence evaluation assistant.",
                &[crate::bridge::LlmMessage::User {
                    content: prompt.into(),
                    images: vec![],
                }],
                &[],
                &options,
            )
            .await?;
        let mut text = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                crate::bridge::LlmEvent::TextDelta { delta } => {
                    anyhow::ensure!(text.len() + delta.len() <= 16_384, "response byte limit");
                    text.push_str(&delta);
                }
                crate::bridge::LlmEvent::Done {
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cache_creation_tokens,
                    ..
                } => {
                    let input = aggregate_input(
                        model,
                        input_tokens,
                        cache_read_tokens,
                        cache_creation_tokens,
                    );
                    return Ok((text, input, output_tokens, serving));
                }
                crate::bridge::LlmEvent::Error { .. }
                | crate::bridge::LlmEvent::UpstreamFailure { .. } => {
                    anyhow::bail!("provider request failed")
                }
                _ => {}
            }
        }
        anyhow::bail!("provider stream incomplete")
    }

    fn stream_options(model: &str) -> crate::bridge::StreamOptions {
        crate::bridge::StreamOptions {
            model: Some(model.into()),
            reasoning: model.starts_with("openai-codex:").then(|| "low".into()),
            extended_context: false,
            // Codex has no request-level output ceiling. Reserve the registered full
            // model ceiling, including reasoning, and send no unsupported extras.
            extra_body: Default::default(),
        }
    }

    fn aggregate_input(model: &str, input: u64, cache_read: u64, cache_creation: u64) -> u64 {
        if model.starts_with("anthropic:") {
            input
                .saturating_add(cache_read)
                .saturating_add(cache_creation)
        } else {
            // OpenAI cache tokens are a subset of input_tokens, not additional input.
            input
        }
    }

    #[tokio::test]
    async fn memory_evaluation_offline_host_uses_shared_contract() {
        let rows = offline("development", true).await.unwrap();
        assert_eq!(rows.len(), 16);
        assert!(
            rows.iter()
                .all(|row| row.trace.injected_tokens <= row.trace.budget)
        );
    }

    #[tokio::test]
    #[ignore = "explicit live intent required; at most 100000 aggregate tokens and 600 seconds"]
    async fn memory_evaluation_live() -> anyhow::Result<()> {
        anyhow::ensure!(
            std::env::var("OMEGON_MEMORY_EVAL_LIVE").as_deref() == Ok("1"),
            "set explicit live intent"
        );
        let output = std::env::var("OMEGON_MEMORY_EVAL_REPORT")?;
        // A second invocation must not silently reset this attempt's aggregate budget.
        let _attempt = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        let profile = crate::settings::Profile::load(&std::env::current_dir()?);
        let reader = profile
            .model_intent
            .as_ref()
            .and_then(|m| m.exact_model_override.clone())
            .or_else(|| {
                profile
                    .last_used_model
                    .as_ref()
                    .map(|m| format!("{}:{}", m.provider, m.model_id))
            });
        let extractor = profile
            .memory_extraction_model
            .unwrap_or_else(|| "anthropic:claude-haiku-4-5-20251001".into());
        let cap = profile.memory_context_tokens.unwrap_or(1024);
        let codex = std::env::var("OMEGON_MEMORY_EVAL_CODEX").as_deref() == Ok("1");
        let (reader, extractor, output_allowance, codex_config) = if codex {
            let registry = crate::model_registry::ModelRegistry::global();
            let model = registry
                .grade_model("D", "openai-codex")
                .ok_or_else(|| anyhow::anyhow!("no configured Codex grade-D model"))?;
            let qualified = format!("openai-codex:{model}");
            let info = registry
                .model_info(&qualified)
                .ok_or_else(|| anyhow::anyhow!("Codex model missing from registry"))?;
            let cache: serde_json::Value = serde_json::from_slice(&std::fs::read(
                dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("home unavailable"))?
                    .join(".codex/models_cache.json"),
            )?)?;
            let offering = cache["models"]
                .as_array()
                .and_then(|models| {
                    models
                        .iter()
                        .find(|entry| entry["slug"] == model && entry["supported_in_api"] == true)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "configured Codex model absent from existing subscription catalog"
                    )
                })?;
            anyhow::ensure!(
                offering["supported_reasoning_levels"]
                    .as_array()
                    .is_some_and(|levels| levels.iter().any(|level| level["effort"] == "low")),
                "subscription model does not advertise low reasoning"
            );
            let allowance = info.context_output as u64;
            anyhow::ensure!(
                allowance > 0 && allowance + 8192 < TOKEN_LIMIT,
                "full registered model output ceiling does not fit authorized budget"
            );
            let config = json!({"model":qualified,"selection":"registry grade D, corroborated by existing Codex subscription models_cache","catalog_client_version":cache["client_version"],"catalog_etag":cache["etag"],"reasoning":"low","registered_full_output_ceiling":allowance,"unsupported_request_fields":[],"paid_api_fallback":false});
            (qualified.clone(), qualified, allowance, config)
        } else {
            (
                reader.ok_or_else(|| anyhow::anyhow!("no configured reader model"))?,
                extractor,
                OUTPUT_RESERVATION,
                serde_json::Value::Null,
            )
        };
        let (prior_charge, prior_measured, prior_elapsed, prior_evidence) = if codex {
            let path = std::env::var("OMEGON_MEMORY_EVAL_PRIOR_REPORT")?;
            let previous: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            let no_dispatch = previous["measured_tokens"] == 0
                && previous["calls"].as_array().is_some_and(|calls| {
                    !calls.is_empty()
                        && calls.iter().all(|call| {
                            call["reason"] == "no configured provider route"
                                && call["status"] == "unavailable"
                        })
                });
            let charge = if no_dispatch {
                0
            } else {
                previous["charged_or_reserved_tokens"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("prior reservation missing"))?
            };
            let measured = previous["measured_tokens"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("prior usage missing"))?;
            let elapsed = previous["elapsed_ms"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("prior elapsed missing"))?;
            (
                charge,
                measured,
                elapsed,
                json!({"report":path,"carried_charge":charge,"carried_elapsed_ms":elapsed,"released_reservation":if no_dispatch {previous["charged_or_reserved_tokens"].clone()} else {json!(0)},"basis":if no_dispatch {"no configured provider route is returned before route.stream, so no inference dispatch occurred"} else {"carry prior aggregate charge and elapsed; no unproven reservation released"}}),
            )
        } else {
            (0, 0, 0, serde_json::Value::Null)
        };
        let mut budget = Budget {
            start: Instant::now() - Duration::from_millis(prior_elapsed),
            charged: prior_charge,
            measured: prior_measured,
            stopped: None,
            calls: vec![],
        };
        if !codex && profile.memory_extraction_enabled == Some(false) {
            budget.stopped = Some("configured extraction disabled".into());
        }
        let auth_status = if codex {
            // Existing resolver adopts a usable external Codex grant or refreshes the
            // stored expired grant, with guarded persistence. Never print credentials.
            match tokio::time::timeout(
                Duration::from_secs(30),
                crate::auth::resolve_with_refresh_result("openai-codex"),
            )
            .await
            {
                Ok(Ok(Some((_, true)))) => "existing subscription OAuth ready".to_owned(),
                Ok(Err(error)) => {
                    let reason = format!("Codex subscription refresh: {error}");
                    budget.stopped = Some(reason.clone());
                    reason
                }
                _ => {
                    let reason = "existing Codex subscription OAuth unavailable".to_owned();
                    budget.stopped = Some(reason.clone());
                    reason
                }
            }
        } else {
            "resolved during dispatch".into()
        };
        let development = offline("development", true).await?;
        let mut manifest = json!({"revision":REVISION,"corpus_sha256":omegon_memory::retrieval::raw_content_hash(CORPUS),"labels_sha256":omegon_memory::retrieval::raw_content_hash(LABELS),
        "reader":reader,"extractor":extractor,"reader_selection":"project merged Profile exact override or last-used model","extractor_selection":"Profile override or host default",
        "seed":null,"seed_support":"not requested through existing routes","reasoning":null,"temperature":null,"provider_defaults":true,
        "prompt_revision":"memory-wave5d-v3","scorer_revision":"cutoff-grounded-verbatim-claims-v1","token_limit":TOKEN_LIMIT,"wall_seconds":TIME_LIMIT,"call_timeout_seconds":CALL_TIMEOUT,"output_reservation":{"reader":dispatch_allowance(&reader).ok(),"extractor":dispatch_allowance(&extractor).ok()},"configured_model_output_ceiling":output_allowance,"codex_subscription":codex_config,"prior_attempt":prior_evidence,"auth_status":auth_status,
        "current_cap":cap,"candidate_cap":cap.min(256),"accounting":"conservative_utf8_bytes","embedding":"disabled, lexical-only paired evaluation",
        "cost_note":"Extraction is shared once per case and attributed in full to each memory arm; sum calls, not arm costs, for actual spend. File curation labor is unmeasured. USD pricing unavailable.",
        "development_offline_summary":summarize(&development)});
        if codex {
            manifest["reader_selection"] = json!(
                "existing Codex subscription; configured registry grade D and cached offering"
            );
            manifest["extractor_selection"] = manifest["reader_selection"].clone();
            manifest["reasoning"] = json!("low");
            manifest["credential_source"] = json!(
                std::env::var("OMEGON_MEMORY_EVAL_CREDENTIAL_SOURCE")
                    .unwrap_or_else(|_| "existing Omegon/Codex OAuth resolution".into())
            );
            manifest["provider_defaults"] = json!({"temperature":"provider default","verbosity":"medium from native Responses bridge","reasoning":"explicit low"});
        }
        std::fs::write(
            format!("{output}.configuration.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        let gold = labels();
        let mut rows = vec![];
        for split in ["development", "heldout"] {
            if split == "heldout" {
                // Persist immutable acceptance policy before constructing held-out views.
                manifest["frozen_before_heldout"] = json!(true);
                manifest["development_live_summary"] = summarize(&rows);
                manifest["thresholds"] = json!({"basis":"measured deterministic development: memory arms 4/4; no model-quality claim if live incomplete",
                "minimum_task_success":0.75,"minimum_evidence_recall":1.0,"max_stale_memory_usage":0.0,"max_repeated_error_rate":0.0,
                "candidate_regression_tolerance":0.0,"max_request_latency_ms":45000,"require_complete_paired_rows":true});
                std::fs::write(
                    format!("{output}.manifest.json"),
                    serde_json::to_vec_pretty(&manifest)?,
                )?;
            }
            for case in cases(split)? {
                let view = case.view()?;
                let (extraction, ingestion_cost) = budget
                    .call(&extractor, &extraction_prompt(&view), "formation", &case.id)
                    .await;
                for adapter in ADAPTERS {
                    let start = Instant::now();
                    let mut trace = adapt(
                        &view,
                        adapter,
                        &omegon_memory::SqliteBackend::in_memory()?,
                        extraction.as_deref().ok_or("extractor unavailable"),
                        cap,
                    )
                    .await?;
                    if matches!(adapter, Adapter::CurrentMemory | Adapter::CandidatePolicy) {
                        trace.ingestion_cost = ingestion_cost.clone();
                    }
                    let (response, query_cost) = budget
                        .call(
                            &reader,
                            &reader_prompt(&view.query, &trace),
                            "task",
                            &case.id,
                        )
                        .await;
                    trace.query_cost = query_cost;
                    let mut row = score(&case.id, &gold[&case.id], trace, response);
                    row.latency_ms = row
                        .response
                        .as_ref()
                        .map(|_| start.elapsed().as_millis() as u64);
                    rows.push(row);
                }
                std::fs::write(
                    &output,
                    serde_json::to_vec_pretty(
                        &json!({"manifest":manifest,"rows":rows,"summary":summarize(&rows),"heldout_summary":summarize(&rows.iter().filter(|row| row.case_id.starts_with("held-")).cloned().collect::<Vec<_>>()),"calls":budget.calls,"measured_tokens":budget.measured,"charged_or_reserved_tokens":budget.charged,"elapsed_ms":budget.start.elapsed().as_millis(),"stop_reason":budget.stopped,"acceptance":acceptance(&rows)}),
                    )?,
                )?;
            }
        }
        assert!(budget.charged <= TOKEN_LIMIT);
        Ok(())
    }

    fn acceptance(rows: &[ResultRow]) -> &'static str {
        let held: Vec<_> = rows
            .iter()
            .filter(|row| row.case_id.starts_with("held-"))
            .collect();
        if held.len() != 16
            || held.iter().any(|row| {
                matches!(row.task, Outcome::Unavailable | Outcome::Uncertain)
                    || row.trace.formation_error.is_some()
                    || row.trace.retrieval_error.is_some()
                    || row.trace.selection_error.is_some()
            })
        {
            return "incomplete; no model-quality acceptance";
        }
        let successes = |adapter| {
            held.iter()
                .filter(|r| r.trace.adapter == adapter && r.task == Outcome::Pass)
                .count()
        };
        if successes(Adapter::CurrentMemory) < 3
            || successes(Adapter::CandidatePolicy) < successes(Adapter::CurrentMemory)
            || held.iter().any(|r| {
                matches!(
                    r.trace.adapter,
                    Adapter::CurrentMemory | Adapter::CandidatePolicy
                ) && (r.evidence_recall.is_some_and(|v| v < 1.0)
                    || r.stale_memory_used == Some(true)
                    || r.repeated_error == Some(true))
            })
            || held
                .iter()
                .any(|r| r.latency_ms.is_some_and(|ms| ms > 45_000))
        {
            return "smoke thresholds failed";
        }
        "smoke thresholds passed; synthetic sample does not establish general model quality"
    }

    #[tokio::test]
    async fn aggregate_budget_exhaustion_stops_before_dispatch() {
        let codex_options = stream_options("openai-codex:test");
        assert!(
            codex_options.extra_body.is_empty(),
            "Codex must not receive unsupported output-limit fields"
        );
        assert_eq!(codex_options.reasoning.as_deref(), Some("low"));
        assert_eq!(aggregate_input("anthropic:test", 2, 100, 20), 122);
        assert_eq!(aggregate_input("openai:test", 122, 100, 0), 122);
        let mut budget = Budget {
            start: Instant::now(),
            charged: TOKEN_LIMIT - 1,
            measured: 0,
            stopped: None,
            calls: vec![],
        };
        assert!(
            budget
                .call("anthropic:test", "prompt", "task", "case")
                .await
                .0
                .is_none()
        );
        assert_eq!(budget.charged, TOKEN_LIMIT - 1);
        assert!(budget.calls.is_empty());
        budget.stopped = None;
        let configured = format!(
            "openai-codex:{}",
            crate::model_registry::ModelRegistry::global()
                .grade_model("D", "openai-codex")
                .unwrap()
        );
        assert_eq!(
            dispatch_allowance(&configured).unwrap(),
            crate::model_registry::ModelRegistry::global()
                .model_info(&configured)
                .unwrap()
                .context_output as u64
        );
        budget.charged = TOKEN_LIMIT.saturating_sub(dispatch_allowance(&configured).unwrap());
        let charged_before = budget.charged;
        assert!(
            budget
                .call(&configured, "prompt", "task", "case")
                .await
                .0
                .is_none()
        );
        assert_eq!(budget.charged, charged_before);
        assert!(
            budget.calls.is_empty(),
            "full model ceiling must fit before dispatch"
        );
        assert!(dispatch_allowance("openai:gpt-5.4").is_err());
        assert!(dispatch_allowance("openai:gpt-5.4-mini").is_err());
        assert!(dispatch_allowance("openai:gpt-6-astra").is_err());
        assert_eq!(acceptance(&[]), "incomplete; no model-quality acceptance");
        let rows = offline("heldout", true).await.unwrap();
        assert_eq!(
            acceptance(&rows),
            "smoke thresholds passed; synthetic sample does not establish general model quality"
        );
    }
}

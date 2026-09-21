//! Owned interval and awaited pre-eviction capture. Canonical replay owns sources.
use super::*;
use omegon_traits::ContextCheckpointOutcome as Outcome;
use tokio::sync::oneshot;

const TURN_INTERVAL: u32 = 8;

pub(super) fn on_start(feature: &mut MemoryFeature) {
    reap(feature);
    if feature.session_binding.is_some()
        && feature.memory_binding.available()
        && feature.checkpoint_task.is_none()
        && feature.session_end_tasks.lock().unwrap().accepting
    {
        let _ = spawn(feature);
    }
}

pub(super) fn status(feature: &MemoryFeature) -> serde_json::Value {
    let running = feature
        .checkpoint_task
        .as_ref()
        .is_some_and(|task| !task.handle.is_finished());
    let finalizing = feature.finalization_running.load(Ordering::SeqCst);
    let backpressure = if finalization::queued(feature) == 8 {
        Some("finalization_queue_full")
    } else if feature.checkpoint_backlog.load(Ordering::SeqCst) {
        Some("capture_backlog")
    } else if feature.checkpoint_turns >= TURN_INTERVAL && (running || finalizing) {
        Some("capture_busy")
    } else {
        None
    };
    serde_json::json!({"interval_turns":TURN_INTERVAL,"pages_per_pass":8,"turns_since_request":feature.checkpoint_turns,"capture_due":feature.checkpoint_turns>=TURN_INTERVAL,"worker_running":running,"finalization_running":finalizing,"finalization_queued":finalization::queued(feature),"backpressure":backpressure,"last_pre_eviction":feature.last_pre_eviction})
}

fn reap(feature: &mut MemoryFeature) {
    if feature
        .checkpoint_task
        .as_ref()
        .is_some_and(|task| task.handle.is_finished())
    {
        let task = feature.checkpoint_task.take().expect("finished checkpoint");
        if !matches!(task.handle.join(), Ok(Ok(()))) {
            feature.checkpoint_turns = TURN_INTERVAL;
            tracing::warn!("memory checkpoint failed; committed source remains in session log");
        }
    }
}

pub(super) fn on_turn(feature: &mut MemoryFeature) {
    if !feature.memory_binding.available() || !feature.session_end_tasks.lock().unwrap().accepting {
        return;
    }
    feature.checkpoint_turns = feature
        .checkpoint_turns
        .saturating_add(1)
        .min(TURN_INTERVAL);
    reap(feature);
    if feature.checkpoint_turns < TURN_INTERVAL || feature.checkpoint_task.is_some() {
        return;
    }
    let _ = spawn(feature);
}

pub(super) async fn before_eviction(
    feature: &mut MemoryFeature,
    cancel: tokio_util::sync::CancellationToken,
) -> Outcome {
    feature.last_pre_eviction = None;
    let unavailable = |reason: &str| Outcome::Unavailable {
        reason: reason.into(),
    };
    reap(feature);
    let outcome = if cancel.is_cancelled() {
        unavailable("cancelled")
    } else if !feature.memory_binding.available() {
        unavailable("store_unavailable")
    } else if !feature.session_end_tasks.lock().unwrap().accepting {
        unavailable("admission_closed")
    } else if feature.checkpoint_task.is_some() {
        unavailable("capture_busy")
    } else {
        match spawn(feature) {
            Err(reason) => Outcome::Unavailable { reason },
            Ok(receiver) => {
                let worker = feature
                    .checkpoint_task
                    .as_ref()
                    .expect("spawned checkpoint")
                    .cancellation
                    .clone();
                let _cancel_on_drop = worker.drop_guard();
                tokio::select! {
                    biased;
                    _=cancel.cancelled()=>unavailable("cancelled"),
                    result=tokio::time::timeout(std::time::Duration::from_secs(10),receiver)=>match result {
                        Ok(Ok(outcome))=>outcome,
                        Ok(Err(_))=>unavailable("checkpoint_worker_failed"),
                        Err(_)=>unavailable("checkpoint_timed_out"),
                    }
                }
            }
        }
    };
    if matches!(outcome, Outcome::Unavailable { .. }) {
        feature.checkpoint_turns = TURN_INTERVAL;
    }
    feature.last_pre_eviction = Some(outcome.clone());
    outcome
}

fn spawn(feature: &mut MemoryFeature) -> Result<oneshot::Receiver<Outcome>, String> {
    let session_id = feature
        .session_id
        .lock()
        .unwrap()
        .clone()
        .ok_or("sessionless")?;
    let binding = feature.session_binding.clone().ok_or("sessionless")?;
    let target = binding.snapshot().ok_or("source_unavailable")?;
    let memory_binding = feature.memory_binding.clone();
    let mind = feature.mind.clone();
    let extractor = feature.extractor.clone();
    let status_root = feature.status_root.clone();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let worker = cancellation.clone();
    let backlog = feature.checkpoint_backlog.clone();
    let (send, receive) = oneshot::channel();
    let handle=std::thread::Builder::new().name("memory-checkpoint".into()).spawn(move|| {
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"checkpoint_runtime_failed".to_string())?;
        let result:Result<bool,String>=runtime.block_on(async {
            tokio::select! {
                biased;
                _=worker.cancelled()=>Err("cancelled".into()),
                result=async {
                    drain(&binding, &target, SessionEndPipelineInput {mind,memory_binding,extractor,evidence:formation::unavailable(&session_id,"capture_pending"),session_id,status_root,turns:0,tool_calls:0,duration_secs:0.0}, Some(backlog), worker.clone()).await?;
                    Ok(true)
                }=>result,
            }
        });
        let outcome=match &result {Ok(true)=>Outcome::Persisted,Ok(false)=>Outcome::NotApplicable,Err(reason)=>Outcome::Unavailable {reason:reason.clone()}};
        let _=send.send(outcome);
        // Cancellation is a settled lifecycle outcome, not a shutdown failure.
        match result {Err(reason) if reason=="cancelled"=>Ok(()),other=>other.map(|_|())}
    }).map_err(|_|"checkpoint_worker_unavailable".to_string())?;
    feature.checkpoint_task = Some(SessionEndTask {
        cancellation,
        handle,
    });
    feature.checkpoint_turns = 0;
    Ok(receive)
}

type CapturedPage = (
    SessionEndPipelineInput,
    String,
    omegon_memory::EpisodeFormation,
    omegon_memory::MemoryMutationOutcome,
);

pub(super) async fn drain(
    binding: &crate::session_consumers::DeferredSessionViewBinding,
    target: &crate::session_consumers::SessionViewTarget,
    input: SessionEndPipelineInput,
    backlog: Option<Arc<AtomicBool>>,
    cancellation: tokio_util::sync::CancellationToken,
) -> Result<Vec<CapturedPage>, String> {
    struct ResetBacklog(Option<Arc<AtomicBool>>);
    impl Drop for ResetBacklog {
        fn drop(&mut self) {
            if let Some(backlog) = &self.0 {
                backlog.store(false, Ordering::SeqCst);
            }
        }
    }
    let backlog = ResetBacklog(backlog);
    let mut write_retries = 0;
    loop {
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            capture_pass(binding, target, input.clone(), &cancellation),
        )
        .await
        {
            Ok(Err(reason)) if reason == "capture_backlog" => {
                if let Some(backlog) = &backlog.0 {
                    backlog.store(true, Ordering::SeqCst);
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await
            }
            Ok(Err(reason)) if reason == "checkpoint_store_failed" && write_retries < 3 => {
                // A concurrent capture can win the cursor CAS. Reread the durable
                // cursor on retry; bounded retries also cover transient storage failure.
                write_retries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Ok(result) => return result,
            Err(_) => return Err("checkpoint_store_timed_out".into()),
        }
    }
}

pub(super) async fn capture_pass(
    binding: &crate::session_consumers::DeferredSessionViewBinding,
    target: &crate::session_consumers::SessionViewTarget,
    mut input: SessionEndPipelineInput,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<CapturedPage>, String> {
    let snapshot =
        formation::CaptureSnapshot::load(binding, target, &input.session_id, cancellation)?;
    let first = snapshot
        .page(None)?
        .ok_or("checkpoint_source_unavailable")?;
    input.evidence = first;
    let (_, request) = formation_episode_request(&input);
    let key =
        omegon_memory::formation::capture_key(&request).map_err(|_| "checkpoint_source_invalid")?;
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let response = input
        .memory_binding
        .invoke(crate::memory_service::MemoryRequestV1::FormationCursor {
            scope: crate::memory_service::MemoryScopeV1::Project,
            key,
            cancellation,
        })
        .await
        .map_err(|_| "checkpoint_cursor_unavailable")?;
    let crate::memory_service::MemoryPayloadV1::FormationCursor(mut cursor) = response.payload
    else {
        return Err("checkpoint_response_invalid".into());
    };
    let mut pages = Vec::new();
    for _ in 0..8 {
        let Some(page) = snapshot.page(cursor.as_ref())? else {
            return Ok(pages);
        };
        input.evidence = page;
        let (next, captured) = persist(&input, cursor).await?;
        cursor = Some(next);
        pages.push(captured);
    }
    if snapshot.page(cursor.as_ref())?.is_none() {
        Ok(pages)
    } else {
        Err("capture_backlog".into())
    }
}

async fn persist(
    input: &SessionEndPipelineInput,
    expected: Option<omegon_memory::FormationCursor>,
) -> Result<(omegon_memory::FormationCursor, CapturedPage), String> {
    let (operation_id, request) = formation_episode_request(input);
    let evidence = request
        .formation
        .as_deref()
        .ok_or("checkpoint_source_invalid")?
        .clone();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let response = input
        .memory_binding
        .invoke(crate::memory_service::MemoryRequestV1::ApplyMutation {
            scope: crate::memory_service::MemoryScopeV1::Project,
            operation_id: operation_id.clone(),
            mutation: MemoryMutation::StoreCoveragePage { request, expected },
            cancellation,
        })
        .await
        .map_err(|_| "checkpoint_store_failed".to_string())?;
    match response.payload {
        crate::memory_service::MemoryPayloadV1::Mutation(stored) => {
            let MemoryMutationEffect::CoverageStored { cursor, .. } = &stored.effect else {
                return Err("checkpoint_response_invalid".into());
            };
            Ok((
                cursor.clone(),
                (input.clone(), operation_id, evidence, stored),
            ))
        }
        _ => Err("checkpoint_response_invalid".into()),
    }
}

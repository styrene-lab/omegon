//! Owned interval and awaited pre-eviction capture. Canonical replay owns sources.
use super::*;
use omegon_traits::ContextCheckpointOutcome as Outcome;
use tokio::sync::oneshot;

const TURN_INTERVAL: u32 = 8;

pub(super) fn status(feature: &MemoryFeature) -> serde_json::Value {
    serde_json::json!({"interval_turns":TURN_INTERVAL,"turns_since_request":feature.checkpoint_turns,"capture_due":feature.checkpoint_turns>=TURN_INTERVAL,"worker_running":feature.checkpoint_task.as_ref().is_some_and(|task|!task.handle.is_finished()),"last_pre_eviction":feature.last_pre_eviction})
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
    let (send, receive) = oneshot::channel();
    let handle=std::thread::Builder::new().name("memory-checkpoint".into()).spawn(move|| {
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"checkpoint_runtime_failed".to_string())?;
        let result:Result<bool,String>=runtime.block_on(async {
            tokio::select! {
                biased;
                _=worker.cancelled()=>Err("cancelled".into()),
                result=tokio::time::timeout(std::time::Duration::from_secs(10),async {
                    let evidence=formation::capture(Some(&binding),Some(&target),&session_id);
                    if !matches!(evidence.source,omegon_memory::FormationSource::Available {..}) {return Err("checkpoint_source_unavailable".into());}
                    if evidence.evidence.is_empty() {return Ok(false);}
                    persist(SessionEndPipelineInput {mind,memory_binding,extractor,evidence,session_id,status_root,turns:0,tool_calls:0,duration_secs:0.0}).await?;
                    Ok(true)
                })=>result.unwrap_or_else(|_|Err("checkpoint_store_timed_out".into())),
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

async fn persist(input: SessionEndPipelineInput) -> Result<(), String> {
    let (operation_id, request) = formation_episode_request(&input);
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let response = input
        .memory_binding
        .invoke(crate::memory_service::MemoryRequestV1::ApplyMutation {
            scope: crate::memory_service::MemoryScopeV1::Project,
            operation_id,
            mutation: MemoryMutation::StoreEpisode { request },
            cancellation,
        })
        .await
        .map_err(|_| "checkpoint_store_failed".to_string())?;
    if !matches!(
        response.payload,
        crate::memory_service::MemoryPayloadV1::Mutation(_)
    ) {
        return Err("checkpoint_response_invalid".into());
    }
    Ok(())
}

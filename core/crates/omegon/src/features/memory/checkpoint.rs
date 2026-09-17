//! Interval capture admission. The canonical session log remains the source owner.
use super::*;

const TURN_INTERVAL: u32 = 8;

pub(super) fn status(feature: &MemoryFeature) -> serde_json::Value {
    serde_json::json!({"interval_turns":TURN_INTERVAL,"turns_since_request":feature.checkpoint_turns,"capture_due":feature.checkpoint_turns>=TURN_INTERVAL,"worker_running":feature.checkpoint_task.as_ref().is_some_and(|task|!task.handle.is_finished())})
}

pub(super) fn on_turn(feature: &mut MemoryFeature) {
    if !feature.memory_binding.available() || !feature.session_end_tasks.lock().unwrap().accepting {
        return;
    }
    feature.checkpoint_turns = feature
        .checkpoint_turns
        .saturating_add(1)
        .min(TURN_INTERVAL);
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
    // Coalesce pressure into the due counter, not an unbounded task queue.
    if feature.checkpoint_turns < TURN_INTERVAL || feature.checkpoint_task.is_some() {
        return;
    }
    let Some(session_id) = feature.session_id.lock().unwrap().clone() else {
        return;
    };
    let Some(binding) = feature.session_binding.clone() else {
        return;
    };
    let Some(target) = binding.snapshot() else {
        return;
    };
    let memory_binding = feature.memory_binding.clone();
    let mind = feature.mind.clone();
    let extractor = feature.extractor.clone();
    let status_root = feature.status_root.clone();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let worker = cancellation.clone();
    let handle=std::thread::Builder::new().name("memory-checkpoint".into()).spawn(move|| {
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|"checkpoint_runtime_failed".to_string())?;
        runtime.block_on(async {
            tokio::select! {
                biased;
                _=worker.cancelled()=>Ok(()),
                result=tokio::time::timeout(std::time::Duration::from_secs(10),async {
                    let evidence=formation::capture(Some(&binding),Some(&target),&session_id);
                    if !matches!(evidence.source,omegon_memory::FormationSource::Available {..}) {return Err("checkpoint_source_unavailable".into());}
                    if evidence.evidence.is_empty() {return Ok(());}
                    persist(SessionEndPipelineInput {mind,memory_binding,extractor,evidence,session_id,status_root,turns:0,tool_calls:0,duration_secs:0.0}).await
                })=>result.unwrap_or_else(|_|Err("checkpoint_store_timed_out".into())),
            }
        })
    });
    match handle {
        Ok(handle) => {
            feature.checkpoint_task = Some(SessionEndTask {
                cancellation,
                handle,
            });
            feature.checkpoint_turns = 0;
        }
        Err(_) => tracing::warn!("memory checkpoint worker unavailable; capture remains due"),
    }
}

async fn persist(input: SessionEndPipelineInput) -> Result<(), String> {
    let (operation_id, request) = formation_episode_request(&input);
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    // Use the same capture identity as finalization. An identical frontier replays
    // its receipt; interval capture never runs inference or vault synchronization.
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

//! One owned finalization worker with a bounded source-bound admission queue.
use super::*;
use crate::session_consumers::{DeferredSessionViewBinding, SessionViewBinding, SessionViewTarget};

const CAPACITY: usize = 8;

pub(super) enum Message {
    Finish(Box<SessionEndPipelineInput>, Option<SessionViewTarget>),
    #[cfg(test)]
    Barrier(tokio::sync::oneshot::Sender<()>),
}

pub(super) fn enqueue(
    feature: &mut MemoryFeature,
    input: SessionEndPipelineInput,
    target: Option<SessionViewTarget>,
) {
    let mut tasks = feature.session_end_tasks.lock().unwrap();
    if !tasks.accepting {
        return;
    }
    if feature.finalization_sender.is_none() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(CAPACITY);
        let cancellation = tokio_util::sync::CancellationToken::new();
        let worker = cancellation.clone();
        let active = feature.finalization_running.clone();
        let handle = std::thread::Builder::new().name("memory-finalization".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "finalization_runtime_failed".to_string())?;
            runtime.block_on(async {
                loop {
                    let message = tokio::select! { biased; _=worker.cancelled()=>break, message=receiver.recv()=>message };
                    let Some(message) = message else { break; };
                    match message {
                        #[cfg(test)]
                        Message::Barrier(done) => { let _ = done.send(()); }
                        Message::Finish(input, target) => {
                            struct Active(Arc<AtomicBool>);
                            impl Drop for Active { fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); } }
                            active.store(true, Ordering::SeqCst);
                            let _active = Active(active.clone());
                            tokio::select! { biased; _=worker.cancelled()=>break, _=finish(*input, target, worker.clone())=>{} }
                        }
                    }
                }
            });
            Ok(())
        });
        match handle {
            Ok(handle) => {
                tasks.tasks.push(SessionEndTask {
                    cancellation,
                    handle,
                });
                feature.finalization_sender = Some(sender);
            }
            Err(_) => {
                tasks
                    .failures
                    .push("finalization_worker_unavailable".into());
                return;
            }
        }
    }
    if feature
        .finalization_sender
        .as_ref()
        .expect("started finalization worker")
        .try_send(Message::Finish(Box::new(input), target))
        .is_err()
    {
        feature.checkpoint_turns = 8;
        tracing::warn!(
            "memory finalization queue full or closed; unacknowledged source remains in canonical storage"
        );
    }
}

async fn finish(
    input: SessionEndPipelineInput,
    target: Option<SessionViewTarget>,
    cancellation: tokio_util::sync::CancellationToken,
) {
    let Some(target) = target else {
        run_session_end_pipeline(input).await;
        return;
    };
    // SessionEnd refers to this ended session, even if the active UI has advanced.
    // Pin its path/session/stream rather than following the mutable current binding.
    let pinned = SessionViewBinding::new(target.snapshot, target.session_id);
    let mut pinned_target = pinned.snapshot();
    pinned_target.stream_id = target.stream_id;
    pinned_target.kind = target.kind;
    pinned.replace(pinned_target);
    let binding = DeferredSessionViewBinding::default();
    binding.bind(pinned);
    let target = binding.snapshot().expect("pinned finalization source");
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        checkpoint::drain(&binding, &target, input.clone(), None, cancellation),
    )
    .await
    {
        Ok(Ok(pages)) => {
            if pages.is_empty() {
                sync_session_end(&input).await;
            }
            for (input, operation_id, evidence, stored) in pages {
                finish_session_episode(input, operation_id, evidence, stored).await;
            }
        }
        _ => tracing::warn!("session capture incomplete; canonical source remains recoverable"),
    }
}

pub(super) fn queued(feature: &MemoryFeature) -> usize {
    feature
        .finalization_sender
        .as_ref()
        .map_or(0, |sender| CAPACITY - sender.capacity())
}

#[cfg(test)]
pub(super) async fn flush(feature: &MemoryFeature) {
    if let Some(sender) = &feature.finalization_sender {
        let (done, receiver) = tokio::sync::oneshot::channel();
        sender.send(Message::Barrier(done)).await.unwrap();
        receiver.await.unwrap();
    }
}

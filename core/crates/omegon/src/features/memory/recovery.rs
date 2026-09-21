//! Bounded background recovery from durable evidence; no session replay is required.
use super::*;
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const PASS_BUDGET: Duration = Duration::from_secs(120);
const IDLE_SECONDS: u64 = 60;
const MAX_BACKOFF_SECONDS: u64 = 900;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    NotStarted,
    Running,
    Idle,
    Backlog,
    Backoff,
    Stopped,
}

/// Runtime observation, not durable work state. Queue samples cover only this
/// feature's mind/model; a full sample is a lower bound, not a total count.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct Status {
    phase: Phase,
    pub passes: u64,
    consecutive_failures: u32,
    scheduled_delay_seconds: Option<u64>,
    pending_sample: Option<usize>,
    pending_sample_capped: bool,
    last_failure: Option<&'static str>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            phase: Phase::NotStarted,
            passes: 0,
            consecutive_failures: 0,
            scheduled_delay_seconds: None,
            pending_sample: None,
            pending_sample_capped: false,
            last_failure: None,
        }
    }
}

#[cfg(test)]
impl Status {
    pub(super) fn stopped(&self) -> bool {
        self.phase == Phase::Stopped
    }
}

pub(super) async fn run(
    binding: crate::memory_service::MemoryBinding,
    mind: String,
    extractor: Arc<dyn formation::Extractor>,
    cancellation: CancellationToken,
    status: watch::Sender<Status>,
    root: std::path::PathBuf,
) {
    run_loop(cancellation, status, || {
        recover(&binding, &mind, &extractor, &root)
    })
    .await;
}

async fn run_loop<F, Fut>(
    cancellation: CancellationToken,
    status: watch::Sender<Status>,
    mut pass: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<usize>>,
{
    loop {
        if cancellation.is_cancelled() {
            break;
        }
        status.send_modify(|state| {
            state.phase = Phase::Running;
            state.scheduled_delay_seconds = None;
            state.pending_sample = None;
            state.pending_sample_capped = false;
        });
        let result = tokio::select! {
            biased;
            _=cancellation.cancelled()=>break,
            result=tokio::time::timeout(PASS_BUDGET,pass())=>result,
        };
        let mut delay = IDLE_SECONDS;
        status.send_modify(|state| {
            state.passes = state.passes.saturating_add(1);
            match result {
                Ok(Ok(pending)) => {
                    state.consecutive_failures = 0;
                    state.last_failure = None;
                    state.pending_sample = Some(pending);
                    state.pending_sample_capped =
                        pending == omegon_memory::formation::MAX_RECOVERY_BATCH;
                    state.phase = if pending == 0 {
                        Phase::Idle
                    } else {
                        Phase::Backlog
                    };
                    if pending > 0 {
                        delay = 1;
                    }
                }
                error => {
                    state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                    state.phase = Phase::Backoff;
                    state.last_failure = Some(if error.is_err() {
                        "pass_timed_out"
                    } else {
                        "pass_failed"
                    });
                    delay = (IDLE_SECONDS.saturating_mul(
                        1u64 << state.consecutive_failures.saturating_sub(1).min(4),
                    ))
                    .min(MAX_BACKOFF_SECONDS);
                }
            }
            state.scheduled_delay_seconds = Some(delay);
        });
        tokio::select! {
            biased;
            _=cancellation.cancelled()=>break,
            _=tokio::time::sleep(Duration::from_secs(delay))=>{},
        }
    }
    status.send_modify(|state| {
        state.phase = Phase::Stopped;
        state.scheduled_delay_seconds = None;
    });
}

async fn inventory(
    binding: &crate::memory_service::MemoryBinding,
    mind: &str,
    model: &str,
    cancellation: CancellationToken,
) -> anyhow::Result<Vec<omegon_memory::Episode>> {
    use crate::memory_service::{MemoryPayloadV1, MemoryRequestV1, MemoryScopeV1};
    let response = binding
        .invoke(MemoryRequestV1::PendingFormations {
            scope: MemoryScopeV1::Project,
            mind: mind.into(),
            model: model.into(),
            limit: omegon_memory::formation::MAX_RECOVERY_BATCH,
            cancellation,
        })
        .await
        .map_err(MemoryFeatureInvokeError)?;
    let MemoryPayloadV1::Episodes(episodes) = response.payload else {
        anyhow::bail!("unexpected formation inventory response");
    };
    Ok(episodes)
}

async fn recover(
    binding: &crate::memory_service::MemoryBinding,
    mind: &str,
    extractor: &Arc<dyn formation::Extractor>,
    root: &std::path::Path,
) -> anyhow::Result<usize> {
    use crate::memory_service::{MemoryRequestV1, MemoryScopeV1};
    let cancellation = CancellationToken::new();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let episodes = inventory(binding, mind, extractor.model(), cancellation.clone()).await?;
    for episode in episodes {
        let evidence = episode
            .formation
            .ok_or_else(|| anyhow::anyhow!("missing pending formation"))?;
        evidence.validate()?;
        let completed = formation::extract_observed(*evidence, Some(extractor), root).await;
        // Atomic completion rejects changed evidence and a concurrent terminal winner.
        binding
            .invoke(MemoryRequestV1::ApplyMutation {
                scope: MemoryScopeV1::Project,
                operation_id: format!("formation-recovery:v1:{}", episode.id),
                mutation: MemoryMutation::CompleteFormation {
                    episode_id: episode.id,
                    formation: Box::new(completed),
                },
                cancellation: cancellation.clone(),
            })
            .await
            .map_err(MemoryFeatureInvokeError)?;
    }
    Ok(
        inventory(binding, mind, extractor.model(), cancellation.clone())
            .await?
            .len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[tokio::test(start_paused = true)]
    async fn scheduler_discovers_later_work_without_idle_hot_polling() {
        let (status, mut observed) = watch::channel(Status::default());
        let cancellation = CancellationToken::new();
        let pending = Arc::new(AtomicUsize::new(0));
        let queue = pending.clone();
        let worker = tokio::spawn(run_loop(cancellation.clone(), status, move || {
            let left = queue
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                    Some(count.saturating_sub(8))
                })
                .unwrap()
                .saturating_sub(8);
            async move { Ok(left.min(8)) }
        }));
        observed.wait_for(|state| state.passes == 1).await.unwrap();
        pending.store(9, Ordering::SeqCst);
        tokio::time::advance(Duration::from_secs(59)).await;
        tokio::task::yield_now().await;
        assert_eq!(pending.load(Ordering::SeqCst), 9);
        tokio::time::advance(Duration::from_secs(1)).await;
        let backlog = observed
            .wait_for(|state| state.passes == 2)
            .await
            .unwrap()
            .clone();
        assert_eq!(backlog.phase, Phase::Backlog);
        assert_eq!(backlog.pending_sample, Some(1));
        assert_eq!(backlog.scheduled_delay_seconds, Some(1));
        tokio::time::advance(Duration::from_secs(1)).await;
        let idle = observed
            .wait_for(|state| state.passes == 3)
            .await
            .unwrap()
            .clone();
        assert_eq!(idle.phase, Phase::Idle);
        assert_eq!(pending.load(Ordering::SeqCst), 0);
        cancellation.cancel();
        worker.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_backoff_is_bounded_and_cancellation_interrupts_wait() {
        let (status, mut observed) = watch::channel(Status::default());
        let cancellation = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let worker = tokio::spawn(run_loop(cancellation.clone(), status, move || {
            counted.fetch_add(1, Ordering::SeqCst);
            async { anyhow::bail!("storage unavailable") }
        }));
        for (index, delay) in [60, 120, 240, 480, 900, 900].into_iter().enumerate() {
            let snapshot = observed
                .wait_for(|state| state.passes == (index + 1) as u64)
                .await
                .unwrap()
                .clone();
            assert_eq!(snapshot.phase, Phase::Backoff);
            assert_eq!(snapshot.scheduled_delay_seconds, Some(delay));
            tokio::time::advance(Duration::from_secs(delay - 1)).await;
            tokio::task::yield_now().await;
            assert_eq!(calls.load(Ordering::SeqCst), index + 1);
            if index < 5 {
                tokio::time::advance(Duration::from_secs(1)).await;
            }
        }
        cancellation.cancel();
        worker.await.unwrap();
        assert_eq!(observed.borrow().phase, Phase::Stopped);
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_timeout_drops_work_and_success_resets_backoff() {
        let (status, mut observed) = watch::channel(Status::default());
        let cancellation = CancellationToken::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let marker = dropped.clone();
        let mut calls = 0;
        let worker = tokio::spawn(run_loop(cancellation.clone(), status, move || {
            calls += 1;
            let first = calls == 1;
            let marker = marker.clone();
            async move {
                if first {
                    struct Guard(Arc<AtomicBool>);
                    impl Drop for Guard {
                        fn drop(&mut self) {
                            self.0.store(true, Ordering::SeqCst);
                        }
                    }
                    let _guard = Guard(marker);
                    std::future::pending::<()>().await;
                }
                Ok(0)
            }
        }));
        observed
            .wait_for(|state| state.phase == Phase::Running)
            .await
            .unwrap();
        tokio::time::advance(PASS_BUDGET).await;
        let failed = observed
            .wait_for(|state| state.passes == 1)
            .await
            .unwrap()
            .clone();
        assert_eq!(failed.last_failure, Some("pass_timed_out"));
        assert!(dropped.load(Ordering::SeqCst));
        tokio::time::advance(Duration::from_secs(60)).await;
        let idle = observed
            .wait_for(|state| state.passes == 2)
            .await
            .unwrap()
            .clone();
        assert_eq!(idle.phase, Phase::Idle);
        assert_eq!(idle.consecutive_failures, 0);
        assert_eq!(idle.pending_sample, Some(0));
        cancellation.cancel();
        worker.await.unwrap();
    }
}

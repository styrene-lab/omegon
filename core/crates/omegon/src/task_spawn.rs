use futures_util::FutureExt;
use omegon_traits::AgentEvent;
use std::future::Future;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

#[derive(Debug, Clone)]
pub struct OperatorTaskOptions {
    pub panic_notification_prefix: String,
}

impl Default for OperatorTaskOptions {
    fn default() -> Self {
        Self {
            panic_notification_prefix:
                "⚠ Background task crashed — work may not have completed safely".to_string(),
        }
    }
}

pub fn spawn_infra<Fut>(name: &'static str, fut: Fut) -> JoinHandle<()>
where
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(err) = fut.await {
            warn!(task = name, error = %err, "background infrastructure task failed");
        }
    })
}

pub fn spawn_best_effort<Fut>(name: &'static str, fut: Fut) -> JoinHandle<()>
where
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        fut.await;
        debug!(task = name, "best-effort background task completed");
    })
}

pub fn spawn_best_effort_result<Fut>(name: &'static str, fut: Fut) -> JoinHandle<()>
where
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(err) = fut.await {
            debug!(task = name, error = %err, "best-effort background task failed");
        }
    })
}

pub fn spawn_operator_task<Fut>(
    name: &'static str,
    events_tx: broadcast::Sender<AgentEvent>,
    options: OperatorTaskOptions,
    fut: Fut,
) -> JoinHandle<()>
where
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let task = std::panic::AssertUnwindSafe(fut).catch_unwind().await;
        match task {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!(task = name, error = %err, "operator background task failed");
            }
            Err(panic_payload) => {
                let panic_text = panic_payload_text(&panic_payload);
                error!(task = name, %panic_text, "operator background task panicked");
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: format!("{}: {panic_text}", options.panic_notification_prefix),
                });
            }
        }
    })
}

pub fn spawn_local_operator_task<Fut>(
    name: &'static str,
    events_tx: broadcast::Sender<AgentEvent>,
    options: OperatorTaskOptions,
    fut: Fut,
) -> JoinHandle<()>
where
    Fut: Future<Output = anyhow::Result<()>> + 'static,
{
    tokio::task::spawn_local(async move {
        let task = std::panic::AssertUnwindSafe(fut).catch_unwind().await;
        match task {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!(task = name, error = %err, "local operator background task failed");
            }
            Err(panic_payload) => {
                let panic_text = panic_payload_text(&panic_payload);
                error!(task = name, %panic_text, "local operator background task panicked");
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: format!("{}: {panic_text}", options.panic_notification_prefix),
                });
            }
        }
    })
}

fn panic_payload_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;
    use std::sync::{Mutex, OnceLock};

    fn panic_hook_guard() -> std::sync::MutexGuard<'static, ()> {
        static HOOK_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        HOOK_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("panic hook lock")
    }

    #[tokio::test]
    async fn operator_task_panic_emits_notification() {
        let _guard = panic_hook_guard();
        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));

        let (events_tx, mut events_rx) = broadcast::channel(4);
        let handle = spawn_operator_task(
            "panic-test",
            events_tx,
            OperatorTaskOptions {
                panic_notification_prefix: "panic prefix".to_string(),
            },
            async move {
                panic!("boom");
                #[allow(unreachable_code)]
                Ok(())
            },
        );

        handle.await.expect("join");
        panic::set_hook(previous_hook);
        let event = events_rx.recv().await.expect("notification");
        match event {
            AgentEvent::SystemNotification { message } => {
                assert!(message.contains("panic prefix"));
                assert!(message.contains("boom"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_operator_task_panic_emits_notification() {
        let _guard = panic_hook_guard();
        let previous_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (events_tx, mut events_rx) = broadcast::channel(4);
                let handle = spawn_local_operator_task(
                    "local-panic-test",
                    events_tx,
                    OperatorTaskOptions {
                        panic_notification_prefix: "local prefix".to_string(),
                    },
                    async move {
                        panic!("local boom");
                        #[allow(unreachable_code)]
                        Ok(())
                    },
                );

                handle.await.expect("join");
                let event = events_rx.recv().await.expect("notification");
                match event {
                    AgentEvent::SystemNotification { message } => {
                        assert!(message.contains("local prefix"));
                        assert!(message.contains("local boom"));
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            })
            .await;

        panic::set_hook(previous_hook);
    }
}

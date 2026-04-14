//! Daemon session router — per-caller session multiplexing with bounded concurrency.
//!
//! Routes incoming daemon events to per-caller sessions keyed by
//! `(source_user, source_channel, source_thread)`. When no identity metadata
//! is present, all events fold to a singleton fallback session for backward
//! compatibility with pre-0.16 clients.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, info, warn};

/// Default maximum number of concurrent sessions that may be actively processing.
const DEFAULT_MAX_CONCURRENT_SESSIONS: usize = 8;

/// Default idle timeout after which a parked session is reaped.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// Default interval for the background reaper sweep.
const DEFAULT_REAPER_INTERVAL: Duration = Duration::from_secs(30);

/// Identity key for routing daemon events to per-caller sessions.
///
/// When all fields are `None`, this produces a singleton fallback key so that
/// legacy clients without identity metadata share a single session.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CallerIdentity {
    pub source_user: Option<String>,
    pub source_channel: Option<String>,
    pub source_thread: Option<String>,
}

impl CallerIdentity {
    pub fn new(
        source_user: Option<String>,
        source_channel: Option<String>,
        source_thread: Option<String>,
    ) -> Self {
        Self {
            source_user,
            source_channel,
            source_thread,
        }
    }

    /// Returns `true` when all identity fields are `None`, meaning this caller
    /// will be routed to the singleton fallback session.
    pub fn is_anonymous(&self) -> bool {
        self.source_user.is_none()
            && self.source_channel.is_none()
            && self.source_thread.is_none()
    }

    /// A human-readable label for logging.
    pub fn display_key(&self) -> String {
        if self.is_anonymous() {
            return "<fallback>".to_string();
        }
        let parts: Vec<&str> = [
            self.source_user.as_deref(),
            self.source_channel.as_deref(),
            self.source_thread.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        parts.join("/")
    }
}

/// Message routed to a per-caller session.
///
/// The integration layer wraps the dispatched `WebCommand` (or equivalent)
/// into this envelope so the session task can process it.
#[derive(Debug)]
pub struct SessionMessage {
    pub event_id: String,
    pub payload: SessionPayload,
}

/// The actual payload dispatched to a session.
#[derive(Debug)]
pub enum SessionPayload {
    /// A user prompt to be run through the agent loop.
    Prompt { text: String },
    /// A slash command.
    SlashCommand {
        name: String,
        args: String,
    },
    /// Cancel the currently running turn in this session.
    Cancel,
    /// Request a new session (reset conversation state).
    NewSession,
}

/// Handle to a live session — holds the sender side of the per-session channel
/// and tracks activity for idle-timeout reaping.
#[derive(Debug)]
struct SessionEntry {
    tx: mpsc::Sender<SessionMessage>,
    last_activity: Instant,
    identity: CallerIdentity,
}

/// Configuration for the session router.
#[derive(Debug, Clone)]
pub struct SessionRouterConfig {
    /// Maximum concurrent sessions allowed to be actively processing.
    pub max_concurrent_sessions: usize,
    /// Duration of inactivity after which a session is parked (reaped).
    pub idle_timeout: Duration,
    /// How often the background reaper checks for idle sessions.
    pub reaper_interval: Duration,
    /// Per-session channel buffer size.
    pub session_channel_buffer: usize,
}

impl Default for SessionRouterConfig {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: DEFAULT_MAX_CONCURRENT_SESSIONS,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            reaper_interval: DEFAULT_REAPER_INTERVAL,
            session_channel_buffer: 32,
        }
    }
}

/// A routed session handle returned by `SessionRouter::route()`.
///
/// The caller sends `SessionMessage`s through this handle. The session router
/// manages the lifecycle of the underlying session task.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    tx: mpsc::Sender<SessionMessage>,
    pub identity: CallerIdentity,
}

impl SessionHandle {
    /// Send a message to this session. Returns `Err` if the session has been reaped.
    pub async fn send(&self, msg: SessionMessage) -> Result<(), mpsc::error::SendError<SessionMessage>> {
        self.tx.send(msg).await
    }
}

/// Daemon session router — multiplexes incoming events across per-caller sessions
/// with semaphore-bounded concurrency and idle-timeout reaping.
pub struct SessionRouter {
    sessions: Arc<Mutex<HashMap<CallerIdentity, SessionEntry>>>,
    semaphore: Arc<Semaphore>,
    config: SessionRouterConfig,
}

impl SessionRouter {
    pub fn new(config: SessionRouterConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_sessions));
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            semaphore,
            config,
        }
    }

    /// Route an event to the session for the given caller identity.
    ///
    /// If a session already exists and its channel is still open, returns the
    /// existing handle (updating last-activity). Otherwise creates a new session
    /// and returns a fresh handle along with the receiver end so the caller can
    /// spawn the session task.
    ///
    /// # Returns
    ///
    /// `RouteResult::Existing` — reuse an active session's sender.
    /// `RouteResult::New` — a new session was created; the caller must spawn a
    /// task that reads from the returned `mpsc::Receiver<SessionMessage>`.
    pub fn route(&self, identity: CallerIdentity) -> RouteResult {
        let mut sessions = self.sessions.lock().expect("session map lock poisoned");

        // Check for an existing live session
        if let Some(entry) = sessions.get_mut(&identity) {
            if !entry.tx.is_closed() {
                entry.last_activity = Instant::now();
                debug!(
                    caller = %identity.display_key(),
                    "routing to existing session"
                );
                return RouteResult::Existing(SessionHandle {
                    tx: entry.tx.clone(),
                    identity,
                });
            }
            // Channel closed — session task has exited; remove stale entry
            debug!(
                caller = %identity.display_key(),
                "removing stale session (channel closed)"
            );
            sessions.remove(&identity);
        }

        // Create a new session
        let (tx, rx) = mpsc::channel(self.config.session_channel_buffer);
        let handle = SessionHandle {
            tx: tx.clone(),
            identity: identity.clone(),
        };

        sessions.insert(
            identity.clone(),
            SessionEntry {
                tx,
                last_activity: Instant::now(),
                identity: identity.clone(),
            },
        );

        info!(
            caller = %identity.display_key(),
            active_sessions = sessions.len(),
            "created new session"
        );

        RouteResult::New {
            handle,
            rx,
            semaphore: Arc::clone(&self.semaphore),
        }
    }

    /// Returns a clone of the concurrency semaphore for use by session tasks.
    pub fn semaphore(&self) -> Arc<Semaphore> {
        Arc::clone(&self.semaphore)
    }

    /// Number of currently tracked sessions (including potentially stale ones).
    pub fn active_session_count(&self) -> usize {
        self.sessions
            .lock()
            .expect("session map lock poisoned")
            .len()
    }

    /// Remove a specific session by identity. Used when a session task exits.
    pub fn remove_session(&self, identity: &CallerIdentity) {
        let mut sessions = self.sessions.lock().expect("session map lock poisoned");
        if sessions.remove(identity).is_some() {
            debug!(
                caller = %identity.display_key(),
                remaining = sessions.len(),
                "session removed"
            );
        }
    }

    /// Sweep idle sessions whose last activity exceeds the configured timeout.
    /// Returns the identities of reaped sessions.
    pub fn reap_idle_sessions(&self) -> Vec<CallerIdentity> {
        let mut sessions = self.sessions.lock().expect("session map lock poisoned");
        let now = Instant::now();
        let timeout = self.config.idle_timeout;

        let idle_keys: Vec<CallerIdentity> = sessions
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_activity) > timeout)
            .map(|(key, _)| key.clone())
            .collect();

        for key in &idle_keys {
            if let Some(entry) = sessions.remove(key) {
                info!(
                    caller = %entry.identity.display_key(),
                    idle_secs = now.duration_since(entry.last_activity).as_secs(),
                    "parking idle session"
                );
                // Dropping the sender closes the channel, signaling the session
                // task to wind down gracefully.
                drop(entry);
            }
        }

        idle_keys
    }

    /// Spawn a background task that periodically reaps idle sessions.
    /// Returns a `JoinHandle` so the caller can abort it on shutdown.
    pub fn spawn_reaper(&self) -> tokio::task::JoinHandle<()> {
        let sessions = Arc::clone(&self.sessions);
        let config = self.config.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.reaper_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let reaped = reap_idle_from_map(&sessions, config.idle_timeout);
                if !reaped.is_empty() {
                    info!(count = reaped.len(), "reaper swept idle sessions");
                }
            }
        })
    }
}

/// Result of routing an event to a session.
pub enum RouteResult {
    /// An existing session was found and is still active.
    Existing(SessionHandle),
    /// A new session was created. The caller must spawn a task to consume
    /// messages from `rx`. The `semaphore` permit should be acquired before
    /// processing each turn to enforce bounded concurrency.
    New {
        handle: SessionHandle,
        rx: mpsc::Receiver<SessionMessage>,
        semaphore: Arc<Semaphore>,
    },
}

impl RouteResult {
    /// Get the session handle regardless of variant.
    pub fn handle(&self) -> &SessionHandle {
        match self {
            RouteResult::Existing(h) => h,
            RouteResult::New { handle, .. } => handle,
        }
    }
}

/// Standalone reaper function operating on the shared map — used by the
/// background reaper task to avoid holding `&self`.
fn reap_idle_from_map(
    sessions: &Arc<Mutex<HashMap<CallerIdentity, SessionEntry>>>,
    idle_timeout: Duration,
) -> Vec<CallerIdentity> {
    let mut map = match sessions.lock() {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "reaper could not lock session map");
            return Vec::new();
        }
    };

    let now = Instant::now();
    let idle_keys: Vec<CallerIdentity> = map
        .iter()
        .filter(|(_, entry)| now.duration_since(entry.last_activity) > idle_timeout)
        .map(|(key, _)| key.clone())
        .collect();

    for key in &idle_keys {
        if let Some(entry) = map.remove(key) {
            info!(
                caller = %entry.identity.display_key(),
                idle_secs = now.duration_since(entry.last_activity).as_secs(),
                "reaper: parking idle session"
            );
        }
    }

    idle_keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_router() -> SessionRouter {
        SessionRouter::new(SessionRouterConfig::default())
    }

    fn identity(user: &str, channel: &str) -> CallerIdentity {
        CallerIdentity::new(
            Some(user.to_string()),
            Some(channel.to_string()),
            None,
        )
    }

    fn anonymous() -> CallerIdentity {
        CallerIdentity::new(None, None, None)
    }

    #[test]
    fn anonymous_identity_is_singleton_fallback() {
        let a = anonymous();
        let b = anonymous();
        assert_eq!(a, b);
        assert!(a.is_anonymous());
        assert_eq!(a.display_key(), "<fallback>");
    }

    #[test]
    fn distinct_callers_get_distinct_keys() {
        let a = identity("alice", "general");
        let b = identity("bob", "general");
        assert_ne!(a, b);
        assert!(!a.is_anonymous());
    }

    #[test]
    fn route_creates_new_session_on_first_call() {
        let router = default_router();
        let id = identity("alice", "general");
        let result = router.route(id);
        assert!(matches!(result, RouteResult::New { .. }));
        assert_eq!(router.active_session_count(), 1);
    }

    #[test]
    fn route_returns_existing_for_same_caller() {
        let router = default_router();
        let id = identity("alice", "general");

        // First call creates
        let first = router.route(id.clone());
        assert!(matches!(first, RouteResult::New { .. }));

        // Second call reuses
        let second = router.route(id);
        assert!(matches!(second, RouteResult::Existing(_)));
        assert_eq!(router.active_session_count(), 1);
    }

    #[test]
    fn route_creates_separate_sessions_for_different_callers() {
        let router = default_router();
        let alice = identity("alice", "general");
        let bob = identity("bob", "general");

        let r1 = router.route(alice);
        let r2 = router.route(bob);
        assert!(matches!(r1, RouteResult::New { .. }));
        assert!(matches!(r2, RouteResult::New { .. }));
        assert_eq!(router.active_session_count(), 2);
    }

    #[test]
    fn anonymous_callers_share_fallback_session() {
        let router = default_router();
        let a = anonymous();
        let b = anonymous();

        let r1 = router.route(a);
        assert!(matches!(r1, RouteResult::New { .. }));

        let r2 = router.route(b);
        assert!(matches!(r2, RouteResult::Existing(_)));
        assert_eq!(router.active_session_count(), 1);
    }

    #[test]
    fn remove_session_decrements_count() {
        let router = default_router();
        let id = identity("alice", "general");
        let _ = router.route(id.clone());
        assert_eq!(router.active_session_count(), 1);

        router.remove_session(&id);
        assert_eq!(router.active_session_count(), 0);
    }

    #[test]
    fn route_replaces_stale_session_when_channel_closed() {
        let router = default_router();
        let id = identity("alice", "general");

        // Create session, then drop the receiver to close channel
        let result = router.route(id.clone());
        if let RouteResult::New { rx, .. } = result {
            drop(rx);
        }

        // Next route should detect closed channel and create new
        let result2 = router.route(id);
        assert!(matches!(result2, RouteResult::New { .. }));
        assert_eq!(router.active_session_count(), 1);
    }

    #[test]
    fn reap_idle_sessions_removes_expired() {
        let config = SessionRouterConfig {
            idle_timeout: Duration::from_millis(1),
            ..SessionRouterConfig::default()
        };
        let router = SessionRouter::new(config);
        let id = identity("alice", "general");
        let _ = router.route(id.clone());

        // Wait for idle timeout
        std::thread::sleep(Duration::from_millis(10));

        let reaped = router.reap_idle_sessions();
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0], id);
        assert_eq!(router.active_session_count(), 0);
    }

    #[test]
    fn reap_idle_sessions_preserves_active() {
        let config = SessionRouterConfig {
            idle_timeout: Duration::from_secs(3600), // 1 hour — won't expire
            ..SessionRouterConfig::default()
        };
        let router = SessionRouter::new(config);
        let id = identity("alice", "general");
        let _ = router.route(id);

        let reaped = router.reap_idle_sessions();
        assert!(reaped.is_empty());
        assert_eq!(router.active_session_count(), 1);
    }

    #[test]
    fn route_result_handle_returns_correct_handle() {
        let router = default_router();
        let id = identity("alice", "general");

        let result = router.route(id.clone());
        let handle = result.handle();
        assert_eq!(handle.identity, id);
    }

    #[test]
    fn semaphore_has_correct_permits() {
        let config = SessionRouterConfig {
            max_concurrent_sessions: 4,
            ..SessionRouterConfig::default()
        };
        let router = SessionRouter::new(config);
        assert_eq!(router.semaphore().available_permits(), 4);
    }

    #[tokio::test]
    async fn session_handle_send_works() {
        let router = default_router();
        let id = identity("alice", "general");
        let result = router.route(id);

        match result {
            RouteResult::New { handle, mut rx, .. } => {
                let msg = SessionMessage {
                    event_id: "evt-1".into(),
                    payload: SessionPayload::Prompt {
                        text: "hello".into(),
                    },
                };
                handle.send(msg).await.unwrap();
                let received = rx.recv().await.unwrap();
                assert_eq!(received.event_id, "evt-1");
                match received.payload {
                    SessionPayload::Prompt { text } => assert_eq!(text, "hello"),
                    other => panic!("unexpected payload: {other:?}"),
                }
            }
            RouteResult::Existing(_) => panic!("expected New"),
        }
    }

    #[tokio::test]
    async fn session_handle_send_fails_after_receiver_dropped() {
        let router = default_router();
        let id = identity("alice", "general");
        let result = router.route(id);

        match result {
            RouteResult::New { handle, rx, .. } => {
                drop(rx);
                let msg = SessionMessage {
                    event_id: "evt-1".into(),
                    payload: SessionPayload::Cancel,
                };
                assert!(handle.send(msg).await.is_err());
            }
            RouteResult::Existing(_) => panic!("expected New"),
        }
    }

    #[test]
    fn display_key_formats_identity_parts() {
        let id = CallerIdentity::new(
            Some("alice".into()),
            Some("general".into()),
            Some("thread-1".into()),
        );
        assert_eq!(id.display_key(), "alice/general/thread-1");

        let partial = CallerIdentity::new(Some("bob".into()), None, Some("t2".into()));
        assert_eq!(partial.display_key(), "bob/t2");
    }

    #[tokio::test]
    async fn reaper_task_can_be_spawned_and_aborted() {
        let config = SessionRouterConfig {
            idle_timeout: Duration::from_millis(1),
            reaper_interval: Duration::from_millis(10),
            ..SessionRouterConfig::default()
        };
        let router = SessionRouter::new(config);
        let id = identity("alice", "general");
        let _ = router.route(id.clone());

        let reaper = router.spawn_reaper();

        // Give the reaper time to run at least once
        tokio::time::sleep(Duration::from_millis(50)).await;
        reaper.abort();

        assert_eq!(router.active_session_count(), 0);
    }
}

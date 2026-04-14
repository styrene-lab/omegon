//! Daemon session router — maps caller identity to per-caller sessions
//! with bounded concurrency and idle-timeout parking.
//!
//! When a `DaemonEventEnvelope` carries `source_user` / `source_channel` /
//! `source_thread` metadata, the router assigns the event to a dedicated
//! session for that caller. Events without identity metadata route to a
//! default session, preserving single-session backward compatibility.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, Semaphore, SemaphorePermit};

use omegon_traits::DaemonEventEnvelope;

use crate::bus::EventBus;
use crate::context::ContextManager;
use crate::conversation::ConversationState;

// ── Caller identity key ────────────────────────────────────────────────────

/// Composite key derived from the identity fields on `DaemonEventEnvelope`.
///
/// Two envelopes that share the same (user, channel, thread) triple route
/// to the same `DaemonSession`.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionKey {
    pub user: Option<String>,
    pub channel: Option<String>,
    pub thread: Option<String>,
}

impl SessionKey {
    /// Build a key from the identity fields of an envelope.
    pub fn from_envelope(env: &DaemonEventEnvelope) -> Self {
        Self {
            user: env.source_user.clone(),
            channel: env.source_channel.clone(),
            thread: env.source_thread.clone(),
        }
    }

    /// `true` when no identity metadata is present — routes to the default
    /// single session for backward compatibility.
    pub fn is_default(&self) -> bool {
        self.user.is_none() && self.channel.is_none() && self.thread.is_none()
    }
}

impl std::fmt::Display for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.user, &self.channel, &self.thread) {
            (None, None, None) => write!(f, "<default>"),
            _ => write!(
                f,
                "{}:{}:{}",
                self.user.as_deref().unwrap_or("*"),
                self.channel.as_deref().unwrap_or("*"),
                self.thread.as_deref().unwrap_or("*"),
            ),
        }
    }
}

// ── Per-caller session state ───────────────────────────────────────────────

/// Mutable state owned by a single caller session. Each field corresponds
/// to a parameter of `loop::run()`.
pub struct DaemonSession {
    pub bus: EventBus,
    pub context_manager: ContextManager,
    pub conversation: ConversationState,
    pub last_activity: Instant,
}

impl DaemonSession {
    /// Touch the session to reset the idle timer.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}

// ── Session handle (shareable across tasks) ────────────────────────────────

/// A clonable handle to a session's mutable state. The inner `Mutex` ensures
/// only one turn executes per session at a time.
pub type SessionHandle = Arc<Mutex<DaemonSession>>;

// ── Router ─────────────────────────────────────────────────────────────────

/// Default maximum concurrent sessions executing turns simultaneously.
const DEFAULT_MAX_CONCURRENT: usize = 8;

/// Default idle timeout before a session is parked.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Routes daemon events to per-caller sessions with bounded concurrency.
///
/// The router does NOT own the default session — that is the pre-existing
/// agent state in `run_embedded_command`. The router only manages sessions
/// for callers that carry identity metadata.
pub struct SessionRouter {
    sessions: Mutex<HashMap<SessionKey, SessionHandle>>,
    semaphore: Arc<Semaphore>,
    idle_timeout: Duration,
}

impl SessionRouter {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT)),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    pub fn with_options(max_concurrent: usize, idle_timeout: Duration) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            idle_timeout,
        }
    }

    /// Retrieve or create a session for the given caller.
    ///
    /// The `factory` closure is called only when a session for this key does
    /// not yet exist. It should return a fresh `DaemonSession` with empty
    /// conversation state and a configured bus + context manager.
    pub async fn get_or_create(
        &self,
        key: &SessionKey,
        factory: impl FnOnce() -> DaemonSession,
    ) -> SessionHandle {
        let mut sessions = self.sessions.lock().await;
        sessions
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(factory())))
            .clone()
    }

    /// Look up an existing session without creating one.
    pub async fn get(&self, key: &SessionKey) -> Option<SessionHandle> {
        let sessions = self.sessions.lock().await;
        sessions.get(key).cloned()
    }

    /// Acquire a concurrency permit. Returns `None` if the semaphore is closed.
    pub async fn acquire_permit(&self) -> Option<SemaphorePermit<'_>> {
        self.semaphore.acquire().await.ok()
    }

    /// Reference to the underlying semaphore for use with `tokio::spawn`
    /// where the permit must be owned.
    pub fn semaphore(&self) -> &Arc<Semaphore> {
        &self.semaphore
    }

    /// Remove sessions that have been idle longer than `idle_timeout`.
    /// Sessions that are currently locked (turn in progress) are never removed.
    pub async fn park_idle_sessions(&self) -> Vec<SessionKey> {
        let mut sessions = self.sessions.lock().await;
        let now = Instant::now();
        let mut parked = Vec::new();

        sessions.retain(|key, handle| {
            // Try-lock: if the session is busy, keep it regardless of age.
            if let Ok(session) = handle.try_lock() {
                if now.duration_since(session.last_activity) >= self.idle_timeout {
                    parked.push(key.clone());
                    return false;
                }
            }
            true
        });

        parked
    }

    /// Number of tracked sessions (excluding the default session).
    pub async fn session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Returns the idle timeout configured for this router.
    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_envelope(
        user: Option<&str>,
        channel: Option<&str>,
        thread: Option<&str>,
    ) -> DaemonEventEnvelope {
        DaemonEventEnvelope {
            event_id: "evt-test".into(),
            source: "test".into(),
            trigger_kind: "prompt".into(),
            payload: serde_json::json!({"text": "hello"}),
            caller_role: None,
            source_user: user.map(Into::into),
            source_channel: channel.map(Into::into),
            source_thread: thread.map(Into::into),
        }
    }

    #[test]
    fn caller_key_default_when_no_identity() {
        let env = make_envelope(None, None, None);
        let key = SessionKey::from_envelope(&env);
        assert!(key.is_default());
    }

    #[test]
    fn caller_key_not_default_with_user() {
        let env = make_envelope(Some("U123"), None, None);
        let key = SessionKey::from_envelope(&env);
        assert!(!key.is_default());
    }

    #[test]
    fn caller_key_equality() {
        let a = make_envelope(Some("U1"), Some("C1"), Some("T1"));
        let b = make_envelope(Some("U1"), Some("C1"), Some("T1"));
        assert_eq!(SessionKey::from_envelope(&a), SessionKey::from_envelope(&b));
    }

    #[test]
    fn caller_key_inequality_on_thread() {
        let a = make_envelope(Some("U1"), Some("C1"), Some("T1"));
        let b = make_envelope(Some("U1"), Some("C1"), Some("T2"));
        assert_ne!(SessionKey::from_envelope(&a), SessionKey::from_envelope(&b));
    }

    #[test]
    fn caller_key_display() {
        let key = SessionKey {
            user: Some("alice".into()),
            channel: None,
            thread: Some("t-1".into()),
        };
        assert_eq!(key.to_string(), "alice:*:t-1");

        let default_key = SessionKey {
            user: None,
            channel: None,
            thread: None,
        };
        assert_eq!(default_key.to_string(), "<default>");
    }

    #[tokio::test]
    async fn router_creates_session_on_first_access() {
        let router = SessionRouter::new();
        let key = SessionKey {
            user: Some("U1".into()),
            channel: None,
            thread: None,
        };
        let session = router
            .get_or_create(&key, || DaemonSession {
                bus: EventBus::new(),
                context_manager: ContextManager::new(String::new(), Vec::new()),
                conversation: ConversationState::new(),
                last_activity: Instant::now(),
            })
            .await;
        assert_eq!(router.session_count().await, 1);
        // Second call returns the same handle
        let session2 = router.get(&key).await.unwrap();
        assert!(Arc::ptr_eq(&session, &session2));
    }

    #[tokio::test]
    async fn router_parks_idle_sessions() {
        let router = SessionRouter::with_options(8, Duration::from_millis(50));
        let key = SessionKey {
            user: Some("U1".into()),
            channel: None,
            thread: None,
        };
        router
            .get_or_create(&key, || DaemonSession {
                bus: EventBus::new(),
                context_manager: ContextManager::new(String::new(), Vec::new()),
                conversation: ConversationState::new(),
                last_activity: Instant::now() - Duration::from_secs(1),
            })
            .await;
        assert_eq!(router.session_count().await, 1);

        let parked = router.park_idle_sessions().await;
        assert_eq!(parked.len(), 1);
        assert_eq!(router.session_count().await, 0);
    }

    #[tokio::test]
    async fn router_does_not_park_active_session() {
        let router = SessionRouter::with_options(8, Duration::from_millis(1));
        let key = SessionKey {
            user: Some("U1".into()),
            channel: None,
            thread: None,
        };
        let handle = router
            .get_or_create(&key, || DaemonSession {
                bus: EventBus::new(),
                context_manager: ContextManager::new(String::new(), Vec::new()),
                conversation: ConversationState::new(),
                last_activity: Instant::now() - Duration::from_secs(10),
            })
            .await;

        // Hold the session lock to simulate an active turn
        let _guard = handle.lock().await;
        tokio::time::sleep(Duration::from_millis(5)).await;

        let parked = router.park_idle_sessions().await;
        assert!(parked.is_empty());
        assert_eq!(router.session_count().await, 1);
    }

    #[tokio::test]
    async fn semaphore_bounds_concurrency() {
        let router = SessionRouter::with_options(2, DEFAULT_IDLE_TIMEOUT);
        let sem = router.semaphore().clone();
        let _p1 = sem.acquire().await.unwrap();
        let _p2 = sem.acquire().await.unwrap();
        // Third acquire should not succeed immediately
        assert!(sem.try_acquire().is_err());
    }
}

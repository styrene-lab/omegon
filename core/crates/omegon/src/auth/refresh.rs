//! Bounded OAuth refresh and process-local coordination. Never retain diagnostics
//! supplied by a token endpoint: those can contain credentials or account data.

use super::OAuthCredentials;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RefreshFailure {
    #[error("OAuth refresh was rejected; reconnect this provider to renew credentials")]
    Rejected,
    #[error("OAuth refresh returned an unusable credential response")]
    InvalidResponse,
    #[error("OAuth refresh is temporarily unavailable; retry shortly")]
    Transient,
    #[error("OAuth refresh is not supported for this provider")]
    Unsupported,
}
impl RefreshFailure {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Transient)
    }
}

type RefreshResult = Result<OAuthCredentials, RefreshFailure>;
struct Attempt {
    generation: [u8; 32],
    result: RefreshResult,
    at: Instant,
}
#[derive(Default)]
struct Entry {
    gate: tokio::sync::Mutex<()>,
    attempt: Mutex<Option<Attempt>>,
}
static ENTRIES: LazyLock<Mutex<HashMap<String, Arc<Entry>>>> = LazyLock::new(Default::default);
const TRANSIENT_RETRY: Duration = Duration::from_secs(5);

fn entry(provider: &str) -> Arc<Entry> {
    ENTRIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(super::auth_json_key(provider).to_owned())
        .or_default()
        .clone()
}
fn generation(creds: &OAuthCredentials) -> [u8; 32] {
    let mut digest = Sha256::new();
    for field in [&creds.cred_type, &creds.access, &creds.refresh] {
        digest.update((field.len() as u64).to_le_bytes());
        digest.update(field.as_bytes());
    }
    digest.update(creds.expires.to_le_bytes());
    digest.finalize().into()
}
pub fn retry_provider_refresh(provider: &str) {
    ENTRIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(super::auth_json_key(provider));
}
pub fn refresh_terminally_rejected(provider: &str, creds: &OAuthCredentials) -> bool {
    let entry = entry(provider);
    let state = entry.attempt.lock().unwrap_or_else(|e| e.into_inner());
    state.as_ref().is_some_and(|attempt| {
        attempt.generation == generation(creds)
            && attempt
                .result
                .as_ref()
                .is_err_and(|error| error.is_terminal())
    })
}
pub(super) fn cached_success(provider: &str, creds: &OAuthCredentials) -> Option<OAuthCredentials> {
    let entry = entry(provider);
    let state = entry.attempt.lock().unwrap_or_else(|e| e.into_inner());
    let attempt = state.as_ref()?;
    if attempt.generation != generation(creds) {
        return None;
    }
    attempt
        .result
        .as_ref()
        .ok()
        .filter(|fresh| !fresh.is_expired())
        .cloned()
}
fn entry_is_current(provider: &str, expected: &Arc<Entry>) -> bool {
    ENTRIES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(super::auth_json_key(provider))
        .is_some_and(|current| Arc::ptr_eq(current, expected))
}
async fn coordinated<F, Fut>(provider: &str, creds: &OAuthCredentials, refresh: F) -> RefreshResult
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = RefreshResult>,
{
    let entry = entry(provider);
    let _gate = entry.gate.lock().await;
    if !entry_is_current(provider, &entry) {
        return Err(RefreshFailure::Transient);
    }
    let generation = generation(creds);
    {
        let state = entry.attempt.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(attempt) = state.as_ref().filter(|a| a.generation == generation) {
            let reusable = match &attempt.result {
                Ok(fresh) => !fresh.is_expired(),
                Err(error) => error.is_terminal() || attempt.at.elapsed() < TRANSIENT_RETRY,
            };
            if reusable {
                return attempt.result.clone();
            }
        }
    }
    let result = refresh().await;
    if !entry_is_current(provider, &entry) {
        return Err(RefreshFailure::Transient);
    }
    *entry.attempt.lock().unwrap_or_else(|e| e.into_inner()) = Some(Attempt {
        generation,
        result: result.clone(),
        at: Instant::now(),
    });
    result
}
pub(super) async fn refresh_expired(provider: &str, creds: &OAuthCredentials) -> RefreshResult {
    coordinated(provider, creds, || refresh_token(provider, &creds.refresh)).await
}
#[cfg(test)]
pub(super) static TEST_REFRESH_URL: Mutex<Option<String>> = Mutex::new(None);

pub async fn refresh_token(provider: &str, refresh: &str) -> RefreshResult {
    #[cfg(test)]
    {
        let test_url = TEST_REFRESH_URL
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(url) = test_url {
            return refresh_http_at(super::auth_json_key(provider), refresh, &url).await;
        }
    }

    let url = match super::auth_json_key(provider) {
        "anthropic" => super::TOKEN_URL,
        "openai-codex" => super::OPENAI_TOKEN_URL,
        "google-antigravity" => super::ANTIGRAVITY_TOKEN_URL,
        _ => return Err(RefreshFailure::Unsupported),
    };
    refresh_http_at(super::auth_json_key(provider), refresh, url).await
}
async fn refresh_http_at(provider: &str, refresh: &str, url: &str) -> RefreshResult {
    refresh_http_bounded(provider, refresh, url, Duration::from_secs(10)).await
}
async fn refresh_http_bounded(
    provider: &str,
    refresh: &str,
    url: &str,
    deadline: Duration,
) -> RefreshResult {
    if refresh.is_empty() {
        return Err(RefreshFailure::Rejected);
    }
    let client = reqwest::Client::builder()
        .timeout(deadline)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| RefreshFailure::Transient)?;
    let request = client.post(url);
    let request = match provider {
        "anthropic" => request.json(&serde_json::json!({"grant_type":"refresh_token", "client_id":super::CLIENT_ID, "refresh_token":refresh})),
        "openai-codex" => request.form(&[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", super::OPENAI_CLIENT_ID)]),
        "google-antigravity" => request.form(&[("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", super::ANTIGRAVITY_CLIENT_ID), ("client_secret", super::ANTIGRAVITY_CLIENT_SECRET)]),
        _ => return Err(RefreshFailure::Unsupported),
    };
    let mut response = request
        .send()
        .await
        .map_err(|_| RefreshFailure::Transient)?;
    let status = response.status();
    const BODY_LIMIT: usize = 64 * 1024;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RefreshFailure::Transient)?
    {
        if body.len().saturating_add(chunk.len()) > BODY_LIMIT {
            return Err(RefreshFailure::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    // Read only the standardized error code. Never preserve error_description
    // or arbitrary provider payloads in diagnostics or the suppression cache.
    if !status.is_success() {
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let code = data["error"].as_str();
        return Err(match code {
            Some("temporarily_unavailable" | "server_error") => RefreshFailure::Transient,
            Some(
                "invalid_grant"
                | "invalid_client"
                | "unauthorized_client"
                | "unsupported_grant_type",
            ) => RefreshFailure::Rejected,
            _ if status.as_u16() == 429 || status.is_server_error() => RefreshFailure::Transient,
            _ => RefreshFailure::Rejected,
        });
    }
    let data: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| RefreshFailure::InvalidResponse)?;
    let access = data["access_token"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or(RefreshFailure::InvalidResponse)?;
    let expires_in = data["expires_in"].as_u64().unwrap_or(3600);
    if expires_in == 0 {
        return Err(RefreshFailure::InvalidResponse);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    // Do not turn a short but usable grant into an already expired credential.
    let margin = 300.min(expires_in / 10);
    Ok(OAuthCredentials {
        cred_type: "oauth".into(),
        access: access.into(),
        refresh: data["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(refresh)
            .into(),
        expires: now.saturating_add(expires_in.saturating_sub(margin).saturating_mul(1000)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn expired(grant: &str) -> OAuthCredentials {
        OAuthCredentials {
            cred_type: "oauth".into(),
            access: "expired".into(),
            refresh: grant.into(),
            expires: 0,
        }
    }

    async fn fixture(status: u16, body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            let count = stream.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).contains("refresh_token"));
            let response = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn recovery_auth_http_terminal_transient_and_safe_diagnostics() {
        for (status, body, terminal) in [
            (
                400,
                r#"{"error":"invalid_grant","error_description":"SECRET-CANARY"}"#,
                true,
            ),
            (401, "SECRET-CANARY", true),
            (400, r#"{"error":"temporarily_unavailable"}"#, false),
            (400, r#"{"error":"server_error"}"#, false),
            (429, "SECRET-CANARY", false),
            (503, "SECRET-CANARY", false),
            (
                200,
                r#"{"access_token":"","error_description":"SECRET-CANARY"}"#,
                true,
            ),
        ] {
            let url = fixture(status, body).await;
            let error = refresh_http_at("anthropic", "secret-grant", &url)
                .await
                .unwrap_err();
            assert_eq!(error.is_terminal(), terminal);
            assert!(!format!("{error:?} {error}").contains("SECRET-CANARY"));
            assert!(!format!("{error:?} {error}").contains("secret-grant"));
        }
    }

    #[tokio::test]
    async fn recovery_auth_coalesces_and_suppresses_until_generation_or_retry() {
        let provider = "recovery-auth-coordinator-fixture";
        retry_provider_refresh(provider);
        let calls = AtomicUsize::new(0);
        let creds = expired("generation-one");
        let attempt = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Err(RefreshFailure::Rejected)
        };
        let (first, second) = tokio::join!(
            coordinated(provider, &creds, attempt),
            coordinated(provider, &creds, attempt)
        );
        assert!(first.unwrap_err().is_terminal());
        assert!(second.unwrap_err().is_terminal());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(coordinated(provider, &creds, attempt).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let changed = expired("generation-two");
        assert!(coordinated(provider, &changed, attempt).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        retry_provider_refresh(provider);
        assert!(coordinated(provider, &changed, attempt).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn recovery_auth_success_is_shared_without_writeback() {
        let provider = "recovery-auth-success-fixture";
        let creds = expired("success-generation");
        retry_provider_refresh(provider);
        let calls = AtomicUsize::new(0);
        let attempt = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            let url = fixture(
                200,
                r#"{"access_token":"fresh","refresh_token":"rotated","expires_in":3600}"#,
            )
            .await;
            refresh_http_at("anthropic", "old", &url).await
        };
        let (first, second) = tokio::join!(
            coordinated(provider, &creds, attempt),
            coordinated(provider, &creds, attempt)
        );
        assert_eq!(first.unwrap().access, "fresh");
        assert_eq!(second.unwrap().refresh, "rotated");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(cached_success(provider, &creds).unwrap().access, "fresh");
    }
    #[tokio::test]
    async fn recovery_auth_timeout_is_transient_and_bounded() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let result =
            refresh_http_bounded("anthropic", "grant", &url, Duration::from_millis(25)).await;
        task.abort();
        assert_eq!(result.unwrap_err(), RefreshFailure::Transient);
    }

    #[tokio::test]
    async fn recovery_auth_transient_retry_waits_for_interval() {
        let provider = "recovery-auth-transient-fixture";
        retry_provider_refresh(provider);
        let creds = expired("transient");
        let calls = AtomicUsize::new(0);
        let attempt = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(RefreshFailure::Transient)
        };
        assert!(coordinated(provider, &creds, attempt).await.is_err());
        assert!(coordinated(provider, &creds, attempt).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        entry(provider).attempt.lock().unwrap().as_mut().unwrap().at =
            Instant::now() - TRANSIENT_RETRY;
        assert!(coordinated(provider, &creds, attempt).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn recovery_auth_explicit_retry_discards_inflight_old_generation() {
        let provider = "recovery-auth-inflight-fixture";
        let creds = expired("inflight-generation");
        retry_provider_refresh(provider);
        let result = coordinated(provider, &creds, || async {
            retry_provider_refresh(provider);
            Ok(OAuthCredentials {
                cred_type: "oauth".into(),
                access: "obsolete".into(),
                refresh: "obsolete".into(),
                expires: u64::MAX,
            })
        })
        .await;
        assert_eq!(result.unwrap_err(), RefreshFailure::Transient);
        assert!(cached_success(provider, &creds).is_none());
    }
}

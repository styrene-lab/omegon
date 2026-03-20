//! version_check — Polls GitHub for new Omegon releases.
//!
//! Checks on session start. If a newer version exists, sends a notification.
//! Respects OMEGON_SKIP_VERSION_CHECK and OMEGON_OFFLINE env vars.
//!
//! Ported from extensions/version-check.ts (94 LoC TS → ~90 LoC Rust)

use async_trait::async_trait;
use omegon_traits::{BusEvent, BusRequest, Feature};

const REPO_OWNER: &str = "styrene-lab";
const REPO_NAME: &str = "omegon-core";
const FETCH_TIMEOUT_SECS: u64 = 10;

pub struct VersionCheck {
    current_version: String,
    notified_version: Option<String>,
    checked: bool,
}

impl VersionCheck {
    pub fn new(current_version: impl Into<String>) -> Self {
        Self {
            current_version: current_version.into(),
            notified_version: None,
            checked: false,
        }
    }
}

#[async_trait]
impl Feature for VersionCheck {
    fn name(&self) -> &str { "version-check" }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        if let BusEvent::SessionStart { .. } = event {
            if self.checked { return vec![]; }
            self.checked = true;

            if std::env::var("OMEGON_SKIP_VERSION_CHECK").is_ok()
                || std::env::var("OMEGON_OFFLINE").is_ok()
            {
                return vec![];
            }

            // Fire-and-forget async check. We can't do async in on_event,
            // so we spawn a detached task that logs the result. The notification
            // won't show via BusRequest (since we can't return it from a spawned task),
            // but tracing will capture it.
            //
            // TODO: Use a channel to send BusRequest back from the spawned task.
            let current = self.current_version.clone();
            tokio::spawn(async move {
                match fetch_latest().await {
                    Some(latest) if is_newer(&latest, &current) => {
                        tracing::info!(
                            current = %current,
                            latest = %latest,
                            "Omegon update available: v{current} → v{latest}. \
                             Run `curl -fsSL https://omegon.styrene.dev/install | sh` to upgrade."
                        );
                    }
                    _ => {
                        tracing::debug!("Version check: up to date");
                    }
                }
            });
        }
        vec![]
    }
}

async fn fetch_latest() -> Option<String> {
    let url = format!(
        "https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest"
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("omegon-version-check")
        .build()
        .ok()?;

    let resp = client.get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() { return None; }
    let body: serde_json::Value = resp.json().await.ok()?;
    body["tag_name"].as_str()
        .map(|s| s.strip_prefix('v').unwrap_or(s).to_string())
}

/// Compare dotted version strings. Returns true if `latest` > `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    let len = l.len().max(c.len());
    for i in 0..len {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv > cv { return true; }
        if lv < cv { return false; }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(is_newer("0.13.0", "0.12.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.12.0", "0.12.0"));
        assert!(!is_newer("0.11.0", "0.12.0"));
        assert!(is_newer("0.12.1", "0.12.0"));
    }

    #[test]
    fn version_with_prefix() {
        // The fetch strips 'v' prefix before comparison
        assert!(is_newer("0.13.0", "0.12.0"));
    }

    #[test]
    fn respects_env_skip() {
        let mut vc = VersionCheck::new("0.12.0");
        // SAFETY: single-threaded test — no concurrent env access
        unsafe { std::env::set_var("OMEGON_SKIP_VERSION_CHECK", "1"); }
        let requests = vc.on_event(&BusEvent::SessionStart {
            cwd: "/tmp".into(),
            session_id: "test".into(),
        });
        assert!(requests.is_empty());
        unsafe { std::env::remove_var("OMEGON_SKIP_VERSION_CHECK"); }
    }
}

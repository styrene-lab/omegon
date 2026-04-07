use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use omegon_traits::DaemonEventEnvelope;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use tempfile::TempDir;

#[derive(Debug, Deserialize)]
struct StartupEvent {
    #[serde(rename = "type")]
    event_type: String,
    schema_version: u32,
    pid: u32,
    http_base: String,
    startup_url: String,
    health_url: String,
    ready_url: String,
    ws_url: String,
    auth_mode: String,
    auth_source: String,
}

#[derive(Debug, Deserialize)]
struct StartupPayload {
    schema_version: u32,
    addr: String,
    http_base: String,
    state_url: String,
    startup_url: String,
    health_url: String,
    ready_url: String,
    ws_url: String,
    token: String,
    auth_mode: String,
    auth_source: String,
}

#[derive(Debug, Deserialize)]
struct EventAccepted {
    accepted: bool,
    queued_events: usize,
}

struct SpawnedDaemon {
    child: Child,
    _tmp: TempDir,
}

impl Drop for SpawnedDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn resolve_omegon_binary() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_omegon") {
        return Ok(PathBuf::from(path));
    }

    let current = std::env::current_exe().context("current_exe")?;
    let deps_dir = current.parent().context("integration test executable has no parent")?;
    let debug_dir = deps_dir.parent().context("deps dir has no parent")?;
    let candidate = debug_dir.join(if cfg!(windows) { "omegon.exe" } else { "omegon" });
    if candidate.is_file() {
        return Ok(candidate);
    }

    anyhow::bail!(
        "unable to locate omegon binary: CARGO_BIN_EXE_omegon unset and fallback {} missing",
        candidate.display()
    )
}

fn spawn_daemon() -> Result<(SpawnedDaemon, StartupEvent)> {
    let tmp = tempfile::tempdir().context("tempdir")?;
    let bin = resolve_omegon_binary()?;

    let mut child = Command::new(bin)
        .args(["serve", "--control-port", "7854", "--strict-port"])
        .current_dir(tmp.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("spawn omegon serve")?;

    let stdout = child
        .stdout
        .take()
        .context("serve stdout not captured")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).context("read startup line")?;
    let startup: StartupEvent = serde_json::from_str(line.trim()).context("parse startup event json")?;

    Ok((
        SpawnedDaemon { child, _tmp: tmp },
        startup,
    ))
}

async fn wait_for_startup_payload(startup_url: &str, deadline: Duration) -> Result<StartupPayload> {
    let client = reqwest::Client::new();
    let end = Instant::now() + deadline;
    let mut last_err = None;

    while Instant::now() < end {
        match client.get(startup_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                return resp.json::<StartupPayload>().await.context("decode startup payload");
            }
            Ok(resp) => {
                last_err = Some(anyhow::anyhow!("startup status {}", resp.status()));
            }
            Err(err) => {
                last_err = Some(anyhow::anyhow!(err));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("startup payload unavailable before deadline")))
}

#[tokio::test]
async fn serve_exposes_startup_and_accepts_event_over_http() -> Result<()> {
    let (mut daemon, startup_event) = spawn_daemon()?;
    assert_eq!(startup_event.event_type, "omegon.startup");
    assert_eq!(startup_event.schema_version, 2);
    assert_eq!(startup_event.auth_mode, "ephemeral-bearer");
    assert_eq!(startup_event.auth_source, "generated");
    assert!(startup_event.pid > 0);
    assert!(startup_event.http_base.starts_with("http://127.0.0.1:"));
    assert!(startup_event.startup_url.ends_with("/api/startup"));
    assert!(startup_event.health_url.ends_with("/api/healthz"));
    assert!(startup_event.ready_url.ends_with("/api/readyz"));
    assert!(startup_event.ws_url.contains("/ws?token="));

    let payload = wait_for_startup_payload(&startup_event.startup_url, Duration::from_secs(5)).await?;
    assert_eq!(payload.schema_version, 2);
    assert_eq!(payload.startup_url, startup_event.startup_url);
    assert_eq!(payload.health_url, startup_event.health_url);
    assert_eq!(payload.ready_url, startup_event.ready_url);
    assert_eq!(payload.auth_mode, "ephemeral-bearer");
    assert_eq!(payload.auth_source, "generated");
    assert!(payload.addr.starts_with("127.0.0.1:"));
    assert!(payload.state_url.ends_with("/api/state"));
    assert!(payload.ws_url.contains("/ws?token="));
    assert!(payload.token.len() >= 4);

    let event = DaemonEventEnvelope {
        event_id: "evt-blackbox-1".into(),
        source: "integration-test".into(),
        trigger_kind: "manual".into(),
        payload: serde_json::json!({"ok": true}),
    };
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/events", payload.http_base))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", payload.token))
        .json(&event)
        .send()
        .await
        .context("post daemon event")?;
    assert_eq!(resp.status(), reqwest::StatusCode::ACCEPTED);
    let accepted: EventAccepted = resp.json().await.context("decode event accepted")?;
    assert!(accepted.accepted);
    assert_eq!(accepted.queued_events, 1);

    let _ = daemon.child.kill();
    let _ = daemon.child.wait();
    Ok(())
}

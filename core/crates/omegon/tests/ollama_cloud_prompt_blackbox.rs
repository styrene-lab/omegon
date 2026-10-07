#![cfg(all(unix, feature = "product"))]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{Json, Router, http::HeaderMap, routing::post};
use serde_json::Value;
use tokio::process::Command;

async fn prompt(base_url: &str, disabled_tools: &[String]) -> Result<std::process::Output> {
    let home = tempfile::tempdir()?;
    let cwd = home.path().join("project");
    std::fs::create_dir_all(cwd.join(".git"))?;
    std::fs::write(cwd.join("README.md"), "offline provider fixture\n")?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_omegon"))
        .args([
            "--model",
            "ollama-cloud:gpt-oss:120b-cloud",
            "--prompt",
            "Reply with exactly OK",
            "--max-turns",
            "1",
            "--max-retries",
            "0",
            "--fresh",
        ])
        .current_dir(cwd)
        // No real credentials, profiles, or provider override URLs reach the child.
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("OMEGON_HOME", home.path().join("omegon"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("OMEGON_NO_KEYRING", "1")
        .env("NO_COLOR", "1")
        .env("OMEGON_NERD_FONT", "1")
        .env("OLLAMA_API_KEY", "fixture-key")
        .env("OLLAMA_CLOUD_BASE_URL", base_url)
        .env("OMEGON_CHILD_DISABLED_TOOLS", disabled_tools.join(","))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .context("launch source-built Ollama Cloud prompt fixture")?;
    let pid = child.id().context("fixture child pid")?;
    // Drain output concurrently so a full pipe cannot stall the process.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>| {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).await?;
            std::io::Result::Ok(bytes)
        })
    };
    let stdout = read(Box::new(stdout));
    let stderr = read(Box::new(stderr));
    let status = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
    // Cleanup includes descendants even if the immediate CLI has exited.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let status = match status {
        Ok(status) => status?,
        Err(error) => {
            let _ = child.wait().await;
            let _ = stdout.await;
            let _ = stderr.await;
            return Err(error).context("prompt fixture exceeded its process deadline");
        }
    };
    Ok(std::process::Output {
        status,
        stdout: stdout.await??,
        stderr: stderr.await??,
    })
}

#[tokio::test]
async fn ollama_cloud_prompt_advertises_tools_and_honors_explicit_no_tools() -> Result<()> {
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let app = Router::new().route("/api/chat", post(move |headers: HeaderMap, Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            assert_eq!(headers["authorization"], "Bearer fixture-key");
            captured.lock().unwrap().push(body);
            ([("content-type", "application/x-ndjson")], concat!(
                "{\"message\":{\"content\":\"OK\"},\"done\":false}\n",
                "{\"message\":{\"content\":\"\"},\"done\":true,\"prompt_eval_count\":3,\"eval_count\":1}\n"
            ))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base_url = format!("http://{}/api", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let result = async {
        let mut disabled = Vec::new();
        for with_tools in [true, false] {
            let output = prompt(&base_url, &disabled).await?;
            assert!(
                output.status.success(),
                "CLI status {}: stdout={} stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("OK"));
            let requests = std::mem::take(&mut *requests.lock().unwrap());
            assert_eq!(
                requests.len(),
                1,
                "fixture should make one inference request"
            );
            let request = &requests[0];
            assert_eq!(request["model"], "gpt-oss:120b-cloud");
            if with_tools {
                let tools = request["tools"]
                    .as_array()
                    .context("normal prompt omitted tools")?;
                assert!(!tools.is_empty());
                disabled = tools
                    .iter()
                    .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
                    .collect();
            } else {
                assert!(
                    request.get("tools").is_none(),
                    "explicit no-tools configuration advertised tools"
                );
            }
        }
        anyhow::Ok(())
    }
    .await;
    server.abort();
    let _ = server.await;
    result
}

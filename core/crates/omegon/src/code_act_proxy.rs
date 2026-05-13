//! Unix socket JSON-RPC proxy for code-act scripts.
//!
//! The host omegon process starts a listener before executing a script.
//! The Python prelude connects and sends tool calls as JSON-RPC requests.
//! The host dispatches them through the normal tool execution pipeline.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;

use crate::tools::bash;
use omegon_traits::{ContentBlock, ToolProvider, ToolResult};

#[derive(Deserialize)]
struct RpcRequest {
    id: u64,
    method: String,
    params: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct RpcResponse {
    id: u64,
    result: Option<String>,
    error: Option<String>,
}

pub struct ProxyServer {
    socket_path: PathBuf,
    cwd: PathBuf,
}

impl ProxyServer {
    pub fn new(cwd: PathBuf) -> Result<Self> {
        let run_id = uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("tmp")
            .to_string();
        let socket_dir = cwd.join(".omegon");
        std::fs::create_dir_all(&socket_dir)?;
        let socket_path = socket_dir.join(format!("code-act-proxy-{run_id}.sock"));
        Ok(Self { socket_path, cwd })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn python_prelude(&self) -> String {
        format!(
            r#"
import socket, json

_OMEGON_SOCK = "{sock}"
_OMEGON_RPC_ID = 0

def _omegon_rpc(method: str, params: dict = None) -> str:
    global _OMEGON_RPC_ID
    _OMEGON_RPC_ID += 1
    req = json.dumps({{"id": _OMEGON_RPC_ID, "method": method, "params": params or {{}}}}) + "\n"
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(30)
    s.connect(_OMEGON_SOCK)
    s.sendall(req.encode())
    data = b""
    while True:
        chunk = s.recv(65536)
        if not chunk:
            break
        data += chunk
        if b"\n" in data:
            break
    s.close()
    resp = json.loads(data.decode().strip())
    if resp.get("error"):
        raise RuntimeError(f"Tool error: {{resp['error']}}")
    return resp.get("result", "")

def web_search(query: str) -> str:
    """Search the web and return results."""
    return _omegon_rpc("web_search", {{"query": query}})

def web_fetch(url: str) -> str:
    """Fetch a URL and return its content."""
    return _omegon_rpc("web_fetch", {{"url": url}})

"#,
            sock = self.socket_path.display()
        )
    }

    pub async fn serve(&self, cancel: CancellationToken) -> Result<()> {
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }
        let listener = UnixListener::bind(&self.socket_path)?;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _)) => {
                            let cwd = self.cwd.clone();
                            let cancel = cancel.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, &cwd, cancel).await {
                                    tracing::debug!(error = %e, "proxy connection error");
                                }
                            });
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "proxy accept error");
                        }
                    }
                }
            }
        }

        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }

    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    cwd: &Path,
    cancel: CancellationToken,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = RpcResponse {
                    id: 0,
                    result: None,
                    error: Some(format!("invalid request: {e}")),
                };
                let mut buf = serde_json::to_vec(&resp)?;
                buf.push(b'\n');
                writer.write_all(&buf).await?;
                continue;
            }
        };

        let response = dispatch_rpc(&req.method, &req.params, cwd, &cancel).await;
        let resp = match response {
            Ok(text) => RpcResponse {
                id: req.id,
                result: Some(text),
                error: None,
            },
            Err(e) => RpcResponse {
                id: req.id,
                result: None,
                error: Some(e.to_string()),
            },
        };
        let mut buf = serde_json::to_vec(&resp)?;
        buf.push(b'\n');
        writer.write_all(&buf).await?;
    }

    Ok(())
}

async fn dispatch_rpc(
    method: &str,
    params: &serde_json::Value,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<String> {
    match method {
        "web_search" => {
            let args = serde_json::json!({"query": params.get("query").and_then(|v| v.as_str()).unwrap_or("")});
            let provider = crate::tools::web_search::WebSearchProvider::new();
            let result = provider
                .execute("web_search", "proxy", args, cancel.clone())
                .await?;
            Ok(extract_text(&result))
        }
        "web_fetch" => {
            let args = serde_json::json!({"url": params.get("url").and_then(|v| v.as_str()).unwrap_or("")});
            let provider = crate::tools::web_search::WebSearchProvider::new();
            let result = provider
                .execute("web_fetch", "proxy", args, cancel.clone())
                .await?;
            Ok(extract_text(&result))
        }
        "bash" => {
            let command = params
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing 'command' parameter"))?;
            let result = bash::execute(command, cwd, Some(30), cancel.clone()).await?;
            Ok(extract_text(&result))
        }
        other => Err(anyhow::anyhow!("unknown method: {other}")),
    }
}

fn extract_text(result: &ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_server_creates_socket_path() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = ProxyServer::new(tmp.path().to_path_buf()).unwrap();
        assert!(
            proxy
                .socket_path()
                .to_str()
                .unwrap()
                .contains("code-act-proxy-")
        );
        assert!(proxy.socket_path().to_str().unwrap().ends_with(".sock"));
    }

    #[test]
    fn python_prelude_contains_socket_path() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = ProxyServer::new(tmp.path().to_path_buf()).unwrap();
        let prelude = proxy.python_prelude();
        assert!(prelude.contains(&proxy.socket_path().display().to_string()));
        assert!(prelude.contains("def web_search"));
        assert!(prelude.contains("def web_fetch"));
        assert!(prelude.contains("_omegon_rpc"));
    }

    #[tokio::test]
    async fn proxy_server_accepts_and_responds() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = ProxyServer::new(tmp.path().to_path_buf()).unwrap();
        let cancel = CancellationToken::new();

        let sock_path = proxy.socket_path().to_path_buf();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move { proxy.serve(server_cancel).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();

        let req = serde_json::json!({"id": 1, "method": "bash", "params": {"command": "echo proxy-test"}});
        let mut msg = serde_json::to_vec(&req).unwrap();
        msg.push(b'\n');
        writer.write_all(&msg).await.unwrap();

        let mut lines = BufReader::new(reader).lines();
        let line = lines.next_line().await.unwrap().unwrap();
        let resp: RpcResponse = serde_json::from_str(&line).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap().contains("proxy-test"));

        cancel.cancel();
        let _ = server.await;
    }

    #[tokio::test]
    async fn proxy_returns_error_for_unknown_method() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = ProxyServer::new(tmp.path().to_path_buf()).unwrap();
        let cancel = CancellationToken::new();

        let sock_path = proxy.socket_path().to_path_buf();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move { proxy.serve(server_cancel).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();

        let req = serde_json::json!({"id": 2, "method": "nonexistent", "params": {}});
        let mut msg = serde_json::to_vec(&req).unwrap();
        msg.push(b'\n');
        writer.write_all(&msg).await.unwrap();

        let mut lines = BufReader::new(reader).lines();
        let line = lines.next_line().await.unwrap().unwrap();
        let resp: RpcResponse = serde_json::from_str(&line).unwrap();
        assert_eq!(resp.id, 2);
        assert!(resp.error.unwrap().contains("unknown method"));

        cancel.cancel();
        let _ = server.await;
    }

    #[test]
    fn cleanup_removes_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = ProxyServer::new(tmp.path().to_path_buf()).unwrap();
        let path = proxy.socket_path().to_path_buf();
        std::fs::write(&path, "").unwrap();
        assert!(path.exists());
        proxy.cleanup();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn python_script_calls_bash_through_proxy() {
        let tmp = tempfile::tempdir().unwrap();
        let proxy = ProxyServer::new(tmp.path().to_path_buf()).unwrap();
        let sock_path = proxy.socket_path().to_path_buf();
        let prelude = proxy.python_prelude();
        let cancel = CancellationToken::new();

        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move { proxy.serve(server_cancel).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let script = format!(
            r#"{}
result = _omegon_rpc("bash", {{"command": "echo proxy-e2e-works"}})
print(f"Got: {{result.strip()}}")
"#,
            prelude
        );

        let script_path = tmp.path().join("proxy_test.py");
        std::fs::write(&script_path, &script).unwrap();
        let output = tokio::process::Command::new("python3")
            .arg(&script_path)
            .current_dir(tmp.path())
            .output()
            .await
            .unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Got: proxy-e2e-works"),
            "expected proxy-e2e-works in stdout, got: {stdout}"
        );

        cancel.cancel();
        let _ = server.await;
    }
}

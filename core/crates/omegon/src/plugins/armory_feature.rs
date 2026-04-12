//! ArmoryFeature — executes script-backed and OCI container-backed tools
//! declared in armory plugin.toml manifests.
//!
//! # Execution contract
//!
//! All runners use the same JSON stdin/stdout protocol:
//! - **Input**: tool arguments as a JSON object on stdin
//! - **Output**: `{"result": "...", "error": null}` or `{"result": null, "error": "..."}`
//! - **Exit code**: 0 = success, non-zero = error (stderr captured as message)
//! - **Timeout**: enforced by the harness (per-tool `timeout_secs`, default 30s)
//!
//! ## Script runners (Python/Node/Bash)
//!
//! Spawns `python3 script.py`, `node script.js`, or `bash script.sh`.
//! Arguments piped as JSON on stdin, result read from stdout.
//!
//! ## OCI container runner
//!
//! Runs `podman run` (or docker/nerdctl fallback) with configurable mount and
//! network policy. Same stdin/stdout contract. Container runtime detected via
//! `detect_container_runtime()` from the MCP module.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

use async_trait::async_trait;
use omegon_traits::{ContentBlock, Feature, ToolDefinition, ToolResult};

use super::armory::{ArmoryManifest, ToolEntry, ToolRunner};

/// Build a successful text ToolResult.
fn tool_ok(text: String) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: serde_json::json!({}),
    }
}

/// Build an error text ToolResult (still Ok at the Result level —
/// the bus marks is_error based on content, not Result::Err).
fn tool_err(text: String) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!("Error: {text}"),
        }],
        details: serde_json::json!({ "error": true }),
    }
}

/// Feature implementation for armory-style functional plugins.
///
/// Handles script-backed (Python/Node/Bash) and OCI container tools
/// declared in a single plugin.toml. HTTP-only tools are handled by
/// `HttpPluginFeature` separately.
pub struct ArmoryFeature {
    /// Plugin display name.
    name: String,
    /// Plugin root directory (parent of plugin.toml).
    plugin_root: PathBuf,
    /// Executable tool entries (script + OCI only).
    tools: Vec<ToolEntry>,
    /// Detected container runtime (lazy — only probed if OCI tools exist).
    container_runtime: std::sync::OnceLock<String>,
    /// Pre-cached dynamic context (generated at load time by context script/endpoint).
    cached_context: Option<CachedContext>,
}

/// Pre-generated context from a plugin's `[context]` section.
struct CachedContext {
    content: String,
    ttl_turns: u32,
}

impl ArmoryFeature {
    /// Create from a parsed manifest. Returns None if no executable tools
    /// and no dynamic context.
    ///
    /// Only includes tools with a runner (script/OCI). HTTP-only tools
    /// (endpoint without runner) are handled by HttpPluginFeature.
    ///
    /// If the manifest has a `[context]` section, the context script is
    /// executed at load time and the output is cached.
    pub async fn from_manifest(manifest: &ArmoryManifest, plugin_root: &Path) -> Option<Self> {
        let executable_tools: Vec<ToolEntry> = manifest
            .tools
            .iter()
            .filter(|t| t.is_script() || t.is_oci())
            .cloned()
            .collect();

        // Generate dynamic context if declared
        let cached_context = if let Some(ref ctx) = manifest.context {
            match generate_context(ctx, plugin_root).await {
                Ok(content) if !content.is_empty() => {
                    tracing::info!(
                        plugin = manifest.plugin.name,
                        len = content.len(),
                        ttl = ctx.ttl_turns,
                        "generated dynamic context"
                    );
                    Some(CachedContext {
                        content,
                        ttl_turns: ctx.ttl_turns,
                    })
                }
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!(
                        plugin = manifest.plugin.name,
                        error = %e,
                        "failed to generate dynamic context"
                    );
                    None
                }
            }
        } else {
            None
        };

        if executable_tools.is_empty() && cached_context.is_none() {
            return None;
        }

        Some(Self {
            name: manifest.plugin.name.clone(),
            plugin_root: plugin_root.to_path_buf(),
            tools: executable_tools,
            container_runtime: std::sync::OnceLock::new(),
            cached_context,
        })
    }

    fn container_runtime(&self) -> &str {
        self.container_runtime
            .get_or_init(super::mcp::detect_container_runtime)
    }

    /// Execute a script-backed tool (Python/Node/Bash).
    async fn execute_script(
        &self,
        tool: &ToolEntry,
        args: &Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let runner = tool
            .runner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("script tool '{}' has no runner", tool.name))?;
        let script = tool
            .script
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("script tool '{}' has no script path", tool.name))?;

        let script_path = self.plugin_root.join(script);
        if !script_path.exists() {
            anyhow::bail!("script not found: {}", script_path.display());
        }

        let cmd = match runner {
            ToolRunner::Python => "python3",
            ToolRunner::Node => "node",
            ToolRunner::Bash => "bash",
            other => anyhow::bail!("unsupported script runner: {other}"),
        };

        let script_str = script_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 script path: {}", script_path.display()))?;

        let timeout = Duration::from_secs(tool.timeout_secs);
        let input = serde_json::to_string(args)?;

        let mut child = tokio::process::Command::new(cmd)
            .arg(script_str)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.plugin_root)
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {cmd} {script_str}: {e}"))?;

        // Write args to stdin then close. We tolerate broken-pipe errors:
        // a fast script that doesn't actually read its stdin (e.g.
        // `echo 'hello'`) will close the read end of the pipe before
        // we finish writing, which is harmless — the child has already
        // moved on to producing its output. Propagating that error here
        // would manifest as a flaky test on busy CI runners (the race
        // is wall-clock dependent).
        write_stdin_best_effort(child.stdin.take(), input.as_bytes()).await;

        // Wait with timeout + cancellation
        let wait_fut = child.wait_with_output();
        tokio::pin!(wait_fut);

        let output = tokio::select! {
            result = tokio::time::timeout(timeout, &mut wait_fut) => {
                result.map_err(|_| anyhow::anyhow!(
                    "tool '{}' timed out after {}s", tool.name, tool.timeout_secs
                ))??
            }
            _ = cancel.cancelled() => {
                anyhow::bail!("tool '{}' cancelled", tool.name);
            }
        };

        parse_tool_output(&tool.name, &output)
    }

    /// Execute an OCI container-backed tool.
    async fn execute_oci(
        &self,
        tool: &ToolEntry,
        args: &Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let image = tool
            .image
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OCI tool '{}' has no image", tool.name))?;

        let runtime = self.container_runtime();
        let timeout = Duration::from_secs(tool.timeout_secs);
        let input = serde_json::to_string(args)?;

        let mut cmd_args: Vec<String> = vec![
            "run".into(),
            "--rm".into(),
            "-i".into(), // stdin pipe
        ];

        // Network policy — deny by default
        if !tool.network {
            cmd_args.push("--network=none".into());
        }

        // Mount working directory
        if tool.mount_cwd
            && let Ok(cwd) = std::env::current_dir()
        {
            cmd_args.push("-v".into());
            cmd_args.push(format!("{}:/workspace:Z", cwd.display()));
            cmd_args.push("-w".into());
            cmd_args.push("/workspace".into());
        }

        // Timeout (container-level stop signal)
        cmd_args.push(format!("--stop-timeout={}", tool.timeout_secs));

        // Image
        cmd_args.push(image.clone());

        let mut child = tokio::process::Command::new(runtime)
            .args(&cmd_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {runtime} run: {e}"))?;

        // Same broken-pipe tolerance as execute_script — see the comment
        // there for the rationale.
        write_stdin_best_effort(child.stdin.take(), input.as_bytes()).await;

        let wait_fut = child.wait_with_output();
        tokio::pin!(wait_fut);

        let output = tokio::select! {
            result = tokio::time::timeout(timeout, &mut wait_fut) => {
                result.map_err(|_| anyhow::anyhow!(
                    "OCI tool '{}' timed out after {}s", tool.name, tool.timeout_secs
                ))??
            }
            _ = cancel.cancelled() => {
                anyhow::bail!("OCI tool '{}' cancelled", tool.name);
            }
        };

        parse_tool_output(&tool.name, &output)
    }
}

/// Write `input` to a child process's stdin and close it, tolerating
/// broken-pipe errors (which are benign and racy: a fast child that
/// doesn't actually read its stdin will have closed the read end before
/// we finish writing). All other I/O errors are silently dropped — if
/// the child can't receive its arguments, that will surface as a tool
/// error from the script's own behavior, not from this function.
async fn write_stdin_best_effort(stdin: Option<tokio::process::ChildStdin>, input: &[u8]) {
    let Some(mut stdin) = stdin else {
        return;
    };
    let _ = stdin.write_all(input).await;
    let _ = stdin.shutdown().await;
}

#[async_trait]
impl Feature for ArmoryFeature {
    fn name(&self) -> &str {
        &self.name
    }

    fn provide_context(
        &self,
        _signals: &omegon_traits::ContextSignals<'_>,
    ) -> Option<omegon_traits::ContextInjection> {
        let ctx = self.cached_context.as_ref()?;
        Some(omegon_traits::ContextInjection {
            source: format!("armory:{}", self.name),
            content: ctx.content.clone(),
            priority: 60, // below core directives (90+), above background memory (40)
            ttl_turns: ctx.ttl_turns,
        })
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|t| {
                let runner_prefix = t
                    .runner
                    .as_ref()
                    .map(|r| format!("{r}:"))
                    .unwrap_or_default();
                ToolDefinition {
                    name: t.name.clone(),
                    label: format!("armory:{}{}", runner_prefix, t.name),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                }
            })
            .collect()
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| anyhow::anyhow!("unknown armory tool: {tool_name}"))?;

        if tool.is_script() {
            self.execute_script(tool, &args, cancel).await
        } else if tool.is_oci() {
            self.execute_oci(tool, &args, cancel).await
        } else {
            anyhow::bail!("tool '{}' has no supported execution method", tool_name)
        }
    }
}

/// Generate dynamic context by running the plugin's context script or calling its endpoint.
///
/// The script is expected to output plain text (not JSON) — this text is injected
/// directly into the system prompt as context.
async fn generate_context(
    ctx: &super::armory::ContextEntry,
    plugin_root: &Path,
) -> anyhow::Result<String> {
    // Script-backed context
    if let (Some(runner), Some(script)) = (&ctx.runner, &ctx.script) {
        let cmd = match runner {
            ToolRunner::Python => "python3",
            ToolRunner::Node => "node",
            ToolRunner::Bash => "bash",
            other => anyhow::bail!("unsupported context runner: {other}"),
        };

        let script_path = plugin_root.join(script);
        if !script_path.exists() {
            anyhow::bail!("context script not found: {}", script_path.display());
        }

        let script_str = script_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 path"))?;

        let output = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(cmd)
                .arg(script_str)
                .current_dir(plugin_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("context script timed out after 15s"))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("context script failed: {stderr}");
        }

        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    // HTTP-backed context
    if let Some(ref endpoint) = ctx.endpoint {
        let client = reqwest::Client::new();
        let resp = client
            .get(endpoint)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("context endpoint returned {}", resp.status());
        }

        return Ok(resp.text().await?.trim().to_string());
    }

    anyhow::bail!("context entry has no runner+script or endpoint")
}

/// Parse subprocess output into a ToolResult.
///
/// Tries to parse stdout as JSON with `result`/`error` fields.
/// Falls back to raw text if not valid JSON.
fn parse_tool_output(tool_name: &str, output: &std::process::Output) -> anyhow::Result<ToolResult> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        // Non-zero exit — use stderr as error message, fall back to stdout
        let msg = if !stderr.is_empty() {
            stderr.to_string()
        } else if !stdout.is_empty() {
            stdout.to_string()
        } else {
            format!(
                "tool '{}' failed with exit code {}",
                tool_name,
                output.status.code().unwrap_or(-1)
            )
        };
        return Ok(tool_err(msg));
    }

    // Try JSON { "result": ..., "error": ... } contract
    if let Ok(json) = serde_json::from_str::<Value>(&stdout) {
        if let Some(error) = json.get("error").and_then(|e| e.as_str())
            && !error.is_empty()
        {
            return Ok(tool_err(error.to_string()));
        }
        if let Some(result) = json.get("result") {
            return Ok(tool_ok(result.to_string()));
        }
        // JSON but not in contract format — return as-is
        return Ok(tool_ok(stdout.to_string()));
    }

    // Raw text output
    Ok(tool_ok(stdout.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: extract text from a ToolResult for assertions.
    fn result_text(result: &ToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Helper: check if a ToolResult signals an error (via details or text prefix).
    fn result_is_error(result: &ToolResult) -> bool {
        result
            .details
            .get("error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || result_text(result).starts_with("Error:")
    }

    #[test]
    fn parse_output_success_json() {
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: br#"{"result": "42 rows analyzed", "error": null}"#.to_vec(),
            stderr: vec![],
        };
        let result = parse_tool_output("test", &output).unwrap();
        assert!(!result_is_error(&result));
        assert!(result_text(&result).contains("42 rows analyzed"));
    }

    #[test]
    fn parse_output_success_raw_text() {
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: b"Hello, world!\n".to_vec(),
            stderr: vec![],
        };
        let result = parse_tool_output("test", &output).unwrap();
        assert!(!result_is_error(&result));
        assert!(result_text(&result).contains("Hello, world!"));
    }

    #[test]
    fn parse_output_json_error_field() {
        let output = std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: br#"{"result": null, "error": "file not found"}"#.to_vec(),
            stderr: vec![],
        };
        let result = parse_tool_output("test", &output).unwrap();
        assert!(result_is_error(&result));
        assert!(result_text(&result).contains("file not found"));
    }

    #[tokio::test]
    async fn from_manifest_no_executable_tools() {
        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "persona"
            id = "dev.test.passive"
            name = "Passive"
            version = "1.0.0"
            description = "test plugin"
        "#,
        )
        .unwrap();

        assert!(
            ArmoryFeature::from_manifest(&manifest, Path::new("/tmp"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn from_manifest_with_script_tool() {
        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.csv"
            name = "CSV Analyzer"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "analyze"
            description = "analyze a CSV"
            runner = "python"
            script = "tools/analyze.py"
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, Path::new("/tmp"))
            .await
            .unwrap();
        assert_eq!(feature.name(), "CSV Analyzer");
        let tools = feature.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "analyze");
        assert!(tools[0].label.contains("armory:python:"));
    }

    #[tokio::test]
    async fn from_manifest_with_oci_tool() {
        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.drc"
            name = "DRC Checker"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "drc_check"
            description = "run design rule check"
            runner = "oci"
            image = "ghcr.io/test/drc:latest"
            mount_cwd = true
            network = false
            timeout_secs = 120
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, Path::new("/tmp"))
            .await
            .unwrap();
        let tools = feature.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "drc_check");
        assert!(tools[0].label.contains("armory:oci:"));
    }

    #[tokio::test]
    async fn from_manifest_mixed_tools_only_executable() {
        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.mixed"
            name = "Mixed"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "script_tool"
            description = "runs a script"
            runner = "bash"
            script = "tools/run.sh"

            [[tools]]
            name = "http_tool"
            description = "calls an endpoint"
            endpoint = "http://localhost:9999/api"

            [[tools]]
            name = "oci_tool"
            description = "runs in container"
            runner = "oci"
            image = "test:latest"
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, Path::new("/tmp"))
            .await
            .unwrap();
        let tools = feature.tools();
        // Only script + OCI — HTTP-only tool excluded
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "script_tool"));
        assert!(tools.iter().any(|t| t.name == "oci_tool"));
        assert!(!tools.iter().any(|t| t.name == "http_tool"));
    }

    #[tokio::test]
    async fn execute_script_missing_script_file() {
        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.missing"
            name = "Missing Script"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "nope"
            description = "nonexistent script"
            runner = "python"
            script = "tools/nonexistent.py"
        "#,
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let feature = ArmoryFeature::from_manifest(&manifest, dir.path())
            .await
            .unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("nope", "call-1", serde_json::json!({}), cancel)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn execute_script_success() {
        let dir = tempfile::tempdir().unwrap();
        let tools_dir = dir.path().join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();

        // Write a trivial Python script that echoes JSON
        std::fs::write(
            tools_dir.join("echo.py"),
            r#"
import sys, json
args = json.load(sys.stdin)
print(json.dumps({"result": f"got {args.get('name', 'nobody')}", "error": None}))
"#,
        )
        .unwrap();

        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.echo"
            name = "Echo"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "echo"
            description = "echoes input"
            runner = "python"
            script = "tools/echo.py"
            timeout_secs = 10
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, dir.path())
            .await
            .unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute(
                "echo",
                "call-1",
                serde_json::json!({"name": "operator"}),
                cancel,
            )
            .await;

        match result {
            Ok(tr) => {
                let text = result_text(&tr);
                assert!(
                    !result_is_error(&tr),
                    "tool result should not be error: {text}"
                );
                assert!(
                    text.contains("got operator"),
                    "expected 'got operator' in: {text}"
                );
            }
            Err(e) => {
                // python3 might not be available in CI — skip gracefully
                if e.to_string().contains("spawn") {
                    eprintln!("skipping: python3 not available");
                } else {
                    panic!("unexpected error: {e}");
                }
            }
        }
    }

    #[tokio::test]
    async fn execute_script_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let tools_dir = dir.path().join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();

        std::fs::write(
            tools_dir.join("fail.sh"),
            "#!/bin/bash\necho 'something broke' >&2\nexit 1\n",
        )
        .unwrap();

        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.fail"
            name = "Fail"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "fail"
            description = "always fails"
            runner = "bash"
            script = "tools/fail.sh"
            timeout_secs = 5
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, dir.path())
            .await
            .unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("fail", "call-1", serde_json::json!({}), cancel)
            .await
            .unwrap();
        let text = result_text(&result);
        assert!(result_is_error(&result), "should be an error result");
        assert!(
            text.contains("something broke"),
            "expected stderr in error: {text}"
        );
    }

    #[tokio::test]
    async fn execute_unknown_tool() {
        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.x"
            name = "X"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "real"
            description = "exists"
            runner = "bash"
            script = "tools/real.sh"
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, Path::new("/tmp"))
            .await
            .unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("nonexistent", "call-1", serde_json::json!({}), cancel)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown armory tool")
        );
    }

    #[tokio::test]
    async fn execute_script_raw_text_output() {
        let dir = tempfile::tempdir().unwrap();
        let tools_dir = dir.path().join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();

        // Script that outputs plain text, not JSON
        std::fs::write(
            tools_dir.join("plain.sh"),
            "#!/bin/bash\necho 'plain text result'\n",
        )
        .unwrap();

        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.plain"
            name = "Plain"
            version = "1.0.0"
            description = "test plugin"

            [[tools]]
            name = "plain"
            description = "plain text output"
            runner = "bash"
            script = "tools/plain.sh"
            timeout_secs = 5
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, dir.path())
            .await
            .unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute("plain", "call-1", serde_json::json!({}), cancel)
            .await
            .unwrap();
        assert!(!result_is_error(&result));
        assert!(result_text(&result).contains("plain text result"));
    }

    #[tokio::test]
    async fn from_manifest_with_context_script() {
        let dir = tempfile::tempdir().unwrap();
        let ctx_dir = dir.path().join("context");
        std::fs::create_dir_all(&ctx_dir).unwrap();

        std::fs::write(
            ctx_dir.join("status.sh"),
            "#!/bin/bash\necho 'Library: 42 components loaded'\n",
        )
        .unwrap();

        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.ctx"
            name = "Context Test"
            version = "1.0.0"
            description = "test plugin"

            [context]
            runner = "bash"
            script = "context/status.sh"
            ttl_turns = 30

            [[tools]]
            name = "dummy"
            description = "dummy tool"
            runner = "bash"
            script = "tools/dummy.sh"
        "#,
        )
        .unwrap();

        let feature = ArmoryFeature::from_manifest(&manifest, dir.path())
            .await
            .unwrap();

        // Check cached context is populated
        assert!(feature.cached_context.is_some());
        let ctx = feature.cached_context.as_ref().unwrap();
        assert!(
            ctx.content.contains("42 components"),
            "context should contain script output: {}",
            ctx.content
        );
        assert_eq!(ctx.ttl_turns, 30);

        // Check provide_context returns the cached content
        let signals = omegon_traits::ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 10000,
        };
        let injection = feature.provide_context(&signals).unwrap();
        assert_eq!(injection.source, "armory:Context Test");
        assert!(injection.content.contains("42 components"));
        assert_eq!(injection.ttl_turns, 30);
        assert_eq!(injection.priority, 60);
    }

    #[tokio::test]
    async fn from_manifest_context_only_no_tools() {
        let dir = tempfile::tempdir().unwrap();
        let ctx_dir = dir.path().join("context");
        std::fs::create_dir_all(&ctx_dir).unwrap();

        std::fs::write(
            ctx_dir.join("info.sh"),
            "#!/bin/bash\necho 'project info here'\n",
        )
        .unwrap();

        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.ctx-only"
            name = "Context Only"
            version = "1.0.0"
            description = "test plugin"

            [context]
            runner = "bash"
            script = "context/info.sh"
        "#,
        )
        .unwrap();

        // Should create a feature even with no tools (context-only plugin)
        let feature = ArmoryFeature::from_manifest(&manifest, dir.path()).await;
        assert!(
            feature.is_some(),
            "context-only plugin should create a feature"
        );
        assert!(feature.unwrap().tools().is_empty(), "no tools expected");
    }

    #[tokio::test]
    async fn from_manifest_context_script_fails_gracefully() {
        let dir = tempfile::tempdir().unwrap();

        let manifest = ArmoryManifest::parse(
            r#"
            [plugin]
            type = "extension"
            id = "dev.test.ctx-fail"
            name = "Ctx Fail"
            version = "1.0.0"
            description = "test plugin"

            [context]
            runner = "bash"
            script = "context/nonexistent.sh"

            [[tools]]
            name = "tool"
            description = "a tool"
            runner = "bash"
            script = "tools/tool.sh"
        "#,
        )
        .unwrap();

        // Should still create a feature (tools exist) even if context fails
        let feature = ArmoryFeature::from_manifest(&manifest, dir.path()).await;
        assert!(
            feature.is_some(),
            "should still load despite context failure"
        );
        assert!(
            feature.unwrap().cached_context.is_none(),
            "context should be None on failure"
        );
    }
}

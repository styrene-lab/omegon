//! Local inference tool — Ollama management and delegation.
//!
//! Three tools: ask_local_model, list_local_models, manage_ollama.
//! Communicates with Ollama's OpenAI-compatible API at localhost:11434.

use async_trait::async_trait;
use futures_util::StreamExt;
use omegon_traits::{
    ContentBlock, PartialToolResult, ProgressUnits, ToolDefinition, ToolProgress, ToolProgressSink,
    ToolProvider, ToolResult,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::process::Command;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const DEFAULT_URL: &str = "http://localhost:11434";

/// Minimum interval between content-bearing streaming partials, mirroring
/// the bash runner's flush rate. Token streams from local models can fire
/// at hundreds of tokens per second on small models — without this we'd
/// flood the broadcast channel for no benefit.
const STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(150);

fn base_url() -> String {
    env::var("LOCAL_INFERENCE_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}

pub struct LocalInferenceProvider {
    client: reqwest::Client,
}

impl LocalInferenceProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn list_models(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let url = format!("{}/v1/models", base_url());
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("Ollama not reachable at {}", base_url());
        }
        let data: ModelsResponse = resp.json().await?;
        Ok(data.data)
    }

    async fn chat_completion(
        &self,
        model: &str,
        prompt: &str,
        system: Option<&str>,
        temperature: f32,
        max_tokens: u32,
    ) -> anyhow::Result<String> {
        let url = format!("{}/v1/chat/completions", base_url());
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(json!({"role": "system", "content": sys}));
        }
        messages.push(json!({"role": "user", "content": prompt}));

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": false,
        });

        let resp = self.client.post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Chat completion failed ({status}): {text}");
        }

        let data: ChatResponse = resp.json().await?;
        let content = data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        Ok(content)
    }

    /// Streaming variant of [`chat_completion`]. Sets `stream: true` on
    /// the OpenAI-compatible request, parses Server-Sent Events from the
    /// response body, and pushes rate-limited [`PartialToolResult`]s into
    /// the supplied [`ToolProgressSink`] as tokens arrive.
    ///
    /// On the wire each chunk looks like:
    ///
    /// ```text
    /// data: {"id":"...","choices":[{"delta":{"content":"hello"}}]}
    ///
    /// data: {"id":"...","choices":[{"delta":{"content":" world"}}]}
    ///
    /// data: [DONE]
    ///
    /// ```
    ///
    /// We accumulate `delta.content` strings into the response buffer
    /// and emit a partial whenever the rate limiter allows. Token count
    /// is approximated as "deltas observed", which is a slight
    /// undercount for multi-token deltas but accurate enough for the
    /// progress display ("how much has the model produced so far?").
    async fn chat_completion_streaming(
        &self,
        model: &str,
        prompt: &str,
        system: Option<&str>,
        temperature: f32,
        max_tokens: u32,
        sink: &ToolProgressSink,
    ) -> anyhow::Result<String> {
        let url = format!("{}/v1/chat/completions", base_url());
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(json!({"role": "system", "content": sys}));
        }
        messages.push(json!({"role": "user", "content": prompt}));

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": true,
        });

        let resp = self.client.post(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Chat completion (streaming) failed ({status}): {text}");
        }

        let started = Instant::now();
        let max_tokens_u64 = u64::from(max_tokens);
        let sink_active = sink.is_active();
        let mut accumulated = String::new();
        let mut deltas_seen: u64 = 0;
        let mut last_flush = Instant::now();

        // SSE chunks may not align with line boundaries — buffer the
        // partial bytes between chunks until we see a `\n\n` separator.
        let mut sse_buffer = String::new();
        let mut byte_stream = resp.bytes_stream();

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk?;
            sse_buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process every complete SSE event in the buffer. Events are
            // delimited by `\n\n`. Anything left over stays in the buffer
            // for the next chunk.
            while let Some(end) = sse_buffer.find("\n\n") {
                let event = sse_buffer[..end].to_string();
                sse_buffer.drain(..end + 2);

                for line in event.lines() {
                    let Some(payload) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    if payload == "[DONE]" {
                        // End of stream — flush a final partial below.
                        continue;
                    }
                    let parsed: Result<StreamingChatChunk, _> = serde_json::from_str(payload);
                    let Ok(chunk_obj) = parsed else {
                        // Skip malformed chunks rather than aborting the
                        // whole call. Ollama occasionally sends keep-alive
                        // garbage that doesn't fit the schema; the rest
                        // of the stream is still useful.
                        continue;
                    };
                    if let Some(choice) = chunk_obj.choices.first()
                        && let Some(content) = choice.delta.content.as_deref()
                        && !content.is_empty()
                    {
                        accumulated.push_str(content);
                        deltas_seen += 1;
                    }
                }
            }

            // Rate-limited partial emission. Same pattern as bash:
            // 150ms minimum between flushes, sink-aware so we skip the
            // string clone when nobody's listening.
            if sink_active && last_flush.elapsed() >= STREAM_FLUSH_INTERVAL {
                last_flush = Instant::now();
                sink.send(PartialToolResult {
                    tail: accumulated.clone(),
                    progress: ToolProgress {
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        heartbeat: false,
                        phase: Some(format!("generating ({model})")),
                        units: Some(ProgressUnits {
                            current: deltas_seen,
                            // Total is the model's max_tokens cap, not
                            // the actual token count it'll produce — the
                            // model usually stops well before. Still
                            // gives consumers a sane upper bound for
                            // progress bars.
                            total: Some(max_tokens_u64),
                            unit: "tokens".to_string(),
                        }),
                        tally: None,
                    },
                    details: json!({
                        "model": model,
                        "max_tokens": max_tokens,
                    }),
                });
            }
        }

        Ok(accumulated)
    }

    async fn ollama_status(&self) -> String {
        match self.client.get(base_url()).send().await {
            Ok(resp) if resp.status().is_success() => match self.list_models().await {
                Ok(models) => {
                    if models.is_empty() {
                        "Ollama is running but no models are loaded.".into()
                    } else {
                        let names: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
                        format!(
                            "Ollama is running. {} model(s): {}",
                            models.len(),
                            names.join(", ")
                        )
                    }
                }
                Err(_) => "Ollama is running but model listing failed.".into(),
            },
            _ => "Ollama is not running or not reachable.".into(),
        }
    }

    async fn ollama_start(&self) -> String {
        // Check if already running
        if self.client.get(base_url()).send().await.is_ok() {
            return "Ollama is already running.".into();
        }

        // On macOS, try the desktop app first
        #[cfg(target_os = "macos")]
        {
            if Command::new("open").arg("-a").arg("Ollama").spawn().is_ok() {
                // Poll for the server to come up
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if self.client.get(base_url()).send().await.is_ok() {
                        return "Ollama started (macOS app).".into();
                    }
                }
            }
        }

        // Fallback: start via `ollama serve`
        match Command::new("ollama").arg("serve").spawn() {
            Ok(_child) => {
                // Poll for the server to become reachable
                for i in 0..10 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if self.client.get(base_url()).send().await.is_ok() {
                        return "Ollama server started and reachable.".into();
                    }
                    if i == 4 {
                        tracing::debug!("Ollama server still starting after 5s...");
                    }
                }
                "Ollama server spawned but not reachable after 10s. Check `ollama serve` logs."
                    .into()
            }
            Err(e) => format!("Failed to start Ollama: {e}. Is it installed?"),
        }
    }

    fn ollama_stop(&self, is_ollama_integration: bool) -> String {
        if is_ollama_integration {
            return "Refusing to stop Ollama — omegon was launched by Ollama.".into();
        }

        // On macOS, gracefully quit the desktop app
        #[cfg(target_os = "macos")]
        {
            let quit_result = Command::new("osascript")
                .arg("-e")
                .arg("tell application \"Ollama\" to quit")
                .output();
            if let Ok(output) = quit_result
                && output.status.success()
            {
                return "Ollama stopped (macOS app).".into();
            }
        }

        // Fall back to exact process name match (not -f substring match)
        match Command::new("pkill").arg("-x").arg("ollama").output() {
            Ok(output) if output.status.success() => "Ollama stopped.".into(),
            Ok(_) => "No Ollama process found.".into(),
            Err(e) => format!("Failed to stop Ollama: {e}"),
        }
    }

    async fn ollama_pull(&self, model: &str, sink: Option<&ToolProgressSink>) -> String {
        let url = format!("{}/api/pull", base_url());

        // Use a long-timeout client for pulls (models can be 20+ GB)
        let pull_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1800))
            .build()
            .unwrap_or_default();

        let resp = match pull_client
            .post(&url)
            .json(&json!({"name": model, "stream": true}))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => return format!("Pull failed ({})", r.status()),
            Err(e) => return format!("Pull failed: {e}"),
        };

        // Parse streaming JSON lines for progress
        let started = Instant::now();
        let mut last_status = String::new();
        let mut byte_stream = resp.bytes_stream();

        while let Some(chunk) = byte_stream.next().await {
            let Ok(bytes) = chunk else { continue };
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(obj) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                let status = obj["status"].as_str().unwrap_or("");
                let total = obj["total"].as_u64().unwrap_or(0);
                let completed = obj["completed"].as_u64().unwrap_or(0);

                last_status = status.to_string();

                if let Some(sink) = sink
                    && sink.is_active()
                    && total > 0
                {
                    sink.send(PartialToolResult {
                        tail: format!("{status}: {completed}/{total}"),
                        progress: ToolProgress {
                            elapsed_ms: started.elapsed().as_millis() as u64,
                            heartbeat: false,
                            phase: Some(format!("pulling {model}")),
                            units: Some(ProgressUnits {
                                current: completed,
                                total: Some(total),
                                unit: "bytes".to_string(),
                            }),
                            tally: None,
                        },
                        details: json!({"model": model, "status": status}),
                    });
                }
            }
        }

        if last_status == "success" {
            format!("Pulled model: {model}")
        } else {
            format!("Pull finished with status: {last_status}")
        }
    }

    async fn ollama_delete(&self, model: &str) -> String {
        let url = format!("{}/api/delete", base_url());
        match self
            .client
            .delete(&url)
            .json(&json!({"name": model}))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => format!("Deleted model: {model}"),
            Ok(resp) => format!("Delete failed ({})", resp.status()),
            Err(e) => format!("Delete failed: {e}"),
        }
    }

    async fn auto_select_model(&self) -> Option<String> {
        let models = self.list_models().await.ok()?;
        if models.is_empty() {
            return None;
        }

        let hw = crate::ollama::OllamaManager::hardware_profile();
        let max_params = hw.recommended_max_params;

        // Preferred models ordered by quality, grouped by size class.
        // The hardware profile determines the maximum size class to consider.
        let preferred: &[&str] = match max_params {
            "100B+" => &[
                "qwen3:72b",
                "llama3:70b",
                "devstral-small",
                "qwen3:32b",
                "qwen3:30b",
                "qwen3:14b",
                "qwen3:8b",
            ],
            "70B" => &[
                "qwen3:72b",
                "llama3:70b",
                "devstral-small",
                "qwen3:32b",
                "qwen3:30b",
                "qwen3:14b",
                "qwen3:8b",
            ],
            "32B" => &[
                "devstral-small",
                "qwen3:32b",
                "qwen3:30b",
                "qwen3:14b",
                "qwen3:8b",
            ],
            "14B" => &["qwen3:14b", "qwen3:8b", "llama3:8b"],
            _ => &["qwen3:8b", "llama3:8b", "phi3:mini"],
        };

        for pref in preferred {
            if let Some(m) = models.iter().find(|m| m.id.contains(pref)) {
                return Some(m.id.clone());
            }
        }

        // Fall back to any available model
        models.first().map(|m| m.id.clone())
    }
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Deserialize)]
struct StreamingChatChunk {
    choices: Vec<StreamingChoice>,
}

#[derive(Deserialize)]
struct StreamingChoice {
    delta: StreamingDelta,
}

#[derive(Deserialize)]
struct StreamingDelta {
    #[serde(default)]
    content: Option<String>,
}

#[async_trait]
impl ToolProvider for LocalInferenceProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::local_inference::ASK_LOCAL_MODEL.into(),
                label: "Ask Local Model".into(),
                description: "Delegate a sub-task to a locally running LLM (zero API cost). The local model runs on-device via Ollama.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "Complete prompt. Include ALL necessary context." },
                        "system": { "type": "string", "description": "Optional system prompt." },
                        "model": { "type": "string", "description": "Specific model ID. Omit to auto-select." },
                        "temperature": { "type": "number", "description": "Sampling temperature 0.0-1.0 (default: 0.3)" },
                        "max_tokens": { "type": "number", "description": "Maximum response tokens (default: 2048)" }
                    },
                    "required": ["prompt"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::local_inference::LIST_LOCAL_MODELS.into(),
                label: "List Local Models".into(),
                description: "List all models currently available in the local inference server (Ollama).".into(),
                parameters: json!({ "type": "object", "properties": {} }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::local_inference::MANAGE_OLLAMA.into(),
                label: "Manage Ollama".into(),
                description: "Manage the Ollama local inference server: start, stop, check status, pull or delete models.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["start", "stop", "status", "pull", "delete"], "description": "Action to perform" },
                        "model": { "type": "string", "description": "Model name for 'pull' or 'delete' action" }
                    },
                    "required": ["action"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "ask_local_model" => {
                let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                let system = args.get("system").and_then(|v| v.as_str());
                let temperature = args
                    .get("temperature")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.3) as f32;
                let max_tokens = args
                    .get("max_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(2048) as u32;

                let model = if let Some(m) = args.get("model").and_then(|v| v.as_str()) {
                    m.to_string()
                } else {
                    self.auto_select_model()
                        .await
                        .unwrap_or_else(|| "qwen3:8b".into())
                };

                match self
                    .chat_completion(&model, prompt, system, temperature, max_tokens)
                    .await
                {
                    Ok(response) => Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("[Model: {model}]\n\n{response}"),
                        }],
                        details: json!({"model": model}),
                    }),
                    Err(e) => Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("Local model error: {e}"),
                        }],
                        details: json!({"error": true}),
                    }),
                }
            }
            "list_local_models" => match self.list_models().await {
                Ok(models) => {
                    let text = if models.is_empty() {
                        "No models available. Run `manage_ollama` with action 'pull' to download a model.".into()
                    } else {
                        let list: Vec<_> = models.iter().map(|m| format!("- {}", m.id)).collect();
                        format!("{} model(s) available:\n{}", models.len(), list.join("\n"))
                    };
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text { text }],
                        details: json!({"count": models.len()}),
                    })
                }
                Err(e) => Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Cannot list models: {e}. Is Ollama running?"),
                    }],
                    details: json!({"error": true}),
                }),
            },
            "manage_ollama" => {
                let action = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("status");
                let is_ollama_integration = std::env::args().any(|a| a == "--ollama-integration");
                let text = match action {
                    "status" => self.ollama_status().await,
                    "start" => self.ollama_start().await,
                    "stop" => self.ollama_stop(is_ollama_integration),
                    "pull" => {
                        let model = args
                            .get("model")
                            .and_then(|v| v.as_str())
                            .unwrap_or("qwen3:8b");
                        self.ollama_pull(model, None).await
                    }
                    "delete" => {
                        let model = args.get("model").and_then(|v| v.as_str()).unwrap_or("");
                        if model.is_empty() {
                            "Model name required for delete action.".into()
                        } else {
                            self.ollama_delete(model).await
                        }
                    }
                    _ => {
                        format!("Unknown action: {action}. Use: start, stop, status, pull, delete")
                    }
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details: json!({"action": action}),
                })
            }
            _ => anyhow::bail!("Unknown tool: {tool_name}"),
        }
    }

    /// Sink-aware override that routes `ask_local_model` through the
    /// streaming chat-completion path when a consumer is attached.
    /// `list_local_models` and `manage_ollama` are inherently single-shot
    /// HTTP calls and fall back to the buffered `execute` path.
    async fn execute_with_sink(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: CancellationToken,
        sink: ToolProgressSink,
    ) -> anyhow::Result<ToolResult> {
        // Route manage_ollama pull through the streaming path for progress.
        if tool_name == "manage_ollama" && sink.is_active() {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
            if action == "pull" {
                let model = args
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("qwen3:8b");
                let text = self.ollama_pull(model, Some(&sink)).await;
                return Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details: json!({"action": "pull", "model": model}),
                });
            }
        }

        // Only ask_local_model benefits from streaming. Skip the
        // streaming dispatch entirely if no sink is attached so the
        // non-streaming path stays the default for consumers that
        // don't need live token output.
        if tool_name != "ask_local_model" || !sink.is_active() {
            return self.execute(tool_name, call_id, args, cancel).await;
        }

        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let system = args.get("system").and_then(|v| v.as_str());
        let temperature = args
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.3) as f32;
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(2048) as u32;

        let model = if let Some(m) = args.get("model").and_then(|v| v.as_str()) {
            m.to_string()
        } else {
            self.auto_select_model()
                .await
                .unwrap_or_else(|| "qwen3:8b".into())
        };

        match self
            .chat_completion_streaming(&model, prompt, system, temperature, max_tokens, &sink)
            .await
        {
            Ok(response) => Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("[Model: {model}]\n\n{response}"),
                }],
                details: json!({"model": model}),
            }),
            Err(e) => Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("Local model error: {e}"),
                }],
                details: json!({"error": true}),
            }),
        }
    }
}

/// Execute local inference tools with standard CoreTools signature.
pub async fn execute(
    tool_name: &str,
    _call_id: &str,
    args: serde_json::Value,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<omegon_traits::ToolResult> {
    let provider = LocalInferenceProvider::new();
    provider.execute(tool_name, _call_id, args, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions() {
        let provider = LocalInferenceProvider::new();
        let tools = provider.tools();
        assert_eq!(tools.len(), 3);
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"ask_local_model"));
        assert!(names.contains(&"list_local_models"));
        assert!(names.contains(&"manage_ollama"));
    }

    #[test]
    fn base_url_default() {
        // Without env var, should return default
        let url = base_url();
        assert!(url.contains("11434") || env::var("LOCAL_INFERENCE_URL").is_ok());
    }

    #[test]
    fn streaming_chat_chunk_parses_openai_format() {
        // Verify the SSE chunk deserializer accepts the OpenAI-compatible
        // shape Ollama emits. Each chunk has `choices[0].delta.content`
        // which we extract and accumulate.
        let payload = r#"{"id":"chatcmpl-123","object":"chat.completion.chunk","created":1700000000,"model":"qwen3:8b","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#;
        let chunk: StreamingChatChunk = serde_json::from_str(payload).unwrap();
        assert_eq!(chunk.choices.len(), 1);
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("hello"));
    }

    #[test]
    fn streaming_chat_chunk_handles_empty_delta() {
        // OpenAI sends an initial chunk with `delta: {"role": "assistant"}`
        // and a final chunk with `delta: {}` followed by [DONE]. Both
        // should deserialize cleanly with `content` as None.
        let initial =
            r#"{"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#;
        let chunk: StreamingChatChunk = serde_json::from_str(initial).unwrap();
        assert!(chunk.choices[0].delta.content.is_none());

        let final_chunk = r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let chunk: StreamingChatChunk = serde_json::from_str(final_chunk).unwrap();
        assert!(chunk.choices[0].delta.content.is_none());
    }

    #[test]
    fn streaming_chat_chunk_skips_unrelated_fields() {
        // Real Ollama responses include extra fields beyond what we
        // model. The deserializer should ignore them.
        let payload = r#"{"id":"x","object":"chat.completion.chunk","model":"qwen","system_fingerprint":"fp_abc","choices":[{"index":0,"delta":{"content":"world","role":null},"logprobs":null,"finish_reason":null}],"usage":null}"#;
        let chunk: StreamingChatChunk = serde_json::from_str(payload).unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("world"));
    }
}

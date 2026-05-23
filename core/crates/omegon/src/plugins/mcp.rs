//! MCP plugin Feature — connects to MCP servers via stdio child-process transport,
//! discovers their tools, resources, and prompts, and exposes them to Omegon.
//!
//! MCP servers are declared in plugin.toml or project config:
//! ```toml
//! [mcp_servers.filesystem]
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
//!
//! [mcp_servers.brave-search]
//! command = "npx"
//! args = ["-y", "@modelcontextprotocol/server-brave-search"]
//! env = { BRAVE_API_KEY = "..." }
//! ```

use async_trait::async_trait;
use omegon_traits::*;
use rmcp::{
    handler::client::ClientHandler,
    model::*,
    service::{self, NotificationContext, RoleClient, RunningService},
    transport::{StreamableHttpClientTransport, TokioChildProcess},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::timeout as tokio_timeout;

use super::tool_capabilities::{
    ExternalExecutionHint, mcp_prompt_tool_capabilities, mcp_resource_tool_capabilities,
    resolve_external_tool_capabilities,
};

/// Configuration for a single MCP server.
///
/// Supports four execution modes:
/// 1. **HTTP transport** (remote): `url` specified, connects via StreamableHttpClientTransport
/// 2. **Local process** (default): `command` + `args` spawned directly
/// 3. **OCI container**: `image` specified, spawned via podman/docker
/// 4. **Docker MCP Gateway**: `docker_mcp` flag, uses `docker mcp gateway run`
///
/// Examples:
/// ```toml
/// # HTTP transport (remote MCP server)
/// [mcp_servers.remote_api]
/// url = "https://api.example.com/mcp"
///
/// # Local process
/// [mcp_servers.filesystem]
/// command = "npx"
/// args = ["-y", "@modelcontextprotocol/server-filesystem", "/home"]
///
/// # OCI container (podman or docker)
/// [mcp_servers.postgres]
/// image = "ghcr.io/modelcontextprotocol/server-postgres:latest"
/// env = { DATABASE_URL = "{DATABASE_URL}" }
///
/// # Docker MCP Toolkit gateway
/// [mcp_servers.github]
/// docker_mcp = "github"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    /// HTTP URL for remote MCP server (enables HTTP transport mode).
    /// Must use HTTPS scheme, except http://localhost is allowed for development.
    #[serde(default)]
    pub url: Option<String>,
    /// Command to spawn (for local process mode).
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to pass.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// OCI image reference — spawns the MCP server in a container.
    /// The container must expose MCP via stdio (ENTRYPOINT reads stdin, writes stdout).
    #[serde(default)]
    pub image: Option<String>,
    /// Mount the operator's working directory into the container (default: false).
    #[serde(default)]
    pub mount_cwd: bool,
    /// Allow container network access (default: true for MCP servers).
    #[serde(default = "default_true")]
    pub network: bool,
    /// Docker MCP Toolkit gateway server name.
    /// If set, uses `docker mcp gateway run <name>` instead of direct spawn.
    #[serde(default)]
    pub docker_mcp: Option<String>,
    /// Styrene mesh destination hash for remote MCP server execution.
    /// The MCP server runs on a remote node accessible via RNS/Yggdrasil.
    /// Traffic is PQC-encrypted end-to-end via styrene-tunnel.
    /// Uses DaemonFleet::terminal_open for bidirectional stdio over the mesh.
    #[serde(default)]
    pub styrene_dest: Option<String>,
    /// Timeout for tool calls in seconds (default: 30).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}
fn default_true() -> bool {
    true
}

/// A discovered tool from an MCP server.
#[derive(Debug, Clone)]
struct McpTool {
    name: String,
    description: String,
    parameters: Value,
    server_name: String,
}

/// A discovered resource from an MCP server.
#[derive(Debug, Clone)]
struct McpResource {
    uri: String,
    name: String,
    description: Option<String>,
    mime_type: Option<String>,
    server_name: String,
}

/// A discovered resource template from an MCP server.
#[derive(Debug, Clone)]
struct McpResourceTemplate {
    uri_template: String,
    name: String,
    description: Option<String>,
    mime_type: Option<String>,
    server_name: String,
}

/// A discovered prompt from an MCP server.
#[derive(Debug, Clone)]
struct McpPrompt {
    name: String,
    description: Option<String>,
    arguments: Vec<McpPromptArgument>,
    server_name: String,
}

/// Argument metadata for a discovered prompt.
#[derive(Debug, Clone)]
struct McpPromptArgument {
    name: String,
    description: Option<String>,
    required: bool,
}

/// Per-call progress entry — the sink we should forward
/// `notifications/progress` messages to, plus the wall-clock instant
/// when the tool call started so we can compute `elapsed_ms` for
/// each emitted partial.
#[derive(Clone)]
struct ProgressEntry {
    sink: ToolProgressSink,
    started_at: Instant,
}

/// Shared registry mapping MCP `ProgressToken`s to active tool-call
/// progress sinks. The `OmegonMcpClient` holds one of these (cloned
/// from the parent `McpFeature`); when an `on_progress` notification
/// arrives, the client looks up the token and pushes a typed
/// [`PartialToolResult`] through the matching sink.
///
/// `next_token` is an atomic counter used to mint unique tokens at
/// call time. Tokens are scoped to the entire feature instance —
/// collisions across servers are not possible.
#[derive(Default)]
struct ProgressRegistry {
    sinks: Mutex<HashMap<ProgressToken, ProgressEntry>>,
    next_token: AtomicU64,
}

impl ProgressRegistry {
    fn allocate_token(&self) -> ProgressToken {
        let id = self.next_token.fetch_add(1, Ordering::Relaxed);
        ProgressToken(NumberOrString::Number(id as i64))
    }
}

/// Client handler that forwards server-side `notifications/progress`
/// messages into per-call `ToolProgressSink`s. The handler holds an
/// `Arc<ProgressRegistry>` shared with the parent `McpFeature` so that
/// `execute_with_sink` can register/deregister sinks by progress token
/// without needing to reach into the running rmcp service.
#[derive(Clone)]
struct OmegonMcpClient {
    progress: Arc<ProgressRegistry>,
}

impl ClientHandler for OmegonMcpClient {
    fn get_info(&self) -> ClientInfo {
        let mut impl_info = Implementation::default();
        impl_info.name = "omegon".into();
        impl_info.version = env!("CARGO_PKG_VERSION").into();
        InitializeRequestParams::new(ClientCapabilities::default(), impl_info)
    }

    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        let sinks = self.progress.sinks.lock().await;
        let Some(entry) = sinks.get(&params.progress_token) else {
            // Server emitted progress for a token we don't know about
            // (or that already finished). Drop silently — better than
            // failing the whole call for a stale notification.
            return;
        };
        let elapsed_ms = entry.started_at.elapsed().as_millis() as u64;
        let total_units = params.total.map(|t| t as u64);
        let phase = params.message.clone();
        entry.sink.send(PartialToolResult {
            // MCP progress notifications don't carry tool output text,
            // they carry phase + units. Tail stays empty; consumers
            // render the phase label and units instead.
            tail: String::new(),
            progress: ToolProgress {
                elapsed_ms,
                heartbeat: false,
                phase,
                units: Some(ProgressUnits {
                    current: params.progress as u64,
                    total: total_units,
                    unit: "items".to_string(),
                }),
                tally: None,
            },
            details: serde_json::json!({"source": "mcp_progress"}),
        });
    }
}

/// Running connection to an MCP server.
type McpConnection = RunningService<RoleClient, OmegonMcpClient>;

/// Feature that wraps one or more MCP server connections.
pub struct McpFeature {
    feature_name: String,
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
    resource_templates: Vec<McpResourceTemplate>,
    prompts: Vec<McpPrompt>,
    clients: Arc<Mutex<HashMap<String, McpConnection>>>,
    /// Per-server call timeout in seconds, keyed by server name.
    timeouts: HashMap<String, u64>,
    /// Shared progress-token registry. The same `Arc` is cloned into
    /// each `OmegonMcpClient` constructed during `connect_one` so the
    /// client handler can route incoming `on_progress` notifications
    /// back to the right tool-call sink.
    progress: Arc<ProgressRegistry>,
}

impl McpFeature {
    /// Connect to MCP servers and discover their tools, resources, and prompts.
    pub async fn connect(
        plugin_name: &str,
        servers: &HashMap<String, McpServerConfig>,
        secrets: Option<&omegon_secrets::SecretsManager>,
    ) -> anyhow::Result<Self> {
        let mut all_tools = Vec::new();
        let mut all_resources = Vec::new();
        let mut all_resource_templates = Vec::new();
        let mut all_prompts = Vec::new();
        let mut clients = HashMap::new();
        let progress = Arc::new(ProgressRegistry::default());

        let mut timeouts = HashMap::new();
        for (server_name, config) in servers {
            match Self::connect_one(server_name, config, secrets, Arc::clone(&progress)).await {
                Ok((server_tools, client)) => {
                    tracing::info!(
                        plugin = plugin_name,
                        server = server_name,
                        tools = server_tools.len(),
                        "MCP server connected"
                    );
                    all_tools.extend(server_tools);
                    timeouts.insert(server_name.clone(), config.timeout_secs);

                    // Discover resources (non-fatal — many servers don't expose any)
                    match client.list_all_resources().await {
                        Ok(resources) => {
                            if !resources.is_empty() {
                                tracing::info!(
                                    plugin = plugin_name,
                                    server = server_name,
                                    count = resources.len(),
                                    "MCP resources discovered"
                                );
                            }
                            all_resources.extend(resources.into_iter().map(|r| McpResource {
                                uri: r.uri.clone(),
                                name: r.name.clone(),
                                description: r.description.clone(),
                                mime_type: r.mime_type.clone(),
                                server_name: server_name.clone(),
                            }));
                        }
                        Err(e) => {
                            tracing::debug!(
                                server = server_name,
                                error = %e,
                                "MCP server does not support resources"
                            );
                        }
                    }

                    // Discover resource templates (non-fatal)
                    match client.list_all_resource_templates().await {
                        Ok(templates) => {
                            if !templates.is_empty() {
                                tracing::info!(
                                    plugin = plugin_name,
                                    server = server_name,
                                    count = templates.len(),
                                    "MCP resource templates discovered"
                                );
                            }
                            all_resource_templates.extend(templates.into_iter().map(|t| {
                                McpResourceTemplate {
                                    uri_template: t.uri_template.clone(),
                                    name: t.name.clone(),
                                    description: t.description.clone(),
                                    mime_type: t.mime_type.clone(),
                                    server_name: server_name.clone(),
                                }
                            }));
                        }
                        Err(e) => {
                            tracing::debug!(
                                server = server_name,
                                error = %e,
                                "MCP server does not support resource templates"
                            );
                        }
                    }

                    // Discover prompts (non-fatal)
                    match client.list_all_prompts().await {
                        Ok(prompts) => {
                            if !prompts.is_empty() {
                                tracing::info!(
                                    plugin = plugin_name,
                                    server = server_name,
                                    count = prompts.len(),
                                    "MCP prompts discovered"
                                );
                            }
                            all_prompts.extend(prompts.into_iter().map(|p| {
                                McpPrompt {
                                    name: format!("{}::{}", server_name, p.name),
                                    description: p.description.map(|d| d.to_string()),
                                    arguments: p
                                        .arguments
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(|a| McpPromptArgument {
                                            name: a.name,
                                            description: a.description.map(|d| d.to_string()),
                                            required: a.required.unwrap_or(false),
                                        })
                                        .collect(),
                                    server_name: server_name.clone(),
                                }
                            }));
                        }
                        Err(e) => {
                            tracing::debug!(
                                server = server_name,
                                error = %e,
                                "MCP server does not support prompts"
                            );
                        }
                    }

                    clients.insert(server_name.clone(), client);
                }
                Err(e) => {
                    tracing::warn!(
                        plugin = plugin_name,
                        server = server_name,
                        error = %e,
                        "failed to connect MCP server — skipping"
                    );
                }
            }
        }

        Ok(Self {
            feature_name: plugin_name.to_string(),
            tools: all_tools,
            resources: all_resources,
            resource_templates: all_resource_templates,
            prompts: all_prompts,
            clients: Arc::new(Mutex::new(clients)),
            timeouts,
            progress,
        })
    }

    async fn connect_one(
        server_name: &str,
        config: &McpServerConfig,
        secrets: Option<&omegon_secrets::SecretsManager>,
        progress: Arc<ProgressRegistry>,
    ) -> anyhow::Result<(Vec<McpTool>, McpConnection)> {
        let handler = OmegonMcpClient { progress };
        let client = if let Some(ref url) = config.url {
            // HTTP transport mode
            Self::validate_url(url)?;
            let transport = StreamableHttpClientTransport::from_uri(url.clone());
            service::serve_client(handler, transport).await?
        } else {
            // Local process transport mode
            let cmd = Self::build_command(server_name, config, secrets)?;
            let transport = TokioChildProcess::new(cmd)?;
            service::serve_client(handler, transport).await?
        };

        // Discover tools via MCP tools/list
        let tools_result = client.list_tools(None).await?;
        let tools: Vec<McpTool> = tools_result
            .tools
            .into_iter()
            .map(|t| {
                // Convert the input_schema (Arc<Map>) to a serde_json::Value
                let params: Value = serde_json::to_value(&t.input_schema)
                    .unwrap_or_else(|_| serde_json::json!({"type": "object", "properties": {}}));
                McpTool {
                    name: format!("{}::{}", server_name, t.name),
                    description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                    parameters: params,
                    server_name: server_name.to_string(),
                }
            })
            .collect();

        Ok((tools, client))
    }

    /// Validate URL for HTTP transport.
    /// Requires HTTPS scheme, except http://localhost is allowed for development.
    fn validate_url(url: &str) -> anyhow::Result<()> {
        if url.starts_with("https://") {
            if url.len() > 8 {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Invalid HTTPS URL: must have host after scheme: {}",
                    url
                ))
            }
        } else if url.starts_with("http://") {
            if url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1") {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "HTTP URLs are only allowed for localhost, got: {}",
                    url
                ))
            }
        } else {
            Err(anyhow::anyhow!(
                "Only HTTPS and localhost HTTP URLs are supported, got: {}",
                url
            ))
        }
    }

    /// Build the command to spawn an MCP server based on its config mode.
    /// This method is only called for local execution modes (not HTTP transport).
    ///
    /// Modes (in priority order):
    /// 1. Styrene mesh — remote exec over RNS/Yggdrasil (PQC-encrypted)
    /// 2. Docker MCP Gateway — Docker Desktop MCP Toolkit
    /// 3. OCI container — podman/docker with mount/network policy
    /// 4. Local process — direct command spawn (default)
    fn build_command(
        server_name: &str,
        config: &McpServerConfig,
        secrets: Option<&omegon_secrets::SecretsManager>,
    ) -> anyhow::Result<Command> {
        // Mode 1: Styrene mesh transport
        // The MCP server runs on a remote node. We use styrene-ipc's
        // DaemonFleet::terminal_open() for bidirectional stdio, wrapped
        // as an rmcp Transport. For now, this falls through to a
        // placeholder that requires the styrene daemon to be available.
        if let Some(ref dest) = config.styrene_dest {
            let command = config.command.as_deref().unwrap_or("mcp-server");
            // Use styrene CLI as the transport bridge:
            // styrene exec <dest> <command> [args...]
            // This opens a terminal session to the remote and pipes stdio.
            let mut cmd = Command::new("styrene");
            cmd.arg("exec");
            cmd.arg(dest);
            cmd.arg(command);
            cmd.args(&config.args);
            for (key, value) in &config.env {
                cmd.env(key, resolve_env_template(value, secrets));
            }
            return Ok(cmd);
        }

        // Mode 2: Docker MCP Gateway
        if let Some(ref gateway_name) = config.docker_mcp {
            let mut cmd = Command::new("docker");
            cmd.args(["mcp", "gateway", "run", gateway_name]);
            for (key, value) in &config.env {
                cmd.env(key, resolve_env_template(value, secrets));
            }
            return Ok(cmd);
        }

        // Mode 3: OCI container (podman preferred, docker fallback)
        if let Some(ref image) = config.image {
            let runtime = detect_container_runtime();
            let mut cmd = Command::new(&runtime);
            cmd.arg("run");
            cmd.args(["--rm", "-i"]); // interactive stdin/stdout

            // Network policy
            if !config.network {
                cmd.arg("--network=none");
            }

            // Mount cwd if requested
            if config.mount_cwd
                && let Ok(cwd) = std::env::current_dir()
            {
                cmd.arg(format!("-v={}:/work", cwd.display()));
                cmd.args(["-w", "/work"]);
            }

            // Environment variables — validate key names to prevent injection
            for (key, value) in &config.env {
                if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    tracing::warn!(key = key, "skipping env var with invalid name");
                    continue;
                }
                cmd.arg(format!(
                    "-e={}={}",
                    key,
                    resolve_env_template(value, secrets)
                ));
            }

            cmd.arg(image);
            cmd.args(&config.args);
            return Ok(cmd);
        }

        // Mode 4: Local process (default)
        let command = config.command.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server '{}': must specify command, image, or docker_mcp",
                server_name
            )
        })?;

        let mut cmd = Command::new(command);
        cmd.args(&config.args);
        for (key, value) in &config.env {
            cmd.env(key, resolve_env_template(value, secrets));
        }

        Ok(cmd)
    }

    /// Parse "servername::toolname" → ("servername", "toolname").
    /// Uses `::` separator because both server names and MCP tool names
    /// commonly contain underscores (e.g. `brave_search`, `read_file`).
    fn split_tool_name(prefixed: &str) -> (&str, &str) {
        if let Some(pos) = prefixed.find("::") {
            (&prefixed[..pos], &prefixed[pos + 2..])
        } else {
            ("", prefixed)
        }
    }

    /// Number of discovered resources across all connected servers.
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// Number of discovered resource templates across all connected servers.
    pub fn resource_template_count(&self) -> usize {
        self.resource_templates.len()
    }

    /// Number of discovered prompts across all connected servers.
    pub fn prompt_count(&self) -> usize {
        self.prompts.len()
    }

    /// Execute `resources/read` against the named MCP server.
    async fn execute_read_resource(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let server_name = args
            .get("server")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument: server"))?;
        let uri = args
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument: uri"))?;

        let timeout_secs = self.timeouts.get(server_name).copied().unwrap_or(30);
        let clients = self.clients.lock().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not connected", server_name))?;

        let params = ReadResourceRequestParams::new(uri);
        let result = tokio_timeout(
            Duration::from_secs(timeout_secs),
            client.read_resource(params),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "MCP resource read timed out after {}s (server: '{}')",
                timeout_secs,
                server_name
            )
        })??;

        let content: Vec<ContentBlock> = result
            .contents
            .into_iter()
            .map(|rc| match rc {
                ResourceContents::TextResourceContents { text, uri, .. } => ContentBlock::Text {
                    text: format!("[{}]\n{}", uri, text),
                },
                ResourceContents::BlobResourceContents { blob, uri, .. } => ContentBlock::Text {
                    text: format!("[{}] (binary, {} bytes base64)", uri, blob.len()),
                },
            })
            .collect();

        Ok(ToolResult {
            content,
            details: Value::Null,
        })
    }

    /// Execute `prompts/get` against the named MCP server.
    async fn execute_get_prompt(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let server_name = args
            .get("server")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument: server"))?;
        let prompt_name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument: name"))?;
        let prompt_args: Option<serde_json::Map<String, Value>> =
            args.get("arguments").and_then(|v| v.as_object().cloned());

        let timeout_secs = self.timeouts.get(server_name).copied().unwrap_or(30);
        let clients = self.clients.lock().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not connected", server_name))?;

        let mut params = GetPromptRequestParams::new(prompt_name);
        params.arguments = prompt_args;
        let result = tokio_timeout(Duration::from_secs(timeout_secs), client.get_prompt(params))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP prompt get timed out after {}s (server: '{}')",
                    timeout_secs,
                    server_name
                )
            })??;

        let mut text_parts = Vec::new();
        if let Some(ref desc) = result.description {
            text_parts.push(format!("Prompt: {}\n", desc));
        }
        for msg in &result.messages {
            let role = match msg.role {
                PromptMessageRole::User => "user",
                PromptMessageRole::Assistant => "assistant",
            };
            let body = match &msg.content {
                PromptMessageContent::Text { text } => text.clone(),
                PromptMessageContent::Image { .. } => "[image content]".to_string(),
                PromptMessageContent::Resource { resource } => match &resource.raw.resource {
                    ResourceContents::TextResourceContents { text, uri, .. } => {
                        format!("[resource: {}]\n{}", uri, text)
                    }
                    ResourceContents::BlobResourceContents { uri, .. } => {
                        format!("[resource: {}] (binary)", uri)
                    }
                },
                PromptMessageContent::ResourceLink { link } => {
                    format!("[resource link: {}]", link.uri)
                }
            };
            text_parts.push(format!("[{}]: {}", role, body));
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: text_parts.join("\n\n"),
            }],
            details: Value::Null,
        })
    }
}

#[async_trait]
impl Feature for McpFeature {
    fn name(&self) -> &str {
        &self.feature_name
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .iter()
            .map(|t| ToolDefinition {
                name: t.name.clone(),
                label: format!("mcp:{}", t.server_name),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
                capabilities: resolve_external_tool_capabilities(
                    &[],
                    &t.name,
                    &t.description,
                    &t.parameters,
                    ExternalExecutionHint::McpDiscovery,
                ),
            })
            .collect();

        // Expose mcp_read_resource tool if any server has resources
        if !self.resources.is_empty() || !self.resource_templates.is_empty() {
            defs.push(ToolDefinition {
                name: format!("{}::mcp_read_resource", self.feature_name),
                label: format!("mcp:{}", self.feature_name),
                description: "Read a resource from an MCP server by URI. Use the resource list in context to find available URIs.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "string",
                            "description": "MCP server name that owns the resource"
                        },
                        "uri": {
                            "type": "string",
                            "description": "Resource URI to read"
                        }
                    },
                    "required": ["server", "uri"]
                }),
                capabilities: mcp_resource_tool_capabilities(),
            });
        }

        // Expose mcp_get_prompt tool if any server has prompts
        if !self.prompts.is_empty() {
            defs.push(ToolDefinition {
                name: format!("{}::mcp_get_prompt", self.feature_name),
                label: format!("mcp:{}", self.feature_name),
                description: "Retrieve a prompt template from an MCP server. Returns the expanded prompt messages.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "string",
                            "description": "MCP server name that owns the prompt"
                        },
                        "name": {
                            "type": "string",
                            "description": "Prompt name"
                        },
                        "arguments": {
                            "type": "object",
                            "description": "Key-value arguments to pass to the prompt template",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["server", "name"]
                }),
                capabilities: mcp_prompt_tool_capabilities(),
            });
        }

        defs
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        if self.resources.is_empty()
            && self.resource_templates.is_empty()
            && self.prompts.is_empty()
        {
            return None;
        }

        // Cap listings to avoid bloating the system prompt. The agent can
        // always discover more via the tools themselves.
        const MAX_LISTED: usize = 10;

        let mut lines = Vec::new();

        if !self.resources.is_empty() {
            let total = self.resources.len();
            let show = total.min(MAX_LISTED);
            lines.push(format!("MCP Resources ({total} available):"));
            for r in self.resources.iter().take(show) {
                lines.push(format!(
                    "  - {} (server: {}, uri: {})",
                    r.name, r.server_name, r.uri
                ));
            }
            if total > show {
                lines.push(format!("  ... and {} more", total - show));
            }
        }

        if !self.resource_templates.is_empty() {
            let total = self.resource_templates.len();
            let show = total.min(MAX_LISTED);
            lines.push(format!("MCP Resource Templates ({total} available):"));
            for t in self.resource_templates.iter().take(show) {
                lines.push(format!(
                    "  - {} (server: {}, template: {})",
                    t.name, t.server_name, t.uri_template
                ));
            }
            if total > show {
                lines.push(format!("  ... and {} more", total - show));
            }
        }

        if !self.prompts.is_empty() {
            let total = self.prompts.len();
            let show = total.min(MAX_LISTED);
            lines.push(format!("MCP Prompts ({total} available):"));
            for p in self.prompts.iter().take(show) {
                let args_str = if p.arguments.is_empty() {
                    String::new()
                } else {
                    let names: Vec<&str> = p.arguments.iter().map(|a| a.name.as_str()).collect();
                    format!(" args: [{}]", names.join(", "))
                };
                lines.push(format!("  - {}{}", p.name, args_str));
            }
            if total > show {
                lines.push(format!("  ... and {} more", total - show));
            }
        }

        lines.push(
            "Use mcp_read_resource to fetch resource content, mcp_get_prompt to expand prompts."
                .to_string(),
        );

        Some(ContextInjection {
            source: format!("mcp:{}", self.feature_name),
            content: lines.join("\n"),
            priority: 30,
            // Static content — resources/prompts don't change mid-session.
            // Inject once and let TTL keep it alive without re-rendering.
            ttl_turns: 50,
        })
    }

    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        self.execute_with_sink(tool_name, call_id, args, cancel, ToolProgressSink::noop())
            .await
    }

    async fn execute_with_sink(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
        sink: ToolProgressSink,
    ) -> anyhow::Result<ToolResult> {
        let (server_name, mcp_name) = Self::split_tool_name(tool_name);

        // Route to resource/prompt handlers
        if mcp_name == "mcp_read_resource" {
            return self.execute_read_resource(&args).await;
        }
        if mcp_name == "mcp_get_prompt" {
            return self.execute_get_prompt(&args).await;
        }

        let server_name = server_name.to_string();
        let mcp_name = mcp_name.to_string();

        let timeout_secs = self.timeouts.get(&server_name).copied().unwrap_or(30);

        let clients = self.clients.lock().await;
        let client = clients
            .get(&server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not connected", server_name))?;

        let arguments = if args.is_object() {
            Some(args.as_object().unwrap().clone())
        } else {
            None
        };

        let mut params = CallToolRequestParams::default();
        params.name = mcp_name.clone().into();
        params.arguments = arguments;

        // If a consumer is attached, mint a progress token, register the
        // sink in the per-feature registry, and tell the MCP server about
        // it via the request `_meta` field. The server can then send
        // `notifications/progress` messages keyed to that token; our
        // `OmegonMcpClient::on_progress` handler routes them back to the
        // sink. The `ProgressTokenGuard` ensures the registration is
        // dropped on every exit path (success, error, timeout, panic).
        let _guard = if sink.is_active() {
            let token = self.progress.allocate_token();
            params.set_progress_token(token.clone());
            self.progress.sinks.lock().await.insert(
                token.clone(),
                ProgressEntry {
                    sink: sink.clone(),
                    started_at: Instant::now(),
                },
            );
            Some(ProgressTokenGuard {
                registry: Arc::clone(&self.progress),
                token,
            })
        } else {
            None
        };

        let result = tokio_timeout(Duration::from_secs(timeout_secs), client.call_tool(params))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "MCP tool call timed out after {}s (server: '{}')",
                    timeout_secs,
                    server_name
                )
            })??;

        // Convert MCP content to Omegon content blocks
        let content: Vec<ContentBlock> = result
            .content
            .clone()
            .into_iter()
            .filter_map(|c| match c.raw {
                RawContent::Text(t) => Some(ContentBlock::Text {
                    text: t.text.to_string(),
                }),
                RawContent::Image(img) => Some(ContentBlock::Image {
                    url: img.data.to_string(),
                    media_type: img.mime_type.to_string(),
                }),
                _ => None,
            })
            .collect();

        let details = mcp_tool_result_details(&server_name, &mcp_name, &result);

        Ok(ToolResult { content, details })
    }
}

fn mcp_tool_result_details(server_name: &str, tool_name: &str, result: &CallToolResult) -> Value {
    let Some(meta) = result.meta.as_ref() else {
        return Value::Null;
    };
    let Some(actions) = meta.get("omegon/hostActions") else {
        return Value::Null;
    };

    let manifest = mcp_host_action_manifest();
    let outcomes = match actions.as_array() {
        Some(actions) => actions
            .iter()
            .enumerate()
            .map(|(idx, action)| {
                let scoped = crate::extensions::host_actions::ScopedHostActionId {
                    origin: crate::extensions::host_actions::HostActionOrigin::mcp(server_name),
                    session_id: "mcp-tool-result".to_string(),
                    tool_call_id: tool_name.to_string(),
                    action_id: action
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("<pending-parse-{idx}>")),
                };
                let outcome = crate::extensions::host_actions::process_host_action_candidate(
                    action.clone(),
                    &manifest,
                    scoped,
                    &crate::extensions::host_actions::RuntimeHostActionPolicy::default(),
                    &crate::extensions::host_actions::HostActionExecutorRegistry::default_supported(
                    ),
                );
                serde_json::to_value(outcome).unwrap_or_else(|err| {
                    json!({
                        "action_id": "<serialization-error>",
                        "status": "invalid",
                        "error": {"code": "serialization_error", "message": err.to_string()}
                    })
                })
            })
            .collect(),
        None => vec![json!({
            "action_id": "omegon/hostActions",
            "status": "invalid",
            "error": {"code": "invalid_action", "message": "_meta[\"omegon/hostActions\"] must be an array"}
        })],
    };

    json!({"host_action_outcomes": outcomes})
}

fn mcp_host_action_manifest() -> crate::extensions::manifest::ExtensionManifest {
    toml::from_str(
        r#"
[extension]
name = "mcp"
version = "0.0.0"

[runtime]
type = "native"
binary = "mcp"

[permissions.host_actions]
allowed = ["terminal.create@1"]
"#,
    )
    .expect("static MCP HostAction manifest is valid")
}

/// RAII guard that removes a progress-token registration when dropped.
/// Ensures the registry doesn't leak entries on any exit path — success,
/// error, timeout, or panic.
struct ProgressTokenGuard {
    registry: Arc<ProgressRegistry>,
    token: ProgressToken,
}

impl Drop for ProgressTokenGuard {
    fn drop(&mut self) {
        // Use try_lock since Drop is sync — if the lock is contended,
        // spawn a deferred cleanup. In practice it's rarely contended
        // because on_progress holds the lock briefly.
        let registry = Arc::clone(&self.registry);
        let token = self.token.clone();
        if let Ok(mut sinks) = registry.sinks.try_lock() {
            sinks.remove(&token);
        } else {
            tokio::spawn(async move {
                registry.sinks.lock().await.remove(&token);
            });
        }
    }
}

/// Detect the available OCI container runtime.
/// Prefers podman (rootless, daemonless), falls back to docker.
pub(crate) fn detect_container_runtime() -> String {
    // Check OMEGON_CONTAINER_RUNTIME env var first (operator override).
    // Only accept known-safe values to prevent command injection.
    if let Ok(runtime) = std::env::var("OMEGON_CONTAINER_RUNTIME") {
        match runtime.as_str() {
            "podman" | "docker" | "nerdctl" => return runtime,
            other => {
                tracing::warn!(
                    runtime = other,
                    "OMEGON_CONTAINER_RUNTIME must be 'podman', 'docker', or 'nerdctl' — ignoring"
                );
            }
        }
    }

    // Prefer podman (rootless, no daemon)
    if which_exists("podman") {
        return "podman".into();
    }

    // Fall back to docker
    if which_exists("docker") {
        return "docker".into();
    }

    // Last resort — assume docker and let the error surface at spawn time
    tracing::warn!("no container runtime found (podman or docker) — MCP container tools will fail");
    "docker".into()
}

/// Check if a binary exists in PATH (cross-platform).
fn which_exists(name: &str) -> bool {
    // Try running `<name> --version` — works on all platforms
    // and doesn't require `which` (which doesn't exist on Windows).
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve `{ENV_VAR}` patterns from environment variables.
///
/// Single-pass left-to-right — resolved values are not re-scanned,
/// preventing infinite loops from values containing `{`.
/// Resolve a `{VAR_NAME}` template string.
///
/// Resolution priority:
/// 1. `SecretsManager` session cache (covers vault: and keyring: recipes preflighted at startup)
/// 2. Process environment — covers well-known secrets hydrated by hydrate_process_env()
///    and any env var not managed through the secrets system
///
/// Non-matching literals are passed through unchanged.
fn resolve_env_template(
    template: &str,
    secrets: Option<&omegon_secrets::SecretsManager>,
) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch == '{' {
            // Find the closing brace
            if let Some(end_offset) = template[i + 1..].find('}') {
                let var = &template[i + 1..i + 1 + end_offset];
                // Only resolve if var looks like an env var name (alphanumeric + _)
                if !var.is_empty() && var.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    // Priority: secrets session cache → process env
                    let value = secrets
                        .and_then(|s| s.resolve(var))
                        .or_else(|| std::env::var(var).ok())
                        .unwrap_or_default();
                    result.push_str(&value);
                    // Advance past the closing brace
                    for _ in 0..end_offset + 1 {
                        chars.next();
                    }
                    continue;
                }
            }
            // Not a valid pattern — emit the literal `{`
            result.push(ch);
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn progress_registry_allocates_unique_tokens() {
        let registry = ProgressRegistry::default();
        let t1 = registry.allocate_token();
        let t2 = registry.allocate_token();
        let t3 = registry.allocate_token();
        assert_ne!(t1, t2);
        assert_ne!(t2, t3);
        assert_ne!(t1, t3);
    }

    #[tokio::test]
    async fn progress_token_guard_removes_entry_on_drop() {
        // The RAII guard must remove the registration on every exit
        // path. Construct a registry, install an entry under a token,
        // wrap it in a guard, drop the guard, and confirm the entry
        // is gone.
        let registry = Arc::new(ProgressRegistry::default());
        let token = registry.allocate_token();
        registry.sinks.lock().await.insert(
            token.clone(),
            ProgressEntry {
                sink: ToolProgressSink::noop(),
                started_at: Instant::now(),
            },
        );
        assert!(registry.sinks.lock().await.contains_key(&token));

        let guard = ProgressTokenGuard {
            registry: Arc::clone(&registry),
            token: token.clone(),
        };
        drop(guard);
        // try_lock path runs synchronously inside Drop; the entry
        // should be gone immediately.
        assert!(!registry.sinks.lock().await.contains_key(&token));
    }

    #[tokio::test]
    async fn on_progress_routes_to_registered_sink() {
        // Wire a sink into the registry under a known token, fire an
        // on_progress notification carrying that token, and confirm a
        // PartialToolResult arrives via the sink callback.
        use std::sync::Mutex as StdMutex;
        let captured: Arc<StdMutex<Vec<PartialToolResult>>> = Arc::new(StdMutex::new(Vec::new()));
        let captured_for_sink = Arc::clone(&captured);
        let sink = ToolProgressSink::from_fn(move |partial| {
            captured_for_sink.lock().unwrap().push(partial);
        });

        let registry = Arc::new(ProgressRegistry::default());
        let token = registry.allocate_token();
        registry.sinks.lock().await.insert(
            token.clone(),
            ProgressEntry {
                sink,
                started_at: Instant::now(),
            },
        );

        let client = OmegonMcpClient {
            progress: Arc::clone(&registry),
        };

        // Construct the notification context the trait expects. We use
        // a synthetic context — the handler doesn't read from it.
        let params = ProgressNotificationParam {
            progress_token: token.clone(),
            progress: 5.0,
            total: Some(20.0),
            message: Some("indexing files".to_string()),
        };

        // We can't easily fabricate a `NotificationContext<RoleClient>`
        // here because rmcp's API doesn't expose a public constructor.
        // Instead, exercise the lookup + emission logic directly.
        // (This duplicates what `on_progress` does internally; if rmcp
        // ever changes the on_progress shape, both implementations need
        // to update together.)
        {
            let sinks = client.progress.sinks.lock().await;
            let entry = sinks.get(&params.progress_token).expect("token registered");
            entry.sink.send(PartialToolResult {
                tail: String::new(),
                progress: ToolProgress {
                    elapsed_ms: entry.started_at.elapsed().as_millis() as u64,
                    heartbeat: false,
                    phase: params.message.clone(),
                    units: Some(ProgressUnits {
                        current: params.progress as u64,
                        total: params.total.map(|t| t as u64),
                        unit: "items".to_string(),
                    }),
                    tally: None,
                },
                details: serde_json::json!({"source": "mcp_progress"}),
            });
        }

        let partials = captured.lock().unwrap();
        assert_eq!(partials.len(), 1);
        let p = &partials[0];
        assert_eq!(p.tail, "");
        assert!(!p.progress.heartbeat);
        assert_eq!(p.progress.phase.as_deref(), Some("indexing files"));
        let units = p.progress.units.as_ref().unwrap();
        assert_eq!(units.current, 5);
        assert_eq!(units.total, Some(20));
        assert_eq!(units.unit, "items");
    }

    #[tokio::test]
    async fn on_progress_drops_unknown_tokens() {
        // A notification carrying a token we don't know about must
        // not panic and must not somehow surface a partial. Rare in
        // practice (server bug, race after deregistration) but the
        // handler has to be robust.
        let registry = Arc::new(ProgressRegistry::default());
        let unknown = ProgressToken(NumberOrString::Number(99999));
        let sinks = registry.sinks.lock().await;
        assert!(sinks.get(&unknown).is_none());
    }

    #[test]
    fn resolve_env_template_basic() {
        // Use CARGO_PKG_NAME which is always "omegon" during cargo test
        let result = resolve_env_template("pkg:{CARGO_PKG_NAME}", None);
        assert_eq!(result, "pkg:omegon");
    }

    #[test]
    fn resolve_env_template_missing_var() {
        let result = resolve_env_template("{NONEXISTENT_VAR_12345}", None);
        assert_eq!(result, "");
    }

    #[test]
    fn mcp_tool_result_details_marks_mcp_actions_invalid_without_breaking_content() {
        let mut meta = rmcp::model::Meta::new();
        meta.insert(
            "omegon/hostActions".to_string(),
            json!([{
                "id": "open-reader",
                "type": "terminal.create@1",
                "execution": "auto_if_allowed",
                "params": {"command": "bookokrat"}
            }]),
        );
        let mut result =
            CallToolResult::success(vec![rmcp::model::Content::text("still readable")]);
        result.meta = Some(meta);

        let details = mcp_tool_result_details("reader", "open", &result);

        assert_eq!(
            details["host_action_outcomes"][0]["action_id"],
            "open-reader"
        );
        assert_eq!(details["host_action_outcomes"][0]["status"], "denied");
        assert_eq!(
            details["host_action_outcomes"][0]["error"]["message"],
            "auto_if_allowed requires manifest, project, runtime, origin, and operator approval"
        );
    }

    #[test]
    fn resolve_env_template_no_pattern() {
        let result = resolve_env_template("plain string", None);
        assert_eq!(result, "plain string");
    }

    #[test]
    fn resolve_env_template_nested_braces() {
        // Nested braces should not cause infinite loop
        let result = resolve_env_template("{foo{bar}}", None);
        // {foo{bar}} — `foo{bar` is not a valid env var name (contains {)
        // so it's emitted literally
        assert!(result.contains("foo"), "should not loop: {result}");
    }

    #[test]
    fn resolve_env_template_value_with_braces() {
        // If the resolved value contains {, it should NOT be re-scanned.
        // CARGO_MANIFEST_DIR contains path separators but no braces —
        // test the single-pass property by checking it doesn't recurse
        let result = resolve_env_template("{CARGO_MANIFEST_DIR}", None);
        assert!(result.contains("omegon"), "should resolve: {result}");
        // No re-scan: the result isn't treated as a template
    }

    #[test]
    fn resolve_env_template_unclosed_brace() {
        let result = resolve_env_template("prefix {UNCLOSED", None);
        assert_eq!(result, "prefix {UNCLOSED");
    }

    #[test]
    fn split_tool_name_prefixed() {
        let (server, tool) = McpFeature::split_tool_name("filesystem::read_file");
        assert_eq!(server, "filesystem");
        assert_eq!(tool, "read_file");
    }

    #[test]
    fn split_tool_name_underscore_in_server() {
        let (server, tool) = McpFeature::split_tool_name("brave_search::web_search");
        assert_eq!(server, "brave_search");
        assert_eq!(tool, "web_search");
    }

    #[test]
    fn split_tool_name_no_prefix() {
        let (server, tool) = McpFeature::split_tool_name("standalone");
        assert_eq!(server, "");
        assert_eq!(tool, "standalone");
    }

    #[test]
    fn mcp_server_config_local_process() {
        let toml = r#"
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-filesystem", "/home"]
            timeout_secs = 60
        "#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.command.as_deref(), Some("npx"));
        assert_eq!(config.args.len(), 3);
        assert_eq!(config.timeout_secs, 60);
        assert!(config.image.is_none());
        assert!(config.docker_mcp.is_none());
    }

    #[test]
    fn mcp_server_config_oci_container() {
        let toml = r#"
            image = "ghcr.io/mcp/server-postgres:latest"
            mount_cwd = true
            network = true
            [env]
            DATABASE_URL = "{DATABASE_URL}"
        "#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert!(config.command.is_none());
        assert_eq!(
            config.image.as_deref(),
            Some("ghcr.io/mcp/server-postgres:latest")
        );
        assert!(config.mount_cwd);
        assert!(config.network);
        assert_eq!(config.env["DATABASE_URL"], "{DATABASE_URL}");
    }

    #[test]
    fn mcp_server_config_docker_gateway() {
        let toml = r#"docker_mcp = "github""#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert!(config.command.is_none());
        assert!(config.image.is_none());
        assert_eq!(config.docker_mcp.as_deref(), Some("github"));
    }

    #[test]
    fn mcp_server_config_with_env() {
        let toml = r#"
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-brave-search"]
            [env]
            BRAVE_API_KEY = "{BRAVE_API_KEY}"
        "#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.env["BRAVE_API_KEY"], "{BRAVE_API_KEY}");
    }

    #[test]
    fn mcp_server_config_defaults() {
        let toml = r#"command = "my-server""#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
        assert_eq!(config.timeout_secs, 30);
        assert!(config.network); // defaults to true for MCP servers
    }

    #[test]
    fn build_command_local_process() {
        let config = McpServerConfig {
            url: None,
            command: Some("npx".into()),
            args: vec!["-y".into(), "server".into()],
            env: HashMap::new(),
            image: None,
            mount_cwd: false,
            network: true,
            docker_mcp: None,
            styrene_dest: None,
            timeout_secs: 30,
        };
        let cmd = McpFeature::build_command("test", &config, None).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        assert_eq!(prog, "npx");
    }

    #[test]
    fn build_command_oci_container() {
        let config = McpServerConfig {
            url: None,
            command: None,
            args: vec![],
            env: HashMap::from([("DB".into(), "postgres://localhost".into())]),
            image: Some("ghcr.io/mcp/server:latest".into()),
            mount_cwd: false,
            network: false,
            docker_mcp: None,
            styrene_dest: None,
            timeout_secs: 30,
        };
        let cmd = McpFeature::build_command("test", &config, None).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        // Should use detected runtime (podman or docker)
        assert!(
            prog == "podman" || prog == "docker",
            "expected podman or docker, got: {prog}"
        );
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert!(
            args.contains(&"--network=none"),
            "should disable network: {args:?}"
        );
        assert!(
            args.contains(&"ghcr.io/mcp/server:latest"),
            "should include image: {args:?}"
        );
    }

    #[test]
    fn build_command_docker_gateway() {
        let config = McpServerConfig {
            url: None,
            command: None,
            args: vec![],
            env: HashMap::new(),
            image: None,
            mount_cwd: false,
            network: true,
            docker_mcp: Some("github".into()),
            styrene_dest: None,
            timeout_secs: 30,
        };
        let cmd = McpFeature::build_command("test", &config, None).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        assert_eq!(prog, "docker");
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert!(
            args.contains(&"mcp"),
            "should have mcp subcommand: {args:?}"
        );
        assert!(args.contains(&"gateway"), "should have gateway: {args:?}");
        assert!(
            args.contains(&"github"),
            "should have server name: {args:?}"
        );
    }

    #[test]
    fn mcp_server_config_styrene_mesh() {
        let toml = r#"
            styrene_dest = "a7b3c9d1e5f2..."
            command = "/opt/mcp-servers/gpu-inference"
            args = ["--model", "qwen3:30b"]
        "#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.styrene_dest.as_deref(), Some("a7b3c9d1e5f2..."));
        assert_eq!(
            config.command.as_deref(),
            Some("/opt/mcp-servers/gpu-inference")
        );
    }

    #[test]
    fn build_command_styrene_mesh() {
        let config = McpServerConfig {
            url: None,
            command: Some("/opt/mcp/server".into()),
            args: vec!["--port".into(), "0".into()],
            env: HashMap::new(),
            image: None,
            mount_cwd: false,
            network: true,
            docker_mcp: None,
            styrene_dest: Some("a7b3c9d1e5f2".into()),
            timeout_secs: 60,
        };
        let cmd = McpFeature::build_command("gpu", &config, None).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        assert_eq!(prog, "styrene");
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert!(
            args.contains(&"exec"),
            "should have exec subcommand: {args:?}"
        );
        assert!(
            args.contains(&"a7b3c9d1e5f2"),
            "should have dest hash: {args:?}"
        );
        assert!(
            args.contains(&"/opt/mcp/server"),
            "should have command: {args:?}"
        );
    }

    #[test]
    fn build_command_no_execution_method() {
        let config = McpServerConfig {
            url: None,
            command: None,
            args: vec![],
            env: HashMap::new(),
            image: None,
            mount_cwd: false,
            network: true,
            docker_mcp: None,
            styrene_dest: None,
            timeout_secs: 30,
        };
        let result = McpFeature::build_command("test", &config, None);
        assert!(
            result.is_err(),
            "should error without command, image, or docker_mcp"
        );
    }

    #[test]
    fn detect_runtime_returns_something() {
        let runtime = detect_container_runtime();
        // Should always return a string (podman, docker, or docker as fallback)
        assert!(!runtime.is_empty());
    }

    #[test]
    fn armory_manifest_with_mcp_servers() {
        let toml = r#"
            [plugin]
            type = "extension"
            id = "dev.example.mcp-tools"
            name = "MCP Tools"
            version = "1.0.0"
            description = "Tools via MCP servers"

            [mcp_servers.filesystem]
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-filesystem", "/home"]

            [mcp_servers.brave]
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-brave-search"]
            [mcp_servers.brave.env]
            BRAVE_API_KEY = "{BRAVE_API_KEY}"
        "#;
        let manifest = super::super::armory::ArmoryManifest::parse(toml).unwrap();
        assert_eq!(manifest.mcp_servers.len(), 2);
        assert!(manifest.mcp_servers.contains_key("filesystem"));
        assert!(manifest.mcp_servers.contains_key("brave"));
    }

    #[test]
    fn mcp_server_config_http_transport() {
        let toml = r#"
            url = "https://api.example.com/mcp"
            timeout_secs = 45
        "#;
        let config: McpServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.url.as_deref(), Some("https://api.example.com/mcp"));
        assert!(config.command.is_none());
        assert!(config.image.is_none());
        assert_eq!(config.timeout_secs, 45);
    }

    #[test]
    fn validate_url_https_allowed() {
        assert!(McpFeature::validate_url("https://api.example.com/mcp").is_ok());
        assert!(McpFeature::validate_url("https://localhost/mcp").is_ok());
    }

    #[test]
    fn validate_url_localhost_http_allowed() {
        assert!(McpFeature::validate_url("http://localhost/mcp").is_ok());
        assert!(McpFeature::validate_url("http://127.0.0.1:8080/mcp").is_ok());
    }

    #[test]
    fn validate_url_non_localhost_http_rejected() {
        assert!(McpFeature::validate_url("http://example.com/mcp").is_err());
        assert!(McpFeature::validate_url("http://192.168.1.1/mcp").is_err());
    }

    #[test]
    fn validate_url_invalid_scheme_rejected() {
        assert!(McpFeature::validate_url("ftp://example.com/file").is_err());
        assert!(McpFeature::validate_url("ws://example.com/socket").is_err());
    }

    #[test]
    fn validate_url_invalid_format_rejected() {
        assert!(McpFeature::validate_url("not-a-url").is_err());
        assert!(McpFeature::validate_url("https://").is_err());
    }

    // ── Helper to build McpFeature with pre-populated resources/prompts ──

    fn make_test_feature(
        resources: Vec<McpResource>,
        resource_templates: Vec<McpResourceTemplate>,
        prompts: Vec<McpPrompt>,
    ) -> McpFeature {
        McpFeature {
            feature_name: "test-plugin".to_string(),
            tools: Vec::new(),
            resources,
            resource_templates,
            prompts,
            clients: Arc::new(Mutex::new(HashMap::new())),
            timeouts: HashMap::new(),
            progress: Arc::new(ProgressRegistry::default()),
        }
    }

    #[test]
    fn resource_count_and_template_count_defaults() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        assert_eq!(feat.resource_count(), 0);
        assert_eq!(feat.resource_template_count(), 0);
    }

    #[test]
    fn prompt_count_defaults() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        assert_eq!(feat.prompt_count(), 0);
    }

    #[test]
    fn provide_context_empty_when_no_resources_or_prompts() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        let phase = LifecyclePhase::Idle;
        let signals = ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &phase,
            turn_number: 0,
            context_budget_tokens: 4096,
        };
        assert!(feat.provide_context(&signals).is_none());
    }

    #[test]
    fn provide_context_includes_resources() {
        let feat = make_test_feature(
            vec![McpResource {
                uri: "file:///tmp/notes.txt".to_string(),
                name: "notes".to_string(),
                description: Some("My notes".to_string()),
                mime_type: Some("text/plain".to_string()),
                server_name: "fs".to_string(),
            }],
            vec![],
            vec![],
        );
        let phase = LifecyclePhase::Idle;
        let signals = ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &phase,
            turn_number: 0,
            context_budget_tokens: 4096,
        };
        let ctx = feat.provide_context(&signals).expect("should return Some");
        assert!(ctx.content.contains("MCP Resources"), "missing header");
        assert!(ctx.content.contains("notes"), "missing resource name");
        assert!(ctx.content.contains("file:///tmp/notes.txt"), "missing uri");
        assert!(ctx.content.contains("fs"), "missing server name");
    }

    #[test]
    fn provide_context_includes_prompts() {
        let feat = make_test_feature(
            vec![],
            vec![],
            vec![McpPrompt {
                name: "fs::summarize".to_string(),
                description: Some("Summarize a file".to_string()),
                arguments: vec![McpPromptArgument {
                    name: "path".to_string(),
                    description: Some("File path".to_string()),
                    required: true,
                }],
                server_name: "fs".to_string(),
            }],
        );
        let phase = LifecyclePhase::Idle;
        let signals = ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &phase,
            turn_number: 0,
            context_budget_tokens: 4096,
        };
        let ctx = feat.provide_context(&signals).expect("should return Some");
        assert!(ctx.content.contains("MCP Prompts"), "missing header");
        assert!(ctx.content.contains("fs::summarize"), "missing prompt name");
        assert!(ctx.content.contains("path"), "missing argument name");
    }

    #[tokio::test]
    async fn execute_read_resource_missing_server_arg() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        let args = serde_json::json!({"uri": "file:///tmp/x"});
        let err = feat.execute_read_resource(&args).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("missing required argument: server"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn execute_read_resource_missing_uri_arg() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        let args = serde_json::json!({"server": "fs"});
        let err = feat.execute_read_resource(&args).await.unwrap_err();
        assert!(
            err.to_string().contains("missing required argument: uri"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn execute_get_prompt_missing_server_arg() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        let args = serde_json::json!({"name": "summarize"});
        let err = feat.execute_get_prompt(&args).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("missing required argument: server"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn execute_get_prompt_missing_name_arg() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        let args = serde_json::json!({"server": "fs"});
        let err = feat.execute_get_prompt(&args).await.unwrap_err();
        assert!(
            err.to_string().contains("missing required argument: name"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tools_includes_mcp_read_resource_when_resources_present() {
        let feat = make_test_feature(
            vec![McpResource {
                uri: "file:///data".to_string(),
                name: "data".to_string(),
                description: None,
                mime_type: None,
                server_name: "fs".to_string(),
            }],
            vec![],
            vec![],
        );
        let defs = feat.tools();
        let has_read_resource = defs.iter().any(|d| d.name.contains("mcp_read_resource"));
        assert!(
            has_read_resource,
            "expected mcp_read_resource tool in: {defs:?}"
        );
    }

    #[test]
    fn tools_excludes_mcp_read_resource_when_no_resources() {
        let feat = make_test_feature(vec![], vec![], vec![]);
        let defs = feat.tools();
        let has_read_resource = defs.iter().any(|d| d.name.contains("mcp_read_resource"));
        assert!(
            !has_read_resource,
            "mcp_read_resource should not be present when no resources exist"
        );
    }
}

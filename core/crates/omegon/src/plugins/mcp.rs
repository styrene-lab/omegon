//! MCP plugin Feature — connects to MCP servers via stdio child-process transport,
//! discovers their tools, and exposes them as Omegon tools.
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
    service::{self, RoleClient, RunningService},
    transport::{TokioChildProcess, StreamableHttpClientTransport},
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;

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

fn default_timeout() -> u64 { 30 }
fn default_true() -> bool { true }

/// A discovered tool from an MCP server.
#[derive(Debug, Clone)]
struct McpTool {
    name: String,
    description: String,
    parameters: Value,
    server_name: String,
}

/// Minimal client handler — we don't need to handle server requests,
/// just connect and call tools.
#[derive(Clone)]
struct OmegonMcpClient;

impl ClientHandler for OmegonMcpClient {
    fn get_info(&self) -> ClientInfo {
        let mut impl_info = Implementation::default();
        impl_info.name = "omegon".into();
        impl_info.version = env!("CARGO_PKG_VERSION").into();
        InitializeRequestParams::new(ClientCapabilities::default(), impl_info)
    }
}

/// Running connection to an MCP server.
type McpConnection = RunningService<RoleClient, OmegonMcpClient>;

/// Feature that wraps one or more MCP server connections.
pub struct McpFeature {
    feature_name: String,
    tools: Vec<McpTool>,
    clients: Arc<Mutex<HashMap<String, McpConnection>>>,
}

impl McpFeature {
    /// Connect to MCP servers and discover their tools.
    pub async fn connect(
        plugin_name: &str,
        servers: &HashMap<String, McpServerConfig>,
    ) -> anyhow::Result<Self> {
        let mut all_tools = Vec::new();
        let mut clients = HashMap::new();

        for (server_name, config) in servers {
            match Self::connect_one(server_name, config).await {
                Ok((server_tools, client)) => {
                    tracing::info!(
                        plugin = plugin_name,
                        server = server_name,
                        tools = server_tools.len(),
                        "MCP server connected"
                    );
                    all_tools.extend(server_tools);
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
            clients: Arc::new(Mutex::new(clients)),
        })
    }

    async fn connect_one(
        server_name: &str,
        config: &McpServerConfig,
    ) -> anyhow::Result<(Vec<McpTool>, McpConnection)> {
        let client = if let Some(ref url) = config.url {
            // HTTP transport mode
            Self::validate_url(url)?;
            let transport = StreamableHttpClientTransport::from_uri(url.clone());
            service::serve_client(OmegonMcpClient, transport).await?
        } else {
            // Local process transport mode  
            let cmd = Self::build_command(server_name, config)?;
            let transport = TokioChildProcess::new(cmd)?;
            service::serve_client(OmegonMcpClient, transport).await?
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
            if url.len() > 8 && !url[8..].is_empty() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("Invalid HTTPS URL: must have host after scheme: {}", url))
            }
        } else if url.starts_with("http://") {
            if url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1") {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "HTTP URLs are only allowed for localhost, got: {}", url
                ))
            }
        } else {
            Err(anyhow::anyhow!(
                "Only HTTPS and localhost HTTP URLs are supported, got: {}", url
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
    fn build_command(server_name: &str, config: &McpServerConfig) -> anyhow::Result<Command> {
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
                cmd.env(key, resolve_env_template(value));
            }
            return Ok(cmd);
        }

        // Mode 2: Docker MCP Gateway
        if let Some(ref gateway_name) = config.docker_mcp {
            let mut cmd = Command::new("docker");
            cmd.args(["mcp", "gateway", "run", gateway_name]);
            for (key, value) in &config.env {
                cmd.env(key, resolve_env_template(value));
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
            if config.mount_cwd {
                if let Ok(cwd) = std::env::current_dir() {
                    cmd.arg(format!("-v={}:/work", cwd.display()));
                    cmd.args(["-w", "/work"]);
                }
            }

            // Environment variables — validate key names to prevent injection
            for (key, value) in &config.env {
                if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    tracing::warn!(key = key, "skipping env var with invalid name");
                    continue;
                }
                cmd.arg(format!("-e={}={}", key, resolve_env_template(value)));
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
            cmd.env(key, resolve_env_template(value));
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
}

#[async_trait]
impl Feature for McpFeature {
    fn name(&self) -> &str {
        &self.feature_name
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|t| ToolDefinition {
                name: t.name.clone(),
                label: format!("mcp:{}", t.server_name),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect()
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let (server_name, mcp_name) = Self::split_tool_name(tool_name);
        let server_name = server_name.to_string();
        let mcp_name = mcp_name.to_string();

        let clients = self.clients.lock().await;
        let client = clients.get(&server_name).ok_or_else(|| {
            anyhow::anyhow!("MCP server '{}' not connected", server_name)
        })?;

        let arguments = if args.is_object() {
            Some(args.as_object().unwrap().clone())
        } else {
            None
        };

        let mut params = CallToolRequestParams::default();
        params.name = mcp_name.into();
        params.arguments = arguments;

        // TODO: Apply config.timeout_secs via tokio::time::timeout wrapping this call.
        // Currently the _cancel token is available but timeout_secs from the config
        // is not propagated to the execute method. Requires storing config per-server
        // in the clients map or a separate timeout map.
        let result = client.call_tool(params).await?;

        // Convert MCP content to Omegon content blocks
        let content: Vec<ContentBlock> = result
            .content
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

        Ok(ToolResult {
            content,
            details: Value::Null,
        })
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
fn resolve_env_template(template: &str) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch == '{' {
            // Find the closing brace
            if let Some(end_offset) = template[i + 1..].find('}') {
                let var = &template[i + 1..i + 1 + end_offset];
                // Only resolve if var looks like an env var name (alphanumeric + _)
                if !var.is_empty() && var.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let value = std::env::var(var).unwrap_or_default();
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

    #[test]
    fn resolve_env_template_basic() {
        unsafe { std::env::set_var("TEST_MCP_KEY", "secret123"); }
        let result = resolve_env_template("Bearer {TEST_MCP_KEY}");
        assert_eq!(result, "Bearer secret123");
        unsafe { std::env::remove_var("TEST_MCP_KEY"); }
    }

    #[test]
    fn resolve_env_template_missing_var() {
        let result = resolve_env_template("{NONEXISTENT_VAR_12345}");
        assert_eq!(result, "");
    }

    #[test]
    fn resolve_env_template_no_pattern() {
        let result = resolve_env_template("plain string");
        assert_eq!(result, "plain string");
    }

    #[test]
    fn resolve_env_template_nested_braces() {
        // Nested braces should not cause infinite loop
        let result = resolve_env_template("{foo{bar}}");
        // {foo{bar}} — `foo{bar` is not a valid env var name (contains {)
        // so it's emitted literally
        assert!(result.contains("foo"), "should not loop: {result}");
    }

    #[test]
    fn resolve_env_template_value_with_braces() {
        // If the resolved value contains {, it should NOT be re-scanned
        unsafe { std::env::set_var("TEST_BRACE_VAL", "has{braces}inside"); }
        let result = resolve_env_template("{TEST_BRACE_VAL}");
        assert_eq!(result, "has{braces}inside");
        unsafe { std::env::remove_var("TEST_BRACE_VAL"); }
    }

    #[test]
    fn resolve_env_template_unclosed_brace() {
        let result = resolve_env_template("prefix {UNCLOSED");
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
        assert_eq!(config.image.as_deref(), Some("ghcr.io/mcp/server-postgres:latest"));
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
        let cmd = McpFeature::build_command("test", &config).unwrap();
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
        let cmd = McpFeature::build_command("test", &config).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        // Should use detected runtime (podman or docker)
        assert!(prog == "podman" || prog == "docker",
            "expected podman or docker, got: {prog}");
        let args: Vec<_> = cmd.as_std().get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"--network=none"), "should disable network: {args:?}");
        assert!(args.contains(&"ghcr.io/mcp/server:latest"), "should include image: {args:?}");
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
        let cmd = McpFeature::build_command("test", &config).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        assert_eq!(prog, "docker");
        let args: Vec<_> = cmd.as_std().get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"mcp"), "should have mcp subcommand: {args:?}");
        assert!(args.contains(&"gateway"), "should have gateway: {args:?}");
        assert!(args.contains(&"github"), "should have server name: {args:?}");
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
        assert_eq!(config.command.as_deref(), Some("/opt/mcp-servers/gpu-inference"));
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
        let cmd = McpFeature::build_command("gpu", &config).unwrap();
        let prog = cmd.as_std().get_program().to_str().unwrap();
        assert_eq!(prog, "styrene");
        let args: Vec<_> = cmd.as_std().get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"exec"), "should have exec subcommand: {args:?}");
        assert!(args.contains(&"a7b3c9d1e5f2"), "should have dest hash: {args:?}");
        assert!(args.contains(&"/opt/mcp/server"), "should have command: {args:?}");
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
        let result = McpFeature::build_command("test", &config);
        assert!(result.is_err(), "should error without command, image, or docker_mcp");
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
}

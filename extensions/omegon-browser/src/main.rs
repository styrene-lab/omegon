use async_trait::async_trait;
use omegon_extension::{Error, Extension, SDK_CONTRACT_VERSION};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const NAME: &str = "omegon-browser";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_MAX_OUTPUT: u64 = 50_000;
const MAX_BATCH_COMMANDS: usize = 20;
const MAX_BATCH_ARGS: usize = 24;

macro_rules! common_schema {
    ($props:tt) => {
        common_schema_required(json!($props), Vec::<&str>::new())
    };
    ($props:tt, [$($required:expr),*]) => {
        common_schema_required(json!($props), vec![$($required),*])
    };
}

#[derive(Debug, Clone)]
struct BrowserConfig {
    binary: String,
    default_session: Option<String>,
    allowed_domains: Vec<String>,
    max_output: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            binary: "agent-browser".to_string(),
            default_session: None,
            allowed_domains: Vec::new(),
            max_output: DEFAULT_MAX_OUTPUT,
        }
    }
}

#[derive(Debug, Default)]
struct BrowserExtension {
    config: Arc<RwLock<BrowserConfig>>,
}

#[async_trait]
impl Extension for BrowserExtension {
    fn name(&self) -> &str {
        NAME
    }

    fn version(&self) -> &str {
        VERSION
    }

    async fn handle_rpc(&self, method: &str, params: Value) -> omegon_extension::Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocol_version": 2,
                "extension_info": {
                    "name": NAME,
                    "version": VERSION,
                    "sdk_version": SDK_CONTRACT_VERSION
                },
                "capabilities": {
                    "tools": true,
                    "widgets": false,
                    "mind": false,
                    "vox": false,
                    "resources": false,
                    "prompts": false,
                    "sampling": false,
                    "elicitation": false,
                    "streaming": false
                },
                "tools": self.tool_defs()
            })),
            "get_tools" | "tools/list" => Ok(Value::Array(self.tool_defs())),
            "bootstrap_secrets" => Ok(json!({"acknowledged": true})),
            "bootstrap_config" => {
                self.apply_config(params)?;
                Ok(json!({"acknowledged": true}))
            }
            "execute_tool" | "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("args")
                    .or_else(|| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.execute_tool(name, args).await
            }
            _ => Err(Error::method_not_found(method)),
        }
    }

    async fn on_config(&self, config: HashMap<String, Value>) {
        let _ = self.apply_config(json!(config));
    }
}

impl BrowserExtension {
    fn apply_config(&self, params: Value) -> omegon_extension::Result<()> {
        let mut config = self
            .config
            .write()
            .map_err(|_| Error::internal_error("browser config lock poisoned"))?;

        if let Some(binary) = string_field(&params, "agent_browser_binary") {
            config.binary = binary;
        }
        if let Some(session) = string_field(&params, "default_session") {
            config.default_session = non_empty(session);
        }
        if let Some(domains) = string_field(&params, "allowed_domains") {
            config.allowed_domains = parse_domains(&domains);
        }
        if let Some(max_output) = u64_field(&params, "max_output") {
            config.max_output = max_output;
        }

        Ok(())
    }

    async fn execute_tool(&self, name: &str, args: Value) -> omegon_extension::Result<Value> {
        match name {
            "browser_status" => self.browser_status(args).await,
            "browser_open" => self.browser_open(args).await,
            "browser_snapshot" => self.browser_snapshot(args).await,
            "browser_click" => self.browser_click(args).await,
            "browser_fill" => self.browser_fill(args).await,
            "browser_wait" => self.browser_wait(args).await,
            "browser_get" => self.browser_get(args).await,
            "browser_screenshot" => self.browser_screenshot(args).await,
            "browser_batch" => self.browser_batch(args).await,
            _ => Err(Error::method_not_found(&format!("tool '{name}'"))),
        }
    }

    async fn browser_status(&self, args: Value) -> omegon_extension::Result<Value> {
        let request = RequestOptions::from_args(&args, self.config()?)?;
        let output = run_agent_browser(&request, vec!["--version".to_string()], None, false).await;
        match output {
            Ok(result) => Ok(json!({
                "available": result.status_success,
                "binary": request.binary,
                "version": result.stdout.trim(),
                "stderr": result.stderr.trim()
            })),
            Err(err) => Ok(json!({
                "available": false,
                "binary": request.binary,
                "error": err.message()
            })),
        }
    }

    async fn browser_open(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: OpenArgs = parse_args(args.clone())?;
        let url = input
            .url
            .ok_or_else(|| Error::invalid_params("url is required"))?;
        let mut request = RequestOptions::from_args(&args, self.config()?)?;
        if request.allowed_domains.is_empty() {
            if let Some(domain) = domain_from_url(&url) {
                request.allowed_domains.push(domain);
            }
        }

        let mut command = vec!["open".to_string()];
        if input.enable_react_devtools.unwrap_or(false) {
            command.extend(["--enable".to_string(), "react-devtools".to_string()]);
        }
        command.push(url);
        command.push("--json".to_string());
        run_tool(&request, command, None).await
    }

    async fn browser_snapshot(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: SnapshotArgs = parse_args(args.clone())?;
        let request = RequestOptions::from_args(&args, self.config()?)?;
        let mut command = vec!["snapshot".to_string()];
        if input.interactive.unwrap_or(true) {
            command.push("--interactive".to_string());
        }
        if input.urls.unwrap_or(false) {
            command.push("--urls".to_string());
        }
        if input.compact.unwrap_or(true) {
            command.push("--compact".to_string());
        }
        if let Some(depth) = input.depth {
            command.extend(["--depth".to_string(), depth.to_string()]);
        }
        if let Some(selector) = input.selector {
            command.extend(["--selector".to_string(), selector]);
        }
        command.push("--json".to_string());
        run_tool(&request, command, None).await
    }

    async fn browser_click(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: TargetArgs = parse_args(args.clone())?;
        let target = required_string(input.target, "target")?;
        let request = RequestOptions::from_args(&args, self.config()?)?;
        run_tool(
            &request,
            vec!["click".to_string(), target, "--json".to_string()],
            None,
        )
        .await
    }

    async fn browser_fill(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: FillArgs = parse_args(args.clone())?;
        let target = required_string(input.target, "target")?;
        let text = required_string(input.text, "text")?;
        let request = RequestOptions::from_args(&args, self.config()?)?;
        run_tool(
            &request,
            vec!["fill".to_string(), target, text, "--json".to_string()],
            None,
        )
        .await
    }

    async fn browser_wait(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: WaitArgs = parse_args(args.clone())?;
        let request = RequestOptions::from_args(&args, self.config()?)?;
        let mut command = vec!["wait".to_string()];
        let mut criteria = 0;

        if let Some(ms) = input.ms {
            command.push(ms.to_string());
            criteria += 1;
        }
        if let Some(selector) = input.selector {
            command.push(selector);
            criteria += 1;
        }
        if let Some(text) = input.text {
            command.extend(["--text".to_string(), text]);
            criteria += 1;
        }
        if let Some(url) = input.url {
            command.extend(["--url".to_string(), url]);
            criteria += 1;
        }
        if let Some(load) = input.load {
            command.extend(["--load".to_string(), load]);
            criteria += 1;
        }
        if let Some(js) = input.js {
            command.extend(["--fn".to_string(), js]);
            criteria += 1;
        }
        if let Some(state) = input.state {
            command.extend(["--state".to_string(), state]);
        }

        if criteria != 1 {
            return Err(Error::invalid_params(
                "provide exactly one wait criterion: ms, selector, text, url, load, or js",
            ));
        }

        command.push("--json".to_string());
        run_tool(&request, command, None).await
    }

    async fn browser_get(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: GetArgs = parse_args(args.clone())?;
        let kind = input.kind.unwrap_or_else(|| "text".to_string());
        let target = required_string(input.target, "target")?;
        let request = RequestOptions::from_args(&args, self.config()?)?;
        let mut command = vec!["get".to_string(), kind, target];
        if let Some(attribute) = input.attribute {
            command.push(attribute);
        }
        command.push("--json".to_string());
        run_tool(&request, command, None).await
    }

    async fn browser_screenshot(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: ScreenshotArgs = parse_args(args.clone())?;
        let path = input
            .path
            .unwrap_or_else(|| default_screenshot_path().to_string_lossy().into_owned());
        let request = RequestOptions::from_args(&args, self.config()?)?;
        let mut command = vec!["screenshot".to_string(), path.clone()];
        if input.full_page.unwrap_or(false) {
            command.push("--full".to_string());
        }
        command.push("--json".to_string());
        let mut result = run_tool(&request, command, None).await?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("path".to_string(), Value::String(path));
        }
        Ok(result)
    }

    async fn browser_batch(&self, args: Value) -> omegon_extension::Result<Value> {
        let input: BatchArgs = parse_args(args.clone())?;
        let request = RequestOptions::from_args(&args, self.config()?)?;
        validate_batch(&input.commands)?;

        let mut command = vec!["batch".to_string(), "--json".to_string()];
        if input.bail.unwrap_or(true) {
            command.push("--bail".to_string());
        }
        let stdin = serde_json::to_string(&input.commands)
            .map_err(|e| Error::invalid_params(format!("invalid commands: {e}")))?;
        run_tool(&request, command, Some(stdin)).await
    }

    fn config(&self) -> omegon_extension::Result<BrowserConfig> {
        self.config
            .read()
            .map(|guard| guard.clone())
            .map_err(|_| Error::internal_error("browser config lock poisoned"))
    }

    fn tool_defs(&self) -> Vec<Value> {
        vec![
            json!({
                "name": "browser_status",
                "label": "Browser Status",
                "description": "Check whether the agent-browser CLI is installed and callable.",
                "parameters": common_schema!({})
            }),
            json!({
                "name": "browser_open",
                "label": "Browser Open",
                "description": "Open a URL in an agent-browser-controlled browser session.",
                "parameters": common_schema!({
                    "url": {"type": "string", "description": "URL to open."},
                    "enable_react_devtools": {"type": "boolean", "description": "Launch with the React DevTools hook enabled."}
                }, ["url"])
            }),
            json!({
                "name": "browser_snapshot",
                "label": "Browser Snapshot",
                "description": "Capture an accessibility snapshot, with interactive refs by default.",
                "parameters": common_schema!({
                    "interactive": {"type": "boolean", "description": "Only include interactive elements. Defaults to true."},
                    "compact": {"type": "boolean", "description": "Remove empty structural nodes. Defaults to true."},
                    "urls": {"type": "boolean", "description": "Include URLs for links."},
                    "depth": {"type": "integer", "minimum": 1, "maximum": 20},
                    "selector": {"type": "string", "description": "Optional CSS selector scope."}
                })
            }),
            json!({
                "name": "browser_click",
                "label": "Browser Click",
                "description": "Click a selector, text locator, XPath locator, semantic ref, or agent-browser @ref.",
                "parameters": common_schema!({
                    "target": {"type": "string", "description": "Selector, locator, or @ref to click."}
                }, ["target"])
            }),
            json!({
                "name": "browser_fill",
                "label": "Browser Fill",
                "description": "Fill a form field identified by selector, locator, or @ref.",
                "parameters": common_schema!({
                    "target": {"type": "string", "description": "Selector, locator, or @ref to fill."},
                    "text": {"type": "string", "description": "Text to enter."}
                }, ["target", "text"])
            }),
            json!({
                "name": "browser_wait",
                "label": "Browser Wait",
                "description": "Wait for exactly one browser condition: ms, selector, text, url, load, or js.",
                "parameters": common_schema!({
                    "ms": {"type": "integer", "minimum": 0},
                    "selector": {"type": "string"},
                    "text": {"type": "string"},
                    "url": {"type": "string", "description": "URL glob."},
                    "load": {"type": "string", "enum": ["load", "domcontentloaded", "networkidle"]},
                    "js": {"type": "string", "description": "JavaScript expression passed to agent-browser wait --fn."},
                    "state": {"type": "string", "enum": ["visible", "hidden", "attached", "detached"]}
                })
            }),
            json!({
                "name": "browser_get",
                "label": "Browser Get",
                "description": "Read text, html, value, visibility, or an attribute from a selector or @ref.",
                "parameters": common_schema!({
                    "kind": {"type": "string", "description": "agent-browser get kind, such as text, html, value, visible, or attribute."},
                    "target": {"type": "string"},
                    "attribute": {"type": "string", "description": "Attribute name when kind requires one."}
                }, ["target"])
            }),
            json!({
                "name": "browser_screenshot",
                "label": "Browser Screenshot",
                "description": "Save a browser screenshot and return the path and command result.",
                "parameters": common_schema!({
                    "path": {"type": "string", "description": "Output PNG path. Defaults to a temp file."},
                    "full_page": {"type": "boolean", "description": "Capture the full page when supported."}
                })
            }),
            json!({
                "name": "browser_batch",
                "label": "Browser Batch",
                "description": "Run multiple agent-browser commands in one daemon call. Commands are arrays of argv tokens without the agent-browser binary.",
                "parameters": common_schema!({
                    "commands": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_BATCH_COMMANDS,
                        "items": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": MAX_BATCH_ARGS,
                            "items": {"type": "string"}
                        }
                    },
                    "bail": {"type": "boolean", "description": "Stop on first command failure. Defaults to true."}
                }, ["commands"])
            }),
        ]
    }
}

#[derive(Debug, Clone)]
struct RequestOptions {
    binary: String,
    session_name: Option<String>,
    allowed_domains: Vec<String>,
    max_output: u64,
    headed: Option<bool>,
    state: Option<String>,
    timeout_ms: u64,
}

impl RequestOptions {
    fn from_args(args: &Value, config: BrowserConfig) -> omegon_extension::Result<Self> {
        let session_name = string_field(args, "session_name").or(config.default_session);
        let allowed_domains = if let Some(domains) = array_or_csv_field(args, "allowed_domains") {
            domains
        } else {
            config.allowed_domains
        };
        Ok(Self {
            binary: string_field(args, "binary").unwrap_or(config.binary),
            session_name: session_name.and_then(non_empty),
            allowed_domains,
            max_output: u64_field(args, "max_output").unwrap_or(config.max_output),
            headed: bool_field(args, "headed"),
            state: string_field(args, "state").and_then(non_empty),
            timeout_ms: u64_field(args, "timeout_ms").unwrap_or(DEFAULT_TIMEOUT_MS),
        })
    }
}

#[derive(Debug)]
struct CommandOutput {
    status_success: bool,
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn run_tool(
    request: &RequestOptions,
    command_args: Vec<String>,
    stdin: Option<String>,
) -> omegon_extension::Result<Value> {
    let output = run_agent_browser(request, command_args, stdin, true).await?;
    let parsed = serde_json::from_str::<Value>(&output.stdout).ok();
    Ok(json!({
        "success": output.status_success,
        "status_code": output.status_code,
        "stdout": output.stdout.trim(),
        "stderr": output.stderr.trim(),
        "json": parsed,
    }))
}

async fn run_agent_browser(
    request: &RequestOptions,
    command_args: Vec<String>,
    stdin: Option<String>,
    wrap_failure: bool,
) -> omegon_extension::Result<CommandOutput> {
    let mut command = Command::new(&request.binary);
    apply_global_options(&mut command, request);
    command.args(command_args);
    if stdin.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let mut child = command.spawn().map_err(|e| {
        Error::internal_error(format!(
            "failed to spawn '{}': {e}. Install Vercel agent-browser or set agent_browser_binary.",
            request.binary
        ))
    })?;

    if let Some(input) = stdin {
        let mut child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::internal_error("failed to open agent-browser stdin"))?;
        child_stdin.write_all(input.as_bytes()).await?;
    }

    let timeout = Duration::from_millis(request.timeout_ms);
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            Error::new(
                omegon_extension::ErrorCode::Timeout,
                format!("agent-browser timed out after {}ms", request.timeout_ms),
            )
        })?
        .map_err(|e| Error::internal_error(format!("agent-browser failed: {e}")))?;

    let result = CommandOutput {
        status_success: output.status.success(),
        status_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };

    if wrap_failure || result.status_success {
        Ok(result)
    } else {
        Err(Error::internal_error(result.stderr.trim().to_string()))
    }
}

fn apply_global_options(command: &mut Command, request: &RequestOptions) {
    if let Some(session) = &request.session_name {
        command.args(["--session-name", session]);
    }
    if let Some(state) = &request.state {
        command.args(["--state", state]);
    }
    if let Some(headed) = request.headed {
        command.arg(format!("--headed={headed}"));
    }
    if !request.allowed_domains.is_empty() {
        command.env(
            "AGENT_BROWSER_ALLOWED_DOMAINS",
            request.allowed_domains.join(","),
        );
    }
    command.env("AGENT_BROWSER_MAX_OUTPUT", request.max_output.to_string());
    command.env("AGENT_BROWSER_CONTENT_BOUNDARIES", "1");
}

fn common_schema_required(properties: impl Into<Value>, required: Vec<&str>) -> Value {
    let mut props = properties.into().as_object().cloned().unwrap_or_default();
    props.insert(
        "session_name".to_string(),
        json!({"type": "string", "description": "Optional agent-browser session name."}),
    );
    props.insert(
        "allowed_domains".to_string(),
        json!({
            "description": "Domain allowlist for this command. String CSV or array of strings.",
            "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
        }),
    );
    props.insert(
        "headed".to_string(),
        json!({"type": "boolean", "description": "Run browser headed when supported."}),
    );
    props.insert(
        "state".to_string(),
        json!({"type": "string", "description": "Optional agent-browser auth state path."}),
    );
    props.insert(
        "timeout_ms".to_string(),
        json!({"type": "integer", "minimum": 1000, "description": "Command timeout in milliseconds."}),
    );
    props.insert(
        "max_output".to_string(),
        json!({"type": "integer", "minimum": 1000, "description": "Maximum agent-browser output characters."}),
    );
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize)]
struct OpenArgs {
    url: Option<String>,
    enable_react_devtools: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SnapshotArgs {
    interactive: Option<bool>,
    compact: Option<bool>,
    urls: Option<bool>,
    depth: Option<u8>,
    selector: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TargetArgs {
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FillArgs {
    target: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WaitArgs {
    ms: Option<u64>,
    selector: Option<String>,
    text: Option<String>,
    url: Option<String>,
    load: Option<String>,
    js: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetArgs {
    kind: Option<String>,
    target: Option<String>,
    attribute: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScreenshotArgs {
    path: Option<String>,
    full_page: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct BatchArgs {
    commands: Vec<Vec<String>>,
    bail: Option<bool>,
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: Value) -> omegon_extension::Result<T> {
    serde_json::from_value(args).map_err(|e| Error::invalid_params(e.to_string()))
}

fn required_string(value: Option<String>, field: &str) -> omegon_extension::Result<String> {
    value
        .and_then(non_empty)
        .ok_or_else(|| Error::invalid_params(format!("{field} is required")))
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
    })
}

fn bool_field(value: &Value, field: &str) -> Option<bool> {
    value.get(field).and_then(|v| {
        v.as_bool().or_else(|| match v.as_str()? {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        })
    })
}

fn array_or_csv_field(value: &Value, field: &str) -> Option<Vec<String>> {
    let raw = value.get(field)?;
    if let Some(s) = raw.as_str() {
        return Some(parse_domains(s));
    }
    raw.as_array().map(|items| {
        items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(parse_domains)
            .collect()
    })
}

fn parse_domains(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn domain_from_url(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let host_port_path = rest.split('/').next().unwrap_or(rest);
    let host_port = host_port_path
        .split('@')
        .next_back()
        .unwrap_or(host_port_path);
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn default_screenshot_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "omegon-browser-{}-{}.png",
        std::process::id(),
        now_millis()
    ));
    path
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn validate_batch(commands: &[Vec<String>]) -> omegon_extension::Result<()> {
    if commands.is_empty() {
        return Err(Error::invalid_params("commands must not be empty"));
    }
    if commands.len() > MAX_BATCH_COMMANDS {
        return Err(Error::invalid_params(format!(
            "commands may contain at most {MAX_BATCH_COMMANDS} entries"
        )));
    }
    for command in commands {
        if command.is_empty() {
            return Err(Error::invalid_params("batch commands must not be empty"));
        }
        if command.len() > MAX_BATCH_ARGS {
            return Err(Error::invalid_params(format!(
                "each batch command may contain at most {MAX_BATCH_ARGS} arguments"
            )));
        }
        let first = &command[0];
        if first == "chat" || first == "eval" || first == "clipboard" || first == "download" {
            return Err(Error::invalid_params(format!(
                "batch command '{first}' is not exposed by this extension"
            )));
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--rpc") {
        eprintln!("omegon-browser must be launched by Omegon with --rpc");
        std::process::exit(1);
    }

    if let Err(err) = omegon_extension::serve_v2(BrowserExtension::default()).await {
        eprintln!("omegon-browser RPC server failed: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_domain_from_absolute_url() {
        assert_eq!(
            domain_from_url("https://example.com/dashboard"),
            Some("example.com".to_string())
        );
        assert_eq!(
            domain_from_url("http://localhost:3000/path"),
            Some("localhost".to_string())
        );
    }

    #[test]
    fn parses_domain_csv() {
        assert_eq!(
            parse_domains("example.com, *.example-cdn.com,localhost"),
            vec!["example.com", "*.example-cdn.com", "localhost"]
        );
    }

    #[test]
    fn validates_batch_limits_and_blocks_sensitive_commands() {
        validate_batch(&[vec!["open".into(), "https://example.com".into()]]).unwrap();
        assert!(validate_batch(&[vec!["chat".into(), "do it".into()]]).is_err());
        assert!(validate_batch(&[]).is_err());
    }

    #[test]
    fn request_options_merge_config_and_args() {
        let config = BrowserConfig {
            binary: "agent-browser".into(),
            default_session: Some("default".into()),
            allowed_domains: vec!["example.com".into()],
            max_output: 5000,
        };
        let args = json!({
            "session_name": "task-1",
            "allowed_domains": ["localhost", "*.test"],
            "headed": true,
            "timeout_ms": 2000
        });
        let options = RequestOptions::from_args(&args, config).unwrap();
        assert_eq!(options.session_name.as_deref(), Some("task-1"));
        assert_eq!(options.allowed_domains, vec!["localhost", "*.test"]);
        assert_eq!(options.headed, Some(true));
        assert_eq!(options.timeout_ms, 2000);
    }
}

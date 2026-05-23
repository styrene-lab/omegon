//! MCP compatibility shim — wraps any Omegon Extension as an MCP server.
//!
//! When an extension binary is invoked with `--mcp` or its manifest's
//! `[mcp].serve_subcommand`, this shim translates between MCP's wire
//! format and the Omegon extension's `handle_rpc` method.
//!
//! # What the shim does
//!
//! 1. Speaks MCP protocol (initialize/initialized with MCP capability schema)
//! 2. Maps `tools/list` → extension's tool registry
//! 3. Maps `tools/call` → extension's `execute_tool` / `tools/call`
//! 4. Maps `resources/*` → strips `widget_renderer`, `mind_section`, `trust_level`
//! 5. Maps `prompts/*` → strips `mind_context`, `inject_project_context`
//! 6. Drops: widgets, mind, vox bridge, secrets vault, stability tracking
//!
//! # What MCP clients lose
//!
//! - Widgets (UI panels)
//! - Mind (persistent knowledge)
//! - Vox bridge (messaging connectors)
//! - Secret vault delivery (MCP uses env vars)
//! - Stability tracking
//! - Styrene mesh transport
//! - Resource metadata: `widget_renderer`, `mind_section`, `trust_level`
//! - Prompt metadata: `mind_context`, `inject_project_context`, `MindFacts` content
//! - Sampling routing: `route` field
//! - Elicitation routing: `vox_eligible`, `vox_channel_hint`, `source`
//! - Progress content blocks (only numeric progress forwarded)

use crate::Extension;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// MCP protocol version we speak.
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Run an extension as an MCP server over stdin/stdout.
///
/// This is the entry point for `--mcp` mode. It translates MCP protocol
/// to Omegon extension RPC calls.
pub async fn serve_mcp<E: Extension>(ext: E) -> crate::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let mut reader = tokio::io::BufReader::new(stdin);
    let mut writer = tokio::io::BufWriter::new(stdout);

    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = mcp_error_response(None, -32700, &format!("parse error: {e}"));
                write_line(&mut writer, &resp).await?;
                continue;
            }
        };

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let has_id = msg.get("id").is_some();

        // Handle notifications (no id).
        if !has_id {
            // MCP notifications don't get responses.
            continue;
        }

        let response = match method {
            "initialize" => handle_mcp_initialize(&ext, &params),
            "tools/list" => handle_tools_list(&ext, &params).await,
            "tools/call" => handle_tools_call(&ext, &params).await,
            "resources/list" => handle_resources_list(&ext, &params).await,
            "resources/read" => handle_resources_read(&ext, &params).await,
            "resources/templates/list" => handle_resource_templates_list(&ext, &params).await,
            "resources/subscribe" => handle_resources_subscribe(&ext, &params).await,
            "prompts/list" => handle_prompts_list(&ext, &params).await,
            "prompts/get" => handle_prompts_get(&ext, &params).await,
            "ping" => Ok(json!({})),
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        let resp = match response {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err((code, message)) => mcp_error_response(id, code, &message),
        };

        write_line(&mut writer, &resp).await?;
    }
}

// ─── MCP method handlers ─────────────────────────────────────────────────

fn handle_mcp_initialize<E: Extension>(ext: &E, _params: &Value) -> Result<Value, (i32, String)> {
    // Build MCP capabilities based on what the extension supports.
    // We always advertise tools; resources and prompts depend on
    // whether the extension handles those methods.
    Ok(json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": {},
            "prompts": {}
        },
        "serverInfo": {
            "name": ext.name(),
            "version": ext.version(),
        }
    }))
}

async fn handle_tools_list<E: Extension>(ext: &E, params: &Value) -> Result<Value, (i32, String)> {
    // Try tools/list first (v2), fall back to get_tools (v1).
    let tools = match ext.handle_rpc("tools/list", params.clone()).await {
        Ok(v) => v,
        Err(_) => ext
            .handle_rpc("get_tools", json!({}))
            .await
            .map_err(|e| (e.code().numeric(), e.message().to_string()))?,
    };

    // Convert Omegon tool format to MCP tool format.
    // Omegon: { name, label, description, parameters }
    // MCP:    { name, title, description, inputSchema }
    let mcp_tools: Vec<Value> = match tools.as_array() {
        Some(arr) => arr.iter().map(omegon_tool_to_mcp).collect(),
        None => vec![],
    };

    Ok(json!({ "tools": mcp_tools }))
}

async fn handle_tools_call<E: Extension>(ext: &E, params: &Value) -> Result<Value, (i32, String)> {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    // Try tools/call first (v2), fall back to execute_tool (v1).
    let result = match ext
        .handle_rpc("tools/call", json!({"name": name, "arguments": arguments}))
        .await
    {
        Ok(v) => v,
        Err(_) => ext
            .handle_rpc("execute_tool", json!({"name": name, "args": arguments}))
            .await
            .map_err(|e| (e.code().numeric(), e.message().to_string()))?,
    };

    // Convert to MCP content format. HostActions remain useful to
    // Omegon-aware MCP clients via namespaced metadata while ordinary
    // content stays readable for generic MCP clients.
    let content = extract_mcp_content(&result);
    let host_actions = extract_host_actions_meta(&result);
    let mut out = json!({ "content": content, "isError": false });
    if let Some(actions) = host_actions {
        out["_meta"] = json!({ "omegon/hostActions": actions });
    }
    Ok(out)
}

async fn handle_resources_list<E: Extension>(
    ext: &E,
    params: &Value,
) -> Result<Value, (i32, String)> {
    let result = ext
        .handle_rpc("resources/list", params.clone())
        .await
        .map_err(|e| (e.code().numeric(), e.message().to_string()))?;

    // Strip Omegon-specific fields from each resource.
    if let Some(resources) = result.get("resources").and_then(|v| v.as_array()) {
        let stripped: Vec<Value> = resources.iter().map(strip_omegon_resource).collect();
        let mut out = json!({ "resources": stripped });
        if let Some(cursor) = result.get("next_cursor") {
            out["nextCursor"] = cursor.clone();
        }
        Ok(out)
    } else {
        Ok(result)
    }
}

async fn handle_resources_read<E: Extension>(
    ext: &E,
    params: &Value,
) -> Result<Value, (i32, String)> {
    ext.handle_rpc("resources/read", params.clone())
        .await
        .map_err(|e| (e.code().numeric(), e.message().to_string()))
}

async fn handle_resource_templates_list<E: Extension>(
    ext: &E,
    params: &Value,
) -> Result<Value, (i32, String)> {
    ext.handle_rpc("resources/templates/list", params.clone())
        .await
        .map_err(|e| (e.code().numeric(), e.message().to_string()))
}

async fn handle_resources_subscribe<E: Extension>(
    ext: &E,
    params: &Value,
) -> Result<Value, (i32, String)> {
    ext.handle_rpc("resources/subscribe", params.clone())
        .await
        .map_err(|e| (e.code().numeric(), e.message().to_string()))
}

async fn handle_prompts_list<E: Extension>(
    ext: &E,
    params: &Value,
) -> Result<Value, (i32, String)> {
    let result = ext
        .handle_rpc("prompts/list", params.clone())
        .await
        .map_err(|e| (e.code().numeric(), e.message().to_string()))?;

    // Strip Omegon-specific fields from each prompt.
    if let Some(prompts) = result.get("prompts").and_then(|v| v.as_array()) {
        let stripped: Vec<Value> = prompts.iter().map(strip_omegon_prompt).collect();
        let mut out = json!({ "prompts": stripped });
        if let Some(cursor) = result.get("next_cursor") {
            out["nextCursor"] = cursor.clone();
        }
        Ok(out)
    } else {
        Ok(result)
    }
}

async fn handle_prompts_get<E: Extension>(ext: &E, params: &Value) -> Result<Value, (i32, String)> {
    let result = ext
        .handle_rpc("prompts/get", params.clone())
        .await
        .map_err(|e| (e.code().numeric(), e.message().to_string()))?;

    // Strip MindFacts content from messages — MCP clients can't resolve them.
    if let Some(messages) = result.get("messages").and_then(|v| v.as_array()) {
        let filtered: Vec<&Value> = messages
            .iter()
            .filter(|m| {
                m.get("content")
                    .and_then(|c| c.get("type"))
                    .and_then(|t| t.as_str())
                    != Some("mind_facts")
            })
            .collect();
        let mut out = result.clone();
        out["messages"] = json!(filtered);
        Ok(out)
    } else {
        Ok(result)
    }
}

// ─── Translation helpers ─────────────────────────────────────────────────

/// Convert an Omegon tool definition to MCP format.
fn omegon_tool_to_mcp(tool: &Value) -> Value {
    json!({
        "name": tool.get("name").cloned().unwrap_or(json!("")),
        "title": tool.get("label").cloned().unwrap_or(json!("")),
        "description": tool.get("description").cloned().unwrap_or(json!("")),
        "inputSchema": tool.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
    })
}

/// Strip Omegon-specific fields from a resource.
fn strip_omegon_resource(resource: &Value) -> Value {
    let mut r = resource.clone();
    if let Some(obj) = r.as_object_mut() {
        obj.remove("widget_renderer");
        obj.remove("mind_section");
        obj.remove("trust_level");
        // MCP uses mimeType, not mime_type
        if let Some(mt) = obj.remove("mime_type") {
            obj.insert("mimeType".to_string(), mt);
        }
    }
    r
}

/// Strip Omegon-specific fields from a prompt.
fn strip_omegon_prompt(prompt: &Value) -> Value {
    let mut p = prompt.clone();
    if let Some(obj) = p.as_object_mut() {
        obj.remove("mind_context");
        obj.remove("inject_project_context");
    }
    p
}

/// Extract MCP-compatible content blocks from a tool result.
fn extract_mcp_content(result: &Value) -> Vec<Value> {
    // If result has "content" array, use it.
    if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
        return content
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type").and_then(|t| t.as_str())?;
                match block_type {
                    "text" | "markdown" => Some(json!({
                        "type": "text",
                        "text": block.get("text").and_then(|t| t.as_str()).unwrap_or("")
                    })),
                    "image" => Some(json!({
                        "type": "image",
                        "data": block.get("url").or(block.get("data")).cloned().unwrap_or(json!("")),
                        "mimeType": block.get("media_type").or(block.get("mime_type")).cloned().unwrap_or(json!("image/png"))
                    })),
                    _ => None,
                }
            })
            .collect();
    }

    // Fallback: wrap the entire result as text.
    vec![json!({
        "type": "text",
        "text": serde_json::to_string(result).unwrap_or_default()
    })]
}

/// Extract HostActions for Omegon-aware MCP clients.
fn extract_host_actions_meta(result: &Value) -> Option<Value> {
    let actions = result.get("actions")?;
    match actions.as_array() {
        Some(array) if !array.is_empty() => Some(Value::Array(array.clone())),
        _ => None,
    }
}

/// Build an MCP error response.
fn mcp_error_response(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

/// Write a JSON-RPC message line to the writer.
async fn write_line(
    writer: &mut tokio::io::BufWriter<tokio::io::Stdout>,
    value: &Value,
) -> crate::Result<()> {
    let json = serde_json::to_string(value)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Tool translation ────────────────────────────────────────────

    #[test]
    fn test_omegon_tool_to_mcp() {
        let omegon_tool = json!({
            "name": "list_issues",
            "label": "List Issues",
            "description": "List all issues for an engagement",
            "parameters": {
                "type": "object",
                "properties": {
                    "engagement": { "type": "string" }
                },
                "required": ["engagement"]
            }
        });

        let mcp_tool = omegon_tool_to_mcp(&omegon_tool);

        assert_eq!(mcp_tool["name"], "list_issues");
        assert_eq!(mcp_tool["title"], "List Issues"); // label → title
        assert_eq!(mcp_tool["description"], "List all issues for an engagement");
        assert!(mcp_tool["inputSchema"]["properties"]["engagement"].is_object()); // parameters → inputSchema
        assert!(mcp_tool.get("label").is_none()); // label removed
        assert!(mcp_tool.get("parameters").is_none()); // parameters renamed
    }

    #[test]
    fn test_omegon_tool_to_mcp_minimal() {
        let tool = json!({"name": "foo"});
        let mcp = omegon_tool_to_mcp(&tool);

        assert_eq!(mcp["name"], "foo");
        assert_eq!(mcp["title"], "");
        assert_eq!(mcp["inputSchema"]["type"], "object");
    }

    // ─── Resource stripping ──────────────────────────────────────────

    #[test]
    fn test_strip_omegon_resource() {
        let resource = json!({
            "uri": "omegon://scribe/issues",
            "name": "Issues",
            "mime_type": "application/json",
            "widget_renderer": "table",
            "mind_section": "Issues",
            "trust_level": "internal"
        });

        let stripped = strip_omegon_resource(&resource);

        assert_eq!(stripped["uri"], "omegon://scribe/issues");
        assert_eq!(stripped["name"], "Issues");
        assert_eq!(stripped["mimeType"], "application/json"); // mime_type → mimeType
        assert!(stripped.get("widget_renderer").is_none());
        assert!(stripped.get("mind_section").is_none());
        assert!(stripped.get("trust_level").is_none());
        assert!(stripped.get("mime_type").is_none()); // renamed, not duplicated
    }

    #[test]
    fn test_strip_omegon_resource_no_omegon_fields() {
        let resource = json!({
            "uri": "file:///path",
            "name": "File"
        });

        let stripped = strip_omegon_resource(&resource);
        assert_eq!(stripped["uri"], "file:///path");
        assert_eq!(stripped["name"], "File");
    }

    // ─── Prompt stripping ────────────────────────────────────────────

    #[test]
    fn test_strip_omegon_prompt() {
        let prompt = json!({
            "name": "review",
            "description": "Code review",
            "arguments": [{"name": "file", "required": true}],
            "mind_context": true,
            "inject_project_context": true
        });

        let stripped = strip_omegon_prompt(&prompt);

        assert_eq!(stripped["name"], "review");
        assert_eq!(stripped["description"], "Code review");
        assert!(stripped["arguments"].is_array());
        assert!(stripped.get("mind_context").is_none());
        assert!(stripped.get("inject_project_context").is_none());
    }

    // ─── Content extraction ──────────────────────────────────────────

    #[test]
    fn test_extract_mcp_content_text() {
        let result = json!({
            "content": [
                {"type": "text", "text": "hello world"}
            ]
        });

        let content = extract_mcp_content(&result);
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "hello world");
    }

    #[test]
    fn test_extract_mcp_content_markdown_becomes_text() {
        let result = json!({
            "content": [
                {"type": "markdown", "text": "# Hello"}
            ]
        });

        let content = extract_mcp_content(&result);
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text"); // MCP doesn't have markdown type
        assert_eq!(content[0]["text"], "# Hello");
    }

    #[test]
    fn test_extract_mcp_content_fallback() {
        let result = json!({"status": "ok", "data": [1, 2, 3]});

        let content = extract_mcp_content(&result);
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        // Should contain the JSON-serialized result
        let text = content[0]["text"].as_str().unwrap();
        assert!(text.contains("status"));
    }

    #[test]
    fn test_extract_mcp_content_filters_unknown_types() {
        let result = json!({
            "content": [
                {"type": "text", "text": "good"},
                {"type": "mind_facts", "query": "test"},
                {"type": "text", "text": "also good"}
            ]
        });

        let content = extract_mcp_content(&result);
        assert_eq!(content.len(), 2); // mind_facts filtered out
    }

    // ─── Error responses ─────────────────────────────────────────────

    #[test]
    fn test_mcp_error_response() {
        let resp = mcp_error_response(Some(json!(1)), -32601, "not found");

        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["error"]["code"], -32601);
        assert_eq!(resp["error"]["message"], "not found");
    }

    #[test]
    fn test_mcp_error_response_null_id() {
        let resp = mcp_error_response(None, -32700, "parse error");
        assert!(resp["id"].is_null());
    }

    // ─── MCP initialize ──────────────────────────────────────────────

    #[test]
    fn test_mcp_initialize_response() {
        use async_trait::async_trait;

        #[derive(Default)]
        struct TestExt;

        #[async_trait]
        impl Extension for TestExt {
            fn name(&self) -> &str {
                "test-ext"
            }
            fn version(&self) -> &str {
                "0.1.0"
            }
            async fn handle_rpc(&self, _m: &str, _p: Value) -> crate::Result<Value> {
                Ok(json!([]))
            }
        }

        let ext = TestExt;
        let result = handle_mcp_initialize(&ext, &json!({})).unwrap();

        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], "test-ext");
        assert_eq!(result["serverInfo"]["version"], "0.1.0");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["resources"].is_object());
        assert!(result["capabilities"]["prompts"].is_object());
    }

    // ─── Integration: tools/list translation ─────────────────────────

    #[tokio::test]
    async fn test_tools_list_translates_format() {
        use async_trait::async_trait;

        #[derive(Default)]
        struct ToolExt;

        #[async_trait]
        impl Extension for ToolExt {
            fn name(&self) -> &str {
                "tool-ext"
            }
            fn version(&self) -> &str {
                "0.1.0"
            }
            async fn handle_rpc(&self, method: &str, _p: Value) -> crate::Result<Value> {
                match method {
                    "tools/list" | "get_tools" => Ok(json!([
                        {
                            "name": "search",
                            "label": "Search Docs",
                            "description": "Full-text search",
                            "parameters": {
                                "type": "object",
                                "properties": { "query": { "type": "string" } },
                                "required": ["query"]
                            }
                        }
                    ])),
                    _ => Err(crate::Error::method_not_found(method)),
                }
            }
        }

        let ext = ToolExt;
        let result = handle_tools_list(&ext, &json!({})).await.unwrap();

        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "search");
        assert_eq!(tools[0]["title"], "Search Docs"); // label → title
        assert!(tools[0]["inputSchema"]["properties"].is_object()); // parameters → inputSchema
        assert!(tools[0].get("label").is_none());
        assert!(tools[0].get("parameters").is_none());
    }

    // ─── Integration: tools/call translation ─────────────────────────

    #[tokio::test]
    async fn test_tools_call_translates() {
        use async_trait::async_trait;

        #[derive(Default)]
        struct ToolExt;

        #[async_trait]
        impl Extension for ToolExt {
            fn name(&self) -> &str {
                "tool-ext"
            }
            fn version(&self) -> &str {
                "0.1.0"
            }
            async fn handle_rpc(&self, method: &str, params: Value) -> crate::Result<Value> {
                match method {
                    "tools/call" => {
                        let name = params["name"].as_str().unwrap_or("");
                        Ok(json!({
                            "content": [{"type": "text", "text": format!("called {name}")}]
                        }))
                    }
                    _ => Err(crate::Error::method_not_found(method)),
                }
            }
        }

        let ext = ToolExt;
        let result = handle_tools_call(
            &ext,
            &json!({"name": "search", "arguments": {"query": "test"}}),
        )
        .await
        .unwrap();

        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["text"], "called search");
    }

    #[tokio::test]
    async fn test_tools_call_maps_actions_to_omegon_meta() {
        use async_trait::async_trait;

        #[derive(Default)]
        struct ActionExt;

        #[async_trait]
        impl Extension for ActionExt {
            fn name(&self) -> &str {
                "action-ext"
            }
            fn version(&self) -> &str {
                "0.1.0"
            }
            async fn handle_rpc(&self, method: &str, _params: Value) -> crate::Result<Value> {
                match method {
                    "tools/call" => Ok(json!({
                        "content": [{"type": "text", "text": "open reader"}],
                        "actions": [{
                            "id": "open-reader",
                            "type": "terminal.create@1",
                            "execution": "auto_if_allowed",
                            "params": {"command": "bookokrat"}
                        }]
                    })),
                    _ => Err(crate::Error::method_not_found(method)),
                }
            }
        }

        let ext = ActionExt;
        let result = handle_tools_call(&ext, &json!({"name": "reader"}))
            .await
            .unwrap();

        assert_eq!(result["content"][0]["text"], "open reader");
        assert_eq!(
            result["_meta"]["omegon/hostActions"][0]["id"],
            "open-reader"
        );
    }

    #[test]
    fn test_extract_host_actions_meta_omits_empty_actions() {
        assert!(extract_host_actions_meta(&json!({"actions": []})).is_none());
    }

    // ─── Integration: resources/list strips fields ────────────────────

    #[tokio::test]
    async fn test_resources_list_strips_omegon_fields() {
        use async_trait::async_trait;

        #[derive(Default)]
        struct ResExt;

        #[async_trait]
        impl Extension for ResExt {
            fn name(&self) -> &str {
                "res-ext"
            }
            fn version(&self) -> &str {
                "0.1.0"
            }
            async fn handle_rpc(&self, method: &str, _p: Value) -> crate::Result<Value> {
                match method {
                    "resources/list" => Ok(json!({
                        "resources": [{
                            "uri": "omegon://ext/data",
                            "name": "Data",
                            "mime_type": "application/json",
                            "widget_renderer": "table",
                            "mind_section": "Data"
                        }],
                        "next_cursor": "page2"
                    })),
                    _ => Err(crate::Error::method_not_found(method)),
                }
            }
        }

        let ext = ResExt;
        let result = handle_resources_list(&ext, &json!({})).await.unwrap();

        let resources = result["resources"].as_array().unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0]["mimeType"], "application/json");
        assert!(resources[0].get("widget_renderer").is_none());
        assert!(resources[0].get("mind_section").is_none());
        assert_eq!(result["nextCursor"], "page2");
    }

    // ─── Integration: prompts/get filters mind_facts ──────────────────

    #[tokio::test]
    async fn test_prompts_get_filters_mind_facts() {
        use async_trait::async_trait;

        #[derive(Default)]
        struct PromptExt;

        #[async_trait]
        impl Extension for PromptExt {
            fn name(&self) -> &str {
                "prompt-ext"
            }
            fn version(&self) -> &str {
                "0.1.0"
            }
            async fn handle_rpc(&self, method: &str, _p: Value) -> crate::Result<Value> {
                match method {
                    "prompts/get" => Ok(json!({
                        "description": "Review prompt",
                        "messages": [
                            {"role": "user", "content": {"type": "text", "text": "Review this"}},
                            {"role": "user", "content": {"type": "mind_facts", "query": "patterns", "limit": 3}},
                            {"role": "assistant", "content": {"type": "text", "text": "Sure"}}
                        ]
                    })),
                    _ => Err(crate::Error::method_not_found(method)),
                }
            }
        }

        let ext = PromptExt;
        let result = handle_prompts_get(&ext, &json!({"name": "review"}))
            .await
            .unwrap();

        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2); // mind_facts message filtered out
        assert_eq!(messages[0]["content"]["type"], "text");
        assert_eq!(messages[1]["content"]["type"], "text");
    }
}

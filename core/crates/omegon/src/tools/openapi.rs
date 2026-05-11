use std::collections::HashMap;

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolCapability, ToolDefinition, ToolProvider, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::openapi_resolve;

#[derive(Debug, Clone, Deserialize)]
pub struct OpenApiConfig {
    pub spec: String,
    pub auth: String,
    pub secret: String,
    #[serde(default)]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub confirm: Vec<String>,
    #[serde(default)]
    pub read_only: bool,
}

struct CompiledSpec {
    base_url: String,
    prefix: String,
    endpoints: Vec<CompiledEndpoint>,
    auth_header_name: String,
    auth_header_prefix: String,
    secret_env: String,
}

struct CompiledEndpoint {
    tool_name: String,
    method: reqwest::Method,
    path_template: String,
    path_params: Vec<String>,
    query_params: Vec<String>,
    has_body: bool,
    description: String,
    tool_parameters: Value,
}

pub struct OpenApiToolProvider {
    specs: Vec<CompiledSpec>,
    client: reqwest::Client,
    dispatch: HashMap<String, (usize, usize)>,
}

impl OpenApiToolProvider {
    pub fn from_configs(configs: Vec<(String, OpenApiConfig)>) -> anyhow::Result<Self> {
        let mut specs = Vec::new();
        let mut dispatch = HashMap::new();

        for (name, config) in configs {
            let raw = load_spec_content(&config.spec)?;
            let mut doc: Value = parse_spec(&raw)?;
            openapi_resolve::resolve_refs(&mut doc)?;

            let compiled = compile(&name, &doc, &config)?;
            let spec_idx = specs.len();
            for (ep_idx, ep) in compiled.endpoints.iter().enumerate() {
                dispatch.insert(ep.tool_name.clone(), (spec_idx, ep_idx));
            }
            specs.push(compiled);
        }

        Ok(Self {
            specs,
            client: reqwest::Client::new(),
            dispatch,
        })
    }

    pub fn tool_count(&self) -> usize {
        self.specs.iter().map(|s| s.endpoints.len()).sum()
    }

    fn resolve_secret(&self, env_name: &str) -> Option<String> {
        std::env::var(env_name).ok().filter(|v| !v.is_empty())
    }
}

#[async_trait]
impl ToolProvider for OpenApiToolProvider {
    fn tools(&self) -> Vec<ToolDefinition> {
        self.specs
            .iter()
            .flat_map(|spec| {
                spec.endpoints.iter().map(|ep| ToolDefinition {
                    name: ep.tool_name.clone(),
                    label: ep.tool_name.clone(),
                    description: ep.description.clone(),
                    parameters: ep.tool_parameters.clone(),
                    capabilities: if ep.method == reqwest::Method::GET {
                        vec![ToolCapability::RepoInspection]
                    } else {
                        vec![ToolCapability::StateChanging]
                    },
                })
            })
            .collect()
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let &(spec_idx, ep_idx) = self
            .dispatch
            .get(tool_name)
            .ok_or_else(|| anyhow::anyhow!("unknown OpenAPI tool: {tool_name}"))?;
        let spec = &self.specs[spec_idx];
        let ep = &spec.endpoints[ep_idx];

        let mut path = ep.path_template.clone();
        for param in &ep.path_params {
            let val = args
                .get(param)
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            path = path.replace(&format!("{{{param}}}"), val);
        }
        let mut url = format!("{}{}", spec.base_url.trim_end_matches('/'), path);

        let mut query_parts: Vec<String> = Vec::new();
        for param in &ep.query_params {
            if let Some(val) = args.get(param) {
                let s = match val {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                query_parts.push(format!("{param}={s}"));
            }
        }
        if !query_parts.is_empty() {
            url.push('?');
            url.push_str(&query_parts.join("&"));
        }

        let mut req = self.client.request(ep.method.clone(), &url);

        if ep.has_body {
            let mut body = args.clone();
            if let Some(obj) = body.as_object_mut() {
                for p in &ep.path_params {
                    obj.remove(p);
                }
                for p in &ep.query_params {
                    obj.remove(p);
                }
            }
            req = req.header("Content-Type", "application/json").json(&body);
        }

        if let Some(secret_val) = self.resolve_secret(&spec.secret_env) {
            let header_val = if spec.auth_header_prefix.is_empty() {
                secret_val
            } else {
                format!("{} {secret_val}", spec.auth_header_prefix)
            };
            req = req.header(spec.auth_header_name.as_str(), header_val);
        }

        let resp = req
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await;

        match resp {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                let truncated = if text.len() > 50_000 {
                    format!("{}...(truncated)", &text[..50_000])
                } else {
                    text
                };

                if status.is_success() {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: truncated }],
                        details: json!({"status": status.as_u16()}),
                    })
                } else {
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("API error {status}: {truncated}"),
                        }],
                        details: json!({"status": status.as_u16(), "error": true}),
                    })
                }
            }
            Err(e) => Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("Request failed: {e}"),
                }],
                details: json!({"error": true}),
            }),
        }
    }
}

fn parse_spec(raw: &str) -> anyhow::Result<Value> {
    serde_json::from_str(raw).or_else(|_| {
        serde_yaml::from_str(raw).map_err(|e| anyhow::anyhow!("failed to parse spec as JSON or YAML: {e}"))
    })
}

fn load_spec_content(spec: &str) -> anyhow::Result<String> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        reqwest::blocking::get(spec)?
            .text()
            .map_err(|e| anyhow::anyhow!("failed to fetch spec from {spec}: {e}"))
    } else {
        std::fs::read_to_string(spec)
            .map_err(|e| anyhow::anyhow!("failed to read spec file {spec}: {e}"))
    }
}

fn compile(name: &str, doc: &Value, config: &OpenApiConfig) -> anyhow::Result<CompiledSpec> {
    let base_url = config
        .base_url_override
        .clone()
        .or_else(|| {
            doc.get("servers")
                .and_then(|s| s.get(0))
                .and_then(|s| s.get("url"))
                .and_then(|u| u.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "https://api.example.com".into());

    let prefix = sanitize_prefix(name);

    let (auth_header_name, auth_header_prefix) = match config.auth.as_str() {
        "bearer" => ("Authorization".to_string(), "Bearer".to_string()),
        "basic" => ("Authorization".to_string(), "Basic".to_string()),
        other => (other.to_string(), String::new()),
    };

    let paths = match doc.get("paths").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => {
            return Ok(CompiledSpec {
                base_url,
                prefix,
                endpoints: Vec::new(),
                auth_header_name,
                auth_header_prefix,
                secret_env: config.secret.clone(),
            });
        }
    };

    let methods = ["get", "post", "put", "patch", "delete"];
    let mut endpoints = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for (path, path_item) in paths {
        let path_obj = match path_item.as_object() {
            Some(o) => o,
            None => continue,
        };

        let path_params_shared: Vec<Value> = path_obj
            .get("parameters")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        for method_str in &methods {
            let op = match path_obj.get(*method_str) {
                Some(v) => v,
                None => continue,
            };

            if config.read_only && *method_str != "get" {
                continue;
            }

            let operation_id = op
                .get("operationId")
                .and_then(|v| v.as_str())
                .map(|s| to_snake_case(s))
                .unwrap_or_else(|| {
                    let slug = path.replace('/', "_").replace('{', "").replace('}', "");
                    format!("{method_str}{slug}")
                });

            let mut tool_name = format!("api_{prefix}_{operation_id}");
            if tool_name.len() > 64 {
                tool_name.truncate(64);
            }
            while seen_names.contains(&tool_name) {
                tool_name.push('_');
            }
            seen_names.insert(tool_name.clone());

            let op_params: Vec<Value> = op
                .get("parameters")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            let all_params: Vec<&Value> = path_params_shared.iter().chain(op_params.iter()).collect();

            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();
            let mut path_param_names = Vec::new();
            let mut query_param_names = Vec::new();

            for param in &all_params {
                let param_name = param.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                let location = param.get("in").and_then(|i| i.as_str()).unwrap_or("query");
                let schema = param.get("schema").cloned().unwrap_or(json!({"type": "string"}));
                let param_required = param.get("required").and_then(|r| r.as_bool()).unwrap_or(location == "path");
                let desc = param.get("description").and_then(|d| d.as_str()).unwrap_or("");

                let mut prop = schema;
                if !desc.is_empty() {
                    if let Some(obj) = prop.as_object_mut() {
                        obj.insert("description".into(), json!(desc));
                    }
                }
                properties.insert(param_name.to_string(), prop);

                if param_required {
                    required.push(param_name.to_string());
                }

                match location {
                    "path" => path_param_names.push(param_name.to_string()),
                    "query" => query_param_names.push(param_name.to_string()),
                    _ => {}
                }
            }

            let has_body = op
                .get("requestBody")
                .and_then(|rb| rb.get("content"))
                .and_then(|c| c.get("application/json"))
                .and_then(|j| j.get("schema"))
                .is_some();

            if has_body {
                if let Some(body_schema) = op
                    .get("requestBody")
                    .and_then(|rb| rb.get("content"))
                    .and_then(|c| c.get("application/json"))
                    .and_then(|j| j.get("schema"))
                {
                    if let Some(body_props) = body_schema.get("properties").and_then(|p| p.as_object()) {
                        for (k, v) in body_props {
                            if !properties.contains_key(k) {
                                properties.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    if let Some(body_required) = body_schema.get("required").and_then(|r| r.as_array()) {
                        for r in body_required {
                            if let Some(s) = r.as_str() {
                                if !required.contains(&s.to_string()) {
                                    required.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }

            let tool_parameters = json!({
                "type": "object",
                "properties": properties,
                "required": required,
            });

            let summary = op.get("summary").and_then(|s| s.as_str()).unwrap_or("");
            let op_desc = op.get("description").and_then(|d| d.as_str()).unwrap_or("");
            let description = if !summary.is_empty() && !op_desc.is_empty() {
                format!("{summary} — {op_desc}")
            } else if !summary.is_empty() {
                summary.to_string()
            } else {
                op_desc.to_string()
            };
            let description = if description.len() > 1024 {
                format!("{}...", &description[..1021])
            } else if description.is_empty() {
                format!("{} {}", method_str.to_uppercase(), path)
            } else {
                description
            };

            endpoints.push(CompiledEndpoint {
                tool_name,
                method: method_str.to_uppercase().parse().unwrap_or(reqwest::Method::GET),
                path_template: path.clone(),
                path_params: path_param_names,
                query_params: query_param_names,
                has_body,
                description,
                tool_parameters,
            });
        }
    }

    tracing::info!(
        prefix = %prefix,
        endpoints = endpoints.len(),
        "compiled OpenAPI spec"
    );

    Ok(CompiledSpec {
        base_url,
        prefix,
        endpoints,
        auth_header_name,
        auth_header_prefix,
        secret_env: config.secret.clone(),
    })
}

fn sanitize_prefix(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            if let Some(prev) = s.chars().nth(i - 1) {
                if prev.is_lowercase() || prev.is_ascii_digit() {
                    out.push('_');
                }
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out.replace('-', "_").replace(' ', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PETSTORE_JSON: &str = r#"{
        "openapi": "3.0.0",
        "info": {"title": "Petstore", "version": "1.0.0"},
        "servers": [{"url": "https://petstore.example.com/v1"}],
        "paths": {
            "/pets": {
                "get": {
                    "operationId": "listPets",
                    "summary": "List all pets",
                    "parameters": [
                        {"name": "limit", "in": "query", "schema": {"type": "integer"}}
                    ]
                },
                "post": {
                    "operationId": "createPet",
                    "summary": "Create a pet",
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "name": {"type": "string"},
                                        "tag": {"type": "string"}
                                    },
                                    "required": ["name"]
                                }
                            }
                        }
                    }
                }
            },
            "/pets/{petId}": {
                "get": {
                    "operationId": "showPetById",
                    "summary": "Info for a specific pet",
                    "parameters": [
                        {"name": "petId", "in": "path", "required": true, "schema": {"type": "string"}}
                    ]
                }
            }
        }
    }"#;

    const PETSTORE_YAML: &str = r#"
openapi: "3.0.0"
info:
  title: Petstore
  version: "1.0.0"
servers:
  - url: https://petstore.example.com/v1
paths:
  /pets:
    get:
      operationId: listPets
      summary: List all pets
      parameters:
        - name: limit
          in: query
          schema:
            type: integer
"#;

    fn test_config() -> OpenApiConfig {
        OpenApiConfig {
            spec: String::new(),
            auth: "bearer".into(),
            secret: "TEST_KEY".into(),
            base_url_override: None,
            allow: Vec::new(),
            confirm: Vec::new(),
            read_only: false,
        }
    }

    #[test]
    fn compile_petstore_json() {
        let mut doc: Value = serde_json::from_str(PETSTORE_JSON).unwrap();
        openapi_resolve::resolve_refs(&mut doc).unwrap();
        let spec = compile("petstore", &doc, &test_config()).unwrap();
        assert_eq!(spec.endpoints.len(), 3);
        assert_eq!(spec.base_url, "https://petstore.example.com/v1");
        assert_eq!(spec.prefix, "petstore");

        let names: Vec<&str> = spec.endpoints.iter().map(|e| e.tool_name.as_str()).collect();
        assert!(names.contains(&"api_petstore_list_pets"));
        assert!(names.contains(&"api_petstore_create_pet"));
        assert!(names.contains(&"api_petstore_show_pet_by_id"));
    }

    #[test]
    fn compile_petstore_yaml() {
        let mut doc: Value = parse_spec(PETSTORE_YAML).unwrap();
        openapi_resolve::resolve_refs(&mut doc).unwrap();
        let spec = compile("petstore", &doc, &test_config()).unwrap();
        assert_eq!(spec.endpoints.len(), 1);
        assert_eq!(spec.endpoints[0].tool_name, "api_petstore_list_pets");
    }

    #[test]
    fn path_params_extracted() {
        let mut doc: Value = serde_json::from_str(PETSTORE_JSON).unwrap();
        openapi_resolve::resolve_refs(&mut doc).unwrap();
        let spec = compile("petstore", &doc, &test_config()).unwrap();
        let show = spec.endpoints.iter().find(|e| e.tool_name == "api_petstore_show_pet_by_id").unwrap();
        assert_eq!(show.path_params, vec!["petId"]);
        assert_eq!(show.method.as_str(), "GET");
    }

    #[test]
    fn body_params_merged_into_schema() {
        let mut doc: Value = serde_json::from_str(PETSTORE_JSON).unwrap();
        openapi_resolve::resolve_refs(&mut doc).unwrap();
        let spec = compile("petstore", &doc, &test_config()).unwrap();
        let create = spec.endpoints.iter().find(|e| e.tool_name == "api_petstore_create_pet").unwrap();
        assert!(create.has_body);
        let props = create.tool_parameters.get("properties").unwrap();
        assert!(props.get("name").is_some());
        assert!(props.get("tag").is_some());
        let req = create.tool_parameters.get("required").unwrap().as_array().unwrap();
        assert!(req.contains(&json!("name")));
    }

    #[test]
    fn read_only_filters_mutations() {
        let mut config = test_config();
        config.read_only = true;
        let mut doc: Value = serde_json::from_str(PETSTORE_JSON).unwrap();
        openapi_resolve::resolve_refs(&mut doc).unwrap();
        let spec = compile("petstore", &doc, &config).unwrap();
        assert_eq!(spec.endpoints.len(), 2);
        for ep in &spec.endpoints {
            assert_eq!(ep.method.as_str(), "GET");
        }
    }

    #[test]
    fn snake_case_conversion() {
        assert_eq!(to_snake_case("listPets"), "list_pets");
        assert_eq!(to_snake_case("showPetById"), "show_pet_by_id");
        assert_eq!(to_snake_case("CreateCustomer"), "create_customer");
        assert_eq!(to_snake_case("getAPIKey"), "get_apikey");
        assert_eq!(to_snake_case("already_snake"), "already_snake");
    }

    #[test]
    fn base_url_override() {
        let mut config = test_config();
        config.base_url_override = Some("https://custom.api.com".into());
        let mut doc: Value = serde_json::from_str(PETSTORE_JSON).unwrap();
        openapi_resolve::resolve_refs(&mut doc).unwrap();
        let spec = compile("petstore", &doc, &config).unwrap();
        assert_eq!(spec.base_url, "https://custom.api.com");
    }

    #[test]
    fn empty_paths_compiles_to_zero_endpoints() {
        let doc = json!({"openapi": "3.0.0", "info": {"title": "Empty"}, "paths": {}});
        let spec = compile("empty", &doc, &test_config()).unwrap();
        assert!(spec.endpoints.is_empty());
    }

    #[test]
    fn tool_name_dedup() {
        let doc = json!({
            "openapi": "3.0.0",
            "info": {"title": "Dupe"},
            "paths": {
                "/a": {"get": {"operationId": "fetch"}},
                "/b": {"get": {"operationId": "fetch"}}
            }
        });
        let spec = compile("test", &doc, &test_config()).unwrap();
        let names: Vec<&str> = spec.endpoints.iter().map(|e| e.tool_name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1]);
    }
}

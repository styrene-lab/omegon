use omegon_traits::{ContentBlock, ToolResult};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub(super) struct ParsedExtensionToolResult {
    pub content: Vec<ContentBlock>,
    pub structured: Option<Value>,
    pub metadata: Option<Value>,
    pub host_actions: Vec<Value>,
    pub host_action_outcomes: Vec<Value>,
}

impl ParsedExtensionToolResult {
    pub fn into_tool_result(self) -> ToolResult {
        let mut details = serde_json::Map::new();
        if let Some(structured) = self.structured {
            details.insert("structured".to_string(), structured);
        }
        if let Some(metadata) = self.metadata {
            details.insert("metadata".to_string(), metadata);
        }
        if !self.host_actions.is_empty() {
            details.insert("host_actions".to_string(), Value::Array(self.host_actions));
        }
        if !self.host_action_outcomes.is_empty() {
            details.insert(
                "host_action_outcomes".to_string(),
                Value::Array(self.host_action_outcomes),
            );
        }
        ToolResult {
            content: self.content,
            details: Value::Object(details),
        }
    }
}

/// Parse an extension RPC result into a structured envelope while preserving
/// backward compatibility with legacy extensions that returned arbitrary JSON.
pub(super) fn parse_extension_tool_envelope(output: Value) -> ParsedExtensionToolResult {
    let Some(obj) = output.as_object() else {
        return legacy_envelope(output);
    };

    let has_envelope_fields = obj.contains_key("content")
        || obj.contains_key("structured")
        || obj.contains_key("metadata")
        || obj.contains_key("actions");
    if !has_envelope_fields {
        return legacy_envelope(output);
    }

    let content = parse_content(obj.get("content")).unwrap_or_else(|| {
        vec![ContentBlock::Text {
            text: output.to_string(),
        }]
    });

    let (host_actions, host_action_outcomes) = obj
        .get("actions")
        .map(partition_actions)
        .unwrap_or_default();

    ParsedExtensionToolResult {
        content,
        structured: obj.get("structured").cloned(),
        metadata: obj.get("metadata").cloned(),
        host_actions,
        host_action_outcomes,
    }
}

/// Parse an extension RPC result into Omegon's tool result shape while preserving
/// backward compatibility with legacy extensions that returned arbitrary JSON.
pub(super) fn parse_extension_tool_result(output: Value) -> ToolResult {
    parse_extension_tool_envelope(output).into_tool_result()
}

fn legacy_envelope(output: Value) -> ParsedExtensionToolResult {
    ParsedExtensionToolResult {
        content: vec![ContentBlock::Text {
            text: output.to_string(),
        }],
        structured: None,
        metadata: None,
        host_actions: Vec::new(),
        host_action_outcomes: Vec::new(),
    }
}

fn parse_content(content: Option<&Value>) -> Option<Vec<ContentBlock>> {
    let array = content?.as_array()?;
    let blocks: Vec<ContentBlock> = array
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| ContentBlock::Text {
                        text: text.to_string(),
                    })
            }
            Some("image") => {
                let url = block.get("url")?.as_str()?;
                let media_type = block
                    .get("media_type")
                    .or_else(|| block.get("mediaType"))?
                    .as_str()?;
                Some(ContentBlock::Image {
                    url: url.to_string(),
                    media_type: media_type.to_string(),
                })
            }
            _ => None,
        })
        .collect();
    if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    }
}

fn partition_actions(actions: &Value) -> (Vec<Value>, Vec<Value>) {
    let Some(array) = actions.as_array() else {
        return (
            Vec::new(),
            vec![invalid_outcome("actions", "actions must be an array")],
        );
    };

    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for (idx, action) in array.iter().enumerate() {
        match serde_json::from_value::<omegon_extension::HostAction>(action.clone()) {
            Ok(_) => valid.push(action.clone()),
            Err(err) => invalid.push(invalid_outcome(
                action
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("<invalid>"),
                format!("invalid action at index {idx}: {err}"),
            )),
        }
    }
    (valid, invalid)
}

fn invalid_outcome(action_id: impl Into<String>, message: impl Into<String>) -> Value {
    json!({
        "action_id": action_id.into(),
        "status": "invalid",
        "error": {
            "code": "invalid_action",
            "message": message.into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(result: &ToolResult) -> &str {
        match &result.content[0] {
            ContentBlock::Text { text } => text,
            ContentBlock::Image { .. } => panic!("expected text"),
        }
    }

    #[test]
    fn raw_json_output_still_renders_as_legacy_text() {
        let result = parse_extension_tool_result(json!({"answer": 42}));
        assert_eq!(text(&result), r#"{"answer":42}"#);
        assert_eq!(result.details, json!({}));
    }

    #[test]
    fn content_array_extracts_to_ordinary_content() {
        let result = parse_extension_tool_result(json!({
            "content": [{"type": "text", "text": "hello"}]
        }));
        assert_eq!(text(&result), "hello");
        assert_eq!(result.details, json!({}));
    }

    #[test]
    fn malformed_content_array_falls_back_to_legacy_text() {
        let output = json!({
            "content": [{"type": "text"}],
            "metadata": {"source": "test"}
        });
        let result = parse_extension_tool_result(output.clone());
        assert_eq!(text(&result), output.to_string());
        assert_eq!(result.details["metadata"], json!({"source": "test"}));
    }

    #[test]
    fn image_content_accepts_camel_case_media_type() {
        let result = parse_extension_tool_result(json!({
            "content": [{"type": "image", "url": "file:///tmp/a.png", "mediaType": "image/png"}]
        }));
        match &result.content[0] {
            ContentBlock::Image { url, media_type } => {
                assert_eq!(url, "file:///tmp/a.png");
                assert_eq!(media_type, "image/png");
            }
            ContentBlock::Text { .. } => panic!("expected image"),
        }
    }

    #[test]
    fn valid_actions_are_extracted_separately_from_content() {
        let result = parse_extension_tool_result(json!({
            "content": [{"type": "text", "text": "Opening reader"}],
            "actions": [{
                "id": "open-reader",
                "type": "terminal.create@1",
                "params": {"command": "bookokrat"}
            }]
        }));

        assert_eq!(text(&result), "Opening reader");
        assert_eq!(result.details["host_actions"][0]["id"], "open-reader");
        assert!(result.details.get("host_action_outcomes").is_none());
    }

    #[test]
    fn malformed_actions_preserve_content_and_emit_invalid_outcomes() {
        let result = parse_extension_tool_result(json!({
            "content": [{"type": "text", "text": "still render me"}],
            "actions": [{"id": "broken", "params": {}}]
        }));

        assert_eq!(text(&result), "still render me");
        assert!(result.details.get("host_actions").is_none());
        assert_eq!(
            result.details["host_action_outcomes"][0]["status"],
            "invalid"
        );
        assert_eq!(
            result.details["host_action_outcomes"][0]["action_id"],
            "broken"
        );
    }
}

use omegon_traits::{ContentBlock, ToolResult};
use serde_json::{Value, json};

/// Parse an extension RPC result into Omegon's tool result shape while preserving
/// backward compatibility with legacy extensions that returned arbitrary JSON.
pub(super) fn parse_extension_tool_result(output: Value) -> ToolResult {
    let Some(obj) = output.as_object() else {
        return legacy_result(output);
    };

    let has_envelope_fields = obj.contains_key("content")
        || obj.contains_key("structured")
        || obj.contains_key("metadata")
        || obj.contains_key("actions");
    if !has_envelope_fields {
        return legacy_result(output);
    }

    let content = parse_content(obj.get("content")).unwrap_or_else(|| {
        vec![ContentBlock::Text {
            text: output.to_string(),
        }]
    });

    let mut details = serde_json::Map::new();
    if let Some(structured) = obj.get("structured") {
        details.insert("structured".to_string(), structured.clone());
    }
    if let Some(metadata) = obj.get("metadata") {
        details.insert("metadata".to_string(), metadata.clone());
    }
    if let Some(actions) = obj.get("actions") {
        let (valid, invalid) = partition_actions(actions);
        if !valid.is_empty() {
            details.insert("host_actions".to_string(), Value::Array(valid));
        }
        if !invalid.is_empty() {
            details.insert("host_action_outcomes".to_string(), Value::Array(invalid));
        }
    }

    ToolResult {
        content,
        details: Value::Object(details),
    }
}

fn legacy_result(output: Value) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: output.to_string(),
        }],
        details: json!({}),
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
                let media_type = block.get("media_type")?.as_str()?;
                Some(ContentBlock::Image {
                    url: url.to_string(),
                    media_type: media_type.to_string(),
                })
            }
            _ => None,
        })
        .collect();
    Some(blocks)
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

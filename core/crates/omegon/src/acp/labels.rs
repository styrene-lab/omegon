pub(super) fn compact_tool_call_label(name: &str, args: Option<&serde_json::Value>) -> String {
    let Some(summary) = args.and_then(compact_tool_call_summary) else {
        return name.to_string();
    };
    let summary = normalize_tool_label_fragment(&summary);
    if summary.is_empty() {
        name.to_string()
    } else {
        format!("{name} — {}", truncate_tool_label_fragment(&summary, 80))
    }
}

fn compact_tool_call_summary(args: &serde_json::Value) -> Option<String> {
    if let Some(obj) = args.as_object() {
        for key in [
            "path",
            "query",
            "command",
            "task",
            "directive",
            "prompt",
            "title",
            "name",
            "url",
            "uri",
        ] {
            if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                return Some(value.to_string());
            }
        }
        for key in ["paths", "scope"] {
            if let Some(values) = obj.get(key).and_then(|v| v.as_array()) {
                let joined = values
                    .iter()
                    .filter_map(|v| v.as_str())
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(", ");
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
        }
    }
    args.as_str().map(ToString::to_string)
}

fn normalize_tool_label_fragment(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_tool_label_fragment(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_tool_call_label_summarizes_common_args() {
        assert_eq!(
            compact_tool_call_label(
                "read",
                Some(&serde_json::json!({"path":"docs/acp-surface.md"}))
            ),
            "read — docs/acp-surface.md"
        );
        assert_eq!(
            compact_tool_call_label(
                "memory_recall",
                Some(&serde_json::json!({"query":"ACP hardening"}))
            ),
            "memory_recall — ACP hardening"
        );
        assert_eq!(
            compact_tool_call_label(
                "bash",
                Some(&serde_json::json!({"command":"cargo test -p omegon acp"}))
            ),
            "bash — cargo test -p omegon acp"
        );
    }

    #[test]
    fn compact_tool_call_label_truncates_long_fragments() {
        let label = compact_tool_call_label(
            "bash",
            Some(&serde_json::json!({
                "command": "0123456789 ".repeat(20)
            })),
        );
        assert!(label.starts_with("bash — 0123456789"), "{label}");
        assert!(label.ends_with('…'), "{label}");
        assert!(
            label.chars().count() <= "bash — ".chars().count() + 81,
            "{label}"
        );
    }
}

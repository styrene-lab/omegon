//! Ambient capture — parse omg:-namespaced XML tags from agent responses.
//!
//! The agent marks lifecycle artifacts inline in its reasoning:
//!   <omg:decision status="decided">rationale here</omg:decision>
//!   <omg:constraint>limitation discovered</omg:constraint>
//!   <omg:question>open question to track</omg:question>
//!   <omg:approach>current approach description</omg:approach>
//!   <omg:failed reason="why it failed">approach that didn't work</omg:failed>
//!   <omg:phase>explore</omg:phase>
//!
//! Parsing skips content inside fenced code blocks (``` ... ```)
//! to avoid false positives from code examples.

use std::borrow::Cow;

/// A lifecycle artifact captured from the agent's response.
#[derive(Debug, Clone)]
pub enum AmbientCapture {
    Decision {
        status: String,
        content: String,
    },
    Constraint(String),
    Question(String),
    Approach(String),
    Failed {
        description: String,
        reason: String,
    },
    Phase(String),
}

/// Parse all omg: blocks from an assistant response.
/// Skips content inside fenced code blocks.
pub fn parse_ambient_blocks(text: &str) -> Vec<AmbientCapture> {
    let mut captures = Vec::new();
    let cleaned = strip_fenced_code_blocks(text);

    // Parse <omg:TAG ...>content</omg:TAG> patterns
    let mut remaining = cleaned.as_ref();

    while let Some(start_pos) = remaining.find("<omg:") {
        let after_tag_start = &remaining[start_pos + 5..];

        // Find tag name (up to space or >)
        let tag_end = after_tag_start
            .find([' ', '>'])
            .unwrap_or(after_tag_start.len());
        let tag_name = &after_tag_start[..tag_end];

        // Find the closing >
        let header_end = match after_tag_start.find('>') {
            Some(pos) => pos,
            None => {
                remaining = &remaining[start_pos + 5..];
                continue;
            }
        };

        let attrs = &after_tag_start[tag_end..header_end].trim();
        let content_start = header_end + 1;

        // Find closing tag
        let close_tag = format!("</omg:{}>", tag_name);
        let content_area = &after_tag_start[content_start..];
        let close_pos = match content_area.find(&close_tag) {
            Some(pos) => pos,
            None => {
                remaining = &remaining[start_pos + 5..];
                continue;
            }
        };

        let content = content_area[..close_pos].trim().to_string();

        match tag_name {
            "decision" => {
                let status = extract_attr(attrs, "status")
                    .unwrap_or_else(|| "exploring".to_string());
                captures.push(AmbientCapture::Decision { status, content });
            }
            "constraint" => {
                captures.push(AmbientCapture::Constraint(content));
            }
            "question" => {
                captures.push(AmbientCapture::Question(content));
            }
            "approach" => {
                captures.push(AmbientCapture::Approach(content));
            }
            "failed" => {
                let reason = extract_attr(attrs, "reason")
                    .unwrap_or_default();
                captures.push(AmbientCapture::Failed {
                    description: content,
                    reason,
                });
            }
            "phase" => {
                captures.push(AmbientCapture::Phase(content));
            }
            _ => {} // Unknown tag — skip
        }

        remaining = &content_area[close_pos + close_tag.len()..];
    }

    captures
}

/// Extract an attribute value from a tag header string.
/// Simple parser for `key="value"` patterns.
fn extract_attr(attrs: &str, key: &str) -> Option<String> {
    let pattern = format!("{}=\"", key);
    let start = attrs.find(&pattern)?;
    let value_start = start + pattern.len();
    let value_end = attrs[value_start..].find('"')?;
    Some(attrs[value_start..value_start + value_end].to_string())
}

/// Replace fenced code blocks with empty strings to avoid false positives.
fn strip_fenced_code_blocks(text: &str) -> Cow<'_, str> {
    if !text.contains("```") {
        return Cow::Borrowed(text);
    }

    let mut result = String::with_capacity(text.len());
    let mut in_code_block = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            result.push('\n');
        } else if in_code_block {
            result.push('\n'); // preserve line count
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    Cow::Owned(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_decision() {
        let text = r#"I think we should use token rotation.

<omg:decision status="decided">
Use token rotation instead of direct replacement.
Direct replacement fails because WeakRef cache pointers become stale.
</omg:decision>

Let me look at the implementation."#;

        let captures = parse_ambient_blocks(text);
        assert_eq!(captures.len(), 1);
        match &captures[0] {
            AmbientCapture::Decision { status, content } => {
                assert_eq!(status, "decided");
                assert!(content.contains("token rotation"));
            }
            _ => panic!("expected Decision"),
        }
    }

    #[test]
    fn parse_multiple_tags() {
        let text = r#"
<omg:constraint>OAuth tokens expire in 30 minutes</omg:constraint>
<omg:question>How does the cache handle concurrent refreshes?</omg:question>
<omg:approach>Atomic rotation with cache invalidation</omg:approach>
"#;

        let captures = parse_ambient_blocks(text);
        assert_eq!(captures.len(), 3);
    }

    #[test]
    fn skip_code_blocks() {
        let text = r#"Here's an example:

```xml
<omg:decision status="decided">This should NOT be captured</omg:decision>
```

<omg:constraint>This SHOULD be captured</omg:constraint>
"#;

        let captures = parse_ambient_blocks(text);
        assert_eq!(captures.len(), 1);
        match &captures[0] {
            AmbientCapture::Constraint(text) => {
                assert!(text.contains("SHOULD be captured"));
            }
            _ => panic!("expected Constraint"),
        }
    }

    #[test]
    fn parse_failed_with_reason() {
        let text = r#"
<omg:failed reason="cache holds stale WeakRef pointers">
Direct token replacement without invalidation
</omg:failed>
"#;

        let captures = parse_ambient_blocks(text);
        assert_eq!(captures.len(), 1);
        match &captures[0] {
            AmbientCapture::Failed { description, reason } => {
                assert!(description.contains("Direct token"));
                assert!(reason.contains("WeakRef"));
            }
            _ => panic!("expected Failed"),
        }
    }
}

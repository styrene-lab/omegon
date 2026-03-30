//! Shared utilities.

use omegon_traits::ContentBlock;
use unicode_truncate::UnicodeTruncateStr;

/// Truncate a string to at most `max_width` display columns, appending "…" if truncated.
/// Uses unicode display width — CJK characters count as 2, combining marks as 0, etc.
pub fn truncate(s: &str, max_width: usize) -> String {
    let (truncated, _width) = s.unicode_truncate(max_width);
    if truncated.len() < s.len() {
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

/// Truncate a string to at most `max_width` display columns, returning a `&str`.
/// No suffix appended — caller can add "…" if needed.
pub fn truncate_str(s: &str, max_width: usize) -> &str {
    let (truncated, _width) = s.unicode_truncate(max_width);
    truncated
}

/// Truncate a single text string to at most `max_chars` characters.
/// Appends a summary line showing how many characters were dropped.
/// Used to cap feature tool output before injecting it into LLM context.
pub fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    let dropped = s.chars().count() - max_chars;
    format!("{truncated}\n[output truncated: {dropped} chars dropped — limit {max_chars}]")
}

/// Cap all Text blocks in a ToolResult content vec to `max_chars` each.
/// Non-text blocks (images, tool_use, etc.) are passed through unchanged.
pub fn truncate_content_blocks(
    blocks: &mut Vec<ContentBlock>,
    max_chars: usize,
) {
    for block in blocks.iter_mut() {
        if let ContentBlock::Text { text } = block {
            if text.chars().count() > max_chars {
                *text = truncate_output(text, max_chars);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_ascii() {
        assert_eq!(truncate("hello world", 5), "hello…");
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn truncate_multibyte() {
        // → is 1 display column but 3 bytes
        let s = "hello→world";
        assert_eq!(truncate(s, 6), "hello→…");
        assert_eq!(truncate(s, 5), "hello…");
        // Must not panic
        let _ = truncate(s, 0);
        let _ = truncate(s, 1);
    }

    #[test]
    fn truncate_emoji() {
        let s = "abc🎉def";
        // 🎉 is 2 display columns
        assert_eq!(truncate(s, 5), "abc🎉…");
        assert_eq!(truncate(s, 4), "abc…");
        assert_eq!(truncate(s, 3), "abc…");
    }

    #[test]
    fn truncate_str_returns_ref() {
        assert_eq!(truncate_str("hello→world", 6), "hello→");
        assert_eq!(truncate_str("hello→world", 5), "hello");
        assert_eq!(truncate_str("hi", 5), "hi");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 5), "");
        assert_eq!(truncate_str("", 5), "");
    }

    #[test]
    fn truncate_real_crash_case() {
        // The actual string that crashed: contains → at byte offset 195
        let s = "memory-lifecycle-integration design node decided and implementing \
                 at docs/memory-lifecycle-integration.md: D1 hybrid lifecycle-driven \
                 writes, D2 source precedence (OpenSpec baseline→Design Tree→Memory→session chatter)";
        // This must not panic regardless of truncation point
        for n in 0..s.len() {
            let _ = truncate(s, n);
            let _ = truncate_str(s, n);
        }
    }
}

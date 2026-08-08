//! Stable Markdown publication boundaries for progressively streamed assistant text.
//!
//! Transport chunks are intentionally absent from this module. Projection is a
//! pure function of canonical message text and completion state, so a chunking
//! change cannot alter the rendered document boundary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkdownPublication<'a> {
    pub(crate) committed: &'a str,
    pub(crate) provisional: &'a str,
}

pub(crate) fn project(text: &str, complete: bool) -> MarkdownPublication<'_> {
    if complete || text.is_empty() {
        return MarkdownPublication {
            committed: text,
            provisional: "",
        };
    }

    let mut in_fence = false;
    let mut fence_start = None;
    let mut stable_end = 0;
    let mut offset = 0;

    for line_with_ending in text.split_inclusive('\n') {
        let terminated = line_with_ending.ends_with('\n');
        let line = line_with_ending
            .strip_suffix('\n')
            .unwrap_or(line_with_ending);
        let trimmed = line.trim();
        let line_end = offset + line_with_ending.len();

        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
                fence_start = None;
                if terminated {
                    stable_end = line_end;
                }
            } else {
                in_fence = true;
                fence_start = Some(offset);
            }
        } else if terminated
            && !in_fence
            && (trimmed.is_empty() || is_completed_standalone_block(trimmed))
        {
            stable_end = line_end;
        }

        offset = line_end;
    }

    // An open fence and everything after its opener must remain provisional,
    // even if a prior scan observed boundary-looking lines inside the fence.
    if let Some(start) = fence_start {
        stable_end = stable_end.min(start);
    }

    MarkdownPublication {
        committed: &text[..stable_end],
        provisional: &text[stable_end..],
    }
}

fn is_completed_standalone_block(trimmed: &str) -> bool {
    is_atx_heading(trimmed) || is_horizontal_rule(trimmed)
}

fn is_atx_heading(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace)
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let mut marker = None;
    let mut count = 0;
    for ch in trimmed.chars().filter(|ch| !ch.is_whitespace()) {
        if !matches!(ch, '-' | '*' | '_') {
            return false;
        }
        match marker {
            Some(expected) if expected != ch => return false,
            None => marker = Some(ch),
            _ => {}
        }
        count += 1;
    }
    count >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclosed_fence_and_its_body_remain_provisional() {
        let text = "stable paragraph\n\n```rust\nfn main() {\n";
        let projection = project(text, false);
        assert_eq!(projection.committed, "stable paragraph\n\n");
        assert_eq!(projection.provisional, "```rust\nfn main() {\n");
    }

    #[test]
    fn closed_fence_is_a_stable_boundary() {
        let text = "```rust\nfn main() {}\n```\ntrailing";
        let projection = project(text, false);
        assert_eq!(projection.committed, "```rust\nfn main() {}\n```\n");
        assert_eq!(projection.provisional, "trailing");
    }

    #[test]
    fn incomplete_table_remains_one_provisional_tail() {
        let text = "intro\n\n| name | value |\n| --- | --- |\n| alpha";
        let projection = project(text, false);
        assert_eq!(projection.committed, "intro\n\n");
        assert_eq!(
            projection.provisional,
            "| name | value |\n| --- | --- |\n| alpha"
        );
    }

    #[test]
    fn unterminated_heading_line_remains_provisional() {
        let projection = project("stable\n\n## partial", false);
        assert_eq!(projection.committed, "stable\n\n");
        assert_eq!(projection.provisional, "## partial");
    }

    #[test]
    fn closed_fence_without_terminal_newline_remains_provisional() {
        let projection = project("intro\n\n```rust\nfn main() {}\n```", false);
        assert_eq!(projection.committed, "intro\n\n");
        assert_eq!(projection.provisional, "```rust\nfn main() {}\n```");
    }

    #[test]
    fn completed_heading_line_is_a_stable_boundary() {
        let projection = project("## stable heading\ntail", false);
        assert_eq!(projection.committed, "## stable heading\n");
        assert_eq!(projection.provisional, "tail");
    }

    #[test]
    fn completion_commits_only_the_remaining_tail() {
        let text = "intro\n\n| name | value |\n| --- | --- |\n| alpha | one |";
        let streaming = project(text, false);
        let completed = project(text, true);
        assert_eq!(streaming.committed, "intro\n\n");
        assert_eq!(completed.committed, text);
        assert_eq!(completed.provisional, "");
        assert_eq!(
            &completed.committed[streaming.committed.len()..],
            streaming.provisional
        );
    }
}

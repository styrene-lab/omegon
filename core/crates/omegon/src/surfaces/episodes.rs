//! Renderer-neutral operation episodes derived from canonical conversation evidence.
//!
//! This first reducer deliberately uses only authoritative boundaries supplied by
//! callers. If a caller cannot name a boundary, each tool becomes its own
//! episode rather than guessing that unrelated work belongs together.

use crate::surfaces::conversation::{ConversationSegmentKind, ConversationSegmentProjection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationEpisodeState {
    Running,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationEpisodeProjection {
    pub id: String,
    pub state: OperationEpisodeState,
    pub outcome: String,
    pub evidence_ids: Vec<String>,
    pub tool_count: usize,
}

impl OperationEpisodeProjection {
    pub fn from_authoritative_boundary<TText, TPath>(
        id: impl Into<String>,
        segments: &[ConversationSegmentProjection<TText, TPath>],
    ) -> Option<Self>
    where
        TText: AsRef<str>,
    {
        let tools = segments
            .iter()
            .filter_map(|segment| match &segment.kind {
                ConversationSegmentKind::Tool(tool) => Some(tool),
                _ => None,
            })
            .collect::<Vec<_>>();
        if tools.is_empty() {
            return None;
        }

        let failed = tools.iter().any(|tool| tool.is_error);
        let complete = tools.iter().all(|tool| tool.complete);
        let state = if failed {
            OperationEpisodeState::Failed
        } else if complete {
            OperationEpisodeState::Complete
        } else {
            OperationEpisodeState::Running
        };
        let evidence_ids = tools
            .iter()
            .map(|tool| tool.id.as_ref().to_string())
            .collect::<Vec<_>>();
        let outcome = deterministic_outcome(&tools, state);

        Some(Self {
            id: id.into(),
            state,
            outcome,
            tool_count: evidence_ids.len(),
            evidence_ids,
        })
    }

    pub fn single_tool_fallback<TText, TPath>(
        segment: &ConversationSegmentProjection<TText, TPath>,
    ) -> Option<Self>
    where
        TText: AsRef<str>,
    {
        let ConversationSegmentKind::Tool(tool) = &segment.kind else {
            return None;
        };
        Self::from_authoritative_boundary(
            format!("tool:{}", tool.id.as_ref()),
            std::slice::from_ref(segment),
        )
    }
}

fn deterministic_outcome<TText>(
    tools: &[&crate::surfaces::conversation::ToolSegment<TText>],
    state: OperationEpisodeState,
) -> String
where
    TText: AsRef<str>,
{
    if let Some(tool) = tools.iter().find(|tool| tool.is_error) {
        let result = tool
            .result_summary
            .as_ref()
            .map(AsRef::as_ref)
            .filter(|text| !text.trim().is_empty())
            .unwrap_or("failed");
        return format!("{} failed · {}", tool.name.as_ref(), bounded(result));
    }

    if state == OperationEpisodeState::Running {
        let tool = tools.last().expect("episode has at least one tool");
        return format!("Running {}", tool.name.as_ref());
    }

    if let Some(tool) = tools.iter().rev().find(|tool| {
        tool.result_summary
            .as_ref()
            .is_some_and(|result| !result.as_ref().trim().is_empty())
    }) {
        return format!(
            "{} · {}",
            tool.name.as_ref(),
            bounded(tool.result_summary.as_ref().expect("checked").as_ref())
        );
    }

    match tools {
        [tool] => format!("{} complete", tool.name.as_ref()),
        _ => format!("Completed {} operations", tools.len()),
    }
}

fn bounded(text: &str) -> String {
    const LIMIT: usize = 120;
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= LIMIT {
        compact
    } else {
        format!("{}…", compact.chars().take(LIMIT - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{ToolSegment, UserSegment};

    fn tool<'a>(id: &'a str, name: &'a str, result: Option<&'a str>, complete: bool, is_error: bool) -> ConversationSegmentProjection<&'a str> {
        ConversationSegmentProjection::new(ConversationSegmentKind::Tool(ToolSegment {
            id,
            name,
            args_summary: None,
            detail_args: None,
            result_summary: result,
            detail_result: result,
            is_error,
            complete,
            expanded: false,
        }))
    }

    #[test]
    fn authoritative_boundary_groups_evidence_deterministically() {
        let segments = vec![
            tool("read-1", "read", Some("86 lines"), true, false),
            tool("test-1", "bash", Some("47 tests passed"), true, false),
        ];
        let episode = OperationEpisodeProjection::from_authoritative_boundary("turn:7", &segments).expect("episode");
        assert_eq!(episode.id, "turn:7");
        assert_eq!(episode.state, OperationEpisodeState::Complete);
        assert_eq!(episode.tool_count, 2);
        assert_eq!(episode.evidence_ids, ["read-1", "test-1"]);
        assert_eq!(episode.outcome, "bash · 47 tests passed");
    }

    #[test]
    fn failure_has_precedence_over_successful_later_evidence() {
        let segments = vec![
            tool("test-1", "bash", Some("exit 1"), true, true),
            tool("read-1", "read", Some("diagnostics"), true, false),
        ];
        let episode = OperationEpisodeProjection::from_authoritative_boundary("turn:8", &segments).expect("episode");
        assert_eq!(episode.state, OperationEpisodeState::Failed);
        assert_eq!(episode.outcome, "bash failed · exit 1");
    }

    #[test]
    fn missing_boundary_falls_back_to_one_tool_only() {
        let segment = tool("read-1", "read", Some("12 lines"), true, false);
        let episode = OperationEpisodeProjection::single_tool_fallback(&segment).expect("episode");
        assert_eq!(episode.id, "tool:read-1");
        assert_eq!(episode.evidence_ids, ["read-1"]);
    }

    #[test]
    fn prose_does_not_become_an_episode() {
        let segment = ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::User(UserSegment { text: "hello" }));
        assert!(OperationEpisodeProjection::single_tool_fallback(&segment).is_none());
    }
}

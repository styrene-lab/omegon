//! Presentation-aware conversation projection.
//!
//! Om and Active collapse completed tool evidence under authoritative turn
//! metadata into a synthetic outcome segment. Full returns canonical segments
//! unchanged. The source transcript is never mutated.

use std::collections::BTreeMap;

use crate::surfaces::conversation::ProjectConversationSegment;
use crate::surfaces::episodes::{OperationEpisodeProjection, OperationEpisodeState};
use crate::surfaces::layout::UiPresentationLevel;

use super::segments::{Segment, SegmentContent, SegmentMeta};

pub fn project_conversation_segments(
    segments: &[Segment],
    level: UiPresentationLevel,
) -> Vec<Segment> {
    if level == UiPresentationLevel::Full {
        return segments.to_vec();
    }

    let mut complete_turn_episodes: BTreeMap<u32, OperationEpisodeProjection> = BTreeMap::new();
    let mut tools_by_turn: BTreeMap<u32, Vec<_>> = BTreeMap::new();
    for segment in segments {
        let Some(turn) = segment.meta.turn else {
            continue;
        };
        let projection = segment.project_conversation_segment();
        if matches!(
            projection.kind,
            crate::surfaces::conversation::ConversationSegmentKind::Tool(_)
        ) {
            tools_by_turn.entry(turn).or_default().push(projection);
        }
    }
    for (turn, tools) in &tools_by_turn {
        if let Some(episode) = OperationEpisodeProjection::from_authoritative_boundary(
            format!("turn:{turn}"),
            tools,
        ) && episode.state == OperationEpisodeState::Complete
        {
            complete_turn_episodes.insert(*turn, episode);
        }
    }

    let mut projected = Vec::with_capacity(segments.len());
    let mut emitted_turn = None;
    for segment in segments {
        let collapsible = segment
            .meta
            .turn
            .and_then(|turn| complete_turn_episodes.get(&turn).map(|episode| (turn, episode)))
            .filter(|_| matches!(segment.content, SegmentContent::ToolCard { .. }));
        if let Some((turn, episode)) = collapsible {
            if emitted_turn != Some(turn) {
                projected.push(outcome_segment(segment.meta.clone(), episode));
                emitted_turn = Some(turn);
            }
            continue;
        }
        projected.push(segment.clone());
    }
    projected
}

fn outcome_segment(mut meta: SegmentMeta, episode: &OperationEpisodeProjection) -> Segment {
    meta.duration_ms = None;
    Segment {
        meta,
        content: SegmentContent::SystemNotification {
            text: format!(
                "✓ {} · {} operation{}",
                episode.outcome,
                episode.tool_count,
                if episode.tool_count == 1 { "" } else { "s" }
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(turn: Option<u32>, id: &str, result: &str, complete: bool) -> Segment {
        Segment {
            meta: SegmentMeta {
                turn,
                ..Default::default()
            },
            content: SegmentContent::ToolCard {
                id: id.into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: None,
                result_summary: Some(result.into()),
                detail_result: Some(result.into()),
                is_error: false,
                complete,
                expanded: false,
                started_at: None,
                live_partial: None,
            },
        }
    }

    #[test]
    fn om_collapses_complete_turn_tools_without_mutating_source() {
        let source = vec![tool(Some(7), "a", "read complete", true), tool(Some(7), "b", "47 tests passed", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Om);
        assert_eq!(source.len(), 2);
        assert_eq!(projected.len(), 1);
        let SegmentContent::SystemNotification { text } = &projected[0].content else { panic!("outcome") };
        assert_eq!(text, "✓ bash · 47 tests passed · 2 operations");
    }

    #[test]
    fn active_uses_same_grouped_completed_history() {
        let source = vec![tool(Some(7), "a", "done", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Active);
        assert!(matches!(projected[0].content, SegmentContent::SystemNotification { .. }));
    }

    #[test]
    fn full_preserves_canonical_evidence_rows() {
        let source = vec![tool(Some(7), "a", "done", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Full);
        assert!(matches!(projected[0].content, SegmentContent::ToolCard { .. }));
    }

    #[test]
    fn running_or_unbound_tools_remain_visible() {
        let source = vec![tool(Some(7), "a", "running", false), tool(None, "b", "done", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Om);
        assert_eq!(projected.len(), 2);
        assert!(projected.iter().all(|segment| matches!(segment.content, SegmentContent::ToolCard { .. })));
    }
}

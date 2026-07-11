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

#[derive(Debug, Clone)]
pub struct ConversationProjection {
    pub segments: Vec<Segment>,
    /// Maps each projected row to its canonical source row. Synthetic outcome
    /// rows point at the most relevant evidence item in their episode.
    pub canonical_indices: Vec<usize>,
}

impl ConversationProjection {
    pub fn projected_index_for_canonical(&self, canonical_index: usize) -> Option<usize> {
        self.canonical_indices
            .iter()
            .position(|index| *index == canonical_index)
    }

    fn push_canonical(&mut self, index: usize, segment: &Segment) {
        self.segments.push(segment.clone());
        self.canonical_indices.push(index);
    }

    fn push_synthetic(&mut self, canonical_index: usize, segment: Segment) {
        self.segments.push(segment);
        self.canonical_indices.push(canonical_index);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationExportPolicy {
    /// Operator/assistant prose and durable outcomes, independent of UI level.
    Semantic,
    /// Canonical evidence-inclusive export for diagnostics and audit.
    Evidence,
}

pub fn project_conversation_for_export(
    segments: &[Segment],
    policy: ConversationExportPolicy,
) -> ConversationProjection {
    match policy {
        ConversationExportPolicy::Semantic => {
            project_conversation(segments, UiPresentationLevel::Om)
        }
        ConversationExportPolicy::Evidence => {
            project_conversation(segments, UiPresentationLevel::Full)
        }
    }
}

pub fn project_conversation(
    segments: &[Segment],
    level: UiPresentationLevel,
) -> ConversationProjection {
    if level == UiPresentationLevel::Full {
        return ConversationProjection {
            segments: segments.to_vec(),
            canonical_indices: (0..segments.len()).collect(),
        };
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
        ) && matches!(
            episode.state,
            OperationEpisodeState::Complete | OperationEpisodeState::Failed
        )
        {
            complete_turn_episodes.insert(*turn, episode);
        }
    }

    let mut operation_lifecycle: BTreeMap<String, Vec<&Segment>> = BTreeMap::new();
    for segment in segments {
        if let Some(operation_id) = segment
            .meta
            .source_channel
            .as_deref()
            .and_then(|source| source.strip_prefix("operation:"))
        {
            operation_lifecycle
                .entry(operation_id.to_string())
                .or_default()
                .push(segment);
        }
    }

    let mut projected = ConversationProjection {
        segments: Vec::with_capacity(segments.len()),
        canonical_indices: Vec::with_capacity(segments.len()),
    };
    let mut emitted_operations = std::collections::BTreeSet::new();
    let mut emitted_turn = None;
    for (canonical_index, segment) in segments.iter().enumerate() {
        if let Some(operation_id) = segment
            .meta
            .source_channel
            .as_deref()
            .and_then(|source| source.strip_prefix("operation:"))
        {
            let operation_segments = &operation_lifecycle[operation_id];
            let terminal = operation_segments.iter().rev().find(|candidate| {
                matches!(
                    &candidate.content,
                    SegmentContent::LifecycleEvent { text, .. }
                        if is_terminal_operation_text(text)
                )
            });
            if let Some(terminal) = terminal {
                if emitted_operations.insert(operation_id.to_string()) {
                    let terminal_index = segments
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, *terminal))
                        .unwrap_or(canonical_index);
                    projected.push_synthetic(terminal_index, operation_outcome_segment(
                        terminal.meta.clone(),
                        operation_id,
                        operation_segments,
                    ));
                }
                continue;
            }
        }
        if segment.meta.turn.is_none()
            && let SegmentContent::ToolCard { name, complete: true, .. } = &segment.content
            && name == "operator_shell"
        {
            let semantic = segment.project_conversation_segment();
            if let Some(episode) = OperationEpisodeProjection::single_tool_fallback(&semantic) {
                projected.push_synthetic(canonical_index, outcome_segment(segment.meta.clone(), &episode));
                continue;
            }
        }
        let collapsible = segment
            .meta
            .turn
            .and_then(|turn| complete_turn_episodes.get(&turn).map(|episode| (turn, episode)))
            .filter(|_| matches!(segment.content, SegmentContent::ToolCard { .. }));
        if let Some((turn, episode)) = collapsible {
            if emitted_turn != Some(turn) {
                projected.push_synthetic(canonical_index, outcome_segment(segment.meta.clone(), episode));
                emitted_turn = Some(turn);
            }
            continue;
        }
        projected.push_canonical(canonical_index, segment);
    }
    projected
}

pub fn project_conversation_segments(segments: &[Segment], level: UiPresentationLevel) -> Vec<Segment> {
    project_conversation(segments, level).segments
}

fn operation_episode_id_from_source(source: Option<&str>) -> Option<&str> {
    source?.strip_prefix("operation:")
}

fn is_terminal_operation_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("merged")
        || lower.contains("completed (no merge)")
        || lower.contains("failed")
        || lower.contains("cancelled")
}

fn operation_failed(evidence: &[&Segment]) -> bool {
    evidence.iter().any(|segment| {
        matches!(
            &segment.content,
            SegmentContent::LifecycleEvent { icon, text }
                if icon == "✗"
                    || text.to_ascii_lowercase().contains("failed")
                    || text.to_ascii_lowercase().contains("cancelled")
        )
    })
}

fn operation_outcome_segment(
    mut meta: SegmentMeta,
    operation_id: &str,
    evidence: &[&Segment],
) -> Segment {
    meta.duration_ms = None;
    let terminal_text = evidence
        .iter()
        .rev()
        .find_map(|segment| match &segment.content {
            SegmentContent::LifecycleEvent { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or("completed");
    let label = operation_id
        .split_once(':')
        .map(|(kind, id)| format!("{kind} {id}"))
        .unwrap_or_else(|| operation_id.to_string());
    let state = if operation_failed(evidence) { "✗" } else { "✓" };
    Segment {
        meta,
        content: SegmentContent::SystemNotification {
            text: format!(
                "{state} {label} · {terminal_text} · {} events",
                evidence.len()
            ),
        },
    }
}

fn outcome_segment(mut meta: SegmentMeta, episode: &OperationEpisodeProjection) -> Segment {
    meta.duration_ms = None;
    Segment {
        meta,
        content: SegmentContent::SystemNotification {
            text: format!(
                "{} {} · {} operation{}",
                if episode.state == OperationEpisodeState::Failed {
                    "✗"
                } else {
                    "✓"
                },
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
    fn failed_turn_tools_collapse_to_one_failed_outcome() {
        let mut failed = tool(Some(7), "a", "exit 1", true);
        if let SegmentContent::ToolCard { is_error, .. } = &mut failed.content {
            *is_error = true;
        }
        let source = vec![failed, tool(Some(7), "b", "diagnostics", true)];
        let projected = project_conversation(&source, UiPresentationLevel::Om);
        assert_eq!(projected.segments.len(), 1);
        let SegmentContent::SystemNotification { text } = &projected.segments[0].content else {
            panic!("failed outcome")
        };
        assert!(text.starts_with("✗ "), "{text}");
        assert!(text.contains("bash failed · exit 1"), "{text}");
    }

    #[test]
    fn active_uses_same_grouped_completed_history() {
        let source = vec![tool(Some(7), "a", "done", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Active);
        assert!(matches!(projected[0].content, SegmentContent::SystemNotification { .. }));
    }

    #[test]
    fn completed_operation_lifecycle_collapses_to_one_outcome() {
        let operation = omegon_traits::OperationRef::delegate("delegate-7");
        let mut conversation = crate::tui::conversation::ConversationView::new();
        conversation.push_operation_lifecycle(&operation, "⇉", "Delegate: review started");
        conversation.push_operation_lifecycle(&operation, "✓", "Delegate: review completed");
        conversation.push_operation_lifecycle(
            &operation,
            "↯",
            "Delegate completed (no merge)",
        );

        let projected =
            project_conversation_segments(conversation.segments(), UiPresentationLevel::Om);
        assert_eq!(projected.len(), 1);
        let SegmentContent::SystemNotification { text } = &projected[0].content else {
            panic!("operation outcome")
        };
        assert!(text.contains("delegate delegate-7"), "{text}");
        assert!(text.contains("3 events"), "{text}");

        let full =
            project_conversation_segments(conversation.segments(), UiPresentationLevel::Full);
        assert_eq!(full.len(), 3);
        assert!(full
            .iter()
            .all(|segment| matches!(segment.content, SegmentContent::LifecycleEvent { .. })));
    }

    #[test]
    fn semantic_export_is_independent_of_display_level() {
        let source = vec![
            tool(Some(7), "a", "read complete", true),
            tool(Some(7), "b", "47 tests passed", true),
        ];
        let semantic =
            project_conversation_for_export(&source, ConversationExportPolicy::Semantic);
        let om = project_conversation(&source, UiPresentationLevel::Om);
        let full = project_conversation(&source, UiPresentationLevel::Full);
        assert_eq!(semantic.segments.len(), om.segments.len());
        assert_eq!(semantic.canonical_indices, om.canonical_indices);
        assert_ne!(semantic.segments.len(), full.segments.len());

        let evidence =
            project_conversation_for_export(&source, ConversationExportPolicy::Evidence);
        assert_eq!(evidence.canonical_indices, full.canonical_indices);
        assert_eq!(evidence.segments.len(), full.segments.len());
    }

    #[test]
    fn failed_operation_collapses_to_failed_outcome() {
        let operation = omegon_traits::OperationRef::cleave(Some("run-9".into()));
        let mut conversation = crate::tui::conversation::ConversationView::new();
        conversation.push_operation_lifecycle(&operation, "↯", "Cleave: 2 children dispatched");
        conversation.push_operation_lifecycle(&operation, "✗", "Child 'tests' failed");

        let projected =
            project_conversation_segments(conversation.segments(), UiPresentationLevel::Om);
        assert_eq!(projected.len(), 1);
        let SegmentContent::SystemNotification { text } = &projected[0].content else {
            panic!("failed operation outcome")
        };
        assert!(text.starts_with("✗ cleave run-9"), "{text}");
        assert!(text.contains("failed"), "{text}");
    }

    #[test]
    fn full_preserves_canonical_evidence_rows() {
        let source = vec![tool(Some(7), "a", "done", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Full);
        assert!(matches!(projected[0].content, SegmentContent::ToolCard { .. }));
    }

    #[test]
    fn operator_shell_without_turn_uses_authoritative_single_observation_episode() {
        let mut source = tool(None, "shell-7", "exit 0 · 12ms", true);
        if let SegmentContent::ToolCard { name, .. } = &mut source.content {
            *name = "operator_shell".into();
        }
        let projected = project_conversation_segments(&[source], UiPresentationLevel::Om);
        assert_eq!(projected.len(), 1);
        let SegmentContent::SystemNotification { text } = &projected[0].content else {
            panic!("outcome")
        };
        assert_eq!(text, "✓ operator_shell · exit 0 · 12ms · 1 operation");
    }

    #[test]
    fn running_or_unbound_tools_remain_visible() {
        let source = vec![tool(Some(7), "a", "running", false), tool(None, "b", "done", true)];
        let projected = project_conversation_segments(&source, UiPresentationLevel::Om);
        assert_eq!(projected.len(), 2);
        assert!(projected.iter().all(|segment| matches!(segment.content, SegmentContent::ToolCard { .. })));
    }
}

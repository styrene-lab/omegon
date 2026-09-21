//! Best-effort journal and diagnostic-audit input derived from validated replay.

use crate::{
    session_authority::{AssistantContentKind, SessionFactPayload},
    session_blob_store::ProjectionClass,
    session_consumers::{DeferredSessionViewBinding, SessionViewTarget},
    session_replay::{ReplayEnd, SessionReplay},
};

pub(crate) fn load(
    binding: &DeferredSessionViewBinding,
) -> Result<(SessionViewTarget, SessionReplay), String> {
    let target = binding
        .snapshot()
        .ok_or_else(|| "sessionless host has no semantic replay binding".to_string())?;
    let replay = match target.stream_id {
        Some(stream_id) => SessionReplay::replay_prefix(
            &target.snapshot,
            &target.session_id,
            stream_id,
            ReplayEnd::EndOfStream,
        ),
        None => SessionReplay::replay_session(
            &target.snapshot,
            &target.session_id,
            ReplayEnd::EndOfStream,
        ),
    }
    .map_err(|error| error.to_string())?;
    Ok((target, replay))
}

pub(crate) fn generation_is_current(
    binding: &DeferredSessionViewBinding,
    captured: &SessionViewTarget,
) -> bool {
    binding.snapshot().is_some_and(|current| {
        current.generation == captured.generation
            && current.binding_id == captured.binding_id
            && current.snapshot == captured.snapshot
            && current.session_id == captured.session_id
            && current.stream_id == captured.stream_id
    })
}

pub(crate) fn default_assistant_outcome(
    replay: &SessionReplay,
    minimum_sequence: u64,
    maximum: usize,
) -> Option<String> {
    replay
        .records()
        .iter()
        .rev()
        .filter(|record| record.frontier().sequence() >= minimum_sequence)
        .find_map(|record| {
            let SessionFactPayload::AssistantMessageCommitted(commit) = record.payload() else {
                return None;
            };
            let mut outcome = String::new();
            for manifest in &commit.content {
                if manifest.content_kind != AssistantContentKind::Text {
                    continue;
                }
                for content_ref in &manifest.chunk_refs {
                    if content_ref.projection_class() != ProjectionClass::Default {
                        continue;
                    }
                    let bytes = replay.read_default_content(content_ref).ok()?;
                    outcome.push_str(std::str::from_utf8(&bytes).ok()?);
                    if outcome.chars().count() >= maximum {
                        break;
                    }
                }
            }
            let outcome = outcome.split_whitespace().collect::<Vec<_>>().join(" ");
            (!outcome.is_empty()).then(|| outcome.chars().take(maximum).collect())
        })
}

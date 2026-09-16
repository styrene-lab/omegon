//! Bounded startup recovery from durable evidence; no session replay is required.
use super::*;

pub(super) async fn recover(
    binding: crate::memory_service::MemoryBinding,
    mind: String,
    extractor: Arc<dyn formation::Extractor>,
) -> anyhow::Result<()> {
    use crate::memory_service::{MemoryPayloadV1, MemoryRequestV1, MemoryScopeV1};
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let response = binding
        .invoke(MemoryRequestV1::PendingFormations {
            scope: MemoryScopeV1::Project,
            mind,
            model: extractor.model().into(),
            limit: omegon_memory::formation::MAX_RECOVERY_BATCH,
            cancellation: cancellation.clone(),
        })
        .await
        .map_err(MemoryFeatureInvokeError)?;
    let MemoryPayloadV1::Episodes(episodes) = response.payload else {
        anyhow::bail!("unexpected formation inventory response");
    };
    for episode in episodes {
        let evidence = episode
            .formation
            .ok_or_else(|| anyhow::anyhow!("missing pending formation"))?;
        evidence.validate()?;
        let completed = formation::extract_candidates(*evidence, Some(&extractor)).await;
        // Completion validates immutable source evidence and pending state atomically.
        // A concurrent normal completion wins safely; recovery never creates facts.
        binding
            .invoke(MemoryRequestV1::ApplyMutation {
                scope: MemoryScopeV1::Project,
                operation_id: format!("formation-recovery:v1:{}", episode.id),
                mutation: MemoryMutation::CompleteFormation {
                    episode_id: episode.id,
                    formation: Box::new(completed),
                },
                cancellation: cancellation.clone(),
            })
            .await
            .map_err(MemoryFeatureInvokeError)?;
    }
    Ok(())
}

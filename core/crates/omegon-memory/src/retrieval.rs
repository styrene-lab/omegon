//! Identified vector contracts and bounded top-k accumulation.
use crate::{MemoryError, types::*};
use sha2::{Digest, Sha256};
use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
};

pub fn raw_content_hash(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

pub(crate) fn stored_space(
    encoded: Option<String>,
) -> crate::backend::Result<Option<EmbeddingSpace>> {
    encoded
        .map(|value| {
            let space: EmbeddingSpace =
                serde_json::from_str(&value).map_err(|error| MemoryError::Storage(error.into()))?;
            space.validate().map_err(|_| {
                MemoryError::Storage(anyhow::anyhow!("invalid stored embedding space"))
            })?;
            Ok(space)
        })
        .transpose()
}

impl EmbeddingSpace {
    pub fn validate(&self) -> crate::backend::Result<()> {
        let valid =
            |s: &str| !s.trim().is_empty() && s.len() <= 512 && !s.chars().any(char::is_control);
        if !valid(&self.model)
            || !valid(&self.revision)
            || !valid(&self.preprocessing)
            || self.dimensions == 0
            || self.dimensions > 16_384
        {
            return Err(MemoryError::InvalidMutation(
                "invalid embedding-space identity".into(),
            ));
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("space serialization"),
        ))
    }
}

impl IdentifiedEmbedding {
    pub fn validate(&self) -> crate::backend::Result<()> {
        self.space.validate()?;
        crate::backend::validate_embedding(&self.values)?;
        if self.values.len() != self.space.dimensions as usize
            || !self.values.iter().any(|v| *v != 0.0)
        {
            return Err(MemoryError::InvalidMutation(
                "embedding dimensions or norm are invalid".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn index_state(
    stored: Option<&EmbeddingSpace>,
    source_hash: Option<&str>,
    content: &str,
    query: &EmbeddingSpace,
) -> EmbeddingIndexState {
    let (Some(stored), Some(source_hash)) = (stored, source_hash) else {
        return EmbeddingIndexState::Legacy;
    };
    if stored != query {
        return EmbeddingIndexState::Incompatible;
    }
    if source_hash != raw_content_hash(content) {
        return EmbeddingIndexState::Stale;
    }
    EmbeddingIndexState::Ready
}

pub(crate) fn decode(blob: &[u8], space: &EmbeddingSpace) -> crate::backend::Result<Vec<f32>> {
    if blob.len() != space.dimensions as usize * 4 {
        return Err(MemoryError::Storage(anyhow::anyhow!(
            "corrupt identified vector length"
        )));
    }
    let values = crate::vectors::blob_to_vector(blob);
    IdentifiedEmbedding {
        space: space.clone(),
        values: values.clone(),
    }
    .validate()
    .map_err(|_| MemoryError::Storage(anyhow::anyhow!("corrupt identified vector values")))?;
    Ok(values)
}

struct Ranked(ScoredFact);
impl PartialEq for Ranked {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Ranked {}
impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .score
            .total_cmp(&other.0.score)
            .then_with(|| other.0.fact.id.cmp(&self.0.fact.id))
    }
}

pub(crate) struct VectorAccumulator {
    pub diagnostics: VectorDiagnostics,
    heap: BinaryHeap<Reverse<Ranked>>,
    limit: usize,
}
impl VectorAccumulator {
    pub fn new(
        query: &IdentifiedEmbedding,
        limit: usize,
        minimum: f32,
    ) -> crate::backend::Result<Self> {
        query.validate()?;
        if limit > 10_000 || !minimum.is_finite() || !(-1.0..=1.0).contains(&minimum) {
            return Err(MemoryError::InvalidMutation(
                "invalid vector query bounds".into(),
            ));
        }
        Ok(Self {
            diagnostics: Default::default(),
            heap: BinaryHeap::new(),
            limit,
        })
    }
    pub fn eligible(&mut self, state: EmbeddingIndexState) -> bool {
        match state {
            EmbeddingIndexState::Legacy => self.diagnostics.legacy += 1,
            EmbeddingIndexState::Incompatible => self.diagnostics.incompatible += 1,
            EmbeddingIndexState::Stale => self.diagnostics.stale += 1,
            EmbeddingIndexState::Ready => {
                self.diagnostics.compatible += 1;
                return true;
            }
            EmbeddingIndexState::Missing => {}
        }
        false
    }
    pub fn push(&mut self, fact: Fact, similarity: f64, filter: &SearchFilter) {
        let Some(score) = filter.score(similarity, &fact) else {
            return;
        };
        let mut result = ScoredFact::new(fact, similarity, score);
        result.applicability = filter.applicability_status(&result.fact);
        result.scores.cosine = Some(similarity);
        let ranked = Ranked(result);
        if self.heap.len() < self.limit {
            self.heap.push(Reverse(ranked));
        } else if self.heap.peek().is_some_and(|worst| ranked > worst.0) {
            self.heap.pop();
            self.heap.push(Reverse(ranked));
        }
    }
    pub fn finish(self) -> VectorSearchReport {
        let mut results = self
            .heap
            .into_iter()
            .map(|entry| entry.0.0)
            .collect::<Vec<_>>();
        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.fact.id.cmp(&b.fact.id))
        });
        VectorSearchReport {
            results,
            diagnostics: self.diagnostics,
        }
    }
}

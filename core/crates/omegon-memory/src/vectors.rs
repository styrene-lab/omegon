//! Vector math for embedding similarity search.
//!
//! Direct port of extensions/project-memory/core.ts cosine similarity + BLOB serde.
//! LLVM auto-vectorizes the inner loop (SSE/AVX on x86, NEON on ARM).

use std::collections::HashMap;

use crate::types::ScoredFact;

/// Cosine similarity between two f32 slices.
/// Returns 0.0 if dimensions differ or either vector has zero norm.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot: f32 = 0.0;
    let mut norm_a: f32 = 0.0;
    let mut norm_b: f32 = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// Serialize f32 slice to bytes for SQLite BLOB storage.
/// Layout: raw little-endian IEEE 754 f32 array.
pub fn vector_to_blob(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Deserialize bytes from SQLite BLOB to Vec<f32>.
/// Panics if blob length is not a multiple of 4.
pub fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    assert!(
        blob.len().is_multiple_of(4),
        "BLOB length {} is not a multiple of 4",
        blob.len()
    );
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Reciprocal Rank Fusion merge of two ranked result lists.
///
/// For each fact, computes: `score = Σ 1/(rrf_k + rank_i)` where `rank_i` is
/// the 1-based position in each source list. Facts appearing in both lists
/// receive contributions from both, producing a natural boost.
///
/// `rrf_k` is the smoothing constant (typically 60). Returns merged results
/// sorted by RRF score descending, truncated to `limit`.
pub fn rrf_merge(
    fts_results: &[ScoredFact],
    vec_results: &[ScoredFact],
    rrf_k: f64,
    limit: usize,
) -> Vec<ScoredFact> {
    let mut scores: HashMap<String, (f64, ScoredFact)> = HashMap::new();

    for (rank, sf) in fts_results.iter().enumerate() {
        let contribution = 1.0 / (rrf_k + (rank + 1) as f64);
        scores
            .entry(sf.fact.id.clone())
            .and_modify(|(s, _)| *s += contribution)
            .or_insert_with(|| (contribution, sf.clone()));
    }

    for (rank, sf) in vec_results.iter().enumerate() {
        let contribution = 1.0 / (rrf_k + (rank + 1) as f64);
        scores
            .entry(sf.fact.id.clone())
            .and_modify(|(s, _)| *s += contribution)
            .or_insert_with(|| (contribution, sf.clone()));
    }

    let mut merged: Vec<ScoredFact> = scores
        .into_values()
        .map(|(rrf_score, mut sf)| {
            sf.score = rrf_score;
            sf
        })
        .collect();

    merged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(limit);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_similarity_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6, "got {sim}");
    }

    #[test]
    fn orthogonal_vectors_similarity_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6, "got {sim}");
    }

    #[test]
    fn opposite_vectors_similarity_is_negative_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6, "got {sim}");
    }

    #[test]
    fn different_lengths_returns_zero() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn zero_vector_returns_zero() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn blob_round_trip() {
        let original = vec![1.0f32, -2.5, 3.14159, 0.0, f32::MIN, f32::MAX];
        let blob = vector_to_blob(&original);
        let restored = blob_to_vector(&blob);
        assert_eq!(original, restored);
    }

    #[test]
    fn blob_empty() {
        let original: Vec<f32> = vec![];
        let blob = vector_to_blob(&original);
        assert!(blob.is_empty());
        let restored = blob_to_vector(&blob);
        assert!(restored.is_empty());
    }

    #[test]
    #[should_panic(expected = "not a multiple of 4")]
    fn blob_bad_length_panics() {
        blob_to_vector(&[1, 2, 3]); // 3 bytes, not a multiple of 4
    }

    // ── RRF merge tests ─────────────────────────────────────────────────────

    use crate::types::{DecayProfileName, Fact, FactStatus, Section};

    fn stub_fact(id: &str) -> Fact {
        Fact {
            id: id.into(),
            mind: "test".into(),
            content: format!("fact {id}"),
            section: Section::Architecture,
            status: FactStatus::Active,
            confidence: 1.0,
            reinforcement_count: 1,
            decay_rate: 0.05,
            decay_profile: DecayProfileName::Standard,
            last_reinforced: "2026-01-01T00:00:00Z".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            version: 1,
            superseded_by: None,
            source: None,
            content_hash: None,
            last_accessed: None,
            created_session: None,
            superseded_at: None,
            archived_at: None,
            jj_change_id: None,
            persona_id: None,
            layer: "project".into(),
            tags: vec![],
        }
    }

    fn scored(id: &str, similarity: f64) -> ScoredFact {
        ScoredFact {
            fact: stub_fact(id),
            similarity,
            score: similarity,
        }
    }

    #[test]
    fn rrf_merge_empty_inputs() {
        let result = rrf_merge(&[], &[], 60.0, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn rrf_merge_single_source() {
        let fts = vec![scored("a", 0.9), scored("b", 0.7)];
        let result = rrf_merge(&fts, &[], 60.0, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].fact.id, "a");
        assert_eq!(result[1].fact.id, "b");
        // rank-1 score = 1/(60+1) ≈ 0.01639
        assert!((result[0].score - 1.0 / 61.0).abs() < 1e-9);
    }

    #[test]
    fn rrf_merge_overlap_boosts() {
        // "b" appears in both lists, "a" only in FTS, "c" only in vec
        let fts = vec![scored("a", 0.9), scored("b", 0.7)];
        let vec = vec![scored("b", 0.8), scored("c", 0.6)];
        let result = rrf_merge(&fts, &vec, 60.0, 10);

        // "b" should be ranked first (it appears in both)
        assert_eq!(result[0].fact.id, "b");

        // "b" score = 1/(60+2) + 1/(60+1) = 1/62 + 1/61
        let expected_b = 1.0 / 62.0 + 1.0 / 61.0;
        assert!((result[0].score - expected_b).abs() < 1e-9);

        // "a" score = 1/(60+1), single contribution
        let a = result.iter().find(|sf| sf.fact.id == "a").unwrap();
        assert!((a.score - 1.0 / 61.0).abs() < 1e-9);
    }

    #[test]
    fn rrf_merge_respects_limit() {
        let fts = vec![scored("a", 0.9), scored("b", 0.8), scored("c", 0.7)];
        let result = rrf_merge(&fts, &[], 60.0, 2);
        assert_eq!(result.len(), 2);
    }
}

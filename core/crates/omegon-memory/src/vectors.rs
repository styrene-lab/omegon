//! Vector math for embedding similarity search.
//!
//! Direct port of extensions/project-memory/core.ts cosine similarity + BLOB serde.
//! LLVM auto-vectorizes the inner loop (SSE/AVX on x86, NEON on ARM).

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
    assert!(blob.len().is_multiple_of(4), "BLOB length {} is not a multiple of 4", blob.len());
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
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
}

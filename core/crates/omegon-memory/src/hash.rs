//! Content hashing for deduplication.
//!
//! Direct port of extensions/project-memory/core.ts::contentHash + normalizeForHash.

use sha2::{Digest, Sha256};

/// Normalize content for dedup hashing.
/// Strips leading bullet dash, trims whitespace, lowercases, collapses runs of spaces.
pub fn normalize_for_hash(content: &str) -> String {
    let s = content.strip_prefix("- ").unwrap_or(content);
    let trimmed = s.trim().to_lowercase();
    // Collapse whitespace runs to single space
    let mut result = String::with_capacity(trimmed.len());
    let mut prev_space = false;
    for c in trimmed.chars() {
        if c.is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result
}

/// Compute a 16-hex-char content hash for deduplication.
/// Uses sha256 truncated to 64 bits.
pub fn content_hash(content: &str) -> String {
    let normalized = normalize_for_hash(content);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8]) // 8 bytes = 16 hex chars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_bullet() {
        assert_eq!(normalize_for_hash("- Some fact here"), "some fact here");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_for_hash("hello   world  foo"), "hello world foo");
    }

    #[test]
    fn normalize_trims_and_lowercases() {
        assert_eq!(normalize_for_hash("  Hello World  "), "hello world");
    }

    #[test]
    fn content_hash_is_16_hex_chars() {
        let h = content_hash("some fact content");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn content_hash_is_deterministic() {
        let h1 = content_hash("test content");
        let h2 = content_hash("test content");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_normalizes_before_hashing() {
        let h1 = content_hash("- Hello  World");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2, "normalized forms should produce same hash");
    }

    #[test]
    fn content_hash_differs_for_different_content() {
        let h1 = content_hash("fact one");
        let h2 = content_hash("fact two");
        assert_ne!(h1, h2);
    }
}

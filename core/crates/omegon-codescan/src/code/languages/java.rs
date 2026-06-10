use std::path::Path;

use crate::code::{CodeChunk, PatternPair, scan_with_regex};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_regex(path, content, "java", PATTERNS)
}

const PATTERNS: &[PatternPair] = &[
    (
        r"^\s*(?:(?:public|protected|private|abstract|final|static|sealed|non-sealed|strictfp)\s+)*class\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "class",
    ),
    (
        r"^\s*(?:(?:public|protected|private|abstract|static|sealed|non-sealed|strictfp)\s+)*interface\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "interface",
    ),
    (
        r"^\s*(?:(?:public|protected|private|static)\s+)*enum\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "enum",
    ),
    (
        r"^\s*(?:(?:public|protected|private|final|static)\s+)*record\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "record",
    ),
    (
        r"^\s*(?:(?:public|protected|private)\s+)*@interface\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "annotation",
    ),
    (
        r"^\s*(?:(?:public|protected|private|static|final|synchronized|native|abstract|default|strictfp)\s+)+[A-Z_a-z][A-Z_a-z0-9_<>, ?\[\]]*\s+([a-zA-Z_][A-Z_a-z0-9_]*)\s*\(",
        "method",
    ),
];

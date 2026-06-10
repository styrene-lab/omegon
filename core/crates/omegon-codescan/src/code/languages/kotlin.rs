use std::path::Path;

use crate::code::{CodeChunk, PatternPair, scan_with_regex};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_regex(path, content, "kotlin", PATTERNS)
}

const PATTERNS: &[PatternPair] = &[
    (
        r"^\s*(?:(?:public|private|protected|internal|open|abstract|sealed|data|value|inner)\s+)*class\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "class",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|sealed)\s+)*interface\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "interface",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|data)\s+)*object\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "object",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal)\s+)*(?:suspend\s+)?fun\s+(?:<[^>]+>\s*)?([A-Z_a-z][A-Z_a-z0-9]*)\s*\(",
        "fun",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|const|lateinit)\s+)*(?:val|var)\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "property",
    ),
];

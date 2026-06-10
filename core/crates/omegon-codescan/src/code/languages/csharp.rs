use std::path::Path;

use crate::code::{CodeChunk, PatternPair, scan_with_regex};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_regex(path, content, "csharp", PATTERNS)
}

const PATTERNS: &[PatternPair] = &[
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*namespace\s+([A-Z_a-z][A-Z_a-z0-9_.]*)\b",
        "namespace",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*class\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "class",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*interface\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "interface",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*record\s+(?:class\s+|struct\s+)?([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "record",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*struct\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "struct",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*enum\s+([A-Z_a-z][A-Z_a-z0-9]*)\b",
        "enum",
    ),
    (
        r"^\s*(?:(?:public|private|protected|internal|static|sealed|abstract|partial|async|virtual|override|readonly|extern|unsafe|new|required)\s+)*(?:[A-Z_a-z][A-Z_a-z0-9_<>,?\[\].]*\s+)+([A-Z_a-z][A-Z_a-z0-9]*)\s*\(",
        "method",
    ),
];

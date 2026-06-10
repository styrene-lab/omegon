use std::path::Path;

use tree_sitter::Language;

use crate::code::{CodeChunk, PatternPair, TreeSitterSpec, generic_name, scan_with_tree_sitter};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_tree_sitter(
        path,
        content,
        TreeSitterSpec {
            language_name: "python",
            language: py_lang,
            top_kinds: KINDS,
            kind_label,
            name_extractor: generic_name,
            regex_fallback: PATTERNS,
        },
    )
}

fn py_lang() -> Language {
    tree_sitter_python::LANGUAGE.into()
}

const KINDS: &[&str] = &[
    "function_definition",
    "class_definition",
    "decorated_definition",
];

fn kind_label(k: &str) -> &'static str {
    match k {
        "function_definition" => "def",
        "class_definition" => "class",
        "decorated_definition" => "decorated",
        _ => "decl",
    }
}

const PATTERNS: &[PatternPair] = &[
    (r"^(?:async\s+)?def\s+([a-zA-Z_][a-zA-Z0-9_]*)\b", "def"),
    (r"^class\s+([a-zA-Z_][a-zA-Z0-9_]*)\b", "class"),
];

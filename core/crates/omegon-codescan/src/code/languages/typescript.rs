use std::path::Path;

use tree_sitter::Language;

use crate::code::{CodeChunk, PatternPair, TreeSitterSpec, generic_name, scan_with_tree_sitter};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_tree_sitter(
        path,
        content,
        TreeSitterSpec {
            language_name: "typescript",
            language: ts_lang,
            top_kinds: KINDS,
            kind_label,
            name_extractor: generic_name,
            regex_fallback: PATTERNS,
        },
    )
}

pub(crate) fn scan_tsx(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_tree_sitter(
        path,
        content,
        TreeSitterSpec {
            language_name: "typescript",
            language: tsx_lang,
            top_kinds: KINDS,
            kind_label,
            name_extractor: generic_name,
            regex_fallback: PATTERNS,
        },
    )
}

fn ts_lang() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

fn tsx_lang() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

const KINDS: &[&str] = &[
    "function_declaration",
    "function",
    "class_declaration",
    "interface_declaration",
    "type_alias_declaration",
    "abstract_class_declaration",
    "export_statement",
    "lexical_declaration",
    "variable_declaration",
];

fn kind_label(k: &str) -> &'static str {
    match k {
        "function_declaration" | "function" => "function",
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "export_statement" => "export",
        "lexical_declaration" | "variable_declaration" => "const",
        _ => "decl",
    }
}

pub(super) const PATTERNS: &[PatternPair] = &[
    (
        r"^(?:export\s+)?(?:async\s+)?function\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\b",
        "function",
    ),
    (
        r"^(?:export\s+)?(?:abstract\s+)?class\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\b",
        "class",
    ),
    (
        r"^(?:export\s+)?interface\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\b",
        "interface",
    ),
    (
        r"^(?:export\s+)?type\s+([a-zA-Z_$][a-zA-Z0-9_$]*)\s*=",
        "type",
    ),
];

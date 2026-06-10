use std::path::Path;

use tree_sitter::Language;

use crate::code::{CodeChunk, TreeSitterSpec, generic_name, scan_with_tree_sitter};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_tree_sitter(
        path,
        content,
        TreeSitterSpec {
            language_name: "javascript",
            language: js_lang,
            top_kinds: KINDS,
            kind_label,
            name_extractor: generic_name,
            regex_fallback: super::typescript::PATTERNS,
        },
    )
}

fn js_lang() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

const KINDS: &[&str] = &[
    "function_declaration",
    "function",
    "class_declaration",
    "export_statement",
    "lexical_declaration",
    "variable_declaration",
];

fn kind_label(k: &str) -> &'static str {
    match k {
        "function_declaration" | "function" => "function",
        "class_declaration" => "class",
        "export_statement" => "export",
        "lexical_declaration" | "variable_declaration" => "const",
        _ => "decl",
    }
}

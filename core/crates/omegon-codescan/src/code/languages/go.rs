use std::path::Path;

use tree_sitter::{Language, Node};

use crate::code::{CodeChunk, PatternPair, TreeSitterSpec, generic_name, scan_with_tree_sitter};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_tree_sitter(
        path,
        content,
        TreeSitterSpec {
            language_name: "go",
            language: go_lang,
            top_kinds: KINDS,
            kind_label,
            name_extractor: name,
            regex_fallback: PATTERNS,
        },
    )
}

fn go_lang() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

const KINDS: &[&str] = &[
    "function_declaration",
    "method_declaration",
    "type_declaration",
];

fn kind_label(k: &str) -> &'static str {
    match k {
        "function_declaration" | "method_declaration" => "func",
        "type_declaration" => "type",
        _ => "decl",
    }
}

fn name(node: &Node, source: &[u8]) -> String {
    if node.kind() == "type_declaration" {
        let cursor = &mut node.walk();
        for child in node.children(cursor) {
            if child.kind() == "type_spec"
                && let Some(name_node) = child.child_by_field_name("name")
                && let Ok(text) = name_node.utf8_text(source)
            {
                return text.to_string();
            }
        }
    }
    generic_name(node, source)
}

const PATTERNS: &[PatternPair] = &[
    (
        r"^func\s+(?:\([^)]+\)\s+)?([a-zA-Z_][a-zA-Z0-9_]*)\b",
        "func",
    ),
    (
        r"^type\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+(?:struct|interface)\b",
        "type",
    ),
];

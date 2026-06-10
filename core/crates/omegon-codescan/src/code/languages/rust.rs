use std::path::Path;

use tree_sitter::{Language, Node};

use crate::code::{CodeChunk, PatternPair, TreeSitterSpec, generic_name, scan_with_tree_sitter};

pub(crate) fn scan(path: &Path, content: &str) -> Vec<CodeChunk> {
    scan_with_tree_sitter(
        path,
        content,
        TreeSitterSpec {
            language_name: "rust",
            language: rust_lang,
            top_kinds: KINDS,
            kind_label,
            name_extractor: name,
            regex_fallback: PATTERNS,
        },
    )
}

fn rust_lang() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

const KINDS: &[&str] = &[
    "function_item",
    "impl_item",
    "struct_item",
    "enum_item",
    "trait_item",
    "mod_item",
    "type_alias",
    "const_item",
    "static_item",
    "function_signature_item",
];

fn kind_label(k: &str) -> &'static str {
    match k {
        "function_item" | "function_signature_item" => "fn",
        "impl_item" => "impl",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "mod_item" => "mod",
        "type_alias" => "type",
        "const_item" => "const",
        "static_item" => "static",
        _ => "item",
    }
}

fn name(node: &Node, source: &[u8]) -> String {
    if node.kind() == "impl_item" {
        let type_name = node
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("?")
            .to_string();
        if let Some(trait_node) = node.child_by_field_name("trait") {
            let trait_name = trait_node.utf8_text(source).unwrap_or("?");
            return format!("{} for {}", trait_name, type_name);
        }
        return type_name;
    }
    generic_name(node, source)
}

const PATTERNS: &[PatternPair] = &[
    (
        r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\b",
        "fn",
    ),
    (
        r"^(?:pub(?:\([^)]*\))?\s+)?impl(?:<[^>]*>)?\s+(?:\S+\s+for\s+)?([a-zA-Z_][a-zA-Z0-9_:<>]*)",
        "impl",
    ),
    (
        r"^(?:pub(?:\([^)]*\))?\s+)?struct\s+([a-zA-Z_][a-zA-Z0-9_]*)\b",
        "struct",
    ),
    (
        r"^(?:pub(?:\([^)]*\))?\s+)?enum\s+([a-zA-Z_][a-zA-Z0-9_]*)\b",
        "enum",
    ),
    (
        r"^(?:pub(?:\([^)]*\))?\s+)?trait\s+([a-zA-Z_][a-zA-Z0-9_]*)\b",
        "trait",
    ),
    (
        r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-zA-Z_][a-zA-Z0-9_]*)\b",
        "mod",
    ),
];

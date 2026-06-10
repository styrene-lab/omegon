//! Code scanning entry point for codebase_search.
//!
//! Language-specific declaration rules live under `code/languages/`. This file
//! owns only dispatch and the shared chunking engines so new languages do not
//! turn into a monolithic `code.rs` edit.

use std::path::{Path, PathBuf};

use tree_sitter::{Language, Node, Parser};

mod languages;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionStrategy {
    TreeSitter,
    Regex,
}

impl ExtractionStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TreeSitter => "tree_sitter",
            Self::Regex => "regex",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "tree_sitter" => Self::TreeSitter,
            _ => Self::Regex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionConfidence {
    Extracted,
    Inferred,
    Ambiguous,
}

impl ExtractionConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracted => "extracted",
            Self::Inferred => "inferred",
            Self::Ambiguous => "ambiguous",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "extracted" => Self::Extracted,
            "ambiguous" => Self::Ambiguous,
            _ => Self::Inferred,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodeChunk {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub item_name: String,
    pub item_kind: String,
    pub text: String,
    /// Enclosing scope (e.g., "impl ShadowContext" for a method inside it).
    /// None for top-level declarations.
    pub parent_scope: Option<String>,
    pub language: String,
    pub strategy: ExtractionStrategy,
    pub confidence: ExtractionConfidence,
}

struct LanguageScanner {
    extensions: &'static [&'static str],
    scan: fn(&Path, &str) -> Vec<CodeChunk>,
}

const LANGUAGE_SCANNERS: &[LanguageScanner] = &[
    LanguageScanner {
        extensions: &["rs"],
        scan: languages::rust::scan,
    },
    LanguageScanner {
        extensions: &["ts", "mts"],
        scan: languages::typescript::scan,
    },
    LanguageScanner {
        extensions: &["tsx"],
        scan: languages::typescript::scan_tsx,
    },
    LanguageScanner {
        extensions: &["js", "jsx", "mjs"],
        scan: languages::javascript::scan,
    },
    LanguageScanner {
        extensions: &["py"],
        scan: languages::python::scan,
    },
    LanguageScanner {
        extensions: &["go"],
        scan: languages::go::scan,
    },
    LanguageScanner {
        extensions: &["java"],
        scan: languages::java::scan,
    },
    LanguageScanner {
        extensions: &["kt", "kts"],
        scan: languages::kotlin::scan,
    },
    LanguageScanner {
        extensions: &["cs"],
        scan: languages::csharp::scan,
    },
];

pub struct CodeScanner;

impl CodeScanner {
    pub fn scan_file(path: &Path, content: &str) -> Vec<CodeChunk> {
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            return vec![];
        };
        let Some(scanner) = scanner_for_extension(extension) else {
            return vec![];
        };
        (scanner.scan)(path, content)
    }
}

pub fn is_supported_code_extension(extension: &str) -> bool {
    scanner_for_extension(extension).is_some()
}

fn scanner_for_extension(extension: &str) -> Option<&'static LanguageScanner> {
    LANGUAGE_SCANNERS
        .iter()
        .find(|scanner| scanner.extensions.contains(&extension))
}

pub(crate) type PatternPair = (&'static str, &'static str);

pub(crate) fn scan_with_regex(
    path: &Path,
    content: &str,
    language: &str,
    patterns: &[PatternPair],
) -> Vec<CodeChunk> {
    use std::collections::BTreeMap;

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return vec![];
    }
    let total = lines.len();
    let mut matches: BTreeMap<usize, (String, String)> = BTreeMap::new();

    for &(pattern, kind) in patterns {
        let Ok(re) = regex::Regex::new(pattern) else {
            continue;
        };
        for (i, line) in lines.iter().enumerate() {
            if let Some(cap) = re.captures(line) {
                let name = cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                if !name.is_empty() {
                    matches.entry(i).or_insert_with(|| (name, kind.to_string()));
                }
            }
        }
    }

    let starts: Vec<usize> = matches.keys().copied().collect();
    let mut chunks = Vec::with_capacity(starts.len());
    for (i, &start) in starts.iter().enumerate() {
        let end = if i + 1 < starts.len() {
            starts[i + 1].saturating_sub(1)
        } else {
            total.saturating_sub(1)
        };
        let chunk_end = end.min(start + 99).min(total.saturating_sub(1));
        let (name, kind) = &matches[&start];
        let text = lines[start..=chunk_end].join("\n");
        chunks.push(CodeChunk {
            path: path.to_path_buf(),
            start_line: start + 1,
            end_line: chunk_end + 1,
            item_name: name.clone(),
            item_kind: kind.clone(),
            text,
            parent_scope: None,
            language: language.to_string(),
            strategy: ExtractionStrategy::Regex,
            confidence: ExtractionConfidence::Inferred,
        });
    }
    chunks
}

pub(crate) struct TreeSitterSpec {
    pub language_name: &'static str,
    pub language: fn() -> Language,
    pub top_kinds: &'static [&'static str],
    pub kind_label: fn(&str) -> &'static str,
    pub name_extractor: fn(&Node, &[u8]) -> String,
    pub regex_fallback: &'static [PatternPair],
}

pub(crate) fn scan_with_tree_sitter(
    path: &Path,
    content: &str,
    spec: TreeSitterSpec,
) -> Vec<CodeChunk> {
    let chunks = scan_with_ts(path, content, &spec);
    if chunks.is_empty() {
        scan_with_regex(path, content, spec.language_name, spec.regex_fallback)
    } else {
        chunks
    }
}

fn scan_with_ts(path: &Path, content: &str, spec: &TreeSitterSpec) -> Vec<CodeChunk> {
    let mut parser = Parser::new();
    if parser.set_language(&(spec.language)()).is_err() {
        return vec![];
    }
    let Some(tree) = parser.parse(content, None) else {
        return vec![];
    };

    let root = tree.root_node();
    let source = content.as_bytes();
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return vec![];
    }
    let total = lines.len();
    let mut chunks = Vec::new();

    let mut visitor = TreeSitterVisitor {
        path,
        source,
        lines: &lines,
        total,
        spec,
        chunks: &mut chunks,
    };
    visitor.visit(&root, None);
    chunks
}

struct TreeSitterVisitor<'a> {
    path: &'a Path,
    source: &'a [u8],
    lines: &'a [&'a str],
    total: usize,
    spec: &'a TreeSitterSpec,
    chunks: &'a mut Vec<CodeChunk>,
}

impl TreeSitterVisitor<'_> {
    fn visit(&mut self, node: &Node, parent_scope: Option<&str>) {
        let cursor = &mut node.walk();
        for child in node.children(cursor) {
            let kind = child.kind();
            let is_top = self.spec.top_kinds.contains(&kind);
            let is_inner = INNER_KINDS.contains(&kind);
            let is_container = CONTAINER_KINDS.contains(&kind);

            if !is_top && !is_inner && !is_container {
                continue;
            }

            let name = (self.spec.name_extractor)(&child, self.source);
            if name == "(anonymous)" && child.end_position().row == child.start_position().row {
                continue;
            }

            let start = child.start_position().row;
            let end = child.end_position().row;
            let chunk_end = end.min(start + 99).min(self.total.saturating_sub(1));

            if is_top || is_inner {
                let text = self.lines[start..=chunk_end].join("\n");
                let qualified_name = match parent_scope {
                    Some(scope) => format!("{scope}::{name}"),
                    None => name.clone(),
                };
                self.chunks.push(CodeChunk {
                    path: self.path.to_path_buf(),
                    start_line: start + 1,
                    end_line: chunk_end + 1,
                    item_name: qualified_name,
                    item_kind: (self.spec.kind_label)(kind).to_string(),
                    text,
                    parent_scope: parent_scope.map(String::from),
                    language: self.spec.language_name.to_string(),
                    strategy: ExtractionStrategy::TreeSitter,
                    confidence: ExtractionConfidence::Extracted,
                });
            }

            if is_container {
                let scope_name = match parent_scope {
                    Some(scope) => format!("{scope}::{name}"),
                    None => name,
                };
                self.visit(&child, Some(&scope_name));
            }
        }
    }
}

const CONTAINER_KINDS: &[&str] = &[
    "impl_item",
    "trait_item",
    "mod_item",
    "class_declaration",
    "abstract_class_declaration",
    "class_body",
    "class_definition",
];

const INNER_KINDS: &[&str] = &[
    "function_item",
    "function_signature_item",
    "type_alias",
    "const_item",
    "method_definition",
    "public_field_definition",
    "function_declaration",
    "function",
    "function_definition",
    "decorated_definition",
];

pub(crate) fn generic_name(node: &Node, source: &[u8]) -> String {
    if let Some(name_node) = node.child_by_field_name("name")
        && let Ok(text) = name_node.utf8_text(source)
    {
        return text.to_string();
    }
    if node.kind() == "export_statement" {
        if let Some(decl) = node.child_by_field_name("declaration") {
            return generic_name(&decl, source);
        }
        let cursor = &mut node.walk();
        for child in node.children(cursor) {
            if child.kind() == "export_clause" {
                let sub = &mut child.walk();
                for spec in child.children(sub) {
                    if spec.kind() == "export_specifier"
                        && let Some(n) = spec.child_by_field_name("name")
                        && let Ok(t) = n.utf8_text(source)
                    {
                        return t.to_string();
                    }
                }
            }
        }
    }
    if matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
        let cursor = &mut node.walk();
        for child in node.children(cursor) {
            if matches!(child.kind(), "variable_declarator")
                && let Some(n) = child.child_by_field_name("name")
                && let Ok(t) = n.utf8_text(source)
            {
                return t.to_string();
            }
        }
    }
    "(anonymous)".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sitter_chunks_are_extracted_confidence() {
        let chunks = CodeScanner::scan_file(Path::new("x.ts"), "export class Indexed {}\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].language, "typescript");
        assert_eq!(chunks[0].strategy, ExtractionStrategy::TreeSitter);
        assert_eq!(chunks[0].confidence, ExtractionConfidence::Extracted);
    }

    #[test]
    fn regex_chunks_are_inferred_confidence() {
        let chunks = CodeScanner::scan_file(
            Path::new("InvoiceService.java"),
            "class InvoiceService {}\n",
        );
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].language, "java");
        assert_eq!(chunks[0].strategy, ExtractionStrategy::Regex);
        assert_eq!(chunks[0].confidence, ExtractionConfidence::Inferred);
    }

    #[test]
    fn scan_rust_treesitter() {
        let src = r#"
pub struct Foo {
    x: i32,
}

impl Foo {
    pub fn new(x: i32) -> Self { Self { x } }
    fn private_helper(&self) {}
}

pub async fn top_level() {}

pub trait Bar {
    fn do_thing(&self);
}

pub enum Color { Red, Green, Blue }
"#;
        let chunks = CodeScanner::scan_file(Path::new("x.rs"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(names.contains(&"Foo"), "struct: {:?}", names);
        assert!(names.contains(&"Bar"), "trait: {:?}", names);
        assert!(names.contains(&"Color"), "enum: {:?}", names);
        assert!(
            chunks.iter().any(|c| c.item_kind == "impl"),
            "impl block: {:?}",
            names
        );
    }

    #[test]
    fn scan_rust_fn_name_extracted() {
        let src = "pub fn greet(name: &str) -> String {\n    format!(\"Hello, {name}\")\n}\n";
        let chunks = CodeScanner::scan_file(Path::new("greet.rs"), src);
        assert!(!chunks.is_empty(), "expected chunks");
        assert_eq!(chunks[0].item_name, "greet");
        assert_eq!(chunks[0].item_kind, "fn");
    }

    #[test]
    fn scan_typescript_treesitter() {
        let src = r#"
export class MyService {
    constructor(private repo: Repo) {}
}

export async function fetchData(url: string): Promise<void> {}

export interface Config {
    host: string;
}

export type Status = "active" | "inactive";
"#;
        let chunks = CodeScanner::scan_file(Path::new("x.ts"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(names.contains(&"MyService"), "class: {:?}", names);
        assert!(names.contains(&"fetchData"), "function: {:?}", names);
        assert!(names.contains(&"Config"), "interface: {:?}", names);
    }

    #[test]
    fn scan_python_treesitter() {
        let src =
            "class Foo:\n    def method(self): pass\n\nasync def handler(req):\n    return 'ok'\n";
        let chunks = CodeScanner::scan_file(Path::new("x.py"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(names.contains(&"Foo"), "class: {:?}", names);
        assert!(names.contains(&"handler"), "async def: {:?}", names);
    }

    #[test]
    fn scan_go_treesitter() {
        let src = "package main\n\ntype Server struct {\n\taddr string\n}\n\nfunc NewServer(addr string) *Server {\n\treturn &Server{addr: addr}\n}\n";
        let chunks = CodeScanner::scan_file(Path::new("x.go"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(names.contains(&"NewServer"), "func: {:?}", names);
        assert!(names.contains(&"Server"), "type: {:?}", names);
    }

    #[test]
    fn scan_java_regex() {
        let src = "public class InvoiceService {\n  public BigDecimal totalFor(Customer c) { return BigDecimal.ZERO; }\n}\n";
        let chunks = CodeScanner::scan_file(Path::new("InvoiceService.java"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(names.contains(&"InvoiceService"), "class: {:?}", names);
        assert!(names.contains(&"totalFor"), "method: {:?}", names);
    }

    #[test]
    fn scan_kotlin_regex() {
        let src = "data class TimeEntry(val id: String)\n\nfun calculateHours(start: Instant): Double = 0.0\n";
        let chunks = CodeScanner::scan_file(Path::new("TimeEntry.kt"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(names.contains(&"TimeEntry"), "class: {:?}", names);
        assert!(names.contains(&"calculateHours"), "fun: {:?}", names);
    }

    #[test]
    fn scan_csharp_regex() {
        let src = r#"
namespace Billing.Services;

public sealed class InvoiceService
{
    public decimal TotalFor(Customer customer) => 0m;
}

public interface ITimecardRepository {}
public record TimeEntry(string Id, decimal Hours);
"#;
        let chunks = CodeScanner::scan_file(Path::new("InvoiceService.cs"), src);
        let names: Vec<&str> = chunks.iter().map(|c| c.item_name.as_str()).collect();
        assert!(
            names.contains(&"Billing.Services"),
            "namespace: {:?}",
            names
        );
        assert!(names.contains(&"InvoiceService"), "class: {:?}", names);
        assert!(names.contains(&"TotalFor"), "method: {:?}", names);
        assert!(
            names.contains(&"ITimecardRepository"),
            "interface: {:?}",
            names
        );
        assert!(names.contains(&"TimeEntry"), "record: {:?}", names);
    }

    #[test]
    fn supported_code_extensions_include_jvm_and_dotnet_languages() {
        for extension in ["java", "kt", "kts", "cs"] {
            assert!(
                is_supported_code_extension(extension),
                "expected {extension} to be supported"
            );
        }
    }

    #[test]
    fn language_registry_scanners_cover_declared_extensions() {
        for scanner in LANGUAGE_SCANNERS {
            for extension in scanner.extensions {
                let path = PathBuf::from(format!("sample.{extension}"));
                let chunks = (scanner.scan)(&path, "");
                assert!(
                    chunks.is_empty(),
                    "empty source for {extension} should not produce chunks"
                );
                assert!(is_supported_code_extension(extension));
            }
        }
    }

    #[test]
    fn unknown_ext_empty() {
        assert!(CodeScanner::scan_file(Path::new("x.toml"), "key = 1").is_empty());
    }

    #[test]
    fn malformed_rust_falls_back_to_regex() {
        let src = "pub fn broken_but_matches(";
        let chunks = CodeScanner::scan_file(Path::new("x.rs"), src);
        assert!(!chunks.is_empty(), "should attempt extraction");
    }
}

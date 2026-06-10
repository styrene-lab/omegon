//! In-process BM25 ranking (k1=1.5, b=0.75) over code and knowledge chunks.

use std::collections::HashMap;

use crate::code::CodeChunk;
use crate::knowledge::KnowledgeChunk;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchScope {
    All,
    Code,
    Knowledge,
}

impl SearchScope {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "code" => Self::Code,
            "knowledge" | "docs" => Self::Knowledge,
            _ => Self::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChunkType {
    Code,
    Knowledge,
}

impl ChunkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChunkType::Code => "code",
            ChunkType::Knowledge => "knowledge",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchChunk {
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub chunk_type: ChunkType,
    pub score: f64,
    pub preview: String,
    pub label: String,
}

pub struct BM25Index {
    code_docs: Vec<(CodeChunk, Vec<String>)>,
    knowledge_docs: Vec<(KnowledgeChunk, Vec<String>)>,
    avg_code_len: f64,
    avg_knowledge_len: f64,
}

impl BM25Index {
    pub fn build(code: &[CodeChunk], knowledge: &[KnowledgeChunk]) -> Self {
        let code_docs: Vec<_> = code
            .iter()
            .map(|c| {
                let scope = c.parent_scope.as_deref().unwrap_or("");
                let text = format!("{} {} {} {}", scope, c.item_name, c.item_kind, c.text);
                (c.clone(), tokenize(&text))
            })
            .collect();
        let knowledge_docs: Vec<_> = knowledge
            .iter()
            .map(|c| {
                let text = format!("{} {} {}", c.heading, c.tags.join(" "), c.text);
                (c.clone(), tokenize(&text))
            })
            .collect();
        let avg_code_len = avg_len(&code_docs);
        let avg_knowledge_len = avg_len(&knowledge_docs);
        Self {
            code_docs,
            knowledge_docs,
            avg_code_len,
            avg_knowledge_len,
        }
    }

    pub fn search(&self, query: &str, scope: SearchScope, max_results: usize) -> Vec<SearchChunk> {
        let qtoks = tokenize(query);
        if qtoks.is_empty() {
            return vec![];
        }
        let mut results = Vec::new();

        if matches!(scope, SearchScope::All | SearchScope::Code) {
            let n = self.code_docs.len() as f64;
            let df = build_df(self.code_docs.iter().map(|(_, t)| t.as_slice()));
            for (chunk, tokens) in &self.code_docs {
                let score = bm25(&qtoks, tokens, n, &df, self.avg_code_len);
                if score > 0.0 {
                    results.push(SearchChunk {
                        file: chunk.path.to_string_lossy().to_string(),
                        start_line: chunk.start_line,
                        end_line: chunk.end_line,
                        chunk_type: ChunkType::Code,
                        score,
                        preview: trunc(&chunk.text, 1000),
                        label: format!("{}::{}", chunk.item_kind, chunk.item_name),
                    });
                }
            }
        }

        if matches!(scope, SearchScope::All | SearchScope::Knowledge) {
            let n = self.knowledge_docs.len() as f64;
            let df = build_df(self.knowledge_docs.iter().map(|(_, t)| t.as_slice()));
            for (chunk, tokens) in &self.knowledge_docs {
                let score = bm25(&qtoks, tokens, n, &df, self.avg_knowledge_len);
                if score > 0.0 {
                    results.push(SearchChunk {
                        file: chunk.path.to_string_lossy().to_string(),
                        start_line: chunk.start_line,
                        end_line: chunk.end_line,
                        chunk_type: ChunkType::Knowledge,
                        score,
                        preview: trunc(&chunk.text, 1000),
                        label: chunk.heading.clone(),
                    });
                }
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(max_results);
        results
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_string())
        .collect()
}

fn avg_len<T>(docs: &[(T, Vec<String>)]) -> f64 {
    if docs.is_empty() {
        1.0
    } else {
        docs.iter().map(|(_, t)| t.len() as f64).sum::<f64>() / docs.len() as f64
    }
}

fn build_df<'a>(docs: impl Iterator<Item = &'a [String]>) -> HashMap<String, f64> {
    let mut df: HashMap<String, f64> = HashMap::new();
    for doc in docs {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for t in doc {
            if seen.insert(t.as_str()) {
                *df.entry(t.clone()).or_default() += 1.0;
            }
        }
    }
    df
}

fn bm25(query: &[String], doc: &[String], n: f64, df: &HashMap<String, f64>, avg_dl: f64) -> f64 {
    if n == 0.0 {
        return 0.0;
    }
    let k1 = 1.5;
    let b = 0.75;
    let dl = doc.len() as f64;
    let mut tf_map: HashMap<&str, f64> = HashMap::new();
    for t in doc {
        *tf_map.entry(t.as_str()).or_default() += 1.0;
    }
    let mut score = 0.0;
    for q in query {
        let tf = *tf_map.get(q.as_str()).unwrap_or(&0.0);
        if tf == 0.0 {
            continue;
        }
        let df_val = *df.get(q.as_str()).unwrap_or(&0.0);
        let idf = ((n - df_val + 0.5) / (df_val + 0.5) + 1.0).ln();
        score += idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_dl.max(1.0)));
    }
    score
}

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk_code(name: &str, text: &str) -> CodeChunk {
        CodeChunk {
            path: PathBuf::from("src/f.rs"),
            start_line: 1,
            end_line: 10,
            item_name: name.into(),
            item_kind: "fn".into(),
            text: text.into(),
            parent_scope: None,
            language: "rust".into(),
            strategy: crate::code::ExtractionStrategy::TreeSitter,
            confidence: crate::code::ExtractionConfidence::Extracted,
        }
    }
    fn mk_knowledge(heading: &str, text: &str) -> KnowledgeChunk {
        KnowledgeChunk {
            path: PathBuf::from("docs/f.md"),
            heading: heading.into(),
            start_line: 1,
            end_line: 10,
            tags: vec![],
            text: text.into(),
        }
    }

    #[test]
    fn exact_match_scores_highest() {
        let code = vec![
            mk_code("sanitize_tool_id", "sanitize tool id strips invalid chars"),
            mk_code("unrelated", "buffer allocation strategy"),
        ];
        let idx = BM25Index::build(&code, &[]);
        let res = idx.search("sanitize tool id", SearchScope::Code, 10);
        assert!(!res.is_empty());
        assert_eq!(res[0].label, "fn::sanitize_tool_id");
    }

    #[test]
    fn knowledge_search() {
        let k = vec![
            mk_knowledge(
                "LSP integration",
                "Language Server Protocol code navigation",
            ),
            mk_knowledge("Memory system", "Fact storage retrieval sessions"),
        ];
        let idx = BM25Index::build(&[], &k);
        let res = idx.search("language server protocol", SearchScope::Knowledge, 10);
        assert!(!res.is_empty());
        assert_eq!(res[0].label, "LSP integration");
    }

    #[test]
    fn empty_query_returns_nothing() {
        let idx = BM25Index::build(&[mk_code("foo", "bar")], &[]);
        assert!(idx.search("", SearchScope::All, 10).is_empty());
    }

    #[test]
    fn scope_filtering() {
        let code = vec![mk_code("foo", "progress sink transport")];
        let k = vec![mk_knowledge("Transport", "progress sink transport")];
        let idx = BM25Index::build(&code, &k);
        let code_only = idx.search("progress sink", SearchScope::Code, 10);
        assert!(code_only.iter().all(|r| r.chunk_type == ChunkType::Code));
        let k_only = idx.search("progress sink", SearchScope::Knowledge, 10);
        assert!(k_only.iter().all(|r| r.chunk_type == ChunkType::Knowledge));
    }

    #[test]
    fn max_results_respected() {
        let code: Vec<_> = (0..20)
            .map(|i| mk_code(&format!("fn_{i}"), "common token here"))
            .collect();
        let idx = BM25Index::build(&code, &[]);
        let res = idx.search("common token", SearchScope::Code, 5);
        assert!(res.len() <= 5);
    }
}

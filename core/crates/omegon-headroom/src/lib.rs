use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Json,
    Code,
    Log,
    Diff,
    Markdown,
    PlainText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadroomRef {
    pub id: String,
    pub sha256: String,
    pub bytes: usize,
    pub content_kind: ContentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeadroomPolicy {
    pub enabled: bool,
    pub min_bytes: usize,
    pub target_bytes: usize,
    pub reversible: bool,
    pub excerpt_lines: usize,
    pub signal_lines: usize,
}

impl Default for HeadroomPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            min_bytes: 8 * 1024,
            target_bytes: 24 * 1024,
            reversible: true,
            excerpt_lines: 16,
            signal_lines: 48,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressionInput {
    pub kind_hint: Option<ContentKind>,
    pub source: String,
    pub text: String,
    pub policy: HeadroomPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionStats {
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub saved_bytes: usize,
    pub savings_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionOutput {
    pub text: String,
    pub content_kind: ContentKind,
    pub original_ref: Option<HeadroomRef>,
    pub stats: CompressionStats,
    pub compressed: bool,
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryHeadroomStore {
    objects: BTreeMap<String, StoredOriginal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOriginal {
    pub reference: HeadroomRef,
    pub source: String,
    pub text: String,
}

impl InMemoryHeadroomStore {
    pub fn compress(&mut self, input: CompressionInput) -> CompressionOutput {
        let kind = input.kind_hint.unwrap_or_else(|| detect_kind(&input.text));
        let original_bytes = input.text.len();

        if !input.policy.enabled || original_bytes < input.policy.min_bytes {
            return CompressionOutput {
                text: input.text,
                content_kind: kind,
                original_ref: None,
                stats: CompressionStats {
                    original_bytes,
                    compressed_bytes: original_bytes,
                    saved_bytes: 0,
                    savings_percent: 0,
                },
                compressed: false,
            };
        }

        let reference = input
            .policy
            .reversible
            .then(|| self.store_original(&input.source, kind, &input.text));
        let compressed_text = compress_by_kind(kind, &input.text, input.policy, reference.as_ref());
        let compressed_bytes = compressed_text.len();
        let saved_bytes = original_bytes.saturating_sub(compressed_bytes);
        let savings_percent = if original_bytes == 0 {
            0
        } else {
            ((saved_bytes * 100) / original_bytes).min(100) as u8
        };

        CompressionOutput {
            text: compressed_text,
            content_kind: kind,
            original_ref: reference,
            stats: CompressionStats {
                original_bytes,
                compressed_bytes,
                saved_bytes,
                savings_percent,
            },
            compressed: true,
        }
    }

    pub fn retrieve(&self, id: &str) -> Result<&StoredOriginal> {
        self.objects
            .get(id)
            .ok_or_else(|| anyhow!("unknown headroom object: {id}"))
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    fn store_original(&mut self, source: &str, kind: ContentKind, text: &str) -> HeadroomRef {
        let sha256 = sha256_hex(text.as_bytes());
        let id = format!("hr:{}", &sha256[..16]);
        let reference = HeadroomRef {
            id: id.clone(),
            sha256,
            bytes: text.len(),
            content_kind: kind,
        };
        self.objects.entry(id).or_insert_with(|| StoredOriginal {
            reference: reference.clone(),
            source: source.to_owned(),
            text: text.to_owned(),
        });
        reference
    }
}

pub fn detect_kind(text: &str) -> ContentKind {
    let trimmed = text.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return ContentKind::Json;
    }
    if text
        .lines()
        .take(20)
        .any(|line| line.starts_with("diff --git") || line.starts_with("@@ "))
    {
        return ContentKind::Diff;
    }
    if text.lines().take(40).any(|line| line.starts_with('#')) {
        return ContentKind::Markdown;
    }
    if looks_like_code(text) {
        return ContentKind::Code;
    }
    if text.lines().take(100).any(is_signal_line) {
        return ContentKind::Log;
    }
    ContentKind::PlainText
}

fn compress_by_kind(
    kind: ContentKind,
    text: &str,
    policy: HeadroomPolicy,
    reference: Option<&HeadroomRef>,
) -> String {
    match kind {
        ContentKind::Json => compress_json(text, policy, reference),
        ContentKind::Code => compress_code(text, policy, reference),
        ContentKind::Log | ContentKind::Diff => compress_signal_text(kind, text, policy, reference),
        ContentKind::Markdown | ContentKind::PlainText => {
            compress_plain(kind, text, policy, reference)
        }
    }
}

fn header(kind: ContentKind, text: &str, reference: Option<&HeadroomRef>) -> String {
    let mut out = format!(
        "[headroom: compressed {kind:?}]\noriginal_bytes: {}\noriginal_lines: {}\n",
        text.len(),
        text.lines().count()
    );
    if let Some(reference) = reference {
        out.push_str(&format!(
            "retrieve: headroom_retrieve id={}\nsha256: {}\n",
            reference.id, reference.sha256
        ));
    }
    out.push('\n');
    out
}

fn compress_json(text: &str, policy: HeadroomPolicy, reference: Option<&HeadroomRef>) -> String {
    let mut out = header(ContentKind::Json, text, reference);
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Array(items)) => {
            out.push_str(&format!("json_array_items: {}\n", items.len()));
            if let Some(keys) = common_object_keys(&items) {
                out.push_str(&format!("object_keys: {}\n", keys.join(", ")));
            }
            out.push_str("\nsample_first:\n");
            for item in items.iter().take(policy.excerpt_lines.min(5)) {
                out.push_str(&compact_json_line(item));
                out.push('\n');
            }
            if items.len() > policy.excerpt_lines.min(5) {
                out.push_str("\nsample_last:\n");
                let mut tail = items.iter().rev().take(3).collect::<Vec<_>>();
                tail.reverse();
                for item in tail {
                    out.push_str(&compact_json_line(item));
                    out.push('\n');
                }
            }
        }
        Ok(value) => {
            out.push_str("json_shape:\n");
            out.push_str(&json_shape(&value, 0));
        }
        Err(_) => out.push_str("json_parse: failed; falling back to text excerpts\n"),
    }
    trim_to_target(out, policy.target_bytes)
}

fn compress_signal_text(
    kind: ContentKind,
    text: &str,
    policy: HeadroomPolicy,
    reference: Option<&HeadroomRef>,
) -> String {
    let mut out = header(kind, text, reference);
    push_excerpt(&mut out, "first", text.lines().take(policy.excerpt_lines));
    let signals = text
        .lines()
        .filter(|line| is_signal_line(line))
        .take(policy.signal_lines)
        .collect::<Vec<_>>();
    if !signals.is_empty() {
        push_excerpt(&mut out, "signal_lines", signals.into_iter());
    }
    let mut tail = text
        .lines()
        .rev()
        .take(policy.excerpt_lines)
        .collect::<Vec<_>>();
    tail.reverse();
    push_excerpt(&mut out, "last", tail.into_iter());
    trim_to_target(out, policy.target_bytes)
}

fn compress_code(text: &str, policy: HeadroomPolicy, reference: Option<&HeadroomRef>) -> String {
    let mut out = header(ContentKind::Code, text, reference);
    let signatures = text
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            t.starts_with("use ")
                || t.starts_with("pub ")
                || t.starts_with("fn ")
                || t.starts_with("impl ")
                || t.starts_with("struct ")
                || t.starts_with("enum ")
                || t.starts_with("trait ")
                || t.starts_with("class ")
                || t.starts_with("def ")
                || t.starts_with("function ")
        })
        .take(policy.signal_lines)
        .collect::<Vec<_>>();
    push_excerpt(&mut out, "signatures", signatures.into_iter());
    trim_to_target(out, policy.target_bytes)
}

fn compress_plain(
    kind: ContentKind,
    text: &str,
    policy: HeadroomPolicy,
    reference: Option<&HeadroomRef>,
) -> String {
    let mut out = header(kind, text, reference);
    let headings = text
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .take(policy.signal_lines)
        .collect::<Vec<_>>();
    if !headings.is_empty() {
        push_excerpt(&mut out, "headings", headings.into_iter());
    }
    push_excerpt(&mut out, "first", text.lines().take(policy.excerpt_lines));
    let mut tail = text
        .lines()
        .rev()
        .take(policy.excerpt_lines)
        .collect::<Vec<_>>();
    tail.reverse();
    push_excerpt(&mut out, "last", tail.into_iter());
    trim_to_target(out, policy.target_bytes)
}

fn push_excerpt<'a>(out: &mut String, label: &str, lines: impl Iterator<Item = &'a str>) {
    out.push_str(&format!("\n{label}:\n"));
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
}

fn is_signal_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("warn")
        || lower.contains("panic")
        || lower.contains("fatal")
        || lower.contains("failed")
        || lower.contains("failure")
        || lower.contains("todo")
        || lower.contains("fixme")
        || line.starts_with('+')
        || line.starts_with('-')
}

fn looks_like_code(text: &str) -> bool {
    text.lines()
        .take(80)
        .filter(|line| {
            let t = line.trim_start();
            t.starts_with("fn ")
                || t.starts_with("pub fn ")
                || t.starts_with("use ")
                || t.starts_with("impl ")
                || t.starts_with("struct ")
                || t.starts_with("def ")
                || t.starts_with("class ")
                || t.contains(" => ")
        })
        .count()
        >= 2
}

fn common_object_keys(items: &[serde_json::Value]) -> Option<Vec<String>> {
    let mut keys = BTreeMap::<String, usize>::new();
    for item in items.iter().take(50) {
        if let serde_json::Value::Object(map) = item {
            for key in map.keys() {
                *keys.entry(key.clone()).or_default() += 1;
            }
        }
    }
    if keys.is_empty() {
        return None;
    }
    Some(keys.into_keys().collect())
}

fn compact_json_line(value: &serde_json::Value) -> String {
    let s = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    if s.len() > 240 {
        let boundary = floor_char_boundary(&s, 240);
        format!("{}…", &s[..boundary])
    } else {
        s
    }
}

fn json_shape(value: &serde_json::Value, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .take(40)
            .map(|(k, v)| format!("{indent}{k}: {}\n", json_type(v)))
            .collect(),
        serde_json::Value::Array(items) => format!("{indent}array_items: {}\n", items.len()),
        other => format!("{indent}{}\n", json_type(other)),
    }
}

fn json_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn trim_to_target(mut text: String, target_bytes: usize) -> String {
    if text.len() <= target_bytes {
        return text;
    }
    let boundary = floor_char_boundary(&text, target_bytes.saturating_sub(64));
    text.truncate(boundary);
    text.push_str("\n[headroom: compressed output clipped to policy target]\n");
    text
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut i = index.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HeadroomPolicy {
        HeadroomPolicy {
            min_bytes: 1,
            target_bytes: 4096,
            ..HeadroomPolicy::default()
        }
    }

    #[test]
    fn detects_json_arrays() {
        assert_eq!(detect_kind(r#"[{"path":"a","line":1}]"#), ContentKind::Json);
    }

    #[test]
    fn compresses_json_and_retrieves_original() {
        let mut store = InMemoryHeadroomStore::default();
        let rows = (0..200).map(|i| format!(r#"{{"path":"src/{i}.rs","line":{i},"kind":"function","preview":"fn item_{i}() {{}}"}}"#)).collect::<Vec<_>>().join(",");
        let text = format!("[{rows}]");
        let output = store.compress(CompressionInput {
            kind_hint: None,
            source: "test".into(),
            text: text.clone(),
            policy: policy(),
        });
        assert!(output.compressed);
        assert_eq!(output.content_kind, ContentKind::Json);
        assert!(output.text.contains("json_array_items: 200"));
        assert!(output.stats.compressed_bytes < output.stats.original_bytes);
        let id = output.original_ref.unwrap().id;
        assert_eq!(store.retrieve(&id).unwrap().text, text);
    }

    #[test]
    fn compresses_logs_around_signal_lines() {
        let mut store = InMemoryHeadroomStore::default();
        let text = (0..400)
            .map(|i| {
                if i == 250 {
                    "ERROR failed to open database".to_owned()
                } else {
                    format!("info line {i}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let output = store.compress(CompressionInput {
            kind_hint: Some(ContentKind::Log),
            source: "log".into(),
            text,
            policy: policy(),
        });
        assert!(output.text.contains("ERROR failed to open database"));
        assert!(output.text.contains("retrieve: headroom_retrieve id=hr:"));
    }

    #[test]
    fn leaves_small_inputs_uncompressed() {
        let mut store = InMemoryHeadroomStore::default();
        let output = store.compress(CompressionInput {
            kind_hint: None,
            source: "small".into(),
            text: "short".into(),
            policy: HeadroomPolicy::default(),
        });
        assert!(!output.compressed);
        assert!(output.original_ref.is_none());
        assert!(store.is_empty());
    }
}

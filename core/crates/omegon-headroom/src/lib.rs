use std::collections::{BTreeMap, VecDeque};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod validation;

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
pub struct CompressionProviderInfo {
    pub id: String,
    pub kind: String,
    pub version: String,
}

pub fn native_deterministic_provider() -> CompressionProviderInfo {
    CompressionProviderInfo {
        id: "native_deterministic".into(),
        kind: "deterministic".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionOutput {
    pub text: String,
    pub content_kind: ContentKind,
    pub original_ref: Option<HeadroomRef>,
    pub stats: CompressionStats,
    pub compressed: bool,
    pub provider: CompressionProviderInfo,
}

#[derive(Debug, Clone)]
pub struct InMemoryHeadroomStore {
    objects: BTreeMap<String, StoredOriginal>,
    order: VecDeque<String>,
    policy: HeadroomStorePolicy,
    total_original_bytes: usize,
    evicted_count: usize,
}

impl Default for InMemoryHeadroomStore {
    fn default() -> Self {
        Self::with_policy(HeadroomStorePolicy::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadroomStorePolicy {
    pub max_objects: usize,
    pub max_original_bytes: usize,
}

impl Default for HeadroomStorePolicy {
    fn default() -> Self {
        Self {
            max_objects: 128,
            max_original_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadroomStoreStats {
    pub objects: usize,
    pub original_bytes: usize,
    pub max_objects: usize,
    pub max_original_bytes: usize,
    pub evicted_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOriginal {
    pub reference: HeadroomRef,
    pub source: String,
    pub text: String,
}

impl InMemoryHeadroomStore {
    pub fn with_policy(policy: HeadroomStorePolicy) -> Self {
        Self {
            objects: BTreeMap::new(),
            order: VecDeque::new(),
            policy,
            total_original_bytes: 0,
            evicted_count: 0,
        }
    }

    pub fn set_policy(&mut self, policy: HeadroomStorePolicy) {
        self.policy = policy;
        self.enforce_policy();
    }

    pub fn policy(&self) -> HeadroomStorePolicy {
        self.policy
    }

    pub fn evicted_count(&self) -> usize {
        self.evicted_count
    }

    pub fn stats(&self) -> HeadroomStoreStats {
        HeadroomStoreStats {
            objects: self.len(),
            original_bytes: self.total_original_bytes,
            max_objects: self.policy.max_objects,
            max_original_bytes: self.policy.max_original_bytes,
            evicted_count: self.evicted_count,
        }
    }

    pub fn compress(&mut self, input: CompressionInput) -> CompressionOutput {
        let kind = input.kind_hint.unwrap_or_else(|| detect_kind(&input.text));
        let original_bytes = input.text.len();

        let provider = native_deterministic_provider();

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
                provider: provider.clone(),
            };
        }

        let reference = input
            .policy
            .reversible
            .then(|| make_reference(kind, &input.text));
        let compressed_text = compress_by_kind(kind, &input.text, input.policy, reference.as_ref());
        let compressed_bytes = compressed_text.len();
        if compressed_bytes >= original_bytes {
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
                provider: provider.clone(),
            };
        }
        if let Some(reference) = reference.as_ref() {
            self.store_original(reference.clone(), &input.source, &input.text);
        }
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
            provider,
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

    pub fn total_original_bytes(&self) -> usize {
        self.total_original_bytes
    }

    pub fn objects(&self) -> impl Iterator<Item = &StoredOriginal> {
        self.objects.values()
    }

    fn store_original(&mut self, reference: HeadroomRef, source: &str, text: &str) {
        if self.objects.contains_key(&reference.id) {
            return;
        }
        if self.policy.max_objects == 0 || self.policy.max_original_bytes == 0 {
            self.evicted_count += 1;
            return;
        }
        let id = reference.id.clone();
        self.total_original_bytes += reference.bytes;
        self.order.push_back(id.clone());
        self.objects.insert(
            id,
            StoredOriginal {
                reference,
                source: source.to_owned(),
                text: text.to_owned(),
            },
        );
        self.enforce_policy();
    }

    fn enforce_policy(&mut self) {
        while self.objects.len() > self.policy.max_objects
            || self.total_original_bytes > self.policy.max_original_bytes
        {
            let Some(id) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.objects.remove(&id) {
                self.total_original_bytes = self
                    .total_original_bytes
                    .saturating_sub(removed.reference.bytes);
                self.evicted_count += 1;
            }
        }
    }
}

fn make_reference(kind: ContentKind, text: &str) -> HeadroomRef {
    let sha256 = sha256_hex(text.as_bytes());
    HeadroomRef {
        id: format!("hr:{}", &sha256[..16]),
        sha256,
        bytes: text.len(),
        content_kind: kind,
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
            let signal_items = json_signal_items(&items, policy.signal_lines);
            if !signal_items.is_empty() {
                out.push_str("\nsignal_items:\n");
                for item in signal_items {
                    out.push_str(&compact_json_line(item));
                    out.push('\n');
                }
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
    let mut budget = SectionBudget::new(policy.target_bytes, out.len());
    push_budgeted_section(
        &mut out,
        "protected_anchors",
        protected_anchor_lines(text, policy.signal_lines).into_iter(),
        &mut budget,
    );
    let signals = text
        .lines()
        .filter(|line| is_signal_line(line))
        .take(policy.signal_lines)
        .collect::<Vec<_>>();
    push_budgeted_section(&mut out, "signal_lines", signals.into_iter(), &mut budget);
    push_budgeted_section(
        &mut out,
        "first",
        text.lines().take(policy.excerpt_lines),
        &mut budget,
    );
    let mut tail = text
        .lines()
        .rev()
        .take(policy.excerpt_lines)
        .collect::<Vec<_>>();
    tail.reverse();
    push_budgeted_section(&mut out, "last", tail.into_iter(), &mut budget);
    out
}

fn compress_code(text: &str, policy: HeadroomPolicy, reference: Option<&HeadroomRef>) -> String {
    let mut out = header(ContentKind::Code, text, reference);
    let mut budget = SectionBudget::new(policy.target_bytes, out.len());
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
    push_budgeted_section(&mut out, "signatures", signatures.into_iter(), &mut budget);
    push_budgeted_section(
        &mut out,
        "code_signal_anchors",
        code_signal_anchor_lines(text, policy.signal_lines).into_iter(),
        &mut budget,
    );
    out
}

fn compress_plain(
    kind: ContentKind,
    text: &str,
    policy: HeadroomPolicy,
    reference: Option<&HeadroomRef>,
) -> String {
    let mut out = header(kind, text, reference);
    let mut budget = SectionBudget::new(policy.target_bytes, out.len());
    let heading_anchors = if matches!(kind, ContentKind::Markdown | ContentKind::Diff) {
        markdown_heading_anchor_lines(text, policy.signal_lines)
    } else {
        Vec::new()
    };
    push_budgeted_section(
        &mut out,
        "heading_anchors",
        heading_anchors.iter().copied(),
        &mut budget,
    );
    push_budgeted_section(
        &mut out,
        "protected_anchors",
        protected_anchor_lines(text, policy.signal_lines).into_iter(),
        &mut budget,
    );
    let headings = text
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .take(policy.signal_lines)
        .collect::<Vec<_>>();
    push_budgeted_section(&mut out, "headings", headings.into_iter(), &mut budget);
    push_budgeted_section(
        &mut out,
        "first",
        text.lines().take(policy.excerpt_lines),
        &mut budget,
    );
    let mut tail = text
        .lines()
        .rev()
        .take(policy.excerpt_lines)
        .collect::<Vec<_>>();
    tail.reverse();
    push_budgeted_section(&mut out, "last", tail.into_iter(), &mut budget);
    out
}

struct SectionBudget {
    remaining: usize,
}

impl SectionBudget {
    fn new(target_bytes: usize, used_bytes: usize) -> Self {
        Self {
            remaining: target_bytes.saturating_sub(used_bytes),
        }
    }

    fn consume(&mut self, bytes: usize) {
        self.remaining = self.remaining.saturating_sub(bytes);
    }
}

fn push_budgeted_section<'a>(
    out: &mut String,
    label: &str,
    lines: impl Iterator<Item = &'a str>,
    budget: &mut SectionBudget,
) {
    if budget.remaining == 0 {
        return;
    }
    let lines = lines.collect::<Vec<_>>();
    if lines.is_empty() {
        return;
    }

    let header = format!("\n{label}:\n");
    if budget.remaining <= header.len() {
        return;
    }
    out.push_str(&header);
    budget.consume(header.len());

    let mut emitted = 0usize;
    let mut clipped = false;
    for line in lines {
        let needed = line.len() + 1;
        if needed > budget.remaining {
            clipped = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
        budget.consume(needed);
        emitted += 1;
    }

    if clipped && budget.remaining > 0 {
        let marker = format!("[headroom: {label} clipped]\n");
        if marker.len() <= budget.remaining {
            out.push_str(&marker);
            budget.consume(marker.len());
        }
    }

    if emitted == 0 {
        out.push_str("[headroom: section omitted by budget]\n");
    }
}

fn code_signal_anchor_lines(text: &str, limit: usize) -> Vec<&str> {
    let mut anchors = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !(has_quoted_signal_token(trimmed) || has_code_signal_token(trimmed)) {
            continue;
        }
        if anchors.contains(&line) {
            continue;
        }
        anchors.push(line);
        if anchors.len() >= limit {
            break;
        }
    }
    anchors
}

fn has_code_signal_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "json_parse: failed",
        "error_code",
        "exit_code",
        "failures",
        "failed",
        "blocked",
        "critical",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn markdown_heading_anchor_lines(text: &str, limit: usize) -> Vec<&str> {
    let mut anchors = Vec::new();
    for line in text.lines() {
        let trimmed = line
            .trim_start()
            .trim_start_matches('+')
            .trim_start_matches('-')
            .trim_start();
        let Some(heading) = trimmed.strip_prefix('#') else {
            continue;
        };
        let heading = heading.trim_start_matches('#').trim_start();
        if !is_relevant_markdown_heading(heading) {
            continue;
        }
        if anchors.contains(&line) {
            continue;
        }
        anchors.push(line);
        if anchors.len() >= limit {
            break;
        }
    }
    anchors
}

fn is_relevant_markdown_heading(heading: &str) -> bool {
    let lower = heading.to_ascii_lowercase();
    [
        "headroom",
        "evaluation",
        "compression",
        "provider",
        "dogfood",
        "ccr",
        "policy",
        "kompressor",
        "manual",
        "overflow",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn protected_anchor_lines(text: &str, limit: usize) -> Vec<&str> {
    scored_anchor_lines(text)
        .into_iter()
        .take(limit)
        .map(|(_, _, line)| line)
        .collect()
}

fn scored_anchor_lines(text: &str) -> Vec<(u8, usize, &str)> {
    let mut scored = Vec::<(u8, usize, &str)>::new();
    for (index, line) in text.lines().enumerate() {
        if !is_protected_anchor_line(line)
            || scored.iter().any(|(_, _, existing)| *existing == line)
        {
            continue;
        }
        scored.push((anchor_score(line), index, line));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored
}

fn is_protected_anchor_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    is_signal_line(trimmed)
        || is_task_checkbox_line(trimmed)
        || has_path_with_line_number(trimmed)
        || has_hash_like_token(trimmed)
        || has_nonzero_exit_code(trimmed)
        || has_test_count_summary(trimmed)
        || is_rust_test_result_line(trimmed)
        || is_rust_function_declaration_anchor(trimmed)
        || has_cli_flag_token(trimmed)
        || has_headroom_token(trimmed)
        || has_quoted_signal_token(trimmed)
}

fn anchor_score(line: &str) -> u8 {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if is_rust_test_result_line(trimmed)
        && ["headroom", "compression", "retrieve", "store"]
            .iter()
            .any(|needle| lower.contains(needle))
    {
        return 110;
    }
    if has_test_count_summary(trimmed) && lower.contains(" passed") && !lower.contains("0 passed") {
        return 100;
    }
    if lower.contains("failed")
        || lower.contains("failure")
        || lower.contains("panic")
        || lower.contains("fatal")
        || lower.contains("error")
    {
        return 90;
    }
    if has_path_with_line_number(trimmed) {
        return 80;
    }
    if has_quoted_signal_token(trimmed) {
        return 106;
    }
    if has_headroom_token(trimmed) {
        return 85;
    }
    if is_rust_function_declaration_anchor(trimmed) {
        return if lower.contains("headroom") { 108 } else { 82 };
    }
    if has_nonzero_exit_code(trimmed) {
        return 70;
    }
    if has_cli_flag_token(trimmed) {
        return 75;
    }
    if is_rust_test_result_line(trimmed) {
        return 60;
    }
    if is_task_checkbox_line(trimmed) || has_hash_like_token(trimmed) {
        return 50;
    }
    40
}

fn is_task_checkbox_line(trimmed: &str) -> bool {
    trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]")
}

fn is_rust_test_result_line(trimmed: &str) -> bool {
    trimmed.starts_with("test ")
        && trimmed.contains("::")
        && trimmed.contains(" ... ")
        && (trimmed.ends_with(" ok")
            || trimmed.ends_with(" FAILED")
            || trimmed.ends_with(" ignored")
            || trimmed.ends_with(" measured"))
}

fn is_rust_function_declaration_anchor(trimmed: &str) -> bool {
    let trimmed = trimmed
        .trim_start_matches('+')
        .trim_start_matches('-')
        .trim_start();
    let Some(rest) = trimmed
        .strip_prefix("fn ")
        .or_else(|| trimmed.strip_prefix("pub fn "))
        .or_else(|| trimmed.strip_prefix("async fn "))
        .or_else(|| trimmed.strip_prefix("pub async fn "))
    else {
        return false;
    };
    let Some(name) = rest.split_once('(').map(|(name, _)| name.trim()) else {
        return false;
    };
    !name.is_empty()
        && [
            "headroom",
            "compression",
            "compress",
            "retrieve",
            "anchor",
            "cli",
            "read_compress",
            "dogfood",
            "fixture",
            "eval",
        ]
        .iter()
        .any(|needle| name.contains(needle))
}

fn has_cli_flag_token(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| {
            matches!(c, '`' | ',' | '.' | ')' | '(' | '[' | ']' | ':' | ';')
        });
        token.starts_with("--")
            && token.len() > 2
            && token[2..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn has_quoted_signal_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let normalized = lower.replace("\\\\\\\"", "\"").replace("\\\"", "\"");
    [
        "\"test result:\"",
        "'test result:'",
        "\"error\"",
        "'error'",
        "\"failed\"",
        "'failed'",
        "\"panic\"",
        "'panic'",
        "\"warning\"",
        "'warning'",
    ]
    .iter()
    .any(|needle| lower.contains(needle) || normalized.contains(needle))
}

fn has_headroom_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "headroom_compress",
        "headroom_retrieve",
        "headroom_stats",
        "set_headroom_compression",
        "headroom-eval",
        "headroom-fixture",
        "omegon-headroom",
        "test(headroom):",
        "fix(headroom):",
        "feat(headroom):",
        "chore(headroom):",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_path_with_line_number(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| matches!(c, ',' | ')' | '(' | '[' | ']'));
        token.contains('/')
            && token.contains(':')
            && token
                .rsplit_once(':')
                .is_some_and(|(_, suffix)| suffix.chars().all(|c| c.is_ascii_digit()))
    })
}

fn has_hash_like_token(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_hexdigit())
        .any(|token| token.len() >= 12 && token.chars().all(|c| c.is_ascii_hexdigit()))
}

fn has_nonzero_exit_code(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ["exit code", "exit_code", "status code", "status_code"]
        .iter()
        .any(|needle| lower.contains(needle))
        && lower
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|part| part.parse::<u64>().ok())
            .any(|value| value != 0)
}

fn has_test_count_summary(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("passed")
        && (lower.contains("failed") || lower.contains("failure") || lower.contains("ignored"))
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
        || lower.contains("timeout")
        || lower.contains("decision")
        || lower.contains("blocked")
        || lower.contains("critical")
        || lower.contains("todo")
        || lower.contains("fixme")
        || lower.contains("exit code")
        || lower.contains("exit_code")
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

fn json_signal_items(items: &[serde_json::Value], limit: usize) -> Vec<&serde_json::Value> {
    const SCAN_LIMIT: usize = 10_000;
    items
        .iter()
        .take(SCAN_LIMIT)
        .filter(|item| json_value_has_signal(item))
        .take(limit)
        .collect()
}

fn json_value_has_signal(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            is_json_signal_key(key)
                || json_value_has_signal_for_key(key, value)
                || json_value_has_signal(value)
        }),
        serde_json::Value::Array(items) => items.iter().any(json_value_has_signal),
        serde_json::Value::String(value) => is_signal_line(value) || is_json_signal_value(value),
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Null => {
            false
        }
    }
}

fn json_value_has_signal_for_key(key: &str, value: &serde_json::Value) -> bool {
    let key = key.to_ascii_lowercase();
    match value {
        serde_json::Value::String(value) => {
            matches!(
                key.as_str(),
                "status" | "severity" | "risk" | "priority" | "state" | "result"
            ) && is_json_signal_value(value)
        }
        serde_json::Value::Number(value) => {
            matches!(
                key.as_str(),
                "exit_code" | "error_code" | "failures" | "failed"
            ) && value.as_i64().is_some_and(|n| n != 0)
        }
        serde_json::Value::Bool(value) => {
            matches!(
                key.as_str(),
                "failed" | "stale" | "dirty" | "changed" | "blocked" | "critical"
            ) && *value
        }
        _ => false,
    }
}

fn is_json_signal_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "error"
            | "errors"
            | "warning"
            | "warnings"
            | "panic"
            | "fatal"
            | "failure"
            | "failures"
            | "failed"
    )
}

fn is_json_signal_value(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "blocked"
            | "critical"
            | "high"
            | "fatal"
            | "failed"
            | "failure"
            | "error"
            | "warning"
            | "warn"
            | "panic"
            | "data_loss"
    ) || is_signal_line(value)
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

    fn large_text(label: &str, lines: usize) -> String {
        (0..lines)
            .map(|i| format!("{label} info line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
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
    fn protected_anchors_are_emitted_for_plain_text() {
        let mut store = InMemoryHeadroomStore::default();
        let mut lines = (0..200)
            .map(|i| format!("routine line {i}"))
            .collect::<Vec<_>>();
        lines.insert(
            100,
            "DECISION: keep compression default-off until dogfood benchmarks pass".into(),
        );
        let output = store.compress(CompressionInput {
            kind_hint: Some(ContentKind::PlainText),
            source: "plain".into(),
            text: lines.join("\n"),
            policy: policy(),
        });
        assert!(output.compressed);
        assert!(output.text.contains("protected_anchors:"));
        assert!(output.text.contains("keep compression default-off"));
    }

    #[test]
    fn protected_anchors_capture_paths_and_exit_codes() {
        let mut store = InMemoryHeadroomStore::default();
        let mut lines = (0..200)
            .map(|i| format!("info line {i}"))
            .collect::<Vec<_>>();
        lines.insert(80, "src/main.rs:42: failed assertion".into());
        lines.insert(81, "process exited with exit code 101".into());
        let output = store.compress(CompressionInput {
            kind_hint: Some(ContentKind::Log),
            source: "log".into(),
            text: lines.join("\n"),
            policy: policy(),
        });
        assert!(output.text.contains("src/main.rs:42"));
        assert!(output.text.contains("exit code 101"));
    }

    #[test]
    fn scored_anchors_prioritize_headroom_test_and_cli_tokens() {
        let text = [
            "test result: ok. 84 passed; 0 failed; 0 ignored; 0 measured; 2742 filtered out; finished in 0.18s",
            "test tools::tests::ordinary_read_test ... ok",
            "test tools::tests::read_compression_respects_headroom_mode_and_shared_store ... ok",
            "cargo run -p omegon-headroom --bin headroom-eval -- --fixtures DIR",
            "headroom_compress stores originals for headroom_retrieve",
            "+    fn scored_anchors_prioritize_headroom_test_and_cli_tokens() {",
        ]
        .join("\n");

        let anchors = scored_anchor_lines(&text);
        assert_eq!(
            anchors.first().map(|(_, _, line)| *line),
            Some(
                "test tools::tests::read_compression_respects_headroom_mode_and_shared_store ... ok"
            )
        );
        assert!(
            anchors
                .iter()
                .any(|(_, _, line)| line.contains("--fixtures"))
        );
        assert!(
            anchors
                .iter()
                .any(|(_, _, line)| line.contains("headroom_compress"))
        );
        assert!(anchors.iter().any(|(_, _, line)| {
            line.contains("scored_anchors_prioritize_headroom_test_and_cli_tokens")
        }));
    }

    #[test]
    fn store_evicts_oldest_by_object_limit() {
        let mut store = InMemoryHeadroomStore::with_policy(HeadroomStorePolicy {
            max_objects: 1,
            max_original_bytes: 1024 * 1024,
        });
        let first = large_text("first", 300);
        let second = large_text("second", 300);
        let first_ref = store
            .compress(CompressionInput {
                kind_hint: Some(ContentKind::Log),
                source: "first".into(),
                text: first,
                policy: policy(),
            })
            .original_ref
            .expect("first compressed");
        let second_ref = store
            .compress(CompressionInput {
                kind_hint: Some(ContentKind::Log),
                source: "second".into(),
                text: second,
                policy: policy(),
            })
            .original_ref
            .expect("second compressed");

        assert_eq!(store.len(), 1);
        assert_eq!(store.evicted_count(), 1);
        assert!(store.retrieve(&first_ref.id).is_err());
        assert!(store.retrieve(&second_ref.id).is_ok());
    }

    #[test]
    fn store_evicts_until_under_byte_limit() {
        let mut store = InMemoryHeadroomStore::with_policy(HeadroomStorePolicy {
            max_objects: 10,
            max_original_bytes: 2_000,
        });
        for i in 0..3 {
            let output = store.compress(CompressionInput {
                kind_hint: Some(ContentKind::Log),
                source: format!("item-{i}"),
                text: large_text(&format!("item-{i}"), 220),
                policy: policy(),
            });
            assert!(output.compressed);
        }

        let stats = store.stats();
        assert!(stats.original_bytes <= stats.max_original_bytes);
        assert!(stats.evicted_count >= 1);
    }

    #[test]
    fn store_deduplicates_same_original() {
        let mut store = InMemoryHeadroomStore::default();
        let text = large_text("duplicate", 300);
        let first = store.compress(CompressionInput {
            kind_hint: Some(ContentKind::Log),
            source: "a".into(),
            text: text.clone(),
            policy: policy(),
        });
        let second = store.compress(CompressionInput {
            kind_hint: Some(ContentKind::Log),
            source: "b".into(),
            text,
            policy: policy(),
        });
        assert_eq!(first.original_ref, second.original_ref);
        assert_eq!(store.len(), 1);
        assert_eq!(store.evicted_count(), 0);
    }

    #[test]
    fn native_provider_identity_is_stable() {
        let provider = native_deterministic_provider();
        assert_eq!(provider.id, "native_deterministic");
        assert_eq!(provider.kind, "deterministic");
        assert_eq!(provider.version, env!("CARGO_PKG_VERSION"));
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

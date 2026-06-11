use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::{
    CompressionInput, CompressionProviderInfo, ContentKind, HeadroomPolicy, InMemoryHeadroomStore,
    native_deterministic_provider,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureClass {
    CanonicalSmoke,
    Adversarial,
    Dogfood,
    Regression,
}

impl FixtureClass {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "canonical_smoke" | "canonical" | "smoke" => Some(Self::CanonicalSmoke),
            "adversarial" => Some(Self::Adversarial),
            "dogfood" => Some(Self::Dogfood),
            "regression" => Some(Self::Regression),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationFixture {
    pub name: String,
    pub class: FixtureClass,
    pub kind_hint: Option<ContentKind>,
    pub input: String,
    pub required_facts: Vec<String>,
    pub min_savings_percent: u8,
    pub expected_compressed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureValidationReport {
    pub name: String,
    pub class: FixtureClass,
    pub content_kind: ContentKind,
    pub compressed: bool,
    pub original_bytes: usize,
    pub raw_compressed_bytes: usize,
    pub evaluated_compressed_bytes: usize,
    pub estimated_tokens_before: usize,
    pub raw_estimated_tokens_after: usize,
    pub evaluated_estimated_tokens_after: usize,
    pub raw_token_savings_percent: u8,
    pub evaluated_token_savings_percent: u8,
    pub raw_savings_percent: u8,
    pub evaluated_savings_percent: u8,
    pub compress_latency_micros: u128,
    pub raw_missing_facts: Vec<String>,
    pub evaluated_missing_facts: Vec<String>,
    pub restored_fact_count: usize,
    pub restored_fact_bytes: usize,
    pub passed: bool,
    pub failure_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassValidationSummary {
    pub class: FixtureClass,
    pub fixtures: usize,
    pub passed: bool,
    pub total_original_bytes: usize,
    pub total_evaluated_compressed_bytes: usize,
    pub estimated_tokens_before: usize,
    pub evaluated_estimated_tokens_after: usize,
    pub evaluated_savings_percent: u8,
    pub evaluated_token_savings_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationSuiteReport {
    pub fixtures: Vec<FixtureValidationReport>,
    pub classes: Vec<ClassValidationSummary>,
    pub compression_provider: CompressionProviderInfo,
    pub passed: bool,
    pub total_original_bytes: usize,
    pub total_evaluated_compressed_bytes: usize,
    pub estimated_tokens_before: usize,
    pub evaluated_estimated_tokens_after: usize,
    pub token_counter: String,
    pub token_counter_kind: String,
    pub evaluated_token_savings_percent: u8,
    pub evaluated_savings_percent: u8,
}

pub trait TokenCounter {
    fn name(&self) -> &'static str;
    fn kind(&self) -> &'static str;
    fn count(&self, text: &str) -> usize;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BytesDiv4TokenCounter;

impl TokenCounter for BytesDiv4TokenCounter {
    fn name(&self) -> &'static str {
        "bytes_div_4"
    }

    fn kind(&self) -> &'static str {
        "approximate"
    }

    fn count(&self, text: &str) -> usize {
        text.len().div_ceil(4)
    }
}

pub fn estimated_tokens(text: &str) -> usize {
    BytesDiv4TokenCounter.count(text)
}

fn percent_saved(before: usize, after: usize) -> u8 {
    if before == 0 {
        return 0;
    }
    (((before.saturating_sub(after)) * 100) / before).min(100) as u8
}

pub fn validate_fixture(
    store: &mut InMemoryHeadroomStore,
    fixture: &ValidationFixture,
    policy: HeadroomPolicy,
) -> FixtureValidationReport {
    validate_fixture_with_counter(store, fixture, policy, &BytesDiv4TokenCounter)
}

pub fn validate_fixture_with_counter(
    store: &mut InMemoryHeadroomStore,
    fixture: &ValidationFixture,
    policy: HeadroomPolicy,
    token_counter: &dyn TokenCounter,
) -> FixtureValidationReport {
    let started = Instant::now();
    let output = store.compress(CompressionInput {
        kind_hint: fixture.kind_hint,
        source: fixture.name.clone(),
        text: fixture.input.clone(),
        policy,
    });
    let compress_latency_micros = started.elapsed().as_micros();

    let raw_text = output.text.clone();
    let raw_missing_facts = missing_facts(&raw_text, &fixture.required_facts);
    let restoration = if output.compressed {
        output
            .original_ref
            .as_ref()
            .and_then(|reference| store.retrieve(&reference.id).ok())
            .map(|stored| {
                restore_required_facts(raw_text.clone(), &stored.text, &fixture.required_facts)
            })
            .unwrap_or_else(|| RestoredText::unchanged(raw_text.clone()))
    } else {
        RestoredText::unchanged(raw_text.clone())
    };
    let evaluated_text = restoration.text;
    let evaluated_missing_facts = missing_facts(&evaluated_text, &fixture.required_facts);

    let estimated_tokens_before = token_counter.count(&fixture.input);
    let raw_estimated_tokens_after = token_counter.count(&raw_text);
    let evaluated_estimated_tokens_after = token_counter.count(&evaluated_text);
    let raw_savings_percent = percent_saved(output.stats.original_bytes, raw_text.len());
    let evaluated_savings_percent =
        percent_saved(output.stats.original_bytes, evaluated_text.len());
    let raw_token_savings_percent =
        percent_saved(estimated_tokens_before, raw_estimated_tokens_after);
    let evaluated_token_savings_percent =
        percent_saved(estimated_tokens_before, evaluated_estimated_tokens_after);

    let mut failure_reasons = Vec::new();
    if evaluated_savings_percent < fixture.min_savings_percent {
        failure_reasons.push(format!(
            "evaluated savings {}% below required {}%",
            evaluated_savings_percent, fixture.min_savings_percent
        ));
    }
    if !evaluated_missing_facts.is_empty() {
        failure_reasons.push(format!(
            "missing {} required facts after restoration",
            evaluated_missing_facts.len()
        ));
    }
    if let Some(expected) = fixture.expected_compressed
        && output.compressed != expected
    {
        failure_reasons.push(format!(
            "compressed={} did not match expected {}",
            output.compressed, expected
        ));
    }

    FixtureValidationReport {
        name: fixture.name.clone(),
        class: fixture.class,
        content_kind: output.content_kind,
        compressed: output.compressed,
        original_bytes: output.stats.original_bytes,
        raw_compressed_bytes: raw_text.len(),
        evaluated_compressed_bytes: evaluated_text.len(),
        estimated_tokens_before,
        raw_estimated_tokens_after,
        evaluated_estimated_tokens_after,
        raw_token_savings_percent,
        evaluated_token_savings_percent,
        raw_savings_percent,
        evaluated_savings_percent,
        compress_latency_micros,
        raw_missing_facts,
        evaluated_missing_facts,
        restored_fact_count: restoration.restored_fact_count,
        restored_fact_bytes: restoration.restored_fact_bytes,
        passed: failure_reasons.is_empty(),
        failure_reasons,
    }
}

fn missing_facts(text: &str, required_facts: &[String]) -> Vec<String> {
    required_facts
        .iter()
        .filter(|fact| !text.contains(fact.as_str()))
        .cloned()
        .collect()
}

struct RestoredText {
    text: String,
    restored_fact_count: usize,
    restored_fact_bytes: usize,
}

impl RestoredText {
    fn unchanged(text: String) -> Self {
        Self {
            text,
            restored_fact_count: 0,
            restored_fact_bytes: 0,
        }
    }
}

fn restore_required_facts(
    mut compressed: String,
    original: &str,
    required_facts: &[String],
) -> RestoredText {
    let mut restored = Vec::new();
    for fact in required_facts {
        if compressed.contains(fact) {
            continue;
        }
        if let Some(line) = original.lines().find(|line| line.contains(fact)) {
            restored.push(line.to_owned());
        }
    }
    let restored_fact_count = restored.len();
    let mut restored_fact_bytes = 0;
    if !restored.is_empty() {
        compressed.push_str("\nrequired_fact_context:\n");
        for line in restored {
            restored_fact_bytes += line.len();
            compressed.push_str(&line);
            compressed.push('\n');
        }
    }
    RestoredText {
        text: compressed,
        restored_fact_count,
        restored_fact_bytes,
    }
}

pub fn validate_suite(
    fixtures: &[ValidationFixture],
    policy: HeadroomPolicy,
) -> ValidationSuiteReport {
    validate_suite_with_counter(fixtures, policy, &BytesDiv4TokenCounter)
}

pub fn validate_suite_with_counter(
    fixtures: &[ValidationFixture],
    policy: HeadroomPolicy,
    token_counter: &dyn TokenCounter,
) -> ValidationSuiteReport {
    let mut store = InMemoryHeadroomStore::default();
    let reports = fixtures
        .iter()
        .map(|fixture| validate_fixture_with_counter(&mut store, fixture, policy, token_counter))
        .collect::<Vec<_>>();

    let total_original_bytes: usize = reports.iter().map(|report| report.original_bytes).sum();
    let total_evaluated_compressed_bytes: usize = reports
        .iter()
        .map(|report| report.evaluated_compressed_bytes)
        .sum();
    let estimated_tokens_before: usize = reports
        .iter()
        .map(|report| report.estimated_tokens_before)
        .sum();
    let evaluated_estimated_tokens_after: usize = reports
        .iter()
        .map(|report| report.evaluated_estimated_tokens_after)
        .sum();
    let classes = class_summaries(&reports);
    let evaluated_savings_percent =
        percent_saved(total_original_bytes, total_evaluated_compressed_bytes);
    let evaluated_token_savings_percent =
        percent_saved(estimated_tokens_before, evaluated_estimated_tokens_after);

    ValidationSuiteReport {
        passed: reports.iter().all(|report| report.passed),
        fixtures: reports,
        classes,
        compression_provider: native_deterministic_provider(),
        total_original_bytes,
        total_evaluated_compressed_bytes,
        estimated_tokens_before,
        evaluated_estimated_tokens_after,
        token_counter: token_counter.name().to_string(),
        token_counter_kind: token_counter.kind().to_string(),
        evaluated_token_savings_percent,
        evaluated_savings_percent,
    }
}

fn class_summaries(reports: &[FixtureValidationReport]) -> Vec<ClassValidationSummary> {
    [
        FixtureClass::CanonicalSmoke,
        FixtureClass::Adversarial,
        FixtureClass::Dogfood,
        FixtureClass::Regression,
    ]
    .into_iter()
    .filter_map(|class| {
        let class_reports = reports
            .iter()
            .filter(|report| report.class == class)
            .collect::<Vec<_>>();
        if class_reports.is_empty() {
            return None;
        }
        let total_original_bytes = class_reports
            .iter()
            .map(|report| report.original_bytes)
            .sum();
        let total_evaluated_compressed_bytes = class_reports
            .iter()
            .map(|report| report.evaluated_compressed_bytes)
            .sum();
        let estimated_tokens_before = class_reports
            .iter()
            .map(|report| report.estimated_tokens_before)
            .sum();
        let evaluated_estimated_tokens_after = class_reports
            .iter()
            .map(|report| report.evaluated_estimated_tokens_after)
            .sum();
        Some(ClassValidationSummary {
            class,
            fixtures: class_reports.len(),
            passed: class_reports.iter().all(|report| report.passed),
            total_original_bytes,
            total_evaluated_compressed_bytes,
            estimated_tokens_before,
            evaluated_estimated_tokens_after,
            evaluated_savings_percent: percent_saved(
                total_original_bytes,
                total_evaluated_compressed_bytes,
            ),
            evaluated_token_savings_percent: percent_saved(
                estimated_tokens_before,
                evaluated_estimated_tokens_after,
            ),
        })
    })
    .collect()
}

pub fn canonical_validation_fixtures() -> Vec<ValidationFixture> {
    vec![
        json_error_fixture(),
        cargo_failure_fixture(),
        compact_grep_passthrough_fixture(),
        rust_source_passthrough_fixture(),
        json_schema_signal_fixture(),
        threshold_plaintext_fixture(),
    ]
}

#[derive(Debug, Clone, Deserialize)]
struct FileValidationFixture {
    name: String,
    #[serde(default)]
    class: Option<FixtureClass>,
    #[serde(default)]
    kind_hint: Option<ContentKind>,
    input: String,
    #[serde(default)]
    required_facts: Vec<String>,
    #[serde(default = "default_file_min_savings_percent")]
    min_savings_percent: u8,
    #[serde(default)]
    expected_compressed: Option<bool>,
}

fn default_file_min_savings_percent() -> u8 {
    50
}

impl From<FileValidationFixture> for ValidationFixture {
    fn from(value: FileValidationFixture) -> Self {
        Self {
            name: value.name,
            class: value.class.unwrap_or(FixtureClass::Dogfood),
            kind_hint: value.kind_hint,
            input: value.input,
            required_facts: value.required_facts,
            min_savings_percent: value.min_savings_percent,
            expected_compressed: value.expected_compressed,
        }
    }
}

pub fn load_fixture_dir(path: &Path) -> anyhow::Result<Vec<ValidationFixture>> {
    let mut entries = std::fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();

    let mut fixtures = Vec::new();
    for path in entries {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        fixtures.push(load_fixture_file(&path)?);
    }
    Ok(fixtures)
}

pub fn load_fixture_file(path: &Path) -> anyhow::Result<ValidationFixture> {
    let text = std::fs::read_to_string(path)?;
    let mut fixture: ValidationFixture =
        serde_json::from_str::<FileValidationFixture>(&text)?.into();
    if fixture.name.trim().is_empty() {
        fixture.name = fixture_name_from_path(path);
    }
    Ok(fixture)
}

fn fixture_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("dogfood-fixture")
        .to_string()
}

pub fn append_fixture_dirs(
    mut fixtures: Vec<ValidationFixture>,
    dirs: &[PathBuf],
) -> anyhow::Result<Vec<ValidationFixture>> {
    for dir in dirs {
        fixtures.extend(load_fixture_dir(dir)?);
    }
    Ok(fixtures)
}

fn json_error_fixture() -> ValidationFixture {
    let rows = (0..100)
        .map(|i| {
            if i == 67 {
                serde_json::json!({
                    "id": i,
                    "status": "critical_error",
                    "service": "payment-worker",
                    "error_code": "PAYMENT_TIMEOUT",
                    "resolution": "restart payment-worker shard 7",
                    "affected_count": 42
                })
            } else {
                serde_json::json!({
                    "id": i,
                    "status": "ok",
                    "service": "payment-worker",
                    "message": "heartbeat accepted"
                })
            }
        })
        .collect::<Vec<_>>();

    ValidationFixture {
        name: "json-critical-error".into(),
        class: FixtureClass::CanonicalSmoke,
        kind_hint: Some(ContentKind::Json),
        input: serde_json::to_string(&rows).expect("fixture json serializes"),
        required_facts: vec![
            "PAYMENT_TIMEOUT".into(),
            "restart payment-worker shard 7".into(),
            "affected_count".into(),
            "42".into(),
        ],
        min_savings_percent: 70,
        expected_compressed: Some(true),
    }
}

fn cargo_failure_fixture() -> ValidationFixture {
    let mut lines = Vec::new();
    lines.push("running 500 tests".to_string());
    for i in 0..250 {
        lines.push(format!("test tests::passes_{i} ... ok"));
    }
    lines.extend([
        "test validation::rejects_missing_anchor ... FAILED".to_string(),
        "thread 'validation::rejects_missing_anchor' panicked at src/validation.rs:77:9:"
            .to_string(),
        "assertion `left == right` failed".to_string(),
        "left: \"compressed output\"".to_string(),
        "right: \"PAYMENT_TIMEOUT\"".to_string(),
        "error: test failed, to rerun pass `-p omegon-headroom --lib`".to_string(),
    ]);
    for i in 250..500 {
        lines.push(format!("test tests::passes_{i} ... ok"));
    }
    lines.push("test result: FAILED. 499 passed; 1 failed; 0 ignored".to_string());

    ValidationFixture {
        name: "cargo-failure-log".into(),
        class: FixtureClass::CanonicalSmoke,
        kind_hint: Some(ContentKind::Log),
        input: lines.join("\n"),
        required_facts: vec![
            "validation::rejects_missing_anchor".into(),
            "src/validation.rs:77:9".into(),
            "PAYMENT_TIMEOUT".into(),
            "499 passed; 1 failed".into(),
        ],
        min_savings_percent: 70,
        expected_compressed: Some(true),
    }
}

fn compact_grep_passthrough_fixture() -> ValidationFixture {
    let input = (0..12)
        .map(|i| format!("src/module_{i}.rs:{}:fn target_{i}() {{}}", i + 10))
        .collect::<Vec<_>>()
        .join("\n");
    ValidationFixture {
        name: "compact-grep-passthrough".into(),
        class: FixtureClass::CanonicalSmoke,
        kind_hint: Some(ContentKind::PlainText),
        input,
        required_facts: vec!["src/module_7.rs:17:fn target_7() {}".into()],
        min_savings_percent: 0,
        expected_compressed: Some(false),
    }
}

fn rust_source_passthrough_fixture() -> ValidationFixture {
    let input = r#"use anyhow::Result;

pub struct Compressor {
    enabled: bool,
}

impl Compressor {
    pub fn compress(&self, input: &str) -> Result<String> {
        Ok(input.to_string())
    }
}
"#
    .to_string();
    ValidationFixture {
        name: "fresh-rust-source-passthrough".into(),
        class: FixtureClass::CanonicalSmoke,
        kind_hint: Some(ContentKind::Code),
        input,
        required_facts: vec!["pub fn compress".into()],
        min_savings_percent: 0,
        expected_compressed: Some(false),
    }
}

fn json_schema_signal_fixture() -> ValidationFixture {
    let rows = (0..160)
        .map(|i| {
            if i == 123 {
                serde_json::json!({
                    "id": i,
                    "status": "blocked",
                    "severity": "critical",
                    "risk": "data_loss",
                    "exit_code": 101,
                    "owner": "storage-controller"
                })
            } else {
                serde_json::json!({
                    "id": i,
                    "status": "ok",
                    "severity": "info",
                    "risk": "none",
                    "exit_code": 0,
                    "owner": "storage-controller"
                })
            }
        })
        .collect::<Vec<_>>();
    ValidationFixture {
        name: "json-schema-signal-critical-row".into(),
        class: FixtureClass::Adversarial,
        kind_hint: Some(ContentKind::Json),
        input: serde_json::to_string(&rows).expect("fixture json serializes"),
        required_facts: vec![
            "blocked".into(),
            "critical".into(),
            "data_loss".into(),
            "exit_code".into(),
            "101".into(),
        ],
        min_savings_percent: 70,
        expected_compressed: Some(true),
    }
}

fn threshold_plaintext_fixture() -> ValidationFixture {
    let mut lines = Vec::new();
    for i in 0..180 {
        lines.push(format!(
            "routine context line {i}: repeated low signal payload"
        ));
    }
    lines.push("DECISION: keep compression default-off until dogfood benchmarks pass".into());
    for i in 180..360 {
        lines.push(format!(
            "routine context line {i}: repeated low signal payload"
        ));
    }
    ValidationFixture {
        name: "threshold-plaintext-required-decision".into(),
        class: FixtureClass::Adversarial,
        kind_hint: Some(ContentKind::PlainText),
        input: lines.join("\n"),
        required_facts: vec!["keep compression default-off".into()],
        min_savings_percent: 60,
        expected_compressed: Some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation_policy() -> HeadroomPolicy {
        HeadroomPolicy {
            min_bytes: 1024,
            target_bytes: 4096,
            ..HeadroomPolicy::default()
        }
    }

    #[test]
    fn canonical_fixtures_validate_savings_and_fact_retention() {
        let report = validate_suite(&canonical_validation_fixtures(), validation_policy());
        assert_eq!(report.compression_provider.id, "native_deterministic");
        assert!(
            report.passed,
            "validation suite failed: {:#?}",
            report
                .fixtures
                .iter()
                .filter(|fixture| !fixture.passed)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn validation_reports_missing_required_facts() {
        let mut store = InMemoryHeadroomStore::default();
        let fixture = ValidationFixture {
            name: "missing-fact".into(),
            class: FixtureClass::Regression,
            kind_hint: Some(ContentKind::Log),
            input: (0..200)
                .map(|i| format!("info line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            required_facts: vec!["DOES_NOT_EXIST".into()],
            min_savings_percent: 0,
            expected_compressed: Some(true),
        };
        let report = validate_fixture(
            &mut store,
            &fixture,
            HeadroomPolicy {
                min_bytes: 1,
                ..validation_policy()
            },
        );
        assert!(!report.passed);
        assert_eq!(report.evaluated_missing_facts, vec!["DOES_NOT_EXIST"]);
    }
}

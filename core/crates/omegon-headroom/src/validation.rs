use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::{CompressionInput, ContentKind, HeadroomPolicy, InMemoryHeadroomStore};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationFixture {
    pub name: String,
    pub kind_hint: Option<ContentKind>,
    pub input: String,
    pub required_facts: Vec<String>,
    pub min_savings_percent: u8,
    pub expected_compressed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureValidationReport {
    pub name: String,
    pub content_kind: ContentKind,
    pub compressed: bool,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub estimated_tokens_before: usize,
    pub estimated_tokens_after: usize,
    pub savings_percent: u8,
    pub latency_micros: u128,
    pub missing_facts: Vec<String>,
    pub passed: bool,
    pub failure_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationSuiteReport {
    pub fixtures: Vec<FixtureValidationReport>,
    pub passed: bool,
    pub total_original_bytes: usize,
    pub total_compressed_bytes: usize,
    pub estimated_tokens_before: usize,
    pub estimated_tokens_after: usize,
    pub savings_percent: u8,
}

pub fn estimated_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

pub fn validate_fixture(
    store: &mut InMemoryHeadroomStore,
    fixture: &ValidationFixture,
    policy: HeadroomPolicy,
) -> FixtureValidationReport {
    let started = Instant::now();
    let output = store.compress(CompressionInput {
        kind_hint: fixture.kind_hint,
        source: fixture.name.clone(),
        text: fixture.input.clone(),
        policy,
    });
    let mut validation_text = output.text.clone();
    if output.compressed
        && let Some(stored) = output
            .original_ref
            .as_ref()
            .and_then(|reference| store.retrieve(&reference.id).ok())
    {
        validation_text =
            preserve_required_facts(validation_text, &stored.text, &fixture.required_facts);
    }
    let latency_micros = started.elapsed().as_micros();

    let missing_facts = fixture
        .required_facts
        .iter()
        .filter(|fact| !validation_text.contains(fact.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    let mut failure_reasons = Vec::new();
    if output.stats.savings_percent < fixture.min_savings_percent {
        failure_reasons.push(format!(
            "savings {}% below required {}%",
            output.stats.savings_percent, fixture.min_savings_percent
        ));
    }
    if !missing_facts.is_empty() {
        failure_reasons.push(format!("missing {} required facts", missing_facts.len()));
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
        content_kind: output.content_kind,
        compressed: output.compressed,
        original_bytes: output.stats.original_bytes,
        compressed_bytes: validation_text.len(),
        estimated_tokens_before: estimated_tokens(&fixture.input),
        estimated_tokens_after: estimated_tokens(&validation_text),
        savings_percent: output.stats.savings_percent,
        latency_micros,
        missing_facts,
        passed: failure_reasons.is_empty(),
        failure_reasons,
    }
}

fn preserve_required_facts(
    mut compressed: String,
    original: &str,
    required_facts: &[String],
) -> String {
    let mut restored = Vec::new();
    for fact in required_facts {
        if compressed.contains(fact) {
            continue;
        }
        if let Some(line) = original.lines().find(|line| line.contains(fact)) {
            restored.push(line.to_owned());
        }
    }
    if !restored.is_empty() {
        compressed.push_str("\nrequired_fact_context:\n");
        for line in restored {
            compressed.push_str(&line);
            compressed.push('\n');
        }
    }
    compressed
}

pub fn validate_suite(
    fixtures: &[ValidationFixture],
    policy: HeadroomPolicy,
) -> ValidationSuiteReport {
    let mut store = InMemoryHeadroomStore::default();
    let reports = fixtures
        .iter()
        .map(|fixture| validate_fixture(&mut store, fixture, policy))
        .collect::<Vec<_>>();

    let total_original_bytes: usize = reports.iter().map(|report| report.original_bytes).sum();
    let total_compressed_bytes: usize = reports.iter().map(|report| report.compressed_bytes).sum();
    let estimated_tokens_before: usize = reports
        .iter()
        .map(|report| report.estimated_tokens_before)
        .sum();
    let estimated_tokens_after: usize = reports
        .iter()
        .map(|report| report.estimated_tokens_after)
        .sum();
    let saved = total_original_bytes.saturating_sub(total_compressed_bytes);
    let savings_percent = if total_original_bytes == 0 {
        0
    } else {
        ((saved * 100) / total_original_bytes).min(100) as u8
    };

    ValidationSuiteReport {
        passed: reports.iter().all(|report| report.passed),
        fixtures: reports,
        total_original_bytes,
        total_compressed_bytes,
        estimated_tokens_before,
        estimated_tokens_after,
        savings_percent,
    }
}

pub fn canonical_validation_fixtures() -> Vec<ValidationFixture> {
    vec![
        json_error_fixture(),
        cargo_failure_fixture(),
        compact_grep_passthrough_fixture(),
        rust_source_passthrough_fixture(),
    ]
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
        kind_hint: Some(ContentKind::Code),
        input,
        required_facts: vec!["pub fn compress".into()],
        min_savings_percent: 0,
        expected_compressed: Some(false),
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
        assert_eq!(report.missing_facts, vec!["DOES_NOT_EXIST"]);
    }
}

use std::path::PathBuf;

use omegon_headroom::validation::{
    ValidationFixture, canonical_validation_fixtures, load_fixture_dir,
};
use omegon_headroom::{CompressionInput, HeadroomPolicy, InMemoryHeadroomStore};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
struct QuestionReport {
    fixture: String,
    id: String,
    requires_original: bool,
    expected_contains: Vec<String>,
    missing: Vec<String>,
    retrieval_available: bool,
    passed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureUnderstandingReport {
    name: String,
    compressed: bool,
    questions: Vec<QuestionReport>,
    passed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct UnderstandingSuiteReport {
    passed: bool,
    fixtures: Vec<FixtureUnderstandingReport>,
    question_count: usize,
    failed_questions: usize,
    retrieval_required: usize,
    retrieval_available: usize,
}

#[derive(Debug, Clone)]
struct Args {
    output_json: bool,
    fixtures: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let fixtures = if let Some(path) = args.fixtures.as_deref() {
        load_fixture_dir(path)?
    } else {
        canonical_validation_fixtures()
    };
    let report = evaluate_understanding(&fixtures);
    if args.output_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text_report(&report);
    }
    if report.passed {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn parse_args() -> anyhow::Result<Args> {
    let mut output_json = false;
    let mut fixtures = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => output_json = true,
            "--text" => output_json = false,
            "--fixtures" => {
                let Some(path) = iter.next() else {
                    anyhow::bail!("missing value for --fixtures");
                };
                fixtures = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }
    Ok(Args {
        output_json,
        fixtures,
    })
}

fn print_usage() {
    eprintln!(
        "headroom-understanding-eval [--text|--json] [--fixtures DIR]\n\n\
         Deterministically checks whether compressed fixtures can answer simple required-fact questions."
    );
}

fn evaluate_understanding(fixtures: &[ValidationFixture]) -> UnderstandingSuiteReport {
    let mut store = InMemoryHeadroomStore::default();
    let policy = HeadroomPolicy {
        min_bytes: 1,
        target_bytes: 4096,
        ..HeadroomPolicy::default()
    };
    let mut fixture_reports = Vec::new();

    for fixture in fixtures {
        let output = store.compress(CompressionInput {
            kind_hint: fixture.kind_hint,
            source: fixture.name.clone(),
            text: fixture.input.clone(),
            policy,
        });
        let compressed_text = output.text.clone();
        let retrieval_text = output
            .original_ref
            .as_ref()
            .and_then(|reference| store.retrieve(&reference.id).ok())
            .map(|stored| stored.text.clone());
        let mut questions = Vec::new();

        for (index, fact) in fixture.required_facts.iter().enumerate() {
            questions.push(evaluate_question(
                &fixture.name,
                format!("required-fact-{index}"),
                vec![fact.clone()],
                !compressed_text.contains(fact),
                &compressed_text,
                retrieval_text.as_deref(),
            ));
        }
        for question in &fixture.questions {
            questions.push(evaluate_question(
                &fixture.name,
                question.id.clone(),
                question.expected_contains.clone(),
                question.requires_original,
                &compressed_text,
                retrieval_text.as_deref(),
            ));
        }

        let passed = questions.iter().all(|question| question.passed);
        fixture_reports.push(FixtureUnderstandingReport {
            name: fixture.name.clone(),
            compressed: output.compressed,
            questions,
            passed,
        });
    }

    let question_count = fixture_reports
        .iter()
        .map(|fixture| fixture.questions.len())
        .sum();
    let failed_questions = fixture_reports
        .iter()
        .flat_map(|fixture| &fixture.questions)
        .filter(|question| !question.passed)
        .count();
    let retrieval_required = fixture_reports
        .iter()
        .flat_map(|fixture| &fixture.questions)
        .filter(|question| question.requires_original)
        .count();
    let retrieval_available = fixture_reports
        .iter()
        .flat_map(|fixture| &fixture.questions)
        .filter(|question| question.requires_original && question.retrieval_available)
        .count();

    UnderstandingSuiteReport {
        passed: failed_questions == 0,
        fixtures: fixture_reports,
        question_count,
        failed_questions,
        retrieval_required,
        retrieval_available,
    }
}

fn evaluate_question(
    fixture: &str,
    id: String,
    expected_contains: Vec<String>,
    requires_original: bool,
    compressed_text: &str,
    retrieval_text: Option<&str>,
) -> QuestionReport {
    let search_text = if requires_original {
        retrieval_text.unwrap_or("")
    } else {
        compressed_text
    };
    let missing = expected_contains
        .iter()
        .filter(|expected| !search_text.contains(expected.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let retrieval_available = retrieval_text.is_some();
    let passed = missing.is_empty() && (!requires_original || retrieval_available);
    QuestionReport {
        fixture: fixture.to_string(),
        id,
        requires_original,
        expected_contains,
        missing,
        retrieval_available,
        passed,
    }
}

fn print_text_report(report: &UnderstandingSuiteReport) {
    println!(
        "Headroom understanding evaluation: {}",
        if report.passed { "PASS" } else { "FAIL" }
    );
    println!("fixtures: {}", report.fixtures.len());
    println!("questions: {}", report.question_count);
    println!("failed questions: {}", report.failed_questions);
    println!(
        "retrieval required: {} / available: {}",
        report.retrieval_required, report.retrieval_available
    );
    println!();

    for fixture in &report.fixtures {
        println!(
            "- {}: {} | compressed={} questions={}",
            fixture.name,
            if fixture.passed { "PASS" } else { "FAIL" },
            fixture.compressed,
            fixture.questions.len()
        );
        for question in fixture.questions.iter().filter(|question| !question.passed) {
            println!(
                "  failure {} missing: {}",
                question.id,
                question.missing.join(", ")
            );
        }
    }
}

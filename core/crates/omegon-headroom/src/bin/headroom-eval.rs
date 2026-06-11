use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use omegon_headroom::HeadroomPolicy;
use omegon_headroom::validation::{
    ValidationSuiteReport, append_fixture_dirs, canonical_validation_fixtures, validate_suite,
};

fn main() -> ExitCode {
    let mut format = OutputFormat::Text;
    let mut min_bytes = 1024;
    let mut target_bytes = 4096;
    let mut save_path: Option<PathBuf> = None;
    let mut compare_path: Option<PathBuf> = None;
    let mut fixture_dirs: Vec<PathBuf> = Vec::new();
    let mut max_restored_facts: Option<usize> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => format = OutputFormat::Json,
            "--text" => format = OutputFormat::Text,
            "--min-bytes" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value for --min-bytes");
                    return ExitCode::from(2);
                };
                min_bytes = match value.parse() {
                    Ok(value) => value,
                    Err(_) => {
                        eprintln!("invalid --min-bytes value: {value}");
                        return ExitCode::from(2);
                    }
                };
            }
            "--target-bytes" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value for --target-bytes");
                    return ExitCode::from(2);
                };
                target_bytes = match value.parse() {
                    Ok(value) => value,
                    Err(_) => {
                        eprintln!("invalid --target-bytes value: {value}");
                        return ExitCode::from(2);
                    }
                };
            }
            "--fixtures" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value for --fixtures");
                    return ExitCode::from(2);
                };
                fixture_dirs.push(PathBuf::from(value));
            }
            "--max-restored-facts" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value for --max-restored-facts");
                    return ExitCode::from(2);
                };
                max_restored_facts = match value.parse() {
                    Ok(value) => Some(value),
                    Err(_) => {
                        eprintln!("invalid --max-restored-facts value: {value}");
                        return ExitCode::from(2);
                    }
                };
            }
            "--save" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value for --save");
                    return ExitCode::from(2);
                };
                save_path = Some(PathBuf::from(value));
            }
            "--compare" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value for --compare");
                    return ExitCode::from(2);
                };
                compare_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_help();
                return ExitCode::from(2);
            }
        }
    }

    let fixtures = match append_fixture_dirs(canonical_validation_fixtures(), &fixture_dirs) {
        Ok(fixtures) => fixtures,
        Err(err) => {
            eprintln!("failed to load fixture directory: {err}");
            return ExitCode::from(2);
        }
    };

    let report = validate_suite(
        &fixtures,
        HeadroomPolicy {
            min_bytes,
            target_bytes,
            ..HeadroomPolicy::default()
        },
    );

    if let Some(path) = save_path.as_ref()
        && let Err(err) = save_report(path, &report)
    {
        eprintln!("failed to save report to {}: {err}", path.display());
        return ExitCode::from(2);
    }

    let restoration_gate = max_restored_facts.map(|max| restoration_gate_report(&report, max));

    let comparison = match compare_path.as_ref() {
        Some(path) => match compare_report(path, &report) {
            Ok(comparison) => Some(comparison),
            Err(err) => {
                eprintln!("failed to compare against {}: {err}", path.display());
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "report": report,
                "comparison": comparison,
                "restoration_gate": restoration_gate,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&output).expect("report serializes")
            );
        }
        OutputFormat::Text => {
            print_text_report(&report, comparison.as_ref(), restoration_gate.as_ref())
        }
    }

    let comparison_passed = comparison
        .as_ref()
        .is_none_or(|comparison| comparison.passed);
    let restoration_gate_passed = restoration_gate.as_ref().is_none_or(|gate| gate.passed);
    if report.passed && comparison_passed && restoration_gate_passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, serde::Serialize)]
struct RestorationGateReport {
    passed: bool,
    max_restored_facts: usize,
    actual_restored_facts: usize,
    over_budget: usize,
}

#[derive(Debug, serde::Serialize)]
struct ComparisonReport {
    passed: bool,
    baseline_path: String,
    baseline_passed: bool,
    current_passed: bool,
    baseline_evaluated_savings_percent: u8,
    current_evaluated_savings_percent: u8,
    baseline_evaluated_token_savings_percent: u8,
    current_evaluated_token_savings_percent: u8,
    baseline_restored_fact_count: usize,
    current_restored_fact_count: usize,
    regressions: Vec<String>,
}

fn print_help() {
    println!(
        "headroom-eval [--text|--json] [--min-bytes N] [--target-bytes N] [--fixtures DIR] [--max-restored-facts N] [--save PATH] [--compare PATH]\n\n\
         Runs native headroom evaluation fixtures. Exits non-zero if evaluated savings,\n\
         fact retention, or expected compression behavior regress. JSON output is intended\n\
         for CI snapshots and longitudinal benchmark comparison. --fixtures loads additional\n\
         dogfood/regression JSON fixtures from a directory. --save writes the current report as\n\
         JSON; --compare checks the current report against a saved baseline. --max-restored-facts fails when evaluator restoration exceeds the given budget."
    );
}

fn save_report(path: &PathBuf, report: &ValidationSuiteReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(report)?;
    fs::write(path, format!("{json}\n"))?;
    Ok(())
}

fn restoration_gate_report(
    report: &ValidationSuiteReport,
    max_restored_facts: usize,
) -> RestorationGateReport {
    let actual_restored_facts = total_restored_facts(report);
    RestorationGateReport {
        passed: actual_restored_facts <= max_restored_facts,
        max_restored_facts,
        actual_restored_facts,
        over_budget: actual_restored_facts.saturating_sub(max_restored_facts),
    }
}

fn compare_report(
    path: &PathBuf,
    current: &ValidationSuiteReport,
) -> anyhow::Result<ComparisonReport> {
    let baseline_text = fs::read_to_string(path)?;
    let baseline: ValidationSuiteReport = serde_json::from_str(&baseline_text)?;
    let mut regressions = Vec::new();

    if !current.passed {
        regressions.push("current report failed validation".to_string());
    }
    if current.evaluated_savings_percent < baseline.evaluated_savings_percent {
        regressions.push(format!(
            "evaluated byte savings dropped from {}% to {}%",
            baseline.evaluated_savings_percent, current.evaluated_savings_percent
        ));
    }
    if current.evaluated_token_savings_percent < baseline.evaluated_token_savings_percent {
        regressions.push(format!(
            "evaluated token savings dropped from {}% to {}%",
            baseline.evaluated_token_savings_percent, current.evaluated_token_savings_percent
        ));
    }
    let baseline_restored = total_restored_facts(&baseline);
    let current_restored = total_restored_facts(current);
    if current_restored > baseline_restored {
        regressions.push(format!(
            "restored fact count increased from {baseline_restored} to {current_restored}"
        ));
    }

    Ok(ComparisonReport {
        passed: regressions.is_empty(),
        baseline_path: path.display().to_string(),
        baseline_passed: baseline.passed,
        current_passed: current.passed,
        baseline_evaluated_savings_percent: baseline.evaluated_savings_percent,
        current_evaluated_savings_percent: current.evaluated_savings_percent,
        baseline_evaluated_token_savings_percent: baseline.evaluated_token_savings_percent,
        current_evaluated_token_savings_percent: current.evaluated_token_savings_percent,
        baseline_restored_fact_count: baseline_restored,
        current_restored_fact_count: current_restored,
        regressions,
    })
}

fn total_restored_facts(report: &ValidationSuiteReport) -> usize {
    report
        .fixtures
        .iter()
        .map(|fixture| fixture.restored_fact_count)
        .sum()
}

fn print_text_report(
    report: &ValidationSuiteReport,
    comparison: Option<&ComparisonReport>,
    restoration_gate: Option<&RestorationGateReport>,
) {
    println!(
        "Headroom evaluation: {}",
        if report.passed { "PASS" } else { "FAIL" }
    );
    println!("fixtures: {}", report.fixtures.len());
    println!(
        "evaluated bytes: {} -> {} ({}% saved)",
        report.total_original_bytes,
        report.total_evaluated_compressed_bytes,
        report.evaluated_savings_percent
    );
    println!(
        "estimated tokens ({}/{}): {} -> {} ({}% saved)",
        report.token_counter,
        report.token_counter_kind,
        report.estimated_tokens_before,
        report.evaluated_estimated_tokens_after,
        report.evaluated_token_savings_percent
    );
    if let Some(gate) = restoration_gate {
        println!(
            "restoration gate: {} (actual {} / max {})",
            if gate.passed { "PASS" } else { "FAIL" },
            gate.actual_restored_facts,
            gate.max_restored_facts
        );
    }
    if let Some(comparison) = comparison {
        println!(
            "comparison: {} against {}",
            if comparison.passed { "PASS" } else { "FAIL" },
            comparison.baseline_path
        );
        for regression in &comparison.regressions {
            println!("  regression: {regression}");
        }
    }
    println!();

    for class in &report.classes {
        println!(
            "class {:?}: {} | fixtures {} | bytes {} -> {} ({}% saved), tokens {} -> {} ({}% saved)",
            class.class,
            if class.passed { "PASS" } else { "FAIL" },
            class.fixtures,
            class.total_original_bytes,
            class.total_evaluated_compressed_bytes,
            class.evaluated_savings_percent,
            class.estimated_tokens_before,
            class.evaluated_estimated_tokens_after,
            class.evaluated_token_savings_percent
        );
    }
    println!();

    for fixture in &report.fixtures {
        println!(
            "- {} [{:?}/{:?}]: {} | raw bytes {} -> {} ({}% saved), evaluated bytes -> {} ({}% saved), tokens {} -> {} ({}% saved), compression {}µs",
            fixture.name,
            fixture.class,
            fixture.content_kind,
            if fixture.passed { "PASS" } else { "FAIL" },
            fixture.original_bytes,
            fixture.raw_compressed_bytes,
            fixture.raw_savings_percent,
            fixture.evaluated_compressed_bytes,
            fixture.evaluated_savings_percent,
            fixture.estimated_tokens_before,
            fixture.evaluated_estimated_tokens_after,
            fixture.evaluated_token_savings_percent,
            fixture.compress_latency_micros
        );
        if fixture.restored_fact_count > 0 {
            println!(
                "  restored facts: {} ({} bytes)",
                fixture.restored_fact_count, fixture.restored_fact_bytes
            );
        }
        if !fixture.raw_missing_facts.is_empty() {
            println!(
                "  raw missing facts: {}",
                fixture.raw_missing_facts.join(", ")
            );
        }
        if !fixture.evaluated_missing_facts.is_empty() {
            println!(
                "  evaluated missing facts: {}",
                fixture.evaluated_missing_facts.join(", ")
            );
        }
        for reason in &fixture.failure_reasons {
            println!("  failure: {reason}");
        }
    }
}

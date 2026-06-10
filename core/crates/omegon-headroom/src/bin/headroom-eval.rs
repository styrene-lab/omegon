use std::process::ExitCode;

use omegon_headroom::HeadroomPolicy;
use omegon_headroom::validation::{
    ValidationSuiteReport, canonical_validation_fixtures, validate_suite,
};

fn main() -> ExitCode {
    let mut format = OutputFormat::Text;
    let mut min_bytes = 1024;
    let mut target_bytes = 4096;

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

    let report = validate_suite(
        &canonical_validation_fixtures(),
        HeadroomPolicy {
            min_bytes,
            target_bytes,
            ..HeadroomPolicy::default()
        },
    );

    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("report serializes")
            );
        }
        OutputFormat::Text => print_text_report(&report),
    }

    if report.passed {
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

fn print_help() {
    println!(
        "headroom-eval [--text|--json] [--min-bytes N] [--target-bytes N]\n\n\
         Runs native headroom evaluation fixtures. Exits non-zero if evaluated savings,\n\
         fact retention, or expected compression behavior regress. JSON output is intended\n\
         for CI snapshots and longitudinal benchmark comparison."
    );
}

fn print_text_report(report: &ValidationSuiteReport) {
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

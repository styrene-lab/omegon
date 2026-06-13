use std::process::Command;

#[test]
fn native_provider_flag_succeeds_and_reports_identity() {
    let output = Command::new(env!("CARGO_BIN_EXE_headroom-eval"))
        .args(["--provider", "native_deterministic", "--text"])
        .output()
        .expect("headroom-eval runs");

    assert!(
        output.status.success(),
        "expected success, status={:?}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("provider: native_deterministic"),
        "stdout did not report provider identity:\n{stdout}"
    );
}

#[test]
fn unknown_provider_flag_fails_with_native_provider_hint() {
    let output = Command::new(env!("CARGO_BIN_EXE_headroom-eval"))
        .args(["--provider", "kompressor", "--text"])
        .output()
        .expect("headroom-eval runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown provider"),
        "stderr did not explain provider failure:\n{stderr}"
    );
    assert!(
        stderr.contains("native_deterministic"),
        "stderr did not hint supported provider:\n{stderr}"
    );
}

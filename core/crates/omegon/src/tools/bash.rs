//! Bash tool — execute shell commands with output capture.

use crate::tools::permissions::{
    FsIntent, FsOperation, IntentActor, IntentConfidence, IntentSource, PathTarget, PathWarning,
    WorkspaceRelation, resolve_intent_target, suspicious_low_confidence_shell_path,
};
use anyhow::Result;
use omegon_traits::{
    ContentBlock, PartialToolResult, ProgressUnits, ToolProgress, ToolProgressSink, ToolResult,
};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const MAX_OUTPUT_LINES: usize = 2000;

/// Minimum interval between content-bearing streamed partials. Cheap
/// rate-limit so a command spewing thousands of lines a second doesn't
/// flood the broadcast channel — the partial is still a complete tail
/// snapshot, not an incremental delta, so consumers always see fresh
/// state on the next tick.
const STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(150);

/// How long the buffer must be quiet before bash emits an idle heartbeat
/// partial. Lets operators see "still alive, just waiting" for commands
/// like `sleep 60 && echo done` that produce no output for long stretches.
const IDLE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

pub async fn execute(
    command: &str,
    cwd: &Path,
    timeout_secs: Option<u64>,
    cancel: CancellationToken,
) -> Result<ToolResult> {
    execute_with_boundary(command, cwd, timeout_secs, cancel, None).await
}

pub async fn execute_with_boundary(
    command: &str,
    cwd: &Path,
    timeout_secs: Option<u64>,
    cancel: CancellationToken,
    boundary: Option<super::WorkspaceBoundary>,
) -> Result<ToolResult> {
    execute_streaming(
        command,
        cwd,
        timeout_secs,
        cancel,
        ToolProgressSink::noop(),
        boundary,
    )
    .await
}

/// Like [`execute`] but streams partial output through `sink` while the
/// command is running. Each partial is a *snapshot* of the combined
/// stdout+stderr buffer (not a delta), built with the same noise-stripping
/// and tail-truncation as the final result, so consumers can render the
/// latest partial directly without merging.
///
/// When `sink` is inactive (the no-op sink) this is byte-for-byte
/// equivalent to the previous `wait_with_output`-based implementation:
/// no partials are constructed and the only behavioral difference is
/// that we read stdout/stderr line-by-line instead of in one shot.
pub async fn execute_streaming(
    command: &str,
    cwd: &Path,
    timeout_secs: Option<u64>,
    cancel: CancellationToken,
    sink: ToolProgressSink,
    boundary: Option<super::WorkspaceBoundary>,
) -> Result<ToolResult> {
    let start = Instant::now();

    // Static blocklist: commands that are known to require interactive input.
    // This is best-effort — programs can prompt from anywhere — but catches
    // the common footguns before they wedge the process. SSH is treated
    // separately below because BatchMode=yes makes it explicitly non-interactive.
    static INTERACTIVE_PREFIXES: &[&str] = &["sudo ", "sudo\t", "passwd", "su ", "su\t", "kinit"];
    let trimmed = command.trim_start();
    if let Some(blocked) = blocked_interactive_command(trimmed) {
        return Ok(blocked_interactive_result(blocked));
    }
    for prefix in INTERACTIVE_PREFIXES {
        if trimmed.starts_with(prefix) || trimmed == prefix.trim() {
            return Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!(
                        "Blocked: `{}` requires interactive input (password/passphrase) \
                         which the agent cannot provide.\n\n\
                         Ask the operator to run this command in their terminal, \
                         then retry the dependent step.",
                        trimmed.split_whitespace().next().unwrap_or(trimmed)
                    ),
                }],
                details: serde_json::json!({
                    "exitCode": -1,
                    "durationMs": 0,
                    "blocked": true,
                    "reason": "interactive_input_required",
                }),
            });
        }
    }

    // ─── Workspace boundary heuristic scan ─────────────────────────
    // Best-effort scan for filesystem write patterns targeting paths outside
    // the workspace boundary. This is not a complete shell sandbox — shell
    // variable expansion, subshells, and programmatic I/O require the Nex
    // container boundary — but detected violations must flow through the same
    // typed permission mediation path as read/write/edit instead of becoming
    // ad hoc bash-local blocks that the agent can route around.
    if let Some(ref boundary) = boundary {
        let intents = extract_shell_fs_intents(trimmed);
        for intent in &intents {
            let resolved = resolve_intent_target(intent, cwd, boundary);
            if matches!(
                resolved.relation,
                WorkspaceRelation::InsideWorkspace | WorkspaceRelation::SpecialAllowed
            ) {
                continue;
            }
            if suspicious_low_confidence_shell_path(intent, &resolved) {
                return Ok(blocked_suspicious_intent_result(intent, &resolved));
            }
            return Err(permission_error_for_intent(intent, &resolved, boundary).into());
        }
    }

    // ─── Native command dispatch ───────────────────────────────────
    // Intercept common single-command invocations (cat, head, ls, etc.)
    // and execute in-process without forking bash. Falls through for
    // pipes, redirects, variable expansion, and unknown commands.
    if let Some(native) = super::native_cmd::try_dispatch(command, cwd, boundary.as_ref()) {
        let duration_ms = start.elapsed().as_millis() as u64;
        let truncated = truncate_tail(&native.stdout);
        let mut text = truncated.content;
        if native.exit_code != 0 {
            text.push_str(&format!(
                "\n\nCommand exited with code {}",
                native.exit_code
            ));
        }
        return Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details: serde_json::json!({
                "exitCode": native.exit_code,
                "durationMs": duration_ms,
                "truncated": truncated.was_truncated,
                "totalLines": truncated.total_lines,
                "totalBytes": truncated.total_bytes,
                "native": true,
            }),
        });
    }

    let mut cmd = Command::new("bash");
    cmd.args(["-c", command])
        .current_dir(cwd)
        .envs(git_discovery_env(cwd))
        .stdin(std::process::Stdio::null()) // /dev/null — commands needing input fail fast
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stderr"))?;

    let mut stdout_lines = BufReader::new(stdout);
    let mut stderr_lines = BufReader::new(stderr);

    // Combined buffer — same shape as the legacy join order (stdout, then
    // a separator newline if stderr is non-empty, then stderr) but here we
    // interleave by arrival because line-readers don't preserve the
    // stdout-then-stderr ordering of the original implementation. The agent
    // gets the same semantic info; live consumers see lines in chronological
    // arrival order which is what they want.
    let mut combined = String::new();
    let mut lines_seen: u64 = 0;
    let mut last_flush = Instant::now();
    let sink_active = sink.is_active();

    let timeout_fut = async {
        if let Some(secs) = timeout_secs {
            tokio::time::sleep(Duration::from_secs(secs)).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::pin!(timeout_fut);

    // Heartbeat ticker — only matters when a sink is attached. We construct
    // it unconditionally so the select arm exists, and gate the actual send
    // on `sink_active` inside the arm. The first tick fires after one full
    // interval, so a fast command never sees a heartbeat.
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + IDLE_HEARTBEAT_INTERVAL,
        IDLE_HEARTBEAT_INTERVAL,
    );

    let exit_status = loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                anyhow::bail!("Command aborted");
            }
            _ = &mut timeout_fut => {
                let secs = timeout_secs.unwrap();
                let _ = child.kill().await;
                anyhow::bail!(
                    "Command timed out after {secs} seconds; the process was killed before Omegon could observe a final exit status. This is an indeterminate host-action result: the operation may have partially completed, completed just before the timeout, or made no progress. Verify with an idempotent status/check command before retrying or reporting failure. For installers, check whether the target is already present."
                );
            }
            _ = heartbeat.tick() => {
                // Only emit if quiet — if we've flushed content recently
                // there's nothing to signal beyond what just went out.
                if sink_active && last_flush.elapsed() >= IDLE_HEARTBEAT_INTERVAL {
                    sink.send(PartialToolResult::heartbeat(start.elapsed().as_millis() as u64));
                    last_flush = Instant::now();
                }
            }
            line = read_lossy_line(&mut stdout_lines) => {
                match line? {
                    Some(l) => {
                        combined.push_str(&l);
                        combined.push('\n');
                        lines_seen += 1;
                        maybe_flush_partial(&sink, sink_active, &combined, lines_seen, start, &mut last_flush);
                    }
                    None => {
                        // stdout closed — drain remaining stderr then wait for exit
                        while let Some(l) = read_lossy_line(&mut stderr_lines).await? {
                            combined.push_str(&l);
                            combined.push('\n');
                            lines_seen += 1;
                            maybe_flush_partial(&sink, sink_active, &combined, lines_seen, start, &mut last_flush);
                        }
                        break child.wait().await?;
                    }
                }
            }
            line = read_lossy_line(&mut stderr_lines) => {
                match line? {
                    Some(l) => {
                        combined.push_str(&l);
                        combined.push('\n');
                        lines_seen += 1;
                        maybe_flush_partial(&sink, sink_active, &combined, lines_seen, start, &mut last_flush);
                    }
                    None => {
                        // stderr closed — drain remaining stdout then wait for exit
                        while let Some(l) = read_lossy_line(&mut stdout_lines).await? {
                            combined.push_str(&l);
                            combined.push('\n');
                            lines_seen += 1;
                            maybe_flush_partial(&sink, sink_active, &combined, lines_seen, start, &mut last_flush);
                        }
                        break child.wait().await?;
                    }
                }
            }
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = exit_status.code().unwrap_or(-1);

    // Strip the trailing newline we added per line so the legacy String
    // shape matches (the old impl did `lossy(stdout) + "\n" + lossy(stderr)`
    // without a final newline).
    if combined.ends_with('\n') {
        combined.pop();
    }

    // Strip terminal control noise (mouse reports, bracketed paste, etc.)
    let clean_output = strip_terminal_noise(&combined);

    // Command-aware output filtering — domain-specific patterns strip
    // noise (progress bars, repeated compile lines, boilerplate) while
    // preserving actionable content (errors, warnings, summaries).
    let compressed = super::output_filter::filter_tool_output(command, &clean_output);

    // Tail-truncate if needed
    let truncated = truncate_tail(&compressed);
    let mut text = truncated.content;

    if exit_code != 0 {
        text.push_str(&format!("\n\nCommand exited with code {exit_code}"));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: serde_json::json!({
            "exitCode": exit_code,
            "durationMs": duration_ms,
            "truncated": truncated.was_truncated,
            "totalLines": truncated.total_lines,
            "totalBytes": truncated.total_bytes,
        }),
    })
}

pub(crate) fn git_discovery_env(cwd: &Path) -> Vec<(&'static str, String)> {
    crate::setup::git_ceiling_directory(cwd)
        .map(|ceiling| vec![("GIT_CEILING_DIRECTORIES", ceiling.display().to_string())])
        .unwrap_or_default()
}

fn blocked_interactive_result(command_name: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Blocked: `{command_name}` requires interactive input (password/passphrase) \
                 which the agent cannot provide.\n\n\
                 Ask the operator to run this command in their terminal, \
                 then retry the dependent step."
            ),
        }],
        details: serde_json::json!({
            "exitCode": -1,
            "durationMs": 0,
            "blocked": true,
            "reason": "interactive_input_required",
        }),
    }
}

fn blocked_interactive_command(trimmed: &str) -> Option<&'static str> {
    if command_after_env_assignments(trimmed) != Some("ssh") {
        return None;
    }
    if ssh_batch_mode_enabled(trimmed) {
        None
    } else {
        Some("ssh")
    }
}

fn command_after_env_assignments(trimmed: &str) -> Option<&str> {
    trimmed
        .split_whitespace()
        .find(|token| !looks_like_env_assignment(token))
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, _value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && !name.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn ssh_batch_mode_enabled(command: &str) -> bool {
    let normalized = command.replace('=', " ");
    let mut tokens = normalized.split_whitespace();
    while let Some(token) = tokens.next() {
        if token.eq_ignore_ascii_case("BatchMode") {
            return tokens
                .next()
                .is_some_and(|value| value.eq_ignore_ascii_case("yes"));
        }
        if token
            .strip_prefix("-o")
            .is_some_and(|option| option.eq_ignore_ascii_case("BatchMode"))
        {
            return tokens
                .next()
                .is_some_and(|value| value.eq_ignore_ascii_case("yes"));
        }
    }
    false
}

/// Push a tail-truncated snapshot of `buffer` to the sink, rate-limited
/// by [`STREAM_FLUSH_INTERVAL`]. Cheap when no consumer is attached.
///
/// The partial carries:
/// - `tail`: noise-stripped, tail-truncated buffer (consumers render directly)
/// - `progress.elapsed_ms`: wall-clock since the command started
/// - `progress.units`: cumulative line count, no upper bound (bash doesn't
///   know how much output is coming)
/// - `details`: legacy bash-specific keys preserved for any consumer that
///   was sniffing them
fn maybe_flush_partial(
    sink: &ToolProgressSink,
    sink_active: bool,
    buffer: &str,
    lines_seen: u64,
    started_at: Instant,
    last_flush: &mut Instant,
) {
    if !sink_active {
        return;
    }
    if last_flush.elapsed() < STREAM_FLUSH_INTERVAL {
        return;
    }
    *last_flush = Instant::now();

    let cleaned = strip_terminal_noise(buffer);
    let truncated = truncate_tail(&cleaned);

    sink.send(PartialToolResult {
        tail: truncated.content,
        progress: ToolProgress {
            elapsed_ms: started_at.elapsed().as_millis() as u64,
            heartbeat: false,
            phase: None,
            units: Some(ProgressUnits {
                current: lines_seen,
                total: None,
                unit: "lines".to_string(),
            }),
            tally: None,
        },
        details: serde_json::json!({
            "totalLines": truncated.total_lines,
            "totalBytes": truncated.total_bytes,
            "truncated": truncated.was_truncated,
        }),
    });
}

async fn read_lossy_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<String>> {
    let mut buf = Vec::new();
    let n = reader.read_until(b'\n', &mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    if buf.ends_with(b"\n") {
        buf.pop();
        if buf.ends_with(b"\r") {
            buf.pop();
        }
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Strip CSI terminal control sequences that aren't SGR color codes.
///
/// Piped stdout/stderr shouldn't contain these, but they can leak through
/// when programs detect a pseudo-tty or when terminal multiplexers inject
/// mouse-tracking reports, bracketed-paste markers, or cursor positioning.
///
/// We preserve SGR (Select Graphic Rendition, ending with 'm') because
/// the TUI renderer converts those to styled spans via `ansi_to_tui`.
pub(crate) fn strip_terminal_noise(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'\x1b' && i + 1 < len && bytes[i + 1] == b'[' {
            // CSI sequence: ESC [ ... <final byte>
            let start = i;
            i += 2; // skip ESC [
            // Skip parameter bytes (0x30–0x3F) and intermediate bytes (0x20–0x2F)
            while i < len && bytes[i] >= 0x20 && bytes[i] <= 0x3F {
                i += 1;
            }
            // Final byte (0x40–0x7E)
            if i < len && bytes[i] >= 0x40 && bytes[i] <= 0x7E {
                let final_byte = bytes[i];
                i += 1;
                if final_byte == b'm' {
                    // SGR — keep it for color rendering, BUT the SGR mouse
                    // protocol also ends with 'm' (button release). Distinguish
                    // by the leading '<' parameter byte that SGR mouse always has.
                    let params = &input[start + 2..i - 1]; // between ESC[ and final byte
                    if !params.starts_with('<') {
                        result.push_str(&input[start..i]);
                    }
                    // else: SGR mouse release — drop
                }
                // All other CSI sequences (mouse reports, cursor movement, etc.) — drop
            } else {
                // Malformed CSI — drop the whole thing
                if i < len {
                    i += 1;
                }
            }
        } else if bytes[i] == b'\x1b' && i + 1 < len && bytes[i + 1] == b']' {
            // OSC sequence: ESC ] ... (ST or BEL)
            i += 2;
            while i < len {
                if bytes[i] == b'\x07' {
                    i += 1;
                    break;
                }
                if bytes[i] == b'\x1b' && i + 1 < len && bytes[i + 1] == b'\\' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            // OSC sequences (title changes, hyperlinks) — drop entirely
        } else {
            result.push(input[i..].chars().next().unwrap());
            i += input[i..].chars().next().unwrap().len_utf8();
        }
    }

    result
}

// Output compression moved to tools/output_filter.rs

struct Truncated {
    content: String,
    was_truncated: bool,
    total_lines: usize,
    total_bytes: usize,
}

fn truncate_tail(output: &str) -> Truncated {
    let total_bytes = output.len();
    let lines: Vec<&str> = output.lines().collect();
    let total_lines = lines.len();

    if total_bytes <= MAX_OUTPUT_BYTES && total_lines <= MAX_OUTPUT_LINES {
        return Truncated {
            content: output.to_string(),
            was_truncated: false,
            total_lines,
            total_bytes,
        };
    }

    // Take the last N lines within byte budget
    let mut kept = Vec::new();
    let mut bytes = 0;
    for line in lines.iter().rev() {
        let line_bytes = line.len() + 1; // +1 for newline
        if bytes + line_bytes > MAX_OUTPUT_BYTES || kept.len() >= MAX_OUTPUT_LINES {
            break;
        }
        kept.push(*line);
        bytes += line_bytes;
    }
    kept.reverse();

    let content = kept.join("\n");
    Truncated {
        content,
        was_truncated: true,
        total_lines,
        total_bytes,
    }
}

// ── Workspace boundary heuristic scanner ──────────────────────────────
//
// Best-effort detection of filesystem write patterns in bash commands.
// Scans for redirects, copy/move commands, and mkdir targeting absolute
// paths outside the workspace.
//
// EXPLICITLY NOT A SECURITY BOUNDARY:
// - Does not analyze shell variables, subshells, heredocs, or command substitution
// - Does not catch programmatic I/O (python -c "open('/x','w')")
// - Trivially bypassable via indirection
// - The Nex container sandbox is the security boundary

/// Paths that are always allowed regardless of workspace boundary.
const ALLOWED_PATHS: &[&str] = &[
    "/dev/null",
    "/dev/stdin",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/fd/0",
    "/dev/fd/1",
    "/dev/fd/2",
    "/proc/self/fd/0",
    "/proc/self/fd/1",
    "/proc/self/fd/2",
];

/// Scan a bash command for filesystem write patterns targeting paths
/// outside the workspace boundary. Returns a list of paths that need
/// permission mediation before bash may execute.
pub(crate) fn scan_boundary_violations(
    command: &str,
    boundary: &super::WorkspaceBoundary,
    cwd: &Path,
) -> Vec<String> {
    let mut violations = extract_shell_fs_intents(command)
        .into_iter()
        .filter_map(|intent| {
            let resolved = resolve_intent_target(&intent, cwd, boundary);
            if matches!(
                resolved.relation,
                WorkspaceRelation::InsideWorkspace | WorkspaceRelation::SpecialAllowed
            ) {
                None
            } else {
                Some(intent.raw_path().to_string())
            }
        })
        .collect::<Vec<_>>();
    violations.sort();
    violations.dedup();
    violations
}

pub(crate) fn extract_shell_fs_intents(command: &str) -> Vec<FsIntent> {
    let mut intents = Vec::new();

    // Pattern 1: Output redirects — > /path, >> /path, 2> /path
    for cap in regex_lite::Regex::new(r"([012]?>>?)\s*([^\s;|&]+)")
        .unwrap()
        .captures_iter(command)
    {
        if let (Some(op_match), Some(path_match)) = (cap.get(1), cap.get(2)) {
            push_shell_intent(
                &mut intents,
                if op_match.as_str().contains(">>") {
                    FsOperation::Append
                } else {
                    FsOperation::Write
                },
                path_match.as_str(),
                IntentSource::ShellRedirect {
                    command_excerpt: command_excerpt(command, path_match.start(), path_match.end()),
                    redirect_op: op_match.as_str().to_string(),
                },
                IntentConfidence::Heuristic,
            );
        }
    }

    // Pattern 2: tee to path — tee /path, tee -a /path
    for cap in regex_lite::Regex::new(r"\btee\s+(-a\s+)?([^\s;|&]+)")
        .unwrap()
        .captures_iter(command)
    {
        if let Some(path_match) = cap.get(2) {
            push_shell_intent(
                &mut intents,
                if cap.get(1).is_some() {
                    FsOperation::Append
                } else {
                    FsOperation::Write
                },
                path_match.as_str(),
                IntentSource::ShellCommandArgument {
                    command_excerpt: command_excerpt(command, path_match.start(), path_match.end()),
                    command_name: "tee".to_string(),
                    argv_index: if cap.get(1).is_some() { 2 } else { 1 },
                },
                IntentConfidence::Heuristic,
            );
        }
    }

    // Pattern 3: cp/mv/install destination — last arg if path-like
    for cap in regex_lite::Regex::new(r"\b(cp|mv|install)\s+(?:[^\s]+\s+)+([^\s;|&]+)")
        .unwrap()
        .captures_iter(command)
    {
        if let (Some(cmd_match), Some(path_match)) = (cap.get(1), cap.get(2)) {
            let operation = match cmd_match.as_str() {
                "mv" => FsOperation::Move,
                _ => FsOperation::Copy,
            };
            push_shell_intent(
                &mut intents,
                operation,
                path_match.as_str(),
                IntentSource::ShellCommandArgument {
                    command_excerpt: command_excerpt(command, path_match.start(), path_match.end()),
                    command_name: cmd_match.as_str().to_string(),
                    argv_index: 2,
                },
                IntentConfidence::Heuristic,
            );
        }
    }

    // Pattern 4: mkdir on path
    for cap in regex_lite::Regex::new(r"\bmkdir\s+(?:-p\s+)?([^\s;|&]+)")
        .unwrap()
        .captures_iter(command)
    {
        if let Some(path_match) = cap.get(1) {
            push_shell_intent(
                &mut intents,
                FsOperation::CreateDir,
                path_match.as_str(),
                IntentSource::ShellCommandArgument {
                    command_excerpt: command_excerpt(command, path_match.start(), path_match.end()),
                    command_name: "mkdir".to_string(),
                    argv_index: 1,
                },
                IntentConfidence::Heuristic,
            );
        }
    }

    // Pattern 5: rm on path
    for cap in regex_lite::Regex::new(r"\brm\s+(?:-[rRf]+\s+)*([^\s;|&]+)")
        .unwrap()
        .captures_iter(command)
    {
        if let Some(path_match) = cap.get(1) {
            push_shell_intent(
                &mut intents,
                FsOperation::Delete,
                path_match.as_str(),
                IntentSource::ShellCommandArgument {
                    command_excerpt: command_excerpt(command, path_match.start(), path_match.end()),
                    command_name: "rm".to_string(),
                    argv_index: 1,
                },
                IntentConfidence::Heuristic,
            );
        }
    }

    intents
}

fn push_shell_intent(
    intents: &mut Vec<FsIntent>,
    operation: FsOperation,
    raw_path: &str,
    source: IntentSource,
    confidence: IntentConfidence,
) {
    let target = PathTarget::classify(raw_path);
    match target {
        PathTarget::PosixAbsolute { .. }
        | PathTarget::WorkspaceRelative { .. }
        | PathTarget::PosixHomeRelative { .. }
        | PathTarget::SpecialDevice { .. }
        | PathTarget::FileDescriptor { .. }
        | PathTarget::WindowsDriveAbsolute { .. }
        | PathTarget::WindowsDriveRelative { .. }
        | PathTarget::WindowsRootRelative { .. }
        | PathTarget::WindowsUnc { .. }
        | PathTarget::WindowsVerbatim { .. }
        | PathTarget::WindowsDevice { .. }
        | PathTarget::WslDriveMount { .. }
        | PathTarget::MsysDriveMount { .. }
        | PathTarget::CygwinDriveMount { .. } => intents.push(FsIntent {
            operation,
            target,
            actor: IntentActor::Model,
            source,
            confidence,
        }),
        PathTarget::Unknown { .. } => {}
    }
}

fn command_excerpt(command: &str, start: usize, end: usize) -> String {
    let excerpt_start = command[..start].rfind(['\n', ';']).map_or(0, |idx| idx + 1);
    let excerpt_end = command[end..]
        .find(['\n', ';'])
        .map_or(command.len(), |idx| end + idx);
    command[excerpt_start..excerpt_end].trim().to_string()
}

fn permission_error_for_intent(
    intent: &FsIntent,
    resolved: &crate::tools::permissions::ResolvedFsTarget,
    boundary: &super::WorkspaceBoundary,
) -> super::PathPermissionError {
    let mut requested_path = intent.raw_path().to_string();
    if !resolved.warnings.is_empty() {
        requested_path.push_str("\n");
        requested_path.push_str(&permission_warning_text(intent, resolved));
    }
    super::PathPermissionError {
        requested_path,
        directory: resolved
            .canonical
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        workspace: boundary.cwd().display().to_string(),
    }
}

fn blocked_suspicious_intent_result(
    intent: &FsIntent,
    resolved: &crate::tools::permissions::ResolvedFsTarget,
) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "BLOCKED: suspicious filesystem intent\n\nPath: {}\nOperation: {:?}\nSource: {}\nConfidence: {:?}\n{}\n\nThis low-confidence shell extraction looks malformed or truncated. Rewrite the command with an explicit valid path.",
                intent.raw_path(),
                intent.operation,
                intent.source.description(),
                intent.confidence,
                permission_warning_text(intent, resolved),
            ),
        }],
        details: serde_json::json!({
            "exitCode": -1,
            "blocked": true,
            "reason": "suspicious_filesystem_intent",
            "path": intent.raw_path(),
            "operation": format!("{:?}", intent.operation),
            "source": intent.source.description(),
            "confidence": format!("{:?}", intent.confidence),
        }),
    }
}

fn permission_warning_text(
    _intent: &FsIntent,
    resolved: &crate::tools::permissions::ResolvedFsTarget,
) -> String {
    let mut lines = Vec::new();
    for warning in &resolved.warnings {
        match warning {
            PathWarning::RootDotPath { suggested_workspace_relative } => lines.push(format!(
                "Warning: `{}` is host-absolute and looks like workspace-relative `{}` with an accidental leading slash.",
                resolved.raw, suggested_workspace_relative
            )),
            PathWarning::ShortRootPath => lines.push(format!(
                "Warning: `{}` is a short root path that may be a malformed or truncated token.",
                resolved.raw
            )),
            PathWarning::WindowsDriveRelative => lines.push(format!(
                "Warning: `{}` is Windows drive-relative (`C:foo`), not drive-absolute (`C:\\foo`).",
                resolved.raw
            )),
            PathWarning::WindowsVerbatimPath => lines.push(format!(
                "Warning: `{}` is a Windows absolute/verbatim path; it requires exact host-boundary approval and is not treated as workspace-relative.",
                resolved.raw
            )),
            PathWarning::WindowsUncPath => lines.push(format!(
                "Warning: `{}` is a Windows UNC/network path; it requires exact host-boundary approval.",
                resolved.raw
            )),
            PathWarning::WindowsDeviceName => lines.push(format!(
                "Warning: `{}` is a Windows device name and is blocked unless explicitly supported.",
                resolved.raw
            )),
            PathWarning::WslWindowsDriveMount => lines.push(format!(
                "Warning: `{}` is a WSL Windows-drive mount path; verify whether you intended Linux workspace storage or Windows host storage.",
                resolved.raw
            )),
            PathWarning::MsysWindowsDriveMount => lines.push(format!(
                "Warning: `{}` is an MSYS/Git-Bash Windows-drive mount path; verify whether you intended POSIX workspace storage or Windows host storage.",
                resolved.raw
            )),
            PathWarning::CygwinWindowsDriveMount => lines.push(format!(
                "Warning: `{}` is a Cygwin Windows-drive mount path; verify whether you intended POSIX workspace storage or Windows host storage.",
                resolved.raw
            )),
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_tail_no_truncation() {
        let output = "line1\nline2\nline3";
        let result = truncate_tail(output);
        assert!(!result.was_truncated);
        assert_eq!(result.total_lines, 3);
        assert_eq!(result.content, output);
    }

    #[test]
    fn truncate_tail_by_lines() {
        let output = (0..3000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_tail(&output);
        assert!(result.was_truncated);
        assert_eq!(result.total_lines, 3000);
        assert!(result.content.lines().count() <= MAX_OUTPUT_LINES);
        // Should keep the LAST lines (tail)
        assert!(result.content.contains("line 2999"));
    }

    #[test]
    fn truncate_tail_by_bytes() {
        let output = (0..100)
            .map(|_| "x".repeat(1000))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_tail(&output);
        assert!(result.was_truncated);
        assert!(result.content.len() <= MAX_OUTPUT_BYTES);
    }

    #[test]
    fn truncate_empty() {
        let result = truncate_tail("");
        assert!(!result.was_truncated);
        assert_eq!(result.total_lines, 0);
    }

    #[test]
    fn strip_terminal_noise_preserves_sgr_colors() {
        let input = "\x1b[32mhello\x1b[0m world";
        let result = strip_terminal_noise(input);
        assert_eq!(result, "\x1b[32mhello\x1b[0m world");
    }

    #[test]
    fn strip_terminal_noise_removes_mouse_reports() {
        // SGR mouse report: ESC [ < Ps ; Ps ; Ps M
        let input = "before\x1b[<39;80;45Mafter";
        let result = strip_terminal_noise(input);
        assert_eq!(result, "beforeafter", "mouse report should be stripped");
    }

    #[test]
    fn strip_terminal_noise_removes_cursor_movement() {
        // Cursor up: ESC [ A
        let input = "line1\x1b[Aline2";
        let result = strip_terminal_noise(input);
        assert_eq!(result, "line1line2");
    }

    #[test]
    fn strip_terminal_noise_removes_bracketed_paste() {
        // Bracketed paste mode: ESC [ ? 2004 h / l
        let input = "before\x1b[?2004hpasted\x1b[?2004lafter";
        let result = strip_terminal_noise(input);
        assert_eq!(result, "beforepastedafter");
    }

    #[test]
    fn strip_terminal_noise_removes_osc_sequences() {
        // Title change: ESC ] 0 ; title BEL
        let input = "before\x1b]0;window title\x07after";
        let result = strip_terminal_noise(input);
        assert_eq!(result, "beforeafter");
    }

    #[test]
    fn strip_terminal_noise_plain_text_unchanged() {
        let input = "just plain text\nwith newlines\n";
        assert_eq!(strip_terminal_noise(input), input);
    }

    #[test]
    fn strip_terminal_noise_mixed_sgr_and_mouse() {
        let input = "\x1b[1;31merror:\x1b[0m failed\x1b[<0;10;20M\x1b[<0;10;20m";
        let result = strip_terminal_noise(input);
        // SGR (31m, 0m) preserved, mouse reports (M, m endings with <) stripped
        assert_eq!(result, "\x1b[1;31merror:\x1b[0m failed");
    }

    #[test]
    fn strip_terminal_noise_empty_input() {
        assert_eq!(strip_terminal_noise(""), "");
    }

    #[tokio::test]
    async fn blocks_sudo() {
        let cancel = CancellationToken::new();
        let result = execute("sudo chown user file", Path::new("."), None, cancel)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Blocked"), "should block sudo: {text}");
        assert_eq!(result.details["blocked"], true);
        assert_eq!(result.details["reason"], "interactive_input_required");
    }

    #[tokio::test]
    async fn blocks_sudo_with_leading_whitespace() {
        let cancel = CancellationToken::new();
        let result = execute("  sudo rm -rf /", Path::new("."), None, cancel)
            .await
            .unwrap();
        assert_eq!(result.details["blocked"], true);
    }

    #[tokio::test]
    async fn blocks_ssh() {
        let cancel = CancellationToken::new();
        let result = execute("ssh user@host", Path::new("."), None, cancel)
            .await
            .unwrap();
        assert_eq!(result.details["blocked"], true);
    }

    #[test]
    fn ssh_batch_mode_allows_non_interactive_ssh() {
        assert_eq!(
            blocked_interactive_command("ssh -o BatchMode=yes -i ~/.ssh/key user@host 'hostname'"),
            None
        );
        assert_eq!(
            blocked_interactive_command(
                "ssh -oBatchMode=yes -o StrictHostKeyChecking=no user@host hostname"
            ),
            None
        );
        assert_eq!(
            blocked_interactive_command(
                "SSH_AUTH_SOCK=$SSH_AUTH_SOCK ssh -o BatchMode=yes user@host hostname"
            ),
            None
        );
    }

    #[test]
    fn ssh_without_batch_mode_is_blocked() {
        assert_eq!(blocked_interactive_command("ssh user@host"), Some("ssh"));
        assert_eq!(
            blocked_interactive_command("ssh -i ~/.ssh/key user@host hostname"),
            Some("ssh")
        );
        assert_eq!(
            blocked_interactive_command("ssh -o BatchMode=no user@host hostname"),
            Some("ssh")
        );
        assert_eq!(
            blocked_interactive_command("SSH_AUTH_SOCK=$SSH_AUTH_SOCK ssh user@host hostname"),
            Some("ssh")
        );
    }

    #[tokio::test]
    async fn does_not_block_echo_sudo() {
        let cancel = CancellationToken::new();
        let result = execute("echo sudo is great", Path::new("."), None, cancel)
            .await
            .unwrap();
        assert_eq!(result.details["exitCode"], 0);
        assert!(result.details.get("blocked").is_none());
    }

    #[tokio::test]
    async fn stdin_is_null_so_read_fails() {
        let cancel = CancellationToken::new();
        // `read` from stdin should get immediate EOF, not hang
        let result = execute(
            "read -t 1 VAR; echo \"got: $VAR\"",
            Path::new("."),
            Some(5),
            cancel,
        )
        .await
        .unwrap();
        // Should complete quickly (not timeout), read gets EOF
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("got:"),
            "read should get EOF, not hang: {text}"
        );
    }

    #[tokio::test]
    async fn execute_echo() {
        let cancel = CancellationToken::new();
        let result = execute("echo hello", Path::new("."), None, cancel)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("hello"), "should contain output: {text}");
        assert_eq!(result.details["exitCode"], 0);
    }

    #[tokio::test]
    async fn execute_nonzero_exit() {
        let cancel = CancellationToken::new();
        let result = execute("exit 42", Path::new("."), None, cancel)
            .await
            .unwrap();
        assert_eq!(result.details["exitCode"], 42);
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("42"), "should mention exit code: {text}");
    }

    #[tokio::test]
    async fn execute_stderr() {
        let cancel = CancellationToken::new();
        let result = execute("echo err >&2", Path::new("."), None, cancel)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("err"), "should capture stderr: {text}");
    }

    #[tokio::test]
    async fn execute_binary_output_is_lossy_not_fatal() {
        let cancel = CancellationToken::new();
        let result = execute("printf 'abc\\377def\\n'", Path::new("."), None, cancel)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("abc"), "should capture prefix: {text}");
        assert!(text.contains("def"), "should capture suffix: {text}");
        assert!(
            text.contains('\u{fffd}'),
            "invalid byte should be replacement char: {text}"
        );
    }

    #[tokio::test]
    async fn execute_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = execute("sleep 10", Path::new("."), None, cancel).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_timeout() {
        let cancel = CancellationToken::new();
        let result = execute("sleep 10", Path::new("."), Some(1), cancel).await;
        let err = result.expect_err("sleep should time out").to_string();
        assert!(err.contains("Command timed out after 1 seconds"));
        assert!(err.contains("indeterminate host-action result"));
        assert!(err.contains("Verify with an idempotent status/check command"));
    }

    #[tokio::test]
    async fn streaming_sink_receives_typed_partials() {
        use std::sync::{Arc, Mutex};
        let collected: Arc<Mutex<Vec<PartialToolResult>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_for_sink = collected.clone();
        let sink = ToolProgressSink::from_fn(move |partial| {
            collected_for_sink.lock().unwrap().push(partial);
        });

        // Emit a handful of lines spaced beyond STREAM_FLUSH_INTERVAL so the
        // rate-limiter actually flushes more than once.
        let cancel = CancellationToken::new();
        let result = execute_streaming(
            "for i in 1 2 3 4; do echo line-$i; sleep 0.2; done",
            Path::new("."),
            Some(10),
            cancel,
            sink,
            None,
        )
        .await
        .unwrap();

        // Final result still contains every line.
        let final_text = result.content[0].as_text().unwrap();
        for i in 1..=4 {
            assert!(
                final_text.contains(&format!("line-{i}")),
                "final result missing line-{i}: {final_text}"
            );
        }
        assert_eq!(result.details["exitCode"], 0);

        // At least one partial should have flown through the sink. We don't
        // assert an exact count because flush timing is wall-clock dependent.
        let partials = collected.lock().unwrap();
        assert!(
            !partials.is_empty(),
            "expected at least one streamed partial"
        );

        // Find content-bearing partials (heartbeats may also exist for slow
        // tests but the body of this command is < IDLE_HEARTBEAT_INTERVAL).
        let content_partials: Vec<&PartialToolResult> =
            partials.iter().filter(|p| !p.progress.heartbeat).collect();
        assert!(
            !content_partials.is_empty(),
            "expected at least one content partial"
        );

        // Every content partial should populate the typed shape correctly.
        for p in &content_partials {
            assert!(!p.tail.is_empty(), "content partial should carry tail");
            assert!(p.progress.elapsed_ms > 0, "elapsed_ms should be set");
            let units = p
                .progress
                .units
                .as_ref()
                .expect("content partial should carry units");
            assert_eq!(units.unit, "lines");
            assert!(units.current > 0, "lines counter should advance");
            assert!(
                units.total.is_none(),
                "bash has no concept of total line count"
            );
            assert!(p.progress.phase.is_none(), "bash sets no phase label");
            assert!(p.progress.tally.is_none(), "bash has no outcome tally");
        }

        // Counter should be monotonically non-decreasing across partials.
        let mut last = 0u64;
        for p in &content_partials {
            let cur = p.progress.units.as_ref().unwrap().current;
            assert!(cur >= last, "lines counter regressed: {last} -> {cur}");
            last = cur;
        }
    }

    #[tokio::test]
    async fn streaming_sink_emits_idle_heartbeat() {
        use std::sync::{Arc, Mutex};
        let collected: Arc<Mutex<Vec<PartialToolResult>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_for_sink = collected.clone();
        let sink = ToolProgressSink::from_fn(move |partial| {
            collected_for_sink.lock().unwrap().push(partial);
        });

        // Sleep longer than IDLE_HEARTBEAT_INTERVAL (5s) with no output, then
        // emit a single line. We should see at least one heartbeat partial
        // followed by a content partial.
        let cancel = CancellationToken::new();
        let result = execute_streaming(
            "sleep 6 && echo done",
            Path::new("."),
            Some(15),
            cancel,
            sink,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.details["exitCode"], 0);
        assert!(result.content[0].as_text().unwrap().contains("done"));

        let partials = collected.lock().unwrap();
        let heartbeats: Vec<&PartialToolResult> =
            partials.iter().filter(|p| p.progress.heartbeat).collect();
        assert!(
            !heartbeats.is_empty(),
            "expected at least one idle heartbeat during 6s sleep, got {} partials total",
            partials.len()
        );
        for hb in &heartbeats {
            assert!(hb.tail.is_empty(), "heartbeat should carry no content");
            assert!(hb.progress.elapsed_ms > 0);
        }
    }

    #[tokio::test]
    async fn streaming_sink_inactive_is_zero_overhead() {
        // The default no-op sink should not affect outcome — covered by the
        // existing `execute_*` tests since `execute` now forwards to
        // `execute_streaming` with a noop sink, but assert it explicitly.
        let cancel = CancellationToken::new();
        let result = execute_streaming(
            "echo hello",
            Path::new("."),
            None,
            cancel,
            ToolProgressSink::noop(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.details["exitCode"], 0);
        assert!(result.content[0].as_text().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn bash_git_discovery_does_not_escape_marked_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        let child = dir.path().join("child-workspace");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(child.join("AGENTS.md"), "instructions").unwrap();

        let result = execute(
            "git rev-parse --show-toplevel",
            &child,
            None,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_ne!(result.details["exitCode"], 0);
    }

    // Output filter tests moved to tools/output_filter.rs

    // ── Boundary scanner tests ────────────────────────────────────────

    fn test_boundary(workspace: &str) -> crate::tools::WorkspaceBoundary {
        crate::tools::WorkspaceBoundary::new(std::path::PathBuf::from(workspace))
    }

    #[test]
    fn scanner_catches_redirect_to_absolute_path() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations(
            "echo secret > /etc/evil.txt",
            &b,
            Path::new("/tmp/workspace"),
        );
        assert!(!v.is_empty(), "should catch redirect to /etc/evil.txt");
        assert!(v.iter().any(|p| p.contains("/etc/evil.txt")));
    }

    #[tokio::test]
    async fn bash_boundary_hit_returns_typed_permission_error() {
        let b = test_boundary("/tmp/workspace");
        let err = execute_streaming(
            "echo secret > /etc/evil.txt",
            Path::new("/tmp/workspace"),
            Some(1),
            CancellationToken::new(),
            ToolProgressSink::noop(),
            Some(b),
        )
        .await
        .expect_err("outside-workspace bash writes should request permission");

        let permission = err
            .downcast_ref::<crate::tools::PathPermissionError>()
            .expect("bash boundary hits must use PathPermissionError");
        assert_eq!(permission.requested_path, "/etc/evil.txt");
        assert!(
            permission.directory.ends_with("/etc"),
            "unexpected directory: {}",
            permission.directory
        );
    }

    #[tokio::test]
    async fn bash_root_dot_path_permission_error_carries_diagnostic() {
        let b = test_boundary("/tmp/workspace");
        let err = execute_streaming(
            "mkdir -p /.omegon/runtime",
            Path::new("/tmp/workspace"),
            Some(1),
            CancellationToken::new(),
            ToolProgressSink::noop(),
            Some(b),
        )
        .await
        .expect_err("root-dot paths should still hit the boundary");

        let permission = err
            .downcast_ref::<crate::tools::PathPermissionError>()
            .expect("root-dot boundary hits must use PathPermissionError");
        assert!(permission.requested_path.contains("/.omegon/runtime"));
        assert!(permission.requested_path.contains(".omegon/runtime"));
        assert!(
            permission
                .requested_path
                .contains("accidental leading slash")
        );
    }

    #[tokio::test]
    async fn bash_short_root_path_is_diagnostic_block_not_permission_prompt() {
        let b = test_boundary("/tmp/workspace");
        let result = execute_streaming(
            "echo secret > /Ig",
            Path::new("/tmp/workspace"),
            Some(1),
            CancellationToken::new(),
            ToolProgressSink::noop(),
            Some(b),
        )
        .await
        .expect("suspicious shell paths should return a blocked tool result");

        assert_eq!(result.details["blocked"], true);
        assert_eq!(result.details["reason"], "suspicious_filesystem_intent");
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("/Ig"));
        assert!(text.contains("malformed or truncated"));
    }

    #[test]
    fn shell_intent_extraction_keeps_relative_omegon_workspace_relative() {
        let intents = extract_shell_fs_intents("mkdir -p .omegon/runtime");
        assert_eq!(intents.len(), 1);
        assert!(matches!(
            intents[0].target,
            crate::tools::permissions::PathTarget::WorkspaceRelative { .. }
        ));
        assert_eq!(intents[0].raw_path(), ".omegon/runtime");
    }

    #[test]
    fn shell_intent_extraction_marks_root_dot_host_absolute() {
        let intents = extract_shell_fs_intents("mkdir -p /.omegon/runtime");
        assert_eq!(intents.len(), 1);
        assert!(matches!(
            intents[0].target,
            crate::tools::permissions::PathTarget::PosixAbsolute { .. }
        ));
        assert_eq!(intents[0].raw_path(), "/.omegon/runtime");
    }

    #[test]
    fn scanner_allows_redirect_to_devnull() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations("command 2> /dev/null", &b, Path::new("/tmp/workspace"));
        assert!(v.is_empty(), "should allow /dev/null: {:?}", v);
    }

    #[test]
    fn scanner_allows_standard_fd_redirects() {
        let b = test_boundary("/tmp/workspace");
        for path in [
            "/dev/stdout",
            "/dev/stderr",
            "/dev/fd/1",
            "/dev/fd/2",
            "/proc/self/fd/1",
            "/proc/self/fd/2",
        ] {
            let command = format!("echo data > {path}");
            let v = scan_boundary_violations(&command, &b, Path::new("/tmp/workspace"));
            assert!(v.is_empty(), "should allow {path}: {v:?}");
        }
    }

    #[test]
    fn scanner_blocks_unsafe_device_redirects() {
        let b = test_boundary("/tmp/workspace");
        for path in ["/dev/zero", "/dev/fd/3", "/proc/self/fd/3"] {
            let command = format!("echo data > {path}");
            let v = scan_boundary_violations(&command, &b, Path::new("/tmp/workspace"));
            assert!(!v.is_empty(), "should block {path}");
        }
    }

    #[test]
    fn scanner_catches_tee_to_absolute_path() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations(
            "echo data | tee /etc/config.yml",
            &b,
            Path::new("/tmp/workspace"),
        );
        assert!(!v.is_empty(), "should catch tee to /etc/config.yml");
    }

    #[test]
    fn scanner_catches_cp_to_absolute_path() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations(
            "cp important.txt /etc/stolen.txt",
            &b,
            Path::new("/tmp/workspace"),
        );
        assert!(!v.is_empty(), "should catch cp dest /etc/stolen.txt");
    }

    #[test]
    fn scanner_catches_mkdir_absolute_path() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations("mkdir -p /opt/evil/dir", &b, Path::new("/tmp/workspace"));
        assert!(!v.is_empty(), "should catch mkdir /opt/evil/dir");
    }

    #[test]
    fn scanner_allows_relative_redirect() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations("echo data > output.txt", &b, Path::new("/tmp/workspace"));
        assert!(v.is_empty(), "relative paths should be allowed: {:?}", v);
    }

    #[test]
    fn scanner_allows_workspace_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let b = crate::tools::WorkspaceBoundary::new(cwd.clone());
        let cmd = format!("echo data > {}/output.txt", cwd.display());
        let v = scan_boundary_violations(&cmd, &b, &cwd);
        assert!(
            v.is_empty(),
            "workspace-internal absolute paths should be allowed: {:?}",
            v
        );
    }

    #[test]
    fn scanner_catches_rm_absolute_path() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations("rm -rf /var/important", &b, Path::new("/tmp/workspace"));
        assert!(!v.is_empty(), "should catch rm /var/important");
    }

    #[test]
    fn scanner_allows_trusted_directory() {
        let b = test_boundary("/tmp/workspace");
        b.approve_directory(std::path::PathBuf::from("/opt/allowed"));
        let v = scan_boundary_violations(
            "echo x > /opt/allowed/file.txt",
            &b,
            Path::new("/tmp/workspace"),
        );
        assert!(v.is_empty(), "trusted directory should be allowed: {:?}", v);
    }

    #[test]
    fn scanner_allows_temp_directory_redirect() {
        let b = test_boundary("/tmp/workspace");
        let temp_file = std::env::temp_dir().join("omegon-permission-intent-test.log");
        let command = format!("echo x > {}", temp_file.display());
        let v = scan_boundary_violations(&command, &b, Path::new("/tmp/workspace"));
        assert!(v.is_empty(), "temp directory should remain allowed: {v:?}");
    }

    #[test]
    fn scanner_preserves_legitimate_etc_boundary_violation() {
        let b = test_boundary("/tmp/workspace");
        let v = scan_boundary_violations("echo x > /etc/hosts", &b, Path::new("/tmp/workspace"));
        assert_eq!(v, vec!["/etc/hosts".to_string()]);
    }
}

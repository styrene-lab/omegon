use anyhow::{Context, Result, bail};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TddCommand {
    pub argv: Vec<String>,
    pub hash: String,
}

impl TddCommand {
    pub fn new(argv: Vec<String>) -> Result<Self> {
        if argv.is_empty() {
            bail!("missing command after --");
        }
        let normalized = normalize_command(&argv);
        let mut hasher = Sha256::new();
        for part in &normalized {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
        let hash = format!("sha256:{:x}", hasher.finalize());
        Ok(Self { argv, hash })
    }
}

fn normalize_command(argv: &[String]) -> Vec<String> {
    argv.iter().map(|s| s.trim().to_string()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TddState {
    Passing,
    Failing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutcome {
    pub exit_code: Option<i32>,
    pub state: TddState,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub duration_ms: u128,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitIdentity {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub staged: bool,
    pub diff_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TddEvidenceStatus {
    NoEvidence,
    RedCaptured,
    TddPass,
    PassNoRed,
    StalePass,
    Fail,
}

impl TddEvidenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoEvidence => "no-evidence",
            Self::RedCaptured => "red",
            Self::TddPass => "tdd-pass",
            Self::PassNoRed => "pass-no-red",
            Self::StalePass => "stale-pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EvidenceQuery {
    pub command_hash: Option<String>,
    pub change: Option<String>,
    pub scenario: Option<String>,
    pub task: Option<String>,
    pub current_diff_hash: Option<String>,
}

pub fn current_diff_hash(cwd: &Path, scopes: &[PathBuf]) -> String {
    capture_git_identity(cwd, scopes).diff_hash
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavepointSummary {
    pub kind: String,
    pub event_id: String,
    pub transition: String,
    pub command_hash: String,
    pub current_exit: Option<i32>,
    pub change: Option<String>,
    pub scenario: Option<String>,
    pub task: Option<String>,
    pub worktree_diff_hash_after: String,
    pub created_at_ms: u128,
}

impl From<&SavepointEvent> for SavepointSummary {
    fn from(event: &SavepointEvent) -> Self {
        Self {
            kind: "tdd_savepoint_summary".to_string(),
            event_id: event.event_id.clone(),
            transition: event.transition.clone(),
            command_hash: event.command_hash.clone(),
            current_exit: event.current_exit,
            change: event.change.clone(),
            scenario: event.scenario.clone(),
            task: event.task.clone(),
            worktree_diff_hash_after: event.worktree_diff_hash_after.clone(),
            created_at_ms: event.created_at_ms,
        }
    }
}

impl From<SavepointSummary> for SavepointEvent {
    fn from(summary: SavepointSummary) -> Self {
        Self {
            kind: summary.kind,
            event_id: summary.event_id,
            transition: summary.transition,
            command: Vec::new(),
            command_hash: summary.command_hash,
            previous_exit: None,
            current_exit: summary.current_exit,
            watched_paths: Vec::new(),
            branch: None,
            head_before: None,
            head_after: None,
            worktree_diff_hash_before: String::new(),
            worktree_diff_hash_after: summary.worktree_diff_hash_after,
            dirty_before: false,
            dirty_after: false,
            commit: None,
            change: summary.change,
            scenario: summary.scenario,
            task: summary.task,
            created_at_ms: summary.created_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavepointEvent {
    pub kind: String,
    pub event_id: String,
    pub transition: String,
    pub command: Vec<String>,
    pub command_hash: String,
    pub previous_exit: Option<i32>,
    pub current_exit: Option<i32>,
    pub watched_paths: Vec<PathBuf>,
    pub branch: Option<String>,
    pub head_before: Option<String>,
    pub head_after: Option<String>,
    pub worktree_diff_hash_before: String,
    pub worktree_diff_hash_after: String,
    pub dirty_before: bool,
    pub dirty_after: bool,
    pub commit: Option<String>,
    pub change: Option<String>,
    pub scenario: Option<String>,
    pub task: Option<String>,
    pub created_at_ms: u128,
}

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub cwd: PathBuf,
    pub filetype: Option<String>,
    pub watch_paths: Vec<PathBuf>,
    pub command: TddCommand,
    pub change: Option<String>,
    pub scenario: Option<String>,
    pub task: Option<String>,
    pub once: bool,
    pub emit_baseline: bool,
    pub persist_failures: bool,
    pub timeout: Option<Duration>,
}

pub fn run_command(cwd: &Path, command: &TddCommand) -> Result<RunOutcome> {
    run_command_with_timeout(cwd, command, None)
}

pub fn run_command_with_timeout(
    cwd: &Path,
    command: &TddCommand,
    timeout: Option<Duration>,
) -> Result<RunOutcome> {
    let start = std::time::Instant::now();
    let mut child = Command::new(&command.argv[0])
        .args(&command.argv[1..])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {:?}", command.argv))?;

    if let Some(timeout) = timeout {
        loop {
            if let Some(status) = child.try_wait()? {
                let output = child.wait_with_output()?;
                return Ok(outcome_from_parts(
                    status.code(),
                    status.success(),
                    &output.stdout,
                    &output.stderr,
                    start.elapsed(),
                    false,
                ));
            }
            if start.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                return Ok(outcome_from_parts(
                    None,
                    false,
                    &output.stdout,
                    &output.stderr,
                    start.elapsed(),
                    true,
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    let output = child.wait_with_output()?;
    Ok(outcome_from_parts(
        output.status.code(),
        output.status.success(),
        &output.stdout,
        &output.stderr,
        start.elapsed(),
        false,
    ))
}

fn outcome_from_parts(
    exit_code: Option<i32>,
    success: bool,
    stdout: &[u8],
    stderr: &[u8],
    duration: Duration,
    timed_out: bool,
) -> RunOutcome {
    let state = if success {
        TddState::Passing
    } else {
        TddState::Failing
    };
    RunOutcome {
        exit_code,
        state,
        stdout_tail: tail_string(&String::from_utf8_lossy(stdout), 8192),
        stderr_tail: tail_string(&String::from_utf8_lossy(stderr), 8192),
        duration_ms: duration.as_millis(),
        timed_out,
    }
}

fn tail_string(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[s.len() - max..].to_string()
    }
}

pub fn capture_git_identity(cwd: &Path, scopes: &[PathBuf]) -> GitIdentity {
    let branch = git_output(cwd, &["branch", "--show-current"])
        .ok()
        .filter(|s| !s.is_empty());
    let head = git_output(cwd, &["rev-parse", "HEAD"])
        .ok()
        .filter(|s| !s.is_empty());
    let porcelain = git_output(cwd, &["status", "--porcelain=v1"]).unwrap_or_default();
    let dirty = !porcelain.trim().is_empty();
    let staged = porcelain
        .lines()
        .any(|l| !l.starts_with("??") && !l.starts_with(' ') && !l.starts_with("  "));
    let diff_material = scoped_diff_material(cwd, scopes);
    let mut hasher = Sha256::new();
    hasher.update(diff_material.as_bytes());
    GitIdentity {
        branch,
        head,
        dirty,
        staged,
        diff_hash: format!("sha256:{:x}", hasher.finalize()),
    }
}

fn scoped_diff_material(cwd: &Path, scopes: &[PathBuf]) -> String {
    let mut args = vec!["diff", "--binary", "--"];
    let scope_strings: Vec<String> = if scopes.is_empty() {
        vec![".".to_string()]
    } else {
        scopes
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    };
    for s in &scope_strings {
        args.push(s);
    }
    let mut material = git_output(cwd, &args).unwrap_or_default();
    let mut ls_args = vec!["ls-files", "--others", "--exclude-standard", "--"];
    for s in &scope_strings {
        ls_args.push(s);
    }
    if let Ok(untracked) = git_output(cwd, &ls_args) {
        material.push_str("\n-- untracked --\n");
        material.push_str(&untracked);
        for path in untracked.lines() {
            let full = cwd.join(path);
            if let Ok(bytes) = fs::read(&full) {
                material.push_str("\n-- untracked-content ");
                material.push_str(path);
                material.push_str(" --\n");
                material.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }
    material
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        bail!("git {:?} failed", args);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn duplicate_failure_event_exists(cwd: &Path, event: &SavepointEvent) -> Result<bool> {
    if event.transition != "fail" {
        return Ok(false);
    }
    let query = EvidenceQuery {
        command_hash: Some(event.command_hash.clone()),
        ..EvidenceQuery::default()
    };
    Ok(read_events(cwd, &query)?.iter().any(|existing| {
        existing.transition == "fail"
            && existing.command_hash == event.command_hash
            && existing.worktree_diff_hash_after == event.worktree_diff_hash_after
    }))
}

pub fn append_event(cwd: &Path, event: &SavepointEvent) -> Result<PathBuf> {
    let dir = cwd.join(".omegon/lifecycle/savepoints");
    fs::create_dir_all(&dir)?;
    let safe = event.command_hash.replace(':', "_");
    let path = dir.join(format!("{safe}.jsonl"));
    if duplicate_failure_event_exists(cwd, event)? {
        return Ok(path);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    project_openspec_summary(cwd, event)?;
    Ok(path)
}

fn project_openspec_summary(cwd: &Path, event: &SavepointEvent) -> Result<()> {
    let Some(change) = &event.change else {
        return Ok(());
    };
    let dir = cwd.join("openspec/changes").join(change);
    if !dir.is_dir() {
        return Ok(());
    }
    let evidence_dir = dir.join("evidence");
    fs::create_dir_all(&evidence_dir)?;
    let path = evidence_dir.join("tdd-savepoints.jsonl");
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, &SavepointSummary::from(event))?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn read_events(cwd: &Path, query: &EvidenceQuery) -> Result<Vec<SavepointEvent>> {
    let mut events = Vec::new();
    if let Some(change) = &query.change {
        let path = cwd
            .join("openspec/changes")
            .join(change)
            .join("evidence")
            .join("tdd-savepoints.jsonl");
        read_event_file(&path, query, &mut events)?;
        let legacy_path = cwd
            .join("openspec/changes")
            .join(change)
            .join("tdd-savepoints.jsonl");
        read_event_file(&legacy_path, query, &mut events)?;
        return Ok(events);
    }

    let dir = cwd.join(".omegon/lifecycle/savepoints");
    if !dir.is_dir() {
        return Ok(events);
    }
    if let Some(command_hash) = &query.command_hash {
        let safe = command_hash.replace(':', "_");
        read_event_file(&dir.join(format!("{safe}.jsonl")), query, &mut events)?;
        return Ok(events);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().extension() == Some(OsStr::new("jsonl")) {
            read_event_file(&entry.path(), query, &mut events)?;
        }
    }
    Ok(events)
}

fn read_event_file(
    path: &Path,
    query: &EvidenceQuery,
    out: &mut Vec<SavepointEvent>,
) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let file = File::open(path)?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event = match serde_json::from_str::<SavepointEvent>(&line) {
            Ok(event) => event,
            Err(_) => match serde_json::from_str::<SavepointSummary>(&line) {
                Ok(summary) => SavepointEvent::from(summary),
                Err(_) => continue,
            },
        };
        if event_matches(&event, query) {
            out.push(event);
        }
    }
    Ok(())
}

fn event_matches(event: &SavepointEvent, query: &EvidenceQuery) -> bool {
    query
        .command_hash
        .as_ref()
        .is_none_or(|v| &event.command_hash == v)
        && query
            .change
            .as_ref()
            .is_none_or(|v| event.change.as_ref() == Some(v))
        && query
            .scenario
            .as_ref()
            .is_none_or(|v| event.scenario.as_ref() == Some(v))
        && query
            .task
            .as_ref()
            .is_none_or(|v| event.task.as_ref() == Some(v))
}

pub fn classify_evidence(events: &[SavepointEvent], query: &EvidenceQuery) -> TddEvidenceStatus {
    if events.iter().any(|e| e.transition == "failing_to_passing") {
        if let Some(current) = &query.current_diff_hash {
            if events
                .iter()
                .filter(|e| e.transition == "failing_to_passing")
                .any(|e| &e.worktree_diff_hash_after == current)
            {
                TddEvidenceStatus::TddPass
            } else {
                TddEvidenceStatus::StalePass
            }
        } else {
            TddEvidenceStatus::TddPass
        }
    } else if events
        .iter()
        .any(|e| e.transition == "baseline" && e.current_exit != Some(0))
    {
        TddEvidenceStatus::RedCaptured
    } else if events
        .iter()
        .any(|e| e.transition == "baseline" && e.current_exit == Some(0))
    {
        TddEvidenceStatus::PassNoRed
    } else if events
        .iter()
        .any(|e| e.transition == "fail" || e.current_exit != Some(0))
    {
        TddEvidenceStatus::Fail
    } else {
        TddEvidenceStatus::NoEvidence
    }
}

pub fn evidence_status(cwd: &Path, query: &EvidenceQuery) -> Result<TddEvidenceStatus> {
    let events = read_events(cwd, query)?;
    Ok(classify_evidence(&events, query))
}

pub fn watch(opts: WatchOptions) -> Result<()> {
    let mut previous_git = capture_git_identity(&opts.cwd, &opts.watch_paths);
    let mut previous = run_command_with_timeout(&opts.cwd, &opts.command, opts.timeout)?;
    println!(
        "tdd baseline: {:?} exit={:?} command_hash={}",
        previous.state, previous.exit_code, opts.command.hash
    );
    let paths = if opts.watch_paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        opts.watch_paths.clone()
    };
    if opts.emit_baseline {
        let event = build_event(
            &opts,
            EventBuild {
                event_id: "baseline",
                transition: "baseline",
                previous: &previous,
                current: &previous,
                previous_git: &previous_git,
                current_git: &previous_git,
                paths: &paths,
            },
        );
        let path = append_event(&opts.cwd, &event)?;
        println!(
            "tdd baseline event: {} -> {}",
            event.event_id,
            path.display()
        );
    }
    if opts.once {
        return Ok(());
    }

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    for path in &paths {
        watcher.watch(&opts.cwd.join(path), RecursiveMode::Recursive)?;
    }

    loop {
        wait_for_relevant_event(&rx, opts.filetype.as_deref())?;
        let current = run_command_with_timeout(&opts.cwd, &opts.command, opts.timeout)?;
        let current_git = capture_git_identity(&opts.cwd, &opts.watch_paths);
        println!("tdd run: {:?} exit={:?}", current.state, current.exit_code);
        if opts.persist_failures && current.state == TddState::Failing {
            let event_id = format!("fail-{}", Uuid::new_v4());
            let event = build_event(
                &opts,
                EventBuild {
                    event_id: &event_id,
                    transition: "fail",
                    previous: &previous,
                    current: &current,
                    previous_git: &previous_git,
                    current_git: &current_git,
                    paths: &paths,
                },
            );
            let path = append_event(&opts.cwd, &event)?;
            println!(
                "tdd failure evidence: {} -> {}",
                event.event_id,
                path.display()
            );
        }
        if previous.state == TddState::Failing && current.state == TddState::Passing {
            let event_id = format!("redgreen-{}", Uuid::new_v4());
            let event = build_event(
                &opts,
                EventBuild {
                    event_id: &event_id,
                    transition: "failing_to_passing",
                    previous: &previous,
                    current: &current,
                    previous_git: &previous_git,
                    current_git: &current_git,
                    paths: &paths,
                },
            );
            let path = append_event(&opts.cwd, &event)?;
            println!("tdd savepoint: {} -> {}", event.event_id, path.display());
        }
        previous = current;
        previous_git = current_git;
    }
}

struct EventBuild<'a> {
    event_id: &'a str,
    transition: &'a str,
    previous: &'a RunOutcome,
    current: &'a RunOutcome,
    previous_git: &'a GitIdentity,
    current_git: &'a GitIdentity,
    paths: &'a [PathBuf],
}

fn build_event(opts: &WatchOptions, build: EventBuild<'_>) -> SavepointEvent {
    SavepointEvent {
        kind: "tdd_savepoint".to_string(),
        event_id: build.event_id.to_string(),
        transition: build.transition.to_string(),
        command: opts.command.argv.clone(),
        command_hash: opts.command.hash.clone(),
        previous_exit: build.previous.exit_code,
        current_exit: build.current.exit_code,
        watched_paths: build.paths.to_vec(),
        branch: build
            .current_git
            .branch
            .clone()
            .or_else(|| build.previous_git.branch.clone()),
        head_before: build.previous_git.head.clone(),
        head_after: build.current_git.head.clone(),
        worktree_diff_hash_before: build.previous_git.diff_hash.clone(),
        worktree_diff_hash_after: build.current_git.diff_hash.clone(),
        dirty_before: build.previous_git.dirty,
        dirty_after: build.current_git.dirty,
        commit: None,
        change: opts.change.clone(),
        scenario: opts.scenario.clone(),
        task: opts.task.clone(),
        created_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    }
}

fn wait_for_relevant_event(
    rx: &mpsc::Receiver<notify::Result<Event>>,
    filetype: Option<&str>,
) -> Result<()> {
    loop {
        match rx.recv()? {
            Ok(Event {
                kind: EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_),
                paths,
                ..
            }) => {
                if filetype.is_none()
                    || paths
                        .iter()
                        .any(|p| p.extension() == filetype.map(OsStr::new))
                {
                    break;
                }
            }
            Ok(_) => {}
            Err(err) => return Err(err.into()),
        }
    }
    while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn command_hash_is_stable_for_same_argv() {
        let a = TddCommand::new(vec!["test-runner".into(), "test".into()]).unwrap();
        let b = TddCommand::new(vec!["test-runner".into(), "test".into()]).unwrap();
        assert_eq!(a.hash, b.hash);
    }

    #[test]
    fn command_hash_changes_for_different_argv() {
        let a = TddCommand::new(vec!["test-runner".into(), "test".into()]).unwrap();
        let b = TddCommand::new(vec!["test-runner".into(), "check".into()]).unwrap();
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn runner_classifies_pass_and_fail() {
        let cwd = std::env::current_dir().unwrap();
        let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".to_string());
        let pass = TddCommand::new(vec![
            python.clone(),
            "-c".into(),
            "raise SystemExit(0)".into(),
        ])
        .unwrap();
        assert_eq!(run_command(&cwd, &pass).unwrap().state, TddState::Passing);
        let fail =
            TddCommand::new(vec![python, "-c".into(), "raise SystemExit(7)".into()]).unwrap();
        assert_eq!(run_command(&cwd, &fail).unwrap().state, TddState::Failing);
    }

    #[test]
    fn event_roundtrips_to_jsonl() {
        let dir = tempdir().unwrap();
        let event = SavepointEvent {
            kind: "tdd_savepoint".into(),
            event_id: "redgreen-test".into(),
            transition: "failing_to_passing".into(),
            command: vec!["true".into()],
            command_hash: "sha256:test".into(),
            previous_exit: Some(1),
            current_exit: Some(0),
            watched_paths: vec![PathBuf::from(".")],
            branch: None,
            head_before: None,
            head_after: None,
            worktree_diff_hash_before: "a".into(),
            worktree_diff_hash_after: "b".into(),
            dirty_before: true,
            dirty_after: false,
            commit: None,
            change: None,
            scenario: None,
            task: None,
            created_at_ms: 0,
        };
        let path = append_event(dir.path(), &event).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("redgreen-test"));
    }

    #[test]
    fn attributed_event_projects_openspec_summary() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("openspec/changes/demo-change")).unwrap();
        let event = SavepointEvent {
            kind: "tdd_savepoint".into(),
            event_id: "redgreen-demo".into(),
            transition: "failing_to_passing".into(),
            command: vec!["true".into()],
            command_hash: "sha256:demo".into(),
            previous_exit: Some(1),
            current_exit: Some(0),
            watched_paths: vec![PathBuf::from(".")],
            branch: None,
            head_before: None,
            head_after: None,
            worktree_diff_hash_before: "a".into(),
            worktree_diff_hash_after: "b".into(),
            dirty_before: true,
            dirty_after: false,
            commit: None,
            change: Some("demo-change".into()),
            scenario: Some("demo/scenario".into()),
            task: Some("1.1".into()),
            created_at_ms: 0,
        };
        append_event(dir.path(), &event).unwrap();
        let text = fs::read_to_string(
            dir.path()
                .join("openspec/changes/demo-change/evidence/tdd-savepoints.jsonl"),
        )
        .unwrap();
        assert!(text.contains("redgreen-demo"));
        assert!(text.contains("demo/scenario"));

        let status = evidence_status(
            dir.path(),
            &EvidenceQuery {
                change: Some("demo-change".into()),
                scenario: Some("demo/scenario".into()),
                current_diff_hash: Some("b".into()),
                ..EvidenceQuery::default()
            },
        )
        .unwrap();
        assert_eq!(status, TddEvidenceStatus::TddPass);
    }

    #[test]
    fn evidence_classification_distinguishes_red_green_and_stale() {
        let baseline_red = SavepointEvent {
            kind: "tdd_savepoint".into(),
            event_id: "baseline".into(),
            transition: "baseline".into(),
            command: vec!["false".into()],
            command_hash: "sha256:evidence".into(),
            previous_exit: Some(1),
            current_exit: Some(1),
            watched_paths: vec![PathBuf::from(".")],
            branch: None,
            head_before: None,
            head_after: None,
            worktree_diff_hash_before: "red".into(),
            worktree_diff_hash_after: "red".into(),
            dirty_before: true,
            dirty_after: true,
            commit: None,
            change: None,
            scenario: None,
            task: None,
            created_at_ms: 0,
        };
        assert_eq!(
            classify_evidence(
                std::slice::from_ref(&baseline_red),
                &EvidenceQuery::default()
            ),
            TddEvidenceStatus::RedCaptured
        );

        let mut green = baseline_red.clone();
        green.event_id = "redgreen".into();
        green.transition = "failing_to_passing".into();
        green.previous_exit = Some(1);
        green.current_exit = Some(0);
        green.worktree_diff_hash_after = "green".into();
        assert_eq!(
            classify_evidence(
                &[green.clone()],
                &EvidenceQuery {
                    current_diff_hash: Some("green".into()),
                    ..EvidenceQuery::default()
                }
            ),
            TddEvidenceStatus::TddPass
        );
        assert_eq!(
            classify_evidence(
                &[green],
                &EvidenceQuery {
                    current_diff_hash: Some("newer".into()),
                    ..EvidenceQuery::default()
                }
            ),
            TddEvidenceStatus::StalePass
        );
    }

    #[test]
    fn duplicate_failure_events_are_deduped_by_command_and_diff_hash() {
        let dir = tempdir().unwrap();
        let event = SavepointEvent {
            kind: "tdd_savepoint".into(),
            event_id: "fail-one".into(),
            transition: "fail".into(),
            command: vec!["false".into()],
            command_hash: "sha256:fail".into(),
            previous_exit: Some(1),
            current_exit: Some(1),
            watched_paths: vec![PathBuf::from(".")],
            branch: None,
            head_before: None,
            head_after: None,
            worktree_diff_hash_before: "same".into(),
            worktree_diff_hash_after: "same".into(),
            dirty_before: true,
            dirty_after: true,
            commit: None,
            change: None,
            scenario: None,
            task: None,
            created_at_ms: 0,
        };
        append_event(dir.path(), &event).unwrap();
        let mut duplicate = event.clone();
        duplicate.event_id = "fail-two".into();
        append_event(dir.path(), &duplicate).unwrap();
        let events = read_events(
            dir.path(),
            &EvidenceQuery {
                command_hash: Some("sha256:fail".into()),
                ..EvidenceQuery::default()
            },
        )
        .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn timeout_classifies_as_failing() {
        let cwd = std::env::current_dir().unwrap();
        let command = TddCommand::new(vec![
            "python3".into(),
            "-c".into(),
            "import time; time.sleep(2)".into(),
        ])
        .unwrap();
        let outcome =
            run_command_with_timeout(&cwd, &command, Some(Duration::from_millis(50))).unwrap();
        assert_eq!(outcome.state, TddState::Failing);
        assert!(outcome.timed_out);
    }

    #[test]
    fn git_identity_includes_untracked_content() {
        let dir = tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        fs::write(dir.path().join("new_test.txt"), "generic test fixture").unwrap();
        let id = capture_git_identity(dir.path(), &[PathBuf::from(".")]);
        assert!(id.dirty);
        assert!(id.diff_hash.starts_with("sha256:"));
    }
}

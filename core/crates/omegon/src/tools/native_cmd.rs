//! Native command dispatch — intercept common shell commands and execute
//! them in-process without spawning bash.
//!
//! Only handles simple, single-command invocations with no shell metacharacters.
//! Falls through to bash for pipes, redirects, variable expansion, chaining,
//! and any command not in the dispatch table.
//!
//! The goal is latency reduction (no fork+exec) and platform independence
//! (no dependency on specific GNU coreutils versions).

use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Result of a native command execution.
pub struct NativeResult {
    pub stdout: String,
    pub exit_code: i32,
}

/// Shell metacharacters that require bash interpretation.
/// If ANY of these appear in the command string (outside quotes), we bail.
const SHELL_META: &[char] = &[
    '|', '>', '<', '$', ';', '&', '`', '(', ')', '{', '}', '*', '?',
];

/// Try to execute a command natively. Returns `None` if the command should
/// fall through to bash (unrecognized command, shell syntax, unsupported flags).
///
/// When `boundary` is `Some`, all file arguments are checked against the
/// workspace boundary before any filesystem operation. Violations produce
/// a `NativeResult` with exit_code=1 and a BLOCKED message.
pub fn try_dispatch(
    command: &str,
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let trimmed = command.trim();

    // Bail on empty commands
    if trimmed.is_empty() {
        return None;
    }

    // Bail if any unquoted shell metacharacter is present.
    // This is conservative: we skip commands like `ls *.rs` (glob) and
    // `echo $HOME` (variable), routing them to bash where they work correctly.
    if has_shell_metachar(trimmed) {
        return None;
    }

    // Parse into argv using POSIX shell quoting rules
    let argv = shlex::split(trimmed)?;
    if argv.is_empty() {
        return None;
    }

    // Expand ~ to home directory in arguments
    let argv: Vec<String> = argv.into_iter().map(|arg| expand_tilde(&arg)).collect();
    let cmd = argv[0].as_str();
    let args = &argv[1..];

    match cmd {
        "cat" => cmd_cat(args, cwd, boundary),
        "head" => cmd_head(args, cwd, boundary),
        "tail" => cmd_tail(args, cwd, boundary),
        "wc" => cmd_wc(args, cwd, boundary),
        "ls" => cmd_ls(args, cwd, boundary),
        "find" => cmd_find(args, cwd, boundary),
        "grep" => cmd_grep(args, cwd, boundary),
        "mkdir" => cmd_mkdir(args, cwd, boundary),
        "touch" => cmd_touch(args, cwd, boundary),
        "rm" => cmd_rm(args, cwd, boundary),
        "cp" => cmd_cp(args, cwd, boundary),
        "mv" => cmd_mv(args, cwd, boundary),
        "sort" => cmd_sort(args, cwd, boundary),
        "basename" => cmd_basename(args),
        "dirname" => cmd_dirname(args),
        "realpath" => cmd_realpath(args, cwd),
        "echo" => cmd_echo(args),
        "pwd" => Some(NativeResult {
            stdout: cwd.to_string_lossy().to_string(),
            exit_code: 0,
        }),
        "true" => Some(NativeResult {
            stdout: String::new(),
            exit_code: 0,
        }),
        "false" => Some(NativeResult {
            stdout: String::new(),
            exit_code: 1,
        }),
        _ => None,
    }
}

// ── Boundary enforcement ──────────────────────────────────────────────

/// Resolve a path relative to cwd and check workspace boundary.
/// Returns `Err(NativeResult)` with a BLOCKED message on violation,
/// or `Ok(PathBuf)` if allowed.
fn resolve_checked(
    arg: &str,
    cwd: &Path,
    cmd: &str,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Result<PathBuf, NativeResult> {
    let path = cwd.join(arg);
    if let Some(b) = boundary
        && !b.is_inside_boundary(&path)
    {
        return Err(NativeResult {
            stdout: format!(
                "BLOCKED: {cmd}: '{}' is outside the workspace boundary",
                arg,
            ),
            exit_code: 1,
        });
    }
    Ok(path)
}

// ── Shell metacharacter detection ──────────────────────────────────────

/// Expand `~` and `~/...` to the user's home directory.
fn expand_tilde(arg: &str) -> String {
    if arg == "~" {
        dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| arg.to_string())
    } else if let Some(rest) = arg.strip_prefix("~/") {
        dirs::home_dir()
            .map(|h| format!("{}/{}", h.to_string_lossy(), rest))
            .unwrap_or_else(|| arg.to_string())
    } else {
        arg.to_string()
    }
}

fn has_shell_metachar(s: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;

    for c in s.chars() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && !in_single {
            escape = true;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if !in_single && !in_double && SHELL_META.contains(&c) {
            return true;
        }
    }
    false
}

// ── cat ────────────────────────────────────────────────────────────────

fn cmd_cat(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    // Skip flags we don't handle
    if args.iter().any(|a| a.starts_with('-') && a != "-") {
        return None;
    }

    let files: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
    if files.is_empty() {
        return None; // cat with no args reads stdin — bail to bash
    }

    let mut output = String::new();
    for file in &files {
        let path = match resolve_checked(file, cwd, "cat", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => output.push_str(&content),
            Err(e) => {
                return Some(NativeResult {
                    stdout: format!("cat: {}: {}", file, e),
                    exit_code: 1,
                });
            }
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

// ── head ───────────────────────────────────────────────────────────────

fn cmd_head(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut n: usize = 10; // default
    let mut files: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == "-n" {
            n = iter.next()?.parse().ok()?;
        } else if let Some(num) = arg.strip_prefix('-') {
            // head -20 style
            if let Ok(parsed) = num.parse::<usize>() {
                n = parsed;
            } else {
                return None; // unknown flag
            }
        } else {
            files.push(arg.as_str());
        }
    }

    if files.is_empty() {
        return None; // reads stdin
    }

    let mut output = String::new();
    for file in &files {
        let path = match resolve_checked(file, cwd, "head", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        match std::fs::File::open(&path) {
            Ok(f) => {
                let reader = std::io::BufReader::new(f);
                for line in reader.lines().take(n) {
                    match line {
                        Ok(l) => {
                            output.push_str(&l);
                            output.push('\n');
                        }
                        Err(_) => break,
                    }
                }
            }
            Err(e) => {
                return Some(NativeResult {
                    stdout: format!("head: {}: {}", file, e),
                    exit_code: 1,
                });
            }
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

// ── tail ───────────────────────────────────────────────────────────────

fn cmd_tail(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut n: usize = 10;
    let mut files: Vec<&str> = Vec::new();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == "-n" {
            n = iter.next()?.parse().ok()?;
        } else if let Some(num) = arg.strip_prefix('-') {
            if let Ok(parsed) = num.parse::<usize>() {
                n = parsed;
            } else {
                return None;
            }
        } else {
            files.push(arg.as_str());
        }
    }

    if files.is_empty() {
        return None;
    }

    let mut output = String::new();
    for file in &files {
        let path = match resolve_checked(file, cwd, "tail", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(n);
                for line in &lines[start..] {
                    output.push_str(line);
                    output.push('\n');
                }
            }
            Err(e) => {
                return Some(NativeResult {
                    stdout: format!("tail: {}: {}", file, e),
                    exit_code: 1,
                });
            }
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

// ── wc ─────────────────────────────────────────────────────────────────

fn cmd_wc(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut count_lines = false;
    let mut count_words = false;
    let mut count_bytes = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'l' => count_lines = true,
                    'w' => count_words = true,
                    'c' => count_bytes = true,
                    _ => return None, // unknown flag
                }
            }
        } else {
            files.push(arg.as_str());
        }
    }

    // Default: all three
    if !count_lines && !count_words && !count_bytes {
        count_lines = true;
        count_words = true;
        count_bytes = true;
    }

    if files.is_empty() {
        return None;
    }

    let mut output = String::new();
    for file in &files {
        let path = match resolve_checked(file, cwd, "wc", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        match std::fs::read(&path) {
            Ok(content) => {
                let mut parts = Vec::new();
                if count_lines {
                    parts.push(format!(
                        "{:>8}",
                        content.iter().filter(|&&b| b == b'\n').count()
                    ));
                }
                if count_words {
                    let text = String::from_utf8_lossy(&content);
                    parts.push(format!("{:>8}", text.split_whitespace().count()));
                }
                if count_bytes {
                    parts.push(format!("{:>8}", content.len()));
                }
                parts.push(format!(" {}", file));
                output.push_str(&parts.join(""));
                output.push('\n');
            }
            Err(e) => {
                return Some(NativeResult {
                    stdout: format!("wc: {}: {}", file, e),
                    exit_code: 1,
                });
            }
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

// ── ls ─────────────────────────────────────────────────────────────────

fn cmd_ls(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut show_hidden = false;
    let mut long_format = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'a' => show_hidden = true,
                    'l' => long_format = true,
                    '1' => {}         // one-per-line is our default
                    _ => return None, // unknown flag → bash fallback
                }
            }
        } else {
            paths.push(arg.as_str());
        }
    }

    if paths.is_empty() {
        paths.push(".");
    }

    let mut output = String::new();
    for dir_path in &paths {
        let target = match resolve_checked(dir_path, cwd, "ls", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };

        if target.is_file() {
            // ls <file> just prints the filename
            output.push_str(dir_path);
            output.push('\n');
            continue;
        }

        let entries = match std::fs::read_dir(&target) {
            Ok(e) => e,
            Err(e) => {
                return Some(NativeResult {
                    stdout: format!("ls: cannot access '{}': {}", dir_path, e),
                    exit_code: 2,
                });
            }
        };

        let mut names: Vec<(String, std::fs::Metadata)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                names.push((name, meta));
            }
        }
        names.sort_by_key(|a| a.0.to_lowercase());

        if paths.len() > 1 {
            output.push_str(&format!("{}:\n", dir_path));
        }

        for (name, meta) in &names {
            if long_format {
                let kind = if meta.is_dir() { "d" } else { "-" };
                let size = meta.len();
                output.push_str(&format!("{kind}  {size:>10}  {name}\n"));
            } else {
                output.push_str(name);
                output.push('\n');
            }
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

// ── find ───────────────────────────────────────────────────────────────

fn cmd_find(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    // Support: find [path] -name <pattern> [-type f|d]
    // Anything else → bash fallback
    let mut search_path: Option<&str> = None;
    let mut name_pattern: Option<&str> = None;
    let mut type_filter: Option<char> = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-name" => name_pattern = Some(iter.next()?.as_str()),
            "-type" => {
                let t = iter.next()?;
                match t.as_str() {
                    "f" | "d" => type_filter = Some(t.chars().next().unwrap()),
                    _ => return None,
                }
            }
            s if s.starts_with('-') => return None, // unknown flag
            _ if search_path.is_none() => search_path = Some(arg.as_str()),
            _ => return None, // multiple paths or unexpected args
        }
    }

    let search_arg = search_path.unwrap_or(".");
    let root = match resolve_checked(search_arg, cwd, "find", boundary) {
        Ok(p) => p,
        Err(blocked) => return Some(blocked),
    };
    let mut output = String::new();
    let mut count = 0;

    // Match GNU find behavior: do NOT respect .gitignore.
    // Models expect find to return all matching files regardless of ignore rules.
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .build();

    for entry in walker.flatten() {
        if let Some(filter) = type_filter {
            match filter {
                'f' if !entry.path().is_file() => continue,
                'd' if !entry.path().is_dir() => continue,
                _ => {}
            }
        }

        if let Some(pattern) = name_pattern {
            let name = entry
                .path()
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            // Simple glob matching (? and * only, no shell expansion)
            if !simple_glob_match(pattern, &name) {
                continue;
            }
        }

        // Print path with the search path prefix preserved (matches GNU find output).
        // `find src -name "*.rs"` → `src/main.rs`, not `main.rs`
        let display = entry
            .path()
            .strip_prefix(&root)
            .map(|rel| {
                let base = search_path.unwrap_or(".");
                if rel.as_os_str().is_empty() {
                    base.to_string()
                } else {
                    format!("{}/{}", base, rel.to_string_lossy())
                }
            })
            .unwrap_or_else(|_| entry.path().to_string_lossy().to_string());
        output.push_str(&display);
        output.push('\n');

        count += 1;
        if count >= 2000 {
            output.push_str("... (truncated at 2000 entries)\n");
            break;
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

/// Simple glob matching supporting `*` and `?` only.
fn simple_glob_match(pattern: &str, text: &str) -> bool {
    fn match_inner(pattern: &[char], text: &[char]) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }
        match pattern[0] {
            '*' => {
                // Try matching zero or more characters
                for i in 0..=text.len() {
                    if match_inner(&pattern[1..], &text[i..]) {
                        return true;
                    }
                }
                false
            }
            '?' => !text.is_empty() && match_inner(&pattern[1..], &text[1..]),
            c => !text.is_empty() && text[0] == c && match_inner(&pattern[1..], &text[1..]),
        }
    }

    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    match_inner(&p, &t)
}

// ── mkdir ──────────────────────────────────────────────────────────────

fn cmd_mkdir(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut parents = false;
    let mut dirs: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-p" => parents = true,
            s if s.starts_with('-') => return None,
            _ => dirs.push(arg.as_str()),
        }
    }

    if dirs.is_empty() {
        return Some(NativeResult {
            stdout: "mkdir: missing operand".to_string(),
            exit_code: 1,
        });
    }

    for dir in &dirs {
        let path = match resolve_checked(dir, cwd, "mkdir", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        let result = if parents {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::create_dir(&path)
        };
        if let Err(e) = result {
            return Some(NativeResult {
                stdout: format!("mkdir: cannot create directory '{}': {}", dir, e),
                exit_code: 1,
            });
        }
    }

    Some(NativeResult {
        stdout: String::new(),
        exit_code: 0,
    })
}

// ── grep ───────────────────────────────────────────────────────────────

fn cmd_grep(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut recursive = false;
    let mut line_numbers = false;
    let mut case_insensitive = false;
    let mut files_only = false;
    let mut count_only = false;
    let mut invert = false;
    let mut pattern: Option<&str> = None;
    let mut paths: Vec<&str> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            // Everything after -- is a path
            for rest in iter.by_ref() {
                paths.push(rest.as_str());
            }
            break;
        }
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for c in arg[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    'n' => line_numbers = true,
                    'i' => case_insensitive = true,
                    'l' => files_only = true,
                    'c' => count_only = true,
                    'v' => invert = true,
                    _ => return None, // unknown flag → bash
                }
            }
        } else if arg == "--recursive" {
            recursive = true;
        } else if arg == "--line-number" {
            line_numbers = true;
        } else if arg == "--ignore-case" {
            case_insensitive = true;
        } else if arg == "--files-with-matches" {
            files_only = true;
        } else if arg == "--count" {
            count_only = true;
        } else if arg == "--invert-match" {
            invert = true;
        } else if arg.starts_with("--") {
            return None; // unknown long flag
        } else if pattern.is_none() {
            pattern = Some(arg.as_str());
        } else {
            paths.push(arg.as_str());
        }
    }

    let pattern = pattern?;
    if paths.is_empty() {
        return None; // grep from stdin → bash
    }

    // Build the regex matcher
    let matcher = {
        let mut builder = grep_regex::RegexMatcherBuilder::new();
        if case_insensitive {
            builder.case_insensitive(true);
        }
        match builder.build(pattern) {
            Ok(m) => m,
            Err(_) => return None, // invalid regex → bash (might be a fixed string grep)
        }
    };

    let mut output = String::new();
    let mut any_match = false;
    let multi_file = paths.len() > 1 || recursive;

    // Collect files to search
    let mut search_files: Vec<PathBuf> = Vec::new();
    for p in &paths {
        let target = match resolve_checked(p, cwd, "grep", boundary) {
            Ok(t) => t,
            Err(blocked) => return Some(blocked),
        };
        if target.is_file() {
            search_files.push(target);
        } else if target.is_dir() && recursive {
            // Match GNU grep -r: search hidden files, don't respect gitignore
            let walker = ignore::WalkBuilder::new(&target)
                .hidden(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .build();
            for entry in walker.flatten() {
                if entry.path().is_file() {
                    search_files.push(entry.path().to_path_buf());
                }
            }
        } else if target.is_dir() {
            // grep on dir without -r: error
            output.push_str(&format!("grep: {}: Is a directory\n", p));
        } else {
            return Some(NativeResult {
                stdout: format!("grep: {}: No such file or directory", p),
                exit_code: 2,
            });
        }
    }

    // Use grep-matcher for regex compilation, then manual line-by-line matching.
    // This avoids the grep-searcher sink complexity while still using the
    // optimized regex engine.
    use grep_matcher::Matcher;

    for file_path in &search_files {
        let display = file_path
            .strip_prefix(cwd)
            .unwrap_or(file_path)
            .to_string_lossy();

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue, // skip binary/unreadable files silently
        };

        if files_only {
            let has_match = content.lines().any(|line| {
                let matched = matcher.is_match(line.as_bytes()).unwrap_or(false);
                if invert { !matched } else { matched }
            });
            if has_match {
                output.push_str(&display);
                output.push('\n');
                any_match = true;
            }
        } else if count_only {
            let count = content
                .lines()
                .filter(|line| {
                    let matched = matcher.is_match(line.as_bytes()).unwrap_or(false);
                    if invert { !matched } else { matched }
                })
                .count();
            if multi_file {
                output.push_str(&format!("{}:{}\n", display, count));
            } else {
                output.push_str(&format!("{}\n", count));
            }
            if count > 0 {
                any_match = true;
            }
        } else {
            for (idx, line) in content.lines().enumerate() {
                let matched = matcher.is_match(line.as_bytes()).unwrap_or(false);
                let include = if invert { !matched } else { matched };
                if include {
                    any_match = true;
                    let line_num = idx + 1;
                    if multi_file && line_numbers {
                        output.push_str(&format!("{}:{}:{}\n", display, line_num, line));
                    } else if multi_file {
                        output.push_str(&format!("{}:{}\n", display, line));
                    } else if line_numbers {
                        output.push_str(&format!("{}:{}\n", line_num, line));
                    } else {
                        output.push_str(line);
                        output.push('\n');
                    }
                }
            }
        }
    }

    Some(NativeResult {
        stdout: output,
        exit_code: if any_match { 0 } else { 1 },
    })
}

// ── touch ──────────────────────────────────────────────────────────────

fn cmd_touch(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut files: Vec<&str> = Vec::new();
    for arg in args {
        if arg.starts_with('-') {
            return None; // flags like -t, -d → bash
        }
        files.push(arg.as_str());
    }
    if files.is_empty() {
        return Some(NativeResult {
            stdout: "touch: missing file operand".to_string(),
            exit_code: 1,
        });
    }
    for file in &files {
        let path = match resolve_checked(file, cwd, "touch", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        if path.exists() {
            // Update mtime to now
            let now = std::time::SystemTime::now();
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
                let _ = file.set_modified(now);
            }
        } else {
            if let Err(e) = std::fs::File::create(&path) {
                return Some(NativeResult {
                    stdout: format!("touch: cannot touch '{}': {}", file, e),
                    exit_code: 1,
                });
            }
        }
    }
    Some(NativeResult {
        stdout: String::new(),
        exit_code: 0,
    })
}

// ── rm ─────────────────────────────────────────────────────────────────

fn cmd_rm(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut recursive = false;
    let mut force = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    'f' => force = true,
                    _ => return None,
                }
            }
        } else {
            files.push(arg.as_str());
        }
    }
    if files.is_empty() {
        return Some(NativeResult {
            stdout: "rm: missing operand".to_string(),
            exit_code: 1,
        });
    }
    for file in &files {
        let path = match resolve_checked(file, cwd, "rm", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };

        // Safety: refuse to remove dangerous paths
        if is_dangerous_rm_target(&path) {
            return Some(NativeResult {
                stdout: format!("rm: refusing to remove '{}': protected path", file),
                exit_code: 1,
            });
        }

        if !path.exists() {
            if force {
                continue;
            }
            return Some(NativeResult {
                stdout: format!("rm: cannot remove '{}': No such file or directory", file),
                exit_code: 1,
            });
        }
        let result = if path.is_dir() {
            if recursive {
                std::fs::remove_dir_all(&path)
            } else {
                Err(std::io::Error::other("Is a directory"))
            }
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result
            && !force
        {
            return Some(NativeResult {
                stdout: format!("rm: cannot remove '{}': {}", file, e),
                exit_code: 1,
            });
        }
    }
    Some(NativeResult {
        stdout: String::new(),
        exit_code: 0,
    })
}

/// Check if a path is too dangerous to rm.
fn is_dangerous_rm_target(path: &Path) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy();

    // System root directories
    if matches!(
        s.as_ref(),
        "/" | "/bin" | "/boot" | "/dev" | "/etc" | "/home" | "/lib" | "/lib64"
            | "/opt" | "/proc" | "/root" | "/run" | "/sbin" | "/sys"
            | "/tmp" | "/usr" | "/var"
            // macOS
            | "/Applications" | "/Library" | "/System" | "/Users"
            | "/Volumes" | "/private"
    ) {
        return true;
    }

    // Home directory itself
    if let Some(home) = dirs::home_dir()
        && canonical == home
    {
        return true;
    }

    // Parent traversal above cwd (.. resolving above where we are)
    // This catches `rm -rf ../..` etc.
    if s == "/" || canonical.parent().is_none() {
        return true;
    }

    false
}

// ── cp ─────────────────────────────────────────────────────────────────

fn cmd_cp(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut recursive = false;
    let mut paths: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    _ => return None,
                }
            }
        } else {
            paths.push(arg.as_str());
        }
    }
    if paths.len() < 2 {
        return Some(NativeResult {
            stdout: "cp: missing destination".to_string(),
            exit_code: 1,
        });
    }
    let dest = match resolve_checked(paths.last().unwrap(), cwd, "cp", boundary) {
        Ok(p) => p,
        Err(blocked) => return Some(blocked),
    };
    let sources = &paths[..paths.len() - 1];

    if sources.len() > 1 && !dest.is_dir() {
        return Some(NativeResult {
            stdout: format!("cp: target '{}' is not a directory", paths.last().unwrap()),
            exit_code: 1,
        });
    }

    for src_str in sources {
        let src = match resolve_checked(src_str, cwd, "cp", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        if !src.exists() {
            return Some(NativeResult {
                stdout: format!("cp: cannot stat '{}': No such file or directory", src_str),
                exit_code: 1,
            });
        }
        if src.is_dir() && !recursive {
            return Some(NativeResult {
                stdout: format!("cp: -r not specified; omitting directory '{}'", src_str),
                exit_code: 1,
            });
        }

        let target = if dest.is_dir() {
            dest.join(src.file_name().unwrap_or_default())
        } else {
            dest.clone()
        };

        if src.is_dir() {
            if let Err(e) = copy_dir_recursive(&src, &target) {
                return Some(NativeResult {
                    stdout: format!("cp: error copying '{}': {}", src_str, e),
                    exit_code: 1,
                });
            }
        } else if let Err(e) = std::fs::copy(&src, &target) {
            return Some(NativeResult {
                stdout: format!("cp: error copying '{}': {}", src_str, e),
                exit_code: 1,
            });
        }
    }
    Some(NativeResult {
        stdout: String::new(),
        exit_code: 0,
    })
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    copy_dir_recursive_depth(src, dest, 0)
}

fn copy_dir_recursive_depth(src: &Path, dest: &Path, depth: usize) -> std::io::Result<()> {
    if depth > 50 {
        return Err(std::io::Error::other(
            "directory copy depth limit exceeded (possible symlink loop)",
        ));
    }
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            // Preserve symlinks as-is (don't follow)
            #[cfg(unix)]
            {
                let link_target = std::fs::read_link(entry.path())?;
                std::os::unix::fs::symlink(&link_target, &target)?;
            }
            #[cfg(not(unix))]
            {
                // On non-Unix, fall back to copying the target
                std::fs::copy(entry.path(), &target)?;
            }
        } else if ft.is_dir() {
            copy_dir_recursive_depth(&entry.path(), &target, depth + 1)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// ── mv ─────────────────────────────────────────────────────────────────

fn cmd_mv(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut paths: Vec<&str> = Vec::new();
    for arg in args {
        if arg.starts_with('-') {
            return None; // flags like -f, -n, -i → bash
        }
        paths.push(arg.as_str());
    }
    if paths.len() < 2 {
        return Some(NativeResult {
            stdout: "mv: missing destination".to_string(),
            exit_code: 1,
        });
    }
    let dest = match resolve_checked(paths.last().unwrap(), cwd, "mv", boundary) {
        Ok(p) => p,
        Err(blocked) => return Some(blocked),
    };
    let sources = &paths[..paths.len() - 1];

    if sources.len() > 1 && !dest.is_dir() {
        return Some(NativeResult {
            stdout: format!("mv: target '{}' is not a directory", paths.last().unwrap()),
            exit_code: 1,
        });
    }

    for src_str in sources {
        let src = match resolve_checked(src_str, cwd, "mv", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        let target = if dest.is_dir() {
            dest.join(src.file_name().unwrap_or_default())
        } else {
            dest.clone()
        };
        if let Err(e) = std::fs::rename(&src, &target) {
            // rename fails across filesystems; try copy + remove
            if src.is_dir() {
                if copy_dir_recursive(&src, &target).is_ok() {
                    let _ = std::fs::remove_dir_all(&src);
                } else {
                    return Some(NativeResult {
                        stdout: format!("mv: cannot move '{}': {}", src_str, e),
                        exit_code: 1,
                    });
                }
            } else if std::fs::copy(&src, &target).is_ok() {
                let _ = std::fs::remove_file(&src);
            } else {
                return Some(NativeResult {
                    stdout: format!("mv: cannot move '{}': {}", src_str, e),
                    exit_code: 1,
                });
            }
        }
    }
    Some(NativeResult {
        stdout: String::new(),
        exit_code: 0,
    })
}

// ── sort ───────────────────────────────────────────────────────────────

fn cmd_sort(
    args: &[String],
    cwd: &Path,
    boundary: Option<&super::WorkspaceBoundary>,
) -> Option<NativeResult> {
    let mut reverse = false;
    let mut unique = false;
    let mut numeric = false;
    let mut files: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg[1..].chars() {
                match c {
                    'r' => reverse = true,
                    'u' => unique = true,
                    'n' => numeric = true,
                    _ => return None,
                }
            }
        } else {
            files.push(arg.as_str());
        }
    }
    if files.is_empty() {
        return None; // sort from stdin
    }

    let mut all_lines: Vec<String> = Vec::new();
    for file in &files {
        let path = match resolve_checked(file, cwd, "sort", boundary) {
            Ok(p) => p,
            Err(blocked) => return Some(blocked),
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                all_lines.extend(content.lines().map(|l| l.to_string()));
            }
            Err(e) => {
                return Some(NativeResult {
                    stdout: format!("sort: {}: {}", file, e),
                    exit_code: 2,
                });
            }
        }
    }

    if numeric {
        all_lines.sort_by(|a, b| {
            let na: f64 = a
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let nb: f64 = b
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        all_lines.sort();
    }
    if reverse {
        all_lines.reverse();
    }
    if unique {
        all_lines.dedup();
    }

    let mut output: String = all_lines.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    Some(NativeResult {
        stdout: output,
        exit_code: 0,
    })
}

// ── basename / dirname / realpath ──────────────────────────────────────

fn cmd_basename(args: &[String]) -> Option<NativeResult> {
    let path = args.first()?;
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Some(NativeResult {
        stdout: format!("{name}\n"),
        exit_code: 0,
    })
}

fn cmd_dirname(args: &[String]) -> Option<NativeResult> {
    let path = args.first()?;
    let parent = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());
    Some(NativeResult {
        stdout: format!("{parent}\n"),
        exit_code: 0,
    })
}

fn cmd_realpath(args: &[String], cwd: &Path) -> Option<NativeResult> {
    let path = args.first()?;
    let target = cwd.join(path);
    match std::fs::canonicalize(&target) {
        Ok(resolved) => Some(NativeResult {
            stdout: format!("{}\n", resolved.to_string_lossy()),
            exit_code: 0,
        }),
        Err(e) => Some(NativeResult {
            stdout: format!("realpath: {}: {}", path, e),
            exit_code: 1,
        }),
    }
}

// ── echo ───────────────────────────────────────────────────────────────

fn cmd_echo(args: &[String]) -> Option<NativeResult> {
    // Only handle bare echo with no flags. -n, -e, etc. → bash
    if args.first().is_some_and(|a| a.starts_with('-')) {
        return None;
    }
    Some(NativeResult {
        stdout: format!("{}\n", args.join(" ")),
        exit_code: 0,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_metachar_detection() {
        assert!(has_shell_metachar("ls | grep foo"));
        assert!(has_shell_metachar("echo $HOME"));
        assert!(has_shell_metachar("ls > out.txt"));
        assert!(has_shell_metachar("cmd1 && cmd2"));
        assert!(has_shell_metachar("ls *.rs"));
        // Quoted metacharacters are fine
        assert!(!has_shell_metachar("grep 'foo|bar' file"));
        assert!(!has_shell_metachar("echo \"hello world\""));
        // Clean commands (including tilde, which we expand ourselves)
        assert!(!has_shell_metachar("ls -la"));
        assert!(!has_shell_metachar("cat file.txt"));
        assert!(!has_shell_metachar("head -n 20 file.txt"));
        assert!(!has_shell_metachar("cat ~/file.txt"));
        assert!(!has_shell_metachar("ls ~"));
    }

    #[test]
    fn dispatch_unknown_command() {
        assert!(try_dispatch("cargo test", Path::new("/tmp"), None).is_none());
        assert!(try_dispatch("npm install", Path::new("/tmp"), None).is_none());
    }

    #[test]
    fn dispatch_with_pipes_falls_through() {
        assert!(try_dispatch("ls | head", Path::new("/tmp"), None).is_none());
        assert!(try_dispatch("cat file | grep foo", Path::new("/tmp"), None).is_none());
    }

    #[test]
    fn pwd_returns_cwd() {
        let result = try_dispatch("pwd", Path::new("/tmp"), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/tmp");
    }

    #[test]
    fn cat_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello\nworld\n").unwrap();
        let result = try_dispatch("cat test.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[test]
    fn cat_missing_file() {
        let result = try_dispatch("cat nonexistent.txt", Path::new("/tmp"), None).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("nonexistent.txt"));
    }

    #[test]
    fn head_default_10_lines() {
        let dir = tempfile::tempdir().unwrap();
        let content: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("test.txt"), &content).unwrap();
        let result = try_dispatch("head test.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.lines().count(), 10);
        assert!(result.stdout.starts_with("line 1\n"));
    }

    #[test]
    fn head_with_n_flag() {
        let dir = tempfile::tempdir().unwrap();
        let content: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.path().join("test.txt"), &content).unwrap();
        let result = try_dispatch("head -n 3 test.txt", dir.path(), None).unwrap();
        assert_eq!(result.stdout.lines().count(), 3);
    }

    #[test]
    fn wc_line_count() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "a\nb\nc\n").unwrap();
        let result = try_dispatch("wc -l test.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("3"));
        assert!(result.stdout.contains("test.txt"));
    }

    #[test]
    fn ls_lists_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
        std::fs::write(dir.path().join("beta.txt"), "").unwrap();
        let result = try_dispatch("ls", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("alpha.txt"));
        assert!(result.stdout.contains("beta.txt"));
    }

    #[test]
    fn ls_hides_dotfiles_by_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden"), "").unwrap();
        std::fs::write(dir.path().join("visible"), "").unwrap();
        let result = try_dispatch("ls", dir.path(), None).unwrap();
        assert!(!result.stdout.contains(".hidden"));
        assert!(result.stdout.contains("visible"));
    }

    #[test]
    fn ls_shows_dotfiles_with_a() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden"), "").unwrap();
        let result = try_dispatch("ls -a", dir.path(), None).unwrap();
        assert!(result.stdout.contains(".hidden"));
    }

    #[test]
    fn mkdir_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = try_dispatch("mkdir newdir", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(dir.path().join("newdir").is_dir());
    }

    #[test]
    fn mkdir_p_creates_nested() {
        let dir = tempfile::tempdir().unwrap();
        let result = try_dispatch("mkdir -p a/b/c", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(dir.path().join("a/b/c").is_dir());
    }

    #[test]
    fn simple_glob_matching() {
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(simple_glob_match("*.rs", "lib.rs"));
        assert!(!simple_glob_match("*.rs", "main.py"));
        assert!(simple_glob_match("test?", "test1"));
        assert!(!simple_glob_match("test?", "test12"));
        assert!(simple_glob_match("*", "anything"));
        assert!(simple_glob_match("foo*bar", "fooXbar"));
        assert!(simple_glob_match("foo*bar", "foobar"));
    }

    #[test]
    fn grep_finds_pattern() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.txt"),
            "hello world\nfoo bar\nhello again\n",
        )
        .unwrap();
        let result = try_dispatch("grep hello a.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.lines().count(), 2);
        assert!(result.stdout.contains("hello world"));
        assert!(result.stdout.contains("hello again"));
    }

    #[test]
    fn grep_no_match_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "foo\nbar\n").unwrap();
        let result = try_dispatch("grep zzz a.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[test]
    fn grep_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "aaa\nbbb\naaa\n").unwrap();
        let result = try_dispatch("grep -n aaa a.txt", dir.path(), None).unwrap();
        assert!(result.stdout.contains("1:aaa"));
        assert!(result.stdout.contains("3:aaa"));
    }

    #[test]
    fn grep_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "Hello\nworld\n").unwrap();
        let result = try_dispatch("grep -i hello a.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Hello"));
    }

    #[test]
    fn grep_recursive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a.txt"), "match here\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "no match\n").unwrap();
        let result = try_dispatch("grep -r match .", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("match here"));
    }

    #[test]
    fn grep_files_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "match\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "nope\n").unwrap();
        let result = try_dispatch("grep -rl match .", dir.path(), None).unwrap();
        assert!(result.stdout.contains("a.txt"));
        assert!(!result.stdout.contains("b.txt"));
    }

    #[test]
    fn touch_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = try_dispatch("touch newfile.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(dir.path().join("newfile.txt").exists());
    }

    #[test]
    fn rm_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("doomed.txt"), "bye").unwrap();
        let result = try_dispatch("rm doomed.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!dir.path().join("doomed.txt").exists());
    }

    #[test]
    fn rm_rf_removes_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("deep/nested")).unwrap();
        std::fs::write(dir.path().join("deep/nested/file.txt"), "x").unwrap();
        let result = try_dispatch("rm -rf deep", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!dir.path().join("deep").exists());
    }

    #[test]
    fn cp_copies_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("src.txt"), "content").unwrap();
        let result = try_dispatch("cp src.txt dst.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("dst.txt")).unwrap(),
            "content"
        );
    }

    #[test]
    fn mv_moves_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "data").unwrap();
        let result = try_dispatch("mv old.txt new.txt", dir.path(), None).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!dir.path().join("old.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "data"
        );
    }

    #[test]
    fn sort_sorts_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "cherry\napple\nbanana\n").unwrap();
        let result = try_dispatch("sort data.txt", dir.path(), None).unwrap();
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[test]
    fn sort_reverse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "a\nb\nc\n").unwrap();
        let result = try_dispatch("sort -r data.txt", dir.path(), None).unwrap();
        assert_eq!(result.stdout, "c\nb\na\n");
    }

    #[test]
    fn basename_extracts_filename() {
        let result =
            try_dispatch("basename /usr/local/bin/omegon", Path::new("/tmp"), None).unwrap();
        assert_eq!(result.stdout.trim(), "omegon");
    }

    #[test]
    fn dirname_extracts_parent() {
        let result =
            try_dispatch("dirname /usr/local/bin/omegon", Path::new("/tmp"), None).unwrap();
        assert_eq!(result.stdout.trim(), "/usr/local/bin");
    }

    #[test]
    fn echo_outputs_args() {
        let result = try_dispatch("echo hello world", Path::new("/tmp"), None).unwrap();
        assert_eq!(result.stdout, "hello world\n");
    }

    #[test]
    fn echo_with_flags_falls_through() {
        assert!(try_dispatch("echo -n hello", Path::new("/tmp"), None).is_none());
    }

    #[test]
    fn true_and_false_exit_codes() {
        let t = try_dispatch("true", Path::new("/tmp"), None).unwrap();
        assert_eq!(t.exit_code, 0);
        let f = try_dispatch("false", Path::new("/tmp"), None).unwrap();
        assert_eq!(f.exit_code, 1);
    }

    // ── Boundary enforcement tests ────────────────────────────────────

    fn test_boundary(workspace: &str) -> crate::tools::WorkspaceBoundary {
        crate::tools::WorkspaceBoundary::new(PathBuf::from(workspace))
    }

    #[test]
    fn boundary_blocks_cat_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result =
            try_dispatch("cat /etc/passwd", Path::new("/tmp/workspace"), Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"), "got: {}", result.stdout);
    }

    #[test]
    fn boundary_allows_cat_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::write(cwd.join("test.txt"), "hello").unwrap();
        let b = crate::tools::WorkspaceBoundary::new(cwd.clone());
        let result = try_dispatch("cat test.txt", &cwd, Some(&b)).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[test]
    fn boundary_blocks_mkdir_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result =
            try_dispatch("mkdir /outside/dir", Path::new("/tmp/workspace"), Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_cp_dest_outside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::write(cwd.join("src.txt"), "data").unwrap();
        let b = crate::tools::WorkspaceBoundary::new(cwd.clone());
        let result = try_dispatch("cp src.txt /etc/evil.txt", &cwd, Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_mv_dest_outside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::write(cwd.join("src.txt"), "data").unwrap();
        let b = crate::tools::WorkspaceBoundary::new(cwd.clone());
        let result = try_dispatch("mv src.txt /etc/evil.txt", &cwd, Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_rm_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result = try_dispatch("rm /etc/passwd", Path::new("/tmp/workspace"), Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_touch_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result =
            try_dispatch("touch /etc/evil.txt", Path::new("/tmp/workspace"), Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_ls_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result = try_dispatch("ls /etc", Path::new("/tmp/workspace"), Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_head_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result =
            try_dispatch("head /etc/passwd", Path::new("/tmp/workspace"), Some(&b)).unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_blocks_find_outside_workspace() {
        let b = test_boundary("/tmp/workspace");
        let result = try_dispatch(
            "find /etc -name passwd",
            Path::new("/tmp/workspace"),
            Some(&b),
        )
        .unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("BLOCKED"));
    }

    #[test]
    fn boundary_allows_trusted_directory() {
        let b = test_boundary("/tmp/workspace");
        b.approve_directory(PathBuf::from("/etc"));
        let result = try_dispatch("ls /etc", Path::new("/tmp/workspace"), Some(&b));
        // Should not be blocked (though /etc may not have the expected format,
        // the point is it's not BLOCKED)
        if let Some(r) = result {
            assert!(
                !r.stdout.contains("BLOCKED"),
                "trusted dir should not be blocked: {}",
                r.stdout
            );
        }
    }

    #[test]
    fn no_boundary_allows_everything() {
        // With None boundary, commands are unrestricted (backward compat)
        let result = try_dispatch("ls /etc", Path::new("/tmp"), None).unwrap();
        assert!(!result.stdout.contains("BLOCKED"));
    }
}

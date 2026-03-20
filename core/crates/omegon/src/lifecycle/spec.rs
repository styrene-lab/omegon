//! OpenSpec read-only parser — spec content, scenarios, change listing.
//!
//! Parses openspec/ directories to extract change info, spec files,
//! and Given/When/Then scenarios. No mutation support (Phase 1b).

use std::fs;
use std::path::{Path, PathBuf};

use super::types::*;

/// Locate the openspec/ directory in a repository.
pub fn find_openspec_dir(repo_path: &Path) -> Option<PathBuf> {
    let dir = repo_path.join("openspec");
    if dir.is_dir() { Some(dir) } else { None }
}

/// List all active OpenSpec changes (in openspec/changes/).
pub fn list_changes(repo_path: &Path) -> Vec<ChangeInfo> {
    let Some(openspec_dir) = find_openspec_dir(repo_path) else {
        return vec![];
    };
    let changes_dir = openspec_dir.join("changes");
    if !changes_dir.is_dir() {
        return vec![];
    }

    let mut changes = Vec::new();
    let entries = match fs::read_dir(&changes_dir) {
        Ok(e) => e,
        Err(_) => return changes,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if let Some(info) = read_change(&path, &name) {
            changes.push(info);
        }
    }

    changes.sort_by(|a, b| a.name.cmp(&b.name));
    changes
}

/// Read a single change directory into a ChangeInfo.
pub fn get_change(repo_path: &Path, name: &str) -> Option<ChangeInfo> {
    let openspec_dir = find_openspec_dir(repo_path)?;
    let change_dir = openspec_dir.join("changes").join(name);
    if !change_dir.is_dir() {
        return None;
    }
    read_change(&change_dir, name)
}

fn read_change(change_dir: &Path, name: &str) -> Option<ChangeInfo> {
    let has_proposal = change_dir.join("proposal.md").exists();
    let has_design = change_dir.join("design.md").exists();
    let specs_dir = change_dir.join("specs");
    let has_specs = specs_dir.is_dir()
        && fs::read_dir(&specs_dir)
            .ok()
            .map(|e| e.flatten().any(|f| {
                f.path().extension().and_then(|e| e.to_str()) == Some("md")
            }))
            .unwrap_or(false);
    let tasks_path = change_dir.join("tasks.md");
    let has_tasks = tasks_path.exists();

    let (total_tasks, done_tasks) = if has_tasks {
        count_tasks(&tasks_path)
    } else {
        (0, 0)
    };

    let specs = if has_specs {
        parse_specs_dir(&specs_dir)
    } else {
        vec![]
    };

    let stage = compute_stage(has_proposal, has_specs, has_tasks, total_tasks, done_tasks);

    Some(ChangeInfo {
        name: name.to_string(),
        path: change_dir.to_path_buf(),
        stage,
        has_proposal,
        has_design,
        has_specs,
        has_tasks,
        total_tasks,
        done_tasks,
        specs,
    })
}

/// Compute the lifecycle stage from file presence and task counts.
pub fn compute_stage(
    has_proposal: bool,
    has_specs: bool,
    has_tasks: bool,
    total_tasks: usize,
    done_tasks: usize,
) -> ChangeStage {
    if !has_proposal {
        return ChangeStage::Proposed;
    }
    if !has_specs {
        return ChangeStage::Proposed;
    }
    if !has_tasks {
        return ChangeStage::Specified;
    }
    if total_tasks == 0 {
        return ChangeStage::Planned;
    }
    if done_tasks >= total_tasks {
        return ChangeStage::Verifying;
    }
    ChangeStage::Implementing
}

/// Count tasks in a tasks.md file.
/// Tasks are lines matching `- [x]` (done) or `- [ ]` (pending).
fn count_tasks(path: &Path) -> (usize, usize) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

    let mut total = 0;
    let mut done = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            total += 1;
            done += 1;
        } else if trimmed.starts_with("- [ ]") {
            total += 1;
        }
    }
    (total, done)
}

/// Parse all spec files in a specs/ directory.
pub fn parse_specs_dir(specs_dir: &Path) -> Vec<SpecFile> {
    let mut specs = Vec::new();

    let entries = match fs::read_dir(specs_dir) {
        Ok(e) => e,
        Err(_) => return specs,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let domain = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let requirements = parse_spec_content(&content);
        specs.push(SpecFile {
            domain,
            file_path: path,
            requirements,
        });
    }

    // Also handle nested directories (e.g., specs/auth/tokens.md → domain "auth/tokens")
    if let Ok(entries) = fs::read_dir(specs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let parent_domain = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if let Ok(sub_entries) = fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_path = sub.path();
                        if sub_path.extension().and_then(|e| e.to_str()) != Some("md") {
                            continue;
                        }
                        let name = sub_path.file_stem().and_then(|n| n.to_str()).unwrap_or("unknown");
                        let domain = format!("{parent_domain}/{name}");
                        let content = match fs::read_to_string(&sub_path) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };
                        let requirements = parse_spec_content(&content);
                        specs.push(SpecFile {
                            domain,
                            file_path: sub_path,
                            requirements,
                        });
                    }
                }
            }
        }
    }

    specs
}

/// Parse spec content into requirements with scenarios.
/// Format:
///   ### Requirement: <title>
///   <description>
///   #### Scenario: <title>
///   Given <precondition>
///   When <action>
///   Then <expected>
///   And <additional>
pub fn parse_spec_content(content: &str) -> Vec<Requirement> {
    let mut requirements = Vec::new();
    let mut current_req: Option<(String, String, Vec<Scenario>)> = None;
    let mut current_scenario: Option<ScenarioBuilder> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // New requirement: "### Requirement: <title>" or bare "### <title>" (without ####)
        if trimmed.starts_with("### ") && !trimmed.starts_with("#### ") {
            let rest = &trimmed[4..];
            let title = rest.strip_prefix("Requirement:").unwrap_or(rest).trim();
            // Flush previous
            flush_scenario(&mut current_scenario, current_req.as_mut().map(|r| &mut r.2));
            if let Some((t, d, s)) = current_req.take() {
                requirements.push(Requirement { title: t, description: d.trim().to_string(), scenarios: s });
            }
            current_req = Some((title.to_string(), String::new(), Vec::new()));
            continue;
        }

        // New scenario: "#### Scenario: <title>" or bare "#### <title>"
        if let Some(after) = trimmed.strip_prefix("#### ") {
            let rest = after.strip_prefix("Scenario:").unwrap_or(after).trim();
            flush_scenario(&mut current_scenario, current_req.as_mut().map(|r| &mut r.2));
            current_scenario = Some(ScenarioBuilder {
                title: rest.trim().to_string(),
                given: String::new(),
                when: String::new(),
                then: String::new(),
                and_clauses: Vec::new(),
            });
            continue;
        }

        if let Some(ref mut builder) = current_scenario {
            if let Some(rest) = trimmed.strip_prefix("Given ") {
                builder.given = rest.to_string();
            } else if let Some(rest) = trimmed.strip_prefix("When ") {
                builder.when = rest.to_string();
            } else if let Some(rest) = trimmed.strip_prefix("Then ") {
                builder.then = rest.to_string();
            } else if let Some(rest) = trimmed.strip_prefix("And ") {
                builder.and_clauses.push(rest.to_string());
            }
        } else if let Some(ref mut req) = current_req {
            // Description lines (between requirement header and first scenario)
            if !trimmed.is_empty() {
                req.1.push_str(trimmed);
                req.1.push('\n');
            }
        }
    }

    // Flush final
    flush_scenario(&mut current_scenario, current_req.as_mut().map(|r| &mut r.2));
    if let Some((t, d, s)) = current_req {
        requirements.push(Requirement { title: t, description: d.trim().to_string(), scenarios: s });
    }

    requirements
}

struct ScenarioBuilder {
    title: String,
    given: String,
    when: String,
    then: String,
    and_clauses: Vec<String>,
}

fn flush_scenario(
    builder: &mut Option<ScenarioBuilder>,
    target: Option<&mut Vec<Scenario>>,
) {
    if let Some(b) = builder.take()
        && (!b.given.is_empty() || !b.when.is_empty() || !b.then.is_empty())
        && let Some(scenarios) = target
    {
        scenarios.push(Scenario {
            title: b.title,
            given: b.given,
            when: b.when,
            then: b.then,
            and_clauses: b.and_clauses,
        });
    }
}

/// Build a context injection string for relevant OpenSpec changes.
pub fn build_context_injection(changes: &[ChangeInfo]) -> String {
    if changes.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("[OpenSpec — active changes]".to_string());

    for change in changes {
        let icon = match change.stage {
            ChangeStage::Proposed => "◌",
            ChangeStage::Specified => "◐",
            ChangeStage::Planned => "▸",
            ChangeStage::Implementing => "⟳",
            ChangeStage::Verifying => "◉",
            ChangeStage::Archived => "✓",
        };
        let progress = if change.total_tasks > 0 {
            format!(" ({}/{})", change.done_tasks, change.total_tasks)
        } else {
            String::new()
        };
        lines.push(format!(
            "  {icon} {} — {}{progress}",
            change.name,
            change.stage.as_str()
        ));

        // Include scenario summaries for implementing/verifying changes
        if matches!(change.stage, ChangeStage::Implementing | ChangeStage::Verifying) {
            for spec in &change.specs {
                let scenario_count: usize = spec.requirements.iter().map(|r| r.scenarios.len()).sum();
                if scenario_count > 0 {
                    lines.push(format!("    specs/{}: {} scenarios", spec.domain, scenario_count));
                }
            }
        }
    }

    lines.join("\n")
}

/// Count total scenarios across all specs in a change.
pub fn count_scenarios(change: &ChangeInfo) -> usize {
    change
        .specs
        .iter()
        .flat_map(|s| &s.requirements)
        .map(|r| r.scenarios.len())
        .sum()
}

// ═══════════════════════════════════════════════════════════════════════════
// Mutation functions — create and modify OpenSpec changes
// ═══════════════════════════════════════════════════════════════════════════

/// Scaffold a new OpenSpec change directory with a proposal.
pub fn propose_change(
    repo_path: &Path,
    name: &str,
    title: &str,
    intent: &str,
) -> anyhow::Result<ChangeInfo> {
    let openspec_dir = repo_path.join("openspec");
    let changes_dir = openspec_dir.join("changes");
    let change_dir = changes_dir.join(name);

    if change_dir.exists() {
        anyhow::bail!("Change '{name}' already exists");
    }

    fs::create_dir_all(&change_dir)?;

    // Write proposal.md
    let proposal = format!(
        "# {title}\n\n## Intent\n\n{intent}\n\n## Scope\n\n_TBD_\n\n## Constraints\n\n_None identified yet._\n"
    );
    fs::write(change_dir.join("proposal.md"), &proposal)?;

    Ok(ChangeInfo {
        name: name.to_string(),
        path: change_dir,
        stage: ChangeStage::Proposed,
        has_proposal: true,
        has_design: false,
        has_specs: false,
        has_tasks: false,
        total_tasks: 0,
        done_tasks: 0,
        specs: vec![],
    })
}

/// Add a spec file to an existing change.
pub fn add_spec(
    repo_path: &Path,
    change_name: &str,
    domain: &str,
    spec_content: &str,
) -> anyhow::Result<PathBuf> {
    let change_dir = repo_path
        .join("openspec/changes")
        .join(change_name);

    if !change_dir.exists() {
        anyhow::bail!("Change '{change_name}' does not exist");
    }

    let specs_dir = change_dir.join("specs");
    fs::create_dir_all(&specs_dir)?;

    // domain can be "auth" or "auth/tokens" — create subdirs as needed
    let spec_path = if domain.contains('/') {
        let parts: Vec<&str> = domain.rsplitn(2, '/').collect();
        let subdir = specs_dir.join(parts[1]);
        fs::create_dir_all(&subdir)?;
        subdir.join(format!("{}.md", parts[0]))
    } else {
        specs_dir.join(format!("{domain}.md"))
    };

    fs::write(&spec_path, spec_content)?;
    Ok(spec_path)
}

/// Archive a change by moving it to openspec/archive/.
pub fn archive_change(repo_path: &Path, change_name: &str) -> anyhow::Result<()> {
    let change_dir = repo_path
        .join("openspec/changes")
        .join(change_name);

    if !change_dir.exists() {
        anyhow::bail!("Change '{change_name}' does not exist");
    }

    let archive_dir = repo_path.join("openspec/archive");
    fs::create_dir_all(&archive_dir)?;
    let dest = archive_dir.join(change_name);

    fs::rename(&change_dir, &dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_spec_content_basic() {
        let content = r#"# progress

### Requirement: Events emitted on stdout

Description of the requirement.

#### Scenario: Child lifecycle events

Given a cleave run with 2 children
When the orchestrator dispatches children
Then stdout contains a wave_start event
And stdout contains a child_spawned event for each child
And each JSON line is valid

#### Scenario: Merge events

Given a cleave run where all children complete
When the orchestrator enters merge
Then stdout contains a merge_start event

### Requirement: TS wrapper maps events

#### Scenario: Dashboard shows running children

Given a cleave_run invocation
When child_spawned events arrive
Then sharedState.cleave.children[i].status becomes running
"#;

        let reqs = parse_spec_content(content);
        assert_eq!(reqs.len(), 2, "Should have 2 requirements");

        assert_eq!(reqs[0].title, "Events emitted on stdout");
        assert!(reqs[0].description.contains("Description"));
        assert_eq!(reqs[0].scenarios.len(), 2);
        assert_eq!(reqs[0].scenarios[0].title, "Child lifecycle events");
        assert!(reqs[0].scenarios[0].given.contains("2 children"));
        assert_eq!(reqs[0].scenarios[0].and_clauses.len(), 2);

        assert_eq!(reqs[1].title, "TS wrapper maps events");
        assert_eq!(reqs[1].scenarios.len(), 1);
    }

    #[test]
    fn count_tasks_basic() {
        let dir = std::env::temp_dir().join("omegon-test-tasks");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("tasks.md");
        fs::write(&path, "# Tasks\n\n## Group 1\n\n- [x] Done task\n- [ ] Pending task\n- [x] Another done\n").unwrap();

        let (total, done) = count_tasks(&path);
        assert_eq!(total, 3);
        assert_eq!(done, 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compute_stage_progression() {
        assert_eq!(compute_stage(false, false, false, 0, 0), ChangeStage::Proposed);
        assert_eq!(compute_stage(true, false, false, 0, 0), ChangeStage::Proposed);
        assert_eq!(compute_stage(true, true, false, 0, 0), ChangeStage::Specified);
        assert_eq!(compute_stage(true, true, true, 0, 0), ChangeStage::Planned);
        assert_eq!(compute_stage(true, true, true, 5, 2), ChangeStage::Implementing);
        assert_eq!(compute_stage(true, true, true, 5, 5), ChangeStage::Verifying);
    }

    #[test]
    fn context_injection_format() {
        let changes = vec![ChangeInfo {
            name: "test-change".into(),
            path: PathBuf::new(),
            stage: ChangeStage::Implementing,
            has_proposal: true,
            has_design: true,
            has_specs: true,
            has_tasks: true,
            total_tasks: 10,
            done_tasks: 7,
            specs: vec![SpecFile {
                domain: "auth".into(),
                file_path: PathBuf::new(),
                requirements: vec![Requirement {
                    title: "Auth".into(),
                    description: String::new(),
                    scenarios: vec![Scenario {
                        title: "Login".into(),
                        given: "user".into(),
                        when: "login".into(),
                        then: "success".into(),
                        and_clauses: vec![],
                    }],
                }],
            }],
        }];

        let injection = build_context_injection(&changes);
        assert!(injection.contains("[OpenSpec"));
        assert!(injection.contains("test-change"));
        assert!(injection.contains("7/10"));
        assert!(injection.contains("specs/auth: 1 scenarios"));
    }

    #[test]
    fn parse_real_baseline_format() {
        // Test against the actual baseline format used by Omegon
        let content = r#"# progress

### Requirement: Rust orchestrator emits NDJSON progress events on stdout

#### Scenario: Child lifecycle events appear on stdout as JSON

Given a cleave run with 2 children in one wave
When the Rust orchestrator dispatches children
Then stdout contains a `wave_start` event with both child labels
And stdout contains a `child_spawned` event for each child with pid
And stdout contains a `child_status` event with status `completed` or `failed` for each child
And each JSON line is valid self-contained JSON (parseable independently)
"#;

        let reqs = parse_spec_content(content);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].scenarios.len(), 1);
        assert_eq!(reqs[0].scenarios[0].and_clauses.len(), 3);
        assert!(reqs[0].scenarios[0].given.contains("2 children"));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn scan_real_openspec_directory() {
        let repo_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap();

        let changes = list_changes(repo_path);
        tracing::debug!("Found {} active OpenSpec changes", changes.len());

        // Verify baseline specs can be parsed
        let baseline_dir = repo_path.join("openspec/baseline");
        if baseline_dir.is_dir() {
            let specs = parse_specs_dir(&baseline_dir);
            tracing::debug!("Parsed {} baseline spec files", specs.len());
            for spec in &specs {
                let scenario_count: usize = spec.requirements.iter().map(|r| r.scenarios.len()).sum();
                tracing::debug!("  {}: {} requirements, {} scenarios", spec.domain, spec.requirements.len(), scenario_count);
                assert!(!spec.requirements.is_empty() || scenario_count == 0,
                    "Spec {} should have requirements if it has scenarios", spec.domain);
            }
        }
    }
}

#[cfg(test)]
mod mutation_tests {
    use super::*;

    #[test]
    fn propose_creates_directory_and_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();

        let change = propose_change(repo, "my-change", "My Change", "Do things").unwrap();
        assert_eq!(change.name, "my-change");
        assert_eq!(change.stage, ChangeStage::Proposed);
        assert!(change.has_proposal);
        assert!(change.path.join("proposal.md").exists());

        let content = fs::read_to_string(change.path.join("proposal.md")).unwrap();
        assert!(content.contains("My Change"));
        assert!(content.contains("Do things"));
    }

    #[test]
    fn propose_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        propose_change(dir.path(), "dup", "Dup", "intent").unwrap();
        assert!(propose_change(dir.path(), "dup", "Dup2", "intent2").is_err());
    }

    #[test]
    fn add_spec_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        propose_change(dir.path(), "spec-test", "Spec Test", "intent").unwrap();

        let path = add_spec(dir.path(), "spec-test", "auth", "# auth specs").unwrap();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "# auth specs");
    }

    #[test]
    fn add_spec_nested_domain() {
        let dir = tempfile::tempdir().unwrap();
        propose_change(dir.path(), "nested", "Nested", "intent").unwrap();

        let path = add_spec(dir.path(), "nested", "auth/tokens", "# token specs").unwrap();
        assert!(path.exists());
        assert!(path.to_str().unwrap().contains("auth"));
    }

    #[test]
    fn add_spec_nonexistent_change_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(add_spec(dir.path(), "nope", "auth", "specs").is_err());
    }

    #[test]
    fn archive_moves_to_archive_dir() {
        let dir = tempfile::tempdir().unwrap();
        let change = propose_change(dir.path(), "to-archive", "Archive Me", "intent").unwrap();
        assert!(change.path.exists());

        archive_change(dir.path(), "to-archive").unwrap();
        assert!(!change.path.exists(), "original should be gone");
        assert!(dir.path().join("openspec/archive/to-archive").exists(), "should be in archive");
    }

    #[test]
    fn archive_nonexistent_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(archive_change(dir.path(), "nope").is_err());
    }

    #[test]
    fn propose_then_list() {
        let dir = tempfile::tempdir().unwrap();
        propose_change(dir.path(), "listed", "Listed Change", "intent").unwrap();

        let changes = list_changes(dir.path());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "listed");
        assert_eq!(changes[0].stage, ChangeStage::Proposed);
    }

    #[test]
    fn propose_add_spec_updates_stage() {
        let dir = tempfile::tempdir().unwrap();
        propose_change(dir.path(), "staged", "Staged", "intent").unwrap();
        add_spec(dir.path(), "staged", "core", "# core\n\n### Requirement: Works\n\n#### Scenario: Basic\n\nGiven X\nWhen Y\nThen Z\n").unwrap();

        let changes = list_changes(dir.path());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].stage, ChangeStage::Specified);
        assert!(changes[0].has_specs);
    }
}

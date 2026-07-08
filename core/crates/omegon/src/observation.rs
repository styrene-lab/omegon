use crate::behavior::ToolCapabilityCatalog;
use crate::conversation::{ToolCall, ToolResultEntry};
use omegon_traits::ToolCapability;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObservationEvent {
    FileRead {
        source_tool: String,
        path: PathBuf,
    },
    SearchPerformed {
        source_tool: String,
        query: Option<String>,
        roots: Vec<PathBuf>,
    },
    FileMutated {
        source_tool: String,
        path: PathBuf,
    },
    ValidationRun {
        source_tool: String,
    },
    ProgressBoundary {
        source_tool: String,
        clears_mutation_state: bool,
    },
}

pub(crate) struct ObservationNormalizer<'a> {
    catalog: &'a ToolCapabilityCatalog,
}

impl<'a> ObservationNormalizer<'a> {
    pub(crate) fn new(catalog: &'a ToolCapabilityCatalog) -> Self {
        Self { catalog }
    }

    pub(crate) fn normalize(
        &self,
        calls: &[ToolCall],
        results: &[ToolResultEntry],
    ) -> Vec<ObservationEvent> {
        let mut events = Vec::new();
        for call in calls {
            if !call_succeeded(call, results) {
                continue;
            }
            if call.name == "bash" {
                events.extend(normalize_bash(call));
                continue;
            }
            events.extend(self.normalize_structured_tool(call));
        }
        events
    }

    fn normalize_structured_tool(&self, call: &ToolCall) -> Vec<ObservationEvent> {
        let mut events = Vec::new();
        let caps = self.catalog.capabilities_for(&call.name);

        if caps.contains(&ToolCapability::ProgressBoundary) || call.name == "commit" {
            events.push(ObservationEvent::ProgressBoundary {
                source_tool: call.name.clone(),
                clears_mutation_state: call.name == "commit",
            });
        }

        if caps.contains(&ToolCapability::Mutation)
            || matches!(call.name.as_str(), "change" | "write" | "edit")
        {
            for path in mutation_paths(call) {
                events.push(ObservationEvent::FileMutated {
                    source_tool: call.name.clone(),
                    path,
                });
            }
        }

        if caps.contains(&ToolCapability::Validation) {
            events.push(ObservationEvent::ValidationRun {
                source_tool: call.name.clone(),
            });
        }

        let is_repo_inspection = caps.iter().any(|cap| {
            matches!(
                cap,
                ToolCapability::RepoInspection
                    | ToolCapability::TargetedRepoInspection
                    | ToolCapability::BroadRepoInspection
            )
        }) || matches!(call.name.as_str(), "read" | "understand" | "view");

        if is_repo_inspection {
            if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                events.push(ObservationEvent::FileRead {
                    source_tool: call.name.clone(),
                    path: PathBuf::from(path),
                });
            } else if caps.contains(&ToolCapability::BroadRepoInspection) {
                events.push(ObservationEvent::SearchPerformed {
                    source_tool: call.name.clone(),
                    query: call
                        .arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    roots: search_roots(call),
                });
            }
        }

        events
    }
}

fn search_roots(call: &ToolCall) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(within) = call.arguments.get("within").and_then(|v| v.as_str()) {
        roots.push(PathBuf::from(within));
    }
    if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
        roots.push(PathBuf::from(path));
    }
    if let Some(paths) = call.arguments.get("paths").and_then(|v| v.as_array()) {
        roots.extend(paths.iter().filter_map(|v| v.as_str()).map(PathBuf::from));
    }
    roots
}

fn search_query(program: &str, tokens: &[String]) -> Option<String> {
    match program {
        "rg" | "grep" => tokens.iter().skip(1).find(|t| !t.starts_with('-')).cloned(),
        "find" | "fd" | "ls" | "tree" => None,
        _ => None,
    }
}

fn search_roots_from_tokens(program: &str, tokens: &[String]) -> Vec<PathBuf> {
    let args: Vec<&String> = tokens
        .iter()
        .skip(1)
        .filter(|t| !t.starts_with('-'))
        .collect();
    match program {
        "rg" | "grep" => args.into_iter().skip(1).map(PathBuf::from).collect(),
        "find" | "fd" | "ls" | "tree" => args.into_iter().map(PathBuf::from).collect(),
        _ => Vec::new(),
    }
}

fn call_succeeded(call: &ToolCall, results: &[ToolResultEntry]) -> bool {
    results
        .iter()
        .any(|result| result.call_id == call.id && !result.is_error)
}

fn mutation_paths(call: &ToolCall) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
        paths.push(PathBuf::from(path));
    }
    if let Some(edits) = call.arguments.get("edits").and_then(|v| v.as_array()) {
        for edit in edits {
            if let Some(path) = edit.get("file").and_then(|v| v.as_str()) {
                paths.push(PathBuf::from(path));
            }
        }
    }
    paths
}

fn normalize_bash(call: &ToolCall) -> Vec<ObservationEvent> {
    let Some(command) = call.arguments.get("command").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    command
        .split(['\n', ';', '|'])
        .flat_map(|segment| segment.split("&&"))
        .flat_map(|segment| segment.split("||"))
        .flat_map(classify_bash_segment)
        .collect()
}

fn classify_bash_segment(segment: &str) -> Vec<ObservationEvent> {
    let tokens = shell_words(segment);
    let Some(program) = tokens.first().map(String::as_str) else {
        return Vec::new();
    };
    match program {
        "git" | "jj" if tokens.get(1).is_some_and(|arg| arg == "commit") => {
            vec![ObservationEvent::ProgressBoundary {
                source_tool: "bash".into(),
                clears_mutation_state: true,
            }]
        }
        "cargo"
            if tokens.get(1).is_some_and(|arg| {
                matches!(arg.as_str(), "test" | "check" | "clippy" | "build")
            }) =>
        {
            vec![ObservationEvent::ValidationRun {
                source_tool: "bash".into(),
            }]
        }
        "just"
            if tokens.get(1).is_some_and(|arg| {
                arg.starts_with("test") || matches!(arg.as_str(), "lint" | "check" | "build")
            }) =>
        {
            vec![ObservationEvent::ValidationRun {
                source_tool: "bash".into(),
            }]
        }
        "npm" | "pnpm" | "yarn" if tokens.iter().any(|arg| arg == "test" || arg == "check") => {
            vec![ObservationEvent::ValidationRun {
                source_tool: "bash".into(),
            }]
        }
        "rg" | "grep" | "find" | "fd" | "ls" | "tree" => vec![ObservationEvent::SearchPerformed {
            source_tool: format!("bash:{program}"),
            query: search_query(program, &tokens),
            roots: search_roots_from_tokens(program, &tokens),
        }],
        "cat" | "head" | "tail" | "sed" | "awk" | "wc" | "nl" | "strings" | "xxd" | "hexdump" => {
            read_paths_from_tokens(&tokens)
                .into_iter()
                .map(|path| ObservationEvent::FileRead {
                    source_tool: format!("bash:{program}"),
                    path,
                })
                .collect()
        }
        "touch" | "rm" => mutation_paths_from_tokens(&tokens)
            .into_iter()
            .map(|path| ObservationEvent::FileMutated {
                source_tool: format!("bash:{program}"),
                path,
            })
            .collect(),
        "mv" | "cp" => tokens
            .last()
            .filter(|path| !path.starts_with('-'))
            .map(|path| {
                vec![ObservationEvent::FileMutated {
                    source_tool: format!("bash:{program}"),
                    path: PathBuf::from(path),
                }]
            })
            .unwrap_or_default(),
        _ => redirect_mutation_targets(&tokens)
            .into_iter()
            .map(|path| ObservationEvent::FileMutated {
                source_tool: "bash:redirection".into(),
                path,
            })
            .collect(),
    }
}

fn mutation_paths_from_tokens(tokens: &[String]) -> Vec<PathBuf> {
    tokens
        .iter()
        .skip(1)
        .filter(|token| !token.starts_with('-'))
        .map(PathBuf::from)
        .collect()
}

fn redirect_mutation_targets(tokens: &[String]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut iter = tokens.iter();
    while let Some(token) = iter.next() {
        if (token == ">" || token == ">>")
            && let Some(path) = iter.next()
        {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}
fn read_paths_from_tokens(tokens: &[String]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut skip_next = false;
    for token in tokens.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if token == ">" || token == ">>" || token == "<" {
            skip_next = token != "<";
            continue;
        }
        if token.starts_with('-') || token.contains('=') || token.parse::<usize>().is_ok() {
            continue;
        }
        if token.contains('/') || token.contains('.') {
            paths.push(PathBuf::from(token));
        }
    }
    paths
}

fn shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            (_, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_traits::ToolDefinition;
    use serde_json::json;

    fn catalog(defs: Vec<(&str, Vec<ToolCapability>)>) -> ToolCapabilityCatalog {
        ToolCapabilityCatalog::from_tool_defs(
            &defs
                .into_iter()
                .map(|(name, capabilities)| ToolDefinition {
                    name: name.into(),
                    label: String::new(),
                    description: String::new(),
                    parameters: json!({}),
                    capabilities,
                })
                .collect::<Vec<_>>(),
        )
    }

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: name.into(),
            arguments,
        }
    }

    fn ok_result() -> ToolResultEntry {
        ToolResultEntry {
            call_id: "1".into(),
            tool_name: String::new(),
            content: Vec::new(),
            is_error: false,
            args_summary: None,
        }
    }

    fn error_result() -> ToolResultEntry {
        ToolResultEntry {
            is_error: true,
            ..ok_result()
        }
    }

    #[test]
    fn targeted_repo_inspection_records_file_read() {
        let catalog = catalog(vec![("view", vec![ToolCapability::TargetedRepoInspection])]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call("view", json!({"path": "docs/a.md"}))],
            &[ok_result()],
        );
        assert_eq!(
            events,
            vec![ObservationEvent::FileRead {
                source_tool: "view".into(),
                path: PathBuf::from("docs/a.md")
            }]
        );
    }

    #[test]
    fn broad_repo_inspection_records_search_not_file_read() {
        let catalog = catalog(vec![(
            "codebase_search",
            vec![ToolCapability::BroadRepoInspection],
        )]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call(
                "codebase_search",
                json!({"query": "OrientationChurn"}),
            )],
            &[ok_result()],
        );
        assert_eq!(
            events,
            vec![ObservationEvent::SearchPerformed {
                source_tool: "codebase_search".into(),
                query: Some("OrientationChurn".into()),
                roots: Vec::new(),
            }]
        );
    }

    #[test]
    fn failed_call_records_no_positive_evidence() {
        let catalog = catalog(vec![("view", vec![ToolCapability::TargetedRepoInspection])]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call("view", json!({"path": "docs/a.md"}))],
            &[error_result()],
        );
        assert!(events.is_empty());
    }

    #[test]
    fn missing_result_records_no_positive_evidence() {
        let catalog = catalog(vec![("view", vec![ToolCapability::TargetedRepoInspection])]);
        let events = ObservationNormalizer::new(&catalog)
            .normalize(&[call("view", json!({"path": "docs/a.md"}))], &[]);
        assert!(events.is_empty());
    }

    #[test]
    fn bash_mutation_commands_are_observed() {
        let catalog = catalog(vec![]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call(
                "bash",
                json!({"command": "touch docs/a.md && mv docs/a.md docs/b.md && echo hi > docs/c.md"}),
            )],
            &[ok_result()],
        );
        assert_eq!(
            events,
            vec![
                ObservationEvent::FileMutated {
                    source_tool: "bash:touch".into(),
                    path: PathBuf::from("docs/a.md"),
                },
                ObservationEvent::FileMutated {
                    source_tool: "bash:mv".into(),
                    path: PathBuf::from("docs/b.md"),
                },
                ObservationEvent::FileMutated {
                    source_tool: "bash:redirection".into(),
                    path: PathBuf::from("docs/c.md"),
                },
            ]
        );
    }

    #[test]
    fn bash_sed_records_file_read() {
        let catalog = catalog(vec![]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call(
                "bash",
                json!({"command": "sed -n '1,80p' core/crates/omegon/src/conversation.rs"}),
            )],
            &[ok_result()],
        );
        assert_eq!(
            events,
            vec![ObservationEvent::FileRead {
                source_tool: "bash:sed".into(),
                path: PathBuf::from("core/crates/omegon/src/conversation.rs")
            }]
        );
    }

    #[test]
    fn bash_search_records_search() {
        let catalog = catalog(vec![]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call(
                "bash",
                json!({"command": "rg OrientationChurn core/crates/omegon/src docs"}),
            )],
            &[ok_result()],
        );
        assert_eq!(
            events,
            vec![ObservationEvent::SearchPerformed {
                source_tool: "bash:rg".into(),
                query: Some("OrientationChurn".into()),
                roots: vec![
                    PathBuf::from("core/crates/omegon/src"),
                    PathBuf::from("docs")
                ],
            }]
        );
    }

    #[test]
    fn bash_validation_and_commit_are_observed() {
        let catalog = catalog(vec![]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call(
                "bash",
                json!({"command": "cargo test -p omegon pressure_behavior --locked && git commit -m 'fix: x'"}),
            )],
            &[ok_result()],
        );
        assert_eq!(
            events,
            vec![
                ObservationEvent::ValidationRun {
                    source_tool: "bash".into(),
                },
                ObservationEvent::ProgressBoundary {
                    source_tool: "bash".into(),
                    clears_mutation_state: true,
                },
            ]
        );
    }

    #[test]
    fn unknown_bash_program_is_opaque() {
        let catalog = catalog(vec![]);
        let events = ObservationNormalizer::new(&catalog).normalize(
            &[call("bash", json!({"command": "custom-tool --flag value"}))],
            &[ok_result()],
        );
        assert!(events.is_empty());
    }
}

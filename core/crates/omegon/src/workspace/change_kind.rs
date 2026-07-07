use std::path::{Path, PathBuf};

use super::types::WorkspaceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Code,
    ProjectDocs,
    HumanDocs,
    KnowledgeNotes,
    Spec,
    Mixed,
    Unknown,
}

pub fn classify_change_kind(
    cwd: &Path,
    workspace_kind: WorkspaceKind,
    paths: &[PathBuf],
) -> ChangeKind {
    let mut saw_code = false;
    let mut saw_project_docs = false;
    let mut saw_human_docs = false;
    let mut saw_notes = false;
    let mut saw_spec = false;
    let mut saw_unknown = false;

    for path in paths {
        let rel = normalize_relative(cwd, path);
        if is_code_path(&rel) {
            saw_code = true;
        } else if is_spec_path(&rel) {
            saw_spec = true;
        } else if is_knowledge_note_path(&rel, workspace_kind) {
            saw_notes = true;
        } else if is_project_docs_path(&rel, workspace_kind) {
            saw_project_docs = true;
        } else if is_human_document_path(&rel, workspace_kind) {
            saw_human_docs = true;
        } else {
            saw_unknown = true;
        }
    }

    let categories = [
        saw_code,
        saw_project_docs,
        saw_human_docs,
        saw_notes,
        saw_spec,
        saw_unknown,
    ]
    .into_iter()
    .filter(|seen| *seen)
    .count();

    if categories > 1 {
        return ChangeKind::Mixed;
    }
    if saw_code {
        ChangeKind::Code
    } else if saw_spec {
        ChangeKind::Spec
    } else if saw_notes {
        ChangeKind::KnowledgeNotes
    } else if saw_project_docs {
        ChangeKind::ProjectDocs
    } else if saw_human_docs {
        ChangeKind::HumanDocs
    } else {
        ChangeKind::Unknown
    }
}

fn normalize_relative(cwd: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(cwd).unwrap_or(path).to_path_buf()
}

fn path_str(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

fn is_code_path(path: &Path) -> bool {
    let text = path_str(path);
    if matches!(
        text.as_str(),
        "Cargo.toml"
            | "Cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "pyproject.toml"
            | "go.mod"
            | "go.sum"
    ) || text.starts_with(".github/workflows/")
    {
        return true;
    }
    matches!(
        extension(path).as_deref(),
        Some(
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "kt" | "swift"
                | "c" | "h" | "cpp" | "hpp" | "cs" | "rb" | "php" | "sh" | "sql"
        )
    )
}

fn is_spec_path(path: &Path) -> bool {
    let text = path_str(path);
    text.starts_with("openspec/") || text.ends_with(".openapi.yaml") || text.ends_with(".openapi.yml")
}

fn is_project_docs_path(path: &Path, workspace_kind: WorkspaceKind) -> bool {
    let text = path_str(path);
    if matches!(text.as_str(), "README.md" | "CHANGELOG.md")
        || text.starts_with("docs/")
        || text.starts_with("skills/") && text.ends_with("/SKILL.md")
    {
        return true;
    }
    matches!(workspace_kind, WorkspaceKind::Code | WorkspaceKind::Mixed | WorkspaceKind::Spec)
        && matches!(extension(path).as_deref(), Some("md" | "rst" | "adoc"))
}

fn is_knowledge_note_path(path: &Path, workspace_kind: WorkspaceKind) -> bool {
    let text = path_str(path);
    text.starts_with(".obsidian/")
        || text.starts_with("notes/")
        || matches!(workspace_kind, WorkspaceKind::Vault)
            && matches!(extension(path).as_deref(), Some("md" | "txt"))
}

fn is_human_document_path(path: &Path, workspace_kind: WorkspaceKind) -> bool {
    let text = path_str(path);
    if text.starts_with("drafts/") || text.starts_with("writing/") || text.starts_with("content/")
    {
        return true;
    }
    matches!(workspace_kind, WorkspaceKind::Knowledge | WorkspaceKind::Generic)
        && matches!(extension(path).as_deref(), Some("md" | "txt" | "rst" | "adoc"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn classifies_vault_markdown_as_knowledge_notes() {
        assert_eq!(
            classify_change_kind(Path::new("."), WorkspaceKind::Vault, &[p("notes.md")]),
            ChangeKind::KnowledgeNotes
        );
    }

    #[test]
    fn classifies_plain_markdown_workspace_as_human_docs() {
        assert_eq!(
            classify_change_kind(
                Path::new("."),
                WorkspaceKind::Knowledge,
                &[p("chapter-1.md")]
            ),
            ChangeKind::HumanDocs
        );
    }

    #[test]
    fn classifies_docs_in_code_repo_as_project_docs() {
        assert_eq!(
            classify_change_kind(Path::new("."), WorkspaceKind::Mixed, &[p("docs/api.md")]),
            ChangeKind::ProjectDocs
        );
    }

    #[test]
    fn classifies_changelog_as_project_docs() {
        assert_eq!(
            classify_change_kind(Path::new("."), WorkspaceKind::Code, &[p("CHANGELOG.md")]),
            ChangeKind::ProjectDocs
        );
    }

    #[test]
    fn classifies_code_and_docs_as_mixed() {
        assert_eq!(
            classify_change_kind(
                Path::new("."),
                WorkspaceKind::Mixed,
                &[p("src/lib.rs"), p("docs/api.md")]
            ),
            ChangeKind::Mixed
        );
    }

    #[test]
    fn classifies_skill_manifest_as_project_docs() {
        assert_eq!(
            classify_change_kind(
                Path::new("."),
                WorkspaceKind::Mixed,
                &[p("skills/git/SKILL.md")]
            ),
            ChangeKind::ProjectDocs
        );
    }
}

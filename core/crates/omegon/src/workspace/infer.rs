use std::path::Path;

use super::types::WorkspaceKind;

pub fn infer_workspace_kind(cwd: &Path) -> WorkspaceKind {
    let has_obsidian = cwd.join(".obsidian").is_dir();
    let has_openspec = cwd.join("openspec").is_dir();
    let has_docs = cwd.join("docs").is_dir();
    let has_markdown = has_markdown_files(cwd);
    let has_code_manifests = ["Cargo.toml", "package.json", "pyproject.toml"]
        .iter()
        .any(|name| cwd.join(name).exists());

    if has_obsidian {
        return WorkspaceKind::Vault;
    }
    if has_openspec && !has_code_manifests {
        return WorkspaceKind::Spec;
    }
    if has_code_manifests && (has_docs || has_openspec || has_markdown) {
        return WorkspaceKind::Mixed;
    }
    if has_code_manifests {
        return WorkspaceKind::Code;
    }
    if has_docs || has_markdown {
        return WorkspaceKind::Knowledge;
    }
    WorkspaceKind::Generic
}

fn has_markdown_files(cwd: &Path) -> bool {
    std::fs::read_dir(cwd)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_vault_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        std::fs::write(dir.path().join("notes.md"), "# Notes\n").unwrap();
        assert_eq!(infer_workspace_kind(dir.path()), WorkspaceKind::Vault);
    }

    #[test]
    fn infer_spec_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("openspec")).unwrap();
        assert_eq!(infer_workspace_kind(dir.path()), WorkspaceKind::Spec);
    }

    #[test]
    fn infer_mixed_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        assert_eq!(infer_workspace_kind(dir.path()), WorkspaceKind::Mixed);
    }

    #[test]
    fn infer_code_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}\n").unwrap();
        assert_eq!(infer_workspace_kind(dir.path()), WorkspaceKind::Code);
    }

    #[test]
    fn infer_knowledge_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Notes\n").unwrap();
        assert_eq!(infer_workspace_kind(dir.path()), WorkspaceKind::Knowledge);
    }

    #[test]
    fn infer_generic_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "hello\n").unwrap();
        assert_eq!(infer_workspace_kind(dir.path()), WorkspaceKind::Generic);
    }
}

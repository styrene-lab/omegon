//! Validate explicit lifecycle conclusions against bounded repository artifacts.
use omegon_memory::{LifecycleConclusionKind, LifecycleConclusionSource, Section};
use std::path::Path;

pub(super) struct ValidatedConclusion {
    pub content: String,
    pub source: LifecycleConclusionSource,
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn validate(
    root: &Path,
    path: &str,
    source_kind: &str,
    reference_type: &str,
    sub: &str,
    section: &Section,
    claim: &str,
) -> anyhow::Result<ValidatedConclusion> {
    if claim.trim().is_empty() || claim.len() > 65_536 || sub.trim().is_empty() || sub.len() > 2048
    {
        anyhow::bail!("explicit lifecycle claim or subreference exceeds its bounds");
    }
    if path.len() > 2048
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        || !path.ends_with(".md")
    {
        anyhow::bail!("lifecycle artifact requires a repository-relative Markdown path");
    }
    let design = source_kind == "design-tree"
        && reference_type == "design"
        && path.starts_with("docs/design/");
    let spec = source_kind == "openspec"
        && reference_type == "spec"
        && (path.starts_with("openspec/baseline/") || archived_spec(path));
    if !design && !spec {
        anyhow::bail!("unsupported explicit lifecycle artifact scope");
    }
    let bytes = snapshot(root, path)?;
    let text = std::str::from_utf8(&bytes)?;
    let mut choices = Vec::new();
    let mut artifact_id = None;
    if design {
        let parsed = omegon_opsx::parse_design_artifact(text, path)?;
        if !matches!(
            parsed.artifact.state,
            omegon_opsx::NodeState::Decided
                | omegon_opsx::NodeState::Implementing
                | omegon_opsx::NodeState::Implemented
        ) || !parsed.diagnostics.is_empty()
        {
            anyhow::bail!(
                "explicit lifecycle admission requires an unambiguous decided design artifact"
            );
        }
        artifact_id = Some(parsed.artifact.id);
        match section {
            Section::Decisions => {
                for decision in parsed.sections.decisions {
                    if decision.title == sub
                        && decision.status.eq_ignore_ascii_case("decided")
                        && !decision.rationale.trim().is_empty()
                    {
                        choices.push((
                            LifecycleConclusionKind::Decision,
                            normalize(&format!("{}: {}", decision.title, decision.rationale)),
                        ));
                    }
                }
            }
            Section::Constraints if sub == "Implementation Notes/Constraints" => {
                for constraint in parsed.sections.implementation.constraints {
                    choices.push((LifecycleConclusionKind::Constraint, normalize(&constraint)));
                }
            }
            _ => anyhow::bail!(
                "design conclusion must reference a decision or implementation constraint"
            ),
        }
    } else {
        if *section != Section::Specs {
            anyhow::bail!("specification conclusions require the Specs section");
        }
        if text
            .lines()
            .filter(|line| {
                line.trim()
                    .strip_prefix("### Requirement:")
                    .is_some_and(|title| title.trim() == sub)
            })
            .count()
            != 1
        {
            anyhow::bail!(
                "explicit specification admission requires one matching Requirement heading"
            );
        }
        for requirement in omegon_opsx::parse_spec_content(text) {
            if requirement.title == sub && !requirement.description.trim().is_empty() {
                choices.push((
                    LifecycleConclusionKind::Specification,
                    normalize(&format!(
                        "{}: {}",
                        requirement.title, requirement.description
                    )),
                ));
            }
        }
    }
    let claim = normalize(claim);
    let mut matches = choices.into_iter().filter(|(_, text)| *text == claim);
    let Some((kind, content)) = matches.next() else {
        anyhow::bail!("claim does not match a structured explicit conclusion in the artifact");
    };
    if matches.next().is_some() {
        anyhow::bail!("ambiguous lifecycle conclusion");
    }
    let source = LifecycleConclusionSource {
        kind,
        artifact_path: path.into(),
        artifact_id,
        artifact_sub: sub.into(),
        artifact_sha256: omegon_memory::retrieval::raw_content_hash(text),
        statement_sha256: omegon_memory::retrieval::raw_content_hash(&content),
    };
    source.validate(&content, section)?;
    Ok(ValidatedConclusion { content, source })
}

fn archived_spec(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    parts.len() >= 5
        && parts[0] == "openspec"
        && parts[1] == "archive"
        && parts[3] == "specs"
        && parts[2]
            .get(..10)
            .is_some_and(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
        && parts[2].as_bytes().get(10) == Some(&b'-')
        && parts[2].len() > 11
}

#[cfg(unix)]
fn snapshot(root: &Path, path: &str) -> anyhow::Result<Vec<u8>> {
    let root = std::fs::canonicalize(root)?;
    let mut directory = omegon_maintenance_contracts::open_secure_root(&root)?;
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return crate::contribution_loading::read_file_at(
                &directory,
                part.as_bytes(),
                1024 * 1024,
            )?
            .ok_or_else(|| anyhow::anyhow!("lifecycle artifact is unavailable"));
        }
        directory = crate::contribution_loading::open_child_directory(&directory, part.as_bytes())?
            .ok_or_else(|| anyhow::anyhow!("lifecycle artifact directory is unavailable"))?;
    }
    anyhow::bail!("lifecycle artifact is unavailable")
}

#[cfg(not(unix))]
fn snapshot(_: &Path, _: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::bail!("explicit lifecycle artifact validation requires a supported Unix host")
}

#[cfg(test)]
pub(super) const TEST_DESIGN: &str = "---\nid: zircon\ntitle: Zircon\nstatus: decided\nopen_questions:\n  - Is remote sync safe?\n---\n# Zircon\n\n## Decisions\n\n### Use transactions\n\n**Status:** decided\n\n**Rationale:** Keep corrections atomic.\n\n## Implementation Notes\n\n### Constraints\n\n- Never infer deletion.\n\n## Open Questions\n\n- Is remote sync safe?\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_artifact_validation_distinguishes_conclusions_from_chatter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs/design")).unwrap();
        let path = dir.path().join("docs/design/zircon.md");
        std::fs::write(&path, TEST_DESIGN).unwrap();
        let check = |sub, section: &Section, claim| {
            validate(
                dir.path(),
                "docs/design/zircon.md",
                "design-tree",
                "design",
                sub,
                section,
                claim,
            )
        };
        let decision = check(
            "Use transactions",
            &Section::Decisions,
            "Use transactions: Keep corrections atomic.",
        )
        .unwrap();
        assert_eq!(decision.source.artifact_id.as_deref(), Some("zircon"));
        assert_eq!(
            decision.source.artifact_sha256,
            omegon_memory::retrieval::raw_content_hash(TEST_DESIGN)
        );
        assert!(
            check(
                "Use transactions",
                &Section::Decisions,
                "Use transactions: Tests passed."
            )
            .is_err()
        );
        assert!(
            check(
                "Implementation Notes/Constraints",
                &Section::Constraints,
                "Never infer deletion."
            )
            .is_ok()
        );
        assert!(
            check(
                "Implementation Notes/Constraints",
                &Section::Constraints,
                "Is remote sync safe?"
            )
            .is_err()
        );
        std::fs::write(
            &path,
            TEST_DESIGN.replace("status: decided", "status: exploring"),
        )
        .unwrap();
        assert!(
            check(
                "Use transactions",
                &Section::Decisions,
                "Use transactions: Keep corrections atomic."
            )
            .is_err()
        );
    }

    #[test]
    fn lifecycle_artifact_scope_allows_baseline_and_archive_specs() {
        let dir = tempfile::tempdir().unwrap();
        let spec = "### Requirement: Atomic corrections\n\nCorrections SHALL be atomic.\n\n#### Scenario: Failure\nGiven an error\nWhen committing\nThen roll back\n";
        for path in [
            "openspec/baseline/memory/test.md",
            "openspec/archive/2026-09-08-test/specs/memory/test.md",
            "openspec/changes/test/specs/memory/test.md",
        ] {
            let absolute = dir.path().join(path);
            std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
            std::fs::write(absolute, spec).unwrap();
            let result = validate(
                dir.path(),
                path,
                "openspec",
                "spec",
                "Atomic corrections",
                &Section::Specs,
                "Atomic corrections: Corrections SHALL be atomic.",
            );
            assert_eq!(
                result.is_ok(),
                !path.starts_with("openspec/changes/"),
                "{path}"
            );
        }
        let note = dir.path().join("openspec/baseline/memory/note.md");
        std::fs::write(note, spec.replace("### Requirement:", "###")).unwrap();
        assert!(
            validate(
                dir.path(),
                "openspec/baseline/memory/note.md",
                "openspec",
                "spec",
                "Atomic corrections",
                &Section::Specs,
                "Atomic corrections: Corrections SHALL be atomic."
            )
            .is_err()
        );
        assert!(
            validate(
                dir.path(),
                "../outside.md",
                "openspec",
                "spec",
                "title",
                &Section::Specs,
                "claim"
            )
            .is_err()
        );
        assert!(
            validate(
                dir.path(),
                "docs/design/missing.md",
                "design-tree",
                "design",
                "title",
                &Section::Decisions,
                "claim"
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_artifact_snapshot_rejects_symlinks_and_oversized_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs/design")).unwrap();
        let path = dir.path().join("docs/design/zircon.md");
        let target = outside.path().join("zircon.md");
        std::fs::write(&target, TEST_DESIGN).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(snapshot(dir.path(), "docs/design/zircon.md").is_err());
        std::fs::remove_file(&path).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(1024 * 1024 + 1).unwrap();
        assert!(snapshot(dir.path(), "docs/design/zircon.md").is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(dir.path().join("docs/design")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("docs/design")).unwrap();
        assert!(snapshot(dir.path(), "docs/design/zircon.md").is_err());
    }
}

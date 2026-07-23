//! Explicit bridges from Omegon's canonical skill inventory to editor-native inventories.
//!
//! Omegon remains the source of truth. The Zed bridge uses symlinks rather than
//! copying skill contents, writes only links it can prove it owns, and refuses
//! to replace user-authored files or links.

use std::path::{Path, PathBuf};

const ZED_MANAGED_DIR: &str = "omegon-managed";
const ZED_LEGACY_SKILLS_DIR: &str = ".config/zed/skills";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ZedSkillBridgeReport {
    pub source_count: usize,
    pub linked_count: usize,
    pub unchanged_count: usize,
    pub removed_count: usize,
    pub conflict_count: usize,
    pub conflicts: Vec<String>,
    pub dry_run: bool,
    pub target_dir: PathBuf,
}

impl ZedSkillBridgeReport {
    pub fn render(&self) -> String {
        let mut lines = vec![
            "Zed Skill Bridge".to_string(),
            format!(
                "  source: ~/.omegon/skills ({} portable skills)",
                self.source_count
            ),
            format!("  target: {}", self.target_dir.display()),
            format!(
                "  status: {} linked · {} unchanged · {} removed · {} conflicts{}",
                self.linked_count,
                self.unchanged_count,
                self.removed_count,
                self.conflict_count,
                if self.dry_run { " (dry run)" } else { "" }
            ),
            "  ownership: Omegon canonical; Zed entries are scope-qualified managed symlinks"
                .to_string(),
            "  runtime: Zed discovers skills; Omegon does not import Zed-native skills".to_string(),
        ];
        if !self.conflicts.is_empty() {
            lines.push("  conflicts:".to_string());
            lines.extend(self.conflicts.iter().map(|item| format!("    - {item}")));
        }
        lines.join("\n")
    }
}

pub fn zed_status() -> anyhow::Result<ZedSkillBridgeReport> {
    let source = user_skills_dir()?;
    let target = zed_managed_skills_dir()?;
    inspect_zed_bridge(&source, &target)
}

pub fn zed_sync(dry_run: bool) -> anyhow::Result<ZedSkillBridgeReport> {
    let source = user_skills_dir()?;
    let target = zed_managed_skills_dir()?;
    let mut report = sync_zed_bridge(&source, &target, dry_run)?;
    cleanup_legacy_zed_bridge(&mut report, dry_run)?;
    Ok(report)
}

fn user_skills_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".omegon/skills"))
        .ok_or_else(|| anyhow::anyhow!("home directory is unavailable"))
}

fn zed_managed_skills_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".agents/skills"))
        .ok_or_else(|| anyhow::anyhow!("home directory is unavailable"))
}

fn zed_legacy_managed_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(ZED_LEGACY_SKILLS_DIR).join(ZED_MANAGED_DIR))
        .ok_or_else(|| anyhow::anyhow!("home directory is unavailable"))
}

fn zed_link_name(skill_name: &str) -> String {
    format!("omegon-{skill_name}")
}

fn portable_skill_sources(source: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut skills = Vec::new();
    if !source.exists() {
        return Ok(skills);
    }
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if crate::skills::validate_skill_name(&name).is_err() {
            continue;
        }
        let skill_file = entry.path().join("SKILL.md");
        if !skill_file.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&skill_file)?;
        let (manifest, _) = omegon_skills::parse_skill_file(&content);
        if manifest.name.trim().is_empty() || manifest.description.trim().is_empty() {
            continue;
        }
        skills.push((name, entry.path()));
    }
    skills.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(skills)
}

fn inspect_zed_bridge(source: &Path, target: &Path) -> anyhow::Result<ZedSkillBridgeReport> {
    let sources = portable_skill_sources(source)?;
    let mut report = ZedSkillBridgeReport {
        source_count: sources.len(),
        target_dir: target.to_path_buf(),
        ..Default::default()
    };
    for (name, source_path) in sources {
        let link = target.join(zed_link_name(&name));
        match std::fs::read_link(&link) {
            Ok(destination) if destination == source_path => report.unchanged_count += 1,
            Ok(_) => {
                report.conflict_count += 1;
                report
                    .conflicts
                    .push(format!("{name}: managed link points elsewhere"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                report.conflict_count += 1;
                report.conflicts.push(format!(
                    "{name}: target exists and is not a managed symlink"
                ));
            }
        }
    }
    Ok(report)
}

fn sync_zed_bridge(
    source: &Path,
    target: &Path,
    dry_run: bool,
) -> anyhow::Result<ZedSkillBridgeReport> {
    let sources = portable_skill_sources(source)?;
    let source_names: std::collections::BTreeSet<_> = sources
        .iter()
        .map(|(name, _)| zed_link_name(name))
        .collect();
    let mut report = ZedSkillBridgeReport {
        source_count: sources.len(),
        dry_run,
        target_dir: target.to_path_buf(),
        ..Default::default()
    };

    if !dry_run {
        std::fs::create_dir_all(target)?;
    }

    for (name, source_path) in &sources {
        let link_name = zed_link_name(name);
        let link = target.join(&link_name);
        match std::fs::symlink_metadata(&link) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if std::fs::read_link(&link)? == *source_path {
                    report.unchanged_count += 1;
                } else {
                    report.conflict_count += 1;
                    report
                        .conflicts
                        .push(format!("{name}: existing symlink points elsewhere"));
                }
            }
            Ok(_) => {
                report.conflict_count += 1;
                report
                    .conflicts
                    .push(format!("{name}: target exists and is not a symlink"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !dry_run {
                    create_dir_symlink(source_path, &link)?;
                }
                report.linked_count += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }

    if target.exists() {
        for entry in std::fs::read_dir(target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if source_names.contains(&name) {
                continue;
            }
            if !name.starts_with("omegon-") {
                continue;
            }
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() {
                if !dry_run {
                    std::fs::remove_file(entry.path())?;
                }
                report.removed_count += 1;
            } else {
                report.conflict_count += 1;
                report
                    .conflicts
                    .push(format!("{name}: stale target is not a symlink"));
            }
        }
    }

    Ok(report)
}

fn cleanup_legacy_zed_bridge(
    report: &mut ZedSkillBridgeReport,
    dry_run: bool,
) -> anyhow::Result<()> {
    let legacy = zed_legacy_managed_dir()?;
    if !legacy.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&legacy)? {
        let entry = entry?;
        if std::fs::symlink_metadata(entry.path())?
            .file_type()
            .is_symlink()
        {
            if !dry_run {
                std::fs::remove_file(entry.path())?;
            }
            report.removed_count += 1;
        }
    }
    if !dry_run && legacy.read_dir()?.next().is_none() {
        std::fs::remove_dir(&legacy)?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_dir_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn create_dir_symlink(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\n---\n\n# Test\n"),
        )
        .unwrap();
    }

    #[test]
    fn zed_sync_links_portable_skills_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        write_skill(&source, "rust");

        let first = sync_zed_bridge(&source, &target, false).unwrap();
        assert_eq!(first.linked_count, 1);
        assert_eq!(
            std::fs::read_link(target.join("omegon-rust")).unwrap(),
            source.join("rust")
        );

        let second = sync_zed_bridge(&source, &target, false).unwrap();
        assert_eq!(second.unchanged_count, 1);
        assert_eq!(second.linked_count, 0);
    }

    #[test]
    fn zed_sync_refuses_non_symlink_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        write_skill(&source, "rust");
        std::fs::create_dir_all(target.join("omegon-rust")).unwrap();

        let report = sync_zed_bridge(&source, &target, false).unwrap();
        assert_eq!(report.conflict_count, 1);
        assert!(target.join("omegon-rust").is_dir());
    }

    #[test]
    fn zed_sync_dry_run_does_not_create_target() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        write_skill(&source, "rust");

        let report = sync_zed_bridge(&source, &target, true).unwrap();
        assert_eq!(report.linked_count, 1);
        assert!(!target.exists());
    }
}

//! Skill management — schema, parsing, listing, and installation.
//!
//! Skills are markdown directive files injected into the system prompt at session start.
//! Shipped skills are discovered from an installed contribution-pack directory;
//! user and project skills retain their existing override precedence.
//!
//! Two-tier load order (established by AugmentRegistry::load_skills):
//!   1. ~/.omegon/skills/*/SKILL.md   — bundled / user-installed
//!   2. <cwd>/.omegon/skills/*/SKILL.md — project-local (overrides same-named user skills)
//!
//! ## Skill Schema
//!
//! SKILL.md files use YAML (`---`) frontmatter canonically, with TOML (`+++`)
//! retained as a compatibility format for existing Omegon skills:
//!
//! ```yaml
//! ---
//! # ── Required ─────────────────────────────────────────
//! name: my-skill
//! description: What this skill does
//!
//! # ── Identity (auto-generated) ────────────────────────
//! id: uuid                          # unique identifier
//! version: 1.0.0                    # skill version
//! tags: [domain, category]          # discovery/filtering
//! aliases: [shortname]              # alternative invocation names
//!
//! # ── Invocation ───────────────────────────────────────
//! triggers:                         # phrases that activate this skill
//!   - evaluate this opportunity
//!   - assess this solicitation
//! activation: intent_detected       # always | intent_detected | project_detected | domain_detected | lifecycle_gated
//! profile: [coding]                 # coding | lifecycle | docs | infra | design
//! project_signals: [Cargo.toml]     # files/globs that suggest activation
//!
//! # ── Access ───────────────────────────────────────────
//! trusted_paths:                    # auto-trusted on load
//!   - ~/Documents/data/
//!
//! # ── Output ───────────────────────────────────────────
//! output_path: ~/Documents/output/  # where results are written
//! output_format: markdown           # markdown | json
//!
//! # ── Constraints ──────────────────────────────────────
//! max_turns: 100                    # override session default
//! posture: architect                # recommended posture
//! ---
//! ```
//!
//! All fields except `name` and `description` are optional.
//! The markdown body after the frontmatter is the skill's directive content.

pub use omegon_skills::{
    SkillEntry, SkillManifest, SkillPhaseInfo, collect_phase_info, collect_trusted_paths,
};

use omegon_skills::{
    PendingSkillEntry, SkillBundleSummary, adapted_skill_manifest_warnings, discover_skill_bundles,
    doctor_candidate_conflicts, finalize_skill_entries, parse_skill_file,
    skill_entry_provider_rank,
};

#[cfg(test)]
use omegon_skills::{
    SkillSignalKind, find_script_references, match_project_signal, skill_sources_conflict,
    validate_activation_metadata, validate_project_signal,
};

#[cfg(any(feature = "tui", test))]
pub use omegon_skills::skill_builder_prompt;

#[derive(Debug, Clone)]
struct ShippedSkill {
    name: String,
    content: String,
}

fn shipped_skills() -> anyhow::Result<Vec<ShippedSkill>> {
    let pack = crate::content_pack::boot_pack()
        .ok_or_else(|| anyhow::anyhow!("shipped content pack is unavailable"))?;
    shipped_skills_from_pack(&pack)
}

fn shipped_skills_from_pack(
    pack: &crate::content_pack::ContentPack,
) -> anyhow::Result<Vec<ShippedSkill>> {
    let mut skills = Vec::new();
    for asset in pack.assets("skill") {
        let path = std::path::Path::new(&asset.manifest.path);
        if path.components().count() != 3
            || !path.starts_with("skills")
            || path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md")
        {
            continue;
        }
        let name = path
            .parent()
            .and_then(std::path::Path::file_name)
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("shipped skill has no valid name"))?;
        skills.push(ShippedSkill {
            name: name.to_string(),
            content: std::str::from_utf8(&asset.bytes)?.to_string(),
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

fn skills_dir() -> Option<std::path::PathBuf> {
    crate::paths::omegon_home().ok().map(|h| h.join("skills"))
}

/// Render bundled skills and their installation status as terminal-friendly text.
pub fn list_summary() -> anyhow::Result<String> {
    let skills_dir = skills_dir();

    let mut lines = vec![format!("Bundled skills ({})\n", shipped_skills()?.len())];

    for shipped in shipped_skills()? {
        let name = shipped.name;
        let content = shipped.content;
        // Extract description from frontmatter if present
        let description = extract_description(&content).unwrap_or("(no description)");

        let installed = skills_dir
            .as_ref()
            .is_some_and(|d| d.join(&name).join("SKILL.md").exists());
        let status = if installed { "✓" } else { "○" };
        lines.push(format!("  {status} {name:<14} {description}"));
    }

    let install_path = skills_dir
        .as_ref()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "(unknown)".into());

    lines.push(format!("\nInstall location: {install_path}"));
    lines.push("  ✓ = installed    ○ = not yet installed".into());
    lines.push("\nRun `omegon skills install` to install all bundled skills.".into());

    // Show any project-local skills if cwd has them
    let cwd = std::env::current_dir()?;
    let project_skills = cwd.join(".omegon").join("skills");
    if project_skills.is_dir() {
        let mut local: Vec<String> = std::fs::read_dir(&project_skills)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("SKILL.md").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        local.sort();
        if !local.is_empty() {
            lines.push("\nProject-local skills (.omegon/skills/):".into());
            for name in &local {
                lines.push(format!("  ● {name}"));
            }
        }
    }

    Ok(lines.join("\n"))
}

/// List bundled skills and their installation status.
pub fn cmd_list() -> anyhow::Result<()> {
    println!("{}", list_summary()?);
    Ok(())
}

fn claude_skill_roots(cwd: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(("claude:user".into(), home.join(".claude").join("skills")));
        roots.push((
            "claude:user".into(),
            home.join(".claude-code").join("skills"),
        ));
    }
    roots.push(("claude:project".into(), cwd.join(".claude").join("skills")));
    roots.push((
        "claude:project".into(),
        cwd.join(".claude-code").join("skills"),
    ));
    roots
}

fn shell_quote_path(path: &std::path::Path) -> String {
    let value = path.display().to_string();
    if value.is_empty() {
        return "''".into();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '+'))
    {
        value
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub fn doctor_report() -> anyhow::Result<String> {
    let cwd = std::env::current_dir()?;
    doctor_report_for(&cwd)
}

fn doctor_report_for(cwd: &std::path::Path) -> anyhow::Result<String> {
    let entries = list_structured()?;
    let mut lines = vec!["# Skills doctor".to_string(), String::new()];
    lines.push("Detected compatible skill roots:".into());
    let mut total = 0usize;
    let mut conflict_count = 0usize;
    let mut missing_scripts = 0usize;
    let mut retrieval_key_findings = 0usize;
    for (source, root) in claude_skill_roots(cwd) {
        let bundles = discover_skill_bundles(&source, &root)?;
        if bundles.is_empty() {
            lines.push(format!("  ○ {source:<15} {}", root.display()));
            continue;
        }
        total += bundles.len();
        lines.push(format!(
            "  ● {source:<15} {} ({} skills)",
            root.display(),
            bundles.len()
        ));
        for bundle in bundles {
            let conflicts = doctor_candidate_conflicts(&bundle, &entries);
            conflict_count += conflicts.len();
            missing_scripts += bundle.missing_script_refs.len();
            let mut metadata = Vec::new();
            if bundle.manifest.description.is_empty() {
                metadata.push("missing-description".to_string());
            }
            if !bundle.missing_script_refs.is_empty() {
                metadata.push(format!(
                    "missing-scripts:{}",
                    bundle.missing_script_refs.join(",")
                ));
            }
            if !conflicts.is_empty() {
                metadata.push(format!("conflicts:{}", conflicts.join(",")));
            }
            let skill_file = bundle.path.join("SKILL.md");
            if let Ok(content) = std::fs::read_to_string(&skill_file) {
                let (manifest, body) = parse_skill_file(&content);
                for warning in adapted_skill_manifest_warnings(&manifest, &body) {
                    metadata.push(format!("adaptation-warning:{warning}"));
                }
                if let Some(finding) =
                    omegon_skills::disclosure::lint_retrieval_key(&manifest.description)
                {
                    retrieval_key_findings += 1;
                    metadata.push(format!("retrieval-key:{}", finding.message()));
                }
            }
            if metadata.is_empty() {
                metadata.push("compatible".into());
            }
            let import_flag = if bundle.source.contains(":project") {
                " --project"
            } else {
                ""
            };
            lines.push(format!(
                "    - {} — {} · import:`omegon skills import {}{}`",
                bundle.name,
                metadata.join(" · "),
                shell_quote_path(&bundle.path),
                import_flag
            ));
        }
    }
    lines.push(String::new());
    if let Some(skills_dir) = skills_dir() {
        let legacy_file = skills_dir.join("vault/SKILL.md");
        if let Ok(content) = std::fs::read_to_string(&legacy_file)
            && is_legacy_bundled_vault_skill(&content)
        {
            lines.push("Bundled skill rename notice:".into());
            lines.push(format!(
                "  - {} is the old bundled markdown skill; it was renamed to `flynt`. Run `omegon skills install` to remove the stale copy and install `flynt`.",
                legacy_file.display()
            ));
            lines.push(String::new());
        }
    }
    lines.push(format!("Summary: {total} compatible external skill bundle(s), {conflict_count} conflict marker(s), {missing_scripts} missing script reference(s), {retrieval_key_findings} retrieval-key finding(s)."));
    lines.push(String::new());
    lines.push("Recommended next steps:".into());
    lines.push("  - Fast path: run `omegon migrate claude-code` to copy detected Claude user/project skills plus Claude settings into Omegon.".into());
    lines.push("  - Import user-level Claude skills selectively with `omegon skills import <skill-dir>` (creates a copy under ~/.omegon/skills).".into());
    lines.push("  - Import project-level Claude skills with `omegon skills import <skill-dir> --project` (creates a copy under .omegon/skills).".into());
    lines.push("  - Re-run import with `--force` to refresh a copied skill after editing its Claude source.".into());
    lines.push("  - Resolve conflicts by creating a project-local merged skill; Omegon will not inject conflicting skill directives together.".into());
    Ok(lines.join("\n"))
}

pub fn cmd_doctor() -> anyhow::Result<()> {
    println!("{}", doctor_report()?);
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillInstallSummary {
    pub destination: std::path::PathBuf,
    pub installed: usize,
    pub updated: usize,
    pub removed_legacy: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillImportSummary {
    pub name: String,
    pub scope: String,
    pub source: std::path::PathBuf,
    pub destination: std::path::PathBuf,
    pub bundle: SkillBundleSummary,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillDeleteSummary {
    pub name: String,
    pub scope: String,
    pub path: std::path::PathBuf,
}

pub fn validate_skill_name(name: &str) -> anyhow::Result<String> {
    let slug: String = name
        .trim()
        .to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if slug.is_empty()
        || slug.contains("..")
        || slug.contains('/')
        || slug.contains('\\')
        || slug.contains('\0')
    {
        anyhow::bail!("invalid skill name");
    }
    Ok(slug)
}

fn copy_skill_bundle_dir(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if file_name.to_string_lossy().starts_with('.') {
            continue;
        }
        let src = entry.path();
        let dst = destination.join(&file_name);
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_skill_bundle_dir(&src, &dst)?;
        } else if ty.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn summarize_imported_skill(root: &std::path::Path, entry_name: &str) -> SkillBundleSummary {
    let entries = list_structured().unwrap_or_default();
    omegon_skills::summarize_imported_skill(root, entry_name, &entries)
}

fn print_import_summary(summary: &SkillBundleSummary) {
    println!("Summary:");
    println!("  scripts: {}", summary.scripts.len());
    for script in summary.scripts.iter().take(5) {
        println!("    - {script}");
    }
    if summary.scripts.len() > 5 {
        println!("    - … {} more", summary.scripts.len() - 5);
    }
    println!("  resources: {}", summary.resources.len());
    for resource in summary.resources.iter().take(5) {
        println!("    - {resource}");
    }
    if summary.resources.len() > 5 {
        println!("    - … {} more", summary.resources.len() - 5);
    }
    if summary.conflicts.is_empty() {
        println!("  conflicts: none");
    } else {
        println!("  conflicts: {}", summary.conflicts.join(", "));
        println!(
            "  resolution: create a project-local merged skill; Omegon will not inject conflicting directives together"
        );
    }
}

pub fn import_skill(
    path: &std::path::Path,
    project: bool,
    force: bool,
) -> anyhow::Result<SkillImportSummary> {
    let project_root = project.then(std::env::current_dir).transpose()?;
    import_skill_at_root(path, project_root.as_deref(), force)
}

pub(crate) fn import_skill_at_root(
    path: &std::path::Path,
    project_root: Option<&std::path::Path>,
    force: bool,
) -> anyhow::Result<SkillImportSummary> {
    let source = path.canonicalize()?;
    let (source_dir, skill_file) = if source.is_dir() {
        (source.clone(), source.join("SKILL.md"))
    } else {
        let parent = source
            .parent()
            .ok_or_else(|| anyhow::anyhow!("skill file has no parent directory"))?
            .to_path_buf();
        (parent, source.clone())
    };
    if !skill_file.is_file() {
        anyhow::bail!("{} does not contain SKILL.md", source.display());
    }
    let content = std::fs::read_to_string(&skill_file)?;
    let (manifest, _body) = parse_skill_file(&content);
    let fallback_name = source_dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".into());
    let name = if manifest.name.trim().is_empty() {
        fallback_name
    } else {
        manifest.name
    };
    let slug = validate_skill_name(&name)?;
    let base = if let Some(project_root) = project_root {
        project_root.join(".omegon/skills")
    } else {
        skills_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
    };
    let destination = base.join(&slug);
    if destination.exists() {
        if !force {
            anyhow::bail!(
                "skill '{}' already exists at {}; pass --force to overwrite",
                slug,
                destination.display()
            );
        }
        std::fs::remove_dir_all(&destination)?;
    }
    std::fs::create_dir_all(&base)?;
    if source.is_dir() {
        copy_skill_bundle_dir(&source_dir, &destination)?;
    } else {
        std::fs::create_dir_all(&destination)?;
        std::fs::copy(&skill_file, destination.join("SKILL.md"))?;
    }
    let bundle = summarize_imported_skill(&destination, &slug);
    Ok(SkillImportSummary {
        name: slug,
        scope: if project_root.is_some() {
            "project"
        } else {
            "user"
        }
        .into(),
        source,
        destination,
        bundle,
    })
}

pub(crate) fn import_project_skill_guarded(
    path: &std::path::Path,
    project_root: &std::path::Path,
    home: &std::path::Path,
    force: bool,
) -> anyhow::Result<SkillImportSummary> {
    let source = path.canonicalize()?;
    let (source_dir, skill_file) = if source.is_dir() {
        (source.clone(), source.join("SKILL.md"))
    } else {
        let parent = source
            .parent()
            .ok_or_else(|| anyhow::anyhow!("skill file has no parent directory"))?
            .to_path_buf();
        (parent, source.clone())
    };
    if !skill_file.is_file() {
        anyhow::bail!("{} does not contain SKILL.md", source.display());
    }
    let source_parent = omegon_maintenance_contracts::open_secure_root(&source_dir)?;
    let skill_name = skill_file
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("skill file has no basename"))?
        .as_encoded_bytes();
    let content =
        crate::contribution_loading::read_file_at(&source_parent, skill_name, 4 * 1024 * 1024)?
            .ok_or_else(|| anyhow::anyhow!("skill file disappeared"))?;
    let content = String::from_utf8(content)?;
    let (manifest, _body) = parse_skill_file(&content);
    let fallback_name = source_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let name = if manifest.name.trim().is_empty() {
        fallback_name
    } else {
        &manifest.name
    };
    let slug = validate_skill_name(name)?;
    let directory =
        crate::contribution_loading::GuardedContributionMutationDirectory::open_or_create(
            project_root,
            &[b".omegon", b"skills"],
            home,
            omegon_maintenance_contracts::ContributionKind::Skill,
            "project",
        )?;
    if source.is_dir() {
        directory.import_directory(slug.as_bytes(), &source_parent, force)?;
    } else {
        directory.write_single_file_directory(
            slug.as_bytes(),
            b"SKILL.md",
            content.as_bytes(),
            force,
        )?;
    }
    let destination = project_root.join(".omegon/skills").join(&slug);
    let bundle = omegon_skills::summarize_imported_skill(&destination, &slug, &[]);
    Ok(SkillImportSummary {
        name: slug,
        scope: "project".into(),
        source,
        destination,
        bundle,
    })
}

pub fn cmd_import(path: &std::path::Path, project: bool, force: bool) -> anyhow::Result<()> {
    let summary = import_skill(path, project, force)?;
    println!(
        "Imported {} skill '{}' from {} to {}",
        summary.scope,
        summary.name,
        summary.source.display(),
        summary.destination.display()
    );
    print_import_summary(&summary.bundle);
    Ok(())
}

const LEGACY_VAULT_SKILL_ID: &str = "8d7961f6-4742-416f-89eb-bef9f6cc12f6";

fn is_legacy_bundled_vault_skill(content: &str) -> bool {
    let (manifest, _body) = parse_skill_file(content);
    manifest.name == "vault" && manifest.id.as_deref() == Some(LEGACY_VAULT_SKILL_ID)
}

fn remove_legacy_bundled_vault_skill(
    directory: &crate::contribution_loading::GuardedContributionMutationDirectory,
) -> anyhow::Result<bool> {
    let Some(legacy) = directory.open_directory(b"vault")? else {
        return Ok(false);
    };
    let Some(content) =
        crate::contribution_loading::read_file_at(&legacy, b"SKILL.md", 4 * 1024 * 1024)?
    else {
        return Ok(false);
    };
    let Ok(content) = std::str::from_utf8(&content) else {
        return Ok(false);
    };
    if !is_legacy_bundled_vault_skill(content)
        || crate::contribution_loading::read_directory_names(&legacy, 10_000)?
            .iter()
            .any(|name| name != b"SKILL.md")
    {
        return Ok(false); // Do not delete operator-owned supporting files.
    }
    directory.remove_directory(b"vault")
}

/// Install all bundled skills to ~/.omegon/skills/.
/// Existing files are overwritten. Project-local skills are never touched.
pub fn install_bundled_skills() -> anyhow::Result<SkillInstallSummary> {
    let home = crate::paths::omegon_home()?;
    let pack = crate::content_pack::boot_pack()
        .ok_or_else(|| anyhow::anyhow!("shipped content pack is unavailable"))?;
    install_bundled_skills_at(&home, &pack)
}

/// Explicit destination and admitted pack inputs also serve isolated installation tests.
pub(crate) fn install_bundled_skills_at(
    home: &std::path::Path,
    pack: &crate::content_pack::ContentPack,
) -> anyhow::Result<SkillInstallSummary> {
    let mut plans = Vec::new();
    for shipped in shipped_skills_from_pack(pack)? {
        let name = shipped.name;
        anyhow::ensure!(
            validate_skill_name(&name)? == name,
            "invalid bundled skill directory: {name}"
        );
        let prefix = format!("skills/{name}/");
        let files = pack
            .assets("skill")
            .filter_map(|asset| {
                asset
                    .manifest
                    .path
                    .strip_prefix(&prefix)
                    .map(|relative| (std::path::Path::new(relative), asset.bytes.as_ref()))
            })
            .collect::<Vec<_>>();
        plans.push((name, files));
    }
    let directory =
        crate::contribution_loading::GuardedContributionMutationDirectory::open_or_create(
            home,
            &[b"skills"],
            home,
            omegon_maintenance_contracts::ContributionKind::Skill,
            "user",
        )?;
    let mut installed = 0;
    let mut updated = 0;
    for (name, files) in plans {
        let (already_exists, changed) =
            directory.merge_manifest_files(name.as_bytes(), &files, b"SKILL.md")?;
        if !already_exists {
            installed += 1;
        } else if changed {
            updated += 1;
        }
    }
    let removed_legacy = remove_legacy_bundled_vault_skill(&directory)?;

    Ok(SkillInstallSummary {
        destination: home.join("skills"),
        installed,
        updated,
        removed_legacy,
    })
}

/// Install all bundled skills to ~/.omegon/skills/.
/// Existing files are overwritten. Project-local skills are never touched.
pub fn cmd_install() -> anyhow::Result<()> {
    let summary = install_bundled_skills()?;

    if summary.removed_legacy {
        println!("  - vault  (removed; renamed to flynt)");
    }
    for shipped in shipped_skills()? {
        let name = shipped.name;
        println!("  ✓ {name}");
    }

    println!(
        "\n{} skill(s) installed, {} updated → {}",
        summary.installed,
        summary.updated,
        summary.destination.display()
    );
    println!("Skills are active immediately in new sessions.");

    Ok(())
}

/// List all skills as structured entries for the ACP settings surface.
fn skill_path_stays_within_extension_root(
    extension_dir: &std::path::Path,
    relative_path: &str,
) -> Option<std::path::PathBuf> {
    let relative = std::path::Path::new(relative_path);
    if relative.is_absolute() {
        return None;
    }
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(extension_dir.join(relative))
}

fn read_extension_skill_entry(
    extension_dir: &std::path::Path,
    extension_name: &str,
    skill: &crate::extensions::manifest::ExtensionSkillConfig,
    existing_entries: &[SkillEntry],
) -> Option<SkillEntry> {
    let skill_path = skill_path_stays_within_extension_root(extension_dir, &skill.path)?;
    if !skill_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&skill_path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    let (manifest, _body) = parse_skill_file(&content);
    let name = skill
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .or_else(|| (!manifest.name.is_empty()).then(|| manifest.name.clone()))
        .or_else(|| {
            skill_path
                .parent()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
        })?;
    let source = format!("extension:{extension_name}");
    let shadows = existing_entries
        .iter()
        .filter(|entry| {
            entry.name == name
                && skill_entry_provider_rank(&source) >= skill_entry_provider_rank(&entry.source)
        })
        .map(|entry| entry.source.clone())
        .collect();
    Some(SkillEntry {
        name,
        description: manifest.description.clone(),
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        tags: manifest.tags.clone(),
        aliases: manifest.aliases.clone(),
        triggers: manifest.triggers.clone(),
        activation: manifest.activation.clone(),
        profile: manifest.profile.clone(),
        project_signals: manifest.project_signals.clone(),
        posture: manifest.posture.clone(),
        max_turns: manifest.max_turns,
        installed: true,
        bundled: false,
        project_local: false,
        source,
        editable: false,
        reloadable: true,
        shadows,
        conflicts: Vec::new(),
        path: skill_path
            .parent()
            .unwrap_or(extension_dir)
            .display()
            .to_string(),
    })
}

fn load_extension_skill_entries(existing_entries: &[SkillEntry]) -> Vec<SkillEntry> {
    let Ok(extensions_dir) = crate::extension_cli::extensions_dir() else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(extensions_dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for dir_entry in read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
    {
        let extension_dir = dir_entry.path();
        let Ok(manifest) =
            crate::extensions::manifest::ExtensionManifest::from_extension_dir(&extension_dir)
        else {
            continue;
        };
        for skill in &manifest.skills {
            let mut visible = existing_entries.to_vec();
            visible.extend(entries.clone());
            if let Some(entry) = read_extension_skill_entry(
                &extension_dir,
                &manifest.extension.name,
                skill,
                &visible,
            ) {
                entries.push(entry);
            }
        }
    }
    entries
}

/// Returns bundled skills (with installation status), user-installed skills,
/// and project-local skills in a single sorted list.
pub fn list_structured() -> anyhow::Result<Vec<SkillEntry>> {
    let home_skills = skills_dir();
    let cwd = std::env::current_dir()?;
    let project_skills = cwd.join(".omegon").join("skills");
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Bundled skills — always present, may or may not be installed
    for shipped in shipped_skills()? {
        let name = shipped.name;
        let content = shipped.content;
        let (manifest, _body) = parse_skill_file(&content);
        let installed = home_skills
            .as_ref()
            .is_some_and(|d| d.join(&name).join("SKILL.md").exists());
        let path = home_skills
            .as_ref()
            .map(|d| d.join(&name).display().to_string())
            .unwrap_or_default();
        entries.push(SkillEntry {
            name: name.to_string(),
            description: manifest.description.clone(),
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            tags: manifest.tags.clone(),
            aliases: manifest.aliases.clone(),
            triggers: manifest.triggers.clone(),
            activation: manifest.activation.clone(),
            profile: manifest.profile.clone(),
            project_signals: manifest.project_signals.clone(),
            posture: manifest.posture.clone(),
            max_turns: manifest.max_turns,
            installed,
            bundled: true,
            project_local: false,
            source: "bundled".into(),
            editable: false,
            reloadable: false,
            shadows: Vec::new(),
            conflicts: Vec::new(),
            path,
        });
        seen.insert(name.to_string());
    }

    // Extension-provided skills sit above bundled defaults and below operator-owned user/project skills.
    entries.extend(load_extension_skill_entries(&entries));

    // User-installed skills (non-bundled)
    if let Some(ref dir) = home_skills
        && dir.is_dir()
    {
        let mut user_skills: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("SKILL.md").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| !seen.contains(name))
            .collect();
        user_skills.sort();
        for name in user_skills {
            let skill_path = dir.join(&name).join("SKILL.md");
            let content = std::fs::read_to_string(&skill_path).unwrap_or_default();
            let (manifest, _body) = parse_skill_file(&content);
            entries.push(SkillEntry {
                name: name.clone(),
                description: manifest.description.clone(),
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                tags: manifest.tags.clone(),
                aliases: manifest.aliases.clone(),
                triggers: manifest.triggers.clone(),
                activation: manifest.activation.clone(),
                profile: manifest.profile.clone(),
                project_signals: manifest.project_signals.clone(),
                posture: manifest.posture.clone(),
                max_turns: manifest.max_turns,
                installed: true,
                bundled: false,
                project_local: false,
                source: "user".into(),
                editable: true,
                reloadable: true,
                shadows: Vec::new(),
                conflicts: Vec::new(),
                path: dir.join(&name).display().to_string(),
            });
            seen.insert(name);
        }
    }

    // Project-local skills
    if project_skills.is_dir() {
        let mut local: Vec<_> = std::fs::read_dir(&project_skills)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("SKILL.md").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        local.sort();
        for name in local {
            let skill_path = project_skills.join(&name).join("SKILL.md");
            let content = std::fs::read_to_string(&skill_path).unwrap_or_default();
            let (manifest, _body) = parse_skill_file(&content);
            let shadows = entries
                .iter()
                .filter(|entry| entry.name == name)
                .map(|entry| entry.source.clone())
                .collect();
            entries.push(SkillEntry {
                name: name.clone(),
                description: manifest.description.clone(),
                id: manifest.id.clone(),
                version: manifest.version.clone(),
                tags: manifest.tags.clone(),
                aliases: manifest.aliases.clone(),
                triggers: manifest.triggers.clone(),
                activation: manifest.activation.clone(),
                profile: manifest.profile.clone(),
                project_signals: manifest.project_signals.clone(),
                posture: manifest.posture.clone(),
                max_turns: manifest.max_turns,
                installed: true,
                bundled: false,
                project_local: true,
                source: "project".into(),
                editable: true,
                reloadable: true,
                shadows,
                conflicts: Vec::new(),
                path: project_skills.join(&name).display().to_string(),
            });
            seen.insert(name);
        }
    }

    Ok(finalize_skill_entries(
        entries
            .into_iter()
            .map(|entry| PendingSkillEntry {
                provider_rank: skill_entry_provider_rank(&entry.source),
                entry,
            })
            .collect(),
    ))
}

#[derive(Debug, Clone)]
pub struct SkillDetails {
    pub manifest: SkillManifest,
    pub body: String,
    pub path: std::path::PathBuf,
    pub entry: Option<SkillEntry>,
}

/// One skill as projected for progressive disclosure.
///
/// `body` is populated *only* when the disclosure entry admits it. This is the
/// safety property of the type: a caller cannot accidentally project a withheld
/// body, because a withheld body is not present to project.
#[derive(Debug, Clone)]
pub struct DisclosedSkill {
    pub entry: omegon_skills::disclosure::SkillDisclosureEntry,
    pub body: Option<String>,
}

impl DisclosedSkill {
    /// Resident-tier metadata is always available for discovery.
    pub fn name(&self) -> &str {
        &self.entry.name
    }

    pub fn description(&self) -> &str {
        &self.entry.description
    }
}

/// Build the progressive-disclosure projection over the installed skill set.
///
/// Resolution reuses [`get_skill`] so project-local skills continue to shadow
/// user skills, which continue to shadow bundled skills. Admission is decided
/// by declared activation plus workspace evidence under `root` and the current
/// operator `prompt`; a skill whose body is not admitted still appears in the
/// projection with its name and description intact.
pub fn disclosure_projection(
    root: &std::path::Path,
    prompt: Option<&str>,
) -> anyhow::Result<Vec<DisclosedSkill>> {
    let listed = list_structured()?;
    let mut disclosed = Vec::with_capacity(listed.len());

    for entry in listed {
        // Only skills that are actually resolvable can contribute a body.
        let Ok((manifest, body, _path)) = get_skill(&entry.name) else {
            continue;
        };
        let disclosure_entry = omegon_skills::disclosure::disclose(&manifest, root, prompt);
        let body = disclosure_entry.admits_body().then_some(body);
        disclosed.push(DisclosedSkill {
            entry: disclosure_entry,
            body,
        });
    }

    Ok(disclosed)
}

pub fn delete_external_skill(name: &str) -> anyhow::Result<SkillDeleteSummary> {
    let cwd = std::env::current_dir()?;
    delete_external_skill_at_root(name, &cwd)
}

pub(crate) fn delete_external_skill_at_root(
    name: &str,
    project_root: &std::path::Path,
) -> anyhow::Result<SkillDeleteSummary> {
    let slug = validate_skill_name(name)?;
    let project_dir = project_root.join(".omegon/skills").join(&slug);
    if project_dir.exists() {
        std::fs::remove_dir_all(&project_dir)?;
        return Ok(SkillDeleteSummary {
            name: slug,
            scope: "project".into(),
            path: project_dir,
        });
    }

    let user_dir = skills_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(&slug);
    if user_dir.exists() {
        std::fs::remove_dir_all(&user_dir)?;
        return Ok(SkillDeleteSummary {
            name: slug,
            scope: "user".into(),
            path: user_dir,
        });
    }

    anyhow::bail!("external skill '{slug}' not found")
}

pub(crate) fn delete_project_skill_guarded(
    name: &str,
    project_root: &std::path::Path,
    home: &std::path::Path,
) -> anyhow::Result<Option<SkillDeleteSummary>> {
    let slug = validate_skill_name(name)?;
    let Some(directory) =
        crate::contribution_loading::GuardedContributionMutationDirectory::open_existing(
            project_root,
            &[b".omegon", b"skills"],
            home,
            omegon_maintenance_contracts::ContributionKind::Skill,
            "project",
        )?
    else {
        return Ok(None);
    };
    if !directory.remove_directory(slug.as_bytes())? {
        return Ok(None);
    }
    Ok(Some(SkillDeleteSummary {
        name: slug.clone(),
        scope: "project".into(),
        path: project_root.join(".omegon/skills").join(slug),
    }))
}

pub(crate) fn delete_user_skill_at_home(
    name: &str,
    home: &std::path::Path,
) -> anyhow::Result<SkillDeleteSummary> {
    let slug = validate_skill_name(name)?;
    let path = home.join("skills").join(&slug);
    if !path.exists() {
        anyhow::bail!("external skill '{slug}' not found");
    }
    std::fs::remove_dir_all(&path)?;
    Ok(SkillDeleteSummary {
        name: slug,
        scope: "user".into(),
        path,
    })
}

/// Read a single skill's resolved manifest, body content, and listing metadata.
pub fn get_skill_details(name: &str) -> anyhow::Result<SkillDetails> {
    let (manifest, body, path) = get_skill(name)?;
    let entry = list_structured().ok().and_then(|entries| {
        let resolved_path = path.display().to_string();
        entries
            .into_iter()
            .find(|entry| entry.name == name && entry.path == resolved_path)
    });
    Ok(SkillDetails {
        manifest,
        body,
        path,
        entry,
    })
}

/// Read a single skill's manifest and body content.
pub fn get_skill(name: &str) -> anyhow::Result<(SkillManifest, String, std::path::PathBuf)> {
    if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains('\0') {
        anyhow::bail!("invalid skill name: path traversal rejected");
    }

    // Project-local takes precedence
    let cwd = std::env::current_dir()?;
    let project_path = cwd.join(".omegon/skills").join(name).join("SKILL.md");
    if project_path.exists() {
        let content = std::fs::read_to_string(&project_path)?;
        let (manifest, body) = parse_skill_file(&content);
        return Ok((manifest, body, project_path.parent().unwrap().to_path_buf()));
    }

    // User-installed / bundled
    if let Some(dir) = skills_dir() {
        let skill_path = dir.join(name).join("SKILL.md");
        if skill_path.exists() {
            let content = std::fs::read_to_string(&skill_path)?;
            let (manifest, body) = parse_skill_file(&content);
            return Ok((manifest, body, skill_path.parent().unwrap().to_path_buf()));
        }
    }

    // Check if it's a known bundled skill (not yet installed)
    for shipped in shipped_skills()? {
        let bname = shipped.name;
        let content = shipped.content;
        if bname == name {
            let (manifest, body) = parse_skill_file(&content);
            let path = skills_dir().map(|d| d.join(name)).unwrap_or_default();
            return Ok((manifest, body, path));
        }
    }

    anyhow::bail!("skill '{name}' not found")
}

/// Extract the `description` field from YAML frontmatter.
fn extract_description(content: &str) -> Option<&str> {
    // Support both YAML (---) and TOML (+++) frontmatter delimiters.
    let (body, delimiter) = if let Some(b) = content.strip_prefix("---\n") {
        (b, "\n---")
    } else {
        let b = content.strip_prefix("+++\n")?;
        (b, "\n+++")
    };
    let end = body.find(delimiter)?;
    let frontmatter = &body[..end];

    for line in frontmatter.lines() {
        // YAML: `description: Some text`
        if let Some(rest) = line.strip_prefix("description:") {
            return Some(rest.trim());
        }
        // TOML: `description = "Some text"`
        if let Some(rest) = line.strip_prefix("description") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                if rest.starts_with('"')
                    && rest.len() > 1
                    && let Some(end) = rest[1..].find('"')
                {
                    return Some(&rest[1..1 + end]);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn bundled_install_uses_admitted_bytes_not_uninventoried_or_changed_source_files() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let source = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let bundled = crate::content_pack::boot_pack().unwrap();
        for kind in ["skill", "prompt", "persona", "tone", "workflow", "catalog"] {
            for asset in bundled.assets(kind) {
                let path = source.path().join(&asset.manifest.path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &asset.bytes).unwrap();
            }
        }
        std::fs::copy(
            bundled.root.join("content-pack.toml"),
            source.path().join("content-pack.toml"),
        )
        .unwrap();
        std::fs::write(
            source.path().join("skills/scry/credentials.json"),
            "UNINVENTORIED SECRET",
        )
        .unwrap();
        std::fs::create_dir_all(source.path().join("skills/uninventoried-neighbor")).unwrap();
        std::fs::write(
            source.path().join("skills/uninventoried-neighbor/SKILL.md"),
            "not admitted",
        )
        .unwrap();
        let admitted = crate::content_pack::ContentPack::load(source.path()).unwrap();
        let reference = "skills/scry/references/usage.md";
        std::fs::write(outside.path().join("secret"), "OUTSIDE SECRET").unwrap();
        std::fs::remove_file(source.path().join(reference)).unwrap();
        symlink(outside.path().join("secret"), source.path().join(reference)).unwrap();
        assert!(
            crate::content_pack::ContentPack::load(source.path()).is_err(),
            "a new generation must reject the escaping source"
        );
        install_bundled_skills_at(home.path(), &admitted).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.path().join(reference)).unwrap(),
            admitted.text(reference).unwrap()
        );
        assert!(!home.path().join("skills/scry/credentials.json").exists());
        assert!(!home.path().join("skills/uninventoried-neighbor").exists());
        assert_eq!(
            std::fs::read_to_string(outside.path().join("secret")).unwrap(),
            "OUTSIDE SECRET"
        );
        let script = home
            .path()
            .join("skills/ratatui-tui/scripts/resolve_stack.py");
        assert_eq!(
            std::fs::read(&script).unwrap(),
            admitted
                .text("skills/ratatui-tui/scripts/resolve_stack.py")
                .unwrap()
                .as_bytes()
        );
        assert_eq!(
            std::fs::metadata(script).unwrap().permissions().mode() & 0o111,
            0,
            "installation grants no execution mode"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bundled_install_rejects_destination_symlinks_without_publishing_incomplete_entry() {
        use std::os::unix::fs::symlink;
        let pack = crate::content_pack::boot_pack().unwrap();
        for relative in [
            "",
            "skills",
            "skills/scry",
            "skills/scry/references",
            "skills/scry/references/usage.md",
        ] {
            let fixture = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let home = fixture.path().join("home");
            let link = if relative.is_empty() {
                home.clone()
            } else {
                home.join(relative)
            };
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            std::fs::write(outside.path().join("sentinel"), "UNCHANGED").unwrap();
            let target = if relative.ends_with("usage.md") {
                outside.path().join("sentinel")
            } else {
                outside.path().to_path_buf()
            };
            symlink(target, &link).unwrap();
            assert!(
                install_bundled_skills_at(&home, &pack).is_err(),
                "accepted linked destination {relative}"
            );
            assert_eq!(
                std::fs::read_to_string(outside.path().join("sentinel")).unwrap(),
                "UNCHANGED"
            );
            assert!(
                !home.join("skills/scry/SKILL.md").exists(),
                "failed reference plan published an entry"
            );
            assert!(!outside.path().join("SKILL.md").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn bundled_install_manifest_merge_is_bounded_confined_and_preserves_unmanaged_files() {
        let home = tempfile::tempdir().unwrap();
        let directory =
            crate::contribution_loading::GuardedContributionMutationDirectory::open_or_create(
                home.path(),
                &[b"skills"],
                home.path(),
                omegon_maintenance_contracts::ContributionKind::Skill,
                "user",
            )
            .unwrap();
        let entry = (std::path::Path::new("SKILL.md"), b"entry".as_slice());
        for path in ["../escape", "/absolute", "nested/../../escape"] {
            assert!(
                directory
                    .merge_manifest_files(
                        b"test",
                        &[entry, (std::path::Path::new(path), b"bad")],
                        b"SKILL.md"
                    )
                    .is_err()
            );
        }
        let deep = format!("{}file", "nested/".repeat(33));
        assert!(
            directory
                .merge_manifest_files(
                    b"test",
                    &[entry, (std::path::Path::new(&deep), b"bad")],
                    b"SKILL.md"
                )
                .is_err()
        );
        assert!(
            directory
                .merge_manifest_files(b"test", &vec![entry; 10_001], b"SKILL.md")
                .is_err()
        );
        assert!(
            directory
                .merge_manifest_files(b"test", &[entry, entry], b"SKILL.md")
                .is_err()
        );
        assert!(
            directory
                .merge_manifest_files(b"../escape", &[entry], b"SKILL.md")
                .is_err()
        );
        assert!(
            directory
                .merge_manifest_files(
                    b"test",
                    &[
                        entry,
                        (std::path::Path::new("refs"), b"file"),
                        (std::path::Path::new("refs/child"), b"nested")
                    ],
                    b"SKILL.md"
                )
                .is_err()
        );
        assert!(!home.path().join("skills/test").exists());
        std::fs::create_dir_all(home.path().join("skills/test")).unwrap();
        std::fs::write(home.path().join("skills/test/operator.txt"), "KEEP").unwrap();
        let files = [
            entry,
            (
                std::path::Path::new("references/nested/guide.md"),
                b"guide".as_slice(),
            ),
        ];
        assert_eq!(
            directory
                .merge_manifest_files(b"test", &files, b"SKILL.md")
                .unwrap(),
            (false, true)
        );
        assert_eq!(
            directory
                .merge_manifest_files(b"test", &files, b"SKILL.md")
                .unwrap(),
            (true, false)
        );
        assert_eq!(
            std::fs::read_to_string(home.path().join("skills/test/operator.txt")).unwrap(),
            "KEEP"
        );
        assert_eq!(
            std::fs::read_to_string(home.path().join("skills/test/references/nested/guide.md"))
                .unwrap(),
            "guide"
        );
    }

    #[test]
    fn skill_doctor_surfaces_retrieval_key_findings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill = tmp.path().join(".claude/skills/vague");
        std::fs::create_dir_all(&skill).expect("skill dir");
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: vague\ndescription: Helps with things\n---\nDo useful work.\n",
        )
        .expect("skill file");

        let report = doctor_report_for(tmp.path()).expect("doctor report");
        assert!(report.contains("retrieval-key:"), "{report}");
        assert!(report.contains("1 retrieval-key finding(s)"), "{report}");
    }

    #[test]
    fn rust_only_workspace_keeps_unmatched_skills_resident_only() {
        use omegon_skills::disclosure::{DisclosureTier, disclose};

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname='x'\n").expect("write Cargo.toml");

        // Every bundled skill, judged against a workspace that only proves Rust.
        let mut resident = Vec::new();
        let mut admitted = Vec::new();
        for shipped in shipped_skills().expect("shipped skills") {
            let name = shipped.name;
            let content = shipped.content;
            let (manifest, _body) = parse_skill_file(&content);
            let entry = disclose(&manifest, root, None);
            if entry.tier == DisclosureTier::Resident {
                resident.push(name.clone());
            } else {
                admitted.push(name.clone());
            }
            // Resident-tier metadata must survive regardless of admission.
            assert!(!entry.name.is_empty(), "{name} lost its name");
            assert!(!entry.description.is_empty(), "{name} lost its description");
        }

        // The Rust skill is signal-backed and its signal is present.
        assert!(
            admitted.iter().any(|name| name == "rust"),
            "rust must be admitted in a Cargo workspace, admitted: {admitted:?}"
        );

        // Skills whose evidence is absent must stay resident, not be guessed in.
        for withheld in ["oci", "typescript", "flynt"] {
            assert!(
                resident.iter().any(|name| name == withheld),
                "{withheld} has no evidence here and must stay resident, resident: {resident:?}"
            );
        }

        assert!(
            !resident.is_empty(),
            "a Rust-only workspace should withhold at least one bundled skill body"
        );
    }

    #[test]
    fn bundled_skills_all_have_content() {
        for shipped in shipped_skills().expect("shipped skills") {
            let name = shipped.name;
            let content = shipped.content;
            assert!(!content.is_empty(), "skill '{name}' is empty");
            assert!(content.len() > 100, "skill '{name}' seems too short");
        }
    }

    #[test]
    fn bundled_skills_all_have_descriptions() {
        for shipped in shipped_skills().expect("shipped skills") {
            let name = shipped.name;
            let content = shipped.content;
            assert!(
                extract_description(&content).is_some(),
                "skill '{name}' missing frontmatter description"
            );
        }
    }

    #[test]
    fn bundled_count_matches_skills_directory() {
        let skills_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../skills");
        let mut directory_names = std::fs::read_dir(&skills_dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", skills_dir.display()))
            .map(|entry| {
                entry.unwrap_or_else(|error| {
                    panic!(
                        "failed to read an entry in {}: {error}",
                        skills_dir.display()
                    )
                })
            })
            .filter(|entry| entry.path().join("SKILL.md").is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        directory_names.sort();

        let mut bundled_names = shipped_skills()
            .unwrap()
            .into_iter()
            .map(|skill| skill.name)
            .collect::<Vec<_>>();
        bundled_names.sort();

        assert_eq!(bundled_names, directory_names);
    }

    #[test]
    fn extract_description_parses_frontmatter() {
        let content = "---\nname: test\ndescription: A test skill\n---\n\n# Test";
        assert_eq!(extract_description(content), Some("A test skill"));
    }

    #[test]
    fn extract_description_returns_none_without_frontmatter() {
        let content = "# No frontmatter here";
        assert_eq!(extract_description(content), None);
    }

    #[test]
    fn extract_description_parses_toml_frontmatter() {
        let content =
            "+++\nid = \"abc\"\nname = \"test\"\ndescription = \"A TOML skill\"\n+++\n\n# Test";
        assert_eq!(extract_description(content), Some("A TOML skill"));
    }

    #[test]
    fn manifest_to_frontmatter_minimal() {
        let manifest = SkillManifest {
            name: "my-skill".into(),
            description: "Does a thing".into(),
            ..Default::default()
        };
        let fm = manifest.to_frontmatter();
        assert!(fm.starts_with("---"));
        assert!(fm.ends_with("---"));
        assert!(fm.contains("name: my-skill"));
        assert!(fm.contains("description: Does a thing"));
    }

    #[test]
    fn manifest_to_frontmatter_full() {
        let manifest = SkillManifest {
            name: "opportunity-eval".into(),
            description: "Evaluate govt contracts".into(),
            id: Some("abc-123".into()),
            version: Some("1.0.0".into()),
            tags: vec!["govcon".into()],
            aliases: vec!["eval".into()],
            triggers: vec!["evaluate this".into()],
            activation: Some("intent_detected".into()),
            profile: vec!["coding".into()],
            project_signals: vec!["solicitation/*.md".into()],
            trusted_paths: vec!["~/Documents/data/".into()],
            output_path: Some("~/output/".into()),
            output_format: Some("markdown".into()),
            max_turns: Some(100),
            posture: Some("architect".into()),
            provenance: None,
        };
        let fm = manifest.to_frontmatter();
        assert!(fm.contains("id: abc-123"));
        assert!(fm.contains("version: 1.0.0"));
        assert!(fm.contains("tags:"));
        assert!(fm.contains("- govcon"));
        assert!(fm.contains("triggers:"));
        assert!(fm.contains("- evaluate this"));
        assert!(fm.contains("activation: intent_detected"));
        assert!(fm.contains("profile:"));
        assert!(fm.contains("- coding"));
        assert!(fm.contains("project_signals:"));
        assert!(fm.contains("- solicitation/*.md"));
        assert!(fm.contains("trusted_paths:"));
        assert!(fm.contains("- ~/Documents/data/"));
        assert!(fm.contains("output_path: ~/output/"));
        assert!(fm.contains("max_turns: 100"));
        assert!(fm.contains("posture: architect"));
    }

    #[test]
    fn list_summary_mentions_bundled_skills() {
        let summary = list_summary().unwrap();
        assert!(summary.contains("Bundled skills"));
        assert!(summary.contains("Run `omegon skills install`"));
    }

    #[test]
    fn doctor_script_references_are_bundle_relative_only() {
        let refs = find_script_references(
            "Use scripts/local.py and ../scripts/escape.py and docs/scripts/not-local.py",
        );
        assert_eq!(refs, vec!["scripts/local.py"]);
    }

    fn write_extension_manifest(
        dir: &std::path::Path,
        extension_name: &str,
        skill_name: &str,
        skill_path: &str,
    ) {
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                r#"[extension]
name = "{extension_name}"
version = "0.1.0"
description = "test extension"

[runtime]
type = "native"
binary = "bin/test"

[[skills]]
name = "{skill_name}"
path = "{skill_path}"
"#
            ),
        )
        .unwrap();
    }

    struct EnvRestore {
        key: &'static str,
        value: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                value: previous,
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    struct CwdRestore {
        original: std::path::PathBuf,
        _guard: tokio::sync::MutexGuard<'static, ()>,
    }

    impl CwdRestore {
        fn enter(path: &std::path::Path) -> Self {
            let guard = crate::test_support::cwd::lock();
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self {
                original,
                _guard: guard,
            }
        }
    }

    impl Drop for CwdRestore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    #[test]
    fn doctor_report_mentions_claude_migration_fast_path() {
        let _guard = crate::test_support::env::lock();
        let dir = tempfile::tempdir().unwrap();
        let _cwd = CwdRestore::enter(dir.path());
        let report = doctor_report().unwrap();
        assert!(report.contains("omegon migrate claude-code"));
    }

    #[test]
    fn doctor_report_mentions_claude_roots() {
        let _guard = crate::test_support::env::lock();
        let dir = tempfile::tempdir().unwrap();
        let _cwd = CwdRestore::enter(dir.path());
        let report = doctor_report().unwrap();

        assert!(report.contains("# Skills doctor"));
        assert!(report.contains("claude:user"));
        assert!(report.contains("claude:project"));
        assert!(report.contains("omegon skills import <skill-dir>"));
        assert!(report.contains("omegon skills import <skill-dir> --project"));
        assert!(report.contains("--force"));
        assert!(!report.contains("sync --all"));
    }

    #[test]
    fn doctor_import_commands_quote_paths_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("Claude Skills/example skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---
name: example-skill
description: Example
---

# Example
",
        )
        .unwrap();

        let bundles = discover_skill_bundles("claude:user", dir.path()).unwrap();
        assert_eq!(bundles.len(), 1);
        let command = format!(
            "omegon skills import {}",
            shell_quote_path(&bundles[0].path)
        );
        assert!(command.contains("'"));
        assert!(command.contains("Claude Skills/example skill"));
    }

    #[test]
    fn imported_skill_summary_lists_scripts_resources_and_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("rust-helper");
        std::fs::create_dir_all(skill_dir.join("scripts/nested")).unwrap();
        std::fs::create_dir_all(skill_dir.join("resources/templates")).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: rust-helper\ndescription: Rust helper\nactivation: project_detected\nprofile: [coding]\nproject_signals: [Cargo.toml]\n---\n\nBody\n",
        )
        .unwrap();
        std::fs::write(skill_dir.join("scripts/run.sh"), "echo run\n").unwrap();
        std::fs::write(skill_dir.join("scripts/nested/check.py"), "print('ok')\n").unwrap();
        std::fs::write(
            skill_dir.join("resources/templates/readme.md"),
            "template\n",
        )
        .unwrap();

        let summary = summarize_imported_skill(&skill_dir, "rust-helper");

        assert_eq!(
            summary.scripts,
            vec![
                "scripts/nested/check.py".to_string(),
                "scripts/run.sh".to_string()
            ]
        );
        assert_eq!(
            summary.resources,
            vec!["resources/templates/readme.md".to_string()]
        );
        assert!(
            summary
                .conflicts
                .iter()
                .any(|conflict| conflict == "bundled/rust")
        );
    }

    #[test]
    fn import_skill_bundle_preserves_scripts_and_refuses_overwrite() {
        let _guard = crate::test_support::env::lock();
        let home = tempfile::tempdir().unwrap();
        let _home = EnvRestore::set("OMEGON_HOME", home.path());
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("scripts")).unwrap();
        std::fs::write(
            source.path().join("SKILL.md"),
            "---\nname: claude-rust\ndescription: Claude Rust\n---\n\nUse scripts/check.py\n",
        )
        .unwrap();
        std::fs::write(source.path().join("scripts/check.py"), "print('ok')\n").unwrap();

        cmd_import(source.path(), false, false).unwrap();
        let imported = home.path().join("skills/claude-rust");
        assert!(imported.join("SKILL.md").is_file());
        assert!(imported.join("scripts/check.py").is_file());
        let err = cmd_import(source.path(), false, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--force"), "{err}");
    }

    #[test]
    fn import_direct_skill_file_does_not_copy_unrelated_sibling_files() {
        let _guard = crate::test_support::env::lock();
        let home = tempfile::tempdir().unwrap();
        let _home = EnvRestore::set("OMEGON_HOME", home.path());
        let source = tempfile::tempdir().unwrap();
        let skill_file = source.path().join("SKILL.md");
        std::fs::write(
            &skill_file,
            "---\nname: solo\ndescription: Solo\n---\n\nBody\n",
        )
        .unwrap();
        std::fs::write(source.path().join("unrelated.txt"), "do not import").unwrap();

        cmd_import(&skill_file, false, false).unwrap();

        let imported = home.path().join("skills/solo");
        assert!(imported.join("SKILL.md").is_file());
        assert!(!imported.join("unrelated.txt").exists());
    }

    #[test]
    fn import_skill_file_into_project_uses_manifest_name() {
        let _guard = crate::test_support::env::lock();
        let cwd = tempfile::tempdir().unwrap();
        let _cwd = CwdRestore::enter(cwd.path());
        let source = tempfile::tempdir().unwrap();
        let skill_file = source.path().join("SKILL.md");
        std::fs::write(
            &skill_file,
            "---\nname: Claude Helper\ndescription: Helper\n---\n\nBody\n",
        )
        .unwrap();

        cmd_import(&skill_file, true, false).unwrap();
        assert!(
            cwd.path()
                .join(".omegon/skills/claude-helper/SKILL.md")
                .is_file()
        );
    }

    #[test]
    fn extension_skill_path_cannot_escape_extension_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            skill_path_stays_within_extension_root(dir.path(), "skills/rust/SKILL.md").is_some()
        );
        assert!(
            skill_path_stays_within_extension_root(dir.path(), "../outside/SKILL.md").is_none()
        );
        assert!(skill_path_stays_within_extension_root(dir.path(), "/tmp/SKILL.md").is_none());
    }

    #[test]
    fn extension_skill_conflicts_are_case_insensitive() {
        let first = SkillEntry {
            name: "rust".into(),
            description: String::new(),
            id: None,
            version: None,
            tags: Vec::new(),
            aliases: vec!["RS".into()],
            triggers: vec!["Rust".into()],
            activation: Some("intent_detected".into()),
            profile: vec!["coding".into()],
            project_signals: Vec::new(),
            posture: None,
            max_turns: None,
            installed: true,
            bundled: true,
            project_local: false,
            source: "bundled".into(),
            editable: false,
            reloadable: false,
            shadows: Vec::new(),
            conflicts: Vec::new(),
            path: String::new(),
        };
        let second = SkillEntry {
            name: "recro-rust-dev".into(),
            description: String::new(),
            id: None,
            version: None,
            tags: Vec::new(),
            aliases: vec!["rs".into()],
            triggers: vec!["rust".into()],
            activation: Some("intent_detected".into()),
            profile: vec!["coding".into()],
            project_signals: Vec::new(),
            posture: None,
            max_turns: None,
            installed: true,
            bundled: false,
            project_local: false,
            source: "extension:recro".into(),
            editable: false,
            reloadable: true,
            shadows: Vec::new(),
            conflicts: Vec::new(),
            path: String::new(),
        };
        assert!(skill_sources_conflict(&first, &second));
    }

    #[test]
    fn list_structured_includes_extension_skill_and_conflict_metadata() {
        let _guard = crate::test_support::env::lock();
        let home = tempfile::tempdir().unwrap();
        let _home = EnvRestore::set("OMEGON_HOME", home.path());
        let extension_dir = home.path().join("extensions/recro");
        let skill_dir = extension_dir.join("skills/recro-rust-dev");
        std::fs::create_dir_all(&skill_dir).unwrap();
        write_extension_manifest(
            &extension_dir,
            "recro",
            "recro-rust-dev",
            "skills/recro-rust-dev/SKILL.md",
        );
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: recro-rust-dev\ndescription: Recro Rust\nactivation: project_detected\nprofile: [coding]\nproject_signals: [Cargo.toml]\n---\n\n# Recro Rust\n",
        )
        .unwrap();

        let entries = list_structured().unwrap();
        let recro = entries
            .iter()
            .find(|entry| entry.name == "recro-rust-dev")
            .expect("extension skill should be listed");
        assert_eq!(recro.source, "extension:recro");
        assert!(!recro.editable);
        assert!(recro.reloadable);
        assert!(
            recro
                .conflicts
                .iter()
                .any(|conflict| conflict == "bundled/rust")
        );
    }

    #[test]
    fn list_structured_includes_bundled() {
        let entries = list_structured().unwrap();
        assert!(entries.iter().any(|e| e.name == "git" && e.bundled));
        assert!(entries.iter().any(|e| e.name == "security" && e.bundled));

        let rust = entries
            .iter()
            .find(|e| e.name == "rust" && e.bundled)
            .expect("bundled rust skill should be listed");
        assert_eq!(rust.activation.as_deref(), Some("project_detected"));
        assert_eq!(rust.source, "bundled");
        assert!(!rust.editable);
        assert!(!rust.reloadable);
        assert!(rust.shadows.is_empty());
        assert!(rust.profile.iter().any(|p| p == "coding"));
        assert!(rust.project_signals.iter().any(|s| s == "Cargo.toml"));
    }

    #[test]
    fn list_structured_includes_project_override_shadow_metadata() {
        let _guard = crate::test_support::env::lock();
        let dir = tempfile::tempdir().unwrap();
        let project_skill = dir.path().join(".omegon/skills/git");
        std::fs::create_dir_all(&project_skill).unwrap();
        std::fs::write(
            project_skill.join("SKILL.md"),
            "---
name: git
description: Project git override
---

# Git override
",
        )
        .unwrap();

        let _cwd = CwdRestore::enter(dir.path());
        let entries = list_structured().unwrap();
        drop(_cwd);

        let project_git = entries
            .iter()
            .find(|entry| entry.name == "git" && entry.project_local)
            .expect("project git override should be listed");
        assert_eq!(project_git.source, "project");
        assert!(project_git.editable);
        assert!(project_git.reloadable);
        assert!(project_git.shadows.iter().any(|source| source == "bundled"));
    }

    #[test]
    fn get_skill_details_uses_resolved_project_override_metadata() {
        let _guard = crate::test_support::env::lock();
        let dir = tempfile::tempdir().unwrap();
        let project_skill = dir.path().join(".omegon/skills/git");
        std::fs::create_dir_all(&project_skill).unwrap();
        std::fs::write(
            project_skill.join("SKILL.md"),
            "---
name: git
description: Project git override
---

# Git override
",
        )
        .unwrap();

        let _cwd = CwdRestore::enter(dir.path());
        let details = get_skill_details("git").unwrap();
        drop(_cwd);

        assert_eq!(details.manifest.description, "Project git override");
        let entry = details.entry.expect("resolved listing metadata");
        assert_eq!(entry.source, "project");
        assert!(entry.project_local);
        assert!(entry.shadows.iter().any(|source| source == "bundled"));
    }

    #[test]
    fn project_signal_matches_literal_file_and_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::create_dir_all(root.join("openspec/changes")).unwrap();

        let cargo = match_project_signal(root, "Cargo.toml").unwrap().unwrap();
        assert_eq!(cargo.kind, SkillSignalKind::Literal);
        assert_eq!(cargo.matched_path, "Cargo.toml");

        let openspec = match_project_signal(root, "openspec/changes")
            .unwrap()
            .unwrap();
        assert_eq!(openspec.kind, SkillSignalKind::Literal);
        assert_eq!(openspec.matched_path, "openspec/changes");
    }

    #[test]
    fn project_signal_matches_root_glob_only_at_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "").unwrap();
        assert!(match_project_signal(root, "*.rs").unwrap().is_none());

        std::fs::write(root.join("main.rs"), "").unwrap();
        let matched = match_project_signal(root, "*.rs").unwrap().unwrap();
        assert_eq!(matched.kind, SkillSignalKind::RootGlob);
        assert_eq!(matched.matched_path, "main.rs");
    }

    #[test]
    fn project_signal_matches_recursive_glob_and_ignores_vendor_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs/nested")).unwrap();
        std::fs::create_dir_all(root.join("docs/target")).unwrap();
        std::fs::write(root.join("docs/target/ignored.md"), "").unwrap();
        assert!(
            match_project_signal(root, "docs/**/*.md")
                .unwrap()
                .is_none()
        );

        std::fs::write(root.join("docs/nested/guide.md"), "").unwrap();
        let matched = match_project_signal(root, "docs/**/*.md").unwrap().unwrap();
        assert_eq!(matched.kind, SkillSignalKind::RecursiveGlob);
        assert_eq!(matched.matched_path, "docs/nested/guide.md");
    }

    #[test]
    fn project_signal_rejects_invalid_patterns() {
        for signal in [
            "",
            "/Cargo.toml",
            "../Cargo.toml",
            "docs//*.md",
            "docs\\*.md",
            "docs/**/**/*.md",
            "src/*.rs",
        ] {
            assert!(
                validate_project_signal(signal).is_err(),
                "signal should be rejected: {signal}"
            );
        }
    }

    #[test]
    fn bundled_skills_declare_activation_metadata() {
        for shipped in shipped_skills().expect("shipped skills") {
            let name = shipped.name;
            let content = shipped.content;
            let (manifest, _) = parse_skill_file(&content);
            assert!(
                manifest.activation.is_some(),
                "bundled skill {name} must declare activation"
            );
            assert!(
                !manifest.profile.is_empty(),
                "bundled skill {name} must declare at least one profile"
            );

            let diagnostics = validate_activation_metadata(&manifest);
            assert!(
                diagnostics.warnings.is_empty(),
                "bundled skill {name} has activation metadata warnings: {:?}",
                diagnostics.warnings
            );
        }
    }

    #[test]
    fn skill_builder_prompt_supports_upstream_assisted_authoring() {
        let prompt = skill_builder_prompt(std::path::Path::new("/tmp/project"));

        assert!(prompt.contains("create or adapt an Omegon skill"));
        assert!(prompt.contains("upstream-assisted skill workflow"));
        assert!(prompt.contains("anthropics/webapp-testing"));
        assert!(prompt.contains("Do not blindly install arbitrary prompt packs"));
        assert!(
            prompt
                .contains("Do not claim static inspection can prove upstream executable code safe")
        );
        assert!(prompt.contains("trust-and-import, omit executable assets, or clean-room rewrite"));
        assert!(prompt.contains(
            "Default posture for Node/npm/pnpm/yarn assets is clean-room rewrite or omission"
        ));
        assert!(prompt.contains("## Provenance"));
        assert!(prompt.contains("## Omitted Upstream Assets"));
        assert!(
            prompt.contains("Do not build or rely on a fake security proof from script analysis")
        );
        assert!(prompt.contains("/skills refresh"));
        assert!(prompt.contains("/tmp/project/.omegon/skills/<name>/SKILL.md"));
    }

    #[test]
    fn bundled_codebase_init_skill_is_first_order_and_evidence_led() {
        let (manifest, body, _path) = get_skill("codebase-init").unwrap();
        assert_eq!(manifest.activation.as_deref(), Some("project_detected"));
        assert!(
            manifest
                .project_signals
                .iter()
                .any(|signal| signal == ".git")
        );
        assert!(body.contains("Initialization has two phases"));
        assert!(body.contains("Do **not** create one per directory mechanically"));
        assert!(body.contains("## `/init` integration"));
    }

    #[test]
    fn get_skill_bundled() {
        let (manifest, body, _path) = get_skill("rust").unwrap();
        assert!(!manifest.description.is_empty());
        assert!(!body.is_empty());
    }

    #[test]
    fn get_skill_not_found() {
        let result = get_skill("nonexistent-skill-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn get_skill_traversal_rejected() {
        let result = get_skill("../../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }
}

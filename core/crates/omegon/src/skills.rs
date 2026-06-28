//! Skill management — schema, parsing, listing, and installation.
//!
//! Skills are markdown directive files injected into the system prompt at session start.
//! Bundled skills ship embedded in the binary so `omegon skills install` works regardless
//! of whether a source tree is present.
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

pub use omegon_skills::skill_builder_prompt;

/// All skills bundled into the binary at compile time.
/// Each entry is (name, skill_markdown_content).
pub const BUNDLED: &[(&str, &str)] = &[
    (
        "code-act",
        include_str!("../../../../skills/code-act/SKILL.md"),
    ),
    ("git", include_str!("../../../../skills/git/SKILL.md")),
    ("oci", include_str!("../../../../skills/oci/SKILL.md")),
    (
        "openspec",
        include_str!("../../../../skills/openspec/SKILL.md"),
    ),
    ("python", include_str!("../../../../skills/python/SKILL.md")),
    ("rust", include_str!("../../../../skills/rust/SKILL.md")),
    (
        "security",
        include_str!("../../../../skills/security/SKILL.md"),
    ),
    ("style", include_str!("../../../../skills/style/SKILL.md")),
    (
        "typescript",
        include_str!("../../../../skills/typescript/SKILL.md"),
    ),
    ("flynt", include_str!("../../../../skills/flynt/SKILL.md")),
];

fn skills_dir() -> Option<std::path::PathBuf> {
    crate::paths::omegon_home().ok().map(|h| h.join("skills"))
}

/// Render bundled skills and their installation status as terminal-friendly text.
pub fn list_summary() -> anyhow::Result<String> {
    let skills_dir = skills_dir();

    let mut lines = vec![format!("Bundled skills ({})\n", BUNDLED.len())];

    for (name, content) in BUNDLED {
        // Extract description from frontmatter if present
        let description = extract_description(content).unwrap_or("(no description)");

        let installed = skills_dir
            .as_ref()
            .is_some_and(|d| d.join(name).join("SKILL.md").exists());
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
    let entries = list_structured()?;
    let mut lines = vec!["# Skills doctor".to_string(), String::new()];
    lines.push("Detected compatible skill roots:".into());
    let mut total = 0usize;
    let mut conflict_count = 0usize;
    let mut missing_scripts = 0usize;
    for (source, root) in claude_skill_roots(&cwd) {
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
    lines.push(format!("Summary: {total} compatible external skill bundle(s), {conflict_count} conflict marker(s), {missing_scripts} missing script reference(s)."));
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

fn validate_skill_name(name: &str) -> anyhow::Result<String> {
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

pub fn cmd_import(path: &std::path::Path, project: bool, force: bool) -> anyhow::Result<()> {
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
    let base = if project {
        std::env::current_dir()?.join(".omegon/skills")
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
    let summary = summarize_imported_skill(&destination, &slug);
    println!(
        "Imported {} skill '{}' from {} to {}",
        if project { "project-local" } else { "user" },
        slug,
        source.display(),
        destination.display()
    );
    print_import_summary(&summary);
    Ok(())
}

const LEGACY_VAULT_SKILL_ID: &str = "8d7961f6-4742-416f-89eb-bef9f6cc12f6";

fn is_legacy_bundled_vault_skill(content: &str) -> bool {
    let (manifest, _body) = parse_skill_file(content);
    manifest.name == "vault" && manifest.id.as_deref() == Some(LEGACY_VAULT_SKILL_ID)
}

fn remove_legacy_bundled_vault_skill(skills_dir: &std::path::Path) -> anyhow::Result<bool> {
    let legacy_dir = skills_dir.join("vault");
    let legacy_file = legacy_dir.join("SKILL.md");
    let Ok(content) = std::fs::read_to_string(&legacy_file) else {
        return Ok(false);
    };
    if !is_legacy_bundled_vault_skill(&content) {
        return Ok(false);
    }
    std::fs::remove_dir_all(&legacy_dir)?;
    Ok(true)
}

/// Install all bundled skills to ~/.omegon/skills/.
/// Existing files are overwritten. Project-local skills are never touched.
pub fn cmd_install() -> anyhow::Result<()> {
    let skills_dir =
        skills_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;

    std::fs::create_dir_all(&skills_dir)?;

    let mut installed = 0;
    let mut updated = 0;

    if remove_legacy_bundled_vault_skill(&skills_dir)? {
        println!("  - vault  (removed; renamed to flynt)");
    }

    for (name, content) in BUNDLED {
        let skill_dir = skills_dir.join(name);
        let skill_file = skill_dir.join("SKILL.md");

        std::fs::create_dir_all(&skill_dir)?;

        let already_exists = skill_file.exists();
        let existing_content = if already_exists {
            std::fs::read_to_string(&skill_file).ok()
        } else {
            None
        };

        let changed = existing_content.as_deref() != Some(content);

        std::fs::write(&skill_file, content)?;

        if !already_exists {
            println!("  + {name}");
            installed += 1;
        } else if changed {
            println!("  ↑ {name}  (updated)");
            updated += 1;
        } else {
            println!("  ✓ {name}  (unchanged)");
        }
    }

    println!(
        "\n{} skill(s) installed, {} updated → {}",
        installed,
        updated,
        skills_dir.display()
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
    for (name, content) in BUNDLED {
        let (manifest, _body) = parse_skill_file(content);
        let installed = home_skills
            .as_ref()
            .is_some_and(|d| d.join(name).join("SKILL.md").exists());
        let path = home_skills
            .as_ref()
            .map(|d| d.join(name).display().to_string())
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
    for (bname, content) in BUNDLED {
        if *bname == name {
            let (manifest, body) = parse_skill_file(content);
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
    } else if let Some(b) = content.strip_prefix("+++\n") {
        (b, "\n+++")
    } else {
        return None;
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

    #[test]
    fn bundled_skills_all_have_content() {
        for (name, content) in BUNDLED {
            assert!(!content.is_empty(), "skill '{name}' is empty");
            assert!(content.len() > 100, "skill '{name}' seems too short");
        }
    }

    #[test]
    fn bundled_skills_all_have_descriptions() {
        for (name, content) in BUNDLED {
            assert!(
                extract_description(content).is_some(),
                "skill '{name}' missing frontmatter description"
            );
        }
    }

    #[test]
    fn bundled_count_matches_skills_directory() {
        // 10 skills: code-act, git, oci, openspec, python, rust, security, style, typescript, flynt
        assert_eq!(BUNDLED.len(), 10);
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
    }

    impl CwdRestore {
        fn enter(path: &std::path::Path) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { original }
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

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let entries = list_structured().unwrap();
        std::env::set_current_dir(original).unwrap();

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

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let details = get_skill_details("git").unwrap();
        std::env::set_current_dir(original).unwrap();

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
        for (name, content) in BUNDLED {
            let (manifest, _) = parse_skill_file(content);
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
